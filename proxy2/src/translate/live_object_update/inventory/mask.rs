use super::*;

// Generic inventory mask orchestration and mask-local branch helpers.

pub(super) fn try_parse_generic_inventory_with_branching(
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

pub(super) fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
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
