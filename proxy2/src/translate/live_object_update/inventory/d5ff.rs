use super::*;

// Focused D5FF inventory-family parsers.
//
// This module intentionally keeps D5FF differences as typed reader variants
// instead of widening unrelated inventory masks. The older self-inventory D5FF
// packets still use the generic path; these helpers own only D5FF live-object
// creature equipment/UI-state bodies whose mask reader order is decompile
// backed.
//
// Reader-order evidence:
//
// * EE `sub_1407B4F70` and Diamond `sub_455940` consume mask branches in the
//   same order: compact 0x0001, 0x0002, 0x0008, 0x8000, 0x0080, 0x0010,
//   0x0020, 0x0040, 0x0400, 0x0004, 0x0100, 0x4000.
// * EE gates the 0x0400 equipment counts through `sub_1407C4AB0(0x2001,0x25)`;
//   this captured stream is the zero/zero WORD-count form.  Treating those
//   bytes as Diamond's BYTE-count form shifts the following 0x0004 icon list
//   two bytes early and corrupts the cursor.
// * The standard D5FF shape follows the generic Diamond/EE inventory mask
//   walker exactly, including byte-sized 0x0400 equipment counts.
// * The HG D5FF sibling differs only at the 0x0400 branch: EE-player state
//   gating uses WORD clear/set counts. Keep that as a D5FF-local count-width
//   variant rather than as a named module/area/character exception.

const D5FF_MASK: u16 = 0xD5FF;
const D500_MISSING_LOW_D5FF_MASK: u16 = 0xD500;
const D5FF_CREATURE_STATE_RICH_CATEGORY_COUNT: usize = 3;
const D5FF_CREATURE_STATE_RICH_FIRST_ENTRY_BYTES: usize = 8;
const D5FF_CREATURE_STATE_RICH_SECOND_ENTRY_BYTES: usize = 7;
const D5FF_CREATURE_STATE_EXPECTED_RICH_EQUIPMENT_ROWS: u16 = 33;

pub(super) fn d5ff_live_stream_object_id_is_allowed(object_id: u32) -> bool {
    // CNWSMessage/CNWMessage read this field as an OBJECTID; the stricter
    // high-byte heuristic used by the generic path was a proxy guardrail, not
    // a decompiled reader rule.  Keep this exception narrow to the captured
    // live-stream-local creature ids plus Diamond's current-player inventory
    // owner.  The current-player sentinel is still only accepted after the
    // byte-exact D5FF reader-order proof below, so random sentinel/zero values
    // cannot claim a D5FF record.
    (1..0x0001_0000).contains(&object_id) || object_id == LEGACY_INVENTORY_CURRENT_PLAYER_OWNER
}

pub(super) fn try_parse_inventory_d5ff_hg_creature_equipment_state_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    try_parse_inventory_d5ff_hg_creature_equipment_state_shape_inner(
        bytes,
        record_offset,
        record_end,
    )
    .or_else(|| {
        try_parse_inventory_d5ff_standard_reader_order_shape(bytes, record_offset, record_end)
    })
}

pub(super) fn repair_d500_missing_low_d5ff_mask_for_ee(
    bytes: &mut [u8],
    record_offset: usize,
    record_end: usize,
) -> Option<()> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u16_le(bytes, record_offset + 5)? != D500_MISSING_LOW_D5FF_MASK
    {
        return None;
    }

    // Some legacy streams expose a header with the low mask byte cleared
    // (`0xD500`) while the following byte body is the decompile-owned D5FF
    // creature inventory state: compact 0x0001, 0x0002, 0x0008, 0x8000,
    // 0x0080, 0x0010, 0x0020, 0x0040, 0x0400, 0x0004, 0x0100, and 0x4000.
    // Mutate only after the standard D5FF reader-order parser proves that full
    // cursor.
    let original = *bytes.get(record_offset + 5)?;
    bytes[record_offset + 5] = 0xFF;
    let accepted =
        try_parse_inventory_d5ff_standard_reader_order_shape(bytes, record_offset, record_end)
            .is_some();
    if !accepted {
        bytes[record_offset + 5] = original;
        return None;
    }
    Some(())
}

fn try_parse_inventory_d5ff_hg_creature_equipment_state_shape_inner(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if mask != D5FF_MASK || !d5ff_live_stream_object_id_is_allowed(object_id) {
        return None;
    }

    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;

    // 0x0001 compact branch: SHORT, DWORD, INT, BOOL.  This exact D5FF
    // creature-state shape uses the compact branch, so the owned BOOL must be
    // false when the caller compares fragment requirements.
    cursor = cursor.checked_add(10)?;
    if cursor > record_end {
        return None;
    }
    fragment_bits = fragment_bits.checked_add(1)?;

    cursor = cursor.checked_add(4)?; // 0x0002 DWORD
    cursor = cursor.checked_add(4)?; // 0x0008 DWORD
    cursor = cursor.checked_add(12)?; // 0x8000 three INTs
    if cursor > record_end {
        return None;
    }

    cursor = advance_d5ff_ten_bit_value_groups(bytes, cursor, record_end)?; // 0x0080
    cursor = advance_d5ff_simple_categories(bytes, cursor, record_end)?; // 0x0010
    cursor = advance_d5ff_creature_state_rich_table(bytes, cursor, record_end)?; // 0x0020

    // 0x0040: this shape has a zero group count.
    if cursor >= record_end || bytes.get(cursor).copied() != Some(0) {
        return None;
    }
    cursor = cursor.checked_add(1)?;

    // 0x0400: EE count-width form selected by the reader's 0x2001/0x25 gate.
    // Both clear and set lists are empty, so this branch consumes no BOOLs.
    if read_u16_le(bytes, cursor)? != 0 || read_u16_le(bytes, cursor.checked_add(2)?)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;

    cursor = advance_d5ff_legacy_icon_list(bytes, cursor, record_end)?; // 0x0004

    // 0x0100 opcode stream and 0x4000 state stream are both present but empty.
    if cursor >= record_end || bytes.get(cursor).copied() != Some(0) {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    if read_u16_le(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(2)?;

    if cursor != record_end {
        return None;
    }

    GenericInventoryCandidate::new(record_end, fragment_bits).require_fragment_bit(0, false)
}

fn try_parse_inventory_d5ff_standard_reader_order_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if mask != D5FF_MASK || !d5ff_live_stream_object_id_is_allowed(object_id) {
        return None;
    }

    try_parse_generic_inventory_claim_with_branching(bytes, record_offset, record_end, mask)
}

pub(super) fn advance_verified_inventory_d5ff_hg_creature_equipment_state_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> Option<InventoryRecordClaim> {
    let candidate = try_parse_inventory_d5ff_hg_creature_equipment_state_shape(
        bytes,
        record_offset,
        record_end,
    )?;
    let remaining_bits = fragment_bits.len().checked_sub(*bit_cursor)?;
    if candidate.bits > remaining_bits
        || !candidate.fragment_requirements_match(fragment_bits, *bit_cursor)
    {
        return None;
    }

    if remaining_bits == candidate.bits {
        *bit_cursor = bit_cursor.saturating_add(candidate.bits);
        return Some(InventoryRecordClaim {
            fragment_bits: candidate.bits,
        });
    }

    if record_end != bytes.len() {
        // Midstream D5FF rows hand off fragment ownership exactly like other
        // inventory records. Applying terminal-style recovery before another
        // live-object submessage can silently drain the next record's BOOL
        // cursor.
        *bit_cursor = bit_cursor.saturating_add(candidate.bits);
        return Some(InventoryRecordClaim {
            fragment_bits: candidate.bits,
        });
    }

    // Diamond sub_455940 and EE sub_1407B4F70 return to the caller after the
    // enabled inventory mask branches; neither reader has a generic terminal
    // fragment-storage drain after 0x4000. Any terminal residual bits must be
    // owned by branch-specific counts already reflected in candidate.bits.
    None
}

fn advance_d5ff_ten_bit_value_groups(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<usize> {
    if cursor >= record_end {
        return None;
    }
    let group_count = bytes[cursor];
    cursor = cursor.checked_add(1)?;
    if group_count > MAX_REASONABLE_VALUE_GROUPS {
        return None;
    }

    for _ in 0..group_count {
        if record_end - cursor < 3 {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        let mask = read_u16_le(bytes, cursor)?;
        if (mask & !0x03FF) != 0 {
            return None;
        }
        cursor = cursor.checked_add(2)?;
        cursor = cursor.checked_add(usize::try_from(mask.count_ones()).ok()?)?;
        if cursor > record_end {
            return None;
        }
    }
    Some(cursor)
}

fn advance_d5ff_simple_categories(bytes: &[u8], cursor: usize, record_end: usize) -> Option<usize> {
    advance_d5ff_category_block(bytes, cursor, record_end, 3, 4, 4).map(|(cursor, _, _)| cursor)
}

fn advance_d5ff_creature_state_rich_table(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<usize> {
    let (cursor, first_entries, second_entries) = advance_d5ff_category_block(
        bytes,
        cursor,
        record_end,
        D5FF_CREATURE_STATE_RICH_CATEGORY_COUNT,
        D5FF_CREATURE_STATE_RICH_FIRST_ENTRY_BYTES,
        D5FF_CREATURE_STATE_RICH_SECOND_ENTRY_BYTES,
    )?;
    if first_entries != u32::from(D5FF_CREATURE_STATE_EXPECTED_RICH_EQUIPMENT_ROWS)
        || second_entries != 0
    {
        return None;
    }
    Some(cursor)
}

fn advance_d5ff_category_block(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
    category_count: usize,
    first_entry_bytes: usize,
    second_entry_bytes: usize,
) -> Option<(usize, u32, u32)> {
    let mut first_total = 0u32;
    let mut second_total = 0u32;
    for _ in 0..category_count {
        let first_count = read_u16_le(bytes, cursor)?;
        cursor = cursor.checked_add(2)?;
        if first_count > MAX_REASONABLE_CATEGORY_ENTRIES {
            return None;
        }
        let first_bytes = usize::from(first_count).checked_mul(first_entry_bytes)?;
        if first_bytes > record_end.saturating_sub(cursor) {
            return None;
        }
        cursor = cursor.checked_add(first_bytes)?;
        first_total = first_total.checked_add(u32::from(first_count))?;

        let second_count = read_u16_le(bytes, cursor)?;
        cursor = cursor.checked_add(2)?;
        if second_count > MAX_REASONABLE_CATEGORY_ENTRIES {
            return None;
        }
        let second_bytes = usize::from(second_count).checked_mul(second_entry_bytes)?;
        if second_bytes > record_end.saturating_sub(cursor) {
            return None;
        }
        cursor = cursor.checked_add(second_bytes)?;
        second_total = second_total.checked_add(u32::from(second_count))?;
    }
    Some((cursor, first_total, second_total))
}

fn advance_d5ff_legacy_icon_list(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<usize> {
    let first_count = read_u16_le(bytes, cursor)?;
    cursor = cursor.checked_add(2)?;
    if first_count > MAX_REASONABLE_CATEGORY_ENTRIES {
        return None;
    }
    cursor = cursor.checked_add(usize::from(first_count).checked_mul(3)?)?;
    if cursor > record_end {
        return None;
    }

    let second_count = read_u16_le(bytes, cursor)?;
    cursor = cursor.checked_add(2)?;
    if second_count > MAX_REASONABLE_CATEGORY_ENTRIES {
        return None;
    }
    cursor = cursor.checked_add(usize::from(second_count).checked_mul(3)?)?;
    (cursor <= record_end).then_some(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d5ff_standard_reader_order_record(mask: u16) -> Vec<u8> {
        let mut record = vec![b'I'];
        record.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        record.extend_from_slice(&mask.to_le_bytes());
        record.extend_from_slice(&[0; 10]); // 0x0001 compact body
        record.extend_from_slice(&[0; 4]); // 0x0002
        record.extend_from_slice(&[0; 4]); // 0x0008
        record.extend_from_slice(&[0; 12]); // 0x8000
        record.push(0); // 0x0080 ten-bit groups
        record.extend_from_slice(&[0; 12]); // 0x0010 simple categories
        record.extend_from_slice(&[0; 12]); // 0x0020 rich categories
        record.push(0); // 0x0040 ten-bit groups
        record.extend_from_slice(&[0, 0]); // 0x0400 byte clear/set counts
        record.extend_from_slice(&[0; 4]); // 0x0004 icon lists
        record.push(0); // 0x0100 opcode stream
        record.extend_from_slice(&[0; 2]); // 0x4000 state stream
        record
    }

    #[test]
    fn d5ff_standard_shape_uses_generic_reader_order_without_named_capture_values() {
        let record = d5ff_standard_reader_order_record(D5FF_MASK);

        let candidate =
            try_parse_inventory_d5ff_standard_reader_order_shape(&record, 0, record.len())
                .expect("standard D5FF reader-order shape should parse");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(candidate.bits, 1);
    }

    #[test]
    fn d5ff_standard_shape_repairs_missing_low_mask_after_full_cursor_proof() {
        let mut record = d5ff_standard_reader_order_record(D500_MISSING_LOW_D5FF_MASK);

        let len = record.len();
        repair_d500_missing_low_d5ff_mask_for_ee(&mut record, 0, len)
            .expect("D500 header should repair only after standard D5FF cursor proof");
        assert_eq!(read_u16_le(&record, 5), Some(D5FF_MASK));
    }

    #[test]
    fn d5ff_midstream_claim_consumes_only_decompiled_inventory_bits() {
        let mut stream = d5ff_standard_reader_order_record(D5FF_MASK);
        let record_end = stream.len();
        stream.extend_from_slice(&[b'W', 0x01, 0x0E]);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_d5ff_hg_creature_equipment_state_shape(
            &stream,
            0,
            record_end,
            &[false, true],
            &mut bit_cursor,
        )
        .expect("midstream D5FF should claim the proven inventory cursor");

        assert_eq!(
            claim.fragment_bits, 1,
            "0x0001 compact branch owns one BOOL; following records own later bits"
        );
        assert_eq!(
            bit_cursor, 1,
            "midstream D5FF must not drain terminal-only fragment tail bits"
        );
    }

    #[test]
    fn d5ff_terminal_small_tail_rejects_without_typed_owner() {
        let record = d5ff_standard_reader_order_record(D5FF_MASK);

        let mut bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_d5ff_hg_creature_equipment_state_shape(
                &record,
                0,
                record.len(),
                &[false, true],
                &mut bit_cursor,
            )
            .is_none(),
            "terminal D5FF fallback must not hide a one-bit cursor shift"
        );
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn d5ff_terminal_residual_byte_rejects_without_typed_owner() {
        let record = d5ff_standard_reader_order_record(D5FF_MASK);

        let mut bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_d5ff_hg_creature_equipment_state_shape(
                &record,
                0,
                record.len(),
                &[false, true, false, true, false, true, false, true, false],
                &mut bit_cursor,
            )
            .is_none(),
            "terminal D5FF must not drain residual fragment bytes without a typed branch owner"
        );
        assert_eq!(bit_cursor, 0);
    }
}
