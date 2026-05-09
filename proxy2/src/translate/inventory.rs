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
const MAX_FRAGMENT_BYTES: usize = 8;
const SERVER_OBJECT_ID_NAMESPACE_MASK: u32 = 0xFF00_0000;
const SERVER_ITEM_OBJECT_ID_NAMESPACE: u32 = 0x8000_0000;

#[derive(Debug, Clone, Copy)]
pub struct InventoryClaimSummary {
    pub minor: u8,
    pub old_declared: usize,
    pub new_declared: usize,
    pub legacy_prefix_removed: bool,
    pub object_id: u32,
    pub equip_slot: u32,
    pub fragment_bytes: usize,
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

fn claim_or_rewrite_equip_shape(payload: &mut Vec<u8>, minor: u8) -> Option<InventoryClaimSummary> {
    if payload.len() < READ_START + EE_READ_BYTES {
        return None;
    }

    let old_declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if old_declared < READ_START
        || old_declared > payload.len()
        || payload.len().saturating_sub(old_declared) > MAX_FRAGMENT_BYTES
    {
        return None;
    }

    let read_len = old_declared.checked_sub(READ_START)?;
    match read_len {
        EE_READ_BYTES => claim_ee_equip_shape(payload, minor, old_declared),
        LEGACY_READ_BYTES => rewrite_legacy_prefixed_equip_shape(payload, minor, old_declared),
        _ => None,
    }
}

fn claim_ee_equip_shape(
    payload: &[u8],
    minor: u8,
    declared: usize,
) -> Option<InventoryClaimSummary> {
    let object_id = read_le_u32(payload, READ_START)?;
    let equip_slot = read_le_u32(payload, READ_START + CNW_LENGTH_BYTES)?;
    if !looks_like_server_item_object_id(object_id) || !looks_like_equip_slot(equip_slot) {
        return None;
    }

    Some(InventoryClaimSummary {
        minor,
        old_declared: declared,
        new_declared: declared,
        legacy_prefix_removed: false,
        object_id,
        equip_slot,
        fragment_bytes: payload.len() - declared,
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
        equip_slot,
        fragment_bytes: payload.len() - new_declared,
    })
}

fn looks_like_server_item_object_id(object_id: u32) -> bool {
    (object_id & SERVER_OBJECT_ID_NAMESPACE_MASK) == SERVER_ITEM_OBJECT_ID_NAMESPACE
}

fn looks_like_equip_slot(equip_slot: u32) -> bool {
    equip_slot != 0 && (equip_slot & SERVER_OBJECT_ID_NAMESPACE_MASK) == 0
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let target = bytes.get_mut(offset..offset.checked_add(CNW_LENGTH_BYTES)?)?;
    target.copy_from_slice(&value.to_le_bytes());
    Some(())
}
