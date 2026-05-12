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

mod client_filters;
mod coalesced;
mod deferred_module_resources;
mod deflate;
mod live_stream;
mod live_update;
mod login_waypoint;
mod local_ack;
mod parse_window;
mod quickbar_stream;
mod reassembly;
mod sequence;
mod server_dispatch;
mod state;
mod stream_continuation;
mod synthetic_area;
mod transport_identity;

use deflate::{deflate_zlib, looks_like_zlib_wrapped_deflate};
use reassembly::{
    CompletedDeflatedReplay, InflatedGameplayPayload, ServerDeflatedReassembly,
};
use sequence::{
    SequenceShift, record_forward_progress, sequence_at_or_after, shift_sequence_for_peer,
    trim_sequence_shifts, unshift_ack_for_origin,
};

const MAX_REASSEMBLY_FRAMES: usize = 256;
const MAX_INTERLEAVED_PACKETS: usize = 32;
const CNW_LENGTH_BYTES: usize = 4;

pub use state::SessionState;

pub fn take_pending_client_to_server_packets(state: &mut SessionState) -> Vec<Vec<u8>> {
    std::mem::take(&mut state.sequence.pending_client_to_server_packets)
}

pub fn take_pending_server_to_client_packets(state: &mut SessionState) -> Emit {
    let mut packets = deferred_module_resources::take_releasable_held_server_packets(
        &mut state.deferred_module_resources.pending,
    );

    if state.synthetic_area.server_hold_gate.is_some() {
        let mut area_filtered = Vec::new();
        for (proof, packet) in packets {
            area_filtered.extend(hold_or_release_server_packets(
                state,
                proof,
                vec![packet],
                "module-resource released packet still gated by area-load ACK",
            ));
        }
        packets = area_filtered;
    } else if !state.synthetic_area.held_server_to_client_packets.is_empty() {
        packets.extend(
            state
                .synthetic_area
                .held_server_to_client_packets
                .drain(..)
                .map(|pending| {
                    tracing::info!(
                        proof = pending.proof.as_str(),
                        reason = pending.reason,
                        len = pending.packet.len(),
                        "server-to-client held packet released after area-load ACK gate opened"
                    );
                    (pending.proof, pending.packet)
                }),
        );
    }

    if packets.is_empty() {
        return Emit::Consumed;
    }

    let emit = Emit::MixedVerifiedProofPacketsPreShifted(packets);
    record_server_emit_window_state(state, &emit);
    emit
}

pub fn translate_client_to_server(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        anyhow::bail!("client M frame failed reliable-window parse");
    };
    let defer_module_loaded_until_released_packets_are_acked =
        is_client_module_loaded(view.high)
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
    let duplicate_native_area_loaded_after_synthetic =
        native_area_loaded && state.synthetic_area.in_flight_area_loaded.is_some();
    if native_area_loaded {
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
        translate_client_to_server_packet(state, outbound, &shifted_view)?
    };
    if let Some(synthetic) = synthetic_area_loaded {
        let synthetic_view = MFrameView::parse(&synthetic)
            .ok_or_else(|| anyhow::anyhow!("synthetic Area_AreaLoaded M frame failed to parse"))?;
        let synthetic = translate_client_to_server_packet(state, synthetic, &synthetic_view)?;
        return Ok(Emit::MixedVerifiedPackets(vec![
            (packet.family, packet.packet),
            (synthetic.family, synthetic.packet),
        ]));
    }

    Ok(Emit::VerifiedPackets {
        family: packet.family,
        packets: vec![packet.packet],
    })
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
    observe_verified_client_m_packet(state, translated.family, &translated.packet);
    Ok(translated)
}

fn translate_client_to_server_packet(
    state: &mut SessionState,
    bytes: Vec<u8>,
    view: &MFrameView,
) -> anyhow::Result<client_filters::ClientFrameTranslation> {
    let translated = client_filters::translate_client_frame(bytes, view, &mut state.semantic)?;
    observe_verified_client_m_packet(state, translated.family, &translated.packet);
    Ok(translated)
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
    } else if let Some(verified) =
        server_dispatch::rewrite_direct_frame_if_needed(&inbound, &view, &state.module_resources)?
    {
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
    crate::translate::semantic::observe_verified_payload(
        &mut state.semantic,
        crate::packet::Direction::ServerToClient,
        proof,
        payload,
    );
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
    let proof = VerifiedProof::family(family);
    crate::translate::semantic::observe_verified_payload(
        &mut state.semantic,
        crate::packet::Direction::ClientToServer,
        &proof,
        payload,
    );
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
    crate::translate::semantic::observe_verified_payload(
        &mut state.semantic,
        crate::packet::Direction::ServerToClientSynthetic,
        &proof,
        payload,
    );
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
    if view.sequence == 0 || state.sequence.client_sequence_shifts.is_empty() {
        return Ok(());
    }

    let shifted = shift_sequence_for_peer(&state.sequence.client_sequence_shifts, view.sequence);
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
        "client M sequence shifted for synthetic Area_AreaLoaded"
    );
    Ok(())
}

fn unshift_server_ack_for_client(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if view.ack_sequence == 0 || state.sequence.client_sequence_shifts.is_empty() {
        return Ok(());
    }

    let unshifted =
        unshift_ack_for_origin(&state.sequence.client_sequence_shifts, view.ack_sequence);
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
        "server M ack unshifted after synthetic Area_AreaLoaded"
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
        "client M ack unshifted after synthetic LoadBar frames"
    );
    Ok(())
}

fn queue_area_client_area_side_effects(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    summary: &area::AreaRewriteSummary,
) -> anyhow::Result<()> {
    let Some(last_frame) = reassembly.frames.last() else {
        return Ok(());
    };

    queue_area_client_area_side_effects_after_sequence(
        state,
        last_frame.sequence,
        last_frame.ack_sequence,
        summary,
    )
}

fn queue_area_client_area_side_effects_after_sequence(
    state: &mut SessionState,
    original_after_sequence: u16,
    ack_sequence: u16,
    summary: &area::AreaRewriteSummary,
) -> anyhow::Result<()> {
    let fallback_reason = synthetic_area::fallback_reason_for_area_rewrite(summary);
    synthetic_area::queue_loadbar_and_area_loaded_fallback(
        &mut state.synthetic_area.pending_server_to_client_packets,
        &mut state.synthetic_area.pending_area_loaded,
        &mut state.sequence.server_sequence_shifts,
        original_after_sequence,
        ack_sequence,
        fallback_reason,
    )
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
        "server M sequence shifted after synthetic LoadBar frames"
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
    let mut kept = Vec::new();
    let mut released_synthetic_loadbar_end = false;

    let pending_packets = state
        .synthetic_area
        .pending_server_to_client_packets
        .drain(..)
        .enumerate()
        .collect::<Vec<_>>();
    for (index, pending) in pending_packets
    {
        if pending.due_at > now {
            kept.push(pending);
            continue;
        }

        tracing::info!(
            reason = pending.reason,
            due_ms_ago = now.saturating_duration_since(pending.due_at).as_millis(),
            "server synthetic M packet released"
        );
        state.semantic.synthetic.server_synthetic_packets =
            state.semantic.synthetic.server_synthetic_packets.saturating_add(1);
        observe_verified_synthetic_server_m_packet(state, pending.family, &pending.packet);
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
    state.synthetic_area.pending_server_to_client_packets = kept;
    if released_synthetic_loadbar_end {
        synthetic_area::arm_server_hold_gate_after_loadbar_release(
            &mut state.synthetic_area.server_hold_gate,
            state.synthetic_area.pending_area_loaded.as_ref(),
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
        Emit::Consumed
        | Emit::ConsumedRetireSession { .. }
        | Emit::Drop => {}
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
        Emit::Consumed
        | Emit::ConsumedRetireSession { .. }
        | Emit::Drop => Ok(emit),
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
        && maybe_queue_area_loaded_fallback_from_server_tick(state)?
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
                "post-area server packet held until EE ACKs synthetic LoadBar_End",
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
                "post-area server packet held until EE ACKs synthetic LoadBar_End",
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
                    "post-area mixed server packet held until EE ACKs synthetic LoadBar_End",
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
                    "post-area mixed server packet held until EE ACKs synthetic LoadBar_End",
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

fn maybe_queue_area_loaded_fallback_from_server_tick(
    state: &mut SessionState,
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
        Emit::Packets(packets) | Emit::PacketsPreShifted(packets) => {
            for packet in packets {
                if let Some(family) = transport_only_verified_family_for_plain_server_packet(&packet)
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
                "server-to-client held packet released after area-load ACK gate opened"
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
        if area_load_gate_window_packet_can_pass(state, &packet) {
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
                "server-to-client packet released through area-load ACK gate because its sequence belongs to the gated area/loadbar window"
            );
            released.push((proof.clone(), packet));
        } else {
            hold_one_server_packet(state, proof.clone(), packet, reason);
        }
    }
    released
}

fn area_load_gate_window_packet_can_pass(state: &SessionState, packet: &[u8]) -> bool {
    let Some(gate) = state.synthetic_area.server_hold_gate.as_ref() else {
        return false;
    };
    let Some(view) = MFrameView::parse(packet) else {
        return false;
    };
    if view.sequence == 0 {
        return false;
    }

    !sequence_at_or_after(view.sequence, gate.release_client_ack_sequence.wrapping_add(1))
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
    tracing::info!(
        proof = proof.as_str(),
        release_client_ack_sequence,
        len = packet.len(),
        reason,
        held_packets = state.synthetic_area.held_server_to_client_packets.len() + 1,
        "server-to-client verified packet held behind area-load ACK gate"
    );
    state
        .synthetic_area
        .held_server_to_client_packets
        .push(synthetic_area::PendingVerifiedServerPacket {
            proof,
            packet,
            reason,
        });
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
    let base = reassembly.first_sequence.wrapping_add(original_count as u16);
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
    log_inflated_high_level_summary(&bytes, &reassembly);
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
        None,
    );
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
    crate::translate::semantic::observe_verified_payload(
        &mut state.semantic,
        crate::packet::Direction::ServerToClient,
        &verified_proof,
        &bytes,
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

    let compressed = deflate_zlib(&bytes)?;
    let mut combined = Vec::with_capacity(4 + compressed.len());
    combined.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);

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
            compressed_length: compressed.len(),
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
