//! Client-originated inventory semantic claims.
//!
//! `Inventory_EquipToggle` (`P/0C/0B`) has the same reader contract in EE and
//! Diamond/1.69:
//!
//! - EE's client writer at `0x1407C0E20..0x1407C0EC0` writes the primary item
//!   id, compares the secondary id with `INVALID_OBJECT_ID`, writes a false
//!   `BOOL` for that sentinel or a true `BOOL` plus the secondary id otherwise,
//!   then sends major `0x0C`, minor `0x0B`.
//! - EE's `CNWSMessage::HandlePlayerToServerInventoryMessage` case 11 at
//!   `0x140456238..0x140456290` reads `OBJECTIDServer`, `BOOL`, and the optional
//!   second `OBJECTIDServer`, then requires both no overflow and no underflow.
//! - Diamond's corresponding case 11 at `0x005418ED..0x00541940` performs the
//!   same reads, branch, overflow checks, and terminal underflow check.
//! - Diamond's client writer at `0x004B09C0..0x004B0A45` uses the same invalid
//!   sentinel branch and writes that identical primary-id/guard/optional-id
//!   sequence before sending `0x0C/0x0B`.
//!
//! The optional guard is stored in one CNW fragment byte after the declared
//! read buffer. `CNWMessage::GetWriteMessage` stores the final fragment cursor
//! in the high three bits while preserving lower residual bits (Diamond
//! `0x004FC9A5..0x004FC9B8`). Diamond's one-bit reader at
//! `0x0050782B..0x00507899` uses `1 << (7 - bit_cursor)`, proving MSB-first
//! BOOL order. Therefore the validator owns the exact cursor and semantic BOOL
//! bit, but deliberately ignores lower residual bits. The live HG false-branch
//! seed ends in `0x88`: cursor `0x80`, false data bit, and residual `0x08`.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const INVENTORY_MAJOR: u8 = 0x0C;
const EQUIP_TOGGLE_MINOR: u8 = 0x0B;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const OBJECT_ID_BYTES: usize = 4;
const FALSE_BRANCH_DECLARED_BYTES: usize = READ_START + OBJECT_ID_BYTES;
const TRUE_BRANCH_DECLARED_BYTES: usize = FALSE_BRANCH_DECLARED_BYTES + OBJECT_ID_BYTES;
const FRAGMENT_BYTES: usize = 1;
const FRAGMENT_CURSOR_MASK: u8 = 0xE0;
const SINGLE_BOOL_FINAL_CURSOR: u8 = 0x80;
const SINGLE_BOOL_DATA_BIT: u8 = 0x10;
const INVALID_OBJECT_ID: u32 = 0x7F00_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientInventoryClaimSummary {
    pub packet_name: &'static str,
    pub primary_object_id: u32,
    pub secondary_object_id: Option<u32>,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientInventoryClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != INVENTORY_MAJOR || high.minor != EQUIP_TOGGLE_MINOR {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let fragment = *payload.last()?;
    if fragment & FRAGMENT_CURSOR_MASK != SINGLE_BOOL_FINAL_CURSOR {
        return None;
    }
    let has_secondary_object = fragment & SINGLE_BOOL_DATA_BIT != 0;
    let expected_declared = if has_secondary_object {
        TRUE_BRANCH_DECLARED_BYTES
    } else {
        FALSE_BRANCH_DECLARED_BYTES
    };
    if declared != expected_declared || payload.len() != declared.checked_add(FRAGMENT_BYTES)? {
        return None;
    }

    let primary_object_id = read_le_u32(payload, READ_START)?;
    let secondary_object_id = has_secondary_object
        .then(|| read_le_u32(payload, FALSE_BRANCH_DECLARED_BYTES))
        .flatten();
    // Both client writers select the false branch when the secondary argument
    // equals INVALID_OBJECT_ID, so a true guard plus that sentinel cannot be an
    // authentic writer result.
    if has_secondary_object
        && (secondary_object_id.is_none() || secondary_object_id == Some(INVALID_OBJECT_ID))
    {
        return None;
    }

    Some(ClientInventoryClaimSummary {
        packet_name: "Inventory_EquipToggle",
        primary_object_id,
        secondary_object_id,
        declared,
        fragment_bytes: FRAGMENT_BYTES,
    })
}

/// Build the exact EE/Diamond client writer shape for `Inventory_EquipToggle`.
///
/// Both original writers encode an absent secondary object as one false BOOL.
/// A present secondary object is encoded as one true BOOL followed by the raw
/// OBJECTID. The final fragment byte is canonicalized to zero residual bits;
/// captured client packets may retain unrelated low scratch bits, which the
/// exact reader intentionally ignores.
pub fn build_equip_toggle_payload(
    primary_object_id: u32,
    secondary_object_id: Option<u32>,
) -> Option<Vec<u8>> {
    if primary_object_id == INVALID_OBJECT_ID || secondary_object_id == Some(INVALID_OBJECT_ID) {
        return None;
    }

    let declared = if secondary_object_id.is_some() {
        TRUE_BRANCH_DECLARED_BYTES
    } else {
        FALSE_BRANCH_DECLARED_BYTES
    };
    let mut payload = Vec::with_capacity(declared.checked_add(FRAGMENT_BYTES)?);
    payload.extend_from_slice(&[0x70, INVENTORY_MAJOR, EQUIP_TOGGLE_MINOR]);
    payload.extend_from_slice(&(u32::try_from(declared).ok()?).to_le_bytes());
    payload.extend_from_slice(&primary_object_id.to_le_bytes());
    if let Some(secondary_object_id) = secondary_object_id {
        payload.extend_from_slice(&secondary_object_id.to_le_bytes());
    }
    payload.push(
        SINGLE_BOOL_FINAL_CURSOR
            | if secondary_object_id.is_some() {
                SINGLE_BOOL_DATA_BIT
            } else {
                0
            },
    );

    claim_payload_if_verified(&payload)?;
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equip_toggle_payload(
        primary_object_id: u32,
        secondary_object_id: Option<u32>,
        fragment: u8,
    ) -> Vec<u8> {
        let declared = if secondary_object_id.is_some() {
            TRUE_BRANCH_DECLARED_BYTES
        } else {
            FALSE_BRANCH_DECLARED_BYTES
        };
        let mut payload = vec![0x70, INVENTORY_MAJOR, EQUIP_TOGGLE_MINOR];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&primary_object_id.to_le_bytes());
        if let Some(secondary_object_id) = secondary_object_id {
            payload.extend_from_slice(&secondary_object_id.to_le_bytes());
        }
        payload.push(fragment);
        payload
    }

    #[test]
    fn claims_live_false_branch_with_residual_fragment_bits() {
        let payload = [
            0x70, 0x0C, 0x0B, 0x0B, 0x00, 0x00, 0x00, 0xE8, 0x5A, 0x01, 0x80, 0x88,
        ];

        let claim =
            claim_payload_if_verified(&payload).expect("live false branch should claim exactly");

        assert_eq!(claim.packet_name, "Inventory_EquipToggle");
        assert_eq!(claim.primary_object_id, 0x8001_5AE8);
        assert_eq!(claim.secondary_object_id, None);
        assert_eq!(claim.declared, FALSE_BRANCH_DECLARED_BYTES);
        assert_eq!(claim.fragment_bytes, FRAGMENT_BYTES);
    }

    #[test]
    fn claims_true_branch_with_optional_second_object() {
        let payload = equip_toggle_payload(0x8000_1234, Some(0x8000_5678), 0x90);

        let claim =
            claim_payload_if_verified(&payload).expect("true branch should own both item ids");

        assert_eq!(claim.primary_object_id, 0x8000_1234);
        assert_eq!(claim.secondary_object_id, Some(0x8000_5678));
        assert_eq!(claim.declared, TRUE_BRANCH_DECLARED_BYTES);
    }

    #[test]
    fn builder_emits_canonical_false_and_true_writer_shapes() {
        let false_branch =
            build_equip_toggle_payload(0x8000_1234, None).expect("false branch should build");
        let true_branch = build_equip_toggle_payload(0x8000_1234, Some(0x8000_5678))
            .expect("true branch should build");

        assert_eq!(
            false_branch,
            [
                0x70, 0x0C, 0x0B, 0x0B, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x80, 0x80,
            ]
        );
        assert_eq!(
            true_branch,
            [
                0x70, 0x0C, 0x0B, 0x0F, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x80, 0x78, 0x56, 0x00,
                0x80, 0x90,
            ]
        );
        assert_eq!(
            claim_payload_if_verified(&false_branch)
                .expect("built false branch should self-validate")
                .secondary_object_id,
            None
        );
        assert_eq!(
            claim_payload_if_verified(&true_branch)
                .expect("built true branch should self-validate")
                .secondary_object_id,
            Some(0x8000_5678)
        );
    }

    #[test]
    fn builder_rejects_invalid_writer_object_ids() {
        assert!(build_equip_toggle_payload(INVALID_OBJECT_ID, None).is_none());
        assert!(build_equip_toggle_payload(0x8000_1234, Some(INVALID_OBJECT_ID)).is_none());
    }

    #[test]
    fn accepts_unowned_low_fragment_residual_bits_on_both_branches() {
        let false_branch = equip_toggle_payload(0x8000_1234, None, 0x8F);
        let true_branch = equip_toggle_payload(0x8000_1234, Some(0x8000_5678), 0x9F);

        assert!(claim_payload_if_verified(&false_branch).is_some());
        assert!(claim_payload_if_verified(&true_branch).is_some());
    }

    #[test]
    fn rejects_optional_object_branch_mismatches() {
        let true_guard_without_second_object = equip_toggle_payload(0x8000_1234, None, 0x90);
        let false_guard_with_second_object =
            equip_toggle_payload(0x8000_1234, Some(0x8000_5678), 0x80);
        let true_guard_with_invalid_sentinel =
            equip_toggle_payload(0x8000_1234, Some(INVALID_OBJECT_ID), 0x90);

        assert!(claim_payload_if_verified(&true_guard_without_second_object).is_none());
        assert!(claim_payload_if_verified(&false_guard_with_second_object).is_none());
        assert!(claim_payload_if_verified(&true_guard_with_invalid_sentinel).is_none());
    }

    #[test]
    fn rejects_shifted_cursor_wrong_declaration_and_tail_slack() {
        let shifted_cursor = equip_toggle_payload(0x8000_1234, None, 0xA0);
        let mut wrong_declaration = equip_toggle_payload(0x8000_1234, None, 0x80);
        wrong_declaration[3..7].copy_from_slice(&(TRUE_BRANCH_DECLARED_BYTES as u32).to_le_bytes());
        let mut tail_slack = equip_toggle_payload(0x8000_1234, None, 0x80);
        tail_slack.push(0);

        assert!(claim_payload_if_verified(&shifted_cursor).is_none());
        assert!(claim_payload_if_verified(&wrong_declaration).is_none());
        assert!(claim_payload_if_verified(&tail_slack).is_none());
    }

    #[test]
    fn rejects_sibling_inventory_minor() {
        let mut payload = equip_toggle_payload(0x8000_1234, None, 0x80);
        payload[2] = 0x01;

        assert!(claim_payload_if_verified(&payload).is_none());
    }
}
