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
    if !appearance::starts_with_typed_live_object_add_marker(bytes, offset) {
        if appearance::advance_verified_ee_item_add_record(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
        ) {
            return true;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ee_shaped_model_type2_typed_item_create_record() -> Vec<u8> {
        vec![
            b'A',
            ITEM_OBJECT_TYPE,
            0xB8,
            0x00,
            0x00,
            0x80,
            0x01,
            0x00,
            0x00,
            0x00,
            0x0C,
            0x00,
            0x0B,
            0x00,
            0x0B,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x05,
            0x00,
            0x00,
            0x00,
            b'L',
            b'a',
            b'n',
            b'c',
            b'e',
            0x02,
            0x00,
            0x00,
            0x00,
            0x01,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0xFF,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
        ]
    }

    #[test]
    fn typed_item_create_rewrite_keeps_following_bits_aligned() {
        // Local CEP starter evidence exposed this as a stock model-type-2 item
        // rule: an `A/6` typed create can already carry EE-shaped appearance
        // bytes while still missing EE's active-property BOOL. At a live-object
        // boundary, `A` followed by a typed object marker must use the typed
        // item-create reader, not the top-level visible-equipment add reader.
        let mut live = ee_shaped_model_type2_typed_item_create_record();
        let source_record_bits = [false, false, true, false, false];
        let following_record_bits = [false, true];
        let mut fragment_bits = source_record_bits
            .into_iter()
            .chain(following_record_bits)
            .collect::<Vec<_>>();

        let mut raw_cursor = 0usize;
        assert!(
            !advance_verified_add_record(&live, 0, live.len(), &fragment_bits, &mut raw_cursor,),
            "raw typed A/6 must not exact-claim through the top-level item-add shape"
        );
        assert_eq!(raw_cursor, 0);

        let mut record_end = live.len();
        let rewrite = appearance::insert_ee_item_create_extras_for_ee(
            &mut live,
            2,
            &mut record_end,
            &mut fragment_bits,
            0,
        )
        .expect("typed item-create should insert EE's missing active-property bit");
        assert_eq!(rewrite.bits_inserted, 1);
        assert_eq!(record_end, live.len());

        let mut cursor = 0usize;
        assert!(advance_verified_add_record(
            &live,
            0,
            record_end,
            &fragment_bits,
            &mut cursor,
        ));
        assert_eq!(
            cursor,
            source_record_bits.len() + rewrite.bits_inserted,
            "typed A/6 item-create must consume the rewritten item body/name/active-property cursor"
        );
        assert_eq!(
            &fragment_bits[cursor..],
            &following_record_bits,
            "rewriting the item-create row must preserve the following record bits"
        );
    }

    fn ee_placeable_add(optional_object_id: Option<u32>) -> Vec<u8> {
        let mut live = vec![b'A', PLACEABLE_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_0042u32.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.push(5);
        live.extend_from_slice(&0x0011u16.to_le_bytes());
        live.extend_from_slice(&0u16.to_le_bytes());
        if let Some(object_id) = optional_object_id {
            live.extend_from_slice(&object_id.to_le_bytes());
        }
        live.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        live
    }

    #[test]
    fn placeable_add_fragment_layout_ties_state_cursor_to_optional_branch() {
        let live = ee_placeable_add(Some(0x8000_1234));
        let bits = vec![
            true, false, // inline locstring helper branch: outer=true, inner=false.
            false, // reputation/visual selector.
            true,  // optional OBJECTID branch; bytes are present before the map.
            false, // static/plot.
            true,  // useable.
            false, // trap-disarmable.
            true,  // lockable.
            false, // locked.
            true,  // unknown sibling.
            true,  // name-valid.
            false, // EE-only visual-transform guard.
        ];

        let layout = verified_ee_placeable_add_fragment_layout(&live, 0, live.len(), &bits, 0)
            .expect("exact A/09 helper should own the optional-object fragment layout");
        assert_eq!(layout.post_name_bit, 2);
        assert_eq!(layout.next_bit_cursor, bits.len());
        assert!(layout.byte_layout.optional_object_id);

        let mut mismatched_optional = bits.clone();
        mismatched_optional[layout.post_name_bit + 1] = false;
        assert!(
            verified_ee_placeable_add_fragment_layout(
                &live,
                0,
                live.len(),
                &mismatched_optional,
                0,
            )
            .is_none(),
            "optional-object BOOL must match the guarded byte branch"
        );

        let mut tlk_inner_branch = bits.clone();
        tlk_inner_branch[1] = true;
        assert!(
            verified_ee_placeable_add_fragment_layout(&live, 0, live.len(), &tlk_inner_branch, 0,)
                .is_none(),
            "unimplemented inner locstring branch must not shift the state cursor"
        );

        let mut nonneutral_final_guard = bits;
        nonneutral_final_guard[layout.post_name_bit + 9] = true;
        assert!(
            verified_ee_placeable_add_fragment_layout(
                &live,
                0,
                live.len(),
                &nonneutral_final_guard,
                0,
            )
            .is_none(),
            "EE-only visual-transform guard must stay neutral until modeled"
        );
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerifiedEePlaceableAddLayout {
    pub(super) tail_offset: usize,
    pub(super) base_tail_end: usize,
    pub(super) optional_object_id: bool,
    pub(super) map_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerifiedEePlaceableAddFragmentLayout {
    pub(super) byte_layout: VerifiedEePlaceableAddLayout,
    pub(super) post_name_bit: usize,
    pub(super) next_bit_cursor: usize,
}

pub(super) fn verified_ee_placeable_add_layout(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<VerifiedEePlaceableAddLayout> {
    if offset + 6 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied()? != b'A'
        || bytes.get(offset + 1).copied()? != PLACEABLE_OBJECT_TYPE
    {
        return None;
    }
    let name_offset = offset + 6;
    let tail_offset =
        locstring::inline_cexo_string_end(bytes, name_offset).unwrap_or(name_offset + 4);
    let base_tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if base_tail_end > record_end || base_tail_end > bytes.len() {
        return None;
    }
    if read_u16_le(bytes, tail_offset + 1).is_none()
        || read_u16_le(bytes, tail_offset + 3).is_none()
    {
        return None;
    }

    // EE `AddPlaceableAppearanceToMessage` reads the name/type/appearance/static
    // tail, then a fragment BOOL guarding an optional OBJECTID, then more BOOLs
    // from the fragment stream, then `ObjectVisualTransformData::Write`.
    // Diamond's placeable reader has the same optional-object branch. The byte
    // validator therefore accepts either exact cursor: no guarded object id, or
    // a four-byte guarded OBJECTID immediately before the EE visual map. The
    // fragment cursor validator ties the chosen byte cursor back to the BOOL.
    if creature::has_ee_identity_visual_transform_map_at(bytes, base_tail_end, record_end)
        && base_tail_end + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
            == record_end
    {
        return Some(VerifiedEePlaceableAddLayout {
            tail_offset,
            base_tail_end,
            optional_object_id: false,
            map_offset: base_tail_end,
        });
    }

    let optional_object_end = base_tail_end.checked_add(4)?;
    if optional_object_end <= record_end
        && read_u32_le(bytes, base_tail_end).is_some()
        && creature::has_ee_identity_visual_transform_map_at(bytes, optional_object_end, record_end)
        && optional_object_end
            + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
            == record_end
    {
        return Some(VerifiedEePlaceableAddLayout {
            tail_offset,
            base_tail_end,
            optional_object_id: true,
            map_offset: optional_object_end,
        });
    }

    None
}

pub(super) fn verified_ee_placeable_add_fragment_layout(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<VerifiedEePlaceableAddFragmentLayout> {
    if bit_cursor >= fragment_bits.len() {
        return None;
    }

    let outer_locstring = fragment_bits.get(bit_cursor).copied()?;
    let destination_name_inner_bits = if outer_locstring {
        let inner_client_tlk = fragment_bits.get(bit_cursor + 1).copied()?;
        if inner_client_tlk {
            // EE `sub_1407A7800` routes outer=true into the locstring helper.
            // The bridge currently emits only the decompile-confirmed inline
            // CExoString/empty-name form: outer=true, inner=false. A true
            // inner bit would select the TLK/object-table branch and requires
            // a different typed byte parser, so exact validation must reject it.
            return None;
        }
        1
    } else {
        0
    };
    let post_name_bit = bit_cursor.checked_add(1 + destination_name_inner_bits)?;
    if fragment_bits.len() <= post_name_bit + 9 {
        return None;
    }

    let byte_layout = verified_ee_placeable_add_layout(bytes, offset, record_end)?;
    let optional_object_id = fragment_bits.get(post_name_bit + 1).copied()?;
    if optional_object_id != byte_layout.optional_object_id {
        return None;
    }
    if fragment_bits.get(post_name_bit + 9).copied()? {
        // EE adds one more trailing BOOL before its visual-transform map. The
        // bridge emits false until a captured/decompiled non-default field is
        // modeled explicitly.
        return None;
    }

    Some(VerifiedEePlaceableAddFragmentLayout {
        byte_layout,
        post_name_bit,
        next_bit_cursor: post_name_bit.saturating_add(10),
    })
}

fn verified_ee_placeable_add_record(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    verified_ee_placeable_add_layout(bytes, offset, record_end).is_some()
}
