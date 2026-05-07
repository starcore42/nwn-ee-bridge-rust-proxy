//! `BNDS` legacy disconnect no-op claim.
//!
//! Diamond's BN dispatcher routes `BNDS` on the server-mode side, and the
//! legacy disconnect datagram is exactly the four-byte tag followed by the
//! client's little-endian UDP port. EE `BNDM` is rewritten into this shape by
//! `bndm.rs`; this module exists for the rare case where an already-legacy
//! client packet reaches the bridge. It is claimed unchanged only after the
//! exact six-byte Diamond shape is verified.

pub(super) fn claim_client_to_legacy_if_verified(bytes: &[u8]) -> Option<()> {
    (bytes.get(..4)? == b"BNDS" && bytes.len() == 6).then_some(())
}
