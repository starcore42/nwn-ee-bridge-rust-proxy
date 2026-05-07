//! Client-originated area semantic claims.
//!
//! EE `CNWSMessage::HandlePlayerToServerAreaMessage` dispatches family `0x04`
//! by minor id. The `Area_AreaLoaded` startup acknowledgement observed from
//! the harnessed EE client is a bare high-level signal, and the 1.69 server
//! handler consumes the same semantic acknowledgement without any EE-only body
//! fields. This module claims the byte-identical translation explicitly.

use crate::packet::m::HighLevel;

const AREA_MAJOR: u8 = 0x04;
const AREA_LOADED_MINOR: u8 = 0x03;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ClientAreaClaimSummary {
    pub packet_name: &'static str,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientAreaClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major == AREA_MAJOR
        && high.minor == AREA_LOADED_MINOR
        && payload.len() == EMPTY_HIGH_LEVEL_BYTES
    {
        return Some(ClientAreaClaimSummary {
            packet_name: "Area_AreaLoaded",
        });
    }
    None
}
