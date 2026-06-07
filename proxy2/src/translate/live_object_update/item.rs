//! Item-family live-object update helpers.
//!
//! Keep item-specific update parsing here. The generic record walker only asks
//! whether a bounded `U/06` record can be emitted in the EE reader shape.

use super::{
    EE_UPDATE_APPEARANCE_RESREF_READ_BYTES, EE_UPDATE_APPEARANCE_WORD_READ_BYTES,
    EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS, EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES,
    EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS, EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES,
    EE_UPDATE_SCALE_STATE_READ_BYTES, ITEM_OBJECT_TYPE, LEGACY_UPDATE_APPEARANCE_MASK,
    LEGACY_UPDATE_HEADER_BYTES, LEGACY_UPDATE_NAME_MASK, LEGACY_UPDATE_ORIENTATION_MASK,
    LEGACY_UPDATE_POSITION_FRAGMENT_BITS, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_POSITION_READ_BYTES, LEGACY_UPDATE_SCALE_STATE_MASK,
    LEGACY_UPDATE_STATE_FRAGMENT_BITS, LEGACY_UPDATE_STATE_MASK, boundary, locstring, read_u16_le,
    read_u32_le, write_u32_le,
};

const EE_ITEM_UPDATE_HIDDEN_MASK: u32 = 0x0000_0040;
const LEGACY_ITEM_IGNORED_LOW_80_MASK: u32 = 0x0000_0080;
const DIAMOND_ITEM_FULL_UPDATE_MASK: u32 = 0xFFFF_FFF3;
// Full U/6 item ownership is proven by the decompiled client readers and direct
// Diamond server binary evidence, not by a neighboring cursor retry. Diamond
// client `sub_459700 -> sub_467AE0 -> sub_451AF0` reads the generic prefix and
// item name. The local fullNwnDecompilePart*.txt `0x445160`/`sub_444CC0`
// neighborhood is only a client read handler, but direct `nwserver.exe`
// disassembly shows the server U serializer at 0x445160 writes U/type/id/mask,
// reaches the item name branch, then gates later low-0x40 branches behind
// object type 5 at 0x446247; item type 6 exits. EE
// `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0` can read a hidden-state
// BOOL for explicit EE-shaped mask 0x40, but Diamond full item mask
// 0xFFFF_FFF3 must drop that bit rather than consume the following source bit.
const DIAMOND_ITEM_FULL_UPDATE_EE_MASK: u32 = LEGACY_UPDATE_POSITION_MASK
    | LEGACY_UPDATE_ORIENTATION_MASK
    | LEGACY_UPDATE_STATE_MASK
    | LEGACY_UPDATE_APPEARANCE_MASK
    | LEGACY_UPDATE_NAME_MASK;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ItemUpdateRewrite {
    pub(super) rewritten: bool,
    pub(super) mask_changed: bool,
    pub(super) bytes_removed: u32,
    pub(super) next_bit_cursor: usize,
}

pub(super) fn is_known_legacy_item_marker(marker: u8) -> bool {
    matches!(marker, 0x05 | 0xC5)
}

pub(super) fn is_legacy_item_sentinel(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset + 1) == Some(&0xFD)
        && bytes.get(offset + 2) == Some(&0xFF)
        && bytes.get(offset + 3) == Some(&0xFF)
        && bytes.get(offset + 4) == Some(&0xFF)
}

pub(super) fn translate_update_mask(raw_mask: u32) -> u32 {
    if raw_mask == DIAMOND_ITEM_FULL_UPDATE_MASK {
        return DIAMOND_ITEM_FULL_UPDATE_EE_MASK;
    }

    raw_mask & !LEGACY_ITEM_IGNORED_LOW_80_MASK
}

pub(super) fn rewrite_update_record_for_ee(
    live_bytes: &mut Vec<u8>,
    record_offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<ItemUpdateRewrite> {
    let result = rewrite_update_record_for_ee_inner(
        live_bytes,
        record_offset,
        record_end,
        fragment_bits,
        bit_cursor,
    );
    if result.is_none() && std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        trace_rejected_item_update_cursor(
            live_bytes,
            record_offset,
            *record_end,
            fragment_bits,
            bit_cursor,
        );
    }
    result
}

fn rewrite_update_record_for_ee_inner(
    live_bytes: &mut Vec<u8>,
    record_offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<ItemUpdateRewrite> {
    if let Some(next_bit_cursor) = advance_verified_ee_item_update_record(
        live_bytes,
        record_offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return Some(ItemUpdateRewrite {
            next_bit_cursor,
            ..ItemUpdateRewrite::default()
        });
    }

    let raw_mask = item_update_mask(live_bytes, record_offset, *record_end)?;
    let translated_mask = translate_update_mask(raw_mask);
    let common = parse_item_update_common_prefix(
        live_bytes,
        record_offset,
        *record_end,
        fragment_bits,
        bit_cursor,
        raw_mask,
    )?;

    if raw_mask == DIAMOND_ITEM_FULL_UPDATE_MASK {
        let mut candidate = live_bytes.clone();
        write_u32_le(&mut candidate, record_offset + 6, translated_mask)?;
        let verified_next = advance_verified_ee_item_update_record(
            &candidate,
            record_offset,
            *record_end,
            fragment_bits,
            bit_cursor,
        )?;

        *live_bytes = candidate;
        return Some(ItemUpdateRewrite {
            rewritten: translated_mask != raw_mask,
            mask_changed: translated_mask != raw_mask,
            next_bit_cursor: verified_next,
            ..ItemUpdateRewrite::default()
        });
    }

    let mut candidate = live_bytes.clone();
    if (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        return None;
    }

    // Re-audit: Diamond client `sub_459700` dispatches item updates through the
    // shared generic reader `sub_467AE0`, then item helper `sub_451AF0`.
    // `sub_467AE0` owns only generic low bits 0x1/0x2/0x4/0x8/0x20, and
    // `sub_451AF0` owns only item-name mask 0x80000. Direct `nwserver.exe`
    // writer disassembly agrees: after the item name path, type 6 exits before
    // the later type-5 low-0x40 branch. Item low 0x40 has no Diamond-owned
    // read-buffer tail here.
    if common.read_end != *record_end {
        return None;
    }

    write_u32_le(&mut candidate, record_offset + 6, translated_mask)?;
    let verified_next = advance_verified_ee_item_update_record(
        &candidate,
        record_offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    )?;

    *live_bytes = candidate;
    Some(ItemUpdateRewrite {
        rewritten: translated_mask != raw_mask,
        mask_changed: translated_mask != raw_mask,
        next_bit_cursor: verified_next,
        ..ItemUpdateRewrite::default()
    })
}

fn trace_rejected_item_update_cursor(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) {
    let raw_mask = item_update_mask(bytes, offset, record_end);
    let translated_mask = raw_mask.map(translate_update_mask);
    let nearby = raw_mask
        .zip(translated_mask)
        .map(|(raw, translated)| {
            verified_neighboring_item_update_cursors(
                bytes,
                offset,
                record_end,
                fragment_bits,
                bit_cursor,
                raw,
                translated,
            )
        })
        .unwrap_or_default();
    eprintln!(
        "live-object item update rejected: offset={offset} record_end={record_end} bit_cursor={bit_cursor} raw_mask={} translated_mask={} next_bits={:?} nearby_verified_cursors={nearby:?}",
        raw_mask
            .map(|mask| format!("0x{mask:08X}"))
            .unwrap_or_else(|| "none".to_string()),
        translated_mask
            .map(|mask| format!("0x{mask:08X}"))
            .unwrap_or_else(|| "none".to_string()),
        fragment_bits
            .get(bit_cursor..bit_cursor.saturating_add(16).min(fragment_bits.len()))
            .unwrap_or(&[])
    );
}

fn verified_neighboring_item_update_cursors(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    raw_mask: u32,
    translated_mask: u32,
) -> Vec<(isize, usize)> {
    let mut candidate = bytes.to_vec();
    if translated_mask != raw_mask
        && write_u32_le(&mut candidate, offset + 6, translated_mask).is_none()
    {
        return Vec::new();
    }

    let start = bit_cursor.saturating_sub(4);
    let end = bit_cursor.saturating_add(4).min(fragment_bits.len());
    let mut verified = Vec::new();
    for cursor in start..=end {
        if cursor == bit_cursor {
            continue;
        }
        if let Some(next_cursor) = advance_verified_ee_item_update_record(
            &candidate,
            offset,
            record_end,
            fragment_bits,
            cursor,
        ) {
            let delta = cursor as isize - bit_cursor as isize;
            verified.push((delta, next_cursor));
        }
    }
    verified
}

pub(super) fn advance_verified_ee_item_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let mask = item_update_mask(bytes, offset, record_end)?;
    if !ee_item_update_mask_supported(mask) {
        return None;
    }

    let common = parse_item_update_common_prefix(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        mask,
    )?;
    let (read_end, next_bit_cursor) = advance_verified_ee_item_tail(
        bytes,
        common.read_end,
        fragment_bits,
        common.next_bit_cursor,
        mask,
    )?;

    (read_end == record_end).then_some(next_bit_cursor)
}

pub(super) fn advance_legacy_item_update_fragment_cursor_for_transport(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let Some(raw_mask) = item_update_mask(bytes, offset, record_end) else {
        return false;
    };
    if !legacy_item_update_mask_supported(raw_mask) {
        return false;
    }

    let translated_mask = translate_update_mask(raw_mask);
    let mut candidate = bytes.to_vec();
    if translated_mask != raw_mask
        && write_u32_le(&mut candidate, offset + 6, translated_mask).is_none()
    {
        return false;
    }

    let Some(next_cursor) = advance_verified_ee_item_update_record(
        &candidate,
        offset,
        record_end,
        fragment_bits,
        *bit_cursor,
    ) else {
        return false;
    };
    *bit_cursor = next_cursor;
    true
}

pub(super) fn update_record_read_end_candidates_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<Vec<usize>> {
    let mask = item_update_mask(bytes, offset, scan_end)?;
    if !legacy_item_update_mask_supported(mask) {
        return None;
    }

    let mut cursors = vec![offset.checked_add(LEGACY_UPDATE_HEADER_BYTES)?];
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        cursors = cursors
            .into_iter()
            .filter_map(|cursor| cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES))
            .collect();
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let mut branch_cursors = Vec::with_capacity(cursors.len().saturating_mul(2));
        for cursor in cursors {
            if let Some(next) = cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES) {
                branch_cursors.push(next);
            }
            if let Some(next) = cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES) {
                branch_cursors.push(next);
            }
        }
        cursors = branch_cursors;
    }
    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        cursors = cursors
            .into_iter()
            .filter_map(|cursor| {
                let appearance = read_u16_le(bytes, cursor)?;
                let mut next = cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
                if appearance >= 0xFFFE {
                    next = next.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
                }
                Some(next)
            })
            .collect();
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        cursors = cursors
            .into_iter()
            .filter_map(|cursor| cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES))
            .collect();
    }
    if (mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        let mut name_cursors = Vec::with_capacity(cursors.len().saturating_mul(2));
        for cursor in cursors {
            if let Some(next) = locstring::inline_cexo_string_end(bytes, cursor) {
                name_cursors.push(next);
            }
            if let Some(next) = locstring::tlk_locstring_ref_end(bytes, cursor) {
                name_cursors.push(next);
            }
        }
        cursors = name_cursors;
    }

    cursors.retain(|cursor| *cursor <= scan_end && *cursor <= bytes.len());
    cursors.sort_unstable();
    cursors.dedup();
    (!cursors.is_empty()).then_some(cursors)
}

#[derive(Debug, Clone, Copy)]
struct ItemUpdateCommonPrefix {
    read_end: usize,
    next_bit_cursor: usize,
}

fn item_update_mask(bytes: &[u8], offset: usize, record_end: usize) -> Option<u32> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != ITEM_OBJECT_TYPE
        || !boundary::looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    read_u32_le(bytes, offset + 6)
}

fn parse_item_update_common_prefix(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    mask: u32,
) -> Option<ItemUpdateCommonPrefix> {
    if !legacy_item_update_mask_supported(mask) {
        return None;
    }

    let mut read_cursor = offset.checked_add(LEGACY_UPDATE_HEADER_BYTES)?;
    let mut fragment_cursor = bit_cursor;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
        )?;
        if read_cursor > record_end {
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        // Diamond `sub_467AE0` and EE `sub_14079C050`
        // (0x14079C2CC..0x14079C380) both read this BOOL at the inherited
        // fragment cursor before deciding whether the following read-buffer
        // fields are scalar facing or XYZ orientation. A scalar-shaped byte
        // tail that verifies at a neighboring cursor is therefore ambiguity,
        // not permission to search or skip bits here.
        let vector_branch = fragment_bits.get(fragment_cursor).copied()?;
        if vector_branch {
            read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES)?;
            fragment_cursor = advance_bits(
                fragment_bits,
                fragment_cursor,
                EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS,
            )?;
        } else {
            read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?;
            fragment_cursor = advance_bits(
                fragment_bits,
                fragment_cursor,
                EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS,
            )?;
        }
        if read_cursor > record_end {
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let appearance_word = read_u16_le(bytes, read_cursor)?;
        read_cursor = read_cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
        if appearance_word >= 0xFFFE {
            read_cursor = read_cursor.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
        }
        if read_cursor > record_end {
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        read_cursor = read_cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
        if read_cursor > record_end {
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_STATE_FRAGMENT_BITS,
        )?;
    }

    Some(ItemUpdateCommonPrefix {
        read_end: read_cursor,
        next_bit_cursor: fragment_cursor,
    })
}

fn ee_item_update_mask_supported(mask: u32) -> bool {
    let allowed = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_APPEARANCE_MASK
        | EE_ITEM_UPDATE_HIDDEN_MASK
        | LEGACY_UPDATE_NAME_MASK;
    mask != 0 && (mask & !allowed) == 0
}

fn legacy_item_update_mask_supported(mask: u32) -> bool {
    if mask == DIAMOND_ITEM_FULL_UPDATE_MASK {
        return true;
    }

    let allowed = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_APPEARANCE_MASK
        | EE_ITEM_UPDATE_HIDDEN_MASK
        | LEGACY_ITEM_IGNORED_LOW_80_MASK
        | LEGACY_UPDATE_NAME_MASK;

    mask != 0 && (mask & !allowed) == 0
}

fn advance_verified_ee_item_tail(
    bytes: &[u8],
    read_cursor: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    mask: u32,
) -> Option<(usize, usize)> {
    let mut read_cursor = read_cursor;
    let mut fragment_cursor = bit_cursor;

    if (mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        // Diamond item update `sub_451AF0` tests mask 0x80000, reads one BOOL,
        // then either a locstring helper (`sub_53E700`) or `ReadCExoString(32)`.
        // EE `sub_1407A08F0` matches that shape at
        // 0x1407A0A07..0x1407A0A7A. The following overflow checks are not
        // another fragment bit owner.
        let uses_locstring = fragment_bits.get(fragment_cursor).copied()?;
        fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
        if uses_locstring {
            let uses_tlk_ref = fragment_bits.get(fragment_cursor).copied()?;
            fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
            read_cursor = if uses_tlk_ref {
                locstring::tlk_locstring_ref_end(bytes, read_cursor)?
            } else {
                locstring::inline_cexo_string_end(bytes, read_cursor)?
            };
        } else {
            read_cursor = locstring::inline_cexo_string_end(bytes, read_cursor)?;
        }
    }

    if (mask & EE_ITEM_UPDATE_HIDDEN_MASK) != 0 {
        fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
    }

    Some((read_cursor, fragment_cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_hidden_item_update_live_bytes() -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
        live.extend_from_slice(&EE_ITEM_UPDATE_HIDDEN_MASK.to_le_bytes());
        live
    }

    fn legacy_hidden_item_update_with_mask(raw_mask: u32, tail: &[u8]) -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
        live.extend_from_slice(&raw_mask.to_le_bytes());
        live.extend_from_slice(tail);
        live
    }

    fn legacy_full_scalar_direct_name_item_update_live_bytes(name: &[u8]) -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
        live.extend_from_slice(&DIAMOND_ITEM_FULL_UPDATE_MASK.to_le_bytes());
        live.extend_from_slice(&[0xB7, 0x05, 0xC1, 0x04, 0x0F, 0x0F]);
        live.push(0);
        live.extend_from_slice(&0xFFFFu16.to_le_bytes());
        live.extend_from_slice(&[0; EE_UPDATE_APPEARANCE_RESREF_READ_BYTES]);
        live.extend_from_slice(&(name.len() as u32).to_le_bytes());
        live.extend_from_slice(name);
        live
    }

    #[test]
    fn item_update_40_exact_ee_hidden_claims_without_tail() {
        let live = legacy_hidden_item_update_live_bytes();
        let next = advance_verified_ee_item_update_record(&live, 0, live.len(), &[true], 0)
            .expect("EE item hidden-state update owns exactly one BOOL and no read tail");

        assert_eq!(next, 1);
    }

    #[test]
    fn item_update_40_read_tail_is_not_decompile_owned() {
        let mut live = legacy_hidden_item_update_with_mask(
            EE_ITEM_UPDATE_HIDDEN_MASK,
            &[0x34, 0x12, 0x01, 0x78, 0x56, 0x9A],
        );
        let original = live.clone();
        let mut record_end = live.len();

        assert!(
            rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[true], 0).is_none(),
            "Diamond item readers do not own a low-0x40 read-buffer tail"
        );
        assert_eq!(live, original);
        assert_eq!(record_end, original.len());
    }

    #[test]
    fn item_update_40_missing_bool_does_not_partially_remove_tail() {
        let mut live = legacy_hidden_item_update_live_bytes();
        let original = live.clone();
        let mut record_end = live.len();

        assert!(
            rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[], 0).is_none(),
            "EE item hidden-state updates must not claim without the hidden-state BOOL"
        );
        assert_eq!(live, original);
        assert_eq!(record_end, original.len());
    }

    #[test]
    fn item_update_40_optional_object_id_tail_is_not_decompile_owned() {
        let mut live = legacy_hidden_item_update_with_mask(
            EE_ITEM_UPDATE_HIDDEN_MASK,
            &[
                0x34, 0x12, // WORD
                0x02, // BYTE that guards the optional object id
                0x78, 0x56, // WORD
                0x9A, // BYTE
                0x44, 0x33, 0x22, 0x80, // optional OBJECTID
            ],
        );
        let original = live.clone();
        let mut record_end = live.len();

        assert!(
            rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[false], 0).is_none(),
            "optional object-id-looking bytes after item 0x40 are not Diamond-reader-owned"
        );
        assert_eq!(live, original);
        assert_eq!(record_end, original.len());
    }

    #[test]
    fn item_update_40_ignored_low80_does_not_extend_read_tail() {
        let mut live = legacy_hidden_item_update_with_mask(
            EE_ITEM_UPDATE_HIDDEN_MASK | LEGACY_ITEM_IGNORED_LOW_80_MASK,
            &[
                0x34, 0x12, 0x01, 0x78, 0x56, 0x9A, // decompile-owned 0x40 tail
                0x00, 0x00, 0x00, // unowned padding-like bytes
            ],
        );
        let original = live.clone();
        let mut record_end = live.len();

        assert!(
            rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[true], 0).is_none(),
            "raw item mask 0x80 is ignored for mask translation but does not own extra read bytes"
        );
        assert_eq!(live, original);
        assert_eq!(record_end, original.len());
    }

    #[test]
    fn item_update_40_low80_exact_tail_translates_mask_only() {
        let mut live = legacy_hidden_item_update_with_mask(
            EE_ITEM_UPDATE_HIDDEN_MASK | LEGACY_ITEM_IGNORED_LOW_80_MASK,
            &[],
        );
        let mut record_end = live.len();

        let rewrite = rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[true], 0)
            .expect("ignored 0x80 can be dropped only when no extra bytes are attributed to it");

        assert!(rewrite.rewritten);
        assert!(rewrite.mask_changed);
        assert_eq!(rewrite.bytes_removed, 0);
        assert_eq!(rewrite.next_bit_cursor, 1);
        assert_eq!(read_u32_le(&live, 6), Some(EE_ITEM_UPDATE_HIDDEN_MASK));
        assert_eq!(record_end, LEGACY_UPDATE_HEADER_BYTES);
    }

    #[test]
    fn full_item_update_rewrite_does_not_retry_neighboring_cursor() {
        // CEP-style cursor ambiguity reduced to the item family: the inherited
        // cursor selects vector orientation, while the bounded bytes are the
        // scalar/direct-name full update shape. A neighboring cursor can fit
        // only if some prior decompiled reader has already owned those bits.
        let mut live = legacy_full_scalar_direct_name_item_update_live_bytes(b"Lance");
        let original = live.clone();
        let shifted_bits = vec![
            false, true, // unowned pre-cursor residue.
            true, true, // position residuals if a prior owner consumed it.
            false, true, false, true, true, // scalar branch bits at cursor +2.
            false, false, false, false, false, // item state bits.
            false, // direct CExoString item name.
        ];

        let mut translated = live.clone();
        write_u32_le(
            &mut translated,
            6,
            translate_update_mask(DIAMOND_ITEM_FULL_UPDATE_MASK),
        )
        .expect("mask write");
        assert!(
            advance_verified_ee_item_update_record(
                &translated,
                0,
                translated.len(),
                &shifted_bits,
                0
            )
            .is_none(),
            "the inherited cursor selects vector orientation for scalar-shaped item bytes"
        );
        assert!(
            advance_verified_ee_item_update_record(
                &translated,
                0,
                translated.len(),
                &shifted_bits,
                2
            )
            .is_some(),
            "cursor +2 would fit only after a separate owner consumes the residue"
        );

        let mut record_end = live.len();
        assert!(
            rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &shifted_bits, 0).is_none(),
            "item update rewriting must not search neighboring cursors"
        );
        assert_eq!(live, original);
        assert_eq!(record_end, original.len());
    }

    #[test]
    fn full_item_update_drops_low40_without_consuming_hidden_bit() {
        // Direct `nwserver.exe` disassembly of the server U serializer at
        // 0x445160 writes U/type/id/mask, follows the item name branch, and
        // reaches the object-type gate at 0x446247; type 6 exits before the
        // later type-5 low-0x40 branch. Therefore a raw Diamond full item mask
        // drops low 0x40 instead of consuming a source hidden-state BOOL.
        let mut live = legacy_full_scalar_direct_name_item_update_live_bytes(b"Lance");
        let bits = vec![
            true, true, // position residual bits.
            false, true, false, true, true, // scalar orientation branch.
            false, false, false, false, false, // item state bits.
            false, // direct CExoString item name.
            true,  // following stream bit must remain unconsumed.
        ];
        let expected_next = bits.len() - 1;
        let mut record_end = live.len();

        let rewrite = rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &bits, 0)
            .expect("decompile-shaped full item update should translate its mask");

        assert!(rewrite.rewritten);
        assert!(rewrite.mask_changed);
        assert_eq!(rewrite.next_bit_cursor, expected_next);
        assert_eq!(
            read_u32_le(&live, 6),
            Some(DIAMOND_ITEM_FULL_UPDATE_EE_MASK),
            "Diamond full item mask must not preserve EE's explicit hidden-state bit"
        );
        assert_eq!(record_end, live.len());
    }

    #[test]
    fn full_item_update_extra_tail_is_not_subset_rewritten() {
        // Diamond's later type-dispatched reader branches are not item tails:
        // the local object-type table maps 0x05 to creature and 0x06 to item.
        // A raw full item update may translate only when the generic prefix
        // plus `sub_451AF0` name branch lands exactly on record_end.
        let mut live = legacy_full_scalar_direct_name_item_update_live_bytes(b"Lance");
        live.extend_from_slice(&[0x34, 0x12, 0x01]);
        let original = live.clone();
        let bits = vec![
            true, true, // position residual bits.
            false, true, false, true, true, // scalar orientation branch.
            false, false, false, false, false, // item state bits.
            false, // direct CExoString item name.
        ];
        let mut record_end = live.len();

        assert!(
            rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &bits, 0).is_none(),
            "full-mask item updates with unowned post-name bytes must stay unclaimed"
        );
        assert_eq!(live, original);
        assert_eq!(record_end, original.len());
    }
}

fn advance_bits(bits: &[bool], cursor: usize, count: usize) -> Option<usize> {
    if bits.len().saturating_sub(cursor) < count {
        return None;
    }
    cursor.checked_add(count)
}
