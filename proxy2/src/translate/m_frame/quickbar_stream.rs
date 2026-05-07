//! Buffered `GuiQuickbar` stream handling for deflated M windows.
//!
//! The root M-frame dispatcher should not accumulate partial quickbar payload
//! state. This module owns buffering and flush decisions, then delegates actual
//! quickbar semantics to `translate::quickbar`.

use crate::{
    packet::m::HighLevel,
    translate::{quickbar, Emit},
};

use super::{
    deflate::deflate_zlib,
    reassembly::{
        build_consumed_server_deflated_frames, build_server_deflated_output_frames,
        remember_completed_server_stream_window, CompletedDeflatedReplay,
        ServerDeflatedReassembly,
    },
    SessionState, CNW_LENGTH_BYTES,
};

#[derive(Debug, Clone)]
pub(super) struct PendingQuickbarStream {
    payload: Vec<u8>,
    target_length: Option<usize>,
    fragment_wait: bool,
    first_sequence: u16,
    chunks: u32,
}

pub(super) fn maybe_buffer_or_flush_server_quickbar_stream(
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
    if quickbar_rewrite.is_none() {
        let mut outputs = build_consumed_server_deflated_frames(reassembly)?;
        remember_completed_server_stream_window(
            state,
            reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets(outputs.clone()),
        );
        outputs.extend(reassembly.interleaved_packets.clone());
        tracing::warn!(
            first_sequence = pending.first_sequence,
            current_sequence = reassembly.first_sequence,
            chunks = pending.chunks,
            buffered = pending.payload.len(),
            fragment_wait = pending.fragment_wait,
            trailing_bytes,
            high_level = HighLevel::parse(&quickbar_payload).map(|high| high.name()).unwrap_or("none"),
            "server GuiQuickbar_SetAllButtons stream quarantined: semantic quickbar translator did not claim payload"
        );
        return Ok(Some(Emit::VerifiedPackets(outputs)));
    }
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

