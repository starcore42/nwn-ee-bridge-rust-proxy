//! Reliable-window sequence/ack arithmetic.
//!
//! NWN's `M` reliable window uses wrapping 16-bit sequence numbers. The bridge
//! sometimes inserts synthetic packets, so peer-facing sequence numbers and
//! origin-facing ACKs must be shifted without confusing retransmit windows.
//! Keep this pure and packet-format-free: callers own packet mutation, this
//! module only answers sequence-ordering and delta questions.

#[derive(Debug, Clone)]
pub(super) struct SequenceShift {
    pub(super) base: u16,
    pub(super) delta: u16,
}

#[derive(Debug, Clone)]
pub(super) struct CoalescedSplitSequenceShift {
    pub(super) source_sequence: u16,
    pub(super) base: u16,
    pub(super) delta: u16,
}

#[derive(Debug, Clone)]
pub(super) struct SequenceElision {
    pub(super) sequence: u16,
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
