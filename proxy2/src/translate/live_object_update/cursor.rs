//! Fragment-bit cursor advancement for live-object submessages.
//!
//! Cursor advancement is deliberately separate from update-record rewriting:
//! add/delete records are not translated here, but they still consume fragment
//! BOOLs and therefore affect whether the shared tail can be trimmed safely.

use super::{
    DOOR_OBJECT_TYPE, LEGACY_UPDATE_HEADER_BYTES, PLACEABLE_OBJECT_TYPE, TRIGGER_OBJECT_TYPE,
    boundary, creature, locstring, read_u32_le, trigger,
};

pub(super) fn advance_live_add_record_bit_cursor(
    bytes: &[u8],
    bits: &[bool],
    record_offset: usize,
    record_end: usize,
    bit_cursor: &mut usize,
) -> bool {
    if record_offset + 6 > record_end || record_end > bytes.len() {
        return false;
    }

    match bytes[record_offset + 1] {
        0x05 => true,
        TRIGGER_OBJECT_TYPE => trigger::advance_trigger_add_bit_cursor(
            bytes,
            record_offset,
            record_end,
            bits,
            bit_cursor,
        ),
        DOOR_OBJECT_TYPE => {
            advance_door_add_bit_cursor(bytes, bits, record_offset, record_end, bit_cursor)
        }
        PLACEABLE_OBJECT_TYPE => {
            advance_placeable_add_bit_cursor(bytes, record_offset, record_end, bits, bit_cursor)
        }
        _ => false,
    }
}

pub(super) fn advance_legacy_add_record_bit_cursor_for_update_pass(
    bytes: &[u8],
    bits: &[bool],
    record_offset: usize,
    record_end: usize,
    bit_cursor: &mut usize,
) -> bool {
    if record_offset + 6 > record_end || record_end > bytes.len() || *bit_cursor >= bits.len() {
        return false;
    }

    match bytes[record_offset + 1] {
        0x05 => true,
        TRIGGER_OBJECT_TYPE => trigger::advance_trigger_add_bit_cursor(
            bytes,
            record_offset,
            record_end,
            bits,
            bit_cursor,
        ),
        DOOR_OBJECT_TYPE => {
            let Some(first_dword) = read_u32_le(bytes, record_offset + 6) else {
                return false;
            };
            let visual_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };
            let name_offset = if creature::has_ee_identity_visual_transform_map_at(
                bytes,
                visual_offset,
                record_end,
            ) {
                visual_offset + 40
            } else {
                visual_offset
            };
            if name_offset > record_end {
                return false;
            }
            let source_inner_bits = usize::from(
                locstring::inline_cexo_string_end(bytes, name_offset).is_some()
                    && bits.get(*bit_cursor).copied().unwrap_or(false)
                    && !bits.get(*bit_cursor + 1).copied().unwrap_or(false),
            );
            let consumed = 6usize.saturating_add(source_inner_bits);
            if bits.len().saturating_sub(*bit_cursor) < consumed {
                return false;
            }
            *bit_cursor = bit_cursor.saturating_add(consumed);
            true
        }
        PLACEABLE_OBJECT_TYPE => {
            let name_offset = record_offset + 6;
            let source_inner_bits = usize::from(
                locstring::inline_cexo_string_end(bytes, name_offset).is_some()
                    && bits.get(*bit_cursor).copied().unwrap_or(false),
            );
            let consumed = 10usize.saturating_add(source_inner_bits);
            if bits.len().saturating_sub(*bit_cursor) < consumed {
                return false;
            }
            *bit_cursor = bit_cursor.saturating_add(consumed);
            true
        }
        _ => false,
    }
}

fn advance_door_add_bit_cursor(
    bytes: &[u8],
    bits: &[bool],
    record_offset: usize,
    record_end: usize,
    bit_cursor: &mut usize,
) -> bool {
    let Some(first_dword) = read_u32_le(bytes, record_offset + 6) else {
        return false;
    };
    let visual_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };
    let name_offset =
        if creature::has_ee_identity_visual_transform_map_at(bytes, visual_offset, record_end) {
            visual_offset + 40
        } else {
            visual_offset
        };
    if name_offset > record_end || *bit_cursor >= bits.len() {
        return false;
    }

    let source_inner_bits = usize::from(bits[*bit_cursor]);
    if source_inner_bits != 0 && bits.get(*bit_cursor + 1).copied().unwrap_or(true) {
        return false;
    }
    *bit_cursor = bit_cursor.saturating_add(6 + source_inner_bits);
    *bit_cursor <= bits.len()
}

fn advance_placeable_add_bit_cursor(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if record_offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return false;
    }
    if *bit_cursor >= bits.len() {
        return false;
    }
    let name_offset = record_offset + LEGACY_UPDATE_HEADER_BYTES - 4;
    if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
        if inline_end > name_offset + 4
            && bits[*bit_cursor]
            && bits.get(*bit_cursor + 1).copied().unwrap_or(true)
        {
            return false;
        }
    }
    let dest_inner_bits = usize::from(bits[*bit_cursor]);
    *bit_cursor = bit_cursor.saturating_add(11 + dest_inner_bits);
    *bit_cursor <= bits.len()
}

pub(super) fn legacy_live_delete_fragment_bit_count(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    if record_end != record_offset + 6
        || record_end > bytes.len()
        || bytes.get(record_offset).copied() != Some(b'D')
        || !boundary::looks_like_legacy_live_object_id_at(bytes, record_offset + 2)
    {
        return None;
    }

    match bytes[record_offset + 1] {
        0x05 | 0x06 | 0x09 => Some(1),
        0x07 | 0x0A => Some(0),
        _ => None,
    }
}
