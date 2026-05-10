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
            let legacy_short_name = legacy_short_door_name_token_at(bytes, name_offset, record_end);
            let source_inner_bits = if bits.get(*bit_cursor).copied().unwrap_or(false) {
                if locstring::tlk_locstring_ref_end(bytes, name_offset)
                    .map(|end| end + 2 == record_end)
                    .unwrap_or(false)
                {
                    if !bits.get(*bit_cursor + 1).copied().unwrap_or(false) {
                        return false;
                    }
                    1
                } else if locstring::inline_cexo_string_end(bytes, name_offset).is_some() {
                    if bits.get(*bit_cursor + 1).copied().unwrap_or(true) {
                        return false;
                    }
                    1
                } else if legacy_short_name {
                    0
                } else {
                    return false;
                }
            } else {
                0
            };
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

    let source_inner_bits = if bits[*bit_cursor] {
        if locstring::tlk_locstring_ref_end(bytes, name_offset)
            .map(|end| end + 2 == record_end)
            .unwrap_or(false)
        {
            if !bits.get(*bit_cursor + 1).copied().unwrap_or(false) {
                return false;
            }
            1
        } else if locstring::inline_cexo_string_end(bytes, name_offset).is_some() {
            if bits.get(*bit_cursor + 1).copied().unwrap_or(true) {
                return false;
            }
            1
        } else {
            return false;
        }
    } else {
        if locstring::tlk_locstring_ref_end(bytes, name_offset)
            .map(|end| end + 2 == record_end)
            .unwrap_or(false)
        {
            return false;
        }
        0
    };
    *bit_cursor = bit_cursor.saturating_add(6 + source_inner_bits);
    *bit_cursor <= bits.len()
}

fn legacy_short_door_name_token_at(bytes: &[u8], name_offset: usize, record_end: usize) -> bool {
    name_offset
        .checked_add(4 + 2)
        .map(|end| end == record_end)
        .unwrap_or(false)
        && record_end <= bytes.len()
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
    let outer_locstring = bits[*bit_cursor];
    let dest_inner_bits = if outer_locstring {
        let Some(inner_client_tlk) = bits.get(*bit_cursor + 1).copied() else {
            return false;
        };
        if inner_client_tlk {
            // EE `sub_1407A7800` routes outer=true into the locstring helper.
            // The bridge currently emits only the decompile-confirmed inline
            // CExoString/empty-name form: outer=true, inner=false. A true inner
            // bit would select the TLK/object-table branch and requires a
            // different typed byte parser, so exact validation must reject it.
            return false;
        }
        1
    } else {
        0
    };
    let post_name_bit = bit_cursor.saturating_add(1 + dest_inner_bits);
    if bits.len() <= post_name_bit + 9 {
        return false;
    }
    let optional_object_id = bits.get(post_name_bit + 1).copied().unwrap_or(true);
    if bits.get(post_name_bit + 9).copied().unwrap_or(true) {
        // EE adds one more trailing BOOL before its visual-transform map. The
        // bridge emits false until a captured/decompiled non-default field is
        // modeled explicitly.
        return false;
    }
    let tail_offset =
        locstring::inline_cexo_string_end(bytes, name_offset).unwrap_or(name_offset + 4);
    let Some(base_tail_end) = tail_offset.checked_add(1 + 2 + 2) else {
        return false;
    };
    let map_offset = if optional_object_id {
        let Some(optional_end) = base_tail_end.checked_add(4) else {
            return false;
        };
        if read_u32_le(bytes, base_tail_end).is_none() {
            return false;
        }
        optional_end
    } else {
        base_tail_end
    };
    if map_offset > record_end
        || map_offset + 40 != record_end
        || !creature::has_ee_identity_visual_transform_map_at(bytes, map_offset, record_end)
    {
        if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_PLACEABLE_ADD").is_some() {
            eprintln!(
                "placeable-add cursor reject record_offset={record_offset} record_end={record_end} bit_cursor={} post_name_bit={post_name_bit} optional_object_id={optional_object_id} tail_offset={tail_offset} base_tail_end={base_tail_end} map_offset={map_offset}",
                *bit_cursor
            );
        }
        return false;
    }
    *bit_cursor = post_name_bit.saturating_add(10);
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
