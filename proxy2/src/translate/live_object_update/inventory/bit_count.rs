use super::*;

// Ten-bit value-group parser and fragment-neutral bit-count helpers.

pub(super) fn apply_ten_bit_groups(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    // Diamond `sub_455940` and EE `sub_1407B4F70` both use this byte-only
    // grouped ten-bit value shape for inventory masks 0x0080 and 0x0040:
    // BYTE group count, then each group reads one BYTE selector, a 10-bit
    // WORD mask, and one BYTE value for each set bit. No ReadBOOL call occurs
    // in either branch, so following mask branches own the next fragment bit.
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, _)) =
            advance_ten_bit_value_groups(bytes, candidate.cursor, record_end)
        {
            next.push(candidate.advanced(cursor, candidate.bits));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory_mask_record(mask: u16, body: &[u8]) -> Vec<u8> {
        let mut record = vec![b'I'];
        record.extend_from_slice(&LEGACY_INVENTORY_CURRENT_PLAYER_OWNER.to_le_bytes());
        record.extend_from_slice(&mask.to_le_bytes());
        record.extend_from_slice(body);
        record
    }

    fn ten_bit_group_body() -> Vec<u8> {
        vec![
            2, // two groups
            0x10, 0x05, 0x00, 0xAA, 0xBB, // mask bits 0 and 2 -> two values
            0x20, 0x00, 0x02, 0xCC, // mask bit 9 -> one value
        ]
    }

    #[test]
    fn inventory_0080_ten_bit_groups_consume_no_fragment_bools() {
        // Verified EE inventory read order: mask 0x0080 reads grouped ten-bit
        // byte values and does not call ReadBOOL. The group selector and values
        // are read-buffer bytes only.
        let record = inventory_mask_record(0x0080, &ten_bit_group_body());

        let candidate =
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0080)
                .expect("0x0080 grouped ten-bit values should parse exactly");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(candidate.bits, 0);

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut bit_cursor)
                .expect("0x0080 should exact-claim without fragment BOOLs");
        assert_eq!(claim.fragment_bits, 0);
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn inventory_0040_ten_bit_groups_hand_off_to_following_state_stream_bool() {
        // 0x0040 is the same byte-only grouped ten-bit shape as 0x0080. A
        // following 0x4000 `U` row therefore owns the first fragment BOOL.
        let mut body = ten_bit_group_body();
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[b'U', 0x33, 0x44, 0x55, 0x66, 0x77]);
        let record = inventory_mask_record(0x4040, &body);

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[true], &mut bit_cursor)
                .expect("0x4000 `U` BOOL should follow the byte-only 0x0040 group");
        assert_eq!(claim.fragment_bits, 1);
        assert_eq!(bit_cursor, 1);

        let mut missing_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut missing_cursor)
                .is_none(),
            "0x0040 must not hide the following 0x4000 `U` row BOOL"
        );
        assert_eq!(missing_cursor, 0);
    }

    #[test]
    fn inventory_ten_bit_group_rejects_missing_value_bytes() {
        let record = inventory_mask_record(
            0x0040,
            &[
                1, // one group
                0x10, 0xFF, 0x03, // all ten mask bits set
                0xAA, 0xBB, 0xCC, // seven values missing
            ],
        );

        assert!(
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0040)
                .is_none(),
            "ten-bit group masks own one value byte for each set mask bit"
        );
    }
}
