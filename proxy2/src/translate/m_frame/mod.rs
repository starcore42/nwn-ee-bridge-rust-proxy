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
    packet::m::{HighLevel, MFrameView},
    translate::{ContinuationOwner, Emit, VerifiedFamily, VerifiedProof, area},
};

mod client_ack;
mod client_filters;
mod coalesced;
mod deferred_module_resources;
mod deflate;
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
mod state;
mod stream_continuation;
mod synthetic_area;
mod transport_identity;
mod zlib_zero_fill;

use deflate::{deflate_zlib, looks_like_zlib_wrapped_deflate};
use reassembly::{CompletedDeflatedReplay, InflatedGameplayPayload, ServerDeflatedReassembly};
use sequence::{
    SequenceElision, SequenceShift, record_forward_progress, sequence_at_or_after,
    shift_sequence_for_peer, shift_sequence_for_peer_with_elisions, trim_sequence_elisions,
    trim_sequence_shifts, unshift_ack_for_origin, unshift_ack_for_origin_with_elisions,
};

const MAX_REASSEMBLY_FRAMES: usize = 256;
const MAX_INTERLEAVED_PACKETS: usize = 32;
const CNW_LENGTH_BYTES: usize = 4;

pub use state::SessionState;

#[cfg(test)]
pub(crate) fn rewrite_live_object_payload_to_exact_ee_for_test(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> bool {
    live_update::rewrite_payload_to_exact_ee_if_possible(payload, latest_area_placeables).is_some()
}

pub fn take_pending_client_to_server_packets(state: &mut SessionState) -> Vec<Vec<u8>> {
    if let Err(err) =
        maybe_queue_area_loaded_fallback_from_timer(state, "session pending client drain")
    {
        tracing::warn!(
            error = %err,
            "failed to evaluate synthetic Area_AreaLoaded fallback during pending client drain"
        );
    }
    std::mem::take(&mut state.sequence.pending_client_to_server_packets)
}

pub fn take_pending_server_to_client_packets(state: &mut SessionState) -> Emit {
    let mut proof_packets = deferred_module_resources::take_releasable_held_server_packets(
        &mut state.deferred_module_resources.pending,
    );
    proof_packets.extend(
        take_due_pending_server_packets(
            state,
            Instant::now(),
            "pending server-to-client proxy-owned packet released from session drain",
            true,
        )
        .into_iter()
        .map(|pending| (VerifiedProof::family(pending.family), pending.packet)),
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
        return Emit::Consumed;
    }

    let emit = Emit::MixedVerifiedProofPacketsPreShifted(proof_packets);
    record_server_emit_window_state(state, &emit);
    emit
}

pub fn translate_client_to_server(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        anyhow::bail!("client M frame failed reliable-window parse");
    };
    let defer_module_loaded_until_released_packets_are_acked = is_client_module_loaded(view.high)
        && deferred_module_resources::client_ack_would_release_held_server_packets(
            &state.deferred_module_resources.pending,
            view.ack_sequence,
        );
    observe_client_window_state(state, &view);
    synthetic_area::observe_server_hold_gate_client_ack(
        &mut state.synthetic_area.server_hold_gate,
        view.ack_sequence,
    );
    deferred_module_resources::observe_resource_hold_gate_client_ack(
        &mut state.deferred_module_resources.pending,
        view.ack_sequence,
    );

    if defer_module_loaded_until_released_packets_are_acked {
        tracing::info!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            "client Module_Loaded consumed once while releasing module-resource held server packets; waiting for EE retransmit after it ACKs those packets"
        );
        return Ok(Emit::Consumed);
    }

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

    let mut outbound = bytes.to_vec();
    unshift_client_ack_for_server(state, &mut outbound, &view)?;
    let ack_adjusted_view = MFrameView::parse(&outbound).unwrap_or_else(|| view.clone());
    shift_client_sequence_for_server(state, &mut outbound, &ack_adjusted_view)?;
    let origin_client_ack_sequence = ack_adjusted_view.ack_sequence;
    let shifted_view = MFrameView::parse(&outbound).unwrap_or(ack_adjusted_view);
    let synthetic_area_loaded = synthetic_area::maybe_build_area_loaded_client_packet(
        &mut state.synthetic_area.pending_area_loaded,
        &mut state.synthetic_area.in_flight_area_loaded,
        &mut state.synthetic_area.server_hold_gate,
        &mut state.sequence.latest_client_sequence_from_client,
        &mut state.sequence.client_sequence_shifts,
        view.ack_sequence,
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

    if let Some(packet_bytes) = packet.packet {
        Ok(Emit::VerifiedPackets {
            family: packet.family,
            packets: vec![packet_bytes],
        })
    } else {
        Ok(Emit::Consumed)
    }
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
    if translated.proxy_ack_client_sequence.is_some() && origin_client_sequence != 0 {
        queue_proxy_owned_ack_for_consumed_client_frame(state, origin_client_sequence)?;
    }
    if translated.elide_client_sequence && origin_client_sequence != 0 {
        record_client_sequence_elision(state, origin_client_sequence);
    }
    for observation in &translated.semantic_observations {
        observe_verified_client_payload(state, observation.family, &observation.payload);
    }
    let observe_packet = translated.packet.is_some()
        && !(translated.family == VerifiedFamily::ConsumedEmptyMFrame
            && !translated.semantic_observations.is_empty());
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

fn take_due_pending_server_packets(
    state: &mut SessionState,
    now: Instant,
    release_log: &'static str,
    include_client_ack: bool,
) -> Vec<synthetic_area::PendingServerPacket> {
    let mut due = Vec::new();
    let mut kept = Vec::new();
    if include_client_ack {
        due.extend(client_ack::take_due_consumed_ee_only_ack_packets(
            &mut state.client_ack.pending,
            now,
        ));
    }
    let pending_packets = state
        .synthetic_area
        .pending_server_to_client_packets
        .drain(..)
        .collect::<Vec<_>>();

    for pending in pending_packets {
        if pending.due_at > now {
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
        due.push(pending);
    }

    state.synthetic_area.pending_server_to_client_packets = kept;
    due
}

fn is_client_module_loaded(high: Option<HighLevel>) -> bool {
    matches!(high, Some(high) if high.major == 0x03 && high.minor == 0x02)
}

pub fn translate_server_to_client(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        anyhow::bail!("server M frame failed reliable-window parse");
    };
    synthetic_area::maybe_queue_area_loaded_retransmit(
        &mut state.synthetic_area.in_flight_area_loaded,
        &mut state.synthetic_area.completed_area_loaded,
        &mut state.sequence.pending_client_to_server_packets,
        view.ack_sequence,
    );
    let pending_count_before = state.synthetic_area.pending_server_to_client_packets.len();
    let mut inbound = bytes.to_vec();
    unshift_server_ack_for_client(state, &mut inbound, &view)?;
    let view = MFrameView::parse(&inbound).unwrap_or(view);
    deferred_module_resources::capture_early_server_status_if_needed(
        &inbound,
        &view,
        &state.module_resources,
        &mut state.deferred_module_resources.pending,
    );

    if let Some(rewrite) = coalesced::rewrite_server_window_spans_if_needed(&inbound, &view, state)?
    {
        return match rewrite {
            coalesced::CoalescedRewrite::Single { proof, packet } => {
                observe_verified_server_m_packet(state, &proof, &packet);
                finalize_server_to_client_emit(
                    state,
                    Emit::VerifiedProofPackets {
                        proof,
                        packets: vec![packet],
                    },
                    pending_count_before,
                )
            }
            coalesced::CoalescedRewrite::Split { packets } => {
                for (proof, packet) in &packets {
                    observe_verified_server_m_packet(state, proof, packet);
                }
                finalize_server_to_client_emit(
                    state,
                    Emit::MixedVerifiedProofPackets(packets),
                    pending_count_before,
                )
            }
            coalesced::CoalescedRewrite::SplitPreShifted { packets } => {
                for (proof, packet) in &packets {
                    observe_verified_server_m_packet(state, proof, packet);
                }
                finalize_server_to_client_emit(
                    state,
                    Emit::MixedVerifiedProofPacketsPreShifted(packets),
                    pending_count_before,
                )
            }
        };
    }

    let emit = if state.deflate.server_reassembly.is_some() {
        reassembly::continue_server_deflated_reassembly(&inbound, &view, state)?
    } else if reassembly::should_start_server_deflated_reassembly(&view) {
        reassembly::start_server_deflated_reassembly(&inbound, &view, state)?
    } else if let Some(verified) = server_dispatch::rewrite_direct_frame_if_needed(
        &inbound,
        &view,
        &state.module_resources,
        Some(&state.area_context.latest_area_placeables),
        Some(&state.semantic.objects),
    )? {
        login_waypoint::maybe_queue_empty_waypoint_response(state, &inbound, &view)?;
        observe_verified_server_m_packet(state, &verified.proof, &verified.packet);
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
                observe_verified_server_m_packet(state, &verified.proof, &verified.packet);
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
) {
    let Some(view) = MFrameView::parse(packet) else {
        return;
    };
    let Some(payload) = parse_window::primary_payload(packet, &view) else {
        return;
    };
    crate::translate::semantic::observe_verified_payload_with_area_context(
        &mut state.semantic,
        crate::packet::Direction::ServerToClient,
        proof,
        payload,
        Some(&state.area_context.latest_area_placeables),
    );
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
        "semantic state observed stream-probe GuiQuickbar summary"
    );
    update_quickbar_item_refresh_hint(state);
}

fn observe_client_window_state(state: &mut SessionState, view: &MFrameView) {
    if view.sequence != 0 {
        record_forward_progress(
            &mut state.sequence.latest_client_sequence_from_client,
            view.sequence,
        );
    }
    if view.ack_sequence != 0 {
        record_forward_progress(
            &mut state.sequence.latest_client_ack_from_client,
            view.ack_sequence,
        );
    }
}

fn shift_client_sequence_for_server(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if view.sequence == 0
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
    if view.ack_sequence == 0
        || (state.sequence.client_sequence_shifts.is_empty()
            && state.sequence.client_sequence_elisions.is_empty())
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
    if view.ack_sequence == 0 || state.sequence.server_sequence_shifts.is_empty() {
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

fn shift_server_sequence_for_client(state: &SessionState, packet: &mut [u8]) -> anyhow::Result<()> {
    if state.sequence.server_sequence_shifts.is_empty() {
        return Ok(());
    }

    let Some(view) = MFrameView::parse(packet) else {
        return Ok(());
    };
    if view.sequence == 0 {
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

fn finalize_server_to_client_emit(
    state: &mut SessionState,
    emit: Emit,
    pending_count_before: usize,
) -> anyhow::Result<Emit> {
    let now = Instant::now();
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut released_synthetic_loadbar_end = false;

    let pending_packets =
        take_due_pending_server_packets(state, now, "server synthetic M packet released", false);
    for (index, pending) in pending_packets.into_iter().enumerate() {
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
        if force_prefix || index < pending_count_before {
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

    #[test]
    fn proxy_owned_client_ack_coalesces_and_releases_from_session_drain() {
        let mut state = SessionState::default();
        state.sequence.latest_server_sequence_to_client = Some(7);
        queue_proxy_owned_ack_for_consumed_client_frame(&mut state, 40).expect("queue ACK");
        queue_proxy_owned_ack_for_consumed_client_frame(&mut state, 42).expect("coalesce ACK");

        let emit = take_pending_server_to_client_packets(&mut state);
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
            vec![empty_reliable_m_frame(9)],
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
            vec![empty_reliable_m_frame(14)],
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
            vec![empty_reliable_m_frame(16)],
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
    if view.sequence == 0 {
        return false;
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

    let observed_client_ack = state.sequence.latest_client_ack_from_client.unwrap_or(0);
    let origin_ack_sequence = if observed_client_ack != 0 {
        unshift_ack_for_origin(&state.sequence.server_sequence_shifts, observed_client_ack)
    } else {
        state.sequence.latest_server_sequence_to_client.unwrap_or(0)
    };

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

    state.sequence.pending_client_to_server_packets.push(packet);
    tracing::info!(
        observed_client_ack,
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
    let Some(gate) = state.synthetic_area.server_hold_gate.as_mut() else {
        return None;
    };
    let Some(view) = MFrameView::parse(packet) else {
        return None;
    };
    if view.sequence == 0 {
        return None;
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
    if view.sequence == 0 {
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
    let Some(reassembly) = state.deflate.server_reassembly.take() else {
        return Ok(Emit::Consumed);
    };
    if reassembly.frames.is_empty() || reassembly.frames.len() < reassembly.expected_frames {
        return Ok(Emit::Consumed);
    }

    let compressed = reassembly
        .frames
        .iter()
        .flat_map(|frame| frame.compressed_chunk.iter().copied())
        .collect::<Vec<_>>();
    let source_compressed_length = compressed.len();

    let stream_payload = reassembly.zlib_stream && !looks_like_zlib_wrapped_deflate(&compressed);
    if stream_payload {
        if let Some(window) =
            reassembly::completed_server_stream_window(state, &reassembly, source_compressed_length)
        {
            let window_first_sequence = window.first_sequence;
            let window_expected_frames = window.expected_frames;
            let window_packetized_sequence = window.packetized_sequence;
            let window_inflated_length = window.inflated_length;
            let replay = window.replay.clone();
            if let Some(emit) = quickbar_stream::force_flush_pending_server_quickbar_stream(
                state,
                &reassembly,
                source_compressed_length,
            )? {
                tracing::info!(
                    first_sequence = reassembly.first_sequence,
                    packetized_sequence = reassembly.packetized_sequence,
                    inflated_length = reassembly.inflated_length,
                    compressed = source_compressed_length,
                    "server deflated M duplicate forced pending quickbar stream disposition"
                );
                return Ok(emit);
            }
            let interleaved_packets = reassembly.interleaved_packets;
            return match replay {
                CompletedDeflatedReplay::Packets(mut packets) => {
                    packets.extend(interleaved_packets.into_iter().map(|packet| packet.packet));
                    tracing::info!(
                        frames = packets.len(),
                        first_sequence = window_first_sequence,
                        packetized_sequence = window_packetized_sequence,
                        inflated_length = window_inflated_length,
                        compressed = source_compressed_length,
                        replay = "packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(Emit::Packets(packets))
                }
                CompletedDeflatedReplay::VerifiedPackets {
                    family,
                    packets: mut packets,
                } => {
                    tracing::info!(
                        frames = packets.len() + interleaved_packets.len(),
                        first_sequence = window_first_sequence,
                        packetized_sequence = window_packetized_sequence,
                        inflated_length = window_inflated_length,
                        compressed = source_compressed_length,
                        replay = "verified-packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(reassembly::emit_family_packets_with_interleaved(
                        family,
                        packets,
                        interleaved_packets,
                    ))
                }
                CompletedDeflatedReplay::VerifiedProofPackets {
                    proof,
                    packets: mut packets,
                } => {
                    tracing::info!(
                        frames = packets.len() + interleaved_packets.len(),
                        first_sequence = window_first_sequence,
                        packetized_sequence = window_packetized_sequence,
                        inflated_length = window_inflated_length,
                        compressed = source_compressed_length,
                        replay = "verified-proof-packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(reassembly::emit_proof_packets_with_interleaved(
                        proof,
                        packets,
                        interleaved_packets,
                    ))
                }
            };
        }
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
    if let Some(emit) = zlib_zero_fill::maybe_claim_server_zlib_zero_fill_window(
        state,
        &reassembly,
        source_compressed_length,
        used_server_stream,
        &bytes,
    )? {
        return Ok(emit);
    }
    if let Some(emit) = quickbar_stream::maybe_buffer_or_flush_server_quickbar_stream(
        state,
        &reassembly,
        source_compressed_length,
        used_server_stream,
        &bytes,
    )? {
        return Ok(emit);
    }
    if let Some(emit) = live_stream::maybe_buffer_or_flush_server_live_object_stream(
        state,
        &reassembly,
        source_compressed_length,
        used_server_stream,
        &mut bytes,
    )? {
        return Ok(emit);
    }

    server_dispatch::wrap_legacy_live_object_continuation_if_needed(&mut bytes);

    if HighLevel::parse(&bytes).is_none() {
        if used_server_stream && state.deflate.server_zlib_stream_proxy_owned {
            let emit = stream_continuation::emit_verified_server_stream_continuation(
                state,
                &reassembly,
                source_compressed_length,
                &bytes,
            )?;
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
        let mut packets = reassembly
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
        packets.extend(
            reassembly
                .interleaved_packets
                .into_iter()
                .map(|packet| packet.packet),
        );
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
    crate::translate::semantic::observe_verified_payload_with_area_context(
        &mut state.semantic,
        crate::packet::Direction::ServerToClient,
        &verified_proof,
        &bytes,
        Some(&state.area_context.latest_area_placeables),
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
    if used_server_stream {
        reassembly::remember_completed_server_stream_window(
            state,
            &reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedProofPackets {
                proof: verified_proof.clone(),
                packets: outputs.clone(),
            },
        );
    }
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
