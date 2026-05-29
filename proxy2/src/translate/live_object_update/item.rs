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
const DIAMOND_ITEM_FULL_UPDATE_EE_MASK: u32 = LEGACY_UPDATE_POSITION_MASK
    | LEGACY_UPDATE_ORIENTATION_MASK
    | LEGACY_UPDATE_STATE_MASK
    | LEGACY_UPDATE_APPEARANCE_MASK
    | EE_ITEM_UPDATE_HIDDEN_MASK
    | LEGACY_UPDATE_NAME_MASK;
const DIAMOND_ITEM_UPDATE_40_FIXED_READ_BYTES: usize = 6;
const DIAMOND_ITEM_UPDATE_40_OPTIONAL_OBJECT_ID_READ_BYTES: usize = 4;

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

    let mut bytes_removed = 0usize;
    let mut candidate = live_bytes.clone();
    let mut candidate_record_end = *record_end;
    if (raw_mask & EE_ITEM_UPDATE_HIDDEN_MASK) != 0 {
        let legacy_tail_end =
            diamond_item_update_40_tail_end(live_bytes, common.read_end, *record_end)?;
        bytes_removed =
            bytes_removed.saturating_add(legacy_tail_end.saturating_sub(common.read_end));
        // Keep the legacy 0x40 tail rewrite transactional: the Diamond reader
        // tail is removed only after the EE item validator proves the final
        // read cursor and fragment cursor.
        candidate.drain(common.read_end..legacy_tail_end);
        candidate_record_end = common.read_end;
    } else if (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        return None;
    } else if common.read_end != *record_end {
        return None;
    }

    write_u32_le(&mut candidate, record_offset + 6, translated_mask)?;
    let verified_next = advance_verified_ee_item_update_record(
        &candidate,
        record_offset,
        candidate_record_end,
        fragment_bits,
        bit_cursor,
    )?;

    *live_bytes = candidate;
    *record_end = candidate_record_end;
    Some(ItemUpdateRewrite {
        rewritten: bytes_removed != 0 || translated_mask != raw_mask,
        mask_changed: translated_mask != raw_mask,
        bytes_removed: u32::try_from(bytes_removed).unwrap_or(u32::MAX),
        next_bit_cursor: verified_next,
    })
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
        // The following `sub_4FBB40` call is an overflow check, not another
        // fragment bit owner.
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

fn diamond_item_update_40_tail_end(
    bytes: &[u8],
    tail_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let fixed_end = tail_offset.checked_add(DIAMOND_ITEM_UPDATE_40_FIXED_READ_BYTES)?;
    if fixed_end > record_end {
        return None;
    }

    // Diamond item update mask `0x40` writes WORD, BYTE, WORD, BYTE, one
    // fragment BOOL, then an optional object id only when the first BYTE is 2.
    let first_mode_byte = *bytes.get(tail_offset + 2)?;
    let decompile_tail_end = if first_mode_byte == 2 {
        fixed_end.checked_add(DIAMOND_ITEM_UPDATE_40_OPTIONAL_OBJECT_ID_READ_BYTES)?
    } else {
        fixed_end
    };
    if decompile_tail_end > record_end {
        return None;
    }

    (decompile_tail_end == record_end).then_some(record_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_hidden_item_update_live_bytes() -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
        live.extend_from_slice(&EE_ITEM_UPDATE_HIDDEN_MASK.to_le_bytes());
        live.extend_from_slice(&[0x34, 0x12, 0x01, 0x78, 0x56, 0x9A]);
        live
    }

    fn legacy_hidden_item_update_with_mask(raw_mask: u32, tail: &[u8]) -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
        live.extend_from_slice(&raw_mask.to_le_bytes());
        live.extend_from_slice(tail);
        live
    }

    #[test]
    fn item_update_40_legacy_tail_rewrites_after_bool_proof() {
        let mut live = legacy_hidden_item_update_live_bytes();
        let mut record_end = live.len();

        let rewrite = rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[true], 0)
            .expect("legacy item 0x40 tail should collapse after its hidden BOOL is present");

        assert!(rewrite.rewritten);
        assert!(!rewrite.mask_changed);
        assert_eq!(rewrite.bytes_removed, 6);
        assert_eq!(rewrite.next_bit_cursor, 1);
        assert_eq!(record_end, LEGACY_UPDATE_HEADER_BYTES);
        assert_eq!(live.len(), LEGACY_UPDATE_HEADER_BYTES);
        assert_eq!(
            read_u32_le(&live, 6),
            Some(EE_ITEM_UPDATE_HIDDEN_MASK),
            "mask stays semantic while the Diamond-only read tail is removed"
        );
    }

    #[test]
    fn item_update_40_missing_bool_does_not_partially_remove_tail() {
        let mut live = legacy_hidden_item_update_live_bytes();
        let original = live.clone();
        let mut record_end = live.len();

        assert!(
            rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[], 0).is_none(),
            "Diamond item 0x40 tail removal needs the following hidden-state BOOL"
        );
        assert_eq!(live, original);
        assert_eq!(record_end, original.len());
    }

    #[test]
    fn item_update_40_optional_object_id_tail_rewrites_after_bool_proof() {
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
        let mut record_end = live.len();

        let rewrite = rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[false], 0)
            .expect("mode-2 legacy item 0x40 tail should include the optional object id");

        assert!(rewrite.rewritten);
        assert_eq!(rewrite.bytes_removed, 10);
        assert_eq!(rewrite.next_bit_cursor, 1);
        assert_eq!(record_end, LEGACY_UPDATE_HEADER_BYTES);
        assert_eq!(live.len(), LEGACY_UPDATE_HEADER_BYTES);
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
            &[0x34, 0x12, 0x01, 0x78, 0x56, 0x9A],
        );
        let mut record_end = live.len();

        let rewrite = rewrite_update_record_for_ee(&mut live, 0, &mut record_end, &[true], 0)
            .expect("ignored 0x80 can be dropped only when no extra bytes are attributed to it");

        assert!(rewrite.rewritten);
        assert!(rewrite.mask_changed);
        assert_eq!(rewrite.bytes_removed, 6);
        assert_eq!(rewrite.next_bit_cursor, 1);
        assert_eq!(read_u32_le(&live, 6), Some(EE_ITEM_UPDATE_HIDDEN_MASK));
        assert_eq!(record_end, LEGACY_UPDATE_HEADER_BYTES);
    }
}

fn advance_bits(bits: &[bool], cursor: usize, count: usize) -> Option<usize> {
    if bits.len().saturating_sub(cursor) < count {
        return None;
    }
    cursor.checked_add(count)
}
