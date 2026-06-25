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
            // session before the normal BNCS/BNVR verifier path exists. If
            // BNCS already exists, keep the older decompile-backed post-BNVR
            // handoff allowance; otherwise BNDM is handled by the focused
            // pre-session disconnect path in `bndm`, because no legacy UDP
            // port exists for a BNDS datagram.
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
                bndm::BndmTranslation::PreSessionDisconnectConsumedRetireSession => {
                    Ok(ClientTranslation::ConsumedRetireSession {
                        reason: "pre-bncs-bndm-disconnect",
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
            if let Some(request) = bnxi::claim_client_to_legacy_if_verified(bytes) {
                match request {
                    bnxi::ClientRequest::EeExtended { udp_port, build } => {
                        state.remember_client_build(build);
                        let response = bnxr::build_proxy_owned_bnxr_response(
                            bnxr::ProxyExtendedInfoResponse {
                                server_port,
                                module_name: discovery_module_name,
                                advertisement: nwsync_advertisement,
                            },
                        )?;
                        if nwsync_advertisement.is_some() {
                            state.remember_nwsync_advertised_to_client();
                        }
                        tracing::info!(
                            tag = "BNXI",
                            udp_port,
                            client_major = build.major,
                            client_minor = build.minor,
                            client_revision = build.revision,
                            nwsync = nwsync_advertisement.is_some(),
                            response_len = response.len(),
                            module_name = discovery_module_name,
                            "BN packet semantically claimed as verified EE extended-info request; consuming and queuing exact proxy-owned EE BNXR response"
                        );
                        state.enqueue_server_to_client(response);
                        return Ok(ClientTranslation::Consumed);
                    }
                    bnxi::ClientRequest::LegacyProbe { udp_port } => {
                        tracing::info!(
                            tag = "BNXI",
                            udp_port,
                            len = bytes.len(),
                            "BN packet semantically claimed as verified legacy extended-info probe; forwarding to legacy server"
                        );
                        return Ok(ClientTranslation::Packet(bytes.to_vec()));
                    }
                }
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
                    "BN packet semantically claimed as verified EE disconnect reason/control"
                );
                return Ok(ServerTranslation::Packet(bytes.to_vec()));
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
    fn server_bndp_is_verified_control_not_session_retire() {
        // Live Diamond HG traffic can send this exact eight-byte BNDP control
        // between BNCS and BNCR, then continue through BNVS/BNVR into gameplay.
        // The proxy must deliver the verified EE-valid control without tearing
        // down the translator state that the following verifier packets need.
        let packet = b"BNDP\xCE\x16\x00\x00";
        let mut state = SessionState::default();

        let translated = translate_server_to_client(packet, &mut state, None)
            .expect("BNDP reason-code shape must be claimed");

        match translated {
            ServerTranslation::Packet(out) => assert_eq!(out, packet),
        }
    }

    #[test]
    fn client_short_bnxi_probe_forwards_without_ee_build_proof() {
        let identity = DiamondIdentity::default();
        let mut state = SessionState::default();

        let translated = translate_client_to_server(
            b"BNXI\x00\x14",
            &identity,
            5120,
            8109,
            3,
            5133,
            "Higher Ground (Party 2-3)",
            "Path of Ascension CEP Legends",
            None,
            &mut state,
        )
        .expect("legacy BNXI probe must be claimed");

        match translated {
            ClientTranslation::Packet(packet) => assert_eq!(packet, b"BNXI\x00\x14"),
            _ => panic!("legacy BNXI probe must be forwarded to the legacy server"),
        }
        assert!(state.latest_client_build().is_none());
        assert!(state.take_pending_server_to_client_packets().is_empty());
    }

    #[test]
    fn client_full_bnxi_consumes_and_records_ee_build() {
        let identity = DiamondIdentity::default();
        let mut state = SessionState::default();
        let packet = [
            b'B', b'N', b'X', b'I', 0x69, 0xC9, 0, 0, 0, 0, 0, 2, 4, b'8', b'1', b'9', b'3', 2,
            b'3', b'7', 2, b'1', b'7', 8, b'2', b'6', b'c', b'6', b'e', b'5', b'7', b'3',
        ];

        let translated = translate_client_to_server(
            &packet,
            &identity,
            5120,
            8109,
            3,
            5133,
            "Higher Ground (Party 2-3)",
            "Path of Ascension CEP Legends",
            None,
            &mut state,
        )
        .expect("full EE BNXI must be claimed");

        assert!(matches!(translated, ClientTranslation::Consumed));
        assert_eq!(
            state.latest_client_build(),
            Some(bnxi::ClientBuild {
                major: 8193,
                minor: 37,
                revision: 17
            })
        );
        let pending = state.take_pending_server_to_client_packets();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].starts_with(b"BNXR"));
    }
}
