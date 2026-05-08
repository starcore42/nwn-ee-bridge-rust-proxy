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
struct GenericInventoryCandidate {
    cursor: usize,
    bits: usize,
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
    let bits = try_get_legacy_live_inventory_fragment_bit_count(bytes, offset, record_end)?;
    if bits > fragment_bits.len().saturating_sub(*bit_cursor) {
        return None;
    }
    *bit_cursor = bit_cursor.saturating_add(bits);
    Some(InventoryRecordClaim {
        fragment_bits: bits,
    })
}

pub(super) fn try_get_legacy_live_inventory_fragment_bit_count(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
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
        return try_parse_inventory_2a00_shape(bytes, record_offset, record_end);
    }

    if mask == 0x2000 {
        let feature25 = try_parse_feature25_record(bytes, record_offset, record_end)?;
        return Some(
            usize::try_from(feature25.second_count)
                .ok()?
                .saturating_mul(3),
        );
    }

    if mask == 0x0400 {
        // Decompile-backed inventory/equipment delta. Diamond and EE both read
        // a clear-count byte plus slot bytes, then a set-count byte plus slot
        // bytes; the set side owns one CNW BOOL fragment bit per slot. This is
        // therefore an identity translation only after this exact cursor
        // consumption succeeds. It must never be accepted as raw passthrough.
        let set_count = try_parse_inventory_0400(bytes, record_offset.checked_add(7)?, record_end)?;
        return Some(usize::from(set_count));
    }

    if mask == 0x0401 {
        let base_cursor = record_offset.checked_add(7)?.checked_add(10)?;
        let set_count = try_parse_inventory_0400(bytes, base_cursor, record_end)
            .or_else(|| try_parse_inventory_0400(bytes, base_cursor.checked_add(2)?, record_end))?;
        return Some(1usize.saturating_add(usize::from(set_count)));
    }

    if mask == 0x2400 {
        return try_parse_inventory_2400_slot_update_shape(bytes, record_offset, record_end);
    }

    if (mask & !GENERIC_INVENTORY_PARSE_MASK) == 0 && (mask & (0x0200 | 0x0800)) != 0 {
        return try_parse_generic_inventory_with_branching(bytes, record_offset, record_end, mask);
    }

    None
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
