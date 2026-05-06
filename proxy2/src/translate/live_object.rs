//! `GameObjUpdate_LiveObject` transport-shape and narrow semantic rewrites.
//!
//! Legacy 1.69 live-object update bursts can arrive as:
//!
//! `P 05 01 [four fragment bytes] [live-object read bytes...]`
//!
//! EE's `CNWMessage::SetReadMessage` path expects:
//!
//! `P 05 01 [u32 declared] [read bytes...] [fragment bytes...]`
//!
//! This module moves only verified/salvaged legacy fragment prefixes to the
//! CNW tail and performs small, decompile-backed EE shape inserts that belong
//! specifically to live-object payloads.
//!
//! EE decompile anchors used by the transforms below:
//!
//! - `sub_140781E80` gates creature visual-transform reads on update mask bit
//!   0x14, then calls `sub_140973160`.
//! - `sub_140973160` takes the 1.69-server-build branch through
//!   `sub_1407C4AB0(..., 0x2001, 0x23)` and reads ten legacy `LerpFloat`
//!   values through `sub_140972C70`.
//! - Door add `sub_140796DD0` reads the live-object id with `sub_1409737C0`,
//!   then one or two DWORDs, then the same visual-transform map before the
//!   door name payload.
//! - Placeable add `sub_1407A7800` reads the live-object id/name/tail fields,
//!   then the same visual-transform map after the legacy appearance tail.
//!
//! The identity-map insertion therefore emits exactly ten 32-bit legacy
//! `LerpFloat` defaults, and only for complete creature add records whose
//! fixed transform prefix ends exactly where EE will begin reading the map, or
//! for verified door/placeable add records at the EE decompile-backed cursor.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const LIVE_OBJECT_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
const LEGACY_LIVE_BYTES_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES;
const MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES: usize = 2048;
const MIN_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x0000_1000;
const MAX_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x00FF_FFFF;
const CREATURE_OBJECT_TYPE: u8 = 0x05;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
const DOOR_OBJECT_TYPE: u8 = 0x0A;
const CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET: usize = 32;
const EE_LEGACY_VISUAL_TRANSFORM_LERP_FLOAT_COUNT: usize = 10;
const EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES: [u8; 40] = [
    0x00, 0x00, 0x80, 0x3F,
    0x00, 0x00, 0x80, 0x3F,
    0x00, 0x00, 0x80, 0x3F,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x80, 0x3F,
];

#[derive(Debug, Clone)]
pub struct LiveObjectNormalizeSummary {
    pub old_wire_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES],
    pub live_bytes_offset: usize,
    pub live_bytes_length: usize,
    pub dropped_leadin_bytes: usize,
    pub salvaged_partial_leadin: bool,
    pub first_record_end: usize,
}

#[derive(Debug, Clone)]
pub struct LiveObjectVisualTransformSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub old_live_bytes_length: usize,
    pub new_live_bytes_length: usize,
    pub records_examined: u32,
    pub maps_inserted: u32,
    pub bytes_inserted: u32,
}

pub fn normalize_prefixed_fragments_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectNormalizeSummary> {
    if payload.len() < LEGACY_LIVE_BYTES_OFFSET + 1
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let old_wire_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if old_wire_declared >= (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u32
        && (old_wire_declared as usize) <= payload.len()
    {
        return None;
    }
    if old_wire_declared == 0 {
        return None;
    }

    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] = payload
        [HIGH_LEVEL_HEADER_BYTES..LEGACY_LIVE_BYTES_OFFSET]
        .try_into()
        .ok()?;
    let mut live_bytes_offset = LEGACY_LIVE_BYTES_OFFSET;
    let first_record_end;
    let mut salvaged_partial_leadin = false;
    if !looks_like_legacy_live_object_sub_message_boundary(payload, live_bytes_offset) {
        let salvaged = find_salvageable_legacy_live_object_boundary_after_prefixed_fragments(payload)?;
        if salvaged.0 <= LEGACY_LIVE_BYTES_OFFSET {
            return None;
        }
        live_bytes_offset = salvaged.0;
        first_record_end = salvaged.1;
        salvaged_partial_leadin = true;
    } else {
        first_record_end = find_next_legacy_live_object_sub_message_boundary_after(
            payload,
            live_bytes_offset,
            payload.len(),
        )
        .min(payload.len());
    }

    let live_bytes_length = payload.len() - live_bytes_offset;
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes_length;
    let new_declared = u32::try_from(new_declared_usize).ok()?;

    let mut rewritten = Vec::with_capacity(payload.len() + CNW_LENGTH_BYTES);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&payload[live_bytes_offset..]);
    rewritten.extend_from_slice(&prefixed_fragment_bytes);

    let old_payload_length = payload.len();
    let new_payload_length = rewritten.len();
    let dropped_leadin_bytes = live_bytes_offset - LEGACY_LIVE_BYTES_OFFSET;
    *payload = rewritten;

    Some(LiveObjectNormalizeSummary {
        old_wire_declared,
        new_declared,
        old_payload_length,
        new_payload_length,
        prefixed_fragment_bytes,
        live_bytes_offset,
        live_bytes_length,
        dropped_leadin_bytes,
        salvaged_partial_leadin,
        first_record_end,
    })
}

pub fn rewrite_creature_add_visual_transform_maps_if_possible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectVisualTransformSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let declared = usize::try_from(old_declared).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES || declared > payload.len() {
        return None;
    }

    let mut live_bytes = payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared].to_vec();
    let old_live_bytes_length = live_bytes.len();
    let mut records_examined = 0u32;
    let mut maps_inserted = 0u32;
    let mut bytes_inserted = 0u32;
    let mut offset = 0usize;

    while offset + 10 <= live_bytes.len() {
        if !looks_like_legacy_live_object_sub_message_boundary(&live_bytes, offset) {
            offset += 1;
            continue;
        }

        records_examined = records_examined.saturating_add(1);
        let record_end =
            find_next_legacy_live_object_sub_message_boundary_after(&live_bytes, offset, live_bytes.len())
                .min(live_bytes.len());
        if record_end <= offset {
            offset += 1;
            continue;
        }

        let Some(insert_offset) =
            legacy_add_visual_transform_insert_offset(&live_bytes, offset, record_end)
        else {
            offset = record_end.max(offset + 1);
            continue;
        };

        if has_ee_identity_visual_transform_map_at(&live_bytes, insert_offset, record_end) {
            offset = record_end.max(offset + 1);
            continue;
        }

        live_bytes.splice(
            insert_offset..insert_offset,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES,
        );
        maps_inserted = maps_inserted.saturating_add(1);
        bytes_inserted = bytes_inserted
            .saturating_add(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32);
        offset = record_end + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
    }

    if maps_inserted == 0 {
        return None;
    }

    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes.len();
    let new_declared = u32::try_from(new_declared_usize).ok()?;
    let mut rewritten = Vec::with_capacity(new_declared_usize + payload.len() - declared);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&live_bytes);
    rewritten.extend_from_slice(&payload[declared..]);

    let summary = LiveObjectVisualTransformSummary {
        old_declared,
        new_declared,
        old_payload_length: payload.len(),
        new_payload_length: rewritten.len(),
        old_live_bytes_length,
        new_live_bytes_length: live_bytes.len(),
        records_examined,
        maps_inserted,
        bytes_inserted,
    };
    *payload = rewritten;
    Some(summary)
}

fn find_salvageable_legacy_live_object_boundary_after_prefixed_fragments(
    payload: &[u8],
) -> Option<(usize, usize)> {
    if payload.len() < 16
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let scan_end = payload
        .len()
        .min(LEGACY_LIVE_BYTES_OFFSET + MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES);
    for candidate in LEGACY_LIVE_BYTES_OFFSET..scan_end.saturating_sub(1) {
        if !looks_like_salvageable_legacy_live_object_record_at(payload, candidate, scan_end) {
            continue;
        }
        let record_end =
            find_next_legacy_live_object_sub_message_boundary_after(payload, candidate, payload.len())
                .min(payload.len());
        return Some((candidate, record_end));
    }
    None
}

fn looks_like_salvageable_legacy_live_object_record_at(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> bool {
    if !looks_like_legacy_live_object_sub_message_boundary(bytes, record_offset)
        || record_offset + 6 > bytes.len()
    {
        return false;
    }

    let record_end =
        find_next_legacy_live_object_sub_message_boundary_after(bytes, record_offset, scan_end)
            .min(bytes.len());
    if record_end <= record_offset {
        return false;
    }

    let opcode = bytes[record_offset];
    let object_type = bytes[record_offset + 1];
    match opcode {
        b'A' if object_type == CREATURE_OBJECT_TYPE => {
            looks_like_legacy_creature_add_transform_fields(bytes, record_offset, record_end)
        }
        b'A' if object_type == 0x09 || object_type == 0x0A => {
            record_end - record_offset >= minimum_legacy_live_object_record_length_at(bytes, record_offset)
        }
        b'U' if matches!(object_type, 0x05 | 0x07 | 0x09 | 0x0A) => {
            let Some(raw_mask) = read_u32_le(bytes, record_offset + 6) else {
                return false;
            };
            raw_mask != 0 && raw_mask != u32::MAX
        }
        b'D' if matches!(object_type, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => {
            record_end - record_offset <= 16
        }
        b'I' => true,
        b'G' | b'W' => true,
        _ => false,
    }
}

fn find_next_legacy_live_object_sub_message_boundary_after(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> usize {
    let scan_end = search_end.min(bytes.len());
    if offset >= scan_end {
        return scan_end;
    }

    let start = scan_end.min(offset + minimum_legacy_live_object_record_length_at(bytes, offset));
    let suppress_inline_string_boundaries = bytes.get(offset).copied() != Some(b'I');
    let string_scan_start = (offset + 2).min(scan_end);
    for candidate in start..scan_end.saturating_sub(1) {
        if suppress_inline_string_boundaries
            && candidate_inside_inline_string(bytes, string_scan_start, candidate)
        {
            continue;
        }
        if looks_like_legacy_live_object_sub_message_boundary(bytes, candidate) {
            return candidate;
        }
    }
    scan_end
}

fn minimum_legacy_live_object_record_length_at(bytes: &[u8], offset: usize) -> usize {
    if !looks_like_legacy_live_object_sub_message_boundary(bytes, offset) {
        return 2;
    }

    let opcode = bytes[offset];
    let marker = bytes[offset + 1];
    match (opcode, marker) {
        (b'A', 0x05) => CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET,
        (b'A', PLACEABLE_OBJECT_TYPE) => {
            let name_offset = offset + 6;
            if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(5).saturating_sub(offset);
            }
            11
        }
        (b'A', DOOR_OBJECT_TYPE) => {
            let Some(first_dword) = read_u32_le(bytes, offset + 6) else {
                return 16;
            };
            let name_offset = offset + 2 + if first_dword == 0 { 12 } else { 8 };
            if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(2).saturating_sub(offset);
            }
            16
        }
        (b'U' | b'P', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 10,
        (b'D', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 6,
        (b'I', _) => 7,
        (b'W', _) if marker <= 0x0F && bytes.get(offset + 2) == Some(&0x0E) => 3,
        _ => 2,
    }
}

fn looks_like_legacy_live_object_sub_message_boundary(bytes: &[u8], offset: usize) -> bool {
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

    let legacy_item_sentinel = marker == 0xFD
        && bytes.len() - offset >= 5
        && bytes[offset + 2] == 0xFF
        && bytes[offset + 3] == 0xFF
        && bytes[offset + 4] == 0xFF;
    if opcode == b'I'
        && (marker == 0x05
            || marker == 0xC5
            || legacy_item_sentinel
            || looks_like_legacy_live_object_id_at(bytes, offset + 1))
    {
        return true;
    }

    let gui_object_boundary = opcode == b'G'
        && is_ee_live_gui_sub_opcode_byte(marker)
        && bytes.len() - offset >= 9
        && looks_like_legacy_live_gui_object_id_at(bytes, offset + 5);
    if gui_object_boundary {
        return true;
    }

    let gui_repository_boundary = opcode == b'G'
        && matches!(marker, b'R' | b'r')
        && bytes.len() - offset >= 3
        && ((matches!(bytes[offset + 2], b'A' | b'M')
            && bytes.len() - offset >= 9
            && looks_like_legacy_live_gui_object_id_at(bytes, offset + 5))
            || (matches!(bytes[offset + 2], b'D' | b'U')
                && bytes.len() - offset >= 7
                && looks_like_legacy_live_gui_object_id_at(bytes, offset + 3)));
    if gui_repository_boundary {
        return true;
    }

    let gui_inventory_boundary = opcode == b'G'
        && matches!(marker, b'I' | b'i')
        && bytes.len() - offset >= 3
        && (matches!(bytes[offset + 2], b'D' | b'U')
            || (bytes[offset + 2] == b'A'
                && bytes.len() - offset >= 15
                && looks_like_legacy_live_gui_object_id_at(bytes, offset + 7)));
    if gui_inventory_boundary {
        return true;
    }

    opcode == b'W' && bytes.len() - offset >= 3 && marker <= 0x0F && bytes[offset + 2] == 0x0E
}

fn looks_like_legacy_creature_add_transform_fields(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if offset > bytes.len()
        || record_end > bytes.len()
        || record_end < offset + CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    if (object_id & 0x8000_0000) == 0 || read_u16_le(bytes, offset + 30).is_none() {
        return false;
    }

    for index in 0..6 {
        let Some(value) = read_f32_le(bytes, offset + 6 + index * 4) else {
            return false;
        };
        if !value.is_finite() || value.abs() > 1_000_000_000.0 {
            return false;
        }
    }
    true
}

fn legacy_add_visual_transform_insert_offset(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<usize> {
    if offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= offset
        || bytes.len() - offset < 10
        || bytes[offset] != b'A'
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    match bytes[offset + 1] {
        CREATURE_OBJECT_TYPE => {
            let insert_offset = offset + CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET;
            if record_end >= insert_offset
                && insert_offset == record_end
                && looks_like_legacy_creature_add_transform_fields(bytes, offset, record_end)
            {
                Some(insert_offset)
            } else {
                None
            }
        }
        DOOR_OBJECT_TYPE => {
            try_find_legacy_door_add_visual_transform_insert_offset(bytes, offset, record_end)
        }
        PLACEABLE_OBJECT_TYPE => {
            try_find_legacy_placeable_add_visual_transform_insert_offset(bytes, offset, record_end)
        }
        _ => None,
    }
}

fn try_find_legacy_door_add_visual_transform_insert_offset(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let first_dword = read_u32_le(bytes, record_offset + 6)?;
    let insert_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };
    if insert_offset > record_end {
        return None;
    }

    // EE `sub_140796DD0` calls `sub_140973160` at this cursor, then consumes a
    // fragment BOOL selecting short-locstring vs inline `CExoString` name data.
    // Accept either verified inline string data or the compact legacy short-name
    // shape where the read-buffer side only has a four-byte name token and a
    // two-byte state tail.
    if let Some(inline_end) = inline_cexo_string_end(bytes, insert_offset) {
        if inline_end <= record_end && record_end - inline_end >= 2 {
            return Some(insert_offset);
        }
    }

    let short_tail_end = insert_offset.checked_add(6)?;
    if short_tail_end <= record_end {
        Some(insert_offset)
    } else {
        None
    }
}

fn try_find_legacy_placeable_add_visual_transform_insert_offset(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let name_offset = record_offset + 6;
    let tail_offset = inline_cexo_string_end(bytes, name_offset).unwrap_or(name_offset + 4);
    let legacy_tail_end = legacy_placeable_add_tail_end(bytes, tail_offset, record_end)?;

    // EE `sub_1407A7800` reads name, type byte, appearance/static WORD fields
    // and paired fragment BOOLs before the add-record visual-transform map.
    Some(legacy_tail_end)
}

fn legacy_placeable_add_tail_end(
    bytes: &[u8],
    tail_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if tail_offset > record_end || tail_end > record_end || tail_end > bytes.len() {
        return None;
    }
    if tail_end == record_end || has_ee_identity_visual_transform_map_at(bytes, tail_end, record_end)
    {
        Some(tail_end)
    } else {
        None
    }
}

fn has_ee_identity_visual_transform_map_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let end = offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
    end <= record_end
        && end <= bytes.len()
        && bytes[offset..end] == EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES
}

fn candidate_inside_inline_string(bytes: &[u8], search_start: usize, candidate: usize) -> bool {
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

fn inline_cexo_string_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let length = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    const MAX_LIVE_OBJECT_NAME_BYTES: usize = 128;
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

fn looks_like_legacy_live_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(looks_like_legacy_live_object_id_value)
        .unwrap_or(false)
}

fn looks_like_legacy_live_gui_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(|object_id| {
            object_id == 0x7F00_0000
                || object_id == u32::MAX
                || looks_like_legacy_live_object_id_value(object_id)
        })
        .unwrap_or(false)
}

fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }

    // EE live-object client readers use `sub_1409737C0`, which is a raw
    // four-byte `CNWMessage` read. Unlike `ReadOBJECTIDServer`, it does not
    // clear or constrain the high bit. Legacy HG captures therefore contain
    // valid object ids such as `0xEC0034D1`; rejecting those made the normalizer
    // skip real door/placeable records and "salvage" from a later false
    // boundary.
    if (object_id & 0xFF00_0000) != 0 {
        return true;
    }

    let high_byte = object_id & 0xFF00_0000;
    matches!(
        high_byte,
        0x8000_0000 | 0x8800_0000 | 0xFF00_0000 | 0x0100_0000 | 0x0500_0000
    ) || (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID)
        .contains(&object_id)
}

fn is_ee_live_gui_sub_opcode_byte(value: u8) -> bool {
    matches!(
        value,
        b'A' | b'B' | b'C' | b'I' | b'M' | b'Q' | b'R' | b'S' | b'c' | b'i' | b'r'
    )
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Option<f32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

const _: () = {
    assert!(
        EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()
            == EE_LEGACY_VISUAL_TRANSFORM_LERP_FLOAT_COUNT * CNW_LENGTH_BYTES
    );
};
