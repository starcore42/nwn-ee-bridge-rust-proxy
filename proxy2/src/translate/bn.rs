//! BN/control packet translation router.
//!
//! Keep this file intentionally small. BN packets are direct-control datagrams,
//! but each tag answers a different protocol question, so the semantic work is
//! split into tag-specific modules:
//!
//! - `bncs`: EE connect-start shape -> Diamond/1.69 connect-start shape.
//! - `bncr`: observe the server challenge needed by the verifier response.
//! - `bnvs`: EE verifier response -> Diamond/1.69 verifier list.
//! - `bnxr`: inject EE NWSync advertisement metadata when configured.
//! - other tag modules: decompile-backed no-op claims for packets whose EE and
//!   Diamond/1.69 wire shapes are intentionally identical.
//!
//! Unknown or unowned BN controls are not passed through. The caller drops
//! them, then the dump/log path gives us a concrete packet to research against
//! the EE and Diamond decompiles before adding a focused translator.

mod bncr;
mod bncs;
mod bndm;
mod bndp;
mod bndr;
mod bnds;
mod bner;
mod bnes;
mod bnlm;
mod bnlr;
mod bnvr;
mod bnvs;
mod bnxi;
mod bnxr;
mod wire;

use crate::{
    identity::DiamondIdentity,
    nwsync::Advertisement,
    packet::bn::{BnPacket, BnTag},
};

#[derive(Debug, Clone, Default)]
pub struct SessionState {
    latest_bncs_udp_port: Option<u16>,
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

    pub(crate) fn latest_bncs_udp_port(&self) -> Option<u16> {
        self.latest_bncs_udp_port
    }

    pub(crate) fn remember_bncs_udp_port(&mut self, udp_port: u16) {
        self.latest_bncs_udp_port = Some(udp_port);
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
    let packet = BnPacket::parse(bytes);
    match packet.tag {
        BnTag::Unknown => {
            anyhow::bail!("unknown BN control tag in client-to-server direction")
        }
        BnTag::Bncs => {
            if bytes.len() >= 6 {
                state.remember_bncs_udp_port(u16::from_le_bytes([bytes[4], bytes[5]]));
            }
            return bncs::rewrite_client_to_diamond(
                bytes,
                identity,
                bncs_private_build,
                bncs_build_field,
            );
        }
        BnTag::Bndm => return bndm::rewrite_client_to_legacy_bnds(bytes, state),
        BnTag::Bnvs => return bnvs::rewrite_client_to_diamond(bytes, identity, state),
        BnTag::Bnds => {
            if bnds::claim_client_to_legacy_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNDS", "verified legacy disconnect datagram");
            }
        }
        BnTag::Bnes => {
            if bnes::claim_client_to_legacy_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNES", "verified server-enumerate request");
            }
        }
        BnTag::Bnlm => {
            if bnlm::claim_client_to_legacy_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNLM", "verified latency/list request");
            }
        }
        BnTag::Bnxi => {
            if bnxi::claim_client_to_legacy_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNXI", "verified extended-info request");
            }
        }
        _ => {}
    };

    unclaimed_bn(packet.tag, "client-to-server", bytes.len())
}

pub fn translate_server_to_client(
    bytes: &[u8],
    state: &mut SessionState,
    nwsync_advertisement: Option<&Advertisement>,
) -> anyhow::Result<Vec<u8>> {
    let packet = BnPacket::parse(bytes);
    match packet.tag {
        BnTag::Unknown => {
            anyhow::bail!("unknown BN control tag in server-to-client direction")
        }
        BnTag::Bncr => {
            if bncr::claim_server_to_ee_if_verified(bytes, state)?.is_some() {
                return claimed_noop(bytes, "BNCR", "verified legacy verifier challenge");
            }
        }
        BnTag::Bnvr => {
            if bnvr::claim_server_to_ee_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNVR", "verified legacy verifier result");
            }
        }
        BnTag::Bndr => {
            if bndr::claim_server_to_ee_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNDR", "verified EE extended server-info response");
            }
        }
        BnTag::Bnxr => {
            if let Some(advertisement) = nwsync_advertisement {
                if let Some(rewritten) = bnxr::rewrite_server_to_ee(bytes, advertisement)? {
                    return Ok(rewritten);
                }
            }
            if bnxr::claim_server_to_ee_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNXR", "verified extended server response");
            }
        }
        BnTag::Bndp => {
            if bndp::claim_server_to_ee_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNDP", "verified EE disconnect reason");
            }
        }
        BnTag::Bner => {
            if bner::claim_server_to_ee_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNER", "verified session-enumerate response");
            }
        }
        BnTag::Bnlr => {
            if bnlr::claim_server_to_ee_if_verified(bytes).is_some() {
                return claimed_noop(bytes, "BNLR", "verified latency/list response");
            }
        }
        _ => {}
    };

    unclaimed_bn(packet.tag, "server-to-client", bytes.len())
}

fn claimed_noop(bytes: &[u8], tag: &'static str, reason: &'static str) -> anyhow::Result<Vec<u8>> {
    tracing::info!(
        tag,
        reason,
        len = bytes.len(),
        "BN packet semantically claimed as verified no-op translation"
    );
    Ok(bytes.to_vec())
}

fn unclaimed_bn(tag: BnTag, direction: &'static str, len: usize) -> anyhow::Result<Vec<u8>> {
    tracing::warn!(
        direction,
        tag = tag.name(),
        len,
        "BN packet quarantined before emit: no semantic translator claimed this tag/direction"
    );
    anyhow::bail!("unclaimed BN packet {:?} in {} direction", tag, direction)
}
