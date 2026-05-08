// Legacy inventory icon-list parser.

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

