//! Reliable `M` payload-window helpers.
//!
//! These helpers perform small, mechanical M-frame operations: find the
//! primary high-level payload, replace it, repair the packetized length, and
//! repair the legacy M CRC. They do not inspect gameplay semantics.

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
};

pub(super) fn primary_payload<'a>(bytes: &'a [u8], view: &MFrameView) -> Option<&'a [u8]> {
    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let payload_end = payload_start.checked_add(view.payload_length)?;
    bytes.get(payload_start..payload_end)
}

pub(super) fn replace_primary_payload_and_repair(
    bytes: &[u8],
    view: &MFrameView,
    rewritten_payload: &[u8],
    context: &'static str,
) -> anyhow::Result<Vec<u8>> {
    if rewritten_payload.len() > u16::MAX as usize {
        anyhow::bail!("{context} payload too large");
    }

    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let payload_end = payload_start
        .checked_add(view.payload_length)
        .ok_or_else(|| anyhow::anyhow!("{context} payload window overflow"))?;
    if payload_end > bytes.len() {
        anyhow::bail!("{context} payload window outside frame");
    }

    let mut rewritten = bytes[..payload_start].to_vec();
    write_be_u16(&mut rewritten, 10, rewritten_payload.len() as u16)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to update {context} payload length"))?;
    rewritten.extend_from_slice(rewritten_payload);
    if let Some(trailing) = bytes.get(payload_end..) {
        rewritten.extend_from_slice(trailing);
    }
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair {context} CRC"))?;
    Ok(rewritten)
}
