//! Coalesced reliable-window span handling.
//!
//! This module owns packetized trailing-span mechanics for bundled server
//! `M` records. It may unwrap/repack a coalesced deflated span, but gameplay
//! meaning must remain delegated to the focused semantic translators.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{
        DeflatedEnvelope, HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView,
        parse_packetized_spans,
    },
    translate::{ContinuationOwner, VerifiedFamily, VerifiedProof},
};

use super::{
    CNW_LENGTH_BYTES, SessionState,
    deflate::{deflate_zlib, looks_like_zlib_wrapped_deflate},
    hex_prefix, inflated_cnw_fragment_offset_valid,
    queue_area_client_area_side_effects_after_sequence,
    reassembly::{self, InflatedGameplayPayload},
    server_dispatch,
    state::CompletedCoalescedStreamRecord,
};

pub(super) fn rewrite_server_window_spans_if_needed(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
) -> anyhow::Result<Option<(VerifiedProof, Vec<u8>)>> {
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

    let mut rewritten = Vec::new();
    let mut changed = false;
    let mut dropped_spans = 0u32;
    let mut rewritten_deflated_spans = 0u32;
    let mut record_proofs = Vec::new();

    let primary_record = &bytes[..primary_len];
    let primary = rewrite_coalesced_record_for_ee(
        primary_record,
        view.flags,
        view.high,
        view.deflated.as_ref(),
        view.payload_length,
        state,
        view.sequence,
        view.ack_sequence,
        0,
    )?;
    changed |= primary.changed;
    if primary.dropped {
        dropped_spans = dropped_spans.saturating_add(1);
    }
    if primary.rewritten_deflated {
        rewritten_deflated_spans = rewritten_deflated_spans.saturating_add(1);
    }
    record_proofs.push(primary.proof.clone());
    if primary.dropped {
        let mut consumed = primary.record;
        encode_legacy_m_crc(&mut consumed)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair consumed coalesced primary CRC"))?;
        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            old_len = bytes.len(),
            new_len = consumed.len(),
            dropped_spans,
            "server coalesced M window consumed because primary semantic record was quarantined"
        );
        return Ok(Some((
            VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
            consumed,
        )));
    }
    rewritten.extend_from_slice(&primary.record);

    for span in spans {
        let record_end = span.offset + span.record_length;
        let record = &bytes[span.offset..record_end];
        let outcome = rewrite_coalesced_record_for_ee(
            record,
            span.flags,
            span.high,
            span.deflated.as_ref(),
            span.payload_length,
            state,
            view.sequence,
            view.ack_sequence,
            span.offset,
        )?;
        changed |= outcome.changed;
        if outcome.dropped {
            dropped_spans = dropped_spans.saturating_add(1);
        }
        if outcome.rewritten_deflated {
            rewritten_deflated_spans = rewritten_deflated_spans.saturating_add(1);
        }
        record_proofs.push(outcome.proof.clone());
        rewritten.extend_from_slice(&outcome.record);
    }

    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair coalesced M CRC"))?;
    tracing::info!(
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        changed,
        rewritten_deflated_spans,
        dropped_spans,
        "server coalesced M window spans rewritten for strict EE delivery"
    );
    Ok(Some((
        VerifiedProof::CoalescedWindow(record_proofs),
        rewritten,
    )))
}

struct CoalescedRecordRewrite {
    record: Vec<u8>,
    proof: VerifiedProof,
    changed: bool,
    dropped: bool,
    rewritten_deflated: bool,
}

fn replay_completed_coalesced_stream_record(
    state: &SessionState,
    sequence: u16,
    offset: usize,
    payload_length: usize,
    inflated_length: usize,
    compressed: &[u8],
) -> Option<CoalescedRecordRewrite> {
    let entry = state
        .deflate
        .completed_coalesced_stream_records
        .iter()
        .find(|entry| {
            entry.sequence == sequence
                && entry.offset == offset
                && entry.payload_length == payload_length
                && entry.inflated_length == inflated_length
                && entry.compressed.as_slice() == compressed
        })?;

    tracing::info!(
        sequence,
        offset,
        payload_length,
        inflated_length,
        proof = entry.proof.as_str(),
        dropped = entry.dropped,
        rewritten_deflated = entry.rewritten_deflated,
        "server coalesced zlib-stream record replayed from typed cache without re-inflating duplicate"
    );
    Some(CoalescedRecordRewrite {
        record: entry.record.clone(),
        proof: entry.proof.clone(),
        changed: true,
        dropped: entry.dropped,
        rewritten_deflated: entry.rewritten_deflated,
    })
}

fn remember_completed_coalesced_stream_record(
    state: &mut SessionState,
    sequence: u16,
    offset: usize,
    payload_length: usize,
    inflated_length: usize,
    compressed: &[u8],
    outcome: &CoalescedRecordRewrite,
) {
    let entry = CompletedCoalescedStreamRecord {
        sequence,
        offset,
        payload_length,
        inflated_length,
        compressed: compressed.to_vec(),
        proof: outcome.proof.clone(),
        record: outcome.record.clone(),
        dropped: outcome.dropped,
        rewritten_deflated: outcome.rewritten_deflated,
    };

    if let Some(existing) = state
        .deflate
        .completed_coalesced_stream_records
        .iter_mut()
        .find(|existing| {
            existing.sequence == sequence
                && existing.offset == offset
                && existing.payload_length == payload_length
                && existing.inflated_length == inflated_length
                && existing.compressed.as_slice() == compressed
        })
    {
        *existing = entry;
        return;
    }

    const MAX_COMPLETED_COALESCED_STREAM_RECORDS: usize = 64;
    state.deflate.completed_coalesced_stream_records.push(entry);
    if state.deflate.completed_coalesced_stream_records.len()
        > MAX_COMPLETED_COALESCED_STREAM_RECORDS
    {
        let overflow = state.deflate.completed_coalesced_stream_records.len()
            - MAX_COMPLETED_COALESCED_STREAM_RECORDS;
        state
            .deflate
            .completed_coalesced_stream_records
            .drain(0..overflow);
    }
}

fn rewrite_coalesced_record_for_ee(
    record: &[u8],
    flags: u8,
    high: Option<HighLevel>,
    deflated: Option<&DeflatedEnvelope>,
    payload_length: usize,
    state: &mut SessionState,
    sequence: u16,
    ack_sequence: u16,
    offset: usize,
) -> anyhow::Result<CoalescedRecordRewrite> {
    if payload_length == 0 {
        return Ok(CoalescedRecordRewrite {
            record: record.to_vec(),
            proof: VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
            changed: false,
            dropped: false,
            rewritten_deflated: false,
        });
    }

    let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + payload_length;
    let Some(payload) = record.get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end) else {
        return consume_coalesced_record(record, offset, "payload-overflow");
    };

    if let Some(high) = high {
        let mut payload = payload.to_vec();
        let semantic_rewrite_summary = server_dispatch::rewrite_inflated_payload_for_ee(
            &mut payload,
            Some(&state.area_context.latest_area_placeables),
            server_dispatch::SemanticScope::CoalescedSpan,
            Some(&state.module_resources),
            None,
        );
        if semantic_rewrite_summary.should_quarantine()
            || !semantic_rewrite_summary.any_rewrite()
            || payload.len() > u16::MAX as usize
        {
            tracing::warn!(
                offset,
                payload_length,
                major = high.major,
                minor = high.minor,
                name = high.name(),
                known = high.is_known(),
                prefix = %hex_prefix(record, 32),
                "server coalesced M record quarantined: semantic translator did not claim high-level payload"
            );
            return consume_coalesced_record(record, offset, "unclaimed-high-level");
        }
        if let Some(summary) = semantic_rewrite_summary.area_rewrite.as_ref() {
            state.area_context.latest_area_placeables = summary.placeable_context.clone();
            queue_area_client_area_side_effects_after_sequence(
                state,
                sequence,
                ack_sequence,
                summary,
            )?;
        }

        let mut out_record = record[..LEGACY_GAMEPLAY_PAYLOAD_OFFSET].to_vec();
        write_be_u16(&mut out_record, 10, payload.len() as u16)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update coalesced direct record length"))?;
        out_record.extend_from_slice(&payload);
        let changed = out_record.as_slice() != record;
        tracing::info!(
            offset,
            name = high.name(),
            major = high.major,
            minor = high.minor,
            old_payload_length = payload_length,
            new_payload_length = payload.len(),
            changed,
            "server coalesced direct high-level record semantically claimed for EE"
        );
        return Ok(CoalescedRecordRewrite {
            record: out_record,
            proof: semantic_rewrite_summary.verified_proof(),
            changed,
            dropped: false,
            rewritten_deflated: false,
        });
    }

    let Some(deflated) = deflated else {
        tracing::warn!(
            offset,
            payload_length,
            prefix = %hex_prefix(record, 32),
            "server coalesced M record quarantined: unknown non-deflated payload"
        );
        return consume_coalesced_record(record, offset, "unknown-non-deflated");
    };

    if !deflated.plausible || payload_length < CNW_LENGTH_BYTES {
        tracing::warn!(
            offset,
            payload_length,
            inflated_length = deflated.inflated_length,
            prefix = %hex_prefix(record, 32),
            "server coalesced M deflated record quarantined: implausible envelope"
        );
        return consume_coalesced_record(record, offset, "implausible-deflated-envelope");
    }

    let compressed = &payload[CNW_LENGTH_BYTES..];
    let stream_payload = (flags & 0x01) != 0 && !looks_like_zlib_wrapped_deflate(compressed);
    if stream_payload {
        if let Some(replay) = replay_completed_coalesced_stream_record(
            state,
            sequence,
            offset,
            payload_length,
            deflated.inflated_length,
            compressed,
        ) {
            return Ok(replay);
        }
    }
    let InflatedGameplayPayload {
        bytes: mut inflated,
        used_server_stream,
    } = reassembly::inflate_gameplay_payload(
        compressed,
        deflated.inflated_length,
        (flags & 0x01) != 0,
        &mut state.deflate.server_zlib_inflater,
    )?;

    server_dispatch::wrap_legacy_live_object_continuation_if_needed(&mut inflated);
    if HighLevel::parse(&inflated).is_none() {
        if let Some(outcome) = rewrite_coalesced_stream_continuation_for_ee(
            record,
            offset,
            &inflated,
            used_server_stream,
            state,
            sequence,
        )? {
            if used_server_stream {
                remember_completed_coalesced_stream_record(
                    state,
                    sequence,
                    offset,
                    payload_length,
                    deflated.inflated_length,
                    compressed,
                    &outcome,
                );
            }
            return Ok(outcome);
        }
        dump_invalid_inflated_payload_for_span(&inflated, sequence, "coalesced-no-high-level");
        tracing::warn!(
            offset,
            inflated = inflated.len(),
            prefix = %hex_prefix(&inflated, 32),
            used_server_stream,
            "server coalesced M deflated record quarantined: no high-level payload"
        );
        return consume_coalesced_record(record, offset, "deflated-no-high-level");
    }

    let semantic_rewrite_summary = server_dispatch::rewrite_inflated_payload_for_ee(
        &mut inflated,
        Some(&state.area_context.latest_area_placeables),
        server_dispatch::SemanticScope::CoalescedSpan,
        Some(&state.module_resources),
        None,
    );
    if semantic_rewrite_summary.should_quarantine() || !semantic_rewrite_summary.any_rewrite() {
        let reason = semantic_rewrite_summary
            .quarantine_reason
            .unwrap_or("coalesced-untranslated-required-semantic-family");
        dump_invalid_inflated_payload_for_span(&inflated, sequence, reason);
        tracing::warn!(
            offset,
            inflated = inflated.len(),
            reason,
            prefix = %hex_prefix(&inflated, 32),
            "server coalesced M deflated record quarantined: required semantic translation is missing"
        );
        return consume_coalesced_record(record, offset, reason);
    }
    if !inflated_cnw_fragment_offset_valid(&inflated) {
        dump_invalid_inflated_payload_for_span(
            &inflated,
            sequence,
            "coalesced-invalid-cnw-fragment-offset",
        );
        tracing::warn!(
            offset,
            inflated = inflated.len(),
            prefix = %hex_prefix(&inflated, 32),
            "server coalesced M deflated record quarantined: invalid CNW fragment offset"
        );
        return consume_coalesced_record(record, offset, "invalid-cnw-fragment-offset");
    }
    if let Some(summary) = semantic_rewrite_summary.area_rewrite.as_ref() {
        state.area_context.latest_area_placeables = summary.placeable_context.clone();
        queue_area_client_area_side_effects_after_sequence(state, sequence, ack_sequence, summary)?;
    }

    let must_convert_stream = used_server_stream || state.deflate.server_zlib_stream_proxy_owned;
    if used_server_stream {
        state.deflate.server_zlib_stream_proxy_owned = true;
    }

    let rewritten_compressed = deflate_zlib(&inflated)?;
    let new_payload_length = CNW_LENGTH_BYTES + rewritten_compressed.len();
    if new_payload_length > u16::MAX as usize {
        tracing::warn!(
            offset,
            new_payload_length,
            "server coalesced M deflated record quarantined: rewritten payload too large"
        );
        return consume_coalesced_record(record, offset, "rewritten-payload-too-large");
    }

    let mut out_record = record[..LEGACY_GAMEPLAY_PAYLOAD_OFFSET].to_vec();
    if !out_record.is_empty() {
        out_record[7] &= !0x01;
    }
    write_be_u16(&mut out_record, 10, new_payload_length as u16)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to update coalesced deflated record length"))?;
    out_record.extend_from_slice(&(inflated.len() as u32).to_le_bytes());
    out_record.extend_from_slice(&rewritten_compressed);
    let changed = must_convert_stream || out_record.as_slice() != record;
    tracing::info!(
        offset,
        families = ?semantic_rewrite_summary,
        used_server_stream,
        changed,
        "server coalesced deflated record semantically claimed and emitted as EE zlib"
    );

    let outcome = CoalescedRecordRewrite {
        record: out_record,
        proof: semantic_rewrite_summary.verified_proof(),
        changed,
        dropped: false,
        rewritten_deflated: true,
    };
    if used_server_stream {
        remember_completed_coalesced_stream_record(
            state,
            sequence,
            offset,
            payload_length,
            deflated.inflated_length,
            compressed,
            &outcome,
        );
    }
    Ok(outcome)
}

fn rewrite_coalesced_stream_continuation_for_ee(
    record: &[u8],
    offset: usize,
    inflated: &[u8],
    used_server_stream: bool,
    state: &mut SessionState,
    sequence: u16,
) -> anyhow::Result<Option<CoalescedRecordRewrite>> {
    // A no-header inflated chunk is valid only as continuation bytes from an
    // already-classified Diamond server zlib stream. This is deliberately not a
    // raw passthrough: the source stream is consumed by the proxy inflater, and
    // the EE-facing coalesced record is reduced to an empty reliable progress
    // shell with a typed owner/epoch proof.
    if !used_server_stream || !state.deflate.server_zlib_stream_proxy_owned {
        return Ok(None);
    }

    let owner = state
        .deflate
        .server_zlib_stream_owner
        .unwrap_or(ContinuationOwner::UnknownProxyOwned);
    let stream_epoch = state.deflate.server_zlib_stream_epoch;
    if owner == ContinuationOwner::UnknownProxyOwned || stream_epoch == 0 || inflated.is_empty() {
        tracing::warn!(
            offset,
            owner = owner.as_str(),
            stream_epoch,
            continuation_len = inflated.len(),
            "server coalesced zlib-stream continuation rejected: missing known semantic owner"
        );
        return Ok(None);
    }

    dump_invalid_inflated_payload_for_span(
        inflated,
        sequence,
        "claimed-coalesced-zlib-stream-continuation",
    );
    tracing::info!(
        offset,
        sequence,
        owner = owner.as_str(),
        stream_epoch,
        inflated = inflated.len(),
        prefix = %hex_prefix(inflated, 32),
        "server coalesced zlib-stream continuation claimed as proxy-owned semantic stream tail"
    );

    consume_coalesced_record_with_proof(
        record,
        offset,
        "claimed-zlib-stream-continuation",
        VerifiedProof::family(VerifiedFamily::ServerZlibStreamContinuation {
            owner,
            stream_epoch,
            first_sequence: sequence,
        }),
        false,
    )
    .map(Some)
}

fn consume_coalesced_record(
    record: &[u8],
    offset: usize,
    reason: &'static str,
) -> anyhow::Result<CoalescedRecordRewrite> {
    consume_coalesced_record_with_proof(
        record,
        offset,
        reason,
        VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
        true,
    )
}

fn consume_coalesced_record_with_proof(
    record: &[u8],
    offset: usize,
    reason: &'static str,
    proof: VerifiedProof,
    warn: bool,
) -> anyhow::Result<CoalescedRecordRewrite> {
    let mut out_record = record[..LEGACY_GAMEPLAY_PAYLOAD_OFFSET.min(record.len())].to_vec();
    if out_record.len() < LEGACY_GAMEPLAY_PAYLOAD_OFFSET {
        out_record.resize(LEGACY_GAMEPLAY_PAYLOAD_OFFSET, 0);
    }
    if out_record.len() > 7 {
        out_record[7] &= !0x07;
    }
    write_be_u16(&mut out_record, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear quarantined coalesced record length"))?;
    if warn {
        tracing::warn!(
            offset,
            reason,
            old_len = record.len(),
            "server coalesced M record consumed because strict semantic translation is unavailable"
        );
    } else {
        tracing::info!(
            offset,
            reason,
            old_len = record.len(),
            "server coalesced M record consumed as verified empty progress shell"
        );
    }
    Ok(CoalescedRecordRewrite {
        record: out_record,
        proof,
        changed: true,
        dropped: true,
        rewritten_deflated: false,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesced_stream_continuation_requires_known_proxy_owned_owner() {
        let record = [0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        let inflated = [0xEC, 0x00, 0x3C, 0x56, 0xFE, 0x3E];
        let mut state = SessionState::default();
        state.deflate.server_zlib_stream_proxy_owned = true;

        let rejected = rewrite_coalesced_stream_continuation_for_ee(
            &record, 0, &inflated, true, &mut state, 42,
        )
        .expect("continuation helper should not fail");

        assert!(rejected.is_none());
    }

    #[test]
    fn coalesced_stream_continuation_consumes_known_owner_without_raw_emit() {
        let mut record = [0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        record[7] = 0x01;
        let inflated = [0xEC, 0x00, 0x3C, 0x56, 0xFE, 0x3E];
        let mut state = SessionState::default();
        state.deflate.server_zlib_stream_proxy_owned = true;
        state.deflate.server_zlib_stream_owner = Some(ContinuationOwner::GameObjUpdateLiveObject);
        state.deflate.server_zlib_stream_epoch = 9;

        let rewritten = rewrite_coalesced_stream_continuation_for_ee(
            &record, 0, &inflated, true, &mut state, 42,
        )
        .expect("continuation helper should not fail")
        .expect("known proxy-owned stream should be consumed as a typed continuation shell");

        assert!(rewritten.dropped);
        assert!(!rewritten.rewritten_deflated);
        assert_eq!(rewritten.record.len(), LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
        assert_eq!(rewritten.record[10], 0);
        assert_eq!(rewritten.record[11], 0);
        assert_eq!(
            rewritten.proof,
            VerifiedProof::family(VerifiedFamily::ServerZlibStreamContinuation {
                owner: ContinuationOwner::GameObjUpdateLiveObject,
                stream_epoch: 9,
                first_sequence: 42,
            })
        );
    }

    #[test]
    fn coalesced_stream_replay_cache_returns_typed_shell_without_reinflate() {
        let compressed = [0x10, 0x20, 0x30, 0x40];
        let proof = VerifiedProof::family(VerifiedFamily::ServerZlibStreamContinuation {
            owner: ContinuationOwner::GuiQuickbar,
            stream_epoch: 21,
            first_sequence: 34,
        });
        let outcome = CoalescedRecordRewrite {
            record: vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET],
            proof: proof.clone(),
            changed: true,
            dropped: true,
            rewritten_deflated: false,
        };
        let mut state = SessionState::default();

        remember_completed_coalesced_stream_record(
            &mut state,
            34,
            67,
            381,
            604,
            &compressed,
            &outcome,
        );

        let replay =
            replay_completed_coalesced_stream_record(&state, 34, 67, 381, 604, &compressed)
                .expect("matching coalesced stream duplicate should replay cached proof");

        assert!(replay.changed);
        assert!(replay.dropped);
        assert!(!replay.rewritten_deflated);
        assert_eq!(replay.record, outcome.record);
        assert_eq!(replay.proof, proof);
    }
}
