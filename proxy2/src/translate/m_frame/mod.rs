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
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use flate2::Decompress;

use crate::{
    crc::{encode_legacy_m_crc, read_le_u32, write_be_u16},
    packet::m::{HighLevel, MFrameView},
    translate::{Emit, area, module_resources},
};

mod client_filters;
mod coalesced;
mod deflate;
mod live_stream;
mod live_update;
mod parse_window;
mod quickbar_stream;
mod reassembly;
mod sequence;
mod server_dispatch;
mod synthetic_area;
mod transport_identity;

use deflate::{deflate_zlib, looks_like_zlib_wrapped_deflate};
use sequence::{
    SequenceShift, shift_sequence_for_peer, unshift_ack_for_origin,
};
use reassembly::{
    CompletedDeflatedReplay, CompletedDeflatedStreamWindow, InflatedGameplayPayload,
    ServerDeflatedReassembly,
};

const MAX_REASSEMBLY_FRAMES: usize = 256;
const MAX_INTERLEAVED_PACKETS: usize = 32;
const CNW_LENGTH_BYTES: usize = 4;

#[derive(Debug, Default)]
pub struct SessionState {
    server_deflated: Option<ServerDeflatedReassembly>,
    server_zlib_inflater: Option<Decompress>,
    completed_server_stream_windows: Vec<CompletedDeflatedStreamWindow>,
    server_zlib_stream_proxy_owned: bool,
    server_quickbar_stream: Option<quickbar_stream::PendingQuickbarStream>,
    server_live_object_stream: Option<live_stream::PendingLiveObjectStream>,
    latest_client_sequence_from_client: Option<u16>,
    latest_client_ack_from_client: Option<u16>,
    client_sequence_shifts: Vec<SequenceShift>,
    server_sequence_shifts: Vec<SequenceShift>,
    pending_server_to_client_packets: Vec<synthetic_area::PendingServerPacket>,
    pending_synthetic_area_loaded: Option<synthetic_area::PendingAreaLoaded>,
    latest_area_placeables: area::AreaPlaceableContext,
    module_resources: module_resources::ModuleResourceRuntime,
}

impl SessionState {
    pub fn new(module_resources: module_resources::ModuleResourceRuntime) -> Self {
        Self {
            module_resources,
            ..Self::default()
        }
    }
}

pub fn translate_client_to_server(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        anyhow::bail!("client M frame failed reliable-window parse");
    };
    observe_client_window_state(state, &view);

    if synthetic_area::is_native_area_loaded(view.high) {
        synthetic_area::clear_pending_area_loaded(&mut state.pending_synthetic_area_loaded);
    }

    let mut outbound = bytes.to_vec();
    unshift_client_ack_for_server(state, &mut outbound, &view)?;
    let ack_adjusted_view = MFrameView::parse(&outbound).unwrap_or_else(|| view.clone());
    shift_client_sequence_for_server(state, &mut outbound, &ack_adjusted_view)?;
    let shifted_view = MFrameView::parse(&outbound).unwrap_or(ack_adjusted_view);
    let synthetic_area_loaded = synthetic_area::maybe_build_area_loaded_client_packet(
        &mut state.pending_synthetic_area_loaded,
        &mut state.latest_client_sequence_from_client,
        &mut state.client_sequence_shifts,
        view.ack_sequence,
    )?;

    let packet = translate_client_to_server_packet(outbound, &shifted_view)?;
    if let Some(synthetic) = synthetic_area_loaded {
        let synthetic_view = MFrameView::parse(&synthetic)
            .ok_or_else(|| anyhow::anyhow!("synthetic Area_AreaLoaded M frame failed to parse"))?;
        let synthetic = translate_client_to_server_packet(synthetic, &synthetic_view)?;
        return Ok(Emit::Packets(vec![packet, synthetic]));
    }

    Ok(Emit::Packet(packet))
}

fn translate_client_to_server_packet(
    bytes: Vec<u8>,
    view: &MFrameView,
) -> anyhow::Result<Vec<u8>> {
    client_filters::translate_client_frame(bytes, view)
}

pub fn translate_server_to_client(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        anyhow::bail!("server M frame failed reliable-window parse");
    };
    let pending_count_before = state.pending_server_to_client_packets.len();
    let mut inbound = bytes.to_vec();
    unshift_server_ack_for_client(state, &mut inbound, &view)?;
    let view = MFrameView::parse(&inbound).unwrap_or(view);

    if let Some(rewritten) = coalesced::rewrite_server_window_spans_if_needed(&inbound, &view, state)? {
        return finalize_server_to_client_emit(
            state,
            Emit::VerifiedPackets(vec![rewritten]),
            pending_count_before,
        );
    }

    let emit = if state.server_deflated.is_some() {
        reassembly::continue_server_deflated_reassembly(&inbound, &view, state)?
    } else if reassembly::should_start_server_deflated_reassembly(&view) {
        reassembly::start_server_deflated_reassembly(&inbound, &view, state)?
    } else if let Some(rewritten) =
        server_dispatch::rewrite_direct_frame_if_needed(&inbound, &view, &state.module_resources)?
    {
        Emit::VerifiedPackets(vec![rewritten])
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
        Emit::Packet(inbound)
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

fn observe_client_window_state(state: &mut SessionState, view: &MFrameView) {
    if view.sequence != 0 {
        state.latest_client_sequence_from_client = Some(view.sequence);
    }
    if view.ack_sequence != 0 {
        state.latest_client_ack_from_client = Some(view.ack_sequence);
    }
}

fn shift_client_sequence_for_server(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if view.sequence == 0 || state.client_sequence_shifts.is_empty() {
        return Ok(());
    }

    let shifted = shift_sequence_for_peer(&state.client_sequence_shifts, view.sequence);
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
        shifts = state.client_sequence_shifts.len(),
        "client M sequence shifted for synthetic Area_AreaLoaded"
    );
    Ok(())
}

fn unshift_server_ack_for_client(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if view.ack_sequence == 0 || state.client_sequence_shifts.is_empty() {
        return Ok(());
    }

    let unshifted = unshift_ack_for_origin(&state.client_sequence_shifts, view.ack_sequence);
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
        shifts = state.client_sequence_shifts.len(),
        "server M ack unshifted after synthetic Area_AreaLoaded"
    );
    Ok(())
}

fn unshift_client_ack_for_server(
    state: &SessionState,
    packet: &mut [u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    if view.ack_sequence == 0 || state.server_sequence_shifts.is_empty() {
        return Ok(());
    }

    let unshifted = unshift_ack_for_origin(&state.server_sequence_shifts, view.ack_sequence);
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
        shifts = state.server_sequence_shifts.len(),
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

    let fallback_reason = synthetic_area::fallback_reason_for_area_rewrite(summary);
    synthetic_area::queue_loadbar_and_area_loaded_fallback(
        &mut state.pending_server_to_client_packets,
        &mut state.pending_synthetic_area_loaded,
        &mut state.server_sequence_shifts,
        last_frame.sequence,
        last_frame.ack_sequence,
        fallback_reason,
    )
}

fn shift_server_sequence_for_client(
    state: &SessionState,
    packet: &mut [u8],
) -> anyhow::Result<()> {
    if state.server_sequence_shifts.is_empty() {
        return Ok(());
    }

    let Some(view) = MFrameView::parse(packet) else {
        return Ok(());
    };
    if view.sequence == 0 {
        return Ok(());
    }

    let shifted = shift_sequence_for_peer(&state.server_sequence_shifts, view.sequence);
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
        shifts = state.server_sequence_shifts.len(),
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

    for (index, pending) in state.pending_server_to_client_packets.drain(..).enumerate() {
        if pending.due_at > now {
            kept.push(pending);
            continue;
        }

        tracing::info!(
            reason = pending.reason,
            due_ms_ago = now.saturating_duration_since(pending.due_at).as_millis(),
            "server synthetic M packet released"
        );
        if index < pending_count_before {
            prefix.push(pending.packet);
        } else {
            suffix.push(pending.packet);
        }
    }
    state.pending_server_to_client_packets = kept;

    match emit {
        Emit::Consumed => {
            prefix.extend(suffix);
            if prefix.is_empty() {
                Ok(Emit::Consumed)
            } else {
                Ok(Emit::Packets(prefix))
            }
        }
        Emit::Drop => {
            prefix.extend(suffix);
            if prefix.is_empty() {
                Ok(Emit::Drop)
            } else {
                Ok(Emit::Packets(prefix))
            }
        }
        Emit::Packet(mut packet) => {
            shift_server_sequence_for_client(state, &mut packet)?;
            if prefix.is_empty() && suffix.is_empty() {
                Ok(Emit::Packet(packet))
            } else {
                prefix.push(packet);
                prefix.extend(suffix);
                Ok(Emit::Packets(prefix))
            }
        }
        Emit::Packets(mut packets) => {
            for packet in &mut packets {
                shift_server_sequence_for_client(state, packet)?;
            }
            prefix.extend(packets);
            prefix.extend(suffix);
            Ok(Emit::Packets(prefix))
        }
        Emit::VerifiedPackets(mut packets) => {
            for packet in &mut packets {
                shift_server_sequence_for_client(state, packet)?;
            }
            prefix.extend(packets);
            prefix.extend(suffix);
            Ok(Emit::VerifiedPackets(prefix))
        }
    }
}

fn emit_completed_server_deflated_reassembly(state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(reassembly) = state.server_deflated.take() else {
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

    let stream_payload =
        reassembly.zlib_stream && !looks_like_zlib_wrapped_deflate(&compressed);
    if stream_payload {
        if let Some(window) =
            reassembly::completed_server_stream_window(state, &reassembly, source_compressed_length)
        {
            let replay = window.replay.clone();
            let interleaved_packets = reassembly.interleaved_packets;
            return match replay {
                CompletedDeflatedReplay::Packets(mut packets) => {
                    packets.extend(interleaved_packets);
                    tracing::info!(
                        frames = packets.len(),
                        first_sequence = window.first_sequence,
                        packetized_sequence = window.packetized_sequence,
                        inflated_length = window.inflated_length,
                        compressed = source_compressed_length,
                        replay = "packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(Emit::Packets(packets))
                }
                CompletedDeflatedReplay::VerifiedPackets(mut packets) => {
                    packets.extend(interleaved_packets);
                    tracing::info!(
                        frames = packets.len(),
                        first_sequence = window.first_sequence,
                        packetized_sequence = window.packetized_sequence,
                        inflated_length = window.inflated_length,
                        compressed = source_compressed_length,
                        replay = "verified-packets",
                        "server deflated M stream duplicate replayed without advancing inflater"
                    );
                    Ok(Emit::VerifiedPackets(packets))
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
        &mut state.server_zlib_inflater,
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

    let live_object_continuation_wrapped =
        server_dispatch::wrap_legacy_live_object_continuation_if_needed(&mut bytes);

    if HighLevel::parse(&bytes).is_none() {
        dump_invalid_inflated_payload(&bytes, &reassembly, "no-high-level");
        let mut outputs = reassembly::build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            reassembly::remember_completed_server_stream_window(
                state,
                &reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
            );
        }
        outputs.extend(reassembly.interleaved_packets);
        tracing::warn!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            prefix = %hex_prefix(&bytes, 32),
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server deflated payload consumed because it has no high-level packet header"
        );
        return Ok(Emit::VerifiedPackets(outputs));
    }

    let semantic_rewrite_summary = server_dispatch::rewrite_inflated_payload_for_ee(
        &mut bytes,
        Some(&state.latest_area_placeables),
        server_dispatch::SemanticScope::DeflatedReassembly,
        live_object_continuation_wrapped.then_some("GameObjUpdate_LiveObjectContinuation"),
    );
    if semantic_rewrite_summary.should_quarantine() {
        let reason = semantic_rewrite_summary
            .quarantine_reason
            .unwrap_or("untranslated-required-semantic-family");
        dump_invalid_inflated_payload(&bytes, &reassembly, reason);
        let mut outputs = reassembly::build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            reassembly::remember_completed_server_stream_window(
                state,
                &reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
            );
        }
        outputs.extend(reassembly.interleaved_packets);
        tracing::warn!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            reason,
            prefix = %hex_prefix(&bytes, 32),
            "server deflated high-level payload consumed because required semantic translation is missing"
        );
        return Ok(Emit::VerifiedPackets(outputs));
    }
    if !inflated_cnw_fragment_offset_valid(&bytes) {
        dump_invalid_inflated_payload(&bytes, &reassembly, "invalid-cnw-fragment-offset");
        let mut outputs = reassembly::build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            reassembly::remember_completed_server_stream_window(
                state,
                &reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
            );
        }
        outputs.extend(reassembly.interleaved_packets);
        tracing::warn!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            prefix = %hex_prefix(&bytes, 32),
            "server deflated high-level payload consumed because CNW fragment offset is invalid"
        );
        return Ok(Emit::VerifiedPackets(outputs));
    }
    if let Some(summary) = semantic_rewrite_summary.area_rewrite.as_ref() {
        state.latest_area_placeables = summary.placeable_context.clone();
        queue_area_client_area_side_effects(state, &reassembly, summary)?;
    }
    let semantic_rewrite = semantic_rewrite_summary.any_rewrite();
    let must_convert_server_stream =
        used_server_stream && (semantic_rewrite || state.server_zlib_stream_proxy_owned);

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
        packets.extend(reassembly.interleaved_packets);
        tracing::debug!(
            frames = packets.len(),
            inflated_length = old_inflated_length,
            "server deflated M reassembly understood with no semantic rewrite"
        );
        return Ok(Emit::Packets(packets));
    }

    if used_server_stream {
        state.server_zlib_stream_proxy_owned = true;
    }

    let compressed = deflate_zlib(&bytes)?;
    let mut combined = Vec::with_capacity(4 + compressed.len());
    combined.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);

    let mut outputs = reassembly::build_server_deflated_output_frames(&reassembly, &combined, 0x01, true)?;
    if used_server_stream {
        reassembly::remember_completed_server_stream_window(
            state,
            &reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
        );
    }
    outputs.extend(reassembly.interleaved_packets);

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
            proxy_owned_stream: state.server_zlib_stream_proxy_owned,
        },
    );

    Ok(Emit::VerifiedPackets(outputs))
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
    let Ok(dir) = std::env::var("HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR") else {
        return;
    };

    let dir = PathBuf::from(dir);
    if let Err(error) = fs::create_dir_all(&dir) {
        tracing::warn!(
            path = %dir.display(),
            %error,
            "failed to create invalid inflated payload dump directory"
        );
        return;
    }

    let high_name = HighLevel::parse(inflated)
        .map(|high| high.name().replace(['<', '>', '/', '\\', ':', '*', '?', '"', '|'], "_"))
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
    let Ok(dir) = std::env::var("HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR") else {
        return;
    };

    let dir = PathBuf::from(dir);
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
