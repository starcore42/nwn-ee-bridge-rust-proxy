//! Non-live-object `GameObjUpdate` semantic claims.
//!
//! `GameObjUpdate_LiveObject` (`0x05/0x01`) stays in
//! `translate::live_object` and `translate::live_object_update`. This module
//! owns the sibling high-level families so they cannot become accidental
//! strict-pass-through packets.
//!
//! Decompile evidence:
//! - EE's message-name table identifies `0x05/0x02` as
//!   `GameObjUpdate_ObjControl` (`nwn ee decompile.txt:1099736`).
//! - EE sender `CNWSMessage::SendServerToPlayerGameObjUpdate_ObjControl`
//!   (`nwn ee decompile.txt:1848042`) creates an 8-byte CNW write buffer,
//!   writes a DWORD player id and `WriteOBJECTIDServer` object id, then sends
//!   family `0x05`, minor `0x02`.
//! - The observed Diamond-compatible packet is the same compact two-DWORD
//!   read-buffer shape wrapped as `declared = 15` plus one CNW fragment byte,
//!   so the translator is an explicit verified no-op rather than an implicit
//!   allow.
//! - EE's message-name table identifies `0x05/0x03` as
//!   `GameObjUpdate_VisEffect` (`nwn ee decompile.txt:1099740`).
//! - EE sender `CNWSMessage::SendServerToPlayerGameObjUpdateVisEffect`
//!   (`nwn ee decompile.txt:1847610`) creates a CNW write message, writes the
//!   target object id, WORD visual-effect id, and either object-derived or
//!   caller-provided Vector floats before sending family `0x05`, minor `0x03`.
//!   The local XP2 captures hit the no-target compact branch: object id, WORD,
//!   three FLOATs, and one CNW fragment byte.
//! - EE's message-name table identifies `0x05/0x07` as
//!   `GameObjUpdate_DestroyItem` (`nwn ee decompile.txt:1099752`).
//! - EE sender `CNWSMessage::SendServerPlayerItemUpdate_DestroyItem`
//!   (`nwn ee decompile.txt:1830113`) creates a 4-byte CNW write buffer, writes
//!   only `WriteOBJECTIDServer(item)`, then sends family `0x05`, minor `0x07`.
//!   EE client `HandleServerToPlayerItemUpdate_DestroyItem` reads the matching
//!   raw 4-byte object id (`nwn ee decompile.txt:2897413`,
//!   `sub_1409737C0` at `nwn ee decompile.txt:3561098`). The local XP2
//!   Diamond-compatible captures match that one-object-id read window plus one
//!   CNW fragment byte.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const OBJ_CONTROL_MINOR: u8 = 0x02;
const VIS_EFFECT_MINOR: u8 = 0x03;
const DESTROY_ITEM_MINOR: u8 = 0x07;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const OBJ_CONTROL_READ_BYTES: usize = 8;
const OBJ_CONTROL_DECLARED_BYTES: usize = READ_START + OBJ_CONTROL_READ_BYTES;
const OBJ_CONTROL_PAYLOAD_BYTES: usize = OBJ_CONTROL_DECLARED_BYTES + SINGLE_FRAGMENT_BYTE;
const VIS_EFFECT_READ_BYTES: usize = 4 + 2 + (3 * 4);
const VIS_EFFECT_DECLARED_BYTES: usize = READ_START + VIS_EFFECT_READ_BYTES;
const VIS_EFFECT_PAYLOAD_BYTES: usize = VIS_EFFECT_DECLARED_BYTES + SINGLE_FRAGMENT_BYTE;
const DESTROY_ITEM_READ_BYTES: usize = 4;
const DESTROY_ITEM_DECLARED_BYTES: usize = READ_START + DESTROY_ITEM_READ_BYTES;
const DESTROY_ITEM_PAYLOAD_BYTES: usize = DESTROY_ITEM_DECLARED_BYTES + SINGLE_FRAGMENT_BYTE;

#[derive(Debug, Clone, Copy)]
pub struct GameObjUpdateClaimSummary {
    pub minor: u8,
    pub packet_name: &'static str,
    pub declared: usize,
    pub read_bytes: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<GameObjUpdateClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (GAME_OBJECT_UPDATE_MAJOR, OBJ_CONTROL_MINOR) => {
            claim_obj_control_payload_if_verified(payload)
        }
        (GAME_OBJECT_UPDATE_MAJOR, VIS_EFFECT_MINOR) => {
            claim_vis_effect_payload_if_verified(payload)
        }
        (GAME_OBJECT_UPDATE_MAJOR, DESTROY_ITEM_MINOR) => {
            claim_destroy_item_payload_if_verified(payload)
        }
        _ => None,
    }
}

pub fn claim_obj_control_payload_if_verified(payload: &[u8]) -> Option<GameObjUpdateClaimSummary> {
    let message = parse_game_obj_update_message(payload, OBJ_CONTROL_MINOR)?;
    let rewritten = message.to_ee_payload();
    (rewritten == payload).then(|| message.summary())
}

pub fn claim_vis_effect_payload_if_verified(payload: &[u8]) -> Option<GameObjUpdateClaimSummary> {
    let message = parse_game_obj_update_message(payload, VIS_EFFECT_MINOR)?;
    let rewritten = message.to_ee_payload();
    (rewritten == payload).then(|| message.summary())
}

pub fn claim_destroy_item_payload_if_verified(payload: &[u8]) -> Option<GameObjUpdateClaimSummary> {
    let message = parse_game_obj_update_message(payload, DESTROY_ITEM_MINOR)?;
    let rewritten = message.to_ee_payload();
    (rewritten == payload).then(|| message.summary())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameObjUpdateMessage {
    ObjControl {
        player_id: u32,
        object_id: u32,
        fragment_tail: u8,
    },
    VisEffectSimple {
        object_id: u32,
        effect_id: u16,
        position_bits: [u32; 3],
        fragment_tail: u8,
    },
    DestroyItem {
        object_id: u32,
        fragment_tail: u8,
    },
}

fn parse_game_obj_update_message(payload: &[u8], minor: u8) -> Option<GameObjUpdateMessage> {
    let high = HighLevel::parse(payload)?;
    if high.major != GAME_OBJECT_UPDATE_MAJOR || high.minor != minor {
        return None;
    }

    match minor {
        OBJ_CONTROL_MINOR => parse_obj_control_payload(payload),
        VIS_EFFECT_MINOR => parse_vis_effect_simple_payload(payload),
        DESTROY_ITEM_MINOR => parse_destroy_item_payload(payload),
        _ => None,
    }
}

fn parse_obj_control_payload(payload: &[u8]) -> Option<GameObjUpdateMessage> {
    let declared = exact_declared(payload, OBJ_CONTROL_DECLARED_BYTES)?;
    let fragment_tail = exact_single_empty_fragment_tail(payload, declared)?;
    Some(GameObjUpdateMessage::ObjControl {
        player_id: read_le_u32(payload, READ_START)?,
        object_id: read_le_u32(payload, READ_START + 4)?,
        fragment_tail,
    })
}

fn parse_vis_effect_simple_payload(payload: &[u8]) -> Option<GameObjUpdateMessage> {
    let declared = exact_declared(payload, VIS_EFFECT_DECLARED_BYTES)?;
    let fragment_tail = exact_single_empty_fragment_tail(payload, declared)?;
    let cursor = READ_START;
    Some(GameObjUpdateMessage::VisEffectSimple {
        object_id: read_le_u32(payload, cursor)?,
        effect_id: read_le_u16(payload, cursor + 4)?,
        position_bits: [
            read_le_u32(payload, cursor + 6)?,
            read_le_u32(payload, cursor + 10)?,
            read_le_u32(payload, cursor + 14)?,
        ],
        fragment_tail,
    })
}

fn parse_destroy_item_payload(payload: &[u8]) -> Option<GameObjUpdateMessage> {
    let declared = exact_declared(payload, DESTROY_ITEM_DECLARED_BYTES)?;
    let fragment_tail = exact_single_empty_fragment_tail(payload, declared)?;
    Some(GameObjUpdateMessage::DestroyItem {
        object_id: read_le_u32(payload, READ_START)?,
        fragment_tail,
    })
}

fn exact_declared(payload: &[u8], expected_declared: usize) -> Option<usize> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    (declared == expected_declared).then_some(declared)
}

fn exact_single_empty_fragment_tail(payload: &[u8], declared: usize) -> Option<u8> {
    if payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)? {
        return None;
    }
    let tail = *payload.get(declared)?;
    let final_bits = usize::from((tail & 0xE0) >> 5);
    (final_bits == CNW_FRAGMENT_HEADER_BITS).then_some(tail)
}

fn read_le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

impl GameObjUpdateMessage {
    fn packet_name(self) -> &'static str {
        match self {
            Self::ObjControl { .. } => "GameObjUpdate_ObjControl",
            Self::VisEffectSimple { .. } => "GameObjUpdate_VisEffect",
            Self::DestroyItem { .. } => "GameObjUpdate_DestroyItem",
        }
    }

    fn declared(self) -> usize {
        match self {
            Self::ObjControl { .. } => OBJ_CONTROL_DECLARED_BYTES,
            Self::VisEffectSimple { .. } => VIS_EFFECT_DECLARED_BYTES,
            Self::DestroyItem { .. } => DESTROY_ITEM_DECLARED_BYTES,
        }
    }

    fn read_bytes(self) -> usize {
        match self {
            Self::ObjControl { .. } => OBJ_CONTROL_READ_BYTES,
            Self::VisEffectSimple { .. } => VIS_EFFECT_READ_BYTES,
            Self::DestroyItem { .. } => DESTROY_ITEM_READ_BYTES,
        }
    }

    fn minor(self) -> u8 {
        match self {
            Self::ObjControl { .. } => OBJ_CONTROL_MINOR,
            Self::VisEffectSimple { .. } => VIS_EFFECT_MINOR,
            Self::DestroyItem { .. } => DESTROY_ITEM_MINOR,
        }
    }

    fn fragment_tail(self) -> u8 {
        match self {
            Self::ObjControl { fragment_tail, .. }
            | Self::VisEffectSimple { fragment_tail, .. }
            | Self::DestroyItem { fragment_tail, .. } => fragment_tail,
        }
    }

    fn summary(self) -> GameObjUpdateClaimSummary {
        GameObjUpdateClaimSummary {
            minor: self.minor(),
            packet_name: self.packet_name(),
            declared: self.declared(),
            read_bytes: self.read_bytes(),
            fragment_bytes: SINGLE_FRAGMENT_BYTE,
        }
    }

    fn to_ee_payload(self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(self.declared() + SINGLE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[b'P', GAME_OBJECT_UPDATE_MAJOR, self.minor()]);
        payload.extend_from_slice(&(self.declared() as u32).to_le_bytes());
        match self {
            Self::ObjControl {
                player_id,
                object_id,
                ..
            } => {
                payload.extend_from_slice(&player_id.to_le_bytes());
                payload.extend_from_slice(&object_id.to_le_bytes());
            }
            Self::VisEffectSimple {
                object_id,
                effect_id,
                position_bits,
                ..
            } => {
                payload.extend_from_slice(&object_id.to_le_bytes());
                payload.extend_from_slice(&effect_id.to_le_bytes());
                for bits in position_bits {
                    payload.extend_from_slice(&bits.to_le_bytes());
                }
            }
            Self::DestroyItem { object_id, .. } => {
                payload.extend_from_slice(&object_id.to_le_bytes());
            }
        }
        payload.push(self.fragment_tail());
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL_OBJ_CONTROL_PAYLOAD: [u8; OBJ_CONTROL_PAYLOAD_BYTES] = [
        0x50, 0x05, 0x02, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0xFF, 0xFF,
        0x73,
    ];
    const LOCAL_XP2_VIS_EFFECT_PAYLOAD: [u8; VIS_EFFECT_PAYLOAD_BYTES] = [
        0x50, 0x05, 0x03, 0x19, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x80, 0x25, 0x00, 0xD8, 0x49,
        0x6F, 0x41, 0x46, 0x1E, 0xC0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x61,
    ];
    const LOCAL_XP2_DESTROY_ITEM_PAYLOAD_A: [u8; DESTROY_ITEM_PAYLOAD_BYTES] = [
        0x50, 0x05, 0x07, 0x0B, 0x00, 0x00, 0x00, 0x67, 0x2C, 0x00, 0x80, 0x7C,
    ];
    const LOCAL_XP2_DESTROY_ITEM_PAYLOAD_B: [u8; DESTROY_ITEM_PAYLOAD_BYTES] = [
        0x50, 0x05, 0x07, 0x0B, 0x00, 0x00, 0x00, 0x68, 0x2C, 0x00, 0x80, 0x7C,
    ];

    #[test]
    fn obj_control_fixture_matches_decompile_cursor_shape() {
        let summary = claim_payload_if_verified(&LOCAL_OBJ_CONTROL_PAYLOAD)
            .expect("ObjControl fixture should be claimed");

        assert_eq!(summary.minor, OBJ_CONTROL_MINOR);
        assert_eq!(summary.packet_name, "GameObjUpdate_ObjControl");
        assert_eq!(summary.declared, OBJ_CONTROL_DECLARED_BYTES);
        assert_eq!(summary.read_bytes, OBJ_CONTROL_READ_BYTES);
        assert_eq!(summary.fragment_bytes, SINGLE_FRAGMENT_BYTE);
    }

    #[test]
    fn vis_effect_fixture_matches_no_target_decompile_cursor_shape() {
        let summary = claim_payload_if_verified(&LOCAL_XP2_VIS_EFFECT_PAYLOAD)
            .expect("VisEffect fixture should be claimed");

        assert_eq!(summary.minor, VIS_EFFECT_MINOR);
        assert_eq!(summary.packet_name, "GameObjUpdate_VisEffect");
        assert_eq!(summary.declared, VIS_EFFECT_DECLARED_BYTES);
        assert_eq!(summary.read_bytes, VIS_EFFECT_READ_BYTES);
        assert_eq!(summary.fragment_bytes, SINGLE_FRAGMENT_BYTE);
    }

    #[test]
    fn destroy_item_fixtures_match_decompile_cursor_shape() {
        for payload in [
            LOCAL_XP2_DESTROY_ITEM_PAYLOAD_A,
            LOCAL_XP2_DESTROY_ITEM_PAYLOAD_B,
        ] {
            let summary =
                claim_payload_if_verified(&payload).expect("DestroyItem fixture should be claimed");

            assert_eq!(summary.minor, DESTROY_ITEM_MINOR);
            assert_eq!(summary.packet_name, "GameObjUpdate_DestroyItem");
            assert_eq!(summary.declared, DESTROY_ITEM_DECLARED_BYTES);
            assert_eq!(summary.read_bytes, DESTROY_ITEM_READ_BYTES);
            assert_eq!(summary.fragment_bytes, SINGLE_FRAGMENT_BYTE);
        }
    }

    #[test]
    fn obj_control_rejects_extra_fragment_bytes() {
        let mut payload = LOCAL_OBJ_CONTROL_PAYLOAD.to_vec();
        payload.push(0);

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn obj_control_rejects_wrong_declared_boundary() {
        let mut payload = LOCAL_OBJ_CONTROL_PAYLOAD;
        payload[HIGH_LEVEL_HEADER_BYTES] = 0x10;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn vis_effect_rejects_stale_declared_boundary() {
        let mut payload = LOCAL_XP2_VIS_EFFECT_PAYLOAD;
        payload[HIGH_LEVEL_HEADER_BYTES] = 0x18;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn vis_effect_rejects_wrong_fragment_final_bits() {
        let mut payload = LOCAL_XP2_VIS_EFFECT_PAYLOAD;
        payload[VIS_EFFECT_DECLARED_BYTES] = 0x41;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn destroy_item_rejects_stale_declared_boundary() {
        let mut payload = LOCAL_XP2_DESTROY_ITEM_PAYLOAD_A;
        payload[HIGH_LEVEL_HEADER_BYTES] = 0x0C;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn destroy_item_rejects_wrong_fragment_final_bits() {
        let mut payload = LOCAL_XP2_DESTROY_ITEM_PAYLOAD_A;
        payload[DESTROY_ITEM_DECLARED_BYTES] = 0x1C;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp2_vis_effect_fixture_is_exact_noop_claim() {
        let payload = include_bytes!(
            "../../fixtures/game_obj_update/local_xp2_game_obj_update_vis_effect_20260522.bin"
        );

        assert!(claim_vis_effect_payload_if_verified(payload).is_some());
    }
}
