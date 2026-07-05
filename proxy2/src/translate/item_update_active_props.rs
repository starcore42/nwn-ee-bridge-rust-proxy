//! Server-originated active item property updates (`0x18/0x01` and `0x18/0x02`).
//!
//! EE decompile evidence:
//! - `CNWSItem::UpdateUsedActiveProperties` calls
//!   `CNWSMessage::SendServerToPlayerUpdateActiveItemPropertiesUses` when the
//!   active-property use mask changes, and calls
//!   `CNWSMessage::SendServerToPlayerUpdateActiveItemProperties` for the full
//!   active-property refresh path.
//! - `SendServerToPlayerUpdateActiveItemPropertiesUses`
//!   (`nwn ee decompile.txt:1858694`) writes OBJECTID, BYTE used mask, BYTE
//!   changed-uses mask, then one BYTE use count for each set bit in the changed
//!   mask before sending family `0x18`, minor `0x01`.
//! - `SendServerToPlayerUpdateActiveItemProperties`
//!   (`nwn ee decompile.txt:1858546`) writes OBJECTID, BYTE active-property
//!   count, then for each property WORD property, WORD subtype, WORD cost table
//!   value, BYTE param, followed by BYTE used mask, BYTE `0xFF`, and eight BYTE
//!   use counts before sending family `0x18`, minor `0x02`.

use crate::{crc::read_le_u32, packet::m::HighLevel};

pub const ACTIVE_ITEM_PROPERTIES_MAJOR: u8 = 0x18;
pub const USES_MINOR: u8 = 0x01;
pub const FULL_MINOR: u8 = 0x02;

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const OBJECT_ID_BYTES: usize = 4;
const BYTE_BYTES: usize = 1;
const WORD_BYTES: usize = 2;
const ACTIVE_PROPERTY_ROW_BYTES: usize = WORD_BYTES + WORD_BYTES + WORD_BYTES + BYTE_BYTES;
const ACTIVE_PROPERTY_USE_COUNT_BYTES: usize = 8;
const EMPTY_FINAL_FRAGMENT_CURSOR: u8 = 0x60;
const SINGLE_FRAGMENT_BYTE: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveItemPropertiesClaimSummary {
    pub minor: u8,
    pub packet_name: &'static str,
    pub declared: usize,
    pub object_id: u32,
    pub used_property_mask: u8,
    pub changed_uses_mask: u8,
    pub changed_use_count_rows: u8,
    pub full_property_count: u8,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ActiveItemPropertiesClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != ACTIVE_ITEM_PROPERTIES_MAJOR {
        return None;
    }

    match high.minor {
        USES_MINOR => claim_uses_payload_if_verified(payload, high),
        FULL_MINOR => claim_full_payload_if_verified(payload, high),
        _ => None,
    }
}

fn claim_uses_payload_if_verified(
    payload: &[u8],
    high: HighLevel,
) -> Option<ActiveItemPropertiesClaimSummary> {
    let declared = exact_cnw_declared_with_empty_tail(payload)?;
    let fixed_body_bytes = OBJECT_ID_BYTES + BYTE_BYTES + BYTE_BYTES;
    let object_id = read_le_u32(payload, READ_START)?;
    let used_property_mask = *payload.get(READ_START + OBJECT_ID_BYTES)?;
    let changed_uses_mask = *payload.get(READ_START + OBJECT_ID_BYTES + BYTE_BYTES)?;
    let changed_use_count_rows = changed_uses_mask.count_ones() as u8;
    let expected_declared = READ_START
        .checked_add(fixed_body_bytes)?
        .checked_add(usize::from(changed_use_count_rows))?;
    if declared != expected_declared {
        return None;
    }

    Some(ActiveItemPropertiesClaimSummary {
        minor: high.minor,
        packet_name: high.name(),
        declared,
        object_id,
        used_property_mask,
        changed_uses_mask,
        changed_use_count_rows,
        full_property_count: 0,
    })
}

fn claim_full_payload_if_verified(
    payload: &[u8],
    high: HighLevel,
) -> Option<ActiveItemPropertiesClaimSummary> {
    let declared = exact_cnw_declared_with_empty_tail(payload)?;
    let object_id = read_le_u32(payload, READ_START)?;
    let property_count = *payload.get(READ_START + OBJECT_ID_BYTES)?;
    let rows_bytes = usize::from(property_count).checked_mul(ACTIVE_PROPERTY_ROW_BYTES)?;
    let expected_declared = READ_START
        .checked_add(OBJECT_ID_BYTES)?
        .checked_add(BYTE_BYTES)?
        .checked_add(rows_bytes)?
        .checked_add(BYTE_BYTES)? // used property mask
        .checked_add(BYTE_BYTES)? // 0xFF sentinel
        .checked_add(ACTIVE_PROPERTY_USE_COUNT_BYTES)?;
    if declared != expected_declared {
        return None;
    }

    let used_property_mask_offset = READ_START + OBJECT_ID_BYTES + BYTE_BYTES + rows_bytes;
    let used_property_mask = *payload.get(used_property_mask_offset)?;
    let sentinel = *payload.get(used_property_mask_offset + BYTE_BYTES)?;
    if sentinel != 0xFF {
        return None;
    }

    Some(ActiveItemPropertiesClaimSummary {
        minor: high.minor,
        packet_name: high.name(),
        declared,
        object_id,
        used_property_mask,
        changed_uses_mask: 0,
        changed_use_count_rows: 0,
        full_property_count: property_count,
    })
}

fn exact_cnw_declared_with_empty_tail(payload: &[u8]) -> Option<usize> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START {
        return None;
    }
    if payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)? {
        return None;
    }
    (*payload.get(declared)? == EMPTY_FINAL_FRAGMENT_CURSOR).then_some(declared)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_header(minor: u8, body: &[u8]) -> Vec<u8> {
        let declared = (READ_START + body.len()) as u32;
        let mut payload = vec![0x70, ACTIVE_ITEM_PROPERTIES_MAJOR, minor];
        payload.extend_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(body);
        payload.push(EMPTY_FINAL_FRAGMENT_CURSOR);
        payload
    }

    #[test]
    fn claims_active_property_uses_delta_sender_shape() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x8001_5989u32.to_le_bytes());
        body.push(0x05);
        body.push(0b0000_0101);
        body.extend_from_slice(&[3, 7]);

        let claim = claim_payload_if_verified(&with_header(USES_MINOR, &body))
            .expect("uses delta should match the EE sender cursor shape");

        assert_eq!(claim.minor, USES_MINOR);
        assert_eq!(claim.object_id, 0x8001_5989);
        assert_eq!(claim.used_property_mask, 0x05);
        assert_eq!(claim.changed_uses_mask, 0b0000_0101);
        assert_eq!(claim.changed_use_count_rows, 2);
        assert_eq!(claim.full_property_count, 0);
    }

    #[test]
    fn rejects_uses_delta_missing_mask_selected_use_count() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x8001_5989u32.to_le_bytes());
        body.push(0x05);
        body.push(0b0000_0101);
        body.push(3);

        assert!(claim_payload_if_verified(&with_header(USES_MINOR, &body)).is_none());
    }

    #[test]
    fn claims_full_active_property_refresh_sender_shape() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x8001_5989u32.to_le_bytes());
        body.push(1);
        body.extend_from_slice(&0x0064u16.to_le_bytes());
        body.extend_from_slice(&0x020Du16.to_le_bytes());
        body.extend_from_slice(&0x0003u16.to_le_bytes());
        body.push(4);
        body.push(0x01);
        body.push(0xFF);
        body.extend_from_slice(&[8, 7, 6, 5, 4, 3, 2, 1]);

        let claim = claim_payload_if_verified(&with_header(FULL_MINOR, &body))
            .expect("full active-property refresh should match the EE sender cursor shape");

        assert_eq!(claim.minor, FULL_MINOR);
        assert_eq!(claim.object_id, 0x8001_5989);
        assert_eq!(claim.full_property_count, 1);
        assert_eq!(claim.used_property_mask, 0x01);
        assert_eq!(claim.changed_use_count_rows, 0);
    }

    #[test]
    fn rejects_full_refresh_without_ff_sentinel_or_empty_tail() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x8001_5989u32.to_le_bytes());
        body.push(0);
        body.push(0x01);
        body.push(0x00);
        body.extend_from_slice(&[0; ACTIVE_PROPERTY_USE_COUNT_BYTES]);

        assert!(claim_payload_if_verified(&with_header(FULL_MINOR, &body)).is_none());

        body[OBJECT_ID_BYTES + BYTE_BYTES + BYTE_BYTES] = 0xFF;
        let mut shifted_tail = with_header(FULL_MINOR, &body);
        *shifted_tail
            .last_mut()
            .expect("test payload has a fragment byte") = 0x80;
        assert!(claim_payload_if_verified(&shifted_tail).is_none());
    }
}
