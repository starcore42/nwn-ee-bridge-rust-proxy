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

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
    translate::{ContinuationOwner, VerifiedFamily, VerifiedPacket, VerifiedProof},
};

#[derive(Debug, Clone, Copy)]
pub(super) struct TransportIdentityClaim {
    pub(super) packet_name: &'static str,
    pub(super) reason: &'static str,
    kind: TransportIdentityKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransportIdentityKind {
    EmptyWindowShell,
    PacketizedContinuation,
    ServerDeflatedContinuation,
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
            kind: TransportIdentityKind::EmptyWindowShell,
        });
    }

    if matches!(direction, DirectionKind::ServerToClient)
        && view.packetized_sequence != 0
        && view.declared_payload_length != 0
    {
        return Some(TransportIdentityClaim {
            packet_name: "packetized reliable-window continuation",
            reason: "verified-window-packetized-continuation",
            kind: TransportIdentityKind::PacketizedContinuation,
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
            kind: TransportIdentityKind::ServerDeflatedContinuation,
        });
    }

    None
}

pub(super) fn verified_server_packet_for_claim(
    bytes: &[u8],
    view: &MFrameView,
    claim: TransportIdentityClaim,
    owner: ContinuationOwner,
    stream_epoch: u64,
    proxy_owned_stream: bool,
) -> anyhow::Result<Option<VerifiedPacket>> {
    if claim.kind == TransportIdentityKind::EmptyWindowShell {
        return Ok(Some(VerifiedPacket {
            proof: VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
            packet: bytes.to_vec(),
        }));
    }

    if claim.kind == TransportIdentityKind::ServerDeflatedContinuation
        && proxy_owned_stream
        && owner != ContinuationOwner::UnknownProxyOwned
        && stream_epoch != 0
    {
        return Ok(Some(VerifiedPacket {
            proof: VerifiedProof::family(VerifiedFamily::ServerZlibStreamContinuation {
                owner,
                stream_epoch,
                first_sequence: view.sequence,
            }),
            packet: consume_transport_only_packet_for_ee(bytes)?,
        }));
    }

    tracing::warn!(
        packet = claim.packet_name,
        reason = claim.reason,
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        flags = view.flags,
        packetized_sequence = view.packetized_sequence,
        payload_len = view.payload_length,
        owner = owner.as_str(),
        stream_epoch,
        proxy_owned_stream,
        "server M transport-only frame was classified but has no exact continuation owner proof"
    );
    Ok(None)
}

pub(super) fn consume_transport_only_packet_for_ee(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out_packet = bytes.to_vec();
    out_packet.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if out_packet.len() > 7 {
        // Decompile-backed EE window behavior: byte 7's high nibble selects
        // the M-frame kind. Only kind 0 enters CNetLayerWindow::FrameReceive's
        // reliable-data path, which stores the frame and advances the incoming
        // sequence/ACK cursor. The 0x10 control kind is ACK-only and does not
        // consume a sequence number, so an empty progress carrier must stay a
        // data frame while clearing zlib/packet-length semantics. Preserve only
        // the high-priority queue bit.
        out_packet[7] &= 0x08;
    }
    write_be_u16(&mut out_packet, 8, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear transport M packetized sequence"))?;
    write_be_u16(&mut out_packet, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear transport M packetized length"))?;
    encode_legacy_m_crc(&mut out_packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair transport M CRC"))?;
    Ok(out_packet)
}
