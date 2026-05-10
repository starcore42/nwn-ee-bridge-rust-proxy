//! Bounded legacy live-object update readers.
//!
//! The update path deliberately keeps byte parsing separate from mutation:
//! callers first parse a bounded legacy or EE shape into one of these tiny
//! structs, then the writer/validator decides what to emit or claim.

use super::{
    DOOR_OBJECT_TYPE, EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS,
    EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES, EE_UPDATE_SCALE_STATE_READ_BYTES,
    LEGACY_UPDATE_HEADER_BYTES, LEGACY_UPDATE_NAME_MASK, LEGACY_UPDATE_ORIENTATION_MASK,
    LEGACY_UPDATE_POSITION_FRAGMENT_BITS, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_POSITION_READ_BYTES, LEGACY_UPDATE_SCALE_STATE_MASK,
    LEGACY_UPDATE_STATE_FRAGMENT_BITS, LEGACY_UPDATE_STATE_MASK, MAX_LIVE_OBJECT_NAME_BYTES,
    PLACEABLE_OBJECT_TYPE, read_f32_le, read_u16_le, read_u32_le,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyNamedUpdateTail {
    pub(super) facing: u16,
    pub(super) scale_raw: u32,
    pub(super) generic_state_word: u16,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VerifiedEeDoorPlaceableUpdateRecord {
    pub(super) read_end: usize,
    pub(super) next_bit_cursor: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyInlineNamedDoorPlaceableUpdateRecord {
    pub(super) read_without_name_end: usize,
    pub(super) name_end: usize,
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

    read_u32_le(bytes, name_offset) == Some(0) && record_end.saturating_sub(name_offset + 4) <= 4
}

pub(super) fn parse_legacy_inline_named_door_placeable_update_record_for_ee(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<LegacyInlineNamedDoorPlaceableUpdateRecord> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U' {
        return None;
    }
    let object_type = bytes.get(offset + 1).copied()?;
    if !matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
        return None;
    }

    let mask = read_u32_le(bytes, offset + 6)?;
    let allowed_mask = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_NAME_MASK;
    if (mask & LEGACY_UPDATE_NAME_MASK) == 0 || (mask & !allowed_mask) != 0 {
        return None;
    }

    // Diamond `CNWSMessage::WriteGameObjUpdate_UpdateObject` writes the
    // generic door/placeable fields in decompile order:
    //
    // 1. position (`0x1`) as the shared packed XYZ field,
    // 2. orientation (`0x2`) as the scalar branch BOOL plus `WriteFLOAT(10,12)`,
    // 3. scale/state (`0x4`) as FLOAT scale plus WORD generic state,
    // 4. state (`0x10`) in fragment BOOLs only,
    // 5. legacy name (`0x80000`) as a fragment presence BOOL plus a CExoString
    //    or locstring payload.
    //
    // EE's generic update reader consumes the same first three read-buffer
    // fields but does not have the final bit-13 name branch. This parser proves
    // the legacy inline-name byte span so the translator can drop exactly that
    // Diamond-only branch instead of scanning around it.
    let mut read_cursor = offset + LEGACY_UPDATE_HEADER_BYTES;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        let scale = read_f32_le(bytes, read_cursor)?;
        if !is_plausible_legacy_object_scale(scale) {
            return None;
        }
        read_cursor = read_cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    }
    if read_cursor > record_end {
        return None;
    }

    let name_end = inline_cexo_string_end(bytes, read_cursor)?;
    if name_end != record_end {
        return None;
    }

    Some(LegacyInlineNamedDoorPlaceableUpdateRecord {
        read_without_name_end: read_cursor,
        name_end,
    })
}

pub(super) fn parse_verified_ee_door_placeable_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<VerifiedEeDoorPlaceableUpdateRecord> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U' {
        return None;
    }
    let object_type = bytes.get(offset + 1).copied()?;
    if !matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
        return None;
    }

    let mask = read_u32_le(bytes, offset + 6)?;
    // EE generic door/placeable updates are verified against the actual EE
    // reader/writer shape. `sub_14079C050` consumes position, orientation,
    // scale/state, and state here; unlike Diamond's generic update path, it has
    // no bit-13 / 0x0008_0000 name branch. A translated EE packet carrying that
    // legacy bit is therefore invalid even if its byte layout looks plausible.
    let allowed_mask = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK;
    if mask == 0 || (mask & !allowed_mask) != 0 {
        return None;
    }

    let mut read_cursor = offset + LEGACY_UPDATE_HEADER_BYTES;
    let mut fragment_cursor = bit_cursor;
    let debug_live_claim = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();

    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
        if read_cursor > record_end {
            return None;
        }
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
        )?;
    }

    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        // EE `sub_14079C050` first reads a BOOL branch for generic
        // orientation. The bridge writer only emits the scalar branch
        // (`false`) plus the 12-bit `ReadFLOAT(10.0,12)` payload.
        read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?;
        if read_cursor > record_end || fragment_bits.get(fragment_cursor).copied()? {
            if debug_live_claim {
                eprintln!(
                    "door/placeable update reject orientation offset={offset} record_end={record_end} bit_cursor={bit_cursor} fragment_cursor={fragment_cursor} branch={:?} read_cursor={read_cursor}",
                    fragment_bits.get(fragment_cursor).copied()
                );
            }
            return None;
        }
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS,
        )?;
    }

    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        let scale = read_f32_le(bytes, read_cursor)?;
        if !is_plausible_legacy_object_scale(scale) {
            if debug_live_claim {
                eprintln!(
                    "door/placeable update reject scale offset={offset} record_end={record_end} read_cursor={read_cursor} scale={scale}"
                );
            }
            return None;
        }
        read_cursor = read_cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
        if read_cursor > record_end {
            if debug_live_claim {
                eprintln!(
                    "door/placeable update reject scale cursor offset={offset} record_end={record_end} read_cursor={read_cursor}"
                );
            }
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_STATE_FRAGMENT_BITS,
        )?;
        if object_type == DOOR_OBJECT_TYPE {
            // EE's door update path has one extra BOOL beyond the five
            // Diamond state bits. The translator inserts `false`, so an exact
            // claimed bridge packet must still carry that neutral branch.
            if fragment_bits.get(fragment_cursor).copied()? {
                return None;
            }
            fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
        }
    }

    if read_cursor != record_end {
        if debug_live_claim {
            eprintln!(
                "door/placeable update reject read cursor offset={offset} record_end={record_end} read_cursor={read_cursor} mask=0x{mask:08X}"
            );
        }
        return None;
    }

    Some(VerifiedEeDoorPlaceableUpdateRecord {
        read_end: read_cursor,
        next_bit_cursor: fragment_cursor,
    })
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

fn advance_bits(bits: &[bool], cursor: usize, count: usize) -> Option<usize> {
    if bits.len().saturating_sub(cursor) < count {
        return None;
    }
    cursor.checked_add(count)
}

fn is_plausible_legacy_object_scale(scale: f32) -> bool {
    scale.is_finite() && (0.01..=100.0).contains(&scale)
}
