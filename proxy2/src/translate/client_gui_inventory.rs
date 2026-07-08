//! Client-originated GUI inventory semantic claims.
//!
//! Decompile anchors:
//!
//! - EE `CNWSMessage::SendPlayerToServerGuiInventory_Status` writes one
//!   `BOOL` and then `WriteOBJECTIDServer`.
//! - EE `CNWSMessage::HandlePlayerToServerGuiInventoryMessage` reads minor
//!   `0x01` as the same `BOOL + OBJECTIDServer` shape and treats either the
//!   current creature id or `0x7F000000` as the self-inventory owner.
//! - EE `CNWSMessage::HandlePlayerToServerGuiInventoryMessage` reads minor
//!   `0x02` as `ReadBYTE(8, 1)` followed by one `ReadBOOL`, checks both
//!   overflow and underflow, then calls `CNWSPlayerInventoryGUI::SetPanel`
//!   (`nwn ee decompile.txt:1655147..1655178`). This is the observed
//!   `GuiInventory_SelectPanel` shape from the Starcore5 sign/placeable probe.
//! - Diamond/HG captures proved the EE harnessed self request
//!   `70 0D 01 0B 00 00 00 FD FF FF FF 90` must rewrite the object id to
//!   `0x7F000000` before the 1.69 server returns the GUI inventory stream.
//!
//! The BOOL lives in the single CNW fragment byte after the declared read
//! buffer. EE `CNWMessage::GetWriteMessage` stores the final fragment cursor in
//! the high three bits and may preserve low residual bits below that cursor, so
//! this translator validates the exact cursor plus owned data bit rather than
//! treating the entire trailing byte as meaningful. It rewrites only the EE self
//! sentinel for `Status`, and otherwise leaves validated bytes untouched.
//! `SelectPanel` is an identity translation, but still must be claimed here
//! before the router may emit it.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const GUI_INVENTORY_MAJOR: u8 = 0x0D;
const STATUS_MINOR: u8 = 0x01;
const SELECT_PANEL_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const STATUS_DECLARED_BYTES: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 4;
const STATUS_FRAGMENT_BYTES: usize = 1;
const STATUS_OBJECT_ID_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const SELECT_PANEL_DECLARED_BYTES: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 1;
const SELECT_PANEL_FRAGMENT_BYTES: usize = 1;
const SELECT_PANEL_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const EE_SELF_OBJECT_ID: u32 = 0xFFFF_FFFD;
pub const DIAMOND_CURRENT_PLAYER_OBJECT_ID: u32 = 0x7F00_0000;
const FRAGMENT_CURSOR_MASK: u8 = 0xE0;
const SINGLE_BOOL_FINAL_CURSOR: u8 = 0x80;
const SINGLE_BOOL_DATA_BIT: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientGuiInventoryKind {
    Status,
    SelectPanel,
}

#[derive(Debug, Clone, Copy)]
pub struct ClientGuiInventoryClaimSummary {
    pub packet_name: &'static str,
    pub kind: ClientGuiInventoryKind,
    pub object_id: Option<u32>,
    pub panel: Option<u8>,
    pub player_inventory_gui: Option<bool>,
    pub rewritten_self_object_id: bool,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientGuiInventoryClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != GUI_INVENTORY_MAJOR {
        return None;
    }
    match high.minor {
        STATUS_MINOR => claim_status_payload_if_verified(payload),
        SELECT_PANEL_MINOR => claim_select_panel_payload_if_verified(payload),
        _ => None,
    }
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut [u8],
) -> Option<ClientGuiInventoryClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != GUI_INVENTORY_MAJOR {
        return None;
    }
    match high.minor {
        STATUS_MINOR => claim_or_rewrite_status_payload_if_verified(payload),
        SELECT_PANEL_MINOR => claim_select_panel_payload_if_verified(payload),
        _ => None,
    }
}

pub fn build_status_payload(object_id: u32, player_inventory_gui: bool) -> Vec<u8> {
    let mut payload = Vec::with_capacity(STATUS_DECLARED_BYTES + STATUS_FRAGMENT_BYTES);
    payload.extend_from_slice(&[0x70, GUI_INVENTORY_MAJOR, STATUS_MINOR]);
    payload.extend_from_slice(&(STATUS_DECLARED_BYTES as u32).to_le_bytes());
    payload.extend_from_slice(&object_id.to_le_bytes());
    payload.push(encode_single_bool_fragment(player_inventory_gui));
    payload
}

pub fn build_select_panel_payload(panel: u8, player_inventory_gui: bool) -> Vec<u8> {
    let mut payload = Vec::with_capacity(SELECT_PANEL_DECLARED_BYTES + SELECT_PANEL_FRAGMENT_BYTES);
    payload.extend_from_slice(&[0x70, GUI_INVENTORY_MAJOR, SELECT_PANEL_MINOR]);
    payload.extend_from_slice(&(SELECT_PANEL_DECLARED_BYTES as u32).to_le_bytes());
    payload.push(panel);
    payload.push(encode_single_bool_fragment(player_inventory_gui));
    payload
}

fn claim_status_payload_if_verified(payload: &[u8]) -> Option<ClientGuiInventoryClaimSummary> {
    let object_id = read_status_object_id_if_verified(payload)?;
    Some(ClientGuiInventoryClaimSummary {
        packet_name: "GuiInventory_Status",
        kind: ClientGuiInventoryKind::Status,
        object_id: Some(object_id),
        panel: None,
        player_inventory_gui: None,
        rewritten_self_object_id: false,
    })
}

fn claim_or_rewrite_status_payload_if_verified(
    payload: &mut [u8],
) -> Option<ClientGuiInventoryClaimSummary> {
    let object_id = read_status_object_id_if_verified(payload)?;
    let rewritten_self_object_id = object_id == EE_SELF_OBJECT_ID;
    if rewritten_self_object_id {
        payload[STATUS_OBJECT_ID_OFFSET..STATUS_OBJECT_ID_OFFSET + 4]
            .copy_from_slice(&DIAMOND_CURRENT_PLAYER_OBJECT_ID.to_le_bytes());
    }

    Some(ClientGuiInventoryClaimSummary {
        packet_name: "GuiInventory_Status",
        kind: ClientGuiInventoryKind::Status,
        object_id: Some(if rewritten_self_object_id {
            DIAMOND_CURRENT_PLAYER_OBJECT_ID
        } else {
            object_id
        }),
        panel: None,
        player_inventory_gui: None,
        rewritten_self_object_id,
    })
}

fn claim_select_panel_payload_if_verified(
    payload: &[u8],
) -> Option<ClientGuiInventoryClaimSummary> {
    if !select_panel_payload_shape_valid(payload) {
        return None;
    }
    Some(ClientGuiInventoryClaimSummary {
        packet_name: "GuiInventory_SelectPanel",
        kind: ClientGuiInventoryKind::SelectPanel,
        object_id: None,
        panel: payload.get(SELECT_PANEL_OFFSET).copied(),
        player_inventory_gui: decode_single_bool_fragment(*payload.last()?),
        rewritten_self_object_id: false,
    })
}

fn read_status_object_id_if_verified(payload: &[u8]) -> Option<u32> {
    status_payload_shape_valid(payload).then_some(())?;
    read_le_u32(payload, STATUS_OBJECT_ID_OFFSET)
}

fn status_payload_shape_valid(payload: &[u8]) -> bool {
    read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)
        .and_then(|declared| usize::try_from(declared).ok())
        == Some(STATUS_DECLARED_BYTES)
        && payload.len() == STATUS_DECLARED_BYTES + STATUS_FRAGMENT_BYTES
        && read_le_u32(payload, STATUS_OBJECT_ID_OFFSET).is_some()
        && payload
            .last()
            .and_then(|byte| decode_single_bool_fragment(*byte))
            .is_some()
}

fn select_panel_payload_shape_valid(payload: &[u8]) -> bool {
    // EE `SendServerToPlayerInventory_SelectPanel` and the player-to-server
    // handler both own a one-byte read cursor plus one fragment BOOL. The same
    // packed single-BOOL fragment byte shape is used by `GuiInventory_Status`.
    read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)
        .and_then(|declared| usize::try_from(declared).ok())
        == Some(SELECT_PANEL_DECLARED_BYTES)
        && payload.len() == SELECT_PANEL_DECLARED_BYTES + SELECT_PANEL_FRAGMENT_BYTES
        && payload.get(SELECT_PANEL_OFFSET).is_some()
        && payload
            .last()
            .and_then(|byte| decode_single_bool_fragment(*byte))
            .is_some()
}

fn decode_single_bool_fragment(byte: u8) -> Option<bool> {
    if byte & FRAGMENT_CURSOR_MASK != SINGLE_BOOL_FINAL_CURSOR {
        return None;
    }
    Some(byte & SINGLE_BOOL_DATA_BIT != 0)
}

fn encode_single_bool_fragment(value: bool) -> u8 {
    SINGLE_BOOL_FINAL_CURSOR | if value { SINGLE_BOOL_DATA_BIT } else { 0 }
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
        assert_eq!(summary.kind, ClientGuiInventoryKind::Status);
        assert_eq!(summary.object_id, Some(DIAMOND_CURRENT_PLAYER_OBJECT_ID));
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

        assert_eq!(summary.object_id, Some(DIAMOND_CURRENT_PLAYER_OBJECT_ID));
        assert!(!summary.rewritten_self_object_id);
    }

    #[test]
    fn builds_exact_status_payload_for_current_player_inventory() {
        let payload = build_status_payload(DIAMOND_CURRENT_PLAYER_OBJECT_ID, true);

        assert_eq!(
            payload,
            vec![
                0x70, 0x0D, 0x01, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0x90
            ]
        );
        let summary = claim_payload_if_verified(&payload).expect("built payload should claim");
        assert_eq!(summary.kind, ClientGuiInventoryKind::Status);
        assert_eq!(summary.object_id, Some(DIAMOND_CURRENT_PLAYER_OBJECT_ID));
        assert!(!summary.rewritten_self_object_id);
    }

    #[test]
    fn builds_exact_status_payload_for_closed_inventory_bool() {
        let payload = build_status_payload(DIAMOND_CURRENT_PLAYER_OBJECT_ID, false);

        assert_eq!(payload[11], 0x80);
        let summary = claim_payload_if_verified(&payload).expect("built payload should claim");
        assert_eq!(summary.kind, ClientGuiInventoryKind::Status);
    }

    #[test]
    fn claims_inventory_status_with_residual_fragment_bits_below_final_cursor() {
        let mut payload = [
            0x70, 0x0D, 0x01, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, 0x98,
        ];

        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("single-BOOL cursor with residual low bits should be claimed");

        assert_eq!(summary.object_id, Some(DIAMOND_CURRENT_PLAYER_OBJECT_ID));
        assert!(!summary.rewritten_self_object_id);
        assert_eq!(
            payload[11], 0x98,
            "residual bits below the final cursor are not proxy-owned"
        );
    }

    #[test]
    fn rejects_inventory_status_with_wrong_declared_length() {
        let mut payload = [
            0x70, 0x0D, 0x01, 0x0A, 0x00, 0x00, 0x00, 0xFD, 0xFF, 0xFF, 0xFF, 0x90,
        ];

        assert!(claim_or_rewrite_payload_if_verified(&mut payload).is_none());
    }

    #[test]
    fn claims_select_panel_exact_single_byte_panel_and_fragment_bool() {
        let payload = [0x70, 0x0D, 0x02, 0x08, 0x00, 0x00, 0x00, 0x03, 0x90];

        let summary = claim_payload_if_verified(&payload)
            .expect("observed GuiInventory_SelectPanel shape should be claimed");

        assert_eq!(summary.packet_name, "GuiInventory_SelectPanel");
        assert_eq!(summary.kind, ClientGuiInventoryKind::SelectPanel);
        assert_eq!(summary.panel, Some(3));
        assert_eq!(summary.player_inventory_gui, Some(true));
    }

    #[test]
    fn builds_exact_select_panel_payload() {
        let payload = build_select_panel_payload(3, true);

        assert_eq!(
            payload,
            vec![0x70, 0x0D, 0x02, 0x08, 0x00, 0x00, 0x00, 0x03, 0x90]
        );
        let summary = claim_payload_if_verified(&payload).expect("built payload should claim");
        assert_eq!(summary.kind, ClientGuiInventoryKind::SelectPanel);
        assert_eq!(summary.panel, Some(3));
        assert_eq!(summary.player_inventory_gui, Some(true));
    }

    #[test]
    fn rejects_select_panel_without_exact_single_bool_fragment() {
        let payload = [0x70, 0x0D, 0x02, 0x08, 0x00, 0x00, 0x00, 0x03, 0xA0];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn claims_select_panel_with_residual_fragment_bits_below_final_cursor() {
        let payload = [0x70, 0x0D, 0x02, 0x08, 0x00, 0x00, 0x00, 0x03, 0x98];

        let summary = claim_payload_if_verified(&payload)
            .expect("single-BOOL cursor with residual low bits should be claimed");

        assert_eq!(summary.panel, Some(3));
        assert_eq!(summary.player_inventory_gui, Some(true));
    }
}
