//! Legacy live-object record boundary detection.
//!
//! This module owns only stream-shape classification. It does not translate
//! records and it does not mutate read bytes or fragment bits.

use super::{
    CNW_LENGTH_BYTES, DOOR_OBJECT_TYPE, EE_UPDATE_APPEARANCE_RESREF_READ_BYTES,
    EE_UPDATE_APPEARANCE_WORD_READ_BYTES, EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES,
    EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES, EE_UPDATE_SCALE_STATE_READ_BYTES,
    LEGACY_UPDATE_APPEARANCE_MASK, LEGACY_UPDATE_HEADER_BYTES, LEGACY_UPDATE_NAME_MASK,
    LEGACY_UPDATE_ORIENTATION_MASK, LEGACY_UPDATE_POSITION_MASK, LEGACY_UPDATE_POSITION_READ_BYTES,
    LEGACY_UPDATE_SCALE_STATE_MASK, LEGACY_UPDATE_STATE_MASK, MAX_COMPACT_LEGACY_LIVE_OBJECT_ID,
    MAX_LIVE_OBJECT_NAME_BYTES, MIN_COMPACT_LEGACY_LIVE_OBJECT_ID, PLACEABLE_OBJECT_TYPE,
    TRIGGER_OBJECT_TYPE, appearance, creature, door, gui, inventory, item, locstring, placeable,
    read_u16_le, read_u32_le, reader, trigger,
};

pub(super) fn find_next_legacy_live_object_sub_message_boundary_after(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> usize {
    let scan_end = search_end.min(bytes.len());
    if offset >= scan_end {
        return scan_end;
    }

    if bytes.get(offset).copied() == Some(b'G') {
        if let Some(record_end) = gui::try_get_legacy_live_gui_record_end(bytes, offset, scan_end) {
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'I') {
        if let Some(prefix) =
            inventory::try_get_legacy_live_inventory_prefix_claim(bytes, offset, scan_end)
        {
            if !prefix.interleaved_fragment_tail_allowed
                && prefix.read_end > offset
                && prefix.read_end < scan_end
            {
                // Diamond `sub_455940` and EE `sub_1407B4F70` both consume the
                // inventory mask branches before returning to the live-object
                // dispatcher. Trust the exact inventory parser's cursor first,
                // even when the following bytes are stale-declared CNW fragment
                // storage rather than another live-object/GUI boundary. The
                // semantic rewrite and exact validator still have to prove the
                // promoted fragment tail before this packet can be emitted.
                return prefix.read_end;
            }
        }
    }

    if bytes.get(offset).copied() == Some(b'A') && bytes.get(offset + 1).copied() == Some(0x05) {
        let record_end = offset.saturating_add(creature::EE_CREATURE_ADD_RECORD_BYTES);
        if record_end <= scan_end
            && creature::looks_like_ee_creature_add_record(bytes, offset, record_end)
        {
            return record_end;
        }
        let record_end = offset.saturating_add(creature::LEGACY_CREATURE_ADD_RECORD_BYTES);
        if record_end <= scan_end
            && creature::looks_like_legacy_creature_add_transform_fields(bytes, offset, record_end)
        {
            // Diamond `sub_4489F0` consumes exactly OBJECTID, six raw FLOAT
            // fields, and a WORD for creature add records before returning to
            // the live-object stream. If the following bytes are fragment
            // storage or another family, they must not be swallowed by a
            // generic opcode scan merely because they do not start with an
            // obvious boundary byte.
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'U') && bytes.get(offset + 1).copied() == Some(0x05) {
        if let Some(record_end) =
            try_get_ee_creature_update_record_end_for_transport(bytes, offset, scan_end)
        {
            return record_end;
        }
        if read_u32_le(bytes, offset + 6) == Some(0x0000_4408) {
            let legacy_4408_end = offset.saturating_add(LEGACY_UPDATE_HEADER_BYTES + 2 + 3 + 8);
            if legacy_4408_end < scan_end
                && inventory::try_get_missing_current_player_2a00_record_end(
                    bytes,
                    legacy_4408_end,
                    scan_end,
                )
                .is_some()
            {
                // Local Diamond auto-inventory can follow the compact
                // `U/5 0x4408` self/status record with a current-player
                // `0x2A00` inventory body whose live-object opcode byte is
                // zero. The inventory translator repairs that slot only after
                // exact parser/fragment-bit proof; the boundary scanner merely
                // prevents the preceding status record from swallowing it.
                return legacy_4408_end;
            }
        }
    }

    if bytes.get(offset).copied() == Some(b'A')
        && bytes.get(offset + 1).copied() == Some(TRIGGER_OBJECT_TYPE)
    {
        if let Some(record_end) = trigger::try_get_trigger_add_record_end(bytes, offset, scan_end) {
            return record_end;
        }
    }

    if matches!(
        (bytes.get(offset).copied(), bytes.get(offset + 1).copied()),
        (
            Some(b'A'),
            Some(PLACEABLE_OBJECT_TYPE) | Some(DOOR_OBJECT_TYPE)
        )
    ) {
        if let Some(record_end) = try_get_ee_door_placeable_add_record_end(bytes, offset, scan_end)
        {
            return record_end;
        }
        if let Some(record_end) = try_get_legacy_placeable_short_name_add_record_end_for_transport(
            bytes, offset, scan_end,
        ) {
            return record_end;
        }
        if let Some(record_end) =
            try_get_legacy_placeable_bare_name_add_record_end_for_transport(bytes, offset, scan_end)
        {
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'P') && bytes.get(offset + 1).copied() == Some(0x05) {
        if let Some(record_end) =
            creature::try_get_zero_mask_creature_appearance_record_end(bytes, offset, scan_end)
        {
            return record_end;
        }
        let legacy_end =
            appearance::try_get_legacy_creature_appearance_record_end(bytes, offset, scan_end);
        let ee_byte_shape_end = appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
            bytes, offset, scan_end,
        );
        if let Some(ee_end) = ee_byte_shape_end {
            if legacy_end
                .map(|legacy_end| ee_end > legacy_end)
                .unwrap_or(false)
                && (ee_end >= scan_end
                    || looks_like_legacy_live_object_sub_message_boundary(bytes, ee_end))
            {
                return ee_end;
            }
        }
        if let Some(record_end) = legacy_end {
            return record_end;
        }
    }

    if matches!(
        (bytes.get(offset).copied(), bytes.get(offset + 1).copied()),
        (
            Some(b'U'),
            Some(PLACEABLE_OBJECT_TYPE) | Some(DOOR_OBJECT_TYPE)
        )
    ) {
        if let Some(record_end) =
            try_get_legacy_door_placeable_inline_name_update_record_end(bytes, offset, scan_end)
        {
            return record_end;
        }
        if let Some(record_end) =
            try_get_legacy_placeable_word_name_suffix_update_record_end(bytes, offset, scan_end)
        {
            return record_end;
        }
        if let Some(record_end) =
            try_get_ee_door_placeable_update_record_end(bytes, offset, scan_end)
        {
            return record_end;
        }
    }

    let start = scan_end.min(offset + minimum_legacy_live_object_record_length_at(bytes, offset));
    let inventory_record = bytes.get(offset).copied() == Some(b'I');
    let mut suppress_inline_string_boundaries = bytes.get(offset).copied() != Some(b'I');
    if bytes.len().saturating_sub(offset) >= 10
        && bytes[offset] == b'U'
        && bytes[offset + 1] == 0x05
        && looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        if let Some(raw_mask) = read_u32_le(bytes, offset + 6) {
            if matches!(
                raw_mask,
                0x0000_0007
                    | 0x0000_0008
                    | 0x0000_0040
                    | 0x0000_0047
                    | 0x0000_3967
                    | 0x0000_4408
                    | 0x0000_8000
                    | 0x0000_C408
                    | 0x0000_C40F
            ) {
                // Decompile/capture-backed creature `U/5` numeric update shapes
                // contain compact status/movement fields, not CExoString names.
                // Mirroring the mature bridge, do not hide candidate live-object
                // boundaries merely because bytes inside these numeric fields
                // look like a one-byte inline string length plus an opcode.
                // `0x0000_3967` does contain the decompiled identity-string
                // branch, but its HG short-declared captures can also place a
                // bounded CNW fragment-storage span before the next `A/5`
                // boundary. Let the focused creature-update parser and
                // `fragment_spans` proof accept or reject the exact split
                // instead of letting generic inline-string suppression hide the
                // following real record.
                // `0x0000_C408` is the HG self/visibility status family. The
                // stock Diamond/EE core is the visual-effect delta count, four
                // SHORT stats, five+two visibility BOOLs, and three
                // self-visibility BOOLs. Some HG captures carry a malformed
                // zero visual-effect count followed by the known three encoded
                // entries; the translator repairs only that count before EE
                // validation. Its next true submessage can be an `I` sentinel
                // record immediately after this compact numeric record, so
                // inline-string suppression would hide the real boundary.
                // `0x0000_0040` is the compact creature state branch already
                // modelled by `creature.rs`: WORD, BYTE mode, WORD, BYTE, then
                // one fragment BOOL and an optional OBJECTID only when mode 2
                // is set. The 2026-05-12 Starcore5 driver capture has mode 1
                // and the next byte is a real `U/5 0x47` boundary at the exact
                // decompiled read cursor; string suppression must not merge it.
                // `0x0000_C40F` is the same self/status family with the
                // Diamond writer's lower movement bits present. The writer at
                // 0x4451DC..0x4458B0 emits `0x0001` position, `0x0002`
                // orientation, `0x0004` action scalar/code, then falls through
                // to the `0x0008` status-effect list and the `0xC400` suffix.
                // Its adjacent fragment-storage bytes can contain opcode-like
                // values before the following inventory record, so only the
                // focused creature parser/span promoter may choose the split.
                suppress_inline_string_boundaries = false;
            }
        }
    }
    let string_scan_start = (offset + 2).min(scan_end);
    for candidate in start..scan_end.saturating_sub(1) {
        if suppress_inline_string_boundaries
            && locstring::candidate_inside_inline_string(bytes, string_scan_start, candidate)
        {
            continue;
        }
        if looks_like_legacy_live_object_sub_message_boundary(bytes, candidate) {
            let inventory_candidate_claimed = !inventory_record
                || inventory::try_get_legacy_live_inventory_fragment_bit_count(
                    bytes, offset, candidate,
                )
                .is_some()
                || inventory::try_get_legacy_live_inventory_prefix_claim(bytes, offset, candidate)
                    .is_some_and(|claim| {
                        claim.interleaved_fragment_tail_allowed
                            && claim.read_end > offset
                            && claim.read_end < candidate
                    });
            if !inventory_candidate_claimed {
                continue;
            }
            return candidate;
        }
    }
    if inventory_record {
        // Decompile/capture-backed inventory discipline: `I` records contain
        // opcode-like row bytes. If no candidate boundary validates as a full
        // inventory record, keep the remaining read buffer together so the
        // semantic inventory translator can either claim the whole record or
        // quarantine it. This mirrors the mature C++ bridge rule documented in
        // `packet-alignment-reference.md`: never split inventory merely because
        // an interior byte looks like `U`, `D`, or another live-object opcode.
        return scan_end;
    }
    scan_end
}

fn try_get_ee_door_placeable_add_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset + 6 > scan_end
        || offset + 6 > bytes.len()
        || bytes.get(offset).copied()? != b'A'
        || !matches!(
            bytes.get(offset + 1).copied()?,
            PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
        )
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    match bytes[offset + 1] {
        PLACEABLE_OBJECT_TYPE => try_get_ee_placeable_add_record_end(bytes, offset, scan_end),
        DOOR_OBJECT_TYPE => try_get_ee_door_add_record_end(bytes, offset, scan_end),
        _ => None,
    }
}

pub(super) fn try_get_ee_door_placeable_add_record_end_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    try_get_ee_door_placeable_add_record_end(bytes, offset, scan_end)
}

pub(super) fn try_get_legacy_placeable_short_name_add_record_end_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset + 15 > scan_end
        || offset + 15 > bytes.len()
        || bytes.get(offset).copied()? != b'A'
        || bytes.get(offset + 1).copied()? != PLACEABLE_OBJECT_TYPE
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
        || read_u32_le(bytes, offset + 6).is_none()
        || read_u16_le(bytes, offset + 11).is_none()
        || read_u16_le(bytes, offset + 13).is_none()
    {
        return None;
    }

    // Diamond `CNWSMessage::AddPlaceableAppearanceToMessage` can emit the
    // compact short-name form seen in local Winds captures: OBJECTID, a
    // four-byte legacy name/token slot, then BYTE type, WORD appearance, WORD
    // static/state. The add translator rewrites that token slot into an empty
    // EE CExoString and inserts the visual-transform map; this boundary helper
    // only prevents the following `U/9` update from being swallowed as an
    // imaginary large inline string.
    let object_id = read_u32_le(bytes, offset + 2)?;
    let record_end = offset + 15;
    if record_end < scan_end
        && !looks_like_legacy_live_object_sub_message_boundary(bytes, record_end)
        && try_get_legacy_missing_opcode_door_placeable_update_body_end_after_add(
            bytes,
            record_end,
            scan_end,
            PLACEABLE_OBJECT_TYPE,
            object_id,
        )
        .is_none()
    {
        return None;
    }
    Some(record_end)
}

pub(super) fn try_get_legacy_placeable_bare_name_add_record_end_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    let scan_end = scan_end.min(bytes.len());
    if offset + 6 > scan_end
        || bytes.get(offset).copied()? != b'A'
        || bytes.get(offset + 1).copied()? != PLACEABLE_OBJECT_TYPE
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let object_id = read_u32_le(bytes, offset + 2)?;
    let name_offset = offset.checked_add(6)?;
    let record_end =
        legacy_placeable_bare_name_add_record_end(bytes, name_offset, scan_end, object_id)?;

    Some(record_end)
}

pub(super) fn try_get_legacy_missing_opcode_door_placeable_update_body_end_after_add(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    expected_object_type: u8,
    expected_object_id: u32,
) -> Option<usize> {
    let scan_end = scan_end.min(bytes.len());
    if !matches!(
        expected_object_type,
        PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) {
        return None;
    }

    let body_offset = if bytes.get(offset).copied()? == expected_object_type {
        offset
    } else if bytes.get(offset).copied()? == 0
        && bytes.get(offset + 1).copied()? == expected_object_type
    {
        offset.checked_add(1)?
    } else {
        return None;
    };

    if body_offset + LEGACY_UPDATE_HEADER_BYTES - 1 > scan_end
        || read_u32_le(bytes, body_offset + 1)? != expected_object_id
    {
        return None;
    }

    let raw_mask = read_u32_le(bytes, body_offset + 5)?;
    if raw_mask == 0 {
        return None;
    }
    let translated_mask = match expected_object_type {
        PLACEABLE_OBJECT_TYPE => placeable::translate_update_mask(raw_mask),
        DOOR_OBJECT_TYPE => door::translate_update_mask(raw_mask),
        _ => return None,
    };
    if translated_mask == 0 {
        return None;
    }

    for vector_orientation in [false, true] {
        if let Some(record_end) = missing_opcode_update_body_read_end(
            bytes,
            body_offset,
            scan_end,
            translated_mask,
            vector_orientation,
        ) {
            if record_end_lands_on_boundary(bytes, record_end, scan_end) {
                return Some(record_end);
            }
        }
    }

    if expected_object_type == PLACEABLE_OBJECT_TYPE
        && (raw_mask & !translated_mask & !placeable::LEGACY_PLACEABLE_LOW_TAIL_MASK) == 0
        && (raw_mask & !translated_mask & placeable::LEGACY_PLACEABLE_LOW_TAIL_MASK) != 0
    {
        for vector_orientation in [false, true] {
            let Some(prefix_end) = missing_opcode_update_body_read_end(
                bytes,
                body_offset,
                scan_end,
                translated_mask,
                vector_orientation,
            ) else {
                continue;
            };
            for suffix_len in [2usize, 4usize, 6usize] {
                let record_end = prefix_end.checked_add(suffix_len)?;
                if record_end <= scan_end
                    && reader::legacy_low_bit_control_tail_ready(bytes, prefix_end, record_end)
                    && record_end_lands_on_boundary(bytes, record_end, scan_end)
                {
                    return Some(record_end);
                }
            }
        }
    }

    None
}

fn missing_opcode_update_body_read_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    mask: u32,
    vector_orientation: bool,
) -> Option<usize> {
    let mut cursor = offset.checked_add(1 + 4 + 4)?;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        cursor = cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        cursor = cursor.checked_add(if vector_orientation {
            EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES
        } else {
            EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES
        })?;
    }
    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let appearance = read_u16_le(bytes, cursor)?;
        cursor = cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
        if appearance >= 0xFFFE {
            cursor = cursor.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
        }
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        cursor = cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    }
    if cursor > scan_end || cursor > bytes.len() {
        return None;
    }
    if (mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        cursor = locstring::inline_cexo_string_end(bytes, cursor)?;
    }
    (cursor <= scan_end && cursor <= bytes.len()).then_some(cursor)
}

fn record_end_lands_on_boundary(bytes: &[u8], record_end: usize, scan_end: usize) -> bool {
    record_end == scan_end || looks_like_legacy_live_object_sub_message_boundary(bytes, record_end)
}

const LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES: usize = 8;

fn legacy_placeable_bare_name_add_record_end(
    bytes: &[u8],
    name_offset: usize,
    scan_end: usize,
    object_id: u32,
) -> Option<usize> {
    if name_offset > scan_end
        || scan_end > bytes.len()
        || scan_end - name_offset < CNW_LENGTH_BYTES + 1 + 1 + 2 + 2
        || read_u32_le(bytes, name_offset)? != 0
    {
        return None;
    }

    let base_text_start = name_offset + CNW_LENGTH_BYTES;
    for prefix_skip in 0usize..=LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES {
        let text_start = base_text_start.checked_add(prefix_skip)?;
        if text_start >= scan_end {
            break;
        }
        let text_limit = text_start
            .saturating_add(MAX_LIVE_OBJECT_NAME_BYTES)
            .min(scan_end);
        for text_end in text_start + 1..=text_limit {
            if !is_legacy_bare_placeable_name_byte(bytes[text_end - 1]) {
                break;
            }
            let padding_limit = text_end.saturating_add(4).min(scan_end);
            for tail_start in text_end..=padding_limit {
                if bytes[text_end..tail_start].iter().any(|byte| *byte != 0) {
                    break;
                }
                if let Some(tail_end) =
                    legacy_placeable_add_tail_end_for_boundary(bytes, tail_start, scan_end)
                {
                    if placeable_add_tail_end_lands_on_following_record(
                        bytes, tail_end, scan_end, object_id,
                    ) {
                        return Some(tail_end);
                    }
                }
            }
        }
    }

    for prefix_skip in 0usize..=LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES {
        let text_start = base_text_start.checked_add(prefix_skip)?;
        if text_start >= scan_end {
            break;
        }
        let tail_limit = text_start
            .saturating_add(MAX_LIVE_OBJECT_NAME_BYTES)
            .min(scan_end);
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
            if let Some(tail_end) =
                legacy_placeable_add_tail_end_for_boundary(bytes, tail_start, scan_end)
            {
                if placeable_add_tail_end_lands_on_following_record(
                    bytes, tail_end, scan_end, object_id,
                ) {
                    return Some(tail_end);
                }
            }
        }
    }

    None
}

fn placeable_add_tail_end_lands_on_following_record(
    bytes: &[u8],
    tail_end: usize,
    scan_end: usize,
    object_id: u32,
) -> bool {
    tail_end == scan_end
        || looks_like_legacy_live_object_sub_message_boundary(bytes, tail_end)
        || try_get_legacy_missing_opcode_door_placeable_update_body_end_after_add(
            bytes,
            tail_end,
            scan_end,
            PLACEABLE_OBJECT_TYPE,
            object_id,
        )
        .is_some()
}

fn legacy_placeable_add_tail_end_for_boundary(
    bytes: &[u8],
    tail_offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if tail_offset > scan_end || scan_end > bytes.len() {
        return None;
    }

    let full_tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if full_tail_end <= scan_end
        && read_u16_le(bytes, tail_offset + 1).is_some()
        && read_u16_le(bytes, tail_offset + 3).is_some()
    {
        return Some(full_tail_end);
    }

    let compact_tail_end = tail_offset.checked_add(1 + 2 + 1)?;
    if compact_tail_end <= scan_end && read_u16_le(bytes, tail_offset + 1).is_some() {
        return Some(compact_tail_end);
    }

    None
}

fn is_legacy_bare_placeable_name_byte(byte: u8) -> bool {
    matches!(byte, 0x20..=0x7E | b'\t')
}

pub(super) fn try_get_ee_creature_update_record_end_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // Transport/salvage boundary ownership is intentionally narrower than the
    // final creature-update semantic validator. Without the CNW fragment cursor
    // and bit stream, masks such as `0x3967` cannot be proven from bytes alone:
    // Diamond and EE consume optional identity/action branches under fragment
    // BOOL control before returning to the live-object dispatcher. Only the
    // compact numeric update families with decompile-owned byte lengths may
    // claim a boundary here.
    creature::try_get_ee_creature_update_4008_record_end(bytes, offset, scan_end)
        .or_else(|| creature::try_get_ee_creature_update_c408_record_end(bytes, offset, scan_end))
        .or_else(|| creature::try_get_ee_creature_update_c40f_record_end(bytes, offset, scan_end))
        .or_else(|| creature::try_get_ee_creature_update_c44f_record_end(bytes, offset, scan_end))
}

fn try_get_ee_placeable_add_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    let name_offset = offset.checked_add(6)?;
    let tail_offset = locstring::inline_cexo_string_end(bytes, name_offset)?;
    let tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if tail_end > scan_end || read_u16_le(bytes, tail_offset + 1).is_none() {
        return None;
    }
    if read_u16_le(bytes, tail_offset + 3).is_none() {
        return None;
    }

    // EE `CNWSMessage::AddPlaceableAppearanceToMessage` writes the direct name
    // bytes, type byte, appearance WORD, static/state WORD, then consumes a
    // fragment BOOL that may guard one optional OBJECTID before the
    // `ObjectVisualTransformData::Write` map. For EE build `2001/0x23`, the
    // identity object map is two zero DWORD counts. Once that identity map is
    // present, the add-record end is decompile-owned; do not let the generic
    // byte scanner merge the following `U/9` update into this record.
    let direct_end = tail_end
        .checked_add(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
    if direct_end <= scan_end
        && creature::has_ee_identity_visual_transform_map_at(bytes, tail_end, direct_end)
    {
        return Some(direct_end);
    }

    let optional_object_end = tail_end.checked_add(4)?;
    let optional_end = optional_object_end
        .checked_add(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
    if optional_end <= scan_end
        && read_u32_le(bytes, tail_end).is_some()
        && creature::has_ee_identity_visual_transform_map_at(
            bytes,
            optional_object_end,
            optional_end,
        )
    {
        return Some(optional_end);
    }

    None
}

fn try_get_ee_door_add_record_end(bytes: &[u8], offset: usize, scan_end: usize) -> Option<usize> {
    let first_dword = read_u32_le(bytes, offset + 6)?;
    let visual_offset = offset.checked_add(2 + if first_dword == 0 { 12 } else { 8 })?;
    let name_offset = visual_offset
        .checked_add(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
    if name_offset > scan_end {
        return None;
    }

    // EE `CNWSMessage::AddDoorAppearanceToMessage` writes one or two DWORDs,
    // the same EE object visual-transform identity map, then the existing
    // name/state tail. The Diamond-only optional model token has already been
    // removed by the focused add-record translator before this boundary helper
    // can claim it.
    if !creature::has_ee_identity_visual_transform_map_at(bytes, visual_offset, name_offset) {
        return None;
    }

    if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
        let record_end = inline_end.checked_add(2)?;
        return (record_end <= scan_end && read_u16_le(bytes, inline_end).is_some())
            .then_some(record_end);
    }

    if let Some(tlk_end) = locstring::tlk_locstring_ref_end(bytes, name_offset) {
        let record_end = tlk_end.checked_add(2)?;
        return (record_end <= scan_end && read_u16_le(bytes, tlk_end).is_some())
            .then_some(record_end);
    }

    let record_end = name_offset.checked_add(6)?;
    (record_end <= scan_end && read_u16_le(bytes, name_offset + 4).is_some()).then_some(record_end)
}

fn try_get_ee_door_placeable_update_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > scan_end
        || offset + LEGACY_UPDATE_HEADER_BYTES > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || !matches!(
            bytes.get(offset + 1).copied()?,
            PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
        )
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let mask = read_u32_le(bytes, offset + 6)?;
    let allowed_mask = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_NAME_MASK;
    if mask == 0 || (mask & !allowed_mask) != 0 {
        return None;
    }

    // EE `WriteGameObjUpdate_UpdateObject` emits read-buffer fields in this
    // fixed order for door/placeable updates: position bytes, scalar
    // orientation bytes, scale/state bytes, then an inline name when the name
    // mask is set. State-only fields live in CNW fragment bits and do not move
    // the read cursor. Once a bridge packet is in this EE shape, the stream
    // boundary is decompile-owned and must not be discovered by scanning for an
    // interior byte that happens to look like a live-object opcode.
    let mut read_cursor = offset + LEGACY_UPDATE_HEADER_BYTES;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        read_cursor = read_cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    }
    if read_cursor > scan_end || read_cursor > bytes.len() {
        return None;
    }
    if (mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        if let Some(name_end) = locstring::inline_cexo_string_end(bytes, read_cursor) {
            read_cursor = name_end;
        } else if is_bridge_empty_state_update_mask(mask) {
            // Bridge-created cleanup shape, not a Diamond/EE server shape:
            // after add/visual-map rewrites, a legacy all-fields door/placeable
            // update can be reduced to state-only semantics while still carrying
            // the previously translated EE mask. EE's reader cannot consume the
            // position/orientation/name bits without their read-buffer fields.
            //
            // Decompile-backed rule: state lives entirely in fragment BOOLs; the
            // dropped position/orientation/name fields require read bytes. If the
            // byte immediately after the update header is a real next live-object
            // boundary and the full EE inline-name form did not parse, the current
            // record's bounded read span is exactly the ten-byte update header.
            let header_end = offset.checked_add(LEGACY_UPDATE_HEADER_BYTES)?;
            if header_end <= scan_end
                && looks_like_legacy_live_object_sub_message_boundary(bytes, header_end)
            {
                return Some(header_end);
            }
            return None;
        } else {
            return None;
        }
    }
    if read_cursor <= scan_end && read_cursor <= bytes.len() {
        Some(read_cursor)
    } else {
        None
    }
}

fn is_bridge_empty_state_update_mask(mask: u32) -> bool {
    let ee_supported_all = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK;
    mask == ee_supported_all || mask == (ee_supported_all | LEGACY_UPDATE_NAME_MASK)
}

fn try_get_legacy_door_placeable_inline_name_update_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > scan_end || scan_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U' {
        return None;
    }
    if !matches!(
        bytes.get(offset + 1).copied()?,
        PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) {
        return None;
    }
    if !looks_like_legacy_live_object_id_at(bytes, offset + 2) {
        return None;
    }

    let raw_mask = read_u32_le(bytes, offset + 6)?;
    if (raw_mask & LEGACY_UPDATE_NAME_MASK) == 0 {
        return None;
    }
    let debug_live_claim = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();

    // Diamond `CNWSMessage::WriteGameObjUpdate_UpdateObject` and EE
    // `sub_14079C050` consume the shared generic door/placeable fields before
    // Diamond's legacy bit-13 inline CExoString name branch:
    //
    //   position -> orientation scalar/vector branch -> appearance/resref
    //   -> scale/state -> fragment-only state -> legacy inline name.
    //
    // Boundary scanning must therefore not accept an `A/U/D/...` byte inside
    // that CExoString as the next live-object submessage. The semantic record
    // translator still owns removing the legacy name bit and bytes; this helper
    // only proves the complete legacy record span.
    let mut cursors = vec![offset.checked_add(LEGACY_UPDATE_HEADER_BYTES)?];
    if (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        for cursor in &mut cursors {
            *cursor = cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
        }
    }

    if (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let mut branch_cursors = Vec::with_capacity(cursors.len().saturating_mul(2));
        for cursor in cursors {
            branch_cursors.push(cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?);
            branch_cursors.push(cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES)?);
        }
        cursors = branch_cursors;
    }

    if (raw_mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let mut appearance_cursors = Vec::with_capacity(cursors.len());
        for cursor in cursors {
            let appearance = read_u16_le(bytes, cursor)?;
            let mut next = cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
            if appearance >= 0xFFFE {
                next = next.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
            }
            appearance_cursors.push(next);
        }
        cursors = appearance_cursors;
    }

    if (raw_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        for cursor in &mut cursors {
            *cursor = cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
        }
    }

    let mut proven_end = None;
    for name_offset in cursors {
        if name_offset > scan_end {
            continue;
        }
        let Some(name_end) = locstring::inline_cexo_string_end(bytes, name_offset) else {
            if debug_live_claim {
                eprintln!(
                    "legacy inline update boundary candidate rejected: offset={offset} name_offset={name_offset} reason=no-inline-cexo mask=0x{raw_mask:08X} preview={:02X?}",
                    bytes
                        .get(name_offset..scan_end.min(name_offset.saturating_add(16)))
                        .unwrap_or(&[])
                );
            }
            continue;
        };
        if name_end > scan_end {
            if debug_live_claim {
                eprintln!(
                    "legacy inline update boundary candidate rejected: offset={offset} name_offset={name_offset} name_end={name_end} scan_end={scan_end} reason=name-overflow mask=0x{raw_mask:08X}"
                );
            }
            continue;
        }
        if name_end == scan_end
            || looks_like_legacy_live_object_sub_message_boundary(bytes, name_end)
        {
            if debug_live_claim {
                eprintln!(
                    "legacy inline update boundary candidate accepted: offset={offset} name_offset={name_offset} name_end={name_end} scan_end={scan_end} mask=0x{raw_mask:08X}"
                );
            }
            proven_end = match proven_end {
                Some(existing) if existing != name_end => return None,
                Some(existing) => Some(existing),
                None => Some(name_end),
            };
        } else if debug_live_claim {
            eprintln!(
                "legacy inline update boundary candidate rejected: offset={offset} name_offset={name_offset} name_end={name_end} reason=no-following-boundary mask=0x{raw_mask:08X} following={:02X?}",
                bytes
                    .get(name_end..scan_end.min(name_end.saturating_add(16)))
                    .unwrap_or(&[])
            );
        }
    }

    proven_end
}

fn try_get_legacy_placeable_word_name_suffix_update_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > scan_end
        || scan_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != PLACEABLE_OBJECT_TYPE
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let raw_mask = read_u32_le(bytes, offset + 6)?;
    let suffix_mask = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK;
    if raw_mask != suffix_mask {
        return None;
    }

    let mut prefix_end = offset
        .checked_add(LEGACY_UPDATE_HEADER_BYTES)?
        .checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?
        .checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?
        .checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    if prefix_end + 2 > scan_end || read_u16_le(bytes, prefix_end).is_none() {
        return None;
    }
    prefix_end = prefix_end.checked_add(2)?;
    let name_end = locstring::inline_cexo_string_end(bytes, prefix_end)?;
    if name_end <= scan_end
        && (name_end == scan_end
            || looks_like_legacy_live_object_sub_message_boundary(bytes, name_end))
        && reader::legacy_name_tail_ready(bytes, prefix_end, name_end)
    {
        return Some(name_end);
    }

    None
}

fn minimum_legacy_live_object_record_length_at(bytes: &[u8], offset: usize) -> usize {
    if !looks_like_legacy_live_object_sub_message_boundary(bytes, offset) {
        return 2;
    }
    match (bytes[offset], bytes[offset + 1]) {
        (b'A', _) if appearance::looks_like_legacy_item_add_record_boundary(bytes, offset) => 9,
        (b'A', 0x05) => 32,
        (b'A', TRIGGER_OBJECT_TYPE) => trigger::TRIGGER_ADD_MIN_RECORD_BYTES,
        (b'A', PLACEABLE_OBJECT_TYPE) => {
            let name_offset = offset + 6;
            if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(4).saturating_sub(offset);
            }
            11
        }
        (b'A', DOOR_OBJECT_TYPE) => {
            let Some(first_dword) = read_u32_le(bytes, offset + 6) else {
                return 16;
            };
            let name_offset = offset + 2 + if first_dword == 0 { 12 } else { 8 };
            if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(2).saturating_sub(offset);
            }
            16
        }
        (b'U', PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) => {
            let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
                return LEGACY_UPDATE_HEADER_BYTES;
            };
            let mut minimum = LEGACY_UPDATE_HEADER_BYTES;
            if (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
                minimum += LEGACY_UPDATE_POSITION_READ_BYTES;
            }
            if (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
                && (raw_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK))
                    != 0
            {
                // HG/Diamond door/placeable name updates can carry a
                // decompile-backed nine-byte anchored generic tail at the name
                // cursor: WORD facing, one legacy generic byte, FLOAT scale,
                // WORD generic state. Bytes inside that tail can look like
                // item/live-object opcodes (`I 00 <compact id>`), so never scan
                // for the next submessage before this bounded tail is consumed.
                minimum += 9;
            }
            minimum
        }
        (b'U', 0x05) => minimum_legacy_creature_update_record_length_at(bytes, offset),
        (b'U' | b'P', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 10,
        (b'D', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 6,
        (b'G', b'Q') => 3,
        (b'I', _) => 7,
        (b'W', marker) if marker <= 0x0F && bytes.get(offset + 2) == Some(&0x0E) => 3,
        _ => 2,
    }
}

fn minimum_legacy_creature_update_record_length_at(bytes: &[u8], offset: usize) -> usize {
    let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
        return LEGACY_UPDATE_HEADER_BYTES;
    };

    if raw_mask == 0x0000_0007 {
        // Diamond/EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` reads this
        // compact creature update as the first three ordered mask branches:
        //
        //   header + mask
        //   0x0001 position: WORD, WORD, WORD plus two fragment bits
        //   0x0002 orientation: scalar form is one read byte plus fragment bits
        //   0x0004 action: FLOAT + WORD action code, state BYTE,
        //                  movement-followup WORD
        //
        // HG's Starcore5 coalesced live-object burst places an inventory
        // record immediately after this 26-byte update. Bytes in the numeric
        // body can look like live-object opcodes, so the generic scanner must
        // not accept any candidate before this decompile-owned lower bound.
        // The exact cursor and bit usage are still proven by creature.rs.
        return 26;
    }

    if raw_mask == 0x0000_0047 {
        // Diamond `CNWSMessage::WriteGameObjUpdate_UpdateObject` writes this
        // creature update family as:
        //
        //   header + mask
        //   0x0001 position: WORD, WORD, WORD plus two fragment bits
        //   0x0002 orientation: scalar form is one read byte plus fragment bits
        //   0x0004 action: FLOAT + WORD action code, state BYTE,
        //                  movement-followup WORD
        //   0x0040 state: WORD, BYTE, WORD, BYTE plus one fragment BOOL
        //
        // The read-buffer lower bound is therefore 32 bytes even before
        // optional target/object/float/path branches. HG captured a legal
        // position word pair `0x49 0x18` at offset +12; the generic scanner
        // mistook that interior byte sequence for an `I` live-object boundary.
        // Keep this as a mask-specific boundary floor and let the focused
        // creature cursor validator prove the exact final cursor and bits.
        return 32;
    }

    if raw_mask == 0x0000_4408 {
        // Local Diamond bw167demo captures this compact creature self/status
        // shape while entering the module:
        //
        //   U/5 header + mask
        //   0x0008 status-effect delta: WORD count, 3-byte legacy effect row
        //   0x0400 scalar/status branch: four WORDs
        //   0x4000 self/status suffix: fragment BOOLs only for the no-master
        //          branch observed here
        //
        // EE `sub_140781E80+0x1126` calls the status-effect reader for mask
        // `0x0008`; the focused creature translator inserts the EE
        // ObjectVisualTransformData identity map and then proves the whole
        // record through `creature.rs`. This boundary floor merely prevents the
        // generic live-object scanner from treating the interior `A` effect byte
        // as a real `A/5` submessage boundary.
        return LEGACY_UPDATE_HEADER_BYTES + 2 + 3 + 8;
    }

    if raw_mask == 0x0000_0008 {
        // Local Dark Ranger captures a standalone LowLightVision effect delta:
        //
        //   U/5 header + mask
        //   0x0008 status-effect delta: WORD count, count * 3 legacy bytes
        //
        // EE `sub_1407B1F00` and Diamond both consume the opcode/row triplets
        // before returning to the live-object stream. The following bytes may
        // be CNW fragment storage, so keep a count-derived read floor before
        // the generic scanner can consider the interior `A` effect opcode as a
        // top-level add boundary.
        let Some(count) = read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES) else {
            return LEGACY_UPDATE_HEADER_BYTES;
        };
        if count > 256 {
            return LEGACY_UPDATE_HEADER_BYTES;
        }
        return LEGACY_UPDATE_HEADER_BYTES + 2 + usize::from(count) * 3;
    }

    if raw_mask == 0x0000_4008 {
        // HG live seq37 proves the same status-effect/self-status writer
        // family without the `0x0400`/`0x8000` scalar/visibility suffixes:
        //
        //   U/5 header + mask
        //   0x0008 status-effect delta: WORD count, count * 3 legacy bytes
        //   0x4000 self/status suffix: fragment BOOLs only
        //
        // The encoded status-effect rows start with `A`/`D` bytes that are not
        // live-object submessage boundaries. Keep this as a count-derived read
        // floor only; the focused creature translator still inserts EE object
        // visual-transform maps and the exact validator proves the fragment
        // cursor before emission.
        let Some(count) = read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES) else {
            return LEGACY_UPDATE_HEADER_BYTES;
        };
        if count > 256 {
            return LEGACY_UPDATE_HEADER_BYTES;
        }
        return LEGACY_UPDATE_HEADER_BYTES + 2 + usize::from(count) * 3;
    }

    if raw_mask == 0x0000_C408 {
        // Diamond/EE `WriteGameObjUpdate_UpdateObject` writes this compact
        // creature self/status family as a WORD looping-visual-effect delta
        // count, encoded 3-byte effect entries, four WORD scalar/status values,
        // then visibility/self-visibility BOOLs in the fragment bitstream.
        //
        // HG captures can carry the malformed count-zero shape followed by the
        // same known three encoded effects that the focused creature translator
        // repairs before EE emission. Those interior effect bytes include `A`
        // markers, so the generic boundary scanner must not consider a later
        // live-object boundary until the decompile-owned fixed read span has
        // been crossed.
        //
        // This remains only a lower bound. The real semantic claim, count
        // repair, scalar cursor proof, and exact ten fragment-bit proof stay in
        // creature.rs.
        return LEGACY_UPDATE_HEADER_BYTES + 2 + 9 + 8;
    }

    if raw_mask == 0x0000_C40F {
        // Diamond 1.69 writer evidence:
        //
        //   0x445212: mask 0x0001 writes 16, 16, and 18 bits.
        //   0x44525B: mask 0x0002 writes the orientation branch.
        //   0x445427: mask 0x0004 writes the action scalar and WORD code.
        //   0x4458B0: mask 0x0008 writes the status-effect list.
        //   later 0x0400/0x4000/0x8000 branches match the C408 suffix.
        //
        // The Starcore5 Sooty Crow transition capture has scalar orientation,
        // action code zero, three status-effect triplets, four WORD scalars,
        // then three bytes of adjacent CNW fragment-storage before the next
        // `I` inventory record. This is still only a scan floor; exact cursor,
        // fragment-bit, and span proof stay in `creature.rs` and
        // `fragment_spans.rs`.
        return LEGACY_UPDATE_HEADER_BYTES + 6 + 1 + 6 + 2 + 9 + 8;
    }

    LEGACY_UPDATE_HEADER_BYTES
}

pub(super) fn looks_like_legacy_live_object_sub_message_boundary(
    bytes: &[u8],
    offset: usize,
) -> bool {
    if offset > bytes.len() || bytes.len() - offset < 2 {
        return false;
    }

    let opcode = bytes[offset];
    let marker = bytes[offset + 1];
    let typed_object_boundary = matches!(marker, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A)
        && looks_like_legacy_live_object_id_at(bytes, offset + 2);
    let legacy_type5_sentinel_boundary = marker == 0x05
        && bytes.len() - offset >= 6
        && bytes[offset + 2] == 0xFD
        && bytes[offset + 3] == 0xFF
        && bytes[offset + 4] == 0xFF
        && bytes[offset + 5] == 0xFF;

    if matches!(opcode, b'A' | b'D' | b'U' | b'P')
        && (typed_object_boundary || legacy_type5_sentinel_boundary)
    {
        return true;
    }

    if opcode == b'A' && appearance::looks_like_legacy_item_add_record_boundary(bytes, offset) {
        return true;
    }

    let legacy_item_sentinel = item::is_legacy_item_sentinel(bytes, offset);
    if opcode == b'I'
        && (item::is_known_legacy_item_marker(marker)
            || legacy_item_sentinel
            || looks_like_legacy_live_object_id_at(bytes, offset + 1))
    {
        return true;
    }

    if opcode == b'G' && gui::looks_like_legacy_live_gui_sub_message_boundary(bytes, offset) {
        return true;
    }

    opcode == b'W' && bytes.len() - offset >= 3 && marker <= 0x0F && bytes[offset + 2] == 0x0E
}

pub(super) fn looks_like_legacy_live_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(looks_like_legacy_live_object_id_value)
        .unwrap_or(false)
}

pub(super) fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }

    let high_byte = object_id & 0xFF00_0000;
    matches!(
        high_byte,
        // EE's decompile treats object ids as opaque DWORDs. These high-byte
        // filters are scanner guards, not engine rules. HG live-object door,
        // placeable, and Starcore5 Sooty Crow creature-add captures use
        // 0x08xxxxxx, 0x35xxxxxx, and 0xACxxxxxx ids, so accept those
        // namespaces explicitly while still rejecting arbitrary shifted ASCII
        // bytes.
        0x8000_0000
            | 0x8800_0000
            | 0xFF00_0000
            | 0x0100_0000
            | 0x0500_0000
            | 0x0800_0000
            | 0x3500_0000
            | 0xAC00_0000
    ) || (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID)
        .contains(&object_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_placeable_add_short_name_boundary_precedes_following_update() {
        let mut live = Vec::new();
        live.extend_from_slice(&[
            b'A', 0x09, 0x7E, 0x00, 0x00, 0x80, 0xF2, 0x38, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00,
            0x00,
        ]);
        live.extend_from_slice(&[
            b'U', 0x09, 0x7E, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00, 0x32, 0x39, 0x54, 0x13,
            0xE3, 0x0F, 0xBD, 0x69, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0xF2, 0x38, 0x00,
            0x00,
        ]);

        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            15
        );
    }

    #[test]
    fn compact_placeable_add_short_name_boundary_precedes_missing_opcode_same_object_update() {
        let mut live = Vec::new();
        live.extend_from_slice(&[
            b'A', 0x09, 0x84, 0x00, 0x00, 0x80, 0x18, 0x16, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
            0x00,
        ]);
        live.extend_from_slice(&[
            0x00, 0x09, 0x84, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00, 0xE5, 0x37, 0xFF, 0x1B,
            0xE3, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x18, 0x16,
        ]);
        live.extend_from_slice(&[
            b'A', 0x09, 0x85, 0x00, 0x00, 0x80, 0x18, 0x16, 0x00, 0x00, 0x05, 0x0E, 0x00, 0x00,
            0x00,
        ]);

        assert_eq!(
            try_get_legacy_placeable_short_name_add_record_end_for_transport(&live, 0, live.len(),),
            Some(15)
        );
        assert_eq!(
            try_get_legacy_missing_opcode_door_placeable_update_body_end_after_add(
                &live,
                15,
                live.len(),
                PLACEABLE_OBJECT_TYPE,
                0x8000_0084,
            ),
            Some(42)
        );
    }

    #[test]
    fn bare_name_placeable_add_boundary_precedes_following_update() {
        let mut live = Vec::new();
        live.extend_from_slice(&[b'A', 0x09, 0x3C, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00]);
        live.extend_from_slice(b"Item Discard Container");
        live.extend_from_slice(&[0x05, 0x00, 0x00, 0x00, 0x00]);
        live.extend_from_slice(&[
            b'U', 0x09, 0x3C, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00, 0x04, 0x09, 0x41, 0x04,
            0x0F, 0x0F,
        ]);

        assert_eq!(
            try_get_legacy_placeable_bare_name_add_record_end_for_transport(&live, 0, live.len(),),
            Some(37)
        );
        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            37
        );
    }

    #[test]
    fn compact_placeable_add_short_name_boundary_requires_following_record() {
        let live = [
            b'A', 0x09, 0x7E, 0x00, 0x00, 0x80, 0xF2, 0x38, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00,
            0x00, b'X', 0x09,
        ];

        assert!(
            try_get_legacy_placeable_short_name_add_record_end_for_transport(&live, 0, live.len(),)
                .is_none()
        );
    }
}
