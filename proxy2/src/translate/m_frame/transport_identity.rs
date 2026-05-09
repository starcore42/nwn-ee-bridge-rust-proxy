//! Verified no-op ownership for reliable-window transport-only `M` frames.
//!
//! This module is intentionally narrow. It does not claim gameplay semantics
//! and it must not become a bypass around high-level packet translators.
//! Instead, it answers one transport question:
//!
//! "Is this parsed `M` frame a reliable-window shell/continuation whose bytes
//! are version-identical between Diamond/1.69 and EE?"
//!
//! Decompile-backed rationale:
//!
//! - Both Diamond and EE route reliable-window sequencing, ACKs, packetized
//!   continuation records, and deflated-window continuation chunks through the
//!   CNetLayer window machinery before CNW gameplay dispatch sees a complete
//!   `P major minor` payload.
//! - When a frame has no visible high-level CNW header, the proxy may only
//!   leave it unchanged if the transport metadata proves it is one of those
//!   window-level records. Any visible high-level packet is deliberately
//!   refused here and must be claimed by a focused semantic translator.

use crate::packet::m::MFrameView;

#[derive(Debug, Clone, Copy)]
pub(super) struct TransportIdentityClaim {
    pub(super) packet_name: &'static str,
    pub(super) reason: &'static str,
}

pub(super) fn claim_client_frame_if_verified(view: &MFrameView) -> Option<TransportIdentityClaim> {
    claim_frame_if_verified(view, DirectionKind::ClientToServer)
}

pub(super) fn claim_server_frame_if_verified(view: &MFrameView) -> Option<TransportIdentityClaim> {
    claim_frame_if_verified(view, DirectionKind::ServerToClient)
}

#[derive(Debug, Clone, Copy)]
enum DirectionKind {
    ClientToServer,
    ServerToClient,
}

fn claim_frame_if_verified(
    view: &MFrameView,
    direction: DirectionKind,
) -> Option<TransportIdentityClaim> {
    if view.high.is_some() {
        return None;
    }

    if view.declared_payload_length != 0
        && view.declared_payload_length > view.available_payload_length
    {
        return None;
    }

    if view.payload_length == 0 {
        return Some(TransportIdentityClaim {
            packet_name: "empty reliable-window ack/control",
            reason: "verified-empty-M-window-shell",
        });
    }

    if view.packetized_sequence != 0 && view.declared_payload_length != 0 {
        return Some(TransportIdentityClaim {
            packet_name: "packetized reliable-window continuation",
            reason: "verified-window-packetized-continuation",
        });
    }

    if matches!(direction, DirectionKind::ServerToClient)
        && view.declared_payload_length == 0
        && view.packetized_sequence == 0
        && (view.flags & 0x08) != 0
    {
        return Some(TransportIdentityClaim {
            packet_name: "deflated reliable-window continuation",
            reason: "verified-server-deflated-window-continuation",
        });
    }

    None
}
