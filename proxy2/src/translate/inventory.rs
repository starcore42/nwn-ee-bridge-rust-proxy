//! Inventory packet semantic translation.
//!
//! Inventory equip/cancel packets are deliberately routed through this module
//! even when the observed bytes already match EE. That keeps the strict bridge
//! rule intact: a high-level opcode classifier is never an implicit allow.
//!
//! Decompile evidence:
//! - EE `CNWSMessage::SendServerToPlayerInventory_Equip` creates family
//!   `0x0C`, minor `0x01`, then writes `WriteOBJECTIDServer(object_id)`,
//!   `WriteBOOL(result)`, and `WriteDWORD(equip_slot, 0x20)`.
//! - EE `CNWCMessage::HandleServerToPlayerInventory` mirrors that reader shape:
//!   `sub_1409737C0` for the object id, `ReadBOOL`, and for cases 1/2
//!   `ReadDWORD(0x20)`. The bool is owned by CNW's MSB fragment stream, so the
//!   exact proof is the final-bit cursor (`3` header bits + `1` semantic bool),
//!   not zero-filled padding in the unused low bits of the final fragment byte.
//! - EE `SendServerToPlayerInventory_EquipCancel` uses the same body shape and
//!   sends minor `0x02`.
//! - The previous driver-side compatibility hook documented HG/1.69 packets
//!   carrying one legacy leading DWORD before the object id. If that legacy
//!   prefix is present on the wire, this translator removes it and repairs the
//!   CNW declared length; if the payload is already the EE shape, it claims it
//!   as a verified no-op.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const INVENTORY_MAJOR: u8 = 0x0C;
const EQUIP_MINOR: u8 = 0x01;
const EQUIP_CANCEL_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const EE_READ_BYTES: usize = 8;
const LEGACY_READ_BYTES: usize = 12;
const SINGLE_BOOL_FRAGMENT_BYTES: usize = 1;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const INVENTORY_EQUIP_BOOL_BITS: usize = 1;
const SINGLE_BOOL_FINAL_BITS: usize = CNW_FRAGMENT_HEADER_BITS + INVENTORY_EQUIP_BOOL_BITS;
const SERVER_OBJECT_ID_NAMESPACE_MASK: u32 = 0xFF00_0000;
const SERVER_ITEM_OBJECT_ID_NAMESPACE: u32 = 0x8000_0000;

#[derive(Debug, Clone, Copy)]
pub struct InventoryClaimSummary {
    pub minor: u8,
    pub old_declared: usize,
    pub new_declared: usize,
    pub legacy_prefix_removed: bool,
    pub object_id: u32,
    pub result: bool,
    pub equip_slot: u32,
    pub fragment_bytes: usize,
}

pub fn build_ee_inventory_payload(
    minor: u8,
    object_id: u32,
    result: bool,
    equip_slot: u32,
) -> Option<Vec<u8>> {
    if !matches!(minor, EQUIP_MINOR | EQUIP_CANCEL_MINOR)
        || !looks_like_server_item_object_id(object_id)
        || !looks_like_equip_slot(equip_slot)
    {
        return None;
    }

    let declared = READ_START + EE_READ_BYTES;
    let mut payload = Vec::with_capacity(declared + SINGLE_BOOL_FRAGMENT_BYTES);
    payload.extend_from_slice(&[b'P', INVENTORY_MAJOR, minor]);
    payload.extend_from_slice(&(declared as u32).to_le_bytes());
    payload.extend_from_slice(&object_id.to_le_bytes());
    payload.extend_from_slice(&equip_slot.to_le_bytes());
    payload.push(single_bool_fragment_byte(result));

    claim_payload_if_verified(&payload)
        .is_some()
        .then_some(payload)
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut Vec<u8>,
) -> Option<InventoryClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (INVENTORY_MAJOR, EQUIP_MINOR | EQUIP_CANCEL_MINOR) => {
            claim_or_rewrite_equip_shape(payload, high.minor)
        }
        _ => None,
    }
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<InventoryClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (INVENTORY_MAJOR, EQUIP_MINOR | EQUIP_CANCEL_MINOR) => {
            claim_equip_shape(payload, high.minor)
        }
        _ => None,
    }
}

fn claim_or_rewrite_equip_shape(payload: &mut Vec<u8>, minor: u8) -> Option<InventoryClaimSummary> {
    if payload.len() < READ_START + EE_READ_BYTES {
        return None;
    }

    let old_declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if old_declared < READ_START || old_declared > payload.len() {
        return None;
    }

    let read_len = old_declared.checked_sub(READ_START)?;
    match read_len {
        EE_READ_BYTES => claim_ee_equip_shape(payload, minor, old_declared),
        LEGACY_READ_BYTES => rewrite_legacy_prefixed_equip_shape(payload, minor, old_declared),
        _ => None,
    }
}

fn claim_equip_shape(payload: &[u8], minor: u8) -> Option<InventoryClaimSummary> {
    if payload.len() < READ_START + EE_READ_BYTES {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START || declared > payload.len() {
        return None;
    }

    let read_len = declared.checked_sub(READ_START)?;
    if read_len != EE_READ_BYTES {
        return None;
    }

    claim_ee_equip_shape(payload, minor, declared)
}

fn claim_ee_equip_shape(
    payload: &[u8],
    minor: u8,
    declared: usize,
) -> Option<InventoryClaimSummary> {
    let object_id = read_le_u32(payload, READ_START)?;
    let equip_slot = read_le_u32(payload, READ_START + CNW_LENGTH_BYTES)?;
    if !looks_like_server_item_object_id(object_id)
        || !looks_like_equip_slot(equip_slot)
        || !single_bool_fragment_shape_valid(&payload[declared..])
    {
        return None;
    }

    Some(InventoryClaimSummary {
        minor,
        old_declared: declared,
        new_declared: declared,
        legacy_prefix_removed: false,
        object_id,
        result: decode_single_bool_fragment(payload[declared]),
        equip_slot,
        fragment_bytes: SINGLE_BOOL_FRAGMENT_BYTES,
    })
}

fn rewrite_legacy_prefixed_equip_shape(
    payload: &mut Vec<u8>,
    minor: u8,
    old_declared: usize,
) -> Option<InventoryClaimSummary> {
    let legacy_prefix = read_le_u32(payload, READ_START)?;
    let object_id = read_le_u32(payload, READ_START + CNW_LENGTH_BYTES)?;
    let equip_slot = read_le_u32(payload, READ_START + 2 * CNW_LENGTH_BYTES)?;
    if legacy_prefix > 0xFF
        || !looks_like_server_item_object_id(object_id)
        || !looks_like_equip_slot(equip_slot)
        || !single_bool_fragment_shape_valid(&payload[old_declared..])
    {
        return None;
    }

    let new_declared = old_declared.checked_sub(CNW_LENGTH_BYTES)?;
    let new_declared_u32 = u32::try_from(new_declared).ok()?;
    payload.drain(READ_START..READ_START + CNW_LENGTH_BYTES);
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared_u32)?;

    Some(InventoryClaimSummary {
        minor,
        old_declared,
        new_declared,
        legacy_prefix_removed: true,
        object_id,
        result: decode_single_bool_fragment(payload[new_declared]),
        equip_slot,
        fragment_bytes: SINGLE_BOOL_FRAGMENT_BYTES,
    })
}

fn looks_like_server_item_object_id(object_id: u32) -> bool {
    (object_id & SERVER_OBJECT_ID_NAMESPACE_MASK) == SERVER_ITEM_OBJECT_ID_NAMESPACE
}

fn looks_like_equip_slot(equip_slot: u32) -> bool {
    equip_slot != 0 && (equip_slot & SERVER_OBJECT_ID_NAMESPACE_MASK) == 0
}

fn single_bool_fragment_shape_valid(fragment: &[u8]) -> bool {
    if fragment.len() != SINGLE_BOOL_FRAGMENT_BYTES {
        return false;
    }

    usize::from((fragment[0] & 0xE0) >> 5) == SINGLE_BOOL_FINAL_BITS
}

fn decode_single_bool_fragment(byte: u8) -> bool {
    byte & 0x10 != 0
}

fn single_bool_fragment_byte(value: bool) -> u8 {
    0x80 | if value { 0x10 } else { 0x00 }
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let target = bytes.get_mut(offset..offset.checked_add(CNW_LENGTH_BYTES)?)?;
    target.copy_from_slice(&value.to_le_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_exact_ee_inventory_equip_shape() {
        let payload = [
            b'P',
            INVENTORY_MAJOR,
            EQUIP_MINOR,
            0x0F,
            0x00,
            0x00,
            0x00,
            0x34,
            0x12,
            0x00,
            0x80,
            0x04,
            0x00,
            0x00,
            0x00,
            0x90,
        ];

        let claim = claim_payload_if_verified(&payload).expect("exact EE inventory equip claim");
        assert_eq!(claim.minor, EQUIP_MINOR);
        assert_eq!(claim.object_id, 0x8000_1234);
        assert!(claim.result);
        assert_eq!(claim.equip_slot, 4);
        assert_eq!(claim.fragment_bytes, 1);
        assert!(!claim.legacy_prefix_removed);
    }

    #[test]
    fn rewrites_legacy_prefixed_inventory_equip_to_exact_ee_shape() {
        let mut payload = vec![
            b'P',
            INVENTORY_MAJOR,
            EQUIP_MINOR,
            0x13,
            0x00,
            0x00,
            0x00,
            0x01,
            0x00,
            0x00,
            0x00,
            0x34,
            0x12,
            0x00,
            0x80,
            0x04,
            0x00,
            0x00,
            0x00,
            0x80,
        ];

        let rewrite = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("legacy prefixed inventory equip should rewrite");
        assert!(rewrite.legacy_prefix_removed);
        assert_eq!(rewrite.old_declared, 0x13);
        assert_eq!(rewrite.new_declared, 0x0F);
        assert!(!rewrite.result);
        assert_eq!(&payload[3..7], &0x0Fu32.to_le_bytes());
        assert!(claim_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn builds_exact_ee_inventory_equip_payload() {
        let payload = build_ee_inventory_payload(EQUIP_MINOR, 0x8000_1234, true, 4)
            .expect("valid inventory equip payload");

        assert_eq!(
            payload,
            vec![
                b'P',
                INVENTORY_MAJOR,
                EQUIP_MINOR,
                0x0F,
                0x00,
                0x00,
                0x00,
                0x34,
                0x12,
                0x00,
                0x80,
                0x04,
                0x00,
                0x00,
                0x00,
                0x90,
            ]
        );
        let claim = claim_payload_if_verified(&payload).expect("built payload should validate");
        assert_eq!(claim.object_id, 0x8000_1234);
        assert!(claim.result);
        assert_eq!(claim.equip_slot, 4);
    }

    #[test]
    fn rejects_inventory_equip_without_exact_single_bool_fragment() {
        let payload = [
            b'P',
            INVENTORY_MAJOR,
            EQUIP_MINOR,
            0x0F,
            0x00,
            0x00,
            0x00,
            0x34,
            0x12,
            0x00,
            0x80,
            0x04,
            0x00,
            0x00,
            0x00,
            0xA0,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn claims_captured_hg_inventory_equip_with_dirty_fragment_padding() {
        let payload = [
            b'P',
            INVENTORY_MAJOR,
            EQUIP_MINOR,
            0x0F,
            0x00,
            0x00,
            0x00,
            0x69,
            0x8E,
            0x01,
            0x80,
            0x00,
            0x00,
            0x02,
            0x00,
            0x8B,
        ];

        let claim = claim_payload_if_verified(&payload)
            .expect("captured HG Inventory_Equip should prove one bool fragment");
        assert_eq!(claim.object_id, 0x8001_8E69);
        assert_eq!(claim.equip_slot, 0x0002_0000);
        assert_eq!(claim.fragment_bytes, 1);
        assert!(!claim.legacy_prefix_removed);
    }
}
