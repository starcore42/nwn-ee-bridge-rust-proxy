//! Party packet semantic claims.
//!
//! This module exists even for byte-identical party payloads so strict mode does
//! not accidentally treat the high-level opcode table as permission to forward
//! bytes. The translator asks one question: given this verified party CNW
//! wrapper, is the EE layout already the correct opposite-dialect layout?
//!
//! Decompile evidence:
//! - EE's message-name table maps `0x0E01..0x0E0E` to the Party family.
//! - `CNWSMessage::SendServerToPlayerParty_List` creates a CNW write message,
//!   writes a 32-bit party-member count, writes one `WriteOBJECTIDServer`
//!   value per listed member, then sends family `0x0E` with the caller-supplied
//!   minor.
//! - The HG/1.69 empty-party capture is the same read-buffer shape:
//!   `P 0E 01`, declared `0x0B`, DWORD count `0`, plus one trailing fragment
//!   byte. No semantic bytes need to change; this module claims that exact
//!   shape as a verified no-op.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const PARTY_MAJOR: u8 = 0x0E;
const PARTY_LIST_MINOR: u8 = 0x01;
const PARTY_GET_LIST_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const PARTY_LIST_COUNT_BYTES: usize = 4;
const OBJECT_ID_BYTES: usize = 4;
const MAX_FRAGMENT_BYTES: usize = 32;
const MAX_REASONABLE_PARTY_READ_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy)]
pub struct PartyClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub read_bytes: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<PartyClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (PARTY_MAJOR, PARTY_LIST_MINOR) => claim_party_list(payload, high.minor),
        (PARTY_MAJOR, PARTY_GET_LIST_MINOR) => claim_party_get_list(payload, high.minor),
        (PARTY_MAJOR, 0x03..=0x0E) => claim_party_cnw_wrapper(payload, high.minor),
        _ => None,
    }
}

fn claim_party_list(payload: &[u8], minor: u8) -> Option<PartyClaimSummary> {
    let summary = claim_party_cnw_wrapper(payload, minor)?;
    if summary.read_bytes < PARTY_LIST_COUNT_BYTES {
        return None;
    }

    let count = usize::try_from(read_le_u32(payload, READ_START)?).ok()?;
    let expected_read_bytes =
        PARTY_LIST_COUNT_BYTES.checked_add(count.checked_mul(OBJECT_ID_BYTES)?)?;
    if summary.read_bytes != expected_read_bytes {
        return None;
    }

    Some(summary)
}

fn claim_party_get_list(payload: &[u8], minor: u8) -> Option<PartyClaimSummary> {
    if payload.len() == HIGH_LEVEL_HEADER_BYTES {
        return Some(PartyClaimSummary {
            minor,
            declared: HIGH_LEVEL_HEADER_BYTES,
            read_bytes: 0,
            fragment_bytes: 0,
        });
    }
    claim_party_cnw_wrapper(payload, minor)
}

fn claim_party_cnw_wrapper(payload: &[u8], minor: u8) -> Option<PartyClaimSummary> {
    if payload.len() < READ_START {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START
        || declared > payload.len()
        || declared - READ_START > MAX_REASONABLE_PARTY_READ_BYTES
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
    {
        return None;
    }

    Some(PartyClaimSummary {
        minor,
        declared,
        read_bytes: declared - READ_START,
        fragment_bytes: payload.len() - declared,
    })
}
