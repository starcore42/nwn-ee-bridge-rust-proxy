//! `BNLM` latency/list request no-op claim.
//!
//! EE `SendBNLMMessage` emits the same fixed eleven-byte direct-control
//! datagram shape that Diamond routes on its server-mode BN dispatcher. The
//! bridge validates that exact shape here before forwarding it unchanged.

pub(super) fn claim_client_to_legacy_if_verified(bytes: &[u8]) -> Option<()> {
    (bytes.get(..4)? == b"BNLM" && bytes.len() == 11).then_some(())
}
