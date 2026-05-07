//! `BNVR` verifier-result no-op claim.
//!
//! Diamond `BNVR` writer/parser and EE `HandleBNVRMessage` agree on the two
//! legacy result forms used by HG: reject is six bytes (`BNVR`, `R`, reason)
//! and accept is nine bytes (`BNVR`, `A`, little-endian DWORD window value).
//! Longer EE-only accept tails are intentionally not claimed for the
//! 1.69-server path until a capture/decompile pass requires them.

pub(super) fn claim_server_to_ee_if_verified(bytes: &[u8]) -> Option<()> {
    if bytes.get(..4)? != b"BNVR" {
        return None;
    }
    match bytes.get(4).copied()? {
        b'R' => (bytes.len() == 6).then_some(()),
        b'A' => (bytes.len() == 9).then_some(()),
        _ => None,
    }
}
