//! Party packet semantic claims.
//!
//! This module exists even for byte-identical party payloads so strict mode does
//! not accidentally treat the high-level opcode table as permission to forward
//! bytes. The translator asks one question: given this verified party CNW
//! wrapper, is the EE layout already the correct opposite-dialect layout?
//!
//! Decompile evidence:
//! - EE's message-name table maps `0x0E01..0x0E0E` to the Party family, but
//!   that table is only a classifier.  Every minor still needs a typed owner
//!   before strict mode may forward it.
//! - `CNWSMessage::SendServerToPlayerParty_List` creates a CNW write message,
//!   writes a 32-bit party-member count, writes one `WriteOBJECTIDServer`
//!   value per listed member, then sends family `0x0E` with the caller-supplied
//!   minor.
//! - The HG/1.69 empty-party capture is the same read-buffer shape:
//!   `P 0E 01`, declared `0x0B`, DWORD count `0`, plus one trailing fragment
//!   byte. No semantic bytes need to change; this module claims that exact
//!   shape as a verified no-op.
//! - Client `Party_GetList` is currently owned only as the exact no-body
//!   high-level signal.  Other party control minors remain unclaimed until
//!   their branch-specific reader payloads are modeled.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const PARTY_MAJOR: u8 = 0x0E;
const PARTY_LIST_MINOR: u8 = 0x01;
const PARTY_GET_LIST_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const PARTY_LIST_COUNT_BYTES: usize = 4;
const OBJECT_ID_BYTES: usize = 4;
const MAX_REASONABLE_PARTY_READ_BYTES: usize = 1024;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct PartyClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub read_bytes: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<PartyClaimSummary> {
    claim_server_payload_if_verified(payload).or_else(|| claim_client_payload_if_verified(payload))
}

pub fn claim_server_payload_if_verified(payload: &[u8]) -> Option<PartyClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (PARTY_MAJOR, PARTY_LIST_MINOR) => claim_party_list(payload, high.minor),
        _ => None,
    }
}

pub fn claim_client_payload_if_verified(payload: &[u8]) -> Option<PartyClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (PARTY_MAJOR, PARTY_GET_LIST_MINOR) => claim_party_get_list(payload, high.minor),
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
    (payload.len() == HIGH_LEVEL_HEADER_BYTES).then_some(PartyClaimSummary {
        minor,
        declared: HIGH_LEVEL_HEADER_BYTES,
        read_bytes: 0,
        fragment_bytes: 0,
    })
}

fn claim_party_cnw_wrapper(payload: &[u8], minor: u8) -> Option<PartyClaimSummary> {
    if payload.len() < READ_START {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START
        || declared > payload.len()
        || declared - READ_START > MAX_REASONABLE_PARTY_READ_BYTES
        || payload.len() != declared.checked_add(1)?
        || fragment_final_bit_count(payload.get(declared).copied()?) != CNW_FRAGMENT_HEADER_BITS
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

fn fragment_final_bit_count(tail: u8) -> usize {
    usize::from((tail & 0xE0) >> 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_party_list_with_matching_member_count() {
        let payload = party_list_payload(&[0x8000_0001, 0x8000_0002]);
        let claim = claim_payload_if_verified(&payload).expect("party list should claim");

        assert_eq!(claim.minor, PARTY_LIST_MINOR);
        assert_eq!(
            claim.declared,
            READ_START + PARTY_LIST_COUNT_BYTES + 2 * OBJECT_ID_BYTES
        );
        assert_eq!(
            claim.read_bytes,
            PARTY_LIST_COUNT_BYTES + 2 * OBJECT_ID_BYTES
        );
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_party_list_with_count_body_mismatch() {
        let mut payload = party_list_payload(&[0x8000_0001]);
        payload[READ_START..READ_START + 4].copy_from_slice(&2u32.to_le_bytes());

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_party_list_without_exact_empty_fragment_tail() {
        let mut shifted = party_list_payload(&[0x8000_0001]);
        *shifted.last_mut().expect("fragment tail") = 0x80;
        assert!(claim_payload_if_verified(&shifted).is_none());

        let mut slack = party_list_payload(&[0x8000_0001]);
        slack.push(0);
        assert!(claim_payload_if_verified(&slack).is_none());
    }

    #[test]
    fn accepts_party_list_with_nonzero_unused_fragment_padding_bits() {
        let mut payload = party_list_payload(&[]);
        *payload.last_mut().expect("fragment tail") = 0x7E;

        assert!(claim_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn claims_party_get_list_only_as_no_body_signal() {
        let no_body = [0x70, PARTY_MAJOR, PARTY_GET_LIST_MINOR];
        let claim = claim_payload_if_verified(&no_body).expect("party get-list should claim");

        assert_eq!(claim.minor, PARTY_GET_LIST_MINOR);
        assert_eq!(claim.declared, HIGH_LEVEL_HEADER_BYTES);
        assert_eq!(claim.read_bytes, 0);
        assert_eq!(claim.fragment_bytes, 0);

        let cnw_empty = [
            0x70,
            PARTY_MAJOR,
            PARTY_GET_LIST_MINOR,
            READ_START as u8,
            0,
            0,
            0,
            0x60,
        ];
        assert!(claim_payload_if_verified(&cnw_empty).is_none());
    }

    #[test]
    fn splits_server_list_and_client_get_list_owners() {
        let server_list = party_list_payload(&[0x8000_0001]);
        let client_get_list = [0x70, PARTY_MAJOR, PARTY_GET_LIST_MINOR];

        assert!(claim_server_payload_if_verified(&server_list).is_some());
        assert!(claim_client_payload_if_verified(&server_list).is_none());
        assert!(claim_client_payload_if_verified(&client_get_list).is_some());
        assert!(claim_server_payload_if_verified(&client_get_list).is_none());
    }

    #[test]
    fn rejects_unmodeled_party_control_minors_even_when_cnw_shaped() {
        let payload = [0x70, PARTY_MAJOR, 0x0E, READ_START as u8, 0, 0, 0, 0x60];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    fn party_list_payload(member_ids: &[u32]) -> Vec<u8> {
        let declared = READ_START + PARTY_LIST_COUNT_BYTES + member_ids.len() * OBJECT_ID_BYTES;
        let mut payload = vec![0x50, PARTY_MAJOR, PARTY_LIST_MINOR];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&(member_ids.len() as u32).to_le_bytes());
        for member_id in member_ids {
            payload.extend_from_slice(&member_id.to_le_bytes());
        }
        payload.push(0x60);
        payload
    }
}
