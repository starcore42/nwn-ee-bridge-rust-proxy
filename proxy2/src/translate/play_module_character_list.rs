//! Client-originated `PlayModuleCharacterList` semantic claims.
//!
//! Decompile anchors:
//!
//! - EE `CNWSMessage::HandlePlayerToServerPlayModuleCharacterList` dispatches
//!   minor `0x01` to `_Start` and minor `0x02` to `_Stop`.
//! - The handler checks `MessageReadOverflow` before dispatching and does not
//!   read a CNW body for either startup packet.
//! - The EE packet-name table maps `0x3101` and `0x3102` to the same
//!   Start/Stop names. The EE and Diamond shapes are therefore the same
//!   three-byte high-level envelope; this module claims that no-op transform.

use crate::packet::m::HighLevel;

const PLAY_MODULE_CHARACTER_LIST_MAJOR: u8 = 0x31;
const START_MINOR: u8 = 0x01;
const STOP_MINOR: u8 = 0x02;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct PlayModuleCharacterListClaimSummary {
    pub packet_name: &'static str,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<PlayModuleCharacterListClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != PLAY_MODULE_CHARACTER_LIST_MAJOR || payload.len() != EMPTY_HIGH_LEVEL_BYTES {
        return None;
    }

    match high.minor {
        START_MINOR => Some(PlayModuleCharacterListClaimSummary {
            packet_name: "PlayModuleCharacterList_Start",
        }),
        STOP_MINOR => Some(PlayModuleCharacterListClaimSummary {
            packet_name: "PlayModuleCharacterList_Stop",
        }),
        _ => None,
    }
}
