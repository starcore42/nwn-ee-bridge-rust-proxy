//! Custom token server-to-client payload translation.
//!
//! Decompile evidence:
//!
//! - EE `CNWSMessage::SendServerToPlayerSetCustomToken` sends high-level
//!   family `0x32`, minor `0x01`, then writes a DWORD token id and one
//!   `CExoString`.
//! - EE `CNWSMessage::SendServerToPlayerSetCustomTokenList` sends high-level
//!   family `0x32`, minor `0x02`, then writes a DWORD token count followed by
//!   `(DWORD token id, CExoString)` records. When the count is zero, the read
//!   window contains only the count.
//!
//! The HG/1.69 stream has been observed sending malformed legacy list payloads
//! whose declared CNW fragment offset is impossible for EE. Do not pass those
//! through. If a custom-token payload is not an exact EE CNW shape, emit the
//! narrowest valid no-op list update: `P 32 02`, declared `11`, count `0`,
//! plus one fragment terminator byte.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const CUSTOM_TOKEN_MAJOR: u8 = 0x32;
const CUSTOM_TOKEN_SET_MINOR: u8 = 0x01;
const CUSTOM_TOKEN_LIST_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const EMPTY_LIST_DECLARED: u32 = (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 4) as u32;
const MAX_CUSTOM_TOKEN_COUNT: usize = 4096;
const MAX_CUSTOM_TOKEN_STRING_BYTES: usize = 4096;
const MAX_CUSTOM_TOKEN_FRAGMENT_BYTES: usize = 1;

#[derive(Debug, Clone)]
pub struct CustomTokenRewriteSummary {
    pub source_minor: u8,
    pub old_declared_present: bool,
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub old_token_count: u32,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct CustomTokenClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub token_count: u32,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<CustomTokenClaimSummary> {
    if !is_custom_token_payload(payload) {
        return None;
    }

    let parsed = parse_valid_custom_token_payload(payload)?;
    if !parsed.valid {
        return None;
    }

    Some(CustomTokenClaimSummary {
        minor: payload[2],
        declared: read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)? as usize,
        token_count: observed_token_count(payload).unwrap_or(0),
    })
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut Vec<u8>,
) -> Option<CustomTokenRewriteSummary> {
    if !is_custom_token_payload(payload) {
        return None;
    }

    if let Some(parsed) = parse_valid_custom_token_payload(payload) {
        if parsed.valid {
            return Some(CustomTokenRewriteSummary {
                source_minor: payload[2],
                old_declared_present: true,
                old_declared: read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?,
                new_declared: read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?,
                old_payload_length: payload.len(),
                new_payload_length: payload.len(),
                old_token_count: observed_token_count(payload).unwrap_or(0),
                reason: "verified-ee-compatible-custom-token-cnw-window",
            });
        }
    }

    rewrite_payload_if_possible(payload)
}

pub fn rewrite_payload_if_possible(payload: &mut Vec<u8>) -> Option<CustomTokenRewriteSummary> {
    if !is_custom_token_payload(payload) {
        return None;
    }

    if let Some(parsed) = parse_valid_custom_token_payload(payload) {
        if parsed.valid {
            return None;
        }
    }

    let source_minor = payload[2];
    let old_declared_present = payload.len() >= READ_START;
    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES).unwrap_or(0);
    let old_token_count = observed_token_count(payload).unwrap_or(0);
    let old_payload_length = payload.len();

    let mut rewritten = Vec::with_capacity(READ_START + 4 + MAX_CUSTOM_TOKEN_FRAGMENT_BYTES);
    rewritten.push(HIGH_LEVEL_ENVELOPE);
    rewritten.push(CUSTOM_TOKEN_MAJOR);
    rewritten.push(CUSTOM_TOKEN_LIST_MINOR);
    rewritten.extend_from_slice(&EMPTY_LIST_DECLARED.to_le_bytes());
    rewritten.extend_from_slice(&0u32.to_le_bytes());
    rewritten.push(0);

    let summary = CustomTokenRewriteSummary {
        source_minor,
        old_declared_present,
        old_declared,
        new_declared: EMPTY_LIST_DECLARED,
        old_payload_length,
        new_payload_length: rewritten.len(),
        old_token_count,
        reason: "malformed-custom-token-cnw-window",
    };
    *payload = rewritten;
    Some(summary)
}

fn is_custom_token_payload(payload: &[u8]) -> bool {
    payload.len() >= HIGH_LEVEL_HEADER_BYTES
        && payload[0] == HIGH_LEVEL_ENVELOPE
        && payload[1] == CUSTOM_TOKEN_MAJOR
        && matches!(payload[2], CUSTOM_TOKEN_SET_MINOR | CUSTOM_TOKEN_LIST_MINOR)
}

#[derive(Debug, Clone, Copy)]
struct ParsedCustomTokenPayload {
    valid: bool,
}

fn parse_valid_custom_token_payload(payload: &[u8]) -> Option<ParsedCustomTokenPayload> {
    if !is_custom_token_payload(payload) || payload.len() < READ_START {
        return Some(ParsedCustomTokenPayload { valid: false });
    }

    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)? as usize;
    if declared < READ_START
        || declared >= payload.len()
        || payload.len().saturating_sub(declared) > MAX_CUSTOM_TOKEN_FRAGMENT_BYTES
    {
        return Some(ParsedCustomTokenPayload { valid: false });
    }

    let mut cursor = READ_START;
    match payload[2] {
        CUSTOM_TOKEN_SET_MINOR => {
            cursor = cursor.checked_add(4)?;
            if cursor > declared {
                return Some(ParsedCustomTokenPayload { valid: false });
            }
            cursor = read_c_exo_string(payload, cursor, declared)?;
        }
        CUSTOM_TOKEN_LIST_MINOR => {
            let count = read_u32_le(payload, cursor)? as usize;
            if count > MAX_CUSTOM_TOKEN_COUNT {
                return Some(ParsedCustomTokenPayload { valid: false });
            }
            cursor = cursor.checked_add(4)?;
            for _ in 0..count {
                cursor = cursor.checked_add(4)?;
                if cursor > declared {
                    return Some(ParsedCustomTokenPayload { valid: false });
                }
                cursor = read_c_exo_string(payload, cursor, declared)?;
            }
        }
        _ => return Some(ParsedCustomTokenPayload { valid: false }),
    }

    Some(ParsedCustomTokenPayload {
        valid: cursor == declared,
    })
}

fn read_c_exo_string(payload: &[u8], cursor: usize, declared: usize) -> Option<usize> {
    let length = read_u32_le(payload, cursor)? as usize;
    if length > MAX_CUSTOM_TOKEN_STRING_BYTES {
        return None;
    }
    cursor
        .checked_add(4)?
        .checked_add(length)
        .filter(|end| *end <= declared)
}

fn observed_token_count(payload: &[u8]) -> Option<u32> {
    if payload.get(2).copied()? == CUSTOM_TOKEN_LIST_MINOR {
        read_u32_le(payload, READ_START)
    } else {
        Some(1)
    }
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_token_payload(token_id: u32, value: &[u8]) -> Vec<u8> {
        let declared = READ_START + 4 + 4 + value.len();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[
            HIGH_LEVEL_ENVELOPE,
            CUSTOM_TOKEN_MAJOR,
            CUSTOM_TOKEN_SET_MINOR,
        ]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&token_id.to_le_bytes());
        payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
        payload.extend_from_slice(value);
        payload.push(0x60);
        payload
    }

    fn list_token_payload(entries: &[(u32, &[u8])]) -> Vec<u8> {
        let body_len = 4 + entries
            .iter()
            .map(|(_, value)| 4 + 4 + value.len())
            .sum::<usize>();
        let declared = READ_START + body_len;
        let mut payload = Vec::new();
        payload.extend_from_slice(&[
            HIGH_LEVEL_ENVELOPE,
            CUSTOM_TOKEN_MAJOR,
            CUSTOM_TOKEN_LIST_MINOR,
        ]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (token_id, value) in entries {
            payload.extend_from_slice(&token_id.to_le_bytes());
            payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
            payload.extend_from_slice(value);
        }
        payload.push(0x60);
        payload
    }

    #[test]
    fn claims_exact_set_custom_token_payload() {
        let payload = set_token_payload(0x1234, b"hello");

        let claim = claim_payload_if_verified(&payload).expect("custom token should claim");

        assert_eq!(claim.minor, CUSTOM_TOKEN_SET_MINOR);
        assert_eq!(claim.declared, READ_START + 4 + 4 + 5);
        assert_eq!(claim.token_count, 1);
    }

    #[test]
    fn claims_exact_set_custom_token_list_payload() {
        let payload = list_token_payload(&[(0x1234, &b"a"[..]), (0x5678, &b"bc"[..])]);

        let claim = claim_payload_if_verified(&payload).expect("custom token list should claim");

        assert_eq!(claim.minor, CUSTOM_TOKEN_LIST_MINOR);
        assert_eq!(claim.declared, READ_START + 4 + (4 + 4 + 1) + (4 + 4 + 2));
        assert_eq!(claim.token_count, 2);
    }

    #[test]
    fn malformed_custom_token_is_rewrite_only() {
        let mut payload = list_token_payload(&[(0x1234, &b"a"[..])]);
        payload[3..7].copy_from_slice(&(READ_START as u32).to_le_bytes());

        assert!(claim_payload_if_verified(&payload).is_none());

        let summary = rewrite_payload_if_possible(&mut payload)
            .expect("malformed custom token should be rewritten");
        assert_eq!(summary.reason, "malformed-custom-token-cnw-window");
        assert!(claim_payload_if_verified(&payload).is_some());
    }
}
