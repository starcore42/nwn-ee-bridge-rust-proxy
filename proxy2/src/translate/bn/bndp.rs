//! `BNDP` EE disconnect-reason no-op claim.
//!
//! EE `CNetLayerInternal::HandleBNDPMessage` accepts either `BNDP` plus a
//! 32-bit reason code or that fixed header followed by a bounded WORD-length
//! reason string. When the server already sends this EE-valid shape, no bytes
//! are rewritten, but the tag is still owned here before strict validation.

pub(super) fn claim_server_to_ee_if_verified(bytes: &[u8]) -> Option<()> {
    if bytes.get(..4)? != b"BNDP" {
        return None;
    }
    if bytes.len() == 8 {
        return Some(());
    }
    if bytes.len() < 10 {
        return None;
    }
    let reason_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    if reason_len >= 0x400 {
        return None;
    }
    (10usize.checked_add(reason_len)? == bytes.len()).then_some(())
}
