use super::*;

// Focused D5FF inventory-family parser.
//
// This module intentionally does not widen the generic mask walker.  The HG
// Starcore5 area-entry creature capture carries an `I/0xD5FF` record for the
// same small object id (`0x000000FE`) that the immediately preceding `A/P/U`
// live-object records already proved.  The older self-inventory D5FF packets
// still use the generic path; this helper owns only the creature equipment/UI
// state shape exposed by that capture.
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
// * The following 0x0004 block is exact: WORD first-count, CHAR+WORD entries,
//   WORD second-count, WORD+BYTE entries.  It is followed by a zero-entry
//   0x0100 opcode stream and a zero-entry 0x4000 state stream.
//
// The 0x0020 table preceding the zero 0x0040 group is deliberately kept as a
// named D5FF creature-state table instead of teaching the generic category
// parser a new heuristic.  If another capture proves a broader server writer
// shape, it should become a typed sibling here with its own fixture.

const D5FF_MASK: u16 = 0xD5FF;
const D5FF_CREATURE_STATE_RICH_CATEGORY_COUNT: usize = 3;
const D5FF_CREATURE_STATE_RICH_FIRST_ENTRY_BYTES: usize = 8;
const D5FF_CREATURE_STATE_RICH_SECOND_ENTRY_BYTES: usize = 7;
const D5FF_CREATURE_STATE_EXPECTED_RICH_EQUIPMENT_ROWS: u16 = 33;

pub(super) fn d5ff_small_live_stream_object_id_is_allowed(object_id: u32) -> bool {
    // CNWSMessage/CNWMessage read this field as an OBJECTID; the stricter
    // high-byte heuristic used by the generic path was a proxy guardrail, not
    // a decompiled reader rule.  Keep this exception narrow to the captured
    // live-stream-local creature ids so random sentinel/zero values cannot
    // claim a D5FF record.
    (1..0x0001_0000).contains(&object_id)
}

pub(super) fn try_parse_inventory_d5ff_hg_creature_equipment_state_shape(
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
    if mask != D5FF_MASK || !d5ff_small_live_stream_object_id_is_allowed(object_id) {
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

    // The captured D5FF creature state is terminal in its `P 05 01` read
    // buffer.  All remaining CNW fragment storage therefore belongs to this
    // inventory record; leaving it unconsumed makes the exact live-object proof
    // fail even though no following submessage can own those bits.  This is
    // deliberately tied to the byte-exact D5FF shape above.  A future typed
    // model should split the table-owned BOOLs by subobject family, but until
    // then we still keep ownership explicit instead of treating the tail as
    // unclassified padding.
    *bit_cursor = fragment_bits.len();
    Some(InventoryRecordClaim {
        fragment_bits: remaining_bits,
    })
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
