//! Bounded legacy live-object update readers.

use super::{read_f32_le, read_u16_le, read_u32_le, MAX_LIVE_OBJECT_NAME_BYTES};

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyNamedUpdateTail {
    pub(super) facing: u16,
    pub(super) scale_raw: u32,
    pub(super) generic_state_word: u16,
}

pub(super) fn read_legacy_named_update_tail9(
    bytes: &[u8],
    offset: usize,
    require_small_state_byte: bool,
) -> Option<LegacyNamedUpdateTail> {
    let facing = read_u16_le(bytes, offset)?;
    let state_byte = *bytes.get(offset + 2)?;
    if require_small_state_byte && state_byte > 10 {
        return None;
    }
    let scale_raw = read_u32_le(bytes, offset + 3)?;
    let scale = read_f32_le(bytes, offset + 3)?;
    let generic_state_word = read_u16_le(bytes, offset + 7)?;
    if !is_plausible_legacy_object_scale(scale) {
        return None;
    }
    Some(LegacyNamedUpdateTail {
        facing,
        scale_raw,
        generic_state_word,
    })
}

pub(super) fn legacy_named_update_tail_following_payload_ready(
    bytes: &[u8],
    tail_offset: usize,
    record_end: usize,
) -> bool {
    if tail_offset > record_end || record_end > bytes.len() || record_end - tail_offset < 13 {
        return false;
    }

    let name_offset = tail_offset + 9;
    if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
        return inline_end <= record_end && record_end - inline_end <= 4;
    }

    if read_u32_le(bytes, name_offset) == Some(0) && name_offset + 4 < record_end {
        let text_start = name_offset + 4;
        let text_length = record_end - text_start;
        if (1..=MAX_LIVE_OBJECT_NAME_BYTES).contains(&text_length)
            && bytes[text_start..record_end]
                .iter()
                .all(|byte| matches!(*byte, 0x20..=0x7E | b'\t'))
        {
            return true;
        }
    }

    record_end.saturating_sub(name_offset + 4) <= 4
}

fn inline_cexo_string_end(bytes: &[u8], offset: usize) -> Option<usize> {
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

fn is_plausible_legacy_object_scale(scale: f32) -> bool {
    scale.is_finite() && (0.01..=100.0).contains(&scale)
}
