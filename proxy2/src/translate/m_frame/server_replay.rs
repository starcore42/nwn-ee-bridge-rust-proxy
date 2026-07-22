//! Immutable server reliable-data source-window ownership.
//!
//! Diamond and EE use a 16-slot circular type-0 receive window. Proxy2 is a
//! translator in that end-to-end reliable lane, so it retains each immutable
//! source identity until an EE client ACK has itself passed strict validation.
//! That lets an exact server retransmit reproduce the same translation if the
//! earlier proxy-to-client UDP datagram was lost, while stale or conflicting
//! traffic can never replace a live slot.

use std::collections::VecDeque;

use crate::packet::m::{MFrameType, MFrameView};

use super::{sequence::record_forward_progress, transport_identity};

/// Diamond initializes the receive start/end to 0/16 and its slot modulus to
/// 16 at lines 750687-750694 and 750769-750775. EE does the same at lines
/// 891083-891086 and 891172-891173.
pub(super) const MAX_SERVER_RELIABLE_SLOTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ServerReliableSlotKey {
    pub(super) sequence: u16,
    pub(super) origin_generation: u64,
}

#[derive(Debug, Clone)]
pub(super) struct ServerReliableSlot {
    pub(super) key: ServerReliableSlotKey,
    /// Exact bytes from flags onward with only the decompile-proven
    /// FrameSend-owned bit 6 canonicalized away. ACK and CRC are outside this
    /// identity; packetized metadata, low flags, payload, and trailing storage
    /// remain immutable.
    pub(super) transport_identity: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ServerReliableSlotState {
    pub(super) slots: VecDeque<ServerReliableSlot>,
    /// First sequence in the circular half-open receive interval
    /// `[receive_start, receive_start + 16)`. The proxy anchors this on the
    /// first validated type-0 source so isolated replay segments remain usable.
    pub(super) receive_start: Option<u16>,
    /// Generation owning `receive_start`; a slot after an in-window wrap owns
    /// the following generation.
    pub(super) origin_generation: u64,
    /// Latest validated ACK carried by the server source lane. This is peer
    /// transport truth, so it survives rollback of an older speculative
    /// server-to-client reader transaction.
    pub(super) latest_peer_ack_sequence: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PreparedServerReliableSource {
    Excluded,
    Pinned(ServerReliableSlotKey),
    Matched(ServerReliableSlotKey),
    Conflict(ServerReliableSlotKey),
    OutsideWindow(ServerReliableSlotKey),
}

impl PreparedServerReliableSource {
    pub(super) fn key(self) -> Option<ServerReliableSlotKey> {
        match self {
            Self::Excluded => None,
            Self::Pinned(key)
            | Self::Matched(key)
            | Self::Conflict(key)
            | Self::OutsideWindow(key) => Some(key),
        }
    }
}

pub(super) fn observe_peer_ack_sequence(
    state: &mut ServerReliableSlotState,
    ack_sequence: u16,
) -> u16 {
    record_forward_progress(&mut state.latest_peer_ack_sequence, ack_sequence);
    state.latest_peer_ack_sequence.unwrap_or(ack_sequence)
}

/// Pin or match a validated server source before semantic translation.
///
/// Diamond lines 751482-751549 and EE lines 878891-878952 admit type 0 only
/// inside a circular 16-slot half-open interval and never replace an occupied
/// slot. The originals ignore every occupied duplicate. Proxy2 additionally
/// distinguishes an exact match from a conflict because, as a transparent
/// translator rather than the reliable endpoint, it must replay the first
/// translation until EE's strict-accepted ACK retires that source slot.
pub(super) fn prepare_source_slot(
    state: &mut ServerReliableSlotState,
    packet: &[u8],
    view: &MFrameView,
) -> anyhow::Result<PreparedServerReliableSource> {
    if view.frame_kind() != Some(MFrameType::ReliableData) {
        return Ok(PreparedServerReliableSource::Excluded);
    }

    let transport_identity =
        transport_identity::server_reliable_data_transport_identity(packet, view)
            .ok_or_else(|| anyhow::anyhow!("server reliable source identity left type-0 lane"))?;
    let receive_start = *state.receive_start.get_or_insert(view.sequence);
    let distance = view.sequence.wrapping_sub(receive_start) as usize;
    let key = ServerReliableSlotKey {
        sequence: view.sequence,
        origin_generation: generation_for_sequence(state, receive_start, view.sequence, distance),
    };

    if distance >= MAX_SERVER_RELIABLE_SLOTS {
        return Ok(PreparedServerReliableSource::OutsideWindow(key));
    }
    if let Some(existing) = state.slots.iter().find(|slot| slot.key == key) {
        if existing.transport_identity != transport_identity {
            return Ok(PreparedServerReliableSource::Conflict(key));
        }
        return Ok(PreparedServerReliableSource::Matched(key));
    }

    state.slots.push_back(ServerReliableSlot {
        key,
        transport_identity,
    });
    debug_assert!(state.slots.len() <= MAX_SERVER_RELIABLE_SLOTS);
    tracing::trace!(
        sequence = key.sequence,
        origin_generation = key.origin_generation,
        receive_start,
        retained_slots = state.slots.len(),
        "server reliable-data slot pinned inside the 16-frame receive window"
    );
    Ok(PreparedServerReliableSource::Pinned(key))
}

/// Retire server source slots only after the EE client ACK carrying this
/// source-facing sequence has passed the outer strict validator.
///
/// The original common ACK retirement is Diamond lines 751677-751724 and EE
/// lines 879090-879135. Proxy-owned server sequence insertions are removed by
/// the caller before this boundary, so `ack_sequence` is in the source lane.
pub(super) fn retire_through_client_ack(
    state: &mut ServerReliableSlotState,
    ack_sequence: u16,
) -> usize {
    let retired = retirable_prefix_len(state, ack_sequence);
    if retired == 0 {
        return 0;
    }
    let Some(receive_start) = state.receive_start else {
        return 0;
    };
    let distance = ack_sequence.wrapping_sub(receive_start) as usize;

    let before = state.slots.len();
    state
        .slots
        .retain(|slot| slot.key.sequence.wrapping_sub(receive_start) as usize > distance);
    let retired = before.saturating_sub(state.slots.len());
    let next = ack_sequence.wrapping_add(1);
    if next < receive_start {
        state.origin_generation = state.origin_generation.saturating_add(1);
    }
    state.receive_start = Some(next);
    tracing::trace!(
        ack_sequence,
        receive_start,
        next_receive_start = next,
        origin_generation = state.origin_generation,
        retired_slots = retired,
        retained_slots = state.slots.len(),
        "strict-accepted EE ACK advanced the mirrored server receive window"
    );
    retired
}

/// Return the exact contiguous active prefix an ACK would retire without
/// mutating the mirrored server-source window.
pub(super) fn retirable_prefix_len(state: &ServerReliableSlotState, ack_sequence: u16) -> usize {
    let Some(receive_start) = state.receive_start else {
        return 0;
    };
    let distance = ack_sequence.wrapping_sub(receive_start) as usize;
    if distance >= MAX_SERVER_RELIABLE_SLOTS {
        return 0;
    }
    // Diamond/EE bound cumulative ACK cleanup by the active send interval,
    // not by unused capacity in the 16-slot allocation. Never advance over a
    // source sequence the proxy has not actually pinned.
    if !(0..=distance).all(|offset| {
        let sequence = receive_start.wrapping_add(offset as u16);
        let generation = generation_for_sequence(state, receive_start, sequence, offset);
        state
            .slots
            .iter()
            .any(|slot| slot.key.sequence == sequence && slot.key.origin_generation == generation)
    }) {
        return 0;
    }
    distance.saturating_add(1)
}

fn generation_for_sequence(
    state: &ServerReliableSlotState,
    receive_start: u16,
    sequence: u16,
    forward_distance: usize,
) -> u64 {
    if forward_distance < 0x8000 && sequence < receive_start {
        state.origin_generation.saturating_add(1)
    } else if forward_distance >= 0x8000 && sequence > receive_start {
        state.origin_generation.saturating_sub(1)
    } else {
        state.origin_generation
    }
}
