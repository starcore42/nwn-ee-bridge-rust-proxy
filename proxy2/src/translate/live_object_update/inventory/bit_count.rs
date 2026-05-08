use super::*;

// Ten-bit value-group parser and fragment-neutral bit-count helpers.

pub(super) fn apply_ten_bit_groups(
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
