//! Client-originated high-level gameplay semantic claims.
//!
//! The bridge rule is deliberately symmetric: a client-to-server high-level
//! opcode being known is not permission to pass it through. This router only
//! delegates to focused family modules; each family module documents why the
//! verified EE client shape is also valid for the Diamond/1.69 server, or
//! performs a dialect rewrite before claiming the packet.

use crate::{
    packet::m::HighLevel,
    translate::{
        client_area, client_char_list, client_login, client_module, client_server_status,
        play_module_character_list,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct ClientHighClaimSummary {
    pub family_name: &'static str,
    pub packet_name: &'static str,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientHighClaimSummary> {
    let high = HighLevel::parse(payload)?;

    if let Some(summary) = client_server_status::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientServerStatus",
            packet_name: summary.packet_name,
        });
    }
    if let Some(summary) = client_char_list::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientCharList",
            packet_name: summary.packet_name,
        });
    }
    if let Some(summary) = play_module_character_list::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "PlayModuleCharacterList",
            packet_name: summary.packet_name,
        });
    }
    if let Some(summary) = client_login::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientLogin",
            packet_name: summary.packet_name,
        });
    }
    if let Some(summary) = client_module::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientModule",
            packet_name: summary.packet_name,
        });
    }
    if let Some(summary) = client_area::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientArea",
            packet_name: summary.packet_name,
        });
    }

    tracing::warn!(
        major = high.major,
        minor = high.minor,
        name = high.name(),
        payload_len = payload.len(),
        "client high-level payload has no semantic owner"
    );
    None
}
