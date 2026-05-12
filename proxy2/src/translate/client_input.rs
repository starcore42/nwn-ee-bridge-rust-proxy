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
//! - EE server reader case `0x05` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657730`, case block at `0x140453951`) reads
//!   one OBJECTID and immediately checks overflow/underflow.
//! - EE server reader case `0x09` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657870`, case block at `0x1404539E9`) reads
//!   OBJECTID, BYTE, BOOL+optional BYTE, BOOL+optional OBJECTID, and
//!   BOOL+optional three FLOAT values before checking overflow/underflow.
//!   The legacy HG path expects EE's self target sentinel `0xFFFFFFFD` to be
//!   translated to Diamond's invalid/self target sentinel `0x7F000000` only
//!   for the optional target OBJECTID field.
//! - EE server reader case `0x0B` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657740`, case block at `0x140453ACB`) reads
//!   OBJECTID plus two fragment-buffer BOOLs, checks overflow/underflow,
//!   optionally schedules the second-BOOL script event path, optionally marks
//!   inventory GUI state from the first BOOL, then calls
//!   `CNWSObject::AddUseObjectAction(uint)` with the object id.
//! - EE client sender `sub_1407C07C0` (`nwn ee decompile.txt:2942660`)
//!   writes `Input_UseObject` as OBJECTID followed by two `WriteBOOL` calls
//!   before sending family `0x06`, minor `0x0B`.
//!
//! The Diamond/1.69 text decompile available in this workspace is stripped for
//! these handler names. The live HG 1.69 server accepts the same CNW layouts
//! emitted by the EE sender and documented above; until a clearer Diamond
//! symbol anchor is recovered, this module keeps the claim conservative by
//! requiring exact cursor and fragment-bit consumption rather than allowing any
//! broader input-family shape.
//! - The old in-process bridge had to translate a second click on an already
//!   opened transition door from `Input_ChangeDoorState(close)` into a
//!   server-owned transition action. When the verified live-object registry
//!   proves a nearby transition-named trigger/placeable, the proxy emits the
//!   exact `Input_WalkToWaypoint` shape above. When the door is verified but no
//!   transition anchor has been observed on the wire, the proxy still emits the
//!   exact walk-to-door shape using the verified door position and current area.
//!   The EE server decompile shows `Input_ChangeDoorState(close)` only queues
//!   `AddCloseDoorAction`; `HandlePlayerToServerInputWalkToWaypoint` owns the
//!   movement/transition path. The legacy server remains authoritative: the
//!   proxy only chooses the decompile-owned input shape, not the game outcome.

use std::time::{Duration, Instant};

use crate::{
    crc::read_le_u32,
    packet::m::HighLevel,
    translate::semantic::SemanticSessionState,
};

const INPUT_MAJOR: u8 = 0x06;
const WALK_TO_WAYPOINT_MINOR: u8 = 0x01;
const CHANGE_DOOR_STATE_MINOR: u8 = 0x03;
const EXAMINE_MINOR: u8 = 0x05;
const USE_ITEM_MINOR: u8 = 0x09;
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
const USE_ITEM_BOOL_BITS: usize = 3;
const USE_OBJECT_EE_WRITER_BOOL_BITS: usize = 2;
const WALK_READ_BODY_BYTES: usize =
    OBJECT_ID_BYTES + (3 * FLOAT_BYTES) + BYTE_BYTES + BYTE_BYTES + OBJECT_ID_BYTES;
const DOOR_READ_BODY_BYTES: usize = OBJECT_ID_BYTES + WORD_BYTES;
const EXAMINE_READ_BODY_BYTES: usize = OBJECT_ID_BYTES;
const USE_ITEM_BASE_READ_BODY_BYTES: usize = OBJECT_ID_BYTES + BYTE_BYTES;
const USE_OBJECT_READ_BODY_BYTES: usize = OBJECT_ID_BYTES;
const WALK_DECLARED_BYTES: usize = READ_CURSOR_START + WALK_READ_BODY_BYTES;
const DOOR_DECLARED_BYTES: usize = READ_CURSOR_START + DOOR_READ_BODY_BYTES;
const EXAMINE_DECLARED_BYTES: usize = READ_CURSOR_START + EXAMINE_READ_BODY_BYTES;
const USE_ITEM_MIN_DECLARED_BYTES: usize = READ_CURSOR_START + USE_ITEM_BASE_READ_BODY_BYTES;
const USE_OBJECT_DECLARED_BYTES: usize = READ_CURSOR_START + USE_OBJECT_READ_BODY_BYTES;
const ONE_FRAGMENT_BYTE: usize = 1;
const EE_SELF_OBJECT_ID: u32 = 0xFFFF_FFFD;
const INVALID_OBJECT_ID: u32 = 0x7F00_0000;
const DOOR_OPEN_STATE: u16 = 0x0015;
const RECENT_TRANSITION_DOOR_OPEN_WINDOW: Duration = Duration::from_secs(45);
const DOOR_OBJECT_TYPE: u8 = 0x0A;
// EE's client writer and the driver-only transition-click capture both emit
// client gameplay input frames with this high-level envelope byte.  The proxy
// must preserve the same envelope when it replaces a proven
// `Input_ChangeDoorState(close)` with the decompile-owned
// `Input_WalkToWaypoint` shape; emitting ASCII `P` (`0x50`) produces a
// CRC/length-valid packet, but it is not the same client dialect shape the
// legacy server receives from a real transition click.
const CLIENT_INPUT_ENVELOPE: u8 = 0x70;

#[derive(Debug, Clone, Copy)]
pub struct ClientInputClaimSummary {
    pub packet_name: &'static str,
    pub kind: ClientInputKind,
    pub declared: usize,
    pub fragment_bytes: usize,
    pub primary_object_id: u32,
    pub rewritten_self_object_id: bool,
    pub rewritten_transition_door_close: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientInputKind {
    WalkToWaypoint,
    ChangeDoorState,
    Examine,
    UseItem,
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
pub struct Examine {
    pub object_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseItem {
    pub item_object_id: u32,
    pub active_property_subtype: u8,
    pub optional_byte: Option<u8>,
    pub target_object_id: Option<u32>,
    pub target_object_offset: Option<usize>,
    pub position: Option<(f32, f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseObject {
    pub object_id: u32,
    pub mark_inventory_gui_state: bool,
    pub schedule_script_event: bool,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientInputClaimSummary> {
    claim_payload_inner(payload, false)
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut Vec<u8>,
) -> Option<ClientInputClaimSummary> {
    claim_or_rewrite_payload_inner(payload, None)
}

pub(crate) fn claim_or_rewrite_payload_if_verified_with_state(
    payload: &mut Vec<u8>,
    state: &mut SemanticSessionState,
) -> Option<ClientInputClaimSummary> {
    claim_or_rewrite_payload_inner(payload, Some(state))
}

fn claim_or_rewrite_payload_inner(
    payload: &mut Vec<u8>,
    mut state: Option<&mut SemanticSessionState>,
) -> Option<ClientInputClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major == INPUT_MAJOR && high.minor == CHANGE_DOOR_STATE_MINOR {
        if let Some(state) = state.as_deref_mut() {
            if let Some(summary) = claim_or_rewrite_transition_door_close(payload, state) {
                return Some(summary);
            }
        }
    }
    if high.major == INPUT_MAJOR && high.minor == USE_ITEM_MINOR {
        let use_item = parse_use_item(payload)?;
        let rewritten_self_object_id = if use_item.target_object_id == Some(EE_SELF_OBJECT_ID) {
            let offset = use_item.target_object_offset?;
            payload
                .get_mut(offset..offset.checked_add(OBJECT_ID_BYTES)?)?
                .copy_from_slice(&INVALID_OBJECT_ID.to_le_bytes());
            true
        } else {
            false
        };
        return Some(ClientInputClaimSummary {
            packet_name: "Input_UseItem",
            kind: ClientInputKind::UseItem,
            declared: read_declared(payload)?,
            fragment_bytes: ONE_FRAGMENT_BYTE,
            primary_object_id: use_item.item_object_id,
            rewritten_self_object_id,
            rewritten_transition_door_close: false,
        });
    }

    claim_payload_inner(payload, false)
}

fn claim_payload_inner(
    payload: &[u8],
    rewritten_self_object_id: bool,
) -> Option<ClientInputClaimSummary> {
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
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
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
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        EXAMINE_MINOR => {
            let examine = parse_examine(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_Examine",
                kind: ClientInputKind::Examine,
                declared: EXAMINE_DECLARED_BYTES,
                fragment_bytes: payload.len() - EXAMINE_DECLARED_BYTES,
                primary_object_id: examine.object_id,
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        USE_ITEM_MINOR => {
            let use_item = parse_use_item(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_UseItem",
                kind: ClientInputKind::UseItem,
                declared: read_declared(payload)?,
                fragment_bytes: ONE_FRAGMENT_BYTE,
                primary_object_id: use_item.item_object_id,
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
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
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        _ => None,
    }
}

fn claim_or_rewrite_transition_door_close(
    payload: &mut Vec<u8>,
    state: &mut SemanticSessionState,
) -> Option<ClientInputClaimSummary> {
    let door = parse_change_door_state(payload)?;
    let now = Instant::now();
    if door.state == DOOR_OPEN_STATE {
        state.client_input.recent_open_door_id = Some(door.door_id);
        state.client_input.recent_open_at = Some(now);
        tracing::info!(
            door_id = format_args!("0x{:08X}", door.door_id),
            state = format_args!("0x{:04X}", door.state),
            "client Input_ChangeDoorState open remembered for transition-door compatibility"
        );
        return None;
    }

    let recent_open = state.client_input.recent_open_door_id == Some(door.door_id)
        && state
            .client_input
            .recent_open_at
            .is_some_and(|opened_at| now.duration_since(opened_at) <= RECENT_TRANSITION_DOOR_OPEN_WINDOW);
    if !recent_open {
        return None;
    }

    let Some(area_id) = state.area.current_area_object_id else {
        state.client_input.transition_door_close_rewrite_skips =
            state.client_input.transition_door_close_rewrite_skips.saturating_add(1);
        tracing::debug!(
            door_id = format_args!("0x{:08X}", door.door_id),
            requested_state = format_args!("0x{:04X}", door.state),
            reason = "missing-current-area",
            "client Input_ChangeDoorState transition-door close rewrite skipped"
        );
        return None;
    };
    let Some(door_object) = state.objects.get(DOOR_OBJECT_TYPE, door.door_id) else {
        state.client_input.transition_door_close_rewrite_skips =
            state.client_input.transition_door_close_rewrite_skips.saturating_add(1);
        tracing::debug!(
            door_id = format_args!("0x{:08X}", door.door_id),
            requested_state = format_args!("0x{:04X}", door.state),
            area_id = format_args!("0x{:08X}", area_id),
            reason = "door-not-in-live-object-registry",
            known_objects = state.objects.known.len(),
            "client Input_ChangeDoorState transition-door close rewrite skipped"
        );
        return None;
    };
    let Some(door_position) = door_object.position else {
        state.client_input.transition_door_close_rewrite_skips =
            state.client_input.transition_door_close_rewrite_skips.saturating_add(1);
        tracing::debug!(
            door_id = format_args!("0x{:08X}", door.door_id),
            requested_state = format_args!("0x{:04X}", door.state),
            area_id = format_args!("0x{:08X}", area_id),
            door_active = door_object.active,
            door_name = door_object.latest_name.as_deref().unwrap_or(""),
            door_orientation_tenths = ?door_object
                .orientation
                .map(|orientation| orientation.scalar_tenths_degrees),
            reason = "door-position-not-yet-observed",
            "client Input_ChangeDoorState transition-door close rewrite skipped"
        );
        return None;
    };
    let Some(anchor) = state.objects.nearby_transition_anchor_for_door(door.door_id) else {
        let rewritten = build_transition_door_walk_payload(
            area_id,
            door.door_id,
            door_position.x,
            door_position.y,
            door_position.z,
        )?;
        let parsed = parse_walk_to_waypoint(&rewritten)?;
        *payload = rewritten;
        state.client_input.recent_open_door_id = None;
        state.client_input.recent_open_at = None;
        state.client_input.transition_door_close_rewrites =
            state.client_input.transition_door_close_rewrites.saturating_add(1);
        tracing::info!(
            door_id = format_args!("0x{:08X}", door.door_id),
            requested_state = format_args!("0x{:04X}", door.state),
            area_id = format_args!("0x{:08X}", area_id),
            x = door_position.x,
            y = door_position.y,
            z = door_position.z,
            door_active = door_object.active,
            door_name = door_object.latest_name.as_deref().unwrap_or(""),
            door_orientation_tenths = ?door_object
                .orientation
                .map(|orientation| orientation.scalar_tenths_degrees),
            reason = "no-nearby-transition-anchor-door-position-walk",
            known_objects = state.objects.known.len(),
            "client Input_ChangeDoorState transition-door close rewritten to Input_WalkToWaypoint"
        );
        return Some(ClientInputClaimSummary {
            packet_name: "Input_WalkToWaypoint",
            kind: ClientInputKind::WalkToWaypoint,
            declared: WALK_DECLARED_BYTES,
            fragment_bytes: ONE_FRAGMENT_BYTE,
            primary_object_id: parsed.area_id,
            rewritten_self_object_id: false,
            rewritten_transition_door_close: true,
        });
    };

    let rewritten = build_transition_door_walk_payload(
        area_id,
        door.door_id,
        door_position.x,
        door_position.y,
        door_position.z,
    )?;
    let parsed = parse_walk_to_waypoint(&rewritten)?;
    *payload = rewritten;
    state.client_input.recent_open_door_id = None;
    state.client_input.recent_open_at = None;
    state.client_input.transition_door_close_rewrites =
        state.client_input.transition_door_close_rewrites.saturating_add(1);
    tracing::info!(
        door_id = format_args!("0x{:08X}", door.door_id),
        requested_state = format_args!("0x{:04X}", door.state),
        area_id = format_args!("0x{:08X}", area_id),
        x = door_position.x,
        y = door_position.y,
        z = if door_position.z == 0.0 { 0.002 } else { door_position.z },
        anchor_id = format_args!("0x{:08X}", anchor.object_id),
        anchor_type = anchor.object_type,
        anchor_name = anchor.name,
        anchor_distance = anchor.distance,
        "client Input_ChangeDoorState transition-door close rewritten to Input_WalkToWaypoint"
    );
    Some(ClientInputClaimSummary {
        packet_name: "Input_WalkToWaypoint",
        kind: ClientInputKind::WalkToWaypoint,
        declared: WALK_DECLARED_BYTES,
        fragment_bytes: ONE_FRAGMENT_BYTE,
        primary_object_id: parsed.area_id,
        rewritten_self_object_id: false,
        rewritten_transition_door_close: true,
    })
}

fn build_transition_door_walk_payload(
    area_id: u32,
    door_id: u32,
    x: f32,
    y: f32,
    z: f32,
) -> Option<Vec<u8>> {
    if area_id == INVALID_OBJECT_ID || door_id == INVALID_OBJECT_ID {
        return None;
    }
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }

    let mut payload = Vec::with_capacity(WALK_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
    payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, WALK_TO_WAYPOINT_MINOR]);
    payload.extend_from_slice(&(WALK_DECLARED_BYTES as u32).to_le_bytes());
    payload.extend_from_slice(&area_id.to_le_bytes());
    payload.extend_from_slice(&x.to_le_bytes());
    payload.extend_from_slice(&y.to_le_bytes());
    payload.extend_from_slice(&(if z == 0.0 { 0.002 } else { z }).to_le_bytes());
    payload.push(0);
    payload.push(0);
    payload.extend_from_slice(&door_id.to_le_bytes());
    // Final fragment bits = 5 (three CNW fragment header bits + two BOOLs).
    // The data BOOLs are true,false, matching the captured EE writer shape for
    // a transition-click `Input_WalkToWaypoint`.
    payload.push(0xB0);
    parse_walk_to_waypoint(&payload)?;
    Some(payload)
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

pub fn parse_examine(payload: &[u8]) -> Option<Examine> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != EXAMINE_MINOR {
        return None;
    }
    require_declared_and_fragment_bits(payload, EXAMINE_DECLARED_BYTES, 0)?;

    let mut cursor = READ_CURSOR_START;
    let object_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;

    if cursor != EXAMINE_DECLARED_BYTES || object_id == INVALID_OBJECT_ID {
        return None;
    }

    Some(Examine { object_id })
}

pub fn parse_use_item(payload: &[u8]) -> Option<UseItem> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != USE_ITEM_MINOR {
        return None;
    }
    let declared = read_declared(payload)?;
    if declared < USE_ITEM_MIN_DECLARED_BYTES {
        return None;
    }
    require_declared_and_fragment_bits(payload, declared, USE_ITEM_BOOL_BITS)?;

    let fragment_tail = &payload[declared..];
    let mut cursor = READ_CURSOR_START;
    let item_object_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;
    let active_property_subtype = *payload.get(cursor)?;
    cursor += BYTE_BYTES;

    let has_optional_byte = read_fragment_bool(fragment_tail, 0)?;
    let optional_byte = if has_optional_byte {
        let value = *payload.get(cursor)?;
        cursor += BYTE_BYTES;
        Some(value)
    } else {
        None
    };

    let has_target_object = read_fragment_bool(fragment_tail, 1)?;
    let (target_object_id, target_object_offset) = if has_target_object {
        let offset = cursor;
        let value = read_u32_at(payload, cursor)?;
        cursor += OBJECT_ID_BYTES;
        (Some(value), Some(offset))
    } else {
        (None, None)
    };

    let has_position = read_fragment_bool(fragment_tail, 2)?;
    let position = if has_position {
        let x = read_f32_at(payload, cursor)?;
        cursor += FLOAT_BYTES;
        let y = read_f32_at(payload, cursor)?;
        cursor += FLOAT_BYTES;
        let z = read_f32_at(payload, cursor)?;
        cursor += FLOAT_BYTES;
        Some((x, y, z))
    } else {
        None
    };

    if cursor != declared || item_object_id == INVALID_OBJECT_ID {
        return None;
    }

    Some(UseItem {
        item_object_id,
        active_property_subtype,
        optional_byte,
        target_object_id,
        target_object_offset,
        position,
    })
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
    let (mark_inventory_gui_state, schedule_script_event) =
        read_two_fragment_bools(&payload[USE_OBJECT_DECLARED_BYTES..])?;

    if cursor != USE_OBJECT_DECLARED_BYTES || object_id == INVALID_OBJECT_ID {
        return None;
    }

    Some(UseObject {
        object_id,
        mark_inventory_gui_state,
        schedule_script_event,
    })
}

fn read_declared(payload: &[u8]) -> Option<usize> {
    usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()
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
    let first = read_fragment_bool(fragments, 0)?;
    let second = read_fragment_bool(fragments, 1)?;
    Some((first, second))
}

fn read_fragment_bool(fragments: &[u8], data_bit_index: usize) -> Option<bool> {
    let fragment = *fragments.first()?;
    let bit_index = FRAGMENT_HEADER_BITS.checked_add(data_bit_index)?;
    let shift = 7usize.checked_sub(bit_index)?;
    Some(((fragment >> shift) & 1) != 0)
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
    use crate::translate::semantic::{LiveObjectMention, LiveObjectPosition, SemanticSessionState};

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
        assert!(!summary.rewritten_self_object_id);
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

        assert_eq!(door[0], CLIENT_INPUT_ENVELOPE);
        let door = parse_walk_to_waypoint(door).expect("door click fixture should parse");
        assert_eq!(door.area_id, 0x8000_34CB);
        assert_eq!(door.input_byte, 0);
        assert!(door.first_bool);
        assert!(!door.second_bool);
        assert_eq!(door.action_byte, 0);
        assert_eq!(door.action_object_id, 0x8000_34D1);

        assert_eq!(trigger[0], CLIENT_INPUT_ENVELOPE);
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
        assert!(!summary.rewritten_self_object_id);
        assert_eq!(parsed.door_id, 0x8000_F541);
        assert_eq!(parsed.state, 0x0015);
    }

    #[test]
    fn examine_fixture_matches_decompile_cursor_shape() {
        let fixture = include_bytes!("../../fixtures/client_input/examine_item.bin");
        let summary = claim_payload_if_verified(fixture).expect("examine fixture should be claimed");
        let parsed = parse_examine(fixture).expect("examine fixture should parse");

        assert_eq!(summary.kind, ClientInputKind::Examine);
        assert_eq!(summary.packet_name, "Input_Examine");
        assert_eq!(summary.declared, EXAMINE_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert!(!summary.rewritten_self_object_id);
        assert_eq!(parsed.object_id, 0x8001_6012);
    }

    #[test]
    fn use_item_fixture_rewrites_only_target_self_sentinel() {
        let fixture = include_bytes!("../../fixtures/client_input/use_item_self_target.bin");
        let parsed = parse_use_item(fixture).expect("use-item fixture should parse before rewrite");
        assert_eq!(parsed.item_object_id, 0x8001_6012);
        assert_eq!(parsed.active_property_subtype, 0x00);
        assert_eq!(parsed.optional_byte, None);
        assert_eq!(parsed.target_object_id, Some(EE_SELF_OBJECT_ID));
        assert_eq!(parsed.position, None);

        let mut rewritten = fixture.to_vec();
        let summary = claim_or_rewrite_payload_if_verified(&mut rewritten)
            .expect("use-item fixture should be claimed and rewritten");
        let rewritten_parsed =
            parse_use_item(&rewritten).expect("rewritten use-item fixture should still parse");

        assert_eq!(summary.kind, ClientInputKind::UseItem);
        assert_eq!(summary.packet_name, "Input_UseItem");
        assert_eq!(summary.declared, 16);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8001_6012);
        assert!(summary.rewritten_self_object_id);
        assert_eq!(rewritten_parsed.target_object_id, Some(INVALID_OBJECT_ID));
        assert_eq!(&rewritten[12..16], &INVALID_OBJECT_ID.to_le_bytes());
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
        assert!(!summary.rewritten_self_object_id);
        assert_eq!(parsed.object_id, 0x8000_34CB);
        assert!(parsed.mark_inventory_gui_state);
        assert!(!parsed.schedule_script_event);
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

        let mut use_item =
            include_bytes!("../../fixtures/client_input/use_item_self_target.bin").to_vec();
        use_item[16] = 0xB0;
        assert!(claim_payload_if_verified(&use_item).is_none());
    }

    #[test]
    fn transition_door_close_rewrites_to_decompile_owned_walk_payload() {
        let mut state = SemanticSessionState::default();
        state.area.current_area_object_id = Some(0x8000_34CB);
        state.objects.observe_mentions(&[
            LiveObjectMention {
                opcode: b'A',
                object_type: DOOR_OBJECT_TYPE,
                object_id: 0x8000_34D1,
                name: Some("Door".to_string()),
                position: None,
                orientation: None,
                bounds: None,
            },
            LiveObjectMention {
                opcode: b'U',
                object_type: DOOR_OBJECT_TYPE,
                object_id: 0x8000_34D1,
                name: None,
                position: Some(LiveObjectPosition {
                    x: 47.50,
                    y: 43.08,
                    z: 0.0,
                }),
                orientation: None,
                bounds: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: 0x8000_3566,
                name: Some("The Sooty Crow".to_string()),
                position: None,
                orientation: None,
                bounds: None,
            },
            LiveObjectMention {
                opcode: b'U',
                object_type: 0x09,
                object_id: 0x8000_3566,
                name: None,
                position: Some(LiveObjectPosition {
                    x: 45.15,
                    y: 41.89,
                    z: 0.0,
                }),
                orientation: None,
                bounds: None,
            },
        ]);

        let mut open = build_change_door_payload(0x8000_34D1, DOOR_OPEN_STATE);
        let open_summary =
            claim_or_rewrite_payload_if_verified_with_state(&mut open, &mut state)
                .expect("open door packet should be exact-claimed");
        assert_eq!(open_summary.kind, ClientInputKind::ChangeDoorState);
        assert!(!open_summary.rewritten_transition_door_close);

        let mut close = build_change_door_payload(0x8000_34D1, 0x0016);
        let close_summary =
            claim_or_rewrite_payload_if_verified_with_state(&mut close, &mut state)
                .expect("transition door close should rewrite to a walk packet");
        let walk = parse_walk_to_waypoint(&close).expect("rewritten packet should parse");
        assert_eq!(close_summary.kind, ClientInputKind::WalkToWaypoint);
        assert!(close_summary.rewritten_transition_door_close);
        assert_eq!(close[0], CLIENT_INPUT_ENVELOPE);
        assert_eq!(walk.area_id, 0x8000_34CB);
        assert_eq!(walk.action_object_id, 0x8000_34D1);
        assert_eq!(walk.input_byte, 0);
        assert_eq!(walk.action_byte, 0);
        assert!(walk.first_bool);
        assert!(!walk.second_bool);
        assert_eq!(close.len(), WALK_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
    }

    #[test]
    fn transition_door_close_without_anchor_rewrites_to_decompile_owned_walk_payload() {
        let mut state = SemanticSessionState::default();
        state.area.current_area_object_id = Some(0x8000_F6A9);
        state.objects.observe_mentions(&[
            LiveObjectMention {
                opcode: b'A',
                object_type: DOOR_OBJECT_TYPE,
                object_id: 0x8000_F6AC,
                name: Some("Door".to_string()),
                position: None,
                orientation: None,
                bounds: None,
            },
            LiveObjectMention {
                opcode: b'U',
                object_type: DOOR_OBJECT_TYPE,
                object_id: 0x8000_F6AC,
                name: None,
                position: Some(LiveObjectPosition {
                    x: 15.0,
                    y: 3.33,
                    z: 0.001,
                }),
                orientation: None,
                bounds: None,
            },
        ]);

        let mut open = build_change_door_payload(0x8000_F6AC, DOOR_OPEN_STATE);
        let open_summary =
            claim_or_rewrite_payload_if_verified_with_state(&mut open, &mut state)
                .expect("open door packet should be exact-claimed");
        assert_eq!(open_summary.kind, ClientInputKind::ChangeDoorState);
        assert!(!open_summary.rewritten_transition_door_close);

        let mut close = build_change_door_payload(0x8000_F6AC, 0x0016);
        let close_summary =
            claim_or_rewrite_payload_if_verified_with_state(&mut close, &mut state)
                .expect("transition door close should rewrite to a walk packet");
        let walk = parse_walk_to_waypoint(&close).expect("rewritten packet should parse");
        assert_eq!(close_summary.kind, ClientInputKind::WalkToWaypoint);
        assert_eq!(close_summary.packet_name, "Input_WalkToWaypoint");
        assert!(close_summary.rewritten_transition_door_close);
        assert_eq!(close[0], CLIENT_INPUT_ENVELOPE);
        assert_eq!(walk.area_id, 0x8000_F6A9);
        assert_eq!(walk.action_object_id, 0x8000_F6AC);
        assert_eq!(walk.input_byte, 0);
        assert_eq!(walk.action_byte, 0);
        assert!(walk.first_bool);
        assert!(!walk.second_bool);
        assert_eq!(close.len(), WALK_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
    }

    fn build_change_door_payload(door_id: u32, state: u16) -> Vec<u8> {
        let mut payload = Vec::with_capacity(DOOR_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, CHANGE_DOOR_STATE_MINOR]);
        payload.extend_from_slice(&(DOOR_DECLARED_BYTES as u32).to_le_bytes());
        payload.extend_from_slice(&door_id.to_le_bytes());
        payload.extend_from_slice(&state.to_le_bytes());
        payload.push(0x60);
        payload
    }
}
