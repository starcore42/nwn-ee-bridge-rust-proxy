//! Client-originated `Input` semantic claims.
//!
//! This module owns client-to-server family `0x06` packets only after an
//! exact CNW read-buffer and fragment-buffer parse. The current supported
//! packets are byte-identical between the EE client writer and the legacy
//! server reader, so the translation is intentionally an identity transform,
//! but it is not a raw passthrough: the router only emits these packets after
//! this parser consumes the same fields the game code reads.
//!
//! Decompile anchors:
//!
//! - EE client sender `sub_1407C0970` (`nwn ee decompile.txt:2942813`)
//!   writes `Input_WalkToWaypoint` as OBJECTID, three 32-bit FLOAT values,
//!   BYTE, BOOL, BOOL, BYTE, OBJECTID, then sends family `0x06`, minor `0x01`.
//! - EE server reader
//!   `CNWSMessage::HandlePlayerToServerInputWalkToWaypoint`
//!   (`nwn ee decompile.txt:1660119`) reads the same sequence and immediately
//!   checks `MessageReadOverflow` and `MessageReadUnderflow`.
//! - EE client sender `sub_1407BF860` (`nwn ee decompile.txt:2941144`)
//!   writes `Input_ChangeDoorState` as OBJECTID plus 16-bit WORD door state,
//!   then sends family `0x06`, minor `0x03`.
//! - EE server reader case `0x03` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657110`, case block at `0x140453BAE`) reads
//!   OBJECTID plus WORD, checks overflow/underflow, then maps state `0x0015`
//!   to open-door and all other states to close-door action.
//! - EE server reader case `0x0B` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1652827`, group-input case block at
//!   `0x140450081`) reads OBJECTID plus one fragment-buffer BOOL, checks
//!   overflow/underflow, optionally marks inventory GUI state from that BOOL,
//!   then calls `CNWSObject::AddUseObjectAction(uint)` with the object id.
//! - EE client sender `sub_1407C07C0` (`nwn ee decompile.txt:2942660`)
//!   writes `Input_UseObject` as OBJECTID followed by two `WriteBOOL` calls
//!   before sending family `0x06`, minor `0x0B`. The second writer BOOL is
//!   retained and validated as part of the EE sender envelope, but it is not a
//!   separate server-side use-object semantic field.
//!
//! The Diamond/1.69 text decompile available in this workspace is stripped for
//! these handler names. The live HG 1.69 server accepts the same CNW layouts
//! emitted by the EE sender and documented above; until a clearer Diamond
//! symbol anchor is recovered, this module keeps the claim conservative by
//! requiring exact cursor and fragment-bit consumption rather than allowing any
//! broader input-family shape.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const INPUT_MAJOR: u8 = 0x06;
const WALK_TO_WAYPOINT_MINOR: u8 = 0x01;
const CHANGE_DOOR_STATE_MINOR: u8 = 0x03;
const USE_OBJECT_MINOR: u8 = 0x0B;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_CURSOR_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const OBJECT_ID_BYTES: usize = 4;
const FLOAT_BYTES: usize = 4;
const WORD_BYTES: usize = 2;
const BYTE_BYTES: usize = 1;
const FRAGMENT_HEADER_BITS: usize = 3;
const WALK_BOOL_BITS: usize = 2;
const USE_OBJECT_EE_WRITER_BOOL_BITS: usize = 2;
const WALK_READ_BODY_BYTES: usize =
    OBJECT_ID_BYTES + (3 * FLOAT_BYTES) + BYTE_BYTES + BYTE_BYTES + OBJECT_ID_BYTES;
const DOOR_READ_BODY_BYTES: usize = OBJECT_ID_BYTES + WORD_BYTES;
const USE_OBJECT_READ_BODY_BYTES: usize = OBJECT_ID_BYTES;
const WALK_DECLARED_BYTES: usize = READ_CURSOR_START + WALK_READ_BODY_BYTES;
const DOOR_DECLARED_BYTES: usize = READ_CURSOR_START + DOOR_READ_BODY_BYTES;
const USE_OBJECT_DECLARED_BYTES: usize = READ_CURSOR_START + USE_OBJECT_READ_BODY_BYTES;
const ONE_FRAGMENT_BYTE: usize = 1;
const INVALID_OBJECT_ID: u32 = 0x7F00_0000;

#[derive(Debug, Clone, Copy)]
pub struct ClientInputClaimSummary {
    pub packet_name: &'static str,
    pub kind: ClientInputKind,
    pub declared: usize,
    pub fragment_bytes: usize,
    pub primary_object_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientInputKind {
    WalkToWaypoint,
    ChangeDoorState,
    UseObject,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalkToWaypoint {
    pub area_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub input_byte: u8,
    pub first_bool: bool,
    pub second_bool: bool,
    pub action_byte: u8,
    pub action_object_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeDoorState {
    pub door_id: u32,
    pub state: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseObject {
    pub object_id: u32,
    pub server_consumed_bool: bool,
    pub ee_writer_aux_bool: bool,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientInputClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR {
        return None;
    }

    match high.minor {
        WALK_TO_WAYPOINT_MINOR => {
            let walk = parse_walk_to_waypoint(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_WalkToWaypoint",
                kind: ClientInputKind::WalkToWaypoint,
                declared: WALK_DECLARED_BYTES,
                fragment_bytes: payload.len() - WALK_DECLARED_BYTES,
                primary_object_id: walk.area_id,
            })
        }
        CHANGE_DOOR_STATE_MINOR => {
            let door = parse_change_door_state(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_ChangeDoorState",
                kind: ClientInputKind::ChangeDoorState,
                declared: DOOR_DECLARED_BYTES,
                fragment_bytes: payload.len() - DOOR_DECLARED_BYTES,
                primary_object_id: door.door_id,
            })
        }
        USE_OBJECT_MINOR => {
            let use_object = parse_use_object(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_UseObject",
                kind: ClientInputKind::UseObject,
                declared: USE_OBJECT_DECLARED_BYTES,
                fragment_bytes: payload.len() - USE_OBJECT_DECLARED_BYTES,
                primary_object_id: use_object.object_id,
            })
        }
        _ => None,
    }
}

pub fn parse_walk_to_waypoint(payload: &[u8]) -> Option<WalkToWaypoint> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != WALK_TO_WAYPOINT_MINOR {
        return None;
    }
    require_declared_and_fragment_bits(payload, WALK_DECLARED_BYTES, WALK_BOOL_BITS)?;

    let mut cursor = READ_CURSOR_START;
    let area_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;
    let x = read_f32_at(payload, cursor)?;
    cursor += FLOAT_BYTES;
    let y = read_f32_at(payload, cursor)?;
    cursor += FLOAT_BYTES;
    let z = read_f32_at(payload, cursor)?;
    cursor += FLOAT_BYTES;
    let input_byte = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let (first_bool, second_bool) = read_two_fragment_bools(&payload[WALK_DECLARED_BYTES..])?;
    let action_byte = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let action_object_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;

    if cursor != WALK_DECLARED_BYTES || area_id == INVALID_OBJECT_ID {
        return None;
    }

    Some(WalkToWaypoint {
        area_id,
        x,
        y,
        z,
        input_byte,
        first_bool,
        second_bool,
        action_byte,
        action_object_id,
    })
}

pub fn parse_change_door_state(payload: &[u8]) -> Option<ChangeDoorState> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != CHANGE_DOOR_STATE_MINOR {
        return None;
    }
    require_declared_and_fragment_bits(payload, DOOR_DECLARED_BYTES, 0)?;

    let mut cursor = READ_CURSOR_START;
    let door_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;
    let state = read_u16_at(payload, cursor)?;
    cursor += WORD_BYTES;

    if cursor != DOOR_DECLARED_BYTES || door_id == INVALID_OBJECT_ID {
        return None;
    }

    Some(ChangeDoorState { door_id, state })
}

pub fn parse_use_object(payload: &[u8]) -> Option<UseObject> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != USE_OBJECT_MINOR {
        return None;
    }
    require_declared_and_fragment_bits(
        payload,
        USE_OBJECT_DECLARED_BYTES,
        USE_OBJECT_EE_WRITER_BOOL_BITS,
    )?;

    let mut cursor = READ_CURSOR_START;
    let object_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;
    let (server_consumed_bool, ee_writer_aux_bool) =
        read_two_fragment_bools(&payload[USE_OBJECT_DECLARED_BYTES..])?;

    if cursor != USE_OBJECT_DECLARED_BYTES || object_id == INVALID_OBJECT_ID {
        return None;
    }

    Some(UseObject {
        object_id,
        server_consumed_bool,
        ee_writer_aux_bool,
    })
}

fn require_declared_and_fragment_bits(
    payload: &[u8],
    expected_declared: usize,
    data_bits: usize,
) -> Option<()> {
    if payload.len() != expected_declared + ONE_FRAGMENT_BYTE {
        return None;
    }
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != expected_declared {
        return None;
    }
    let fragment = *payload.get(expected_declared)?;
    let final_fragment_bits = usize::from((fragment & 0x80) != 0) << 2
        | usize::from((fragment & 0x40) != 0) << 1
        | usize::from((fragment & 0x20) != 0);
    if final_fragment_bits != FRAGMENT_HEADER_BITS + data_bits {
        return None;
    }
    Some(())
}

fn read_two_fragment_bools(fragments: &[u8]) -> Option<(bool, bool)> {
    let fragment = *fragments.first()?;
    let first = ((fragment >> (7 - FRAGMENT_HEADER_BITS)) & 1) != 0;
    let second = ((fragment >> (7 - FRAGMENT_HEADER_BITS - 1)) & 1) != 0;
    Some((first, second))
}

fn read_u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let pair = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]))
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    read_le_u32(bytes, offset)
}

fn read_f32_at(bytes: &[u8], offset: usize) -> Option<f32> {
    let raw = read_u32_at(bytes, offset)?;
    Some(f32::from_bits(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_to_waypoint_fixture_matches_decompile_cursor_shape() {
        let fixture =
            include_bytes!("../../fixtures/client_input/walk_to_waypoint_transition_click.bin");
        let summary = claim_payload_if_verified(fixture).expect("walk fixture should be claimed");
        let parsed = parse_walk_to_waypoint(fixture).expect("walk fixture should parse");

        assert_eq!(summary.kind, ClientInputKind::WalkToWaypoint);
        assert_eq!(summary.packet_name, "Input_WalkToWaypoint");
        assert_eq!(summary.declared, WALK_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(parsed.area_id, 0x8000_34CB);
        assert_eq!(parsed.input_byte, 0);
        assert!(parsed.first_bool);
        assert!(!parsed.second_bool);
        assert_eq!(parsed.action_byte, 0);
        assert_eq!(parsed.action_object_id, INVALID_OBJECT_ID);
    }

    #[test]
    fn walk_to_waypoint_object_click_fixtures_match_decompile_cursor_shape() {
        let door =
            include_bytes!("../../fixtures/client_input/walk_to_waypoint_door_action_click.bin");
        let trigger =
            include_bytes!("../../fixtures/client_input/walk_to_waypoint_trigger_action_click.bin");

        let door = parse_walk_to_waypoint(door).expect("door click fixture should parse");
        assert_eq!(door.area_id, 0x8000_34CB);
        assert_eq!(door.input_byte, 0);
        assert!(door.first_bool);
        assert!(!door.second_bool);
        assert_eq!(door.action_byte, 0);
        assert_eq!(door.action_object_id, 0x8000_34D1);

        let trigger =
            parse_walk_to_waypoint(trigger).expect("trigger click fixture should parse");
        assert_eq!(trigger.area_id, 0x8000_34CB);
        assert_eq!(trigger.input_byte, 0);
        assert!(trigger.first_bool);
        assert!(!trigger.second_bool);
        assert_eq!(trigger.action_byte, 0);
        assert_eq!(trigger.action_object_id, 0x8000_35FD);
    }

    #[test]
    fn change_door_state_fixture_matches_decompile_cursor_shape() {
        let fixture = include_bytes!("../../fixtures/client_input/change_door_state_open.bin");
        let summary = claim_payload_if_verified(fixture).expect("door fixture should be claimed");
        let parsed = parse_change_door_state(fixture).expect("door fixture should parse");

        assert_eq!(summary.kind, ClientInputKind::ChangeDoorState);
        assert_eq!(summary.packet_name, "Input_ChangeDoorState");
        assert_eq!(summary.declared, DOOR_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(parsed.door_id, 0x8000_F541);
        assert_eq!(parsed.state, 0x0015);
    }

    #[test]
    fn use_object_fixture_matches_decompile_cursor_shape() {
        let fixture = include_bytes!("../../fixtures/client_input/use_object_placeable.bin");
        let summary =
            claim_payload_if_verified(fixture).expect("use-object fixture should be claimed");
        let parsed = parse_use_object(fixture).expect("use-object fixture should parse");

        assert_eq!(summary.kind, ClientInputKind::UseObject);
        assert_eq!(summary.packet_name, "Input_UseObject");
        assert_eq!(summary.declared, USE_OBJECT_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(parsed.object_id, 0x8000_34CB);
        assert!(parsed.server_consumed_bool);
        assert!(!parsed.ee_writer_aux_bool);
    }

    #[test]
    fn input_family_rejects_trailing_or_wrong_fragment_shapes() {
        let mut walk =
            include_bytes!("../../fixtures/client_input/walk_to_waypoint_transition_click.bin")
                .to_vec();
        walk.push(0);
        assert!(claim_payload_if_verified(&walk).is_none());

        let mut door =
            include_bytes!("../../fixtures/client_input/change_door_state_open.bin").to_vec();
        door[13] = 0x80;
        assert!(claim_payload_if_verified(&door).is_none());

        let mut use_object =
            include_bytes!("../../fixtures/client_input/use_object_placeable.bin").to_vec();
        use_object[7..11].copy_from_slice(&INVALID_OBJECT_ID.to_le_bytes());
        assert!(claim_payload_if_verified(&use_object).is_none());
    }
}
