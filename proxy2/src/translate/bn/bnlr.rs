//! `BNLR` latency/list response no-op claim.
//!
//! The Diamond client-side BN dispatcher routes `BNLR`, and EE's
//! `HandleBNLRMessage` consumes the same fixed eleven-byte response shape.
//! Claiming it here makes the identical translation visible and auditable.

pub(super) fn claim_server_to_ee_if_verified(bytes: &[u8]) -> Option<()> {
    (bytes.get(..4)? == b"BNLR" && bytes.len() == 11).then_some(())
}
