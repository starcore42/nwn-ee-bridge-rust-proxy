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
//! - EE server reader case `0x02` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657130`, case block at `0x14045337F`) reads one
//!   OBJECTID, checks overflow/underflow, then queues `AddAttackActions`.
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
//! - EE server reader case `0x06` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657210`, case block at `0x1404533E9`) reads two
//!   WORDs, one OBJECTID, and then reads three FLOATs only when the OBJECTID is
//!   Diamond's invalid/location sentinel `0x7F000000`, before checking
//!   overflow/underflow and calling `CNWSCreature::UseFeat`.
//! - EE server reader case `0x07` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657280`, case block at `0x1404534F7`) reads two
//!   BYTEs, one OBJECTID, and three FLOATs before checking overflow/underflow
//!   and calling `CNWSCreature::UseSkill`.
//! - EE server reader case `0x09` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657870`, case block at `0x1404539E9`) reads
//!   OBJECTID, BYTE, BOOL+optional BYTE, BOOL+optional OBJECTID, and
//!   BOOL+optional three FLOAT values before checking overflow/underflow.
//!   The legacy HG path expects EE's self target sentinel `0xFFFFFFFD` to be
//!   translated to Diamond's invalid/self target sentinel `0x7F000000` only
//!   for the optional target OBJECTID field.
//! - EE server reader case `0x0A` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657514`, case block at `0x140453807`) reads one
//!   BYTE mode, then reads one OBJECTID only when the mode byte is `5`
//!   (counterspell target), then checks overflow/underflow before queuing the
//!   counterspell action and toggling the mode.
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
//! - EE server reader case `0x0C` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657830`, case block at `0x140453C2C`) reads one
//!   OBJECTID, checks overflow/underflow, then queues the unlock-object action
//!   after resolving the target object.
//! - EE server reader case `0x0D` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1658279`, case block at `0x140454244`) reads no
//!   packet fields before checking creature state and calling
//!   `CNWSCreature::Rest`.
//! - EE client sender `sub_1407BFFB0` (`nwn ee decompile.txt:2941899`)
//!   sets the outgoing high-level data pointer/length to null/zero and sends
//!   family `0x06`, minor `0x0D`, so the on-wire `Input_Rest` payload is only
//!   the three-byte high-level header (`70 06 0D`).
//! - EE server reader case `0x0E` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1657940`, case block at `0x140453D1E`) reads one
//!   OBJECTID, checks overflow/underflow, then queues the lock-object action
//!   after resolving the target object.
//! - EE server reader case `0x10` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1658426`, case block at `0x1404543DE`) reads
//!   BYTE, DWORD, BYTE, BYTE, BYTE before checking overflow/underflow and
//!   calling `CNWSCreatureStats::SetMemorizedSpellSlot`.
//! - EE server reader case `0x11` in
//!   `CNWSMessage::HandlePlayerToServerInputMessage`
//!   (`nwn ee decompile.txt:1658487`, case block at `0x1404544B6`) reads three
//!   BYTE values before checking overflow/underflow and calling
//!   `CNWSCreatureStats::ClearMemorizedSpellSlot`.
//!
//! The Diamond/1.69 text decompile available in this workspace is stripped for
//! these handler names. The live HG 1.69 server accepts the same CNW layouts
//! emitted by the EE sender and documented above; until a clearer Diamond
//! symbol anchor is recovered, this module keeps the claim conservative by
//! requiring exact cursor and fragment-bit consumption rather than allowing any
//! broader input-family shape.
//! - The old in-process bridge had to translate a second click on an already
//!   opened transition door from `Input_ChangeDoorState(close)` into a
//!   server-owned transition action. The proxy emits the exact
//!   `Input_WalkToWaypoint` shape above only when the client recently opened
//!   the same verified door and the wire-derived state has both the current
//!   area OBJECTID and the door position. It deliberately does not classify
//!   nearby placeables/triggers by display name; the legacy server remains
//!   authoritative for whether walking to that door fires a transition.
//!   The EE server decompile shows `Input_ChangeDoorState(close)` only queues
//!   `AddCloseDoorAction`; `HandlePlayerToServerInputWalkToWaypoint` owns the
//!   movement/transition path. The legacy server remains authoritative: the
//!   proxy only chooses the decompile-owned input shape, not the game outcome.

use std::time::{Duration, Instant};

use crate::{crc::read_le_u32, packet::m::HighLevel, translate::semantic::SemanticSessionState};

const INPUT_MAJOR: u8 = 0x06;
const WALK_TO_WAYPOINT_MINOR: u8 = 0x01;
const ATTACK_MINOR: u8 = 0x02;
const CHANGE_DOOR_STATE_MINOR: u8 = 0x03;
const EXAMINE_MINOR: u8 = 0x05;
const USE_FEAT_MINOR: u8 = 0x06;
const USE_SKILL_MINOR: u8 = 0x07;
const USE_ITEM_MINOR: u8 = 0x09;
const TOGGLE_MODE_MINOR: u8 = 0x0A;
const USE_OBJECT_MINOR: u8 = 0x0B;
const UNLOCK_OBJECT_MINOR: u8 = 0x0C;
const REST_MINOR: u8 = 0x0D;
const LOCK_OBJECT_MINOR: u8 = 0x0E;
const MEMORIZE_SPELL_MINOR: u8 = 0x10;
const UNMEMORIZE_SPELL_MINOR: u8 = 0x11;
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
const OBJECT_ONLY_READ_BODY_BYTES: usize = OBJECT_ID_BYTES;
const USE_FEAT_TARGET_READ_BODY_BYTES: usize = WORD_BYTES + WORD_BYTES + OBJECT_ID_BYTES;
const USE_FEAT_LOCATION_READ_BODY_BYTES: usize =
    USE_FEAT_TARGET_READ_BODY_BYTES + (3 * FLOAT_BYTES);
const USE_SKILL_READ_BODY_BYTES: usize =
    BYTE_BYTES + BYTE_BYTES + OBJECT_ID_BYTES + (3 * FLOAT_BYTES);
const USE_ITEM_BASE_READ_BODY_BYTES: usize = OBJECT_ID_BYTES + BYTE_BYTES;
const TOGGLE_MODE_BASE_READ_BODY_BYTES: usize = BYTE_BYTES;
const USE_OBJECT_READ_BODY_BYTES: usize = OBJECT_ID_BYTES;
const MEMORIZE_SPELL_READ_BODY_BYTES: usize = BYTE_BYTES + 4 + BYTE_BYTES + BYTE_BYTES + BYTE_BYTES;
const UNMEMORIZE_SPELL_READ_BODY_BYTES: usize = BYTE_BYTES + BYTE_BYTES + BYTE_BYTES;
const WALK_DECLARED_BYTES: usize = READ_CURSOR_START + WALK_READ_BODY_BYTES;
const DOOR_DECLARED_BYTES: usize = READ_CURSOR_START + DOOR_READ_BODY_BYTES;
const EXAMINE_DECLARED_BYTES: usize = READ_CURSOR_START + EXAMINE_READ_BODY_BYTES;
const OBJECT_ONLY_DECLARED_BYTES: usize = READ_CURSOR_START + OBJECT_ONLY_READ_BODY_BYTES;
const USE_FEAT_TARGET_DECLARED_BYTES: usize = READ_CURSOR_START + USE_FEAT_TARGET_READ_BODY_BYTES;
const USE_FEAT_LOCATION_DECLARED_BYTES: usize =
    READ_CURSOR_START + USE_FEAT_LOCATION_READ_BODY_BYTES;
const USE_SKILL_DECLARED_BYTES: usize = READ_CURSOR_START + USE_SKILL_READ_BODY_BYTES;
const USE_ITEM_MIN_DECLARED_BYTES: usize = READ_CURSOR_START + USE_ITEM_BASE_READ_BODY_BYTES;
const TOGGLE_MODE_MIN_DECLARED_BYTES: usize = READ_CURSOR_START + TOGGLE_MODE_BASE_READ_BODY_BYTES;
const USE_OBJECT_DECLARED_BYTES: usize = READ_CURSOR_START + USE_OBJECT_READ_BODY_BYTES;
const HIGH_LEVEL_ONLY_INPUT_BYTES: usize = HIGH_LEVEL_HEADER_BYTES;
const MEMORIZE_SPELL_DECLARED_BYTES: usize = READ_CURSOR_START + MEMORIZE_SPELL_READ_BODY_BYTES;
const UNMEMORIZE_SPELL_DECLARED_BYTES: usize = READ_CURSOR_START + UNMEMORIZE_SPELL_READ_BODY_BYTES;
const ONE_FRAGMENT_BYTE: usize = 1;
const EE_SELF_OBJECT_ID: u32 = 0xFFFF_FFFD;
const INVALID_OBJECT_ID: u32 = 0x7F00_0000;
const DOOR_OPEN_STATE: u16 = 0x0015;
const RECENT_TRANSITION_DOOR_OPEN_WINDOW: Duration = Duration::from_secs(45);
const DOOR_OBJECT_TYPE: u8 = 0x0A;
const TOGGLE_MODE_COUNTERSPELL: u8 = 5;
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
    Attack,
    ChangeDoorState,
    Examine,
    UseFeat,
    UseSkill,
    UseItem,
    ToggleMode,
    UseObject,
    UnlockObject,
    Rest,
    LockObject,
    MemorizeSpell,
    UnMemorizeSpell,
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
pub struct Attack {
    pub target_object_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Examine {
    pub object_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseFeat {
    pub feat_id: u16,
    pub subfeat_id: u16,
    pub target_object_id: u32,
    pub position: Option<(f32, f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseSkill {
    pub skill_id: u8,
    pub subskill_id: u8,
    pub target_object_id: u32,
    pub position: (f32, f32, f32),
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
pub struct ToggleMode {
    pub mode: u8,
    pub counterspell_target: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseObject {
    pub object_id: u32,
    pub mark_inventory_gui_state: bool,
    pub schedule_script_event: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnlockObject {
    pub object_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockObject {
    pub object_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorizeSpell {
    pub class_index: u8,
    pub spell_id: u32,
    pub spell_level: u8,
    pub slot: u8,
    pub domain: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnMemorizeSpell {
    pub class_index: u8,
    pub spell_level: u8,
    pub slot: u8,
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
        ATTACK_MINOR => {
            let attack = parse_attack(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_Attack",
                kind: ClientInputKind::Attack,
                declared: OBJECT_ONLY_DECLARED_BYTES,
                fragment_bytes: payload.len() - OBJECT_ONLY_DECLARED_BYTES,
                primary_object_id: attack.target_object_id,
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
        USE_FEAT_MINOR => {
            let use_feat = parse_use_feat(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_UseFeat",
                kind: ClientInputKind::UseFeat,
                declared: read_declared(payload)?,
                fragment_bytes: ONE_FRAGMENT_BYTE,
                primary_object_id: use_feat.target_object_id,
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        USE_SKILL_MINOR => {
            let use_skill = parse_use_skill(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_UseSkill",
                kind: ClientInputKind::UseSkill,
                declared: USE_SKILL_DECLARED_BYTES,
                fragment_bytes: payload.len() - USE_SKILL_DECLARED_BYTES,
                primary_object_id: use_skill.target_object_id,
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
        TOGGLE_MODE_MINOR => {
            let toggle = parse_toggle_mode(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_ToggleMode",
                kind: ClientInputKind::ToggleMode,
                declared: read_declared(payload)?,
                fragment_bytes: ONE_FRAGMENT_BYTE,
                primary_object_id: toggle.counterspell_target.unwrap_or(INVALID_OBJECT_ID),
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
        UNLOCK_OBJECT_MINOR => {
            let unlock = parse_unlock_object(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_UnlockObject",
                kind: ClientInputKind::UnlockObject,
                declared: OBJECT_ONLY_DECLARED_BYTES,
                fragment_bytes: payload.len() - OBJECT_ONLY_DECLARED_BYTES,
                primary_object_id: unlock.object_id,
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        REST_MINOR => {
            parse_rest(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_Rest",
                kind: ClientInputKind::Rest,
                declared: HIGH_LEVEL_ONLY_INPUT_BYTES,
                fragment_bytes: 0,
                primary_object_id: INVALID_OBJECT_ID,
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        LOCK_OBJECT_MINOR => {
            let lock = parse_lock_object(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_LockObject",
                kind: ClientInputKind::LockObject,
                declared: OBJECT_ONLY_DECLARED_BYTES,
                fragment_bytes: payload.len() - OBJECT_ONLY_DECLARED_BYTES,
                primary_object_id: lock.object_id,
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        MEMORIZE_SPELL_MINOR => {
            parse_memorize_spell(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_MemorizeSpell",
                kind: ClientInputKind::MemorizeSpell,
                declared: MEMORIZE_SPELL_DECLARED_BYTES,
                fragment_bytes: payload.len() - MEMORIZE_SPELL_DECLARED_BYTES,
                primary_object_id: INVALID_OBJECT_ID,
                rewritten_self_object_id,
                rewritten_transition_door_close: false,
            })
        }
        UNMEMORIZE_SPELL_MINOR => {
            parse_unmemorize_spell(payload)?;
            Some(ClientInputClaimSummary {
                packet_name: "Input_UnMemorizeSpell",
                kind: ClientInputKind::UnMemorizeSpell,
                declared: UNMEMORIZE_SPELL_DECLARED_BYTES,
                fragment_bytes: payload.len() - UNMEMORIZE_SPELL_DECLARED_BYTES,
                primary_object_id: INVALID_OBJECT_ID,
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
        && state.client_input.recent_open_at.is_some_and(|opened_at| {
            now.duration_since(opened_at) <= RECENT_TRANSITION_DOOR_OPEN_WINDOW
        });
    if !recent_open {
        return None;
    }

    let Some(area_id) = state.area.current_area_object_id else {
        state.client_input.transition_door_close_rewrite_skips = state
            .client_input
            .transition_door_close_rewrite_skips
            .saturating_add(1);
        tracing::debug!(
            door_id = format_args!("0x{:08X}", door.door_id),
            requested_state = format_args!("0x{:04X}", door.state),
            reason = "missing-current-area",
            "client Input_ChangeDoorState transition-door close rewrite skipped"
        );
        return None;
    };
    let Some(door_object) = state.objects.get(DOOR_OBJECT_TYPE, door.door_id) else {
        state.client_input.transition_door_close_rewrite_skips = state
            .client_input
            .transition_door_close_rewrite_skips
            .saturating_add(1);
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
        state.client_input.transition_door_close_rewrite_skips = state
            .client_input
            .transition_door_close_rewrite_skips
            .saturating_add(1);
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
    state.client_input.transition_door_close_rewrites = state
        .client_input
        .transition_door_close_rewrites
        .saturating_add(1);
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
        known_objects = state.objects.known.len(),
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

pub fn parse_attack(payload: &[u8]) -> Option<Attack> {
    let target_object_id = parse_object_only_input(payload, ATTACK_MINOR)?;
    Some(Attack { target_object_id })
}

pub fn parse_examine(payload: &[u8]) -> Option<Examine> {
    let object_id = parse_object_only_input(payload, EXAMINE_MINOR)?;
    Some(Examine { object_id })
}

pub fn parse_use_feat(payload: &[u8]) -> Option<UseFeat> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != USE_FEAT_MINOR {
        return None;
    }
    let declared = read_declared(payload)?;
    if declared != USE_FEAT_TARGET_DECLARED_BYTES && declared != USE_FEAT_LOCATION_DECLARED_BYTES {
        return None;
    }
    require_declared_and_fragment_bits_at(payload, declared, 0)?;

    let mut cursor = READ_CURSOR_START;
    let feat_id = read_u16_at(payload, cursor)?;
    cursor += WORD_BYTES;
    let subfeat_id = read_u16_at(payload, cursor)?;
    cursor += WORD_BYTES;
    let target_object_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;

    let position = if target_object_id == INVALID_OBJECT_ID {
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

    if cursor != declared {
        return None;
    }

    Some(UseFeat {
        feat_id,
        subfeat_id,
        target_object_id,
        position,
    })
}

pub fn parse_use_skill(payload: &[u8]) -> Option<UseSkill> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != USE_SKILL_MINOR {
        return None;
    }
    require_declared_and_fragment_bits(payload, USE_SKILL_DECLARED_BYTES, 0)?;

    let mut cursor = READ_CURSOR_START;
    let skill_id = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let subskill_id = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let target_object_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;
    let x = read_f32_at(payload, cursor)?;
    cursor += FLOAT_BYTES;
    let y = read_f32_at(payload, cursor)?;
    cursor += FLOAT_BYTES;
    let z = read_f32_at(payload, cursor)?;
    cursor += FLOAT_BYTES;

    if cursor != USE_SKILL_DECLARED_BYTES {
        return None;
    }

    Some(UseSkill {
        skill_id,
        subskill_id,
        target_object_id,
        position: (x, y, z),
    })
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

pub fn parse_toggle_mode(payload: &[u8]) -> Option<ToggleMode> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != TOGGLE_MODE_MINOR {
        return None;
    }
    let declared = read_declared(payload)?;
    if declared < TOGGLE_MODE_MIN_DECLARED_BYTES {
        return None;
    }
    require_declared_and_fragment_bits_at(payload, declared, 0)?;

    let mut cursor = READ_CURSOR_START;
    let mode = *payload.get(cursor)?;
    cursor += BYTE_BYTES;

    let counterspell_target = if mode == TOGGLE_MODE_COUNTERSPELL {
        let value = read_u32_at(payload, cursor)?;
        cursor += OBJECT_ID_BYTES;
        Some(value)
    } else {
        None
    };

    if cursor != declared {
        return None;
    }

    Some(ToggleMode {
        mode,
        counterspell_target,
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

pub fn parse_unlock_object(payload: &[u8]) -> Option<UnlockObject> {
    let object_id = parse_object_only_input(payload, UNLOCK_OBJECT_MINOR)?;
    Some(UnlockObject { object_id })
}

pub fn parse_rest(payload: &[u8]) -> Option<()> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != REST_MINOR {
        return None;
    }
    (payload.len() == HIGH_LEVEL_ONLY_INPUT_BYTES).then_some(())
}

pub fn parse_lock_object(payload: &[u8]) -> Option<LockObject> {
    let object_id = parse_object_only_input(payload, LOCK_OBJECT_MINOR)?;
    Some(LockObject { object_id })
}

pub fn parse_memorize_spell(payload: &[u8]) -> Option<MemorizeSpell> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != MEMORIZE_SPELL_MINOR {
        return None;
    }
    require_declared_and_fragment_bits(payload, MEMORIZE_SPELL_DECLARED_BYTES, 0)?;

    let mut cursor = READ_CURSOR_START;
    let class_index = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let spell_id = read_u32_at(payload, cursor)?;
    cursor += 4;
    let spell_level = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let slot = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let domain = *payload.get(cursor)?;
    cursor += BYTE_BYTES;

    if cursor != MEMORIZE_SPELL_DECLARED_BYTES {
        return None;
    }

    Some(MemorizeSpell {
        class_index,
        spell_id,
        spell_level,
        slot,
        domain,
    })
}

pub fn parse_unmemorize_spell(payload: &[u8]) -> Option<UnMemorizeSpell> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != UNMEMORIZE_SPELL_MINOR {
        return None;
    }
    require_declared_and_fragment_bits(payload, UNMEMORIZE_SPELL_DECLARED_BYTES, 0)?;

    let mut cursor = READ_CURSOR_START;
    let class_index = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let spell_level = *payload.get(cursor)?;
    cursor += BYTE_BYTES;
    let slot = *payload.get(cursor)?;
    cursor += BYTE_BYTES;

    if cursor != UNMEMORIZE_SPELL_DECLARED_BYTES {
        return None;
    }

    Some(UnMemorizeSpell {
        class_index,
        spell_level,
        slot,
    })
}

fn parse_object_only_input(payload: &[u8], minor: u8) -> Option<u32> {
    let high = HighLevel::parse(payload)?;
    if high.major != INPUT_MAJOR || high.minor != minor {
        return None;
    }
    require_declared_and_fragment_bits(payload, OBJECT_ONLY_DECLARED_BYTES, 0)?;

    let mut cursor = READ_CURSOR_START;
    let object_id = read_u32_at(payload, cursor)?;
    cursor += OBJECT_ID_BYTES;

    if cursor != OBJECT_ONLY_DECLARED_BYTES || object_id == INVALID_OBJECT_ID {
        return None;
    }

    Some(object_id)
}

fn read_declared(payload: &[u8]) -> Option<usize> {
    usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()
}

fn require_declared_and_fragment_bits(
    payload: &[u8],
    expected_declared: usize,
    data_bits: usize,
) -> Option<()> {
    require_declared_and_fragment_bits_at(payload, expected_declared, data_bits)
}

fn require_declared_and_fragment_bits_at(
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

#[cfg(all(test, hgbridge_private_fixtures))]
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
        let trigger = parse_walk_to_waypoint(trigger).expect("trigger click fixture should parse");
        assert_eq!(trigger.area_id, 0x8000_34CB);
        assert_eq!(trigger.input_byte, 0);
        assert!(trigger.first_bool);
        assert!(!trigger.second_bool);
        assert_eq!(trigger.action_byte, 0);
        assert_eq!(trigger.action_object_id, 0x8000_35FD);
    }

    #[test]
    fn live_hg_door_trigger_walk_probe_matches_writer_shape() {
        // Live HG door probe, generated by EE Input_WalkToWaypoint and rewritten
        // to the nearby transition trigger by the harness diagnostics.
        let fixture = [
            0x70, 0x06, 0x01, 0x1D, 0x00, 0x00, 0x00, 0xCB, 0x34, 0x00, 0x80, 0xF3, 0xC0, 0x3D,
            0x42, 0x79, 0x5E, 0x26, 0x42, 0x80, 0xF0, 0x23, 0x3C, 0x00, 0x00, 0xFD, 0x35, 0x00,
            0x80, 0xA0,
        ];
        let summary =
            claim_payload_if_verified(&fixture).expect("live trigger probe should be claimed");
        let parsed = parse_walk_to_waypoint(&fixture).expect("live trigger probe should parse");

        assert_eq!(summary.kind, ClientInputKind::WalkToWaypoint);
        assert_eq!(summary.packet_name, "Input_WalkToWaypoint");
        assert_eq!(summary.declared, WALK_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(parsed.area_id, 0x8000_34CB);
        assert_eq!(parsed.input_byte, 0);
        assert!(!parsed.first_bool);
        assert!(!parsed.second_bool);
        assert_eq!(parsed.action_byte, 0);
        assert_eq!(parsed.action_object_id, 0x8000_35FD);
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
    fn attack_object_only_shape_matches_decompile_cursor_shape() {
        let payload = build_object_only_payload(ATTACK_MINOR, 0x8000_34D1);
        let summary = claim_payload_if_verified(&payload).expect("attack packet should be claimed");
        let parsed = parse_attack(&payload).expect("attack packet should parse");

        assert_eq!(summary.kind, ClientInputKind::Attack);
        assert_eq!(summary.packet_name, "Input_Attack");
        assert_eq!(summary.declared, OBJECT_ONLY_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_34D1);
        assert_eq!(parsed.target_object_id, 0x8000_34D1);
    }

    #[test]
    fn examine_fixture_matches_decompile_cursor_shape() {
        let fixture = include_bytes!("../../fixtures/client_input/examine_item.bin");
        let summary =
            claim_payload_if_verified(fixture).expect("examine fixture should be claimed");
        let parsed = parse_examine(fixture).expect("examine fixture should parse");

        assert_eq!(summary.kind, ClientInputKind::Examine);
        assert_eq!(summary.packet_name, "Input_Examine");
        assert_eq!(summary.declared, EXAMINE_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert!(!summary.rewritten_self_object_id);
        assert_eq!(parsed.object_id, 0x8001_6012);
    }

    #[test]
    fn use_feat_shapes_match_decompile_cursor_shape() {
        let target = build_use_feat_payload(0x0123, 0x0045, 0x8000_34D1, None);
        let target_summary =
            claim_payload_if_verified(&target).expect("targeted use-feat should be claimed");
        let target_parsed = parse_use_feat(&target).expect("targeted use-feat should parse");
        assert_eq!(target_summary.kind, ClientInputKind::UseFeat);
        assert_eq!(target_summary.packet_name, "Input_UseFeat");
        assert_eq!(target_summary.declared, USE_FEAT_TARGET_DECLARED_BYTES);
        assert_eq!(target_summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(target_summary.primary_object_id, 0x8000_34D1);
        assert_eq!(target_parsed.feat_id, 0x0123);
        assert_eq!(target_parsed.subfeat_id, 0x0045);
        assert_eq!(target_parsed.position, None);

        let location =
            build_use_feat_payload(0x0123, 0x0045, INVALID_OBJECT_ID, Some((11.0, 12.0, 0.5)));
        let location_summary =
            claim_payload_if_verified(&location).expect("location use-feat should be claimed");
        let location_parsed = parse_use_feat(&location).expect("location use-feat should parse");
        assert_eq!(location_summary.declared, USE_FEAT_LOCATION_DECLARED_BYTES);
        assert_eq!(location_summary.primary_object_id, INVALID_OBJECT_ID);
        assert_eq!(location_parsed.position, Some((11.0, 12.0, 0.5)));
    }

    #[test]
    fn use_skill_shape_matches_decompile_cursor_shape() {
        let payload = build_use_skill_payload(2, 0x66, 0x8000_34D1, (11.0, 12.0, 0.5));
        let summary =
            claim_payload_if_verified(&payload).expect("use-skill packet should be claimed");
        let parsed = parse_use_skill(&payload).expect("use-skill packet should parse");

        assert_eq!(summary.kind, ClientInputKind::UseSkill);
        assert_eq!(summary.packet_name, "Input_UseSkill");
        assert_eq!(summary.declared, USE_SKILL_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_34D1);
        assert_eq!(parsed.skill_id, 2);
        assert_eq!(parsed.subskill_id, 0x66);
        assert_eq!(parsed.target_object_id, 0x8000_34D1);
        assert_eq!(parsed.position, (11.0, 12.0, 0.5));
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
    fn toggle_mode_shapes_match_decompile_cursor_shape() {
        let normal = build_toggle_mode_payload(1, None);
        let normal_summary =
            claim_payload_if_verified(&normal).expect("normal toggle should be claimed");
        let normal_parsed = parse_toggle_mode(&normal).expect("normal toggle should parse");
        assert_eq!(normal_summary.kind, ClientInputKind::ToggleMode);
        assert_eq!(normal_summary.packet_name, "Input_ToggleMode");
        assert_eq!(normal_summary.declared, TOGGLE_MODE_MIN_DECLARED_BYTES);
        assert_eq!(normal_summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(normal_summary.primary_object_id, INVALID_OBJECT_ID);
        assert_eq!(normal_parsed.mode, 1);
        assert_eq!(normal_parsed.counterspell_target, None);

        let counterspell = build_toggle_mode_payload(TOGGLE_MODE_COUNTERSPELL, Some(0x8000_34D1));
        let counterspell_summary = claim_payload_if_verified(&counterspell)
            .expect("counterspell toggle should be claimed");
        let counterspell_parsed =
            parse_toggle_mode(&counterspell).expect("counterspell toggle should parse");
        assert_eq!(counterspell_summary.kind, ClientInputKind::ToggleMode);
        assert_eq!(
            counterspell_summary.declared,
            TOGGLE_MODE_MIN_DECLARED_BYTES + OBJECT_ID_BYTES
        );
        assert_eq!(counterspell_summary.primary_object_id, 0x8000_34D1);
        assert_eq!(counterspell_parsed.mode, TOGGLE_MODE_COUNTERSPELL);
        assert_eq!(counterspell_parsed.counterspell_target, Some(0x8000_34D1));
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
    fn use_object_local_auto_use_fixture_matches_decompile_cursor_shape() {
        // Captured from the local Diamond harness auto-use path against the
        // compact local object id assigned to the bw167demo placeable.
        let fixture = [
            0x70, 0x06, 0x0B, 0x0B, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x80, 0xA3,
        ];
        let summary =
            claim_payload_if_verified(&fixture).expect("local auto-use should be claimed");
        let parsed = parse_use_object(&fixture).expect("local auto-use should parse");

        assert_eq!(summary.kind, ClientInputKind::UseObject);
        assert_eq!(summary.packet_name, "Input_UseObject");
        assert_eq!(summary.declared, USE_OBJECT_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_0006);
        assert_eq!(parsed.object_id, 0x8000_0006);
        assert!(!parsed.mark_inventory_gui_state);
        assert!(!parsed.schedule_script_event);
    }

    #[test]
    fn use_object_local_auto_use_zero_residual_fragment_bits_match_decompile_cursor_shape() {
        // Captured from C:\nwnbridge\local-diamond-bridge-20260519-120243
        // after the driver used bw167demo's compact Abandoned Home placeable.
        let fixture = [
            0x70, 0x06, 0x0B, 0x0B, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x80, 0xA0,
        ];
        let summary =
            claim_payload_if_verified(&fixture).expect("local auto-use should be claimed");
        let parsed = parse_use_object(&fixture).expect("local auto-use should parse");

        assert_eq!(summary.kind, ClientInputKind::UseObject);
        assert_eq!(summary.packet_name, "Input_UseObject");
        assert_eq!(summary.declared, USE_OBJECT_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_0006);
        assert_eq!(parsed.object_id, 0x8000_0006);
        assert!(!parsed.mark_inventory_gui_state);
        assert!(!parsed.schedule_script_event);
    }

    #[test]
    fn local_diamond_door_open_fixture_matches_decompile_cursor_shape() {
        // Captured from the local Diamond harness door auto-use path against
        // bw167demo's compact door id.
        let fixture = [
            0x70, 0x06, 0x03, 0x0D, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x80, 0x15, 0x00, 0x68,
        ];
        let summary =
            claim_payload_if_verified(&fixture).expect("local door-open should be claimed");
        let parsed = parse_change_door_state(&fixture).expect("local door-open should parse");

        assert_eq!(summary.kind, ClientInputKind::ChangeDoorState);
        assert_eq!(summary.packet_name, "Input_ChangeDoorState");
        assert_eq!(summary.declared, DOOR_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_0003);
        assert_eq!(parsed.door_id, 0x8000_0003);
        assert_eq!(parsed.state, DOOR_OPEN_STATE);
    }

    #[test]
    fn local_diamond_door_transition_walk_fixture_matches_decompile_cursor_shape() {
        // Captured from the local Diamond harness transition-click follow-up
        // after opening bw167demo's compact door id.
        let fixture = [
            0x70, 0x06, 0x01, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x9A, 0x99, 0xDD,
            0x41, 0x00, 0x00, 0x10, 0x42, 0x6F, 0x12, 0x03, 0x3B, 0x00, 0x00, 0x03, 0x00, 0x00,
            0x80, 0xB0,
        ];
        let summary =
            claim_payload_if_verified(&fixture).expect("local transition walk should be claimed");
        let parsed = parse_walk_to_waypoint(&fixture).expect("local transition walk should parse");

        assert_eq!(summary.kind, ClientInputKind::WalkToWaypoint);
        assert_eq!(summary.packet_name, "Input_WalkToWaypoint");
        assert_eq!(summary.declared, WALK_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_0000);
        assert_eq!(parsed.area_id, 0x8000_0000);
        assert_eq!(parsed.action_object_id, 0x8000_0003);
        assert_eq!(parsed.input_byte, 0);
        assert_eq!(parsed.action_byte, 0);
        assert!(parsed.first_bool);
        assert!(!parsed.second_bool);
    }

    #[test]
    fn local_diamond_20260519_door_open_fixture_matches_decompile_cursor_shape() {
        // Captured from C:\nwnbridge\local-diamond-bridge-20260519-100428
        // after the driver opened bw167demo's compact transition door.
        let fixture = include_bytes!(
            "../../fixtures/client_input/local_diamond_bw167demo_door_open_20260519.bin"
        );
        let summary =
            claim_payload_if_verified(fixture).expect("captured door-open should be claimed");
        let parsed = parse_change_door_state(fixture).expect("captured door-open should parse");

        assert_eq!(fixture[13], 0x70);
        assert_eq!(summary.kind, ClientInputKind::ChangeDoorState);
        assert_eq!(summary.packet_name, "Input_ChangeDoorState");
        assert_eq!(summary.declared, DOOR_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_0003);
        assert_eq!(parsed.door_id, 0x8000_0003);
        assert_eq!(parsed.state, DOOR_OPEN_STATE);
    }

    #[test]
    fn local_diamond_20260519_walk_probe_fixture_matches_decompile_cursor_shape() {
        // Captured from the same local Diamond harness run after the
        // auto-trigger walk probe fired near the opened transition door.
        let fixture = include_bytes!(
            "../../fixtures/client_input/local_diamond_bw167demo_walk_probe_20260519.bin"
        );
        let summary =
            claim_payload_if_verified(fixture).expect("captured walk probe should be claimed");
        let parsed = parse_walk_to_waypoint(fixture).expect("captured walk probe should parse");

        assert_eq!(fixture[29], 0xA0);
        assert_eq!(summary.kind, ClientInputKind::WalkToWaypoint);
        assert_eq!(summary.packet_name, "Input_WalkToWaypoint");
        assert_eq!(summary.declared, WALK_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_0000);
        assert_eq!(parsed.area_id, 0x8000_0000);
        assert_eq!(parsed.input_byte, 1);
        assert!(!parsed.first_bool);
        assert!(!parsed.second_bool);
        assert_eq!(parsed.action_byte, 0);
        assert_eq!(parsed.action_object_id, INVALID_OBJECT_ID);
    }

    #[test]
    fn unlock_object_only_shape_matches_decompile_cursor_shape() {
        let payload = build_object_only_payload(UNLOCK_OBJECT_MINOR, 0x8000_34D1);
        let summary = claim_payload_if_verified(&payload).expect("unlock packet should be claimed");
        let parsed = parse_unlock_object(&payload).expect("unlock packet should parse");

        assert_eq!(summary.kind, ClientInputKind::UnlockObject);
        assert_eq!(summary.packet_name, "Input_UnlockObject");
        assert_eq!(summary.declared, OBJECT_ONLY_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_34D1);
        assert_eq!(parsed.object_id, 0x8000_34D1);
    }

    #[test]
    fn rest_high_level_only_shape_matches_decompile_sender_and_reader() {
        let payload = build_high_level_only_input_payload(REST_MINOR);
        let summary = claim_payload_if_verified(&payload).expect("rest packet should be claimed");
        parse_rest(&payload).expect("rest packet should parse");

        assert_eq!(summary.kind, ClientInputKind::Rest);
        assert_eq!(summary.packet_name, "Input_Rest");
        assert_eq!(summary.declared, HIGH_LEVEL_ONLY_INPUT_BYTES);
        assert_eq!(summary.fragment_bytes, 0);
        assert_eq!(summary.primary_object_id, INVALID_OBJECT_ID);
    }

    #[test]
    fn lock_object_only_shape_matches_decompile_cursor_shape() {
        let payload = build_object_only_payload(LOCK_OBJECT_MINOR, 0x8000_34D1);
        let summary = claim_payload_if_verified(&payload).expect("lock packet should be claimed");
        let parsed = parse_lock_object(&payload).expect("lock packet should parse");

        assert_eq!(summary.kind, ClientInputKind::LockObject);
        assert_eq!(summary.packet_name, "Input_LockObject");
        assert_eq!(summary.declared, OBJECT_ONLY_DECLARED_BYTES);
        assert_eq!(summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(summary.primary_object_id, 0x8000_34D1);
        assert_eq!(parsed.object_id, 0x8000_34D1);
    }

    #[test]
    fn memorize_spell_shapes_match_decompile_cursor_shape() {
        let memorize = build_memorize_spell_payload(1, 0x1234, 7, 2, 0);
        let memorize_summary =
            claim_payload_if_verified(&memorize).expect("memorize-spell packet should be claimed");
        let memorize_parsed =
            parse_memorize_spell(&memorize).expect("memorize-spell packet should parse");
        assert_eq!(memorize_summary.kind, ClientInputKind::MemorizeSpell);
        assert_eq!(memorize_summary.packet_name, "Input_MemorizeSpell");
        assert_eq!(memorize_summary.declared, MEMORIZE_SPELL_DECLARED_BYTES);
        assert_eq!(memorize_summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(memorize_parsed.class_index, 1);
        assert_eq!(memorize_parsed.spell_id, 0x1234);
        assert_eq!(memorize_parsed.spell_level, 7);
        assert_eq!(memorize_parsed.slot, 2);
        assert_eq!(memorize_parsed.domain, 0);

        let unmemorize = build_unmemorize_spell_payload(1, 7, 2);
        let unmemorize_summary = claim_payload_if_verified(&unmemorize)
            .expect("unmemorize-spell packet should be claimed");
        let unmemorize_parsed =
            parse_unmemorize_spell(&unmemorize).expect("unmemorize-spell packet should parse");
        assert_eq!(unmemorize_summary.kind, ClientInputKind::UnMemorizeSpell);
        assert_eq!(unmemorize_summary.packet_name, "Input_UnMemorizeSpell");
        assert_eq!(unmemorize_summary.declared, UNMEMORIZE_SPELL_DECLARED_BYTES);
        assert_eq!(unmemorize_summary.fragment_bytes, ONE_FRAGMENT_BYTE);
        assert_eq!(unmemorize_parsed.class_index, 1);
        assert_eq!(unmemorize_parsed.spell_level, 7);
        assert_eq!(unmemorize_parsed.slot, 2);
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

        let mut use_feat_missing_location =
            build_use_feat_payload(0x0123, 0x0045, INVALID_OBJECT_ID, None);
        assert!(claim_payload_if_verified(&use_feat_missing_location).is_none());
        use_feat_missing_location[15] = 0x80;
        assert!(claim_payload_if_verified(&use_feat_missing_location).is_none());

        let use_feat_unexpected_location =
            build_use_feat_payload(0x0123, 0x0045, 0x8000_34D1, Some((1.0, 2.0, 3.0)));
        assert!(claim_payload_if_verified(&use_feat_unexpected_location).is_none());

        let mut toggle_extra = build_toggle_mode_payload(1, None);
        toggle_extra.splice(8..8, 0x8000_34D1u32.to_le_bytes());
        toggle_extra[3..7].copy_from_slice(&12u32.to_le_bytes());
        assert!(claim_payload_if_verified(&toggle_extra).is_none());

        let mut toggle_missing = build_toggle_mode_payload(TOGGLE_MODE_COUNTERSPELL, None);
        assert!(claim_payload_if_verified(&toggle_missing).is_none());
        toggle_missing[8] = 0x80;
        assert!(claim_payload_if_verified(&toggle_missing).is_none());

        let mut attack = build_object_only_payload(ATTACK_MINOR, INVALID_OBJECT_ID);
        assert!(claim_payload_if_verified(&attack).is_none());
        attack[11] = 0x80;
        assert!(claim_payload_if_verified(&attack).is_none());

        let mut rest = build_high_level_only_input_payload(REST_MINOR);
        rest.push(0);
        assert!(claim_payload_if_verified(&rest).is_none());

        let old_wrapped_rest = build_cnw_wrapped_empty_input_payload(REST_MINOR);
        assert!(claim_payload_if_verified(&old_wrapped_rest).is_none());

        let mut memorize = build_memorize_spell_payload(1, 0x1234, 7, 2, 0);
        memorize[15] = 0x80;
        assert!(claim_payload_if_verified(&memorize).is_none());
    }

    #[test]
    fn transition_door_close_uses_verified_door_position_not_named_placeable() {
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
                placeable_state: None,
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
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: 0x8000_3566,
                name: Some("Portal marker".to_string()),
                position: None,
                orientation: None,
                bounds: None,
                placeable_state: None,
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
                placeable_state: None,
            },
        ]);

        let mut open = build_change_door_payload(0x8000_34D1, DOOR_OPEN_STATE);
        let open_summary = claim_or_rewrite_payload_if_verified_with_state(&mut open, &mut state)
            .expect("open door packet should be exact-claimed");
        assert_eq!(open_summary.kind, ClientInputKind::ChangeDoorState);
        assert!(!open_summary.rewritten_transition_door_close);

        let mut close = build_change_door_payload(0x8000_34D1, 0x0016);
        let close_summary = claim_or_rewrite_payload_if_verified_with_state(&mut close, &mut state)
            .expect("transition door close should rewrite to a walk packet");
        let walk = parse_walk_to_waypoint(&close).expect("rewritten packet should parse");
        assert_eq!(close_summary.kind, ClientInputKind::WalkToWaypoint);
        assert!(close_summary.rewritten_transition_door_close);
        assert_eq!(close[0], CLIENT_INPUT_ENVELOPE);
        assert_eq!(walk.area_id, 0x8000_34CB);
        assert_eq!(walk.x, 47.50);
        assert_eq!(walk.y, 43.08);
        assert_eq!(walk.z, 0.002);
        assert_eq!(walk.action_object_id, 0x8000_34D1);
        assert_eq!(walk.input_byte, 0);
        assert_eq!(walk.action_byte, 0);
        assert!(walk.first_bool);
        assert!(!walk.second_bool);
        assert_eq!(close.len(), WALK_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
    }

    #[test]
    fn transition_door_close_without_nearby_objects_uses_verified_door_position() {
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
                placeable_state: None,
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
                placeable_state: None,
            },
        ]);

        let mut open = build_change_door_payload(0x8000_F6AC, DOOR_OPEN_STATE);
        let open_summary = claim_or_rewrite_payload_if_verified_with_state(&mut open, &mut state)
            .expect("open door packet should be exact-claimed");
        assert_eq!(open_summary.kind, ClientInputKind::ChangeDoorState);
        assert!(!open_summary.rewritten_transition_door_close);

        let mut close = build_change_door_payload(0x8000_F6AC, 0x0016);
        let close_summary = claim_or_rewrite_payload_if_verified_with_state(&mut close, &mut state)
            .expect("transition door close should rewrite to a walk packet");
        let walk = parse_walk_to_waypoint(&close).expect("rewritten packet should parse");
        assert_eq!(close_summary.kind, ClientInputKind::WalkToWaypoint);
        assert_eq!(close_summary.packet_name, "Input_WalkToWaypoint");
        assert!(close_summary.rewritten_transition_door_close);
        assert_eq!(close[0], CLIENT_INPUT_ENVELOPE);
        assert_eq!(walk.area_id, 0x8000_F6A9);
        assert_eq!(walk.x, 15.0);
        assert_eq!(walk.y, 3.33);
        assert_eq!(walk.z, 0.001);
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

    fn build_object_only_payload(minor: u8, object_id: u32) -> Vec<u8> {
        let mut payload = Vec::with_capacity(OBJECT_ONLY_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, minor]);
        payload.extend_from_slice(&(OBJECT_ONLY_DECLARED_BYTES as u32).to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.push(0x60);
        payload
    }

    fn build_high_level_only_input_payload(minor: u8) -> Vec<u8> {
        vec![CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, minor]
    }

    fn build_cnw_wrapped_empty_input_payload(minor: u8) -> Vec<u8> {
        let mut payload = Vec::with_capacity(READ_CURSOR_START + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, minor]);
        payload.extend_from_slice(&(READ_CURSOR_START as u32).to_le_bytes());
        payload.push(0x60);
        payload
    }

    fn build_use_feat_payload(
        feat_id: u16,
        subfeat_id: u16,
        target_object_id: u32,
        position: Option<(f32, f32, f32)>,
    ) -> Vec<u8> {
        let declared = USE_FEAT_TARGET_DECLARED_BYTES + position.map_or(0, |_| 3 * FLOAT_BYTES);
        let mut payload = Vec::with_capacity(declared + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, USE_FEAT_MINOR]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&feat_id.to_le_bytes());
        payload.extend_from_slice(&subfeat_id.to_le_bytes());
        payload.extend_from_slice(&target_object_id.to_le_bytes());
        if let Some((x, y, z)) = position {
            payload.extend_from_slice(&x.to_le_bytes());
            payload.extend_from_slice(&y.to_le_bytes());
            payload.extend_from_slice(&z.to_le_bytes());
        }
        payload.push(0x60);
        payload
    }

    fn build_use_skill_payload(
        skill_id: u8,
        subskill_id: u8,
        target_object_id: u32,
        position: (f32, f32, f32),
    ) -> Vec<u8> {
        let mut payload = Vec::with_capacity(USE_SKILL_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, USE_SKILL_MINOR]);
        payload.extend_from_slice(&(USE_SKILL_DECLARED_BYTES as u32).to_le_bytes());
        payload.push(skill_id);
        payload.push(subskill_id);
        payload.extend_from_slice(&target_object_id.to_le_bytes());
        payload.extend_from_slice(&position.0.to_le_bytes());
        payload.extend_from_slice(&position.1.to_le_bytes());
        payload.extend_from_slice(&position.2.to_le_bytes());
        payload.push(0x60);
        payload
    }

    fn build_toggle_mode_payload(mode: u8, counterspell_target: Option<u32>) -> Vec<u8> {
        let declared = TOGGLE_MODE_MIN_DECLARED_BYTES
            + usize::from(counterspell_target.is_some()) * OBJECT_ID_BYTES;
        let mut payload = Vec::with_capacity(declared + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, TOGGLE_MODE_MINOR]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.push(mode);
        if let Some(target) = counterspell_target {
            payload.extend_from_slice(&target.to_le_bytes());
        }
        payload.push(0x60);
        payload
    }

    fn build_memorize_spell_payload(
        class_index: u8,
        spell_id: u32,
        spell_level: u8,
        slot: u8,
        domain: u8,
    ) -> Vec<u8> {
        let mut payload = Vec::with_capacity(MEMORIZE_SPELL_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, MEMORIZE_SPELL_MINOR]);
        payload.extend_from_slice(&(MEMORIZE_SPELL_DECLARED_BYTES as u32).to_le_bytes());
        payload.push(class_index);
        payload.extend_from_slice(&spell_id.to_le_bytes());
        payload.push(spell_level);
        payload.push(slot);
        payload.push(domain);
        payload.push(0x60);
        payload
    }

    fn build_unmemorize_spell_payload(class_index: u8, spell_level: u8, slot: u8) -> Vec<u8> {
        let mut payload = Vec::with_capacity(UNMEMORIZE_SPELL_DECLARED_BYTES + ONE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[CLIENT_INPUT_ENVELOPE, INPUT_MAJOR, UNMEMORIZE_SPELL_MINOR]);
        payload.extend_from_slice(&(UNMEMORIZE_SPELL_DECLARED_BYTES as u32).to_le_bytes());
        payload.push(class_index);
        payload.push(spell_level);
        payload.push(slot);
        payload.push(0x60);
        payload
    }
}
