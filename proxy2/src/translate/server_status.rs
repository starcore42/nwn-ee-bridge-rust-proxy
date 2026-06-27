//! Server-originated `ServerStatus` semantic claims.
//!
//! EE `CNWSMessage::SendServerToPlayerServerStatus_Status` is the mode/status
//! transition sent after module load completion. The writer emits only the
//! high-level envelope `P/01/01`: no CNW read window and no fragment cursor.
//! Keeping this tiny packet in a focused module prevents strict validation and
//! synthetic writers from treating any empty high-level wrapper as a status
//! transition.

use crate::packet::m::HighLevel;

const SERVER_STATUS_MAJOR: u8 = 0x01;
const STATUS_MINOR: u8 = 0x01;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ServerStatusClaimSummary {
    pub packet_name: &'static str,
}

pub fn status_payload() -> [u8; EMPTY_HIGH_LEVEL_BYTES] {
    [b'P', SERVER_STATUS_MAJOR, STATUS_MINOR]
}

pub fn claim_status_payload_if_verified(payload: &[u8]) -> Option<ServerStatusClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major == SERVER_STATUS_MAJOR
        && high.minor == STATUS_MINOR
        && payload.len() == EMPTY_HIGH_LEVEL_BYTES
    {
        return Some(ServerStatusClaimSummary {
            packet_name: "ServerStatus_Status",
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_exact_status_payload_only() {
        let exact = status_payload();
        let claim = claim_status_payload_if_verified(&exact)
            .expect("ServerStatus_Status should be an exact empty high-level payload");
        assert_eq!(claim.packet_name, "ServerStatus_Status");

        assert!(claim_status_payload_if_verified(&[b'P', SERVER_STATUS_MAJOR, 0x03]).is_none());
        assert!(
            claim_status_payload_if_verified(&[b'P', SERVER_STATUS_MAJOR, STATUS_MINOR, 0])
                .is_none()
        );
    }
}
