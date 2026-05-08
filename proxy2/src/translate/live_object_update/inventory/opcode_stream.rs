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
        next.push(GenericInventoryCandidate {
            cursor,
            bits: candidate.bits,
        });
    }
    next
}

fn advance_inventory_0100_opcode_stream(
    bytes: &[u8],
    mut cursor: usize,
    record_end: usize,
) -> Option<usize> {
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
