//! EE `LoadBar_*` payload construction and identity claims.
//!
//! This module answers one narrow question:
//! "Given a verified semantic load-bar event, what exact EE-facing high-level
//! payload bytes should be emitted?"
//!
//! Decompile anchors used for these payloads:
//!
//! - EE `CNWSMessage::SendServerToPlayerLoadBar_StartStallEvent` creates a
//!   CNW write message, writes one 32-bit stall-event id, obtains the write
//!   buffer, then sends high-level family `0x2C` minor `0x01`.
//! - EE `CNWSMessage::SendServerToPlayerLoadBar_EndStallEvent` writes the same
//!   32-bit stall-event id followed by a 4-bit result code, then sends
//!   high-level family `0x2C` minor `0x03`.
//! - EE `CNWSMessage::SendServerToPlayerLoadBar_UpdateStallEvent` writes the
//!   stall-event id and a 32-bit progress value, then sends minor `0x02`.
//! - Diamond's packet-name table exposes the same `LoadBar` high-level family.
//!   Observed 1.69/HG load-bar payloads use the same declared CNW read-window
//!   shape, so native load-bar traffic is an explicit verified no-op claim:
//!   the translator validates the read cursor exactly and changes no bytes.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const LOADBAR_MAJOR: u8 = 0x2C;
const LOADBAR_START_MINOR: u8 = 0x01;
const LOADBAR_UPDATE_MINOR: u8 = 0x02;
const LOADBAR_END_MINOR: u8 = 0x03;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;

const LOADBAR_ONE_DWORD_DECLARED: u32 = (READ_START + CNW_LENGTH_BYTES) as u32;
const LOADBAR_TWO_DWORD_DECLARED: u32 = (READ_START + 2 * CNW_LENGTH_BYTES) as u32;

const START_FRAGMENT_BYTE: u8 = 0x60;
const END_SUCCESS_FRAGMENT_BYTE: u8 = 0xE0;
const UPDATE_FRAGMENT_BYTE: u8 = 0x60;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const END_RESULT_BITS: usize = 4;

#[derive(Debug, Clone, Copy)]
pub struct LoadBarClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub read_dwords: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<LoadBarClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.envelope != HIGH_LEVEL_ENVELOPE || high.major != LOADBAR_MAJOR {
        return None;
    }

    let expected_declared = match high.minor {
        LOADBAR_START_MINOR | LOADBAR_END_MINOR => LOADBAR_ONE_DWORD_DECLARED as usize,
        LOADBAR_UPDATE_MINOR => LOADBAR_TWO_DWORD_DECLARED as usize,
        _ => return None,
    };
    if payload.len() < expected_declared {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != expected_declared || declared >= payload.len() {
        return None;
    }
    let expected_fragment_bits = match high.minor {
        LOADBAR_START_MINOR | LOADBAR_UPDATE_MINOR => CNW_FRAGMENT_HEADER_BITS,
        LOADBAR_END_MINOR => CNW_FRAGMENT_HEADER_BITS + END_RESULT_BITS,
        _ => return None,
    };
    if !exact_single_fragment_tail(payload, declared, expected_fragment_bits) {
        return None;
    }

    let read_dwords = match high.minor {
        LOADBAR_START_MINOR | LOADBAR_END_MINOR => 1,
        LOADBAR_UPDATE_MINOR => 2,
        _ => return None,
    };
    Some(LoadBarClaimSummary {
        minor: high.minor,
        declared,
        read_dwords,
        fragment_bytes: payload.len() - declared,
    })
}

fn exact_single_fragment_tail(payload: &[u8], declared: usize, expected_final_bits: usize) -> bool {
    let tail = match payload.get(declared..) {
        Some([tail]) => *tail,
        _ => return false,
    };
    usize::from((tail & 0xE0) >> 5) == expected_final_bits
}

pub fn start_payload(stall_event_id: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    payload.push(HIGH_LEVEL_ENVELOPE);
    payload.push(LOADBAR_MAJOR);
    payload.push(LOADBAR_START_MINOR);
    payload.extend_from_slice(&LOADBAR_ONE_DWORD_DECLARED.to_le_bytes());
    payload.extend_from_slice(&stall_event_id.to_le_bytes());
    payload.push(START_FRAGMENT_BYTE);
    payload
}

pub fn end_success_payload(stall_event_id: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    payload.push(HIGH_LEVEL_ENVELOPE);
    payload.push(LOADBAR_MAJOR);
    payload.push(LOADBAR_END_MINOR);
    payload.extend_from_slice(&LOADBAR_ONE_DWORD_DECLARED.to_le_bytes());
    payload.extend_from_slice(&stall_event_id.to_le_bytes());
    payload.push(END_SUCCESS_FRAGMENT_BYTE);
    payload
}

#[allow(dead_code)]
pub fn update_payload(stall_event_id: u32, progress: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16);
    payload.push(HIGH_LEVEL_ENVELOPE);
    payload.push(LOADBAR_MAJOR);
    payload.push(LOADBAR_UPDATE_MINOR);
    payload.extend_from_slice(&LOADBAR_TWO_DWORD_DECLARED.to_le_bytes());
    payload.extend_from_slice(&stall_event_id.to_le_bytes());
    payload.extend_from_slice(&progress.to_le_bytes());
    payload.push(UPDATE_FRAGMENT_BYTE);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_exact_start_update_and_end_fragment_tails() {
        let start = start_payload(2);
        let update = update_payload(2, 50);
        let end = end_success_payload(2);

        let start_claim = claim_payload_if_verified(&start).expect("LoadBar_Start should claim");
        assert_eq!(start_claim.minor, LOADBAR_START_MINOR);
        assert_eq!(start_claim.declared, LOADBAR_ONE_DWORD_DECLARED as usize);
        assert_eq!(start_claim.read_dwords, 1);
        assert_eq!(start_claim.fragment_bytes, 1);

        let update_claim = claim_payload_if_verified(&update).expect("LoadBar_Update should claim");
        assert_eq!(update_claim.minor, LOADBAR_UPDATE_MINOR);
        assert_eq!(update_claim.declared, LOADBAR_TWO_DWORD_DECLARED as usize);
        assert_eq!(update_claim.read_dwords, 2);
        assert_eq!(update_claim.fragment_bytes, 1);

        let end_claim = claim_payload_if_verified(&end).expect("LoadBar_End should claim");
        assert_eq!(end_claim.minor, LOADBAR_END_MINOR);
        assert_eq!(end_claim.declared, LOADBAR_ONE_DWORD_DECLARED as usize);
        assert_eq!(end_claim.read_dwords, 1);
        assert_eq!(end_claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_loadbar_fragment_tails_with_wrong_final_bit_count() {
        let mut start = start_payload(2);
        *start.last_mut().expect("fragment tail") = END_SUCCESS_FRAGMENT_BYTE;
        assert!(
            claim_payload_if_verified(&start).is_none(),
            "LoadBar_Start owns no result bits"
        );

        let mut update = update_payload(2, 50);
        *update.last_mut().expect("fragment tail") = END_SUCCESS_FRAGMENT_BYTE;
        assert!(
            claim_payload_if_verified(&update).is_none(),
            "LoadBar_Update owns read-buffer DWORDs, not result bits"
        );

        let mut end = end_success_payload(2);
        *end.last_mut().expect("fragment tail") = START_FRAGMENT_BYTE;
        assert!(
            claim_payload_if_verified(&end).is_none(),
            "LoadBar_End must own the four result bits"
        );
    }

    #[test]
    fn rejects_loadbar_fragment_tail_slack() {
        let mut start = start_payload(2);
        start.push(0);

        assert!(
            claim_payload_if_verified(&start).is_none(),
            "LoadBar has no decompiled multi-byte fragment tail owner"
        );
    }
}
