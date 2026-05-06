//! BN/control packet translation router.
//!
//! Keep this file intentionally small. BN packets are direct-control datagrams,
//! but each tag answers a different protocol question, so the semantic work is
//! split into tag-specific modules:
//!
//! - `bncs`: EE connect-start shape -> Diamond/1.69 connect-start shape.
//! - `bncr`: observe the server challenge needed by the verifier response.
//! - `bnvs`: EE verifier response -> Diamond/1.69 verifier list.
//!
//! Unknown BN controls are not translated here. They flow to `strict`, which
//! either recognizes the already-valid packet shape or quarantines it.

mod bncr;
mod bncs;
mod bnvs;
mod wire;

use crate::identity::DiamondIdentity;

#[derive(Debug, Clone, Default)]
pub struct SessionState {
    latest_bncr_status: Option<u8>,
    latest_cd_key_challenge: Vec<u8>,
}

impl SessionState {
    pub(crate) fn latest_bncr_status(&self) -> Option<u8> {
        self.latest_bncr_status
    }

    pub(crate) fn latest_cd_key_challenge(&self) -> &[u8] {
        &self.latest_cd_key_challenge
    }

    pub(crate) fn remember_bncr_challenge(&mut self, status: u8, cd_key_challenge: &[u8]) {
        self.latest_bncr_status = Some(status);
        self.latest_cd_key_challenge.clear();
        self.latest_cd_key_challenge
            .extend_from_slice(cd_key_challenge);
    }

    pub(crate) fn clear_bncr_challenge(&mut self) {
        self.latest_bncr_status = None;
        self.latest_cd_key_challenge.clear();
    }
}

pub fn translate_client_to_server(
    bytes: &[u8],
    identity: &DiamondIdentity,
    bncs_private_build: u32,
    bncs_build_field: u16,
    state: &mut SessionState,
) -> anyhow::Result<Vec<u8>> {
    if bytes.starts_with(b"BNCS") {
        return bncs::rewrite_client_to_diamond(
            bytes,
            identity,
            bncs_private_build,
            bncs_build_field,
        );
    }
    if bytes.starts_with(b"BNVS") {
        return bnvs::rewrite_client_to_diamond(bytes, identity, state);
    }

    Ok(bytes.to_vec())
}

pub fn translate_server_to_client(
    bytes: &[u8],
    state: &mut SessionState,
) -> anyhow::Result<Vec<u8>> {
    if bytes.starts_with(b"BNCR") {
        bncr::observe(bytes, state)?;
    }
    Ok(bytes.to_vec())
}
