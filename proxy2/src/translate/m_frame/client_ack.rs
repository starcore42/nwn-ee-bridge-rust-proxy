//! Proxy-owned ACKs for consumed EE-only client reliable frames.
//!
//! This module is intentionally transport-only. It does not decide game truth
//! and it does not claim arbitrary client packets. Its only job is to keep the
//! EE reliable window coherent after a semantic client filter has already
//! verified that a client reliable frame is EE-only and must not be forwarded to
//! the 1.69 server.

use std::time::{Duration, Instant};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
    translate::VerifiedFamily,
};

use super::synthetic_area::{PendingServerPacket, PendingServerPacketPlacement};

pub(super) const PROXY_OWNED_CLIENT_ACK_REASON: &str =
    "proxy-owned ACK for consumed EE-only client reliable frame";

// EE 8193.37 `CNetLayerWindow::FrameReceive` handles type-1 ACK-control
// frames (`flags & 0xF0 == 0x10`) cumulatively: after accepting ACK N it
// advances `oldest_out` until N is no longer outstanding, then calls
// `LoadWindowWithFrames` if capacity opened.
//
// Driver-only Starcore5 captures showed that `Device_AdvertiseProperty` can
// flood EE's pregame outgoing reliable window before `CharList_RequestUpdateChar`
// can leave the client. The first drain after a consumed frame is immediate so
// the EE window does not fill; if several ACK intents are queued before a drain,
// they still coalesce to the latest cumulative sequence.
const PROXY_OWNED_CLIENT_ACK_COALESCE_DELAY: Duration = Duration::from_millis(0);
const PROXY_OWNED_CLIENT_ACK_RETRANSMIT_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default)]
pub(super) struct ClientAckState {
    pub(super) pending_consumed_ee_only_ack: Option<PendingConsumedEeOnlyAck>,
}

#[derive(Debug, Clone)]
pub(super) struct PendingConsumedEeOnlyAck {
    pub(super) ack_sequence: u16,
    pub(super) due_at: Instant,
    pub(super) transmits: u32,
}

pub(super) fn queue_consumed_ee_only_ack(state: &mut ClientAckState, ack_sequence: u16) {
    let due_at = Instant::now() + PROXY_OWNED_CLIENT_ACK_COALESCE_DELAY;
    let replaced_ack_sequence = state
        .pending_consumed_ee_only_ack
        .replace(PendingConsumedEeOnlyAck {
            ack_sequence,
            due_at,
            transmits: 0,
        })
        .map(|pending| pending.ack_sequence);

    if let Some(replaced) = replaced_ack_sequence {
        tracing::debug!(
            replaced_ack_sequence = replaced,
            ack_sequence,
            "coalesced older proxy-owned EE-only ACK into latest cumulative reliable-window ACK"
        );
    }

    tracing::info!(
        ack_sequence,
        coalesce_delay_ms = PROXY_OWNED_CLIENT_ACK_COALESCE_DELAY.as_millis(),
        "queued coalesced proxy-owned ACK for consumed EE-only client reliable frame"
    );
}

pub(super) fn has_due_consumed_ee_only_ack(state: &ClientAckState, now: Instant) -> bool {
    state
        .pending_consumed_ee_only_ack
        .as_ref()
        .is_some_and(|pending| pending.due_at <= now)
}

pub(super) fn take_due_consumed_ee_only_ack_packets(
    ack_state: &mut ClientAckState,
    now: Instant,
) -> Vec<PendingServerPacket> {
    let Some(pending) = ack_state.pending_consumed_ee_only_ack.as_ref() else {
        return Vec::new();
    };
    if pending.due_at > now {
        return Vec::new();
    }

    let Some(packet) = build_ack_control_frame(pending.ack_sequence) else {
        let dropped = ack_state
            .pending_consumed_ee_only_ack
            .take()
            .expect("pending ACK was checked above");
        tracing::warn!(
            ack_sequence = dropped.ack_sequence,
            "failed to build proxy-owned ACK-control frame for consumed EE-only client reliable frame"
        );
        return Vec::new();
    };

    let pending = ack_state
        .pending_consumed_ee_only_ack
        .as_mut()
        .expect("pending ACK was checked above");
    pending.transmits = pending.transmits.saturating_add(1);
    pending.due_at = now + PROXY_OWNED_CLIENT_ACK_RETRANSMIT_DELAY;

    tracing::info!(
        ack_sequence = pending.ack_sequence,
        transmits = pending.transmits,
        retransmit_delay_ms = PROXY_OWNED_CLIENT_ACK_RETRANSMIT_DELAY.as_millis(),
        "proxy-owned ACK-control emitted for consumed EE-only client reliable frame"
    );

    vec![PendingServerPacket {
        family: VerifiedFamily::ConsumedEmptyMFrame,
        packet,
        due_at: now,
        reason: PROXY_OWNED_CLIENT_ACK_REASON,
        placement: PendingServerPacketPlacement::BeforeCurrentEmit,
    }]
}

fn build_ack_control_frame(ack_sequence: u16) -> Option<Vec<u8>> {
    let mut packet = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
    packet[0] = b'M';
    write_be_u16(&mut packet, 3, 0).then_some(())?;
    write_be_u16(&mut packet, 5, ack_sequence).then_some(())?;
    packet[7] = 0x10;
    write_be_u16(&mut packet, 8, 0).then_some(())?;
    write_be_u16(&mut packet, 10, 0).then_some(())?;
    encode_legacy_m_crc(&mut packet).then_some(())?;

    let view = MFrameView::parse(&packet)?;
    if view.sequence != 0
        || view.ack_sequence != ack_sequence
        || view.flags != 0x10
        || view.payload_length != 0
        || view.trailing_payload_length != 0
    {
        return None;
    }

    Some(packet)
}
