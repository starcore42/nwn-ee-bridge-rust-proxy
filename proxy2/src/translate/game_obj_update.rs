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

use crate::{crc::read_le_u32, packet::m::HighLevel};

const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const OBJ_CONTROL_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const OBJ_CONTROL_READ_BYTES: usize = 8;
const OBJ_CONTROL_DECLARED_BYTES: usize = READ_START + OBJ_CONTROL_READ_BYTES;
const OBJ_CONTROL_FRAGMENT_BYTES: usize = 1;
const OBJ_CONTROL_PAYLOAD_BYTES: usize = OBJ_CONTROL_DECLARED_BYTES + OBJ_CONTROL_FRAGMENT_BYTES;

#[derive(Debug, Clone, Copy)]
pub struct GameObjUpdateClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub read_bytes: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<GameObjUpdateClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (GAME_OBJECT_UPDATE_MAJOR, OBJ_CONTROL_MINOR) => claim_obj_control(payload, high.minor),
        _ => None,
    }
}

fn claim_obj_control(payload: &[u8], minor: u8) -> Option<GameObjUpdateClaimSummary> {
    if payload.len() != OBJ_CONTROL_PAYLOAD_BYTES {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != OBJ_CONTROL_DECLARED_BYTES {
        return None;
    }

    Some(GameObjUpdateClaimSummary {
        minor,
        declared,
        read_bytes: OBJ_CONTROL_READ_BYTES,
        fragment_bytes: payload.len() - declared,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL_OBJ_CONTROL_PAYLOAD: [u8; OBJ_CONTROL_PAYLOAD_BYTES] = [
        0x50, 0x05, 0x02, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0xFF, 0xFF,
        0x73,
    ];

    #[test]
    fn obj_control_fixture_matches_decompile_cursor_shape() {
        let summary = claim_payload_if_verified(&LOCAL_OBJ_CONTROL_PAYLOAD)
            .expect("ObjControl fixture should be claimed");

        assert_eq!(summary.minor, OBJ_CONTROL_MINOR);
        assert_eq!(summary.declared, OBJ_CONTROL_DECLARED_BYTES);
        assert_eq!(summary.read_bytes, OBJ_CONTROL_READ_BYTES);
        assert_eq!(summary.fragment_bytes, OBJ_CONTROL_FRAGMENT_BYTES);
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
}
