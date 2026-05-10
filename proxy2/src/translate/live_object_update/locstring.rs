//! Live-object inline string and locstring boundary helpers.
//!
//! Name payload handling is a separate concern from door/placeable transform
//! bytes. Keeping it here prevents door/placeable modules from growing string
//! cursor heuristics.

use super::{
    LEGACY_UPDATE_HEADER_BYTES, MAX_LIVE_OBJECT_NAME_BYTES, read_u32_le,
    record::door_placeable_legacy_inline_name_cursor,
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
    let legacy_name_offset = door_placeable_legacy_inline_name_cursor(record_offset, raw_mask);
    inline_cexo_string_end(bytes, legacy_name_offset)
        .map(|end| end <= record_end)
        .unwrap_or(false)
}

pub(super) fn candidate_inside_inline_string(
    bytes: &[u8],
    search_start: usize,
    candidate: usize,
) -> bool {
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

pub(super) fn tlk_locstring_ref_end(bytes: &[u8], offset: usize) -> Option<usize> {
    const TLK_LOCSTRING_REF_BYTES: usize = 1 + 4;
    if bytes.len().saturating_sub(offset) < TLK_LOCSTRING_REF_BYTES {
        return None;
    }

    // EE's shared client locstring reader (`sub_1409735F0`) consumes an inner
    // BOOL. When that BOOL is true it reads `ReadBYTE(1, 1)` followed by
    // `ReadDWORD(32)` instead of an inline CExoString. The byte is the observed
    // client TLK selector in HG captures; keeping it to 0/1 avoids turning this
    // into an arbitrary non-string byte escape hatch.
    if matches!(bytes[offset], 0 | 1) && read_u32_le(bytes, offset + 1).is_some() {
        Some(offset + TLK_LOCSTRING_REF_BYTES)
    } else {
        None
    }
}
