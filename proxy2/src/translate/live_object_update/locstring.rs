//! Live-object inline string and locstring boundary helpers.
//!
//! Name payload handling is a separate concern from door/placeable transform
//! bytes. Keeping it here prevents door/placeable modules from growing string
//! cursor heuristics.

use super::{
    door_placeable_update_name_cursor, read_u32_le, LEGACY_UPDATE_HEADER_BYTES,
    MAX_LIVE_OBJECT_NAME_BYTES,
};

pub(super) fn legacy_live_update_name_payload_ready(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> bool {
    if record_offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return false;
    }
    let Some(raw_mask) = read_u32_le(bytes, record_offset + 6) else {
        return false;
    };
    let legacy_name_offset = door_placeable_update_name_cursor(record_offset, raw_mask);
    inline_cexo_string_end(bytes, legacy_name_offset)
        .map(|end| end <= record_end)
        .unwrap_or(false)
}

pub(super) fn candidate_inside_inline_string(bytes: &[u8], search_start: usize, candidate: usize) -> bool {
    let mut string_offset = search_start;
    while string_offset + 4 <= candidate && string_offset < bytes.len() {
        if let Some(string_end) = inline_cexo_string_end(bytes, string_offset) {
            if string_offset + 4 <= candidate && candidate < string_end {
                return true;
            }
        }
        string_offset += 1;
    }
    false
}

pub(super) fn inline_cexo_string_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let length = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    if length > MAX_LIVE_OBJECT_NAME_BYTES || bytes.len().saturating_sub(offset + 4) < length {
        return None;
    }
    let text_start = offset + 4;
    let end = text_start + length;
    if bytes[text_start..end]
        .iter()
        .all(|byte| matches!(*byte, 0x20..=0x7E | b'\t'))
    {
        Some(end)
    } else {
        None
    }
}

