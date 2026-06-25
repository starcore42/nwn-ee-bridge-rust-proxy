//! Diamond-era master/auth controls that do not belong on a stock EE client.
//!
//! These packets are top-level `BM*` controls from the 1.69 master/auth
//! exchange. They are not gameplay M-frames and are not BN packets. The bridge
//! consumes only the exact counted-field shapes observed in Diamond auth
//! traffic so unrelated unknown top-level packets remain strict failures.

const CHALLENGE_BYTES: usize = 32;
const DIGEST_HEX_BYTES: usize = 32;
const CD_KEY_PUBLIC_BYTES: usize = 8;
const MAX_ACCOUNT_NAME_BYTES: usize = 64;
const BMAU_FIXED_HEADER_AFTER_PORT_BYTES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegacyMasterControlClaim {
    pub(crate) tag: &'static str,
    pub(crate) account_name_len: usize,
    pub(crate) cd_key_count: Option<u16>,
}

pub(crate) fn claim_legacy_server_master_control(bytes: &[u8]) -> Option<LegacyMasterControlClaim> {
    if bytes.starts_with(b"BMPA") {
        return claim_bmpa(bytes);
    }
    if bytes.starts_with(b"BMAU") {
        return claim_bmau(bytes);
    }
    None
}

fn claim_bmpa(bytes: &[u8]) -> Option<LegacyMasterControlClaim> {
    let mut cursor = 4;

    let _legacy_port = read_u16(bytes, &mut cursor)?;
    let challenge_len = usize::from(read_u16(bytes, &mut cursor)?);
    if challenge_len != CHALLENGE_BYTES {
        return None;
    }
    read_bytes(bytes, &mut cursor, challenge_len)?;

    let account_name = read_counted_printable_ascii(bytes, &mut cursor, MAX_ACCOUNT_NAME_BYTES)?;
    if account_name.is_empty() {
        return None;
    }

    let digest = read_counted_printable_ascii(bytes, &mut cursor, DIGEST_HEX_BYTES)?;
    if !is_hex_digest(digest) {
        return None;
    }

    let empty_message = read_counted_printable_ascii(bytes, &mut cursor, 0)?;
    if !empty_message.is_empty() {
        return None;
    }

    let _status = read_u16(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return None;
    }

    Some(LegacyMasterControlClaim {
        tag: "BMPA",
        account_name_len: account_name.len(),
        cd_key_count: None,
    })
}

fn claim_bmau(bytes: &[u8]) -> Option<LegacyMasterControlClaim> {
    let mut cursor = 4;

    let _legacy_port = read_u16(bytes, &mut cursor)?;
    read_bytes(bytes, &mut cursor, BMAU_FIXED_HEADER_AFTER_PORT_BYTES)?;

    let challenge_len = usize::from(read_u16(bytes, &mut cursor)?);
    if challenge_len != CHALLENGE_BYTES {
        return None;
    }
    read_bytes(bytes, &mut cursor, challenge_len)?;

    let cd_key_count = read_u16(bytes, &mut cursor)?;
    if !(1..=3).contains(&cd_key_count) {
        return None;
    }

    for _ in 0..cd_key_count {
        let public_key = read_counted_printable_ascii(bytes, &mut cursor, CD_KEY_PUBLIC_BYTES)?;
        if public_key.len() != CD_KEY_PUBLIC_BYTES
            || !public_key.iter().all(u8::is_ascii_alphanumeric)
        {
            return None;
        }

        let digest = read_counted_printable_ascii(bytes, &mut cursor, DIGEST_HEX_BYTES)?;
        if !is_hex_digest(digest) {
            return None;
        }
    }

    let account_name = read_counted_printable_ascii(bytes, &mut cursor, MAX_ACCOUNT_NAME_BYTES)?;
    if account_name.is_empty() || cursor != bytes.len() {
        return None;
    }

    Some(LegacyMasterControlClaim {
        tag: "BMAU",
        account_name_len: account_name.len(),
        cd_key_count: Some(cd_key_count),
    })
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Option<u16> {
    let end = cursor.checked_add(2)?;
    let chunk = bytes.get(*cursor..end)?;
    *cursor = end;
    Some(u16::from_le_bytes([chunk[0], chunk[1]]))
}

fn read_bytes<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = cursor.checked_add(len)?;
    let chunk = bytes.get(*cursor..end)?;
    *cursor = end;
    Some(chunk)
}

fn read_counted_printable_ascii<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    max_len: usize,
) -> Option<&'a [u8]> {
    let len = usize::from(read_u16(bytes, cursor)?);
    if len > max_len {
        return None;
    }
    let value = read_bytes(bytes, cursor, len)?;
    if !value.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return None;
    }
    Some(value)
}

fn is_hex_digest(value: &[u8]) -> bool {
    value.len() == DIGEST_HEX_BYTES && value.iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_bm_bmpa_claims_password_accept_shape() {
        let bytes = build_bmpa();

        let claim = claim_legacy_server_master_control(&bytes).expect("BMPA should claim");

        assert_eq!(claim.tag, "BMPA");
        assert_eq!(claim.account_name_len, 9);
        assert_eq!(claim.cd_key_count, None);
    }

    #[test]
    fn legacy_bm_bmau_claims_auth_update_shape() {
        let bytes = build_bmau();

        let claim = claim_legacy_server_master_control(&bytes).expect("BMAU should claim");

        assert_eq!(claim.tag, "BMAU");
        assert_eq!(claim.account_name_len, 9);
        assert_eq!(claim.cd_key_count, Some(3));
    }

    #[test]
    fn legacy_bm_bmau_rejects_trailing_padding() {
        let mut bytes = build_bmau();
        bytes.push(0);

        assert!(claim_legacy_server_master_control(&bytes).is_none());
    }

    #[test]
    fn legacy_bm_bmpa_rejects_non_hex_digest() {
        let mut bytes = build_bmpa();
        let digest_start = 4 + 2 + 2 + CHALLENGE_BYTES + 2 + 9 + 2;
        bytes[digest_start] = b'g';

        assert!(claim_legacy_server_master_control(&bytes).is_none());
    }

    fn build_bmpa() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BMPA");
        push_u16(&mut bytes, 5122);
        push_u16(&mut bytes, CHALLENGE_BYTES as u16);
        bytes.extend([0x5a; CHALLENGE_BYTES]);
        push_counted(&mut bytes, b"PlayerOne");
        push_counted(&mut bytes, b"0123456789abcdef0123456789ABCDEF");
        push_counted(&mut bytes, b"");
        push_u16(&mut bytes, 0x0057);
        bytes
    }

    fn build_bmau() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BMAU");
        push_u16(&mut bytes, 5122);
        push_u16(&mut bytes, 1);
        bytes.extend_from_slice(&[127, 0, 0, 1]);
        push_u16(&mut bytes, 0xe868);
        push_u16(&mut bytes, CHALLENGE_BYTES as u16);
        bytes.extend([0xa5; CHALLENGE_BYTES]);
        push_u16(&mut bytes, 3);
        push_key_pair(&mut bytes, b"ABCDEFGH", b"0123456789abcdef0123456789ABCDEF");
        push_key_pair(&mut bytes, b"12345678", b"abcdef0123456789ABCDEF0123456789");
        push_key_pair(&mut bytes, b"ZXCVBN12", b"99999999999999999999999999999999");
        push_counted(&mut bytes, b"PlayerOne");
        bytes
    }

    fn push_key_pair(bytes: &mut Vec<u8>, public_key: &[u8], digest: &[u8]) {
        push_counted(bytes, public_key);
        push_counted(bytes, digest);
    }

    fn push_counted(bytes: &mut Vec<u8>, value: &[u8]) {
        push_u16(bytes, value.len() as u16);
        bytes.extend_from_slice(value);
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
