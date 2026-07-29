//! Server-originated GUI inventory semantic claims.
//!
//! `GuiInventory_SelectPanel` is direction-asymmetric. The server-to-player
//! packet owns only an eight-bit panel value; the player-to-server packet in
//! `client_gui_inventory` owns the same byte followed by a `BOOL`.
//!
//! Exact decompile evidence:
//! - EE `CNWSMessage::SendServerToPlayerInventory_SelectPanel`
//!   (`0x1404DAA40`) creates a one-byte read message, writes
//!   `WriteBYTE(panel, 8, 1)`, and sends `0x0D/0x02`.
//! - EE client handler `sub_14079D4F0` reads `ReadBYTE(8, 1)` and then checks
//!   both overflow and underflow, with no `ReadBOOL`.
//! - Diamond server writer `sub_62DE90` and client handler `sub_450250`
//!   use the same one-byte shape.
//!
//! `CNWMessage::GetWriteMessage` still emits one compact fragment-storage byte.
//! With no semantic fragment bits, its high three bits report cursor `3`
//! (the CNW fragment header itself). Low residual bits are not owned and may be
//! dirty, as in the live HG tail `0x62`, so validation keys only on the cursor.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const GUI_INVENTORY_MAJOR: u8 = 0x0D;
const SELECT_PANEL_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const PANEL_BYTES: usize = 1;
const SELECT_PANEL_DECLARED_BYTES: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + PANEL_BYTES;
const SELECT_PANEL_FRAGMENT_BYTES: usize = 1;
const PANEL_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const FRAGMENT_CURSOR_MASK: u8 = 0xE0;
const EMPTY_FINAL_CURSOR: u8 = 0x60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuiInventoryClaimSummary {
    pub packet_name: &'static str,
    pub panel: u8,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn build_select_panel_payload(panel: u8) -> Vec<u8> {
    let mut payload = Vec::with_capacity(SELECT_PANEL_DECLARED_BYTES + SELECT_PANEL_FRAGMENT_BYTES);
    payload.extend_from_slice(&[b'P', GUI_INVENTORY_MAJOR, SELECT_PANEL_MINOR]);
    payload.extend_from_slice(&(SELECT_PANEL_DECLARED_BYTES as u32).to_le_bytes());
    payload.push(panel);
    payload.push(EMPTY_FINAL_CURSOR);
    debug_assert!(claim_payload_if_verified(&payload).is_some());
    payload
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<GuiInventoryClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.envelope != b'P'
        || high.major != GUI_INVENTORY_MAJOR
        || high.minor != SELECT_PANEL_MINOR
    {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != SELECT_PANEL_DECLARED_BYTES
        || payload.len() != declared + SELECT_PANEL_FRAGMENT_BYTES
        || payload.last().copied()? & FRAGMENT_CURSOR_MASK != EMPTY_FINAL_CURSOR
    {
        return None;
    }

    Some(GuiInventoryClaimSummary {
        packet_name: "GuiInventory_SelectPanel",
        panel: *payload.get(PANEL_OFFSET)?,
        declared,
        fragment_bytes: SELECT_PANEL_FRAGMENT_BYTES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_live_hg_select_panel_with_dirty_unowned_tail_bits() {
        let payload = [b'P', 0x0D, 0x02, 0x08, 0, 0, 0, 0x02, 0x62];

        let claim = claim_payload_if_verified(&payload)
            .expect("live server SelectPanel must claim the byte-only shape");

        assert_eq!(claim.packet_name, "GuiInventory_SelectPanel");
        assert_eq!(claim.panel, 0x02);
        assert_eq!(claim.declared, 8);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn builds_canonical_server_select_panel_shape() {
        let payload = build_select_panel_payload(0x04);

        assert_eq!(payload, [b'P', 0x0D, 0x02, 0x08, 0, 0, 0, 0x04, 0x60]);
        assert!(claim_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn rejects_client_bool_cursor_and_wrong_direction() {
        let client_cursor = [b'P', 0x0D, 0x02, 0x08, 0, 0, 0, 0x02, 0x80];
        let client_envelope = [0x70, 0x0D, 0x02, 0x08, 0, 0, 0, 0x02, 0x60];

        assert!(claim_payload_if_verified(&client_cursor).is_none());
        assert!(claim_payload_if_verified(&client_envelope).is_none());
    }

    #[test]
    fn rejects_declared_slack_and_missing_fragment_storage() {
        let slack = [b'P', 0x0D, 0x02, 0x09, 0, 0, 0, 0x02, 0x00, 0x60];
        let no_fragment = [b'P', 0x0D, 0x02, 0x08, 0, 0, 0, 0x02];

        assert!(claim_payload_if_verified(&slack).is_none());
        assert!(claim_payload_if_verified(&no_fragment).is_none());
    }
}
