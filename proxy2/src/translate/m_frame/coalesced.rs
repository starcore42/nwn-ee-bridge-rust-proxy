//! Coalesced reliable-window span handling.
//!
//! This module owns packetized trailing-span mechanics for bundled server
//! `M` records. It may unwrap/repack a coalesced deflated span, but gameplay
//! meaning must remain delegated to the focused semantic translators.

use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{
        HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView, parse_packetized_spans,
    },
};

use super::{
    deflate::deflate_zlib, hex_prefix, inflated_cnw_fragment_offset_valid,
    reassembly::{self, InflatedGameplayPayload}, server_dispatch, SessionState, CNW_LENGTH_BYTES,
};

pub(super) fn rewrite_server_window_spans_if_needed(
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
        } = reassembly::inflate_gameplay_payload(
            compressed,
            deflated.inflated_length,
            (span.flags & 0x01) != 0,
            &mut state.server_zlib_inflater,
        )?;

        let live_object_continuation_wrapped =
            server_dispatch::wrap_legacy_live_object_continuation_if_needed(&mut inflated);
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

        let mut semantic_rewrite_summary = server_dispatch::rewrite_inflated_payload_for_ee(
            &mut inflated,
            Some(&state.latest_area_placeables),
            server_dispatch::SemanticScope::CoalescedSpan,
        );
        if live_object_continuation_wrapped {
            semantic_rewrite_summary.note_rewrite("GameObjUpdate_LiveObjectContinuation");
        }
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

        let semantic_rewrite = semantic_rewrite_summary.any_rewrite();
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

        if semantic_rewrite {
            tracing::info!(
                offset = span.offset,
                families = ?semantic_rewrite_summary,
                used_server_stream,
                "server coalesced semantic payload rewritten for EE"
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

