use super::*;

// Generic inventory mask orchestration and mask-local branch helpers.

pub(super) fn try_parse_generic_inventory_with_branching(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    mask: u16,
) -> Option<usize> {
    try_parse_generic_inventory_claim_with_branching(bytes, record_offset, record_end, mask)
        .map(|candidate| candidate.bits)
}

pub(super) fn try_parse_generic_inventory_claim_with_branching(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    mask: u16,
) -> Option<GenericInventoryCandidate> {
    generic_inventory_candidates_after_mask(bytes, record_offset, record_end, mask)
        .into_iter()
        .find(|candidate| candidate.cursor == record_end)
}

pub(super) fn try_parse_generic_inventory_prefix_with_branching(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
    mask: u16,
) -> Option<GenericInventoryCandidate> {
    let mut candidates =
        generic_inventory_candidates_after_mask(bytes, record_offset, search_end, mask)
            .into_iter()
            .filter(|candidate| candidate.cursor <= search_end)
            .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| (candidate.cursor, candidate.bits));
    candidates.dedup_by_key(|candidate| (candidate.cursor, candidate.bits));
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "inventory prefix candidates: mask=0x{mask:04X} offset={record_offset} search_end={search_end} candidates={:?}",
            candidates
                .iter()
                .take(16)
                .map(|candidate| (candidate.cursor, candidate.bits))
                .collect::<Vec<_>>()
        );
    }
    (candidates.len() == 1).then(|| candidates[0])
}

fn generic_inventory_candidates_after_mask(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    mask: u16,
) -> Vec<GenericInventoryCandidate> {
    let Some(cursor) = record_offset.checked_add(7) else {
        return Vec::new();
    };
    let mut candidates = vec![GenericInventoryCandidate::new(cursor, 0)];
    trace_inventory_stage(mask, "start", &candidates);

    if (mask & 0x0001) != 0 {
        candidates = apply_0001(&candidates, record_end);
        trace_inventory_stage(mask, "0001", &candidates);
    }
    if (mask & 0x0002) != 0 {
        candidates = advance_candidates(&candidates, record_end, 4, 0);
        trace_inventory_stage(mask, "0002", &candidates);
    }
    if (mask & 0x0008) != 0 {
        candidates = advance_candidates(&candidates, record_end, 4, 0);
        trace_inventory_stage(mask, "0008", &candidates);
    }
    if (mask & 0x8000) != 0 {
        candidates = advance_candidates(&candidates, record_end, 12, 0);
        trace_inventory_stage(mask, "8000", &candidates);
    }
    if (mask & 0x0080) != 0 {
        candidates = apply_ten_bit_groups(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0080", &candidates);
    }
    if (mask & LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK) != 0 {
        candidates = apply_simple_categories(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0010", &candidates);
    }
    if (mask & LEGACY_INVENTORY_RICH_CATEGORY_MASK) != 0 {
        candidates = apply_rich_categories(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0020", &candidates);
    }
    if (mask & 0x0040) != 0 {
        candidates = apply_ten_bit_groups(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0040", &candidates);
    }
    if (mask & 0x0400) != 0 {
        candidates = apply_0400(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0400", &candidates);
    }
    if (mask & LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK) != 0 {
        candidates = apply_legacy_icon_list(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0004", &candidates);
    }
    if (mask & 0x0200) != 0 {
        // EE `sub_1407B4F70` reads the live-inventory mask branches in the same
        // order observed in Diamond's `sub_455940`: the equipment/icon blocks
        // precede the two-BOOL 0x0200 branch, which then precedes the 0x0100
        // GUI opcode stream. Large HG masks such as D5FF rely on that ordering:
        // their 0x0004 icon list begins with bytes that look like an equipment
        // set-list if 0x0100 is incorrectly consumed first.
        //
        // The second CNW BOOL selects the DWORD zero-count path when false;
        // captures show the first BOOL can vary without changing that read
        // cursor, so the branch parser consumes/counts it without treating it
        // as a shape discriminator.
        candidates = apply_0200(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0200", &candidates);
    }
    if (mask & 0x0100) != 0 {
        candidates = apply_0100(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0100", &candidates);
    }
    if (mask & 0x2000) != 0 {
        candidates = apply_2000(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "2000", &candidates);
    }
    if (mask & 0x0800) != 0 {
        candidates = apply_0800(&candidates, record_end);
        trace_inventory_stage(mask, "0800", &candidates);
    }
    if (mask & 0x1000) != 0 {
        // EE and Diamond both treat this inventory bit as local UI-state clear.
        // No read-buffer bytes or CNW fragment BOOLs are consumed.
    }
    if (mask & 0x4000) != 0 {
        candidates = apply_4000(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "4000", &candidates);
    }

    candidates
}

fn trace_inventory_stage(
    mask: u16,
    stage: &'static str,
    candidates: &[GenericInventoryCandidate],
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "inventory generic stage: mask=0x{mask:04X} stage={stage} count={} sample={:?}",
        candidates.len(),
        candidates
            .iter()
            .take(8)
            .map(|candidate| (candidate.cursor, candidate.bits))
            .collect::<Vec<_>>()
    );
}

fn apply_0001(
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    // Diamond sub_455940 and EE sub_1407B4F70 both read:
    //
    //   WORD, DWORD, INT, BOOL
    //
    // before the optional extended row/string branch. The compact 10-byte
    // inventory delta shape is therefore exact only when the branch BOOL is
    // false. If the BOOL is true, a focused extended-branch parser must own
    // the additional read-buffer fields instead of letting this generic
    // candidate stop early and misalign following live-object records.
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in advance_candidates(candidates, record_end, 10, 1) {
        if let Some(candidate) =
            candidate.require_fragment_bit(candidate.bits.saturating_sub(1), false)
        {
            next.push(candidate);
        }
    }
    next
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
            next.push(candidate.advanced(
                candidate.cursor + bytes_to_advance,
                candidate.bits.saturating_add(bits_to_add),
            ));
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
            if let Some(candidate) = candidate
                .advanced(candidate.cursor + 4, candidate.bits.saturating_add(2))
                .require_fragment_bit(candidate.bits.saturating_add(1), false)
            {
                next.push(candidate);
            }
        }
        if candidate.cursor < record_end {
            let byte_mask_count = usize::from(bytes[candidate.cursor]);
            let masks_offset = candidate.cursor + 1;
            if byte_mask_count <= 64
                && masks_offset <= record_end
                && byte_mask_count <= record_end - masks_offset
            {
                if let Some(candidate) = candidate
                    .advanced(masks_offset + byte_mask_count, candidate.bits.saturating_add(2))
                    .require_fragment_bit(candidate.bits.saturating_add(1), true)
                {
                    next.push(candidate);
                }
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
        if let Some(candidate) = candidate
            .advanced(candidate.cursor, candidate.bits.saturating_add(1))
            .require_fragment_bit(candidate.bits, false)
        {
            next.push(candidate);
        }
        if candidate.cursor <= record_end && 12 <= record_end - candidate.cursor {
            if let Some(candidate) = candidate
                .advanced(candidate.cursor + 12, candidate.bits.saturating_add(1))
                .require_fragment_bit(candidate.bits, true)
            {
                next.push(candidate);
            }
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
            next.push(candidate.advanced(cursor, bits));
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
