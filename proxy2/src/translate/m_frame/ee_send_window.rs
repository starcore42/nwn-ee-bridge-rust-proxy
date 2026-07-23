//! EE-facing reliable send-window ownership for validated server output.
//!
//! Proxy2 terminates and rebuilds the reliable server-to-client lane.  A
//! validated translated frame therefore needs its own destination send slot;
//! retaining only the Diamond source slot is insufficient when the translated
//! datagram is lost after the source has already been processed.
//!
//! Diamond initializes 16 outgoing slots at decompile lines 750687-750695 and
//! EE does the same at lines 891083-891087.  Their receive paths cumulatively
//! retire occupied outgoing slots through the peer ACK (Diamond
//! `sub_5F3940` lines 751677-751724; EE `FrameReceive` lines 879090-879135).
//! `FrameTimeout` then fetches one retained frame without re-entering CNW
//! dispatch and retries it after 0xDAC/3500 ms (Diamond lines 751817-751907;
//! EE lines 880417-880509).
//!
//! Keep the exact strictly validated plaintext as the immutable retry source.
//! On a retry, only the cumulative client-source ACK and its dependent CRC are
//! refreshed.  No CNW field, BOOL/bit order, payload cursor, nested boundary,
//! or semantic state is read or rebuilt here.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{MFrameType, MFrameView},
    translate::Emit,
};

use super::transport_identity;

pub(super) const MAX_EE_SERVER_SEND_SLOTS: usize = 16;
pub(super) const EE_SERVER_RETRANSMIT_DELAY: Duration = Duration::from_millis(0xDAC);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EeServerSendOwner {
    DirectServer,
    PendingServerDrain,
}

impl EeServerSendOwner {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::DirectServer => "direct_server",
            Self::PendingServerDrain => "pending_server_drain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EeServerSendKey {
    pub(super) sequence: u16,
    pub(super) generation: u64,
}

impl EeServerSendKey {
    fn successor(self) -> Self {
        Self {
            sequence: self.sequence.wrapping_add(1),
            generation: self
                .generation
                .saturating_add(u64::from(self.sequence == u16::MAX)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EeServerAckObservation {
    /// Exact destination-generation identity resolved from the active window
    /// or from the last cumulatively retired slot. A raw `u16` ACK that cannot
    /// be placed in either interval remains unresolved instead of borrowing a
    /// generation through half-range arithmetic.
    pub(super) acknowledged: Option<EeServerSendKey>,
    /// First destination identity that remains unacknowledged. When the active
    /// window is empty this is the retained next-new key, so a later sequence
    /// wrap cannot erase the epoch boundary needed by the sequence mapper.
    pub(super) destination_floor: Option<EeServerSendKey>,
    pub(super) retired_slots: usize,
}

#[derive(Debug, Clone)]
pub(super) struct EeServerSendSlot {
    pub(super) key: EeServerSendKey,
    /// Complete plaintext bytes that already passed the outer strict owner.
    pub(super) packet: Vec<u8>,
    /// Exact flags/payload identity with ACK, CRC, sequence, and the
    /// FrameSend-owned outer bit 6 excluded by the shared canonicalizer.
    pub(super) transport_identity: Vec<u8>,
    /// Latest destination FrameSend-owned bit accepted for this slot. A source
    /// replay may refresh it without changing immutable slot identity.
    pub(super) send_window_bit6: u8,
    pub(super) next_retransmit_at: Instant,
    pub(super) retransmits: u32,
}

#[derive(Debug)]
struct PendingEeServerRefresh {
    key: EeServerSendKey,
    packet: Vec<u8>,
    send_window_bit6: u8,
    next_retransmit_at: Instant,
}

#[derive(Debug)]
pub(super) struct PendingEeServerSend {
    pub(super) owner: EeServerSendOwner,
    new_slots: Vec<EeServerSendSlot>,
    refreshed_slots: Vec<PendingEeServerRefresh>,
    next_key: Option<EeServerSendKey>,
}

#[derive(Debug, Default)]
pub(super) struct EeServerSendWindowState {
    pub(super) slots: VecDeque<EeServerSendSlot>,
    /// Next new reliable destination identity.  Retain it while the window is
    /// empty so a post-ACK sequence jump cannot silently create a new epoch.
    next_key: Option<EeServerSendKey>,
    /// Exact terminal identity most recently retired by a raw EE cumulative
    /// ACK. Keep it after the active window becomes empty so an identical ACK
    /// remains generation-resolvable without guessing from a bare `u16`.
    pub(super) last_retired_key: Option<EeServerSendKey>,
    pub(super) pending: Option<PendingEeServerSend>,
    pub(super) retired_slots: u64,
    pub(super) retransmitted_slots: u64,
}

/// Atomically preflight the reliable type-0 members of one final validated
/// server output batch.  Nothing becomes retransmittable until `finish`
/// receives the matching accepted callback.
pub(super) fn stage(
    state: &mut EeServerSendWindowState,
    owner: EeServerSendOwner,
    emit: &Emit,
    now: Instant,
) -> anyhow::Result<()> {
    if let Some(pending) = state.pending.as_ref() {
        anyhow::bail!(
            "EE server send window already staged for {} before {}",
            pending.owner.as_str(),
            owner.as_str()
        );
    }

    #[derive(Debug)]
    struct ReliableCandidate {
        sequence: u16,
        packet: Vec<u8>,
        transport_identity: Vec<u8>,
        send_window_bit6: u8,
    }

    // Diamond and EE allocate 16 sequence-indexed outgoing slots before the
    // datagram scheduler transmits their bytes (Diamond 750687-750695 and
    // 751243-751389; EE 891083-891087 and 879824-879987). One final proxy Emit
    // can therefore visit those already-assigned sequences in wire-scheduling
    // order rather than slot order. Collect the complete batch first, then
    // prove that its new reliable members are exactly one contiguous interval.
    // This changes no CNW payload bytes, BOOL/bit order, or outgoing Emit order.
    let mut candidates = Vec::<ReliableCandidate>::new();
    visit_emit_packets(emit, &mut |packet| {
        let view = MFrameView::parse(packet)
            .ok_or_else(|| anyhow::anyhow!("EE send-window candidate is not a complete M frame"))?;
        if !view.crc_valid {
            anyhow::bail!("EE send-window candidate has an invalid M CRC");
        }
        let Some(kind) = view.frame_kind() else {
            anyhow::bail!("EE send-window candidate has unsupported M frame type");
        };
        if kind != MFrameType::ReliableData {
            if !view.is_exact_control_frame() {
                anyhow::bail!("EE send-window candidate has an impossible control shape");
            }
            return Ok(());
        }

        let transport_identity = transport_identity::server_reliable_data_transport_identity(
            packet, &view,
        )
        .ok_or_else(|| anyhow::anyhow!("EE send-window candidate left the reliable type-0 lane"))?;
        candidates.push(ReliableCandidate {
            sequence: view.sequence,
            packet: packet.to_vec(),
            transport_identity,
            send_window_bit6: view.flags & transport_identity::SEND_WINDOW_BIT6_MASK,
        });
        Ok(())
    })?;

    let mut unique_new = Vec::<ReliableCandidate>::new();
    let mut refreshed_slots = Vec::<PendingEeServerRefresh>::new();
    let mut same_batch_new_refreshes = 0usize;
    for candidate in candidates {
        if let Some(existing) = state
            .slots
            .iter()
            .find(|slot| slot.key.sequence == candidate.sequence)
        {
            if existing.transport_identity != candidate.transport_identity {
                anyhow::bail!(
                    "EE send-window sequence {} conflicts with retained validated bytes",
                    candidate.sequence
                );
            }
            let refresh = PendingEeServerRefresh {
                key: existing.key,
                packet: candidate.packet,
                send_window_bit6: candidate.send_window_bit6,
                next_retransmit_at: now + EE_SERVER_RETRANSMIT_DELAY,
            };
            if let Some(staged) = refreshed_slots
                .iter_mut()
                .find(|staged| staged.key == refresh.key)
            {
                *staged = refresh;
            } else {
                refreshed_slots.push(refresh);
            }
            continue;
        }

        if let Some(staged) = unique_new
            .iter_mut()
            .find(|staged| staged.sequence == candidate.sequence)
        {
            if staged.transport_identity != candidate.transport_identity {
                anyhow::bail!(
                    "EE send-window sequence {} conflicts inside one validated batch",
                    candidate.sequence
                );
            }
            // Match FrameSend's latest wire image for a repeated slot in the
            // same batch: ACK, bit 6, CRC, and retry deadline are refreshed,
            // while the immutable transport identity remains unchanged.
            staged.packet = candidate.packet;
            staged.send_window_bit6 = candidate.send_window_bit6;
            same_batch_new_refreshes = same_batch_new_refreshes.saturating_add(1);
            continue;
        }
        unique_new.push(candidate);
    }

    if state.slots.len().saturating_add(unique_new.len()) > MAX_EE_SERVER_SEND_SLOTS {
        anyhow::bail!(
            "EE server send window exceeded {} unacknowledged reliable frames",
            MAX_EE_SERVER_SEND_SLOTS
        );
    }

    if state.next_key.is_none() && !state.slots.is_empty() {
        anyhow::bail!("EE send-window active slots have no exact next-sequence anchor");
    }
    let interval_first = state.next_key.or_else(|| {
        unique_new.first().map(|candidate| EeServerSendKey {
            sequence: candidate.sequence,
            generation: 0,
        })
    });
    let mut ordered_new = Vec::<Option<ReliableCandidate>>::new();
    ordered_new.resize_with(unique_new.len(), || None);
    if let Some(interval_first) = interval_first {
        for candidate in unique_new {
            let distance = candidate.sequence.wrapping_sub(interval_first.sequence) as usize;
            if distance >= ordered_new.len() || distance >= MAX_EE_SERVER_SEND_SLOTS {
                anyhow::bail!(
                    "EE send-window new output is not one contiguous interval from sequence {}",
                    interval_first.sequence
                );
            }
            if ordered_new[distance].is_some() {
                anyhow::bail!(
                    "EE send-window sequence {} is ambiguous inside one validated batch",
                    candidate.sequence
                );
            }
            ordered_new[distance] = Some(candidate);
        }
    }
    if ordered_new.iter().any(Option::is_none) {
        anyhow::bail!("EE send-window new output contains a reliable sequence gap");
    }

    let mut key = interval_first;
    let mut new_slots = Vec::with_capacity(ordered_new.len());
    for candidate in ordered_new {
        let candidate =
            candidate.expect("complete contiguous EE send interval retains every candidate");
        let slot_key = key.ok_or_else(|| {
            anyhow::anyhow!("EE send-window new output has no exact sequence anchor")
        })?;
        if candidate.sequence != slot_key.sequence {
            anyhow::bail!(
                "EE send-window output is noncontiguous: expected sequence {}, got {}",
                slot_key.sequence,
                candidate.sequence
            );
        }
        new_slots.push(EeServerSendSlot {
            key: slot_key,
            packet: candidate.packet,
            transport_identity: candidate.transport_identity,
            send_window_bit6: candidate.send_window_bit6,
            next_retransmit_at: now + EE_SERVER_RETRANSMIT_DELAY,
            retransmits: 0,
        });
        key = Some(slot_key.successor());
    }
    let next_key = if new_slots.is_empty() {
        state.next_key
    } else {
        key
    };

    tracing::trace!(
        owner = owner.as_str(),
        new_slots = new_slots.len(),
        refreshed_slots = refreshed_slots
            .len()
            .saturating_add(same_batch_new_refreshes),
        retained_slots = state.slots.len(),
        prospective_slots = state.slots.len().saturating_add(new_slots.len()),
        "staged strictly validated server output for the EE reliable send window"
    );
    state.pending = Some(PendingEeServerSend {
        owner,
        new_slots,
        refreshed_slots,
        next_key,
    });
    Ok(())
}

pub(super) fn finish(
    state: &mut EeServerSendWindowState,
    owner: EeServerSendOwner,
    accepted: bool,
) -> usize {
    let Some(staged_owner) = state.pending.as_ref().map(|pending| pending.owner) else {
        return 0;
    };
    if staged_owner != owner {
        tracing::warn!(
            staged_owner = staged_owner.as_str(),
            callback_owner = owner.as_str(),
            accepted,
            "foreign EE send-window callback left the staged batch with its owner"
        );
        return 0;
    }
    let Some(pending) = state.pending.take() else {
        return 0;
    };
    if !accepted {
        tracing::trace!(
            owner = owner.as_str(),
            discarded_slots = pending.new_slots.len(),
            discarded_refreshes = pending.refreshed_slots.len(),
            "discarded staged EE send slots after final output rejection"
        );
        return 0;
    }

    // Insert newly staged slots before applying refreshes: one accepted Emit
    // may contain a new sequence followed by an exact resend of that same
    // sequence. The later wire image owns the retained ACK/bit-6/timer state.
    let committed = pending.new_slots.len();
    state.slots.extend(pending.new_slots);
    let refreshed = pending.refreshed_slots.len();
    for refresh in pending.refreshed_slots {
        let Some(slot) = state.slots.iter_mut().find(|slot| slot.key == refresh.key) else {
            tracing::warn!(
                sequence = refresh.key.sequence,
                generation = refresh.key.generation,
                "accepted EE send-window refresh lost its retained slot"
            );
            continue;
        };
        slot.packet = refresh.packet;
        slot.send_window_bit6 = refresh.send_window_bit6;
        slot.next_retransmit_at = refresh.next_retransmit_at;
    }
    state.next_key = pending.next_key;
    debug_assert!(state.slots.len() <= MAX_EE_SERVER_SEND_SLOTS);
    tracing::trace!(
        owner = owner.as_str(),
        committed_slots = committed,
        refreshed_slots = refreshed,
        retained_slots = state.slots.len(),
        "committed final validated output to the EE reliable send window"
    );
    committed
}

/// Resolve and retire only an ACK that lands inside the current contiguous
/// active interval. A duplicate of the last cumulative ACK retains its exact
/// generation even after the window empties. Any other stale or impossible
/// bare `u16` ACK stays unresolved and changes no state.
pub(super) fn retire_through_raw_client_ack(
    state: &mut EeServerSendWindowState,
    ack_sequence: u16,
) -> EeServerAckObservation {
    let unresolved = |state: &EeServerSendWindowState| EeServerAckObservation {
        acknowledged: state
            .last_retired_key
            .filter(|key| key.sequence == ack_sequence),
        destination_floor: destination_floor(state),
        retired_slots: 0,
    };
    let Some(first) = state.slots.front().map(|slot| slot.key) else {
        return unresolved(state);
    };
    let distance = ack_sequence.wrapping_sub(first.sequence) as usize;
    if distance >= state.slots.len() || distance >= MAX_EE_SERVER_SEND_SLOTS {
        return unresolved(state);
    }
    if state
        .slots
        .get(distance)
        .is_none_or(|slot| slot.key.sequence != ack_sequence)
    {
        return unresolved(state);
    }

    let acknowledged = state
        .slots
        .get(distance)
        .map(|slot| slot.key)
        .expect("validated EE ACK distance still identifies its terminal slot");
    let retired = distance + 1;
    for _ in 0..retired {
        let _ = state.slots.pop_front();
    }
    state.last_retired_key = Some(acknowledged);
    state.retired_slots = state.retired_slots.saturating_add(retired as u64);
    let destination_floor = destination_floor(state);
    tracing::trace!(
        ack_sequence,
        ack_generation = acknowledged.generation,
        retired_slots = retired,
        retained_slots = state.slots.len(),
        destination_floor_sequence = ?destination_floor.map(|key| key.sequence),
        destination_floor_generation = ?destination_floor.map(|key| key.generation),
        total_retired_slots = state.retired_slots,
        "raw EE ACK advanced the destination reliable send window"
    );
    EeServerAckObservation {
        acknowledged: Some(acknowledged),
        destination_floor,
        retired_slots: retired,
    }
}

pub(super) fn destination_floor(state: &EeServerSendWindowState) -> Option<EeServerSendKey> {
    state.slots.front().map(|slot| slot.key).or(state.next_key)
}

/// Return at most one expired retry per network-loop pass, as the original
/// timeout path does.  The retained validated bytes remain unchanged; the
/// outgoing clone refreshes only FrameSend-owned ACK/CRC transport fields.
pub(super) fn take_due_retransmit(
    state: &mut EeServerSendWindowState,
    now: Instant,
    current_client_source_ack: Option<u16>,
) -> anyhow::Result<Option<Vec<u8>>> {
    if state.pending.is_some() {
        anyhow::bail!("EE send-window retransmit requested during staged output validation");
    }
    let Some(index) = state
        .slots
        .iter()
        .enumerate()
        .filter(|(_, slot)| slot.next_retransmit_at <= now)
        .min_by_key(|(_, slot)| slot.next_retransmit_at)
        .map(|(index, _)| index)
    else {
        return Ok(None);
    };
    let retained_slots = state.slots.len();
    let slot = state
        .slots
        .get_mut(index)
        .expect("due EE send-window slot remains indexed");
    let mut packet = slot.packet.clone();
    transport_identity::refresh_send_window_bit6(&mut packet, slot.send_window_bit6)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to refresh EE retransmit FrameSend bit 6"))?;
    if let Some(ack_sequence) = current_client_source_ack {
        write_be_u16(&mut packet, 5, ack_sequence)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to refresh EE retransmit ACK"))?;
    }
    encode_legacy_m_crc(&mut packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to refresh EE retransmit CRC"))?;
    slot.next_retransmit_at = now + EE_SERVER_RETRANSMIT_DELAY;
    slot.retransmits = slot.retransmits.saturating_add(1);
    state.retransmitted_slots = state.retransmitted_slots.saturating_add(1);
    tracing::info!(
        sequence = slot.key.sequence,
        generation = slot.key.generation,
        retransmits = slot.retransmits,
        retained_slots,
        current_client_source_ack = ?current_client_source_ack,
        "EE reliable send-window timer retransmitting retained validated server output"
    );
    Ok(Some(packet))
}

fn visit_emit_packets(
    emit: &Emit,
    visitor: &mut impl FnMut(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    match emit {
        Emit::Packet(packet) | Emit::PacketRetireSession { packet, .. } => visitor(packet),
        Emit::Packets(packets)
        | Emit::PacketsPreShifted(packets)
        | Emit::VerifiedPackets { packets, .. }
        | Emit::VerifiedPacketsPreShifted { packets, .. }
        | Emit::VerifiedProofPackets { packets, .. }
        | Emit::VerifiedProofPacketsPreShifted { packets, .. } => {
            for packet in packets {
                visitor(packet)?;
            }
            Ok(())
        }
        Emit::MixedVerifiedPackets(packets) => {
            for (_, packet) in packets {
                visitor(packet)?;
            }
            Ok(())
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            for (_, packet) in packets {
                visitor(packet)?;
            }
            Ok(())
        }
        Emit::Consumed | Emit::ConsumedRetireSession { .. } | Emit::Drop => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reliable(sequence: u16, ack_sequence: u16, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0; crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, sequence));
        assert!(write_be_u16(&mut packet, 5, ack_sequence));
        packet[7] = 0x08;
        assert!(write_be_u16(&mut packet, 8, 1));
        assert!(write_be_u16(&mut packet, 10, payload.len() as u16));
        packet.extend_from_slice(payload);
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }

    fn commit(
        state: &mut EeServerSendWindowState,
        now: Instant,
        packets: Vec<Vec<u8>>,
    ) -> anyhow::Result<()> {
        stage(
            state,
            EeServerSendOwner::DirectServer,
            &Emit::Packets(packets),
            now,
        )?;
        assert!(finish(state, EeServerSendOwner::DirectServer, true) <= MAX_EE_SERVER_SEND_SLOTS);
        Ok(())
    }

    #[test]
    fn validated_slots_commit_atomically_and_raw_ack_retires_prefix() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState::default();
        commit(
            &mut state,
            now,
            vec![reliable(0xFFFE, 7, b"a"), reliable(0xFFFF, 7, b"b")],
        )
        .expect("first validated batch");
        commit(&mut state, now, vec![reliable(0, 7, b"c")]).expect("wrapped validated frame");
        assert_eq!(state.slots.len(), 3);
        assert_eq!(state.slots[2].key.generation, 1);

        let stale = retire_through_raw_client_ack(&mut state, 0xFFFD);
        assert_eq!(stale.acknowledged, None);
        assert_eq!(stale.retired_slots, 0);
        assert_eq!(
            stale.destination_floor,
            Some(EeServerSendKey {
                sequence: 0xFFFE,
                generation: 0,
            })
        );
        let pre_wrap = retire_through_raw_client_ack(&mut state, 0xFFFF);
        assert_eq!(
            pre_wrap.acknowledged,
            Some(EeServerSendKey {
                sequence: 0xFFFF,
                generation: 0,
            })
        );
        assert_eq!(pre_wrap.retired_slots, 2);
        assert_eq!(state.slots.front().map(|slot| slot.key.sequence), Some(0));
        let wrapped = retire_through_raw_client_ack(&mut state, 0);
        assert_eq!(
            wrapped.acknowledged,
            Some(EeServerSendKey {
                sequence: 0,
                generation: 1,
            })
        );
        assert_eq!(
            wrapped.destination_floor,
            Some(EeServerSendKey {
                sequence: 1,
                generation: 1,
            })
        );
        assert_eq!(wrapped.retired_slots, 1);
        assert!(state.slots.is_empty());

        let duplicate = retire_through_raw_client_ack(&mut state, 0);
        assert_eq!(duplicate.acknowledged, wrapped.acknowledged);
        assert_eq!(duplicate.destination_floor, wrapped.destination_floor);
        assert_eq!(duplicate.retired_slots, 0);
        assert_eq!(state.last_retired_key, wrapped.acknowledged);

        let unresolvable = retire_through_raw_client_ack(&mut state, u16::MAX);
        assert_eq!(unresolvable.acknowledged, None);
        assert_eq!(unresolvable.destination_floor, wrapped.destination_floor);
        assert_eq!(unresolvable.retired_slots, 0);
    }

    #[test]
    fn known_next_key_admits_permuted_contiguous_batch_in_exact_slot_order() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState {
            next_key: Some(EeServerSendKey {
                sequence: 61,
                generation: 4,
            }),
            ..EeServerSendWindowState::default()
        };
        let wire_sequences = [61u16, 65, 62, 63, 64];
        let wire_packets = wire_sequences
            .into_iter()
            .map(|sequence| reliable(sequence, 7, &[sequence as u8]))
            .collect::<Vec<_>>();
        let emit = Emit::Packets(wire_packets.clone());

        stage(&mut state, EeServerSendOwner::DirectServer, &emit, now)
            .expect("one permuted contiguous destination interval should stage");

        let pending = state.pending.as_ref().expect("permuted batch pending");
        assert_eq!(
            pending
                .new_slots
                .iter()
                .map(|slot| slot.key)
                .collect::<Vec<_>>(),
            (61..=65)
                .map(|sequence| EeServerSendKey {
                    sequence,
                    generation: 4,
                })
                .collect::<Vec<_>>()
        );
        let Emit::Packets(staged_wire_packets) = &emit else {
            panic!("test Emit shape changed during send-window admission");
        };
        assert_eq!(
            staged_wire_packets, &wire_packets,
            "send-window admission must not reorder the outgoing Emit"
        );

        assert_eq!(
            finish(&mut state, EeServerSendOwner::DirectServer, true),
            wire_sequences.len()
        );
        assert_eq!(
            state
                .slots
                .iter()
                .map(|slot| slot.key.sequence)
                .collect::<Vec<_>>(),
            vec![61, 62, 63, 64, 65]
        );
        assert_eq!(
            state.next_key,
            Some(EeServerSendKey {
                sequence: 66,
                generation: 4,
            })
        );
    }

    #[test]
    fn permuted_batch_with_a_sequence_gap_fails_atomically() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState {
            next_key: Some(EeServerSendKey {
                sequence: 61,
                generation: 2,
            }),
            ..EeServerSendWindowState::default()
        };
        let baseline_next_key = state.next_key;

        assert!(
            stage(
                &mut state,
                EeServerSendOwner::DirectServer,
                &Emit::Packets(vec![reliable(61, 7, b"first"), reliable(63, 7, b"gap"),]),
                now,
            )
            .is_err()
        );
        assert!(state.pending.is_none());
        assert!(state.slots.is_empty());
        assert_eq!(state.next_key, baseline_next_key);
    }

    #[test]
    fn permuted_contiguous_batch_preserves_exact_generation_across_wrap() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState {
            next_key: Some(EeServerSendKey {
                sequence: u16::MAX - 1,
                generation: 7,
            }),
            ..EeServerSendWindowState::default()
        };

        commit(
            &mut state,
            now,
            vec![
                reliable(u16::MAX - 1, 7, b"fffe"),
                reliable(1, 7, b"one"),
                reliable(u16::MAX, 7, b"ffff"),
                reliable(0, 7, b"zero"),
            ],
        )
        .expect("wrapped permutation should commit");

        assert_eq!(
            state.slots.iter().map(|slot| slot.key).collect::<Vec<_>>(),
            vec![
                EeServerSendKey {
                    sequence: u16::MAX - 1,
                    generation: 7,
                },
                EeServerSendKey {
                    sequence: u16::MAX,
                    generation: 7,
                },
                EeServerSendKey {
                    sequence: 0,
                    generation: 8,
                },
                EeServerSendKey {
                    sequence: 1,
                    generation: 8,
                },
            ]
        );
        assert_eq!(
            state.next_key,
            Some(EeServerSendKey {
                sequence: 2,
                generation: 8,
            })
        );
    }

    #[test]
    fn unseeded_rotated_interval_and_same_sequence_conflict_fail_atomically() {
        let now = Instant::now();
        let mut rotated = EeServerSendWindowState::default();
        assert!(
            stage(
                &mut rotated,
                EeServerSendOwner::DirectServer,
                &Emit::Packets(
                    [65u16, 61, 62, 63, 64]
                        .into_iter()
                        .map(|sequence| reliable(sequence, 7, &[sequence as u8]))
                        .collect(),
                ),
                now,
            )
            .is_err(),
            "an unseeded window must not guess an anchor before the first new sequence"
        );
        assert!(rotated.pending.is_none());
        assert!(rotated.slots.is_empty());
        assert_eq!(rotated.next_key, None);

        let mut conflict = EeServerSendWindowState::default();
        assert!(
            stage(
                &mut conflict,
                EeServerSendOwner::DirectServer,
                &Emit::Packets(vec![
                    reliable(40, 3, b"first"),
                    reliable(40, 8, b"different"),
                ]),
                now,
            )
            .is_err(),
            "one new sequence cannot claim two immutable identities"
        );
        assert!(conflict.pending.is_none());
        assert!(conflict.slots.is_empty());
        assert_eq!(conflict.next_key, None);
    }

    #[test]
    fn rejection_conflict_and_capacity_never_partially_commit() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState::default();
        stage(
            &mut state,
            EeServerSendOwner::DirectServer,
            &Emit::Packets(vec![reliable(10, 1, b"a"), reliable(11, 1, b"b")]),
            now,
        )
        .expect("stage");
        assert_eq!(
            finish(&mut state, EeServerSendOwner::DirectServer, false),
            0
        );
        assert!(state.slots.is_empty());

        commit(&mut state, now, vec![reliable(10, 1, b"a")]).expect("commit first");
        assert!(
            stage(
                &mut state,
                EeServerSendOwner::DirectServer,
                &Emit::Packets(vec![reliable(10, 2, b"different")]),
                now,
            )
            .is_err()
        );
        assert_eq!(state.slots.len(), 1);

        let remaining = (11..26)
            .map(|sequence| reliable(sequence, 1, &[sequence as u8]))
            .collect();
        commit(&mut state, now, remaining).expect("fill all sixteen slots");
        assert_eq!(state.slots.len(), MAX_EE_SERVER_SEND_SLOTS);
        assert!(
            stage(
                &mut state,
                EeServerSendOwner::DirectServer,
                &Emit::Packets(vec![reliable(26, 1, b"overflow")]),
                now,
            )
            .is_err()
        );
        assert_eq!(state.slots.len(), MAX_EE_SERVER_SEND_SLOTS);
    }

    #[test]
    fn timer_replays_retained_bytes_once_and_refreshes_only_ack_crc() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState::default();
        let original = reliable(20, 3, b"payload");
        commit(&mut state, now, vec![original.clone()]).expect("commit");

        assert!(
            take_due_retransmit(
                &mut state,
                now + EE_SERVER_RETRANSMIT_DELAY - Duration::from_millis(1),
                Some(9),
            )
            .expect("not due")
            .is_none()
        );
        let retry = take_due_retransmit(&mut state, now + EE_SERVER_RETRANSMIT_DELAY, Some(9))
            .expect("due retry")
            .expect("one retry");
        let retry_view = MFrameView::parse(&retry).expect("valid retry");
        assert_eq!(retry_view.sequence, 20);
        assert_eq!(retry_view.ack_sequence, 9);
        assert!(retry_view.crc_valid);
        assert_eq!(&retry[7..], &original[7..]);
        assert_eq!(state.slots[0].packet, original);
        assert_eq!(state.slots[0].retransmits, 1);
        assert!(
            take_due_retransmit(&mut state, now + EE_SERVER_RETRANSMIT_DELAY, Some(9),)
                .expect("rescheduled")
                .is_none()
        );
    }

    #[test]
    fn accepted_duplicate_refreshes_send_bit_and_restarts_slot_timer() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState::default();
        commit(&mut state, now, vec![reliable(30, 3, b"payload")]).expect("commit");

        let resent_at = now + EE_SERVER_RETRANSMIT_DELAY - Duration::from_millis(100);
        let mut duplicate = reliable(30, 8, b"payload");
        duplicate[7] |= transport_identity::SEND_WINDOW_BIT6_MASK;
        assert!(encode_legacy_m_crc(&mut duplicate));
        stage(
            &mut state,
            EeServerSendOwner::DirectServer,
            &Emit::Packet(duplicate.clone()),
            resent_at,
        )
        .expect("stage exact duplicate");
        assert_eq!(finish(&mut state, EeServerSendOwner::DirectServer, true), 0);
        assert_eq!(state.slots[0].packet, duplicate);
        assert_eq!(
            state.slots[0].send_window_bit6,
            transport_identity::SEND_WINDOW_BIT6_MASK
        );

        assert!(
            take_due_retransmit(&mut state, now + EE_SERVER_RETRANSMIT_DELAY, Some(9))
                .expect("old deadline")
                .is_none(),
            "an accepted resend must restart the original FrameSend timer"
        );
        let retry =
            take_due_retransmit(&mut state, resent_at + EE_SERVER_RETRANSMIT_DELAY, Some(9))
                .expect("new deadline")
                .expect("one retry");
        let retry_view = MFrameView::parse(&retry).expect("valid retry");
        assert_eq!(
            retry_view.flags & transport_identity::SEND_WINDOW_BIT6_MASK,
            transport_identity::SEND_WINDOW_BIT6_MASK
        );
        assert_eq!(retry_view.ack_sequence, 9);
        assert!(retry_view.crc_valid);
    }

    #[test]
    fn same_batch_duplicate_refreshes_new_slot_to_latest_wire_image() {
        let now = Instant::now();
        let mut state = EeServerSendWindowState::default();
        let first = reliable(40, 3, b"payload");
        let mut duplicate = reliable(40, 8, b"payload");
        duplicate[7] |= transport_identity::SEND_WINDOW_BIT6_MASK;
        assert!(encode_legacy_m_crc(&mut duplicate));

        stage(
            &mut state,
            EeServerSendOwner::DirectServer,
            &Emit::Packets(vec![first, duplicate.clone()]),
            now,
        )
        .expect("stage new sequence followed by exact duplicate");
        assert_eq!(finish(&mut state, EeServerSendOwner::DirectServer, true), 1);
        assert_eq!(state.slots.len(), 1);
        assert_eq!(state.slots[0].packet, duplicate);
        assert_eq!(
            state.slots[0].send_window_bit6,
            transport_identity::SEND_WINDOW_BIT6_MASK
        );
    }
}
