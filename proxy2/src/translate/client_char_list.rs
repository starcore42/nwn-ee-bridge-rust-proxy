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
//! - `CharList_RequestUpdateChar` dispatches minor `0x03` by reading one
//!   `BYTE(8)` request/status value, then one fixed-width `CResRef(16)`, then
//!   checking `MessageReadUnderflow`. The CResRef is a 16-byte binary field,
//!   not a variable ASCII identifier string, so NUL padding is legal.
//! - No EE-only fields were observed or found in this handler path, so the
//!   bridge validates the exact declared read window and leaves bytes unchanged.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const CHAR_LIST_MAJOR: u8 = 0x11;
const REQUEST_MINOR: u8 = 0x01;
const REQUEST_UPDATE_CHAR_MINOR: u8 = 0x03;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const UPDATE_REQUEST_TYPE_BYTES: usize = 1;
const UPDATE_REQUEST_CRESREF_BYTES: usize = 16;
const REQUEST_UPDATE_CHAR_DECLARED_BYTES: usize = HIGH_LEVEL_HEADER_BYTES
    + CNW_LENGTH_BYTES
    + UPDATE_REQUEST_TYPE_BYTES
    + UPDATE_REQUEST_CRESREF_BYTES;
const MAX_OBSERVED_REQUEST_UPDATE_CHAR_FRAGMENT_BYTES: usize = 1;

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
    if declared != REQUEST_UPDATE_CHAR_DECLARED_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_OBSERVED_REQUEST_UPDATE_CHAR_FRAGMENT_BYTES
    {
        return false;
    }

    let body_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    let cresref_start = body_start + UPDATE_REQUEST_TYPE_BYTES;
    payload.get(body_start).is_some()
        && payload
            .get(cresref_start..cresref_start + UPDATE_REQUEST_CRESREF_BYTES)
            .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_request_update_char_with_fixed_cresref_and_fragment_byte() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[b'P', CHAR_LIST_MAJOR, REQUEST_UPDATE_CHAR_MINOR]);
        payload.extend_from_slice(&(REQUEST_UPDATE_CHAR_DECLARED_BYTES as u32).to_le_bytes());
        payload.push(0x01);
        payload.extend_from_slice(b"starcore-druid60");
        payload.push(0x80);

        let summary = claim_payload_if_verified(&payload)
            .expect("decompile-backed RequestUpdateChar shape should be claimed");
        assert_eq!(summary.packet_name, "CharList_RequestUpdateChar");
    }

    #[test]
    fn claims_request_update_char_with_nul_padded_cresref() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[b'P', CHAR_LIST_MAJOR, REQUEST_UPDATE_CHAR_MINOR]);
        payload.extend_from_slice(&(REQUEST_UPDATE_CHAR_DECLARED_BYTES as u32).to_le_bytes());
        payload.push(0x00);
        payload.extend_from_slice(b"starcore");
        payload.extend_from_slice(&[0; UPDATE_REQUEST_CRESREF_BYTES - b"starcore".len()]);

        let summary = claim_payload_if_verified(&payload)
            .expect("CResRef NUL padding is part of the exact binary shape");
        assert_eq!(summary.packet_name, "CharList_RequestUpdateChar");
    }

    #[test]
    fn rejects_request_update_char_with_variable_identifier_length() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[b'P', CHAR_LIST_MAJOR, REQUEST_UPDATE_CHAR_MINOR]);
        payload.extend_from_slice(
            &((HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + UPDATE_REQUEST_TYPE_BYTES + 8) as u32)
                .to_le_bytes(),
        );
        payload.push(0x01);
        payload.extend_from_slice(b"starcore");

        assert!(claim_payload_if_verified(&payload).is_none());
    }
}
