//! Non-live-object `GameObjUpdate` semantic claims.
//!
//! `GameObjUpdate_LiveObject` (`0x05/0x01`) stays in
//! `translate::live_object` and `translate::live_object_update`. This module
//! owns the sibling high-level families so they cannot become accidental
//! strict-pass-through packets.
//!
//! Decompile evidence:
//! - EE's message-name table identifies `0x05/0x02` as
//!   `GameObjUpdate_ObjList`.
//! - The EE server/client message path uses the normal CNW declared read
//!   buffer plus fragment tail for this family; the HG capture seen in
//!   driver-only mode is the same compact two-DWORD read-buffer shape, so the
//!   translator is an explicit verified no-op rather than an implicit allow.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const OBJ_LIST_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const OBJ_LIST_READ_BYTES: usize = 8;
const MAX_FRAGMENT_BYTES: usize = 8;

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
        (GAME_OBJECT_UPDATE_MAJOR, OBJ_LIST_MINOR) => claim_obj_list(payload, high.minor),
        _ => None,
    }
}

fn claim_obj_list(payload: &[u8], minor: u8) -> Option<GameObjUpdateClaimSummary> {
    if payload.len() < READ_START + OBJ_LIST_READ_BYTES {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != READ_START + OBJ_LIST_READ_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
    {
        return None;
    }

    Some(GameObjUpdateClaimSummary {
        minor,
        declared,
        read_bytes: OBJ_LIST_READ_BYTES,
        fragment_bytes: payload.len() - declared,
    })
}
