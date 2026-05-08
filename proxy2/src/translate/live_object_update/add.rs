//! Verified live-object `A` add-record classifiers.
//!
//! Add records are deliberately claimed separately from `U` update records:
//! EE and Diamond both dispatch them through the live-object update reader, but
//! door/placeable add records need EE visual-transform storage inserted by the
//! focused `translate::live_object` transformer before this module can claim
//! the final shape. This file only validates cursor shape and advances the CNW
//! fragment-bit cursor; it never mutates bytes.

use super::{
    boundary, creature, cursor, locstring, read_u16_le, read_u32_le, DOOR_OBJECT_TYPE,
    PLACEABLE_OBJECT_TYPE,
};

pub(super) fn advance_verified_add_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if offset + 6 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'A')
        || !boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return false;
    }

    let original_bit_cursor = *bit_cursor;
    let verified = match bytes[offset + 1] {
        0x05 => creature::looks_like_legacy_creature_add_transform_fields(bytes, offset, record_end),
        DOOR_OBJECT_TYPE => verified_ee_door_add_record(bytes, offset, record_end),
        PLACEABLE_OBJECT_TYPE => verified_ee_placeable_add_record(bytes, offset, record_end),
        _ => false,
    } && cursor::advance_live_add_record_bit_cursor(
        bytes,
        fragment_bits,
        offset,
        record_end,
        bit_cursor,
    );

    if !verified {
        *bit_cursor = original_bit_cursor;
    }
    verified
}

fn verified_ee_door_add_record(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    let Some(first_dword) = read_u32_le(bytes, offset + 6) else {
        return false;
    };
    let visual_offset = offset + 2 + if first_dword == 0 { 12 } else { 8 };
    if !creature::has_ee_identity_visual_transform_map_at(bytes, visual_offset, record_end) {
        return false;
    }

    let name_offset = visual_offset + 40;
    if name_offset > record_end {
        return false;
    }

    // EE `AddDoorAppearanceToMessage` writes one/two DWORDs, then
    // `ObjectVisualTransformData::Write`, then the existing door name branch.
    // The old Diamond-only optional model token is removed by
    // `translate::live_object`; after that, a legal EE door add ends with either
    // an inline CExoString plus the two-byte state tail, or the compact
    // four-byte empty-name token plus that same two-byte tail.
    if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
        return inline_end + 2 == record_end && read_u16_le(bytes, inline_end).is_some();
    }

    name_offset + 6 == record_end && read_u16_le(bytes, name_offset + 4).is_some()
}

fn verified_ee_placeable_add_record(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    let name_offset = offset + 6;
    let tail_offset = locstring::inline_cexo_string_end(bytes, name_offset).unwrap_or(name_offset + 4);
    let Some(tail_end) = tail_offset.checked_add(1 + 2 + 2) else {
        return false;
    };
    if tail_end > record_end || tail_end > bytes.len() {
        return false;
    }
    if read_u16_le(bytes, tail_offset + 1).is_none()
        || read_u16_le(bytes, tail_offset + 3).is_none()
    {
        return false;
    }

    // EE `AddPlaceableAppearanceToMessage` reads the name/type/appearance/static
    // tail, several BOOLs from the fragment stream, then
    // `ObjectVisualTransformData::Write`. A translated placeable add is
    // therefore claimable only when the identity map is present exactly at the
    // decompiled post-tail cursor and consumes the rest of the record.
    creature::has_ee_identity_visual_transform_map_at(bytes, tail_end, record_end)
        && tail_end + 40 == record_end
}
