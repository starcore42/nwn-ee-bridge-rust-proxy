use super::*;

// Simple and rich inventory category block parsers.

pub(super) fn apply_simple_categories(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    // Diamond `sub_455940` and EE `sub_1407B4F70` legacy-build inventory
    // paths read three simple categories for mask 0x0010. Each category owns
    // WORD first-count + DWORD entries, then WORD second-count + DWORD entries.
    // This branch is read-buffer only and never advances the CNW BOOL cursor.
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
    // The legacy-build 0x0020 rich-category branch also walks three
    // categories. First-list entries are two read-buffer bytes. Second-list
    // entries are DWORD, BYTE, BYTE, BOOL, BOOL, BYTE: seven read-buffer bytes
    // plus two CNW fragment BOOLs per entry. Diamond's adjacent first-list
    // helper is an overflow check, not a BOOL read.
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

    fn zero_category_counts(body: &mut Vec<u8>, categories: usize) {
        for _ in 0..categories {
            body.extend_from_slice(&0u16.to_le_bytes());
            body.extend_from_slice(&0u16.to_le_bytes());
        }
    }

    fn simple_category_body() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&0x0102_0304u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&0x0506_0708u32.to_le_bytes());
        zero_category_counts(&mut body, 2);
        body
    }

    fn rich_category_body(second_entries: usize) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[0x11, 0x22]);
        body.extend_from_slice(&(second_entries as u16).to_le_bytes());
        for index in 0..second_entries {
            body.extend_from_slice(&(0x8000_0040u32 + index as u32).to_le_bytes());
            body.extend_from_slice(&[0x01, 0x02, 0x03]);
        }
        zero_category_counts(&mut body, 2);
        body
    }

    fn icon_list_body() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[0x2A, 0x34, 0x12]); // CHAR + WORD
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[0x78, 0x56, 0x9A]); // WORD + BYTE
        body
    }

    #[test]
    fn inventory_0010_simple_categories_are_read_buffer_only() {
        // EE's legacy-compatible 0x0010 path reads exactly three categories
        // with DWORD entries and no fragment BOOLs.
        let record = inventory_mask_record(0x0010, &simple_category_body());

        let candidate =
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0010)
                .expect("0x0010 simple categories should parse exactly");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(candidate.bits, 0);

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut bit_cursor)
                .expect("simple categories should claim without fragment BOOLs");
        assert_eq!(claim.fragment_bits, 0);
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn inventory_0020_rich_categories_count_only_second_list_bools() {
        // Rich first-list rows are two byte-width fields and do not consume a
        // fragment bit. Each second-list row consumes exactly two BOOLs after
        // its seven read-buffer bytes.
        let record = inventory_mask_record(0x0020, &rich_category_body(2));

        let candidate =
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0020)
                .expect("0x0020 rich categories should parse exactly");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(candidate.bits, 4);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false, false, true],
            &mut bit_cursor,
        )
        .expect("two rich second-list rows should own four fragment BOOLs");
        assert_eq!(claim.fragment_bits, 4);
        assert_eq!(bit_cursor, 4);

        let mut missing_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, false, false],
                &mut missing_cursor,
            )
            .is_none(),
            "byte-complete rich categories still require two BOOLs per second-list row"
        );
        assert_eq!(missing_cursor, 0);
    }

    #[test]
    fn inventory_0024_rich_categories_hand_off_to_icon_list_without_extra_bools() {
        // Mask order reaches 0x0020 before 0x0004. The icon/list delta is
        // byte-only, so the combined 0x0024 mask consumes exactly the rich
        // second-list BOOLs and no extra fragment bits for the icon rows.
        let mut body = rich_category_body(1);
        body.extend_from_slice(&icon_list_body());
        let record = inventory_mask_record(0x0024, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[false, true],
            &mut bit_cursor,
        )
        .expect("0x0024 should claim rich BOOLs then byte-only icon rows");
        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(bit_cursor, 2);

        let mut missing_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[false],
                &mut missing_cursor
            )
            .is_none(),
            "the following 0x0004 icon list cannot hide a missing rich-category BOOL"
        );
        assert_eq!(missing_cursor, 0);
    }

    #[test]
    fn inventory_0020_rejects_truncated_rich_second_list_row() {
        let mut body = rich_category_body(1);
        body.pop();
        let record = inventory_mask_record(0x0020, &body);

        assert!(
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0020)
                .is_none(),
            "rich second-list rows own all seven read-buffer bytes before their BOOLs"
        );
    }
}
