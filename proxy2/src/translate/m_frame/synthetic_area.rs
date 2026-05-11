//! Synthetic area-load fallback routing for reliable `M` frames.
//!
//! This is intentionally a transport-side compatibility shim, not an area
//! semantic parser. Native client `Area_AreaLoaded` always wins.
//!
//! The old in-client driver hook had a stronger signal than the proxy: it
//! could see the EE `Area_ClientArea` dispatch return successfully and then
//! synthesize `Area_AreaLoaded` only when no native client packet appeared.
//! The Rust proxy cannot observe that return value, so it uses the narrower
//! transport-safe version: arm a delayed fallback only when a named, audited
//! area compatibility transform ran, and cancel it immediately if the client
//! sends the native `Area_AreaLoaded` first.

use std::time::{Duration, Instant};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET},
    translate::{VerifiedFamily, VerifiedProof, area, loadbar},
};

use super::sequence::{
    SequenceShift, sequence_at_or_after, shift_sequence_for_peer, trim_sequence_shifts,
};

const AREA_MAJOR: u8 = 0x04;
const AREA_LOADED_MINOR: u8 = 0x03;
// The old in-process hook synthesized Area_AreaLoaded as soon as the EE
// Area_ClientArea dispatch returned. The proxy cannot observe that return, but
// it can keep the same protocol order: emit exact synthetic LoadBar_Start/End
// immediately after the audited Area_ClientArea rewrite, before post-area live
// objects are forwarded. A timed grace window let live-object updates arrive
// while EE was still in the load transition and driver-only captures showed the
// client disconnecting before LoadBar_End fired.
const LOADBAR_DELAY: Duration = Duration::from_millis(0);
// The fallback is gated on the client ACKing the synthetic LoadBar_End
// sequence. That ACK is transport evidence that EE's receive window reached
// the proxy-owned loadbar completion before the legacy server is told the area
// finished loading. Native Area_AreaLoaded still wins because it clears the
// pending fallback before this ACK-gated release path runs.
const AREA_LOADED_FALLBACK_GRACE: Duration = Duration::from_millis(0);
const AREA_LOADED_RETRANSMIT_DELAY: Duration = Duration::from_millis(1250);
const LOADBAR_FRAME_COUNT: u16 = 2;
const LOADBAR_STALL_EVENT_ID: u32 = 2;

#[derive(Debug, Clone)]
pub(super) struct PendingAreaLoaded {
    pub(super) server_ack_sequence: u16,
    pub(super) release_client_ack_sequence: u16,
    pub(super) release_at: Instant,
    pub(super) reason: AreaLoadedFallbackReason,
}

#[derive(Debug, Clone)]
pub(super) struct PendingServerPacket {
    pub(super) family: VerifiedFamily,
    pub(super) packet: Vec<u8>,
    pub(super) due_at: Instant,
    pub(super) reason: &'static str,
}

#[derive(Debug, Clone)]
pub(super) struct PendingVerifiedServerPacket {
    pub(super) proof: VerifiedProof,
    pub(super) packet: Vec<u8>,
    pub(super) reason: &'static str,
}

#[derive(Debug, Clone)]
pub(super) struct ServerHoldGate {
    pub(super) release_client_ack_sequence: u16,
    pub(super) reason: AreaLoadedFallbackReason,
    pub(super) armed_at: Instant,
}

#[derive(Debug, Clone)]
pub(super) struct InFlightAreaLoaded {
    pub(super) packet: Vec<u8>,
    pub(super) original_sequence: u16,
    pub(super) shifted_sequence: u16,
    pub(super) ack_sequence: u16,
    pub(super) next_retransmit_at: Instant,
    pub(super) retransmits: u32,
    pub(super) reason: AreaLoadedFallbackReason,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AreaLoadedFallbackReason {
    /// EE consumes a client-only area-name-mode BOOL before the legacy
    /// `PackAreaIntoMessage` fragment cursor reaches the area name. The
    /// translator forces that proven bit rather than shifting the stream.
    ExactEeAreaNameModeBitForce,
    /// EE expects two post-static object list counts that Diamond did not send
    /// for this legacy `Area_ClientArea` shape. The translator inserts the
    /// decompile-backed zero-count DWORDs before the fragment section.
    ExactEePostStaticListZeroWords,
    /// HG legacy area streams can omit a usable height DWORD while still
    /// carrying a tile stream whose width * inferred height is exact. This
    /// fallback is allowed only when that named compatibility transform ran.
    LegacyHgMissingHeightRepair,
}

impl AreaLoadedFallbackReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::ExactEeAreaNameModeBitForce => "ExactEeAreaNameModeBitForce",
            Self::ExactEePostStaticListZeroWords => "ExactEePostStaticListZeroWords",
            Self::LegacyHgMissingHeightRepair => "LegacyHgMissingHeightRepair",
        }
    }
}

pub(super) fn fallback_reason_for_area_rewrite(
    summary: &area::AreaRewriteSummary,
) -> Option<AreaLoadedFallbackReason> {
    let reason = if summary
        .rewrite_kinds
        .contains(&area::AreaRewriteKind::LegacyHgMissingHeightRepair)
    {
        Some(AreaLoadedFallbackReason::LegacyHgMissingHeightRepair)
    } else if summary
        .rewrite_kinds
        .contains(&area::AreaRewriteKind::ExactEePostStaticListZeroWords)
    {
        Some(AreaLoadedFallbackReason::ExactEePostStaticListZeroWords)
    } else if summary
        .rewrite_kinds
        .contains(&area::AreaRewriteKind::ExactEeAreaNameModeBitForce)
    {
        Some(AreaLoadedFallbackReason::ExactEeAreaNameModeBitForce)
    } else {
        None
    };

    if let Some(reason) = reason {
        tracing::info!(
            area_resref = %summary.area_resref,
            reason = reason.as_str(),
            rewrite_kinds = ?summary.rewrite_kinds,
            "client synthetic Area_AreaLoaded fallback armed candidate: named Area_ClientArea compatibility transform ran"
        );
    }
    reason
}

pub(super) fn is_native_area_loaded(high: Option<HighLevel>) -> bool {
    high.map(|high| high.major == AREA_MAJOR && high.minor == AREA_LOADED_MINOR)
        .unwrap_or(false)
}

pub(super) fn clear_pending_area_loaded(pending: &mut Option<PendingAreaLoaded>) {
    *pending = None;
}

pub(super) fn clear_in_flight_area_loaded(in_flight: &mut Option<InFlightAreaLoaded>) {
    *in_flight = None;
}

pub(super) fn observe_area_loaded_server_ack(
    in_flight: &mut Option<InFlightAreaLoaded>,
    server_ack_sequence: u16,
) {
    let Some(pending) = in_flight.as_ref() else {
        return;
    };
    if server_ack_sequence == 0
        || !sequence_at_or_after(server_ack_sequence, pending.shifted_sequence)
    {
        return;
    }

    tracing::info!(
        server_ack_sequence,
        original_sequence = pending.original_sequence,
        shifted_sequence = pending.shifted_sequence,
        retransmits = pending.retransmits,
        reason = pending.reason.as_str(),
        "client synthetic Area_AreaLoaded acknowledged by server"
    );
    *in_flight = None;
}

pub(super) fn arm_server_hold_gate_until_client_ack(
    gate: &mut Option<ServerHoldGate>,
    pending_area_loaded: Option<&PendingAreaLoaded>,
) {
    let Some(pending) = pending_area_loaded else {
        return;
    };
    *gate = Some(ServerHoldGate {
        release_client_ack_sequence: pending.release_client_ack_sequence,
        reason: pending.reason,
        armed_at: Instant::now(),
    });
    tracing::info!(
        release_client_ack_sequence = pending.release_client_ack_sequence,
        reason = pending.reason.as_str(),
        "server-to-client post-area hold gate armed until EE ACKs synthetic LoadBar_End"
    );
}

pub(super) fn observe_server_hold_gate_client_ack(
    gate: &mut Option<ServerHoldGate>,
    observed_client_ack: u16,
) {
    let Some(active) = gate.as_ref() else {
        return;
    };
    if observed_client_ack == 0
        || !sequence_at_or_after(observed_client_ack, active.release_client_ack_sequence)
    {
        return;
    }

    tracing::info!(
        observed_client_ack,
        release_client_ack_sequence = active.release_client_ack_sequence,
        held_ms = Instant::now()
            .saturating_duration_since(active.armed_at)
            .as_millis(),
        reason = active.reason.as_str(),
        "server-to-client post-area hold gate opened by EE ACK"
    );
    *gate = None;
}

pub(super) fn maybe_queue_area_loaded_retransmit(
    in_flight: &mut Option<InFlightAreaLoaded>,
    pending_client_to_server_packets: &mut Vec<Vec<u8>>,
    server_ack_sequence: u16,
) {
    observe_area_loaded_server_ack(in_flight, server_ack_sequence);

    let Some(pending) = in_flight.as_mut() else {
        return;
    };
    let now = Instant::now();
    if now < pending.next_retransmit_at {
        return;
    }

    pending.retransmits = pending.retransmits.saturating_add(1);
    pending.next_retransmit_at = now + AREA_LOADED_RETRANSMIT_DELAY;
    pending_client_to_server_packets.push(pending.packet.clone());
    tracing::warn!(
        server_ack_sequence,
        original_sequence = pending.original_sequence,
        shifted_sequence = pending.shifted_sequence,
        ack_sequence = pending.ack_sequence,
        retransmits = pending.retransmits,
        reason = pending.reason.as_str(),
        pending_local_client_packets = pending_client_to_server_packets.len(),
        "client synthetic Area_AreaLoaded not yet ACKed; retransmitting reliable M frame"
    );
}

pub(super) fn queue_loadbar_and_area_loaded_fallback(
    pending_packets: &mut Vec<PendingServerPacket>,
    pending_area_loaded: &mut Option<PendingAreaLoaded>,
    server_sequence_shifts: &mut Vec<SequenceShift>,
    original_after_sequence: u16,
    ack_sequence: u16,
    area_loaded_fallback_reason: Option<AreaLoadedFallbackReason>,
) -> anyhow::Result<()> {
    let shifted_after_sequence =
        shift_sequence_for_peer(server_sequence_shifts, original_after_sequence);
    let start_sequence = shifted_after_sequence.wrapping_add(1);
    let end_sequence = shifted_after_sequence.wrapping_add(2);

    let start_payload = loadbar::start_payload(LOADBAR_STALL_EVENT_ID);
    let end_payload = loadbar::end_success_payload(LOADBAR_STALL_EVENT_ID);
    let start_packet =
        build_synthetic_gameplay_frame(start_sequence, ack_sequence, &start_payload)?;
    let end_packet = build_synthetic_gameplay_frame(end_sequence, ack_sequence, &end_payload)?;

    let now = Instant::now();
    let end_due_at = now + LOADBAR_DELAY;
    server_sequence_shifts.push(SequenceShift {
        base: original_after_sequence.wrapping_add(1),
        delta: LOADBAR_FRAME_COUNT,
    });
    trim_sequence_shifts(server_sequence_shifts);
    pending_packets.push(PendingServerPacket {
        family: VerifiedFamily::LoadBar,
        packet: start_packet,
        due_at: now,
        reason: "Area_ClientArea synthetic LoadBar_Start",
    });
    pending_packets.push(PendingServerPacket {
        family: VerifiedFamily::LoadBar,
        packet: end_packet,
        due_at: end_due_at,
        reason: "Area_ClientArea synthetic LoadBar_End",
    });
    if let Some(reason) = area_loaded_fallback_reason {
        arm_area_loaded_fallback(
            pending_area_loaded,
            original_after_sequence,
            end_sequence,
            end_due_at + AREA_LOADED_FALLBACK_GRACE,
            reason,
        );
    } else {
        *pending_area_loaded = None;
        tracing::info!(
            original_after_sequence,
            end_sequence,
            "client synthetic Area_AreaLoaded fallback not armed: no named compatibility reason"
        );
    }

    tracing::info!(
        original_after_sequence,
        shifted_after_sequence,
        start_sequence,
        end_sequence,
        ack_sequence,
        shift_base = original_after_sequence.wrapping_add(1),
        shift_delta = LOADBAR_FRAME_COUNT,
        end_delay_ms = LOADBAR_DELAY.as_millis(),
        fallback_grace_ms = AREA_LOADED_FALLBACK_GRACE.as_millis(),
        pending_server_packets = pending_packets.len(),
        shifts = server_sequence_shifts.len(),
        area_loaded_fallback_reason =
            area_loaded_fallback_reason.map(AreaLoadedFallbackReason::as_str),
        "server Area_ClientArea synthetic LoadBar frames queued"
    );

    Ok(())
}

pub(super) fn maybe_build_area_loaded_client_packet(
    pending_area_loaded: &mut Option<PendingAreaLoaded>,
    in_flight_area_loaded: &mut Option<InFlightAreaLoaded>,
    latest_client_sequence_from_client: &mut Option<u16>,
    client_sequence_shifts: &mut Vec<SequenceShift>,
    observed_client_ack: u16,
    origin_ack_sequence: u16,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(pending) = pending_area_loaded.clone() else {
        return Ok(None);
    };
    if observed_client_ack == 0
        || !sequence_at_or_after(observed_client_ack, pending.release_client_ack_sequence)
        || Instant::now() < pending.release_at
    {
        return Ok(None);
    }

    release_area_loaded_client_packet(
        pending_area_loaded,
        in_flight_area_loaded,
        latest_client_sequence_from_client,
        client_sequence_shifts,
        pending,
        Some(observed_client_ack),
        origin_ack_sequence,
        "client ACKed synthetic LoadBar_End",
    )
}

fn release_area_loaded_client_packet(
    pending_area_loaded: &mut Option<PendingAreaLoaded>,
    in_flight_area_loaded: &mut Option<InFlightAreaLoaded>,
    latest_client_sequence_from_client: &mut Option<u16>,
    client_sequence_shifts: &mut Vec<SequenceShift>,
    pending: PendingAreaLoaded,
    observed_client_ack: Option<u16>,
    ack_sequence: u16,
    release_trigger: &'static str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(latest_client_sequence) = *latest_client_sequence_from_client else {
        tracing::warn!(
            observed_client_ack = ?observed_client_ack,
            release_client_ack_sequence = pending.release_client_ack_sequence,
            release_trigger,
            "client synthetic Area_AreaLoaded fallback cannot release without a client sequence"
        );
        return Ok(None);
    };

    let original_sequence = latest_client_sequence.wrapping_add(1);
    let shifted_sequence = shift_sequence_for_peer(client_sequence_shifts, original_sequence);
    let payload = [0x70, AREA_MAJOR, AREA_LOADED_MINOR];
    let packet = build_synthetic_gameplay_frame(shifted_sequence, ack_sequence, &payload)?;

    client_sequence_shifts.push(SequenceShift {
        base: original_sequence,
        delta: 1,
    });
    trim_sequence_shifts(client_sequence_shifts);
    *latest_client_sequence_from_client = Some(original_sequence);
    *pending_area_loaded = None;
    *in_flight_area_loaded = Some(InFlightAreaLoaded {
        packet: packet.clone(),
        original_sequence,
        shifted_sequence,
        ack_sequence,
        next_retransmit_at: Instant::now() + AREA_LOADED_RETRANSMIT_DELAY,
        retransmits: 0,
        reason: pending.reason,
    });

    tracing::info!(
        original_sequence,
        shifted_sequence,
        observed_client_ack = ?observed_client_ack,
        ack_sequence,
        armed_server_ack_sequence = pending.server_ack_sequence,
        release_client_ack_sequence = pending.release_client_ack_sequence,
        reason = pending.reason.as_str(),
        release_trigger,
        shifts = client_sequence_shifts.len(),
        "client synthetic Area_AreaLoaded released"
    );

    Ok(Some(packet))
}

fn arm_area_loaded_fallback(
    pending: &mut Option<PendingAreaLoaded>,
    server_ack_sequence: u16,
    release_client_ack_sequence: u16,
    release_at: Instant,
    reason: AreaLoadedFallbackReason,
) {
    *pending = Some(PendingAreaLoaded {
        server_ack_sequence,
        release_client_ack_sequence,
        release_at,
        reason,
    });
    tracing::info!(
        server_ack_sequence,
        release_client_ack_sequence,
        delay_ms = release_at
            .saturating_duration_since(Instant::now())
            .as_millis(),
        reason = reason.as_str(),
        "client synthetic Area_AreaLoaded fallback armed after synthetic LoadBar completion"
    );
}

pub(super) fn build_synthetic_gameplay_frame(
    sequence: u16,
    ack_sequence: u16,
    payload: &[u8],
) -> anyhow::Result<Vec<u8>> {
    if payload.len() > u16::MAX as usize {
        anyhow::bail!("synthetic gameplay payload is too large: {}", payload.len());
    }

    let mut packet = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
    packet[0] = b'M';
    write_be_u16(&mut packet, 3, sequence)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic M sequence"))?;
    write_be_u16(&mut packet, 5, ack_sequence)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic M ack"))?;
    packet[7] = 0x0A;
    write_be_u16(&mut packet, 8, 1)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic packetized sequence"))?;
    write_be_u16(&mut packet, 10, payload.len() as u16)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write synthetic packetized length"))?;
    packet.extend_from_slice(payload);
    encode_legacy_m_crc(&mut packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair synthetic M CRC"))?;
    Ok(packet)
}
