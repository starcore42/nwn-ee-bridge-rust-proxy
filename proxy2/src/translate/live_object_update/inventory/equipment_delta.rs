// Equipment delta masks, including exact set/clear cursor and fragment-bit counts.

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

