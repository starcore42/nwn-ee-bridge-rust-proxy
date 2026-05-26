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

pub(super) fn try_parse_generic_inventory_claim_matching_fragment_bits(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    mask: u16,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<GenericInventoryCandidate> {
    let mut candidates =
        generic_inventory_candidates_after_mask(bytes, record_offset, record_end, mask)
            .into_iter()
            .filter(|candidate| {
                candidate.cursor == record_end
                    && candidate.bits <= fragment_bits.len().saturating_sub(bit_cursor)
                    && candidate.fragment_requirements_match(fragment_bits, bit_cursor)
            })
            .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| {
        (
            candidate.cursor,
            candidate.bits,
            candidate.required_true_bits,
            candidate.required_false_bits,
        )
    });
    candidates.dedup_by_key(|candidate| {
        (
            candidate.cursor,
            candidate.bits,
            candidate.required_true_bits,
            candidate.required_false_bits,
        )
    });
    (candidates.len() == 1).then(|| candidates[0])
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
        // Diamond `sub_455940` (004559D0..00455AAD) and EE `sub_1407B4F70`
        // (1407B50D7..1407B51ED) both treat this inventory bit as local
        // UI-state clear. No read-buffer bytes or CNW fragment BOOLs are
        // consumed, so its relative position cannot move the cursor.
    }
    if (mask & 0x4000) != 0 {
        candidates = apply_4000(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "4000", &candidates);
    }

    candidates
}

fn trace_inventory_stage(mask: u16, stage: &'static str, candidates: &[GenericInventoryCandidate]) {
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
            if let Some(count) =
                read_u32_le(bytes, candidate.cursor).and_then(|value| usize::try_from(value).ok())
            {
                if count == 0 {
                    if let Some(candidate) = candidate
                        .advanced(candidate.cursor + 4, candidate.bits.saturating_add(2))
                        .require_fragment_bit(candidate.bits.saturating_add(1), false)
                    {
                        next.push(candidate);
                    }
                } else if count <= usize::from(MAX_REASONABLE_VALUE_GROUPS) {
                    let Some(cells_end) = candidate
                        .cursor
                        .checked_add(4)
                        .and_then(|cursor| cursor.checked_add(count.checked_mul(2)?))
                    else {
                        continue;
                    };
                    if cells_end <= record_end {
                        // Diamond `sub_455940` and EE `sub_1407B4F70` both
                        // read two CNW BOOLs for mask 0x0200 before choosing
                        // the branch body.  When the second BOOL is false,
                        // the branch begins with a DWORD count.  If the first
                        // BOOL is false as well, each counted two-byte cell is
                        // followed by one CNW BOOL; if the first BOOL is true,
                        // the counted cells are byte-buffer only.  Model both
                        // decompiled cursor shapes here, then let the final
                        // fragment-bit proof select the exact one.
                        if let Some(candidate) = candidate
                            .advanced(cells_end, candidate.bits.saturating_add(2 + count))
                            .require_fragment_bit(candidate.bits, false)
                            .and_then(|candidate| {
                                candidate.require_fragment_bit(
                                    candidate.bits.saturating_sub(1 + count),
                                    false,
                                )
                            })
                        {
                            next.push(candidate);
                        }
                        if let Some(candidate) = candidate
                            .advanced(cells_end, candidate.bits.saturating_add(2))
                            .require_fragment_bit(candidate.bits, true)
                            .and_then(|candidate| {
                                candidate
                                    .require_fragment_bit(candidate.bits.saturating_sub(1), false)
                            })
                        {
                            next.push(candidate);
                        }
                    }
                }
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
                    .advanced(
                        masks_offset + byte_mask_count,
                        candidate.bits.saturating_add(2),
                    )
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
    // Diamond `sub_455940` (0045816C..0045823C) and EE `sub_1407B4F70`
    // (1407B7DDB..1407B7EEA) both read one CNW BOOL for mask 0x0800. The
    // false branch consumes no read-buffer bytes; the true branch consumes the
    // following twelve BYTEs before control reaches the later 0x4000 branch.
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
    super::super::object_ids::looks_like_legacy_live_object_id_value_with_compact_min(
        object_id,
        0x0000_1000,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory_0200_record(cells: &[(u8, u8)]) -> Vec<u8> {
        let mut record = vec![b'I'];
        record.extend_from_slice(&LEGACY_INVENTORY_CURRENT_PLAYER_OWNER.to_le_bytes());
        record.extend_from_slice(&0x0200u16.to_le_bytes());
        record.extend_from_slice(&(cells.len() as u32).to_le_bytes());
        for (x, y) in cells {
            record.push(*x);
            record.push(*y);
        }
        record
    }

    fn inventory_4000_record(rows: &[&[u8]]) -> Vec<u8> {
        let mut record = vec![b'I'];
        record.extend_from_slice(&LEGACY_INVENTORY_CURRENT_PLAYER_OWNER.to_le_bytes());
        record.extend_from_slice(&0x4000u16.to_le_bytes());
        record.extend_from_slice(&(rows.len() as u16).to_le_bytes());
        for row in rows {
            record.extend_from_slice(row);
        }
        record
    }

    fn inventory_mask_record(mask: u16, body: &[u8]) -> Vec<u8> {
        let mut record = vec![b'I'];
        record.extend_from_slice(&LEGACY_INVENTORY_CURRENT_PLAYER_OWNER.to_le_bytes());
        record.extend_from_slice(&mask.to_le_bytes());
        record.extend_from_slice(body);
        record
    }

    fn inventory_2000_body(first_ids: &[u32], second_ids: &[u32]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&(first_ids.len() as u32).to_le_bytes());
        for object_id in first_ids {
            body.extend_from_slice(&object_id.to_le_bytes());
        }
        body.extend_from_slice(&(second_ids.len() as u32).to_le_bytes());
        for object_id in second_ids {
            body.extend_from_slice(&object_id.to_le_bytes());
        }
        body
    }

    #[test]
    fn inventory_2000_feature25_counts_only_second_list_bools() {
        // Diamond sub_455940 and EE sub_1407B4F70 read mask 0x2000 as
        // DWORD removal count, removal OBJECTIDs, DWORD second-list count,
        // second-list OBJECTIDs, then three CNW BOOLs per second-list object.
        // Removal entries are byte-buffer only.
        let record = inventory_mask_record(
            0x2000,
            &inventory_2000_body(&[0x8000_0041, 0x8000_0042], &[0x8000_0043, 0x8000_0044]),
        );

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false, true, false, true, false],
            &mut bit_cursor,
        )
        .expect("0x2000 Feature-25 second list should own three BOOLs per object");
        assert_eq!(claim.fragment_bits, 6);
        assert_eq!(bit_cursor, 6);

        let mut missing_bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, false, true, false, true],
                &mut missing_bit_cursor,
            )
            .is_none(),
            "a byte-complete 0x2000 row is not exact when a second-list BOOL is missing"
        );
        assert_eq!(missing_bit_cursor, 0);
    }

    #[test]
    fn inventory_2000_prefix_hands_off_to_following_0800_selector() {
        // Mask order is 0x2000 before 0x0800. The Feature-25 block must stop
        // after its object lists, so the following 0x0800 branch owns the next
        // fragment BOOL and, when true, the twelve-byte read-buffer tail.
        let mut body = inventory_2000_body(&[], &[0x8000_0043]);
        body.extend_from_slice(&[
            0xAA, 0xBB, 0x10, 0x11, 0x12, 0x13, 0xCC, 0xDD, 0x20, 0x21, 0x22, 0x23,
        ]);
        let record = inventory_mask_record(0x2800, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false, true, true],
            &mut bit_cursor,
        )
        .expect("0x2000 should prefix-claim before 0x0800 consumes its selector");
        assert_eq!(claim.fragment_bits, 4);
        assert_eq!(bit_cursor, 4);

        let mut missing_0800_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, false, true],
                &mut missing_0800_cursor,
            )
            .is_none(),
            "the 0x0800 selector must not be counted as a fourth 0x2000 BOOL"
        );
        assert_eq!(missing_0800_cursor, 0);
    }

    #[test]
    fn inventory_2000_prefix_hands_off_to_following_4000_update_bool() {
        // A mask may continue from 0x2000 directly to 0x4000. The second-list
        // Feature-25 BOOLs must be counted before the 0x4000 `U` row owns its
        // own BOOL at the next fragment-bit position.
        let mut body = inventory_2000_body(&[], &[0x8000_0043]);
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[b'U', 0x33, 0x44, 0x55, 0x66, 0x77]);
        let record = inventory_mask_record(0x6000, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false, true, false],
            &mut bit_cursor,
        )
        .expect("0x4000 update BOOL should follow the three 0x2000 Feature-25 bits");
        assert_eq!(claim.fragment_bits, 4);
        assert_eq!(bit_cursor, 4);

        let mut missing_update_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, false, true],
                &mut missing_update_cursor,
            )
            .is_none(),
            "a following 0x4000 `U` row still needs its own BOOL after 0x2000"
        );
        assert_eq!(missing_update_cursor, 0);
    }

    #[test]
    fn inventory_0200_zero_count_dword_branch_allows_either_first_bool() {
        // Diamond sub_455940 and EE sub_1407B4F70 read two CNW BOOLs before
        // the 0x0200 branch body. The second BOOL selects the DWORD-count
        // path when false; with a zero count, the first BOOL owns state but no
        // additional per-cell BOOLs. Exact validation must therefore choose
        // the candidate whose BOOL requirements match the fragment stream
        // instead of always picking the first byte-valid cursor.
        let record = inventory_0200_record(&[]);

        for first_bool in [false, true] {
            let mut bit_cursor = 0usize;
            let claim = advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[first_bool, false],
                &mut bit_cursor,
            )
            .expect("0x0200 zero-count DWORD branch should accept either first BOOL");

            assert_eq!(claim.fragment_bits, 2);
            assert_eq!(bit_cursor, 2);
        }

        let mut bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, true],
                &mut bit_cursor
            )
            .is_none(),
            "the second 0x0200 BOOL still selects the byte-mask branch and must reject the DWORD cursor"
        );
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn inventory_0200_counted_cells_true_first_bool_consumes_no_cell_bools() {
        // For a nonzero DWORD-count body, the first 0x0200 BOOL chooses whether
        // each counted two-byte cell owns an additional fragment BOOL. The true
        // branch is byte-buffer only after the two branch BOOLs.
        let record = inventory_0200_record(&[(2, 2), (3, 2)]);

        let mut true_first_cursor = 0usize;
        let true_first_claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut true_first_cursor,
        )
        .expect("first BOOL true should choose the no-cell-BOOL 0x0200 cursor");
        assert_eq!(true_first_claim.fragment_bits, 2);
        assert_eq!(true_first_cursor, 2);

        let mut false_first_cursor = 0usize;
        let false_first_claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[false, false, true, false],
            &mut false_first_cursor,
        )
        .expect("first BOOL false should consume one BOOL for each counted cell");
        assert_eq!(false_first_claim.fragment_bits, 4);
        assert_eq!(false_first_cursor, 4);
    }

    #[test]
    fn inventory_4000_state_stream_counts_only_update_row_bools() {
        // EE sub_1407B4F70 reads the 0x4000 row stream as a WORD count, then
        // per-row opcodes. `S` rows consume only two read-buffer bytes; `U`
        // rows consume WORD, BOOL, BYTE, WORD in that order. Diamond sub_455940
        // follows the same row walk, so the proxy must advance one fragment bit
        // per `U` row and none for `S`.
        let record = inventory_4000_record(&[
            &[b'S', 0x11, 0x22],
            &[b'U', 0x33, 0x44, 0x55, 0x66, 0x77],
            &[b'U', 0x88, 0x99, 0xAA, 0xBB, 0xCC],
        ]);

        let candidate =
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x4000)
                .expect("0x4000 state stream should parse exact row cursor");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(
            candidate.bits, 2,
            "only the two `U` rows own inventory fragment BOOLs"
        );

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut bit_cursor,
        )
        .expect("0x4000 state stream should claim with two available BOOLs");
        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(bit_cursor, 2);

        let mut short_bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true],
                &mut short_bit_cursor,
            )
            .is_none(),
            "a byte-complete 0x4000 row stream is not exact when a `U` row BOOL is missing"
        );
        assert_eq!(short_bit_cursor, 0);
    }

    #[test]
    fn inventory_4000_state_stream_rejects_truncated_update_row() {
        let record = inventory_4000_record(&[&[b'U', 0x33, 0x44, 0x55, 0x66]]);

        assert!(
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x4000)
                .is_none(),
            "0x4000 `U` rows must own the final WORD after their BOOL/BYTE fields"
        );
    }

    #[test]
    fn inventory_0800_false_branch_consumes_selector_only() {
        // Diamond sub_455940 and EE sub_1407B4F70 both read one BOOL for mask
        // 0x0800. When that BOOL is false, the branch owns no read-buffer
        // bytes before the next mask bit is considered.
        let record = inventory_mask_record(0x0800, &[]);

        let candidate =
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0800)
                .expect("0x0800 false branch should parse at the current read cursor");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(candidate.bits, 1);

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[false], &mut bit_cursor)
                .expect("0x0800 false branch should consume exactly one selector BOOL");
        assert_eq!(claim.fragment_bits, 1);
        assert_eq!(bit_cursor, 1);

        let mut missing_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut missing_cursor)
                .is_none(),
            "a byte-complete 0x0800 false branch is not exact without its selector BOOL"
        );
        assert_eq!(missing_cursor, 0);
    }

    #[test]
    fn inventory_0800_true_branch_consumes_exact_twelve_byte_tail() {
        // With the 0x0800 selector true, both clients read exactly twelve
        // BYTEs. A false selector against the same bytes would leave those
        // bytes for the following mask branch, so exact validation must reject
        // it instead of choosing a semantically plausible shorter cursor.
        let record = inventory_mask_record(
            0x0800,
            &[
                0xAA, 0xBB, 0x10, 0x11, 0x12, 0x13, 0xCC, 0xDD, 0x20, 0x21, 0x22, 0x23,
            ],
        );

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[true], &mut bit_cursor)
                .expect("0x0800 true branch should claim the twelve read-buffer bytes");
        assert_eq!(claim.fragment_bits, 1);
        assert_eq!(bit_cursor, 1);

        let mut false_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[false],
                &mut false_cursor
            )
            .is_none(),
            "the false selector must not be accepted when the twelve-byte 0x0800 tail is present"
        );
        assert_eq!(false_cursor, 0);

        let truncated = inventory_mask_record(0x0800, &[0xAA; 11]);
        assert!(
            try_parse_generic_inventory_claim_with_branching(
                &truncated,
                0,
                truncated.len(),
                0x0800
            )
            .is_none(),
            "the true 0x0800 branch owns exactly twelve bytes, not a short prefix"
        );
    }

    #[test]
    fn inventory_0800_selector_precedes_following_4000_update_bool() {
        // The 0x0800 selector is consumed before the later 0x4000 row stream.
        // A `U` row in the 0x4000 stream therefore owns the second fragment
        // BOOL, never the first one.
        let mut body = vec![
            0xAA, 0xBB, 0x10, 0x11, 0x12, 0x13, 0xCC, 0xDD, 0x20, 0x21, 0x22, 0x23,
        ];
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[b'U', 0x33, 0x44, 0x55, 0x66, 0x77]);
        let record = inventory_mask_record(0x4800, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut bit_cursor,
        )
        .expect("0x0800 true selector should precede the 0x4000 `U` row BOOL");
        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(bit_cursor, 2);

        let mut missing_update_bool_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true],
                &mut missing_update_bool_cursor,
            )
            .is_none(),
            "a following 0x4000 `U` row still needs its own BOOL after the 0x0800 selector"
        );
        assert_eq!(missing_update_bool_cursor, 0);
    }

    #[test]
    fn inventory_1000_consumes_no_bytes_or_bools() {
        // The decompiled 0x1000 branch is local UI-state work in both clients;
        // it does not read from the message. It must therefore not add a
        // fragment bit between 0x0800 and 0x4000 or require any standalone
        // branch body.
        let record = inventory_mask_record(0x1000, &[]);
        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut bit_cursor)
                .expect("standalone 0x1000 should exact-claim without cursor movement");
        assert_eq!(claim.fragment_bits, 0);
        assert_eq!(bit_cursor, 0);

        let mut body = vec![
            0xAA, 0xBB, 0x10, 0x11, 0x12, 0x13, 0xCC, 0xDD, 0x20, 0x21, 0x22, 0x23,
        ];
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[b'U', 0x33, 0x44, 0x55, 0x66, 0x77]);
        let combined = inventory_mask_record(0x5800, &body);

        let mut combined_cursor = 0usize;
        let combined_claim = advance_verified_inventory_record(
            &combined,
            0,
            combined.len(),
            &[true, true],
            &mut combined_cursor,
        )
        .expect("0x1000 must not insert a phantom BOOL before 0x4000");
        assert_eq!(combined_claim.fragment_bits, 2);
        assert_eq!(combined_cursor, 2);
    }
}
