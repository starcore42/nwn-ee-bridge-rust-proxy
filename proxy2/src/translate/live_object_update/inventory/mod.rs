//! Inventory-family live-object update policy.
//!
//! Inventory and GUI item-create submessages can own fragment BOOLs. The
//! decompile-backed rule is intentionally stricter than "the next byte looks
//! like a live-object opcode": legacy `I` records contain opcode-like row bytes,
//! so a boundary inside an inventory record is safe only after the inventory
//! family validates the exact record shape and fragment-bit count.

use super::{read_u16_le, read_u32_le};

const LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK: u16 = 0x0010;
const LEGACY_INVENTORY_RICH_CATEGORY_MASK: u16 = 0x0020;
const LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK: u16 = 0x0004;
const LEGACY_INVENTORY_CATEGORY_COUNT: usize = 3;
const MAX_REASONABLE_CATEGORY_ENTRIES: u16 = 4096;
const MAX_REASONABLE_VALUE_GROUPS: u8 = 64;
const MAX_REASONABLE_FEATURE25_OBJECTS: u32 = 128;
const GENERIC_INVENTORY_PARSE_MASK: u16 = 0x0001
    | 0x0002
    | LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK
    | 0x0008
    | LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK
    | LEGACY_INVENTORY_RICH_CATEGORY_MASK
    | 0x0040
    | 0x0080
    | 0x0100
    | 0x0200
    | 0x0400
    | 0x0800
    | 0x1000
    | 0x2000
    | 0x4000
    | 0x8000;

#[derive(Debug, Clone, Copy)]
pub(super) struct InventoryRecordClaim {
    pub fragment_bits: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct InventoryRecordPrefixClaim {
    pub read_end: usize,
    pub fragment_bits: usize,
    pub interleaved_fragment_tail_allowed: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct InventoryRecordRewrite {
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
}

#[derive(Debug, Clone, Copy)]
struct GenericInventoryCandidate {
    cursor: usize,
    bits: usize,
    required_true_bits: u128,
    required_false_bits: u128,
}

impl GenericInventoryCandidate {
    fn new(cursor: usize, bits: usize) -> Self {
        Self {
            cursor,
            bits,
            required_true_bits: 0,
            required_false_bits: 0,
        }
    }

    fn advanced(self, cursor: usize, bits: usize) -> Self {
        Self { cursor, bits, ..self }
    }

    fn require_fragment_bit(mut self, bit_index: usize, value: bool) -> Option<Self> {
        if bit_index >= u128::BITS as usize {
            return None;
        }
        let bit = 1u128 << bit_index;
        if value {
            if (self.required_false_bits & bit) != 0 {
                return None;
            }
            self.required_true_bits |= bit;
        } else {
            if (self.required_true_bits & bit) != 0 {
                return None;
            }
            self.required_false_bits |= bit;
        }
        Some(self)
    }

    fn fragment_requirements_match(self, fragment_bits: &[bool], bit_cursor: usize) -> bool {
        for bit_index in 0..u128::BITS as usize {
            let bit = 1u128 << bit_index;
            let expected = if (self.required_true_bits & bit) != 0 {
                Some(true)
            } else if (self.required_false_bits & bit) != 0 {
                Some(false)
            } else {
                None
            };
            if let Some(expected) = expected {
                if fragment_bits.get(bit_cursor.saturating_add(bit_index)) != Some(&expected) {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Feature25Shape {
    second_count: u32,
    block_end: usize,
    missing_second_count: bool,
}

pub(super) fn owns_fragment_tail(opcode: u8) -> bool {
    matches!(opcode, b'I' | b'G')
}

pub(super) fn advance_verified_inventory_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> Option<InventoryRecordClaim> {
    if let Some(shape) =
        try_parse_inventory_2e00_or_2e01_gui_quickbar_link_shape(bytes, offset, record_end)
    {
        if shape.fragment_bits > fragment_bits.len().saturating_sub(*bit_cursor) {
            return None;
        }
        // The 0x2E01 sibling prepends the same 0x0001 compact inventory delta
        // branch that Diamond `sub_455940` and EE `sub_1407B4F70` both read as
        // WORD, DWORD, INT, BOOL. This shape is exact only for the compact
        // branch; if the BOOL is true, the extended row/string branch must be
        // modelled separately instead of accepting this cursor.
        if let Some(bit_index) = shape.branch_0001_extended_bit {
            if fragment_bits.get(*bit_cursor + bit_index) != Some(&false) {
                return None;
            }
        }
        // Diamond `sub_455940` and EE `sub_1407B4F70` both drive the 0x0200
        // inventory branch from two CNW BOOLs. The first BOOL can choose either
        // clear path, but the second BOOL must be false for the captured
        // zero-count DWORD shape; otherwise the reader switches to the byte-mask
        // path and the following Feature-25 cursor is not valid.
        if fragment_bits.get(*bit_cursor + shape.branch_0200_second_bit) != Some(&false) {
            return None;
        }
        // The same readers gate the 0x0800 appearance/status byte block behind
        // one CNW BOOL. Most exact quickbar-link shapes require the bit to be
        // true because they own the 12-byte read-buffer block. The focused
        // Sooty Crow large-equipment sibling below records its expected value
        // explicitly so this verifier remains shape-owned, not generic.
        if fragment_bits.get(*bit_cursor + shape.branch_0800_present_bit)
            != Some(&shape.branch_0800_present_value)
        {
            return None;
        }
        *bit_cursor = bit_cursor.saturating_add(shape.fragment_bits);
        return Some(InventoryRecordClaim {
            fragment_bits: shape.fragment_bits,
        });
    }

    let candidate = try_get_legacy_live_inventory_claim_candidate(bytes, offset, record_end)?;
    if candidate.bits > fragment_bits.len().saturating_sub(*bit_cursor)
        || !candidate.fragment_requirements_match(fragment_bits, *bit_cursor)
    {
        return None;
    }
    *bit_cursor = bit_cursor.saturating_add(candidate.bits);
    Some(InventoryRecordClaim {
        fragment_bits: candidate.bits,
    })
}

pub(super) fn try_get_legacy_live_inventory_fragment_bit_count(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    try_get_legacy_live_inventory_claim_candidate(bytes, record_offset, record_end)
        .map(|candidate| candidate.bits)
}

fn try_get_legacy_live_inventory_claim_candidate(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if !matches!(object_id, 0xFFFF_FFFD | 0xFFFF_FFFE)
        && !looks_like_legacy_live_object_id_value(object_id)
    {
        return None;
    }

    if mask == 0x2A00 {
        return try_parse_inventory_2a00_shape(bytes, record_offset, record_end)
            .map(|bits| GenericInventoryCandidate::new(record_end, bits));
    }

    if mask == 0x2000 {
        let feature25 = try_parse_inventory_2000_record(bytes, record_offset, record_end)?;
        return Some(GenericInventoryCandidate::new(
            record_end,
            usize::try_from(feature25.second_count)
                .ok()?
                .saturating_mul(3),
        ));
    }

    if mask == 0x0400 {
        // Decompile-backed inventory/equipment delta. Diamond and EE both read
        // a clear-count byte plus slot bytes, then a set-count byte plus slot
        // bytes; the set side owns one CNW BOOL fragment bit per slot. This is
        // therefore an identity translation only after this exact cursor
        // consumption succeeds. It must never be accepted as raw passthrough.
        let set_count = try_parse_inventory_0400(bytes, record_offset.checked_add(7)?, record_end)?;
        return Some(GenericInventoryCandidate::new(
            record_end,
            usize::from(set_count),
        ));
    }

    if mask == 0x0401 {
        let base_cursor = record_offset.checked_add(7)?.checked_add(10)?;
        let set_count = try_parse_inventory_0400(bytes, base_cursor, record_end)
            .or_else(|| try_parse_inventory_0400(bytes, base_cursor.checked_add(2)?, record_end))?;
        // Diamond `sub_455940` and EE `sub_1407B4F70` both read the 0x0001
        // branch as SHORT, DWORD, INT, BOOL before the 0x0400 equipment delta.
        // This compact 0x0401 parser is exact only when that BOOL is false; a
        // true branch enters the extended row/string reader and must be modelled
        // by a separate typed parser before it can claim bytes.
        return GenericInventoryCandidate::new(
            record_end,
            1usize.saturating_add(usize::from(set_count)),
        )
        .require_fragment_bit(0, false);
    }

    if mask == 0x2400 {
        return try_parse_inventory_2400_slot_update_shape(bytes, record_offset, record_end)
            .map(|bits| GenericInventoryCandidate::new(record_end, bits));
    }

    if mask == 0x2700 {
        return try_parse_inventory_2700_zero_count_feature25_shape(bytes, record_offset, record_end);
    }

    if matches!(mask, 0x2E00 | 0x2E01) {
        let shape = try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(
            bytes,
            record_offset,
            record_end,
        )?;
        if shape.read_end == record_end || shape.interleaved_fragment_tail_allowed {
            return Some(GenericInventoryCandidate::new(
                record_end,
                shape.fragment_bits,
            ));
        }
        return None;
    }

    if (mask & !GENERIC_INVENTORY_PARSE_MASK) == 0 {
        // Diamond and EE both walk the inventory mask in the fixed reader order
        // modelled by the generic parser. Masks with 0x0200/0x0800 need the
        // parser's branch search, but deterministic masks such as the HG self
        // inventory D5FF packet are the same decompile-backed family and must
        // not be excluded just because they do not contain an ambiguous branch.
        return try_parse_generic_inventory_claim_with_branching(
            bytes,
            record_offset,
            record_end,
            mask,
        );
    }

    None
}

pub(super) fn try_get_legacy_live_inventory_prefix_claim(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
) -> Option<InventoryRecordPrefixClaim> {
    if record_offset > bytes.len()
        || search_end > bytes.len()
        || search_end <= record_offset
        || search_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if !matches!(object_id, 0xFFFF_FFFD | 0xFFFF_FFFE)
        && !looks_like_legacy_live_object_id_value(object_id)
    {
        return None;
    }

    if (mask & !GENERIC_INVENTORY_PARSE_MASK) != 0 {
        return None;
    }

    if mask == 0x2B00 {
        return try_get_inventory_2b00_compact_self_state_prefix_claim(
            bytes,
            record_offset,
            search_end,
        );
    }

    if matches!(mask, 0x2E00 | 0x2E01) {
        let shape = try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(
            bytes,
            record_offset,
            search_end,
        )?;
        if shape.read_end <= record_offset + 7 || shape.read_end >= search_end {
            return None;
        }
        return Some(InventoryRecordPrefixClaim {
            read_end: shape.read_end,
            fragment_bits: shape.fragment_bits,
            interleaved_fragment_tail_allowed: shape.interleaved_fragment_tail_allowed,
        });
    }

    // This prefix proof is intentionally limited to deterministic inventory
    // masks. Ambiguous 0x0200/0x0800 records need an exact candidate boundary
    // to choose the byte shape, but large HG inventory state packets such as
    // D5FF have a single decompile-owned read order. When a legacy zlib stream
    // interleaves CNW fragment storage after that deterministic read body, the
    // parser can prove the read-buffer end before the scanner reaches the next
    // live-object opcode.
    if (mask & (0x0200 | 0x0800)) != 0 {
        return None;
    }

    let candidate =
        try_parse_generic_inventory_prefix_with_branching(bytes, record_offset, search_end, mask)?;
    if candidate.cursor <= record_offset + 7 || candidate.cursor >= search_end {
        return None;
    }

    Some(InventoryRecordPrefixClaim {
        read_end: candidate.cursor,
        fragment_bits: candidate.bits,
        interleaved_fragment_tail_allowed:
            generic_inventory_prefix_allows_interleaved_fragment_tail(mask, candidate),
    })
}

fn generic_inventory_prefix_allows_interleaved_fragment_tail(
    mask: u16,
    candidate: GenericInventoryCandidate,
) -> bool {
    // Diamond `sub_455940` and EE `sub_1407B4F70` both consume these
    // deterministic mask branches in the fixed generic order modelled by
    // `mask.rs`. The Starcore5/HG seq31 self-inventory stream proves that the
    // legacy server can then place chunk-local CNW fragment storage immediately
    // after the read-buffer cursor before the next live-object submessage.
    //
    // Keep this deliberately family-owned: ambiguous 0x0200/0x0800 masks are
    // excluded before the generic prefix parser reaches this helper, and the
    // caller still has to promote the adjacent bytes and re-run the exact
    // inventory reader against the resulting fragment-bit stream before the
    // packet can be emitted.
    matches!(mask, 0x0401 | 0xD5FF) && candidate.bits > 0
}

fn try_get_inventory_2b00_compact_self_state_prefix_claim(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
) -> Option<InventoryRecordPrefixClaim> {
    if record_offset > bytes.len()
        || search_end > bytes.len()
        || search_end <= record_offset
        || search_end - record_offset < 12
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u32_le(bytes, record_offset + 1)? != 0xFFFF_FFFE
        || read_u16_le(bytes, record_offset + 5)? != 0x2B00
    {
        return None;
    }

    // Same exact Starcore5 self-inventory shape as the rewrite helper below.
    // Prefix proof is deliberately narrow: it only splits a zero-entry 0x0100
    // stream plus Feature-25 zero first-count. Pre-rewrite legacy bytes stop
    // after the first count; post-rewrite EE bytes include the inserted zero
    // second count. In both cases the final exact claim must still validate
    // fragment bits for 0x0200=false and 0x0800=false.
    if bytes.get(record_offset + 7).copied() != Some(0)
        || read_u32_le(bytes, record_offset + 8)? != 0
    {
        return None;
    }

    let ee_rewritten_end = record_offset.checked_add(16)?;
    if ee_rewritten_end < search_end && read_u32_le(bytes, record_offset + 12) == Some(0) {
        return Some(InventoryRecordPrefixClaim {
            read_end: ee_rewritten_end,
            fragment_bits: 3,
            interleaved_fragment_tail_allowed: false,
        });
    }

    let legacy_missing_second_end = record_offset.checked_add(12)?;
    if legacy_missing_second_end < search_end {
        return Some(InventoryRecordPrefixClaim {
            read_end: legacy_missing_second_end,
            fragment_bits: 3,
            interleaved_fragment_tail_allowed: false,
        });
    }

    None
}

pub(super) fn rewrite_legacy_inventory_record_for_ee(
    bytes: &mut Vec<u8>,
    record_offset: usize,
    record_end: &mut usize,
) -> Option<InventoryRecordRewrite> {
    if record_offset > bytes.len()
        || *record_end > bytes.len()
        || *record_end <= record_offset
        || *record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let mask = read_u16_le(bytes, record_offset + 5)?;
    let feature25_cursor = match mask {
        0x2000 => record_offset.checked_add(7)?,
        0x2008 => {
            // Diamond and EE both consume the low 0x0008 inventory branch before
            // the feature-25 0x2000 branch (`sub_455940` / `sub_1407B4F70`).
            // This exact HG transition capture has that 0x0008 DWORD set to
            // zero, then a legacy 0x2000 zero-first/sentinel object tail. EE has
            // no reader for that tail, so normalize only this proved branch
            // cursor to EE's exact zero/zero feature-25 list shape.
            record_offset.checked_add(7)?.checked_add(4)?
        }
        0x2B00 => {
            // Starcore5/Sooty Crow capture, verified against the EE inventory
            // reader order in `sub_1407B4F70`: 0x0200 reads two CNW BOOLs
            // first, a false first BOOL consumes no read-buffer bytes, 0x0100
            // then reads an opcode-stream count byte, and 0x2000 then reads
            // Feature-25 OBJECTID lists. The legacy server emitted the
            // removal-list first count (`0`) but omitted EE's required second
            // DWORD count before the following `A/5` live-object record. This
            // exact repair inserts that zero second count; the final strict
            // claim still has to prove the 0x0200 false branch and 0x0800 false
            // branch from fragment bits.
            try_get_inventory_2b00_missing_feature25_second_count_cursor(
                bytes,
                record_offset,
                *record_end,
            )?
        }
        _ => return None,
    };

    if mask == 0x2B00 {
        return insert_missing_feature25_second_count_zero_for_ee(
            bytes,
            feature25_cursor,
            record_end,
        )
        .map(|bytes_inserted| InventoryRecordRewrite {
            bytes_inserted,
            bytes_removed: 0,
        });
    }

    normalize_legacy_feature25_tail_for_ee(bytes, feature25_cursor, record_end).map(
        |bytes_removed| InventoryRecordRewrite {
            bytes_inserted: 0,
            bytes_removed,
        },
    )
}

fn try_get_inventory_2b00_missing_feature25_second_count_cursor(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset != 12
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u32_le(bytes, record_offset + 1)? != 0xFFFF_FFFE
        || read_u16_le(bytes, record_offset + 5)? != 0x2B00
    {
        return None;
    }

    // After the zero-byte 0x0200 false branch, the decompiled 0x0100 reader
    // consumes a single opcode-stream count byte. This captured self-inventory
    // row has zero entries, so the following DWORD is the 0x2000 first-count.
    let opcode_stream_count = *bytes.get(record_offset + 7)?;
    if opcode_stream_count != 0 {
        return None;
    }
    let feature25_cursor = record_offset.checked_add(8)?;
    if feature25_cursor.checked_add(4)? != record_end
        || read_u32_le(bytes, feature25_cursor)? != 0
    {
        return None;
    }
    Some(feature25_cursor)
}

fn try_parse_inventory_2400_slot_update_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    // Decompile/capture evidence:
    //
    // * EE's CNWSMessage::WriteGameObjUpdate_WriteInventorySlotUpdate initializes
    //   an internal object-id sentinel of 0xFFFF_FFFE before emitting an inventory
    //   slot/appearance update path.
    // * The HG/Diamond capture that exposed this packet uses the legacy live-object
    //   inventory read-buffer opcode ('I') with object id 0xFFFF_FFFE and combined
    //   mask 0x2400, immediately followed by a D/5 delete record. The fragment tail
    //   has exactly two semantic bits: one for this inventory slot update and one
    //   for the D/5 record.
    //
    // This is intentionally not a broad plausibility parser. It only claims the
    // exact legacy read-buffer shape we can account for byte-for-byte, allowing the
    // higher live-object dispatcher to keep strict record boundaries instead of
    // quarantining a known semantic packet as an unknown raw zlib blob.
    if record_end.checked_sub(record_offset)? != 23 {
        return None;
    }

    if read_u32_le(bytes, record_offset + 1)? != 0xFFFF_FFFE {
        return None;
    }
    if read_u16_le(bytes, record_offset + 5)? != 0x2400 {
        return None;
    }

    let slot_count = *bytes.get(record_offset + 7)?;
    if slot_count != 1 {
        return None;
    }

    // The observed legacy shape repeats the slot/resource discriminator twice.
    // Requiring equality prevents the parser from swallowing unrelated bytes when
    // a future inventory mask needs a different decompile-backed model.
    let first_slot_or_resref = read_u16_le(bytes, record_offset + 8)?;
    let second_slot_or_resref = read_u16_le(bytes, record_offset + 10)?;
    if first_slot_or_resref != second_slot_or_resref {
        return None;
    }

    if read_u32_le(bytes, record_offset + 12)? != 0 {
        return None;
    }

    let compact_object_id = read_u32_le(bytes, record_offset + 16)?;
    let expanded_object_id = compact_object_id | 0x8000_0000;
    if !looks_like_legacy_live_object_id_value(compact_object_id)
        && !looks_like_legacy_live_object_id_value(expanded_object_id)
    {
        return None;
    }

    if read_u32_le(bytes, record_offset + 20)? != 0 {
        return None;
    }

    Some(1)
}

fn try_parse_inventory_2700_zero_count_feature25_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    // Focused exact shape for captured inventory mask 0x2700:
    //
    //   0x0400 equipment delta,
    //   0x0200 two CNW BOOLs with the second BOOL false selecting the DWORD
    //          zero-count branch,
    //   0x0100 opcode stream,
    //   0x2000 Feature-25 object lists.
    //
    // The large deterministic inventory masks still use the generic reader
    // order; keep this 0x2700 path local so a capture-specific mixed branch
    // does not change unrelated masks such as D5FF.
    let trace = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();
    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;

    let set_count = try_parse_inventory_0400_prefix(bytes, &mut cursor, record_end)?;
    fragment_bits = fragment_bits.checked_add(usize::from(set_count))?;
    if trace {
        eprintln!(
            "inventory 2700 stage: after_0400 offset={record_offset} record_end={record_end} cursor={cursor} bits={fragment_bits} set_count={set_count}"
        );
    }

    if cursor > record_end
        || record_end - cursor < 4
        || read_u32_le(bytes, cursor)? != 0
    {
        if trace {
            eprintln!(
                "inventory 2700 rejected: 0200-zero-dword offset={record_offset} cursor={cursor} record_end={record_end}"
            );
        }
        return None;
    }
    let second_0200_bit = fragment_bits.checked_add(1)?;
    let after_0200 = GenericInventoryCandidate::new(
        cursor.checked_add(4)?,
        fragment_bits.checked_add(2)?,
    )
    .require_fragment_bit(second_0200_bit, false)?;
    let candidates = [after_0200];

    let candidates = apply_0100(bytes, &candidates, record_end);
    if trace {
        eprintln!(
            "inventory 2700 stage: after_0100 candidates={:?}",
            candidates
                .iter()
                .map(|candidate| (candidate.cursor, candidate.bits))
                .collect::<Vec<_>>()
        );
    }
    let candidates = apply_2000(bytes, &candidates, record_end);
    if trace {
        eprintln!(
            "inventory 2700 stage: after_2000 candidates={:?}",
            candidates
                .iter()
                .map(|candidate| (candidate.cursor, candidate.bits))
                .collect::<Vec<_>>()
        );
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.cursor == record_end)
}

#[derive(Debug, Clone, Copy)]
struct Inventory2e00Or2e01GuiQuickbarLinkShape {
    read_end: usize,
    fragment_bits: usize,
    branch_0001_extended_bit: Option<usize>,
    branch_0200_second_bit: usize,
    branch_0800_present_bit: usize,
    branch_0800_present_value: bool,
    interleaved_fragment_tail_allowed: bool,
}

fn try_parse_inventory_2e00_or_2e01_gui_quickbar_link_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    let shape =
        try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(bytes, record_offset, record_end)?;
    (shape.read_end == record_end).then_some(shape)
}

fn try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    // Exact HG/Diamond inventory read-buffer shapes exposed by the seq40/seq42
    // quarantines:
    //
    //   I <object id> <mask 0x2E00>
    //     0x0400: BYTE clear_count, clear slots, BYTE set_count, set slots,
    //             set_count CNW BOOLs
    //     0x0200: BOOL, BOOL, DWORD zero-count branch
    //     0x2000: DWORD first_count, OBJECTIDs, DWORD second_count, OBJECTIDs,
    //             three CNW BOOLs per second-list object
    //     0x0800: BOOL-present, 12 read-buffer bytes
    //
    //   I <object id> <mask 0x2E01>
    //     0x0001: WORD, DWORD, INT, compact-branch BOOL=false
    //     then the same 0x0400/0x0200/0x2000/0x0800 sequence above
    //
    // Diamond client `sub_455940` and EE client `sub_1407B4F70` use the same
    // reader order and branch gates here. This is therefore an identity
    // translation only after the typed cursor proof succeeds; it is not a raw
    // passthrough escape hatch. The following read-buffer byte must be `G Q`
    // because the server immediately emits the quickbar-link GUI row block as
    // the next live-object submessage.
    if record_offset > bytes.len()
        || scan_end > bytes.len()
        || scan_end <= record_offset
        || scan_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if !matches!(mask, 0x2E00 | 0x2E01) {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    if !looks_like_legacy_live_object_id_value(object_id) {
        return None;
    }

    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;
    let mut branch_0001_extended_bit = None;

    if (mask & 0x0001) != 0 {
        if cursor > scan_end || scan_end - cursor < 10 {
            return None;
        }
        branch_0001_extended_bit = Some(fragment_bits);
        cursor = cursor.checked_add(10)?;
        fragment_bits = fragment_bits.saturating_add(1);
    }

    let set_count = try_parse_inventory_0400_prefix(bytes, &mut cursor, scan_end)?;
    fragment_bits = fragment_bits.saturating_add(usize::from(set_count));

    if let Some(shape) = try_parse_inventory_2e01_large_equipment_quickbar_link_tail(
        bytes,
        mask,
        cursor,
        scan_end,
        fragment_bits,
        branch_0001_extended_bit,
        set_count,
    ) {
        return Some(shape);
    }

    let branch_0200_first_bit = fragment_bits;
    let branch_0200_second_bit = branch_0200_first_bit.checked_add(1)?;
    if cursor > scan_end || scan_end - cursor < 4 || read_u32_le(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    fragment_bits = fragment_bits.saturating_add(2);

    let feature25 = try_parse_inventory_2000_prefix_at(bytes, cursor, scan_end)?;
    cursor = feature25.block_end;
    fragment_bits = fragment_bits.saturating_add(
        usize::try_from(feature25.second_count)
            .ok()?
            .checked_mul(3)?,
    );

    let branch_0800_present_bit = fragment_bits;
    if cursor > scan_end || scan_end - cursor < 12 {
        return None;
    }
    cursor = cursor.checked_add(12)?;
    fragment_bits = fragment_bits.saturating_add(1);

    if cursor < scan_end
        && (bytes.get(cursor).copied() != Some(b'G') || bytes.get(cursor + 1).copied() != Some(b'Q'))
    {
        return None;
    }

    Some(Inventory2e00Or2e01GuiQuickbarLinkShape {
        read_end: cursor,
        fragment_bits,
        branch_0001_extended_bit,
        branch_0200_second_bit,
        branch_0800_present_bit,
        branch_0800_present_value: true,
        interleaved_fragment_tail_allowed: false,
    })
}

fn try_parse_inventory_2e01_large_equipment_quickbar_link_tail(
    bytes: &[u8],
    mask: u16,
    cursor: usize,
    scan_end: usize,
    fragment_bits: usize,
    branch_0001_extended_bit: Option<usize>,
    set_count: u8,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    // Focused HG Sooty Crow sibling of the existing 0x2E01 quickbar-link
    // inventory family. The decompile-backed common prefix is unchanged:
    // compact 0x0001, then 0x0400 equipment-delta bytes. This capture carries
    // a large 0x0400 set-list followed by an eleven-byte bounded tail before
    // the next `GQ` row stream. Keep this intentionally narrow so it cannot
    // become a general inventory plausibility parser.
    if mask != 0x2E01 || set_count < 16 {
        return None;
    }

    const SOOTY_CROW_QUICKBAR_LINK_FRAGMENT_TAIL_BYTES: usize = 11;
    let span_end = cursor.checked_add(SOOTY_CROW_QUICKBAR_LINK_FRAGMENT_TAIL_BYTES)?;
    if cursor > scan_end {
        return None;
    }
    if cursor < scan_end
        && (span_end > scan_end
            || (span_end < scan_end
                && (bytes.get(span_end).copied() != Some(b'G')
                    || bytes.get(span_end + 1).copied() != Some(b'Q'))))
    {
        return None;
    }

    let branch_0200_first_bit = fragment_bits;
    let branch_0200_second_bit = branch_0200_first_bit.checked_add(1)?;
    let branch_0800_present_bit = branch_0200_second_bit.checked_add(1)?;
    Some(Inventory2e00Or2e01GuiQuickbarLinkShape {
        read_end: cursor,
        fragment_bits: fragment_bits.checked_add(3)?,
        branch_0001_extended_bit,
        branch_0200_second_bit,
        branch_0800_present_bit,
        branch_0800_present_value: false,
        interleaved_fragment_tail_allowed: true,
    })
}

fn try_parse_inventory_0400_prefix(
    bytes: &[u8],
    cursor: &mut usize,
    record_end: usize,
) -> Option<u8> {
    if *cursor >= record_end {
        return None;
    }
    let clear_count = usize::from(bytes[*cursor]);
    *cursor = cursor.checked_add(1)?;
    if clear_count > record_end.saturating_sub(*cursor) {
        return None;
    }
    *cursor = cursor.checked_add(clear_count)?;
    if *cursor >= record_end {
        return None;
    }
    let set_count = bytes[*cursor];
    *cursor = cursor.checked_add(1)?;
    if usize::from(set_count) > record_end.saturating_sub(*cursor) {
        return None;
    }
    *cursor = cursor.checked_add(usize::from(set_count))?;
    Some(set_count)
}

mod bit_count;
mod categories;
mod equipment_delta;
mod feature25;
mod icon_list;
mod mask;
mod opcode_stream;

use bit_count::*;
use categories::*;
use equipment_delta::*;
use feature25::*;
use icon_list::*;
use mask::*;
use opcode_stream::*;
