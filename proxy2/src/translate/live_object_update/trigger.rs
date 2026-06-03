//! Verified trigger `A` add-record shape.
//!
//! Diamond `sub_4552E0` and EE `sub_1407B1670` read the same trigger add
//! envelope: object id, name selector/payload, two state BOOLs, an optional
//! third state BOOL when the first state BOOL is true, cursor BYTE, height
//! FLOAT, vertex count BYTE, and XYZ FLOAT triples. No semantic byte rewrite is
//! required, but the exact validator must still own both the read-buffer shape
//! and the source fragment BOOL span so following live-object records stay
//! aligned.

use super::{
    LEGACY_UPDATE_HEADER_BYTES, LEGACY_UPDATE_POSITION_FRAGMENT_BITS, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_POSITION_READ_BYTES, TRIGGER_OBJECT_TYPE, boundary, locstring, read_f32_le,
    read_u32_le,
};

pub(super) const TRIGGER_ADD_MIN_RECORD_BYTES: usize = 16;
const TRIGGER_SHORT_NAME_BYTES: usize = 4;
const TRIGGER_POST_NAME_FIXED_BYTES: usize = 6;
const TRIGGER_CURSOR_BYTES: usize = 1;
const TRIGGER_HEIGHT_BYTES: usize = 4;
const TRIGGER_VERTEX_FLOATS: usize = 3;
const FLOAT_BYTES: usize = 4;
const TRIGGER_VERTEX_BYTES: usize = TRIGGER_VERTEX_FLOATS * FLOAT_BYTES;
const TRIGGER_MAX_VERTICES: usize = 64;
const TRIGGER_MAX_ABS_VERTEX_COORDINATE: f32 = 100_000.0;
const TRIGGER_MIN_HEIGHT: f32 = -1000.0;
const TRIGGER_MAX_HEIGHT: f32 = 1000.0;
const LEGACY_HG_TRIGGER_UPDATE_MASK_WITH_POSITION_TAIL: u32 = 0xFFFF_FFF3;
const LEGACY_HG_TRIGGER_UPDATE_EXTRA_READ_BYTES: usize = 3;

#[derive(Debug, Clone, Copy)]
struct TriggerAddShape {
    cursor_offset: usize,
    vertex_count: usize,
    record_end: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyTriggerUpdateRecord {
    pub(super) raw_mask: u32,
    pub(super) translated_mask: u32,
    pub(super) position_read_end: usize,
    pub(super) next_bit_cursor: usize,
}

pub(super) fn try_get_trigger_add_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    read_trigger_add_shape(bytes, offset, scan_end).map(|shape| shape.record_end)
}

pub(super) fn trigger_add_geometry_start_and_count(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<(usize, usize)> {
    let shape = read_trigger_add_shape(bytes, offset, record_end)?;
    (shape.record_end == record_end).then_some((
        shape.cursor_offset + TRIGGER_CURSOR_BYTES + TRIGGER_HEIGHT_BYTES + 1,
        shape.vertex_count,
    ))
}

fn read_trigger_add_shape(bytes: &[u8], offset: usize, scan_end: usize) -> Option<TriggerAddShape> {
    if offset + TRIGGER_ADD_MIN_RECORD_BYTES > scan_end
        || offset + TRIGGER_ADD_MIN_RECORD_BYTES > bytes.len()
        || bytes.get(offset).copied() != Some(b'A')
        || bytes.get(offset + 1).copied() != Some(TRIGGER_OBJECT_TYPE)
        || !boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let short_cursor_offset = offset.checked_add(6 + TRIGGER_SHORT_NAME_BYTES)?;
    let mut cursor_offsets = Vec::with_capacity(2);
    if let Some(direct_name_end) = locstring::inline_cexo_string_end(bytes, offset + 6) {
        if direct_name_end >= short_cursor_offset
            && direct_name_end != short_cursor_offset
            && direct_name_end
                .checked_add(TRIGGER_POST_NAME_FIXED_BYTES)
                .is_some_and(|end| end <= scan_end.min(bytes.len()))
        {
            cursor_offsets.push(direct_name_end);
        }
    }
    cursor_offsets.push(short_cursor_offset);

    cursor_offsets
        .into_iter()
        .find_map(|cursor_offset| read_trigger_add_shape_at_cursor(bytes, cursor_offset, scan_end))
}

fn read_trigger_add_shape_at_cursor(
    bytes: &[u8],
    cursor_offset: usize,
    scan_end: usize,
) -> Option<TriggerAddShape> {
    let fixed_end = cursor_offset.checked_add(TRIGGER_POST_NAME_FIXED_BYTES)?;
    if fixed_end > scan_end.min(bytes.len()) {
        return None;
    }

    let height = read_f32_le(bytes, cursor_offset + TRIGGER_CURSOR_BYTES)?;
    if !height.is_finite() || !(TRIGGER_MIN_HEIGHT..=TRIGGER_MAX_HEIGHT).contains(&height) {
        return None;
    }

    let vertex_count = bytes[cursor_offset + TRIGGER_CURSOR_BYTES + TRIGGER_HEIGHT_BYTES] as usize;
    if vertex_count == 0 || vertex_count > TRIGGER_MAX_VERTICES {
        return None;
    }

    let geometry_start = fixed_end;
    let geometry_bytes = vertex_count.checked_mul(TRIGGER_VERTEX_BYTES)?;
    let record_end = geometry_start.checked_add(geometry_bytes)?;
    if record_end > scan_end || record_end > bytes.len() {
        return None;
    }

    let mut cursor = geometry_start;
    for _ in 0..vertex_count {
        for _ in 0..TRIGGER_VERTEX_FLOATS {
            let value = read_f32_le(bytes, cursor)?;
            if !value.is_finite() || value.abs() > TRIGGER_MAX_ABS_VERTEX_COORDINATE {
                return None;
            }
            cursor = cursor.checked_add(FLOAT_BYTES)?;
        }
    }

    Some(TriggerAddShape {
        cursor_offset,
        vertex_count,
        record_end,
    })
}

pub(super) fn verified_ee_trigger_add_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    try_get_trigger_add_record_end(bytes, offset, record_end) == Some(record_end)
}

pub(super) fn advance_trigger_add_bit_cursor(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    // Diamond and EE both read the name selector before the trigger state
    // BOOLs. The locstring branch owns the client-TLK selector bit plus a
    // DWORD strref in the read buffer; the direct branch owns only the outer
    // selector before `ReadCExoString(32)`.
    let Some(name_is_locstring) = bits.get(*bit_cursor).copied() else {
        return false;
    };
    if !trigger_add_shape_matches_name_mode(bytes, record_offset, record_end, name_is_locstring) {
        return false;
    }

    let minimum_bits = if name_is_locstring { 4 } else { 3 };
    if bits.len().saturating_sub(*bit_cursor) < minimum_bits {
        return false;
    }

    let first_state_bit = *bit_cursor + if name_is_locstring { 2 } else { 1 };
    let first_state = bits[first_state_bit];
    let source_bits = minimum_bits + usize::from(first_state);
    if bits.len().saturating_sub(*bit_cursor) < source_bits {
        return false;
    }

    *bit_cursor = (*bit_cursor).saturating_add(source_bits);
    true
}

fn trigger_add_shape_matches_name_mode(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    name_is_locstring: bool,
) -> bool {
    if bytes.get(record_offset).copied() != Some(b'A')
        || bytes.get(record_offset + 1).copied() != Some(TRIGGER_OBJECT_TYPE)
        || !boundary::looks_like_legacy_live_object_id_at(bytes, record_offset + 2)
    {
        return false;
    }

    let Some(name_cursor) = record_offset.checked_add(6) else {
        return false;
    };
    let Some(cursor_offset) = (if name_is_locstring {
        // The locstring/token branch is a compact DWORD token in the read
        // buffer. It does not own direct CExoString bytes, even if the token
        // DWORD is also byte-plausible as a length field.
        name_cursor.checked_add(TRIGGER_SHORT_NAME_BYTES)
    } else {
        locstring::inline_cexo_string_end(bytes, name_cursor)
    }) else {
        return false;
    };

    read_trigger_add_shape_at_cursor(bytes, cursor_offset, record_end)
        .is_some_and(|shape| shape.record_end == record_end)
}

pub(super) fn parse_legacy_trigger_update_for_ee(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    bits: &[bool],
    bit_cursor: usize,
) -> Option<LegacyTriggerUpdateRecord> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(TRIGGER_OBJECT_TYPE)
        || !boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let raw_mask = read_u32_le(bytes, offset + 6)?;
    let translated_mask = raw_mask & LEGACY_UPDATE_POSITION_MASK;
    let position_read_end = offset
        .checked_add(LEGACY_UPDATE_HEADER_BYTES)?
        .checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    let legacy_record_end =
        position_read_end.checked_add(LEGACY_HG_TRIGGER_UPDATE_EXTRA_READ_BYTES)?;

    // HG/Diamond trigger updates in the captured live-object burst carry a
    // legacy trigger-specific three-byte read tail after the generic position
    // read block. EE's generic `WriteGameObjUpdate_UpdateObject` position path
    // is the common decompile-owned subset: `U`, object type, object id, mask,
    // then position mask `0x0001` as three WORD read-buffer fields plus two
    // CNW fragment bits. Keep this deliberately exact so any future trigger
    // mask/tail shape is quarantined until researched instead of guessing at a
    // shifted bit cursor.
    let remaining_fragment_bits = bits.len().saturating_sub(bit_cursor);
    let terminal_record = record_end == bytes.len();
    if raw_mask != LEGACY_HG_TRIGGER_UPDATE_MASK_WITH_POSITION_TAIL
        || translated_mask != LEGACY_UPDATE_POSITION_MASK
        || legacy_record_end != record_end
        || remaining_fragment_bits < LEGACY_UPDATE_POSITION_FRAGMENT_BITS
        || (terminal_record && remaining_fragment_bits != LEGACY_UPDATE_POSITION_FRAGMENT_BITS)
    {
        return None;
    }

    Some(LegacyTriggerUpdateRecord {
        raw_mask,
        translated_mask,
        position_read_end,
        next_bit_cursor: bit_cursor + LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
    })
}

pub(super) fn trigger_update_record_end_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(TRIGGER_OBJECT_TYPE)
        || !boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let raw_mask = read_u32_le(bytes, offset + 6)?;
    let position_read_end = offset
        .checked_add(LEGACY_UPDATE_HEADER_BYTES)?
        .checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    let record_end = match raw_mask {
        LEGACY_UPDATE_POSITION_MASK => position_read_end,
        LEGACY_HG_TRIGGER_UPDATE_MASK_WITH_POSITION_TAIL => {
            position_read_end.checked_add(LEGACY_HG_TRIGGER_UPDATE_EXTRA_READ_BYTES)?
        }
        _ => return None,
    };

    if record_end <= scan_end.min(bytes.len()) {
        Some(record_end)
    } else {
        None
    }
}

pub(super) fn advance_trigger_update_fragment_cursor_for_transport(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if trigger_update_record_end_for_transport(bytes, offset, record_end) != Some(record_end) {
        return false;
    }

    let next_bit_cursor = bit_cursor.saturating_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS);
    if next_bit_cursor > bits.len() {
        return false;
    }
    *bit_cursor = next_bit_cursor;
    true
}

pub(super) fn advance_verified_ee_trigger_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(TRIGGER_OBJECT_TYPE)
        || !boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
        || read_u32_le(bytes, offset + 6)? != LEGACY_UPDATE_POSITION_MASK
    {
        return None;
    }

    let expected_end = offset
        .checked_add(LEGACY_UPDATE_HEADER_BYTES)?
        .checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    if expected_end != record_end
        || bits.len().saturating_sub(bit_cursor) < LEGACY_UPDATE_POSITION_FRAGMENT_BITS
    {
        return None;
    }

    Some(bit_cursor + LEGACY_UPDATE_POSITION_FRAGMENT_BITS)
}
