//! Client-originated module semantic claims.
//!
//! EE `CNWSMessage::HandlePlayerToServerModuleMessage` dispatches family
//! `0x03` by minor id. The harnessed EE client sends `Module_Loaded`
//! (`0x03/0x02`) after receiving the area/module startup stream as a bare
//! high-level signal. The Diamond server handler expects the same empty signal,
//! so this translator owns the no-op pass-through claim explicitly.

use crate::packet::m::HighLevel;

const MODULE_MAJOR: u8 = 0x03;
const MODULE_LOADED_MINOR: u8 = 0x02;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ClientModuleClaimSummary {
    pub packet_name: &'static str,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientModuleClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major == MODULE_MAJOR
        && high.minor == MODULE_LOADED_MINOR
        && payload.len() == EMPTY_HIGH_LEVEL_BYTES
    {
        return Some(ClientModuleClaimSummary {
            packet_name: "Module_Loaded",
        });
    }
    None
}
