//! Verified trigger `A` add-record geometry shape.
//!
//! EE `CNWSMessage::AddTriggerGeometryToMessage` writes a BYTE vertex count
//! followed by that many XYZ float triples. Diamond/HG uses the same geometry
//! block inside live-object trigger adds; no semantic byte rewrite is required,
//! but the exact validator must still own the shape so trigger records do not
//! fall through a generic live-object claim.

use super::{
    LEGACY_UPDATE_HEADER_BYTES, LEGACY_UPDATE_POSITION_FRAGMENT_BITS, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_POSITION_READ_BYTES, TRIGGER_OBJECT_TYPE, boundary, read_u32_le,
};

pub(super) const TRIGGER_ADD_MIN_RECORD_BYTES: usize = 16;
const TRIGGER_GEOMETRY_COUNT_OFFSET: usize = 15;
const TRIGGER_VERTEX_FLOATS: usize = 3;
const FLOAT_BYTES: usize = 4;
const TRIGGER_VERTEX_BYTES: usize = TRIGGER_VERTEX_FLOATS * FLOAT_BYTES;
const LEGACY_HG_TRIGGER_UPDATE_MASK_WITH_POSITION_TAIL: u32 = 0xFFFF_FFF3;
const LEGACY_HG_TRIGGER_UPDATE_EXTRA_READ_BYTES: usize = 3;

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
    if offset + TRIGGER_ADD_MIN_RECORD_BYTES > scan_end
        || offset + TRIGGER_ADD_MIN_RECORD_BYTES > bytes.len()
        || bytes.get(offset).copied() != Some(b'A')
        || bytes.get(offset + 1).copied() != Some(TRIGGER_OBJECT_TYPE)
        || !boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let vertex_count = bytes[offset + TRIGGER_GEOMETRY_COUNT_OFFSET] as usize;
    let geometry_bytes = vertex_count.checked_mul(TRIGGER_VERTEX_BYTES)?;
    let record_end = offset
        .checked_add(TRIGGER_ADD_MIN_RECORD_BYTES)?
        .checked_add(geometry_bytes)?;
    if record_end <= scan_end && record_end <= bytes.len() {
        Some(record_end)
    } else {
        None
    }
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
    if !verified_ee_trigger_add_record(bytes, record_offset, record_end) {
        return false;
    }

    // `AddTriggerGeometryToMessage` writes BYTE/FLOAT fields only. It does not
    // read or write CNW fragment BOOLs, so a verified trigger add leaves the
    // shared fragment cursor unchanged.
    *bit_cursor <= bits.len()
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
    if raw_mask != LEGACY_HG_TRIGGER_UPDATE_MASK_WITH_POSITION_TAIL
        || translated_mask != LEGACY_UPDATE_POSITION_MASK
        || legacy_record_end != record_end
        || bits.len().saturating_sub(bit_cursor) < LEGACY_UPDATE_POSITION_FRAGMENT_BITS
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
