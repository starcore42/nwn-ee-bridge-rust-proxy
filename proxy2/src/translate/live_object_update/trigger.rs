//! Verified trigger `A` add-record geometry shape.
//!
//! EE `CNWSMessage::AddTriggerGeometryToMessage` writes a BYTE vertex count
//! followed by that many XYZ float triples. Diamond/HG uses the same geometry
//! block inside live-object trigger adds; no semantic byte rewrite is required,
//! but the exact validator must still own the shape so trigger records do not
//! fall through a generic live-object claim.

use super::{TRIGGER_OBJECT_TYPE, boundary};

pub(super) const TRIGGER_ADD_MIN_RECORD_BYTES: usize = 16;
const TRIGGER_GEOMETRY_COUNT_OFFSET: usize = 15;
const TRIGGER_VERTEX_FLOATS: usize = 3;
const FLOAT_BYTES: usize = 4;
const TRIGGER_VERTEX_BYTES: usize = TRIGGER_VERTEX_FLOATS * FLOAT_BYTES;

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
