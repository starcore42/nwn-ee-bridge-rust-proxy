//! M-frame routing for live-object update payload rewrites.
//!
//! This module is intentionally small: it extracts a direct `M` gameplay
//! payload, delegates semantic `U` record translation to
//! `translate::live_object_update`, then repairs only the M-frame payload
//! length and legacy CRC. It should not grow packet semantics of its own.

use crate::translate::live_object_update;

use super::{
    encode_legacy_m_crc, write_be_u16, MFrameView, LEGACY_GAMEPLAY_PAYLOAD_OFFSET,
};

pub type RewriteSummary = live_object_update::LiveObjectUpdateRewriteSummary;

pub fn rewrite_payload_if_needed(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    live_object_update::rewrite_update_records_payload_if_possible(payload)
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

    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let payload_end = payload_start + view.payload_length;
    let Some(payload) = bytes.get(payload_start..payload_end) else {
        return Ok(None);
    };

    let mut rewritten_payload = payload.to_vec();
    let Some(summary) = rewrite_payload_if_needed(&mut rewritten_payload) else {
        return Ok(None);
    };
    if rewritten_payload.len() > u16::MAX as usize {
        anyhow::bail!("GameObjUpdate_LiveObject update payload too large");
    }

    let mut rewritten = bytes[..payload_start].to_vec();
    write_be_u16(&mut rewritten, 10, rewritten_payload.len() as u16)
        .then_some(())
        .ok_or_else(|| {
            anyhow::anyhow!("failed to update GameObjUpdate_LiveObject payload length")
        })?;
    rewritten.extend_from_slice(&rewritten_payload);
    if let Some(trailing) = bytes.get(payload_end..) {
        rewritten.extend_from_slice(trailing);
    }
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair GameObjUpdate_LiveObject CRC"))?;

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
        world_status_records_normalized = summary.world_status_records_normalized,
        "server GameObjUpdate_LiveObject update records rewritten for EE"
    );
    Ok(Some(rewritten))
}
