use super::*;

// Legacy inventory icon-list parser.

pub(super) fn apply_legacy_icon_list(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    // EE `sub_1407B4F70` reaches mask 0x0004 after 0x0400 at
    // `loc_1407B6FA9`; Diamond `sub_455940` follows the same inventory mask
    // order. The branch reads WORD count + `CHAR, WORD` tuples, then WORD
    // count + `WORD, BYTE` tuples. It is entirely read-buffer data and does
    // not consume CNW fragment BOOLs before later 0x0200/0x0100 branches.
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if let Some((cursor, _, _, fragment_bits)) =
            advance_legacy_icon_list_block(bytes, candidate.cursor, record_end)
        {
            next.push(
                candidate.advanced(
                    cursor,
                    candidate
                        .bits
                        .saturating_add(usize::try_from(fragment_bits).unwrap_or(usize::MAX)),
                ),
            );
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

    fn icon_list_body() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&[0x2A, 0x34, 0x12]);
        body.extend_from_slice(&[0x2B, 0x78, 0x56]);
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[0xBC, 0x9A, 0xDE]);
        body
    }

    #[test]
    fn inventory_0004_icon_list_consumes_no_fragment_bools() {
        // 0x0004 icon rows are two counted read-buffer lists. Neither list
        // calls ReadBOOL, so a standalone icon delta owns no fragment bits.
        let record = inventory_mask_record(0x0004, &icon_list_body());

        let candidate =
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0004)
                .expect("0x0004 icon list should parse exact cursor");
        assert_eq!(candidate.cursor, record.len());
        assert_eq!(candidate.bits, 0);

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut bit_cursor)
                .expect("0x0004 should exact-claim without fragment BOOLs");
        assert_eq!(claim.fragment_bits, 0);
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn inventory_0004_icon_list_hands_off_to_following_0200_bools() {
        // The fixed mask order reads 0x0004 before 0x0200. The two 0x0200
        // selector BOOLs therefore start immediately after the icon-list bytes.
        let mut body = icon_list_body();
        body.extend_from_slice(&0u32.to_le_bytes());
        let record = inventory_mask_record(0x0204, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false],
            &mut bit_cursor,
        )
        .expect("0x0200 selector BOOLs should follow the byte-only icon list");
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
            "the second 0x0200 BOOL still selects the branch after 0x0004"
        );
        assert_eq!(shifted_cursor, 0);
    }

    #[test]
    fn inventory_0004_icon_list_rejects_truncated_tuple() {
        let mut body = icon_list_body();
        body.pop();
        let record = inventory_mask_record(0x0004, &body);

        assert!(
            try_parse_generic_inventory_claim_with_branching(&record, 0, record.len(), 0x0004)
                .is_none(),
            "0x0004 count fields own three bytes per tuple in both lists"
        );
    }
}
