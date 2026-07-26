//! Proxy-owned ACKs for client reliable receive-window events.
//!
//! This module is intentionally transport-only. It does not decide game truth
//! and it does not claim arbitrary client packets. It keeps the EE reliable
//! window coherent when a semantic client filter consumes an EE-only frame and
//! when a valid type-0 datagram falls outside the mirrored receive interval.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::translate::VerifiedFamily;

use super::{
    ack_carrier::build_exact_ack_control_frame,
    synthetic_area::{PendingServerPacket, PendingServerPacketPlacement},
};

pub(super) const PROXY_OWNED_CLIENT_ACK_REASON: &str =
    "proxy-owned ACK for consumed EE-only client reliable frame";
pub(super) const PROXY_OWNED_OUTSIDE_WINDOW_ACK_REASON: &str =
    "proxy-owned cumulative ACK for out-of-window client reliable frame";

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
    pub(super) pending_consumed_ee_only_ack: Option<PendingProxyOwnedAck>,
    /// Diamond `sub_5F3940` lines 751485-751517 and EE `FrameReceive` lines
    /// 878891-878922 send an immediate type-1 control when valid type-0 data is
    /// outside the active receive interval. This queue is one-shot: another
    /// source retransmit is the original trigger for another control.
    pub(super) pending_outside_window_acks: VecDeque<PendingProxyOwnedAck>,
}

#[derive(Debug, Clone)]
pub(super) struct PendingProxyOwnedAck {
    pub(super) ack_sequence: u16,
    pub(super) due_at: Instant,
    pub(super) transmits: u32,
}

pub(super) fn queue_consumed_ee_only_ack(state: &mut ClientAckState, ack_sequence: u16) {
    let due_at = Instant::now() + PROXY_OWNED_CLIENT_ACK_COALESCE_DELAY;
    let replaced_ack_sequence = state
        .pending_consumed_ee_only_ack
        .replace(PendingProxyOwnedAck {
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

pub(super) fn queue_outside_window_ack(state: &mut ClientAckState, ack_sequence: u16) {
    let due_at = Instant::now();
    state
        .pending_outside_window_acks
        .push_back(PendingProxyOwnedAck {
            ack_sequence,
            due_at,
            transmits: 0,
        });

    tracing::info!(
        ack_sequence,
        pending_outside_window_acks = state.pending_outside_window_acks.len(),
        "queued one-shot cumulative ACK for out-of-window client reliable data"
    );
}

pub(super) fn has_due_proxy_owned_ack(state: &ClientAckState, now: Instant) -> bool {
    state
        .pending_consumed_ee_only_ack
        .as_ref()
        .is_some_and(|pending| pending.due_at <= now)
        || state
            .pending_outside_window_acks
            .front()
            .is_some_and(|pending| pending.due_at <= now)
}

pub(super) fn take_due_proxy_owned_ack_packets(
    ack_state: &mut ClientAckState,
    now: Instant,
) -> Vec<PendingServerPacket> {
    let mut packets = Vec::new();
    while let Some(packet) = take_due_outside_window_ack_packet(ack_state, now) {
        packets.push(packet);
    }
    if let Some(packet) = take_due_consumed_ee_only_ack_packet(ack_state, now) {
        packets.push(packet);
    }
    packets
}

fn take_due_outside_window_ack_packet(
    ack_state: &mut ClientAckState,
    now: Instant,
) -> Option<PendingServerPacket> {
    let pending = ack_state.pending_outside_window_acks.front()?;
    if pending.due_at > now {
        return None;
    }

    let pending = ack_state
        .pending_outside_window_acks
        .pop_front()
        .expect("pending outside-window ACK was checked above");
    let Ok(packet) = build_exact_ack_control_frame(pending.ack_sequence) else {
        tracing::warn!(
            ack_sequence = pending.ack_sequence,
            "failed to build cumulative ACK-control frame for out-of-window client reliable data"
        );
        return None;
    };

    tracing::info!(
        ack_sequence = pending.ack_sequence,
        "one-shot cumulative ACK-control emitted for out-of-window client reliable data"
    );

    Some(PendingServerPacket {
        family: VerifiedFamily::ConsumedEmptyMFrame,
        packet,
        insertion_sequence: None,
        due_at: now,
        reason: PROXY_OWNED_OUTSIDE_WINDOW_ACK_REASON,
        placement: PendingServerPacketPlacement::BeforeCurrentEmit,
    })
}

fn take_due_consumed_ee_only_ack_packet(
    ack_state: &mut ClientAckState,
    now: Instant,
) -> Option<PendingServerPacket> {
    let Some(pending) = ack_state.pending_consumed_ee_only_ack.as_ref() else {
        return None;
    };
    if pending.due_at > now {
        return None;
    }

    let Ok(packet) = build_exact_ack_control_frame(pending.ack_sequence) else {
        let dropped = ack_state
            .pending_consumed_ee_only_ack
            .take()
            .expect("pending ACK was checked above");
        tracing::warn!(
            ack_sequence = dropped.ack_sequence,
            "failed to build proxy-owned ACK-control frame for consumed EE-only client reliable frame"
        );
        return None;
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

    Some(PendingServerPacket {
        family: VerifiedFamily::ConsumedEmptyMFrame,
        packet,
        insertion_sequence: None,
        due_at: now,
        reason: PROXY_OWNED_CLIENT_ACK_REASON,
        placement: PendingServerPacketPlacement::BeforeCurrentEmit,
    })
}
