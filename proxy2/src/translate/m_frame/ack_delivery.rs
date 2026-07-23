//! Final-output ACK ownership for the transparent reliable bridge.
//!
//! A source `M` frame can be consumed, held, split, or accompanied by
//! proxy-owned siblings.  Its input ACK is therefore not proof that either
//! downstream peer actually received an ACK-bearing frame.  Capture ACKs from
//! the exact final output batch and publish them only after that batch passes
//! the outer strict validator.

use crate::{
    packet::m::{MFrameType, MFrameView},
    translate::Emit,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AckDeliveryOwner {
    DirectClient,
    PendingClientDrain,
    DirectServer,
    PendingServerDrain,
}

impl AckDeliveryOwner {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::DirectClient => "direct_client",
            Self::PendingClientDrain => "pending_client_drain",
            Self::DirectServer => "direct_server",
            Self::PendingServerDrain => "pending_server_drain",
        }
    }

    pub(super) fn acknowledges_server_sources(self) -> bool {
        // Only a batch derived from a validated EE client datagram proves
        // downstream receipt. Timer/session-owned client packets may carry a
        // coherent ACK toward Diamond, but their acceptance by the upstream
        // socket cannot retire the proxy's retained server source or an owned
        // multi-frame EE output span.
        matches!(self, Self::DirectClient)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PendingAckDelivery {
    pub(super) owner: AckDeliveryOwner,
    /// ACKs in the destination peer's source-sequence domain, in exact output
    /// order. Applying them in order preserves modulo-u16 cumulative progress.
    pub(super) ack_sequences: Vec<u16>,
}

#[derive(Debug, Default)]
pub(super) struct AckDeliveryState {
    pub(super) pending: Option<PendingAckDelivery>,
}

pub(super) fn stage(
    state: &mut AckDeliveryState,
    owner: AckDeliveryOwner,
    emit: &Emit,
) -> anyhow::Result<()> {
    if let Some(pending) = state.pending.as_ref() {
        anyhow::bail!(
            "outgoing M ACK delivery already staged for {} before {}",
            pending.owner.as_str(),
            owner.as_str()
        );
    }

    let mut ack_sequences = Vec::new();
    visit_emit_packets(emit, &mut |packet| {
        let view = MFrameView::parse(packet)
            .ok_or_else(|| anyhow::anyhow!("outgoing ACK candidate is not a complete M frame"))?;
        if !view.crc_valid {
            anyhow::bail!("outgoing ACK candidate has an invalid M CRC");
        }
        let Some(kind) = view.frame_kind() else {
            anyhow::bail!("outgoing ACK candidate has unsupported M frame type");
        };
        if kind != MFrameType::ReliableData && !view.is_exact_control_frame() {
            anyhow::bail!("outgoing ACK candidate has an impossible control shape");
        }
        ack_sequences.push(view.ack_sequence);
        Ok(())
    })?;

    tracing::trace!(
        owner = owner.as_str(),
        ack_sequences = ?ack_sequences,
        packets = ack_sequences.len(),
        "staged destination-facing ACKs from exact outgoing M batch"
    );
    state.pending = Some(PendingAckDelivery {
        owner,
        ack_sequences,
    });
    Ok(())
}

pub(super) fn finish(
    state: &mut AckDeliveryState,
    owner: AckDeliveryOwner,
    accepted: bool,
) -> Vec<u16> {
    let Some(staged_owner) = state.pending.as_ref().map(|pending| pending.owner) else {
        return Vec::new();
    };
    if staged_owner != owner {
        tracing::warn!(
            staged_owner = staged_owner.as_str(),
            callback_owner = owner.as_str(),
            accepted,
            "foreign outgoing M ACK delivery retained for its validation owner"
        );
        return Vec::new();
    }
    let Some(pending) = state.pending.take() else {
        tracing::warn!(
            owner = owner.as_str(),
            accepted,
            "matching outgoing M ACK delivery disappeared before commit"
        );
        return Vec::new();
    };
    if !accepted {
        tracing::trace!(
            owner = owner.as_str(),
            ack_sequences = ?pending.ack_sequences,
            "outgoing M ACK delivery discarded after strict validation rejection"
        );
        return Vec::new();
    }
    pending.ack_sequences
}

fn visit_emit_packets(
    emit: &Emit,
    visitor: &mut impl FnMut(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    match emit {
        Emit::Packet(packet) | Emit::PacketRetireSession { packet, .. } => visitor(packet),
        Emit::Packets(packets)
        | Emit::PacketsPreShifted(packets)
        | Emit::VerifiedPackets { packets, .. }
        | Emit::VerifiedPacketsPreShifted { packets, .. }
        | Emit::VerifiedProofPackets { packets, .. }
        | Emit::VerifiedProofPacketsPreShifted { packets, .. } => {
            for packet in packets {
                visitor(packet)?;
            }
            Ok(())
        }
        Emit::MixedVerifiedPackets(packets) => {
            for (_, packet) in packets {
                visitor(packet)?;
            }
            Ok(())
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            for (_, packet) in packets {
                visitor(packet)?;
            }
            Ok(())
        }
        Emit::Consumed | Emit::ConsumedRetireSession { .. } | Emit::Drop => Ok(()),
    }
}
