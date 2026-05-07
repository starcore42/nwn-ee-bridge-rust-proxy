//! M-frame routing for live-object update payload rewrites.
//!
//! This module is intentionally small: it extracts a direct `M` gameplay
//! payload, delegates semantic `U` record translation to
//! `translate::live_object_update`, then repairs only the M-frame payload
//! length and legacy CRC. It should not grow packet semantics of its own.

use crate::translate::live_object_update;

use super::{parse_window, MFrameView};

pub type RewriteSummary = live_object_update::LiveObjectUpdateRewriteSummary;
pub type ClaimSummary = live_object_update::LiveObjectUpdateClaimSummary;

pub fn rewrite_payload_if_needed(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    live_object_update::rewrite_update_records_payload_if_possible(payload)
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClaimSummary> {
    live_object_update::claim_payload_if_verified(payload)
}

pub fn rewrite_direct_frame_if_needed(
    bytes: &[u8],
    view: &MFrameView,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(high) = view.high else {
        return Ok(None);
    };
    if high.major != 0x05 || high.minor != 0x01 || view.payload_length == 0 {
        return Ok(None);
    }

    let Some(payload) = parse_window::primary_payload(bytes, view) else {
        return Ok(None);
    };

    let mut rewritten_payload = payload.to_vec();
    let Some(summary) = rewrite_payload_if_needed(&mut rewritten_payload) else {
        return Ok(None);
    };
    let rewritten = parse_window::replace_primary_payload_and_repair(
        bytes,
        view,
        &rewritten_payload,
        "GameObjUpdate_LiveObject",
    )?;

    tracing::info!(
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
        fragment_bits_trimmed = summary.fragment_bits_trimmed,
        world_status_records_normalized = summary.world_status_records_normalized,
        "server GameObjUpdate_LiveObject update records rewritten for EE"
    );
    Ok(Some(rewritten))
}
