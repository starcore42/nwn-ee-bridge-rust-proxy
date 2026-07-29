//! Immutable server reliable-data source-window ownership.
//!
//! Diamond and EE use separate 16-slot receive and send windows. Proxy2
//! terminates both reliable lanes, so its HG-facing receive slots advance as
//! soon as a source passes final strict dispatch while its EE-facing send
//! window independently retains translated bytes until the EE client ACKs
//! them. This module keeps a generation-aware source-output ownership queue
//! for exact source retransmission classification and later EE ACK mapping;
//! the queue is not a receive window and cannot restrict new HG source
//! admission.

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

#[derive(Debug, Clone)]
struct RetiredServerReliableSlot {
    key: ServerReliableSlotKey,
    transport_identity: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ServerReliableSlotState {
    /// Active HG-facing receive slots. A slot leaves this decompile-proven
    /// 16-entry window immediately after strict semantic dispatch commits.
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
    /// dispatch. Strict acceptance advances this together with
    /// `receive_start`; downstream output ownership is tracked separately.
    pub(super) dispatch_next_key: Option<ServerReliableSlotKey>,
    /// Exact dispatch identity currently awaiting the proxy's outer strict
    /// validator. Rejection leaves `dispatch_next_key` unchanged so only an
    /// immutable retransmit can retry the same semantic position.
    pub(super) pending_dispatch_key: Option<ServerReliableSlotKey>,
    /// Strict-committed source identities whose translated EE-facing output
    /// has not yet been cumulatively acknowledged. The independent
    /// `EeServerSendWindowState` owns exact output bytes, retransmission
    /// timing, and its own 16-slot limit. These full sources classify exact HG
    /// retransmissions so whole-window cache replays remain possible while a
    /// lone non-leading stream member can be acknowledged without repeating
    /// CNW dispatch. Their source-coordinate order also supports mapped ACK
    /// retirement and ordered-epoch compaction.
    pub(super) output_sources: VecDeque<ServerReliableSlot>,
    /// Exact first source identity not yet retired by a mapped, strict-accepted
    /// EE ACK. Keep the next identity after the queue empties so sequence-wrap
    /// generation cannot be reconstructed from a bare `u16`.
    output_retirement_floor: Option<ServerReliableSlotKey>,
    /// The originals free cumulatively acknowledged receive slots and never
    /// dispatch them again. Retain only their immutable identities so proxy2
    /// can classify an exact delayed HG retransmit without reopening gameplay
    /// effects. The side ledger is bounded to the same 16-slot interval.
    retired_history: VecDeque<RetiredServerReliableSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PreparedServerReliableSource {
    Excluded,
    Pinned(ServerReliableSlotKey),
    Matched(ServerReliableSlotKey),
    Conflict(ServerReliableSlotKey),
    OutputReplay(ServerReliableSlotKey),
    OutputConflict(ServerReliableSlotKey),
    RetiredReplay(ServerReliableSlotKey),
    RetiredConflict(ServerReliableSlotKey),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerOutsideWindowRelation {
    Ahead,
    Behind,
    AmbiguousHalfRange,
}

impl ServerOutsideWindowRelation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Ahead => "ahead",
            Self::Behind => "behind",
            Self::AmbiguousHalfRange => "ambiguous-half-range",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerOutsideWindowDispatchRelation {
    AtFrontier,
    AheadOfFrontier,
    BehindFrontier,
    Uninitialized,
}

impl ServerOutsideWindowDispatchRelation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::AtFrontier => "at-frontier",
            Self::AheadOfFrontier => "ahead-of-frontier",
            Self::BehindFrontier => "behind-frontier",
            Self::Uninitialized => "uninitialized",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ServerOutsideWindowDiagnostic {
    pub(super) relation: ServerOutsideWindowRelation,
    pub(super) distance: u16,
    pub(super) dispatch_relation: ServerOutsideWindowDispatchRelation,
    pub(super) dispatch_next_key: Option<ServerReliableSlotKey>,
    /// Exact sequence distance from the HG admission floor to the next
    /// semantic dispatch identity. This stays zero after a committed
    /// contiguous drain; a positive value exposes receive/dispatch coupling.
    pub(super) receive_to_dispatch_distance: Option<u64>,
    pub(super) retained_slots: usize,
    pub(super) retained_output_sources: usize,
    /// Exact signature observed in live HG traffic when proxy-generated ACKs
    /// let the upstream sender advance but the source admission floor remains
    /// coupled to a later EE-facing ACK. This is diagnostic evidence only; it
    /// does not widen the decompile-proven 16-slot receive interval.
    pub(super) at_dispatch_frontier_after_full_retention: bool,
}

impl PreparedServerReliableSource {
    pub(super) fn key(self) -> Option<ServerReliableSlotKey> {
        match self {
            Self::Excluded => None,
            Self::Pinned(key)
            | Self::Matched(key)
            | Self::Conflict(key)
            | Self::OutputReplay(key)
            | Self::OutputConflict(key)
            | Self::RetiredReplay(key)
            | Self::RetiredConflict(key)
            | Self::OutsideWindow(key) => Some(key),
        }
    }
}

/// Classify a rejected source against both original receive-loop frontiers.
///
/// Diamond `sub_5F3940` lines 751485-751549 and EE `FrameReceive` lines
/// 878891-878952 perform the circular 16-slot admission test. Their contiguous
/// dispatch loops then advance the receive start/end together (Diamond
/// 751605-751673; EE 879029-879088). Proxy2 deliberately tracks semantic
/// dispatch separately from downstream ACK retention, so report both
/// coordinates whenever they diverge. The result is production diagnostics
/// only and changes no frame, ACK, cursor, or gameplay payload.
pub(super) fn outside_window_diagnostic(
    state: &ServerReliableSlotState,
    key: ServerReliableSlotKey,
) -> ServerOutsideWindowDiagnostic {
    let receive_start = state.receive_start.unwrap_or(key.sequence);
    let forward_distance = key.sequence.wrapping_sub(receive_start);
    let (relation, distance) = if forward_distance == 0x8000 {
        (
            ServerOutsideWindowRelation::AmbiguousHalfRange,
            forward_distance,
        )
    } else if forward_distance < 0x8000 {
        (ServerOutsideWindowRelation::Ahead, forward_distance)
    } else {
        (
            ServerOutsideWindowRelation::Behind,
            receive_start.wrapping_sub(key.sequence),
        )
    };
    let dispatch_relation = match state.dispatch_next_key {
        None => ServerOutsideWindowDispatchRelation::Uninitialized,
        Some(expected) if key == expected => ServerOutsideWindowDispatchRelation::AtFrontier,
        Some(expected) if key > expected => ServerOutsideWindowDispatchRelation::AheadOfFrontier,
        Some(_) => ServerOutsideWindowDispatchRelation::BehindFrontier,
    };
    let receive_floor = receive_floor(state);
    let receive_to_dispatch_distance = receive_floor
        .zip(state.dispatch_next_key)
        .and_then(|(receive, dispatch)| exact_forward_distance(receive, dispatch));
    let retained_slots = state.slots.len();
    let retained_output_sources = state.output_sources.len();
    let at_dispatch_frontier_after_full_retention = dispatch_relation
        == ServerOutsideWindowDispatchRelation::AtFrontier
        && receive_to_dispatch_distance
            .is_some_and(|distance| distance >= MAX_SERVER_RELIABLE_SLOTS as u64)
        && retained_slots >= MAX_SERVER_RELIABLE_SLOTS;

    ServerOutsideWindowDiagnostic {
        relation,
        distance,
        dispatch_relation,
        dispatch_next_key: state.dispatch_next_key,
        receive_to_dispatch_distance,
        retained_slots,
        retained_output_sources,
        at_dispatch_frontier_after_full_retention,
    }
}

fn exact_forward_distance(
    first: ServerReliableSlotKey,
    last: ServerReliableSlotKey,
) -> Option<u64> {
    if last < first {
        return None;
    }
    let generation_delta = last
        .origin_generation
        .checked_sub(first.origin_generation)?;
    generation_delta
        .checked_mul(u64::from(u16::MAX) + 1)?
        .checked_add(u64::from(last.sequence))?
        .checked_sub(u64::from(first.sequence))
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
/// distinguishes an exact match from a conflict while the source is active;
/// after dispatch, the bounded retired history classifies delayed HG replays
/// without reopening gameplay effects or coupling admission to the EE ACK.
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
        if let Some(existing) = state.output_sources.iter_mut().find(|slot| slot.key == key) {
            if existing.transport_identity != transport_identity {
                return Ok(PreparedServerReliableSource::OutputConflict(key));
            }
            existing.packet = packet.to_vec();
            return Ok(PreparedServerReliableSource::OutputReplay(key));
        }
        if let Some(retired) = state
            .retired_history
            .iter()
            .rev()
            .find(|retired| retired.key == key)
        {
            return Ok(if retired.transport_identity == transport_identity {
                PreparedServerReliableSource::RetiredReplay(key)
            } else {
                PreparedServerReliableSource::RetiredConflict(key)
            });
        }
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
        PreparedServerReliableSource::OutputReplay(key) => {
            return Ok(ServerReliableDispatchAdmission::Replay(key));
        }
        PreparedServerReliableSource::Conflict(key)
        | PreparedServerReliableSource::OutputConflict(key)
        | PreparedServerReliableSource::RetiredReplay(key)
        | PreparedServerReliableSource::RetiredConflict(key)
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

    let Some(slot_index) = state.slots.iter().position(|slot| slot.key == pending) else {
        tracing::error!(
            sequence = pending.sequence,
            origin_generation = pending.origin_generation,
            "server reliable semantic dispatch lost its active receive slot before commit"
        );
        return None;
    };
    let Some(next) = successor_key(pending) else {
        tracing::error!(
            sequence = pending.sequence,
            origin_generation = pending.origin_generation,
            "server reliable semantic dispatch stopped at generation overflow"
        );
        return None;
    };
    if let Some(expected_output) = state
        .output_sources
        .back()
        .map(|slot| slot.key)
        .and_then(successor_key)
        .or(state.output_retirement_floor)
        && expected_output != pending
    {
        tracing::error!(
            sequence = pending.sequence,
            origin_generation = pending.origin_generation,
            expected_output_sequence = expected_output.sequence,
            expected_output_origin_generation = expected_output.origin_generation,
            "server reliable semantic dispatch would break source-output ownership order"
        );
        return None;
    }

    let slot = state
        .slots
        .remove(slot_index)
        .expect("located active server receive slot");
    if state.output_retirement_floor.is_none() {
        state.output_retirement_floor = Some(pending);
    }
    state.output_sources.push_back(slot);
    state.receive_start = Some(next.sequence);
    state.origin_generation = next.origin_generation;
    state.dispatch_next_key = Some(next);
    tracing::trace!(
        sequence = pending.sequence,
        origin_generation = pending.origin_generation,
        next_sequence = next.sequence,
        next_origin_generation = next.origin_generation,
        active_receive_slots = state.slots.len(),
        retained_output_sources = state.output_sources.len(),
        "strict-accepted server reliable semantic dispatch released the HG receive slot and advanced its contiguous frontier"
    );
    Some(pending)
}

/// Retire server source-output ownership only after the EE client ACK carrying
/// this source-facing sequence has passed the outer strict validator.
///
/// The exact translated bytes are owned by the separate EE send window. This
/// queue preserves the original source identity needed by exact HG
/// retransmission classification, expanded-output ACK mapping, and
/// ordered-epoch compaction. Proxy-owned server sequence insertions are
/// removed by the caller before this boundary, so `ack_sequence` is in the
/// source lane.
pub(super) fn retire_through_client_ack(
    state: &mut ServerReliableSlotState,
    ack_sequence: u16,
) -> Vec<ServerReliableSlotKey> {
    let retired = retirable_prefix_len(state, ack_sequence);
    if retired == 0 {
        return Vec::new();
    }
    let Some(output_floor) = state.output_retirement_floor else {
        return Vec::new();
    };
    let retired_slots = state.output_sources.drain(..retired).collect::<Vec<_>>();
    let retired_sources = retired_slots
        .iter()
        .map(|slot| slot.key)
        .collect::<Vec<_>>();
    for slot in retired_slots {
        state.retired_history.push_back(RetiredServerReliableSlot {
            key: slot.key,
            transport_identity: slot.transport_identity,
        });
        while state.retired_history.len() > MAX_SERVER_RELIABLE_SLOTS {
            let _ = state.retired_history.pop_front();
        }
    }
    let next = advance_key(output_floor, retired as u64)
        .expect("retirable output prefix cannot overflow its exact generation");
    state.output_retirement_floor = Some(next);
    tracing::trace!(
        ack_sequence,
        output_floor_sequence = output_floor.sequence,
        output_floor_origin_generation = output_floor.origin_generation,
        next_output_floor_sequence = next.sequence,
        next_output_floor_origin_generation = next.origin_generation,
        retired_slots = retired,
        retained_output_sources = state.output_sources.len(),
        "strict-accepted EE ACK retired source-output ownership independently of the HG receive window"
    );
    retired_sources
}

/// Exact HG-facing receive/admission frontier. This advances on strict local
/// dispatch and does not wait for the EE-facing output ACK.
pub(super) fn receive_floor(state: &ServerReliableSlotState) -> Option<ServerReliableSlotKey> {
    state.receive_start.map(|sequence| ServerReliableSlotKey {
        sequence,
        origin_generation: state.origin_generation,
    })
}

/// Exact first source identity whose EE-facing output remains unacknowledged.
pub(super) fn output_retirement_floor(
    state: &ServerReliableSlotState,
) -> Option<ServerReliableSlotKey> {
    state.output_retirement_floor
}

/// Return the exact contiguous source-output prefix an ACK would retire
/// without mutating either protocol window.
pub(super) fn retirable_prefix_len(state: &ServerReliableSlotState, ack_sequence: u16) -> usize {
    let Some(output_floor) = state.output_retirement_floor else {
        return 0;
    };
    let distance = ack_sequence.wrapping_sub(output_floor.sequence) as usize;
    if distance >= 0x8000 {
        return 0;
    }
    let retired = distance.saturating_add(1);
    if retired > state.output_sources.len()
        || !state
            .output_sources
            .iter()
            .take(retired)
            .enumerate()
            .all(|(offset, slot)| {
                advance_key(output_floor, offset as u64).as_ref() == Some(&slot.key)
            })
    {
        return 0;
    }
    retired
}

fn successor_key(key: ServerReliableSlotKey) -> Option<ServerReliableSlotKey> {
    advance_key(key, 1)
}

fn advance_key(key: ServerReliableSlotKey, distance: u64) -> Option<ServerReliableSlotKey> {
    let total = u64::from(key.sequence).checked_add(distance)?;
    let generation_delta = total / (u64::from(u16::MAX) + 1);
    Some(ServerReliableSlotKey {
        sequence: total as u16,
        origin_generation: key.origin_generation.checked_add(generation_delta)?,
    })
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
