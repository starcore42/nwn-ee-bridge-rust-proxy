//! Reliable-window sequence/ack arithmetic.
//!
//! NWN's `M` reliable window uses wrapping 16-bit sequence numbers. The bridge
//! sometimes inserts synthetic packets, so peer-facing sequence numbers and
//! origin-facing ACKs must be shifted without confusing retransmit windows.
//! Keep this pure and packet-format-free: callers own packet mutation, this
//! module only answers sequence-ordering and delta questions.

use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub(super) struct SequenceShift {
    pub(super) base: u16,
    pub(super) delta: u16,
}

#[derive(Debug, Clone)]
pub(super) struct CoalescedSplitSequenceShift {
    pub(super) source_sequence: u16,
    pub(super) source_origin_generation: u64,
    pub(super) base: u16,
    pub(super) delta: u16,
}

#[derive(Debug, Clone)]
pub(super) struct SequenceElision {
    pub(super) sequence: u16,
}

/// Exact position in one ordered 16-bit reliable-sequence domain.
///
/// A signed generation is intentional. When a session begins at sequence zero,
/// an insertion before that first source maps its partial ACK to the logical
/// predecessor `(generation - 1, 0xFFFF)`. Keeping that predecessor exact
/// avoids reintroducing half-range guesses at the epoch boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SequenceEpochKey {
    pub(super) generation: i64,
    pub(super) sequence: u16,
}

impl SequenceEpochKey {
    const SEQUENCES_PER_GENERATION: i128 = u16::MAX as i128 + 1;

    pub(super) const fn new(sequence: u16, generation: i64) -> Self {
        Self {
            generation,
            sequence,
        }
    }

    fn ordinal(self) -> i128 {
        i128::from(self.generation) * Self::SEQUENCES_PER_GENERATION + i128::from(self.sequence)
    }

    fn from_ordinal(ordinal: i128) -> anyhow::Result<Self> {
        let generation = ordinal.div_euclid(Self::SEQUENCES_PER_GENERATION);
        let sequence = ordinal.rem_euclid(Self::SEQUENCES_PER_GENERATION);
        let generation = i64::try_from(generation)
            .map_err(|_| anyhow::anyhow!("reliable sequence generation overflow"))?;
        let sequence = u16::try_from(sequence)
            .map_err(|_| anyhow::anyhow!("reliable sequence remainder overflow"))?;
        Ok(Self {
            generation,
            sequence,
        })
    }

    pub(super) fn checked_advance(self, distance: u64) -> anyhow::Result<Self> {
        let ordinal = self
            .ordinal()
            .checked_add(i128::from(distance))
            .ok_or_else(|| anyhow::anyhow!("reliable sequence ordinal overflow"))?;
        Self::from_ordinal(ordinal)
    }

    pub(super) fn checked_retreat(self, distance: u64) -> anyhow::Result<Self> {
        let ordinal = self
            .ordinal()
            .checked_sub(i128::from(distance))
            .ok_or_else(|| anyhow::anyhow!("reliable sequence ordinal underflow"))?;
        Self::from_ordinal(ordinal)
    }
}

/// Stable identity for one proxy-owned server-lane insertion.
///
/// `operation` is caller-owned and distinguishes independent insertions
/// derived from the same exact reliable source. Replaying the same owner with
/// the same shape is idempotent; reusing it with a different shape is a
/// transport conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ServerSequenceInsertionOwner {
    pub(super) source: SequenceEpochKey,
    pub(super) operation: u64,
}

impl ServerSequenceInsertionOwner {
    pub(super) const fn new(source: SequenceEpochKey, operation: u64) -> Self {
        Self { source, operation }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ServerSequenceInsertionRange {
    /// First destination sequence owned by the proxy insertion.
    pub(super) destination_first: SequenceEpochKey,
    /// First destination sequence after the proxy insertion. This is also the
    /// destination sequence of `source_base`.
    pub(super) destination_after: SequenceEpochKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ServerSequenceEpochCheckpoint {
    source: SequenceEpochKey,
    destination: SequenceEpochKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ServerSequenceEpochInsertion {
    owner: ServerSequenceInsertionOwner,
    source_base: SequenceEpochKey,
    count: u16,
    range: ServerSequenceInsertionRange,
}

/// Ordered, generation-aware coordinate transform for proxy-owned insertions
/// in the server-to-EE reliable lane.
///
/// The checkpoint carries every compacted insertion permanently. Active
/// discontinuities remain explicit until both mirrored reliable windows have
/// advanced beyond them. No bounded-history operation discards a cumulative
/// delta: capacity pressure fails closed and leaves the ledger unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct OrderedServerSequenceEpochs {
    checkpoint: Option<ServerSequenceEpochCheckpoint>,
    insertions: VecDeque<ServerSequenceEpochInsertion>,
}

impl OrderedServerSequenceEpochs {
    /// Defensive ceiling for discontinuities that are still live in either
    /// mirrored 16-slot window. The real output window normally imposes a
    /// tighter bound; this guard catches broken ownership without trimming.
    pub(super) const MAX_ACTIVE_INSERTIONS: usize = 64;

    /// Unseeded identity mapping. It remains exact for every generation until
    /// the first insertion establishes a checkpoint.
    pub(super) fn identity() -> Self {
        Self::default()
    }

    /// Identity mapping anchored at an exact source/destination position.
    pub(super) fn identity_at(key: SequenceEpochKey) -> Self {
        Self {
            checkpoint: Some(ServerSequenceEpochCheckpoint {
                source: key,
                destination: key,
            }),
            insertions: VecDeque::new(),
        }
    }

    /// Seed a carried server-lane mapping, for example after restoring an
    /// already compacted epoch checkpoint.
    pub(super) fn seed(
        source: SequenceEpochKey,
        destination: SequenceEpochKey,
    ) -> anyhow::Result<Self> {
        if destination.ordinal() < source.ordinal() {
            anyhow::bail!("server sequence epoch seed would remove destination reliable sequences");
        }
        Ok(Self {
            checkpoint: Some(ServerSequenceEpochCheckpoint {
                source,
                destination,
            }),
            insertions: VecDeque::new(),
        })
    }

    pub(super) fn checkpoint(&self) -> Option<(SequenceEpochKey, SequenceEpochKey)> {
        self.checkpoint
            .map(|checkpoint| (checkpoint.source, checkpoint.destination))
    }

    pub(super) fn active_insertions(&self) -> usize {
        self.insertions.len()
    }

    /// Insert `count` proxy-owned destination sequences immediately before
    /// `source_base`.
    ///
    /// Entries append in exact source order. Equal bases are legal and retain
    /// caller order, which is required when one source transaction creates
    /// several adjacent proxy-owned ranges.
    pub(super) fn insert_before(
        &mut self,
        owner: ServerSequenceInsertionOwner,
        source_base: SequenceEpochKey,
        count: u16,
    ) -> anyhow::Result<ServerSequenceInsertionRange> {
        if let Some(existing) = self
            .insertions
            .iter()
            .find(|insertion| insertion.owner == owner)
        {
            if existing.source_base == source_base && existing.count == count {
                return Ok(existing.range);
            }
            anyhow::bail!("server sequence insertion owner replayed with a conflicting shape");
        }
        if count == 0 {
            anyhow::bail!("server sequence insertion must own at least one destination sequence");
        }
        if self.insertions.len() >= Self::MAX_ACTIVE_INSERTIONS {
            anyhow::bail!(
                "server sequence epoch exceeded {} active insertions",
                Self::MAX_ACTIVE_INSERTIONS
            );
        }
        if let Some(checkpoint) = self.checkpoint {
            if source_base.ordinal() < checkpoint.source.ordinal() {
                anyhow::bail!("server sequence insertion precedes the compacted source epoch");
            }
        }
        if let Some(previous) = self.insertions.back() {
            if source_base.ordinal() < previous.source_base.ordinal() {
                anyhow::bail!("server sequence insertion arrived out of source order");
            }
        }

        // Compute and validate the complete new entry before mutating either
        // the checkpoint or active deque.
        let destination_first = self.map_source(source_base)?;
        let destination_after = destination_first.checked_advance(u64::from(count))?;
        let range = ServerSequenceInsertionRange {
            destination_first,
            destination_after,
        };
        let insertion = ServerSequenceEpochInsertion {
            owner,
            source_base,
            count,
            range,
        };

        if self.checkpoint.is_none() {
            self.checkpoint = Some(ServerSequenceEpochCheckpoint {
                source: source_base,
                destination: source_base,
            });
        }
        self.insertions.push_back(insertion);
        Ok(range)
    }

    /// Map one exact Diamond/server source key into the EE destination domain.
    pub(super) fn map_source(&self, source: SequenceEpochKey) -> anyhow::Result<SequenceEpochKey> {
        let Some(checkpoint) = self.checkpoint else {
            return Ok(source);
        };
        let source_distance = source
            .ordinal()
            .checked_sub(checkpoint.source.ordinal())
            .filter(|distance| *distance >= 0)
            .ok_or_else(|| {
                anyhow::anyhow!("server source sequence precedes the compacted epoch checkpoint")
            })?;
        let mut destination_ordinal = checkpoint
            .destination
            .ordinal()
            .checked_add(source_distance)
            .ok_or_else(|| anyhow::anyhow!("server destination sequence ordinal overflow"))?;
        for insertion in &self.insertions {
            if insertion.source_base.ordinal() > source.ordinal() {
                break;
            }
            destination_ordinal = destination_ordinal
                .checked_add(i128::from(insertion.count))
                .ok_or_else(|| anyhow::anyhow!("server insertion delta overflow"))?;
        }
        SequenceEpochKey::from_ordinal(destination_ordinal)
    }

    /// Map one exact EE cumulative ACK into the Diamond/server source domain.
    ///
    /// An ACK inside a proxy-owned insertion range remains at the predecessor
    /// of that insertion's source base. At `destination_after`, the source base
    /// itself becomes acknowledged.
    pub(super) fn map_destination_ack(
        &self,
        destination_ack: SequenceEpochKey,
    ) -> anyhow::Result<SequenceEpochKey> {
        let Some(checkpoint) = self.checkpoint else {
            return Ok(destination_ack);
        };
        let destination_distance = destination_ack
            .ordinal()
            .checked_sub(checkpoint.destination.ordinal())
            .filter(|distance| *distance >= 0)
            .ok_or_else(|| {
                anyhow::anyhow!("server destination ACK precedes the compacted epoch checkpoint")
            })?;
        let mut inserted_before = 0i128;
        for insertion in &self.insertions {
            let destination = destination_ack.ordinal();
            if destination < insertion.range.destination_first.ordinal() {
                break;
            }
            if destination < insertion.range.destination_after.ordinal() {
                return insertion.source_base.checked_retreat(1);
            }
            inserted_before = inserted_before
                .checked_add(i128::from(insertion.count))
                .ok_or_else(|| anyhow::anyhow!("server ACK insertion delta overflow"))?;
        }
        let source_distance = destination_distance
            .checked_sub(inserted_before)
            .filter(|distance| *distance >= 0)
            .ok_or_else(|| anyhow::anyhow!("server ACK maps before the source epoch checkpoint"))?;
        let source_ordinal = checkpoint
            .source
            .ordinal()
            .checked_add(source_distance)
            .ok_or_else(|| anyhow::anyhow!("server ACK source ordinal overflow"))?;
        SequenceEpochKey::from_ordinal(source_ordinal)
    }

    /// Fold a safe prefix into the exact checkpoint.
    ///
    /// `source_floor` is the first source slot that has not been retired by a
    /// strict-accepted mapped ACK. `destination_floor` is the first EE output
    /// slot not retired by a validated raw ACK. A source owner must be strictly
    /// before its floor so an exact retransmit cannot recreate a compacted
    /// insertion.
    pub(super) fn compact(
        &mut self,
        source_floor: SequenceEpochKey,
        destination_floor: SequenceEpochKey,
    ) -> anyhow::Result<usize> {
        let Some(checkpoint) = self.checkpoint else {
            return Ok(0);
        };
        if source_floor.ordinal() < checkpoint.source.ordinal() {
            anyhow::bail!("server source compaction floor moved behind its epoch checkpoint");
        }
        if destination_floor.ordinal() < checkpoint.destination.ordinal() {
            anyhow::bail!("server destination compaction floor moved behind its epoch checkpoint");
        }

        let mut compacted = 0usize;
        let mut next_checkpoint = checkpoint;
        for insertion in &self.insertions {
            let source_owner_retired = source_floor.ordinal() > insertion.source_base.ordinal();
            let destination_range_retired =
                destination_floor.ordinal() >= insertion.range.destination_after.ordinal();
            if !source_owner_retired || !destination_range_retired {
                break;
            }
            next_checkpoint = ServerSequenceEpochCheckpoint {
                source: insertion.source_base,
                destination: insertion.range.destination_after,
            };
            compacted += 1;
        }
        if compacted == 0 {
            return Ok(0);
        }

        self.insertions.drain(..compacted);
        self.checkpoint = Some(next_checkpoint);
        Ok(compacted)
    }
}

pub(super) fn sequence_at_or_after(sequence: u16, base: u16) -> bool {
    sequence.wrapping_sub(base) < 0x8000
}

pub(super) fn record_forward_progress(latest: &mut Option<u16>, observed: u16) {
    let should_update = latest
        .map(|current| sequence_at_or_after(observed, current))
        .unwrap_or(true);
    if should_update {
        *latest = Some(observed);
    }
}

fn sequence_before(sequence: u16, base: u16) -> bool {
    sequence != base && base.wrapping_sub(sequence) < 0x8000
}

fn sequence_in_forward_closed_range(sequence: u16, first: u16, last: u16) -> bool {
    sequence_at_or_after(sequence, first) && sequence_at_or_after(last, sequence)
}

pub(super) fn shift_sequence_for_peer(shifts: &[SequenceShift], original_sequence: u16) -> u16 {
    let mut delta = 0u16;
    for shift in shifts {
        if shift.delta != 0 && sequence_at_or_after(original_sequence, shift.base) {
            delta = delta.wrapping_add(shift.delta);
        }
    }
    original_sequence.wrapping_add(delta)
}

pub(super) fn shift_sequence_for_peer_with_elisions(
    shifts: &[SequenceShift],
    elisions: &[SequenceElision],
    original_sequence: u16,
) -> Option<u16> {
    if elisions
        .iter()
        .any(|elision| elision.sequence == original_sequence)
    {
        return None;
    }

    let shifted = shift_sequence_for_peer(shifts, original_sequence);
    let elided_before = elisions
        .iter()
        .filter(|elision| sequence_at_or_after(original_sequence, elision.sequence))
        .count() as u16;
    Some(shifted.wrapping_sub(elided_before))
}

pub(super) fn unshift_ack_for_origin(shifts: &[SequenceShift], shifted_ack_sequence: u16) -> u16 {
    let mut cumulative_delta = 0u16;
    for shift in shifts {
        if shift.delta == 0 {
            continue;
        }

        let synthetic_first = shift.base.wrapping_add(cumulative_delta);
        if sequence_before(shifted_ack_sequence, synthetic_first) {
            return shifted_ack_sequence.wrapping_sub(cumulative_delta);
        }

        let synthetic_last = synthetic_first.wrapping_add(shift.delta).wrapping_sub(1);
        if sequence_in_forward_closed_range(shifted_ack_sequence, synthetic_first, synthetic_last) {
            return shift.base.wrapping_sub(1);
        }

        cumulative_delta = cumulative_delta.wrapping_add(shift.delta);
    }
    shifted_ack_sequence.wrapping_sub(cumulative_delta)
}

pub(super) fn unshift_ack_for_origin_with_elisions(
    shifts: &[SequenceShift],
    elisions: &[SequenceElision],
    shifted_ack_sequence: u16,
) -> u16 {
    let mut unshifted = unshift_ack_for_origin(shifts, shifted_ack_sequence);
    for elision in elisions {
        if sequence_at_or_after(unshifted.wrapping_add(1), elision.sequence) {
            unshifted = unshifted.wrapping_add(1);
        }
    }
    unshifted
}

pub(super) fn trim_sequence_shifts(shifts: &mut Vec<SequenceShift>) {
    const MAX_SEQUENCE_SHIFTS: usize = 16;
    if shifts.len() > MAX_SEQUENCE_SHIFTS {
        let overflow = shifts.len() - MAX_SEQUENCE_SHIFTS;
        shifts.drain(0..overflow);
    }
}

pub(super) fn trim_coalesced_split_sequence_shifts(shifts: &mut Vec<CoalescedSplitSequenceShift>) {
    const MAX_COALESCED_SPLIT_SEQUENCE_SHIFTS: usize = 16;
    if shifts.len() > MAX_COALESCED_SPLIT_SEQUENCE_SHIFTS {
        let overflow = shifts.len() - MAX_COALESCED_SPLIT_SEQUENCE_SHIFTS;
        shifts.drain(0..overflow);
    }
}

pub(super) fn trim_sequence_elisions(elisions: &mut Vec<SequenceElision>) {
    const MAX_SEQUENCE_ELISIONS: usize = 64;
    if elisions.len() > MAX_SEQUENCE_ELISIONS {
        let overflow = elisions.len() - MAX_SEQUENCE_ELISIONS;
        elisions.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch(sequence: u16, generation: i64) -> SequenceEpochKey {
        SequenceEpochKey::new(sequence, generation)
    }

    fn insertion_owner(
        sequence: u16,
        generation: i64,
        operation: u64,
    ) -> ServerSequenceInsertionOwner {
        ServerSequenceInsertionOwner::new(epoch(sequence, generation), operation)
    }

    #[test]
    fn ordered_server_epochs_keep_the_seventeenth_insertion_exact() {
        let mut epochs = OrderedServerSequenceEpochs::identity();
        for base in 1..=17u16 {
            epochs
                .insert_before(insertion_owner(base, 0, u64::from(base)), epoch(base, 0), 1)
                .expect("ordered insertion");
        }

        assert_eq!(epochs.active_insertions(), 17);
        assert_eq!(
            epochs.map_source(epoch(17, 0)).expect("map source 17"),
            epoch(34, 0)
        );
        assert_eq!(
            epochs
                .map_destination_ack(epoch(33, 0))
                .expect("map seventeenth synthetic ACK"),
            epoch(16, 0),
            "the inserted destination slot before source 17 must not ACK source 17"
        );
        assert_eq!(
            epochs
                .map_destination_ack(epoch(34, 0))
                .expect("map source 17 ACK"),
            epoch(17, 0)
        );
    }

    #[test]
    fn ordered_server_epoch_compaction_carries_the_trimmed_prefix_delta() {
        let mut epochs = OrderedServerSequenceEpochs::identity();
        for base in 1..=16u16 {
            epochs
                .insert_before(insertion_owner(base, 0, u64::from(base)), epoch(base, 0), 1)
                .expect("ordered insertion");
        }

        assert_eq!(
            epochs
                .compact(epoch(17, 0), epoch(32, 0))
                .expect("two-sided compaction"),
            16
        );
        assert_eq!(epochs.active_insertions(), 0);
        assert_eq!(epochs.checkpoint(), Some((epoch(16, 0), epoch(32, 0))));

        let seventeenth = epochs
            .insert_before(insertion_owner(17, 0, 17), epoch(17, 0), 1)
            .expect("post-compaction insertion");
        assert_eq!(seventeenth.destination_first, epoch(33, 0));
        assert_eq!(seventeenth.destination_after, epoch(34, 0));
        assert_eq!(
            epochs.map_source(epoch(17, 0)).expect("map source 17"),
            epoch(34, 0)
        );
        assert_eq!(
            epochs
                .map_destination_ack(epoch(33, 0))
                .expect("map synthetic ACK after compaction"),
            epoch(16, 0)
        );
    }

    #[test]
    fn ordered_server_epoch_compaction_requires_both_windows_to_pass_owner() {
        let mut epochs = OrderedServerSequenceEpochs::identity();
        let range = epochs
            .insert_before(insertion_owner(10, 0, 1), epoch(10, 0), 1)
            .expect("one insertion");
        let baseline = epochs.clone();

        assert_eq!(
            epochs
                .compact(epoch(10, 0), range.destination_after)
                .expect("source owner remains active"),
            0,
            "an equal source floor still permits an exact owner retransmit"
        );
        assert_eq!(epochs, baseline);
        assert_eq!(
            epochs
                .compact(epoch(11, 0), range.destination_first)
                .expect("destination insertion remains active"),
            0
        );
        assert_eq!(epochs, baseline);
        assert_eq!(
            epochs
                .compact(epoch(11, 0), range.destination_after)
                .expect("both windows passed insertion"),
            1
        );
        assert_eq!(
            epochs.checkpoint(),
            Some((epoch(10, 0), range.destination_after))
        );
        assert!(
            epochs.map_destination_ack(range.destination_first).is_err(),
            "a destination key before the compacted checkpoint is stale, not guessed"
        );
    }

    #[test]
    fn ordered_server_epochs_preserve_source_and_ack_order_across_wrap() {
        let source = epoch(u16::MAX, 0);
        let mut epochs = OrderedServerSequenceEpochs::identity_at(source);
        let range = epochs
            .insert_before(insertion_owner(u16::MAX, 0, 1), source, 1)
            .expect("wrapped insertion");

        assert_eq!(range.destination_first, epoch(u16::MAX, 0));
        assert_eq!(range.destination_after, epoch(0, 1));
        assert_eq!(
            epochs.map_source(source).expect("map wrapped source"),
            epoch(0, 1)
        );
        assert_eq!(
            epochs
                .map_source(epoch(0, 1))
                .expect("map post-wrap source"),
            epoch(1, 1)
        );
        assert_eq!(
            epochs
                .map_destination_ack(epoch(u16::MAX, 0))
                .expect("map inserted wrapped ACK"),
            epoch(u16::MAX - 1, 0)
        );
        assert_eq!(
            epochs
                .map_destination_ack(epoch(0, 1))
                .expect("map completed wrapped source"),
            source
        );
    }

    #[test]
    fn insertion_before_initial_zero_has_an_exact_negative_generation_predecessor() {
        let source = epoch(0, 0);
        let mut epochs = OrderedServerSequenceEpochs::identity_at(source);
        let range = epochs
            .insert_before(insertion_owner(0, 0, 1), source, 1)
            .expect("initial zero insertion");

        assert_eq!(range.destination_first, epoch(0, 0));
        assert_eq!(
            epochs
                .map_destination_ack(range.destination_first)
                .expect("map partial initial ACK"),
            epoch(u16::MAX, -1)
        );
        assert_eq!(
            epochs
                .map_destination_ack(range.destination_after)
                .expect("map completed initial source"),
            source
        );
    }

    #[test]
    fn same_base_insertions_retain_order_and_exact_owner_replay_is_idempotent() {
        let source = epoch(10, 0);
        let first_owner = insertion_owner(10, 0, 1);
        let second_owner = insertion_owner(10, 0, 2);
        let mut epochs = OrderedServerSequenceEpochs::identity_at(source);
        let first = epochs
            .insert_before(first_owner, source, 2)
            .expect("first same-base insertion");
        let second = epochs
            .insert_before(second_owner, source, 3)
            .expect("second same-base insertion");

        assert_eq!(first.destination_first, epoch(10, 0));
        assert_eq!(first.destination_after, epoch(12, 0));
        assert_eq!(second.destination_first, first.destination_after);
        assert_eq!(second.destination_after, epoch(15, 0));
        assert_eq!(
            epochs.map_source(source).expect("map source after both"),
            epoch(15, 0)
        );
        assert_eq!(
            epochs
                .map_destination_ack(epoch(14, 0))
                .expect("map second inserted range"),
            epoch(9, 0)
        );

        let baseline = epochs.clone();
        assert_eq!(
            epochs
                .insert_before(second_owner, source, 3)
                .expect("exact owner replay"),
            second
        );
        assert_eq!(epochs, baseline);
        assert!(
            epochs.insert_before(second_owner, source, 4).is_err(),
            "one exact owner cannot acquire a conflicting range"
        );
        assert_eq!(epochs, baseline);
    }

    #[test]
    fn ordered_server_epochs_fail_closed_on_order_capacity_and_generation_overflow() {
        let mut out_of_order = OrderedServerSequenceEpochs::identity();
        out_of_order
            .insert_before(insertion_owner(10, 0, 1), epoch(10, 0), 1)
            .expect("seed insertion");
        let baseline = out_of_order.clone();
        assert!(
            out_of_order
                .insert_before(insertion_owner(9, 0, 2), epoch(9, 0), 1)
                .is_err()
        );
        assert_eq!(out_of_order, baseline);

        let mut capacity = OrderedServerSequenceEpochs::identity_at(epoch(0, 0));
        for operation in 0..OrderedServerSequenceEpochs::MAX_ACTIVE_INSERTIONS {
            capacity
                .insert_before(insertion_owner(0, 0, operation as u64), epoch(0, 0), 1)
                .expect("fill active insertion capacity");
        }
        let baseline = capacity.clone();
        assert!(
            capacity
                .insert_before(
                    insertion_owner(
                        0,
                        0,
                        OrderedServerSequenceEpochs::MAX_ACTIVE_INSERTIONS as u64,
                    ),
                    epoch(0, 0),
                    1,
                )
                .is_err()
        );
        assert_eq!(capacity, baseline);

        let maximum = epoch(u16::MAX, i64::MAX);
        let mut overflow = OrderedServerSequenceEpochs::identity_at(maximum);
        let baseline = overflow.clone();
        assert!(
            overflow
                .insert_before(ServerSequenceInsertionOwner::new(maximum, 1), maximum, 1,)
                .is_err()
        );
        assert_eq!(overflow, baseline);
    }

    #[test]
    fn ordered_server_epoch_seed_preserves_an_existing_carried_delta() {
        let epochs = OrderedServerSequenceEpochs::seed(epoch(10, 2), epoch(13, 2))
            .expect("seed carried insertion delta");

        assert_eq!(
            epochs.map_source(epoch(12, 2)).expect("map seeded source"),
            epoch(15, 2)
        );
        assert_eq!(
            epochs
                .map_destination_ack(epoch(14, 2))
                .expect("map seeded ACK"),
            epoch(11, 2)
        );
        assert!(
            OrderedServerSequenceEpochs::seed(epoch(13, 2), epoch(12, 2)).is_err(),
            "server insertion epochs cannot seed a negative destination delta"
        );
    }

    #[test]
    fn record_forward_progress_ignores_retransmitted_older_sequence() {
        let mut latest = Some(74);

        record_forward_progress(&mut latest, 73);

        assert_eq!(latest, Some(74));
    }

    #[test]
    fn record_forward_progress_accepts_equal_forward_and_wrapped_sequences() {
        let mut latest = Some(74);
        record_forward_progress(&mut latest, 74);
        assert_eq!(latest, Some(74));

        let mut wrapped = Some(u16::MAX);
        record_forward_progress(&mut wrapped, 0);
        assert_eq!(wrapped, Some(0));
        record_forward_progress(&mut wrapped, u16::MAX);
        assert_eq!(
            wrapped,
            Some(0),
            "the pre-wrap value is stale after ACK/data sequence zero commits"
        );
        record_forward_progress(&mut wrapped, 1);
        assert_eq!(wrapped, Some(1));
    }

    #[test]
    fn wrapped_ack_zero_unshifts_across_inserted_sequence_zero() {
        let shifts = [SequenceShift { base: 0, delta: 1 }];

        assert_eq!(unshift_ack_for_origin(&shifts, 0), u16::MAX);
    }

    #[test]
    fn client_sequence_elision_maps_later_sequences_down() {
        let elisions = vec![
            SequenceElision { sequence: 3 },
            SequenceElision { sequence: 5 },
        ];

        assert_eq!(
            shift_sequence_for_peer_with_elisions(&[], &elisions, 2),
            Some(2)
        );
        assert_eq!(
            shift_sequence_for_peer_with_elisions(&[], &elisions, 3),
            None
        );
        assert_eq!(
            shift_sequence_for_peer_with_elisions(&[], &elisions, 4),
            Some(3)
        );
        assert_eq!(
            shift_sequence_for_peer_with_elisions(&[], &elisions, 6),
            Some(4)
        );
    }

    #[test]
    fn client_sequence_elision_maps_server_ack_back_up() {
        let elisions = vec![
            SequenceElision { sequence: 3 },
            SequenceElision { sequence: 5 },
        ];

        assert_eq!(unshift_ack_for_origin_with_elisions(&[], &elisions, 2), 3);
        assert_eq!(unshift_ack_for_origin_with_elisions(&[], &elisions, 3), 5);
        assert_eq!(unshift_ack_for_origin_with_elisions(&[], &elisions, 4), 6);
    }
}
