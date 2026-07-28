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
        if crate::translate::live_object_update::live_object_debug_env_enabled(
            "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
        ) {
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

    fn ee_placeable_add_tlk_name(optional_object_id: Option<u32>) -> Vec<u8> {
        let mut live = vec![b'A', PLACEABLE_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_0042u32.to_le_bytes());
        live.extend_from_slice(&0x0100_75D6u32.to_le_bytes());
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

    fn diamond_placeable_add_localized_name(
        name_bytes: &[u8],
        optional_object_id: Option<u32>,
    ) -> Vec<u8> {
        let mut live = vec![b'A', PLACEABLE_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_0042u32.to_le_bytes());
        live.extend_from_slice(name_bytes);
        live.push(5);
        live.extend_from_slice(&0x0011u16.to_le_bytes());
        live.extend_from_slice(&0x0002u16.to_le_bytes());
        if let Some(object_id) = optional_object_id {
            live.extend_from_slice(&object_id.to_le_bytes());
        }
        live
    }

    #[test]
    fn diamond_placeable_add_source_claims_stock_tlk_tail_and_exact_suffix() {
        let strref = 0x0100_75D6u32;
        let live = diamond_placeable_add_localized_name(&strref.to_le_bytes(), None);
        let leading_bits = [false, true];
        let source_bits = [
            true, true, true, // outer locstring, inner TLK, client-TLK selector.
            true, false, true, false, true, false, true, false,
            true,
            // Nine post-name state BOOLs; optional-object guard is false.
        ];
        let following_bits = [true, false, true];
        let bits = leading_bits
            .into_iter()
            .chain(source_bits)
            .chain(following_bits)
            .collect::<Vec<_>>();

        let claim = parse_verified_diamond_placeable_add_localized_name_source(
            &live,
            0,
            live.len(),
            &bits,
            leading_bits.len(),
        )
        .expect("stock Diamond A/09 localized name should exact-claim");
        assert_eq!(
            claim.name,
            VerifiedDiamondPlaceableAddLocalizedName::StockTlk {
                client_tlk: true,
                strref,
            }
        );
        assert_eq!(claim.byte_layout.tail_offset, 10);
        assert_eq!(claim.byte_layout.base_tail_end, 15);
        assert_eq!(claim.byte_layout.optional_object_id, None);
        assert_eq!(claim.post_name_bit, leading_bits.len() + 3);
        assert_eq!(
            claim.next_bit_cursor,
            leading_bits.len() + source_bits.len()
        );
        assert_eq!(
            claim.state,
            VerifiedDiamondPlaceableAddState {
                reputation_visual: true,
                static_plot: true,
                useable: false,
                trap_disarmable: true,
                lockable: false,
                locked: true,
                unknown_1ac: false,
                name_valid: true,
            }
        );
        assert_eq!(
            &bits[claim.next_bit_cursor..],
            &following_bits,
            "the stock parser must own exactly 12 source bits"
        );
    }

    #[test]
    fn diamond_placeable_add_source_claims_custom_byte_tlk_optional_branch() {
        let strref = 0x0100_75D6u32;
        let optional_object_id = 0x8000_1234u32;
        let mut custom_name = vec![1];
        custom_name.extend_from_slice(&strref.to_le_bytes());
        let live = diamond_placeable_add_localized_name(&custom_name, Some(optional_object_id));
        let leading_bits = [false];
        let source_bits = [
            true, true, // outer locstring, inner TLK; selector is a source byte.
            false, true, false, true, false, true, false, true,
            false,
            // Nine post-name state BOOLs; optional-object guard is true.
        ];
        let following_bits = [false, false];
        let bits = leading_bits
            .into_iter()
            .chain(source_bits)
            .chain(following_bits)
            .collect::<Vec<_>>();

        let claim = parse_verified_diamond_placeable_add_localized_name_source(
            &live,
            0,
            live.len(),
            &bits,
            leading_bits.len(),
        )
        .expect("custom byte-selector A/09 localized name should exact-claim");
        assert_eq!(
            claim.name,
            VerifiedDiamondPlaceableAddLocalizedName::CustomByteTlk {
                client_tlk: 1,
                strref,
            }
        );
        assert_eq!(claim.byte_layout.tail_offset, 11);
        assert_eq!(claim.byte_layout.base_tail_end, 16);
        assert_eq!(
            claim.byte_layout.optional_object_id,
            Some(optional_object_id)
        );
        assert_eq!(claim.post_name_bit, leading_bits.len() + 2);
        assert_eq!(
            claim.next_bit_cursor,
            leading_bits.len() + source_bits.len()
        );
        assert_eq!(
            claim.state,
            VerifiedDiamondPlaceableAddState {
                reputation_visual: false,
                static_plot: false,
                useable: true,
                trap_disarmable: false,
                lockable: true,
                locked: false,
                unknown_1ac: true,
                name_valid: false,
            }
        );
        assert_eq!(
            &bits[claim.next_bit_cursor..],
            &following_bits,
            "the custom parser must own exactly 11 source bits"
        );
    }

    #[test]
    fn diamond_placeable_add_source_rejects_unbounded_or_mismatched_branches() {
        let strref = 0x0100_75D6u32;
        let stock_live = diamond_placeable_add_localized_name(&strref.to_le_bytes(), None);
        let stock_bits = [
            true, true, false, // stock name selectors.
            false, true, false, false, false, false, false, false,
            false,
            // Guard says optional OBJECTID, but the bytes end at the base tail.
        ];
        assert!(
            parse_verified_diamond_placeable_add_localized_name_source(
                &stock_live,
                0,
                stock_live.len(),
                &stock_bits,
                0,
            )
            .is_none(),
            "the optional-object BOOL must agree with exact guarded bytes"
        );

        let mut invalid_custom_name = vec![2];
        invalid_custom_name.extend_from_slice(&strref.to_le_bytes());
        let invalid_custom_live = diamond_placeable_add_localized_name(&invalid_custom_name, None);
        let custom_bits = [
            true, true, // selector would be a byte.
            false, false, false, false, false, false, false, false, false,
        ];
        assert!(
            parse_verified_diamond_placeable_add_localized_name_source(
                &invalid_custom_live,
                0,
                invalid_custom_live.len(),
                &custom_bits,
                0,
            )
            .is_none(),
            "the custom source selector byte is bounded to the captured 0/1 dialect"
        );

        let valid_custom_name = {
            let mut name = vec![0];
            name.extend_from_slice(&strref.to_le_bytes());
            name
        };
        let mut extra_byte_live = diamond_placeable_add_localized_name(&valid_custom_name, None);
        extra_byte_live.push(0);
        assert!(
            parse_verified_diamond_placeable_add_localized_name_source(
                &extra_byte_live,
                0,
                extra_byte_live.len(),
                &custom_bits,
                0,
            )
            .is_none(),
            "the source parser must own the whole bounded byte record"
        );

        assert!(
            parse_verified_diamond_placeable_add_localized_name_source(
                &stock_live,
                0,
                stock_live.len(),
                &stock_bits[..stock_bits.len() - 1],
                0,
            )
            .is_none(),
            "the source parser must own all nine post-name BOOLs"
        );
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
            "a TLK selector bit over inline CExoString bytes must not shift the state cursor"
        );

        let tlk_live = ee_placeable_add_tlk_name(Some(0x8000_1234));
        let tlk_bits = vec![
            true, true, true,  // outer locstring, inner TLK, one-bit language selector.
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
        let tlk_layout =
            verified_ee_placeable_add_fragment_layout(&tlk_live, 0, tlk_live.len(), &tlk_bits, 0)
                .expect("TLK locstring placeable add should own the exact name branch");
        assert_eq!(
            tlk_layout.name,
            VerifiedEePlaceableAddName::StockTlkStrRef {
                client_tlk: true,
                strref: 0x0100_75D6,
            }
        );
        assert_eq!(tlk_layout.byte_layout.tail_offset, 10);
        assert_eq!(tlk_layout.byte_layout.base_tail_end, 15);
        assert_eq!(tlk_layout.byte_layout.map_offset, 19);
        assert_eq!(tlk_layout.post_name_bit, 3);
        assert_eq!(tlk_layout.next_bit_cursor, tlk_bits.len());
        assert!(tlk_layout.byte_layout.optional_object_id);

        let mut cursor = 0usize;
        assert!(
            advance_verified_add_record(&tlk_live, 0, tlk_live.len(), &tlk_bits, &mut cursor,),
            "full add verifier must accept the decompile-shaped TLK branch"
        );
        assert_eq!(cursor, tlk_bits.len());

        let mut missing_language_or_final = tlk_bits.clone();
        missing_language_or_final.pop();
        assert!(
            verified_ee_placeable_add_fragment_layout(
                &tlk_live,
                0,
                tlk_live.len(),
                &missing_language_or_final,
                0,
            )
            .is_none(),
            "the stock TLK branch must own its fragment language selector and the EE final guard"
        );

        let mut custom_tlk_live = tlk_live.clone();
        custom_tlk_live.insert(6, 1);
        assert!(
            verified_ee_placeable_add_fragment_layout(
                &custom_tlk_live,
                0,
                custom_tlk_live.len(),
                &tlk_bits,
                0,
            )
            .is_none(),
            "a custom full-byte selector plus DWORD is not the stock EE ReadBYTE(1, 1) layout"
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

    #[test]
    fn zero_strref_name_dialect_is_selected_only_by_fragment_bits() {
        let mut live = ee_placeable_add_tlk_name(None);
        live[6..10].copy_from_slice(&0u32.to_le_bytes());

        let stock_bits = vec![
            true, true, false, // localized, TLK, language zero.
            false, false, false, false, false, false, false, false, false, false,
        ];
        let stock = verified_ee_placeable_add_fragment_layout(&live, 0, live.len(), &stock_bits, 0)
            .expect("zero StrRef must remain a legal stock TLK reference");
        assert_eq!(
            stock.name,
            VerifiedEePlaceableAddName::StockTlkStrRef {
                client_tlk: false,
                strref: 0,
            }
        );
        assert_eq!(stock.next_bit_cursor, 13);

        let direct_bits = vec![
            false, // direct zero-length CExoString.
            false, false, false, false, false, false, false, false, false, false,
        ];
        let direct =
            verified_ee_placeable_add_fragment_layout(&live, 0, live.len(), &direct_bits, 0)
                .expect("the same zero bytes are also the direct empty CExoString payload");
        assert_eq!(direct.name, VerifiedEePlaceableAddName::DirectCExoString);
        assert_eq!(direct.next_bit_cursor, 11);

        let inline_bits = vec![
            true, false, // localized inline zero-length CExoString.
            false, false, false, false, false, false, false, false, false, false,
        ];
        let inline =
            verified_ee_placeable_add_fragment_layout(&live, 0, live.len(), &inline_bits, 0)
                .expect("the same zero bytes are also the localized inline empty string");
        assert_eq!(
            inline.name,
            VerifiedEePlaceableAddName::LocStringInlineCExoString
        );
        assert_eq!(inline.next_bit_cursor, 12);
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

const DIAMOND_PLACEABLE_ADD_POST_NAME_FRAGMENT_BITS: usize = 9;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerifiedDiamondPlaceableAddSourceLayout {
    pub(crate) tail_offset: usize,
    pub(crate) base_tail_end: usize,
    pub(crate) optional_object_id: Option<u32>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerifiedDiamondPlaceableAddSource {
    pub(crate) byte_layout: VerifiedDiamondPlaceableAddSourceLayout,
    pub(crate) name: VerifiedDiamondPlaceableAddLocalizedName,
    pub(crate) state: VerifiedDiamondPlaceableAddState,
    pub(crate) post_name_bit: usize,
    pub(crate) next_bit_cursor: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerifiedDiamondPlaceableAddLocalizedName {
    StockTlk { client_tlk: bool, strref: u32 },
    CustomByteTlk { client_tlk: u8, strref: u32 },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerifiedDiamondPlaceableAddState {
    pub(crate) reputation_visual: bool,
    pub(crate) static_plot: bool,
    pub(crate) useable: bool,
    pub(crate) trap_disarmable: bool,
    pub(crate) lockable: bool,
    pub(crate) locked: bool,
    pub(crate) unknown_1ac: bool,
    pub(crate) name_valid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiamondPlaceableAddLocalizedNameMode {
    StockTlk,
    CustomByteTlk,
}

/// Recognize only the read-buffer footprint shared by the bounded stock and
/// custom localized-name source parsers. This does not authorize translation:
/// fragment ownership and the optional-object guard still require the typed
/// parser plus successor/terminal cursor proof.
pub(crate) fn looks_like_diamond_placeable_add_localized_name_source_byte_shape(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if offset
        .checked_add(6)
        .is_none_or(|header_end| header_end > record_end)
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'A')
        || bytes.get(offset + 1).copied() != Some(PLACEABLE_OBJECT_TYPE)
    {
        return false;
    }

    let name_offset = offset + 6;
    [
        locstring::fragment_selector_tlk_strref_end(bytes, name_offset),
        locstring::tlk_locstring_ref_end(bytes, name_offset),
    ]
    .into_iter()
    .flatten()
    .any(|tail_offset| {
        let Some(base_tail_end) = tail_offset.checked_add(1 + 2 + 2) else {
            return false;
        };
        read_u16_le(bytes, tail_offset + 1).is_some()
            && read_u16_le(bytes, tail_offset + 3).is_some()
            && (base_tail_end == record_end
                || base_tail_end.checked_add(4).is_some_and(|optional_end| {
                    optional_end == record_end && read_u32_le(bytes, base_tail_end).is_some()
                }))
    })
}

/// Parse one bounded Diamond/HG source `A/09` localized-name record.
///
/// Diamond `sub_44E4A0` -> `sub_53E700` owns the stock name as three fragment
/// bits (outer locstring, inner TLK, one-bit client-TLK selector) followed by a
/// four-byte StrRef. Older custom-server captures instead keep the first two
/// fragment bits and store a full 0/1 selector byte before the StrRef. After
/// either name shape, `sub_44E4A0` owns the same five-byte read-buffer tail,
/// an optional four-byte OBJECTID guarded by the second post-name BOOL, and
/// exactly nine post-name fragment BOOLs.
///
/// This is a source classifier only. It deliberately does not choose between
/// a localized-name candidate and a compact four-byte token based on payload
/// plausibility; callers must prove record ownership with a bounded successor
/// walk or a terminal fragment cursor before authorizing canonicalization.
#[allow(dead_code)]
pub(crate) fn parse_verified_diamond_placeable_add_localized_name_source(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<VerifiedDiamondPlaceableAddSource> {
    if bit_cursor >= fragment_bits.len()
        || !fragment_bits.get(bit_cursor).copied()?
        || !fragment_bits.get(bit_cursor.checked_add(1)?).copied()?
    {
        return None;
    }

    parse_verified_diamond_placeable_add_localized_name_source_for_mode(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        DiamondPlaceableAddLocalizedNameMode::StockTlk,
    )
    .or_else(|| {
        parse_verified_diamond_placeable_add_localized_name_source_for_mode(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
            DiamondPlaceableAddLocalizedNameMode::CustomByteTlk,
        )
    })
}

fn parse_verified_diamond_placeable_add_localized_name_source_for_mode(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    name_mode: DiamondPlaceableAddLocalizedNameMode,
) -> Option<VerifiedDiamondPlaceableAddSource> {
    if offset.checked_add(6)? > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied()? != b'A'
        || bytes.get(offset.checked_add(1)?).copied()? != PLACEABLE_OBJECT_TYPE
    {
        return None;
    }

    let name_offset = offset.checked_add(6)?;
    let (name, name_fragment_bits, tail_offset) = match name_mode {
        DiamondPlaceableAddLocalizedNameMode::StockTlk => {
            let client_tlk = fragment_bits.get(bit_cursor.checked_add(2)?).copied()?;
            let strref = read_u32_le(bytes, name_offset)?;
            let tail_offset = locstring::fragment_selector_tlk_strref_end(bytes, name_offset)?;
            (
                VerifiedDiamondPlaceableAddLocalizedName::StockTlk { client_tlk, strref },
                3usize,
                tail_offset,
            )
        }
        DiamondPlaceableAddLocalizedNameMode::CustomByteTlk => {
            let tail_offset = locstring::tlk_locstring_ref_end(bytes, name_offset)?;
            let client_tlk = *bytes.get(name_offset)?;
            let strref = read_u32_le(bytes, name_offset.checked_add(1)?)?;
            (
                VerifiedDiamondPlaceableAddLocalizedName::CustomByteTlk { client_tlk, strref },
                2usize,
                tail_offset,
            )
        }
    };

    // Diamond reads BYTE type, WORD appearance, and WORD static tail values
    // before consulting the optional-object fragment guard.
    let base_tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if base_tail_end > record_end
        || read_u16_le(bytes, tail_offset.checked_add(1)?).is_none()
        || read_u16_le(bytes, tail_offset.checked_add(3)?).is_none()
    {
        return None;
    }

    let post_name_bit = bit_cursor.checked_add(name_fragment_bits)?;
    let next_bit_cursor =
        post_name_bit.checked_add(DIAMOND_PLACEABLE_ADD_POST_NAME_FRAGMENT_BITS)?;
    let state_bits = fragment_bits.get(post_name_bit..next_bit_cursor)?;
    let optional_object_guard = *state_bits.get(1)?;

    let optional_object_id = if optional_object_guard {
        let optional_object_id = read_u32_le(bytes, base_tail_end)?;
        if base_tail_end.checked_add(4)? != record_end {
            return None;
        }
        Some(optional_object_id)
    } else {
        if base_tail_end != record_end {
            return None;
        }
        None
    };

    Some(VerifiedDiamondPlaceableAddSource {
        byte_layout: VerifiedDiamondPlaceableAddSourceLayout {
            tail_offset,
            base_tail_end,
            optional_object_id,
        },
        name,
        state: VerifiedDiamondPlaceableAddState {
            reputation_visual: state_bits[0],
            static_plot: state_bits[2],
            useable: state_bits[3],
            trap_disarmable: state_bits[4],
            lockable: state_bits[5],
            locked: state_bits[6],
            unknown_1ac: state_bits[7],
            name_valid: state_bits[8],
        },
        post_name_bit,
        next_bit_cursor,
    })
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
    pub(super) name: VerifiedEePlaceableAddName,
    pub(super) post_name_bit: usize,
    pub(super) next_bit_cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VerifiedEePlaceableAddName {
    DirectCExoString,
    LocStringInlineCExoString,
    StockTlkStrRef { client_tlk: bool, strref: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EePlaceableAddNameMode {
    DirectOrInlineCExoString,
    StockTlkStrRef,
}

pub(super) fn verified_ee_placeable_add_layout(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<VerifiedEePlaceableAddLayout> {
    verified_ee_placeable_add_layout_for_name_mode(
        bytes,
        offset,
        record_end,
        EePlaceableAddNameMode::DirectOrInlineCExoString,
    )
    .or_else(|| {
        verified_ee_placeable_add_layout_for_name_mode(
            bytes,
            offset,
            record_end,
            EePlaceableAddNameMode::StockTlkStrRef,
        )
    })
}

fn verified_ee_placeable_add_layout_for_name_mode(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    name_mode: EePlaceableAddNameMode,
) -> Option<VerifiedEePlaceableAddLayout> {
    if offset + 6 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied()? != b'A'
        || bytes.get(offset + 1).copied()? != PLACEABLE_OBJECT_TYPE
    {
        return None;
    }
    let name_offset = offset + 6;
    let tail_offset = match name_mode {
        EePlaceableAddNameMode::DirectOrInlineCExoString => {
            locstring::inline_cexo_string_end(bytes, name_offset).unwrap_or(name_offset + 4)
        }
        EePlaceableAddNameMode::StockTlkStrRef => {
            locstring::fragment_selector_tlk_strref_end(bytes, name_offset)?
        }
    };
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
    let (name_fragment_bits, name_mode, name) = if outer_locstring {
        let inner_client_tlk = fragment_bits.get(bit_cursor + 1).copied()?;
        if inner_client_tlk {
            // Diamond `sub_44E4A0` -> `sub_53E700` and EE
            // `sub_1407A7800` -> `sub_1409735F0` read the same exact stock
            // sequence: outer BOOL, inner BOOL, `ReadBYTE(1, 1)` from the
            // fragment stream, then one 32-bit StrRef from the read buffer.
            // Older custom captures with a full selector byte before the
            // DWORD are a separate source dialect and must never exact-claim
            // as EE.
            let client_tlk = fragment_bits.get(bit_cursor + 2).copied()?;
            let strref = read_u32_le(bytes, offset.checked_add(6)?)?;
            (
                3,
                EePlaceableAddNameMode::StockTlkStrRef,
                VerifiedEePlaceableAddName::StockTlkStrRef { client_tlk, strref },
            )
        } else {
            (
                2,
                EePlaceableAddNameMode::DirectOrInlineCExoString,
                VerifiedEePlaceableAddName::LocStringInlineCExoString,
            )
        }
    } else {
        (
            1,
            EePlaceableAddNameMode::DirectOrInlineCExoString,
            VerifiedEePlaceableAddName::DirectCExoString,
        )
    };
    let post_name_bit = bit_cursor.checked_add(name_fragment_bits)?;
    if fragment_bits.len() <= post_name_bit + 9 {
        return None;
    }

    let byte_layout =
        verified_ee_placeable_add_layout_for_name_mode(bytes, offset, record_end, name_mode)?;
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

    if crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_PLACEABLE_ADD",
    ) {
        eprintln!(
            "placeable-add exact claim offset={offset} record_end={record_end} bit_cursor={bit_cursor} post_name_bit={post_name_bit} next_bit_cursor={} name={name:?}",
            post_name_bit.saturating_add(10)
        );
    }

    Some(VerifiedEePlaceableAddFragmentLayout {
        byte_layout,
        name,
        post_name_bit,
        next_bit_cursor: post_name_bit.saturating_add(10),
    })
}

fn verified_ee_placeable_add_record(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    verified_ee_placeable_add_layout(bytes, offset, record_end).is_some()
}
