//! Client-originated login semantic claims.
//!
//! Decompile anchors:
//!
//! - EE `CNWSMessage::HandlePlayerToServerLoginMessage` dispatches family
//!   `0x02` by minor id.
//! - The EE packet-name table maps minor `0x11` to
//!   `Login_ServerSubDirectoryCharacter`.
//! - The harnessed EE client emits this request as a CNW declared read-message
//!   window containing the selected server-side character identifier, followed
//!   by the usual fragment byte. The 1.69 handler consumes the same semantic
//!   identifier; there are no EE-only fields to remove.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const LOGIN_MAJOR: u8 = 0x02;
const SERVER_SUBDIRECTORY_CHARACTER_MINOR: u8 = 0x11;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const MAX_LOGIN_FRAGMENT_BYTES: usize = 8;
const MAX_CHARACTER_IDENTIFIER_BYTES: usize = 256;

#[derive(Debug, Clone, Copy)]
pub struct ClientLoginClaimSummary {
    pub packet_name: &'static str,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientLoginClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major == LOGIN_MAJOR
        && high.minor == SERVER_SUBDIRECTORY_CHARACTER_MINOR
        && server_subdirectory_character_shape_valid(payload)
    {
        return Some(ClientLoginClaimSummary {
            packet_name: "Login_ServerSubDirectoryCharacter",
        });
    }
    None
}

fn server_subdirectory_character_shape_valid(payload: &[u8]) -> bool {
    let Some(declared) =
        read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_LOGIN_FRAGMENT_BYTES
    {
        return false;
    }

    let identifier = &payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared];
    !identifier.is_empty()
        && identifier.len() <= MAX_CHARACTER_IDENTIFIER_BYTES
        && identifier.iter().all(|byte| is_safe_identifier_byte(*byte))
}

fn is_safe_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}
