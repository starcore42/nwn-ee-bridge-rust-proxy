//! Buffered `GuiQuickbar` stream handling for deflated M windows.
//!
//! The root M-frame dispatcher should not accumulate partial quickbar payload
//! state. This module owns buffering and flush decisions, then delegates actual
//! quickbar semantics to `translate::quickbar`.

use crate::{
    packet::m::HighLevel,
    translate::{ContinuationOwner, Emit, VerifiedFamily, quickbar},
};
use std::{
    env, fs,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    CNW_LENGTH_BYTES, SessionState,
    deflate::deflate_zlib,
    reassembly::{
        CompletedDeflatedReplay, ServerDeflatedReassembly, build_server_deflated_output_frames,
        remember_completed_server_stream_window,
    },
};

// Current HG captures deliver the legacy-prefixed SetAllButtons stream as the
// initial opcode chunk plus three zlib-stream continuations. Waiting beyond that
// without a semantic quickbar claim only creates an empty-frame replay sink: the
// EE client never accepts those empty shells as the missing reliable payload, so
// the 1.69 server retransmits the same window forever. If a future capture needs
// more than four chunks, it must be backed by a dumped/quarantined payload and a
// decompile-confirmed stream rule rather than another permissive wait.
const MAX_PENDING_QUICKBAR_STREAM_CHUNKS: u32 = 4;
const MAX_PENDING_QUICKBAR_STREAM_BYTES: usize = 64 * 1024;
const MAX_PENDING_QUICKBAR_DUPLICATE_REPLAYS_BEFORE_CLAIM: u32 = 4;
const QUICKBAR_PLACEHOLDER_ENVELOPE: u8 = 0x50;

#[derive(Debug, Clone)]
pub(super) struct PendingQuickbarStream {
    payload: Vec<u8>,
    target_length: Option<usize>,
    fragment_wait: bool,
    first_sequence: u16,
    chunks: u32,
    duplicate_replays: u32,
}

fn build_blank_quickbar_placeholder_frames(
    reassembly: &ServerDeflatedReassembly,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let payload = quickbar::build_blank_set_all_buttons_payload(QUICKBAR_PLACEHOLDER_ENVELOPE)
        .ok_or_else(|| anyhow::anyhow!("failed to build blank quickbar placeholder payload"))?;
    let compressed = deflate_zlib(&payload)?;
    let mut combined = Vec::with_capacity(CNW_LENGTH_BYTES + compressed.len());
    combined.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);
    build_server_deflated_output_frames(reassembly, &combined, 0x01, true)
}

pub(super) fn maybe_buffer_or_flush_server_quickbar_stream(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    used_server_stream: bool,
    bytes: &[u8],
) -> anyhow::Result<Option<Emit>> {
    if !used_server_stream || !state.deflate.server_zlib_stream_proxy_owned {
        return Ok(None);
    }
    claim_server_zlib_stream_owner(state, ContinuationOwner::GuiQuickbar);

    if state.quickbar.pending_stream.is_none() {
        let mut fragment_wait = false;
        let target_length = quickbar::full_set_all_buttons_target_length(bytes);
        if target_length.is_none() {
            let mut probe = bytes.to_vec();
            match quickbar::normalize_and_rewrite_quickbar_payload_if_possible(&mut probe) {
                Some((_, summary)) => {
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
                None => {
                    if !starts_with_implausible_quickbar_set_all_buttons(bytes) {
                        return Ok(None);
                    }
                    fragment_wait = true;
                    tracing::info!(
                        first_sequence = reassembly.first_sequence,
                        packetized_sequence = reassembly.packetized_sequence,
                        buffered = bytes.len(),
                        prefix = %super::hex_prefix(bytes, 32),
                        "server GuiQuickbar_SetAllButtons stream buffering started before semantic split is claimable"
                    );
                }
            }
        }
        state.quickbar.pending_stream = Some(PendingQuickbarStream {
            payload: bytes.to_vec(),
            target_length,
            fragment_wait,
            first_sequence: reassembly.first_sequence,
            chunks: 1,
            duplicate_replays: 0,
        });
        let mut outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
        remember_completed_server_stream_window(
            state,
            reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets {
                family: VerifiedFamily::GuiQuickbarPlaceholder,
                packets: outputs.clone(),
            },
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
        return Ok(Some(Emit::VerifiedPackets {
            family: VerifiedFamily::GuiQuickbarPlaceholder,
            packets: outputs,
        }));
    }

    let pending = state
        .quickbar
        .pending_stream
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("missing quickbar stream state"))?;
    pending.payload.extend_from_slice(bytes);
    pending.chunks = pending.chunks.saturating_add(1);
    if let Some(target_length) = pending.target_length {
        if pending.payload.len() < target_length {
            let buffered = pending.payload.len();
            let chunks = pending.chunks;
            let first_sequence = pending.first_sequence;
            let mut outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::GuiQuickbarPlaceholder,
                    packets: outputs.clone(),
                },
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
            return Ok(Some(Emit::VerifiedPackets {
                family: VerifiedFamily::GuiQuickbarPlaceholder,
                packets: outputs,
            }));
        }
    } else {
        let mut rewritten_payload = pending.payload.clone();
        let rewrite =
            quickbar::normalize_and_rewrite_quickbar_payload_if_possible(&mut rewritten_payload);
        let under_wait_budget = pending.chunks < MAX_PENDING_QUICKBAR_STREAM_CHUNKS
            && pending.payload.len() < MAX_PENDING_QUICKBAR_STREAM_BYTES;
        let should_wait = rewrite
            .as_ref()
            .map(|(_, summary)| {
                let claimable_minimum_stream =
                    pending.chunks >= 3 && summary.trailing_read_bytes == 0;
                quickbar::rewrite_summary_needs_more_quickbar_bytes(summary)
                    && under_wait_budget
                    && !claimable_minimum_stream
            })
            .unwrap_or(under_wait_budget);
        if should_wait {
            let buffered = pending.payload.len();
            let chunks = pending.chunks;
            let first_sequence = pending.first_sequence;
            let mut outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::GuiQuickbarPlaceholder,
                    packets: outputs.clone(),
                },
            );
            outputs.extend(reassembly.interleaved_packets.clone());
            tracing::info!(
                first_sequence,
                current_sequence = reassembly.first_sequence,
                chunks,
                buffered,
                "server GuiQuickbar_SetAllButtons continuation buffering continued"
            );
            return Ok(Some(Emit::VerifiedPackets {
                family: VerifiedFamily::GuiQuickbarPlaceholder,
                packets: outputs,
            }));
        }
    }

    flush_pending_server_quickbar_stream(state, reassembly, source_compressed_length)
}

pub(super) fn force_flush_pending_server_quickbar_stream(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
) -> anyhow::Result<Option<Emit>> {
    claim_server_zlib_stream_owner(state, ContinuationOwner::GuiQuickbar);
    let Some(pending) = state.quickbar.pending_stream.as_mut() else {
        return Ok(None);
    };
    if pending.first_sequence != reassembly.first_sequence {
        return Ok(None);
    }
    pending.duplicate_replays = pending.duplicate_replays.saturating_add(1);
    let claimable_after_duplicate_grace = pending.chunks >= 3
        && pending.duplicate_replays >= MAX_PENDING_QUICKBAR_DUPLICATE_REPLAYS_BEFORE_CLAIM
        && {
            let mut probe = pending.payload.clone();
            quickbar::normalize_and_rewrite_quickbar_payload_if_possible(&mut probe).is_some()
        };
    tracing::warn!(
        first_sequence = pending.first_sequence,
        duplicate_sequence = reassembly.first_sequence,
        chunks = pending.chunks,
        duplicate_replays = pending.duplicate_replays,
        buffered = pending.payload.len(),
        fragment_wait = pending.fragment_wait,
        "server GuiQuickbar_SetAllButtons duplicate replay consumed while pending stream waits for semantic boundary"
    );
    if claimable_after_duplicate_grace {
        tracing::info!(
            first_sequence = pending.first_sequence,
            chunks = pending.chunks,
            duplicate_replays = pending.duplicate_replays,
            buffered = pending.payload.len(),
            "server GuiQuickbar_SetAllButtons duplicate grace exhausted; flushing claimable buffered stream"
        );
        return flush_pending_server_quickbar_stream(state, reassembly, source_compressed_length);
    }
    if pending.duplicate_replays == MAX_PENDING_QUICKBAR_DUPLICATE_REPLAYS_BEFORE_CLAIM {
        dump_unclaimable_pending_quickbar_stream(pending);
    }
    let mut outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
    remember_completed_server_stream_window(
        state,
        reassembly,
        source_compressed_length,
        CompletedDeflatedReplay::VerifiedPackets {
            family: VerifiedFamily::GuiQuickbarPlaceholder,
            packets: outputs.clone(),
        },
    );
    outputs.extend(reassembly.interleaved_packets.clone());
    Ok(Some(Emit::VerifiedPackets {
        family: VerifiedFamily::GuiQuickbarPlaceholder,
        packets: outputs,
    }))
}

fn dump_unclaimable_pending_quickbar_stream(pending: &PendingQuickbarStream) {
    let Ok(dir) = env::var("NWN_BRIDGE_QUICKBAR_DUMP_DIR") else {
        return;
    };
    let Ok(millis) = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
    else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!(
        "quickbar_pending_unclaimable_seq{}_chunks{}_{}.bin",
        pending.first_sequence, pending.chunks, millis
    ));
    if let Err(err) = fs::write(&path, &pending.payload) {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            buffered = pending.payload.len(),
            "failed to dump unclaimable pending quickbar stream"
        );
        return;
    }
    tracing::warn!(
        path = %path.display(),
        first_sequence = pending.first_sequence,
        chunks = pending.chunks,
        duplicate_replays = pending.duplicate_replays,
        buffered = pending.payload.len(),
        "dumped unclaimable pending quickbar stream after duplicate grace"
    );
}

fn flush_pending_server_quickbar_stream(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
) -> anyhow::Result<Option<Emit>> {
    claim_server_zlib_stream_owner(state, ContinuationOwner::GuiQuickbar);
    let pending = state
        .quickbar
        .pending_stream
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
        dump_quickbar_payload_for_diagnostics("quarantined_stream", &quickbar_payload);
        let mut outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
        remember_completed_server_stream_window(
            state,
            reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets {
                family: VerifiedFamily::GuiQuickbarPlaceholder,
                packets: outputs.clone(),
            },
        );
        outputs.extend(reassembly.interleaved_packets.clone());
        tracing::warn!(
            first_sequence = pending.first_sequence,
            current_sequence = reassembly.first_sequence,
            chunks = pending.chunks,
            buffered = pending.payload.len(),
            fragment_wait = pending.fragment_wait,
            trailing_bytes,
            high_level = HighLevel::parse(&quickbar_payload)
                .map(|high| high.name())
                .unwrap_or("none"),
            "server GuiQuickbar_SetAllButtons stream quarantined: semantic quickbar translator did not claim payload"
        );
        return Ok(Some(Emit::VerifiedPackets {
            family: VerifiedFamily::GuiQuickbarPlaceholder,
            packets: outputs,
        }));
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
        CompletedDeflatedReplay::VerifiedPackets {
            family: VerifiedFamily::GuiQuickbar,
            packets: outputs.clone(),
        },
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
    Ok(Some(Emit::VerifiedPackets {
        family: VerifiedFamily::GuiQuickbar,
        packets: outputs,
    }))
}

fn dump_quickbar_payload_for_diagnostics(label: &str, payload: &[u8]) {
    let Ok(enabled) = std::env::var("NWN_BRIDGE_DUMP_QUICKBAR") else {
        return;
    };
    if enabled != "1" && enabled.to_ascii_lowercase() != "true" {
        return;
    }
    let dir = std::env::var("NWN_BRIDGE_QUICKBAR_DUMP_DIR")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ".".to_string());
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let path = std::path::Path::new(&dir).join(format!("quickbar_{label}_{millis}.bin"));
    if let Err(error) = std::fs::write(&path, payload) {
        tracing::warn!(%error, path = %path.display(), "failed to dump quickbar payload");
    } else {
        tracing::warn!(path = %path.display(), len = payload.len(), "dumped quarantined quickbar payload");
    }
}

fn starts_with_implausible_quickbar_set_all_buttons(bytes: &[u8]) -> bool {
    let Some(high) = HighLevel::parse(bytes) else {
        return false;
    };
    if high.major != 30 || high.minor != 1 || bytes.len() < 7 {
        return false;
    }
    let declared = u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]) as usize;
    declared < 3 || declared > bytes.len()
}

fn claim_server_zlib_stream_owner(state: &mut SessionState, owner: ContinuationOwner) {
    if state.deflate.server_zlib_stream_owner != Some(owner) {
        state.deflate.server_zlib_stream_epoch =
            state.deflate.server_zlib_stream_epoch.saturating_add(1);
    }
    state.deflate.server_zlib_stream_owner = Some(owner);
}
