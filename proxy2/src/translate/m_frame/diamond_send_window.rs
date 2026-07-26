//! Diamond-facing reliable send-window ownership for validated client output.
//!
//! The EE receive window and Diamond send window are independent endpoints.
//! A translated client frame therefore needs a retained Diamond-facing output
//! slot before proxy2 can safely release the corresponding EE receive slot.
//! This module establishes that destination ownership without changing source
//! release yet.
//!
//! Diamond initializes 16 outgoing slots at lines 750687-750695, cumulatively
//! retires them through the peer ACK at `sub_5F3940` lines 751677-751724, and
//! retries one retained immutable frame after 0xDAC/3500 ms at lines
//! 751817-751907. EE has the same split at lines 891083-891087,
//! 879090-879135, and 880417-880509.
//!
//! Retain only plaintext frames that passed the outer strict validator. A
//! retry refreshes the current cumulative HG-source ACK, FrameSend-owned bit
//! 6, and CRC. It never rebuilds CNW payload fields or re-enters semantics.

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

pub(super) const MAX_DIAMOND_CLIENT_SEND_SLOTS: usize = 16;
pub(super) const DIAMOND_CLIENT_RETRANSMIT_DELAY: Duration = Duration::from_millis(0xDAC);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiamondClientSendOwner {
    DirectClient,
    PendingClientDrain,
}

impl DiamondClientSendOwner {
    fn as_str(self) -> &'static str {
        match self {
            Self::DirectClient => "direct_client",
            Self::PendingClientDrain => "pending_client_drain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DiamondClientSendKey {
    pub(super) sequence: u16,
    pub(super) generation: u64,
}

impl DiamondClientSendKey {
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
pub(super) struct DiamondClientSendSlot {
    pub(super) key: DiamondClientSendKey,
    pub(super) packet: Vec<u8>,
    /// ACK, CRC, sequence, and FrameSend-owned bit 6 are excluded.
    pub(super) transport_identity: Vec<u8>,
    pub(super) send_window_bit6: u8,
    pub(super) next_retransmit_at: Instant,
    pub(super) retransmits: u32,
}

#[derive(Debug)]
struct PendingRefresh {
    key: DiamondClientSendKey,
    packet: Vec<u8>,
    send_window_bit6: u8,
    next_retransmit_at: Instant,
}

#[derive(Debug)]
pub(super) struct PendingDiamondClientSend {
    pub(super) owner: DiamondClientSendOwner,
    new_slots: Vec<DiamondClientSendSlot>,
    refreshed_slots: Vec<PendingRefresh>,
    next_key: Option<DiamondClientSendKey>,
}

#[derive(Debug, Default)]
pub(super) struct DiamondClientSendWindowState {
    pub(super) slots: VecDeque<DiamondClientSendSlot>,
    /// Preserve the exact next-new key after the active window empties so a
    /// later wrap cannot borrow the wrong generation.
    next_key: Option<DiamondClientSendKey>,
    pub(super) pending: Option<PendingDiamondClientSend>,
    pub(super) retired_slots: u64,
    pub(super) retransmitted_slots: u64,
}

/// Preflight all reliable members of one final validated client-output batch.
/// Emit order need not be slot order, so new members are collected and then
/// proven to be one contiguous destination interval.
pub(super) fn stage(
    state: &mut DiamondClientSendWindowState,
    owner: DiamondClientSendOwner,
    emit: &Emit,
    now: Instant,
) -> anyhow::Result<()> {
    if let Some(pending) = state.pending.as_ref() {
        anyhow::bail!(
            "Diamond client send window already staged for {} before {}",
            pending.owner.as_str(),
            owner.as_str()
        );
    }

    #[derive(Debug)]
    struct Candidate {
        sequence: u16,
        packet: Vec<u8>,
        transport_identity: Vec<u8>,
        send_window_bit6: u8,
    }

    let mut candidates = Vec::<Candidate>::new();
    visit_emit_packets(emit, &mut |packet| {
        let view = MFrameView::parse(packet).ok_or_else(|| {
            anyhow::anyhow!("Diamond client send-window candidate is not a complete M frame")
        })?;
        if !view.crc_valid {
            anyhow::bail!("Diamond client send-window candidate has an invalid M CRC");
        }
        let Some(kind) = view.frame_kind() else {
            anyhow::bail!("Diamond client send-window candidate has unsupported M frame type");
        };
        if kind != MFrameType::ReliableData {
            if !view.is_exact_control_frame() {
                anyhow::bail!(
                    "Diamond client send-window candidate has an impossible control shape"
                );
            }
            return Ok(());
        }
        let transport_identity = transport_identity::reliable_data_transport_identity(
            packet, &view,
        )
        .ok_or_else(|| {
            anyhow::anyhow!("Diamond client send-window candidate left the reliable type-0 lane")
        })?;
        candidates.push(Candidate {
            sequence: view.sequence,
            packet: packet.to_vec(),
            transport_identity,
            send_window_bit6: view.flags & transport_identity::SEND_WINDOW_BIT6_MASK,
        });
        Ok(())
    })?;

    let mut unique_new = Vec::<Candidate>::new();
    let mut refreshed_slots = Vec::<PendingRefresh>::new();
    for candidate in candidates {
        if let Some(existing) = state
            .slots
            .iter()
            .find(|slot| slot.key.sequence == candidate.sequence)
        {
            if existing.transport_identity != candidate.transport_identity {
                anyhow::bail!(
                    "Diamond client send-window sequence {} conflicts with retained bytes",
                    candidate.sequence
                );
            }
            let refresh = PendingRefresh {
                key: existing.key,
                packet: candidate.packet,
                send_window_bit6: candidate.send_window_bit6,
                next_retransmit_at: now + DIAMOND_CLIENT_RETRANSMIT_DELAY,
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
                    "Diamond client send-window sequence {} conflicts inside one batch",
                    candidate.sequence
                );
            }
            staged.packet = candidate.packet;
            staged.send_window_bit6 = candidate.send_window_bit6;
        } else {
            unique_new.push(candidate);
        }
    }

    if state.slots.len().saturating_add(unique_new.len()) > MAX_DIAMOND_CLIENT_SEND_SLOTS {
        anyhow::bail!(
            "Diamond client send window exceeded {} unacknowledged frames",
            MAX_DIAMOND_CLIENT_SEND_SLOTS
        );
    }
    if state.next_key.is_none() && !state.slots.is_empty() {
        anyhow::bail!("Diamond client send-window active slots have no next-sequence anchor");
    }

    let interval_first = state.next_key.or_else(|| {
        unique_new.first().map(|candidate| DiamondClientSendKey {
            sequence: candidate.sequence,
            generation: 0,
        })
    });
    let mut ordered_new = Vec::<Option<Candidate>>::new();
    ordered_new.resize_with(unique_new.len(), || None);
    if let Some(interval_first) = interval_first {
        for candidate in unique_new {
            let distance = candidate.sequence.wrapping_sub(interval_first.sequence) as usize;
            if distance >= ordered_new.len() || distance >= MAX_DIAMOND_CLIENT_SEND_SLOTS {
                anyhow::bail!(
                    "Diamond client send-window new output is not contiguous from sequence {}",
                    interval_first.sequence
                );
            }
            if ordered_new[distance].is_some() {
                anyhow::bail!(
                    "Diamond client send-window sequence {} is ambiguous inside one batch",
                    candidate.sequence
                );
            }
            ordered_new[distance] = Some(candidate);
        }
    }
    if ordered_new.iter().any(Option::is_none) {
        anyhow::bail!("Diamond client send-window new output contains a sequence gap");
    }

    let mut key = interval_first;
    let mut new_slots = Vec::with_capacity(ordered_new.len());
    for candidate in ordered_new {
        let candidate = candidate.expect("contiguous interval retains every candidate");
        let slot_key =
            key.ok_or_else(|| anyhow::anyhow!("Diamond client output has no sequence anchor"))?;
        new_slots.push(DiamondClientSendSlot {
            key: slot_key,
            packet: candidate.packet,
            transport_identity: candidate.transport_identity,
            send_window_bit6: candidate.send_window_bit6,
            next_retransmit_at: now + DIAMOND_CLIENT_RETRANSMIT_DELAY,
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
        refreshed_slots = refreshed_slots.len(),
        retained_slots = state.slots.len(),
        prospective_slots = state.slots.len().saturating_add(new_slots.len()),
        "staged validated client output for the Diamond reliable send window"
    );
    state.pending = Some(PendingDiamondClientSend {
        owner,
        new_slots,
        refreshed_slots,
        next_key,
    });
    Ok(())
}

pub(super) fn finish(
    state: &mut DiamondClientSendWindowState,
    owner: DiamondClientSendOwner,
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
            "foreign Diamond send-window callback retained its staged batch"
        );
        return 0;
    }
    let pending = state.pending.take().expect("matching pending send batch");
    if !accepted {
        return 0;
    }

    let committed = pending.new_slots.len();
    state.slots.extend(pending.new_slots);
    for refresh in pending.refreshed_slots {
        if let Some(slot) = state.slots.iter_mut().find(|slot| slot.key == refresh.key) {
            slot.packet = refresh.packet;
            slot.send_window_bit6 = refresh.send_window_bit6;
            slot.next_retransmit_at = refresh.next_retransmit_at;
        }
    }
    state.next_key = pending.next_key;
    debug_assert!(state.slots.len() <= MAX_DIAMOND_CLIENT_SEND_SLOTS);
    tracing::trace!(
        owner = owner.as_str(),
        committed_slots = committed,
        retained_slots = state.slots.len(),
        "committed validated output to the Diamond reliable send window"
    );
    committed
}

pub(super) fn retire_through_raw_server_ack(
    state: &mut DiamondClientSendWindowState,
    ack_sequence: u16,
) -> usize {
    let Some(first) = state.slots.front().map(|slot| slot.key) else {
        return 0;
    };
    let distance = ack_sequence.wrapping_sub(first.sequence) as usize;
    if distance >= state.slots.len()
        || distance >= MAX_DIAMOND_CLIENT_SEND_SLOTS
        || state
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
        "raw HG ACK advanced the Diamond-facing client send window"
    );
    retired
}

pub(super) fn take_due_retransmit(
    state: &mut DiamondClientSendWindowState,
    now: Instant,
    current_server_source_ack: Option<u16>,
) -> anyhow::Result<Option<Vec<u8>>> {
    if state.pending.is_some() {
        anyhow::bail!("Diamond retransmit requested during staged output validation");
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
        .expect("due slot remains indexed");
    let mut packet = slot.packet.clone();
    transport_identity::refresh_send_window_bit6(&mut packet, slot.send_window_bit6)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to refresh Diamond FrameSend bit 6"))?;
    if let Some(ack_sequence) = current_server_source_ack {
        write_be_u16(&mut packet, 5, ack_sequence)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to refresh Diamond retransmit ACK"))?;
    }
    encode_legacy_m_crc(&mut packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to refresh Diamond retransmit CRC"))?;
    slot.next_retransmit_at = now + DIAMOND_CLIENT_RETRANSMIT_DELAY;
    slot.retransmits = slot.retransmits.saturating_add(1);
    state.retransmitted_slots = state.retransmitted_slots.saturating_add(1);
    tracing::info!(
        sequence = slot.key.sequence,
        generation = slot.key.generation,
        retransmits = slot.retransmits,
        retained_slots,
        current_server_source_ack = ?current_server_source_ack,
        "Diamond send-window timer retransmitting retained client output"
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
    use crate::{
        crc::{encode_legacy_m_crc, write_be_u16},
        translate::VerifiedFamily,
    };

    fn reliable(sequence: u16, ack_sequence: u16, marker: u8) -> Vec<u8> {
        let mut packet = vec![b'M', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, marker];
        assert!(write_be_u16(&mut packet, 3, sequence));
        assert!(write_be_u16(&mut packet, 5, ack_sequence));
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }

    #[test]
    fn split_output_owns_one_contiguous_interval() {
        let now = Instant::now();
        let mut state = DiamondClientSendWindowState::default();
        let emit = Emit::MixedVerifiedPackets(vec![
            (VerifiedFamily::ConsumedEmptyMFrame, reliable(41, 19, 0xA1)),
            (VerifiedFamily::ConsumedEmptyMFrame, reliable(42, 19, 0xA2)),
        ]);
        stage(&mut state, DiamondClientSendOwner::DirectClient, &emit, now)
            .expect("stage split output");
        assert_eq!(
            finish(&mut state, DiamondClientSendOwner::DirectClient, true,),
            2
        );
        assert_eq!(state.slots.len(), 2);
    }

    #[test]
    fn raw_ack_retires_and_retry_refreshes_ack_crc() {
        let now = Instant::now();
        let mut state = DiamondClientSendWindowState::default();
        let emit = Emit::VerifiedPackets {
            family: VerifiedFamily::ConsumedEmptyMFrame,
            packets: vec![reliable(u16::MAX, 30, 0xB1), reliable(0, 30, 0xB2)],
        };
        stage(
            &mut state,
            DiamondClientSendOwner::DirectClient,
            &emit,
            now - DIAMOND_CLIENT_RETRANSMIT_DELAY,
        )
        .expect("stage wrapping output");
        assert_eq!(
            finish(&mut state, DiamondClientSendOwner::DirectClient, true,),
            2
        );
        assert_eq!(state.slots[1].key.generation, 1);

        let retry = take_due_retransmit(&mut state, now, Some(44))
            .expect("retry")
            .expect("due retry");
        let view = MFrameView::parse(&retry).expect("retry view");
        assert_eq!(view.sequence, u16::MAX);
        assert_eq!(view.ack_sequence, 44);
        assert!(view.crc_valid);
        assert_eq!(retry[12], 0xB1);
        assert_eq!(retire_through_raw_server_ack(&mut state, 0), 2);
        assert!(state.slots.is_empty());
    }

    #[test]
    fn rejected_batch_never_becomes_retransmittable() {
        let mut state = DiamondClientSendWindowState::default();
        let emit = Emit::VerifiedPackets {
            family: VerifiedFamily::ConsumedEmptyMFrame,
            packets: vec![reliable(7, 3, 0xCC)],
        };
        stage(
            &mut state,
            DiamondClientSendOwner::PendingClientDrain,
            &emit,
            Instant::now(),
        )
        .expect("stage rejected output");
        assert_eq!(
            finish(
                &mut state,
                DiamondClientSendOwner::PendingClientDrain,
                false,
            ),
            0
        );
        assert!(state.slots.is_empty());
    }
}
