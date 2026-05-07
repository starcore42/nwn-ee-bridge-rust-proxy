//! Synthetic area-load fallback routing for reliable `M` frames.
//!
//! This is intentionally a transport-side compatibility shim, not an area
//! semantic parser. Native client `Area_AreaLoaded` always wins; the synthetic
//! packet is armed only after the bridge has inserted the paired synthetic
//! loadbar frames for a verified translated `Area_ClientArea` window.

use std::time::{Duration, Instant};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET},
    translate::{area, loadbar},
};

use super::sequence::{
    SequenceShift, sequence_at_or_after, shift_sequence_for_peer, trim_sequence_shifts,
};

const AREA_MAJOR: u8 = 0x04;
const AREA_LOADED_MINOR: u8 = 0x03;
const LOADBAR_DELAY: Duration = Duration::from_millis(6500);
const AREA_LOADED_FALLBACK_GRACE: Duration = Duration::from_secs(2);
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
    pub(super) packet: Vec<u8>,
    pub(super) due_at: Instant,
    pub(super) reason: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AreaLoadedFallbackReason {
    /// HG legacy area streams can omit a usable height DWORD while still
    /// carrying a tile stream whose width * inferred height is exact. This
    /// fallback is allowed only when that named compatibility transform ran.
    LegacyHgMissingHeightRepair,
}

impl AreaLoadedFallbackReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::LegacyHgMissingHeightRepair => "LegacyHgMissingHeightRepair",
        }
    }
}

pub(super) fn fallback_reason_for_area_rewrite(
    summary: &area::AreaRewriteSummary,
) -> Option<AreaLoadedFallbackReason> {
    summary
        .rewrite_kinds
        .contains(&area::AreaRewriteKind::LegacyHgMissingHeightRepair)
        .then_some(AreaLoadedFallbackReason::LegacyHgMissingHeightRepair)
}

pub(super) fn is_native_area_loaded(high: Option<HighLevel>) -> bool {
    high.map(|high| high.major == AREA_MAJOR && high.minor == AREA_LOADED_MINOR)
        .unwrap_or(false)
}

pub(super) fn clear_pending_area_loaded(pending: &mut Option<PendingAreaLoaded>) {
    *pending = None;
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
    let start_packet = build_synthetic_gameplay_frame(start_sequence, ack_sequence, &start_payload)?;
    let end_packet = build_synthetic_gameplay_frame(end_sequence, ack_sequence, &end_payload)?;

    let now = Instant::now();
    let end_due_at = now + LOADBAR_DELAY;
    server_sequence_shifts.push(SequenceShift {
        base: original_after_sequence.wrapping_add(1),
        delta: LOADBAR_FRAME_COUNT,
    });
    trim_sequence_shifts(server_sequence_shifts);
    pending_packets.push(PendingServerPacket {
        packet: start_packet,
        due_at: now,
        reason: "Area_ClientArea synthetic LoadBar_Start",
    });
    pending_packets.push(PendingServerPacket {
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
    latest_client_sequence_from_client: &mut Option<u16>,
    client_sequence_shifts: &mut Vec<SequenceShift>,
    observed_client_ack: u16,
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

    let Some(latest_client_sequence) = *latest_client_sequence_from_client else {
        tracing::warn!(
            observed_client_ack,
            release_client_ack_sequence = pending.release_client_ack_sequence,
            "client synthetic Area_AreaLoaded fallback cannot release without a client sequence"
        );
        return Ok(None);
    };

    let original_sequence = latest_client_sequence.wrapping_add(1);
    let shifted_sequence = shift_sequence_for_peer(client_sequence_shifts, original_sequence);
    let payload = [0x70, AREA_MAJOR, AREA_LOADED_MINOR];
    let packet =
        build_synthetic_gameplay_frame(shifted_sequence, pending.server_ack_sequence, &payload)?;

    client_sequence_shifts.push(SequenceShift {
        base: original_sequence,
        delta: 1,
    });
    trim_sequence_shifts(client_sequence_shifts);
    *latest_client_sequence_from_client = Some(original_sequence);
    *pending_area_loaded = None;

    tracing::info!(
        original_sequence,
        shifted_sequence,
        observed_client_ack,
        ack_sequence = pending.server_ack_sequence,
        release_client_ack_sequence = pending.release_client_ack_sequence,
        reason = pending.reason.as_str(),
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
        delay_ms = release_at.saturating_duration_since(Instant::now()).as_millis(),
        reason = reason.as_str(),
        "client synthetic Area_AreaLoaded fallback armed after synthetic LoadBar completion"
    );
}

fn build_synthetic_gameplay_frame(
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
