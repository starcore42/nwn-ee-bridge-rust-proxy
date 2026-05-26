use super::*;

// Inventory opcode stream parser for mask 0x0100.

pub(super) fn apply_0100(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let Some(cursor) =
            advance_inventory_0100_opcode_stream(bytes, candidate.cursor, record_end)
        else {
            continue;
        };
        next.push(candidate.advanced(cursor, candidate.bits));
    }
    next
}

fn advance_inventory_0100_opcode_stream(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<usize> {
    // Diamond `sub_455940` (00457939..00457EB3) and EE `sub_1407B4F70`
    // (1407B7686..1407B79BA) both read mask 0x0100 as a byte-only GUI
    // opcode stream: BYTE row count, one CHAR opcode per row, then row-local
    // WORD/OBJECTID/FLOAT/DWORD bodies. No row in this stream calls
    // ReadBOOL; any following mask branch owns the next CNW fragment bit.
    if cursor >= record_end {
        return None;
    }
    let entry_count = bytes[cursor];
    cursor += 1;
    for _ in 0..entry_count {
        if cursor >= record_end {
            return None;
        }
        let opcode = bytes[cursor];
        cursor += 1;
        if opcode == b'D' {
            if cursor > record_end || 4 > record_end - cursor {
                return None;
            }
            cursor += 4;
        } else if opcode == b'S' || opcode == b'U' {
            if cursor > record_end || 8 > record_end - cursor {
                return None;
            }
            cursor += 8;
        } else if opcode == b'A' {
            if cursor > record_end || 4 > record_end - cursor {
                return None;
            }
            let item_type = read_u16_le(bytes, cursor)?;
            cursor += 4;
            if item_type != 0 && item_type != 2 {
                if cursor > record_end || 4 > record_end - cursor {
                    return None;
                }
                cursor += 4;
            }
            if matches!(item_type, 0 | 2 | 4 | 12 | 19) {
                if cursor > record_end || 12 > record_end - cursor {
                    return None;
                }
                cursor += 12;
            }
            if item_type == 4 || item_type == 19 {
                if cursor > record_end || 4 > record_end - cursor {
                    return None;
                }
                cursor += 4;
            }
        }
    }
    Some(cursor)
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

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_a_row(bytes: &mut Vec<u8>, item_type: u16) {
        bytes.push(b'A');
        push_u16(bytes, item_type);
        push_u16(bytes, 0x1234);
        if item_type != 0 && item_type != 2 {
            push_u32(bytes, 0x8000_0042);
        }
        if matches!(item_type, 0 | 2 | 4 | 12 | 19) {
            push_u32(bytes, 0x3F80_0000);
            push_u32(bytes, 0x4000_0000);
            push_u32(bytes, 0x4040_0000);
        }
        if item_type == 4 || item_type == 19 {
            push_u32(bytes, 0x0000_00AA);
        }
    }

    #[test]
    fn inventory_0100_opcode_stream_consumes_no_fragment_bools() {
        let record = inventory_mask_record(
            0x0100,
            &[
                4, b'D', 0x11, 0x22, 0x33, 0x44, b'S', 0x55, 0x66, 0x77, 0x88, 0x42, 0x00, 0x00,
                0x80, b'U', 0x99, 0xAA, 0xBB, 0xCC, 0x43, 0x00, 0x00, 0x80, b'Z',
            ],
        );

        let mut bit_cursor = 0usize;
        let claim =
            advance_verified_inventory_record(&record, 0, record.len(), &[], &mut bit_cursor)
                .expect("0x0100 byte-only opcode stream should claim without BOOLs");
        assert_eq!(claim.fragment_bits, 0);
        assert_eq!(bit_cursor, 0);
    }

    #[test]
    fn inventory_0100_a_rows_follow_decompiled_item_type_widths() {
        let mut body = vec![6];
        for item_type in [0, 1, 2, 4, 12, 19] {
            push_a_row(&mut body, item_type);
        }
        let record = inventory_mask_record(0x0100, &body);

        let cursor = advance_inventory_0100_opcode_stream(&record, 7, record.len())
            .expect("all decompiled A-row item-type widths should parse");
        assert_eq!(cursor, record.len());

        let mut missing_spell_or_feat_dword = body.clone();
        missing_spell_or_feat_dword.pop();
        let truncated = inventory_mask_record(0x0100, &missing_spell_or_feat_dword);
        assert!(
            advance_inventory_0100_opcode_stream(&truncated, 7, truncated.len()).is_none(),
            "item types 4 and 19 own the trailing DWORD proved by both readers"
        );
    }

    #[test]
    fn inventory_0100_handoff_leaves_following_2000_bools_aligned() {
        let mut body = vec![1];
        push_a_row(&mut body, 4);
        push_u32(&mut body, 0); // 0x2000 first-list count.
        push_u32(&mut body, 1); // 0x2000 second-list count.
        push_u32(&mut body, 0x8000_0042);
        let record = inventory_mask_record(0x2100, &body);

        let mut bit_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[true, false, true],
            &mut bit_cursor,
        )
        .expect("0x0100 should hand off directly to 0x2000 Feature-25 BOOLs");
        assert_eq!(claim.fragment_bits, 3);
        assert_eq!(bit_cursor, 3);

        let mut short_bit_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, false],
                &mut short_bit_cursor,
            )
            .is_none(),
            "missing 0x2000 BOOLs must not be hidden by phantom 0x0100 bits"
        );
        assert_eq!(short_bit_cursor, 0);
    }
}
