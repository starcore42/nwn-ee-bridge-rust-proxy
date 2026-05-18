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
    latest_client_build: Option<bnxi::ClientBuild>,
    nwsync_advertised_to_client: bool,
    server_bnvr_accept_seen: bool,
    nwsync_handoff_bndm_consumed: bool,
    reliable_gameplay_seen: bool,
    pending_server_to_client: Vec<Vec<u8>>,
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

    fn latest_client_build(&self) -> Option<bnxi::ClientBuild> {
        self.latest_client_build
    }

    fn remember_client_build(&mut self, build: bnxi::ClientBuild) {
        self.latest_client_build = Some(build);
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

    pub(crate) fn remember_nwsync_advertised_to_client(&mut self) {
        self.nwsync_advertised_to_client = true;
        self.nwsync_handoff_bndm_consumed = false;
    }

    pub(crate) fn remember_bnvr_result(&mut self, accepted: bool) {
        self.server_bnvr_accept_seen = accepted;
        if !accepted {
            self.nwsync_handoff_bndm_consumed = false;
        }
    }

    pub(crate) fn remember_reliable_gameplay_seen(&mut self) {
        self.reliable_gameplay_seen = true;
    }

    pub(crate) fn should_consume_nwsync_handoff_bndm(&self) -> bool {
        self.nwsync_advertised_to_client
            && !self.nwsync_handoff_bndm_consumed
            && !self.reliable_gameplay_seen
            // EE's native NWSync handoff can tear down the current net-layer
            // session before the normal BNCS/BNVR verifier path exists. When
            // BNCS is not yet captured there is no Diamond UDP-port disconnect
            // shape to emit, and the exact four-byte BNDM is proxy-owned
            // handoff control. If BNCS already exists, keep the older
            // decompile-backed post-BNVR handoff allowance; otherwise BNDM is
            // a real disconnect and must translate to legacy BNDS below.
            && (self.latest_bncs_udp_port.is_none() || self.server_bnvr_accept_seen)
    }

    pub(crate) fn remember_nwsync_handoff_bndm_consumed(&mut self) {
        self.nwsync_handoff_bndm_consumed = true;
    }

    pub(crate) fn take_pending_server_to_client_packets(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_server_to_client)
    }

    fn enqueue_server_to_client(&mut self, packet: Vec<u8>) {
        self.pending_server_to_client.push(packet);
    }
}

pub enum ClientTranslation {
    Packet(Vec<u8>),
    PacketRetireSession {
        packet: Vec<u8>,
        reason: &'static str,
    },
    Consumed,
    ConsumedRetireSession {
        reason: &'static str,
    },
}

pub enum ServerTranslation {
    Packet(Vec<u8>),
    PacketRetireSession {
        packet: Vec<u8>,
        reason: &'static str,
    },
}

pub fn translate_client_to_server(
    bytes: &[u8],
    identity: &DiamondIdentity,
    legacy_udp_port: u16,
    bncs_private_build: u32,
    bncs_build_field: u16,
    server_port: u16,
    discovery_session_name: &str,
    discovery_module_name: &str,
    nwsync_advertisement: Option<&Advertisement>,
    state: &mut SessionState,
) -> anyhow::Result<ClientTranslation> {
    let packet = BnPacket::parse(bytes);
    match packet.tag {
        BnTag::Unknown => {
            anyhow::bail!("unknown BN control tag in client-to-server direction")
        }
        BnTag::Bncs => {
            let rewritten = bncs::rewrite_client_to_diamond(
                bytes,
                identity,
                legacy_udp_port,
                bncs_private_build,
                bncs_build_field,
            )?;
            state.remember_bncs_udp_port(rewritten.advertised_udp_port);
            return Ok(ClientTranslation::Packet(rewritten.packet));
        }
        BnTag::Bndm => {
            return match bndm::translate_client_bndm(bytes, state)? {
                bndm::BndmTranslation::LegacyDisconnectRetireSession(packet) => {
                    Ok(ClientTranslation::PacketRetireSession {
                        packet,
                        reason: "post-gameplay-bndm-disconnect",
                    })
                }
                bndm::BndmTranslation::NwsyncHandoffConsumedRetireSession => {
                    Ok(ClientTranslation::ConsumedRetireSession {
                        reason: "nwsync-handoff-bndm",
                    })
                }
            };
        }
        BnTag::Bnvs => {
            return Ok(ClientTranslation::Packet(bnvs::rewrite_client_to_diamond(
                bytes, identity, state,
            )?));
        }
        BnTag::Bnds => {
            if bnds::claim_client_to_legacy_if_verified(bytes).is_some() {
                return claimed_client_noop(bytes, "BNDS", "verified legacy disconnect datagram");
            }
        }
        BnTag::Bnes => {
            if bnes::claim_client_to_legacy_if_verified(bytes).is_some() {
                let response =
                    bnes::build_proxy_owned_bner_response(bnes::ProxyEnumerateResponse {
                        server_port,
                        section: 0,
                        session_name: discovery_session_name,
                    })?;
                tracing::info!(
                    tag = "BNES",
                    len = bytes.len(),
                    response_len = response.len(),
                    session_name = discovery_session_name,
                    "BN packet semantically claimed as verified server-enumerate request; forwarding to legacy server and queuing exact EE discovery-progress BNER"
                );
                state.enqueue_server_to_client(response);
                return Ok(ClientTranslation::Packet(bytes.to_vec()));
            }
        }
        BnTag::Bnlm => {
            if bnlm::claim_client_to_legacy_if_verified(bytes).is_some() {
                return claimed_client_noop(bytes, "BNLM", "verified latency/list request");
            }
        }
        BnTag::Bnxi => {
            if let Some(build) = bnxi::claim_client_to_legacy_if_verified(bytes) {
                state.remember_client_build(build);
                let response =
                    bnxr::build_proxy_owned_bnxr_response(bnxr::ProxyExtendedInfoResponse {
                        server_port,
                        module_name: discovery_module_name,
                        advertisement: nwsync_advertisement,
                    })?;
                if nwsync_advertisement.is_some() {
                    state.remember_nwsync_advertised_to_client();
                }
                tracing::info!(
                    tag = "BNXI",
                    client_major = build.major,
                    client_minor = build.minor,
                    client_revision = build.revision,
                    nwsync = nwsync_advertisement.is_some(),
                    response_len = response.len(),
                    module_name = discovery_module_name,
                    "BN packet semantically claimed as verified extended-info request; consuming and queuing exact proxy-owned EE BNXR response"
                );
                state.enqueue_server_to_client(response);
                return Ok(ClientTranslation::Consumed);
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
) -> anyhow::Result<ServerTranslation> {
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
            if let Some(rewritten) =
                bnvr::rewrite_server_to_ee_if_verified(bytes, state.latest_client_build())?
            {
                state.remember_bnvr_result(rewritten.get(4).copied() == Some(b'A'));
                tracing::info!(
                    tag = "BNVR",
                    len = rewritten.len(),
                    "BN packet semantically claimed as verified EE verifier result translation"
                );
                return Ok(ServerTranslation::Packet(rewritten));
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
                    state.remember_nwsync_advertised_to_client();
                    return Ok(ServerTranslation::Packet(rewritten));
                }
            }
            if bnxr::claim_server_to_ee_if_verified(bytes).is_some() {
                if nwsync_advertisement.is_some() {
                    state.remember_nwsync_advertised_to_client();
                }
                return claimed_noop(bytes, "BNXR", "verified extended server response");
            }
        }
        BnTag::Bndp => {
            if bndp::claim_server_to_ee_if_verified(bytes).is_some() {
                tracing::info!(
                    tag = "BNDP",
                    len = bytes.len(),
                    "BN packet semantically claimed as verified EE disconnect reason; proxy session will retire after delivery"
                );
                return Ok(ServerTranslation::PacketRetireSession {
                    packet: bytes.to_vec(),
                    reason: "server-bndp-disconnect",
                });
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

    unclaimed_bn_server(packet.tag, "server-to-client", bytes.len()).map(ServerTranslation::Packet)
}

fn claimed_noop(
    bytes: &[u8],
    tag: &'static str,
    reason: &'static str,
) -> anyhow::Result<ServerTranslation> {
    tracing::info!(
        tag,
        reason,
        len = bytes.len(),
        "BN packet semantically claimed as verified no-op translation"
    );
    Ok(ServerTranslation::Packet(bytes.to_vec()))
}

fn claimed_client_noop(
    bytes: &[u8],
    tag: &'static str,
    reason: &'static str,
) -> anyhow::Result<ClientTranslation> {
    tracing::info!(
        tag,
        reason,
        len = bytes.len(),
        "BN packet semantically claimed as verified no-op translation"
    );
    Ok(ClientTranslation::Packet(bytes.to_vec()))
}

fn unclaimed_bn(
    tag: BnTag,
    direction: &'static str,
    len: usize,
) -> anyhow::Result<ClientTranslation> {
    tracing::warn!(
        direction,
        tag = tag.name(),
        len,
        "BN packet quarantined before emit: no semantic translator claimed this tag/direction"
    );
    anyhow::bail!("unclaimed BN packet {:?} in {} direction", tag, direction)
}

fn unclaimed_bn_server(tag: BnTag, direction: &'static str, len: usize) -> anyhow::Result<Vec<u8>> {
    tracing::warn!(
        direction,
        tag = tag.name(),
        len,
        "BN packet quarantined before emit: no semantic translator claimed this tag/direction"
    );
    anyhow::bail!("unclaimed BN packet {:?} in {} direction", tag, direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_bndp_is_verified_disconnect_event_not_plain_noop() {
        // EE `HandleBNDPMessage` accepts this exact eight-byte reason-code
        // form. Once the legacy server sends it, the proxy must deliver the
        // verified disconnect to EE and then retire the session instead of
        // continuing to translate reliable-window M acks for a dead session.
        let packet = b"BNDP\xCE\x16\x00\x00";
        let mut state = SessionState::default();

        let translated = translate_server_to_client(packet, &mut state, None)
            .expect("BNDP reason-code shape must be claimed");

        match translated {
            ServerTranslation::PacketRetireSession {
                packet: out,
                reason,
            } => {
                assert_eq!(out, packet);
                assert_eq!(reason, "server-bndp-disconnect");
            }
            ServerTranslation::Packet(_) => panic!("server BNDP must retire the proxy session"),
        }
    }
}
