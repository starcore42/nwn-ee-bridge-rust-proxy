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
//! - Both decompiles handle `Login_WaypointResponse` (`0x02/0x0D`) as one
//!   `CExoString` bounded to `0x20` bytes. The empty-string form is a valid
//!   Diamond response to `Login_GetWaypoint` when no local waypoint tag exists.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const LOGIN_MAJOR: u8 = 0x02;
const SERVER_SUBDIRECTORY_CHARACTER_MINOR: u8 = 0x11;
const WAYPOINT_RESPONSE_MINOR: u8 = 0x0D;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const CEXOSTRING_LENGTH_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const CEXOSTRING_BYTES_OFFSET: usize = CEXOSTRING_LENGTH_OFFSET + CNW_LENGTH_BYTES;
const MAX_LOGIN_FRAGMENT_BYTES: usize = 8;
const MAX_CHARACTER_IDENTIFIER_BYTES: usize = 256;
const MAX_WAYPOINT_TAG_BYTES: usize = 0x20;
const FINAL_EMPTY_FRAGMENT_BYTE: u8 = 0x60;

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
    if high.major == LOGIN_MAJOR
        && high.minor == WAYPOINT_RESPONSE_MINOR
        && waypoint_response_payload_shape_valid(payload)
    {
        return Some(ClientLoginClaimSummary {
            packet_name: "Login_WaypointResponse",
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

pub fn waypoint_response_payload_shape_valid(payload: &[u8]) -> bool {
    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != LOGIN_MAJOR || high.minor != WAYPOINT_RESPONSE_MINOR {
        return false;
    }

    let Some(declared) =
        read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    let Some(tag_len) =
        read_le_u32(payload, CEXOSTRING_LENGTH_OFFSET).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };

    let expected_declared = CEXOSTRING_BYTES_OFFSET.saturating_add(tag_len);
    declared == expected_declared
        && tag_len <= MAX_WAYPOINT_TAG_BYTES
        && declared < payload.len()
        && payload.len() == declared + 1
        && payload[declared] == FINAL_EMPTY_FRAGMENT_BYTE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_empty_waypoint_response_shape() {
        let payload = [0x70, 0x02, 0x0D, 0x0B, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0x60];

        assert!(waypoint_response_payload_shape_valid(&payload));
        assert!(claim_payload_if_verified(&payload).is_some_and(|claim| {
            claim.packet_name == "Login_WaypointResponse"
        }));
    }

    #[test]
    fn rejects_waypoint_response_with_wrong_declared_end() {
        let payload = [0x70, 0x02, 0x0D, 0x0C, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0x60];

        assert!(!waypoint_response_payload_shape_valid(&payload));
    }

    #[test]
    fn rejects_waypoint_response_without_final_fragment_byte() {
        let payload = [0x70, 0x02, 0x0D, 0x0B, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0x00];

        assert!(!waypoint_response_payload_shape_valid(&payload));
    }
}
