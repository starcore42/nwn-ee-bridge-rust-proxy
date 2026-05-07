//! Login packet semantic claims.
//!
//! `Login_Confirm` and `Login_GetWaypoint` are byte-identical no-body signals
//! in the EE send-side decompile: both functions call
//! `SendServerToPlayerMessage` with major `0x02`, minors `0x05` and `0x0C`,
//! a null payload pointer, and length zero. Diamond/HG captures arrive as the
//! same high-level `P major minor` envelope inside the reliable M layer.
//! Claiming them here keeps strict mode honest: they are pass-through only
//! because this module verified the empty shape.

use crate::packet::m::HighLevel;

const LOGIN_MAJOR: u8 = 0x02;
const LOGIN_CONFIRM_MINOR: u8 = 0x05;
const LOGIN_GET_WAYPOINT_MINOR: u8 = 0x0C;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct LoginClaimSummary {
    pub minor: u8,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<LoginClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != LOGIN_MAJOR
        || !matches!(high.minor, LOGIN_CONFIRM_MINOR | LOGIN_GET_WAYPOINT_MINOR)
        || payload.len() != EMPTY_HIGH_LEVEL_BYTES
    {
        return None;
    }

    Some(LoginClaimSummary { minor: high.minor })
}
