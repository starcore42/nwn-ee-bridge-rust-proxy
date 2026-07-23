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

    let mut prospective = state.slots.iter().cloned().collect::<Vec<_>>();
    let mut new_slots = Vec::new();
    let mut refreshed_slots = Vec::<PendingEeServerRefresh>::new();
    let mut next_key = state.next_key;

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

        let identity = transport_identity::server_reliable_data_transport_identity(packet, &view)
            .ok_or_else(|| {
            anyhow::anyhow!("EE send-window candidate left the reliable type-0 lane")
        })?;
        if let Some(existing) = prospective
            .iter()
            .find(|slot| slot.key.sequence == view.sequence)
        {
            if existing.transport_identity != identity {
                anyhow::bail!(
                    "EE send-window sequence {} conflicts with retained validated bytes",
                    view.sequence
                );
            }
            let refresh = PendingEeServerRefresh {
                key: existing.key,
                packet: packet.to_vec(),
                send_window_bit6: view.flags & transport_identity::SEND_WINDOW_BIT6_MASK,
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
            return Ok(());
        }

        let key = next_key.unwrap_or(EeServerSendKey {
            sequence: view.sequence,
            generation: 0,
        });
        if view.sequence != key.sequence {
            anyhow::bail!(
                "EE send-window output is noncontiguous: expected sequence {}, got {}",
                key.sequence,
                view.sequence
            );
        }
        if prospective.len() >= MAX_EE_SERVER_SEND_SLOTS {
            anyhow::bail!(
                "EE server send window exceeded {} unacknowledged reliable frames",
                MAX_EE_SERVER_SEND_SLOTS
            );
        }

        let slot = EeServerSendSlot {
            key,
            packet: packet.to_vec(),
            transport_identity: identity,
            send_window_bit6: view.flags & transport_identity::SEND_WINDOW_BIT6_MASK,
            next_retransmit_at: now + EE_SERVER_RETRANSMIT_DELAY,
            retransmits: 0,
        };
        prospective.push(slot.clone());
        new_slots.push(slot);
        next_key = Some(key.successor());
        Ok(())
    })?;

    tracing::trace!(
        owner = owner.as_str(),
        new_slots = new_slots.len(),
        refreshed_slots = refreshed_slots.len(),
        retained_slots = state.slots.len(),
        prospective_slots = prospective.len(),
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

/// Retire only an ACK that lands inside the current contiguous active interval.
/// A stale ACK or an impossible ACK beyond the last transmitted slot changes
/// no state, matching the original circular-window boundary.
pub(super) fn retire_through_raw_client_ack(
    state: &mut EeServerSendWindowState,
    ack_sequence: u16,
) -> usize {
    let Some(first) = state.slots.front().map(|slot| slot.key) else {
        return 0;
    };
    let distance = ack_sequence.wrapping_sub(first.sequence) as usize;
    if distance >= state.slots.len() || distance >= MAX_EE_SERVER_SEND_SLOTS {
        return 0;
    }
    if state
        .slots
        .get(distance)
        .is_none_or(|slot| slot.key.sequence != ack_sequence)
    {
        return 0;
    }

    let retired = distance + 1;
    for _ in 0..retired {
        let _ = state.slots.pop_front();
    }
    state.retired_slots = state.retired_slots.saturating_add(retired as u64);
    tracing::trace!(
        ack_sequence,
        retired_slots = retired,
        retained_slots = state.slots.len(),
        total_retired_slots = state.retired_slots,
        "raw EE ACK advanced the destination reliable send window"
    );
    retired
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

        assert_eq!(retire_through_raw_client_ack(&mut state, 0xFFFD), 0);
        assert_eq!(retire_through_raw_client_ack(&mut state, 0xFFFF), 2);
        assert_eq!(state.slots.front().map(|slot| slot.key.sequence), Some(0));
        assert_eq!(retire_through_raw_client_ack(&mut state, 0), 1);
        assert!(state.slots.is_empty());
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
