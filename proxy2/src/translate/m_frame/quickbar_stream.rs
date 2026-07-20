//! Buffered `GuiQuickbar` stream handling for deflated M windows.
//!
//! The root M-frame dispatcher should not accumulate partial quickbar payload
//! state. This module owns buffering and flush decisions, then delegates actual
//! quickbar semantics to `translate::quickbar`.

use crate::{
    packet::m::HighLevel,
    translate::{ContinuationOwner, Emit, VerifiedFamily, VerifiedProof, quickbar},
};
use std::{
    env, fs,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    CNW_LENGTH_BYTES, SessionState,
    deflate::deflate_zlib,
    quickbar_materialization::{self, QuickbarRewriteMode},
    reassembly::{
        CompletedDeflatedReplay, ServerDeflatedReassembly, build_server_deflated_output_frames,
        emit_family_packets_with_interleaved, remember_completed_server_stream_window,
        remember_completed_server_stream_window_with_disposition,
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

fn rewrite_quickbar_payload_for_stream(
    payload: &mut Vec<u8>,
    object_registry: Option<&crate::translate::semantic::ObjectRegistry>,
    mode: QuickbarRewriteMode,
) -> Option<quickbar::QuickbarRewriteSummary> {
    quickbar_materialization::rewrite_payload_with_registry_if_possible(
        payload,
        object_registry,
        mode,
    )
}

fn observe_quickbar_stream_probe_summary(
    state: &mut SessionState,
    summary: &quickbar::QuickbarRewriteSummary,
) {
    let materialization_context = state.semantic.objects.inventory_item_context_summary();
    state
        .semantic
        .ui
        .observe_quickbar_stream_probe(summary, materialization_context);
    let promoted_committed_profile = state
        .semantic
        .ui
        .promote_quickbar_stream_probe_profile(summary, materialization_context);
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
        "semantic state observed quickbar stream-probe summary"
    );
    super::update_quickbar_item_refresh_hint(state);
}

fn observe_committed_quickbar_stream_payload(state: &mut SessionState, payload: &[u8]) {
    let proof = VerifiedProof::family(VerifiedFamily::GuiQuickbar);
    crate::translate::semantic::observe_verified_payload_with_area_context(
        &mut state.semantic,
        crate::packet::Direction::ServerToClient,
        &proof,
        payload,
        Some(&state.area_context.latest_area_placeables),
    );
    super::update_quickbar_item_refresh_hint(state);
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
    if state.quickbar.pending_stream.is_none() {
        let mut fragment_wait = false;
        let target_length = quickbar::full_set_all_buttons_target_length(bytes);
        if target_length.is_none() {
            let mut probe = bytes.to_vec();
            match rewrite_quickbar_payload_for_stream(
                &mut probe,
                Some(&state.semantic.objects),
                QuickbarRewriteMode::StreamProbe,
            ) {
                Some(summary) => {
                    observe_quickbar_stream_probe_summary(state, &summary);
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
                        slot_records_owned = summary.slot_records_owned,
                        spells_preserved = summary.spells_preserved,
                        general_buttons_preserved = summary.general_buttons_preserved,
                        item_buttons_blanked = summary.item_buttons_blanked,
                        item_objects_preserved_by_explicit_self_materialization =
                            summary.item_objects_preserved_by_explicit_self_materialization,
                        item_objects_preserved_by_active_state =
                            summary.item_objects_preserved_by_active_state,
                        item_objects_preserved_by_feature25_first =
                            summary.item_objects_preserved_by_feature25_first,
                        item_objects_preserved_by_feature25_second =
                            summary.item_objects_preserved_by_feature25_second,
                        item_objects_preserved_by_feature25_legacy_tail =
                            summary.item_objects_preserved_by_feature25_legacy_tail,
                        item_buttons_rejected_missing_state_unknown =
                            summary.item_buttons_rejected_missing_state_unknown,
                        item_buttons_rejected_missing_state_cleared_delete =
                            summary.item_buttons_rejected_missing_state_cleared_delete,
                        item_buttons_rejected_missing_state_cleared_area_reset =
                            summary.item_buttons_rejected_missing_state_cleared_area_reset,
                        unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
                        buffered = bytes.len(),
                        "server GuiQuickbar_SetAllButtons continuation buffering started"
                    );
                }
                None => {
                    if !starts_with_implausible_quickbar_set_all_buttons(bytes) {
                        return Ok(None);
                    }
                    dump_quickbar_payload_for_diagnostics("buffering_unclaimable_start", bytes);
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
        claim_server_zlib_stream_owner(state, ContinuationOwner::GuiQuickbar);
        state.quickbar.pending_stream = Some(PendingQuickbarStream {
            payload: bytes.to_vec(),
            target_length,
            fragment_wait,
            first_sequence: reassembly.first_sequence,
            chunks: 1,
            duplicate_replays: 0,
        });
        let outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
        remember_completed_server_stream_window(
            state,
            reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets {
                family: VerifiedFamily::GuiQuickbarPlaceholder,
                packets: outputs.clone(),
            },
        );
        let interleaved_packets = reassembly.interleaved_packets.clone();
        tracing::info!(
            first_sequence = reassembly.first_sequence,
            packetized_sequence = reassembly.packetized_sequence,
            target_length = target_length.unwrap_or(0),
            fragment_wait,
            buffered = bytes.len(),
            "server GuiQuickbar_SetAllButtons stream buffering started"
        );
        return Ok(Some(emit_family_packets_with_interleaved(
            VerifiedFamily::GuiQuickbarPlaceholder,
            outputs,
            interleaved_packets,
        )));
    }

    claim_server_zlib_stream_owner(state, ContinuationOwner::GuiQuickbar);
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
            let outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::GuiQuickbarPlaceholder,
                    packets: outputs.clone(),
                },
            );
            let interleaved_packets = reassembly.interleaved_packets.clone();
            tracing::info!(
                first_sequence,
                current_sequence = reassembly.first_sequence,
                chunks,
                buffered,
                target_length,
                "server GuiQuickbar_SetAllButtons stream buffering continued"
            );
            return Ok(Some(emit_family_packets_with_interleaved(
                VerifiedFamily::GuiQuickbarPlaceholder,
                outputs,
                interleaved_packets,
            )));
        }
    } else {
        let pending_payload = pending.payload.clone();
        let pending_chunks = pending.chunks;
        let pending_len = pending.payload.len();
        let pending_first_sequence = pending.first_sequence;
        let mut rewritten_payload = pending_payload;
        let rewrite = rewrite_quickbar_payload_for_stream(
            &mut rewritten_payload,
            Some(&state.semantic.objects),
            QuickbarRewriteMode::StreamProbe,
        );
        let under_wait_budget = pending_chunks < MAX_PENDING_QUICKBAR_STREAM_CHUNKS
            && pending_len < MAX_PENDING_QUICKBAR_STREAM_BYTES;
        let should_wait = rewrite
            .as_ref()
            .map(|summary| {
                let claimable_minimum_stream =
                    pending_chunks >= 3 && summary.trailing_read_bytes == 0;
                quickbar::rewrite_summary_needs_more_quickbar_bytes(summary)
                    && under_wait_budget
                    && !claimable_minimum_stream
            })
            .unwrap_or(under_wait_budget);
        if let Some(summary) = rewrite.as_ref() {
            observe_quickbar_stream_probe_summary(state, summary);
        }
        if should_wait {
            let buffered = pending_len;
            let chunks = pending_chunks;
            let first_sequence = pending_first_sequence;
            let outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::GuiQuickbarPlaceholder,
                    packets: outputs.clone(),
                },
            );
            let interleaved_packets = reassembly.interleaved_packets.clone();
            tracing::info!(
                first_sequence,
                current_sequence = reassembly.first_sequence,
                chunks,
                buffered,
                "server GuiQuickbar_SetAllButtons continuation buffering continued"
            );
            return Ok(Some(emit_family_packets_with_interleaved(
                VerifiedFamily::GuiQuickbarPlaceholder,
                outputs,
                interleaved_packets,
            )));
        }
    }

    flush_pending_server_quickbar_stream(state, reassembly, source_compressed_length)
}

pub(super) fn force_flush_pending_server_quickbar_stream(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
) -> anyhow::Result<Option<Emit>> {
    let Some(pending_first_sequence) = state
        .quickbar
        .pending_stream
        .as_ref()
        .map(|pending| pending.first_sequence)
    else {
        return Ok(None);
    };
    if pending_first_sequence != reassembly.first_sequence {
        return Ok(None);
    }
    claim_server_zlib_stream_owner(state, ContinuationOwner::GuiQuickbar);
    let pending = state
        .quickbar
        .pending_stream
        .as_mut()
        .expect("pending quickbar stream checked above");
    pending.duplicate_replays = pending.duplicate_replays.saturating_add(1);
    let duplicate_probe_summary = if pending.chunks >= 3
        && pending.duplicate_replays >= MAX_PENDING_QUICKBAR_DUPLICATE_REPLAYS_BEFORE_CLAIM
    {
        let mut probe = pending.payload.clone();
        rewrite_quickbar_payload_for_stream(
            &mut probe,
            Some(&state.semantic.objects),
            QuickbarRewriteMode::StreamProbe,
        )
    } else {
        None
    };
    let claimable_after_duplicate_grace = duplicate_probe_summary.is_some();
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
    let outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
    remember_completed_server_stream_window(
        state,
        reassembly,
        source_compressed_length,
        CompletedDeflatedReplay::VerifiedPackets {
            family: VerifiedFamily::GuiQuickbarPlaceholder,
            packets: outputs.clone(),
        },
    );
    let interleaved_packets = reassembly.interleaved_packets.clone();
    Ok(Some(emit_family_packets_with_interleaved(
        VerifiedFamily::GuiQuickbarPlaceholder,
        outputs,
        interleaved_packets,
    )))
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
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            tracing::warn!(
                error = %err,
                path = %parent.display(),
                buffered = pending.payload.len(),
                "failed to create quickbar dump directory"
            );
            return;
        }
    }
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
    let quickbar_rewrite = rewrite_quickbar_payload_for_stream(
        &mut quickbar_payload,
        Some(&state.semantic.objects),
        QuickbarRewriteMode::Committed,
    );
    let trailing_bytes = pending
        .target_length
        .map(|target_length| pending.payload.len().saturating_sub(target_length))
        .unwrap_or(0);
    if quickbar_rewrite.is_none() {
        dump_quickbar_payload_for_diagnostics("quarantined_stream", &quickbar_payload);
        let outputs = build_blank_quickbar_placeholder_frames(reassembly)?;
        remember_completed_server_stream_window(
            state,
            reassembly,
            source_compressed_length,
            CompletedDeflatedReplay::VerifiedPackets {
                family: VerifiedFamily::GuiQuickbarPlaceholder,
                packets: outputs.clone(),
            },
        );
        let interleaved_packets = reassembly.interleaved_packets.clone();
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
        return Ok(Some(emit_family_packets_with_interleaved(
            VerifiedFamily::GuiQuickbarPlaceholder,
            outputs,
            interleaved_packets,
        )));
    }
    let compressed = deflate_zlib(&quickbar_payload)?;
    let mut combined = Vec::with_capacity(CNW_LENGTH_BYTES + compressed.len());
    combined.extend_from_slice(&(quickbar_payload.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);
    let mut outputs = build_server_deflated_output_frames(reassembly, &combined, 0x01, true)?;
    let inserted_extra_output_frames = outputs.len() > reassembly.frames.len();
    if inserted_extra_output_frames {
        super::pre_shift_current_server_packets(state, &mut outputs)?;
        super::record_extra_deflated_output_sequence_shift(state, reassembly, outputs.len())?;
    }
    observe_committed_quickbar_stream_payload(state, &quickbar_payload);
    remember_completed_server_stream_window_with_disposition(
        state,
        reassembly,
        source_compressed_length,
        inserted_extra_output_frames,
        CompletedDeflatedReplay::VerifiedPackets {
            family: VerifiedFamily::GuiQuickbar,
            packets: outputs.clone(),
        },
    );
    let interleaved_packets = reassembly.interleaved_packets.clone();

    if let Some(summary) = quickbar_rewrite.as_ref() {
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
            slot_records_owned = summary.slot_records_owned,
            spells_preserved = summary.spells_preserved,
            general_buttons_preserved = summary.general_buttons_preserved,
            item_buttons_blanked = summary.item_buttons_blanked,
            item_objects_preserved_by_explicit_self_materialization =
                summary.item_objects_preserved_by_explicit_self_materialization,
            item_objects_preserved_by_active_state = summary.item_objects_preserved_by_active_state,
            item_objects_preserved_by_feature25_first =
                summary.item_objects_preserved_by_feature25_first,
            item_objects_preserved_by_feature25_second =
                summary.item_objects_preserved_by_feature25_second,
            item_objects_preserved_by_feature25_legacy_tail =
                summary.item_objects_preserved_by_feature25_legacy_tail,
            item_buttons_rejected_missing_state_unknown =
                summary.item_buttons_rejected_missing_state_unknown,
            item_buttons_rejected_missing_state_cleared_delete =
                summary.item_buttons_rejected_missing_state_cleared_delete,
            item_buttons_rejected_missing_state_cleared_area_reset =
                summary.item_buttons_rejected_missing_state_cleared_area_reset,
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
    if inserted_extra_output_frames {
        if !interleaved_packets.is_empty() {
            anyhow::bail!(
                "expanded quickbar stream output with interleaved packets needs a pre-shifted mixed proof"
            );
        }
        Ok(Some(Emit::VerifiedPacketsPreShifted {
            family: VerifiedFamily::GuiQuickbar,
            packets: outputs,
        }))
    } else {
        Ok(Some(emit_family_packets_with_interleaved(
            VerifiedFamily::GuiQuickbar,
            outputs,
            interleaved_packets,
        )))
    }
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
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(%error, path = %parent.display(), "failed to create quickbar dump directory");
            return;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crc::{encode_legacy_m_crc, write_be_u16},
        packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET,
        translate::module_resources,
    };

    use super::super::reassembly::BufferedFrame;

    #[test]
    fn committed_quickbar_stream_payload_updates_semantic_profile() {
        let mut state = SessionState::new(
            module_resources::ModuleResourceRuntime::default(),
            true,
            None,
        );
        let payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should be exact EE shape");

        assert_eq!(
            state
                .semantic
                .ui
                .quickbar_item_refresh_harness_idle_reason(),
            "no_committed_quickbar_profile"
        );

        observe_committed_quickbar_stream_payload(&mut state, &payload);

        assert_eq!(state.semantic.ui.quickbar_packets, 1);
        let profile = state
            .semantic
            .ui
            .last_committed_quickbar_profile
            .expect("committed stream payload should update quickbar profile");
        assert_eq!(profile.slot_records, 36);
        assert_eq!(profile.blank_slots, 36);
        assert_eq!(profile.item_slots, 0);
        assert_eq!(
            state
                .semantic
                .ui
                .quickbar_item_refresh_harness_idle_reason(),
            "no_post_committed_item_context"
        );
    }

    #[test]
    fn expanded_quickbar_flush_caches_pre_shifted_replay_disposition() {
        let mut read_buffer = Vec::with_capacity(4_160);
        read_buffer.push(11);
        read_buffer.extend_from_slice(b"long_general_ref");
        read_buffer.extend_from_slice(&4096u32.to_le_bytes());
        let mut value = 0xC0DE_1234u32;
        for _ in 0..4096 {
            value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            read_buffer.push((value >> 24) as u8);
        }
        read_buffer.extend(std::iter::repeat(0).take(35));
        let declared = 3usize + 4 + read_buffer.len();
        let mut payload = vec![b'P', 0x1E, 0x01];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&read_buffer);
        // General slots consume no shared BOOLs, so the fragment buffer only
        // carries its three-bit valid-length prefix.
        payload.push(0x60);
        assert!(quickbar::ee_set_all_buttons_payload_shape_valid(&payload));

        let compressed_chunk = vec![0x78, 0x9C, 0x01, 0x02];
        let mut packet = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + compressed_chunk.len()];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, 200));
        assert!(write_be_u16(&mut packet, 5, 90));
        packet[7] = 0x0D;
        assert!(write_be_u16(&mut packet, 8, 1));
        assert!(write_be_u16(&mut packet, 10, compressed_chunk.len() as u16));
        assert!(encode_legacy_m_crc(&mut packet));
        let reassembly = ServerDeflatedReassembly {
            inflated_length: payload.len(),
            expected_frames: 1,
            first_sequence: 200,
            server_origin_generation: 3,
            packetized_sequence: 1,
            zlib_stream: true,
            frames: vec![BufferedFrame {
                packet,
                payload_length: compressed_chunk.len(),
                sequence: 200,
                server_peer_ack_sequence: 90,
                ack_sequence: 90,
                compressed_chunk: compressed_chunk.clone(),
            }],
            interleaved_packets: Vec::new(),
            interleaved_events: Vec::new(),
        };
        let mut state = SessionState::default();
        state.quickbar.pending_stream = Some(PendingQuickbarStream {
            payload: payload.clone(),
            target_length: Some(payload.len()),
            fragment_wait: false,
            first_sequence: reassembly.first_sequence,
            chunks: 1,
            duplicate_replays: 0,
        });

        let emit =
            flush_pending_server_quickbar_stream(&mut state, &reassembly, compressed_chunk.len())
                .expect("large typed quickbar should flush")
                .expect("quickbar flush should emit");
        let Emit::VerifiedPacketsPreShifted { family, packets } = emit else {
            panic!("expanded quickbar flush must report pre-shifted output");
        };
        assert_eq!(family, VerifiedFamily::GuiQuickbar);
        assert!(packets.len() > reassembly.frames.len());
        let cached = state
            .deflate
            .completed_server_stream_windows
            .last()
            .expect("quickbar flush should cache its replay disposition");
        assert!(cached.pre_shifted);
        assert_eq!(cached.compressed, compressed_chunk);
    }
}
