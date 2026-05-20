//! Verified live-object `A` add-record classifiers.
//!
//! Add records are deliberately claimed separately from `U` update records:
//! EE and Diamond both dispatch them through the live-object update reader, but
//! door/placeable add records need EE visual-transform storage inserted by the
//! focused `translate::live_object` transformer before this module can claim
//! the final shape. This file only validates cursor shape and advances the CNW
//! fragment-bit cursor; it never mutates bytes.

use super::{
    DOOR_OBJECT_TYPE, ITEM_OBJECT_TYPE, PLACEABLE_OBJECT_TYPE, TRIGGER_OBJECT_TYPE, appearance,
    boundary, creature, cursor, locstring, read_u16_le, read_u32_le, trigger,
};

pub(super) fn advance_verified_add_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let creature_add_object_id_ok = bytes.get(offset + 1).copied() == Some(0x05)
        && read_u32_le(bytes, offset + 2).is_some_and(|object_id| object_id != u32::MAX);

    if offset + 6 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'A')
        || (!boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
            && !creature_add_object_id_ok
            && !appearance::looks_like_legacy_item_add_record_boundary(bytes, offset))
    {
        return false;
    }

    let original_bit_cursor = *bit_cursor;
    if appearance::advance_verified_ee_item_add_record(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return true;
    }
    *bit_cursor = original_bit_cursor;
    if bytes.get(offset + 1).copied() == Some(ITEM_OBJECT_TYPE)
        && appearance::advance_verified_ee_item_create_record(
            bytes,
            offset + 2,
            record_end,
            fragment_bits,
            bit_cursor,
        )
    {
        return true;
    }
    *bit_cursor = original_bit_cursor;

    let shape_ok = match bytes[offset + 1] {
        0x05 => creature::looks_like_ee_creature_add_record(bytes, offset, record_end),
        TRIGGER_OBJECT_TYPE => trigger::verified_ee_trigger_add_record(bytes, offset, record_end),
        DOOR_OBJECT_TYPE => verified_ee_door_add_record(bytes, offset, record_end),
        PLACEABLE_OBJECT_TYPE => verified_ee_placeable_add_record(bytes, offset, record_end),
        _ => false,
    };
    let cursor_ok = shape_ok
        && cursor::advance_live_add_record_bit_cursor(
            bytes,
            fragment_bits,
            offset,
            record_end,
            bit_cursor,
        );
    let verified = shape_ok && cursor_ok;

    if !verified {
        if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
            eprintln!(
                "live-object add claim rejected: offset={offset} record_end={record_end} marker=0x{:02X} bit_cursor={} shape_ok={shape_ok} cursor_ok={cursor_ok} next_bits={:?}",
                bytes.get(offset + 1).copied().unwrap_or_default(),
                original_bit_cursor,
                fragment_bits
                    .get(
                        original_bit_cursor
                            ..original_bit_cursor
                                .saturating_add(12)
                                .min(fragment_bits.len())
                    )
                    .unwrap_or(&[])
            );
        }
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

    let name_offset =
        visual_offset + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;
    if name_offset > record_end {
        return false;
    }

    // EE `AddDoorAppearanceToMessage` writes one/two DWORDs, then
    // `ObjectVisualTransformData::Write`, then the existing door name branch.
    // The old Diamond-only optional model token is removed by
    // `translate::live_object`; after that, a legal EE door add ends with an
    // inline CExoString, a TLK-backed locstring ref, or the compact four-byte
    // empty-name token, followed by the two-byte door state tail.
    if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
        return inline_end + 2 == record_end && read_u16_le(bytes, inline_end).is_some();
    }

    if let Some(tlk_end) = locstring::tlk_locstring_ref_end(bytes, name_offset) {
        return tlk_end + 2 == record_end && read_u16_le(bytes, tlk_end).is_some();
    }

    name_offset + 6 == record_end && read_u16_le(bytes, name_offset + 4).is_some()
}

fn verified_ee_placeable_add_record(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    let name_offset = offset + 6;
    let tail_offset =
        locstring::inline_cexo_string_end(bytes, name_offset).unwrap_or(name_offset + 4);
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
    // tail, then a fragment BOOL guarding an optional OBJECTID, then more BOOLs
    // from the fragment stream, then `ObjectVisualTransformData::Write`.
    // Diamond's placeable reader has the same optional-object branch. The byte
    // validator therefore accepts either exact cursor: no guarded object id, or
    // a four-byte guarded OBJECTID immediately before the EE visual map. The
    // fragment cursor validator ties the chosen byte cursor back to the BOOL.
    if creature::has_ee_identity_visual_transform_map_at(bytes, tail_end, record_end)
        && tail_end + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
            == record_end
    {
        return true;
    }

    let Some(optional_object_end) = tail_end.checked_add(4) else {
        return false;
    };
    optional_object_end <= record_end
        && read_u32_le(bytes, tail_end).is_some()
        && creature::has_ee_identity_visual_transform_map_at(bytes, optional_object_end, record_end)
        && optional_object_end
            + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
            == record_end
}
