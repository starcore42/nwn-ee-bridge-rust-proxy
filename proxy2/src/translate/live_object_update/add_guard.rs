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

#[derive(Debug, Clone, Copy)]
struct VerifiedEePlaceableAddGuardShape {
    direct_empty_name: bool,
    direct_inline_name: bool,
    optional_object_bytes_present: bool,
}

pub(super) fn repair_verified_ee_placeable_add_guard_bits(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
) -> Option<bool> {
    if *bit_cursor > fragment_bits.len() {
        return None;
    }

    let shape = verified_ee_placeable_add_guard_shape(live_bytes, offset, record_end)?;

    let remaining_source_bits = fragment_bits.len().saturating_sub(*bit_cursor);
    if shape.direct_empty_name && !shape.optional_object_bytes_present && remaining_source_bits <= 4
    {
        // Winds local Diamond captures can reach this pass after earlier scalar
        // update repairs have consumed all, or all but a bounded residue, of
        // the compact source add bits. The read buffer is already the exact EE
        // empty-direct-name placeable add shape (`sub_1407A7800`): outer name
        // BOOL false, optional-object BOOL false, eight neutral trailing state
        // BOOLs, then the bridge-emitted identity visual-transform map. Drain
        // only the decompile-backed compact residue and materialize that
        // neutral EE guard run.
        fragment_bits.drain(*bit_cursor..);
        fragment_bits.extend(std::iter::repeat(false).take(11));
        *bit_cursor = bit_cursor.saturating_add(11);
        return Some(true);
    }

    let direct_cexo_string_name = shape.direct_empty_name || shape.direct_inline_name;
    let mut changed = false;
    let outer_locstring = fragment_bits.get(*bit_cursor).copied()?;
    let destination_name_inner_bits = if outer_locstring {
        let inner_client_tlk = fragment_bits.get(*bit_cursor + 1).copied()?;
        if direct_cexo_string_name && shape.optional_object_bytes_present && inner_client_tlk {
            // Direct CExoString names are selected by outer=false in EE
            // `sub_1407A7800`. Some Diamond/HG captures reach this already
            // EE-shaped byte layout while the legacy fragment cursor still has
            // outer=true and four optional-object bytes present; the following
            // bit remains the first post-name state bit rather than an EE
            // locstring inner selector.
            changed |= set_bit(fragment_bits, *bit_cursor, false)?;
            0
        } else {
            if direct_cexo_string_name && inner_client_tlk {
                // Same decompile-backed direct-name repair as the add writer:
                // `outer=true, inner=true` would send EE into the TLK helper,
                // but the read-buffer cursor holds a CExoString. The
                // former inner bit is the first post-name state BOOL; only the
                // branch selector is forced to EE's direct-name path.
                changed |= set_bit(fragment_bits, *bit_cursor, false)?;
                0
            } else {
                if inner_client_tlk {
                    return None;
                }
                1
            }
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
        shape.optional_object_bytes_present,
    )?;
    changed |= set_bit(fragment_bits, post_name_bit + 9, false)?;
    *bit_cursor = post_name_bit.saturating_add(10);
    Some(changed)
}

pub(super) fn repair_verified_ee_placeable_add_compact_source_bits(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
) -> Option<bool> {
    if *bit_cursor > fragment_bits.len() {
        return None;
    }
    let shape = verified_ee_placeable_add_guard_shape(live_bytes, offset, record_end)?;
    if !shape.direct_empty_name || shape.optional_object_bytes_present {
        return None;
    }
    let drain_end = bit_cursor.checked_add(4)?;
    if drain_end > fragment_bits.len() {
        return None;
    }

    // This is the nonterminal sibling of the bounded terminal residue repair
    // above. A Diamond compact source add owns exactly four tail BOOLs here;
    // EE's empty direct-name placeable add owns eleven neutral guard/state BOOLs.
    fragment_bits.drain(*bit_cursor..drain_end);
    for delta in 0..11 {
        fragment_bits.insert(*bit_cursor + delta, false);
    }
    *bit_cursor = bit_cursor.saturating_add(11);
    Some(true)
}

fn verified_ee_placeable_add_guard_shape(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<VerifiedEePlaceableAddGuardShape> {
    if offset + 6 > record_end
        || record_end > live_bytes.len()
        || live_bytes.get(offset).copied() != Some(b'A')
        || live_bytes.get(offset + 1).copied() != Some(PLACEABLE_OBJECT_TYPE)
    {
        return None;
    }

    let name_offset = offset + 6;
    let inline_name_end = locstring::inline_cexo_string_end(live_bytes, name_offset);
    let direct_empty_name =
        inline_name_end == Some(name_offset.checked_add(super::CNW_LENGTH_BYTES)?);
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

    Some(VerifiedEePlaceableAddGuardShape {
        direct_empty_name,
        direct_inline_name,
        optional_object_bytes_present,
    })
}

fn set_bit(bits: &mut [bool], bit_index: usize, value: bool) -> Option<bool> {
    let bit = bits.get_mut(bit_index)?;
    let changed = *bit != value;
    *bit = value;
    Some(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ee_placeable_add_with_optional_object() -> Vec<u8> {
        let mut live = Vec::new();
        live.extend_from_slice(&[b'A', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&0x8000_0085u32.to_le_bytes());
        live.extend_from_slice(&5u32.to_le_bytes());
        live.extend_from_slice(b"Chest");
        live.push(0x05);
        live.extend_from_slice(&0x000Eu16.to_le_bytes());
        live.extend_from_slice(&0x0000u16.to_le_bytes());
        live.extend_from_slice(&0x8000_1234u32.to_le_bytes());
        live.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        live
    }

    fn ee_placeable_add_without_optional_object() -> Vec<u8> {
        let mut live = Vec::new();
        live.extend_from_slice(&[b'A', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&0x8000_0085u32.to_le_bytes());
        live.extend_from_slice(&5u32.to_le_bytes());
        live.extend_from_slice(b"Chest");
        live.push(0x05);
        live.extend_from_slice(&0x000Eu16.to_le_bytes());
        live.extend_from_slice(&0x0000u16.to_le_bytes());
        live.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        live
    }

    #[test]
    fn optional_object_inline_helper_keeps_inner_selector_alignment() {
        let live = ee_placeable_add_with_optional_object();
        let mut bits = vec![
            true, false, // EE locstring helper: outer=true, inner=false.
            false, false, true, false, true, false, true, false, true, true,
        ];
        let mut bit_cursor = 0;

        let changed = repair_verified_ee_placeable_add_guard_bits(
            &live,
            0,
            live.len(),
            &mut bits,
            &mut bit_cursor,
        )
        .expect("optional-object guard repair should own the EE-shaped record");

        assert!(changed);
        assert_eq!(bit_cursor, 12);
        assert_eq!(
            &bits[..4],
            &[true, false, false, true],
            "outer/inner stay aligned and optional OBJECTID guard is repaired after them"
        );
        assert!(
            !bits[11],
            "EE-only visual-transform guard is forced neutral at the decompiled final bit"
        );

        let mut verified_cursor = 0;
        assert!(super::super::add::advance_verified_add_record(
            &live,
            0,
            live.len(),
            &bits,
            &mut verified_cursor,
        ));
        assert_eq!(verified_cursor, bit_cursor);
    }

    #[test]
    fn absent_optional_object_inline_helper_keeps_inner_selector_alignment() {
        let live = ee_placeable_add_without_optional_object();
        let mut bits = vec![
            true, false, // EE locstring helper: outer=true, inner=false.
            true, true, false, true, false, true, false, true, false, true,
        ];
        let mut bit_cursor = 0;

        let changed = repair_verified_ee_placeable_add_guard_bits(
            &live,
            0,
            live.len(),
            &mut bits,
            &mut bit_cursor,
        )
        .expect("absent optional-object guard repair should own the EE-shaped record");

        assert!(changed);
        assert_eq!(bit_cursor, 12);
        assert_eq!(
            &bits[..4],
            &[true, false, true, false],
            "outer/inner stay aligned and absent optional OBJECTID guard is repaired after the first state bit"
        );
        assert!(
            !bits[11],
            "EE-only visual-transform guard is forced neutral at the decompiled final bit"
        );

        let mut verified_cursor = 0;
        assert!(super::super::add::advance_verified_add_record(
            &live,
            0,
            live.len(),
            &bits,
            &mut verified_cursor,
        ));
        assert_eq!(verified_cursor, bit_cursor);
    }

    #[test]
    fn optional_object_direct_name_mode_repair_reuses_inner_bit_as_state() {
        let live = ee_placeable_add_with_optional_object();
        let mut bits = vec![
            true, true, false, false, true, false, true, false, true, false, true,
        ];
        let mut bit_cursor = 0;

        let changed = repair_verified_ee_placeable_add_guard_bits(
            &live,
            0,
            live.len(),
            &mut bits,
            &mut bit_cursor,
        )
        .expect("direct-name mode repair should own the EE-shaped record");

        assert!(changed);
        assert_eq!(bit_cursor, 11);
        assert_eq!(
            &bits[..3],
            &[false, true, true],
            "outer is forced direct, former inner remains first state bit, optional guard follows"
        );
        assert!(
            !bits[10],
            "EE-only visual-transform guard is forced neutral at the decompiled final bit"
        );

        let mut verified_cursor = 0;
        assert!(super::super::add::advance_verified_add_record(
            &live,
            0,
            live.len(),
            &bits,
            &mut verified_cursor,
        ));
        assert_eq!(verified_cursor, bit_cursor);
    }

    #[test]
    fn absent_optional_object_direct_name_mode_repair_reuses_inner_bit_as_state() {
        let live = ee_placeable_add_without_optional_object();
        let mut bits = vec![
            true, true, true, false, true, false, true, false, true, false, true,
        ];
        let mut bit_cursor = 0;

        let changed = repair_verified_ee_placeable_add_guard_bits(
            &live,
            0,
            live.len(),
            &mut bits,
            &mut bit_cursor,
        )
        .expect("direct-name mode repair should own the EE-shaped record without optional bytes");

        assert!(changed);
        assert_eq!(bit_cursor, 11);
        assert_eq!(
            &bits[..3],
            &[false, true, false],
            "outer is forced direct, former inner remains first state bit, absent optional guard follows"
        );
        assert!(
            !bits[10],
            "EE-only visual-transform guard is forced neutral at the decompiled final bit"
        );

        let mut verified_cursor = 0;
        assert!(super::super::add::advance_verified_add_record(
            &live,
            0,
            live.len(),
            &bits,
            &mut verified_cursor,
        ));
        assert_eq!(verified_cursor, bit_cursor);
    }
}
