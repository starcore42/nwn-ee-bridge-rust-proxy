//! Client-originated `ServerStatus` semantic claims.
//!
//! EE and Diamond both route the initial client `ServerStatus_0` ping through
//! `CNWSMessage::HandlePlayerToServerMessage` as high-level family `0x01`,
//! minor `0x00`. The EE packet-name table identifies the same opcode, and the
//! harness capture contains only the three-byte high-level envelope. No
//! dialect bytes exist to rewrite, but this module still owns the no-op claim
//! so strict mode never relies on the opcode classifier as an allow decision.

use crate::packet::m::HighLevel;

const SERVER_STATUS_MAJOR: u8 = 0x01;
const SERVER_STATUS_0_MINOR: u8 = 0x00;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ClientServerStatusClaimSummary {
    pub packet_name: &'static str,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientServerStatusClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major == SERVER_STATUS_MAJOR
        && high.minor == SERVER_STATUS_0_MINOR
        && payload.len() == EMPTY_HIGH_LEVEL_BYTES
    {
        return Some(ClientServerStatusClaimSummary {
            packet_name: "ServerStatus_0",
        });
    }
    None
}
