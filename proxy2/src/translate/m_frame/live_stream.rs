//! Buffered live-object stream handling for deflated M windows.
//!
//! This module only decides whether fragmented `GameObjUpdate_LiveObject` bytes
//! need to be accumulated across reliable windows. Record-family semantics stay
//! in `translate::live_object` and `translate::live_object_update`.

use crate::{packet::m::HighLevel, translate::{live_object, Emit}};

use super::{
    hex_prefix,
    reassembly::{
        build_consumed_server_deflated_frames, remember_completed_server_stream_window,
        CompletedDeflatedReplay, ServerDeflatedReassembly,
    },
    SessionState, CNW_LENGTH_BYTES,
};

#[derive(Debug, Clone)]
pub(super) struct PendingLiveObjectStream {
    read_bytes: Vec<u8>,
    fragment_bytes: Vec<u8>,
    first_sequence: u16,
    chunks: u32,
}

pub(super) fn maybe_buffer_or_flush_server_live_object_stream(
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


