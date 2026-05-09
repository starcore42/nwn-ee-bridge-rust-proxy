//! Client-originated GUI inventory semantic claims.
//!
//! Decompile anchors:
//!
//! - EE `CNWSMessage::SendPlayerToServerGuiInventory_Status` writes one
//!   `BOOL` and then `WriteOBJECTIDServer`.
//! - EE `CNWSMessage::HandlePlayerToServerGuiInventoryMessage` reads minor
//!   `0x01` as the same `BOOL + OBJECTIDServer` shape and treats either the
//!   current creature id or `0x7F000000` as the self-inventory owner.
//! - Diamond/HG captures proved the EE harnessed self request
//!   `70 0D 01 0B 00 00 00 FD FF FF FF 90` must rewrite the object id to
//!   `0x7F000000` before the 1.69 server returns the GUI inventory stream.
//!
//! The BOOL lives in the single CNW fragment byte after the declared read
//! buffer. This translator validates that exact packetized shape, rewrites only
//! the EE self sentinel, and otherwise leaves validated object ids untouched.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const GUI_INVENTORY_MAJOR: u8 = 0x0D;
const STATUS_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const STATUS_DECLARED_BYTES: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 4;
const STATUS_FRAGMENT_BYTES: usize = 1;
const STATUS_OBJECT_ID_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const EE_SELF_OBJECT_ID: u32 = 0xFFFF_FFFD;
const DIAMOND_CURRENT_PLAYER_OBJECT_ID: u32 = 0x7F00_0000;

#[derive(Debug, Clone, Copy)]
pub struct ClientGuiInventoryClaimSummary {
    pub packet_name: &'static str,
    pub object_id: u32,
    pub rewritten_self_object_id: bool,
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut [u8],
) -> Option<ClientGuiInventoryClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != GUI_INVENTORY_MAJOR || high.minor != STATUS_MINOR {
        return None;
    }
    claim_or_rewrite_status_payload_if_verified(payload)
}

fn claim_or_rewrite_status_payload_if_verified(
    payload: &mut [u8],
) -> Option<ClientGuiInventoryClaimSummary> {
    if !status_payload_shape_valid(payload) {
        return None;
    }

    let object_id = read_le_u32(payload, STATUS_OBJECT_ID_OFFSET)?;
    let rewritten_self_object_id = object_id == EE_SELF_OBJECT_ID;
    if rewritten_self_object_id {
        payload[STATUS_OBJECT_ID_OFFSET..STATUS_OBJECT_ID_OFFSET + 4]
            .copy_from_slice(&DIAMOND_CURRENT_PLAYER_OBJECT_ID.to_le_bytes());
    }

    Some(ClientGuiInventoryClaimSummary {
        packet_name: "GuiInventory_Status",
        object_id: if rewritten_self_object_id {
            DIAMOND_CURRENT_PLAYER_OBJECT_ID
        } else {
            object_id
        },
        rewritten_self_object_id,
    })
}

fn status_payload_shape_valid(payload: &[u8]) -> bool {
    read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)
        .and_then(|declared| usize::try_from(declared).ok())
        == Some(STATUS_DECLARED_BYTES)
        && payload.len() == STATUS_DECLARED_BYTES + STATUS_FRAGMENT_BYTES
        && read_le_u32(payload, STATUS_OBJECT_ID_OFFSET).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_ee_self_inventory_status_to_diamond_current_player() {
        let mut payload = [
            0x70, 0x0D, 0x01, 0x0B, 0x00, 0x00, 0x00, 0xFD, 0xFF, 0xFF, 0xFF, 0x90,
        ];

        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("observed EE GuiInventory_Status should be claimed");

        assert_eq!(summary.packet_name, "GuiInventory_Status");
        assert_eq!(summary.object_id, DIAMOND_CURRENT_PLAYER_OBJECT_ID);
        assert!(summary.rewritten_self_object_id);
        assert_eq!(
            &payload[STATUS_OBJECT_ID_OFFSET..STATUS_OBJECT_ID_OFFSET + 4],
            &DIAMOND_CURRENT_PLAYER_OBJECT_ID.to_le_bytes()
        );
        assert_eq!(payload[11], 0x90);
    }

    #[test]
    fn claims_existing_diamond_inventory_status_without_rewrite() {
        let mut payload = [
            0x70, 0x0D, 0x01, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0x90,
        ];

        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("Diamond current-player GuiInventory_Status should be claimed");

        assert_eq!(summary.object_id, DIAMOND_CURRENT_PLAYER_OBJECT_ID);
        assert!(!summary.rewritten_self_object_id);
    }

    #[test]
    fn rejects_inventory_status_with_wrong_declared_length() {
        let mut payload = [
            0x70, 0x0D, 0x01, 0x0A, 0x00, 0x00, 0x00, 0xFD, 0xFF, 0xFF, 0xFF, 0x90,
        ];

        assert!(claim_or_rewrite_payload_if_verified(&mut payload).is_none());
    }
}
