//! Login packet semantic claims.
//!
//! `Login_Confirm`, `Login_GetWaypoint`, and `Login_NeedCharacter` are
//! byte-identical no-body signals in the EE send-side decompile: the writer
//! calls `SendServerToPlayerMessage` with major `0x02`, the packet-specific
//! minor, a null payload pointer, and length zero. Diamond/HG captures arrive
//! as the same high-level `P major minor` envelope inside the reliable M layer.
//!
//! `Login_Fail` is also decompile-owned here. EE
//! `CNWSMessage::SendServerToPlayerLogin_Fail` creates a CNW write message for
//! one DWORD, writes that DWORD with `WriteDWORD(..., 0x20)`, then sends major
//! `0x02` / minor `0x12`. The EE-facing high-level payload therefore must be:
//!
//! `P 02 12`, little-endian declared offset `0x0B`, one little-endian DWORD,
//! and the final compact empty-fragment byte `0x60`.
//!
//! `Login_CharacterQuery` is decompile-owned from
//! `CNWSMessage::SendServerToPlayerLogin_CharacterQuery`: the writer creates a
//! 0x80-byte CNW write message, writes one BYTE count, then for each entry an
//! INT token plus BYTE flag, and finally one DWORD. The exact EE/1.69 high-level
//! shape is therefore identity-translated only when that declared cursor is
//! consumed exactly.
//!
//! These packets are pass-through only because this module verifies the exact
//! reader/writer shape before the strict layer allows them.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const LOGIN_MAJOR: u8 = 0x02;
const LOGIN_CONFIRM_MINOR: u8 = 0x05;
const LOGIN_CHARACTER_QUERY_MINOR: u8 = 0x0A;
const LOGIN_GET_WAYPOINT_MINOR: u8 = 0x0C;
const LOGIN_NEED_CHARACTER_MINOR: u8 = 0x10;
const LOGIN_FAIL_MINOR: u8 = 0x12;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const LOGIN_CHARACTER_QUERY_COUNT_BYTES: usize = 1;
const LOGIN_CHARACTER_QUERY_TOKEN_BYTES: usize = 4;
const LOGIN_CHARACTER_QUERY_FLAG_BYTES: usize = 1;
const LOGIN_CHARACTER_QUERY_FINAL_DWORD_BYTES: usize = 4;
const LOGIN_CHARACTER_QUERY_WRITER_BYTES: usize = 0x80;
const LOGIN_FAIL_DWORD_BYTES: usize = 4;
const LOGIN_FAIL_DECLARED: usize =
    HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + LOGIN_FAIL_DWORD_BYTES;
const FINAL_EMPTY_FRAGMENT_BYTE: u8 = 0x60;

#[derive(Debug, Clone, Copy)]
pub struct LoginClaimSummary {
    pub minor: u8,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<LoginClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != LOGIN_MAJOR {
        return None;
    }

    match high.minor {
        LOGIN_CONFIRM_MINOR | LOGIN_GET_WAYPOINT_MINOR | LOGIN_NEED_CHARACTER_MINOR
            if payload.len() == EMPTY_HIGH_LEVEL_BYTES =>
        {
            Some(LoginClaimSummary { minor: high.minor })
        }
        LOGIN_CHARACTER_QUERY_MINOR if character_query_shape_valid(payload) => {
            Some(LoginClaimSummary { minor: high.minor })
        }
        LOGIN_FAIL_MINOR if login_fail_shape_valid(payload) => {
            Some(LoginClaimSummary { minor: high.minor })
        }
        _ => None,
    }
}

pub fn character_query_shape_valid(payload: &[u8]) -> bool {
    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != LOGIN_MAJOR || high.minor != LOGIN_CHARACTER_QUERY_MINOR {
        return false;
    }

    let Some(declared) =
        read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    let Some(count) = payload
        .get(HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES)
        .copied()
        .map(usize::from)
    else {
        return false;
    };

    let per_entry = LOGIN_CHARACTER_QUERY_TOKEN_BYTES + LOGIN_CHARACTER_QUERY_FLAG_BYTES;
    let read_bytes = count
        .checked_mul(per_entry)
        .and_then(|entries| LOGIN_CHARACTER_QUERY_COUNT_BYTES.checked_add(entries))
        .and_then(|value| value.checked_add(LOGIN_CHARACTER_QUERY_FINAL_DWORD_BYTES));
    let Some(read_bytes) = read_bytes else {
        return false;
    };
    if read_bytes > LOGIN_CHARACTER_QUERY_WRITER_BYTES {
        return false;
    }

    let expected_declared = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)
        .and_then(|value| value.checked_add(read_bytes));

    expected_declared == Some(declared)
        && payload.len() == declared + 1
        && payload[declared] == FINAL_EMPTY_FRAGMENT_BYTE
}

pub fn login_fail_shape_valid(payload: &[u8]) -> bool {
    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != LOGIN_MAJOR || high.minor != LOGIN_FAIL_MINOR {
        return false;
    }

    let Some(declared) =
        read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };

    declared == LOGIN_FAIL_DECLARED
        && payload.len() == declared + 1
        && payload[declared] == FINAL_EMPTY_FRAGMENT_BYTE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_empty_server_login_signals() {
        for minor in [
            LOGIN_CONFIRM_MINOR,
            LOGIN_GET_WAYPOINT_MINOR,
            LOGIN_NEED_CHARACTER_MINOR,
        ] {
            let payload = [b'P', LOGIN_MAJOR, minor];
            assert!(
                claim_payload_if_verified(&payload).is_some_and(|claim| { claim.minor == minor })
            );
        }
    }

    #[test]
    fn claims_decompile_backed_login_fail_dword_shape() {
        let payload = [
            b'P',
            LOGIN_MAJOR,
            LOGIN_FAIL_MINOR,
            0x0B,
            0x00,
            0x00,
            0x00,
            0x45,
            0xE2,
            0x00,
            0x00,
            FINAL_EMPTY_FRAGMENT_BYTE,
        ];

        assert!(login_fail_shape_valid(&payload));
        assert!(
            claim_payload_if_verified(&payload)
                .is_some_and(|claim| { claim.minor == LOGIN_FAIL_MINOR })
        );
    }

    #[test]
    fn claims_decompile_backed_character_query_shape() {
        let mut payload = vec![
            b'P',
            LOGIN_MAJOR,
            LOGIN_CHARACTER_QUERY_MINOR,
            0,
            0,
            0,
            0,
            2,
        ];
        payload.extend_from_slice(&1234i32.to_le_bytes());
        payload.push(1);
        payload.extend_from_slice(&5678i32.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let declared = payload.len() as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload.push(FINAL_EMPTY_FRAGMENT_BYTE);

        assert!(character_query_shape_valid(&payload));
        assert!(
            claim_payload_if_verified(&payload)
                .is_some_and(|claim| { claim.minor == LOGIN_CHARACTER_QUERY_MINOR })
        );
    }

    #[test]
    fn rejects_character_query_with_unconsumed_cursor_or_bad_fragment() {
        let mut bad_declared = vec![
            b'P',
            LOGIN_MAJOR,
            LOGIN_CHARACTER_QUERY_MINOR,
            0,
            0,
            0,
            0,
            1,
        ];
        bad_declared.extend_from_slice(&1234i32.to_le_bytes());
        bad_declared.push(1);
        bad_declared.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let declared = (bad_declared.len() as u32) + 1;
        bad_declared[3..7].copy_from_slice(&declared.to_le_bytes());
        bad_declared.push(FINAL_EMPTY_FRAGMENT_BYTE);

        let mut bad_fragment = bad_declared.clone();
        let declared = (bad_fragment.len() - 1) as u32;
        bad_fragment[3..7].copy_from_slice(&declared.to_le_bytes());
        *bad_fragment.last_mut().unwrap() = 0x00;

        assert!(!character_query_shape_valid(&bad_declared));
        assert!(!character_query_shape_valid(&bad_fragment));
    }

    #[test]
    fn rejects_login_fail_without_exact_declared_fragment_boundary() {
        let bad_declared = [
            b'P',
            LOGIN_MAJOR,
            LOGIN_FAIL_MINOR,
            0x0C,
            0x00,
            0x00,
            0x00,
            0x45,
            0xE2,
            0x00,
            0x00,
            FINAL_EMPTY_FRAGMENT_BYTE,
        ];
        let bad_fragment = [
            b'P',
            LOGIN_MAJOR,
            LOGIN_FAIL_MINOR,
            0x0B,
            0x00,
            0x00,
            0x00,
            0x45,
            0xE2,
            0x00,
            0x00,
            0x00,
        ];

        assert!(!login_fail_shape_valid(&bad_declared));
        assert!(!login_fail_shape_valid(&bad_fragment));
    }
}
