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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ServerReliableSlotKey {
    pub(super) origin_generation: u64,
    pub(super) sequence: u16,
}

#[derive(Debug, Clone)]
pub(super) struct ServerReliableSlot {
    pub(super) key: ServerReliableSlotKey,
    /// Latest exact datagram for this immutable reliable identity. ACK, CRC,
    /// and FrameSend-owned bit 6 may refresh on retransmission, so the
    /// canonical identity below remains the conflict authority. Retaining the
    /// complete frame mirrors the original receive window and leaves a future
    /// contiguous-drain path available without semantic predecode.
    pub(super) packet: Vec<u8>,
    /// Exact bytes from flags onward with only the decompile-proven
    /// FrameSend-owned bit 6 canonicalized away. ACK and CRC are outside this
    /// identity; packetized metadata, low flags, payload, and trailing storage
    /// remain immutable.
    pub(super) transport_identity: Vec<u8>,
    /// The original receive loop stored this source behind a missing
    /// predecessor. Once that predecessor commits, the network loop may
    /// dispatch this exact raw datagram once without waiting for another UDP
    /// retransmit. Any attempted dispatch clears the flag; strict rejection
    /// therefore remains fail-closed until a real exact retransmit arrives.
    pub(super) deferred_behind_gap: bool,
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
    /// First reliable source identity not yet admitted to CNW/gameplay
    /// dispatch. This is deliberately separate from `receive_start`: source
    /// slots remain retained there until a strict-accepted EE ACK, while the
    /// original receive window can dispatch later contiguous slots before that
    /// downstream ACK arrives.
    pub(super) dispatch_next_key: Option<ServerReliableSlotKey>,
    /// Exact dispatch identity currently awaiting the proxy's outer strict
    /// validator. Rejection leaves `dispatch_next_key` unchanged so only an
    /// immutable retransmit can retry the same semantic position.
    pub(super) pending_dispatch_key: Option<ServerReliableSlotKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PreparedServerReliableSource {
    Excluded,
    Pinned(ServerReliableSlotKey),
    Matched(ServerReliableSlotKey),
    Conflict(ServerReliableSlotKey),
    OutsideWindow(ServerReliableSlotKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerReliableDispatchAdmission {
    Excluded,
    Ready(ServerReliableSlotKey),
    Replay(ServerReliableSlotKey),
    Pending(ServerReliableSlotKey),
    Future {
        key: ServerReliableSlotKey,
        expected: ServerReliableSlotKey,
    },
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
    if let Some(existing) = state.slots.iter_mut().find(|slot| slot.key == key) {
        if existing.transport_identity != transport_identity {
            return Ok(PreparedServerReliableSource::Conflict(key));
        }
        existing.packet = packet.to_vec();
        return Ok(PreparedServerReliableSource::Matched(key));
    }

    state.slots.push_back(ServerReliableSlot {
        key,
        packet: packet.to_vec(),
        transport_identity,
        deferred_behind_gap: false,
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

/// Admit one pinned type-0 source to semantic dispatch in reliable order.
///
/// Diamond `sub_5F3940` lines 751482-751549 store any free type-0 slot inside
/// the 16-frame receive interval, then lines 751571-751673 dispatch only the
/// occupied receive-frontier slot and loop across a contiguous occupied
/// prefix. EE `CNetLayerWindow::FrameReceive` does the same at lines
/// 878891-878952 and 879029-879088. Therefore a future in-window datagram is
/// transport truth, but it cannot touch packetized reassembly, a persistent
/// inflater, or gameplay state before every predecessor commits.
pub(super) fn prepare_source_dispatch(
    state: &mut ServerReliableSlotState,
    prepared: PreparedServerReliableSource,
) -> anyhow::Result<ServerReliableDispatchAdmission> {
    let key = match prepared {
        PreparedServerReliableSource::Excluded => {
            return Ok(ServerReliableDispatchAdmission::Excluded);
        }
        PreparedServerReliableSource::Pinned(key) | PreparedServerReliableSource::Matched(key) => {
            key
        }
        PreparedServerReliableSource::Conflict(key)
        | PreparedServerReliableSource::OutsideWindow(key) => {
            anyhow::bail!(
                "server reliable source {} generation {} reached dispatch admission after transport rejection",
                key.sequence,
                key.origin_generation
            );
        }
    };

    let expected = *state.dispatch_next_key.get_or_insert(key);
    if key > expected {
        let Some(slot) = state.slots.iter_mut().find(|slot| slot.key == key) else {
            anyhow::bail!(
                "future server reliable dispatch {} generation {} lost its retained raw slot",
                key.sequence,
                key.origin_generation
            );
        };
        slot.deferred_behind_gap = true;
        return Ok(ServerReliableDispatchAdmission::Future { key, expected });
    }
    if key < expected {
        return Ok(ServerReliableDispatchAdmission::Replay(key));
    }
    if let Some(pending) = state.pending_dispatch_key {
        if pending == key {
            return Ok(ServerReliableDispatchAdmission::Pending(key));
        }
        anyhow::bail!(
            "server reliable dispatch {} generation {} arrived while {} generation {} still awaits final validation",
            key.sequence,
            key.origin_generation,
            pending.sequence,
            pending.origin_generation
        );
    }
    Ok(ServerReliableDispatchAdmission::Ready(key))
}

pub(super) fn stage_source_dispatch(
    state: &mut ServerReliableSlotState,
    key: ServerReliableSlotKey,
) -> anyhow::Result<()> {
    if state.dispatch_next_key != Some(key) {
        anyhow::bail!(
            "server reliable dispatch {} generation {} no longer matches frontier {:?}",
            key.sequence,
            key.origin_generation,
            state.dispatch_next_key
        );
    }
    if let Some(pending) = state.pending_dispatch_key {
        anyhow::bail!(
            "server reliable dispatch {} generation {} cannot stage while {} generation {} awaits final validation",
            key.sequence,
            key.origin_generation,
            pending.sequence,
            pending.origin_generation
        );
    }
    let Some(slot) = state.slots.iter_mut().find(|slot| slot.key == key) else {
        anyhow::bail!(
            "server reliable dispatch {} generation {} lost its retained raw slot before staging",
            key.sequence,
            key.origin_generation
        );
    };
    // A network retransmit can reach the frontier before the loop observes
    // the deferred slot. Whichever path dispatches first owns the one attempt;
    // rejection must wait for another exact retransmit rather than busy-loop.
    slot.deferred_behind_gap = false;
    state.pending_dispatch_key = Some(key);
    Ok(())
}

/// Take one exact raw source that became contiguous after its predecessor
/// committed.
///
/// Diamond `sub_5F3940` lines 751571-751673 and EE
/// `CNetLayerWindow::FrameReceive` lines 879029-879088 immediately walk the
/// occupied prefix after the receive-frontier slot is released. Proxy2 keeps
/// final validation asynchronous, so the outer network loop takes at most one
/// retained successor per pass and feeds it through the ordinary translator.
/// Clearing the one-shot marker before that handoff prevents validator
/// rejection from turning into an internal retry loop.
pub(super) fn take_deferred_frontier_packet(
    state: &mut ServerReliableSlotState,
) -> Option<(ServerReliableSlotKey, Vec<u8>)> {
    if state.pending_dispatch_key.is_some() {
        return None;
    }
    let expected = state.dispatch_next_key?;
    let slot = state
        .slots
        .iter_mut()
        .find(|slot| slot.key == expected && slot.deferred_behind_gap)?;
    slot.deferred_behind_gap = false;
    Some((slot.key, slot.packet.clone()))
}

/// Commit or reject the one semantic-dispatch identity staged above.
pub(super) fn finish_source_dispatch(
    state: &mut ServerReliableSlotState,
    accepted: bool,
) -> Option<ServerReliableSlotKey> {
    let pending = state.pending_dispatch_key.take()?;
    if !accepted {
        tracing::trace!(
            sequence = pending.sequence,
            origin_generation = pending.origin_generation,
            "server reliable semantic dispatch retained at the receive frontier after rejection"
        );
        return None;
    }
    if state.dispatch_next_key != Some(pending) {
        tracing::warn!(
            sequence = pending.sequence,
            origin_generation = pending.origin_generation,
            expected_sequence = state.dispatch_next_key.map(|key| key.sequence),
            expected_origin_generation = state.dispatch_next_key.map(|key| key.origin_generation),
            "server reliable semantic dispatch commit ignored because its frontier identity changed"
        );
        return None;
    }

    let next_origin_generation = if pending.sequence == u16::MAX {
        let Some(next_generation) = pending.origin_generation.checked_add(1) else {
            tracing::error!(
                sequence = pending.sequence,
                origin_generation = pending.origin_generation,
                "server reliable semantic dispatch stopped at generation overflow"
            );
            return None;
        };
        next_generation
    } else {
        pending.origin_generation
    };
    let next = ServerReliableSlotKey {
        sequence: pending.sequence.wrapping_add(1),
        origin_generation: next_origin_generation,
    };
    state.dispatch_next_key = Some(next);
    tracing::trace!(
        sequence = pending.sequence,
        origin_generation = pending.origin_generation,
        next_sequence = next.sequence,
        next_origin_generation = next.origin_generation,
        "strict-accepted server reliable semantic dispatch advanced the contiguous receive frontier"
    );
    Some(pending)
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
) -> Vec<ServerReliableSlotKey> {
    let retired = retirable_prefix_len(state, ack_sequence);
    if retired == 0 {
        return Vec::new();
    }
    let Some(receive_start) = state.receive_start else {
        return Vec::new();
    };
    let distance = ack_sequence.wrapping_sub(receive_start) as usize;

    let retired_sources = state
        .slots
        .iter()
        .filter(|slot| slot.key.sequence.wrapping_sub(receive_start) as usize <= distance)
        .map(|slot| slot.key)
        .collect::<Vec<_>>();
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
    retired_sources
}

/// Exact first source identity not yet cumulatively retired. The receive
/// window owns this generation; callers must not reconstruct it from the bare
/// wrapped sequence when coordinating a second destination-facing window.
pub(super) fn receive_floor(state: &ServerReliableSlotState) -> Option<ServerReliableSlotKey> {
    state.receive_start.map(|sequence| ServerReliableSlotKey {
        sequence,
        origin_generation: state.origin_generation,
    })
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
