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
            candidate.normalized_fragment_requirements(),
        )
    });
    candidates.dedup_by_key(|candidate| {
        (
            candidate.cursor,
            candidate.bits,
            candidate.normalized_fragment_requirements(),
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
    if crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
    ) {
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
        candidates = apply_0001(bytes, &candidates, record_end);
        trace_inventory_stage(mask, "0001", &candidates);
    }
    if (mask & 0x0002) != 0 {
        // Diamond `sub_455940` / EE `sub_1407B4F70`: one DWORD, no BOOLs.
        candidates = advance_candidates(&candidates, record_end, 4, 0);
        trace_inventory_stage(mask, "0002", &candidates);
    }
    if (mask & 0x0008) != 0 {
        // Diamond `sub_455940` / EE `sub_1407B4F70`: one DWORD state field,
        // no BOOLs. Later mask branches own the next fragment bit.
        candidates = advance_candidates(&candidates, record_end, 4, 0);
        trace_inventory_stage(mask, "0008", &candidates);
    }
    if (mask & 0x8000) != 0 {
        // Diamond `sub_455940` / EE `sub_1407B4F70`: three INTs, no BOOLs.
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
        let allow_terminal_legacy_tail = (mask & (0x0800 | 0x4000)) == 0;
        candidates = apply_2000(bytes, &candidates, record_end, allow_terminal_legacy_tail);
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
    if !crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
    ) {
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
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    // Diamond sub_455940 (00455AAD..00455D80) and EE sub_1407B4F70
    // (1407B51ED..1407B559F) both read:
    //
    //   WORD, DWORD, INT, BOOL
    //
    // The false BOOL is the compact handoff. The true BOOL owns the
    // skill/string tail: WORD, one BYTE per Skills.2DA row, INT+CExoString,
    // INT+CExoString, then a build-gated scalar plus a final BYTE. Diamond/HG
    // legacy streams use a BYTE scalar; newer EE streams use a DWORD scalar.
    let mut next = Vec::with_capacity(candidates.len().saturating_mul(2));
    for candidate in advance_candidates(candidates, record_end, 10, 1) {
        let branch_bit = candidate.bits.saturating_sub(1);
        if let Some(compact) = candidate.require_fragment_bit(branch_bit, false) {
            next.push(compact);
        }
        if let Some(extended_candidate) = candidate.require_fragment_bit(branch_bit, true) {
            next.extend(advance_0001_extended_tails(
                bytes,
                extended_candidate,
                record_end,
            ));
        }
    }
    next
}

const INVENTORY_0001_EXTENDED_SKILL_BYTES: usize = 28;
const INVENTORY_0001_EXTENDED_LEGACY_TAIL_BYTES: usize = 2;
const INVENTORY_0001_EXTENDED_EE_TAIL_BYTES: usize = 5;
const MAX_REASONABLE_INVENTORY_0001_STRING_BYTES: usize = 4096;

fn advance_0001_extended_tails(
    bytes: &[u8],
    candidate: GenericInventoryCandidate,
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut cursor = candidate.cursor;
    let Some(next_cursor) = cursor.checked_add(2) else {
        return Vec::new();
    }; // WORD at 00455B99 / 1407B52FA.
    cursor = next_cursor;
    let Some(next_cursor) = cursor.checked_add(INVENTORY_0001_EXTENDED_SKILL_BYTES) else {
        return Vec::new();
    };
    cursor = next_cursor;
    let Some(next_cursor) = cursor.checked_add(4) else {
        return Vec::new();
    }; // INT before first CExoString.
    cursor = next_cursor;
    let Some(next_cursor) = advance_inventory_cexo_string(bytes, cursor, record_end) else {
        return Vec::new();
    };
    cursor = next_cursor;
    let Some(next_cursor) = cursor.checked_add(4) else {
        return Vec::new();
    }; // INT before second CExoString.
    cursor = next_cursor;
    let Some(cursor) = advance_inventory_cexo_string(bytes, cursor, record_end) else {
        return Vec::new();
    };

    [
        INVENTORY_0001_EXTENDED_LEGACY_TAIL_BYTES,
        INVENTORY_0001_EXTENDED_EE_TAIL_BYTES,
    ]
    .into_iter()
    .filter_map(|tail_bytes| {
        let cursor = cursor.checked_add(tail_bytes)?;
        (cursor <= record_end).then_some(candidate.advanced(cursor, candidate.bits))
    })
    .collect()
}

fn advance_inventory_cexo_string(bytes: &[u8], cursor: usize, record_end: usize) -> Option<usize> {
    if cursor > record_end || record_end - cursor < 4 {
        return None;
    }
    let len = usize::try_from(read_u32_le(bytes, cursor)?).ok()?;
    if len > MAX_REASONABLE_INVENTORY_0001_STRING_BYTES {
        return None;
    }
    cursor
        .checked_add(4)?
        .checked_add(len)
        .filter(|end| *end <= record_end)
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

    fn inventory_0001_compact_body() -> Vec<u8> {
        vec![0x95, 0x00, 0xD4, 0xD9, 0xE0, 0x05, 0xEB, 0x0A, 0x00, 0x00]
    }

    fn inventory_0001_extended_body_with_tail(tail: &[u8]) -> Vec<u8> {
        let mut body = inventory_0001_compact_body();
        body.extend_from_slice(&0x0007u16.to_le_bytes());
        body.extend(0u8..INVENTORY_0001_EXTENDED_SKILL_BYTES as u8);
        body.extend_from_slice(&0x0102_0304u32.to_le_bytes());
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"left");
        body.extend_from_slice(&0x0506_0708u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(tail);
        body
    }

    fn inventory_0001_extended_body() -> Vec<u8> {
        inventory_0001_extended_body_with_tail(&[0x09, 0x0A])
    }

    fn inventory_0001_extended_ee_body() -> Vec<u8> {
        inventory_0001_extended_body_with_tail(&[0x09, 0x0B, 0x0C, 0x0D, 0x0A])
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

    fn rich_category_body(second_entries: usize) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&(second_entries as u16).to_le_bytes());
        for index in 0..second_entries {
            body.extend_from_slice(&(0x8000_0040u32 + index as u32).to_le_bytes());
            body.extend_from_slice(&[0x01, 0x02, 0x03]);
        }
        for _ in 0..2 {
            body.extend_from_slice(&0u16.to_le_bytes());
            body.extend_from_slice(&0u16.to_le_bytes());
        }
        body
    }

    #[test]
    fn inventory_0001_compact_branch_requires_false_bool() {
        // Diamond sub_455940 (00455AAD..00455D80) and EE sub_1407B4F70
        // (1407B51ED..1407B559F) both read 0x0001 as SHORT, DWORD, INT, BOOL.
        // The false BOOL hands off after the compact ten-byte shape; a true
        // BOOL must own the extended read-buffer tail before any later mask.
        let record = inventory_mask_record(0x0001, &inventory_0001_compact_body());

        let mut false_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[false],
            &mut false_cursor,
        )
        .expect("0x0001 false BOOL should exact-claim the compact branch");
        assert_eq!(claim.fragment_bits, 1);
        assert_eq!(false_cursor, 1);

        let mut missing_tail_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true],
                &mut missing_tail_cursor,
            )
            .is_none(),
            "the true 0x0001 BOOL must not be accepted without the extended tail"
        );
        assert_eq!(missing_tail_cursor, 0);
    }

    #[test]
    fn inventory_0001_extended_branch_owns_skill_and_string_tail() {
        // After the true selector, both decompiles read WORD, one skill BYTE per
        // Skills.2DA row, two INT+CExoString pairs, then the legacy-build BYTE
        // and final BYTE. The tail is byte-only; later mask branches own the
        // next CNW fragment bit.
        let record = inventory_mask_record(0x0001, &inventory_0001_extended_body());

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[true], &mut bit_cursor)
                .expect("true 0x0001 BOOL should exact-claim the extended tail");
        assert_eq!(claim.fragment_bits, 1);
        assert_eq!(bit_cursor, 1);

        let mut wrong_bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[false],
                &mut wrong_bit_cursor,
            )
            .is_none(),
            "false 0x0001 selector must not reinterpret an extended tail"
        );
        assert_eq!(wrong_bit_cursor, 0);
    }

    #[test]
    fn inventory_0001_extended_branch_accepts_ee_dword_tail_scalar() {
        // The newer EE path keeps the same 0x0001 branch BOOL and string tail,
        // but widens the first post-string scalar from BYTE to DWORD before the
        // final BYTE. The widened bytes are read-buffer data only; they do not
        // add a fragment BOOL before the next mask branch.
        let record = inventory_mask_record(0x0001, &inventory_0001_extended_ee_body());

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[true], &mut bit_cursor)
                .expect("true 0x0001 BOOL should exact-claim the EE DWORD tail");
        assert_eq!(claim.fragment_bits, 1);
        assert_eq!(bit_cursor, 1);

        let mut wrong_bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[false],
                &mut wrong_bit_cursor,
            )
            .is_none(),
            "false 0x0001 selector must not reinterpret the EE DWORD tail"
        );
        assert_eq!(wrong_bit_cursor, 0);
    }

    #[test]
    fn inventory_0001_extended_branch_hands_off_to_following_0400() {
        let mut body = inventory_0001_extended_body();
        body.extend_from_slice(&[
            0x01, 0x1E, // 0x0400 clear-count and clear slot
            0x01, 0x6B, // 0x0400 set-count and set slot
        ]);
        let record = inventory_mask_record(0x0401, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut bit_cursor,
        )
        .expect("true 0x0001 extended tail should hand off to 0x0400 set-slot BOOL");

        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(bit_cursor, 2);
    }

    #[test]
    fn inventory_0001_ee_dword_extended_branch_hands_off_to_following_0400() {
        let mut body = inventory_0001_extended_body_with_tail(&[0x09, 0xFF, 0xFF, 0xFF, 0x0A]);
        body.extend_from_slice(&[
            0x01, 0x1E, // 0x0400 clear-count and clear slot
            0x01, 0x6B, // 0x0400 set-count and set slot
        ]);
        let record = inventory_mask_record(0x0401, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut bit_cursor,
        )
        .expect("EE DWORD 0x0001 tail should hand off to 0x0400 after the full widened scalar");

        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(bit_cursor, 2);
    }

    #[test]
    fn inventory_0001_extended_branch_rejects_truncated_cexo_string() {
        let mut body = inventory_0001_extended_body();
        body.pop();
        let record = inventory_mask_record(0x0001, &body);

        let mut bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(&record, 0, record.len(), &[true], &mut bit_cursor)
                .is_none(),
            "extended 0x0001 cannot claim a truncated final byte/string tail"
        );
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn inventory_fixed_scalar_masks_are_byte_only() {
        // Verified EE/Diamond inventory order reads 0x0002 as one DWORD,
        // 0x0008 as one DWORD, and 0x8000 as three INTs. None of these fixed
        // scalar branches consumes a fragment BOOL.
        let record = inventory_mask_record(
            0x800A,
            &[
                0x11, 0x22, 0x33, 0x44, // 0x0002 DWORD
                0x55, 0x66, 0x77, 0x88, // 0x0008 DWORD
                0x01, 0x00, 0x00, 0x00, // 0x8000 INT 0
                0x02, 0x00, 0x00, 0x00, // 0x8000 INT 1
                0x03, 0x00, 0x00, 0x00, // 0x8000 INT 2
            ],
        );

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut bit_cursor)
                .expect("fixed scalar inventory masks should claim without fragment BOOLs");
        assert_eq!(claim.fragment_bits, 0);
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn inventory_fixed_scalar_masks_hand_off_to_following_0200_selector() {
        // Since 0x0002/0x0008/0x8000 are byte-only, the following 0x0200 branch
        // owns the first two fragment BOOLs. The second BOOL must be false for
        // the DWORD zero-count path.
        let mut body = vec![
            0x11, 0x22, 0x33, 0x44, // 0x0002 DWORD
            0x55, 0x66, 0x77, 0x88, // 0x0008 DWORD
            0x01, 0x00, 0x00, 0x00, // 0x8000 INT 0
            0x02, 0x00, 0x00, 0x00, // 0x8000 INT 1
            0x03, 0x00, 0x00, 0x00, // 0x8000 INT 2
        ];
        body.extend_from_slice(&0u32.to_le_bytes());
        let record = inventory_mask_record(0x820A, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut bit_cursor,
        )
        .expect("0x0200 selector BOOLs should follow fixed scalar bytes");
        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(bit_cursor, 2);

        let mut shifted_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, true],
                &mut shifted_cursor,
            )
            .is_none(),
            "fixed scalar branches must not shift the 0x0200 layout selector"
        );
        assert_eq!(shifted_cursor, 0);
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
    fn inventory_2a24_high_fragment_index_selectors_are_preserved() {
        // HG Docks gameplay produced an I/0x2A24 self-inventory update with
        // hundreds of rich-category BOOLs before the later 0x0200 and 0x0800
        // selectors. Those selector requirements are still exact even when
        // their bit positions are beyond a single machine-sized bitset.
        let rich_second_entries = 65usize;
        let mut body = rich_category_body(rich_second_entries);
        body.extend_from_slice(&0u16.to_le_bytes()); // 0x0004 first icon count
        body.extend_from_slice(&0u16.to_le_bytes()); // 0x0004 second icon count
        body.extend_from_slice(&0u32.to_le_bytes()); // 0x0200 DWORD zero-count branch
        body.extend_from_slice(&inventory_2000_body(&[], &[0x8000_0043]));
        body.extend_from_slice(&[
            0xAA, 0xBB, 0x10, 0x11, 0x12, 0x13, 0xCC, 0xDD, 0x20, 0x21, 0x22, 0x23,
        ]);
        let record = inventory_mask_record(0x2A24, &body);

        let rich_bits = rich_second_entries * 2;
        let expected_bits = rich_bits + 2 + 3 + 1;
        let mut fragment_bits = vec![false; expected_bits];
        fragment_bits[rich_bits + 2 + 3] = true; // 0x0800 true-tail selector

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &fragment_bits,
            &mut bit_cursor,
        )
        .expect("high-index 0x0200/0x0800 selectors should still exact-claim");

        assert_eq!(claim.fragment_bits, expected_bits);
        assert_eq!(bit_cursor, expected_bits);
    }

    #[test]
    fn inventory_2000_sentinel_tail_is_terminal_only_before_0800() {
        // The legacy zero-first/sentinel tail compatibility shape is a
        // standalone 0x2000 repair source. When the mask also carries 0x0800,
        // Diamond sub_455940 and EE sub_1407B4F70 hand off to the 0x0800 BOOL
        // branch instead of letting 0x2000 swallow tail-shaped read bytes.
        let mut body = Vec::new();
        body.extend_from_slice(&0u32.to_le_bytes()); // 0x2000 first_count
        body.extend_from_slice(&0x8000_0043u32.to_le_bytes());
        body.extend_from_slice(&0x8000_0044u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes()); // sentinel-looking tail
        let record = inventory_mask_record(0x2800, &body);

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
            "0x2000 must not reinterpret bytes needed by the following 0x0800 branch"
        );
        assert_eq!(false_cursor, 0);
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
