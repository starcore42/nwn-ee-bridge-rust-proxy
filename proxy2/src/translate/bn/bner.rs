//! `BNER` session-enumerate response no-op claim.
//!
//! EE `HandleBNERMessage` reads a section byte at offset seven and a counted
//! session name beginning at offset eight. Diamond routes `BNER` on the
//! client-mode BN dispatcher. The proxy claims only the exact bounded shape so
//! extra bytes cannot hide behind an otherwise-known direct-control tag.

pub(super) fn claim_server_to_ee_if_verified(bytes: &[u8]) -> Option<()> {
    if bytes.get(..4)? != b"BNER" || bytes.len() < 9 {
        return None;
    }
    let section = bytes[7];
    if section >= 6 {
        return None;
    }
    let name_len = bytes[8] as usize;
    (9usize.checked_add(name_len)? == bytes.len()).then_some(())
}
