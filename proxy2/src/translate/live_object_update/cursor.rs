//! Fragment-bit cursor advancement for live-object submessages.
//!
//! Cursor advancement is deliberately separate from update-record rewriting:
//! add/delete records are not translated here, but they still consume fragment
//! BOOLs and therefore affect whether the shared tail can be trimmed safely.

use super::{
    CNW_LENGTH_BYTES, DOOR_OBJECT_TYPE, LEGACY_UPDATE_HEADER_BYTES, MAX_LIVE_OBJECT_NAME_BYTES,
    PLACEABLE_OBJECT_TYPE, TRIGGER_OBJECT_TYPE, add, boundary, creature, locstring, read_u16_le,
    read_u32_le, trigger,
};

const LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS: usize = 4;
const LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES: usize = 8;

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

    let original_bit_cursor = *bit_cursor;
    if add::advance_verified_add_record(bytes, record_offset, record_end, bits, bit_cursor) {
        return true;
    }
    *bit_cursor = original_bit_cursor;

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
                visual_offset
                    + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
            } else {
                visual_offset
            };
            if name_offset > record_end {
                return false;
            }
            let legacy_tail_before_empty_name =
                legacy_tail_before_empty_door_name_token_at(bytes, name_offset, record_end);
            if legacy_tail_before_empty_name {
                if bits.len().saturating_sub(*bit_cursor) < 4 {
                    return false;
                }
                *bit_cursor = bit_cursor.saturating_add(4);
                return true;
            }
            let legacy_short_name = legacy_short_door_name_token_at(bytes, name_offset, record_end);
            let legacy_short_strref_name =
                legacy_short_door_strref_name_token_at(bytes, name_offset, record_end);
            if legacy_short_strref_name {
                if bits.len().saturating_sub(*bit_cursor) < 5 {
                    return false;
                }
                *bit_cursor = bit_cursor.saturating_add(5);
                return true;
            }
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
                    if bits.len().saturating_sub(*bit_cursor) < 4 {
                        return false;
                    }
                    *bit_cursor = bit_cursor.saturating_add(4);
                    return true;
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
            let compact_cursor_shape =
                compact_legacy_placeable_add_cursor_shape(bytes, name_offset, record_end, true);
            if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_PLACEABLE_ADD").is_some() {
                eprintln!(
                    "placeable-add update-pass cursor candidate record_offset={record_offset} record_end={record_end} bit_cursor={} remaining_bits={} compact_cursor_shape={compact_cursor_shape} preview={:02X?}",
                    *bit_cursor,
                    bits.len().saturating_sub(*bit_cursor),
                    bytes
                        .get(record_offset..record_end.min(record_offset.saturating_add(64)))
                        .unwrap_or(&[])
                );
            }
            if compact_cursor_shape {
                if bits.len().saturating_sub(*bit_cursor) < LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS {
                    return false;
                }
                *bit_cursor = bit_cursor.saturating_add(LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS);
                return true;
            }
            let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) else {
                return false;
            };
            if legacy_placeable_add_tail_end_for_cursor(bytes, inline_end, record_end, true)
                .is_none()
            {
                return false;
            }
            let source_inner_bits = usize::from(bits.get(*bit_cursor).copied().unwrap_or(false));
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

fn compact_legacy_placeable_add_cursor_shape(
    bytes: &[u8],
    name_offset: usize,
    record_end: usize,
    allow_transform_suffix: bool,
) -> bool {
    if name_offset > record_end
        || record_end > bytes.len()
        || record_end - name_offset < CNW_LENGTH_BYTES + 1 + 1 + 2 + 2
        || read_u32_le(bytes, name_offset) != Some(0)
        || legacy_placeable_add_tail_end_for_cursor(
            bytes,
            name_offset + CNW_LENGTH_BYTES,
            record_end,
            allow_transform_suffix,
        )
        .is_some()
    {
        return false;
    }

    // Local Diamond compact placeable adds can encode an empty direct
    // CExoString at the name cursor, then carry a byte-aligned padded printable
    // legacy tail before the next live-object boundary. The focused add
    // translator later proves and rewrites the byte payload; this cursor helper
    // only advances the four Diamond tail BOOLs so update-record rewriting can
    // keep walking the same live-object stream without stealing following
    // update bits.
    let base_text_start = name_offset + CNW_LENGTH_BYTES;
    for prefix_skip in 0usize..=LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES {
        let Some(text_start) = base_text_start.checked_add(prefix_skip) else {
            return false;
        };
        if text_start >= record_end {
            break;
        }
        let tail_limit = text_start
            .saturating_add(MAX_LIVE_OBJECT_NAME_BYTES)
            .min(record_end);
        for tail_start in text_start + 1..=tail_limit {
            let text = &bytes[text_start..tail_start];
            if text
                .first()
                .is_none_or(|byte| !is_legacy_bare_placeable_name_byte(*byte))
            {
                break;
            }
            if !text
                .iter()
                .all(|byte| *byte == 0 || is_legacy_bare_placeable_name_byte(*byte))
            {
                break;
            }
            if !text
                .iter()
                .rfind(|byte| **byte != 0)
                .is_some_and(|byte| is_legacy_bare_placeable_name_byte(*byte))
            {
                continue;
            }
            if legacy_placeable_add_tail_end_for_cursor(
                bytes,
                tail_start,
                record_end,
                allow_transform_suffix,
            )
            .is_some()
            {
                return true;
            }
        }
    }
    false
}

fn legacy_placeable_add_tail_end_for_cursor(
    bytes: &[u8],
    tail_offset: usize,
    record_end: usize,
    allow_transform_suffix: bool,
) -> Option<usize> {
    if tail_offset > record_end || record_end > bytes.len() {
        return None;
    }
    let full_tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if placeable_tail_end_matches_record_end(
        bytes,
        full_tail_end,
        record_end,
        allow_transform_suffix,
    ) && full_tail_end <= bytes.len()
        && read_u16_le(bytes, tail_offset + 1).is_some()
        && read_u16_le(bytes, tail_offset + 3).is_some()
    {
        return Some(full_tail_end);
    }
    let compact_tail_end = tail_offset.checked_add(1 + 2 + 1)?;
    if placeable_tail_end_matches_record_end(
        bytes,
        compact_tail_end,
        record_end,
        allow_transform_suffix,
    ) && compact_tail_end <= bytes.len()
        && read_u16_le(bytes, tail_offset + 1).is_some()
    {
        return Some(compact_tail_end);
    }
    None
}

fn placeable_tail_end_matches_record_end(
    bytes: &[u8],
    tail_end: usize,
    record_end: usize,
    allow_transform_suffix: bool,
) -> bool {
    if tail_end == record_end {
        return true;
    }
    allow_transform_suffix
        && (creature::has_ee_identity_visual_transform_map_at(bytes, tail_end, record_end)
            || super::visual_transform::has_legacy_scalar_visual_transform_identity_at(
                bytes, tail_end, record_end,
            ))
}

fn is_legacy_bare_placeable_name_byte(byte: u8) -> bool {
    matches!(byte, 0x20..=0x7E | b'\t')
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
            visual_offset + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
        } else {
            visual_offset
        };
    if name_offset > record_end || *bit_cursor >= bits.len() {
        return false;
    }

    let is_tlk_locstring = locstring::tlk_locstring_ref_end(bytes, name_offset)
        .map(|end| end + 2 == record_end)
        .unwrap_or(false);
    let inline_cexo_string_end =
        locstring::inline_cexo_string_end(bytes, name_offset).filter(|end| end + 2 == record_end);
    let is_inline_cexo_string = inline_cexo_string_end.is_some();
    let is_non_empty_inline_cexo_string = inline_cexo_string_end
        .map(|end| end > name_offset + super::CNW_LENGTH_BYTES)
        .unwrap_or(false);
    let is_short_strref_name =
        legacy_short_door_strref_name_token_at(bytes, name_offset, record_end);
    let is_compact_empty_name = name_offset
        .checked_add(4 + 2)
        .map(|end| end == record_end)
        .unwrap_or(false)
        && read_u32_le(bytes, name_offset) == Some(0)
        && read_u16_le(bytes, name_offset + 4).is_some();

    let source_inner_bits = if is_tlk_locstring {
        // EE `sub_140796DD0` routes outer=true into
        // `ReadCExoLocStringClient` (`sub_1409735F0`), whose inner BOOL must
        // be true for the TLK-table DWORD shape.  Direct CExoString bytes are
        // deliberately not accepted through the outer=true/inner=false helper
        // path: the bridge canonicalises direct names to outer=false so the
        // exact validator proves the same reader branch the bytes model.
        if !bits[*bit_cursor] || !bits.get(*bit_cursor + 1).copied().unwrap_or(false) {
            return false;
        }
        1
    } else if is_inline_cexo_string {
        if bits[*bit_cursor] {
            if !is_non_empty_inline_cexo_string
                || bits.get(*bit_cursor + 1).copied().unwrap_or(true)
            {
                return false;
            }
            // EE `sub_140796DD0` name-mode BOOL true routes into
            // `sub_1409735F0`.  That helper's inner BOOL false reads the same
            // bounded `CExoString(0x20)` bytes as the direct outer=false path.
            // This is an exact EE reader shape, not a passthrough escape hatch:
            // inner=true remains the separate TLK/strref branch above.
            1
        } else {
            0
        }
    } else if is_compact_empty_name {
        if bits[*bit_cursor] {
            return false;
        }
        0
    } else if is_short_strref_name {
        if bits.len().saturating_sub(*bit_cursor) < 5 {
            return false;
        }
        *bit_cursor = bit_cursor.saturating_add(5);
        return *bit_cursor <= bits.len();
    } else {
        return false;
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

fn legacy_short_door_strref_name_token_at(
    bytes: &[u8],
    name_offset: usize,
    record_end: usize,
) -> bool {
    name_offset
        .checked_add(4 + 2)
        .map(|end| end == record_end)
        .unwrap_or(false)
        && record_end <= bytes.len()
        && read_u32_le(bytes, name_offset).is_some()
        && read_u32_le(bytes, name_offset) != Some(0)
        && read_u16_le(bytes, name_offset + 4).is_some()
}

fn legacy_tail_before_empty_door_name_token_at(
    bytes: &[u8],
    name_offset: usize,
    record_end: usize,
) -> bool {
    name_offset
        .checked_add(2 + super::CNW_LENGTH_BYTES)
        .map(|end| end == record_end)
        .unwrap_or(false)
        && record_end <= bytes.len()
        && super::read_u16_le(bytes, name_offset).is_some()
        && read_u32_le(bytes, name_offset + 2) == Some(0)
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
    let name_offset = record_offset + LEGACY_UPDATE_HEADER_BYTES - 4;
    if compact_legacy_placeable_add_cursor_shape(bytes, name_offset, record_end, false) {
        // Keep the update-rewrite pass aligned with the focused placeable add
        // writer. Before visual/name normalization, this local Diamond shape
        // owns only the four legacy tail BOOLs. Requiring the final EE 11-bit
        // guard/state run here would make the stream unreliable before the
        // semantic add translator can repair it.
        let remaining_source_bits = bits.len().saturating_sub(*bit_cursor);
        if remaining_source_bits == 0 {
            return true;
        }
        if remaining_source_bits < LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS {
            return false;
        }
        *bit_cursor = bit_cursor.saturating_add(LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS);
        return *bit_cursor <= bits.len();
    }
    if *bit_cursor >= bits.len() {
        return false;
    }
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
        || map_offset + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
            != record_end
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
