use super::*;

// Simple and rich inventory category block parsers.

pub(super) fn apply_simple_categories(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, _)) =
            advance_simple_category_block(bytes, candidate.cursor, record_end)
        {
            next.push(candidate.advanced(cursor, candidate.bits));
        }
    }
    next
}

pub(super) fn apply_rich_categories(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, second_entries)) =
            advance_rich_category_block(bytes, candidate.cursor, record_end)
        {
            next.push(
                candidate.advanced(
                    cursor,
                    candidate.bits.saturating_add(
                        usize::try_from(second_entries)
                            .unwrap_or(usize::MAX)
                            .saturating_mul(2),
                    ),
                ),
            );
        }
    }
    next
}

fn advance_simple_category_block(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<(usize, u32, u32)> {
    advance_category_block(
        bytes,
        cursor,
        record_end,
        LEGACY_INVENTORY_CATEGORY_COUNT,
        4,
        4,
    )
}

fn advance_rich_category_block(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<(usize, u32, u32)> {
    advance_category_block(
        bytes,
        cursor,
        record_end,
        LEGACY_INVENTORY_CATEGORY_COUNT,
        2,
        7,
    )
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
