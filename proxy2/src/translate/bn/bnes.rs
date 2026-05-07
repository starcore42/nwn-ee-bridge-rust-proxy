//! `BNES` server-enumerate request no-op claim.
//!
//! The packet-alignment reference records the Diamond dispatcher route for
//! `BNES` on the server-mode side and the EE sender
//! `SendBNESDirectMessageToAddress`. The verified direct-control shape is a
//! fixed seven-byte datagram, and there is no dialect field to rewrite for the
//! 1.69 server. This is still an explicit translator claim, not generic BN
//! pass-through.

pub(super) fn claim_client_to_legacy_if_verified(bytes: &[u8]) -> Option<()> {
    (bytes.get(..4)? == b"BNES" && bytes.len() == 7).then_some(())
}
