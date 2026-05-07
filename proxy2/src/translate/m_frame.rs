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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use flate2::{Decompress, FlushDecompress};

use crate::{
    crc::{encode_legacy_m_crc, read_le_u32, write_be_u16},
    packet::m::{
        HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MAX_REASONABLE_GAMEPLAY_PAYLOAD, MFrameView,
        parse_packetized_spans,
    },
    translate::{
        Emit, area, cnw_message, live_object, loadbar, module, module_resources, player_list,
        quickbar,
    },
};

mod deflate;
mod live_update;
mod sequence;

use deflate::{
    deflate_zlib, inflate_with_server_stream, inflate_with_window,
    looks_like_zlib_wrapped_deflate,
};
use sequence::{
    SequenceShift, sequence_at_or_after, shift_sequence_for_peer, trim_sequence_shifts,
    unshift_ack_for_origin,
};

const DEVICE_ADVERTISE_PROPERTY_MAJOR: u8 = 0x36;
const DEVICE_ADVERTISE_PROPERTY_MINOR: u8 = 0x01;
const AREA_MAJOR: u8 = 0x04;
const AREA_LOADED_MINOR: u8 = 0x03;
const SYNTHETIC_AREA_LOADBAR_DELAY: Duration = Duration::from_millis(6500);
const SYNTHETIC_AREA_LOADED_FALLBACK_GRACE: Duration = Duration::from_secs(2);
const SYNTHETIC_AREA_LOADBAR_FRAME_COUNT: u16 = 2;
const SYNTHETIC_AREA_LOADBAR_STALL_EVENT_ID: u32 = 2;
const MAX_REASSEMBLY_FRAMES: usize = 256;
const MAX_INTERLEAVED_PACKETS: usize = 32;
const CNW_LENGTH_BYTES: usize = 4;

#[derive(Debug, Default)]
pub struct SessionState {
    server_deflated: Option<ServerDeflatedReassembly>,
    server_zlib_inflater: Option<Decompress>,
    completed_server_stream_windows: Vec<CompletedDeflatedStreamWindow>,
    server_zlib_stream_proxy_owned: bool,
    server_quickbar_stream: Option<PendingQuickbarStream>,
    server_live_object_stream: Option<PendingLiveObjectStream>,
    latest_client_sequence_from_client: Option<u16>,
    latest_client_ack_from_client: Option<u16>,
    client_sequence_shifts: Vec<SequenceShift>,
    server_sequence_shifts: Vec<SequenceShift>,
    pending_server_to_client_packets: Vec<PendingServerPacket>,
    pending_synthetic_area_loaded: Option<PendingSyntheticAreaLoaded>,
    latest_area_placeables: area::AreaPlaceableContext,
}

#[derive(Debug, Clone)]
struct ServerDeflatedReassembly {
    inflated_length: usize,
    expected_frames: usize,
    first_sequence: u16,
    packetized_sequence: u16,
    zlib_stream: bool,
    frames: Vec<BufferedFrame>,
    interleaved_packets: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct BufferedFrame {
    packet: Vec<u8>,
    payload_length: usize,
    sequence: u16,
    ack_sequence: u16,
    compressed_chunk: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CompletedDeflatedStreamWindow {
    first_sequence: u16,
    expected_frames: usize,
    packetized_sequence: u16,
    inflated_length: usize,
    compressed_length: usize,
    replay: CompletedDeflatedReplay,
}

#[derive(Debug, Clone)]
struct PendingSyntheticAreaLoaded {
    server_ack_sequence: u16,
    release_client_ack_sequence: u16,
    release_at: Instant,
}

#[derive(Debug, Clone)]
struct PendingQuickbarStream {
    payload: Vec<u8>,
    target_length: Option<usize>,
    fragment_wait: bool,
    first_sequence: u16,
    chunks: u32,
}

#[derive(Debug, Clone)]
struct PendingLiveObjectStream {
    read_bytes: Vec<u8>,
    fragment_bytes: Vec<u8>,
    first_sequence: u16,
    chunks: u32,
}

#[derive(Debug, Clone)]
struct PendingServerPacket {
    packet: Vec<u8>,
    due_at: Instant,
    reason: &'static str,
}

#[derive(Debug, Clone)]
enum CompletedDeflatedReplay {
    /// The inflated payload was understood as already EE-safe, so duplicates can
    /// replay the same reliable-window records without touching the inflater.
    Packets(Vec<Vec<u8>>),
    /// The inflated payload was either translated or deliberately quarantined.
    /// Duplicates must preserve that exact safe disposition; raw legacy bytes
    /// must never leak through on retransmit.
    VerifiedPackets(Vec<Vec<u8>>),
}

#[derive(Debug, Clone)]
struct InflatedGameplayPayload {
    bytes: Vec<u8>,
    used_server_stream: bool,
}

pub fn translate_client_to_server(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        return Ok(Emit::Packet(bytes.to_vec()));
    };
    observe_client_window_state(state, &view);

    let native_area_loaded =
        view.high
            .map(|high| high.major == AREA_MAJOR && high.minor == AREA_LOADED_MINOR)
            .unwrap_or(false);
    if native_area_loaded {
        state.pending_synthetic_area_loaded = None;
    }

    let mut outbound = bytes.to_vec();
    unshift_client_ack_for_server(state, &mut outbound, &view)?;
    let ack_adjusted_view = MFrameView::parse(&outbound).unwrap_or_else(|| view.clone());
    shift_client_sequence_for_server(state, &mut outbound, &ack_adjusted_view)?;
    let shifted_view = MFrameView::parse(&outbound).unwrap_or(ack_adjusted_view);
    let synthetic_area_loaded =
        maybe_build_synthetic_area_loaded_client_packet(state, view.ack_sequence)?;

    let packet = translate_client_to_server_packet(outbound, &shifted_view)?;
    if let Some(synthetic) = synthetic_area_loaded {
        return Ok(Emit::Packets(vec![packet, synthetic]));
    }

    Ok(Emit::Packet(packet))
}

fn translate_client_to_server_packet(
    bytes: Vec<u8>,
    view: &MFrameView,
) -> anyhow::Result<Vec<u8>> {
    let Some(high) = view.high else {
        return Ok(bytes);
    };

    if high.major == DEVICE_ADVERTISE_PROPERTY_MAJOR
        && high.minor == DEVICE_ADVERTISE_PROPERTY_MINOR
    {
        return consume_device_advertise_property(&bytes, view);
    }

    Ok(bytes)
}

pub fn translate_server_to_client(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(view) = MFrameView::parse(bytes) else {
        return Ok(Emit::Packet(bytes.to_vec()));
    };
    let pending_count_before = state.pending_server_to_client_packets.len();
    let mut inbound = bytes.to_vec();
    unshift_server_ack_for_client(state, &mut inbound, &view)?;
    let view = MFrameView::parse(&inbound).unwrap_or(view);

    if let Some(rewritten) = rewrite_coalesced_server_window_spans_if_needed(&inbound, &view, state)? {
        return finalize_server_to_client_emit(
            state,
            Emit::VerifiedPackets(vec![rewritten]),
            pending_count_before,
        );
    }

    let emit = if state.server_deflated.is_some() {
        continue_server_deflated_reassembly(&inbound, &view, state)?
    } else if should_start_server_deflated_reassembly(&view) {
        start_server_deflated_reassembly(&inbound, &view, state)?
    } else if let Some(rewritten) = rewrite_server_status_module_resources_frame_if_needed(&inbound, &view)? {
        Emit::VerifiedPackets(vec![rewritten])
    } else if let Some(rewritten) = live_update::rewrite_direct_frame_if_needed(&inbound, &view)? {
        Emit::VerifiedPackets(vec![rewritten])
    } else {
        Emit::Packet(inbound)
    };

    finalize_server_to_client_emit(state, emit, pending_count_before)
}

fn rewrite_server_status_module_resources_frame_if_needed(
    bytes: &[u8],
    view: &MFrameView,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(high) = view.high else {
        return Ok(None);
    };
    if high.major != 0x01 || high.minor != 0x03 || view.payload_length == 0 {
        return Ok(None);
    }

    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let payload_end = payload_start + view.payload_length;
    let Some(payload) = bytes.get(payload_start..payload_end) else {
        return Ok(None);
    };
    let mut rewritten_payload = payload.to_vec();
    let Some(summary) =
        module_resources::rewrite_server_status_module_resources_payload(&mut rewritten_payload)
    else {
        return Ok(None);
    };
    if rewritten_payload.len() > u16::MAX as usize {
        anyhow::bail!("ServerStatus_ModuleRunning module resources payload too large");
    }

    let mut rewritten = bytes[..payload_start].to_vec();
    write_be_u16(&mut rewritten, 10, rewritten_payload.len() as u16)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to update ServerStatus module resources payload length"))?;
    rewritten.extend_from_slice(&rewritten_payload);
    if let Some(trailing) = bytes.get(payload_end..) {
        rewritten.extend_from_slice(trailing);
    }
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair ServerStatus module resources CRC"))?;

    tracing::info!(
        old_declared = summary.old_declared,
        new_declared = summary.new_declared,
        old_payload_length = summary.old_payload_length,
        new_payload_length = summary.new_payload_length,
        status_module_name = %summary.status_module_name,
        hak_count = summary.hak_count,
        nwsync_advertised = summary.nwsync_advertised,
        "server ServerStatus_ModuleRunning module resources rewritten for EE"
    );
    Ok(Some(rewritten))
}

fn rewrite_coalesced_server_window_spans_if_needed(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
) -> anyhow::Result<Option<Vec<u8>>> {
    if view.trailing_payload_length == 0 {
        return Ok(None);
    }

    let primary_len = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + view.payload_length;
    let Some(spans) = parse_packetized_spans(bytes, primary_len) else {
        return Ok(None);
    };
    if spans.is_empty() {
        return Ok(None);
    }

    let mut rewritten = bytes[..primary_len].to_vec();
    let mut changed = false;
    let mut dropped_spans = 0u32;
    let mut rewritten_deflated_spans = 0u32;

    for span in spans {
        let record_end = span.offset + span.record_length;
        let record = &bytes[span.offset..record_end];
        if let Some(high) = span.high {
            if high.is_known() {
                rewritten.extend_from_slice(record);
            } else {
                changed = true;
                dropped_spans = dropped_spans.saturating_add(1);
                tracing::warn!(
                    offset = span.offset,
                    payload_length = span.payload_length,
                    major = high.major,
                    minor = high.minor,
                    name = high.name(),
                    prefix = %hex_prefix(record, 32),
                    "server coalesced M window span quarantined: unknown high-level payload"
                );
            }
            continue;
        }

        let Some(deflated) = span.deflated.as_ref() else {
            changed = true;
            dropped_spans = dropped_spans.saturating_add(1);
            tracing::warn!(
                offset = span.offset,
                payload_length = span.payload_length,
                prefix = %hex_prefix(record, 32),
                "server coalesced M window span quarantined: unknown non-deflated payload"
            );
            continue;
        };

        if !deflated.plausible || span.payload_length < CNW_LENGTH_BYTES {
            changed = true;
            dropped_spans = dropped_spans.saturating_add(1);
            tracing::warn!(
                offset = span.offset,
                payload_length = span.payload_length,
                inflated_length = deflated.inflated_length,
                prefix = %hex_prefix(record, 32),
                "server coalesced M deflated span quarantined: implausible envelope"
            );
            continue;
        }

        let payload_offset = span.offset + LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
        let compressed_offset = payload_offset + CNW_LENGTH_BYTES;
        let compressed_end = payload_offset + span.payload_length;
        let compressed = &bytes[compressed_offset..compressed_end];
        let InflatedGameplayPayload {
            bytes: mut inflated,
            used_server_stream,
        } = inflate_gameplay_payload(
            compressed,
            deflated.inflated_length,
            (span.flags & 0x01) != 0,
            &mut state.server_zlib_inflater,
        )?;

        let live_object_continuation_wrap = if HighLevel::parse(&inflated).is_none() {
            live_object::wrap_legacy_live_object_continuation_payload_if_plausible(&mut inflated)
        } else {
            None
        };
        if HighLevel::parse(&inflated).is_none() {
            changed = true;
            dropped_spans = dropped_spans.saturating_add(1);
            dump_invalid_inflated_payload_for_span(&inflated, view.sequence, "coalesced-no-high-level");
            tracing::warn!(
                offset = span.offset,
                inflated = inflated.len(),
                prefix = %hex_prefix(&inflated, 32),
                used_server_stream,
                "server coalesced M deflated span quarantined: no high-level payload"
            );
            continue;
        }

        let quickbar_normalize = quickbar::normalize_quickbar_payload_if_needed(&mut inflated);
        let quickbar_rewrite = quickbar::rewrite_simple_quickbar_payload_if_possible(&mut inflated);
        let prefixed_fragments_normalize =
            cnw_message::normalize_prefixed_fragments_payload_if_needed(&mut inflated);
        let player_list_rewrite = player_list::rewrite_player_list_payload_if_possible(&mut inflated);
        let live_object_normalize =
            live_object::normalize_prefixed_fragments_payload_if_needed(&mut inflated);
        let live_object_visual_transform =
            live_object::rewrite_creature_add_visual_transform_maps_if_possible(
                &mut inflated,
                Some(&state.latest_area_placeables),
            );
        let live_object_update_rewrite = live_update::rewrite_payload_if_needed(&mut inflated);
        if !inflated_cnw_fragment_offset_valid(&inflated) {
            changed = true;
            dropped_spans = dropped_spans.saturating_add(1);
            dump_invalid_inflated_payload_for_span(&inflated, view.sequence, "coalesced-invalid-cnw-fragment-offset");
            tracing::warn!(
                offset = span.offset,
                inflated = inflated.len(),
                prefix = %hex_prefix(&inflated, 32),
                "server coalesced M deflated span quarantined: invalid CNW fragment offset"
            );
            continue;
        }

        let semantic_rewrite = live_object_continuation_wrap.is_some()
            || quickbar_normalize.is_some()
            || quickbar_rewrite.is_some()
            || prefixed_fragments_normalize.is_some()
            || player_list_rewrite.is_some()
            || live_object_normalize.is_some()
            || live_object_visual_transform.is_some()
            || live_object_update_rewrite.is_some();
        let must_convert_stream = used_server_stream && (semantic_rewrite || state.server_zlib_stream_proxy_owned);
        if !semantic_rewrite && !must_convert_stream {
            rewritten.extend_from_slice(record);
            continue;
        }

        if used_server_stream {
            state.server_zlib_stream_proxy_owned = true;
        }

        let compressed = deflate_zlib(&inflated)?;
        let new_payload_length = CNW_LENGTH_BYTES + compressed.len();
        if new_payload_length > u16::MAX as usize {
            changed = true;
            dropped_spans = dropped_spans.saturating_add(1);
            tracing::warn!(
                offset = span.offset,
                new_payload_length,
                "server coalesced M deflated span quarantined: rewritten payload too large"
            );
            continue;
        }

        let mut out_record = record[..LEGACY_GAMEPLAY_PAYLOAD_OFFSET].to_vec();
        if !out_record.is_empty() {
            out_record[7] &= !0x01;
        }
        write_be_u16(&mut out_record, 10, new_payload_length as u16)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update coalesced span payload length"))?;
        out_record.extend_from_slice(&(inflated.len() as u32).to_le_bytes());
        out_record.extend_from_slice(&compressed);
        rewritten.extend_from_slice(&out_record);
        changed = true;
        rewritten_deflated_spans = rewritten_deflated_spans.saturating_add(1);

        if let Some(summary) = live_object_continuation_wrap.as_ref() {
            tracing::info!(
                offset = span.offset,
                old_payload_length = summary.old_payload_length,
                new_payload_length = summary.new_payload_length,
                dropped_leadin_bytes = summary.dropped_leadin_bytes,
                read_bytes_length = summary.read_bytes_length,
                fragment_bytes_length = summary.fragment_bytes_length,
                new_declared = summary.new_declared,
                used_server_stream,
                "server coalesced GameObjUpdate_LiveObject continuation wrapped for EE"
            );
        }
        if let Some(summary) = live_object_update_rewrite.as_ref() {
            tracing::info!(
                offset = span.offset,
                old_declared = summary.old_declared,
                new_declared = summary.new_declared,
                old_payload_length = summary.old_payload_length,
                new_payload_length = summary.new_payload_length,
                records_examined = summary.records_examined,
                update_records_examined = summary.update_records_examined,
                update_records_rewritten = summary.update_records_rewritten,
                masks_translated = summary.masks_translated,
                bytes_inserted = summary.bytes_inserted,
                bytes_removed = summary.bytes_removed,
                bits_inserted = summary.bits_inserted,
                bits_removed = summary.bits_removed,
                used_server_stream,
                "server coalesced GameObjUpdate_LiveObject update records rewritten for EE"
            );
        }
    }

    if !changed {
        return Ok(None);
    }

    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair coalesced M CRC"))?;
    tracing::info!(
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        rewritten_deflated_spans,
        dropped_spans,
        "server coalesced M window spans rewritten for strict EE delivery"
    );
    Ok(Some(rewritten))
}

fn consume_device_advertise_property(bytes: &[u8], view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume extended-length Device_AdvertiseProperty frame yet");
    }

    let mut rewritten = bytes.to_vec();
    rewritten.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    write_be_u16(&mut rewritten, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to rewrite M packetized length"))?;
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair M CRC"))?;

    tracing::info!(
        old_len = bytes.len(),
        new_len = rewritten.len(),
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        flags = view.flags,
        packetized_sequence = view.packetized_sequence,
        "client Device_AdvertiseProperty consumed as empty reliable M payload"
    );

    Ok(rewritten)
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

fn arm_synthetic_area_loaded_fallback(
    state: &mut SessionState,
    server_ack_sequence: u16,
    release_client_ack_sequence: u16,
    release_at: Instant,
) {
    state.pending_synthetic_area_loaded = Some(PendingSyntheticAreaLoaded {
        server_ack_sequence,
        release_client_ack_sequence,
        release_at,
    });
    tracing::info!(
        server_ack_sequence,
        release_client_ack_sequence,
        delay_ms = release_at.saturating_duration_since(Instant::now()).as_millis(),
        "client synthetic Area_AreaLoaded fallback armed after synthetic LoadBar completion"
    );
}

fn queue_area_client_area_side_effects(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
) -> anyhow::Result<()> {
    let Some(last_frame) = reassembly.frames.last() else {
        return Ok(());
    };

    let original_after_sequence = last_frame.sequence;
    let shifted_after_sequence =
        shift_sequence_for_peer(&state.server_sequence_shifts, original_after_sequence);
    let start_sequence = shifted_after_sequence.wrapping_add(1);
    let end_sequence = shifted_after_sequence.wrapping_add(2);
    let ack_sequence = last_frame.ack_sequence;

    let start_payload = loadbar::start_payload(SYNTHETIC_AREA_LOADBAR_STALL_EVENT_ID);
    let end_payload = loadbar::end_success_payload(SYNTHETIC_AREA_LOADBAR_STALL_EVENT_ID);
    let start_packet = build_synthetic_gameplay_frame(start_sequence, ack_sequence, &start_payload)?;
    let end_packet = build_synthetic_gameplay_frame(end_sequence, ack_sequence, &end_payload)?;

    let now = Instant::now();
    let end_due_at = now + SYNTHETIC_AREA_LOADBAR_DELAY;
    state.server_sequence_shifts.push(SequenceShift {
        base: original_after_sequence.wrapping_add(1),
        delta: SYNTHETIC_AREA_LOADBAR_FRAME_COUNT,
    });
    trim_sequence_shifts(&mut state.server_sequence_shifts);
    state.pending_server_to_client_packets.push(PendingServerPacket {
        packet: start_packet,
        due_at: now,
        reason: "Area_ClientArea synthetic LoadBar_Start",
    });
    state.pending_server_to_client_packets.push(PendingServerPacket {
        packet: end_packet,
        due_at: end_due_at,
        reason: "Area_ClientArea synthetic LoadBar_End",
    });
    arm_synthetic_area_loaded_fallback(
        state,
        original_after_sequence,
        end_sequence,
        end_due_at + SYNTHETIC_AREA_LOADED_FALLBACK_GRACE,
    );

    tracing::info!(
        original_after_sequence,
        shifted_after_sequence,
        start_sequence,
        end_sequence,
        ack_sequence,
        shift_base = original_after_sequence.wrapping_add(1),
        shift_delta = SYNTHETIC_AREA_LOADBAR_FRAME_COUNT,
        end_delay_ms = SYNTHETIC_AREA_LOADBAR_DELAY.as_millis(),
        fallback_grace_ms = SYNTHETIC_AREA_LOADED_FALLBACK_GRACE.as_millis(),
        pending_server_packets = state.pending_server_to_client_packets.len(),
        shifts = state.server_sequence_shifts.len(),
        "server Area_ClientArea synthetic LoadBar frames queued"
    );

    Ok(())
}

fn maybe_build_synthetic_area_loaded_client_packet(
    state: &mut SessionState,
    observed_client_ack: u16,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(pending) = state.pending_synthetic_area_loaded.clone() else {
        return Ok(None);
    };
    if observed_client_ack == 0
        || !sequence_at_or_after(observed_client_ack, pending.release_client_ack_sequence)
        || Instant::now() < pending.release_at
    {
        return Ok(None);
    }

    let Some(latest_client_sequence) = state.latest_client_sequence_from_client else {
        tracing::warn!(
            observed_client_ack,
            release_client_ack_sequence = pending.release_client_ack_sequence,
            "client synthetic Area_AreaLoaded fallback cannot release without a client sequence"
        );
        return Ok(None);
    };

    let original_sequence = latest_client_sequence.wrapping_add(1);
    let shifted_sequence = shift_sequence_for_peer(&state.client_sequence_shifts, original_sequence);
    let payload = [0x70, AREA_MAJOR, AREA_LOADED_MINOR];
    let packet = build_synthetic_gameplay_frame(shifted_sequence, pending.server_ack_sequence, &payload)?;

    state
        .client_sequence_shifts
        .push(SequenceShift { base: original_sequence, delta: 1 });
    trim_sequence_shifts(&mut state.client_sequence_shifts);
    state.latest_client_sequence_from_client = Some(original_sequence);
    state.pending_synthetic_area_loaded = None;

    tracing::info!(
        original_sequence,
        shifted_sequence,
        observed_client_ack,
        ack_sequence = pending.server_ack_sequence,
        release_client_ack_sequence = pending.release_client_ack_sequence,
        shifts = state.client_sequence_shifts.len(),
        "client synthetic Area_AreaLoaded released"
    );

    Ok(Some(packet))
}

fn build_synthetic_gameplay_frame(
    sequence: u16,
    ack_sequence: u16,
    payload: &[u8],
) -> anyhow::Result<Vec<u8>> {
    if payload.len() > u16::MAX as usize {
        anyhow::bail!("synthetic gameplay payload is too large: {}", payload.len());
    }

    let mut packet = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
    packet[0] = b'M';
    write_be_u16(&mut packet, 3, sequence)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic M sequence"))?;
    write_be_u16(&mut packet, 5, ack_sequence)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic M ack"))?;
    packet[7] = 0x0A;
    write_be_u16(&mut packet, 8, 1)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic packetized sequence"))?;
    write_be_u16(&mut packet, 10, payload.len() as u16)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic packetized length"))?;
    packet.extend_from_slice(payload);
    encode_legacy_m_crc(&mut packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair synthetic M CRC"))?;
    Ok(packet)
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

fn should_start_server_deflated_reassembly(view: &MFrameView) -> bool {
    view.deflated
        .as_ref()
        .map(|deflated| deflated.plausible && view.payload_length >= 4)
        .unwrap_or(false)
}

fn start_server_deflated_reassembly(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
) -> anyhow::Result<Emit> {
    let deflated = view
        .deflated
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing deflated envelope"))?;
    let expected_frames = if view.packetized_sequence > 1 {
        usize::from(view.packetized_sequence)
    } else {
        1
    };
    if expected_frames == 0 || expected_frames > MAX_REASSEMBLY_FRAMES {
        return Ok(Emit::Packet(bytes.to_vec()));
    }

    let frame = buffered_frame_from_view(bytes, view, true)?;
    let mut reassembly = ServerDeflatedReassembly {
        inflated_length: deflated.inflated_length,
        expected_frames,
        first_sequence: view.sequence,
        packetized_sequence: view.packetized_sequence,
        zlib_stream: (view.flags & 0x01) != 0,
        frames: Vec::with_capacity(expected_frames),
        interleaved_packets: Vec::new(),
    };
    reassembly.frames.push(frame);
    state.server_deflated = Some(reassembly);

    tracing::info!(
        inflated_length = deflated.inflated_length,
        expected_frames,
        sequence = view.sequence,
        packetized_sequence = view.packetized_sequence,
        zlib_stream = (view.flags & 0x01) != 0,
        "server deflated M reassembly started"
    );

    if expected_frames == 1 {
        emit_completed_server_deflated_reassembly(state)
    } else {
        Ok(Emit::Drop)
    }
}

fn continue_server_deflated_reassembly(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
) -> anyhow::Result<Emit> {
    let Some(reassembly) = state.server_deflated.as_mut() else {
        return Ok(Emit::Packet(bytes.to_vec()));
    };

    let distance = view.sequence.wrapping_sub(reassembly.first_sequence) as usize;
    if distance >= reassembly.expected_frames {
        if reassembly.interleaved_packets.len() >= MAX_INTERLEAVED_PACKETS {
            tracing::warn!(
                sequence = view.sequence,
                first_sequence = reassembly.first_sequence,
                expected_frames = reassembly.expected_frames,
                "server deflated M reassembly abandoned after too many interleaved packets"
            );
            state.server_deflated = None;
            return Ok(Emit::Drop);
        }
        reassembly.interleaved_packets.push(bytes.to_vec());
        return Ok(Emit::Drop);
    }

    if reassembly
        .frames
        .iter()
        .any(|frame| frame.sequence == view.sequence)
    {
        tracing::warn!(
            sequence = view.sequence,
            first_sequence = reassembly.first_sequence,
            "duplicate server deflated M frame dropped"
        );
        return Ok(Emit::Drop);
    }

    let frame = buffered_frame_from_view(bytes, view, false)?;
    let insert_index = reassembly
        .frames
        .iter()
        .position(|existing| {
            existing.sequence.wrapping_sub(reassembly.first_sequence) > distance as u16
        })
        .unwrap_or(reassembly.frames.len());
    reassembly.frames.insert(insert_index, frame);

    if reassembly.frames.len() < reassembly.expected_frames {
        return Ok(Emit::Drop);
    }

    emit_completed_server_deflated_reassembly(state)
}

fn emit_completed_server_deflated_reassembly(state: &mut SessionState) -> anyhow::Result<Emit> {
    let Some(reassembly) = state.server_deflated.take() else {
        return Ok(Emit::Drop);
    };
    if reassembly.frames.is_empty() || reassembly.frames.len() < reassembly.expected_frames {
        return Ok(Emit::Drop);
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
            completed_server_stream_window(state, &reassembly, source_compressed_length)
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
    } = inflate_gameplay_payload(
        &compressed,
        reassembly.inflated_length,
        reassembly.zlib_stream,
        &mut state.server_zlib_inflater,
    )?;

    let old_inflated_length = bytes.len();
    log_inflated_high_level_summary(&bytes, &reassembly);
    if let Some(emit) = maybe_buffer_or_flush_server_quickbar_stream(
        state,
        &reassembly,
        source_compressed_length,
        used_server_stream,
        &bytes,
    )? {
        return Ok(emit);
    }
    if let Some(emit) = maybe_buffer_or_flush_server_live_object_stream(
        state,
        &reassembly,
        source_compressed_length,
        used_server_stream,
        &mut bytes,
    )? {
        return Ok(emit);
    }

    let live_object_continuation_wrap = if HighLevel::parse(&bytes).is_none() {
        live_object::wrap_legacy_live_object_continuation_payload_if_plausible(&mut bytes)
    } else {
        None
    };

    if HighLevel::parse(&bytes).is_none() {
        dump_invalid_inflated_payload(&bytes, &reassembly, "no-high-level");
        let mut outputs = build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            remember_completed_server_stream_window(
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

    let quickbar_normalize = quickbar::normalize_quickbar_payload_if_needed(&mut bytes);
    let quickbar_rewrite = quickbar::rewrite_simple_quickbar_payload_if_possible(&mut bytes);
    let prefixed_fragments_normalize =
        cnw_message::normalize_prefixed_fragments_payload_if_needed(&mut bytes);
    let player_list_rewrite = player_list::rewrite_player_list_payload_if_possible(&mut bytes);
    let live_object_normalize =
        live_object::normalize_prefixed_fragments_payload_if_needed(&mut bytes);
    let live_object_visual_transform =
        live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut bytes,
            Some(&state.latest_area_placeables),
        );
    let live_object_update_rewrite = live_update::rewrite_payload_if_needed(&mut bytes);
    if !inflated_cnw_fragment_offset_valid(&bytes) {
        dump_invalid_inflated_payload(&bytes, &reassembly, "invalid-cnw-fragment-offset");
        let mut outputs = build_consumed_server_deflated_frames(&reassembly)?;
        if used_server_stream {
            remember_completed_server_stream_window(
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
    let area_rewrite = area::rewrite_area_client_area_payload(&mut bytes);
    let module_rewrite = module::rewrite_module_info_payload(&mut bytes);
    if area_rewrite.is_some() {
        if let Some(summary) = area_rewrite.as_ref() {
            state.latest_area_placeables = summary.placeable_context.clone();
        }
        queue_area_client_area_side_effects(state, &reassembly)?;
    }
    let semantic_rewrite = player_list_rewrite.is_some()
        || live_object_continuation_wrap.is_some()
        || quickbar_normalize.is_some()
        || quickbar_rewrite.is_some()
        || prefixed_fragments_normalize.is_some()
        || live_object_normalize.is_some()
        || live_object_visual_transform.is_some()
        || live_object_update_rewrite.is_some()
        || area_rewrite.is_some()
        || module_rewrite.is_some();
    let must_convert_server_stream =
        used_server_stream && (semantic_rewrite || state.server_zlib_stream_proxy_owned);

    if !semantic_rewrite && !must_convert_server_stream {
        if let Some(module_offset) = module::first_module_info_candidate_offset(&bytes) {
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
            remember_completed_server_stream_window(
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

    let mut outputs = build_server_deflated_output_frames(&reassembly, &combined, 0x01, true)?;
    if used_server_stream {
        remember_completed_server_stream_window(
            state,
            &reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
        );
    }
    outputs.extend(reassembly.interleaved_packets);

    let mut logged_specific_rewrite = false;
    if let Some(summary) = player_list_rewrite.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            minor = summary.minor,
            entries = summary.entries,
            insertions = summary.insertions,
            bytes_inserted = summary.bytes_inserted,
            old_declared = summary.old_declared,
            new_declared = summary.new_declared,
            old_fragment_bytes = summary.old_fragment_bytes,
            new_fragment_bytes = summary.new_fragment_bytes,
            consumed_fragment_bits = summary.consumed_fragment_bits,
            fragments_rewritten = summary.fragments_rewritten,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            normalized_prefixed_short_declared = summary.normalized_prefixed_short_declared,
            normalized_short_declared = summary.normalized_short_declared,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server PlayerList deflated stream rewritten for EE"
        );
    }

    if let Some(summary) = live_object_normalize.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            old_wire_declared = format_args!("0x{:08X}", summary.old_wire_declared),
            new_declared = summary.new_declared,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            prefixed_fragment_bytes = %hex_prefix(&summary.prefixed_fragment_bytes, 4),
            live_bytes_offset = summary.live_bytes_offset,
            live_bytes_length = summary.live_bytes_length,
            dropped_leadin_bytes = summary.dropped_leadin_bytes,
            salvaged_partial_leadin = summary.salvaged_partial_leadin,
            first_record_end = summary.first_record_end,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server GameObjUpdate_LiveObject prefixed fragments normalized for EE"
        );
    }

    if let Some(summary) = live_object_continuation_wrap.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            dropped_leadin_bytes = summary.dropped_leadin_bytes,
            read_bytes_length = summary.read_bytes_length,
            fragment_bytes_length = summary.fragment_bytes_length,
            new_declared = summary.new_declared,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server GameObjUpdate_LiveObject continuation wrapped for EE"
        );
    }

    if let Some(summary) = live_object_visual_transform.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            old_declared = summary.old_declared,
            new_declared = summary.new_declared,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            old_live_bytes_length = summary.old_live_bytes_length,
            new_live_bytes_length = summary.new_live_bytes_length,
            records_examined = summary.records_examined,
            maps_inserted = summary.maps_inserted,
            bytes_inserted = summary.bytes_inserted,
            area_placeable_adds_suppressed = summary.area_placeable_adds_suppressed,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server GameObjUpdate_LiveObject add-record translation applied for EE"
        );
    }

    if let Some(summary) = live_object_update_rewrite.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            old_declared = summary.old_declared,
            new_declared = summary.new_declared,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            old_live_bytes_length = summary.old_live_bytes_length,
            new_live_bytes_length = summary.new_live_bytes_length,
            old_fragment_bytes = summary.old_fragment_bytes,
            new_fragment_bytes = summary.new_fragment_bytes,
            records_examined = summary.records_examined,
            update_records_examined = summary.update_records_examined,
            update_records_rewritten = summary.update_records_rewritten,
            masks_translated = summary.masks_translated,
            bytes_inserted = summary.bytes_inserted,
            bytes_removed = summary.bytes_removed,
            bits_inserted = summary.bits_inserted,
            bits_removed = summary.bits_removed,
            world_status_records_normalized = summary.world_status_records_normalized,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server GameObjUpdate_LiveObject update records rewritten for EE"
        );
    }

    if let Some(summary) = quickbar_normalize.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            major = summary.major,
            minor = summary.minor,
            old_wire_declared = format_args!("0x{:08X}", summary.old_wire_declared),
            new_declared = summary.new_declared,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            prefixed_fragment_bytes = %hex_prefix(&summary.prefixed_fragment_bytes, 4),
            read_bytes_offset = summary.read_bytes_offset,
            read_bytes_length = summary.read_bytes_length,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server GuiQuickbar prefixed fragments normalized for EE"
        );
    }

    if let Some(summary) = quickbar_rewrite.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            old_declared = summary.old_declared,
            new_declared = summary.new_declared,
            read_size = summary.read_size,
            fragment_size = summary.fragment_size,
            final_cursor = summary.final_cursor,
            trailing_read_bytes = summary.trailing_read_bytes,
            spells_preserved = summary.spells_preserved,
            general_buttons_preserved = summary.general_buttons_preserved,
            item_buttons_blanked = summary.item_buttons_blanked,
            unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
            direct_opcode_stream = summary.direct_opcode_stream,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server GuiQuickbar semantic payload rewritten for EE"
        );
    }

    if let Some(summary) = prefixed_fragments_normalize.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            major = summary.major,
            minor = summary.minor,
            old_wire_declared = format_args!("0x{:08X}", summary.old_wire_declared),
            new_declared = summary.new_declared,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            prefixed_fragment_bytes = %hex_prefix(&summary.prefixed_fragment_bytes, 4),
            read_bytes_offset = summary.read_bytes_offset,
            read_bytes_length = summary.read_bytes_length,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server CNWMessage prefixed fragments normalized for EE"
        );
    }

    if let Some(summary) = area_rewrite.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            old_declared = summary.old_declared,
            new_declared = summary.new_declared,
            old_read_size = summary.old_read_size,
            new_read_size = summary.new_read_size,
            old_fragment_offset = summary.old_fragment_offset,
            new_fragment_offset = summary.new_fragment_offset,
            fragment_size = summary.fragment_size,
            legacy_area_object_id = format_args!("0x{:08X}", summary.legacy_area_object_id),
            area_resref = %summary.area_resref,
            old_fragment_byte = format_args!("0x{:02X}", summary.old_fragment_byte),
            new_fragment_byte = format_args!("0x{:02X}", summary.new_fragment_byte),
            area_name_length = summary.area_name_length,
            area_name_end_read_offset = summary.area_name_end_read_offset,
            width_read_offset = summary.width_read_offset,
            height_read_offset = summary.height_read_offset,
            tileset_read_offset = summary.tileset_read_offset,
            first_tile_read_offset = summary.first_tile_read_offset,
            width = summary.width,
            packet_height = summary.packet_height,
            inferred_height = summary.inferred_height,
            tile_count = summary.tile_count,
            tile_scan_valid = summary.tile_scan_valid,
            height_repaired = summary.height_repaired,
            placeable_context_valid = summary.placeable_context_valid,
            placeable_light_count = summary.placeable_light_count,
            placeable_static_count = summary.placeable_static_count,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server Area_ClientArea deflated stream rewritten for EE"
        );
    }

    if let Some(summary) = module_rewrite.as_ref() {
        logged_specific_rewrite = true;
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            module_offset = summary.offset,
            inflated = old_inflated_length,
            rewritten_inflated = bytes.len(),
            compressed = compressed.len(),
            hak_count = summary.hak_count,
            removed_hak_bytes = summary.removed_hak_bytes,
            legacy_tail_removed = summary.legacy_tail_removed,
            old_declared = summary.old_declared,
            new_declared = summary.new_declared,
            resource_count = summary.resource_count,
            resource_name_count = summary.resource_name_count,
            zero_length_name_repairs = summary.zero_length_name_repairs,
            zero_length_name_terminator = summary.zero_length_name_terminator,
            used_server_stream,
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server Module_Info deflated stream rewritten for EE"
        );
    }

    if !logged_specific_rewrite {
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = old_inflated_length,
            compressed = compressed.len(),
            proxy_owned_stream = state.server_zlib_stream_proxy_owned,
            "server deflated M stream converted to EE one-shot zlib"
        );
    }

    Ok(Emit::VerifiedPackets(outputs))
}

fn maybe_buffer_or_flush_server_live_object_stream(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    used_server_stream: bool,
    bytes: &mut Vec<u8>,
) -> anyhow::Result<Option<Emit>> {
    if std::env::var_os("HGBRIDGE_PROXY2_ENABLE_LIVE_STREAM_BUFFER").is_none() {
        return Ok(None);
    }

    if !used_server_stream || !state.server_zlib_stream_proxy_owned {
        return Ok(None);
    }

    if starts_with_live_object_high_level(bytes) {
        if looks_like_clean_legacy_live_object_fragment(bytes) {
            append_pending_live_object_clean_fragment(state, reassembly.first_sequence, bytes);
            let mut outputs = build_consumed_server_deflated_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
            );
            outputs.extend(reassembly.interleaved_packets.clone());
            if let Some(pending) = state.server_live_object_stream.as_ref() {
                tracing::info!(
                    first_sequence = pending.first_sequence,
                    current_sequence = reassembly.first_sequence,
                    chunks = pending.chunks,
                    read_bytes = pending.read_bytes.len(),
                    fragment_bytes = pending.fragment_bytes.len(),
                    "server live-object stream fragment buffered pending continuation"
                );
            }
            return Ok(Some(Emit::VerifiedPackets(outputs)));
        }

        if state.server_live_object_stream.is_some() {
            append_pending_live_object_continuation(state, reassembly.first_sequence, bytes);
            if let Some(flushed) = take_pending_live_object_stream_payload(state) {
                tracing::info!(
                    current_sequence = reassembly.first_sequence,
                    old_inflated = bytes.len(),
                    rebuilt_inflated = flushed.len(),
                    prefix = %hex_prefix(&flushed, 32),
                    "server live-object stream flushed on first non-clean P05 continuation"
                );
                *bytes = flushed;
                return Ok(None);
            }
            return Ok(None);
        }

        return Ok(None);
    }

    if HighLevel::parse(bytes).is_none() && state.server_live_object_stream.is_some() {
        append_pending_live_object_continuation(state, reassembly.first_sequence, bytes);
        if let Some(flushed) = take_pending_live_object_stream_payload(state) {
            tracing::info!(
                current_sequence = reassembly.first_sequence,
                old_inflated = bytes.len(),
                rebuilt_inflated = flushed.len(),
                prefix = %hex_prefix(&flushed, 32),
                "server live-object stream continuation flushed as rebuilt GameObjUpdate_LiveObject"
            );
            *bytes = flushed;
            return Ok(None);
        }
    }

    Ok(None)
}

fn starts_with_live_object_high_level(bytes: &[u8]) -> bool {
    bytes.len() >= 7 && bytes[0] == b'P' && bytes[1] == 0x05 && bytes[2] == 0x01
}

fn looks_like_clean_legacy_live_object_fragment(bytes: &[u8]) -> bool {
    let mut probe = bytes.to_vec();
    live_object::normalize_prefixed_fragments_payload_if_needed(&mut probe)
        .map(|summary| summary.dropped_leadin_bytes == 0 && !summary.salvaged_partial_leadin)
        .unwrap_or(false)
}

fn append_pending_live_object_clean_fragment(
    state: &mut SessionState,
    first_sequence: u16,
    bytes: &[u8],
) {
    if bytes.len() < 7 {
        return;
    }
    let pending = state
        .server_live_object_stream
        .get_or_insert_with(|| PendingLiveObjectStream {
            read_bytes: Vec::new(),
            fragment_bytes: Vec::new(),
            first_sequence,
            chunks: 0,
        });
    pending.fragment_bytes.extend_from_slice(&bytes[3..7]);
    pending.read_bytes.extend_from_slice(&bytes[7..]);
    pending.chunks = pending.chunks.saturating_add(1);
}

fn append_pending_live_object_continuation(
    state: &mut SessionState,
    first_sequence: u16,
    bytes: &[u8],
) {
    let pending = state
        .server_live_object_stream
        .get_or_insert_with(|| PendingLiveObjectStream {
            read_bytes: Vec::new(),
            fragment_bytes: Vec::new(),
            first_sequence,
            chunks: 0,
        });
    pending.read_bytes.extend_from_slice(bytes);
    pending.chunks = pending.chunks.saturating_add(1);
}

fn take_pending_live_object_stream_payload(state: &mut SessionState) -> Option<Vec<u8>> {
    let pending = state.server_live_object_stream.take()?;
    if pending.read_bytes.is_empty() || pending.fragment_bytes.is_empty() {
        return None;
    }

    let declared_usize = 3usize
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(pending.read_bytes.len())?;
    let declared = u32::try_from(declared_usize).ok()?;
    let mut rebuilt = Vec::with_capacity(declared_usize + pending.fragment_bytes.len());
    rebuilt.push(b'P');
    rebuilt.push(0x05);
    rebuilt.push(0x01);
    rebuilt.extend_from_slice(&declared.to_le_bytes());
    rebuilt.extend_from_slice(&pending.read_bytes);
    rebuilt.extend_from_slice(&pending.fragment_bytes);
    Some(rebuilt)
}

fn buffered_frame_from_view(
    bytes: &[u8],
    view: &MFrameView,
    first_frame: bool,
) -> anyhow::Result<BufferedFrame> {
    if view.payload_length > bytes.len().saturating_sub(LEGACY_GAMEPLAY_PAYLOAD_OFFSET) {
        anyhow::bail!("M payload length exceeds datagram");
    }

    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let payload_end = payload_start + view.payload_length;
    let compressed_start = if first_frame {
        if view.payload_length < 4 {
            anyhow::bail!("first deflated M frame is too short for inflated length");
        }
        payload_start + 4
    } else {
        payload_start
    };

    Ok(BufferedFrame {
        packet: bytes.to_vec(),
        payload_length: view.payload_length,
        sequence: view.sequence,
        ack_sequence: view.ack_sequence,
        compressed_chunk: bytes[compressed_start..payload_end].to_vec(),
    })
}

fn build_server_deflated_output_frames(
    reassembly: &ServerDeflatedReassembly,
    combined_payload: &[u8],
    clear_first_frame_flags: u8,
    set_first_packetized_sequence_to_output_count: bool,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut outputs = Vec::with_capacity(reassembly.frames.len());
    let mut cursor = 0;

    for (index, frame) in reassembly.frames.iter().enumerate() {
        if cursor > combined_payload.len() {
            anyhow::bail!("deflated output cursor exceeded combined payload");
        }

        let final_frame = index + 1 == reassembly.frames.len();
        let remaining = combined_payload.len() - cursor;
        let chunk_length = if final_frame {
            remaining
        } else {
            frame.payload_length.min(remaining)
        };
        if chunk_length > u16::MAX as usize {
            anyhow::bail!("deflated output chunk too large for legacy packetized length");
        }

        let mut out_packet = frame.packet.clone();
        out_packet.resize(LEGACY_GAMEPLAY_PAYLOAD_OFFSET + chunk_length, 0);
        if chunk_length != 0 {
            out_packet
                [LEGACY_GAMEPLAY_PAYLOAD_OFFSET..LEGACY_GAMEPLAY_PAYLOAD_OFFSET + chunk_length]
                .copy_from_slice(&combined_payload[cursor..cursor + chunk_length]);
        }
        cursor += chunk_length;

        if index == 0 && clear_first_frame_flags != 0 && out_packet.len() > 7 {
            out_packet[7] &= !clear_first_frame_flags;
        }
        write_be_u16(&mut out_packet, 10, chunk_length as u16)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update M packetized length"))?;
        encode_legacy_m_crc(&mut out_packet)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair M CRC"))?;

        outputs.push(out_packet);
    }

    if cursor != combined_payload.len() || outputs.is_empty() {
        anyhow::bail!(
            "deflated output frame capacity mismatch: combined={} emitted={}",
            combined_payload.len(),
            cursor
        );
    }

    if set_first_packetized_sequence_to_output_count {
        let output_count = outputs.len() as u16;
        let first = outputs
            .first_mut()
            .ok_or_else(|| anyhow::anyhow!("missing first deflated output frame"))?;
        write_be_u16(first, 8, output_count)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update first M packetized sequence"))?;
        encode_legacy_m_crc(first)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair first M CRC"))?;
    }

    Ok(outputs)
}

fn build_consumed_server_deflated_frames(
    reassembly: &ServerDeflatedReassembly,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut outputs = Vec::with_capacity(reassembly.frames.len());
    for frame in &reassembly.frames {
        let mut out_packet = frame.packet.clone();
        out_packet.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
        if out_packet.len() > 7 {
            // Keep the reliable-window sequence/ack shell so the client can
            // acknowledge progress, but clear deflate/stream delivery bits and
            // packetized count/length so no unsafe high-level CNWMessage reaches
            // EE's guarded reader.
            out_packet[7] &= !0x05;
        }
        write_be_u16(&mut out_packet, 8, 0)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to clear consumed M packetized sequence"))?;
        write_be_u16(&mut out_packet, 10, 0)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to clear consumed M packetized length"))?;
        encode_legacy_m_crc(&mut out_packet)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair consumed M CRC"))?;
        outputs.push(out_packet);
    }
    Ok(outputs)
}

fn inflate_gameplay_payload(
    compressed: &[u8],
    inflated_length: usize,
    zlib_stream: bool,
    server_stream: &mut Option<Decompress>,
) -> anyhow::Result<InflatedGameplayPayload> {
    if inflated_length > MAX_REASONABLE_GAMEPLAY_PAYLOAD {
        anyhow::bail!("inflated gameplay length is unreasonable: {inflated_length}");
    }

    if zlib_stream && !looks_like_zlib_wrapped_deflate(compressed) {
        match inflate_with_server_stream(compressed, inflated_length, server_stream)? {
            Some(bytes) => {
                if !inflated_cnw_fragment_offset_valid_or_normalizable(&bytes) {
                    tracing::warn!(
                        inflated_length = bytes.len(),
                        prefix = %hex_prefix(&bytes, 32),
                        "server zlib-stream candidate failed CNW fragment-offset validation/normalization; trying independent raw-deflate window"
                    );
                    if let Some(independent) = inflate_with_window(
                        compressed,
                        inflated_length,
                        false,
                        FlushDecompress::Sync,
                    )? {
                        if inflated_cnw_fragment_offset_valid_or_normalizable(&independent) {
                            tracing::info!(
                                inflated_length = independent.len(),
                                prefix = %hex_prefix(&independent, 32),
                                "server deflated M window accepted as independent raw-deflate after stream reset/normalization"
                            );
                            *server_stream = None;
                            return Ok(InflatedGameplayPayload {
                                bytes: independent,
                                used_server_stream: false,
                            });
                        }
                    }
                }
                return Ok(InflatedGameplayPayload {
                    bytes,
                    used_server_stream: true,
                });
            }
            None => {
                *server_stream = None;
            }
        }
    }

    if let Some(inflated) =
        inflate_with_window(compressed, inflated_length, false, FlushDecompress::Sync)?
    {
        return Ok(InflatedGameplayPayload {
            bytes: inflated,
            used_server_stream: false,
        });
    }
    if let Some(inflated) =
        inflate_with_window(compressed, inflated_length, true, FlushDecompress::Finish)?
    {
        return Ok(InflatedGameplayPayload {
            bytes: inflated,
            used_server_stream: false,
        });
    }
    if let Some(inflated) =
        inflate_with_window(compressed, inflated_length, true, FlushDecompress::Sync)?
    {
        return Ok(InflatedGameplayPayload {
            bytes: inflated,
            used_server_stream: false,
        });
    }

    anyhow::bail!(
        "failed to inflate server gameplay payload: compressed={} inflated={}",
        compressed.len(),
        inflated_length
    )
}

fn completed_server_stream_window<'a>(
    state: &'a SessionState,
    reassembly: &ServerDeflatedReassembly,
    compressed_length: usize,
) -> Option<&'a CompletedDeflatedStreamWindow> {
    state.completed_server_stream_windows.iter().find(|window| {
        window.first_sequence == reassembly.first_sequence
            && window.expected_frames == reassembly.expected_frames
            && window.packetized_sequence == reassembly.packetized_sequence
            && window.inflated_length == reassembly.inflated_length
            && window.compressed_length == compressed_length
    })
}

fn remember_completed_server_stream_window(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    compressed_length: usize,
    replay: CompletedDeflatedReplay,
) {
    if completed_server_stream_window(state, reassembly, compressed_length).is_some() {
        return;
    }

    const MAX_COMPLETED_STREAM_WINDOWS: usize = 16;
    state.completed_server_stream_windows.push(CompletedDeflatedStreamWindow {
        first_sequence: reassembly.first_sequence,
        expected_frames: reassembly.expected_frames,
        packetized_sequence: reassembly.packetized_sequence,
        inflated_length: reassembly.inflated_length,
        compressed_length,
        replay,
    });
    if state.completed_server_stream_windows.len() > MAX_COMPLETED_STREAM_WINDOWS {
        let overflow = state.completed_server_stream_windows.len() - MAX_COMPLETED_STREAM_WINDOWS;
        state.completed_server_stream_windows.drain(0..overflow);
    }
}

fn maybe_buffer_or_flush_server_quickbar_stream(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    used_server_stream: bool,
    bytes: &[u8],
) -> anyhow::Result<Option<Emit>> {
    if !used_server_stream || !state.server_zlib_stream_proxy_owned {
        return Ok(None);
    }

    if state.server_quickbar_stream.is_none() {
        let mut fragment_wait = false;
        let target_length = quickbar::full_set_all_buttons_target_length(bytes);
        if target_length.is_none() {
            let mut probe = bytes.to_vec();
            let Some((_, summary)) =
                quickbar::normalize_and_rewrite_quickbar_payload_if_possible(&mut probe)
            else {
                return Ok(None);
            };
            if !quickbar::rewrite_summary_needs_more_quickbar_bytes(&summary) {
                return Ok(None);
            }
            fragment_wait = true;
            tracing::info!(
                first_sequence = reassembly.first_sequence,
                packetized_sequence = reassembly.packetized_sequence,
                read_size = summary.read_size,
                fragment_size = summary.fragment_size,
                final_cursor = summary.final_cursor,
                trailing_read_bytes = summary.trailing_read_bytes,
                spells_preserved = summary.spells_preserved,
                general_buttons_preserved = summary.general_buttons_preserved,
                item_buttons_blanked = summary.item_buttons_blanked,
                unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
                buffered = bytes.len(),
                "server GuiQuickbar_SetAllButtons continuation buffering started"
            );
        }
        state.server_quickbar_stream = Some(PendingQuickbarStream {
            payload: bytes.to_vec(),
            target_length,
            fragment_wait,
            first_sequence: reassembly.first_sequence,
            chunks: 1,
        });
        let mut outputs = build_consumed_server_deflated_frames(reassembly)?;
        remember_completed_server_stream_window(
            state,
            reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
        );
        outputs.extend(reassembly.interleaved_packets.clone());
        tracing::info!(
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            target_length = target_length.unwrap_or(0),
            fragment_wait,
            buffered = bytes.len(),
            "server GuiQuickbar_SetAllButtons stream buffering started"
        );
        return Ok(Some(Emit::VerifiedPackets(outputs)));
    }

    let pending = state
        .server_quickbar_stream
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing quickbar stream state"))?;
    pending.payload.extend_from_slice(bytes);
    pending.chunks = pending.chunks.saturating_add(1);
    if let Some(target_length) = pending.target_length {
        if pending.payload.len() < target_length {
            let buffered = pending.payload.len();
            let chunks = pending.chunks;
            let first_sequence = pending.first_sequence;
            let mut outputs = build_consumed_server_deflated_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
            );
            outputs.extend(reassembly.interleaved_packets.clone());
            tracing::info!(
                first_sequence,
                current_sequence = reassembly.first_sequence,
                chunks,
                buffered,
                target_length,
                "server GuiQuickbar_SetAllButtons stream buffering continued"
            );
            return Ok(Some(Emit::VerifiedPackets(outputs)));
        }
    } else {
        let mut rewritten_payload = pending.payload.clone();
        let rewrite = quickbar::normalize_and_rewrite_quickbar_payload_if_possible(
            &mut rewritten_payload,
        );
        let should_wait = rewrite
            .as_ref()
            .map(|(_, summary)| {
                quickbar::rewrite_summary_needs_more_quickbar_bytes(summary)
                    && summary.spells_preserved == 0
                    && pending.chunks < 2
                    && pending.payload.len() < 32 * 1024
            })
            .unwrap_or(pending.chunks < 2 && pending.payload.len() < 32 * 1024);
        if should_wait {
            let buffered = pending.payload.len();
            let chunks = pending.chunks;
            let first_sequence = pending.first_sequence;
            let mut outputs = build_consumed_server_deflated_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
            );
            outputs.extend(reassembly.interleaved_packets.clone());
            tracing::info!(
                first_sequence,
                current_sequence = reassembly.first_sequence,
                chunks,
                buffered,
                "server GuiQuickbar_SetAllButtons continuation buffering continued"
            );
            return Ok(Some(Emit::VerifiedPackets(outputs)));
        }
    }

    let pending = state
        .server_quickbar_stream
        .take()
        .ok_or_else(|| anyhow::anyhow!("missing quickbar stream state"))?;
    let mut quickbar_payload = if let Some(target_length) = pending.target_length {
        pending.payload[..target_length].to_vec()
    } else {
        pending.payload.clone()
    };
    let quickbar_rewrite =
        quickbar::normalize_and_rewrite_quickbar_payload_if_possible(&mut quickbar_payload);
    let trailing_bytes = pending
        .target_length
        .map(|target_length| pending.payload.len().saturating_sub(target_length))
        .unwrap_or(0);
    let compressed = deflate_zlib(&quickbar_payload)?;
    let mut combined = Vec::with_capacity(CNW_LENGTH_BYTES + compressed.len());
    combined.extend_from_slice(&(quickbar_payload.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);
    let mut outputs = build_server_deflated_output_frames(reassembly, &combined, 0x01, true)?;
    remember_completed_server_stream_window(
        state,
        reassembly,
        source_compressed_length,
        CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
    );
    outputs.extend(reassembly.interleaved_packets.clone());

    if let Some((_, summary)) = quickbar_rewrite.as_ref() {
        tracing::info!(
            first_sequence = pending.first_sequence,
            current_sequence = reassembly.first_sequence,
            chunks = pending.chunks,
            fragment_wait = pending.fragment_wait,
            old_declared = summary.old_declared,
            new_declared = summary.new_declared,
            read_size = summary.read_size,
            fragment_size = summary.fragment_size,
            final_cursor = summary.final_cursor,
            trailing_read_bytes = summary.trailing_read_bytes,
            spells_preserved = summary.spells_preserved,
            general_buttons_preserved = summary.general_buttons_preserved,
            item_buttons_blanked = summary.item_buttons_blanked,
            unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            compressed = compressed.len(),
            "server GuiQuickbar_SetAllButtons stream reassembled and semantically rewritten for EE"
        );
    } else {
        let buffered = pending.payload.len();
        let chunks = pending.chunks;
        tracing::info!(
            first_sequence = pending.first_sequence,
            current_sequence = reassembly.first_sequence,
            chunks,
            buffered,
            fragment_wait = pending.fragment_wait,
            trailing_bytes,
            compressed = compressed.len(),
            "server GuiQuickbar_SetAllButtons stream reassembled for EE"
        );
    }
    Ok(Some(Emit::VerifiedPackets(outputs)))
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

fn inflated_cnw_fragment_offset_valid_or_normalizable(inflated: &[u8]) -> bool {
    if inflated_cnw_fragment_offset_valid(inflated) {
        return true;
    }
    let mut probe = inflated.to_vec();
    if player_list::rewrite_player_list_payload_if_possible(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if quickbar::normalize_quickbar_payload_if_needed(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if quickbar::rewrite_simple_quickbar_payload_if_possible(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if cnw_message::normalize_prefixed_fragments_payload_if_needed(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    live_object::normalize_prefixed_fragments_payload_if_needed(&mut probe).is_some()
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

fn dump_invalid_inflated_payload_for_span(inflated: &[u8], sequence: u16, reason: &str) {
    let Ok(dir) = std::env::var("HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR") else {
        return;
    };

    let dir = PathBuf::from(dir);
    if fs::create_dir_all(&dir).is_err() {
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
        "{}-{}-coalesced-seq{}-{}.bin",
        reason, high_name, sequence, millis
    ));

    if fs::write(&path, inflated).is_ok() {
        tracing::info!(
            path = %path.display(),
            inflated_length = inflated.len(),
            sequence,
            reason,
            "dumped invalid coalesced inflated payload for offline fixture analysis"
        );
    }
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
