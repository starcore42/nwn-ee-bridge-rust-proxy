//! Reliable gameplay `M...` frame translation.
//!
//! This module owns the transport mechanics for high-level CNW gameplay
//! payloads after BN authentication:
//!
//! - reliable-window frame buffering,
//! - Diamond/EE deflated gameplay envelope handling,
//! - packetized length repair,
//! - legacy M CRC repair.
//!
//! It deliberately delegates message-specific semantics to focused siblings
//! such as `translate::module`. That prevents the reliable-window code from
//! becoming another monolith.

use std::{
    fs,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    crc::{encode_legacy_m_crc, read_le_u32, write_be_u16},
    packet::m::{HighLevel, MFrameType, MFrameView},
    translate::{
        ContinuationOwner, Emit, VerifiedFamily, VerifiedPacket, VerifiedProof, area,
        client_gui_inventory, semantic,
    },
};

mod ack_carrier;
mod ack_delivery;
mod client_ack;
mod client_filters;
mod client_replay;
mod coalesced;
mod deferred_module_resources;
mod deflate;
mod inventory_equipment;
mod live_stream;
mod live_update;
mod local_ack;
mod login_waypoint;
mod parse_window;
mod quickbar_materialization;
mod quickbar_stream;
mod reassembly;
mod sequence;
mod server_dispatch;
mod server_replay;
mod state;
mod stream_continuation;
mod synthetic_area;
mod transport_identity;
mod zlib_zero_fill;

use deflate::{deflate_zlib, looks_like_zlib_wrapped_deflate};
use reassembly::{
    CompletedDeflatedReplay, CompletedDeflatedStreamWindowMatch,
    CompletedServerReliableStreamRoute, CompletedServerReliableStreamSlotMatch,
    InflatedGameplayPayload, ServerDeflatedReassembly,
};
use sequence::{
    SequenceElision, SequenceShift, record_forward_progress, sequence_at_or_after,
    shift_sequence_for_peer, shift_sequence_for_peer_with_elisions, trim_sequence_elisions,
    trim_sequence_shifts, unshift_ack_for_origin, unshift_ack_for_origin_with_elisions,
};

const MAX_REASSEMBLY_FRAMES: usize = 256;
const MAX_INTERLEAVED_PACKETS: usize = 32;
const MAX_COMPLETED_DIRECT_SERVER_SEMANTIC_REWRITES: usize = 64;
const CNW_LENGTH_BYTES: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ServerSemanticFrameContext {
    pub(super) sequence: u16,
    pub(super) server_peer_ack_sequence: u16,
    pub(super) client_unshifted_ack_sequence: u16,
    pub(super) live_object_inventory_materialization:
        Option<semantic::LiveObjectInventoryMaterializationSummary>,
}

pub use state::SessionState;

#[cfg(test)]
pub(crate) fn rewrite_live_object_payload_to_exact_ee_for_test(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> bool {
    live_update::rewrite_payload_to_exact_ee_if_possible(payload, latest_area_placeables).is_some()
}

pub fn take_pending_client_to_server_packets(state: &mut SessionState) -> anyhow::Result<Emit> {
    if state.sequence.pending_client_to_server_packets.is_empty()
        && state.synthetic_area.pending_area_loaded.is_none()
    {
        return Ok(Emit::Consumed);
    }
    begin_pending_client_drain_effect_transaction(state)?;
    if let Err(err) =
        maybe_queue_area_loaded_fallback_from_timer(state, "session pending client drain")
    {
        rollback_pending_client_drain_effect_transaction(state);
        return Err(err);
    }

    let pending = std::mem::take(&mut state.sequence.pending_client_to_server_packets);
    if pending.is_empty() {
        // Timer-only state (ACK grace or native-probe gate progress) produced no
        // wire candidate and therefore needs no outer emit decision.
        commit_pending_client_drain_effect_transaction(state);
        return Ok(Emit::Consumed);
    }
    tracing::debug!(
        packets = pending.len(),
        reasons = ?pending.iter().map(|packet| packet.reason).collect::<Vec<_>>(),
        "pending client batch staged for exact-family strict validation"
    );
    Ok(Emit::MixedVerifiedPackets(
        pending
            .into_iter()
            .map(|pending| (pending.family, pending.packet))
            .collect(),
    ))
}

pub fn take_pending_server_to_client_packets(state: &mut SessionState) -> anyhow::Result<Emit> {
    ensure_pending_server_drain_can_start(state)?;
    let now = Instant::now();
    if !pending_server_drain_has_work(state, now) {
        return Ok(Emit::Consumed);
    }
    begin_pending_server_drain_effect_transaction(state)?;
    let mut proof_packets = deferred_module_resources::take_releasable_held_server_packets(
        &mut state.deferred_module_resources.pending,
    );
    proof_packets.extend(
        take_due_pending_server_packets(
            state,
            now,
            "pending server-to-client proxy-owned packet released from session drain",
            true,
            PendingServerGatePolicy::CurrentState,
        )
        .into_iter()
        .map(|(_, pending)| (VerifiedProof::family(pending.family), pending.packet)),
    );

    if state.synthetic_area.server_hold_gate.is_some() {
        let mut area_filtered = Vec::new();
        for (proof, packet) in proof_packets {
            area_filtered.extend(hold_or_release_server_packets(
                state,
                proof,
                vec![packet],
                "module-resource released packet still gated by area-load completion proof",
            ));
        }
        proof_packets = area_filtered;
    } else if !state
        .synthetic_area
        .held_server_to_client_packets
        .is_empty()
    {
        proof_packets.extend(
            state
                .synthetic_area
                .held_server_to_client_packets
                .drain(..)
                .map(|pending| {
                    tracing::info!(
                        proof = pending.proof.as_str(),
                        reason = pending.reason,
                        len = pending.packet.len(),
                        "server-to-client held packet released after area-load completion gate opened"
                    );
                    (pending.proof, pending.packet)
                }),
        );
    }

    if proof_packets.is_empty() {
        return Ok(Emit::Consumed);
    }

    let emit = Emit::MixedVerifiedProofPacketsPreShifted(proof_packets);
    record_server_emit_window_state(state, &emit);
    Ok(emit)
}

pub(super) fn stage_direct_client_ack_delivery(
    state: &mut SessionState,
    emit: &Emit,
) -> anyhow::Result<()> {
    ack_delivery::stage(
        &mut state.ack_delivery,
        ack_delivery::AckDeliveryOwner::DirectClient,
        emit,
    )
}

pub(super) fn stage_pending_client_ack_delivery(
    state: &mut SessionState,
    emit: &Emit,
) -> anyhow::Result<()> {
    ack_delivery::stage(
        &mut state.ack_delivery,
        ack_delivery::AckDeliveryOwner::PendingClientDrain,
        emit,
    )
}

pub(super) fn stage_direct_server_ack_delivery(
    state: &mut SessionState,
    emit: &Emit,
) -> anyhow::Result<()> {
    ack_delivery::stage(
        &mut state.ack_delivery,
        ack_delivery::AckDeliveryOwner::DirectServer,
        emit,
    )
}

pub(super) fn stage_pending_server_ack_delivery(
    state: &mut SessionState,
    emit: &Emit,
) -> anyhow::Result<()> {
    ack_delivery::stage(
        &mut state.ack_delivery,
        ack_delivery::AckDeliveryOwner::PendingServerDrain,
        emit,
    )
}

fn finish_ack_delivery(
    state: &mut SessionState,
    owner: ack_delivery::AckDeliveryOwner,
    accepted: bool,
) {
    let ack_sequences = ack_delivery::finish(&mut state.ack_delivery, owner, accepted);
    if ack_sequences.is_empty() {
        return;
    }

    let mut retired_slots = 0usize;
    for ack_sequence in &ack_sequences {
        retired_slots = retired_slots.saturating_add(if owner.acknowledges_server_sources() {
            server_replay::retire_through_client_ack(
                &mut state.server_reliable_slots,
                *ack_sequence,
            )
        } else {
            client_replay::retire_through_server_ack(
                &mut state.client_reliable_replays,
                *ack_sequence,
            )
        });
    }
    tracing::trace!(
        owner = owner.as_str(),
        ack_sequences = ?ack_sequences,
        retired_slots,
        "strict-accepted outgoing M batch committed destination-facing ACK delivery"
    );
}

fn pending_server_drain_has_work(state: &SessionState, now: Instant) -> bool {
    client_ack::has_due_consumed_ee_only_ack(&state.client_ack.pending, now)
        || state
            .synthetic_area
            .pending_server_to_client_packets
            .iter()
            .any(|pending| {
                pending.due_at <= now
                    && pending_server_packet_can_reach_outer_validator(state, pending)
            })
        || deferred_module_resources::has_releasable_held_server_packets(
            &state.deferred_module_resources.pending,
        )
        || (state.synthetic_area.server_hold_gate.is_none()
            && !state
                .synthetic_area
                .held_server_to_client_packets
                .is_empty())
}

pub fn translate_client_to_server(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        anyhow::bail!("client M frame failed reliable-window parse");
    };
    validate_source_transport(&view, "client")?;
    let prepared_source =
        client_replay::prepare_source_slot(&mut state.client_reliable_replays, bytes, &view)?;
    let defer_module_loaded_until_released_packets_are_acked = is_client_module_loaded(view.high)
        && deferred_module_resources::client_ack_would_release_held_server_packets(
            &state.deferred_module_resources.pending,
            view.ack_sequence,
        );
    let source_sequence_accepted = !matches!(
        &prepared_source,
        client_replay::PreparedClientReliableSource::Conflict(_)
            | client_replay::PreparedClientReliableSource::OutsideWindow(_)
    );
    observe_client_window_state(state, &view, source_sequence_accepted);
    synthetic_area::observe_server_hold_gate_client_ack(
        &mut state.synthetic_area.server_hold_gate,
        view.ack_sequence,
    );
    deferred_module_resources::observe_resource_hold_gate_client_ack(
        &mut state.deferred_module_resources.pending,
        view.ack_sequence,
    );

    if let client_replay::PreparedClientReliableSource::Conflict(key) = &prepared_source {
        tracing::warn!(
            sequence = key.sequence,
            origin_generation = key.origin_generation,
            ack_sequence = view.ack_sequence,
            "client M frame dropped because its reliable slot already committed different immutable transport bytes"
        );
        return Ok(Emit::Drop);
    }
    if let client_replay::PreparedClientReliableSource::OutsideWindow(key) = &prepared_source {
        tracing::warn!(
            sequence = key.sequence,
            origin_generation = key.origin_generation,
            ack_sequence = view.ack_sequence,
            receive_start = state.client_reliable_replays.receive_start,
            "client reliable datagram rejected outside the decompile-proven 16-frame receive window"
        );
        return Ok(Emit::Drop);
    }

    if defer_module_loaded_until_released_packets_are_acked {
        tracing::info!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            "client Module_Loaded consumed once while releasing module-resource held server packets; waiting for EE retransmit after it ACKs those packets"
        );
        return Ok(Emit::Consumed);
    }

    let mut outbound = bytes.to_vec();
    unshift_client_ack_for_server(state, &mut outbound, &view)?;
    let ack_adjusted_view = MFrameView::parse(&outbound).unwrap_or_else(|| view.clone());
    if let client_replay::PreparedClientReliableSource::Replay { key, replay } =
        prepared_source.clone()
    {
        let replay = client_replay::replay_translation(
            &mut state.client_reliable_replays,
            key,
            replay,
            &outbound,
        )?;
        return if let Some(packet) = replay.packet {
            Ok(Emit::VerifiedPackets {
                family: replay.family,
                packets: vec![packet],
            })
        } else {
            Ok(Emit::Consumed)
        };
    }

    let source_slot_key = prepared_source.key();
    let native_area_loaded = synthetic_area::is_native_area_loaded(view.high);
    // EE/Diamond decompiles identify 0x04/0x03 as the client
    // `Area_AreaLoaded` acknowledgement sent through
    // `CNWCMessage::SendPlayerToServerMessage`. Native wins while the proxy
    // fallback is only pending. Once the server has ACKed a proxy-owned
    // fallback, a matching late native packet is the same semantic area-load
    // acknowledgement and is consumed as an empty reliable frame so the legacy
    // server does not see two area-loaded events for one area transition.
    let duplicate_native_area_loaded_after_synthetic = native_area_loaded
        && (state.synthetic_area.in_flight_area_loaded.is_some()
            || synthetic_area::consume_late_native_area_loaded_after_completed_synthetic(
                &mut state.synthetic_area.completed_area_loaded,
                view.sequence,
                view.ack_sequence,
            ));
    if native_area_loaded {
        synthetic_area::release_pending_loadbar_completion_after_native_area_loaded(
            &mut state.synthetic_area.pending_server_to_client_packets,
        );
        synthetic_area::clear_pending_area_loaded(&mut state.synthetic_area.pending_area_loaded);
        synthetic_area::clear_in_flight_area_loaded(
            &mut state.synthetic_area.in_flight_area_loaded,
        );
        synthetic_area::clear_server_hold_gate(
            &mut state.synthetic_area.server_hold_gate,
            "native Area_AreaLoaded observed",
        );
    }

    shift_client_sequence_for_server(state, &mut outbound, &ack_adjusted_view)?;
    let origin_client_ack_sequence = ack_adjusted_view.ack_sequence;
    let shifted_view = MFrameView::parse(&outbound).unwrap_or(ack_adjusted_view);
    let synthetic_area_loaded = synthetic_area::maybe_build_area_loaded_client_packet(
        &mut state.synthetic_area.pending_area_loaded,
        &mut state.synthetic_area.in_flight_area_loaded,
        &mut state.synthetic_area.server_hold_gate,
        &mut state.sequence.latest_client_sequence_from_client,
        &mut state.sequence.client_sequence_shifts,
        Some(view.ack_sequence),
        origin_client_ack_sequence,
    )?;

    let packet = if duplicate_native_area_loaded_after_synthetic {
        translate_duplicate_native_area_loaded_after_synthetic(state, outbound, &shifted_view)?
    } else {
        translate_client_to_server_packet(state, outbound, &shifted_view, view.sequence)?
    };
    if let Some(synthetic) = synthetic_area_loaded {
        let synthetic_view = MFrameView::parse(&synthetic)
            .ok_or_else(|| anyhow::anyhow!("synthetic Area_AreaLoaded M frame failed to parse"))?;
        let synthetic = translate_client_to_server_packet(
            state,
            synthetic,
            &synthetic_view,
            synthetic_view.sequence,
        )?;
        if let Some(key) = source_slot_key {
            client_replay::stage_translation(
                &mut state.client_reliable_replays,
                key,
                packet.family,
                packet.packet.clone(),
            )?;
        }
        let mut packets = Vec::new();
        if let Some(packet_bytes) = packet.packet {
            packets.push((packet.family, packet_bytes));
        }
        if let Some(synthetic_bytes) = synthetic.packet {
            packets.push((synthetic.family, synthetic_bytes));
        }
        return if packets.is_empty() {
            Ok(Emit::Consumed)
        } else {
            Ok(Emit::MixedVerifiedPackets(packets))
        };
    }

    if let Some(key) = source_slot_key {
        client_replay::stage_translation(
            &mut state.client_reliable_replays,
            key,
            packet.family,
            packet.packet.clone(),
        )?;
    }
    if let Some(packet_bytes) = packet.packet {
        Ok(Emit::VerifiedPackets {
            family: packet.family,
            packets: vec![packet_bytes],
        })
    } else {
        Ok(Emit::Consumed)
    }
}

fn validate_source_transport(view: &MFrameView, source: &'static str) -> anyhow::Result<()> {
    // Diamond `sub_5F4F50` and EE `ProcessReceivedFrames` verify the CRC before
    // calling FrameReceive. The proxy likewise must not publish ACK, sequence,
    // gate, or semantic state from a source the original window would reject.
    if !view.crc_valid {
        anyhow::bail!("{source} M frame CRC mismatch");
    }
    if view.declared_payload_length != 0
        && view.declared_payload_length > view.available_payload_length
    {
        anyhow::bail!("{source} M frame declared payload exceeds datagram");
    }

    let Some(frame_kind) = view.frame_kind() else {
        anyhow::bail!(
            "{source} M frame has unsupported frame type {}",
            view.frame_type
        );
    };
    // Both original FrameSend implementations allocate a zeroed 12-byte frame
    // for type-1/type-2 controls. CNW payload and packetized metadata belong
    // only to type 0, regardless of the reusable sequence field's value.
    if frame_kind != MFrameType::ReliableData && !view.is_exact_control_frame() {
        anyhow::bail!("{source} M control frame has an impossible writer shape");
    }
    Ok(())
}

/// Snapshot the client frame's mapped ACK before semantic translation can add
/// any new proxy-owned server sequence shifts. The returned type-1 carrier is
/// still conditional: it is inserted only if the final outgoing batch does not
/// already carry that exact ACK in the Diamond server's domain. A numerically
/// newer ACK can be sparse and must not suppress this independently valid fact.
pub(super) fn prepare_direct_client_source_ack_carrier(
    state: &SessionState,
    bytes: &[u8],
) -> anyhow::Result<Option<ack_carrier::PreparedSourceAckCarrier>> {
    let view = MFrameView::parse(bytes)
        .ok_or_else(|| anyhow::anyhow!("client M frame failed ACK-carrier parse"))?;
    validate_source_transport(&view, "client")?;
    if view.frame_kind() != Some(MFrameType::ReliableData) {
        return Ok(None);
    }
    let mapped_ack_sequence =
        unshift_ack_for_origin(&state.sequence.server_sequence_shifts, view.ack_sequence);
    ack_carrier::prepare(view.ack_sequence, mapped_ack_sequence).map(Some)
}

/// Snapshot the server frame's mapped ACK before semantic translation can add
/// client sequence shifts or elisions. The mapped value is in the EE client's
/// original source-sequence domain.
pub(super) fn prepare_direct_server_source_ack_carrier(
    state: &SessionState,
    bytes: &[u8],
) -> anyhow::Result<Option<ack_carrier::PreparedSourceAckCarrier>> {
    let view = MFrameView::parse(bytes)
        .ok_or_else(|| anyhow::anyhow!("server M frame failed ACK-carrier parse"))?;
    validate_source_transport(&view, "server")?;
    if view.frame_kind() != Some(MFrameType::ReliableData) {
        return Ok(None);
    }
    let mapped_ack_sequence = unshift_ack_for_origin_with_elisions(
        &state.sequence.client_sequence_shifts,
        &state.sequence.client_sequence_elisions,
        view.ack_sequence,
    );
    ack_carrier::prepare(view.ack_sequence, mapped_ack_sequence).map(Some)
}

pub(super) fn ensure_direct_source_ack_carrier(
    emit: Emit,
    prepared: ack_carrier::PreparedSourceAckCarrier,
    source_lane: &'static str,
) -> Emit {
    ack_carrier::ensure_carried(emit, prepared, source_lane)
}

/// Retain peer-facing ACK truth once the server datagram has passed its CRC,
/// length, and writer-shape boundary. A conflicting reliable payload is still
/// rejected, but its independently valid ACK can release proxy-owned client
/// work just as it would in the original receive window.
fn observe_validated_server_source_ack(state: &mut SessionState, ack_sequence: u16) {
    let ack_sequence =
        server_replay::observe_peer_ack_sequence(&mut state.server_reliable_slots, ack_sequence);
    inventory_equipment::observe_server_ack_for_client_gui_status(state, ack_sequence);
    synthetic_area::maybe_queue_area_loaded_retransmit(
        &mut state.synthetic_area.in_flight_area_loaded,
        &mut state.synthetic_area.completed_area_loaded,
        &mut state.sequence.pending_client_to_server_packets,
        ack_sequence,
    );
}

fn translate_duplicate_native_area_loaded_after_synthetic(
    state: &mut SessionState,
    bytes: Vec<u8>,
    view: &MFrameView,
) -> anyhow::Result<client_filters::ClientFrameTranslation> {
    let translated = client_filters::consume_claimed_high_level_as_empty(
        bytes,
        view,
        "Area_AreaLoaded",
        "native Area_AreaLoaded arrived after proxy-owned synthetic fallback was already sent",
    )?;
    if let Some(packet) = translated.packet.as_ref() {
        observe_verified_client_m_packet(state, translated.family, packet);
    }
    Ok(translated)
}

fn translate_client_to_server_packet(
    state: &mut SessionState,
    bytes: Vec<u8>,
    view: &MFrameView,
    origin_client_sequence: u16,
) -> anyhow::Result<client_filters::ClientFrameTranslation> {
    let translated = client_filters::translate_client_frame(bytes, view, &mut state.semantic)?;
    let reliable_data_frame = view.frame_kind() == Some(MFrameType::ReliableData);
    if translated.proxy_ack_client_sequence.is_some() && reliable_data_frame {
        queue_proxy_owned_ack_for_consumed_client_frame(state, origin_client_sequence)?;
    }
    if translated.elide_client_sequence && reliable_data_frame {
        record_client_sequence_elision(state, origin_client_sequence);
    }
    let observe_packet = translated.packet.is_some()
        && !(translated.family == VerifiedFamily::ConsumedEmptyMFrame
            && !translated.semantic_observations.is_empty());
    for observation in &translated.semantic_observations {
        observe_verified_client_payload(state, observation.family, &observation.payload);
    }
    if observe_packet && let Some(packet) = translated.packet.as_ref() {
        observe_verified_client_m_packet(state, translated.family, packet);
    }
    Ok(translated)
}

fn record_client_sequence_elision(state: &mut SessionState, origin_client_sequence: u16) {
    if state
        .sequence
        .client_sequence_elisions
        .iter()
        .any(|elision| elision.sequence == origin_client_sequence)
    {
        tracing::debug!(
            sequence = origin_client_sequence,
            "client M sequence elision already recorded"
        );
        return;
    }

    state
        .sequence
        .client_sequence_elisions
        .push(SequenceElision {
            sequence: origin_client_sequence,
        });
    trim_sequence_elisions(&mut state.sequence.client_sequence_elisions);
    tracing::info!(
        sequence = origin_client_sequence,
        elisions = state.sequence.client_sequence_elisions.len(),
        "client M sequence elided for proxy-owned EE-only packet"
    );
}

fn queue_proxy_owned_ack_for_consumed_client_frame(
    state: &mut SessionState,
    ack_sequence: u16,
) -> anyhow::Result<()> {
    client_ack::queue_consumed_ee_only_ack(&mut state.client_ack.pending, ack_sequence);
    Ok(())
}

#[derive(Clone, Copy)]
enum PendingServerGatePolicy<'a> {
    Ignore,
    CurrentState,
    CurrentEmit {
        pending_count_before: usize,
        area_gate_after_current: &'a Option<synthetic_area::ServerHoldGate>,
    },
}

fn take_due_pending_server_packets(
    state: &mut SessionState,
    now: Instant,
    release_log: &'static str,
    include_client_ack: bool,
    gate_policy: PendingServerGatePolicy<'_>,
) -> Vec<(Option<usize>, synthetic_area::PendingServerPacket)> {
    let mut due = Vec::new();
    let mut kept = Vec::new();
    if include_client_ack {
        due.extend(
            client_ack::take_due_consumed_ee_only_ack_packets(&mut state.client_ack.pending, now)
                .into_iter()
                .map(|pending| (None, pending)),
        );
    }
    let pending_packets = state
        .synthetic_area
        .pending_server_to_client_packets
        .drain(..)
        .collect::<Vec<_>>();

    for (original_index, pending) in pending_packets.into_iter().enumerate() {
        if pending.due_at > now {
            kept.push(pending);
            continue;
        }
        let can_reach_outer_validator = match gate_policy {
            PendingServerGatePolicy::Ignore => true,
            PendingServerGatePolicy::CurrentState => {
                pending_server_packet_can_reach_outer_validator(state, &pending)
            }
            PendingServerGatePolicy::CurrentEmit {
                pending_count_before,
                area_gate_after_current,
            } => pending_server_packet_can_reach_for_current_emit(
                state,
                &pending,
                original_index,
                pending_count_before,
                area_gate_after_current,
            ),
        };
        if !can_reach_outer_validator {
            tracing::trace!(
                family = pending.family.as_str(),
                reason = pending.reason,
                "due synthetic server packet retained in its typed queue behind an active emission gate"
            );
            kept.push(pending);
            continue;
        }

        tracing::info!(
            reason = pending.reason,
            due_ms_ago = now.saturating_duration_since(pending.due_at).as_millis(),
            "{release_log}"
        );
        state.semantic.synthetic.server_synthetic_packets = state
            .semantic
            .synthetic
            .server_synthetic_packets
            .saturating_add(1);
        observe_verified_synthetic_server_m_packet(state, pending.family, &pending.packet);
        if pending.family == VerifiedFamily::Inventory
            && pending.reason == inventory_equipment::CONFIRMED_CLIENT_GUI_INVENTORY_REPLAY_REASON
        {
            state
                .inventory_equipment
                .record_confirmed_inventory_replay_dispatch();
            tracing::info!(
                dispatched_packets = state
                    .inventory_equipment
                    .confirmed_inventory_replay_dispatches,
                update_index = state
                    .inventory_equipment
                    .last_confirmed_inventory_replay_dispatch_update_index
                    .unwrap_or(0),
                "inventory/equipment confirmed Inventory replay dispatched to client transport"
            );
        }
        due.push((Some(original_index), pending));
    }

    state.synthetic_area.pending_server_to_client_packets = kept;
    due
}

fn pending_server_packet_can_reach_outer_validator(
    state: &SessionState,
    pending: &synthetic_area::PendingServerPacket,
) -> bool {
    pending_server_packet_can_reach_outer_validator_with_area_gate(
        state,
        pending,
        &state.synthetic_area.server_hold_gate,
    )
}

fn pending_server_packet_can_reach_outer_validator_with_area_gate(
    state: &SessionState,
    pending: &synthetic_area::PendingServerPacket,
    area_gate: &Option<synthetic_area::ServerHoldGate>,
) -> bool {
    if deferred_module_resources::module_resource_hold_gate_release_sequence(
        &state.deferred_module_resources.pending,
    )
    .is_some()
        && !module_resource_gate_window_packet_can_pass(state, &pending.packet)
    {
        return false;
    }

    let proof = VerifiedProof::family(pending.family);
    area_gate.is_none()
        || area_load_gate_packet_would_release_from(area_gate, &proof, &pending.packet)
}

fn pending_server_packet_can_reach_for_current_emit(
    state: &SessionState,
    pending: &synthetic_area::PendingServerPacket,
    original_index: usize,
    pending_count_before: usize,
    area_gate_after_current: &Option<synthetic_area::ServerHoldGate>,
) -> bool {
    let belongs_before_current = matches!(
        pending.placement,
        synthetic_area::PendingServerPacketPlacement::BeforeCurrentEmit
    ) || original_index < pending_count_before;
    let area_gate = if belongs_before_current {
        &state.synthetic_area.server_hold_gate
    } else {
        area_gate_after_current
    };
    pending_server_packet_can_reach_outer_validator_with_area_gate(state, pending, area_gate)
}

fn active_server_reassembly_will_complete_on_frame(
    state: &SessionState,
    view: &MFrameView,
) -> bool {
    let Some(reassembly) = state.deflate.server_reassembly.as_ref() else {
        return false;
    };
    if view.frame_type != 0 {
        return false;
    }
    let distance = view.sequence.wrapping_sub(reassembly.first_sequence) as usize;
    distance < reassembly.expected_frames
        && reassembly.frames.len().saturating_add(1) >= reassembly.expected_frames
}

fn is_client_module_loaded(high: Option<HighLevel>) -> bool {
    matches!(high, Some(high) if high.major == 0x03 && high.minor == 0x02)
}

pub fn translate_server_to_client(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let source_view = MFrameView::parse(bytes)
        .ok_or_else(|| anyhow::anyhow!("server M frame failed reliable-window parse"))?;
    validate_source_transport(&source_view, "server")?;
    let source_ack_sequence = source_view.ack_sequence;
    if state.client_emit_effect_snapshot.is_some()
        || state.client_emit_pending_validation.is_some()
        || state.pending_client_drain_effect_snapshot.is_some()
        || state.ack_delivery.pending.is_some()
    {
        anyhow::bail!("client emit validation authority still active before server translation");
    }
    // FrameReceive pins a type-0 source slot before CNW gameplay dispatch.
    // This ledger intentionally lives outside the speculative engine-facing
    // snapshot, so a strict reader rejection can retry only the exact stored
    // datagram (ACK/CRC/FrameSend bit 6 aside).
    let prepared_server_source =
        server_replay::prepare_source_slot(&mut state.server_reliable_slots, bytes, &source_view)?;
    let server_origin_generation = prepared_server_source
        .key()
        .map(|key| key.origin_generation)
        .unwrap_or(state.server_reliable_slots.origin_generation);
    match prepared_server_source {
        server_replay::PreparedServerReliableSource::Conflict(key) => {
            observe_validated_server_source_ack(state, source_ack_sequence);
            tracing::warn!(
                sequence = key.sequence,
                origin_generation = key.origin_generation,
                ack_sequence = source_ack_sequence,
                "server reliable retransmit rejected because its pinned source slot carried different immutable bytes"
            );
            return Ok(Emit::Drop);
        }
        server_replay::PreparedServerReliableSource::OutsideWindow(key) => {
            observe_validated_server_source_ack(state, source_ack_sequence);
            tracing::warn!(
                sequence = key.sequence,
                origin_generation = key.origin_generation,
                ack_sequence = source_ack_sequence,
                receive_start = state.server_reliable_slots.receive_start,
                "server reliable datagram rejected outside the decompile-proven 16-frame receive window"
            );
            return Ok(Emit::Drop);
        }
        _ => {}
    }
    let parsed_view = Some(source_view);
    let parsed_sequence = parsed_view.as_ref().map(|view| view.sequence);
    let parsed_frame_type = parsed_view.as_ref().map(|view| view.frame_type);
    let source_transport_identity = parsed_view
        .as_ref()
        .and_then(|view| transport_identity::server_reliable_data_transport_identity(bytes, view));
    let fence_candidate_sequence = match (
        parsed_sequence,
        state.deflate.ordered_successor_next_sequence,
    ) {
        _ if parsed_frame_type != Some(0) => None,
        (None, _) | (_, None) => None,
        (Some(sequence), Some(expected)) if sequence == expected => Some(expected),
        (Some(sequence), Some(expected)) if sequence_at_or_after(sequence, expected) => {
            observe_validated_server_source_ack(state, source_ack_sequence);
            tracing::warn!(
                sequence,
                expected_sequence = expected,
                "future server M withheld behind missing ordered successor"
            );
            return Ok(Emit::Drop);
        }
        _ => None,
    };
    if let Some(fence_sequence) = fence_candidate_sequence {
        if state.deflate.ordered_successor_pending_validation.is_some() {
            observe_validated_server_source_ack(state, source_ack_sequence);
            tracing::warn!(
                sequence = fence_sequence,
                "ordered server successor received before prior emit validation completed"
            );
            return Ok(Emit::Drop);
        }
        if let Some(index) = state
            .deflate
            .ordered_successor_events
            .iter()
            .position(|event| Some(event.sequence) == parsed_sequence)
        {
            let event = &state.deflate.ordered_successor_events[index];
            if source_transport_identity.as_deref()
                != Some(event.transport_payload_identity.as_slice())
            {
                observe_validated_server_source_ack(state, source_ack_sequence);
                tracing::warn!(
                    sequence = parsed_sequence.unwrap_or(0),
                    "conflicting retransmit rejected by ordered successor identity fence"
                );
                return Ok(Emit::Drop);
            }
            let view = parsed_view
                .as_ref()
                .expect("ordered successor candidate has a valid parsed frame");
            state.deflate.ordered_successor_events[index].packet = bytes.to_vec();
            state.deflate.ordered_successor_events[index].server_peer_ack_sequence =
                view.ack_sequence;
        }

        let view = parsed_view
            .as_ref()
            .expect("ordered successor candidate has a valid parsed frame");
        if !state
            .deflate
            .ordered_successor_events
            .iter()
            .any(|event| event.sequence == fence_sequence)
        {
            // A gap fence can name a missing predecessor which was never part
            // of the buffered suffix. Once that exact packet arrives, retain
            // it as the same raw transaction authority used for captured
            // successors; final validation must never advance an identity
            // that has no replayable slot.
            let transport_payload_identity =
                source_transport_identity.clone().ok_or_else(|| {
                    anyhow::anyhow!("ordered server successor left the type-0 data lane")
                })?;
            let event = reassembly::BufferedInterleavedServerPacket {
                packet: bytes.to_vec(),
                sequence: fence_sequence,
                server_peer_ack_sequence: view.ack_sequence,
                server_origin_generation,
                transport_payload_identity,
            };
            if let Err(err) = merge_ordered_server_successor_events(state, &[event]) {
                return Err(err);
            }
        }
    }

    state.deflate.last_server_core_dispatch_accepted = false;
    let emit = match translate_server_to_client_inner(bytes, state) {
        Ok(emit) => emit,
        Err(err) => {
            rollback_server_emit_effect_transaction(state);
            return Err(err);
        }
    };
    if let Some(expected) = fence_candidate_sequence.filter(|_| {
        state.deflate.last_server_core_dispatch_accepted && !matches!(&emit, Emit::Drop)
    }) {
        let final_sequence = state
            .deflate
            .ordered_successor_final_sequence
            .unwrap_or(expected);
        let transport_payload_identity = source_transport_identity.clone().ok_or_else(|| {
            anyhow::anyhow!("validated ordered server successor left the type-0 data lane")
        })?;
        let server_origin_generation = state
            .deflate
            .ordered_successor_events
            .iter()
            .find(|event| {
                event.sequence == expected
                    && event.transport_payload_identity == transport_payload_identity
            })
            .map(|event| event.server_origin_generation)
            .unwrap_or(server_origin_generation);
        state.deflate.ordered_successor_pending_validation =
            Some(state::OrderedSuccessorValidationToken {
                sequence: expected,
                server_origin_generation,
                transport_payload_identity,
            });
        tracing::info!(
            sequence = expected,
            final_sequence,
            "successfully dispatched server M staged ordered successor advancement pending strict validation"
        );
    } else if fence_candidate_sequence.is_some() {
        // No candidate reaches the outer validator when core dispatch fails
        // closed, so restore speculative state here while retaining the raw
        // ordered slot for a later exact retry.
        rollback_server_emit_effect_transaction(state);
    }
    if matches!(&emit, Emit::Drop) {
        // A Drop has no candidate for the outer strict owner. Roll back the
        // ordinary speculative boundary here while retaining the raw source
        // slot pinned above for an exact retry.
        rollback_ordinary_server_emit_after_drop(state);
    }
    Ok(emit)
}

fn begin_ordered_successor_effect_transaction(
    state: &mut SessionState,
    sequence: u16,
) -> anyhow::Result<()> {
    if state
        .deflate
        .ordered_successor_next_sequence
        .is_some_and(|active| active != sequence)
    {
        anyhow::bail!(
            "ordered successor effect transaction sequence {} does not match active fence {:?}",
            sequence,
            state.deflate.ordered_successor_next_sequence
        );
    }
    if state.deflate.ordered_successor_effect_snapshot.is_some()
        || state.deflate.server_emit_effect_transaction_kind.is_some()
    {
        anyhow::bail!(
            "ordered successor effect transaction already active for sequence {}",
            sequence
        );
    }

    let snapshot = capture_engine_facing_effect_snapshot(state);
    state.deflate.ordered_successor_effect_snapshot = Some(Box::new(snapshot));
    state.deflate.server_emit_effect_transaction_kind =
        Some(state::ServerEmitEffectTransactionKind::OrderedSuccessor);
    tracing::debug!(
        sequence,
        "direct ordered successor began speculative engine-facing effect transaction"
    );
    Ok(())
}

fn promote_or_begin_ordered_successor_effect_transaction(
    state: &mut SessionState,
    sequence: u16,
) -> anyhow::Result<()> {
    if state.deflate.ordered_successor_effect_snapshot.is_some()
        && state.deflate.server_emit_effect_transaction_kind
            == Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit)
    {
        if state.deflate.ordered_successor_next_sequence != Some(sequence) {
            anyhow::bail!(
                "server source transaction cannot promote to ordered successor {} with active fence {:?}",
                sequence,
                state.deflate.ordered_successor_next_sequence
            );
        }
        if state.deflate.ordered_successor_pending_validation.is_some() {
            anyhow::bail!(
                "server source transaction cannot promote while another ordered validation identity is staged"
            );
        }
        state.deflate.server_emit_effect_transaction_kind =
            Some(state::ServerEmitEffectTransactionKind::OrderedSuccessor);
        tracing::debug!(
            sequence,
            "completing server source transaction promoted to ordered successor authority"
        );
        return Ok(());
    }
    begin_ordered_successor_effect_transaction(state, sequence)
}

fn begin_ordinary_server_emit_effect_transaction(state: &mut SessionState) -> anyhow::Result<()> {
    if state.deflate.ordered_successor_effect_snapshot.is_some()
        || state.deflate.server_emit_effect_transaction_kind.is_some()
    {
        anyhow::bail!(
            "server emit effect transaction already active before ordinary coalesced dispatch"
        );
    }

    let snapshot = capture_engine_facing_effect_snapshot(state);
    state.deflate.ordered_successor_effect_snapshot = Some(Box::new(snapshot));
    state.deflate.server_emit_effect_transaction_kind =
        Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit);
    tracing::debug!("ordinary server emit began speculative engine-facing effect transaction");
    Ok(())
}

/// Start a reversible client-originated emit transaction before semantic
/// translation runs. The source frame has already passed the same CRC, length,
/// kind, and exact-control checks used by the receive-window owner, so its
/// data-sequence and ACK observations remain transport truth if the translated
/// legacy emit is rejected later.
pub(super) fn begin_client_to_server_emit_validation(
    state: &mut SessionState,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let view = MFrameView::parse(bytes)
        .ok_or_else(|| anyhow::anyhow!("client M frame failed reliable-window parse"))?;
    validate_source_transport(&view, "client")?;
    if state.client_emit_effect_snapshot.is_some()
        || state.client_emit_pending_validation.is_some()
        || state.pending_client_drain_effect_snapshot.is_some()
        || state.ack_delivery.pending.is_some()
        || state.deflate.ordered_successor_effect_snapshot.is_some()
        || state.deflate.server_emit_effect_transaction_kind.is_some()
        || state.deflate.ordered_successor_pending_validation.is_some()
    {
        anyhow::bail!("M emit validation authority already active before client translation");
    }

    // Pin the receive-window identity before the speculative gameplay
    // snapshot. Diamond and EE retain the type-0 slot even when the later CNW
    // reader rejects its reconstructed message; only the translated replay
    // disposition is rolled back for an exact retry.
    let prepared_source =
        client_replay::prepare_source_slot(&mut state.client_reliable_replays, bytes, &view)?;
    let source_reliable_sequence = (view.frame_kind() == Some(MFrameType::ReliableData)
        && !matches!(
            &prepared_source,
            client_replay::PreparedClientReliableSource::Conflict(_)
                | client_replay::PreparedClientReliableSource::OutsideWindow(_)
        ))
    .then_some(view.sequence);
    let token = state::ClientEmitValidationToken {
        source_reliable_sequence,
        source_origin_generation: prepared_source.key().map(|key| key.origin_generation),
        source_ack_sequence: view.ack_sequence,
    };
    let snapshot = capture_engine_facing_effect_snapshot(state);
    state.client_emit_effect_snapshot = Some(Box::new(snapshot));
    state.client_emit_pending_validation = Some(token);
    tracing::trace!(
        source_sequence = token.source_reliable_sequence,
        source_origin_generation = token.source_origin_generation,
        source_ack_sequence = token.source_ack_sequence,
        "client M began speculative engine-facing effect transaction"
    );
    Ok(())
}

fn begin_pending_client_drain_effect_transaction(state: &mut SessionState) -> anyhow::Result<()> {
    if state.pending_client_drain_effect_snapshot.is_some()
        || state.client_emit_effect_snapshot.is_some()
        || state.client_emit_pending_validation.is_some()
        || state.ack_delivery.pending.is_some()
        || state.deflate.ordered_successor_effect_snapshot.is_some()
        || state.deflate.server_emit_effect_transaction_kind.is_some()
        || state.deflate.ordered_successor_pending_validation.is_some()
    {
        anyhow::bail!("M emit validation authority already active before pending client drain");
    }

    state.pending_client_drain_effect_snapshot = Some(state::PendingClientDrainEffectSnapshot {
        pending_packets: state.sequence.pending_client_to_server_packets.clone(),
        pending_area_loaded: state.synthetic_area.pending_area_loaded.clone(),
        in_flight_area_loaded: state.synthetic_area.in_flight_area_loaded.clone(),
        server_hold_gate: state.synthetic_area.server_hold_gate.clone(),
        client_sequence_shifts: state.sequence.client_sequence_shifts.clone(),
    });
    Ok(())
}

fn rollback_pending_client_drain_effect_transaction(state: &mut SessionState) -> bool {
    let Some(snapshot) = state.pending_client_drain_effect_snapshot.take() else {
        return false;
    };
    state.sequence.pending_client_to_server_packets = snapshot.pending_packets;
    state.synthetic_area.pending_area_loaded = snapshot.pending_area_loaded;
    state.synthetic_area.in_flight_area_loaded = snapshot.in_flight_area_loaded;
    state.synthetic_area.server_hold_gate = snapshot.server_hold_gate;
    state.sequence.client_sequence_shifts = snapshot.client_sequence_shifts;
    true
}

fn commit_pending_client_drain_effect_transaction(state: &mut SessionState) -> bool {
    state.pending_client_drain_effect_snapshot.take().is_some()
}

pub(super) fn finish_pending_client_drain_emit_validation(
    state: &mut SessionState,
    accepted: bool,
) {
    finish_pending_client_drain_effect_validation(state, accepted);
    finish_ack_delivery(
        state,
        ack_delivery::AckDeliveryOwner::PendingClientDrain,
        accepted,
    );
}

fn finish_pending_client_drain_effect_validation(state: &mut SessionState, accepted: bool) {
    if accepted {
        if commit_pending_client_drain_effect_transaction(state) {
            tracing::trace!(
                "strict-validated pending client drain committed exact typed packet batch"
            );
        }
        return;
    }

    let rolled_back = rollback_pending_client_drain_effect_transaction(state);
    tracing::warn!(
        rolled_back,
        "pending client packet batch restored after final strict validation rejection"
    );
}

fn begin_pending_server_drain_effect_transaction(state: &mut SessionState) -> anyhow::Result<()> {
    ensure_pending_server_drain_can_start(state)?;

    let snapshot = capture_engine_facing_effect_snapshot(state);
    state.deflate.ordered_successor_effect_snapshot = Some(Box::new(snapshot));
    state.deflate.server_emit_effect_transaction_kind =
        Some(state::ServerEmitEffectTransactionKind::PendingServerDrain);
    tracing::trace!(
        "pending server synthetic drain began speculative engine-facing effect transaction"
    );
    Ok(())
}

fn ensure_pending_server_drain_can_start(state: &SessionState) -> anyhow::Result<()> {
    if state.deflate.ordered_successor_effect_snapshot.is_some()
        || state.deflate.server_emit_effect_transaction_kind.is_some()
        || state.deflate.ordered_successor_pending_validation.is_some()
        || state.client_emit_effect_snapshot.is_some()
        || state.client_emit_pending_validation.is_some()
        || state.pending_client_drain_effect_snapshot.is_some()
        || state.ack_delivery.pending.is_some()
    {
        anyhow::bail!(
            "server emit validation authority already active before pending synthetic drain"
        );
    }
    Ok(())
}

fn capture_engine_facing_effect_snapshot(
    state: &mut SessionState,
) -> state::EngineFacingEffectSnapshot {
    let snapshot = state::EngineFacingEffectSnapshot {
        server_reassembly: state.deflate.server_reassembly.clone(),
        completed_server_stream_windows: state.deflate.completed_server_stream_windows.clone(),
        completed_server_reliable_stream_slots: state
            .deflate
            .completed_server_reliable_stream_slots
            .clone(),
        server_zlib_stream_proxy_owned: state.deflate.server_zlib_stream_proxy_owned,
        server_zlib_stream_owner: state.deflate.server_zlib_stream_owner,
        server_zlib_stream_epoch: state.deflate.server_zlib_stream_epoch,
        server_zlib_inflater: state.deflate.server_zlib_inflater.clone(),
        coalesced_replay: state.coalesced_replay.clone(),
        quickbar: state.quickbar.clone(),
        live_object: state.live_object.clone(),
        sequence: state.sequence.clone(),
        client_reliable_replays: state.client_reliable_replays.clone(),
        client_ack: state.client_ack.pending.clone(),
        direct_server_semantic_replays: state.direct_server_semantic_replays.clone(),
        login_waypoint: state.login_waypoint.clone(),
        inventory_equipment: state.inventory_equipment.clone(),
        synthetic_area: state.synthetic_area.clone(),
        deferred_module_resources: state.deferred_module_resources.pending.clone(),
        area_context: state.area_context.clone(),
        module_resources: state.module_resources.clone(),
        semantic: state.semantic.clone(),
        quickbar_item_refresh_hint_last_body: state.quickbar_item_refresh_hint_last_body.clone(),
    };
    state.module_resources = state.module_resources.for_speculative_transaction();
    snapshot
}

fn restore_engine_facing_effect_snapshot(
    state: &mut SessionState,
    snapshot: state::EngineFacingEffectSnapshot,
) {
    state.deflate.server_reassembly = snapshot.server_reassembly;
    state.deflate.completed_server_stream_windows = snapshot.completed_server_stream_windows;
    state.deflate.completed_server_reliable_stream_slots =
        snapshot.completed_server_reliable_stream_slots;
    state.deflate.server_zlib_stream_proxy_owned = snapshot.server_zlib_stream_proxy_owned;
    state.deflate.server_zlib_stream_owner = snapshot.server_zlib_stream_owner;
    state.deflate.server_zlib_stream_epoch = snapshot.server_zlib_stream_epoch;
    state.deflate.server_zlib_inflater = snapshot.server_zlib_inflater;
    state.coalesced_replay = snapshot.coalesced_replay;
    state.quickbar = snapshot.quickbar;
    state.live_object = snapshot.live_object;
    state.sequence = snapshot.sequence;
    state.client_reliable_replays = snapshot.client_reliable_replays;
    state.client_ack.pending = snapshot.client_ack;
    state.direct_server_semantic_replays = snapshot.direct_server_semantic_replays;
    state.login_waypoint = snapshot.login_waypoint;
    state.inventory_equipment = snapshot.inventory_equipment;
    state.synthetic_area = snapshot.synthetic_area;
    state.deferred_module_resources.pending = snapshot.deferred_module_resources;
    state.area_context = snapshot.area_context;
    state.module_resources = snapshot.module_resources;
    state.semantic = snapshot.semantic;
    state.quickbar_item_refresh_hint_last_body = snapshot.quickbar_item_refresh_hint_last_body;
}

fn commit_engine_facing_effect_transaction(state: &mut SessionState) {
    // Speculative module observations suppress external publication until the
    // enclosing reader transaction succeeds.
    state.module_resources.commit_speculative_observations();
    update_quickbar_item_refresh_hint(state);
}

fn reapply_validated_client_source_transport(
    state: &mut SessionState,
    token: state::ClientEmitValidationToken,
) {
    if let Some(sequence) = token.source_reliable_sequence {
        record_forward_progress(
            &mut state.sequence.latest_client_sequence_from_client,
            sequence,
        );
    }
    record_forward_progress(
        &mut state.sequence.latest_client_ack_from_client,
        token.source_ack_sequence,
    );
    synthetic_area::observe_server_hold_gate_client_ack(
        &mut state.synthetic_area.server_hold_gate,
        token.source_ack_sequence,
    );
    deferred_module_resources::observe_resource_hold_gate_client_ack(
        &mut state.deferred_module_resources.pending,
        token.source_ack_sequence,
    );
}

/// Commit or reject client-originated semantic, replay, queue, and sequence-
/// transform effects after the complete translated emit has passed the outer
/// strict owner. Validated source receive-window progress is restored after a
/// rejection because it belongs to the proxy-facing transport lane, not to the
/// rejected legacy emit.
pub(super) fn finish_client_to_server_emit_validation(state: &mut SessionState, accepted: bool) {
    finish_client_to_server_emit_validation_outcomes(state, accepted, accepted);
}

/// Finish the client payload transaction and destination-facing ACK delivery
/// independently. An exact ACK-only carrier can pass strict validation even
/// when the source payload disposition was fail-closed; that must retire the
/// acknowledged server slots without committing speculative gameplay effects.
pub(super) fn finish_client_to_server_emit_validation_outcomes(
    state: &mut SessionState,
    effects_accepted: bool,
    ack_output_accepted: bool,
) {
    finish_client_to_server_effect_validation(state, effects_accepted, ack_output_accepted);
    finish_ack_delivery(
        state,
        ack_delivery::AckDeliveryOwner::DirectClient,
        ack_output_accepted,
    );
}

fn finish_client_to_server_effect_validation(
    state: &mut SessionState,
    effects_accepted: bool,
    ack_output_accepted: bool,
) {
    let token = state.client_emit_pending_validation.take();
    let snapshot = state.client_emit_effect_snapshot.take();
    let has_token = token.is_some();
    let has_snapshot = snapshot.is_some();
    let (Some(token), Some(snapshot)) = (token, snapshot) else {
        if has_token || has_snapshot {
            tracing::warn!(
                effects_accepted,
                ack_output_accepted,
                has_token,
                has_snapshot,
                "client M validation authority cleared with an incomplete transaction"
            );
        }
        return;
    };

    if effects_accepted {
        commit_engine_facing_effect_transaction(state);
        tracing::trace!(
            source_sequence = token.source_reliable_sequence,
            source_origin_generation = token.source_origin_generation,
            source_ack_sequence = token.source_ack_sequence,
            "strict-validated client M committed speculative engine-facing effects"
        );
        return;
    }

    restore_engine_facing_effect_snapshot(state, *snapshot);
    reapply_validated_client_source_transport(state, token);
    if ack_output_accepted {
        tracing::info!(
            source_sequence = token.source_reliable_sequence,
            source_origin_generation = token.source_origin_generation,
            source_ack_sequence = token.source_ack_sequence,
            "client M payload effects rolled back while its independent ACK-only output passed strict validation"
        );
    } else {
        tracing::warn!(
            source_sequence = token.source_reliable_sequence,
            source_origin_generation = token.source_origin_generation,
            source_ack_sequence = token.source_ack_sequence,
            "client M engine-facing effects rolled back after final strict emit validation rejected the complete batch"
        );
    }
}

fn rollback_server_emit_effect_transaction(state: &mut SessionState) -> bool {
    let transaction_kind = state.deflate.server_emit_effect_transaction_kind.take();
    let Some(snapshot) = state.deflate.ordered_successor_effect_snapshot.take() else {
        if transaction_kind.is_some() {
            tracing::warn!(
                ?transaction_kind,
                "server emit transaction kind cleared without a matching effect snapshot"
            );
        }
        return false;
    };
    restore_engine_facing_effect_snapshot(state, *snapshot);
    // Another validated server datagram can arrive while the outer owner is
    // deciding an earlier candidate. Reapply the newest independently valid
    // peer ACK after restoring that older gameplay snapshot.
    if let Some(ack_sequence) = state.server_reliable_slots.latest_peer_ack_sequence {
        observe_validated_server_source_ack(state, ack_sequence);
    }
    true
}

fn rollback_ordinary_server_emit_after_drop(state: &mut SessionState) -> bool {
    if state.deflate.server_emit_effect_transaction_kind
        != Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit)
    {
        return false;
    }
    rollback_server_emit_effect_transaction(state)
}

fn commit_server_emit_effect_transaction(
    state: &mut SessionState,
    expected_kind: state::ServerEmitEffectTransactionKind,
) -> bool {
    if state.deflate.server_emit_effect_transaction_kind != Some(expected_kind) {
        return false;
    }
    state.deflate.server_emit_effect_transaction_kind = None;
    let Some(_snapshot) = state.deflate.ordered_successor_effect_snapshot.take() else {
        tracing::warn!(
            ?expected_kind,
            "server emit effect commit rejected because its tagged snapshot disappeared"
        );
        return false;
    };
    commit_engine_facing_effect_transaction(state);
    true
}

/// Commit or reject the server emit effect transaction after the outer strict
/// validator has classified the complete emitted packet batch.
///
/// Diamond `sub_5F3940` and EE `CNetLayerWindow::FrameReceive` advance reliable
/// receive storage in sequence order, while gameplay dispatch happens only
/// after the stored message is reconstructed. Proxy2 similarly must not dequeue
/// a buffered successor or retain an ordinary coalesced window merely because
/// its inner translator returned plausible packets: the complete emitted shape
/// must pass the final strict owner first.
pub(super) fn finish_server_to_client_emit_validation(state: &mut SessionState, accepted: bool) {
    finish_server_to_client_emit_validation_outcomes(state, accepted, accepted);
}

/// Finish server payload effects separately from the exact client-facing ACK
/// carrier. This keeps conflict/outside-window payload rejection fail-closed
/// while preserving independently valid peer ACK progress.
pub(super) fn finish_server_to_client_emit_validation_outcomes(
    state: &mut SessionState,
    effects_accepted: bool,
    ack_output_accepted: bool,
) {
    finish_server_to_client_effect_validation(state, effects_accepted);
    finish_ack_delivery(
        state,
        ack_delivery::AckDeliveryOwner::DirectServer,
        ack_output_accepted,
    );
}

fn finish_server_to_client_effect_validation(state: &mut SessionState, accepted: bool) {
    if state.deflate.server_emit_effect_transaction_kind
        == Some(state::ServerEmitEffectTransactionKind::PendingServerDrain)
    {
        tracing::warn!(
            accepted,
            "pending server synthetic drain ignored unrelated server-origin validation callback"
        );
        return;
    }
    let token = state.deflate.ordered_successor_pending_validation.take();
    if !accepted {
        let rolled_back_effects = rollback_server_emit_effect_transaction(state);
        if let Some(token) = token {
            tracing::warn!(
                sequence = token.sequence,
                queued_events = state.deflate.ordered_successor_events.len(),
                rolled_back_effects,
                "ordered server successor retained because final strict emit validation rejected it"
            );
        } else if rolled_back_effects {
            tracing::warn!(
                "server M engine-facing effects rolled back after final strict emit validation rejected the complete batch"
            );
        }
        return;
    }

    let Some(token) = token else {
        match state.deflate.server_emit_effect_transaction_kind {
            Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit) => {
                if commit_server_emit_effect_transaction(
                    state,
                    state::ServerEmitEffectTransactionKind::OrdinaryServerEmit,
                ) {
                    tracing::debug!(
                        "strict-validated ordinary server emit committed speculative engine-facing effects"
                    );
                }
            }
            Some(state::ServerEmitEffectTransactionKind::PendingServerDrain) => {
                tracing::warn!(
                    "pending server synthetic transaction reached server-origin validation match after ownership guard"
                );
                return;
            }
            Some(state::ServerEmitEffectTransactionKind::OrderedSuccessor) => {
                let rolled_back_effects = rollback_server_emit_effect_transaction(state);
                tracing::warn!(
                    rolled_back_effects,
                    "ordered successor engine-facing effects rolled back because no exact validation identity was staged"
                );
            }
            None => {
                if state.deflate.ordered_successor_effect_snapshot.is_some() {
                    let rolled_back_effects = rollback_server_emit_effect_transaction(state);
                    tracing::warn!(
                        rolled_back_effects,
                        "untagged server emit effect snapshot rolled back instead of accepting without ownership"
                    );
                }
            }
        }
        return;
    };
    if state.deflate.server_emit_effect_transaction_kind
        != Some(state::ServerEmitEffectTransactionKind::OrderedSuccessor)
    {
        let transaction_kind = state.deflate.server_emit_effect_transaction_kind;
        let rolled_back_effects = rollback_server_emit_effect_transaction(state);
        tracing::warn!(
            sequence = token.sequence,
            ?transaction_kind,
            rolled_back_effects,
            "ordered successor validation token rejected because its effect transaction kind did not match"
        );
        return;
    }
    let expected = token.sequence;
    if state.deflate.ordered_successor_next_sequence != Some(expected) {
        let rolled_back_effects = rollback_server_emit_effect_transaction(state);
        tracing::warn!(
            sequence = expected,
            active_sequence = state.deflate.ordered_successor_next_sequence.unwrap_or(0),
            rolled_back_effects,
            "ordered server successor validation result ignored because the active fence changed"
        );
        return;
    }

    let queued_identity = state
        .deflate
        .ordered_successor_events
        .iter()
        .find(|event| event.sequence == expected)
        .map(|event| {
            (
                event.server_origin_generation,
                event.server_origin_generation == token.server_origin_generation
                    && event.transport_payload_identity == token.transport_payload_identity,
            )
        });
    let Some((queued_server_origin_generation, queued_identity_matches)) = queued_identity else {
        let rolled_back_effects = rollback_server_emit_effect_transaction(state);
        tracing::warn!(
            sequence = expected,
            validated_server_origin_generation = token.server_origin_generation,
            rolled_back_effects,
            "ordered server successor retained because its exact raw queue identity disappeared before final validation"
        );
        return;
    };
    if !queued_identity_matches {
        let rolled_back_effects = rollback_server_emit_effect_transaction(state);
        tracing::warn!(
            sequence = expected,
            queued_server_origin_generation,
            validated_server_origin_generation = token.server_origin_generation,
            rolled_back_effects,
            "ordered server successor retained because validated token no longer matches raw queue identity"
        );
        return;
    }

    let committed_effects = commit_server_emit_effect_transaction(
        state,
        state::ServerEmitEffectTransactionKind::OrderedSuccessor,
    );

    let final_sequence = state
        .deflate
        .ordered_successor_final_sequence
        .unwrap_or(expected);
    state.deflate.ordered_successor_events.retain(|event| {
        event.sequence != expected
            || event.server_origin_generation != token.server_origin_generation
            || event.transport_payload_identity != token.transport_payload_identity
    });
    if expected == final_sequence {
        state.deflate.ordered_successor_next_sequence = None;
        state.deflate.ordered_successor_final_sequence = None;
        state.deflate.ordered_successor_events.clear();
    } else {
        state.deflate.ordered_successor_next_sequence = Some(next_reliable_sequence(expected));
    }
    tracing::info!(
        sequence = expected,
        final_sequence,
        remaining_queued_events = state.deflate.ordered_successor_events.len(),
        committed_effects,
        "strict-validated server M committed ordered successor advancement"
    );
}

/// Commit or reject only the timer/session-drained synthetic server batch.
/// A pending-drain callback must never consume an ordered-successor token or
/// commit an ordinary server-origin transaction merely because an idle network
/// poll returned `Consumed`.
pub(super) fn finish_pending_server_drain_emit_validation(
    state: &mut SessionState,
    accepted: bool,
) {
    finish_pending_server_drain_effect_validation(state, accepted);
    finish_ack_delivery(
        state,
        ack_delivery::AckDeliveryOwner::PendingServerDrain,
        accepted,
    );
}

fn finish_pending_server_drain_effect_validation(state: &mut SessionState, accepted: bool) {
    let expected = state::ServerEmitEffectTransactionKind::PendingServerDrain;
    match state.deflate.server_emit_effect_transaction_kind {
        None if state.deflate.ordered_successor_effect_snapshot.is_none() => return,
        Some(kind) if kind == expected => {}
        transaction_kind => {
            tracing::warn!(
                ?transaction_kind,
                accepted,
                "pending server synthetic validation callback ignored foreign effect transaction"
            );
            return;
        }
    }

    if accepted {
        if commit_server_emit_effect_transaction(state, expected) {
            tracing::trace!(
                "strict-validated pending server synthetic drain committed speculative engine-facing effects"
            );
        }
    } else {
        let rolled_back_effects = rollback_server_emit_effect_transaction(state);
        tracing::warn!(
            rolled_back_effects,
            "pending server synthetic drain restored after final strict validation rejected the complete batch"
        );
    }
}

fn translate_server_to_client_inner(
    bytes: &[u8],
    state: &mut SessionState,
) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        anyhow::bail!("server M frame failed reliable-window parse");
    };
    // Apply the same original-writer boundary before origin generation, ACK,
    // gate, reassembly, or semantic state can trust the source. In particular,
    // a payload-bearing control must not be dispatchable and then laundered
    // into an empty verified control by a semantic consumer.
    validate_source_transport(&view, "server")?;
    // The public entry point performs this before ordered-fence handling; keep
    // the same boundary here as well because focused core tests enter this
    // dispatcher directly. An exact second prepare is an idempotent match.
    let prepared_server_source =
        server_replay::prepare_source_slot(&mut state.server_reliable_slots, bytes, &view)?;
    let server_origin_generation = prepared_server_source
        .key()
        .map(|key| key.origin_generation)
        .unwrap_or(state.server_reliable_slots.origin_generation);
    match prepared_server_source {
        server_replay::PreparedServerReliableSource::Conflict(key) => {
            observe_validated_server_source_ack(state, view.ack_sequence);
            tracing::warn!(
                sequence = key.sequence,
                origin_generation = key.origin_generation,
                ack_sequence = view.ack_sequence,
                "server reliable retransmit rejected before route selection because its pinned source slot carried different immutable bytes"
            );
            return Ok(Emit::Drop);
        }
        server_replay::PreparedServerReliableSource::OutsideWindow(key) => {
            observe_validated_server_source_ack(state, view.ack_sequence);
            tracing::warn!(
                sequence = key.sequence,
                origin_generation = key.origin_generation,
                ack_sequence = view.ack_sequence,
                receive_start = state.server_reliable_slots.receive_start,
                "server reliable datagram rejected before route selection outside the 16-frame receive window"
            );
            return Ok(Emit::Drop);
        }
        _ => {}
    }
    let server_peer_ack_sequence = view.ack_sequence;
    // ACK ownership is independent of gameplay-slot identity. Preserve it
    // before every older direct/stream/fence conflict return below.
    observe_validated_server_source_ack(state, server_peer_ack_sequence);
    if let Some(source_transport_identity) =
        transport_identity::server_reliable_data_transport_identity(bytes, &view)
    {
        if let Some(committed) =
            state
                .direct_server_semantic_replays
                .completed
                .iter()
                .find(|entry| {
                    entry.sequence == view.sequence
                        && entry.origin_generation == server_origin_generation
                })
        {
            if committed.source_transport_identity != source_transport_identity {
                tracing::warn!(
                    sequence = view.sequence,
                    server_origin_generation,
                    trailing_payload_length = view.trailing_payload_length,
                    packetized_sequence = view.packetized_sequence,
                    "server M frame rejected before route selection because its direct reliable slot carried different immutable transport bytes"
                );
                return Ok(Emit::Drop);
            }
        }
    }
    if state.deflate.ordered_successor_next_sequence == Some(view.sequence) {
        if let Some(event) = state
            .deflate
            .ordered_successor_events
            .iter()
            .find(|event| event.sequence == view.sequence)
        {
            if event.server_origin_generation != server_origin_generation {
                tracing::warn!(
                    sequence = view.sequence,
                    queued_server_origin_generation = event.server_origin_generation,
                    server_origin_generation,
                    "ordered server successor rejected across reliable-origin generation change"
                );
                return Ok(Emit::Drop);
            }
        }
    }
    if let CompletedServerReliableStreamSlotMatch::Conflict(committed_route) =
        reassembly::completed_server_reliable_stream_slot(
            state,
            view.sequence,
            server_origin_generation,
            bytes,
        )
    {
        tracing::warn!(
            sequence = view.sequence,
            server_origin_generation,
            committed_route = committed_route.as_str(),
            trailing_payload_length = view.trailing_payload_length,
            "server M frame rejected before route selection because its reliable slot already committed different immutable transport bytes"
        );
        return Ok(Emit::Drop);
    }
    if state.deflate.ordered_successor_next_sequence == Some(view.sequence) {
        // Transport truth above survives rejection. Everything below this
        // boundary is speculative client-facing dispatch/finalization state,
        // including an exact clone of any live persistent inflater.
        begin_ordered_successor_effect_transaction(state, view.sequence)?;
    }
    let pending_count_before = state.synthetic_area.pending_server_to_client_packets.len();
    let server_emit_started_at = Instant::now();
    let mut inbound = bytes.to_vec();
    unshift_server_ack_for_client(state, &mut inbound, &view)?;
    let view = MFrameView::parse(&inbound).unwrap_or(view);
    let packetized_multi_frame = view.packetized_sequence > 1;
    let starts_server_deflated_reassembly =
        reassembly::should_start_server_deflated_reassembly(&inbound, &view);
    let completes_server_deflated_source =
        active_server_reassembly_will_complete_on_frame(state, &view)
            || (state.deflate.server_reassembly.is_none()
                && starts_server_deflated_reassembly
                && view.packetized_sequence <= 1);
    let ordinary_coalesced_window = view.packetized_sequence == 1
        && view.trailing_payload_length > 0
        && state.deflate.ordered_successor_effect_snapshot.is_none();
    let source_can_queue_immediate_server_packet = completes_server_deflated_source
        || (state.deflate.server_reassembly.is_none()
            && (view
                .high
                .is_some_and(|high| high.major == 0x04 && high.minor == 0x01)
                || (!packetized_multi_frame
                    && state
                        .inventory_equipment
                        .pending_confirmed_inventory_replay
                        .is_some())));
    let ordinary_server_emit_transaction =
        state.deflate.ordered_successor_effect_snapshot.is_none()
            && (ordinary_coalesced_window
                || pending_server_drain_has_work(state, server_emit_started_at)
                || source_can_queue_immediate_server_packet
                || exact_direct_semantic_source_payload(&inbound, &view).is_some());
    if ordinary_server_emit_transaction {
        // A complete count-one stored window is one reader transaction. A due
        // or source-created proxy-owned server packet is likewise not emitted
        // truth until the mixed outer batch validates. Keep both sources'
        // semantic, queue, sequence, replay, and publication effects behind
        // that boundary. Raw Area_ClientArea, an armed direct inventory replay,
        // and a frame which completes a deflated source are the bounded
        // producers visible before dispatch. A completing source may promote
        // this authority if it also drains one buffered ordered successor.
        begin_ordinary_server_emit_effect_transaction(state)?;
    }
    if state.deflate.server_reassembly.is_some() {
        let emit = reassembly::continue_server_deflated_reassembly(
            &inbound,
            &view,
            state,
            server_peer_ack_sequence,
            server_origin_generation,
        )?;
        return finalize_server_to_client_emit(state, emit, pending_count_before);
    }

    if !packetized_multi_frame {
        deferred_module_resources::capture_early_server_status_if_needed(
            &inbound,
            &view,
            &state.module_resources,
            &mut state.deferred_module_resources.pending,
        );
    }
    // Diamond and EE only walk declared-length queued records in the count-one
    // branch. A count-greater-than-one datagram owns a single compressed member
    // assembled from complete stored frames, so it must never enter coalesced
    // span routing even when bytes 10..11 under-claim the stored first frame.
    if view.packetized_sequence == 1 {
        let rewrite = match coalesced::rewrite_server_window_spans_if_needed(
            &inbound,
            &view,
            state,
            server_peer_ack_sequence,
            server_origin_generation,
        ) {
            Ok(rewrite) => rewrite,
            Err(error) => {
                if ordinary_server_emit_transaction {
                    rollback_server_emit_effect_transaction(state);
                }
                return Err(error);
            }
        };
        if rewrite.is_none() && ordinary_coalesced_window {
            // Trailing storage that cannot be reduced to complete declared
            // records is not a successful empty coalesced window. Reject it
            // before finalization so unrelated due synthetic output cannot
            // turn the batch non-Drop and accidentally commit source effects.
            rollback_server_emit_effect_transaction(state);
            tracing::warn!(
                sequence = view.sequence,
                trailing_payload_length = view.trailing_payload_length,
                "count-one server M rejected because trailing storage is not a complete coalesced record set"
            );
            return Ok(Emit::Drop);
        }
        if let Some(rewrite) = rewrite {
            reassembly::remember_completed_server_reliable_stream_slot(
                state,
                view.sequence,
                server_origin_generation,
                &inbound,
                CompletedServerReliableStreamRoute::CoalescedWindow,
            );
            return match rewrite {
                coalesced::CoalescedRewrite::Single { proof, packet } => {
                    finalize_server_to_client_emit(
                        state,
                        Emit::VerifiedProofPackets {
                            proof,
                            packets: vec![packet],
                        },
                        pending_count_before,
                    )
                }
                coalesced::CoalescedRewrite::Split { packets } => finalize_server_to_client_emit(
                    state,
                    Emit::MixedVerifiedProofPackets(packets),
                    pending_count_before,
                ),
                coalesced::CoalescedRewrite::SplitPreShifted { packets } => {
                    finalize_server_to_client_emit(
                        state,
                        Emit::MixedVerifiedProofPacketsPreShifted(packets),
                        pending_count_before,
                    )
                }
            };
        }
    }

    let emit = if packetized_multi_frame && !starts_server_deflated_reassembly {
        tracing::warn!(
            sequence = view.sequence,
            packetized_sequence = view.packetized_sequence,
            declared_payload_length = view.declared_payload_length,
            stored_payload_length = view.available_payload_length,
            "multi-frame server M dropped before dispatch because its full stored first frame has no plausible deflated envelope"
        );
        Emit::Drop
    } else if starts_server_deflated_reassembly
        && (packetized_multi_frame || view.trailing_payload_length == 0)
    {
        reassembly::start_server_deflated_reassembly(
            &inbound,
            &view,
            state,
            server_peer_ack_sequence,
            server_origin_generation,
        )?
    } else if view.trailing_payload_length != 0 {
        tracing::warn!(
            sequence = view.sequence,
            trailing_payload_length = view.trailing_payload_length,
            "server M packet with unclaimed trailing records dropped before deflated or direct semantic dispatch"
        );
        Emit::Drop
    } else if let Some(verified) = replay_completed_direct_server_semantic_rewrite(
        &inbound,
        &view,
        server_origin_generation,
        state,
    )? {
        Emit::VerifiedProofPackets {
            proof: verified.proof,
            packets: vec![verified.packet],
        }
    } else if let Some(rewrite) = server_dispatch::rewrite_direct_frame_if_needed(
        &inbound,
        &view,
        &state.module_resources,
        Some(&state.area_context.latest_area_placeables),
        Some(&state.semantic.objects),
    )? {
        let verified = commit_direct_server_semantic_rewrite(
            state,
            &inbound,
            &view,
            server_peer_ack_sequence,
            server_origin_generation,
            rewrite,
        )?;
        Emit::VerifiedProofPackets {
            proof: verified.proof,
            packets: vec![verified.packet],
        }
    } else if let Some(summary) = transport_identity::claim_server_frame_if_verified(&view) {
        tracing::info!(
            packet = summary.packet_name,
            reason = summary.reason,
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            flags = view.flags,
            packetized_sequence = view.packetized_sequence,
            payload_len = view.payload_length,
            "server M transport-only frame semantically claimed as verified no-op"
        );
        let owner = state
            .deflate
            .server_zlib_stream_owner
            .unwrap_or(ContinuationOwner::UnknownProxyOwned);
        match transport_identity::verified_server_packet_for_claim(
            &inbound,
            &view,
            summary,
            owner,
            state.deflate.server_zlib_stream_epoch,
            state.deflate.server_zlib_stream_proxy_owned,
        )? {
            Some(verified) => {
                observe_verified_server_m_packet(
                    state,
                    &verified.proof,
                    &verified.packet,
                    server_peer_ack_sequence,
                );
                Emit::VerifiedProofPackets {
                    proof: verified.proof,
                    packets: vec![verified.packet],
                }
            }
            None => Emit::Drop,
        }
    } else {
        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            flags = view.flags,
            packetized_sequence = view.packetized_sequence,
            payload_len = view.payload_length,
            "server M frame quarantined: no high-level translator or transport identity owner"
        );
        Emit::Drop
    };

    finalize_server_to_client_emit(state, emit, pending_count_before)
}

fn observe_verified_server_m_packet(
    state: &mut SessionState,
    proof: &VerifiedProof,
    packet: &[u8],
    server_peer_ack_sequence: u16,
) {
    let Some(view) = MFrameView::parse(packet) else {
        return;
    };
    let Some(payload) = parse_window::primary_payload(packet, &view) else {
        return;
    };
    let live_object_inventory_materialization =
        observe_verified_server_payload_semantics(state, proof, payload);
    apply_verified_server_semantic_side_effects(
        state,
        proof,
        ServerSemanticFrameContext {
            sequence: view.sequence,
            server_peer_ack_sequence,
            client_unshifted_ack_sequence: view.ack_sequence,
            live_object_inventory_materialization,
        },
    );
}

pub(super) fn observe_verified_server_payload_semantics(
    state: &mut SessionState,
    proof: &VerifiedProof,
    payload: &[u8],
) -> Option<semantic::LiveObjectInventoryMaterializationSummary> {
    crate::translate::semantic::observe_verified_payload_with_area_context_report(
        &mut state.semantic,
        crate::packet::Direction::ServerToClient,
        proof,
        payload,
        Some(&state.area_context.latest_area_placeables),
    )
    .live_object_inventory_materialization
}

pub(super) fn apply_verified_server_semantic_side_effects(
    state: &mut SessionState,
    proof: &VerifiedProof,
    frame: ServerSemanticFrameContext,
) {
    inventory_equipment::maybe_record_client_gui_status_live_object_frame_response(
        state,
        proof,
        frame.sequence,
        frame.server_peer_ack_sequence,
        frame.client_unshifted_ack_sequence,
        frame.live_object_inventory_materialization.as_ref(),
    );
    if let Err(err) = inventory_equipment::maybe_queue_confirmed_inventory_replay(
        state,
        frame.sequence,
        frame.client_unshifted_ack_sequence,
    ) {
        tracing::warn!(
            error = %err,
            sequence = frame.sequence,
            ack_sequence = frame.client_unshifted_ack_sequence,
            "failed to queue confirmed ClientGui status Inventory replay"
        );
    }
    if let Err(err) = inventory_equipment::maybe_queue_inventory_equipment_bridge_output(
        state,
        frame.sequence,
        frame.client_unshifted_ack_sequence,
    ) {
        tracing::warn!(
            error = %err,
            sequence = frame.sequence,
            ack_sequence = frame.client_unshifted_ack_sequence,
            "failed to queue inventory/equipment bridge output"
        );
    }
    update_quickbar_item_refresh_hint(state);
}

fn observe_verified_client_m_packet(
    state: &mut SessionState,
    family: VerifiedFamily,
    packet: &[u8],
) {
    let Some(view) = MFrameView::parse(packet) else {
        return;
    };
    let Some(payload) = parse_window::primary_payload(packet, &view) else {
        return;
    };
    observe_verified_client_payload(state, family, payload);
}

fn observe_verified_client_payload(
    state: &mut SessionState,
    family: VerifiedFamily,
    payload: &[u8],
) {
    let proof = VerifiedProof::family(family);
    crate::translate::semantic::observe_verified_payload(
        &mut state.semantic,
        crate::packet::Direction::ClientToServer,
        &proof,
        payload,
    );
    if family == VerifiedFamily::ClientGuiInventory {
        inventory_equipment::maybe_record_non_server_inventory_equipment_bridge_output_decision(
            state,
        );
    }
    update_quickbar_item_refresh_hint(state);
}

fn observe_verified_synthetic_server_m_packet(
    state: &mut SessionState,
    family: VerifiedFamily,
    packet: &[u8],
) {
    let Some(view) = MFrameView::parse(packet) else {
        return;
    };
    let Some(payload) = parse_window::primary_payload(packet, &view) else {
        return;
    };
    let proof = VerifiedProof::family(family);
    crate::translate::semantic::observe_verified_payload_with_area_context(
        &mut state.semantic,
        crate::packet::Direction::ServerToClientSynthetic,
        &proof,
        payload,
        Some(&state.area_context.latest_area_placeables),
    );
    update_quickbar_item_refresh_hint(state);
}

fn update_quickbar_item_refresh_hint(state: &mut SessionState) {
    // A server emit transaction is still only a reconstructed-message
    // candidate until SessionTranslator's outer strict validator accepts the
    // complete emit. Do not leak its speculative semantic state through the
    // harness hint file; acceptance flushes the final body after the snapshot
    // is discarded, while rejection restores the prior in-memory body.
    if state.deflate.ordered_successor_effect_snapshot.is_some()
        || state.client_emit_effect_snapshot.is_some()
    {
        tracing::trace!("quickbar item-refresh hint deferred behind M emit strict validation");
        return;
    }
    let Some(path) = state.quickbar_item_refresh_hint_path.clone() else {
        return;
    };
    let hint = state.semantic.quickbar_item_refresh_harness_hint();
    let no_hint_reason = if hint.is_some() {
        "none"
    } else {
        state.semantic.quickbar_item_refresh_harness_idle_reason()
    };
    let body = match hint {
        Some(hint) => hint.to_json(),
        None => state.semantic.quickbar_item_refresh_harness_idle_json(),
    };
    let body =
        augment_quickbar_item_refresh_hint_with_bridge_output(body, &state.inventory_equipment);

    if state.quickbar_item_refresh_hint_last_body.as_deref() == Some(body.as_str()) {
        return;
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(
            path = %path.display(),
            error = %err,
            "failed to create quickbar item-refresh hint directory"
        );
        return;
    }

    if let Err(err) = fs::write(&path, body.as_bytes()) {
        tracing::warn!(
            path = %path.display(),
            error = %err,
            "failed to update quickbar item-refresh hint file"
        );
        return;
    }

    state.quickbar_item_refresh_hint_last_body = Some(body);
    let candidate_object_id = hint.map(|hint| hint.candidate.object_id).unwrap_or(0);
    let candidate_proof = hint
        .map(|hint| hint.candidate.proof.as_str())
        .unwrap_or("none");
    let candidate_source = hint
        .map(|hint| hint.candidate.source.as_str())
        .unwrap_or("none");
    tracing::info!(
        path = %path.display(),
        pending_item_refresh = hint.is_some(),
        candidate_object_id,
        candidate_proof,
        candidate_source,
        no_hint_reason,
        "updated quickbar item-refresh harness hint"
    );
}

fn augment_quickbar_item_refresh_hint_with_bridge_output(
    body: String,
    bridge: &state::InventoryEquipmentBridgeState,
) -> String {
    let last = bridge.last_queued_output.unwrap_or_default();
    let last_known = bridge.last_queued_output.is_some();
    let last_client_gui_status = bridge
        .last_queued_client_gui_status_output
        .unwrap_or_default();
    let last_client_gui_status_known = bridge.last_queued_client_gui_status_output.is_some();
    let last_client_gui_status_candidate = last_client_gui_status.candidate;
    let last_client_gui_status_candidate_known = last_client_gui_status_candidate.is_some();
    let last_client_gui_status_candidate_object_id = last_client_gui_status_candidate
        .map(|candidate| candidate.object_id)
        .unwrap_or(0);
    let last_client_gui_status_candidate_proof = last_client_gui_status_candidate
        .map(|candidate| candidate.proof.as_str())
        .unwrap_or("none");
    let last_client_gui_status_candidate_source = last_client_gui_status_candidate
        .map(|candidate| candidate.source.as_str())
        .unwrap_or("none");
    let last_client_gui_status_payload_hex = if last_client_gui_status_known {
        hex_encode_upper(&client_gui_inventory::build_status_payload(
            last_client_gui_status.object_id,
            last_client_gui_status.player_inventory_gui,
        ))
    } else {
        String::new()
    };
    let last_decision = bridge.last_decision;
    let last_client_gui_status_response =
        bridge.last_client_gui_status_response.unwrap_or_default();
    let last_client_gui_status_response_known = bridge.last_client_gui_status_response.is_some();
    let last_client_gui_status_response_candidate =
        last_client_gui_status_response.compact_item_emission_ready_candidate;
    let last_client_gui_status_response_candidate_known =
        last_client_gui_status_response_candidate.is_some();
    let last_client_gui_status_response_candidate_object_id =
        last_client_gui_status_response_candidate
            .map(|candidate| candidate.object_id)
            .unwrap_or(0);
    let last_client_gui_status_response_candidate_proof = last_client_gui_status_response_candidate
        .map(|candidate| candidate.proof.as_str())
        .unwrap_or("none");
    let last_client_gui_status_response_candidate_source =
        last_client_gui_status_response_candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
    let best_client_gui_status_response =
        bridge.best_client_gui_status_response.unwrap_or_default();
    let best_client_gui_status_response_known = bridge.best_client_gui_status_response.is_some();
    let best_client_gui_status_response_candidate =
        best_client_gui_status_response.compact_item_emission_ready_candidate;
    let best_client_gui_status_response_candidate_known =
        best_client_gui_status_response_candidate.is_some();
    let best_client_gui_status_response_candidate_object_id =
        best_client_gui_status_response_candidate
            .map(|candidate| candidate.object_id)
            .unwrap_or(0);
    let best_client_gui_status_response_candidate_proof = best_client_gui_status_response_candidate
        .map(|candidate| candidate.proof.as_str())
        .unwrap_or("none");
    let best_client_gui_status_response_candidate_source =
        best_client_gui_status_response_candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
    let client_gui_status_response_outcome = bridge.client_gui_status_response_outcome();
    let client_gui_status_request_completion = bridge.client_gui_status_request_completion();
    let best_client_gui_status_response_association =
        bridge.best_client_gui_status_response_association();
    let client_gui_status_refresh_confirmed = bridge.client_gui_status_refresh_confirmed();
    let best_client_gui_status_response_matches_queued_status_candidate =
        best_client_gui_status_response_association
            == state::InventoryEquipmentBridgeClientGuiStatusResponseAssociation::MatchesQueuedStatusCandidate;
    let best_client_gui_status_response_candidate_delta =
        bridge.best_client_gui_status_response_candidate_delta_from_queued_status();
    let last_decision_known = last_decision.is_some();
    let last_decision_kind = last_decision
        .map(|decision| decision.kind)
        .unwrap_or_default();
    let last_decision_update_index = last_decision
        .map(|decision| decision.update_index)
        .or(bridge.last_decision_state_update_index)
        .unwrap_or(0);
    let last_decision_consumer = last_decision
        .map(|decision| decision.consumer.as_str())
        .unwrap_or("unknown");
    let last_decision_emission_index = last_decision
        .map(|decision| decision.emission_index)
        .unwrap_or(0);
    let last_decision_event_index = last_decision
        .map(|decision| decision.event_index)
        .unwrap_or(0);
    let last_decision_candidate = last_decision.map(|decision| decision.candidate);
    let last_decision_candidate_object_id = last_decision_candidate
        .map(|candidate| candidate.object_id)
        .unwrap_or(0);
    let last_decision_candidate_proof = last_decision_candidate
        .map(|candidate| candidate.proof.as_str())
        .unwrap_or("none");
    let last_decision_candidate_source = last_decision_candidate
        .map(|candidate| candidate.source.as_str())
        .unwrap_or("none");
    let last_decision_candidate_object_status = last_decision
        .map(|decision| decision.candidate_object_status.as_str())
        .unwrap_or("unknown");
    let last_decision_candidate_object_status_proof = last_decision
        .and_then(|decision| decision.candidate_object_status.proof())
        .map(|proof| proof.as_str())
        .unwrap_or("none");
    let last_decision_ready_objects = last_decision
        .map(|decision| decision.ready_objects)
        .unwrap_or(0);
    let last_decision_deferred_feature25_only_objects = last_decision
        .map(|decision| decision.deferred_feature25_only_objects)
        .unwrap_or(0);
    let last_decision_claim = last_decision.and_then(|decision| decision.server_inventory_claim);
    let last_decision_claim_known = last_decision_claim.is_some();
    let last_decision_claim_minor = last_decision_claim.map(|claim| claim.minor).unwrap_or(0);
    let last_decision_claim_object_id = last_decision_claim
        .map(|claim| claim.object_id)
        .unwrap_or(0);
    let last_decision_claim_result = last_decision_claim
        .map(|claim| claim.result)
        .unwrap_or(false);
    let last_decision_claim_equip_slot = last_decision_claim
        .map(|claim| claim.equip_slot)
        .unwrap_or(0);
    let last_decision_claim_object_status = last_decision
        .map(|decision| decision.server_inventory_claim_object_status.as_str())
        .unwrap_or("unknown");
    let last_decision_claim_object_status_proof = last_decision
        .and_then(|decision| decision.server_inventory_claim_object_status.proof())
        .map(|proof| proof.as_str())
        .unwrap_or("none");
    let last_decision_claim_proven_neighborhood = last_decision
        .map(|decision| decision.server_inventory_claim_proven_neighborhood)
        .unwrap_or_default();
    let last_decision_claim_lower_proven_neighbor = last_decision_claim_proven_neighborhood
        .lower
        .unwrap_or_default();
    let last_decision_claim_lower_proven_neighbor_known =
        last_decision_claim_proven_neighborhood.lower.is_some();
    let last_decision_claim_higher_proven_neighbor = last_decision_claim_proven_neighborhood
        .higher
        .unwrap_or_default();
    let last_decision_claim_higher_proven_neighbor_known =
        last_decision_claim_proven_neighborhood.higher.is_some();
    let last_decision_claim_closest_proven_neighbor = last_decision_claim_proven_neighborhood
        .closest()
        .unwrap_or_default();
    let last_decision_claim_closest_proven_neighbor_known =
        last_decision_claim_proven_neighborhood.closest().is_some();
    let last_decision_client_gui_claim =
        last_decision.and_then(|decision| decision.client_gui_inventory_claim);
    let last_decision_client_gui_claim_known = last_decision_client_gui_claim.is_some();
    let last_decision_client_gui_claim_kind = last_decision_client_gui_claim
        .map(|claim| claim.kind.as_str())
        .unwrap_or("none");
    let last_decision_client_gui_claim_object_id = last_decision_client_gui_claim
        .and_then(|claim| claim.object_id)
        .unwrap_or(0);
    let last_decision_client_gui_claim_panel = last_decision_client_gui_claim
        .and_then(|claim| claim.panel)
        .unwrap_or(0);
    let last_decision_client_gui_claim_player_inventory_gui = last_decision_client_gui_claim
        .and_then(|claim| claim.player_inventory_gui)
        .unwrap_or(false);
    let last_decision_client_gui_claim_rewritten_self_object_id =
        last_decision_client_gui_claim.is_some_and(|claim| claim.rewritten_self_object_id);
    let client_gui_writer_plan = client_gui_writer_plan_for_decision(last_decision);
    let output_status = bridge.output_status();
    let requires_client_gui_writer = bridge.requires_client_gui_writer();
    let fields = format!(
        concat!(
            ",\n",
            "  \"inventory_equipment_bridge_output_status\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_requires_client_gui_writer\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_request_completion\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_request_acknowledged\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_request_acknowledgements\": {},\n",
            "  \"inventory_equipment_bridge_output_last_acknowledged_client_gui_status_server_ack_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_pre_ack_live_object_packets_ignored\": {},\n",
            "  \"inventory_equipment_bridge_output_last_pre_ack_client_gui_status_live_object_server_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_pre_ack_client_gui_status_live_object_server_ack_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_refresh_confirmed\": {},\n",
            "  \"inventory_equipment_bridge_output_queued_packets\": {},\n",
            "  \"inventory_equipment_bridge_output_confirmed_inventory_replay_packets\": {},\n",
            "  \"inventory_equipment_bridge_output_confirmed_inventory_replay_pending\": {},\n",
            "  \"inventory_equipment_bridge_output_last_confirmed_inventory_replay_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_confirmed_inventory_replay_dispatched_packets\": {},\n",
            "  \"inventory_equipment_bridge_output_confirmed_inventory_replay_queued_for_dispatch\": {},\n",
            "  \"inventory_equipment_bridge_output_last_confirmed_inventory_replay_dispatch_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_deferred_client_gui_updates\": {},\n",
            "  \"inventory_equipment_bridge_output_deferred_missing_claim_updates\": {},\n",
            "  \"inventory_equipment_bridge_output_blocked_candidate_mismatch_updates\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_reason\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_consumer\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_emission_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_event_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_candidate_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_candidate_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_candidate_proof\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_candidate_source\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_candidate_object_status\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_candidate_object_status_proof\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_ready_objects\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_deferred_feature25_only_objects\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_minor\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_status\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_status_proof\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_distance\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_distance\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_distance\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_result\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_server_inventory_claim_equip_slot\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_kind\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_panel\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_player_inventory_gui\": {},\n",
            "  \"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_rewritten_self_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_action\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_emission_enabled\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_blocked_reason\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_payload_available\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_payload_kind\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_payload_hex\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_status_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_status_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_status_object_is_current_player\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_select_panel\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_writer_plan_player_inventory_gui\": {},\n",
            "  \"inventory_equipment_bridge_output_last_deferred_client_gui_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_deferred_missing_claim_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_blocked_candidate_mismatch_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_emission_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_event_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_minor\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_queued_result\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_equip_slot\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_trigger_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_synthetic_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_queued_client_gui_status_packets\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_emission_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_event_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_player_inventory_gui\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_payload_hex\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_trigger_client_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_synthetic_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_ack_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_proof\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_source\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_ready_objects\": {},\n",
            "  \"inventory_equipment_bridge_output_last_queued_client_gui_status_deferred_feature25_only_objects\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_response_live_object_packets\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_response_live_gui_record_packets\": {},\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_response_materialized_item_packets\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_queued_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_server_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_server_peer_ack_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_ack_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_live_gui_records\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_live_gui_fragment_bits\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_ids\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_first\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_first_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_last\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_last_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_min\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_min_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_max\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_max_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_ids_contain_queued_candidate\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_ready_objects\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_candidate_known\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_candidate_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_candidate_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_candidate_proof\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_last_client_gui_status_response_candidate_source\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_client_gui_status_response_outcome\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_known\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_queued_update_index\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_server_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_server_peer_ack_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_ack_sequence\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_live_gui_records\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_live_gui_fragment_bits\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_ids\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_first\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_first_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_last\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_last_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_min\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_min_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_max\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_max_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_ids_contain_queued_candidate\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_ready_objects\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_known\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_object_id\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_object_id_hex\": \"0x{:08X}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_proof\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_source\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_association\": \"{}\",\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_matches_queued_status_candidate\": {},\n",
            "  \"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_delta_from_queued_status_candidate\": {}\n"
        ),
        output_status.as_str(),
        requires_client_gui_writer,
        client_gui_status_request_completion.as_str(),
        bridge.client_gui_status_request_acknowledged(),
        bridge.client_gui_status_request_acknowledgements,
        bridge
            .last_acknowledged_client_gui_status_server_ack_sequence
            .unwrap_or(0),
        bridge.client_gui_status_pre_ack_live_object_packets_ignored,
        bridge
            .last_pre_ack_client_gui_status_live_object_server_sequence
            .unwrap_or(0),
        bridge
            .last_pre_ack_client_gui_status_live_object_server_ack_sequence
            .unwrap_or(0),
        client_gui_status_refresh_confirmed,
        bridge.queued_outputs,
        bridge.confirmed_inventory_replay_outputs,
        bridge.pending_confirmed_inventory_replay.is_some(),
        bridge
            .last_confirmed_inventory_replay_update_index
            .unwrap_or(0),
        bridge.confirmed_inventory_replay_dispatches,
        bridge.confirmed_inventory_replay_queued_for_dispatch(),
        bridge
            .last_confirmed_inventory_replay_dispatch_update_index
            .unwrap_or(0),
        bridge.deferred_client_gui_updates,
        bridge.deferred_missing_claim_updates,
        bridge.blocked_candidate_mismatch_updates,
        last_decision_update_index,
        last_decision_known,
        last_decision_kind.as_str(),
        last_decision_consumer,
        last_decision_emission_index,
        last_decision_event_index,
        last_decision_candidate_object_id,
        last_decision_candidate_object_id,
        last_decision_candidate_proof,
        last_decision_candidate_source,
        last_decision_candidate_object_status,
        last_decision_candidate_object_status_proof,
        last_decision_ready_objects,
        last_decision_deferred_feature25_only_objects,
        last_decision_claim_known,
        last_decision_claim_minor,
        last_decision_claim_object_id,
        last_decision_claim_object_id,
        last_decision_claim_object_status,
        last_decision_claim_object_status_proof,
        last_decision_claim_closest_proven_neighbor_known,
        last_decision_claim_closest_proven_neighbor.object_id,
        last_decision_claim_closest_proven_neighbor.object_id,
        last_decision_claim_closest_proven_neighbor.distance,
        last_decision_claim_lower_proven_neighbor_known,
        last_decision_claim_lower_proven_neighbor.object_id,
        last_decision_claim_lower_proven_neighbor.object_id,
        last_decision_claim_lower_proven_neighbor.distance,
        last_decision_claim_higher_proven_neighbor_known,
        last_decision_claim_higher_proven_neighbor.object_id,
        last_decision_claim_higher_proven_neighbor.object_id,
        last_decision_claim_higher_proven_neighbor.distance,
        last_decision_claim_result,
        last_decision_claim_equip_slot,
        last_decision_client_gui_claim_known,
        last_decision_client_gui_claim_kind,
        last_decision_client_gui_claim_object_id,
        last_decision_client_gui_claim_object_id,
        last_decision_client_gui_claim_panel,
        last_decision_client_gui_claim_player_inventory_gui,
        last_decision_client_gui_claim_rewritten_self_object_id,
        client_gui_writer_plan.action,
        client_gui_writer_plan.emission_enabled,
        client_gui_writer_plan.blocked_reason,
        client_gui_writer_plan.payload_available,
        client_gui_writer_plan.payload_kind,
        client_gui_writer_plan.payload_hex,
        client_gui_writer_plan.status_object_id,
        client_gui_writer_plan.status_object_id,
        client_gui_writer_plan.status_object_is_current_player,
        client_gui_writer_plan.select_panel,
        client_gui_writer_plan.player_inventory_gui,
        bridge.last_deferred_client_gui_update_index.unwrap_or(0),
        bridge.last_deferred_missing_claim_update_index.unwrap_or(0),
        bridge
            .last_blocked_candidate_mismatch_update_index
            .unwrap_or(0),
        last_known,
        last.update_index,
        last.emission_index,
        last.event_index,
        last.minor,
        last.object_id,
        last.object_id,
        last.result,
        last.equip_slot,
        last.trigger_sequence,
        last.synthetic_sequence,
        bridge.queued_client_gui_status_outputs,
        last_client_gui_status_known,
        last_client_gui_status.update_index,
        last_client_gui_status.emission_index,
        last_client_gui_status.event_index,
        last_client_gui_status.object_id,
        last_client_gui_status.object_id,
        last_client_gui_status.player_inventory_gui,
        last_client_gui_status_payload_hex,
        last_client_gui_status.trigger_client_sequence,
        last_client_gui_status.synthetic_sequence,
        last_client_gui_status.ack_sequence,
        last_client_gui_status_candidate_known,
        last_client_gui_status_candidate_object_id,
        last_client_gui_status_candidate_object_id,
        last_client_gui_status_candidate_proof,
        last_client_gui_status_candidate_source,
        last_client_gui_status.ready_objects,
        last_client_gui_status.deferred_feature25_only_objects,
        bridge.client_gui_status_response_live_object_packets,
        bridge.client_gui_status_response_live_gui_record_packets,
        bridge.client_gui_status_response_materialized_item_packets,
        last_client_gui_status_response_known,
        last_client_gui_status_response.queued_update_index,
        last_client_gui_status_response.server_sequence,
        last_client_gui_status_response.server_peer_ack_sequence,
        last_client_gui_status_response.ack_sequence,
        last_client_gui_status_response.live_gui_records,
        last_client_gui_status_response.live_gui_fragment_bits,
        last_client_gui_status_response.materialized_item_object_ids,
        last_client_gui_status_response.materialized_item_object_id_first,
        last_client_gui_status_response.materialized_item_object_id_first,
        last_client_gui_status_response.materialized_item_object_id_last,
        last_client_gui_status_response.materialized_item_object_id_last,
        last_client_gui_status_response.materialized_item_object_id_min,
        last_client_gui_status_response.materialized_item_object_id_min,
        last_client_gui_status_response.materialized_item_object_id_max,
        last_client_gui_status_response.materialized_item_object_id_max,
        last_client_gui_status_response.materialized_item_object_ids_contain_queued_candidate,
        last_client_gui_status_response.compact_item_emission_ready_objects,
        last_client_gui_status_response_candidate_known,
        last_client_gui_status_response_candidate_object_id,
        last_client_gui_status_response_candidate_object_id,
        last_client_gui_status_response_candidate_proof,
        last_client_gui_status_response_candidate_source,
        client_gui_status_response_outcome.as_str(),
        best_client_gui_status_response_known,
        best_client_gui_status_response.queued_update_index,
        best_client_gui_status_response.server_sequence,
        best_client_gui_status_response.server_peer_ack_sequence,
        best_client_gui_status_response.ack_sequence,
        best_client_gui_status_response.live_gui_records,
        best_client_gui_status_response.live_gui_fragment_bits,
        best_client_gui_status_response.materialized_item_object_ids,
        best_client_gui_status_response.materialized_item_object_id_first,
        best_client_gui_status_response.materialized_item_object_id_first,
        best_client_gui_status_response.materialized_item_object_id_last,
        best_client_gui_status_response.materialized_item_object_id_last,
        best_client_gui_status_response.materialized_item_object_id_min,
        best_client_gui_status_response.materialized_item_object_id_min,
        best_client_gui_status_response.materialized_item_object_id_max,
        best_client_gui_status_response.materialized_item_object_id_max,
        best_client_gui_status_response.materialized_item_object_ids_contain_queued_candidate,
        best_client_gui_status_response.compact_item_emission_ready_objects,
        best_client_gui_status_response_candidate_known,
        best_client_gui_status_response_candidate_object_id,
        best_client_gui_status_response_candidate_object_id,
        best_client_gui_status_response_candidate_proof,
        best_client_gui_status_response_candidate_source,
        best_client_gui_status_response_association.as_str(),
        best_client_gui_status_response_matches_queued_status_candidate,
        best_client_gui_status_response_candidate_delta
    );
    if let Some(prefix) = body.strip_suffix("\n}\n") {
        format!("{prefix}{fields}}}\n")
    } else if let Some(prefix) = body.strip_suffix('}') {
        format!("{prefix}{fields}}}")
    } else {
        body
    }
}

struct ClientGuiWriterPlan {
    action: &'static str,
    emission_enabled: bool,
    blocked_reason: &'static str,
    payload_available: bool,
    payload_kind: &'static str,
    payload_hex: String,
    status_object_id: u32,
    status_object_is_current_player: bool,
    select_panel: u8,
    player_inventory_gui: bool,
}

impl Default for ClientGuiWriterPlan {
    fn default() -> Self {
        Self {
            action: "none",
            emission_enabled: false,
            blocked_reason: "none",
            payload_available: false,
            payload_kind: "none",
            payload_hex: String::new(),
            status_object_id: 0,
            status_object_is_current_player: false,
            select_panel: 0,
            player_inventory_gui: false,
        }
    }
}

fn client_gui_writer_plan_for_decision(
    decision: Option<state::InventoryEquipmentBridgeOutputDecision>,
) -> ClientGuiWriterPlan {
    let claim = decision.and_then(|decision| decision.client_gui_inventory_claim);
    let Some(claim) = claim else {
        return ClientGuiWriterPlan::default();
    };
    let decision_kind = decision
        .map(|decision| decision.kind)
        .unwrap_or(state::InventoryEquipmentBridgeOutputDecisionKind::None);

    match claim.kind {
        crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaimKind::Status => {
            let Some(object_id) = claim.object_id else {
                return ClientGuiWriterPlan {
                    action: "status_missing_object",
                    blocked_reason: "client_gui_inventory_status_object_missing",
                    ..ClientGuiWriterPlan::default()
                };
            };
            let player_inventory_gui = claim.player_inventory_gui.unwrap_or(true);
            let payload =
                client_gui_inventory::build_status_payload(object_id, player_inventory_gui);
            let status_object_is_current_player =
                object_id == client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID;
            let emission_enabled = status_object_is_current_player
                && decision_kind
                    == state::InventoryEquipmentBridgeOutputDecisionKind::QueuedClientGuiStatusOutput;
            ClientGuiWriterPlan {
                action: if status_object_is_current_player {
                    "status_current_player_inventory"
                } else {
                    "status_other_object_inventory"
                },
                emission_enabled,
                blocked_reason: if emission_enabled {
                    "none"
                } else if status_object_is_current_player {
                    "client_gui_inventory_status_not_queued"
                } else {
                    "client_gui_inventory_status_not_current_player"
                },
                payload_available: true,
                payload_kind: "GuiInventory_Status",
                payload_hex: hex_encode_upper(&payload),
                status_object_id: object_id,
                status_object_is_current_player,
                player_inventory_gui,
                ..ClientGuiWriterPlan::default()
            }
        }
        crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaimKind::SelectPanel => {
            let (Some(panel), Some(player_inventory_gui)) =
                (claim.panel, claim.player_inventory_gui)
            else {
                return ClientGuiWriterPlan {
                    action: "select_panel_missing_fields",
                    blocked_reason: "client_gui_inventory_select_panel_fields_missing",
                    ..ClientGuiWriterPlan::default()
                };
            };
            let payload =
                client_gui_inventory::build_select_panel_payload(panel, player_inventory_gui);
            ClientGuiWriterPlan {
                action: "select_panel",
                emission_enabled: false,
                blocked_reason: "client_gui_inventory_status_required_before_select_panel",
                payload_available: true,
                payload_kind: "GuiInventory_SelectPanel",
                payload_hex: hex_encode_upper(&payload),
                select_panel: panel,
                player_inventory_gui,
                ..ClientGuiWriterPlan::default()
            }
        }
    }
}

fn hex_encode_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

fn observe_quickbar_stream_probe_from_rewrite(
    state: &mut SessionState,
    rewrite: &server_dispatch::InflatedPayloadRewrite,
) {
    let (Some(summary), Some(materialization_context)) = (
        rewrite.quickbar_stream_probe_summary.as_ref(),
        rewrite.quickbar_stream_probe_materialization_context,
    ) else {
        return;
    };
    state
        .semantic
        .ui
        .observe_quickbar_stream_probe(summary, materialization_context);
    let verified_proof = rewrite.verified_proof();
    let promoted_committed_profile = if !verified_proof.contains_family(VerifiedFamily::GuiQuickbar)
    {
        state
            .semantic
            .ui
            .promote_quickbar_stream_probe_profile(summary, materialization_context)
    } else {
        false
    };
    tracing::info!(
        probes = state.semantic.ui.quickbar_stream_probe_summaries,
        item_buttons_seen = summary.item_buttons_seen,
        item_buttons_source_compact = summary.item_buttons_source_compact,
        item_buttons_preserved = summary.item_buttons_preserved,
        item_buttons_rejected_missing_state_proof =
            summary.item_buttons_rejected_missing_state_proof,
        direct_item_proof_objects = materialization_context.direct_item_proof_objects,
        feature25_item_proof_objects = materialization_context.feature25_item_proof_objects,
        compact_item_emission_proof_objects =
            materialization_context.compact_item_emission_proof_objects,
        validated_slot_profile = summary.validated_slot_profile.is_some(),
        promoted_committed_profile,
        "semantic state observed stream-probe GuiQuickbar summary"
    );
    update_quickbar_item_refresh_hint(state);
}

fn observe_client_window_state(
    state: &mut SessionState,
    view: &MFrameView,
    source_sequence_accepted: bool,
) {
    // Diamond `sub_5F3940` (751460-751763) and EE `FrameReceive`
    // (878825-879146) select the data lane from frame type and explicitly wrap
    // its cursor from FFFF to 0000. Controls never advance that cursor, even if
    // their otherwise-unused sequence field is nonzero.
    if source_sequence_accepted && view.frame_kind() == Some(MFrameType::ReliableData) {
        record_forward_progress(
            &mut state.sequence.latest_client_sequence_from_client,
            view.sequence,
        );
    }
    record_forward_progress(
        &mut state.sequence.latest_client_ack_from_client,
        view.ack_sequence,
    );
}

fn shift_client_sequence_for_server(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if view.frame_kind() != Some(MFrameType::ReliableData)
        || (state.sequence.client_sequence_shifts.is_empty()
            && state.sequence.client_sequence_elisions.is_empty())
    {
        return Ok(());
    }

    let Some(shifted) = shift_sequence_for_peer_with_elisions(
        &state.sequence.client_sequence_shifts,
        &state.sequence.client_sequence_elisions,
        view.sequence,
    ) else {
        anyhow::bail!(
            "client M sequence {} was already claimed as proxy-owned and cannot be forwarded",
            view.sequence
        );
    };
    if shifted == view.sequence {
        return Ok(());
    }

    write_be_u16(packet, 3, shifted)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to shift client M sequence"))?;
    encode_legacy_m_crc(packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair shifted client M CRC"))?;
    tracing::info!(
        sequence = view.sequence,
        shifted_sequence = shifted,
        ack_sequence = view.ack_sequence,
        shifts = state.sequence.client_sequence_shifts.len(),
        elisions = state.sequence.client_sequence_elisions.len(),
        "client M sequence shifted after proxy-owned client M transport transform"
    );
    Ok(())
}

fn unshift_server_ack_for_client(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if state.sequence.client_sequence_shifts.is_empty()
        && state.sequence.client_sequence_elisions.is_empty()
    {
        return Ok(());
    }

    let unshifted = unshift_ack_for_origin_with_elisions(
        &state.sequence.client_sequence_shifts,
        &state.sequence.client_sequence_elisions,
        view.ack_sequence,
    );
    if unshifted == view.ack_sequence {
        return Ok(());
    }

    write_be_u16(packet, 5, unshifted)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to unshift server M ack"))?;
    encode_legacy_m_crc(packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair unshifted server M CRC"))?;
    tracing::info!(
        ack_sequence = view.ack_sequence,
        unshifted_ack_sequence = unshifted,
        server_sequence = view.sequence,
        shifts = state.sequence.client_sequence_shifts.len(),
        elisions = state.sequence.client_sequence_elisions.len(),
        "server M ack unshifted after proxy-owned client M transport transform"
    );
    Ok(())
}

fn unshift_client_ack_for_server(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if state.sequence.server_sequence_shifts.is_empty() {
        return Ok(());
    }

    let unshifted =
        unshift_ack_for_origin(&state.sequence.server_sequence_shifts, view.ack_sequence);
    if unshifted == view.ack_sequence {
        return Ok(());
    }

    write_be_u16(packet, 5, unshifted)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to unshift client M ack"))?;
    encode_legacy_m_crc(packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair unshifted client M CRC"))?;
    tracing::info!(
        ack_sequence = view.ack_sequence,
        unshifted_ack_sequence = unshifted,
        client_sequence = view.sequence,
        shifts = state.sequence.server_sequence_shifts.len(),
        "client M ack unshifted after proxy-owned server M insertion"
    );
    Ok(())
}

fn queue_area_client_area_side_effects(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    summary: &area::AreaRewriteSummary,
) -> anyhow::Result<()> {
    let (Some(first_frame), Some(last_frame)) =
        (reassembly.frames.first(), reassembly.frames.last())
    else {
        return Ok(());
    };

    queue_area_client_area_side_effects_for_window(
        state,
        first_frame.sequence,
        last_frame.sequence,
        last_frame.ack_sequence,
        summary,
    )
}

fn queue_area_client_area_side_effects_for_window(
    state: &mut SessionState,
    original_first_sequence: u16,
    original_last_sequence: u16,
    ack_sequence: u16,
    summary: &area::AreaRewriteSummary,
) -> anyhow::Result<()> {
    let fallback_reason = synthetic_area::fallback_reason_for_area_rewrite(summary);
    synthetic_area::queue_loadbar_and_area_loaded_fallback(
        &mut state.synthetic_area.pending_server_to_client_packets,
        &mut state.synthetic_area.pending_area_loaded,
        &mut state.sequence.server_sequence_shifts,
        original_first_sequence,
        original_last_sequence,
        ack_sequence,
        fallback_reason,
        state.synthetic_area.synthesize_loadbar,
    )?;
    let area_first_client_sequence = shift_sequence_for_peer(
        &state.sequence.server_sequence_shifts,
        original_first_sequence,
    );
    let area_release_client_ack_sequence = shift_sequence_for_peer(
        &state.sequence.server_sequence_shifts,
        original_last_sequence,
    );

    // The post-Area_ClientArea stream is server-authored gameplay state, but the
    // EE client decompile routes native Area_AreaLoaded only after the area
    // reader returns. The proxy cannot see that in-process return, so it keeps a
    // narrow transport split: release the rewritten Area_ClientArea window and
    // proxy-owned area-load UI/control packets, then hold live-object/quickbar
    // gameplay until native Area_AreaLoaded or the audited fallback proves the
    // same semantic boundary. A 2026-05-18 Starcore5 Docks run showed that
    // releasing post-area gameplay on ACK grace alone can leave the world view
    // black while the UI and reliable stream continue.
    synthetic_area::arm_server_hold_gate_after_area_release(
        &mut state.synthetic_area.server_hold_gate,
        area_first_client_sequence,
        area_release_client_ack_sequence,
        fallback_reason,
    );

    Ok(())
}

fn apply_direct_area_rewrite_side_effects(
    state: &mut SessionState,
    sequence: u16,
    ack_sequence: u16,
    summary: Option<&area::AreaRewriteSummary>,
) -> anyhow::Result<()> {
    let Some(summary) = summary else {
        return Ok(());
    };
    state.area_context.latest_area_placeables = summary.placeable_context.clone();
    queue_area_client_area_side_effects_for_window(state, sequence, sequence, ack_sequence, summary)
}

fn commit_direct_server_semantic_rewrite(
    state: &mut SessionState,
    inbound: &[u8],
    view: &MFrameView,
    server_peer_ack_sequence: u16,
    server_origin_generation: u64,
    rewrite: server_dispatch::DirectFrameRewrite,
) -> anyhow::Result<VerifiedPacket> {
    let server_dispatch::DirectFrameRewrite {
        verified,
        area_rewrite,
        source_payload,
    } = rewrite;
    apply_direct_area_rewrite_side_effects(
        state,
        view.sequence,
        view.ack_sequence,
        area_rewrite.as_ref(),
    )?;
    let cache_source = exact_direct_semantic_source_payload(inbound, view)
        .filter(|payload| source_payload.as_deref() == Some(*payload))
        .and_then(|payload| {
            transport_identity::server_reliable_data_transport_identity(inbound, view)
                .map(|identity| (payload.to_vec(), identity))
        });
    login_waypoint::maybe_queue_empty_waypoint_response(state, inbound, view)?;
    observe_verified_server_m_packet(
        state,
        &verified.proof,
        &verified.packet,
        server_peer_ack_sequence,
    );
    if let Some((source_payload, source_transport_identity)) = cache_source {
        remember_completed_direct_server_semantic_rewrite(
            state,
            view.sequence,
            server_origin_generation,
            source_transport_identity,
            source_payload,
            &verified,
        );
    }
    Ok(verified)
}

fn exact_direct_semantic_source_payload<'a>(
    bytes: &'a [u8],
    view: &MFrameView,
) -> Option<&'a [u8]> {
    if view.frame_type != 0
        || view.trailing_payload_length != 0
        || view.packetized_sequence != 1
        || view.deflated.is_some()
        || view.high.is_none()
    {
        return None;
    }
    parse_window::primary_payload(bytes, view)
}

fn replay_completed_direct_server_semantic_rewrite(
    bytes: &[u8],
    view: &MFrameView,
    origin_generation: u64,
    state: &mut SessionState,
) -> anyhow::Result<Option<VerifiedPacket>> {
    let Some(source_payload) = exact_direct_semantic_source_payload(bytes, view) else {
        return Ok(None);
    };
    let source_transport_identity =
        transport_identity::server_reliable_data_transport_identity(bytes, view)
            .ok_or_else(|| anyhow::anyhow!("direct semantic replay left type-0 data lane"))?;
    let Some(entry) = state
        .direct_server_semantic_replays
        .completed
        .iter()
        .find(|entry| {
            entry.sequence == view.sequence && entry.origin_generation == origin_generation
        })
        .cloned()
    else {
        return Ok(None);
    };
    if entry.source_payload.as_slice() != source_payload
        || entry.source_transport_identity != source_transport_identity
    {
        anyhow::bail!(
            "server direct M reliable slot {} generation {} carried different immutable transport bytes",
            view.sequence,
            origin_generation
        );
    }
    let packet = parse_window::replace_primary_payload_and_repair(
        bytes,
        view,
        &entry.rewritten_payload,
        "cached direct semantic high-level payload",
    )?;
    state.direct_server_semantic_replays.duplicates_replayed = state
        .direct_server_semantic_replays
        .duplicates_replayed
        .saturating_add(1);
    tracing::info!(
        sequence = view.sequence,
        origin_generation,
        ack_sequence = view.ack_sequence,
        source_payload_len = source_payload.len(),
        rewritten_payload_len = entry.rewritten_payload.len(),
        proof = entry.proof.as_str(),
        "server direct M semantic rewrite replayed from exact bounded cache without semantic effects"
    );
    Ok(Some(VerifiedPacket {
        proof: entry.proof,
        packet,
    }))
}

fn remember_completed_direct_server_semantic_rewrite(
    state: &mut SessionState,
    sequence: u16,
    origin_generation: u64,
    source_transport_identity: Vec<u8>,
    source_payload: Vec<u8>,
    verified: &VerifiedPacket,
) {
    let Some(view) = MFrameView::parse(&verified.packet) else {
        return;
    };
    let Some(rewritten_payload) = exact_direct_semantic_source_payload(&verified.packet, &view)
    else {
        return;
    };
    if state
        .direct_server_semantic_replays
        .completed
        .iter()
        .any(|entry| entry.sequence == sequence && entry.origin_generation == origin_generation)
    {
        return;
    }
    state.direct_server_semantic_replays.completed.push_back(
        state::CompletedDirectServerSemanticRewrite {
            sequence,
            origin_generation,
            source_transport_identity,
            source_payload,
            rewritten_payload: rewritten_payload.to_vec(),
            proof: verified.proof.clone(),
        },
    );
    while state.direct_server_semantic_replays.completed.len()
        > MAX_COMPLETED_DIRECT_SERVER_SEMANTIC_REWRITES
    {
        state.direct_server_semantic_replays.completed.pop_front();
    }
}

/// Resolve reliable frames that arrived after a packetized deflated window.
///
/// `CNetLayerWindow::FrameReceive` preserves source sequence order even when
/// datagrams arrive out of order. Mirror that contract here: no interleaved
/// semantic packet is translated while its predecessor is unresolved. Once the
/// deflated window has an exact successful disposition, translate at most the
/// first contiguous direct semantic event against the newly committed state.
/// Later events are withheld for reliable retransmission: an Area commit can
/// insert synthetic reliable slots, and replaying a whole buffered suffix in
/// the same emit batch would put those slots after packets whose sequence was
/// shifted past them. This bounded transaction is particularly important for
/// `Area_ClientArea`: its placeable context and object-registry reset commit
/// exactly once before any following event is retried.
fn resolve_buffered_interleaved_server_packets_after_success(
    state: &mut SessionState,
    reassembly: &mut ServerDeflatedReassembly,
) -> anyhow::Result<()> {
    if reassembly.interleaved_events.is_empty() {
        return Ok(());
    }
    if reassembly.interleaved_events.len() != reassembly.interleaved_packets.len() {
        anyhow::bail!("interleaved reassembly event/placeholder count mismatch");
    }

    let events = std::mem::take(&mut reassembly.interleaved_events);
    let fallbacks = std::mem::take(&mut reassembly.interleaved_packets);
    merge_ordered_server_successor_events(state, &events)?;
    let final_sequence = events.last().map(|event| event.sequence);
    if let Some(active_sequence) = state.deflate.ordered_successor_next_sequence {
        // A predecessor which is itself being retried under the ordered fence
        // already owns this callback's validation token. Keep any nested raw
        // successors queued for their own retransmission instead of trying to
        // commit two independent reliable identities through one token.
        if let Some(candidate_final) = final_sequence {
            let current_final = state
                .deflate
                .ordered_successor_final_sequence
                .unwrap_or(active_sequence);
            state.deflate.ordered_successor_final_sequence = Some(
                [current_final, candidate_final]
                    .into_iter()
                    .max_by_key(|sequence| sequence.wrapping_sub(active_sequence))
                    .unwrap_or(candidate_final),
            );
        }
        tracing::info!(
            active_sequence,
            queued_events = state.deflate.ordered_successor_events.len(),
            "nested buffered server successors retained for separate ordered validation transactions"
        );
        return Ok(());
    }
    let mut resolved = Vec::with_capacity(1);
    let next_distance = reassembly.expected_frames;

    for (event, _fallback) in events.into_iter().zip(fallbacks) {
        let event_sequence = event.sequence;
        let event_server_origin_generation = event.server_origin_generation;
        let event_transport_payload_identity = event.transport_payload_identity.clone();
        let distance = event.sequence.wrapping_sub(reassembly.first_sequence) as usize;
        if distance != next_distance {
            arm_ordered_server_successor_fence(
                state,
                advance_reliable_sequence(reassembly.first_sequence, next_distance),
                final_sequence.unwrap_or(event.sequence),
            );
            tracing::warn!(
                sequence = event.sequence,
                first_sequence = reassembly.first_sequence,
                expected_distance = next_distance,
                actual_distance = distance,
                "gapped interleaved server M event withheld for reliable retransmission"
            );
            break;
        }

        // The packet was stored after client-ACK unshifting at arrival. Restore
        // the raw server ACK first, then recompute the EE-facing ACK against any
        // client-sequence shifts committed by the predecessor transaction.
        let mut inbound = event.packet;
        if !write_be_u16(&mut inbound, 5, event.server_peer_ack_sequence)
            || !encode_legacy_m_crc(&mut inbound)
        {
            arm_ordered_server_successor_fence(
                state,
                event.sequence,
                final_sequence.unwrap_or(event.sequence),
            );
            break;
        }
        let Some(raw_view) = MFrameView::parse(&inbound) else {
            arm_ordered_server_successor_fence(
                state,
                event.sequence,
                final_sequence.unwrap_or(event.sequence),
            );
            break;
        };
        unshift_server_ack_for_client(state, &mut inbound, &raw_view)?;
        let Some(view) = MFrameView::parse(&inbound) else {
            arm_ordered_server_successor_fence(
                state,
                event.sequence,
                final_sequence.unwrap_or(event.sequence),
            );
            break;
        };
        if !view.crc_valid
            || view.sequence != event.sequence
            || transport_identity::server_reliable_data_transport_identity(&inbound, &view)
                .as_deref()
                != Some(event.transport_payload_identity.as_slice())
        {
            tracing::warn!(
                sequence = event.sequence,
                "buffered interleaved server M identity changed before ordered commit"
            );
            arm_ordered_server_successor_fence(
                state,
                event.sequence,
                final_sequence.unwrap_or(event.sequence),
            );
            break;
        }
        if exact_direct_semantic_source_payload(&inbound, &view).is_none() {
            tracing::warn!(
                sequence = view.sequence,
                trailing_payload_length = view.trailing_payload_length,
                "non-direct interleaved server M event withheld for full-pipeline retransmission"
            );
            arm_ordered_server_successor_fence(
                state,
                event.sequence,
                final_sequence.unwrap_or(event.sequence),
            );
            break;
        }

        // The first contiguous successor is emitted in the predecessor's
        // reconstructed batch, just as the original receive window drains
        // contiguous occupied slots. Keep its exact raw slot fenced until the
        // *whole* batch passes the outer strict validator; rejection must be
        // able to replay this event rather than only roll back its semantics.
        arm_ordered_server_successor_fence(
            state,
            event_sequence,
            final_sequence.unwrap_or(event_sequence),
        );
        deferred_module_resources::capture_early_server_status_if_needed(
            &inbound,
            &view,
            &state.module_resources,
            &mut state.deferred_module_resources.pending,
        );

        if let Some(verified) = replay_completed_direct_server_semantic_rewrite(
            &inbound,
            &view,
            event_server_origin_generation,
            state,
        )? {
            promote_or_begin_ordered_successor_effect_transaction(state, event_sequence)?;
            stage_ordered_successor_validation_token(
                state,
                event_sequence,
                event_server_origin_generation,
                event_transport_payload_identity.clone(),
            )?;
            resolved.push(verified);
            break;
        }

        if let Some(rewrite) = server_dispatch::rewrite_direct_frame_if_needed(
            &inbound,
            &view,
            &state.module_resources,
            Some(&state.area_context.latest_area_placeables),
            Some(&state.semantic.objects),
        )? {
            promote_or_begin_ordered_successor_effect_transaction(state, event_sequence)?;
            let verified = commit_direct_server_semantic_rewrite(
                state,
                &inbound,
                &view,
                event.server_peer_ack_sequence,
                event_server_origin_generation,
                rewrite,
            )?;
            stage_ordered_successor_validation_token(
                state,
                event_sequence,
                event_server_origin_generation,
                event_transport_payload_identity,
            )?;
            tracing::info!(
                sequence = view.sequence,
                proof = verified.proof.as_str(),
                "buffered interleaved server M staged after typed predecessor pending strict batch validation"
            );
            resolved.push(verified);
            break;
        }

        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            "unowned interleaved server M event withheld for reliable retransmission"
        );
        arm_ordered_server_successor_fence(
            state,
            event.sequence,
            final_sequence.unwrap_or(event.sequence),
        );
        break;
    }

    reassembly.interleaved_packets = resolved;
    if state.deflate.ordered_successor_next_sequence.is_none() {
        state.deflate.ordered_successor_events.clear();
    }
    Ok(())
}

fn arm_ordered_server_successor_fence(
    state: &mut SessionState,
    next_sequence: u16,
    final_sequence: u16,
) {
    state.deflate.ordered_successor_next_sequence = Some(next_sequence);
    state.deflate.ordered_successor_final_sequence = Some(final_sequence);
    tracing::info!(
        next_sequence,
        final_sequence,
        "ordered server successor fence armed for reliable retransmission"
    );
}

fn stage_ordered_successor_validation_token(
    state: &mut SessionState,
    sequence: u16,
    server_origin_generation: u64,
    transport_payload_identity: Vec<u8>,
) -> anyhow::Result<()> {
    if state.deflate.ordered_successor_pending_validation.is_some() {
        anyhow::bail!(
            "ordered successor validation token already active while staging sequence {}",
            sequence
        );
    }
    if state.deflate.ordered_successor_next_sequence != Some(sequence) {
        anyhow::bail!(
            "ordered successor validation token sequence {} does not match active fence {:?}",
            sequence,
            state.deflate.ordered_successor_next_sequence
        );
    }
    state.deflate.ordered_successor_pending_validation =
        Some(state::OrderedSuccessorValidationToken {
            sequence,
            server_origin_generation,
            transport_payload_identity,
        });
    Ok(())
}

fn merge_ordered_server_successor_events(
    state: &mut SessionState,
    events: &[reassembly::BufferedInterleavedServerPacket],
) -> anyhow::Result<()> {
    // Merge transactionally. A late conflict or capacity failure must not
    // leave a prefix of the candidate suffix orphaned behind the active fence.
    let mut merged = state.deflate.ordered_successor_events.clone();
    for event in events {
        if let Some(existing) = merged.iter_mut().find(|existing| {
            existing.server_origin_generation == event.server_origin_generation
                && existing.sequence == event.sequence
        }) {
            if existing.transport_payload_identity != event.transport_payload_identity {
                anyhow::bail!(
                    "conflicting raw ordered successor identity for sequence {} generation {}",
                    event.sequence,
                    event.server_origin_generation
                );
            }
            existing.packet = event.packet.clone();
            existing.server_peer_ack_sequence = event.server_peer_ack_sequence;
            continue;
        }
        if merged.len() >= MAX_INTERLEAVED_PACKETS {
            anyhow::bail!(
                "raw ordered successor queue exceeds bounded capacity {} at sequence {} generation {}",
                MAX_INTERLEAVED_PACKETS,
                event.sequence,
                event.server_origin_generation
            );
        }
        merged.push_back(event.clone());
    }
    state.deflate.ordered_successor_events = merged;
    Ok(())
}

fn next_reliable_sequence(sequence: u16) -> u16 {
    sequence.wrapping_add(1)
}

fn advance_reliable_sequence(mut sequence: u16, steps: usize) -> u16 {
    for _ in 0..steps {
        sequence = next_reliable_sequence(sequence);
    }
    sequence
}

fn arm_withheld_reassembly_successors(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
) -> anyhow::Result<()> {
    let Some(final_sequence) = reassembly
        .interleaved_events
        .last()
        .map(|event| event.sequence)
    else {
        return Ok(());
    };
    merge_ordered_server_successor_events(state, &reassembly.interleaved_events)?;
    let computed_next =
        advance_reliable_sequence(reassembly.first_sequence, reassembly.expected_frames);
    let next_sequence = state
        .deflate
        .ordered_successor_next_sequence
        .unwrap_or(computed_next);
    let final_sequence = state
        .deflate
        .ordered_successor_final_sequence
        .into_iter()
        .chain(std::iter::once(final_sequence))
        .max_by_key(|sequence| sequence.wrapping_sub(next_sequence))
        .unwrap_or(final_sequence);
    arm_ordered_server_successor_fence(state, next_sequence, final_sequence);
    Ok(())
}

fn shift_server_sequence_for_client(state: &SessionState, packet: &mut [u8]) -> anyhow::Result<()> {
    if state.sequence.server_sequence_shifts.is_empty() {
        return Ok(());
    }

    let Some(view) = MFrameView::parse(packet) else {
        return Ok(());
    };
    if view.frame_type != 0 {
        return Ok(());
    }

    let shifted = shift_sequence_for_peer(&state.sequence.server_sequence_shifts, view.sequence);
    if shifted == view.sequence {
        return Ok(());
    }

    write_be_u16(packet, 3, shifted)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to shift server M sequence"))?;
    encode_legacy_m_crc(packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair shifted server M CRC"))?;
    tracing::info!(
        sequence = view.sequence,
        shifted_sequence = shifted,
        ack_sequence = view.ack_sequence,
        shifts = state.sequence.server_sequence_shifts.len(),
        "server M sequence shifted after proxy-owned server M insertion"
    );
    Ok(())
}

fn area_gate_after_current_server_emit(
    state: &SessionState,
    emit: &Emit,
) -> anyhow::Result<Option<synthetic_area::ServerHoldGate>> {
    let mut gate = state.synthetic_area.server_hold_gate.clone();
    match emit {
        Emit::VerifiedPackets { family, packets } => {
            let proof = VerifiedProof::family(*family);
            for packet in packets {
                advance_preview_area_gate(state, &mut gate, &proof, packet, false)?;
            }
        }
        Emit::VerifiedPacketsPreShifted { family, packets } => {
            let proof = VerifiedProof::family(*family);
            for packet in packets {
                advance_preview_area_gate(state, &mut gate, &proof, packet, true)?;
            }
        }
        Emit::VerifiedProofPackets { proof, packets } => {
            for packet in packets {
                advance_preview_area_gate(state, &mut gate, proof, packet, false)?;
            }
        }
        Emit::VerifiedProofPacketsPreShifted { proof, packets } => {
            for packet in packets {
                advance_preview_area_gate(state, &mut gate, proof, packet, true)?;
            }
        }
        Emit::MixedVerifiedPackets(packets) => {
            for (family, packet) in packets {
                let proof = VerifiedProof::family(*family);
                advance_preview_area_gate(state, &mut gate, &proof, packet, false)?;
            }
        }
        Emit::MixedVerifiedProofPackets(packets) => {
            for (proof, packet) in packets {
                advance_preview_area_gate(state, &mut gate, proof, packet, false)?;
            }
        }
        Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            for (proof, packet) in packets {
                advance_preview_area_gate(state, &mut gate, proof, packet, true)?;
            }
        }
        Emit::Packet(_)
        | Emit::PacketRetireSession { .. }
        | Emit::Packets(_)
        | Emit::PacketsPreShifted(_)
        | Emit::Consumed
        | Emit::ConsumedRetireSession { .. }
        | Emit::Drop => {}
    }
    Ok(gate)
}

fn advance_preview_area_gate(
    state: &SessionState,
    gate: &mut Option<synthetic_area::ServerHoldGate>,
    proof: &VerifiedProof,
    packet: &[u8],
    pre_shifted: bool,
) -> anyhow::Result<()> {
    if !proof.contains_family(VerifiedFamily::AreaClientArea) {
        return Ok(());
    }
    let mut shifted;
    let packet = if pre_shifted {
        packet
    } else {
        shifted = packet.to_vec();
        shift_server_sequence_for_client(state, &mut shifted)?;
        &shifted
    };
    if deferred_module_resources::module_resource_hold_gate_release_sequence(
        &state.deferred_module_resources.pending,
    )
    .is_some()
        && !module_resource_gate_window_packet_can_pass(state, packet)
    {
        return Ok(());
    }
    let _ = area_load_gate_packet_release_mode_for_gate(gate, proof, packet);
    Ok(())
}

fn finalize_server_to_client_emit(
    state: &mut SessionState,
    emit: Emit,
    pending_count_before: usize,
) -> anyhow::Result<Emit> {
    state.deflate.last_server_core_dispatch_accepted = !matches!(emit, Emit::Drop);
    if matches!(emit, Emit::Drop) {
        // An unrelated due proxy-owned packet must not turn a rejected source
        // datagram into a successful outer batch. Leave it queued for the
        // session drain, whose own strict transaction can retry it exactly.
        rollback_ordinary_server_emit_after_drop(state);
        return Ok(Emit::Drop);
    }
    let area_gate_after_current = area_gate_after_current_server_emit(state, &emit)?;
    let now = Instant::now();
    if state.deflate.ordered_successor_effect_snapshot.is_none()
        && state
            .synthetic_area
            .pending_server_to_client_packets
            .iter()
            .enumerate()
            .any(|(original_index, pending)| {
                pending.due_at <= now
                    && pending_server_packet_can_reach_for_current_emit(
                        state,
                        pending,
                        original_index,
                        pending_count_before,
                        &area_gate_after_current,
                    )
            })
    {
        // A source translator can queue an immediately due AfterCurrentEmit
        // packet (notably Area_ClientArea -> LoadBar_Start). Snapshot after the
        // accepted core dispatch but before removing that packet so final
        // validation can restore the typed pending queue without changing its
        // required source-before-suffix wire order.
        begin_ordinary_server_emit_effect_transaction(state)?;
    }
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut released_synthetic_loadbar_end = false;

    let pending_packets = take_due_pending_server_packets(
        state,
        now,
        "server synthetic M packet released",
        false,
        PendingServerGatePolicy::CurrentEmit {
            pending_count_before,
            area_gate_after_current: &area_gate_after_current,
        },
    );
    for (original_index, pending) in pending_packets {
        if pending.reason == "Area_ClientArea synthetic LoadBar_End" {
            released_synthetic_loadbar_end = true;
            tracing::info!(
                "client synthetic Area_AreaLoaded remains gated until native packet or fallback grace"
            );
        }
        let force_prefix = matches!(
            pending.placement,
            synthetic_area::PendingServerPacketPlacement::BeforeCurrentEmit
        );
        let was_pending_before_current =
            original_index.is_some_and(|index| index < pending_count_before);
        if force_prefix || was_pending_before_current {
            prefix.push((pending.family, pending.packet));
        } else {
            suffix.push((pending.family, pending.packet));
        }
    }
    if released_synthetic_loadbar_end {
        tracing::info!(
            "synthetic LoadBar_End released without re-arming area hold gate; LoadBar is UI state, not the server-authoritative area stream gate"
        );
    }

    let finalized = match emit {
        Emit::Consumed => {
            prefix.extend(suffix);
            if prefix.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedPackets(prefix))
            }
        }
        Emit::ConsumedRetireSession { reason } => {
            prefix.extend(suffix);
            if prefix.is_empty() {
                Ok(Emit::ConsumedRetireSession { reason })
            } else {
                Ok(Emit::MixedVerifiedPackets(prefix))
            }
        }
        Emit::Drop => {
            prefix.extend(suffix);
            if prefix.is_empty() {
                Ok(Emit::Drop)
            } else {
                Ok(Emit::MixedVerifiedPackets(prefix))
            }
        }
        Emit::Packet(mut packet) => {
            shift_server_sequence_for_client(state, &mut packet)?;
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::Packet(packet))
            } else {
                let Some(family) = transport_only_verified_family_for_plain_server_packet(&packet)
                else {
                    tracing::warn!(
                        pending_verified_prefix = prefix.len(),
                        pending_verified_suffix = suffix.len(),
                        packet_len = packet.len(),
                        "server pending verified packets could not be combined with unverified plain packet"
                    );
                    return Ok(Emit::Drop);
                };
                let mut mixed = Vec::with_capacity(prefix.len() + 1 + suffix.len());
                mixed.extend(prefix);
                mixed.push((family, packet));
                mixed.extend(suffix);
                Ok(Emit::MixedVerifiedPackets(mixed))
            }
        }
        Emit::PacketRetireSession { reason, .. } => {
            tracing::warn!(
                reason,
                pending_verified_prefix = prefix.len(),
                pending_verified_suffix = suffix.len(),
                "client-side retire-session packet reached server M-frame finalizer; dropping instead of releasing or buffering pending server packets"
            );
            Ok(Emit::Drop)
        }
        Emit::Packets(mut packets) => {
            for packet in &mut packets {
                shift_server_sequence_for_client(state, packet)?;
            }
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::Packets(packets))
            } else {
                let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
                mixed.extend(prefix);
                for packet in packets {
                    let Some(family) =
                        transport_only_verified_family_for_plain_server_packet(&packet)
                    else {
                        tracing::warn!(
                            pending_verified_suffix = suffix.len(),
                            packet_len = packet.len(),
                            "server pending verified packets could not be combined with unverified plain packet batch"
                        );
                        return Ok(Emit::Drop);
                    };
                    mixed.push((family, packet));
                }
                mixed.extend(suffix);
                Ok(Emit::MixedVerifiedPackets(mixed))
            }
        }
        Emit::PacketsPreShifted(packets) => {
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::PacketsPreShifted(packets))
            } else {
                let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
                mixed.extend(prefix);
                for packet in packets {
                    let Some(family) =
                        transport_only_verified_family_for_plain_server_packet(&packet)
                    else {
                        tracing::warn!(
                            pending_verified_suffix = suffix.len(),
                            packet_len = packet.len(),
                            "server pending verified packets could not be combined with unverified pre-shifted packet batch"
                        );
                        return Ok(Emit::Drop);
                    };
                    mixed.push((family, packet));
                }
                mixed.extend(suffix);
                Ok(Emit::MixedVerifiedPackets(mixed))
            }
        }
        Emit::MixedVerifiedPackets(mut packets) => {
            for (_, packet) in &mut packets {
                shift_server_sequence_for_client(state, packet)?;
            }
            let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
            mixed.extend(prefix);
            mixed.extend(packets);
            mixed.extend(suffix);
            Ok(Emit::MixedVerifiedPackets(mixed))
        }
        Emit::MixedVerifiedProofPackets(mut packets) => {
            for (_, packet) in &mut packets {
                shift_server_sequence_for_client(state, packet)?;
            }
            let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
            mixed.extend(
                prefix
                    .into_iter()
                    .map(|(family, packet)| (VerifiedProof::family(family), packet)),
            );
            mixed.extend(packets);
            mixed.extend(
                suffix
                    .into_iter()
                    .map(|(family, packet)| (VerifiedProof::family(family), packet)),
            );
            Ok(Emit::MixedVerifiedProofPackets(mixed))
        }
        Emit::MixedVerifiedProofPacketsPreShifted(mut packets) => {
            let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
            mixed.extend(
                prefix
                    .into_iter()
                    .map(|(family, packet)| (VerifiedProof::family(family), packet)),
            );
            mixed.append(&mut packets);
            mixed.extend(
                suffix
                    .into_iter()
                    .map(|(family, packet)| (VerifiedProof::family(family), packet)),
            );
            Ok(Emit::MixedVerifiedProofPackets(mixed))
        }
        Emit::VerifiedPackets {
            family,
            packets: mut packets,
        } => {
            for packet in &mut packets {
                shift_server_sequence_for_client(state, packet)?;
            }
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::VerifiedPackets { family, packets })
            } else {
                let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
                mixed.extend(prefix);
                mixed.extend(packets.into_iter().map(|packet| (family, packet)));
                mixed.extend(suffix);
                Ok(Emit::MixedVerifiedPackets(mixed))
            }
        }
        Emit::VerifiedProofPackets {
            proof,
            packets: mut packets,
        } => {
            for packet in &mut packets {
                shift_server_sequence_for_client(state, packet)?;
            }
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::VerifiedProofPackets { proof, packets })
            } else {
                let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
                mixed.extend(
                    prefix
                        .into_iter()
                        .map(|(family, packet)| (VerifiedProof::family(family), packet)),
                );
                mixed.extend(packets.into_iter().map(|packet| (proof.clone(), packet)));
                mixed.extend(
                    suffix
                        .into_iter()
                        .map(|(family, packet)| (VerifiedProof::family(family), packet)),
                );
                Ok(Emit::MixedVerifiedProofPackets(mixed))
            }
        }
        Emit::VerifiedPacketsPreShifted {
            family,
            packets: mut packets,
        } => {
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::VerifiedPacketsPreShifted { family, packets })
            } else {
                let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
                mixed.extend(prefix);
                mixed.extend(packets.into_iter().map(|packet| (family, packet)));
                mixed.extend(suffix);
                Ok(Emit::MixedVerifiedPackets(mixed))
            }
        }
        Emit::VerifiedProofPacketsPreShifted {
            proof,
            packets: mut packets,
        } => {
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::VerifiedProofPacketsPreShifted { proof, packets })
            } else {
                let mut mixed = Vec::with_capacity(prefix.len() + packets.len() + suffix.len());
                mixed.extend(
                    prefix
                        .into_iter()
                        .map(|(family, packet)| (VerifiedProof::family(family), packet)),
                );
                mixed.extend(packets.into_iter().map(|packet| (proof.clone(), packet)));
                mixed.extend(
                    suffix
                        .into_iter()
                        .map(|(family, packet)| (VerifiedProof::family(family), packet)),
                );
                Ok(Emit::MixedVerifiedProofPackets(mixed))
            }
        }
    };
    let finalized = finalized.and_then(|finalized_emit| {
        let finalized_emit = hold_server_emit_until_module_resource_ack(state, finalized_emit)?;
        if released_synthetic_loadbar_end {
            Ok(finalized_emit)
        } else {
            hold_server_emit_until_area_load_ack(state, finalized_emit)
        }
    });
    if let Ok(ref finalized_emit) = finalized {
        record_server_emit_window_state(state, finalized_emit);
    }
    finalized
}

fn record_server_emit_window_state(state: &mut SessionState, emit: &Emit) {
    match emit {
        Emit::Packet(packet) => {
            record_server_packet_window_state(state, packet);
        }
        Emit::Packets(packets)
        | Emit::PacketsPreShifted(packets)
        | Emit::VerifiedPackets { packets, .. }
        | Emit::VerifiedPacketsPreShifted { packets, .. }
        | Emit::VerifiedProofPackets { packets, .. }
        | Emit::VerifiedProofPacketsPreShifted { packets, .. } => {
            for packet in packets {
                record_server_packet_window_state(state, packet);
            }
        }
        Emit::MixedVerifiedPackets(packets) => {
            for (_, packet) in packets {
                record_server_packet_window_state(state, packet);
            }
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            for (_, packet) in packets {
                record_server_packet_window_state(state, packet);
            }
        }
        Emit::PacketRetireSession { .. } => {}
        Emit::Consumed | Emit::ConsumedRetireSession { .. } | Emit::Drop => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_reliable_m_frame(sequence: u16) -> Vec<u8> {
        let mut packet = vec![0; crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, sequence));
        packet[7] = 0x10;
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }

    fn client_reliable_m_frame(sequence: u16, ack_sequence: u16, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0; crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, sequence));
        assert!(write_be_u16(&mut packet, 5, ack_sequence));
        packet[7] = 0x0A;
        assert!(write_be_u16(&mut packet, 8, 1));
        assert!(write_be_u16(&mut packet, 10, payload.len() as u16));
        packet.extend_from_slice(payload);
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }

    fn reliable_server_m_frame(
        sequence: u16,
        ack_sequence: u16,
        flags: u8,
        packetized_sequence: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut packet = vec![0; crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, sequence));
        assert!(write_be_u16(&mut packet, 5, ack_sequence));
        packet[7] = flags;
        assert!(write_be_u16(&mut packet, 8, packetized_sequence));
        assert!(write_be_u16(
            &mut packet,
            10,
            u16::try_from(payload.len()).expect("test payload length")
        ));
        packet.extend_from_slice(payload);
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }

    fn two_frame_deflated_window(
        first_sequence: u16,
        ack_sequence: u16,
        inflated: &[u8],
    ) -> (Vec<u8>, Vec<u8>) {
        let compressed = deflate_zlib(inflated).expect("test payload should deflate");
        assert!(compressed.len() > 1);
        let split = (compressed.len() / 2).clamp(1, compressed.len() - 1);
        let mut first_payload = (inflated.len() as u32).to_le_bytes().to_vec();
        first_payload.extend_from_slice(&compressed[..split]);
        (
            reliable_server_m_frame(first_sequence, ack_sequence, 0x0C, 2, &first_payload),
            reliable_server_m_frame(
                first_sequence.wrapping_add(1),
                ack_sequence,
                0x48,
                0,
                &compressed[split..],
            ),
        )
    }

    fn proof_packets(emit: Emit) -> Vec<(VerifiedProof, Vec<u8>)> {
        match emit {
            Emit::MixedVerifiedProofPackets(packets)
            | Emit::MixedVerifiedProofPacketsPreShifted(packets) => packets,
            Emit::VerifiedProofPackets { proof, packets }
            | Emit::VerifiedProofPacketsPreShifted { proof, packets } => packets
                .into_iter()
                .map(|packet| (proof.clone(), packet))
                .collect(),
            Emit::MixedVerifiedPackets(packets) => packets
                .into_iter()
                .map(|(family, packet)| (VerifiedProof::family(family), packet))
                .collect(),
            Emit::VerifiedPackets { family, packets }
            | Emit::VerifiedPacketsPreShifted { family, packets } => packets
                .into_iter()
                .map(|packet| (VerifiedProof::family(family), packet))
                .collect(),
            other => panic!("expected verified packet batch, got {other:?}"),
        }
    }

    #[test]
    fn pre_shifted_completed_stream_replay_is_not_shifted_twice() {
        let compressed = vec![0x10, 0x20, 0x30, 0x40];
        let mut source_payload = 16u32.to_le_bytes().to_vec();
        source_payload.extend_from_slice(&compressed);
        let source = reliable_server_m_frame(10, 82, 0x0D, 1, &source_payload);
        let cached_packet = reliable_server_m_frame(
            11,
            70,
            0x08,
            1,
            &crate::translate::loadbar::start_payload(2),
        );
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        state
            .sequence
            .server_sequence_shifts
            .push(SequenceShift { base: 10, delta: 1 });
        state.deflate.completed_server_stream_windows.push(
            reassembly::CompletedDeflatedStreamWindow {
                first_sequence: 10,
                server_origin_generation: 0,
                expected_frames: 1,
                packetized_sequence: 1,
                inflated_length: 16,
                compressed,
                frame_transport_identities: vec![server_transport_identity_for_test(&source)],
                pre_shifted: true,
                replay: CompletedDeflatedReplay::VerifiedProofPackets {
                    proof: VerifiedProof::family(VerifiedFamily::LoadBar),
                    packets: vec![cached_packet],
                },
            },
        );

        let packets = proof_packets(
            translate_server_to_client(&source, &mut state)
                .expect("exact completed stream replay should succeed"),
        );

        assert_eq!(packets.len(), 1);
        let replayed = MFrameView::parse(&packets[0].1).expect("replayed packet should parse");
        assert_eq!(
            replayed.sequence, 11,
            "cached shift must not be applied again"
        );
        assert_eq!(replayed.ack_sequence, 82);
        assert!(replayed.crc_valid);
    }

    #[test]
    fn deflated_frame_with_unclaimed_trailing_bytes_fails_before_stream_state() {
        let inflated = crate::translate::loadbar::start_payload(2);
        let compressed = deflate_zlib(&inflated).expect("test payload should deflate");
        let mut payload = (inflated.len() as u32).to_le_bytes().to_vec();
        payload.extend_from_slice(&compressed);
        let mut frame = reliable_server_m_frame(130, 73, 0x0D, 1, &payload);
        frame.push(0xFF);
        assert!(encode_legacy_m_crc(&mut frame));
        let view = MFrameView::parse(&frame).expect("trailing test frame should parse");
        assert_eq!(view.trailing_payload_length, 1);

        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        let due_packet =
            client_reliable_m_frame(131, 73, &crate::translate::loadbar::start_payload(3));
        state.synthetic_area.pending_server_to_client_packets.push(
            synthetic_area::PendingServerPacket {
                family: VerifiedFamily::LoadBar,
                packet: due_packet.clone(),
                due_at: Instant::now(),
                reason: "malformed coalesced source must not drain valid due output",
                placement: synthetic_area::PendingServerPacketPlacement::BeforeCurrentEmit,
            },
        );
        assert!(matches!(
            translate_server_to_client(&frame, &mut state)
                .expect("unclaimed trailing data should fail closed"),
            Emit::Drop
        ));
        assert!(state.deflate.server_reassembly.is_none());
        assert!(state.deflate.server_zlib_inflater.is_none());
        assert!(state.deflate.completed_server_stream_windows.is_empty());
        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
        assert_eq!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .iter()
                .map(|pending| pending.packet.as_slice())
                .collect::<Vec<_>>(),
            vec![due_packet.as_slice()],
            "malformed source rejection must not be masked by or consume unrelated valid due output"
        );
    }

    #[test]
    fn count_zero_trailing_storage_cannot_enter_count_one_coalesced_routing() {
        let inflated = crate::translate::loadbar::start_payload(2);
        let compressed = deflate_zlib(&inflated).expect("test payload should deflate");
        let mut primary_payload = (inflated.len() as u32).to_le_bytes().to_vec();
        primary_payload.extend_from_slice(&compressed);
        let mut frame = reliable_server_m_frame(135, 73, 0x0C, 0, &primary_payload);
        let queued = reliable_server_m_frame(
            136,
            73,
            0x08,
            1,
            &crate::translate::loadbar::start_payload(3),
        );
        frame.extend_from_slice(&queued);
        assert!(encode_legacy_m_crc(&mut frame));
        let view = MFrameView::parse(&frame).expect("count-zero trailing frame");
        assert_eq!(view.packetized_sequence, 0);
        assert_ne!(view.trailing_payload_length, 0);

        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        assert!(matches!(
            translate_server_to_client(&frame, &mut state)
                .expect("orphan count-zero trailing storage should fail closed"),
            Emit::Drop
        ));
        assert!(state.deflate.server_reassembly.is_none());
        assert!(state.coalesced_replay.completed_deflated_records.is_empty());
        assert!(state.coalesced_replay.completed_direct_records.is_empty());
        assert!(
            state
                .deflate
                .completed_server_reliable_stream_slots
                .is_empty()
        );
    }

    #[test]
    fn packetized_deflate_uses_full_stored_frames_despite_short_declared_lengths() {
        let inflated = crate::translate::loadbar::start_payload(2);
        let (mut first, mut continuation) = two_frame_deflated_window(140, 73, &inflated);
        let payload_offset = crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
        let first_available = first.len() - payload_offset;
        let continuation_available = continuation.len() - payload_offset;
        assert!(first_available > 4);
        assert!(continuation_available > 1);

        // Both original count>1 readers ignore these declarations. A one-byte
        // first declaration also proves routing does not depend on the generic
        // MFrameView's narrowed deflated-envelope probe.
        assert!(write_be_u16(&mut first, 10, 1));
        assert!(encode_legacy_m_crc(&mut first));
        assert!(write_be_u16(
            &mut continuation,
            10,
            (continuation_available - 1) as u16
        ));
        assert!(encode_legacy_m_crc(&mut continuation));
        let first_view = MFrameView::parse(&first).expect("short-declared first frame");
        assert!(first_view.deflated.is_none());
        assert_eq!(first_view.packetized_sequence, 2);
        assert_eq!(first_view.payload_length, 1);
        assert_eq!(first_view.available_payload_length, first_available);
        let continuation_view =
            MFrameView::parse(&continuation).expect("short-declared continuation");
        assert_eq!(continuation_view.payload_length, continuation_available - 1);
        assert_eq!(
            continuation_view.available_payload_length,
            continuation_available
        );
        assert_eq!(continuation_view.trailing_payload_length, 1);

        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        assert!(matches!(
            translate_server_to_client(&first, &mut state).expect("first frame should buffer"),
            Emit::Consumed
        ));
        let pending = state
            .deflate
            .server_reassembly
            .as_ref()
            .expect("full stored first frame should start reassembly");
        assert_eq!(pending.frames.len(), 1);
        assert_eq!(pending.frames[0].payload_length, first_available);
        assert_eq!(
            pending.frames[0].compressed_chunk,
            first[payload_offset + 4..]
        );

        let packets = proof_packets(
            translate_server_to_client(&continuation, &mut state)
                .expect("full stored continuation should complete reassembly"),
        );
        assert!(
            packets
                .iter()
                .any(|(proof, _)| proof.contains_family(VerifiedFamily::LoadBar))
        );
        assert!(state.deflate.server_reassembly.is_none());
        assert!(state.coalesced_replay.completed_deflated_records.is_empty());
    }

    #[test]
    fn completed_sequence_zero_stream_does_not_claim_ack_control_lane() {
        let inflated = crate::translate::loadbar::start_payload(2);
        let compressed = deflate_zlib(&inflated).expect("test payload should deflate");
        let mut source_payload = (inflated.len() as u32).to_le_bytes().to_vec();
        source_payload.extend_from_slice(&compressed);
        let source = reliable_server_m_frame(0, 1, 0x0D, 1, &source_payload);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        let first = proof_packets(
            translate_server_to_client(&source, &mut state)
                .expect("sequence-zero stream should translate"),
        );
        assert!(
            first
                .iter()
                .any(|(proof, _)| proof.contains_family(VerifiedFamily::LoadBar))
        );
        assert_eq!(
            state.deflate.completed_server_reliable_stream_slots.len(),
            1,
            "the translated type-0 stream should pin one reliable route"
        );

        for ack_sequence in [2, 3] {
            let ack_control = reliable_server_m_frame(0, ack_sequence, 0x10, 0, &[]);
            let ack_packets = proof_packets(
                translate_server_to_client(&ack_control, &mut state)
                    .expect("type-1 ACK control should bypass the type-0 route ledger"),
            );
            assert_eq!(ack_packets.len(), 1);
            assert!(
                ack_packets[0]
                    .0
                    .contains_family(VerifiedFamily::ConsumedEmptyMFrame)
            );
            let ack_view = MFrameView::parse(&ack_packets[0].1).expect("ACK control should parse");
            assert_eq!(ack_view.sequence, 0);
            assert_eq!(ack_view.ack_sequence, ack_sequence);
            assert_eq!(ack_view.frame_type, 1);
            assert_eq!(ack_view.payload_length, 0);
            assert!(ack_view.crc_valid);
        }
        assert_eq!(
            state.deflate.completed_server_reliable_stream_slots.len(),
            1,
            "the independent ACK lane must not replace the stream route"
        );

        let retransmit = reliable_server_m_frame(0, 4, 0x0D, 1, &source_payload);
        let replay = proof_packets(
            translate_server_to_client(&retransmit, &mut state)
                .expect("exact stream retransmit should still replay"),
        );
        assert!(
            replay
                .iter()
                .any(|(proof, _)| proof.contains_family(VerifiedFamily::LoadBar))
        );
    }

    #[test]
    fn header_bearing_stream_retransmit_preserves_inflater_for_raw_continuation() {
        fn compress_stream_chunk(compressor: &mut flate2::Compress, payload: &[u8]) -> Vec<u8> {
            let before_out = compressor.total_out();
            let mut output = vec![0u8; payload.len().saturating_mul(2).saturating_add(128)];
            compressor
                .compress(payload, &mut output, flate2::FlushCompress::Sync)
                .expect("persistent stream compression should succeed");
            let produced = (compressor.total_out() - before_out) as usize;
            output.truncate(produced);
            output
        }

        fn stream_frame(
            sequence: u16,
            ack_sequence: u16,
            inflated: &[u8],
            compressed: &[u8],
        ) -> Vec<u8> {
            let mut payload = (inflated.len() as u32).to_le_bytes().to_vec();
            payload.extend_from_slice(compressed);
            reliable_server_m_frame(sequence, ack_sequence, 0x0D, 1, &payload)
        }

        let first_inflated = crate::translate::loadbar::start_payload(2);
        let second_inflated = crate::translate::loadbar::start_payload(3);
        let mut compressor = flate2::Compress::new(flate2::Compression::default(), true);
        let first_compressed = compress_stream_chunk(&mut compressor, &first_inflated);
        let second_compressed = compress_stream_chunk(&mut compressor, &second_inflated);
        assert!(looks_like_zlib_wrapped_deflate(&first_compressed));
        assert!(!looks_like_zlib_wrapped_deflate(&second_compressed));

        let first = stream_frame(140, 74, &first_inflated, &first_compressed);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        let first_emit =
            translate_server_to_client(&first, &mut state).expect("seed stream should translate");
        assert!(!matches!(first_emit, Emit::Drop));
        assert_eq!(state.deflate.completed_server_stream_windows.len(), 1);
        let inflater = state
            .deflate
            .server_zlib_inflater
            .as_ref()
            .expect("stream-bit header must seed persistent inflater");
        let totals_after_first = (inflater.total_in(), inflater.total_out());
        let owner_after_first = state.deflate.server_zlib_stream_owner;
        let owner_epoch_after_first = state.deflate.server_zlib_stream_epoch;

        let first_retransmit = stream_frame(140, 79, &first_inflated, &first_compressed);
        let replay_emit = translate_server_to_client(&first_retransmit, &mut state)
            .expect("header-bearing retransmit should replay");
        assert!(!matches!(replay_emit, Emit::Drop));
        let inflater = state
            .deflate
            .server_zlib_inflater
            .as_ref()
            .expect("replay must retain persistent inflater");
        assert_eq!(
            (inflater.total_in(), inflater.total_out()),
            totals_after_first
        );
        assert_eq!(state.deflate.server_zlib_stream_owner, owner_after_first);
        assert_eq!(
            state.deflate.server_zlib_stream_epoch,
            owner_epoch_after_first
        );

        let mut wrong_stream_disposition = first_retransmit.clone();
        wrong_stream_disposition[7] &= !0x01;
        assert!(encode_legacy_m_crc(&mut wrong_stream_disposition));
        assert!(matches!(
            translate_server_to_client(&wrong_stream_disposition, &mut state)
                .expect("changed stream disposition should fail closed"),
            Emit::Drop
        ));
        let inflater = state
            .deflate
            .server_zlib_inflater
            .as_ref()
            .expect("stream-disposition conflict must retain persistent inflater");
        assert_eq!(
            (inflater.total_in(), inflater.total_out()),
            totals_after_first
        );

        let mut conflicting = first_retransmit;
        let compressed_offset = crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 4;
        conflicting[compressed_offset + 2] ^= 0x40;
        assert!(encode_legacy_m_crc(&mut conflicting));
        assert!(matches!(
            translate_server_to_client(&conflicting, &mut state)
                .expect("conflicting reliable slot should fail closed"),
            Emit::Drop
        ));
        let inflater = state
            .deflate
            .server_zlib_inflater
            .as_ref()
            .expect("conflict must retain persistent inflater");
        assert_eq!(
            (inflater.total_in(), inflater.total_out()),
            totals_after_first
        );

        let second = stream_frame(141, 80, &second_inflated, &second_compressed);
        let second_emit = translate_server_to_client(&second, &mut state)
            .expect("raw continuation should still use seeded inflater");
        assert!(!matches!(second_emit, Emit::Drop));
        let inflater = state
            .deflate
            .server_zlib_inflater
            .as_ref()
            .expect("continuation must retain persistent inflater");
        assert!(inflater.total_in() > totals_after_first.0);
        assert!(inflater.total_out() > totals_after_first.1);
        assert_eq!(state.deflate.server_zlib_stream_owner, owner_after_first);
        assert_eq!(
            state.deflate.server_zlib_stream_epoch,
            owner_epoch_after_first
        );
    }

    #[test]
    fn due_pending_packets_keep_original_queue_positions_after_filtering() {
        let now = Instant::now();
        let payload = crate::translate::loadbar::start_payload(2);
        let mut state = SessionState::default();
        state.synthetic_area.pending_server_to_client_packets = vec![
            synthetic_area::PendingServerPacket {
                family: VerifiedFamily::LoadBar,
                packet: client_reliable_m_frame(40, 74, &payload),
                due_at: now + std::time::Duration::from_secs(60),
                reason: "older delayed packet",
                placement: synthetic_area::PendingServerPacketPlacement::AfterCurrentEmit,
            },
            synthetic_area::PendingServerPacket {
                family: VerifiedFamily::LoadBar,
                packet: client_reliable_m_frame(41, 74, &payload),
                due_at: now,
                reason: "newly due packet",
                placement: synthetic_area::PendingServerPacketPlacement::AfterCurrentEmit,
            },
        ];

        let due = take_due_pending_server_packets(
            &mut state,
            now,
            "test pending packet released",
            false,
            PendingServerGatePolicy::Ignore,
        );

        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, Some(1));
        assert_eq!(due[0].1.reason, "newly due packet");
        assert_eq!(
            state.synthetic_area.pending_server_to_client_packets.len(),
            1
        );
        assert_eq!(
            state.synthetic_area.pending_server_to_client_packets[0].reason,
            "older delayed packet"
        );
    }

    #[test]
    fn ordered_successor_data_sequence_wraps_through_zero() {
        // Diamond sub_5F3940 and EE FrameReceive reserve frame *types* 1/2 for
        // control; the type-0 receive cursor itself wraps FFFF -> 0000.
        assert_eq!(next_reliable_sequence(u16::MAX), 0);
        assert_eq!(advance_reliable_sequence(u16::MAX - 1, 2), 0);
    }

    fn queue_ordered_successor_for_test(state: &mut SessionState, packet: &[u8]) {
        let view = MFrameView::parse(packet).expect("ordered successor test frame");
        let transport_payload_identity = server_transport_identity_for_test(packet);
        state.deflate.ordered_successor_next_sequence = Some(view.sequence);
        state.deflate.ordered_successor_final_sequence = Some(view.sequence);
        state.deflate.ordered_successor_events.push_back(
            reassembly::BufferedInterleavedServerPacket {
                packet: packet.to_vec(),
                sequence: view.sequence,
                server_peer_ack_sequence: view.ack_sequence,
                server_origin_generation: 0,
                transport_payload_identity,
            },
        );
    }

    fn server_transport_identity_for_test(packet: &[u8]) -> Vec<u8> {
        let view = MFrameView::parse(packet).expect("server transport identity test frame");
        transport_identity::server_reliable_data_transport_identity(packet, &view)
            .expect("server type-0 transport identity")
    }

    fn strict_session_translator_for_test() -> crate::translate::SessionTranslator {
        let template = crate::translate::Translator {
            strict_translate: true,
            strict_profile: crate::config::StrictProfile::Player,
            diamond_identity: crate::identity::DiamondIdentity::default(),
            bncs_private_build: 8109,
            bncs_build_field: 3,
            bnxr_nwsync_advertisement: None,
            server_port: 5121,
            discovery_session_name: "test-session",
            discovery_module_name: "test-module",
            module_resources: crate::translate::module_resources::ModuleResourceRuntime::default(),
            synthetic_area_loadbar: true,
            quickbar_item_refresh_hint: None,
        };
        template.new_session(5122)
    }

    fn pending_ordered_successor_sequence(state: &SessionState) -> Option<u16> {
        state
            .deflate
            .ordered_successor_pending_validation
            .as_ref()
            .map(|token| token.sequence)
    }

    #[test]
    fn raw_ordered_successor_dequeues_only_after_final_emit_validation() {
        let payload = crate::translate::loadbar::start_payload(2);
        let first = client_reliable_m_frame(42, 75, &payload);
        let mut state = SessionState::default();
        queue_ordered_successor_for_test(&mut state, &first);

        let first_emit = translate_server_to_client(&first, &mut state)
            .expect("queued successor should reach its typed dispatcher");
        assert!(!matches!(first_emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(42));
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(42));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(state.deflate.ordered_successor_events[0].packet, first);

        finish_server_to_client_emit_validation(&mut state, false);
        assert_eq!(pending_ordered_successor_sequence(&state), None);
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(42));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(
            state.deflate.ordered_successor_events[0].server_peer_ack_sequence,
            75
        );

        let mut retry = client_reliable_m_frame(42, 76, &payload);
        retry[7] |= transport_identity::SEND_WINDOW_BIT6_MASK;
        assert!(encode_legacy_m_crc(&mut retry));
        let retry_emit = translate_server_to_client(&retry, &mut state)
            .expect("exact retransmit should retry from the retained raw queue");
        assert!(!matches!(retry_emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(42));
        assert_eq!(state.deflate.ordered_successor_events[0].packet, retry);
        assert_eq!(
            state.deflate.ordered_successor_events[0].server_peer_ack_sequence,
            76
        );
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(pending_ordered_successor_sequence(&state), None);
        assert_eq!(state.deflate.ordered_successor_next_sequence, None);
        assert_eq!(state.deflate.ordered_successor_final_sequence, None);
        assert!(state.deflate.ordered_successor_events.is_empty());
    }

    #[test]
    fn pending_ordered_successor_preserves_newer_server_ack_through_rollback() {
        let payload = crate::translate::loadbar::start_payload(2);
        let first = client_reliable_m_frame(43, 75, &payload);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(state::InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 7,
                synthetic_sequence: 76,
                ..Default::default()
            });
        queue_ordered_successor_for_test(&mut state, &first);

        let first_emit = translate_server_to_client(&first, &mut state)
            .expect("ordered successor should await outer validation");
        assert!(!matches!(first_emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(43));
        assert_eq!(
            state
                .inventory_equipment
                .last_acknowledged_client_gui_status_update_index,
            None
        );

        let retry = client_reliable_m_frame(43, 76, &payload);
        assert!(matches!(
            translate_server_to_client(&retry, &mut state)
                .expect("pending successor retransmit should fail closed"),
            Emit::Drop
        ));
        assert_eq!(
            state.server_reliable_slots.latest_peer_ack_sequence,
            Some(76)
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_acknowledged_client_gui_status_update_index,
            Some(7)
        );

        finish_server_to_client_emit_validation(&mut state, false);
        assert_eq!(pending_ordered_successor_sequence(&state), None);
        assert_eq!(
            state
                .inventory_equipment
                .last_acknowledged_client_gui_status_update_index,
            Some(7),
            "rolling back the earlier candidate must retain the later valid ACK"
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_acknowledged_client_gui_status_server_ack_sequence,
            Some(76)
        );
    }

    #[test]
    fn coalesced_first_pass_error_rolls_back_complete_window_effects() {
        let source_payload = [
            0x50, 0x09, 0x01, 0x16, 0, 0, 0, 0xC3, 0xFF, 0xFF, 0xFF, 7, 0, 0, 0, b'c', b'h', b'e',
            b'e', b's', b'e', b'7', 0x64,
        ];
        let mut packet = reliable_server_m_frame(44, 80, 0x0A, 1, &source_payload);
        let mut invalid_payload = 64u32.to_le_bytes().to_vec();
        invalid_payload.extend_from_slice(&[0xFF; 8]);
        let mut invalid_span = vec![0u8; crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        invalid_span[7] = 0x04;
        assert!(write_be_u16(&mut invalid_span, 8, 1));
        assert!(write_be_u16(
            &mut invalid_span,
            10,
            invalid_payload.len() as u16
        ));
        invalid_span.extend_from_slice(&invalid_payload);
        packet.extend_from_slice(&invalid_span);
        assert!(encode_legacy_m_crc(&mut packet));
        let view = MFrameView::parse(&packet).expect("coalesced test packet should parse");
        assert!(view.trailing_payload_length > 0);
        let mut state = SessionState::default();

        let first_error = translate_server_to_client_inner(&packet, &mut state)
            .expect_err("invalid later deflated sibling should reject the whole window");
        assert!(
            first_error
                .to_string()
                .contains("failed to inflate server gameplay payload")
        );
        assert!(state.coalesced_replay.completed_windows.is_empty());
        assert!(state.coalesced_replay.completed_direct_records.is_empty());
        assert!(state.coalesced_replay.completed_deflated_records.is_empty());
        assert!(state.deflate.server_zlib_inflater.is_none());
        assert!(state.semantic.recent_events.is_empty());

        let retry_error = translate_server_to_client_inner(&packet, &mut state)
            .expect_err("exact retry must fail from the same clean transaction boundary");
        assert!(
            retry_error
                .to_string()
                .contains("failed to inflate server gameplay payload")
        );
        assert!(state.coalesced_replay.completed_windows.is_empty());
        assert!(state.coalesced_replay.completed_direct_records.is_empty());
        assert!(state.deflate.server_zlib_inflater.is_none());
        assert!(state.semantic.recent_events.is_empty());
    }

    #[test]
    fn ordinary_coalesced_window_rolls_back_after_automatic_strict_reject() {
        // The core reader accepts both records. A deliberately invalid due
        // synthetic prefix then makes the final mixed batch fail strict
        // validation, proving the whole coalesced transaction remains
        // speculative through SessionTranslator::validate_emit.
        let mut packet = client_reliable_m_frame(47, 75, &[0x70, 0x02, 0x0C]);
        let trailing_payload = [b'P', 0x09, 0x05];
        let mut trailing = vec![0u8; crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        assert!(write_be_u16(&mut trailing, 3, 48));
        assert!(write_be_u16(&mut trailing, 5, 75));
        trailing[7] = 0x0A;
        assert!(write_be_u16(&mut trailing, 8, 1));
        assert!(write_be_u16(
            &mut trailing,
            10,
            trailing_payload.len() as u16
        ));
        trailing.extend_from_slice(&trailing_payload);
        packet.extend_from_slice(&trailing);
        assert!(encode_legacy_m_crc(&mut packet));
        let view = MFrameView::parse(&packet).expect("ordinary coalesced test window");
        assert!(view.crc_valid);
        assert_eq!(view.packetized_sequence, 1);
        assert!(view.trailing_payload_length > 0);

        let mut translator = strict_session_translator_for_test();
        translator
            .m_state
            .sequence
            .latest_client_sequence_from_client = Some(31);
        translator
            .m_state
            .synthetic_area
            .pending_server_to_client_packets
            .push(synthetic_area::PendingServerPacket {
                family: VerifiedFamily::LoadBar,
                packet: vec![b'M'],
                due_at: Instant::now(),
                reason: "ordinary coalesced strict rejection callback test",
                placement: synthetic_area::PendingServerPacketPlacement::BeforeCurrentEmit,
            });

        let emit = translator.translate(crate::packet::Direction::ServerToClient, &packet);

        let Emit::Packets(packets) = emit else {
            panic!("strict-rejected successor batch should leave one exact ACK carrier");
        };
        assert_eq!(packets.len(), 1);
        let carrier = MFrameView::parse(&packets[0]).expect("strict-rejection ACK carrier");
        assert_eq!(carrier.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(carrier.sequence, 0);
        assert_eq!(carrier.ack_sequence, 75);
        assert!(carrier.is_exact_control_frame());
        assert!(
            translator
                .m_state
                .deflate
                .last_server_core_dispatch_accepted,
            "the coalesced core must succeed before only the final strict owner rejects the batch"
        );
        assert!(
            translator
                .m_state
                .deflate
                .ordered_successor_effect_snapshot
                .is_none()
        );
        assert_eq!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .len(),
            1,
            "strict rejection restores the synthetic packet drained by speculative finalization"
        );
        assert_eq!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets[0]
                .packet,
            vec![b'M']
        );
        assert!(
            translator
                .m_state
                .sequence
                .pending_client_to_server_packets
                .is_empty()
        );
        assert!(
            translator
                .m_state
                .sequence
                .client_sequence_shifts
                .is_empty()
        );
        assert_eq!(
            translator
                .m_state
                .sequence
                .latest_client_sequence_from_client,
            Some(31)
        );
        assert_eq!(
            translator
                .m_state
                .login_waypoint
                .last_server_get_waypoint_sequence,
            None
        );
        assert_eq!(
            translator
                .m_state
                .login_waypoint
                .synthetic_empty_response_count,
            0
        );
        assert!(translator.m_state.semantic.recent_events.is_empty());
        assert!(
            translator
                .m_state
                .coalesced_replay
                .completed_windows
                .is_empty()
        );
        assert!(
            translator
                .m_state
                .coalesced_replay
                .completed_direct_records
                .is_empty()
        );
        assert!(
            translator
                .m_state
                .coalesced_replay
                .completed_deflated_records
                .is_empty()
        );
        assert!(
            translator
                .m_state
                .deflate
                .completed_server_reliable_stream_slots
                .is_empty()
        );
        assert_eq!(
            translator.m_state.sequence.latest_server_sequence_to_client,
            None
        );
        assert!(translator.m_state.deflate.server_zlib_inflater.is_none());
        assert_eq!(
            translator.m_state.server_reliable_slots.receive_start,
            Some(47),
            "source reliable-generation observation remains transport truth above the effect boundary"
        );
    }

    #[test]
    fn direct_ordered_successor_effects_are_atomic_at_final_validation() {
        // Diamond `Login_GetWaypoint` has no body. A proven direct dispatch
        // observes the semantic packet and queues one exact synthetic client
        // response, which gives this transaction test independent semantic,
        // packet-queue, sequence-shift, and replay-cache effects without a
        // capture-specific fixture.
        let payload = [0x70, 0x02, 0x0C];
        let first = client_reliable_m_frame(46, 75, &payload);
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(31);
        queue_ordered_successor_for_test(&mut state, &first);

        let first_emit = translate_server_to_client(&first, &mut state)
            .expect("ordered direct semantic successor");
        assert!(!matches!(first_emit, Emit::Drop));
        assert!(
            state.deflate.ordered_successor_effect_snapshot.is_some(),
            "engine-facing effects remain speculative until outer validation"
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.sequence.client_sequence_shifts.len(), 1);
        assert_eq!(
            state.login_waypoint.last_server_get_waypoint_sequence,
            Some(46)
        );
        assert_eq!(state.semantic.recent_events.len(), 1);
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);

        finish_server_to_client_emit_validation(&mut state, false);

        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
        assert!(state.sequence.pending_client_to_server_packets.is_empty());
        assert!(state.sequence.client_sequence_shifts.is_empty());
        assert_eq!(state.sequence.latest_client_sequence_from_client, Some(31));
        assert_eq!(state.login_waypoint.last_server_get_waypoint_sequence, None);
        assert_eq!(state.login_waypoint.synthetic_empty_response_count, 0);
        assert!(state.semantic.recent_events.is_empty());
        assert!(state.direct_server_semantic_replays.completed.is_empty());
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(46));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);

        let retry = client_reliable_m_frame(46, 76, &payload);
        let retry_emit = translate_server_to_client(&retry, &mut state)
            .expect("retained raw successor should replay the complete transaction");
        assert!(!matches!(retry_emit, Emit::Drop));
        finish_server_to_client_emit_validation(&mut state, true);

        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.sequence.client_sequence_shifts.len(), 1);
        assert_eq!(
            state.login_waypoint.last_server_get_waypoint_sequence,
            Some(46)
        );
        assert_eq!(state.login_waypoint.synthetic_empty_response_count, 1);
        assert_eq!(state.semantic.recent_events.len(), 1);
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
        assert_eq!(state.deflate.ordered_successor_next_sequence, None);
        assert!(state.deflate.ordered_successor_events.is_empty());
    }

    #[test]
    fn ordered_successor_without_validation_token_rolls_back_fail_closed() {
        let packet = client_reliable_m_frame(49, 75, &[0x70, 0x02, 0x0C]);
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(31);
        queue_ordered_successor_for_test(&mut state, &packet);

        let emit = translate_server_to_client(&packet, &mut state)
            .expect("ordered successor should stage engine-facing effects");
        assert!(!matches!(emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(49));
        assert_eq!(
            state.deflate.server_emit_effect_transaction_kind,
            Some(state::ServerEmitEffectTransactionKind::OrderedSuccessor)
        );

        state.deflate.ordered_successor_pending_validation = None;
        finish_server_to_client_emit_validation(&mut state, true);

        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
        assert_eq!(state.deflate.server_emit_effect_transaction_kind, None);
        assert!(state.sequence.pending_client_to_server_packets.is_empty());
        assert!(state.sequence.client_sequence_shifts.is_empty());
        assert!(state.semantic.recent_events.is_empty());
        assert!(state.direct_server_semantic_replays.completed.is_empty());
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(49));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
    }

    #[test]
    fn persistent_inflater_ordered_successor_is_atomic_through_final_validation() {
        fn compress_stream_chunk(compressor: &mut flate2::Compress, payload: &[u8]) -> Vec<u8> {
            let before_out = compressor.total_out();
            let mut output = vec![0u8; payload.len().saturating_mul(2).saturating_add(128)];
            compressor
                .compress(payload, &mut output, flate2::FlushCompress::Sync)
                .expect("persistent stream compression should succeed");
            output.truncate((compressor.total_out() - before_out) as usize);
            output
        }

        fn stream_frame(
            sequence: u16,
            ack_sequence: u16,
            inflated: &[u8],
            compressed: &[u8],
        ) -> Vec<u8> {
            let mut payload = (inflated.len() as u32).to_le_bytes().to_vec();
            payload.extend_from_slice(compressed);
            reliable_server_m_frame(sequence, ack_sequence, 0x0D, 1, &payload)
        }

        fn custom_token_payload(token_id: u32, value: &[u8]) -> Vec<u8> {
            let declared = 3 + 4 + 4 + 4 + value.len();
            let mut payload = vec![b'P', 0x32, 0x01];
            payload.extend_from_slice(&(declared as u32).to_le_bytes());
            payload.extend_from_slice(&token_id.to_le_bytes());
            payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
            payload.extend_from_slice(value);
            payload.push(0x60);
            payload
        }

        // Repeat one incompressible-looking token value across otherwise
        // distinct exact semantic messages. Later records therefore use the
        // prior 32 KiB dictionary rather than a same-record repetition, and
        // cannot be decoded as independent raw windows.
        let mut token_value = Vec::with_capacity(2048);
        let mut random = 0xA5C3_7E19u32;
        for _ in 0..2048 {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            token_value.push(random as u8);
        }
        let first_inflated = custom_token_payload(2, &token_value);
        let second_inflated = custom_token_payload(3, &token_value);
        let third_inflated = custom_token_payload(4, &token_value);
        let mut compressor = flate2::Compress::new(flate2::Compression::default(), true);
        let first_compressed = compress_stream_chunk(&mut compressor, &first_inflated);
        let second_compressed = compress_stream_chunk(&mut compressor, &second_inflated);
        let third_compressed = compress_stream_chunk(&mut compressor, &third_inflated);
        assert!(looks_like_zlib_wrapped_deflate(&first_compressed));
        assert!(!looks_like_zlib_wrapped_deflate(&second_compressed));
        let fresh_second = deflate::inflate_with_window(
            &second_compressed,
            second_inflated.len(),
            false,
            flate2::FlushDecompress::Sync,
        )
        .expect("fresh-window probe");
        assert_ne!(
            fresh_second.as_deref(),
            Some(second_inflated.as_slice()),
            "the ordered member must require the already-live inflater history"
        );
        let fresh_third = deflate::inflate_with_window(
            &third_compressed,
            third_inflated.len(),
            false,
            flate2::FlushDecompress::Sync,
        )
        .expect("fresh-window probe");
        assert_ne!(
            fresh_third.as_deref(),
            Some(third_inflated.as_slice()),
            "the post-commit member must require the accepted successor history"
        );

        let first = stream_frame(140, 74, &first_inflated, &first_compressed);
        let second = stream_frame(141, 75, &second_inflated, &second_compressed);
        let third = stream_frame(142, 77, &third_inflated, &third_compressed);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        let first_emit = translate_server_to_client(&first, &mut state)
            .expect("first stream member should seed the persistent inflater");
        assert!(!matches!(first_emit, Emit::Drop));
        finish_server_to_client_emit_validation(&mut state, true);
        let baseline_totals = state
            .deflate
            .server_zlib_inflater
            .as_ref()
            .map(|inflater| (inflater.total_in(), inflater.total_out()))
            .expect("first stream member should leave a live inflater");
        let baseline_windows = state.deflate.completed_server_stream_windows.len();
        let baseline_slots = state.deflate.completed_server_reliable_stream_slots.len();
        let baseline_semantic_events = state.semantic.recent_events.len();

        queue_ordered_successor_for_test(&mut state, &second);
        let speculative_emit = translate_server_to_client(&second, &mut state)
            .expect("ordered persistent-stream successor should translate speculatively");
        assert!(!matches!(speculative_emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(141));
        assert!(state.deflate.ordered_successor_effect_snapshot.is_some());
        let speculative_totals = state
            .deflate
            .server_zlib_inflater
            .as_ref()
            .map(|inflater| (inflater.total_in(), inflater.total_out()))
            .expect("speculative successor should advance the working inflater");
        assert_eq!(
            speculative_totals,
            (
                baseline_totals.0 + second_compressed.len() as u64,
                baseline_totals.1 + second_inflated.len() as u64,
            )
        );
        assert_eq!(
            state.deflate.completed_server_stream_windows.len(),
            baseline_windows + 1
        );
        assert_eq!(
            state.deflate.completed_server_reliable_stream_slots.len(),
            baseline_slots + 1
        );
        assert_eq!(
            state.semantic.recent_events.len(),
            baseline_semantic_events + 1
        );

        finish_server_to_client_emit_validation(&mut state, false);

        assert_eq!(pending_ordered_successor_sequence(&state), None);
        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(141));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(state.deflate.ordered_successor_events[0].packet, second);
        assert_eq!(
            state
                .deflate
                .server_zlib_inflater
                .as_ref()
                .map(|inflater| (inflater.total_in(), inflater.total_out())),
            Some(baseline_totals),
            "strict rejection must restore the exact pre-successor inflater"
        );
        assert_eq!(
            state.deflate.completed_server_stream_windows.len(),
            baseline_windows
        );
        assert_eq!(
            state.deflate.completed_server_reliable_stream_slots.len(),
            baseline_slots
        );
        assert_eq!(state.semantic.recent_events.len(), baseline_semantic_events);

        let retry = stream_frame(141, 76, &second_inflated, &second_compressed);
        let retry_emit = translate_server_to_client(&retry, &mut state)
            .expect("exact retransmit should reuse the restored inflater history");
        assert!(!matches!(retry_emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(141));
        assert_eq!(state.deflate.ordered_successor_events[0].packet, retry);
        assert_eq!(
            state
                .deflate
                .server_zlib_inflater
                .as_ref()
                .map(|inflater| (inflater.total_in(), inflater.total_out())),
            Some(speculative_totals),
            "retry must advance the working inflater from the restored checkpoint"
        );
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.deflate.ordered_successor_next_sequence, None);
        assert!(state.deflate.ordered_successor_events.is_empty());
        assert_eq!(
            state
                .deflate
                .server_zlib_inflater
                .as_ref()
                .map(|inflater| (inflater.total_in(), inflater.total_out())),
            Some(speculative_totals),
            "accepted retransmit must commit exactly one inflater advancement"
        );

        let third_emit = translate_server_to_client(&third, &mut state)
            .expect("third history-dependent member should use committed successor state");
        assert!(!matches!(third_emit, Emit::Drop));
        assert_eq!(
            state
                .deflate
                .server_zlib_inflater
                .as_ref()
                .map(|inflater| (inflater.total_in(), inflater.total_out())),
            Some((
                speculative_totals.0 + third_compressed.len() as u64,
                speculative_totals.1 + third_inflated.len() as u64,
            ))
        );
        assert_eq!(
            state.deflate.completed_server_stream_windows.len(),
            baseline_windows + 2
        );
        assert_eq!(
            state.semantic.recent_events.len(),
            baseline_semantic_events + 2
        );
    }

    #[test]
    fn raw_ordered_successor_rejects_conflicting_retransmit_without_dequeue() {
        let expected_payload = crate::translate::loadbar::start_payload(2);
        let expected = client_reliable_m_frame(43, 75, &expected_payload);
        let mut state = SessionState::default();
        queue_ordered_successor_for_test(&mut state, &expected);

        let conflicting_payload = crate::translate::loadbar::start_payload(3);
        let conflicting = client_reliable_m_frame(43, 76, &conflicting_payload);
        assert!(matches!(
            translate_server_to_client(&conflicting, &mut state)
                .expect("conflicting retransmit should fail closed"),
            Emit::Drop
        ));
        assert_eq!(pending_ordered_successor_sequence(&state), None);
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(43));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(state.deflate.ordered_successor_events[0].packet, expected);
    }

    #[test]
    fn ordered_successor_merge_is_transactional_on_late_capacity_failure() {
        let mut state = SessionState::default();
        for sequence in 0..(MAX_INTERLEAVED_PACKETS - 1) as u16 {
            let packet =
                client_reliable_m_frame(sequence, 75, &crate::translate::loadbar::start_payload(2));
            let view = MFrameView::parse(&packet).expect("queued capacity-test frame");
            state.deflate.ordered_successor_events.push_back(
                reassembly::BufferedInterleavedServerPacket {
                    packet: packet.clone(),
                    sequence,
                    server_peer_ack_sequence: view.ack_sequence,
                    server_origin_generation: 0,
                    transport_payload_identity: server_transport_identity_for_test(&packet),
                },
            );
        }
        let before = state.deflate.ordered_successor_events.clone();
        let candidates = [200_u16, 201_u16]
            .into_iter()
            .map(|sequence| {
                let packet = client_reliable_m_frame(
                    sequence,
                    76,
                    &crate::translate::loadbar::start_payload(3),
                );
                reassembly::BufferedInterleavedServerPacket {
                    packet: packet.clone(),
                    sequence,
                    server_peer_ack_sequence: 76,
                    server_origin_generation: 0,
                    transport_payload_identity: server_transport_identity_for_test(&packet),
                }
            })
            .collect::<Vec<_>>();

        assert!(merge_ordered_server_successor_events(&mut state, &candidates).is_err());
        assert_eq!(state.deflate.ordered_successor_events.len(), before.len());
        for (actual, expected) in state
            .deflate
            .ordered_successor_events
            .iter()
            .zip(before.iter())
        {
            assert_eq!(actual.sequence, expected.sequence);
            assert_eq!(
                actual.server_origin_generation,
                expected.server_origin_generation
            );
            assert_eq!(
                actual.server_peer_ack_sequence,
                expected.server_peer_ack_sequence
            );
            assert_eq!(actual.packet, expected.packet);
            assert_eq!(
                actual.transport_payload_identity,
                expected.transport_payload_identity
            );
        }
    }

    #[test]
    fn session_translator_commits_ordered_successor_after_automatic_strict_accept() {
        let packet = client_reliable_m_frame(44, 75, &crate::translate::loadbar::start_payload(2));
        let mut translator = strict_session_translator_for_test();
        queue_ordered_successor_for_test(&mut translator.m_state, &packet);

        let emit = translator.translate(crate::packet::Direction::ServerToClient, &packet);

        assert!(!matches!(emit, Emit::Drop));
        assert_eq!(
            pending_ordered_successor_sequence(&translator.m_state),
            None
        );
        assert_eq!(
            translator.m_state.deflate.ordered_successor_next_sequence,
            None
        );
        assert!(
            translator
                .m_state
                .deflate
                .ordered_successor_events
                .is_empty()
        );
    }

    #[test]
    fn session_translator_retains_ordered_successor_after_automatic_strict_reject() {
        let packet = client_reliable_m_frame(45, 75, &crate::translate::loadbar::start_payload(2));
        let mut translator = strict_session_translator_for_test();
        queue_ordered_successor_for_test(&mut translator.m_state, &packet);
        translator
            .m_state
            .synthetic_area
            .pending_server_to_client_packets
            .push(synthetic_area::PendingServerPacket {
                family: VerifiedFamily::LoadBar,
                packet: vec![b'M'],
                due_at: Instant::now(),
                reason: "strict rejection callback test",
                placement: synthetic_area::PendingServerPacketPlacement::BeforeCurrentEmit,
            });

        let emit = translator.translate(crate::packet::Direction::ServerToClient, &packet);

        let Emit::Packets(packets) = emit else {
            panic!("strict-rejected successor batch should leave one exact ACK carrier");
        };
        assert_eq!(packets.len(), 1);
        let carrier = MFrameView::parse(&packets[0]).expect("strict-rejection ACK carrier");
        assert_eq!(carrier.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(carrier.sequence, 0);
        assert_eq!(carrier.ack_sequence, 75);
        assert!(carrier.is_exact_control_frame());
        assert_eq!(
            pending_ordered_successor_sequence(&translator.m_state),
            None
        );
        assert_eq!(
            translator.m_state.deflate.ordered_successor_next_sequence,
            Some(45)
        );
        assert_eq!(translator.m_state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(
            translator.m_state.deflate.ordered_successor_events[0].packet,
            packet
        );
        assert_eq!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .len(),
            1,
            "strict rejection restores the due packet drained by speculative finalization"
        );
        assert!(translator.m_state.semantic.recent_events.is_empty());
        assert!(
            translator
                .m_state
                .direct_server_semantic_replays
                .completed
                .is_empty()
        );
        assert_eq!(
            translator.m_state.sequence.latest_server_sequence_to_client, None,
            "a rejected batch must not become an ACK source for later synthetic client packets"
        );
        assert_eq!(
            translator.m_state.server_reliable_slots.receive_start,
            Some(45),
            "valid source reliable-window observation survives client-facing strict rejection"
        );
        assert!(
            translator
                .m_state
                .deflate
                .ordered_successor_effect_snapshot
                .is_none()
        );
    }

    #[test]
    fn ordered_successor_lane_uses_frame_type_and_accepts_type_zero_sequence_zero() {
        let data = client_reliable_m_frame(0, 75, &crate::translate::loadbar::start_payload(2));
        let mut state = SessionState::default();
        queue_ordered_successor_for_test(&mut state, &data);

        let data_emit = translate_server_to_client(&data, &mut state)
            .expect("type-0 sequence-zero data should use the ordered lane");
        assert!(!matches!(data_emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(0));
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.deflate.ordered_successor_next_sequence, None);
        assert!(state.deflate.ordered_successor_events.is_empty());

        let expected =
            client_reliable_m_frame(50, 76, &crate::translate::loadbar::start_payload(3));
        queue_ordered_successor_for_test(&mut state, &expected);
        let control = reliable_server_m_frame(99, 77, 0x10, 0, &[]);
        let control_emit = translate_server_to_client(&control, &mut state)
            .expect("type-1 control should bypass ordered data at any sequence");
        assert!(!matches!(control_emit, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), None);
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(50));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
    }

    #[test]
    fn stream_helper_withheld_suffix_persists_complete_raw_successor_event() {
        let successor =
            client_reliable_m_frame(52, 81, &crate::translate::loadbar::start_payload(2));
        let view = MFrameView::parse(&successor).expect("raw successor frame");
        let event = reassembly::BufferedInterleavedServerPacket {
            packet: successor.clone(),
            sequence: view.sequence,
            server_peer_ack_sequence: 91,
            server_origin_generation: 7,
            transport_payload_identity: server_transport_identity_for_test(&successor),
        };
        let reassembly = ServerDeflatedReassembly {
            inflated_length: 32,
            expected_frames: 2,
            first_sequence: 50,
            server_origin_generation: 7,
            packetized_sequence: 2,
            zlib_stream: true,
            frames: Vec::new(),
            interleaved_packets: Vec::new(),
            interleaved_events: vec![event.clone()],
        };
        let mut state = SessionState::default();

        arm_withheld_reassembly_successors(&mut state, &reassembly)
            .expect("raw successor suffix should arm");

        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(52));
        assert_eq!(state.deflate.ordered_successor_final_sequence, Some(52));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        let queued = &state.deflate.ordered_successor_events[0];
        assert_eq!(queued.packet, successor);
        assert_eq!(queued.server_peer_ack_sequence, 91);
        assert_eq!(queued.server_origin_generation, 7);
        assert_eq!(
            queued.transport_payload_identity,
            event.transport_payload_identity
        );
    }

    #[test]
    fn nested_stream_helper_suffix_merges_without_clobbering_active_raw_queue() {
        let active = client_reliable_m_frame(61, 80, &crate::translate::loadbar::start_payload(2));
        let later = client_reliable_m_frame(62, 81, &crate::translate::loadbar::start_payload(3));
        let later_view = MFrameView::parse(&later).expect("later raw successor");
        let mut state = SessionState::default();
        queue_ordered_successor_for_test(&mut state, &active);
        let reassembly = ServerDeflatedReassembly {
            inflated_length: 32,
            expected_frames: 1,
            first_sequence: 61,
            server_origin_generation: 0,
            packetized_sequence: 1,
            zlib_stream: true,
            frames: Vec::new(),
            interleaved_packets: Vec::new(),
            interleaved_events: vec![reassembly::BufferedInterleavedServerPacket {
                packet: later.clone(),
                sequence: later_view.sequence,
                server_peer_ack_sequence: later_view.ack_sequence,
                server_origin_generation: 0,
                transport_payload_identity: server_transport_identity_for_test(&later),
            }],
        };

        arm_withheld_reassembly_successors(&mut state, &reassembly)
            .expect("nested raw successor suffix should merge");

        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(61));
        assert_eq!(state.deflate.ordered_successor_final_sequence, Some(62));
        assert_eq!(state.deflate.ordered_successor_events.len(), 2);
        assert_eq!(state.deflate.ordered_successor_events[0].packet, active);
        assert_eq!(state.deflate.ordered_successor_events[1].packet, later);
    }

    #[test]
    fn nested_stream_helper_suffix_conflict_fails_closed_and_retains_first_event() {
        let first = client_reliable_m_frame(63, 80, &crate::translate::loadbar::start_payload(2));
        let conflicting =
            client_reliable_m_frame(63, 81, &crate::translate::loadbar::start_payload(3));
        let conflicting_view = MFrameView::parse(&conflicting).expect("conflicting raw successor");
        let mut state = SessionState::default();
        queue_ordered_successor_for_test(&mut state, &first);
        let reassembly = ServerDeflatedReassembly {
            inflated_length: 32,
            expected_frames: 1,
            first_sequence: 62,
            server_origin_generation: 0,
            packetized_sequence: 1,
            zlib_stream: true,
            frames: Vec::new(),
            interleaved_packets: Vec::new(),
            interleaved_events: vec![reassembly::BufferedInterleavedServerPacket {
                packet: conflicting.clone(),
                sequence: conflicting_view.sequence,
                server_peer_ack_sequence: conflicting_view.ack_sequence,
                server_origin_generation: 0,
                transport_payload_identity: server_transport_identity_for_test(&conflicting),
            }],
        };

        assert!(arm_withheld_reassembly_successors(&mut state, &reassembly).is_err());
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(63));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(state.deflate.ordered_successor_events[0].packet, first);
    }

    #[test]
    fn packetized_raw_ordered_successor_commits_each_stored_frame_in_order() {
        let payload = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(54, 82, &payload);
        let first_view = MFrameView::parse(&first).expect("first packetized successor");
        let second_view = MFrameView::parse(&second).expect("second packetized successor");
        let mut state = SessionState::default();
        state.deflate.ordered_successor_next_sequence = Some(54);
        state.deflate.ordered_successor_final_sequence = Some(55);
        for (packet, view) in [(&first, &first_view), (&second, &second_view)] {
            state.deflate.ordered_successor_events.push_back(
                reassembly::BufferedInterleavedServerPacket {
                    packet: packet.clone(),
                    sequence: view.sequence,
                    server_peer_ack_sequence: view.ack_sequence,
                    server_origin_generation: 0,
                    transport_payload_identity: server_transport_identity_for_test(packet),
                },
            );
        }

        let first_emit = translate_server_to_client(&first, &mut state)
            .expect("first stored frame should enter reassembly");
        assert!(matches!(first_emit, Emit::Consumed));
        assert!(state.deflate.server_reassembly.is_some());
        assert_eq!(pending_ordered_successor_sequence(&state), Some(54));
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(55));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);

        let second_emit = translate_server_to_client(&second, &mut state)
            .expect("second stored frame should complete typed reassembly");
        assert!(!matches!(second_emit, Emit::Drop));
        assert!(state.deflate.server_reassembly.is_none());
        assert_eq!(pending_ordered_successor_sequence(&state), Some(55));
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.deflate.ordered_successor_next_sequence, None);
        assert!(state.deflate.ordered_successor_events.is_empty());
    }

    #[test]
    fn interleaved_area_commits_only_after_exact_deflated_predecessor() {
        let predecessor = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(50, 74, &predecessor);
        let area_payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let area_frame = client_reliable_m_frame(52, 75, area_payload);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        assert!(matches!(
            translate_server_to_client(&first, &mut state).expect("start predecessor"),
            Emit::Consumed
        ));
        assert!(matches!(
            translate_server_to_client(&area_frame, &mut state).expect("buffer Area"),
            Emit::Consumed
        ));
        let area_retransmit = client_reliable_m_frame(52, 76, area_payload);
        assert!(matches!(
            translate_server_to_client(&area_retransmit, &mut state)
                .expect("refresh buffered Area transport"),
            Emit::Consumed
        ));
        assert_eq!(
            state
                .deflate
                .server_reassembly
                .as_ref()
                .expect("pending predecessor")
                .interleaved_events
                .len(),
            1
        );
        assert!(
            state
                .area_context
                .latest_area_placeables
                .area_resref
                .is_empty()
        );
        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert!(state.synthetic_area.server_hold_gate.is_none());
        assert!(state.synthetic_area.pending_area_loaded.is_none());
        assert!(state.direct_server_semantic_replays.completed.is_empty());

        let packets = proof_packets(
            translate_server_to_client(&second, &mut state).expect("complete predecessor"),
        );
        let sequences = packets
            .iter()
            .filter_map(|(_, packet)| MFrameView::parse(packet).map(|view| view.sequence))
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![50, 51, 52]);
        let (area_proof, rewritten_area) = packets.last().expect("Area output after predecessor");
        assert!(area_proof.contains_family(VerifiedFamily::AreaClientArea));
        let area_view = MFrameView::parse(rewritten_area).expect("rewritten Area frame");
        assert_eq!(area_view.ack_sequence, 76);
        let rewritten_payload = parse_window::primary_payload(rewritten_area, &area_view)
            .expect("rewritten Area payload");
        assert!(area::ee_area_client_area_payload_shape_valid(
            rewritten_payload
        ));
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert!(state.synthetic_area.server_hold_gate.is_some());
        assert!(state.synthetic_area.pending_area_loaded.is_some());
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
        assert_eq!(pending_ordered_successor_sequence(&state), Some(52));
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.deflate.ordered_successor_next_sequence, None);
        assert!(state.deflate.ordered_successor_events.is_empty());

        translate_server_to_client(&client_reliable_m_frame(52, 77, area_payload), &mut state)
            .expect("post-commit Area retransmit should replay");
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(state.direct_server_semantic_replays.duplicates_replayed, 1);
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
    }

    #[test]
    fn interleaved_area_completion_preserves_proofs_with_synthetic_loadbar_enabled() {
        let predecessor = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(55, 74, &predecessor);
        let area_payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let mut state = SessionState::default();

        translate_server_to_client(&first, &mut state).expect("start predecessor");
        translate_server_to_client(&client_reliable_m_frame(57, 75, area_payload), &mut state)
            .expect("buffer Area");
        let packets = proof_packets(
            translate_server_to_client(&second, &mut state).expect("complete predecessor"),
        );
        assert_eq!(pending_ordered_successor_sequence(&state), Some(57));
        finish_server_to_client_emit_validation(&mut state, true);

        assert!(
            packets
                .iter()
                .any(|(proof, _)| proof.contains_family(VerifiedFamily::AreaClientArea))
        );
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
    }

    #[test]
    fn buffered_direct_area_reject_restores_effects_and_retains_raw_retry() {
        let predecessor = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(58, 74, &predecessor);
        let area_payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let area = client_reliable_m_frame(60, 75, area_payload);
        let mut state = SessionState::default();

        assert!(matches!(
            translate_server_to_client(&first, &mut state).expect("start predecessor"),
            Emit::Consumed
        ));
        assert!(matches!(
            translate_server_to_client(&area, &mut state).expect("buffer Area successor"),
            Emit::Consumed
        ));
        finish_server_to_client_emit_validation(&mut state, true);
        let emitted = translate_server_to_client(&second, &mut state)
            .expect("complete predecessor and stage Area successor");
        assert!(!matches!(emitted, Emit::Drop));
        assert_eq!(pending_ordered_successor_sequence(&state), Some(60));
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(60));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
        assert!(!state.sequence.server_sequence_shifts.is_empty());
        assert!(state.synthetic_area.pending_area_loaded.is_some());
        assert!(state.synthetic_area.server_hold_gate.is_some());
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
        assert_eq!(
            state.deflate.server_emit_effect_transaction_kind,
            Some(state::ServerEmitEffectTransactionKind::OrderedSuccessor)
        );

        finish_server_to_client_emit_validation(&mut state, false);

        assert_eq!(pending_ordered_successor_sequence(&state), None);
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(60));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(state.deflate.ordered_successor_events[0].packet, area);
        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert!(
            state
                .area_context
                .latest_area_placeables
                .area_resref
                .is_empty()
        );
        assert!(state.sequence.server_sequence_shifts.is_empty());
        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert!(state.synthetic_area.pending_area_loaded.is_none());
        assert!(state.synthetic_area.server_hold_gate.is_none());
        assert!(state.direct_server_semantic_replays.completed.is_empty());
        let restored_source = state
            .deflate
            .server_reassembly
            .as_ref()
            .expect("rejection must restore the partial predecessor source");
        assert_eq!(restored_source.frames.len(), 1);
        assert_eq!(restored_source.interleaved_events.len(), 1);
        assert_eq!(restored_source.interleaved_events[0].packet, area);

        let retry_emit = translate_server_to_client(&second, &mut state)
            .expect("retransmitted source completion should retry the retained Area successor");
        assert!(!matches!(retry_emit, Emit::Drop));
        finish_server_to_client_emit_validation(&mut state, true);

        assert!(state.deflate.server_reassembly.is_none());
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(60));
        assert_eq!(state.deflate.ordered_successor_events.len(), 1);
        assert_eq!(state.semantic.area.client_area_packets, 0);

        let area_retry = client_reliable_m_frame(60, 76, area_payload);
        let area_retry_emit = translate_server_to_client(&area_retry, &mut state)
            .expect("retained Area successor should retry through the full dispatcher");
        assert!(!matches!(area_retry_emit, Emit::Drop));
        finish_server_to_client_emit_validation(&mut state, true);

        assert_eq!(state.deflate.ordered_successor_next_sequence, None);
        assert!(state.deflate.ordered_successor_events.is_empty());
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
        assert!(!state.sequence.server_sequence_shifts.is_empty());
        assert!(state.synthetic_area.pending_area_loaded.is_some());
        assert!(state.synthetic_area.server_hold_gate.is_some());
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
    }

    #[test]
    fn interleaved_area_stays_fail_closed_after_predecessor_bit_cursor_rejection() {
        let mut predecessor = crate::translate::loadbar::start_payload(2);
        *predecessor.last_mut().expect("LoadBar fragment byte") = 0xE0;
        let (first, second) = two_frame_deflated_window(60, 74, &predecessor);
        let area_payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let area_frame = client_reliable_m_frame(62, 75, area_payload);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        assert!(matches!(
            translate_server_to_client(&first, &mut state).expect("start predecessor"),
            Emit::Consumed
        ));
        assert!(matches!(
            translate_server_to_client(&area_frame, &mut state).expect("buffer Area"),
            Emit::Consumed
        ));
        let packets = proof_packets(
            translate_server_to_client(&second, &mut state).expect("reject predecessor"),
        );
        let (area_proof, area_shell) = packets.last().expect("fail-closed Area shell");
        assert!(area_proof.contains_family(VerifiedFamily::ConsumedEmptyMFrame));
        let shell_view = MFrameView::parse(area_shell).expect("Area progress shell");
        assert_eq!(shell_view.sequence, 62);
        assert_eq!(shell_view.payload_length, 0);
        assert!(
            state
                .area_context
                .latest_area_placeables
                .area_resref
                .is_empty()
        );
        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert!(state.synthetic_area.server_hold_gate.is_none());
        assert!(state.synthetic_area.pending_area_loaded.is_none());
        assert!(state.direct_server_semantic_replays.completed.is_empty());
    }

    #[test]
    fn interleaved_area_with_bad_source_crc_cannot_enter_ordered_commit() {
        let predecessor = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(65, 74, &predecessor);
        let area_payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let mut area_frame = client_reliable_m_frame(67, 75, area_payload);
        area_frame[1] ^= 0x01;
        assert!(
            !MFrameView::parse(&area_frame)
                .expect("corrupt Area frame")
                .crc_valid
        );
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        translate_server_to_client(&first, &mut state).expect("start predecessor");
        assert!(translate_server_to_client(&area_frame, &mut state).is_err());
        assert!(
            state
                .deflate
                .server_reassembly
                .as_ref()
                .expect("pending predecessor")
                .interleaved_events
                .is_empty()
        );
        let packets = proof_packets(
            translate_server_to_client(&second, &mut state).expect("complete predecessor"),
        );
        assert_eq!(packets.len(), 2);
        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert!(
            state
                .area_context
                .latest_area_placeables
                .area_resref
                .is_empty()
        );
        assert!(state.direct_server_semantic_replays.completed.is_empty());
    }

    #[test]
    fn interleaved_area_waits_for_retransmission_across_a_source_sequence_gap() {
        let predecessor = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(70, 74, &predecessor);
        let area_payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let area_frame = client_reliable_m_frame(73, 75, area_payload);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        translate_server_to_client(&first, &mut state).expect("start predecessor");
        translate_server_to_client(&area_frame, &mut state).expect("buffer gapped Area");
        let packets = proof_packets(
            translate_server_to_client(&second, &mut state).expect("complete predecessor"),
        );
        let sequences = packets
            .iter()
            .map(|(_, packet)| MFrameView::parse(packet).expect("reliable packet").sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![70, 71]);
        finish_server_to_client_emit_validation(&mut state, true);
        assert!(
            state
                .area_context
                .latest_area_placeables
                .area_resref
                .is_empty()
        );
        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert!(state.direct_server_semantic_replays.completed.is_empty());

        assert!(matches!(
            translate_server_to_client(&area_frame, &mut state)
                .expect("withhold repeated future Area"),
            Emit::Drop
        ));
        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert_eq!(state.deflate.ordered_successor_next_sequence, Some(72));

        let missing = client_reliable_m_frame(72, 75, &crate::translate::loadbar::start_payload(3));
        let missing_packets = proof_packets(
            translate_server_to_client(&missing, &mut state).expect("receive missing predecessor"),
        );
        assert_eq!(
            MFrameView::parse(&missing_packets[0].1)
                .expect("missing predecessor packet")
                .sequence,
            72
        );
        finish_server_to_client_emit_validation(&mut state, true);
        let retransmit_packets = proof_packets(
            translate_server_to_client(&area_frame, &mut state).expect("retransmit gapped Area"),
        );
        assert!(
            retransmit_packets[0]
                .0
                .contains_family(VerifiedFamily::AreaClientArea)
        );
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
    }

    #[test]
    fn interleaved_events_commit_in_source_order_after_area_reset() {
        let predecessor = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(80, 74, &predecessor);
        let area_payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let later_loadbar = crate::translate::loadbar::start_payload(3);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        translate_server_to_client(&first, &mut state).expect("start predecessor");
        translate_server_to_client(&client_reliable_m_frame(83, 75, &later_loadbar), &mut state)
            .expect("buffer later semantic event first");
        translate_server_to_client(&client_reliable_m_frame(82, 75, area_payload), &mut state)
            .expect("buffer earlier Area second");
        assert_eq!(state.semantic.area.client_area_packets, 0);

        let packets = proof_packets(
            translate_server_to_client(&second, &mut state).expect("complete predecessor"),
        );
        let sequences = packets
            .iter()
            .map(|(_, packet)| MFrameView::parse(packet).expect("reliable packet").sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![80, 81, 82]);
        assert!(packets[2].0.contains_family(VerifiedFamily::AreaClientArea));
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
        finish_server_to_client_emit_validation(&mut state, true);

        let retried = proof_packets(
            translate_server_to_client(
                &client_reliable_m_frame(83, 76, &later_loadbar),
                &mut state,
            )
            .expect("retry later semantic event after Area commit"),
        );
        assert!(retried[0].0.contains_family(VerifiedFamily::LoadBar));
    }

    #[test]
    fn interleaved_ack_control_shell_survives_pending_predecessor_at_any_sequence() {
        let predecessor = crate::translate::loadbar::start_payload(2);
        let (first, second) = two_frame_deflated_window(90, 74, &predecessor);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        translate_server_to_client(&first, &mut state).expect("start predecessor");
        for (sequence, ack_sequence) in [(0, 76), (95, 77)] {
            let ack = reliable_server_m_frame(sequence, ack_sequence, 0x10, 0, &[]);
            let packets = proof_packets(
                translate_server_to_client(&ack, &mut state).expect("forward ACK shell"),
            );
            assert_eq!(packets.len(), 1);
            let (proof, packet) = &packets[0];
            let view = MFrameView::parse(packet).expect("ACK shell should parse");
            assert!(proof.contains_family(VerifiedFamily::ConsumedEmptyMFrame));
            assert_eq!(view.sequence, sequence);
            assert_eq!(view.ack_sequence, ack_sequence);
            assert_eq!(view.flags, 0x10);
            assert_eq!(view.frame_type, 1);
            assert_eq!(view.payload_length, 0);
            assert!(view.crc_valid);
        }
        let predecessor_packets = proof_packets(
            translate_server_to_client(&second, &mut state).expect("complete predecessor"),
        );
        assert_eq!(predecessor_packets.len(), 2);
    }

    #[test]
    fn direct_area_retransmission_applies_state_and_side_effects_once() {
        let payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let mut state = SessionState::default();

        let first = proof_packets(
            translate_server_to_client(&client_reliable_m_frame(30, 74, payload), &mut state)
                .expect("first direct Area_ClientArea should translate"),
        );
        assert_eq!(first.len(), 2);
        assert!(first[0].0.contains_family(VerifiedFamily::AreaClientArea));
        assert!(first[1].0.contains_family(VerifiedFamily::LoadBar));
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
        assert_eq!(state.semantic.area.client_area_packets, 1);
        let shifts = state
            .sequence
            .server_sequence_shifts
            .iter()
            .map(|shift| (shift.base, shift.delta))
            .collect::<Vec<_>>();
        let pending_packets = state.synthetic_area.pending_server_to_client_packets.len();

        translate_server_to_client(&client_reliable_m_frame(30, 75, payload), &mut state)
            .expect("direct Area_ClientArea retransmission should replay");
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state
                .sequence
                .server_sequence_shifts
                .iter()
                .map(|shift| (shift.base, shift.delta))
                .collect::<Vec<_>>(),
            shifts
        );
        assert_eq!(
            state.synthetic_area.pending_server_to_client_packets.len(),
            pending_packets
        );
        assert_eq!(state.direct_server_semantic_replays.duplicates_replayed, 1);

        state.synthetic_area.server_hold_gate = None;
        state.synthetic_area.pending_area_loaded = None;
        state
            .synthetic_area
            .pending_server_to_client_packets
            .clear();
        translate_server_to_client(&client_reliable_m_frame(30, 76, payload), &mut state)
            .expect("post-gate direct retransmission should still replay");
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert!(state.synthetic_area.server_hold_gate.is_none());
        assert!(state.synthetic_area.pending_area_loaded.is_none());
        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert_eq!(state.direct_server_semantic_replays.duplicates_replayed, 2);
    }

    #[test]
    fn direct_area_rejection_restores_source_effects_and_created_suffix() {
        let payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let mut state = SessionState::default();

        let emit =
            translate_server_to_client(&client_reliable_m_frame(30, 74, payload), &mut state)
                .expect("direct Area_ClientArea should reach final validation");

        assert_eq!(proof_packets(emit).len(), 2);
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert!(
            !state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert!(!state.direct_server_semantic_replays.completed.is_empty());
        assert_eq!(
            state.deflate.server_emit_effect_transaction_kind,
            Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit)
        );

        finish_server_to_client_emit_validation(&mut state, false);

        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert!(state.synthetic_area.server_hold_gate.is_none());
        assert!(state.sequence.server_sequence_shifts.is_empty());
        assert!(state.direct_server_semantic_replays.completed.is_empty());
        assert!(
            state
                .area_context
                .latest_area_placeables
                .area_resref
                .is_empty()
        );
        assert_eq!(state.deflate.server_emit_effect_transaction_kind, None);
        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
    }

    #[test]
    fn direct_loadbar_rejection_retains_source_slot_and_retries_from_clean_effects() {
        let payload = crate::translate::loadbar::start_payload(2);
        let first = client_reliable_m_frame(31, 74, &payload);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        let emitted = proof_packets(
            translate_server_to_client(&first, &mut state)
                .expect("direct LoadBar should reach final validation"),
        );
        assert_eq!(emitted.len(), 1);
        assert!(emitted[0].0.contains_family(VerifiedFamily::LoadBar));
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
        assert_eq!(state.server_reliable_slots.slots.len(), 1);
        assert_eq!(
            state.deflate.server_emit_effect_transaction_kind,
            Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit)
        );

        finish_server_to_client_emit_validation(&mut state, false);

        assert_eq!(state.semantic.area.loadbar_packets, 0);
        assert!(state.direct_server_semantic_replays.completed.is_empty());
        assert_eq!(
            state.server_reliable_slots.slots.len(),
            1,
            "strict reader rejection must retain the raw receive-window slot"
        );
        assert_eq!(state.deflate.server_emit_effect_transaction_kind, None);

        let retry = client_reliable_m_frame(31, 75, &payload);
        let retried = proof_packets(
            translate_server_to_client(&retry, &mut state)
                .expect("exact direct LoadBar retransmit should retry"),
        );
        assert_eq!(retried.len(), 1);
        assert!(retried[0].0.contains_family(VerifiedFamily::LoadBar));
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
        assert_eq!(state.server_reliable_slots.slots.len(), 1);
    }

    #[test]
    fn rejected_direct_loadbar_replay_restores_duplicate_counter() {
        let payload = crate::translate::loadbar::start_payload(2);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;

        let first = client_reliable_m_frame(32, 74, &payload);
        proof_packets(
            translate_server_to_client(&first, &mut state)
                .expect("seed direct LoadBar translation"),
        );
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.direct_server_semantic_replays.duplicates_replayed, 0);

        let replay = client_reliable_m_frame(32, 75, &payload);
        proof_packets(
            translate_server_to_client(&replay, &mut state)
                .expect("direct LoadBar replay should reach final validation"),
        );
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.direct_server_semantic_replays.duplicates_replayed, 1);
        finish_server_to_client_emit_validation(&mut state, false);
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);
        assert_eq!(state.direct_server_semantic_replays.duplicates_replayed, 0);

        let retry = client_reliable_m_frame(32, 76, &payload);
        proof_packets(
            translate_server_to_client(&retry, &mut state)
                .expect("rejected direct replay should retry exactly"),
        );
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.direct_server_semantic_replays.duplicates_replayed, 1);
    }

    #[test]
    fn server_slot_conflict_rejects_payload_but_preserves_ack_and_excludes_controls() {
        let payload = crate::translate::loadbar::start_payload(2);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        proof_packets(
            translate_server_to_client(&client_reliable_m_frame(33, 74, &payload), &mut state)
                .expect("seed server source slot"),
        );
        finish_server_to_client_emit_validation(&mut state, true);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(state::InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 7,
                synthetic_sequence: 75,
                ..Default::default()
            });

        let conflicting_payload = crate::translate::loadbar::start_payload(3);
        assert!(matches!(
            translate_server_to_client(
                &client_reliable_m_frame(33, 75, &conflicting_payload),
                &mut state,
            )
            .expect("conflicting source slot should fail closed"),
            Emit::Drop
        ));
        assert_eq!(
            state
                .inventory_equipment
                .last_observed_client_gui_status_server_peer_ack_sequence,
            Some(75)
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_acknowledged_client_gui_status_update_index,
            Some(7)
        );
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.server_reliable_slots.slots.len(), 1);

        let control = reliable_server_m_frame(33, 76, 0x10, 0, &[]);
        let control_packets = proof_packets(
            translate_server_to_client(&control, &mut state)
                .expect("exact ACK control should stay outside reliable slot ownership"),
        );
        assert_eq!(control_packets.len(), 1);
        assert_eq!(state.server_reliable_slots.slots.len(), 1);
    }

    #[test]
    fn server_receive_window_rejects_stale_without_evicting_live_slots() {
        let payload = crate::translate::loadbar::start_payload(2);
        let mut state = SessionState::default();
        for sequence in 100..116 {
            let packet = client_reliable_m_frame(sequence, 74, &payload);
            let view = MFrameView::parse(&packet).expect("server window source");
            assert!(matches!(
                server_replay::prepare_source_slot(
                    &mut state.server_reliable_slots,
                    &packet,
                    &view,
                )
                .expect("pin in-window source"),
                server_replay::PreparedServerReliableSource::Pinned(_)
            ));
        }
        assert_eq!(
            state.server_reliable_slots.slots.len(),
            server_replay::MAX_SERVER_RELIABLE_SLOTS
        );

        let stale = client_reliable_m_frame(99, 75, &payload);
        let stale_view = MFrameView::parse(&stale).expect("stale server source");
        assert!(matches!(
            server_replay::prepare_source_slot(
                &mut state.server_reliable_slots,
                &stale,
                &stale_view,
            )
            .expect("classify stale source"),
            server_replay::PreparedServerReliableSource::OutsideWindow(_)
        ));
        assert_eq!(
            state.server_reliable_slots.slots.len(),
            server_replay::MAX_SERVER_RELIABLE_SLOTS
        );

        let conflict =
            client_reliable_m_frame(100, 76, &crate::translate::loadbar::start_payload(3));
        let conflict_view = MFrameView::parse(&conflict).expect("conflicting server source");
        assert!(matches!(
            server_replay::prepare_source_slot(
                &mut state.server_reliable_slots,
                &conflict,
                &conflict_view,
            )
            .expect("classify occupied source"),
            server_replay::PreparedServerReliableSource::Conflict(_)
        ));
    }

    #[test]
    fn server_receive_slot_retires_only_after_strict_accepted_client_ack() {
        let payload = crate::translate::loadbar::start_payload(2);
        let server = client_reliable_m_frame(35, 74, &payload);
        let mut state = SessionState::default();
        state.synthetic_area.synthesize_loadbar = false;
        proof_packets(
            translate_server_to_client(&server, &mut state).expect("seed translated server slot"),
        );
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.server_reliable_slots.receive_start, Some(35));
        assert_eq!(state.server_reliable_slots.slots.len(), 1);

        let client_ack = client_reliable_m_frame(1, 35, &[b'P', 0xFE, 0xFD]);
        begin_client_to_server_emit_validation(&mut state, &client_ack)
            .expect("begin rejected client ACK transaction");
        let rejected_emit = translate_client_to_server(&client_ack, &mut state)
            .expect("translate rejected client ACK candidate");
        stage_direct_client_ack_delivery(&mut state, &rejected_emit)
            .expect("stage rejected client ACK delivery");
        finish_client_to_server_emit_validation(&mut state, false);
        assert_eq!(state.server_reliable_slots.receive_start, Some(35));
        assert_eq!(state.server_reliable_slots.slots.len(), 1);

        begin_client_to_server_emit_validation(&mut state, &client_ack)
            .expect("begin accepted client ACK transaction");
        let accepted_emit = translate_client_to_server(&client_ack, &mut state)
            .expect("translate accepted client ACK candidate");
        stage_direct_client_ack_delivery(&mut state, &accepted_emit)
            .expect("stage accepted client ACK delivery");
        finish_client_to_server_emit_validation(&mut state, true);
        assert_eq!(state.server_reliable_slots.receive_start, Some(36));
        assert!(state.server_reliable_slots.slots.is_empty());
    }

    #[test]
    fn consumed_client_output_does_not_deliver_its_source_ack() {
        let server_source =
            client_reliable_m_frame(35, 74, &crate::translate::loadbar::start_payload(2));
        let server_view = MFrameView::parse(&server_source).expect("consumed ACK server slot");
        let mut state = SessionState::default();
        assert!(matches!(
            server_replay::prepare_source_slot(
                &mut state.server_reliable_slots,
                &server_source,
                &server_view,
            )
            .expect("pin consumed ACK server slot"),
            server_replay::PreparedServerReliableSource::Pinned(_)
        ));

        stage_direct_client_ack_delivery(&mut state, &Emit::Consumed)
            .expect("stage consumed client output");
        finish_client_to_server_emit_validation(&mut state, true);

        assert_eq!(state.server_reliable_slots.receive_start, Some(35));
        assert_eq!(state.server_reliable_slots.slots.len(), 1);
    }

    #[test]
    fn mapped_client_ack_carrier_retires_only_after_its_strict_acceptance() {
        let server_source =
            client_reliable_m_frame(35, 74, &crate::translate::loadbar::start_payload(2));
        let server_view = MFrameView::parse(&server_source).expect("ACK-carrier server slot");
        let mut state = SessionState::default();
        assert!(matches!(
            server_replay::prepare_source_slot(
                &mut state.server_reliable_slots,
                &server_source,
                &server_view,
            )
            .expect("pin ACK-carrier server slot"),
            server_replay::PreparedServerReliableSource::Pinned(_)
        ));
        state
            .sequence
            .server_sequence_shifts
            .push(SequenceShift { base: 35, delta: 2 });
        let client_source = client_reliable_m_frame(8, 37, &[b'P', 0xfe, 0xfd]);

        let prepared = prepare_direct_client_source_ack_carrier(&state, &client_source)
            .expect("prepare mapped client ACK carrier")
            .expect("ACK 37 should map to active server slot 35");
        let rejected_emit =
            ensure_direct_source_ack_carrier(Emit::Consumed, prepared, "test_client_to_server");
        let rejected_packets = proof_packets(rejected_emit.clone());
        assert_eq!(rejected_packets.len(), 1);
        let rejected_view =
            MFrameView::parse(&rejected_packets[0].1).expect("mapped client ACK carrier");
        assert_eq!(rejected_view.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(rejected_view.sequence, 0);
        assert_eq!(rejected_view.ack_sequence, 35);
        assert!(rejected_view.is_exact_control_frame());

        stage_direct_client_ack_delivery(&mut state, &rejected_emit)
            .expect("stage rejected ACK-only client output");
        finish_client_to_server_emit_validation_outcomes(&mut state, true, false);
        assert_eq!(state.server_reliable_slots.receive_start, Some(35));
        assert_eq!(state.server_reliable_slots.slots.len(), 1);

        let prepared = prepare_direct_client_source_ack_carrier(&state, &client_source)
            .expect("prepare retry mapped client ACK carrier")
            .expect("rejected carrier must leave active server slot");
        let accepted_emit =
            ensure_direct_source_ack_carrier(Emit::Consumed, prepared, "test_client_to_server");
        stage_direct_client_ack_delivery(&mut state, &accepted_emit)
            .expect("stage accepted ACK-only client output");
        finish_client_to_server_emit_validation_outcomes(&mut state, true, true);
        assert_eq!(state.server_reliable_slots.receive_start, Some(36));
        assert!(state.server_reliable_slots.slots.is_empty());

        let retry_after_retirement =
            prepare_direct_client_source_ack_carrier(&state, &client_source)
                .expect("prepare duplicate client ACK after local retirement")
                .expect(
                    "UDP delivery uncertainty requires duplicate type-0 ACKs to remain deliverable",
                );
        let retry_emit = ensure_direct_source_ack_carrier(
            Emit::Consumed,
            retry_after_retirement,
            "test_client_to_server",
        );
        let retry_packets = proof_packets(retry_emit);
        assert_eq!(retry_packets.len(), 1);
        assert_eq!(
            MFrameView::parse(&retry_packets[0].1)
                .expect("duplicate ACK carrier")
                .ack_sequence,
            35
        );
    }

    #[test]
    fn dropped_server_payload_carrier_commits_ack_without_gameplay_effects() {
        let mut state = SessionState::default();
        for sequence in 7..=9 {
            let packet = client_reliable_m_frame(sequence, 30, &[0x70, 0x0d, 0x01, 0]);
            let view = MFrameView::parse(&packet).expect("client ACK-carrier source slot");
            assert!(matches!(
                client_replay::prepare_source_slot(
                    &mut state.client_reliable_replays,
                    &packet,
                    &view,
                )
                .expect("pin client ACK-carrier source slot"),
                client_replay::PreparedClientReliableSource::Pending(_)
            ));
        }
        state
            .sequence
            .client_sequence_shifts
            .push(SequenceShift { base: 7, delta: 2 });
        state
            .sequence
            .client_sequence_elisions
            .push(SequenceElision { sequence: 8 });
        state.semantic.area.loadbar_packets = 1;
        let server_source = client_reliable_m_frame(50, 10, &[b'P', 0xfe, 0xfd]);

        let prepared = prepare_direct_server_source_ack_carrier(&state, &server_source)
            .expect("prepare mapped server ACK carrier")
            .expect("ACK 10 should map to contiguous EE client slot 9");
        begin_ordinary_server_emit_effect_transaction(&mut state)
            .expect("begin rejected server payload effects");
        state.semantic.area.loadbar_packets = 99;
        let emit = ensure_direct_source_ack_carrier(Emit::Drop, prepared, "test_server_to_client");
        let packets = proof_packets(emit.clone());
        assert_eq!(packets.len(), 1);
        let view = MFrameView::parse(&packets[0].1).expect("mapped server ACK carrier");
        assert_eq!(view.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(view.ack_sequence, 9);

        stage_direct_server_ack_delivery(&mut state, &emit)
            .expect("stage accepted ACK-only server output");
        finish_server_to_client_emit_validation_outcomes(&mut state, false, true);
        assert_eq!(
            state.semantic.area.loadbar_packets, 1,
            "ACK acceptance must not commit a dropped payload's speculative effects"
        );
        assert_eq!(state.client_reliable_replays.receive_start, Some(10));
        assert!(state.client_reliable_replays.slots.is_empty());
    }

    #[test]
    fn sparse_future_ack_emits_but_cannot_retire_and_controls_add_no_sibling() {
        let mut state = SessionState::default();
        for sequence in [7, 9] {
            let packet = client_reliable_m_frame(sequence, 30, &[0x70, 0x0d, 0x01, 0]);
            let view = MFrameView::parse(&packet).expect("sparse client source slot");
            client_replay::prepare_source_slot(&mut state.client_reliable_replays, &packet, &view)
                .expect("pin sparse client source slot");
        }

        let sparse_server_source = client_reliable_m_frame(50, 9, &[b'P', 0xfe, 0xfd]);
        let prepared = prepare_direct_server_source_ack_carrier(&state, &sparse_server_source)
            .expect("classify sparse future ACK")
            .expect("every validated type-0 source ACK remains destination-facing truth");
        let emit = ensure_direct_source_ack_carrier(Emit::Drop, prepared, "test_server_to_client");
        stage_direct_server_ack_delivery(&mut state, &emit)
            .expect("stage sparse future ACK carrier");
        finish_server_to_client_emit_validation_outcomes(&mut state, false, true);
        assert_eq!(state.client_reliable_replays.receive_start, Some(7));
        assert_eq!(state.client_reliable_replays.slots.len(), 2);

        let control = reliable_server_m_frame(0, 7, 0x10, 0, &[]);
        assert!(
            prepare_direct_server_source_ack_carrier(&state, &control)
                .expect("classify exact type-1 source")
                .is_none(),
            "an existing control must never acquire a synthetic control sibling"
        );

        let resend_control = reliable_server_m_frame(0, 7, 0x20, 0, &[]);
        assert!(
            prepare_direct_server_source_ack_carrier(&state, &resend_control)
                .expect("classify exact type-2 source")
                .is_none(),
            "an existing resend control must never acquire an ACK-control sibling"
        );

        let mut corrupt = sparse_server_source;
        *corrupt.last_mut().expect("source CRC byte") ^= 0x01;
        assert!(
            prepare_direct_server_source_ack_carrier(&state, &corrupt).is_err(),
            "a malformed source cannot create independently trusted ACK intent"
        );
    }

    #[test]
    fn foreign_validation_callback_preserves_pending_ack_delivery_owner() {
        let client_source = client_reliable_m_frame(7, 30, &[0x70, 0x0D, 0x01, 0]);
        let client_view = MFrameView::parse(&client_source).expect("pending-owner client slot");
        let mut state = SessionState::default();
        assert!(matches!(
            client_replay::prepare_source_slot(
                &mut state.client_reliable_replays,
                &client_source,
                &client_view,
            )
            .expect("pin pending-owner client slot"),
            client_replay::PreparedClientReliableSource::Pending(_)
        ));

        let outgoing_ack = Emit::Packet(reliable_server_m_frame(0, 7, 0x10, 0, &[]));
        stage_pending_server_ack_delivery(&mut state, &outgoing_ack)
            .expect("stage pending-server ACK delivery");
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.client_reliable_replays.receive_start, Some(7));
        assert_eq!(state.client_reliable_replays.slots.len(), 1);
        assert!(matches!(
            state
                .ack_delivery
                .pending
                .as_ref()
                .map(|pending| pending.owner),
            Some(ack_delivery::AckDeliveryOwner::PendingServerDrain)
        ));

        finish_pending_server_drain_emit_validation(&mut state, true);
        assert_eq!(state.client_reliable_replays.receive_start, Some(8));
        assert!(state.client_reliable_replays.slots.is_empty());
        assert!(state.ack_delivery.pending.is_none());
    }

    #[test]
    fn client_receive_slots_retire_only_after_exact_server_ack_output_is_accepted() {
        let mut state = SessionState::default();
        for sequence in 7..=9 {
            let packet = client_reliable_m_frame(sequence, 30, &[0x70, 0x0D, 0x01, 0]);
            let view = MFrameView::parse(&packet).expect("client receive-window source");
            assert!(matches!(
                client_replay::prepare_source_slot(
                    &mut state.client_reliable_replays,
                    &packet,
                    &view,
                )
                .expect("pin client receive-window source"),
                client_replay::PreparedClientReliableSource::Pending(_)
            ));
        }
        state
            .sequence
            .client_sequence_shifts
            .push(SequenceShift { base: 7, delta: 2 });
        state
            .sequence
            .client_sequence_elisions
            .push(SequenceElision { sequence: 8 });

        let server_ack = reliable_server_m_frame(0, 10, 0x10, 0, &[]);
        let rejected_emit = translate_server_to_client(&server_ack, &mut state)
            .expect("translate rejected server ACK candidate");
        let rejected_packets = proof_packets(rejected_emit.clone());
        assert_eq!(rejected_packets.len(), 1);
        assert_eq!(
            MFrameView::parse(&rejected_packets[0].1)
                .expect("mapped server ACK output")
                .ack_sequence,
            9,
            "outgoing ACK must be mapped back through client shifts and elisions"
        );
        stage_direct_server_ack_delivery(&mut state, &rejected_emit)
            .expect("stage rejected server ACK delivery");
        finish_server_to_client_emit_validation(&mut state, false);
        assert_eq!(state.client_reliable_replays.receive_start, Some(7));
        assert_eq!(state.client_reliable_replays.slots.len(), 3);

        let accepted_emit = translate_server_to_client(&server_ack, &mut state)
            .expect("translate accepted server ACK candidate");
        stage_direct_server_ack_delivery(&mut state, &accepted_emit)
            .expect("stage accepted server ACK delivery");
        finish_server_to_client_emit_validation(&mut state, true);
        assert_eq!(state.client_reliable_replays.receive_start, Some(10));
        assert!(state.client_reliable_replays.slots.is_empty());
    }

    #[test]
    fn ordinary_unowned_direct_rejection_rolls_back_without_unpinning_source_slot() {
        let source = client_reliable_m_frame(34, 74, &[b'P', 0xFE, 0xFE]);
        let mut state = SessionState::default();
        state.semantic.area.loadbar_packets = 1;

        let emitted = translate_server_to_client(&source, &mut state)
            .expect("unowned direct source should reach the outer strict owner");
        assert!(!matches!(emitted, Emit::Drop));
        assert_eq!(
            state.deflate.server_emit_effect_transaction_kind,
            Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit)
        );

        finish_server_to_client_emit_validation(&mut state, false);
        assert_eq!(state.semantic.area.loadbar_packets, 1);
        assert_eq!(state.server_reliable_slots.slots.len(), 1);
        assert_eq!(state.deflate.server_emit_effect_transaction_kind, None);
        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
    }

    #[test]
    fn packetized_deflated_area_rejection_restores_partial_reassembly_and_source_effects() {
        let payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let (first, second) = two_frame_deflated_window(110, 74, payload);
        let mut state = SessionState::default();

        assert!(matches!(
            translate_server_to_client(&first, &mut state).expect("start deflated Area window"),
            Emit::Consumed
        ));
        let partial = state
            .deflate
            .server_reassembly
            .as_ref()
            .expect("first frame must remain buffered");
        assert_eq!(partial.first_sequence, 110);
        assert_eq!(partial.expected_frames, 2);
        assert_eq!(partial.frames.len(), 1);
        assert!(partial.interleaved_events.is_empty());
        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());

        let baseline_windows = state.deflate.completed_server_stream_windows.len();
        let baseline_slots = state.deflate.completed_server_reliable_stream_slots.len();
        let baseline_replays = state.direct_server_semantic_replays.completed.len();
        let baseline_shifts = state.sequence.server_sequence_shifts.len();
        let emit = translate_server_to_client(&second, &mut state)
            .expect("complete deflated Area window speculatively");
        assert!(
            proof_packets(emit)
                .iter()
                .any(|(proof, _)| proof.contains_family(VerifiedFamily::AreaClientArea))
        );
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
        assert!(state.synthetic_area.pending_area_loaded.is_some());
        assert!(state.synthetic_area.server_hold_gate.is_some());
        assert!(state.deflate.server_reassembly.is_none());
        assert_eq!(
            state.deflate.server_emit_effect_transaction_kind,
            Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit)
        );

        finish_server_to_client_emit_validation(&mut state, false);

        assert_eq!(state.semantic.area.client_area_packets, 0);
        assert!(
            state
                .area_context
                .latest_area_placeables
                .area_resref
                .is_empty()
        );
        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert!(state.synthetic_area.pending_area_loaded.is_none());
        assert!(state.synthetic_area.server_hold_gate.is_none());
        assert_eq!(
            state.deflate.completed_server_stream_windows.len(),
            baseline_windows
        );
        assert_eq!(
            state.deflate.completed_server_reliable_stream_slots.len(),
            baseline_slots
        );
        assert_eq!(
            state.direct_server_semantic_replays.completed.len(),
            baseline_replays
        );
        assert_eq!(state.sequence.server_sequence_shifts.len(), baseline_shifts);
        assert_eq!(state.deflate.server_emit_effect_transaction_kind, None);
        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
        let restored = state
            .deflate
            .server_reassembly
            .as_ref()
            .expect("strict rejection must restore the partial source window");
        assert_eq!(restored.first_sequence, 110);
        assert_eq!(restored.expected_frames, 2);
        assert_eq!(restored.frames.len(), 1);
        assert!(restored.interleaved_events.is_empty());

        let retry_emit = translate_server_to_client(&second, &mut state)
            .expect("exact completion retransmit should retry from the restored source window");
        assert!(
            proof_packets(retry_emit)
                .iter()
                .any(|(proof, _)| proof.contains_family(VerifiedFamily::AreaClientArea))
        );
        finish_server_to_client_emit_validation(&mut state, true);
        assert!(state.deflate.server_reassembly.is_none());
        assert_eq!(state.semantic.area.client_area_packets, 1);
        assert_eq!(
            state.area_context.latest_area_placeables.area_resref,
            "voyage"
        );
    }

    #[test]
    fn direct_semantic_replay_rejects_trailing_records_and_wrapped_generation() {
        let payload =
            include_bytes!("../../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
        let packet = client_reliable_m_frame(5, 74, payload);
        let view = MFrameView::parse(&packet).expect("direct Area frame");
        let mut state = SessionState::default();
        let rewrite = server_dispatch::rewrite_direct_frame_if_needed(
            &packet,
            &view,
            &state.module_resources,
            None,
            None,
        )
        .expect("dispatch")
        .expect("Area claim");
        remember_completed_direct_server_semantic_rewrite(
            &mut state,
            5,
            0,
            transport_identity::server_reliable_data_transport_identity(&packet, &view)
                .expect("direct source transport identity"),
            payload.to_vec(),
            &rewrite.verified,
        );

        let mut bit6_refresh = client_reliable_m_frame(5, 75, payload);
        bit6_refresh[7] |= transport_identity::SEND_WINDOW_BIT6_MASK;
        assert!(encode_legacy_m_crc(&mut bit6_refresh));
        let bit6_view = MFrameView::parse(&bit6_refresh).expect("bit-6 direct replay frame");
        let bit6_replay = replay_completed_direct_server_semantic_rewrite(
            &bit6_refresh,
            &bit6_view,
            0,
            &mut state,
        )
        .expect("bit-6-only direct retransmit should retain slot identity")
        .expect("bit-6-only direct retransmit should replay");
        let bit6_replay_view =
            MFrameView::parse(&bit6_replay.packet).expect("bit-6 direct replay output");
        assert_eq!(
            bit6_replay_view.flags & transport_identity::SEND_WINDOW_BIT6_MASK,
            transport_identity::SEND_WINDOW_BIT6_MASK
        );
        assert!(bit6_replay_view.crc_valid);

        let mut low_flag_conflict = bit6_refresh.clone();
        low_flag_conflict[7] ^= 0x01;
        assert!(encode_legacy_m_crc(&mut low_flag_conflict));
        let low_flag_view =
            MFrameView::parse(&low_flag_conflict).expect("low-flag direct replay conflict");
        let low_flag_error = replay_completed_direct_server_semantic_rewrite(
            &low_flag_conflict,
            &low_flag_view,
            0,
            &mut state,
        )
        .expect_err("same reliable slot with a changed low flag must fail closed");
        assert!(
            low_flag_error
                .to_string()
                .contains("different immutable transport bytes")
        );

        let mut trailing = packet.clone();
        trailing.extend_from_slice(&client_reliable_m_frame(6, 74, &[b'P', 0x09, 0x05]));
        assert!(encode_legacy_m_crc(&mut trailing));
        let trailing_view = MFrameView::parse(&trailing).expect("trailing frame");
        assert_ne!(trailing_view.trailing_payload_length, 0);
        assert!(
            replay_completed_direct_server_semantic_rewrite(
                &trailing,
                &trailing_view,
                0,
                &mut state,
            )
            .expect("trailing replay probe")
            .is_none()
        );
        let coalesced_before = state.coalesced_replay.completed_windows.len();
        assert!(matches!(
            translate_server_to_client(&trailing, &mut state)
                .expect("conflicting trailing shape should fail closed before route dispatch"),
            Emit::Drop
        ));
        assert_eq!(
            state.coalesced_replay.completed_windows.len(),
            coalesced_before
        );
        assert!(state.deflate.server_reassembly.is_none());
        assert_eq!(state.direct_server_semantic_replays.completed.len(), 1);

        state.server_reliable_slots.receive_start = Some(0xFFFE);
        let wrapped = client_reliable_m_frame(5, 75, payload);
        let wrapped_view = MFrameView::parse(&wrapped).expect("wrapped frame");
        let wrapped_key = server_replay::prepare_source_slot(
            &mut state.server_reliable_slots,
            &wrapped,
            &wrapped_view,
        )
        .expect("pin wrapped server reliable source")
        .key()
        .expect("type-0 source key");
        assert_eq!(wrapped_key.origin_generation, 1);
        assert!(
            replay_completed_direct_server_semantic_rewrite(
                &wrapped,
                &wrapped_view,
                1,
                &mut state,
            )
            .expect("wrapped replay probe")
            .is_none()
        );
    }

    #[test]
    fn reliable_client_slot_replays_first_translation_and_rejects_conflicts() {
        let mut state = SessionState::default();
        let candidate = crate::translate::semantic::InventoryItemContextCandidate {
            object_id: 0x8001_5B01,
            proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
            source: crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
        };
        state
            .semantic
            .ui
            .last_inventory_item_context_after_committed_quickbar =
            Some(crate::translate::semantic::InventoryItemContextSummary {
                active_item_objects: 1,
                materialized_item_objects: 1,
                direct_item_proof_objects: 1,
                compact_item_emission_proof_objects: 1,
                compact_item_emission_candidate: Some(candidate),
                compact_item_emission_ready_objects: 1,
                compact_item_emission_ready_candidate: Some(candidate),
                compact_item_emission_direct_only_proof_objects: 1,
                ..Default::default()
            });
        let payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );

        let first =
            translate_client_to_server(&client_reliable_m_frame(78, 31, &payload), &mut state)
                .expect("first reliable ClientGuiInventory frame should translate");
        let first_packet = match first {
            Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
            other => panic!("expected first verified client packet, got {other:?}"),
        };
        assert_eq!(state.semantic.ui.inventory_packets, 1);
        assert_eq!(
            state.semantic.ui.inventory_equipment_handoff_events, 1,
            "first sequence/payload must apply its semantic bridge handoff"
        );
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs, 1,
            "first bridge handoff should queue one synthetic status request"
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);

        let mut retransmit = client_reliable_m_frame(78, 32, &payload);
        retransmit[7] |= 0x40;
        assert!(encode_legacy_m_crc(&mut retransmit));
        let replay = translate_client_to_server(&retransmit, &mut state)
            .expect("transport retransmission with a newer ACK should replay");
        let replay_packet = match replay {
            Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
            other => panic!("expected replayed verified client packet, got {other:?}"),
        };
        let replay_view = MFrameView::parse(&replay_packet).expect("replayed client frame");
        let first_view = MFrameView::parse(&first_packet).expect("first client frame");
        assert_eq!(replay_view.sequence, first_view.sequence);
        assert_eq!(replay_view.ack_sequence, 32);
        assert_eq!(replay_view.flags & 0x40, 0x40);
        assert_eq!(replay_view.flags & 0x8f, first_view.flags & 0x8f);
        assert_eq!(&replay_packet[8..], &first_packet[8..]);
        assert!(replay_view.crc_valid);
        assert_eq!(
            state.semantic.ui.inventory_packets, 1,
            "ACK changes do not make the same reliable sequence/payload a new semantic event"
        );
        assert_eq!(state.semantic.ui.inventory_equipment_handoff_events, 1);
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs, 1,
            "retransmission must not queue another synthetic status request"
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.client_reliable_replays.exact_replays, 1);
        assert_eq!(state.client_reliable_replays.slots.len(), 1);
        assert!(state.client_reliable_replays.slots[0].replay.is_some());

        let distinct_payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            false,
        );
        let conflict = translate_client_to_server(
            &client_reliable_m_frame(78, 32, &distinct_payload),
            &mut state,
        )
        .expect("immutable slot conflict should produce a fail-closed disposition");
        assert!(matches!(conflict, Emit::Drop));
        assert_eq!(state.semantic.ui.inventory_packets, 1);
        assert_eq!(state.semantic.ui.inventory_equipment_handoff_events, 1);
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            1
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.client_reliable_replays.slots.len(), 1);
    }

    #[test]
    fn reliable_client_slot_identity_covers_flags_metadata_tail_and_length() {
        let payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );
        let mut state = SessionState::default();
        let first = client_reliable_m_frame(79, 30, &payload);
        assert!(!matches!(
            translate_client_to_server(&first, &mut state).expect("first client slot"),
            Emit::Drop
        ));
        assert_eq!(state.semantic.ui.inventory_packets, 1);

        let mut immutable_flag_conflict = client_reliable_m_frame(79, 31, &payload);
        immutable_flag_conflict[7] ^= 0x08;
        assert!(encode_legacy_m_crc(&mut immutable_flag_conflict));

        let mut packetized_metadata_conflict = client_reliable_m_frame(79, 32, &payload);
        assert!(write_be_u16(&mut packetized_metadata_conflict, 8, 2));
        assert!(encode_legacy_m_crc(&mut packetized_metadata_conflict));

        let mut datagram_length_conflict = client_reliable_m_frame(79, 33, &payload);
        datagram_length_conflict.push(0xAA);
        assert!(encode_legacy_m_crc(&mut datagram_length_conflict));

        for conflict in [
            immutable_flag_conflict,
            packetized_metadata_conflict,
            datagram_length_conflict,
        ] {
            assert!(matches!(
                translate_client_to_server(&conflict, &mut state)
                    .expect("immutable source conflict should fail closed"),
                Emit::Drop
            ));
        }
        assert_eq!(state.sequence.latest_client_ack_from_client, Some(33));
        assert_eq!(state.semantic.ui.inventory_packets, 1);
        assert_eq!(state.client_reliable_replays.slots.len(), 1);
        assert_eq!(state.client_reliable_replays.exact_replays, 0);

        let exact = client_reliable_m_frame(79, 34, &payload);
        assert!(!matches!(
            translate_client_to_server(&exact, &mut state).expect("exact source should replay"),
            Emit::Drop
        ));
        assert_eq!(state.sequence.latest_client_ack_from_client, Some(34));
        assert_eq!(state.semantic.ui.inventory_packets, 1);
        assert_eq!(state.client_reliable_replays.exact_replays, 1);
    }

    #[test]
    fn consumed_empty_client_carrier_replays_its_first_accepted_disposition() {
        let payload = [0x70, 0xFE, 0xFE, 0, 0, 0, 0];
        let mut state = SessionState::default();
        let first =
            translate_client_to_server(&client_reliable_m_frame(80, 50, &payload), &mut state)
                .expect("unclaimed client high-level should become an empty carrier");
        let first_packet = match first {
            Emit::VerifiedPackets { family, packets } => {
                assert_eq!(family, VerifiedFamily::ConsumedEmptyMFrame);
                packets.into_iter().next().unwrap()
            }
            other => panic!("expected consumed empty carrier, got {other:?}"),
        };
        assert_eq!(
            first_packet.len(),
            crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET
        );

        let replay =
            translate_client_to_server(&client_reliable_m_frame(80, 51, &payload), &mut state)
                .expect("empty carrier retransmit should replay");
        let replay_packet = match replay {
            Emit::VerifiedPackets { family, packets } => {
                assert_eq!(family, VerifiedFamily::ConsumedEmptyMFrame);
                packets.into_iter().next().unwrap()
            }
            other => panic!("expected replayed empty carrier, got {other:?}"),
        };
        let replay_view = MFrameView::parse(&replay_packet).expect("replayed empty carrier");
        assert_eq!(replay_packet.len(), first_packet.len());
        assert_eq!(replay_view.sequence, 80);
        assert_eq!(replay_view.ack_sequence, 51);
        assert!(replay_view.crc_valid);
        assert_eq!(state.client_reliable_replays.exact_replays, 1);
    }

    #[test]
    fn client_emit_effects_rollback_but_valid_source_transport_survives_rejection() {
        let candidate = crate::translate::semantic::InventoryItemContextCandidate {
            object_id: 0x8001_5B01,
            proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
            source: crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
        };
        let payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );
        let first = client_reliable_m_frame(78, 31, &payload);
        let retry = client_reliable_m_frame(78, 32, &payload);
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(77);
        state.sequence.latest_client_ack_from_client = Some(30);
        state
            .semantic
            .ui
            .last_inventory_item_context_after_committed_quickbar =
            Some(crate::translate::semantic::InventoryItemContextSummary {
                active_item_objects: 1,
                materialized_item_objects: 1,
                direct_item_proof_objects: 1,
                compact_item_emission_proof_objects: 1,
                compact_item_emission_candidate: Some(candidate),
                compact_item_emission_ready_objects: 1,
                compact_item_emission_ready_candidate: Some(candidate),
                compact_item_emission_direct_only_proof_objects: 1,
                ..Default::default()
            });

        begin_client_to_server_emit_validation(&mut state, &first)
            .expect("valid client source should open a validation transaction");
        let speculative = translate_client_to_server(&first, &mut state)
            .expect("client GUI status should translate before final validation");
        assert!(!matches!(speculative, Emit::Drop));
        assert_eq!(state.semantic.ui.inventory_packets, 1);
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            1
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.client_reliable_replays.slots.len(), 1);
        assert!(state.client_reliable_replays.slots[0].replay.is_some());
        assert!(state.client_emit_effect_snapshot.is_some());

        finish_client_to_server_emit_validation(&mut state, false);
        assert_eq!(
            state.sequence.latest_client_sequence_from_client,
            Some(78),
            "validated source receive progress remains transport truth"
        );
        assert_eq!(state.sequence.latest_client_ack_from_client, Some(31));
        assert_eq!(state.semantic.ui.inventory_packets, 0);
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            0
        );
        assert!(state.sequence.pending_client_to_server_packets.is_empty());
        assert_eq!(state.client_reliable_replays.slots.len(), 1);
        assert!(
            state.client_reliable_replays.slots[0].replay.is_none(),
            "strict rejection retains the raw slot fence but rolls back its translated replay"
        );
        assert!(state.client_emit_effect_snapshot.is_none());
        assert!(state.client_emit_pending_validation.is_none());

        begin_client_to_server_emit_validation(&mut state, &retry)
            .expect("exact retry should open a fresh validation transaction");
        let accepted = translate_client_to_server(&retry, &mut state)
            .expect("exact retry should translate from the restored boundary");
        assert!(!matches!(accepted, Emit::Drop));
        finish_client_to_server_emit_validation(&mut state, true);
        assert_eq!(state.sequence.latest_client_ack_from_client, Some(32));
        assert_eq!(state.semantic.ui.inventory_packets, 1);
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            1
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.client_reliable_replays.slots.len(), 1);
        assert!(state.client_reliable_replays.slots[0].replay.is_some());
        assert!(state.client_emit_effect_snapshot.is_none());
        assert!(state.client_emit_pending_validation.is_none());
    }

    #[test]
    fn session_translator_automatically_rolls_back_client_effects_on_strict_reject() {
        // The raw server-admin owner claims only the exact primary payload.
        // Adding trailing storage therefore lets the core translator stage its
        // replay identity while the outer verified owner rejects the complete
        // emitted frame.
        let mut packet = client_reliable_m_frame(91, 41, b"sModule.Run");
        packet.push(0xAA);
        assert!(encode_legacy_m_crc(&mut packet));
        let view = MFrameView::parse(&packet).expect("trailing client frame");
        assert!(view.crc_valid);
        assert_eq!(view.trailing_payload_length, 1);

        let mut translator = strict_session_translator_for_test();
        translator
            .m_state
            .sequence
            .latest_client_sequence_from_client = Some(90);
        translator.m_state.sequence.latest_client_ack_from_client = Some(40);

        let rejected = translator.translate(crate::packet::Direction::ClientToServer, &packet);
        let Emit::Packets(rejected_packets) = rejected else {
            panic!("strict-rejected gameplay payload should leave one exact ACK carrier");
        };
        assert_eq!(rejected_packets.len(), 1);
        let rejected_carrier =
            MFrameView::parse(&rejected_packets[0]).expect("strict-rejection ACK carrier");
        assert_eq!(rejected_carrier.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(rejected_carrier.ack_sequence, 41);
        assert!(rejected_carrier.is_exact_control_frame());
        assert_eq!(
            translator
                .m_state
                .sequence
                .latest_client_sequence_from_client,
            Some(91)
        );
        assert_eq!(
            translator.m_state.sequence.latest_client_ack_from_client,
            Some(41)
        );
        assert_eq!(translator.m_state.client_reliable_replays.slots.len(), 1);
        assert!(
            translator.m_state.client_reliable_replays.slots[0]
                .replay
                .is_none()
        );
        assert!(translator.m_state.semantic.recent_events.is_empty());
        assert!(translator.m_state.client_emit_effect_snapshot.is_none());
        assert!(translator.m_state.client_emit_pending_validation.is_none());

        let mut conflict = packet;
        conflict[5..7].copy_from_slice(&42u16.to_be_bytes());
        *conflict.last_mut().unwrap() = 0xAB;
        assert!(encode_legacy_m_crc(&mut conflict));
        let conflict_emit =
            translator.translate(crate::packet::Direction::ClientToServer, &conflict);
        let Emit::Packets(conflict_packets) = conflict_emit else {
            panic!("occupied payload conflict should preserve its ACK in an exact carrier");
        };
        assert_eq!(conflict_packets.len(), 1);
        let conflict_carrier =
            MFrameView::parse(&conflict_packets[0]).expect("occupied-conflict ACK carrier");
        assert_eq!(conflict_carrier.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(conflict_carrier.ack_sequence, 42);
        assert!(conflict_carrier.is_exact_control_frame());
        assert_eq!(
            translator.m_state.sequence.latest_client_ack_from_client,
            Some(42),
            "occupied-slot conflict still reaches the common ACK retirement path"
        );
        assert_eq!(translator.m_state.client_reliable_replays.slots.len(), 1);
        assert!(
            translator.m_state.client_reliable_replays.slots[0]
                .replay
                .is_none()
        );
    }

    #[test]
    fn session_translator_preserves_ack_when_client_semantic_translation_errors() {
        // A raw one-byte payload is transport-valid type-0 data, but it has
        // neither a complete `P major minor` header nor a verified raw/admin
        // identity, so client semantic dispatch fails closed after FrameReceive
        // has already accepted the cumulative ACK.
        let source = client_reliable_m_frame(92, 43, &[0xff]);
        let source_view = MFrameView::parse(&source).expect("transport-valid semantic-error seed");
        assert!(source_view.crc_valid);
        assert_eq!(source_view.frame_kind(), Some(MFrameType::ReliableData));
        assert!(source_view.high.is_none());
        let direct_error = translate_client_to_server(&source, &mut SessionState::default())
            .expect_err("raw payload without an identity must fail semantic dispatch");
        assert!(
            direct_error
                .to_string()
                .contains("no high-level translator or transport identity owner")
        );
        let mut translator = strict_session_translator_for_test();
        translator.bn_state.remember_nwsync_advertised_to_client();
        translator.bn_state.remember_bncs_udp_port(5122);
        translator.bn_state.remember_bnvr_result(true);
        assert!(translator.bn_state.should_consume_nwsync_handoff_bndm());

        let emit = translator.translate(crate::packet::Direction::ClientToServer, &source);
        let Emit::Packets(packets) = emit else {
            panic!("semantic failure should leave one exact ACK-only output");
        };
        assert_eq!(packets.len(), 1);
        let carrier = MFrameView::parse(&packets[0]).expect("semantic-error ACK carrier");
        assert_eq!(carrier.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(carrier.sequence, 0);
        assert_eq!(carrier.ack_sequence, 43);
        assert!(carrier.is_exact_control_frame());
        assert_eq!(
            translator.m_state.sequence.latest_client_ack_from_client,
            Some(43)
        );
        assert!(translator.m_state.semantic.recent_events.is_empty());
        assert!(translator.m_state.client_emit_effect_snapshot.is_none());
        assert!(translator.m_state.client_emit_pending_validation.is_none());
        assert!(
            !translator.bn_state.should_consume_nwsync_handoff_bndm(),
            "transport-valid gameplay must close the NWSync handoff allowance even when semantic dispatch fails"
        );

        let disconnect = translator.translate(crate::packet::Direction::ClientToServer, b"BNDM");
        let Emit::PacketRetireSession { packet, reason } = disconnect else {
            panic!("post-gameplay BNDM must become a real legacy disconnect");
        };
        assert_eq!(&packet[..4], b"BNDS");
        assert_eq!(reason, "post-gameplay-bndm-disconnect");
    }

    #[test]
    fn client_type_zero_sequence_zero_wraps_generation_and_replays_retransmit() {
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(u16::MAX);
        state.client_reliable_replays.receive_start = Some(u16::MAX);
        state
            .sequence
            .client_sequence_shifts
            .push(SequenceShift { base: 0, delta: 1 });
        let payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );

        let first =
            translate_client_to_server(&client_reliable_m_frame(0, 31, &payload), &mut state)
                .expect("wrapped type-0 client data should translate");
        let first_packet = match first {
            Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
            other => panic!("expected one verified client packet, got {other:?}"),
        };
        let first_view = MFrameView::parse(&first_packet).expect("shifted client frame");
        assert_eq!(first_view.frame_kind(), Some(MFrameType::ReliableData));
        assert_eq!(first_view.sequence, 1);
        assert!(first_view.crc_valid);
        assert_eq!(state.sequence.latest_client_sequence_from_client, Some(0));
        assert_eq!(state.client_reliable_replays.origin_generation, 0);
        assert_eq!(
            state.client_reliable_replays.slots[0].key.origin_generation,
            1
        );
        assert_eq!(state.semantic.ui.inventory_packets, 1);

        let retransmit =
            translate_client_to_server(&client_reliable_m_frame(0, 32, &payload), &mut state)
                .expect("wrapped retransmit with refreshed ACK should translate");
        let retransmit_packet = match retransmit {
            Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
            other => panic!("expected one verified retransmit, got {other:?}"),
        };
        let retransmit_view =
            MFrameView::parse(&retransmit_packet).expect("shifted retransmit frame");
        assert_eq!(retransmit_view.sequence, 1);
        assert_eq!(retransmit_view.ack_sequence, 32);
        assert!(retransmit_view.crc_valid);
        assert_eq!(state.semantic.ui.inventory_packets, 1);
        assert_eq!(state.client_reliable_replays.exact_replays, 1);
    }

    #[test]
    fn delayed_pre_wrap_client_slot_replays_from_previous_generation() {
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(u16::MAX - 1);
        state.client_reliable_replays.receive_start = Some(u16::MAX - 1);
        let first_payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );
        let wrapped_payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            false,
        );

        let first = translate_client_to_server(
            &client_reliable_m_frame(u16::MAX, 40, &first_payload),
            &mut state,
        )
        .expect("pre-wrap client slot");
        let first_packet = match first {
            Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
            other => panic!("expected pre-wrap verified packet, got {other:?}"),
        };
        assert!(!matches!(
            translate_client_to_server(
                &client_reliable_m_frame(0, 41, &wrapped_payload),
                &mut state,
            )
            .expect("post-wrap client slot"),
            Emit::Drop
        ));
        assert_eq!(state.client_reliable_replays.origin_generation, 0);
        assert_eq!(
            state.client_reliable_replays.slots[0].key.origin_generation,
            0
        );
        assert_eq!(
            state.client_reliable_replays.slots[1].key.origin_generation,
            1
        );
        assert_eq!(state.client_reliable_replays.slots.len(), 2);
        assert_eq!(state.semantic.ui.inventory_packets, 2);

        let delayed = translate_client_to_server(
            &client_reliable_m_frame(u16::MAX, 42, &first_payload),
            &mut state,
        )
        .expect("delayed pre-wrap retransmit");
        let delayed_packet = match delayed {
            Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
            other => panic!("expected delayed replay packet, got {other:?}"),
        };
        let delayed_view = MFrameView::parse(&delayed_packet).expect("delayed replay view");
        assert_eq!(delayed_view.ack_sequence, 42);
        assert_eq!(&delayed_packet[8..], &first_packet[8..]);
        assert_eq!(state.client_reliable_replays.origin_generation, 0);
        assert_eq!(
            state.client_reliable_replays.receive_start,
            Some(u16::MAX - 1)
        );
        assert_eq!(state.semantic.ui.inventory_packets, 2);
        assert_eq!(state.client_reliable_replays.exact_replays, 1);

        let control = reliable_server_m_frame(u16::MAX, 43, 0x10, 0, &[]);
        assert!(!matches!(
            translate_client_to_server(&control, &mut state).expect("independent ACK lane"),
            Emit::Drop
        ));
        assert_eq!(state.client_reliable_replays.slots.len(), 2);
        assert_eq!(state.sequence.latest_client_sequence_from_client, Some(0));
        assert_eq!(state.sequence.latest_client_ack_from_client, Some(43));
    }

    #[test]
    fn top_level_ack_zero_is_observed_and_unshifted_as_wrapped_transport_state() {
        // Diamond sub_5F36E0/sub_5F3940 and EE FrameSend/FrameReceive always
        // write and consume receive_next-1 as a modulo-u16 ACK. Zero is the
        // wrapped value after sequence zero, never an absent-field sentinel.
        let mut client_state = SessionState::default();
        client_state.sequence.latest_client_ack_from_client = Some(u16::MAX);
        client_state
            .sequence
            .server_sequence_shifts
            .push(SequenceShift { base: 0, delta: 1 });
        let client_control = reliable_server_m_frame(99, 0, 0x10, 0, &[]);

        let translated = translate_client_to_server(&client_control, &mut client_state)
            .expect("wrapped client ACK control should translate");
        let translated_packet = match translated {
            Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
            other => panic!("expected verified client ACK control, got {other:?}"),
        };
        let translated_view =
            MFrameView::parse(&translated_packet).expect("translated client ACK control");
        assert_eq!(client_state.sequence.latest_client_ack_from_client, Some(0));
        assert_eq!(translated_view.ack_sequence, u16::MAX);
        assert!(translated_view.crc_valid);

        let mut server_state = SessionState::default();
        server_state
            .sequence
            .client_sequence_shifts
            .push(SequenceShift { base: 0, delta: 1 });
        let mut server_control = reliable_server_m_frame(77, 0, 0x10, 0, &[]);
        let server_view = MFrameView::parse(&server_control).expect("server ACK control");
        unshift_server_ack_for_client(&server_state, &mut server_control, &server_view)
            .expect("wrapped server ACK should unshift");
        let server_view = MFrameView::parse(&server_control).expect("unshifted server ACK control");
        assert_eq!(server_view.ack_sequence, u16::MAX);
        assert!(server_view.crc_valid);
    }

    #[test]
    fn client_controls_do_not_advance_or_shift_reliable_data_sequence() {
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(10);
        state
            .sequence
            .client_sequence_shifts
            .push(SequenceShift { base: 10, delta: 1 });

        for (flags, sequence, ack_sequence, expected_kind) in [
            (0x10, 11, 20, MFrameType::AckControl),
            (0x50, 12, 21, MFrameType::AckControl),
            (0x20, 13, 22, MFrameType::ResendControl),
            (0x60, 14, 23, MFrameType::ResendControl),
        ] {
            let control = reliable_server_m_frame(sequence, ack_sequence, flags, 0, &[]);
            let emit = translate_client_to_server(&control, &mut state)
                .expect("exact empty client control should translate");
            let packet = match emit {
                Emit::VerifiedPackets { packets, .. } => packets.into_iter().next().unwrap(),
                other => panic!("expected one verified control, got {other:?}"),
            };
            let view = MFrameView::parse(&packet).expect("translated control frame");
            assert_eq!(packet, control);
            assert_eq!(view.frame_kind(), Some(expected_kind));
            assert_eq!(view.sequence, sequence);
            assert_eq!(view.ack_sequence, ack_sequence);
            assert_eq!(view.flags, flags);
            assert_eq!(view.packetized_sequence, 0);
            assert_eq!(view.declared_payload_length, 0);
            assert_eq!(view.available_payload_length, 0);
            assert!(view.crc_valid);
        }

        assert_eq!(state.sequence.latest_client_sequence_from_client, Some(10));
        assert_eq!(state.sequence.latest_client_ack_from_client, Some(23));
        assert!(state.client_reliable_replays.slots.is_empty());
        assert_eq!(state.semantic.ui.inventory_packets, 0);
    }

    #[test]
    fn client_non_data_payload_and_unknown_frame_type_fail_before_state_mutation() {
        let payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );
        let payload_control = reliable_server_m_frame(18, 7, 0x10, 1, &payload);
        let unknown_control = reliable_server_m_frame(19, 8, 0x30, 0, &[]);
        let low_flag_control = reliable_server_m_frame(20, 9, 0x14, 0, &[]);
        let mut state = SessionState::default();

        assert!(translate_client_to_server(&payload_control, &mut state).is_err());
        assert!(translate_client_to_server(&unknown_control, &mut state).is_err());
        assert!(translate_client_to_server(&low_flag_control, &mut state).is_err());
        assert_eq!(state.sequence.latest_client_sequence_from_client, None);
        assert_eq!(state.sequence.latest_client_ack_from_client, None);
        assert_eq!(state.semantic.ui.inventory_packets, 0);
        assert!(state.client_reliable_replays.slots.is_empty());

        let strict = crate::strict::decide_verified_translated(
            crate::packet::Direction::ClientToServer,
            VerifiedFamily::ConsumedEmptyMFrame,
            &unknown_control,
        );
        assert_eq!(strict.verdict, crate::strict::Verdict::Quarantine);
        assert_eq!(strict.reason, "unsupported-frame-type");

        let strict_low_flags = crate::strict::decide_verified_translated(
            crate::packet::Direction::ClientToServer,
            VerifiedFamily::ConsumedEmptyMFrame,
            &low_flag_control,
        );
        assert_eq!(strict_low_flags.verdict, crate::strict::Verdict::Quarantine);
        assert_eq!(strict_low_flags.reason, "invalid-control-frame-shape");
    }

    #[test]
    fn corrupt_source_frames_do_not_publish_m_or_bn_session_state() {
        let mut translator = strict_session_translator_for_test();
        translator.bn_state.remember_nwsync_advertised_to_client();
        assert!(translator.bn_state.should_consume_nwsync_handoff_bndm());
        translator
            .m_state
            .sequence
            .latest_client_sequence_from_client = Some(41);
        translator.m_state.sequence.latest_client_ack_from_client = Some(52);

        let payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );
        let mut corrupt = client_reliable_m_frame(42, 53, &payload);
        corrupt[1] ^= 0x01;
        let parsed = MFrameView::parse(&corrupt).expect("corrupt frame remains parseable");
        assert!(!parsed.crc_valid);

        assert!(matches!(
            translator.translate(crate::packet::Direction::ClientToServer, &corrupt),
            Emit::Drop
        ));
        assert!(translator.bn_state.should_consume_nwsync_handoff_bndm());
        assert!(matches!(
            translator.translate(crate::packet::Direction::ServerToClient, &corrupt),
            Emit::Drop
        ));
        assert!(translator.bn_state.should_consume_nwsync_handoff_bndm());
        assert_eq!(
            translator
                .m_state
                .sequence
                .latest_client_sequence_from_client,
            Some(41)
        );
        assert_eq!(
            translator.m_state.sequence.latest_client_ack_from_client,
            Some(52)
        );
        assert_eq!(
            translator.m_state.sequence.latest_server_sequence_to_client,
            None
        );
        assert_eq!(translator.m_state.semantic.ui.inventory_packets, 0);
        assert!(translator.m_state.client_reliable_replays.slots.is_empty());
        assert!(
            translator
                .m_state
                .sequence
                .pending_client_to_server_packets
                .is_empty()
        );
    }

    #[test]
    fn invalid_server_control_shapes_fail_before_transport_or_semantic_state() {
        let payload = client_gui_inventory::build_status_payload(
            client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
            true,
        );
        let invalid_frames = [
            reliable_server_m_frame(61, 71, 0x10, 1, &payload),
            reliable_server_m_frame(62, 72, 0x24, 0, &[]),
            reliable_server_m_frame(63, 73, 0x30, 0, &[]),
        ];
        let mut state = SessionState::default();
        state.sequence.latest_server_sequence_to_client = Some(60);

        for frame in invalid_frames {
            assert!(translate_server_to_client_inner(&frame, &mut state).is_err());
            assert_eq!(state.sequence.latest_server_sequence_to_client, Some(60));
            assert!(state.sequence.server_sequence_shifts.is_empty());
            assert!(state.sequence.pending_client_to_server_packets.is_empty());
            assert!(state.direct_server_semantic_replays.completed.is_empty());
            assert_eq!(state.server_reliable_slots.origin_generation, 0);
            assert!(state.server_reliable_slots.slots.is_empty());
            assert!(state.semantic.recent_events.is_empty());
            assert!(state.synthetic_area.pending_area_loaded.is_none());
            assert!(state.synthetic_area.in_flight_area_loaded.is_none());
            assert!(state.deflate.server_reassembly.is_none());
            assert!(state.deflate.server_emit_effect_transaction_kind.is_none());
        }
    }

    #[test]
    fn reliable_client_slot_window_rejects_outside_without_evicting_live_slots() {
        let mut state = SessionState::default();
        let first = client_reliable_m_frame(7, 1, &[0x70, 0x0D, 0x01, 0]);
        let first_view = MFrameView::parse(&first).expect("first client reliable slot");
        let first_key = match client_replay::prepare_source_slot(
            &mut state.client_reliable_replays,
            &first,
            &first_view,
        )
        .expect("pin first client slot")
        {
            client_replay::PreparedClientReliableSource::Pending(key) => key,
            other => panic!("expected pending first client slot, got {other:?}"),
        };
        client_replay::stage_translation(
            &mut state.client_reliable_replays,
            first_key,
            VerifiedFamily::ClientInput,
            Some(first.clone()),
        )
        .expect("stage first client replay");

        for index in 0..(client_replay::MAX_CLIENT_RELIABLE_SLOTS - 1) {
            let packet =
                client_reliable_m_frame(8u16.wrapping_add(index as u16), 2, &[index as u8]);
            let view = MFrameView::parse(&packet).expect("bounded client reliable slot");
            let key = match client_replay::prepare_source_slot(
                &mut state.client_reliable_replays,
                &packet,
                &view,
            )
            .expect("pin bounded client slot")
            {
                client_replay::PreparedClientReliableSource::Pending(key) => key,
                other => panic!("expected pending bounded client slot, got {other:?}"),
            };
            client_replay::stage_translation(
                &mut state.client_reliable_replays,
                key,
                VerifiedFamily::ClientInput,
                Some(packet),
            )
            .expect("stage bounded client replay");
        }

        assert_eq!(
            state.client_reliable_replays.slots.len(),
            client_replay::MAX_CLIENT_RELIABLE_SLOTS
        );
        assert!(
            state
                .client_reliable_replays
                .slots
                .iter()
                .any(|slot| slot.key == first_key)
        );

        let seventeenth = client_reliable_m_frame(23, 3, &[0xAA]);
        let seventeenth_view = MFrameView::parse(&seventeenth).expect("17th client slot");
        assert!(matches!(
            client_replay::prepare_source_slot(
                &mut state.client_reliable_replays,
                &seventeenth,
                &seventeenth_view,
            )
            .expect("classify 17th client slot"),
            client_replay::PreparedClientReliableSource::OutsideWindow(_)
        ));
        assert_eq!(
            state.client_reliable_replays.slots.len(),
            client_replay::MAX_CLIENT_RELIABLE_SLOTS
        );
        assert!(matches!(
            client_replay::prepare_source_slot(
                &mut state.client_reliable_replays,
                &first,
                &first_view,
            )
            .expect("exact live first slot"),
            client_replay::PreparedClientReliableSource::Replay { .. }
        ));

        assert_eq!(
            client_replay::retire_through_server_ack(&mut state.client_reliable_replays, 14,),
            8
        );
        assert_eq!(state.client_reliable_replays.receive_start, Some(15));
        assert_eq!(state.client_reliable_replays.slots.len(), 8);
        assert!(matches!(
            client_replay::prepare_source_slot(
                &mut state.client_reliable_replays,
                &first,
                &first_view,
            )
            .expect("classify stale retired client slot"),
            client_replay::PreparedClientReliableSource::OutsideWindow(_)
        ));
        assert!(matches!(
            client_replay::prepare_source_slot(
                &mut state.client_reliable_replays,
                &seventeenth,
                &seventeenth_view,
            )
            .expect("admit client slot after cumulative ACK"),
            client_replay::PreparedClientReliableSource::Pending(_)
        ));
    }

    #[test]
    fn reliable_ack_cannot_advance_past_contiguous_pinned_frontier() {
        let payload = &[0x70, 0x0D, 0x01, 0];
        let mut client_state = client_replay::ClientReliableReplayState::default();
        for sequence in [7, 9] {
            let packet = client_reliable_m_frame(sequence, 30, payload);
            let view = MFrameView::parse(&packet).expect("sparse client source");
            assert!(matches!(
                client_replay::prepare_source_slot(&mut client_state, &packet, &view)
                    .expect("pin sparse client source"),
                client_replay::PreparedClientReliableSource::Pending(_)
            ));
        }
        assert_eq!(
            client_replay::retire_through_server_ack(&mut client_state, 15),
            0
        );
        assert_eq!(
            client_replay::retire_through_server_ack(&mut client_state, 9),
            0
        );
        assert_eq!(client_state.receive_start, Some(7));
        assert_eq!(client_state.slots.len(), 2);
        let client_gap = client_reliable_m_frame(8, 30, payload);
        let client_gap_view = MFrameView::parse(&client_gap).expect("client gap source");
        assert!(matches!(
            client_replay::prepare_source_slot(&mut client_state, &client_gap, &client_gap_view,)
                .expect("pin client gap source"),
            client_replay::PreparedClientReliableSource::Pending(_)
        ));
        assert_eq!(
            client_replay::retire_through_server_ack(&mut client_state, 9),
            3
        );
        assert_eq!(client_state.receive_start, Some(10));
        assert!(client_state.slots.is_empty());

        let mut server_state = server_replay::ServerReliableSlotState::default();
        for sequence in [40, 42] {
            let packet = client_reliable_m_frame(sequence, 30, payload);
            let view = MFrameView::parse(&packet).expect("sparse server source");
            assert!(matches!(
                server_replay::prepare_source_slot(&mut server_state, &packet, &view)
                    .expect("pin sparse server source"),
                server_replay::PreparedServerReliableSource::Pinned(_)
            ));
        }
        assert_eq!(
            server_replay::retire_through_client_ack(&mut server_state, 48),
            0
        );
        assert_eq!(
            server_replay::retire_through_client_ack(&mut server_state, 42),
            0
        );
        assert_eq!(server_state.receive_start, Some(40));
        assert_eq!(server_state.slots.len(), 2);
        let server_gap = client_reliable_m_frame(41, 30, payload);
        let server_gap_view = MFrameView::parse(&server_gap).expect("server gap source");
        assert!(matches!(
            server_replay::prepare_source_slot(&mut server_state, &server_gap, &server_gap_view)
                .expect("pin server gap source"),
            server_replay::PreparedServerReliableSource::Pinned(_)
        ));
        assert_eq!(
            server_replay::retire_through_client_ack(&mut server_state, 42),
            3
        );
        assert_eq!(server_state.receive_start, Some(43));
        assert!(server_state.slots.is_empty());
    }

    #[test]
    fn quickbar_hint_augmentation_serializes_inventory_bridge_output_queue_state() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.queued_outputs = 1;
        bridge.deferred_client_gui_updates = 2;
        bridge.deferred_missing_claim_updates = 3;
        bridge.blocked_candidate_mismatch_updates = 4;
        bridge.last_decision_state_update_index = Some(8);
        bridge.last_deferred_client_gui_update_index = Some(9);
        bridge.last_deferred_missing_claim_update_index = Some(10);
        bridge.last_blocked_candidate_mismatch_update_index = Some(11);
        bridge.last_queued_output = Some(state::InventoryEquipmentBridgeQueuedOutput {
            update_index: 5,
            emission_index: 6,
            event_index: 7,
            minor: 1,
            object_id: 0x8000_1234,
            result: true,
            equip_slot: 0x0002_0000,
            trigger_sequence: 10,
            synthetic_sequence: 11,
        });

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.ends_with("\n}\n"));
        assert!(
            body.contains(
                "\"inventory_equipment_bridge_output_status\": \"queued_inventory_output\""
            )
        );
        assert!(
            body.contains(
                "\"inventory_equipment_bridge_output_requires_client_gui_writer\": false"
            )
        );
        assert!(body.contains("\"inventory_equipment_bridge_output_queued_packets\": 1"));
        assert!(
            body.contains("\"inventory_equipment_bridge_output_deferred_client_gui_updates\": 2")
        );
        assert!(
            body.contains("\"inventory_equipment_bridge_output_last_decision_update_index\": 8")
        );
        assert!(body.contains("\"inventory_equipment_bridge_output_last_decision_known\": false"));
        assert!(
            body.contains("\"inventory_equipment_bridge_output_last_decision_reason\": \"none\"")
        );
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_deferred_client_gui_update_index\": 9"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_deferred_missing_claim_update_index\": 10"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_blocked_candidate_mismatch_update_index\": 11"
        ));
        assert!(body.contains("\"inventory_equipment_bridge_output_last_queued_known\": true"));
        assert!(body.contains("\"inventory_equipment_bridge_output_last_queued_update_index\": 5"));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_object_id_hex\": \"0x80001234\""
        ));
        assert!(body.contains("\"inventory_equipment_bridge_output_last_queued_result\": true"));
        assert!(
            body.contains("\"inventory_equipment_bridge_output_last_queued_equip_slot\": 131072")
        );
    }

    #[test]
    fn quickbar_hint_augmentation_serializes_confirmed_inventory_replay() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.queued_outputs = 1;
        bridge.confirmed_inventory_replay_outputs = 1;
        bridge.last_confirmed_inventory_replay_update_index = Some(12);

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.contains(
            "\"inventory_equipment_bridge_output_status\": \"client_gui_status_inventory_replay_queued\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_confirmed_inventory_replay_packets\": 1"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_confirmed_inventory_replay_pending\": false"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_confirmed_inventory_replay_update_index\": 12"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_confirmed_inventory_replay_dispatched_packets\": 0"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_confirmed_inventory_replay_queued_for_dispatch\": true"
        ));

        bridge.record_confirmed_inventory_replay_dispatch();
        let dispatched_body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );
        assert!(dispatched_body.contains(
            "\"inventory_equipment_bridge_output_status\": \"client_gui_status_inventory_replay_dispatched\""
        ));
        assert!(dispatched_body.contains(
            "\"inventory_equipment_bridge_output_confirmed_inventory_replay_dispatched_packets\": 1"
        ));
        assert!(dispatched_body.contains(
            "\"inventory_equipment_bridge_output_confirmed_inventory_replay_queued_for_dispatch\": false"
        ));
        assert!(dispatched_body.contains(
            "\"inventory_equipment_bridge_output_last_confirmed_inventory_replay_dispatch_update_index\": 12"
        ));
    }

    #[test]
    fn quickbar_hint_augmentation_serializes_inventory_bridge_output_last_decision() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.last_decision_state_update_index = Some(5);
        bridge.last_decision = Some(state::InventoryEquipmentBridgeOutputDecision {
            kind: state::InventoryEquipmentBridgeOutputDecisionKind::BlockedCandidateMismatch,
            update_index: 5,
            emission_index: 6,
            event_index: 7,
            consumer:
                crate::translate::semantic::InventoryEquipmentHandoffConsumer::ServerInventory,
            candidate: crate::translate::semantic::InventoryItemContextCandidate {
                object_id: 0x8000_1234,
                proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
                source: crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
            },
            candidate_object_status: crate::translate::semantic::InventoryItemObjectStatus::Proven(
                crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
            ),
            ready_objects: 18,
            deferred_feature25_only_objects: 2,
            server_inventory_claim: Some(
                crate::translate::semantic::InventoryEquipmentServerInventoryClaim::new(
                    1,
                    0x8000_5678,
                    true,
                    4,
                ),
            ),
            server_inventory_claim_object_status:
                crate::translate::semantic::InventoryItemObjectStatus::Unknown,
            server_inventory_claim_proven_neighborhood:
                crate::translate::semantic::InventoryItemObjectProvenNeighborhood {
                    lower: Some(
                        crate::translate::semantic::InventoryItemObjectProvenNeighbor {
                            object_id: 0x8000_1234,
                            distance: 0x4444,
                        },
                    ),
                    higher: Some(
                        crate::translate::semantic::InventoryItemObjectProvenNeighbor {
                            object_id: 0x8000_6789,
                            distance: 0x1111,
                        },
                    ),
                },
            client_gui_inventory_claim: None,
        });

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.contains("\"inventory_equipment_bridge_output_last_decision_known\": true"));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_reason\": \"blocked_candidate_mismatch\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_consumer\": \"server_inventory\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_candidate_object_id_hex\": \"0x80001234\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_candidate_proof\": \"active_object\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_candidate_source\": \"direct_only\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_candidate_object_status\": \"proven\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_candidate_object_status_proof\": \"active_object\""
        ));
        assert!(
            body.contains("\"inventory_equipment_bridge_output_last_decision_ready_objects\": 18")
        );
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_deferred_feature25_only_objects\": 2"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_known\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_id_hex\": \"0x80005678\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_status\": \"unknown\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_object_status_proof\": \"none\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_known\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_object_id_hex\": \"0x80006789\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_closest_proven_item_distance\": 4369"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_known\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_object_id_hex\": \"0x80001234\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_lower_proven_item_distance\": 17476"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_known\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_object_id_hex\": \"0x80006789\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_higher_proven_item_distance\": 4369"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_result\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_server_inventory_claim_equip_slot\": 4"
        ));
    }

    #[test]
    fn quickbar_hint_augmentation_serializes_inventory_bridge_output_client_gui_claim() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.deferred_client_gui_updates = 1;
        bridge.last_decision_state_update_index = Some(12);
        bridge.last_decision = Some(state::InventoryEquipmentBridgeOutputDecision {
            kind: state::InventoryEquipmentBridgeOutputDecisionKind::DeferredClientGui,
            update_index: 12,
            emission_index: 13,
            event_index: 14,
            consumer:
                crate::translate::semantic::InventoryEquipmentHandoffConsumer::ClientGuiInventory,
            candidate: crate::translate::semantic::InventoryItemContextCandidate {
                object_id: 0x8000_1234,
                proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
                source: crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
            },
            candidate_object_status: crate::translate::semantic::InventoryItemObjectStatus::Proven(
                crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
            ),
            ready_objects: 18,
            deferred_feature25_only_objects: 2,
            server_inventory_claim: None,
            server_inventory_claim_object_status:
                crate::translate::semantic::InventoryItemObjectStatus::Unknown,
            server_inventory_claim_proven_neighborhood:
                crate::translate::semantic::InventoryItemObjectProvenNeighborhood::default(),
            client_gui_inventory_claim: Some(
                crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaim {
                    kind: crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaimKind::SelectPanel,
                    object_id: None,
                    panel: Some(3),
                    player_inventory_gui: Some(true),
                    rewritten_self_object_id: false,
                },
            ),
        });

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.contains(
            "\"inventory_equipment_bridge_output_status\": \"awaiting_client_gui_writer\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_known\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_kind\": \"select_panel\""
        ));
        assert!(
            body.contains("\"inventory_equipment_bridge_output_last_decision_ready_objects\": 18")
        );
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_deferred_feature25_only_objects\": 2"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_panel\": 3"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_player_inventory_gui\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_decision_client_gui_inventory_claim_rewritten_self_object_id\": false"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_action\": \"select_panel\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_emission_enabled\": false"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_blocked_reason\": \"client_gui_inventory_status_required_before_select_panel\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_payload_kind\": \"GuiInventory_SelectPanel\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_payload_hex\": \"700D02080000000390\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_select_panel\": 3"
        ));
    }

    #[test]
    fn quickbar_hint_augmentation_serializes_client_gui_status_writer_plan() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.deferred_client_gui_updates = 1;
        bridge.last_decision_state_update_index = Some(12);
        bridge.last_decision = Some(state::InventoryEquipmentBridgeOutputDecision {
            kind: state::InventoryEquipmentBridgeOutputDecisionKind::DeferredClientGui,
            update_index: 12,
            emission_index: 13,
            event_index: 14,
            consumer:
                crate::translate::semantic::InventoryEquipmentHandoffConsumer::ClientGuiInventory,
            candidate: crate::translate::semantic::InventoryItemContextCandidate {
                object_id: 0x8000_1234,
                proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
                source: crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
            },
            candidate_object_status: crate::translate::semantic::InventoryItemObjectStatus::Proven(
                crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
            ),
            ready_objects: 51,
            deferred_feature25_only_objects: 0,
            server_inventory_claim: None,
            server_inventory_claim_object_status:
                crate::translate::semantic::InventoryItemObjectStatus::Unknown,
            server_inventory_claim_proven_neighborhood:
                crate::translate::semantic::InventoryItemObjectProvenNeighborhood::default(),
            client_gui_inventory_claim: Some(
                crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaim {
                    kind: crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaimKind::Status,
                    object_id: Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID),
                    panel: None,
                    player_inventory_gui: Some(true),
                    rewritten_self_object_id: false,
                },
            ),
        });

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.contains(
            "\"inventory_equipment_bridge_output_status\": \"awaiting_client_gui_writer\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_action\": \"status_current_player_inventory\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_emission_enabled\": false"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_payload_kind\": \"GuiInventory_Status\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_payload_hex\": \"700D010B0000000000007F90\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_status_object_id_hex\": \"0x7F000000\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_status_object_is_current_player\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_player_inventory_gui\": true"
        ));
    }

    #[test]
    fn client_gui_status_response_tie_breaks_to_latest_equal_strength() {
        let candidate = crate::translate::semantic::InventoryItemContextCandidate {
            object_id: 0x8001_5379,
            proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
            source: crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
        };
        let earlier_live_object_only = state::InventoryEquipmentBridgeClientGuiStatusResponse {
            queued_update_index: 1,
            server_sequence: 58,
            server_peer_ack_sequence: 80,
            ack_sequence: 80,
            live_gui_records: 0,
            live_gui_fragment_bits: 0,
            materialized_item_object_ids: 0,
            materialized_item_object_id_first: 0,
            materialized_item_object_id_last: 0,
            materialized_item_object_id_min: 0,
            materialized_item_object_id_max: 0,
            materialized_item_object_ids_contain_queued_candidate: false,
            compact_item_emission_ready_objects: 51,
            compact_item_emission_ready_candidate: Some(candidate),
        };
        let later_live_object_only = state::InventoryEquipmentBridgeClientGuiStatusResponse {
            queued_update_index: 17,
            server_sequence: 58,
            server_peer_ack_sequence: 81,
            ack_sequence: 81,
            live_gui_records: 0,
            live_gui_fragment_bits: 0,
            materialized_item_object_ids: 0,
            materialized_item_object_id_first: 0,
            materialized_item_object_id_last: 0,
            materialized_item_object_id_min: 0,
            materialized_item_object_id_max: 0,
            materialized_item_object_ids_contain_queued_candidate: false,
            compact_item_emission_ready_objects: 65,
            compact_item_emission_ready_candidate: Some(candidate),
        };
        let earlier_materialized = state::InventoryEquipmentBridgeClientGuiStatusResponse {
            materialized_item_object_ids: 1,
            materialized_item_object_id_first: 0x8001_5379,
            materialized_item_object_id_last: 0x8001_5379,
            materialized_item_object_id_min: 0x8001_5379,
            materialized_item_object_id_max: 0x8001_5379,
            materialized_item_object_ids_contain_queued_candidate: true,
            ..earlier_live_object_only
        };

        assert!(later_live_object_only.is_stronger_than(earlier_live_object_only));
        assert!(!earlier_live_object_only.is_stronger_than(later_live_object_only));
        assert!(earlier_materialized.is_stronger_than(later_live_object_only));
        assert!(!later_live_object_only.is_stronger_than(earlier_materialized));

        let before_wrap = state::InventoryEquipmentBridgeClientGuiStatusResponse {
            server_sequence: u16::MAX,
            server_peer_ack_sequence: u16::MAX,
            ..later_live_object_only
        };
        let after_wrap = state::InventoryEquipmentBridgeClientGuiStatusResponse {
            server_sequence: 1,
            server_peer_ack_sequence: 1,
            ..before_wrap
        };
        assert!(after_wrap.is_stronger_than(before_wrap));
        assert!(!before_wrap.is_stronger_than(after_wrap));

        let mut bridge = state::InventoryEquipmentBridgeState {
            queued_client_gui_status_outputs: 17,
            client_gui_status_response_live_object_packets: 18,
            last_queued_client_gui_status_output: Some(
                state::InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                    update_index: 17,
                    emission_index: 17,
                    event_index: 21,
                    candidate: Some(candidate),
                    ready_objects: 65,
                    deferred_feature25_only_objects: 0,
                    object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                    player_inventory_gui: true,
                    trigger_client_sequence: 83,
                    synthetic_sequence: 99,
                    ack_sequence: 57,
                },
            ),
            best_client_gui_status_response: Some(later_live_object_only),
            ..Default::default()
        };

        assert_eq!(
            bridge.client_gui_status_response_outcome(),
            state::InventoryEquipmentBridgeClientGuiStatusResponseOutcome::LiveObjectOnly
        );
        assert_eq!(
            bridge.best_client_gui_status_response_association(),
            state::InventoryEquipmentBridgeClientGuiStatusResponseAssociation::MatchesQueuedStatusCandidate
        );

        bridge.best_client_gui_status_response = Some(earlier_live_object_only);
        assert_eq!(
            bridge.best_client_gui_status_response_association(),
            state::InventoryEquipmentBridgeClientGuiStatusResponseAssociation::QueuedUpdateMismatch
        );
    }

    #[test]
    fn quickbar_hint_augmentation_serializes_queued_client_gui_status_output() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.queued_client_gui_status_outputs = 1;
        bridge.last_decision_state_update_index = Some(12);
        bridge.last_queued_client_gui_status_update_index = Some(12);
        bridge.last_queued_client_gui_status_output =
            Some(state::InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 12,
                emission_index: 13,
                event_index: 14,
                candidate: Some(crate::translate::semantic::InventoryItemContextCandidate {
                    object_id: 0x8000_1234,
                    proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
                    source:
                        crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
                }),
                ready_objects: 51,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: true,
                trigger_client_sequence: 80,
                synthetic_sequence: 81,
                ack_sequence: 70,
            });
        bridge.client_gui_status_request_acknowledgements = 1;
        bridge.last_acknowledged_client_gui_status_update_index = Some(12);
        bridge.last_acknowledged_client_gui_status_server_ack_sequence = Some(81);
        bridge.client_gui_status_pre_ack_live_object_packets_ignored = 1;
        bridge.last_pre_ack_client_gui_status_live_object_server_sequence = Some(47);
        bridge.last_pre_ack_client_gui_status_live_object_server_ack_sequence = Some(80);
        bridge.client_gui_status_response_live_object_packets = 1;
        bridge.client_gui_status_response_live_gui_record_packets = 1;
        bridge.client_gui_status_response_materialized_item_packets = 1;
        bridge.last_client_gui_status_response =
            Some(state::InventoryEquipmentBridgeClientGuiStatusResponse {
                queued_update_index: 12,
                server_sequence: 48,
                server_peer_ack_sequence: 82,
                ack_sequence: 82,
                live_gui_records: 51,
                live_gui_fragment_bits: 348,
                materialized_item_object_ids: 51,
                materialized_item_object_id_first: 0x8000_1200,
                materialized_item_object_id_last: 0x8000_1234,
                materialized_item_object_id_min: 0x8000_1200,
                materialized_item_object_id_max: 0x8000_1234,
                materialized_item_object_ids_contain_queued_candidate: true,
                compact_item_emission_ready_objects: 51,
                compact_item_emission_ready_candidate: Some(
                    crate::translate::semantic::InventoryItemContextCandidate {
                        object_id: 0x8000_1234,
                        proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
                        source:
                            crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
                    },
                ),
            });
        bridge.best_client_gui_status_response = bridge.last_client_gui_status_response;
        bridge.last_decision = Some(state::InventoryEquipmentBridgeOutputDecision {
            kind: state::InventoryEquipmentBridgeOutputDecisionKind::QueuedClientGuiStatusOutput,
            update_index: 12,
            emission_index: 13,
            event_index: 14,
            consumer:
                crate::translate::semantic::InventoryEquipmentHandoffConsumer::ClientGuiInventory,
            candidate: crate::translate::semantic::InventoryItemContextCandidate {
                object_id: 0x8000_1234,
                proof: crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
                source: crate::translate::semantic::InventoryItemContextCandidateSource::DirectOnly,
            },
            candidate_object_status: crate::translate::semantic::InventoryItemObjectStatus::Proven(
                crate::translate::semantic::InventoryItemObjectProof::ActiveObject,
            ),
            ready_objects: 51,
            deferred_feature25_only_objects: 0,
            server_inventory_claim: None,
            server_inventory_claim_object_status:
                crate::translate::semantic::InventoryItemObjectStatus::Unknown,
            server_inventory_claim_proven_neighborhood:
                crate::translate::semantic::InventoryItemObjectProvenNeighborhood::default(),
            client_gui_inventory_claim: Some(
                crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaim {
                    kind: crate::translate::semantic::InventoryEquipmentClientGuiInventoryClaimKind::Status,
                    object_id: Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID),
                    panel: None,
                    player_inventory_gui: Some(true),
                    rewritten_self_object_id: false,
                },
            ),
        });

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.contains(
            "\"inventory_equipment_bridge_output_status\": \"client_gui_status_refresh_confirmed\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_request_completion\": \"materialized_current_player_inventory\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_request_acknowledged\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_acknowledged_client_gui_status_server_ack_sequence\": 81"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_client_gui_status_response_server_peer_ack_sequence\": 82"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_server_peer_ack_sequence\": 82"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_pre_ack_live_object_packets_ignored\": 1"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_refresh_confirmed\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_emission_enabled\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_writer_plan_blocked_reason\": \"none\""
        ));
        assert!(
            body.contains(
                "\"inventory_equipment_bridge_output_queued_client_gui_status_packets\": 1"
            )
        );
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_object_id_hex\": \"0x7F000000\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_payload_hex\": \"700D010B0000000000007F90\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_synthetic_sequence\": 81"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_ack_sequence\": 70"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_known\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_object_id_hex\": \"0x80001234\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_proof\": \"active_object\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_candidate_source\": \"direct_only\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_queued_client_gui_status_ready_objects\": 51"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_response_live_object_packets\": 1"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_response_live_gui_record_packets\": 1"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_response_materialized_item_packets\": 1"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_client_gui_status_response_live_gui_records\": 51"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_client_gui_status_response_live_gui_fragment_bits\": 348"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_first_hex\": \"0x80001200\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_id_last_hex\": \"0x80001234\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_client_gui_status_response_materialized_item_object_ids_contain_queued_candidate\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_client_gui_status_response_candidate_object_id_hex\": \"0x80001234\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_client_gui_status_response_outcome\": \"materialized_items\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_known\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_live_gui_records\": 51"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_first_hex\": \"0x80001200\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_id_last_hex\": \"0x80001234\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_materialized_item_object_ids_contain_queued_candidate\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_object_id_hex\": \"0x80001234\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_association\": \"matches_queued_status_candidate\""
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_matches_queued_status_candidate\": true"
        ));
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_best_client_gui_status_response_candidate_delta_from_queued_status_candidate\": 0"
        ));
    }

    #[test]
    fn inventory_bridge_output_status_marks_client_gui_writer_gap() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.deferred_client_gui_updates = 1;
        bridge.last_deferred_client_gui_update_index = Some(3);

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.contains(
            "\"inventory_equipment_bridge_output_status\": \"awaiting_client_gui_writer\""
        ));
        assert!(
            body.contains("\"inventory_equipment_bridge_output_requires_client_gui_writer\": true")
        );
        assert!(body.contains(
            "\"inventory_equipment_bridge_output_last_deferred_client_gui_update_index\": 3"
        ));
    }

    #[test]
    fn inventory_bridge_output_status_prefers_server_inventory_blockers() {
        let mut bridge = state::InventoryEquipmentBridgeState::default();
        bridge.deferred_client_gui_updates = 3;
        bridge.deferred_missing_claim_updates = 2;
        bridge.blocked_candidate_mismatch_updates = 1;

        let body = augment_quickbar_item_refresh_hint_with_bridge_output(
            "{\n  \"kind\": \"quickbar_item_refresh_candidate\"\n}\n".to_string(),
            &bridge,
        );

        assert!(body.contains(
            "\"inventory_equipment_bridge_output_status\": \"blocked_candidate_mismatch\""
        ));
        assert!(
            body.contains(
                "\"inventory_equipment_bridge_output_requires_client_gui_writer\": false"
            )
        );
    }

    #[test]
    fn proxy_owned_client_ack_coalesces_and_releases_from_session_drain() {
        let mut state = SessionState::default();
        state.sequence.latest_server_sequence_to_client = Some(7);
        queue_proxy_owned_ack_for_consumed_client_frame(&mut state, 40).expect("queue ACK");
        queue_proxy_owned_ack_for_consumed_client_frame(&mut state, 42).expect("coalesce ACK");

        let emit = take_pending_server_to_client_packets(&mut state)
            .expect("pending proxy-owned ACK drain");
        let Emit::MixedVerifiedProofPacketsPreShifted(packets) = emit else {
            panic!("expected immediate pending ACK release, got {emit:?}");
        };

        assert_eq!(packets.len(), 1);
        assert_eq!(
            packets[0].0,
            VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame)
        );
        let view = MFrameView::parse(&packets[0].1).expect("pending ACK should parse");
        assert_eq!(view.sequence, 0);
        assert_eq!(view.ack_sequence, 42);
        assert_eq!(view.flags, 0x10);
        assert_eq!(view.payload_length, 0);
        assert!(
            state
                .client_ack
                .pending
                .pending_consumed_ee_only_ack
                .is_some()
        );
        assert_eq!(state.sequence.latest_server_sequence_to_client, Some(7));
        finish_pending_server_drain_emit_validation(&mut state, true);
    }

    #[test]
    fn pending_server_drain_reject_restores_ack_queue_semantics_and_window_state() {
        let mut translator = strict_session_translator_for_test();
        for sequence in 40..=42 {
            let packet = client_reliable_m_frame(sequence, 12, &[0x70, 0x0D, 0x01, 0]);
            let view = MFrameView::parse(&packet).expect("pending ACK client slot");
            assert!(matches!(
                client_replay::prepare_source_slot(
                    &mut translator.m_state.client_reliable_replays,
                    &packet,
                    &view,
                )
                .expect("pin pending ACK client slot"),
                client_replay::PreparedClientReliableSource::Pending(_)
            ));
        }
        translator.m_state.sequence.latest_server_sequence_to_client = Some(12);
        translator
            .m_state
            .semantic
            .synthetic
            .server_synthetic_packets = 7;
        queue_proxy_owned_ack_for_consumed_client_frame(&mut translator.m_state, 42)
            .expect("queue pending ACK");
        let original_ack = translator
            .m_state
            .client_ack
            .pending
            .pending_consumed_ee_only_ack
            .clone()
            .expect("ACK should be pending before drain");

        let valid_packet =
            client_reliable_m_frame(54, 74, &crate::translate::loadbar::start_payload(2));
        translator
            .m_state
            .synthetic_area
            .pending_server_to_client_packets
            .extend([
                synthetic_area::PendingServerPacket {
                    family: VerifiedFamily::LoadBar,
                    packet: valid_packet.clone(),
                    due_at: Instant::now(),
                    reason: "pending drain rollback valid sibling",
                    placement: synthetic_area::PendingServerPacketPlacement::AfterCurrentEmit,
                },
                synthetic_area::PendingServerPacket {
                    family: VerifiedFamily::Inventory,
                    packet: vec![b'M'],
                    due_at: Instant::now(),
                    reason: inventory_equipment::CONFIRMED_CLIENT_GUI_INVENTORY_REPLAY_REASON,
                    placement: synthetic_area::PendingServerPacketPlacement::AfterCurrentEmit,
                },
            ]);

        let rejected = translator.take_pending_server_to_client_packets();

        assert!(rejected.is_empty());
        assert_eq!(
            translator
                .m_state
                .deflate
                .server_emit_effect_transaction_kind,
            None
        );
        assert!(
            translator
                .m_state
                .deflate
                .ordered_successor_effect_snapshot
                .is_none()
        );
        assert_eq!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .len(),
            2,
            "the complete due batch must remain queued after one sibling fails strict validation"
        );
        assert_eq!(
            translator
                .m_state
                .semantic
                .synthetic
                .server_synthetic_packets,
            7
        );
        assert!(translator.m_state.semantic.recent_events.is_empty());
        assert_eq!(
            translator.m_state.sequence.latest_server_sequence_to_client,
            Some(12)
        );
        assert_eq!(
            translator
                .m_state
                .inventory_equipment
                .confirmed_inventory_replay_dispatches,
            0
        );
        let restored_ack = translator
            .m_state
            .client_ack
            .pending
            .pending_consumed_ee_only_ack
            .as_ref()
            .expect("rejected ACK transmit must remain pending");
        assert_eq!(restored_ack.ack_sequence, original_ack.ack_sequence);
        assert_eq!(restored_ack.transmits, original_ack.transmits);
        assert_eq!(restored_ack.due_at, original_ack.due_at);
        assert_eq!(
            translator.m_state.client_reliable_replays.receive_start,
            Some(40)
        );
        assert_eq!(translator.m_state.client_reliable_replays.slots.len(), 3);

        let rejected_invalid = translator
            .m_state
            .synthetic_area
            .pending_server_to_client_packets
            .pop()
            .expect("invalid sibling should have been restored");
        assert_eq!(rejected_invalid.packet, vec![b'M']);

        let accepted = translator.take_pending_server_to_client_packets();

        assert_eq!(
            accepted.len(),
            2,
            "exact retry should emit ACK plus LoadBar"
        );
        assert!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert_eq!(
            translator
                .m_state
                .semantic
                .synthetic
                .server_synthetic_packets,
            8
        );
        assert_eq!(
            translator.m_state.sequence.latest_server_sequence_to_client,
            Some(54)
        );
        let committed_ack = translator
            .m_state
            .client_ack
            .pending
            .pending_consumed_ee_only_ack
            .as_ref()
            .expect("accepted ACK remains scheduled for bounded retransmit");
        assert_eq!(committed_ack.ack_sequence, 42);
        assert_eq!(committed_ack.transmits, 1);
        assert!(committed_ack.due_at > original_ack.due_at);
        assert_eq!(
            translator.m_state.client_reliable_replays.receive_start,
            Some(43)
        );
        assert!(translator.m_state.client_reliable_replays.slots.is_empty());
    }

    #[test]
    fn pending_client_drain_validates_typed_batch_and_restores_on_reject() {
        let mut translator = strict_session_translator_for_test();
        let server_source =
            client_reliable_m_frame(74, 12, &crate::translate::loadbar::start_payload(2));
        let server_view =
            MFrameView::parse(&server_source).expect("pending client ACK server slot");
        assert!(matches!(
            server_replay::prepare_source_slot(
                &mut translator.m_state.server_reliable_slots,
                &server_source,
                &server_view,
            )
            .expect("pin pending client ACK server slot"),
            server_replay::PreparedServerReliableSource::Pinned(_)
        ));
        let valid = client_reliable_m_frame(55, 74, &[0x70, 0x04, 0x03]);
        translator
            .m_state
            .sequence
            .pending_client_to_server_packets
            .extend([
                state::PendingClientPacket {
                    family: VerifiedFamily::ClientArea,
                    packet: valid.clone(),
                    reason: "pending client rollback valid sibling",
                },
                state::PendingClientPacket {
                    family: VerifiedFamily::ClientArea,
                    packet: vec![b'M'],
                    reason: "pending client rollback invalid sibling",
                },
            ]);

        let rejected = translator.take_pending_client_to_server_packets();

        assert!(rejected.is_empty());
        assert!(
            translator
                .m_state
                .pending_client_drain_effect_snapshot
                .is_none()
        );
        assert_eq!(
            translator
                .m_state
                .sequence
                .pending_client_to_server_packets
                .len(),
            2,
            "one rejected sibling must restore the complete ordered typed batch"
        );
        assert_eq!(
            translator.m_state.sequence.pending_client_to_server_packets[0].packet,
            valid
        );
        assert_eq!(
            translator.m_state.sequence.pending_client_to_server_packets[1].reason,
            "pending client rollback invalid sibling"
        );
        assert_eq!(
            translator.m_state.server_reliable_slots.receive_start,
            Some(74)
        );
        assert_eq!(translator.m_state.server_reliable_slots.slots.len(), 1);

        let invalid = translator
            .m_state
            .sequence
            .pending_client_to_server_packets
            .pop()
            .expect("invalid sibling should have been restored");
        assert_eq!(invalid.packet, vec![b'M']);
        let accepted = translator.take_pending_client_to_server_packets();

        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0], valid);
        assert!(
            translator
                .m_state
                .sequence
                .pending_client_to_server_packets
                .is_empty()
        );
        assert!(
            translator
                .m_state
                .pending_client_drain_effect_snapshot
                .is_none()
        );
        assert_eq!(
            translator.m_state.server_reliable_slots.receive_start,
            Some(75)
        );
        assert!(translator.m_state.server_reliable_slots.slots.is_empty());

        assert!(
            translator
                .take_pending_client_to_server_packets()
                .is_empty()
        );
        assert!(
            translator
                .m_state
                .pending_client_drain_effect_snapshot
                .is_none(),
            "idle polling must not retain validation authority"
        );
    }

    #[test]
    fn pending_server_drain_defers_observation_while_area_gate_blocks_validation() {
        let mut translator = strict_session_translator_for_test();
        translator.m_state.synthetic_area.server_hold_gate = Some(synthetic_area::ServerHoldGate {
            area_first_sequence: 50,
            release_client_ack_sequence: 55,
            reason: synthetic_area::AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
            armed_at: Instant::now(),
            area_window_released_at: None,
            area_ack_observed_at: None,
            release_at: None,
        });
        translator
            .m_state
            .synthetic_area
            .pending_server_to_client_packets
            .push(synthetic_area::PendingServerPacket {
                family: VerifiedFamily::Inventory,
                packet: vec![b'M'],
                due_at: Instant::now(),
                reason: inventory_equipment::CONFIRMED_CLIENT_GUI_INVENTORY_REPLAY_REASON,
                placement: synthetic_area::PendingServerPacketPlacement::AfterCurrentEmit,
            });

        let gated = translator.take_pending_server_to_client_packets();

        assert!(gated.is_empty());
        assert_eq!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .len(),
            1,
            "blocked synthetic output must stay in its typed queue"
        );
        assert!(
            translator
                .m_state
                .synthetic_area
                .held_server_to_client_packets
                .is_empty(),
            "generic held packets cannot preserve pending synthetic observation ownership"
        );
        assert_eq!(
            translator
                .m_state
                .semantic
                .synthetic
                .server_synthetic_packets,
            0
        );
        assert_eq!(
            translator
                .m_state
                .inventory_equipment
                .confirmed_inventory_replay_dispatches,
            0
        );

        translator.m_state.synthetic_area.server_hold_gate = None;
        let rejected = translator.take_pending_server_to_client_packets();

        assert!(rejected.is_empty());
        assert_eq!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .len(),
            1,
            "later strict rejection must restore the still-owned pending packet"
        );
        assert_eq!(
            translator
                .m_state
                .semantic
                .synthetic
                .server_synthetic_packets,
            0
        );
        assert_eq!(
            translator
                .m_state
                .inventory_equipment
                .confirmed_inventory_replay_dispatches,
            0
        );
    }

    #[test]
    fn direct_source_does_not_consume_pending_packet_blocked_by_outer_gate() {
        for area_gate in [true, false] {
            let mut translator = strict_session_translator_for_test();
            let gate_name = if area_gate { "area" } else { "module" };
            if area_gate {
                translator.m_state.synthetic_area.server_hold_gate =
                    Some(synthetic_area::ServerHoldGate {
                        area_first_sequence: 50,
                        release_client_ack_sequence: 55,
                        reason:
                            synthetic_area::AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
                        armed_at: Instant::now(),
                        area_window_released_at: None,
                        area_ack_observed_at: None,
                        release_at: None,
                    });
            } else {
                deferred_module_resources::arm_hold_gate_for_test(
                    &mut translator.m_state.deferred_module_resources.pending,
                    55,
                );
            }
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .push(synthetic_area::PendingServerPacket {
                    family: VerifiedFamily::Inventory,
                    packet: vec![b'M'],
                    due_at: Instant::now(),
                    reason: inventory_equipment::CONFIRMED_CLIENT_GUI_INVENTORY_REPLAY_REASON,
                    placement: synthetic_area::PendingServerPacketPlacement::AfterCurrentEmit,
                });
            let source =
                client_reliable_m_frame(20, 75, &crate::translate::loadbar::start_payload(2));

            let emit = translator.translate(crate::packet::Direction::ServerToClient, &source);

            assert!(
                !matches!(emit, Emit::Drop),
                "{gate_name} gate should allow the independent source"
            );
            assert_eq!(
                translator
                    .m_state
                    .synthetic_area
                    .pending_server_to_client_packets
                    .len(),
                1,
                "{gate_name}-blocked pending packet must retain typed ownership"
            );
            assert_eq!(
                translator
                    .m_state
                    .semantic
                    .synthetic
                    .server_synthetic_packets,
                0
            );
            assert_eq!(
                translator
                    .m_state
                    .inventory_equipment
                    .confirmed_inventory_replay_dispatches,
                0
            );
            assert_eq!(
                translator
                    .m_state
                    .deflate
                    .server_emit_effect_transaction_kind,
                None
            );
        }
    }

    #[test]
    fn direct_server_emit_with_due_synthetic_rejects_atomically() {
        let mut translator = strict_session_translator_for_test();
        translator
            .m_state
            .synthetic_area
            .pending_server_to_client_packets
            .push(synthetic_area::PendingServerPacket {
                family: VerifiedFamily::Inventory,
                packet: vec![b'M'],
                due_at: Instant::now(),
                reason: inventory_equipment::CONFIRMED_CLIENT_GUI_INVENTORY_REPLAY_REASON,
                placement: synthetic_area::PendingServerPacketPlacement::AfterCurrentEmit,
            });
        let source = client_reliable_m_frame(60, 75, &crate::translate::loadbar::start_payload(2));

        let emit = translator.translate(crate::packet::Direction::ServerToClient, &source);

        let Emit::Packets(packets) = emit else {
            panic!("strict-rejected direct batch should leave one exact ACK carrier");
        };
        assert_eq!(packets.len(), 1);
        let carrier = MFrameView::parse(&packets[0]).expect("strict-rejection ACK carrier");
        assert_eq!(carrier.frame_kind(), Some(MFrameType::AckControl));
        assert_eq!(carrier.sequence, 0);
        assert_eq!(carrier.ack_sequence, 75);
        assert!(carrier.is_exact_control_frame());
        assert!(
            translator
                .m_state
                .deflate
                .last_server_core_dispatch_accepted
        );
        assert_eq!(
            translator
                .m_state
                .synthetic_area
                .pending_server_to_client_packets
                .len(),
            1
        );
        assert!(translator.m_state.semantic.recent_events.is_empty());
        assert_eq!(
            translator
                .m_state
                .semantic
                .synthetic
                .server_synthetic_packets,
            0
        );
        assert_eq!(
            translator
                .m_state
                .inventory_equipment
                .confirmed_inventory_replay_dispatches,
            0
        );
        assert_eq!(
            translator.m_state.sequence.latest_server_sequence_to_client,
            None
        );
        assert_eq!(
            translator
                .m_state
                .deflate
                .server_emit_effect_transaction_kind,
            None
        );
        assert!(
            translator
                .m_state
                .deflate
                .ordered_successor_effect_snapshot
                .is_none()
        );
    }

    #[test]
    fn idle_pending_server_drain_does_not_open_effect_transaction() {
        let mut state = SessionState::default();

        let emit = take_pending_server_to_client_packets(&mut state)
            .expect("empty pending drain should remain a no-op");

        assert!(matches!(emit, Emit::Consumed));
        assert_eq!(state.deflate.server_emit_effect_transaction_kind, None);
        assert!(state.deflate.ordered_successor_effect_snapshot.is_none());
    }

    #[test]
    fn idle_pending_server_drain_cannot_finish_foreign_effect_transaction() {
        let mut translator = strict_session_translator_for_test();
        begin_ordinary_server_emit_effect_transaction(&mut translator.m_state)
            .expect("seed foreign ordinary transaction");

        let packets = translator.take_pending_server_to_client_packets();

        assert!(packets.is_empty());
        assert_eq!(
            translator
                .m_state
                .deflate
                .server_emit_effect_transaction_kind,
            Some(state::ServerEmitEffectTransactionKind::OrdinaryServerEmit)
        );
        assert!(
            translator
                .m_state
                .deflate
                .ordered_successor_effect_snapshot
                .is_some(),
            "the pending-drain callback must not commit or roll back foreign validation authority"
        );
        assert!(rollback_server_emit_effect_transaction(
            &mut translator.m_state
        ));
    }

    #[test]
    fn area_load_gate_releases_verified_packets_before_split_area_window() {
        let mut state = SessionState::default();
        state.synthetic_area.server_hold_gate = Some(synthetic_area::ServerHoldGate {
            area_first_sequence: 14,
            release_client_ack_sequence: 15,
            reason: synthetic_area::AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
            armed_at: Instant::now(),
            area_window_released_at: None,
            area_ack_observed_at: None,
            release_at: None,
        });

        let pre_area = hold_or_release_server_packets(
            &mut state,
            VerifiedProof::family(VerifiedFamily::ClientSideMessage),
            vec![reliable_server_m_frame(9, 0, 0x08, 0, &[])],
            "test pre-area reliable order",
        );
        assert_eq!(pre_area.len(), 1);
        assert!(
            state
                .synthetic_area
                .held_server_to_client_packets
                .is_empty()
        );
        assert!(
            state
                .synthetic_area
                .server_hold_gate
                .as_ref()
                .expect("hold gate")
                .area_window_released_at
                .is_none()
        );

        let area = hold_or_release_server_packets(
            &mut state,
            VerifiedProof::family(VerifiedFamily::AreaClientArea),
            vec![reliable_server_m_frame(14, 0, 0x08, 0, &[])],
            "test area window release",
        );
        assert_eq!(area.len(), 1);
        assert!(
            state
                .synthetic_area
                .server_hold_gate
                .as_ref()
                .expect("hold gate")
                .area_window_released_at
                .is_some()
        );

        let post_area = hold_or_release_server_packets(
            &mut state,
            VerifiedProof::family(VerifiedFamily::ClientSideMessage),
            vec![reliable_server_m_frame(16, 0, 0x08, 0, &[])],
            "test post-area gameplay hold",
        );
        assert!(post_area.is_empty());
        assert_eq!(state.synthetic_area.held_server_to_client_packets.len(), 1);
    }
}

fn hold_server_emit_until_module_resource_ack(
    state: &mut SessionState,
    emit: Emit,
) -> anyhow::Result<Emit> {
    let Some(release_client_ack_sequence) =
        deferred_module_resources::module_resource_hold_gate_release_sequence(
            &state.deferred_module_resources.pending,
        )
    else {
        return Ok(emit);
    };

    match emit {
        Emit::VerifiedPackets { family, packets }
        | Emit::VerifiedPacketsPreShifted { family, packets } => {
            let released = hold_or_release_module_resource_packets(
                state,
                VerifiedProof::family(family),
                packets,
                "post-module-resource server packet held until EE ACKs synthetic ServerStatus_ModuleResources",
            );
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::VerifiedProofPackets { proof, packets }
        | Emit::VerifiedProofPacketsPreShifted { proof, packets } => {
            let released = hold_or_release_module_resource_packets(
                state,
                proof,
                packets,
                "post-module-resource server packet held until EE ACKs synthetic ServerStatus_ModuleResources",
            );
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::MixedVerifiedPackets(packets) => {
            let mut released = Vec::new();
            for (family, packet) in packets {
                released.extend(hold_or_release_module_resource_packets(
                    state,
                    VerifiedProof::family(family),
                    vec![packet],
                    "post-module-resource mixed server packet held until EE ACKs synthetic ServerStatus_ModuleResources",
                ));
            }
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            let mut released = Vec::new();
            for (proof, packet) in packets {
                released.extend(hold_or_release_module_resource_packets(
                    state,
                    proof,
                    vec![packet],
                    "post-module-resource mixed server packet held until EE ACKs synthetic ServerStatus_ModuleResources",
                ));
            }
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::Packet(packet) => {
            let Some(family) = transport_only_verified_family_for_plain_server_packet(&packet)
            else {
                tracing::warn!(
                    release_client_ack_sequence,
                    packet_len = packet.len(),
                    "non-verified server packet encountered while module-resource hold gate is active; dropping instead of buffering without proof"
                );
                return Ok(Emit::Drop);
            };
            let released = hold_or_release_module_resource_packets(
                state,
                VerifiedProof::family(family),
                vec![packet],
                "post-module-resource transport server packet held until EE ACKs synthetic ServerStatus_ModuleResources",
            );
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::Packets(packets) | Emit::PacketsPreShifted(packets) => {
            let mut released = Vec::new();
            for packet in packets {
                let Some(family) = transport_only_verified_family_for_plain_server_packet(&packet)
                else {
                    tracing::warn!(
                        release_client_ack_sequence,
                        packet_len = packet.len(),
                        "non-verified server packet batch encountered while module-resource hold gate is active; dropping instead of buffering without proof"
                    );
                    return Ok(Emit::Drop);
                };
                released.extend(hold_or_release_module_resource_packets(
                    state,
                    VerifiedProof::family(family),
                    vec![packet],
                    "post-module-resource transport server packet held until EE ACKs synthetic ServerStatus_ModuleResources",
                ));
            }
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::PacketRetireSession { reason, .. } => {
            tracing::warn!(
                release_client_ack_sequence,
                reason,
                "client-side retire-session packet reached module-resource server hold gate; dropping instead of holding without a server proof"
            );
            Ok(Emit::Drop)
        }
        Emit::Consumed | Emit::ConsumedRetireSession { .. } | Emit::Drop => Ok(emit),
    }
}

fn hold_or_release_module_resource_packets(
    state: &mut SessionState,
    proof: VerifiedProof,
    packets: Vec<Vec<u8>>,
    reason: &'static str,
) -> Vec<(VerifiedProof, Vec<u8>)> {
    let mut released = Vec::new();
    for packet in packets {
        if module_resource_gate_window_packet_can_pass(state, &packet) {
            let release_client_ack_sequence =
                deferred_module_resources::module_resource_hold_gate_release_sequence(
                    &state.deferred_module_resources.pending,
                );
            tracing::info!(
                proof = proof.as_str(),
                release_client_ack_sequence,
                len = packet.len(),
                reason,
                "server-to-client packet released through module-resource ACK gate because its sequence belongs to the gated module/resource window"
            );
            released.push((proof.clone(), packet));
        } else {
            deferred_module_resources::hold_server_packet(
                &mut state.deferred_module_resources.pending,
                proof.clone(),
                packet,
                reason,
            );
        }
    }
    released
}

fn module_resource_gate_window_packet_can_pass(state: &SessionState, packet: &[u8]) -> bool {
    let Some(release_client_ack_sequence) =
        deferred_module_resources::module_resource_hold_gate_release_sequence(
            &state.deferred_module_resources.pending,
        )
    else {
        return false;
    };
    let Some(view) = MFrameView::parse(packet) else {
        return false;
    };
    if view.frame_type != 0 {
        return view.frame_type == 1
            && view.payload_length == 0
            && view.trailing_payload_length == 0;
    }

    !sequence_at_or_after(view.sequence, release_client_ack_sequence.wrapping_add(1))
}

fn hold_server_emit_until_area_load_ack(
    state: &mut SessionState,
    emit: Emit,
) -> anyhow::Result<Emit> {
    if state.synthetic_area.server_hold_gate.is_some()
        && maybe_queue_area_loaded_fallback_from_timer(state, "server-to-client emit finalizer")?
    {
        return Ok(release_held_area_packets_into_emit(
            state,
            emit,
            "synthetic Area_AreaLoaded fallback timer",
        ));
    }

    let Some(gate) = state.synthetic_area.server_hold_gate.as_ref() else {
        return Ok(release_held_area_packets_into_emit(
            state,
            emit,
            "area-load hold gate already open",
        ));
    };
    match emit {
        Emit::VerifiedPackets { family, packets }
        | Emit::VerifiedPacketsPreShifted { family, packets } => {
            let released = hold_or_release_server_packets(
                state,
                VerifiedProof::family(family),
                packets,
                "post-area server packet held until EE ACKs rewritten Area_ClientArea",
            );
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::VerifiedProofPackets { proof, packets }
        | Emit::VerifiedProofPacketsPreShifted { proof, packets } => {
            let released = hold_or_release_server_packets(
                state,
                proof,
                packets,
                "post-area server packet held until EE ACKs rewritten Area_ClientArea",
            );
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::MixedVerifiedPackets(packets) => {
            let mut released = Vec::new();
            for (family, packet) in packets {
                released.extend(hold_or_release_server_packets(
                    state,
                    VerifiedProof::family(family),
                    vec![packet],
                    "post-area mixed server packet held until EE ACKs rewritten Area_ClientArea",
                ));
            }
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            let mut released = Vec::new();
            for (proof, packet) in packets {
                released.extend(hold_or_release_server_packets(
                    state,
                    proof,
                    vec![packet],
                    "post-area mixed server packet held until EE ACKs rewritten Area_ClientArea",
                ));
            }
            if released.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::MixedVerifiedProofPacketsPreShifted(released))
            }
        }
        Emit::Packet(packet) => {
            if transport_only_verified_family_for_plain_server_packet(&packet).is_some() {
                Ok(Emit::Packet(packet))
            } else {
                tracing::warn!(
                    release_client_ack_sequence = gate.release_client_ack_sequence,
                    packet_len = packet.len(),
                    "non-verified server packet encountered while area-load hold gate is active; dropping instead of buffering without proof"
                );
                Ok(Emit::Drop)
            }
        }
        other => Ok(other),
    }
}

fn maybe_queue_area_loaded_fallback_from_timer(
    state: &mut SessionState,
    trigger: &'static str,
) -> anyhow::Result<bool> {
    if state.synthetic_area.pending_area_loaded.is_none() {
        return Ok(false);
    }

    let observed_client_ack = state.sequence.latest_client_ack_from_client;
    let origin_ack_sequence = observed_client_ack
        .map(|ack| unshift_ack_for_origin(&state.sequence.server_sequence_shifts, ack))
        .or(state.sequence.latest_server_sequence_to_client)
        .unwrap_or(u16::MAX);

    let Some(packet) = synthetic_area::maybe_build_area_loaded_client_packet(
        &mut state.synthetic_area.pending_area_loaded,
        &mut state.synthetic_area.in_flight_area_loaded,
        &mut state.synthetic_area.server_hold_gate,
        &mut state.sequence.latest_client_sequence_from_client,
        &mut state.sequence.client_sequence_shifts,
        observed_client_ack,
        origin_ack_sequence,
    )?
    else {
        return Ok(false);
    };

    state
        .sequence
        .pending_client_to_server_packets
        .push(state::PendingClientPacket {
            family: VerifiedFamily::ClientArea,
            packet,
            reason: "synthetic Area_AreaLoaded fallback released from timer",
        });
    tracing::info!(
        observed_client_ack = ?observed_client_ack,
        origin_ack_sequence,
        pending_client_packets = state.sequence.pending_client_to_server_packets.len(),
        held_server_packets = state.synthetic_area.held_server_to_client_packets.len(),
        trigger,
        "client synthetic Area_AreaLoaded queued from server-side fallback timer"
    );
    Ok(true)
}

fn release_held_area_packets_into_emit(
    state: &mut SessionState,
    emit: Emit,
    trigger: &'static str,
) -> Emit {
    let mut released = drain_held_area_packets(state, trigger);
    if released.is_empty() {
        return emit;
    }

    match emit {
        Emit::Consumed | Emit::Drop => Emit::MixedVerifiedProofPacketsPreShifted(released),
        Emit::ConsumedRetireSession { .. } => Emit::MixedVerifiedProofPacketsPreShifted(released),
        Emit::Packet(packet) => {
            if let Some(family) = transport_only_verified_family_for_plain_server_packet(&packet) {
                released.push((VerifiedProof::family(family), packet));
            } else {
                tracing::warn!(
                    packet_len = packet.len(),
                    trigger,
                    "non-verified server packet encountered while releasing area-load held packets; dropping current packet instead of assigning a proof"
                );
            }
            Emit::MixedVerifiedProofPacketsPreShifted(released)
        }
        Emit::PacketRetireSession { reason, .. } => {
            tracing::warn!(
                trigger,
                reason,
                "client-side retire-session packet reached area-load held-packet release; dropping current packet and not releasing held server packets"
            );
            Emit::Drop
        }
        Emit::Packets(packets) | Emit::PacketsPreShifted(packets) => {
            for packet in packets {
                if let Some(family) =
                    transport_only_verified_family_for_plain_server_packet(&packet)
                {
                    released.push((VerifiedProof::family(family), packet));
                } else {
                    tracing::warn!(
                        packet_len = packet.len(),
                        trigger,
                        "non-verified server packet encountered while releasing area-load held packets; dropping current packet instead of assigning a proof"
                    );
                }
            }
            Emit::MixedVerifiedProofPacketsPreShifted(released)
        }
        Emit::VerifiedPackets { family, packets }
        | Emit::VerifiedPacketsPreShifted { family, packets } => {
            released.extend(
                packets
                    .into_iter()
                    .map(|packet| (VerifiedProof::family(family), packet)),
            );
            Emit::MixedVerifiedProofPacketsPreShifted(released)
        }
        Emit::VerifiedProofPackets { proof, packets }
        | Emit::VerifiedProofPacketsPreShifted { proof, packets } => {
            released.extend(packets.into_iter().map(|packet| (proof.clone(), packet)));
            Emit::MixedVerifiedProofPacketsPreShifted(released)
        }
        Emit::MixedVerifiedPackets(packets) => {
            released.extend(
                packets
                    .into_iter()
                    .map(|(family, packet)| (VerifiedProof::family(family), packet)),
            );
            Emit::MixedVerifiedProofPacketsPreShifted(released)
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            released.extend(packets);
            Emit::MixedVerifiedProofPacketsPreShifted(released)
        }
    }
}

fn drain_held_area_packets(
    state: &mut SessionState,
    trigger: &'static str,
) -> Vec<(VerifiedProof, Vec<u8>)> {
    state
        .synthetic_area
        .held_server_to_client_packets
        .drain(..)
        .map(|pending| {
            tracing::info!(
                proof = pending.proof.as_str(),
                reason = pending.reason,
                len = pending.packet.len(),
                trigger,
                "server-to-client held packet released after area-load completion gate opened"
            );
            (pending.proof, pending.packet)
        })
        .collect()
}

fn hold_or_release_server_packets(
    state: &mut SessionState,
    proof: VerifiedProof,
    packets: Vec<Vec<u8>>,
    reason: &'static str,
) -> Vec<(VerifiedProof, Vec<u8>)> {
    let mut released = Vec::new();
    for packet in packets {
        if let Some(release_mode) = area_load_gate_packet_release_mode(state, &proof, &packet) {
            let release_client_ack_sequence = state
                .synthetic_area
                .server_hold_gate
                .as_ref()
                .map(|gate| gate.release_client_ack_sequence);
            tracing::info!(
                proof = proof.as_str(),
                release_client_ack_sequence,
                len = packet.len(),
                reason,
                release_mode,
                "server-to-client packet released through area-load gate"
            );
            released.push((proof.clone(), packet));
        } else {
            hold_one_server_packet(state, proof.clone(), packet, reason);
        }
    }
    released
}

fn area_load_gate_packet_release_mode(
    state: &mut SessionState,
    proof: &VerifiedProof,
    packet: &[u8],
) -> Option<&'static str> {
    area_load_gate_packet_release_mode_for_gate(
        &mut state.synthetic_area.server_hold_gate,
        proof,
        packet,
    )
}

fn area_load_gate_packet_would_release_from(
    gate: &Option<synthetic_area::ServerHoldGate>,
    proof: &VerifiedProof,
    packet: &[u8],
) -> bool {
    let mut gate = gate.clone();
    area_load_gate_packet_release_mode_for_gate(&mut gate, proof, packet).is_some()
}

fn area_load_gate_packet_release_mode_for_gate(
    gate: &mut Option<synthetic_area::ServerHoldGate>,
    proof: &VerifiedProof,
    packet: &[u8],
) -> Option<&'static str> {
    let Some(gate) = gate.as_mut() else {
        return None;
    };
    let Some(view) = MFrameView::parse(packet) else {
        return None;
    };
    if view.frame_type != 0 {
        return (view.frame_type == 1
            && view.payload_length == 0
            && view.trailing_payload_length == 0)
            .then_some("reliable ACK/control frame bypasses semantic area gate");
    }

    if !sequence_at_or_after(view.sequence, gate.area_first_sequence) {
        return Some("packet sequence precedes rewritten Area_ClientArea window");
    }

    if gate.area_window_released_at.is_some() {
        if area_load_gate_proof_can_release(proof) {
            return Some(
                "Area_ClientArea or proxy-owned area-load UI/control packet follows isolated Area_ClientArea window",
            );
        }
        return None;
    }

    if !area_load_gate_proof_can_release(proof) {
        return None;
    }

    if !sequence_at_or_after(
        view.sequence,
        gate.release_client_ack_sequence.wrapping_add(1),
    ) {
        if area_load_gate_proof_contains_area(proof) {
            gate.area_window_released_at = Some(Instant::now());
        }
        Some("packet sequence belongs to the rewritten Area_ClientArea window")
    } else {
        None
    }
}

fn area_load_gate_proof_can_release(proof: &VerifiedProof) -> bool {
    match proof {
        VerifiedProof::Family(family) => area_load_gate_family_can_release(*family),
        VerifiedProof::GameplayStream(families) => {
            !families.is_empty()
                && families
                    .iter()
                    .copied()
                    .all(area_load_gate_family_can_release)
        }
        VerifiedProof::CoalescedWindow(records) => {
            !records.is_empty() && records.iter().all(area_load_gate_proof_can_release)
        }
    }
}

fn area_load_gate_family_can_release(family: VerifiedFamily) -> bool {
    matches!(
        family,
        VerifiedFamily::AreaClientArea
            | VerifiedFamily::LoadBar
            | VerifiedFamily::ServerStatusStatus
            | VerifiedFamily::ConsumedEmptyMFrame
    )
}

fn area_load_gate_proof_contains_area(proof: &VerifiedProof) -> bool {
    match proof {
        VerifiedProof::Family(family) => *family == VerifiedFamily::AreaClientArea,
        VerifiedProof::GameplayStream(families) => {
            families.contains(&VerifiedFamily::AreaClientArea)
        }
        VerifiedProof::CoalescedWindow(records) => {
            records.iter().any(area_load_gate_proof_contains_area)
        }
    }
}

fn hold_one_server_packet(
    state: &mut SessionState,
    proof: VerifiedProof,
    packet: Vec<u8>,
    reason: &'static str,
) {
    let release_client_ack_sequence = state
        .synthetic_area
        .server_hold_gate
        .as_ref()
        .map(|gate| gate.release_client_ack_sequence);
    let proof_name = proof.as_str();
    let packet_len = packet.len();
    match synthetic_area::queue_pending_verified_server_packet(
        &mut state.synthetic_area.held_server_to_client_packets,
        proof,
        packet,
        reason,
    ) {
        synthetic_area::PendingVerifiedServerPacketQueueResult::Queued { held_packets } => {
            tracing::info!(
                proof = proof_name,
                release_client_ack_sequence,
                len = packet_len,
                reason,
                held_packets,
                "server-to-client verified packet held behind area-load completion gate"
            );
        }
        synthetic_area::PendingVerifiedServerPacketQueueResult::CollapsedReliableReplay {
            sequence,
            held_packets,
        } => {
            tracing::info!(
                proof = proof_name,
                release_client_ack_sequence,
                sequence,
                len = packet_len,
                reason,
                held_packets,
                "server-to-client reliable replay collapsed while area-load completion gate is closed"
            );
        }
    }
}

fn record_server_packet_window_state(state: &mut SessionState, packet: &[u8]) {
    let Some(view) = MFrameView::parse(packet) else {
        return;
    };
    if view.frame_type != 0 {
        return;
    }

    record_forward_progress(
        &mut state.sequence.latest_server_sequence_to_client,
        view.sequence,
    );
}

fn transport_only_verified_family_for_plain_server_packet(packet: &[u8]) -> Option<VerifiedFamily> {
    let view = MFrameView::parse(packet)?;
    if view.payload_length == 0 && view.trailing_payload_length == 0 {
        return Some(VerifiedFamily::ConsumedEmptyMFrame);
    }

    None
}

fn retarget_completed_reassembly_emit_after_progress_shells(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    emit: Emit,
) -> anyhow::Result<Emit> {
    if reassembly.expected_frames <= 1 {
        return Ok(emit);
    }

    match emit {
        Emit::Packet(packet) => {
            let mut packets = vec![packet];
            retarget_completed_reassembly_packets_after_progress_shells(
                state,
                reassembly,
                &mut packets,
            )?;
            Ok(Emit::PacketsPreShifted(packets))
        }
        Emit::Packets(mut packets) => {
            retarget_completed_reassembly_packets_after_progress_shells(
                state,
                reassembly,
                &mut packets,
            )?;
            Ok(Emit::PacketsPreShifted(packets))
        }
        Emit::VerifiedPackets {
            family,
            mut packets,
        } => {
            retarget_completed_reassembly_packets_after_progress_shells(
                state,
                reassembly,
                &mut packets,
            )?;
            Ok(Emit::VerifiedPacketsPreShifted { family, packets })
        }
        Emit::VerifiedProofPackets { proof, mut packets } => {
            retarget_completed_reassembly_packets_after_progress_shells(
                state,
                reassembly,
                &mut packets,
            )?;
            Ok(Emit::VerifiedProofPacketsPreShifted { proof, packets })
        }
        other => Ok(other),
    }
}

fn record_extra_deflated_output_sequence_shift(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    output_count: usize,
) -> anyhow::Result<()> {
    let original_count = reassembly.frames.len();
    let inserted_extra_packets = output_count.saturating_sub(original_count);
    if inserted_extra_packets == 0 {
        return Ok(());
    }
    if inserted_extra_packets > u16::MAX as usize {
        anyhow::bail!("deflated output inserted too many reliable frames");
    }
    let base = reassembly
        .first_sequence
        .wrapping_add(original_count as u16);
    state.sequence.server_sequence_shifts.push(SequenceShift {
        base,
        delta: inserted_extra_packets as u16,
    });
    trim_sequence_shifts(&mut state.sequence.server_sequence_shifts);
    tracing::info!(
        first_sequence = reassembly.first_sequence,
        original_frames = original_count,
        output_frames = output_count,
        shift_base = base,
        inserted_extra_packets,
        "server deflated rewrite inserted reliable M frames; future server sequences shifted"
    );
    Ok(())
}

fn pre_shift_current_server_packets(
    state: &SessionState,
    packets: &mut [Vec<u8>],
) -> anyhow::Result<()> {
    for packet in packets {
        shift_server_sequence_for_client(state, packet)?;
    }
    Ok(())
}

fn retarget_completed_reassembly_packets_after_progress_shells(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    packets: &mut [Vec<u8>],
) -> anyhow::Result<()> {
    if packets.is_empty() {
        return Ok(());
    }
    if packets.len() > u16::MAX as usize {
        anyhow::bail!(
            "too many delayed deflated replacement packets: {}",
            packets.len()
        );
    }

    let replacement_base = reassembly
        .first_sequence
        .wrapping_add(reassembly.expected_frames.saturating_sub(1) as u16);
    let shifted_base =
        shift_sequence_for_peer(&state.sequence.server_sequence_shifts, replacement_base);
    for (index, packet) in packets.iter_mut().enumerate() {
        write_be_u16(packet, 3, shifted_base.wrapping_add(index as u16))
            .then_some(())
            .ok_or_else(|| {
                anyhow::anyhow!("failed to retarget delayed deflated replacement sequence")
            })?;
        encode_legacy_m_crc(packet)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair delayed deflated replacement CRC"))?;
    }

    let future_shift_base = replacement_base.wrapping_add(1);
    let inserted_extra_packets = packets.len().saturating_sub(1);
    if inserted_extra_packets != 0 {
        state.sequence.server_sequence_shifts.push(SequenceShift {
            base: future_shift_base,
            delta: inserted_extra_packets as u16,
        });
        trim_sequence_shifts(&mut state.sequence.server_sequence_shifts);
    }
    tracing::info!(
        first_sequence = reassembly.first_sequence,
        expected_frames = reassembly.expected_frames,
        replacement_base,
        future_shift_base,
        shifted_base,
        replacement_packets = packets.len(),
        inserted_extra_packets,
        shifts = state.sequence.server_sequence_shifts.len(),
        "delayed deflated replacement retargeted after consumed progress shells"
    );
    Ok(())
}

fn emit_completed_server_deflated_reassembly(state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(pending) = state.deflate.server_reassembly.as_ref() else {
        return Ok(Emit::Consumed);
    };
    if pending.frames.is_empty() || pending.frames.len() < pending.expected_frames {
        return Ok(Emit::Consumed);
    }
    let Some(mut reassembly) = state.deflate.server_reassembly.take() else {
        return Ok(Emit::Consumed);
    };

    let compressed = reassembly
        .frames
        .iter()
        .flat_map(|frame| frame.compressed_chunk.iter().copied())
        .collect::<Vec<_>>();
    let source_compressed_length = compressed.len();

    // The stream bit, not the presence or absence of a zlib header, owns the
    // persistent inflater contract. Diamond's first stream window may carry a
    // zlib header and still seed history for later raw continuations. Probe the
    // exact completed-window cache before every stream-bit inflate so neither
    // shape can advance that history twice on retransmission.
    match reassembly::completed_server_stream_window(state, &reassembly, &compressed) {
        CompletedDeflatedStreamWindowMatch::Exact(window) => {
            let window_first_sequence = window.first_sequence;
            let window_server_origin_generation = window.server_origin_generation;
            let window_expected_frames = window.expected_frames;
            let window_packetized_sequence = window.packetized_sequence;
            let window_inflated_length = window.inflated_length;
            let window_pre_shifted = window.pre_shifted;
            let mut replay = window.replay;
            let mut helper_reassembly = reassembly.clone();
            helper_reassembly.interleaved_packets.clear();
            helper_reassembly.interleaved_events.clear();
            if let Some(emit) = quickbar_stream::force_flush_pending_server_quickbar_stream(
                state,
                &helper_reassembly,
                source_compressed_length,
            )? {
                tracing::info!(
                    first_sequence = reassembly.first_sequence,
                    packetized_sequence = reassembly.packetized_sequence,
                    inflated_length = reassembly.inflated_length,
                    compressed = source_compressed_length,
                    "server deflated M duplicate forced pending quickbar stream disposition"
                );
                arm_withheld_reassembly_successors(state, &reassembly)?;
                return Ok(emit);
            }
            reassembly::retarget_completed_server_stream_replay(&mut replay, &reassembly)?;
            arm_withheld_reassembly_successors(state, &reassembly)?;
            return match replay {
                CompletedDeflatedReplay::Packets(packets) => {
                    tracing::info!(
                        frames = packets.len(),
                        withheld_interleaved = reassembly.interleaved_events.len(),
                        first_sequence = window_first_sequence,
                        server_origin_generation = window_server_origin_generation,
                        expected_frames = window_expected_frames,
                        packetized_sequence = window_packetized_sequence,
                        inflated_length = window_inflated_length,
                        compressed = source_compressed_length,
                        pre_shifted = window_pre_shifted,
                        replay = "packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(if window_pre_shifted {
                        Emit::PacketsPreShifted(packets)
                    } else {
                        Emit::Packets(packets)
                    })
                }
                CompletedDeflatedReplay::VerifiedPackets { family, packets } => {
                    tracing::info!(
                        frames = packets.len(),
                        withheld_interleaved = reassembly.interleaved_events.len(),
                        first_sequence = window_first_sequence,
                        server_origin_generation = window_server_origin_generation,
                        expected_frames = window_expected_frames,
                        packetized_sequence = window_packetized_sequence,
                        inflated_length = window_inflated_length,
                        compressed = source_compressed_length,
                        pre_shifted = window_pre_shifted,
                        replay = "verified-packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(if window_pre_shifted {
                        Emit::VerifiedPacketsPreShifted { family, packets }
                    } else {
                        Emit::VerifiedPackets { family, packets }
                    })
                }
                CompletedDeflatedReplay::VerifiedProofPackets { proof, packets } => {
                    tracing::info!(
                        frames = packets.len(),
                        withheld_interleaved = reassembly.interleaved_events.len(),
                        first_sequence = window_first_sequence,
                        server_origin_generation = window_server_origin_generation,
                        expected_frames = window_expected_frames,
                        packetized_sequence = window_packetized_sequence,
                        inflated_length = window_inflated_length,
                        compressed = source_compressed_length,
                        pre_shifted = window_pre_shifted,
                        replay = "verified-proof-packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(if window_pre_shifted {
                        Emit::VerifiedProofPacketsPreShifted { proof, packets }
                    } else {
                        Emit::VerifiedProofPackets { proof, packets }
                    })
                }
            };
        }
        CompletedDeflatedStreamWindowMatch::Conflict => {
            arm_withheld_reassembly_successors(state, &reassembly)?;
            tracing::warn!(
                first_sequence = reassembly.first_sequence,
                server_origin_generation = reassembly.server_origin_generation,
                expected_frames = reassembly.expected_frames,
                packetized_sequence = reassembly.packetized_sequence,
                inflated_length = reassembly.inflated_length,
                compressed = source_compressed_length,
                "server deflated M stream rejected because a committed reliable slot carried different immutable compressed bytes"
            );
            return Ok(Emit::Drop);
        }
        CompletedDeflatedStreamWindowMatch::Miss => {}
    }

    let InflatedGameplayPayload {
        mut bytes,
        used_server_stream,
    } = reassembly::inflate_gameplay_payload(
        &compressed,
        reassembly.inflated_length,
        reassembly.zlib_stream,
        &mut state.deflate.server_zlib_inflater,
    )?;

    let old_inflated_length = bytes.len();
    let original_inflated_for_diagnostics = bytes.clone();
    dump_live_object_input_if_enabled(&original_inflated_for_diagnostics, &reassembly);
    log_inflated_high_level_summary(&bytes, &reassembly);
    // Stream-family helpers own multi-window state and cannot yet stage an
    // arbitrary following reliable event. Give them an interleaving-free view:
    // the raw event receives no data shell and remains unacknowledged so the
    // server can retransmit it through the full dispatcher after the helper's
    // predecessor disposition.
    let mut helper_reassembly = reassembly.clone();
    helper_reassembly.interleaved_packets.clear();
    helper_reassembly.interleaved_events.clear();
    if let Some(emit) = zlib_zero_fill::maybe_claim_server_zlib_zero_fill_window(
        state,
        &helper_reassembly,
        source_compressed_length,
        used_server_stream,
        &bytes,
    )? {
        arm_withheld_reassembly_successors(state, &reassembly)?;
        return Ok(emit);
    }
    if let Some(emit) = quickbar_stream::maybe_buffer_or_flush_server_quickbar_stream(
        state,
        &helper_reassembly,
        source_compressed_length,
        used_server_stream,
        &bytes,
    )? {
        arm_withheld_reassembly_successors(state, &reassembly)?;
        return Ok(emit);
    }
    if let Some(emit) = live_stream::maybe_buffer_or_flush_server_live_object_stream(
        state,
        &helper_reassembly,
        source_compressed_length,
        used_server_stream,
        &mut bytes,
    )? {
        arm_withheld_reassembly_successors(state, &reassembly)?;
        return Ok(emit);
    }

    server_dispatch::wrap_legacy_live_object_continuation_if_needed(&mut bytes);

    if HighLevel::parse(&bytes).is_none() {
        if used_server_stream && state.deflate.server_zlib_stream_proxy_owned {
            let emit = stream_continuation::emit_verified_server_stream_continuation(
                state,
                &helper_reassembly,
                source_compressed_length,
                &bytes,
            )?;
            arm_withheld_reassembly_successors(state, &reassembly)?;
            return Ok(emit);
        }
        dump_invalid_inflated_payload(&bytes, &reassembly, "no-high-level");
        let outputs = reassembly::build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            reassembly::remember_completed_server_stream_window(
                state,
                &reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::ConsumedEmptyMFrame,
                    packets: outputs.clone(),
                },
            );
        }
        let interleaved_packets = reassembly.interleaved_packets;
        tracing::warn!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            prefix = %hex_prefix(&bytes, 32),
            used_server_stream,
            proxy_owned_stream = state.deflate.server_zlib_stream_proxy_owned,
            "server deflated payload consumed because it has no high-level packet header"
        );
        return Ok(reassembly::emit_family_packets_with_interleaved(
            VerifiedFamily::ConsumedEmptyMFrame,
            outputs,
            interleaved_packets,
        ));
    }

    let semantic_rewrite_summary = server_dispatch::rewrite_inflated_payload_for_ee(
        &mut bytes,
        Some(&state.area_context.latest_area_placeables),
        server_dispatch::SemanticScope::DeflatedReassembly,
        Some(&state.module_resources),
        Some(&state.semantic.objects),
        None,
    );
    observe_quickbar_stream_probe_from_rewrite(state, &semantic_rewrite_summary);
    if semantic_rewrite_summary.should_quarantine() {
        let reason = semantic_rewrite_summary
            .quarantine_reason
            .unwrap_or("untranslated-required-semantic-family");
        dump_invalid_inflated_payload(&bytes, &reassembly, reason);
        let outputs = reassembly::build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            reassembly::remember_completed_server_stream_window(
                state,
                &reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::ConsumedEmptyMFrame,
                    packets: outputs.clone(),
                },
            );
        }
        let interleaved_packets = reassembly.interleaved_packets;
        tracing::warn!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            reason,
            prefix = %hex_prefix(&bytes, 32),
            "server deflated high-level payload consumed because required semantic translation is missing"
        );
        return Ok(reassembly::emit_family_packets_with_interleaved(
            VerifiedFamily::ConsumedEmptyMFrame,
            outputs,
            interleaved_packets,
        ));
    }
    if !inflated_cnw_fragment_offset_valid(&bytes) {
        dump_invalid_inflated_payload(&bytes, &reassembly, "invalid-cnw-fragment-offset");
        let outputs = reassembly::build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            reassembly::remember_completed_server_stream_window(
                state,
                &reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::ConsumedEmptyMFrame,
                    packets: outputs.clone(),
                },
            );
        }
        let interleaved_packets = reassembly.interleaved_packets;
        tracing::warn!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            prefix = %hex_prefix(&bytes, 32),
            "server deflated high-level payload consumed because CNW fragment offset is invalid"
        );
        return Ok(reassembly::emit_family_packets_with_interleaved(
            VerifiedFamily::ConsumedEmptyMFrame,
            outputs,
            interleaved_packets,
        ));
    }
    if let Some(summary) = semantic_rewrite_summary.area_rewrite.as_ref() {
        state.area_context.latest_area_placeables = summary.placeable_context.clone();
        queue_area_client_area_side_effects(state, &reassembly, summary)?;
    }
    let semantic_rewrite = semantic_rewrite_summary.any_rewrite();
    let must_convert_server_stream =
        used_server_stream && (semantic_rewrite || state.deflate.server_zlib_stream_proxy_owned);

    if !semantic_rewrite && !must_convert_server_stream {
        if let Some(module_offset) = semantic_rewrite_summary.module_info_candidate_offset {
            dump_module_info_candidate(&bytes, module_offset, &reassembly);
            tracing::debug!(
                frames = reassembly.frames.len(),
                inflated_length = old_inflated_length,
                module_offset,
                "server deflated M reassembly found Module_Info candidate with no semantic rewrite"
            );
        }
        arm_withheld_reassembly_successors(state, &reassembly)?;
        let packets = reassembly
            .frames
            .iter()
            .map(|frame| frame.packet.clone())
            .collect::<Vec<_>>();
        if used_server_stream {
            reassembly::remember_completed_server_stream_window(
                state,
                &reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::Packets(packets.clone()),
            );
        }
        tracing::debug!(
            frames = packets.len(),
            inflated_length = old_inflated_length,
            "server deflated M reassembly understood with no semantic rewrite"
        );
        return Ok(Emit::Packets(packets));
    }

    let verified_family = semantic_rewrite_summary.verified_family();
    let verified_proof = semantic_rewrite_summary.verified_proof();
    dump_accepted_live_object_rewrite_if_enabled(
        &original_inflated_for_diagnostics,
        &bytes,
        &reassembly,
        &verified_proof,
    );
    let live_object_inventory_materialization =
        observe_verified_server_payload_semantics(state, &verified_proof, &bytes);
    let response_ack_sequence = reassembly
        .frames
        .last()
        .map(|frame| frame.ack_sequence)
        .unwrap_or(0);
    let response_server_peer_ack_sequence = reassembly
        .frames
        .last()
        .map(|frame| frame.server_peer_ack_sequence)
        .unwrap_or(response_ack_sequence);
    inventory_equipment::maybe_record_client_gui_status_live_object_frame_response(
        state,
        &verified_proof,
        reassembly.first_sequence,
        response_server_peer_ack_sequence,
        response_ack_sequence,
        live_object_inventory_materialization.as_ref(),
    );
    if verified_proof.primary_family() == Some(VerifiedFamily::ModuleInfo) {
        if let (Some(first_frame), Some(last_frame)) =
            (reassembly.frames.first(), reassembly.frames.last())
        {
            deferred_module_resources::queue_after_module_info_if_ready(
                &mut state.deferred_module_resources.pending,
                &mut state.synthetic_area.pending_server_to_client_packets,
                &mut state.sequence.server_sequence_shifts,
                first_frame.sequence,
                last_frame.sequence,
                last_frame.ack_sequence,
                &state.module_resources,
            )?;
        }
    }
    if used_server_stream {
        state.deflate.server_zlib_stream_proxy_owned = true;
        let owner = verified_proof
            .primary_family()
            .map(ContinuationOwner::from_verified_family)
            .unwrap_or_else(|| ContinuationOwner::from_verified_family(verified_family));
        if state.deflate.server_zlib_stream_owner != Some(owner) {
            state.deflate.server_zlib_stream_epoch =
                state.deflate.server_zlib_stream_epoch.saturating_add(1);
        }
        state.deflate.server_zlib_stream_owner = Some(owner);
    }

    // Preserve the server-stream transport envelope after semantic rewrites.
    // EE decompiles prove raw reliable M payloads are accepted when flag 0x04
    // is clear, but that does not prove replacing one packet from an already
    // deflated server stream with raw gameplay bytes is session-equivalent.
    // Driver-only captures showed the next raw Chat_ServerTell could then
    // become the first unacked packet. The translator therefore keeps the
    // dialect rewrite semantic (`P 1E 01` quickbar, live-object updates, etc.)
    // separate from the transport contract: deflated input windows emit
    // verified deflated replacement windows unless a family has its own
    // decompile-backed transport reason to do otherwise.
    let compressed = deflate_zlib(&bytes)?;
    let mut combined = Vec::with_capacity(4 + compressed.len());
    combined.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);
    let replacement_payload_length = compressed.len();
    let mut outputs =
        reassembly::build_server_deflated_output_frames(&reassembly, &combined, 0x01, true)?;
    let inserted_extra_output_frames = outputs.len() > reassembly.frames.len();
    if inserted_extra_output_frames {
        pre_shift_current_server_packets(state, &mut outputs)?;
        record_extra_deflated_output_sequence_shift(state, &reassembly, outputs.len())?;
    }
    let response_last_sequence = reassembly
        .frames
        .last()
        .map(|frame| frame.sequence)
        .unwrap_or(reassembly.first_sequence);
    if let Err(err) = inventory_equipment::maybe_queue_confirmed_inventory_replay(
        state,
        response_last_sequence,
        response_ack_sequence,
    ) {
        tracing::warn!(
            error = %err,
            response_last_sequence,
            response_ack_sequence,
            "failed to queue confirmed deflated ClientGui status Inventory replay"
        );
    }
    if used_server_stream {
        reassembly::remember_completed_server_stream_window_with_disposition(
            state,
            &reassembly,
            source_compressed_length,
            inserted_extra_output_frames,
            CompletedDeflatedReplay::VerifiedProofPackets {
                proof: verified_proof.clone(),
                packets: outputs.clone(),
            },
        );
    }
    // The completing reliable source and its first contiguous typed successor
    // form one receive-window transaction. The pre-completion snapshot protects
    // both predecessor and successor effects; promotion below only adds exact
    // raw-slot authority. Strict rejection therefore restores the partial
    // source window while retaining the fenced raw successor for retransmission.
    resolve_buffered_interleaved_server_packets_after_success(state, &mut reassembly)?;
    let interleaved_packets = reassembly.interleaved_packets;

    server_dispatch::log_deflated_semantic_rewrite(
        &semantic_rewrite_summary,
        server_dispatch::DeflatedSemanticLogContext {
            frames: reassembly.frames.len(),
            first_sequence: reassembly.first_sequence,
            packetized_sequence: reassembly.packetized_sequence,
            old_inflated_length,
            rewritten_inflated_length: bytes.len(),
            compressed_length: replacement_payload_length,
            used_server_stream,
            proxy_owned_stream: state.deflate.server_zlib_stream_proxy_owned,
        },
    );

    if inserted_extra_output_frames {
        if !interleaved_packets.is_empty() {
            let mut mixed = outputs
                .into_iter()
                .map(|packet| (verified_proof.clone(), packet))
                .collect::<Vec<_>>();
            for mut interleaved in interleaved_packets {
                shift_server_sequence_for_client(state, &mut interleaved.packet)?;
                mixed.push((interleaved.proof, interleaved.packet));
            }
            tracing::info!(
                frames = mixed.len(),
                first_sequence = reassembly.first_sequence,
                original_frames = reassembly.frames.len(),
                output_frames = mixed.len(),
                "expanded deflated output emitted with typed pre-shifted interleaved proofs"
            );
            return Ok(Emit::MixedVerifiedProofPacketsPreShifted(mixed));
        }
        Ok(Emit::VerifiedProofPacketsPreShifted {
            proof: verified_proof,
            packets: outputs,
        })
    } else {
        Ok(reassembly::emit_proof_packets_with_interleaved(
            verified_proof,
            outputs,
            interleaved_packets,
        ))
    }
}

pub(super) fn try_emit_salvaged_incomplete_server_deflated_reassembly(
    state: &mut SessionState,
    reason: &'static str,
) -> anyhow::Result<Option<Emit>> {
    let Some(reassembly) = state.deflate.server_reassembly.as_ref() else {
        return Ok(None);
    };
    let buffered_frames = reassembly.frames.len();
    let expected_frames = reassembly.expected_frames;
    if buffered_frames == 0 || buffered_frames >= expected_frames {
        return Ok(None);
    }

    let compressed = reassembly
        .frames
        .iter()
        .flat_map(|frame| frame.compressed_chunk.iter().copied())
        .collect::<Vec<_>>();
    let stream_payload = reassembly.zlib_stream && !looks_like_zlib_wrapped_deflate(&compressed);
    if stream_payload && state.deflate.server_zlib_inflater.is_some() {
        tracing::debug!(
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            buffered_frames,
            expected_frames,
            compressed = compressed.len(),
            reason,
            "incomplete server deflated M salvage deferred: persistent zlib stream cannot be probed without mutating inflater state"
        );
        return Ok(None);
    }

    let mut probe_inflater = None;
    match reassembly::inflate_gameplay_payload(
        &compressed,
        reassembly.inflated_length,
        reassembly.zlib_stream,
        &mut probe_inflater,
    ) {
        Ok(probe) => {
            if HighLevel::parse(&probe.bytes).is_none() {
                let mut wrapped = probe.bytes.clone();
                server_dispatch::wrap_legacy_live_object_continuation_if_needed(&mut wrapped);
                if HighLevel::parse(&wrapped).is_none()
                    && !(probe.used_server_stream && state.deflate.server_zlib_stream_proxy_owned)
                {
                    tracing::debug!(
                        first_sequence = reassembly.first_sequence,
                        packetized_sequence = reassembly.packetized_sequence,
                        buffered_frames,
                        expected_frames,
                        inflated = probe.bytes.len(),
                        reason,
                        prefix = %hex_prefix(&probe.bytes, 32),
                        "incomplete server deflated M salvage deferred: inflated bytes do not yet form a semantic high-level payload"
                    );
                    return Ok(None);
                }
            }
        }
        Err(error) => {
            tracing::debug!(
                first_sequence = reassembly.first_sequence,
                packetized_sequence = reassembly.packetized_sequence,
                buffered_frames,
                expected_frames,
                compressed = compressed.len(),
                reason,
                error = %error,
                "incomplete server deflated M salvage deferred: buffered bytes do not inflate cleanly"
            );
            return Ok(None);
        }
    }

    let Some(reassembly) = state.deflate.server_reassembly.as_mut() else {
        return Ok(None);
    };
    let original_expected_frames = reassembly.expected_frames;
    reassembly.expected_frames = reassembly.frames.len();
    tracing::warn!(
        first_sequence = reassembly.first_sequence,
        packetized_sequence = reassembly.packetized_sequence,
        original_expected_frames,
        salvaged_frames = reassembly.expected_frames,
        reason,
        "server deflated M reassembly salvaged with fewer frames after exact inflate preflight"
    );
    emit_completed_server_deflated_reassembly(state).map(Some)
}

fn inflated_cnw_fragment_offset_valid(inflated: &[u8]) -> bool {
    let Some(_) = HighLevel::parse(inflated) else {
        return false;
    };
    let Some(declared) = read_le_u32(inflated, 3) else {
        return false;
    };
    let read_message_len = inflated.len().saturating_sub(3);
    if declared < 3 || read_message_len == 0 {
        return false;
    }
    // EE and Diamond both seed CNWMessage reads from the first DWORD after the
    // high-level `P major minor` header by subtracting 3 and treating the result
    // as the fragment-section offset. EE correctly rejects offsets outside the
    // supplied packet; Diamond lacked the guard but would still parse from the
    // same impossible cursor. Use this as a transport invariant when deciding
    // whether a server raw-deflate window really belongs to the persistent
    // stream or starts a fresh independent raw-deflate block.
    (declared as usize - 3) < read_message_len
}

fn log_inflated_high_level_summary(inflated: &[u8], reassembly: &ServerDeflatedReassembly) {
    let Some(first_high) = HighLevel::parse(inflated) else {
        tracing::info!(
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated_length = inflated.len(),
            prefix = %hex_prefix(inflated, 32),
            "server deflated M inflated payload has no high-level packet header"
        );
        return;
    };

    let first_declared = read_le_u32(inflated, 3).unwrap_or(0);
    let first_total_guess = first_declared as usize + 1;
    let read_message_len = inflated.len().saturating_sub(3);
    let fragment_offset = first_declared.saturating_sub(3) as usize;
    let fragment_offset_valid =
        first_declared >= 3 && read_message_len != 0 && fragment_offset < read_message_len;
    let fragment_bytes = if fragment_offset_valid {
        read_message_len - fragment_offset
    } else {
        0
    };
    let mut packet_count_guess = 0usize;
    let mut cursor = 0usize;
    while cursor + 7 <= inflated.len() && packet_count_guess < 8 {
        let Some(high) = HighLevel::parse(&inflated[cursor..]) else {
            break;
        };
        let Some(declared) = read_le_u32(inflated, cursor + 3) else {
            break;
        };
        let total = declared as usize + 1;
        if total < 8 || cursor + total > inflated.len() {
            tracing::debug!(
                first_sequence = reassembly.first_sequence,
                packet_index = packet_count_guess,
                offset = cursor,
                name = high.name(),
                major = high.major,
                minor = high.minor,
                declared,
                remaining = inflated.len() - cursor,
                "server deflated M inflated packet walk stopped on implausible CNWMessage length"
            );
            break;
        }
        packet_count_guess += 1;
        cursor += total;
    }

    tracing::info!(
        first_sequence = reassembly.first_sequence,
        packetized_sequence = reassembly.packetized_sequence,
        inflated_length = inflated.len(),
        first_name = first_high.name(),
        first_major = first_high.major,
        first_minor = first_high.minor,
        first_declared,
        first_total_guess,
        read_message_len,
        fragment_offset,
        fragment_bytes,
        fragment_offset_valid,
        packet_count_guess,
        walked_bytes = cursor,
        prefix = %hex_prefix(inflated, 32),
        "server deflated M inflated high-level summary"
    );
}

fn hex_prefix(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn dump_invalid_inflated_payload(
    inflated: &[u8],
    reassembly: &ServerDeflatedReassembly,
    reason: &str,
) {
    let Some(dir) = crate::translate::diagnostics::diagnostic_dump_dir() else {
        return;
    };

    if let Err(error) = fs::create_dir_all(&dir) {
        tracing::warn!(
            path = %dir.display(),
            %error,
            "failed to create invalid inflated payload dump directory"
        );
        return;
    }

    let high_name = HighLevel::parse(inflated)
        .map(|high| {
            high.name()
                .replace(['<', '>', '/', '\\', ':', '*', '?', '"', '|'], "_")
        })
        .unwrap_or_else(|| "no-high-level".to_string());
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!(
        "{}-{}-seq{}-frames{}-{}.bin",
        reason,
        high_name,
        reassembly.first_sequence,
        reassembly.frames.len(),
        millis
    ));

    if let Err(error) = fs::write(&path, inflated) {
        tracing::warn!(
            path = %path.display(),
            %error,
            "failed to dump invalid inflated payload"
        );
        return;
    }

    tracing::info!(
        path = %path.display(),
        inflated_length = inflated.len(),
        first_sequence = reassembly.first_sequence,
        reason,
        "dumped invalid inflated payload for offline fixture analysis"
    );
}

fn dump_accepted_live_object_rewrite_if_enabled(
    original_inflated: &[u8],
    rewritten_inflated: &[u8],
    reassembly: &ServerDeflatedReassembly,
    proof: &VerifiedProof,
) {
    if proof.primary_family() != Some(VerifiedFamily::GameObjUpdateLiveObject) {
        return;
    }
    let Ok(enabled) = std::env::var("NWN_BRIDGE_DUMP_ACCEPTED_LIVE_OBJECT") else {
        return;
    };
    if enabled.trim() != "1" {
        return;
    }
    let Some(mut dir) = crate::translate::diagnostics::probe_dump_dir() else {
        return;
    };
    dir.push("accepted-live-object");
    if let Err(error) = fs::create_dir_all(&dir) {
        tracing::warn!(
            path = %dir.display(),
            %error,
            "failed to create accepted live-object diagnostic dump directory"
        );
        return;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let stem = format!(
        "seq{}-frames{}-old{}-new{}-{}",
        reassembly.first_sequence,
        reassembly.frames.len(),
        original_inflated.len(),
        rewritten_inflated.len(),
        nanos
    );
    let original_path = dir.join(format!("{stem}-legacy.bin"));
    let rewritten_path = dir.join(format!("{stem}-ee.bin"));
    if let Err(error) = fs::write(&original_path, original_inflated) {
        tracing::warn!(
            path = %original_path.display(),
            %error,
            "failed to dump accepted live-object original inflated payload"
        );
        return;
    }
    if let Err(error) = fs::write(&rewritten_path, rewritten_inflated) {
        tracing::warn!(
            path = %rewritten_path.display(),
            %error,
            "failed to dump accepted live-object rewritten inflated payload"
        );
        return;
    }
    tracing::info!(
        original_path = %original_path.display(),
        rewritten_path = %rewritten_path.display(),
        first_sequence = reassembly.first_sequence,
        frames = reassembly.frames.len(),
        old_len = original_inflated.len(),
        new_len = rewritten_inflated.len(),
        "dumped accepted live-object rewrite for fixture analysis"
    );
}

fn dump_live_object_input_if_enabled(inflated: &[u8], reassembly: &ServerDeflatedReassembly) {
    if !matches!(
        (
            inflated.get(0).copied(),
            inflated.get(1).copied(),
            inflated.get(2).copied()
        ),
        (Some(b'P'), Some(0x05), Some(0x01))
    ) {
        return;
    }
    let Ok(enabled) = std::env::var("NWN_BRIDGE_DUMP_ACCEPTED_LIVE_OBJECT") else {
        return;
    };
    if enabled.trim() != "1" {
        return;
    }
    let Some(mut dir) = crate::translate::diagnostics::probe_dump_dir() else {
        return;
    };
    dir.push("live-object-input");
    if let Err(error) = fs::create_dir_all(&dir) {
        tracing::warn!(
            path = %dir.display(),
            %error,
            "failed to create live-object input diagnostic dump directory"
        );
        return;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!(
        "seq{}-frames{}-len{}-{}.bin",
        reassembly.first_sequence,
        reassembly.frames.len(),
        inflated.len(),
        nanos
    ));
    if let Err(error) = fs::write(&path, inflated) {
        tracing::warn!(
            path = %path.display(),
            %error,
            "failed to dump live-object input payload"
        );
        return;
    }
    tracing::info!(
        path = %path.display(),
        first_sequence = reassembly.first_sequence,
        frames = reassembly.frames.len(),
        len = inflated.len(),
        "dumped live-object input for fixture analysis"
    );
}

fn dump_module_info_candidate(
    inflated: &[u8],
    module_offset: usize,
    reassembly: &ServerDeflatedReassembly,
) {
    let Some(dir) = crate::translate::diagnostics::diagnostic_dump_dir() else {
        return;
    };

    if let Err(error) = fs::create_dir_all(&dir) {
        tracing::warn!(
            path = %dir.display(),
            %error,
            "failed to create Module_Info dump directory"
        );
        return;
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!(
        "module-info-seq{}-frames{}-offset{}-{}.bin",
        reassembly.first_sequence,
        reassembly.frames.len(),
        module_offset,
        millis
    ));

    if let Err(error) = fs::write(&path, &inflated[module_offset..]) {
        tracing::warn!(
            path = %path.display(),
            %error,
            "failed to dump Module_Info candidate"
        );
        return;
    }

    tracing::info!(
        path = %path.display(),
        module_offset,
        inflated_length = inflated.len(),
        "dumped Module_Info candidate for offline fixture analysis"
    );
}
