use super::*;

// Equipment delta masks, including exact set/clear cursor and fragment-bit counts.

pub(super) fn apply_0400(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    // Diamond `sub_455940` (00457182..004572D2) and EE `sub_1407B4F70`
    // (1407B6D51..1407B6FA9) share the legacy-build 0x0400 shape:
    // BYTE clear count + clear slots, then BYTE set count + set slots, with
    // one CNW BOOL read after each set slot. Those set BOOLs are owned before
    // any later inventory mask branch consumes its own BOOLs.
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
        next.push(candidate.advanced(cursor, candidate.bits.saturating_add(second_count)));
    }
    next
}

pub(super) fn try_parse_inventory_0400(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory_delta_record(mask: u16, clear_slots: &[u8], set_slots: &[u8]) -> Vec<u8> {
        let mut record = vec![b'I'];
        record.extend_from_slice(&LEGACY_INVENTORY_CURRENT_PLAYER_OWNER.to_le_bytes());
        record.extend_from_slice(&mask.to_le_bytes());
        record.push(clear_slots.len() as u8);
        record.extend_from_slice(clear_slots);
        record.push(set_slots.len() as u8);
        record.extend_from_slice(set_slots);
        record
    }

    #[test]
    fn inventory_0400_equipment_delta_consumes_only_set_slot_bools() {
        let record = inventory_delta_record(0x0400, &[0x01, 0x02, 0x03], &[0x10, 0x11]);

        let candidate =
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0400)
                .expect("0x0400 equipment delta should parse exact cursor");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(
            candidate.bits, 2,
            "clear slots are read-buffer only; each set slot owns one BOOL"
        );

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut bit_cursor,
        )
        .expect("two set-slot BOOLs should satisfy the exact inventory claim");
        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(bit_cursor, 2);

        let mut short_bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true],
                &mut short_bit_cursor
            )
            .is_none(),
            "a byte-complete 0x0400 delta is not exact when a set-slot BOOL is missing"
        );
        assert_eq!(short_bit_cursor, 0);
    }

    #[test]
    fn inventory_0400_equipment_delta_bools_precede_0200_branch_bools() {
        // The 0x0400 set-slot BOOLs are consumed before the following 0x0200
        // branch BOOLs. The fourth bit below is therefore the 0x0200
        // layout selector and must be false for the DWORD zero-count branch.
        let mut record = inventory_delta_record(0x0600, &[0x01], &[0x10, 0x11]);
        record.extend_from_slice(&0u32.to_le_bytes());

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false, true, false],
            &mut bit_cursor,
        )
        .expect("0x0400 set-slot BOOLs should precede the 0x0200 branch selector");
        assert_eq!(claim.fragment_bits, 4);
        assert_eq!(bit_cursor, 4);

        let mut shifted_bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, false, true, true],
                &mut shifted_bit_cursor,
            )
            .is_none(),
            "flipping the post-0x0400 0x0200 selector must reject the DWORD branch"
        );
        assert_eq!(shifted_bit_cursor, 0);
    }

    #[test]
    fn inventory_0400_equipment_delta_rejects_truncated_lists() {
        let mut truncated_clear = vec![b'I'];
        truncated_clear.extend_from_slice(&LEGACY_INVENTORY_CURRENT_PLAYER_OWNER.to_le_bytes());
        truncated_clear.extend_from_slice(&0x0400u16.to_le_bytes());
        truncated_clear.extend_from_slice(&[2, 0x01]);

        assert!(
            try_parse_generic_inventory_claim_with_branching(
                &truncated_clear,
                0,
                truncated_clear.len(),
                0x0400,
            )
            .is_none(),
            "0x0400 clear-count bytes own exactly that many clear-slot bytes"
        );

        let mut truncated_set = inventory_delta_record(0x0400, &[0x01], &[0x10]);
        let set_count_offset = 7 + 1 + 1;
        truncated_set[set_count_offset] = 2;

        assert!(
            try_parse_generic_inventory_claim_with_branching(
                &truncated_set,
                0,
                truncated_set.len(),
                0x0400,
            )
            .is_none(),
            "0x0400 set-count bytes own exactly that many set-slot bytes"
        );
    }
}
