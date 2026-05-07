//! Client-originated character-list semantic claims.
//!
//! Decompile anchors:
//!
//! - EE `CNWSMessage::HandlePlayerToServerCharListMessage` dispatches family
//!   `0x11` by minor id.
//! - The EE packet-name table maps `0x1101` to `CharList_Request` and `0x1103`
//!   to `CharList_RequestUpdateChar`.
//! - The harnessed EE client emits `CharList_Request` as an empty three-byte
//!   high-level envelope, which is the same shape the 1.69 server handler
//!   accepts.
//! - `CharList_RequestUpdateChar` is a bounded CNW read-message window carrying
//!   the selected character identifier, followed by the normal fragment byte.
//!   No EE-only fields were observed or found in the handler path, so the
//!   bridge validates the declared window exactly and leaves bytes unchanged.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const CHAR_LIST_MAJOR: u8 = 0x11;
const REQUEST_MINOR: u8 = 0x01;
const REQUEST_UPDATE_CHAR_MINOR: u8 = 0x03;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const UPDATE_REQUEST_TYPE_BYTES: usize = 1;
const MAX_CLIENT_CHARLIST_FRAGMENT_BYTES: usize = 8;
const MAX_CHARACTER_IDENTIFIER_BYTES: usize = 256;

#[derive(Debug, Clone, Copy)]
pub struct ClientCharListClaimSummary {
    pub packet_name: &'static str,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientCharListClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (CHAR_LIST_MAJOR, REQUEST_MINOR) if payload.len() == EMPTY_HIGH_LEVEL_BYTES => {
            Some(ClientCharListClaimSummary {
                packet_name: "CharList_Request",
            })
        }
        (CHAR_LIST_MAJOR, REQUEST_UPDATE_CHAR_MINOR)
            if request_update_char_shape_valid(payload) =>
        {
            Some(ClientCharListClaimSummary {
                packet_name: "CharList_RequestUpdateChar",
            })
        }
        _ => None,
    }
}

fn request_update_char_shape_valid(payload: &[u8]) -> bool {
    let Some(declared) =
        read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + UPDATE_REQUEST_TYPE_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_CLIENT_CHARLIST_FRAGMENT_BYTES
    {
        return false;
    }

    let body_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    let Some(request_type) = payload.get(body_start).copied() else {
        return false;
    };
    let identifier = &payload[body_start + UPDATE_REQUEST_TYPE_BYTES..declared];
    request_type <= 0x10
        && !identifier.is_empty()
        && identifier.len() <= MAX_CHARACTER_IDENTIFIER_BYTES
        && identifier.iter().all(|byte| is_safe_identifier_byte(*byte))
}

fn is_safe_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}
