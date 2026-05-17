//! Fragment guard repair for already EE-shaped live-object add records.
//!
//! This module is intentionally narrower than `add.rs`: it does not classify
//! every add record and it does not insert byte fields. It exists for the
//! second/final update-family pass where earlier door/placeable `U` records may
//! have inserted EE-only orientation branch bits ahead of an already translated
//! `A09` placeable add. At that point the byte side can be EE-shaped while the
//! post-name fragment guards still reflect Diamond/HG positions.

use super::{PLACEABLE_OBJECT_TYPE, creature, locstring, read_u16_le};

const EE_VISUAL_TRANSFORM_BYTES: usize =
    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;

pub(super) fn repair_verified_ee_placeable_add_guard_bits(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut [bool],
    bit_cursor: &mut usize,
) -> Option<bool> {
    if offset + 6 > record_end
        || record_end > live_bytes.len()
        || live_bytes.get(offset).copied() != Some(b'A')
        || live_bytes.get(offset + 1).copied() != Some(PLACEABLE_OBJECT_TYPE)
        || *bit_cursor >= fragment_bits.len()
    {
        return None;
    }

    let name_offset = offset + 6;
    let inline_name_end = locstring::inline_cexo_string_end(live_bytes, name_offset);
    let direct_inline_name =
        inline_name_end.is_some_and(|end| end > name_offset + super::CNW_LENGTH_BYTES);
    let tail_offset = inline_name_end.unwrap_or(name_offset.checked_add(4)?);
    let tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if tail_end > record_end || tail_end > live_bytes.len() {
        return None;
    }
    read_u16_le(live_bytes, tail_offset + 1)?;
    read_u16_le(live_bytes, tail_offset + 3)?;

    // Decompile anchors:
    //
    // - Diamond `sub_44E4A0` reads the placeable add tail as a type byte, two
    //   WORDs, one optional-object BOOL/OBJECTID branch, and seven trailing
    //   state BOOLs.
    // - EE `sub_1407A7800` keeps that optional-object branch byte-identical,
    //   then reads one additional trailing BOOL immediately before
    //   `ObjectVisualTransformData::Read`.
    //
    // Therefore the optional-object BOOL must match the presence of four
    // OBJECTID bytes at the decompiled cursor, and the EE-only final BOOL is
    // false for the identity transform emitted by the bridge.
    let optional_object_bytes_present =
        if creature::has_ee_identity_visual_transform_map_at(live_bytes, tail_end, record_end)
            && tail_end + EE_VISUAL_TRANSFORM_BYTES == record_end
        {
            false
        } else {
            let optional_end = tail_end.checked_add(4)?;
            if optional_end <= record_end
                && creature::has_ee_identity_visual_transform_map_at(
                    live_bytes,
                    optional_end,
                    record_end,
                )
                && optional_end + EE_VISUAL_TRANSFORM_BYTES == record_end
            {
                true
            } else {
                return None;
            }
        };

    let mut changed = false;
    let outer_locstring = fragment_bits.get(*bit_cursor).copied()?;
    let destination_name_inner_bits = if outer_locstring {
        if direct_inline_name && optional_object_bytes_present {
            // Direct inline CExoString names are selected by outer=false in EE
            // `sub_1407A7800`. Some Diamond/HG captures reach this already
            // EE-shaped byte layout while the legacy fragment cursor still has
            // outer=true and four optional-object bytes present; the following
            // bit remains the first post-name state bit rather than an EE
            // locstring inner selector. Non-optional direct-name captures keep
            // the older locstring-inline branch proven by existing fixtures.
            changed |= set_bit(fragment_bits, *bit_cursor, false)?;
            0
        } else {
            let inner_client_tlk = fragment_bits.get(*bit_cursor + 1).copied()?;
            if inner_client_tlk {
                return None;
            }
            1
        }
    } else {
        0
    };
    let post_name_bit = bit_cursor.checked_add(1 + destination_name_inner_bits)?;
    if fragment_bits.len() <= post_name_bit + 9 {
        return None;
    }

    changed |= set_bit(
        fragment_bits,
        post_name_bit + 1,
        optional_object_bytes_present,
    )?;
    changed |= set_bit(fragment_bits, post_name_bit + 9, false)?;
    *bit_cursor = post_name_bit.saturating_add(10);
    Some(changed)
}

fn set_bit(bits: &mut [bool], bit_index: usize, value: bool) -> Option<bool> {
    let bit = bits.get_mut(bit_index)?;
    let changed = *bit != value;
    *bit = value;
    Some(changed)
}
