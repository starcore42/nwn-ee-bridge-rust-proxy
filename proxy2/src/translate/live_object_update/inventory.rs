//! Inventory-family live-object update policy.
//!
//! Inventory and GUI item-create submessages can own fragment BOOLs. The
//! decompile-backed rule is intentionally stricter than "the next byte looks
//! like a live-object opcode": legacy `I` records contain opcode-like row bytes,
//! so a boundary inside an inventory record is safe only after the inventory
//! family validates the exact record shape and fragment-bit count.

use super::{read_u16_le, read_u32_le};

const LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK: u16 = 0x0010;
const LEGACY_INVENTORY_RICH_CATEGORY_MASK: u16 = 0x0020;
const LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK: u16 = 0x0004;
const LEGACY_INVENTORY_CATEGORY_COUNT: usize = 3;
const MAX_REASONABLE_CATEGORY_ENTRIES: u16 = 4096;
const MAX_REASONABLE_VALUE_GROUPS: u8 = 64;
const MAX_REASONABLE_FEATURE25_OBJECTS: u32 = 128;
const GENERIC_INVENTORY_PARSE_MASK: u16 = 0x0001
    | 0x0002
    | LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK
    | 0x0008
    | LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK
    | LEGACY_INVENTORY_RICH_CATEGORY_MASK
    | 0x0040
    | 0x0080
    | 0x0100
    | 0x0200
    | 0x0400
    | 0x0800
    | 0x1000
    | 0x2000
    | 0x4000
    | 0x8000;

#[derive(Debug, Clone, Copy)]
pub(super) struct InventoryRecordClaim {
    pub fragment_bits: usize,
}

#[derive(Debug, Clone, Copy)]
struct GenericInventoryCandidate {
    cursor: usize,
    bits: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct Feature25Shape {
    second_count: u32,
    block_end: usize,
    missing_second_count: bool,
}

pub(super) fn owns_fragment_tail(opcode: u8) -> bool {
    matches!(opcode, b'I' | b'G')
}

pub(super) fn advance_verified_inventory_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> Option<InventoryRecordClaim> {
    let bits = try_get_legacy_live_inventory_fragment_bit_count(bytes, offset, record_end)?;
    if bits > fragment_bits.len().saturating_sub(*bit_cursor) {
        return None;
    }
    *bit_cursor = bit_cursor.saturating_add(bits);
    Some(InventoryRecordClaim {
        fragment_bits: bits,
    })
}

pub(super) fn try_get_legacy_live_inventory_fragment_bit_count(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
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
    if object_id != 0xFFFF_FFFD && !looks_like_legacy_live_object_id_value(object_id) {
        return None;
    }

    if mask == 0x2A00 {
        return try_parse_inventory_2a00_shape(bytes, record_offset, record_end);
    }

    if mask == 0x2000 {
        let feature25 = try_parse_feature25_record(bytes, record_offset, record_end)?;
        return Some(usize::try_from(feature25.second_count).ok()?.saturating_mul(3));
    }

    if mask == 0x0400 {
        // Decompile-backed inventory/equipment delta. Diamond and EE both read
        // a clear-count byte plus slot bytes, then a set-count byte plus slot
        // bytes; the set side owns one CNW BOOL fragment bit per slot. This is
        // therefore an identity translation only after this exact cursor
        // consumption succeeds. It must never be accepted as raw passthrough.
        let set_count = try_parse_inventory_0400(bytes, record_offset.checked_add(7)?, record_end)?;
        return Some(usize::from(set_count));
    }

    if mask == 0x0401 {
        let base_cursor = record_offset.checked_add(7)?.checked_add(10)?;
        let set_count = try_parse_inventory_0400(bytes, base_cursor, record_end)
            .or_else(|| try_parse_inventory_0400(bytes, base_cursor.checked_add(2)?, record_end))?;
        return Some(1usize.saturating_add(usize::from(set_count)));
    }

    if (mask & !GENERIC_INVENTORY_PARSE_MASK) == 0 && (mask & (0x0200 | 0x0800)) != 0 {
        return try_parse_generic_inventory_with_branching(bytes, record_offset, record_end, mask);
    }

    None
}

fn try_parse_generic_inventory_with_branching(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    mask: u16,
) -> Option<usize> {
    let mut candidates = vec![GenericInventoryCandidate {
        cursor: record_offset.checked_add(7)?,
        bits: 0,
    }];

    if (mask & 0x0001) != 0 {
        candidates = advance_candidates(&candidates, record_end, 10, 1);
    }
    if (mask & 0x0002) != 0 {
        candidates = advance_candidates(&candidates, record_end, 4, 0);
    }
    if (mask & 0x0008) != 0 {
        candidates = advance_candidates(&candidates, record_end, 4, 0);
    }
    if (mask & 0x8000) != 0 {
        candidates = advance_candidates(&candidates, record_end, 12, 0);
    }
    if (mask & 0x0080) != 0 {
        candidates = apply_ten_bit_groups(bytes, &candidates, record_end);
    }
    if (mask & LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK) != 0 {
        candidates = apply_simple_categories(bytes, &candidates, record_end);
    }
    if (mask & LEGACY_INVENTORY_RICH_CATEGORY_MASK) != 0 {
        candidates = apply_rich_categories(bytes, &candidates, record_end);
    }
    if (mask & 0x0040) != 0 {
        candidates = apply_ten_bit_groups(bytes, &candidates, record_end);
    }
    if (mask & 0x0400) != 0 {
        candidates = apply_0400(bytes, &candidates, record_end);
    }
    if (mask & LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK) != 0 {
        candidates = apply_legacy_icon_list(bytes, &candidates, record_end);
    }
    if (mask & 0x0200) != 0 {
        candidates = apply_0200(bytes, &candidates, record_end);
    }
    if (mask & 0x0100) != 0 {
        candidates = apply_0100(bytes, &candidates, record_end);
    }
    if (mask & 0x2000) != 0 {
        candidates = apply_2000(bytes, &candidates, record_end);
    }
    if (mask & 0x0800) != 0 {
        candidates = apply_0800(&candidates, record_end);
    }
    if (mask & 0x1000) != 0 {
        // EE and Diamond both treat this inventory bit as local UI-state clear.
        // No read-buffer bytes or CNW fragment BOOLs are consumed.
    }
    if (mask & 0x4000) != 0 {
        candidates = apply_4000(bytes, &candidates, record_end);
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.cursor == record_end)
        .map(|candidate| candidate.bits)
}

fn advance_candidates(
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
    bytes_to_advance: usize,
    bits_to_add: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if candidate.cursor <= record_end && bytes_to_advance <= record_end - candidate.cursor {
            next.push(GenericInventoryCandidate {
                cursor: candidate.cursor + bytes_to_advance,
                bits: candidate.bits.saturating_add(bits_to_add),
            });
        }
    }
    next
}

fn apply_ten_bit_groups(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, _)) =
            advance_ten_bit_value_groups(bytes, candidate.cursor, record_end)
        {
            next.push(GenericInventoryCandidate {
                cursor,
                bits: candidate.bits,
            });
        }
    }
    next
}

fn apply_simple_categories(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, _)) =
            advance_simple_category_block(bytes, candidate.cursor, record_end)
        {
            next.push(GenericInventoryCandidate {
                cursor,
                bits: candidate.bits,
            });
        }
    }
    next
}

fn apply_rich_categories(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, second_entries)) =
            advance_rich_category_block(bytes, candidate.cursor, record_end)
        {
            next.push(GenericInventoryCandidate {
                cursor,
                bits: candidate
                    .bits
                    .saturating_add(usize::try_from(second_entries).unwrap_or(usize::MAX).saturating_mul(2)),
            });
        }
    }
    next
}

fn apply_0400(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if candidate.cursor >= record_end {
            continue;
        }
        let mut cursor = candidate.cursor;
        let first_count = usize::from(bytes[cursor]);
        cursor += 1;
        if first_count > record_end.saturating_sub(cursor) {
            continue;
        }
        cursor += first_count;
        if cursor >= record_end {
            continue;
        }
        let second_count = usize::from(bytes[cursor]);
        cursor += 1;
        if second_count > record_end.saturating_sub(cursor) {
            continue;
        }
        cursor += second_count;
        next.push(GenericInventoryCandidate {
            cursor,
            bits: candidate.bits.saturating_add(second_count),
        });
    }
    next
}

fn apply_legacy_icon_list(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, _, fragment_bits)) =
            advance_legacy_icon_list_block(bytes, candidate.cursor, record_end)
        {
            next.push(GenericInventoryCandidate {
                cursor,
                bits: candidate
                    .bits
                    .saturating_add(usize::try_from(fragment_bits).unwrap_or(usize::MAX)),
            });
        }
    }
    next
}

fn apply_0200(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len().saturating_mul(2));
    for candidate in candidates {
        if candidate.cursor <= record_end && record_end - candidate.cursor >= 4 {
            next.push(GenericInventoryCandidate {
                cursor: candidate.cursor + 4,
                bits: candidate.bits.saturating_add(2),
            });
        }
        if candidate.cursor < record_end {
            let byte_mask_count = usize::from(bytes[candidate.cursor]);
            let masks_offset = candidate.cursor + 1;
            if byte_mask_count <= 64
                && masks_offset <= record_end
                && byte_mask_count <= record_end - masks_offset
            {
                next.push(GenericInventoryCandidate {
                    cursor: masks_offset + byte_mask_count,
                    bits: candidate.bits.saturating_add(2),
                });
            }
        }
    }
    next
}

fn apply_0100(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let Some(cursor) = advance_inventory_0100_opcode_stream(bytes, candidate.cursor, record_end)
        else {
            continue;
        };
        next.push(GenericInventoryCandidate {
            cursor,
            bits: candidate.bits,
        });
    }
    next
}

fn apply_2000(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let Some(feature25) = try_parse_feature25_at(bytes, candidate.cursor, record_end) else {
            continue;
        };
        if feature25.missing_second_count || feature25.block_end > record_end {
            continue;
        }
        next.push(GenericInventoryCandidate {
            cursor: feature25.block_end,
            bits: candidate.bits.saturating_add(
                usize::try_from(feature25.second_count)
                    .unwrap_or(usize::MAX)
                    .saturating_mul(3),
            ),
        });
    }
    next
}

fn apply_0800(
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len().saturating_mul(2));
    for candidate in candidates {
        next.push(GenericInventoryCandidate {
            cursor: candidate.cursor,
            bits: candidate.bits.saturating_add(1),
        });
        if candidate.cursor <= record_end && 12 <= record_end - candidate.cursor {
            next.push(GenericInventoryCandidate {
                cursor: candidate.cursor + 12,
                bits: candidate.bits.saturating_add(1),
            });
        }
    }
    next
}

fn apply_4000(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let mut cursor = candidate.cursor;
        if cursor > record_end || record_end - cursor < 2 {
            continue;
        }
        let Some(entry_count) = read_u16_le(bytes, cursor) else {
            continue;
        };
        cursor += 2;
        if entry_count > MAX_REASONABLE_CATEGORY_ENTRIES {
            continue;
        }
        let mut bits = candidate.bits;
        let mut ok = true;
        for _ in 0..entry_count {
            if cursor >= record_end {
                ok = false;
                break;
            }
            let opcode = bytes[cursor];
            cursor += 1;
            if opcode == b'S' {
                if cursor > record_end || 2 > record_end - cursor {
                    ok = false;
                    break;
                }
                cursor += 2;
            } else if opcode == b'U' {
                if cursor > record_end || 5 > record_end - cursor {
                    ok = false;
                    break;
                }
                cursor += 5;
                bits = bits.saturating_add(1);
            }
        }
        if ok {
            next.push(GenericInventoryCandidate { cursor, bits });
        }
    }
    next
}

fn try_parse_inventory_2a00_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let branch_cursor = record_offset.checked_add(7)?;
    let try_parse_after_0200 = |cursor: usize| -> Option<usize> {
        let feature25 = try_parse_feature25_at(bytes, cursor, record_end)?;
        if feature25.missing_second_count || feature25.block_end > record_end {
            return None;
        }
        if feature25.block_end != record_end && record_end - feature25.block_end != 12 {
            return None;
        }
        Some(
            2usize
                .saturating_add(usize::try_from(feature25.second_count).ok()?.saturating_mul(3))
                .saturating_add(1),
        )
    };

    if record_end - branch_cursor >= 4 && read_u32_le(bytes, branch_cursor)? == 0 {
        if let Some(bits) = try_parse_after_0200(branch_cursor + 4) {
            return Some(bits);
        }
    }

    if branch_cursor < record_end {
        let byte_mask_count = usize::from(bytes[branch_cursor]);
        let masks_offset = branch_cursor + 1;
        if byte_mask_count <= 64
            && masks_offset <= record_end
            && byte_mask_count <= record_end - masks_offset
        {
            return try_parse_after_0200(masks_offset + byte_mask_count);
        }
    }

    None
}

fn try_parse_feature25_record(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<Feature25Shape> {
    if record_end - record_offset < 11 || read_u16_le(bytes, record_offset + 5)? != 0x2000 {
        return None;
    }
    let shape = try_parse_feature25_at(bytes, record_offset + 7, record_end)?;
    (shape.block_end == record_end).then_some(shape)
}

fn try_parse_feature25_at(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<Feature25Shape> {
    if cursor > bytes.len() || record_end > bytes.len() || cursor > record_end {
        return None;
    }
    let first_count = read_u32_le(bytes, cursor)?;
    if first_count > MAX_REASONABLE_FEATURE25_OBJECTS {
        return None;
    }
    let first_objects = cursor.checked_add(4)?;
    let first_end = first_objects.checked_add(usize::try_from(first_count).ok()?.checked_mul(4)?)?;
    if first_end > record_end
        || !looks_like_feature25_object_list(bytes, first_objects, first_count, first_end)
    {
        return None;
    }
    if first_end == record_end {
        return Some(Feature25Shape {
            block_end: record_end,
            missing_second_count: true,
            ..Feature25Shape::default()
        });
    }
    if record_end - first_end < 4 {
        return None;
    }
    let second_count = read_u32_le(bytes, first_end)?;
    if second_count > MAX_REASONABLE_FEATURE25_OBJECTS {
        return None;
    }
    let second_objects = first_end.checked_add(4)?;
    let second_end =
        second_objects.checked_add(usize::try_from(second_count).ok()?.checked_mul(4)?)?;
    if second_end > record_end
        || !looks_like_feature25_object_list(bytes, second_objects, second_count, second_end)
    {
        return None;
    }
    Some(Feature25Shape {
        second_count,
        block_end: second_end,
        missing_second_count: false,
    })
}

fn looks_like_feature25_object_list(
    bytes: &[u8],
    offset: usize,
    count: u32,
    record_end: usize,
) -> bool {
    if count > MAX_REASONABLE_FEATURE25_OBJECTS
        || offset > record_end
        || record_end > bytes.len()
        || usize::try_from(count)
            .ok()
            .is_none_or(|count| count > (record_end - offset) / 4)
    {
        return false;
    }
    for index in 0..usize::try_from(count).unwrap_or(usize::MAX) {
        let Some(object_id) = read_u32_le(bytes, offset + index * 4) else {
            return false;
        };
        if !looks_like_legacy_live_object_id_value(object_id) {
            return false;
        }
    }
    true
}

fn try_parse_inventory_0400(bytes: &[u8], mut cursor: usize, record_end: usize) -> Option<u8> {
    if cursor >= record_end {
        return None;
    }
    let clear_count = usize::from(bytes[cursor]);
    cursor += 1;
    if clear_count > record_end.saturating_sub(cursor) {
        return None;
    }
    cursor += clear_count;
    if cursor >= record_end {
        return None;
    }
    let set_count = bytes[cursor];
    cursor += 1;
    if usize::from(set_count) > record_end.saturating_sub(cursor) {
        return None;
    }
    cursor += usize::from(set_count);
    (cursor == record_end).then_some(set_count)
}

fn advance_simple_category_block(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<(usize, u32, u32)> {
    advance_category_block(bytes, cursor, record_end, LEGACY_INVENTORY_CATEGORY_COUNT, 4, 4)
}

fn advance_rich_category_block(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<(usize, u32, u32)> {
    advance_category_block(bytes, cursor, record_end, LEGACY_INVENTORY_CATEGORY_COUNT, 2, 7)
}

fn advance_category_block(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
    category_count: usize,
    first_entry_bytes: usize,
    second_entry_bytes: usize,
) -> Option<(usize, u32, u32)> {
    if record_end > bytes.len() || cursor > record_end {
        return None;
    }
    let mut first_total = 0u32;
    let mut second_total = 0u32;
    for _ in 0..category_count {
        let first_count = read_u16_le(bytes, cursor)?;
        cursor += 2;
        if first_count > MAX_REASONABLE_CATEGORY_ENTRIES {
            return None;
        }
        let first_bytes = usize::from(first_count).checked_mul(first_entry_bytes)?;
        if first_bytes > record_end.saturating_sub(cursor) {
            return None;
        }
        cursor += first_bytes;
        first_total = first_total.saturating_add(u32::from(first_count));

        let second_count = read_u16_le(bytes, cursor)?;
        cursor += 2;
        if second_count > MAX_REASONABLE_CATEGORY_ENTRIES {
            return None;
        }
        let second_bytes = usize::from(second_count).checked_mul(second_entry_bytes)?;
        if second_bytes > record_end.saturating_sub(cursor) {
            return None;
        }
        cursor += second_bytes;
        second_total = second_total.saturating_add(u32::from(second_count));
    }
    Some((cursor, first_total, second_total))
}

fn advance_legacy_icon_list_block(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<(usize, u32, u32, u32)> {
    if record_end > bytes.len() || cursor > record_end || record_end - cursor < 2 {
        return None;
    }
    let first_count = read_u16_le(bytes, cursor)?;
    cursor += 2;
    if first_count > MAX_REASONABLE_CATEGORY_ENTRIES {
        return None;
    }
    let first_bytes = usize::from(first_count).checked_mul(3)?;
    if first_bytes > record_end.saturating_sub(cursor) {
        return None;
    }
    cursor += first_bytes;

    let second_count = read_u16_le(bytes, cursor)?;
    cursor += 2;
    if second_count > MAX_REASONABLE_CATEGORY_ENTRIES {
        return None;
    }
    let second_bytes = usize::from(second_count).checked_mul(3)?;
    if second_bytes > record_end.saturating_sub(cursor) {
        return None;
    }
    cursor += second_bytes;
    Some((cursor, u32::from(first_count), u32::from(second_count), 0))
}

fn advance_ten_bit_value_groups(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<(usize, u32, u32)> {
    if record_end > bytes.len() || cursor >= record_end {
        return None;
    }
    let group_count = bytes[cursor];
    cursor += 1;
    if group_count > MAX_REASONABLE_VALUE_GROUPS {
        return None;
    }
    let mut value_count = 0u32;
    for _ in 0..group_count {
        if record_end - cursor < 3 {
            return None;
        }
        cursor += 1;
        let mask = read_u16_le(bytes, cursor)?;
        cursor += 2;
        if (mask & !0x03FF) != 0 {
            return None;
        }
        let set_bits = mask.count_ones();
        if usize::try_from(set_bits).ok()? > record_end.saturating_sub(cursor) {
            return None;
        }
        cursor += usize::try_from(set_bits).ok()?;
        value_count = value_count.saturating_add(set_bits);
    }
    Some((cursor, u32::from(group_count), value_count))
}

fn advance_inventory_0100_opcode_stream(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<usize> {
    if cursor >= record_end {
        return None;
    }
    let entry_count = bytes[cursor];
    cursor += 1;
    for _ in 0..entry_count {
        if cursor >= record_end {
            return None;
        }
        let opcode = bytes[cursor];
        cursor += 1;
        if opcode == b'D' {
            if cursor > record_end || 4 > record_end - cursor {
                return None;
            }
            cursor += 4;
        } else if opcode == b'S' || opcode == b'U' {
            if cursor > record_end || 8 > record_end - cursor {
                return None;
            }
            cursor += 8;
        } else if opcode == b'A' {
            if cursor > record_end || 4 > record_end - cursor {
                return None;
            }
            let item_type = read_u16_le(bytes, cursor)?;
            cursor += 4;
            if item_type != 0 && item_type != 2 {
                if cursor > record_end || 4 > record_end - cursor {
                    return None;
                }
                cursor += 4;
            }
            if matches!(item_type, 0 | 2 | 4 | 12 | 19) {
                if cursor > record_end || 12 > record_end - cursor {
                    return None;
                }
                cursor += 12;
            }
            if item_type == 4 || item_type == 19 {
                if cursor > record_end || 4 > record_end - cursor {
                    return None;
                }
                cursor += 4;
            }
        }
    }
    Some(cursor)
}

fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }
    let high_byte = object_id & 0xFF00_0000;
    matches!(
        high_byte,
        0x8000_0000
            | 0x8800_0000
            | 0xFF00_0000
            | 0x0100_0000
            | 0x0500_0000
            | 0x0800_0000
            | 0x3500_0000
    ) || (0x0000_1000..=0x00FF_FFFF).contains(&object_id)
}
