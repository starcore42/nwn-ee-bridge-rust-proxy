//! Synthetic area-load fallback routing for reliable `M` frames.
//!
//! This is intentionally a transport-side compatibility shim, not an area
//! semantic parser. Native client `Area_AreaLoaded` wins until the proxy has
//! emitted an audited fallback. After that, a matching late native
//! `Area_AreaLoaded` is a duplicate semantic acknowledgement and must be
//! consumed as an empty reliable frame rather than forwarded to the server.
//!
//! The old in-client driver hook had a stronger signal than the proxy: it
//! could see the EE `Area_ClientArea` dispatch return successfully and then
//! synthesize `Area_AreaLoaded` only when no native client packet appeared.
//! The Rust proxy cannot observe that return value, so it uses the narrower
//! transport-safe version: arm a delayed fallback only when a named, audited
//! area compatibility transform ran, wait for EE to ACK the rewritten
//! `Area_ClientArea` frame, give native completion a grace window, and cancel
//! immediately if the client sends the native `Area_AreaLoaded` first.

use std::time::{Duration, Instant};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
    translate::{VerifiedFamily, VerifiedProof, area, loadbar},
};

use super::sequence::{
    SequenceShift, sequence_at_or_after, shift_sequence_for_peer, trim_sequence_shifts,
};

const AREA_MAJOR: u8 = 0x04;
const AREA_LOADED_MINOR: u8 = 0x03;
// The old in-process hook synthesized Area_AreaLoaded only after it could see
// the EE Area_ClientArea dispatch return. The proxy cannot observe that
// in-process return, so synthetic Area_AreaLoaded must be a true fallback.
// Synthetic LoadBar is deliberately opt-in. EE/Diamond decompiles show LoadBar
// as an exact stall-event UI family (`0x2C`) rather than an area-content proof,
// and driver-only captures on 2026-05-13 showed a proxy-owned LoadBar_Start can
// disconnect EE while Area_ClientArea is still dispatching. When a diagnostic
// run explicitly enables it, keep the synthetic stall event open while EE
// consumes the rewritten Area_ClientArea payload; immediate LoadBar_End can
// race the client-side area load.
const LOADBAR_DELAY: Duration = Duration::from_millis(6_500);
// Driver-only evidence on 2026-05-12 showed that ACKing LoadBar_End is not a
// safe release condition in either direction: EE can ACK a transport frame
// before the area object registry is ready, but it can also never ACK the
// proxy-owned synthetic LoadBar frames when the area stream is already being
// gated. Give native Area_AreaLoaded a normal load window before using the
// proxy-owned fallback, then release by the audited timeout rather than by a
// synthetic ACK that may never arrive. Historical clean Docks loads were about
// five seconds, so eight seconds keeps the fallback available without racing
// the native client.
const AREA_LOADED_FALLBACK_AFTER_LOADBAR_GRACE: Duration = Duration::from_millis(8_000);
// When synthetic LoadBar is disabled for driver-only isolation, the proxy has a
// narrower state proof: an exact Area_ClientArea rewrite was emitted and the EE
// client ACKed the final rewritten area frame. That ACK is still transport
// proof, not in-process load completion. EE/Diamond decompiles route native
// `Area_AreaLoaded` through the client gameplay sender, so that native packet is
// the stronger semantic proof that the area reader returned and gameplay packets
// can resume. Driver-only captures also showed that holding post-area packets
// forever can deadlock native completion. Keep the gate stateful: an area ACK
// starts a short native-completion grace window, native Area_AreaLoaded opens the
// gate immediately, and the grace timeout releases as a fallback only if native
// completion does not arrive.
const AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE: Duration = Duration::from_millis(30_000);
const SERVER_HOLD_GATE_AFTER_AREA_ACK_GRACE: Duration = Duration::from_millis(5_000);
const AREA_LOADED_RETRANSMIT_DELAY: Duration = Duration::from_millis(1250);
const LOADBAR_FRAME_COUNT: u16 = 2;
const LOADBAR_STALL_EVENT_ID: u32 = 2;

#[derive(Debug, Clone)]
pub(super) struct PendingAreaLoaded {
    pub(super) server_ack_sequence: u16,
    pub(super) release_client_ack_sequence: u16,
    pub(super) release_at: Instant,
    pub(super) require_client_ack_before_release: bool,
    pub(super) client_ack_observed_at: Option<Instant>,
    pub(super) reason: AreaLoadedFallbackReason,
}

#[derive(Debug, Clone)]
pub(super) struct PendingServerPacket {
    pub(super) family: VerifiedFamily,
    pub(super) packet: Vec<u8>,
    pub(super) due_at: Instant,
    pub(super) reason: &'static str,
    pub(super) placement: PendingServerPacketPlacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingServerPacketPlacement {
    BeforeCurrentEmit,
    AfterCurrentEmit,
}

#[derive(Debug, Clone)]
pub(super) struct PendingVerifiedServerPacket {
    pub(super) proof: VerifiedProof,
    pub(super) packet: Vec<u8>,
    pub(super) reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingVerifiedServerPacketQueueResult {
    Queued {
        held_packets: usize,
    },
    CollapsedReliableReplay {
        sequence: u16,
        held_packets: usize,
    },
}

/// Queue a verified server packet behind the area-load gate without converting
/// reliable retransmits into multiple future sends.
///
/// Diamond/EE reliable `M` traffic can replay the same server sequence while
/// the proxy is deliberately holding post-area gameplay packets. Once the gate
/// opens, releasing every replay as if it were a distinct packet breaks the
/// server-authoritative reliable-window model: the client sees stale sequence
/// repeats after progress has already moved on. A replay with the same non-zero
/// reliable sequence is therefore collapsed into the already-held proof packet.
pub(super) fn queue_pending_verified_server_packet(
    pending: &mut Vec<PendingVerifiedServerPacket>,
    proof: VerifiedProof,
    packet: Vec<u8>,
    reason: &'static str,
) -> PendingVerifiedServerPacketQueueResult {
    if let Some(sequence) = reliable_m_sequence(&packet) {
        if pending
            .iter()
            .any(|held| reliable_m_sequence(&held.packet) == Some(sequence))
        {
            return PendingVerifiedServerPacketQueueResult::CollapsedReliableReplay {
                sequence,
                held_packets: pending.len(),
            };
        }
    } else if pending
        .iter()
        .any(|held| held.proof == proof && held.packet == packet)
    {
        return PendingVerifiedServerPacketQueueResult::CollapsedReliableReplay {
            sequence: 0,
            held_packets: pending.len(),
        };
    }

    pending.push(PendingVerifiedServerPacket {
        proof,
        packet,
        reason,
    });
    PendingVerifiedServerPacketQueueResult::Queued {
        held_packets: pending.len(),
    }
}

fn reliable_m_sequence(packet: &[u8]) -> Option<u16> {
    let view = MFrameView::parse(packet)?;
    (view.sequence != 0).then_some(view.sequence)
}

#[derive(Debug, Clone)]
pub(super) struct ServerHoldGate {
    pub(super) release_client_ack_sequence: u16,
    pub(super) reason: AreaLoadedFallbackReason,
    pub(super) armed_at: Instant,
    pub(super) area_ack_observed_at: Option<Instant>,
    pub(super) release_at: Option<Instant>,
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

#[derive(Debug, Clone)]
pub(super) struct CompletedAreaLoaded {
    pub(super) original_sequence: u16,
    pub(super) shifted_sequence: u16,
    pub(super) ack_sequence: u16,
    pub(super) acknowledged_at: Instant,
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

pub(super) fn clear_server_hold_gate(gate: &mut Option<ServerHoldGate>, trigger: &'static str) {
    let Some(active) = gate.take() else {
        return;
    };
    tracing::info!(
        release_client_ack_sequence = active.release_client_ack_sequence,
        held_ms = Instant::now()
            .saturating_duration_since(active.armed_at)
            .as_millis(),
        reason = active.reason.as_str(),
        trigger,
        "server-to-client post-area hold gate opened"
    );
}

pub(super) fn observe_area_loaded_server_ack(
    in_flight: &mut Option<InFlightAreaLoaded>,
    completed: &mut Option<CompletedAreaLoaded>,
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
    *completed = Some(CompletedAreaLoaded {
        original_sequence: pending.original_sequence,
        shifted_sequence: pending.shifted_sequence,
        ack_sequence: pending.ack_sequence,
        acknowledged_at: Instant::now(),
        reason: pending.reason,
    });
    *in_flight = None;
}

pub(super) fn consume_late_native_area_loaded_after_completed_synthetic(
    completed: &mut Option<CompletedAreaLoaded>,
    native_sequence: u16,
    native_ack_sequence: u16,
) -> bool {
    let Some(active) = completed.as_ref() else {
        return false;
    };

    if native_sequence != active.original_sequence {
        tracing::info!(
            native_sequence,
            native_ack_sequence,
            completed_original_sequence = active.original_sequence,
            completed_shifted_sequence = active.shifted_sequence,
            completed_ack_sequence = active.ack_sequence,
            acknowledged_ms = Instant::now()
                .saturating_duration_since(active.acknowledged_at)
                .as_millis(),
            reason = active.reason.as_str(),
            "native Area_AreaLoaded does not match the completed proxy-owned fallback; forwarding as native area-load proof"
        );
        *completed = None;
        return false;
    }

    let active = completed.take().expect("completed area-loaded state");
    tracing::warn!(
        native_sequence,
        native_ack_sequence,
        completed_original_sequence = active.original_sequence,
        completed_shifted_sequence = active.shifted_sequence,
        completed_ack_sequence = active.ack_sequence,
        acknowledged_ms = Instant::now()
            .saturating_duration_since(active.acknowledged_at)
            .as_millis(),
        reason = active.reason.as_str(),
        "late native Area_AreaLoaded matched an already-ACKed proxy-owned fallback; consuming duplicate semantic event"
    );
    true
}

pub(super) fn arm_server_hold_gate_after_area_release(
    gate: &mut Option<ServerHoldGate>,
    release_client_ack_sequence: u16,
    reason: Option<AreaLoadedFallbackReason>,
) {
    let Some(reason) = reason else {
        return;
    };
    *gate = Some(ServerHoldGate {
        release_client_ack_sequence,
        reason,
        armed_at: Instant::now(),
        area_ack_observed_at: None,
        release_at: None,
    });
    tracing::info!(
        release_client_ack_sequence,
        reason = reason.as_str(),
        native_grace_ms = SERVER_HOLD_GATE_AFTER_AREA_ACK_GRACE.as_millis(),
        "server-to-client post-area hold gate armed until native Area_AreaLoaded or ACK grace"
    );
}

pub(super) fn observe_server_hold_gate_client_ack(
    gate: &mut Option<ServerHoldGate>,
    observed_client_ack: u16,
) {
    let Some(active) = gate.as_mut() else {
        return;
    };
    if observed_client_ack == 0 {
        return;
    }
    let now = Instant::now();
    let ack_satisfied = sequence_at_or_after(observed_client_ack, active.release_client_ack_sequence);
    if !ack_satisfied {
        return;
    }
    if active.area_ack_observed_at.is_none() {
        active.area_ack_observed_at = Some(now);
        active.release_at = Some(now + SERVER_HOLD_GATE_AFTER_AREA_ACK_GRACE);
        tracing::info!(
            observed_client_ack,
            release_client_ack_sequence = active.release_client_ack_sequence,
            native_grace_ms = SERVER_HOLD_GATE_AFTER_AREA_ACK_GRACE.as_millis(),
            held_ms = now.saturating_duration_since(active.armed_at).as_millis(),
            reason = active.reason.as_str(),
            "area-load ACK observed; holding verified post-area server packets for native Area_AreaLoaded grace"
        );
        return;
    }
    let Some(release_at) = active.release_at else {
        return;
    };
    if now < release_at {
        return;
    }
    tracing::info!(
        observed_client_ack,
        release_client_ack_sequence = active.release_client_ack_sequence,
        held_ms = now.saturating_duration_since(active.armed_at).as_millis(),
        reason = active.reason.as_str(),
        "area-load ACK grace elapsed without native Area_AreaLoaded; verified post-area server packets may now flow"
    );
    clear_server_hold_gate(
        gate,
        "EE ACKed rewritten Area_ClientArea and native Area_AreaLoaded grace elapsed",
    );
}

pub(super) fn maybe_queue_area_loaded_retransmit(
    in_flight: &mut Option<InFlightAreaLoaded>,
    completed: &mut Option<CompletedAreaLoaded>,
    pending_client_to_server_packets: &mut Vec<Vec<u8>>,
    server_ack_sequence: u16,
) {
    observe_area_loaded_server_ack(in_flight, completed, server_ack_sequence);

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
    synthesize_loadbar: bool,
) -> anyhow::Result<()> {
    let shifted_after_sequence =
        shift_sequence_for_peer(server_sequence_shifts, original_after_sequence);
    let now = Instant::now();
    let (
        release_client_ack_sequence,
        release_at,
        start_sequence,
        end_sequence,
        shift_base,
        shift_delta,
        end_delay_ms,
    ) = if synthesize_loadbar {
        let start_sequence = shifted_after_sequence.wrapping_add(1);
        let end_sequence = shifted_after_sequence.wrapping_add(2);

        let start_payload = loadbar::start_payload(LOADBAR_STALL_EVENT_ID);
        let end_payload = loadbar::end_success_payload(LOADBAR_STALL_EVENT_ID);
        let start_packet =
            build_synthetic_gameplay_frame(start_sequence, ack_sequence, &start_payload)?;
        let end_packet = build_synthetic_gameplay_frame(end_sequence, ack_sequence, &end_payload)?;

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
            placement: PendingServerPacketPlacement::AfterCurrentEmit,
        });
        pending_packets.push(PendingServerPacket {
            family: VerifiedFamily::LoadBar,
            packet: end_packet,
            due_at: end_due_at,
            reason: "Area_ClientArea synthetic LoadBar_End",
            placement: PendingServerPacketPlacement::AfterCurrentEmit,
        });
        (
            end_sequence,
            end_due_at + AREA_LOADED_FALLBACK_AFTER_LOADBAR_GRACE,
            Some(start_sequence),
            Some(end_sequence),
            Some(original_after_sequence.wrapping_add(1)),
            LOADBAR_FRAME_COUNT,
            Some(LOADBAR_DELAY.as_millis()),
        )
    } else {
        (
            shifted_after_sequence,
            now + AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE,
            None,
            None,
            None,
            0,
            None,
        )
    };
    if let Some(reason) = area_loaded_fallback_reason {
        arm_area_loaded_fallback(
            pending_area_loaded,
            original_after_sequence,
            release_client_ack_sequence,
            release_at,
            !synthesize_loadbar,
            reason,
        );
    } else {
        *pending_area_loaded = None;
        tracing::info!(
            original_after_sequence,
            release_client_ack_sequence,
            "client synthetic Area_AreaLoaded fallback not armed: no named compatibility reason"
        );
    }

    tracing::info!(
        original_after_sequence,
        shifted_after_sequence,
        start_sequence,
        end_sequence,
        ack_sequence,
        shift_base,
        shift_delta,
        end_delay_ms,
        fallback_after_loadbar_grace_ms = AREA_LOADED_FALLBACK_AFTER_LOADBAR_GRACE.as_millis(),
        fallback_after_area_ack_grace_ms = AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE.as_millis(),
        pending_server_packets = pending_packets.len(),
        shifts = server_sequence_shifts.len(),
        synthetic_loadbar = synthesize_loadbar,
        synthetic_area_loaded_enabled = area_loaded_fallback_reason.is_some(),
        area_loaded_fallback_reason =
            area_loaded_fallback_reason.map(AreaLoadedFallbackReason::as_str),
        "server Area_ClientArea area-load side effects queued"
    );

    Ok(())
}

pub(super) fn maybe_build_area_loaded_client_packet(
    pending_area_loaded: &mut Option<PendingAreaLoaded>,
    in_flight_area_loaded: &mut Option<InFlightAreaLoaded>,
    server_hold_gate: &mut Option<ServerHoldGate>,
    latest_client_sequence_from_client: &mut Option<u16>,
    client_sequence_shifts: &mut Vec<SequenceShift>,
    observed_client_ack: u16,
    origin_ack_sequence: u16,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(active) = pending_area_loaded.as_mut() else {
        return Ok(None);
    };
    let now = Instant::now();
    let client_ack_satisfied = observed_client_ack != 0
        && sequence_at_or_after(observed_client_ack, active.release_client_ack_sequence);
    if active.require_client_ack_before_release && !client_ack_satisfied {
        tracing::trace!(
            observed_client_ack,
            release_client_ack_sequence = active.release_client_ack_sequence,
            reason = active.reason.as_str(),
            "client synthetic Area_AreaLoaded fallback waiting for EE ACK of rewritten Area_ClientArea"
        );
        return Ok(None);
    }
    if active.require_client_ack_before_release
        && client_ack_satisfied
        && active.client_ack_observed_at.is_none()
    {
        active.client_ack_observed_at = Some(now);
        active.release_at = now + AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE;
        tracing::info!(
            observed_client_ack,
            release_client_ack_sequence = active.release_client_ack_sequence,
            native_grace_ms = AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE.as_millis(),
            reason = active.reason.as_str(),
            "area-load release ACK observed; synthetic Area_AreaLoaded fallback is waiting for native EE completion grace"
        );
        return Ok(None);
    }
    let release_due = now >= active.release_at;
    if !release_due {
        return Ok(None);
    }

    let pending = active.clone();

    let release_trigger = if client_ack_satisfied {
        "client ACKed area-load release sequence"
    } else {
        tracing::warn!(
            observed_client_ack,
            release_client_ack_sequence = pending.release_client_ack_sequence,
            reason = pending.reason.as_str(),
            "client synthetic Area_AreaLoaded fallback grace elapsed without area-load release ACK"
        );
        "fallback grace elapsed without area-load release ACK"
    };

    let released = release_area_loaded_client_packet(
        pending_area_loaded,
        in_flight_area_loaded,
        latest_client_sequence_from_client,
        client_sequence_shifts,
        pending,
        Some(observed_client_ack),
        origin_ack_sequence,
        release_trigger,
    )?;
    if released.is_some() {
        clear_server_hold_gate(
            server_hold_gate,
            "synthetic Area_AreaLoaded fallback released",
        );
    }
    Ok(released)
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

    // Proxy-owned client packets are inserted *before* the next native client
    // reliable sequence. Do not advance `latest_client_sequence_from_client`
    // here: that field tracks the last sequence actually observed from EE.
    //
    // This matters when two synthetic client packets are needed before EE sends
    // another native reliable packet. If the first insert advanced the native
    // cursor, the second insert would skip the server-facing sequence reserved
    // for the future native packet and the 1.69 server would never ACK it.
    let original_sequence = latest_client_sequence.wrapping_add(1);
    let shifted_sequence = shift_sequence_for_peer(client_sequence_shifts, original_sequence);
    let payload = [0x70, AREA_MAJOR, AREA_LOADED_MINOR];
    let packet = build_synthetic_gameplay_frame(shifted_sequence, ack_sequence, &payload)?;

    client_sequence_shifts.push(SequenceShift {
        base: original_sequence,
        delta: 1,
    });
    trim_sequence_shifts(client_sequence_shifts);
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
    require_client_ack_before_release: bool,
    reason: AreaLoadedFallbackReason,
) {
    *pending = Some(PendingAreaLoaded {
        server_ack_sequence,
        release_client_ack_sequence,
        release_at,
        require_client_ack_before_release,
        client_ack_observed_at: None,
        reason,
    });
    tracing::info!(
        server_ack_sequence,
        release_client_ack_sequence,
        require_client_ack_before_release,
        delay_ms = release_at
            .saturating_duration_since(Instant::now())
            .as_millis(),
        reason = reason.as_str(),
        "client synthetic Area_AreaLoaded fallback armed after verified Area_ClientArea rewrite"
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

#[cfg(test)]
mod tests {
    use crate::packet::m::MFrameView;

    use super::*;

    fn due_area_loaded(reason: AreaLoadedFallbackReason) -> PendingAreaLoaded {
        PendingAreaLoaded {
            server_ack_sequence: 30,
            release_client_ack_sequence: 31,
            release_at: Instant::now() - Duration::from_millis(1),
            require_client_ack_before_release: false,
            client_ack_observed_at: None,
            reason,
        }
    }

    fn ack_gated_area_loaded(reason: AreaLoadedFallbackReason) -> PendingAreaLoaded {
        PendingAreaLoaded {
            server_ack_sequence: 30,
            release_client_ack_sequence: 31,
            release_at: Instant::now(),
            require_client_ack_before_release: true,
            client_ack_observed_at: None,
            reason,
        }
    }

    #[test]
    fn area_loaded_fallback_waits_for_native_grace_after_area_ack_without_loadbar() {
        let mut latest_native_client_sequence = Some(73);
        let mut client_sequence_shifts = vec![SequenceShift { base: 73, delta: 1 }];
        let mut in_flight = None;
        let mut hold_gate = Some(ServerHoldGate {
            release_client_ack_sequence: 31,
            reason: AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
            armed_at: Instant::now(),
            area_ack_observed_at: None,
            release_at: None,
        });
        let mut pending =
            Some(ack_gated_area_loaded(AreaLoadedFallbackReason::LegacyHgMissingHeightRepair));

        let early = maybe_build_area_loaded_client_packet(
            &mut pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            30,
            29,
        )
        .expect("early fallback check");
        assert!(early.is_none());
        assert!(pending.is_some());
        assert!(in_flight.is_none());
        assert!(hold_gate.is_some());

        let acked_but_waiting_for_native = maybe_build_area_loaded_client_packet(
            &mut pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            31,
            30,
        )
        .expect("acked fallback release check");
        assert!(acked_but_waiting_for_native.is_none());
        assert!(pending
            .as_ref()
            .and_then(|pending| pending.client_ack_observed_at)
            .is_some());
        assert!(in_flight.is_none());
        assert!(hold_gate.is_some());

        pending.as_mut().expect("pending fallback").release_at =
            Instant::now() - Duration::from_millis(1);
        let released = maybe_build_area_loaded_client_packet(
            &mut pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            31,
            30,
        )
        .expect("native grace elapsed fallback release")
        .expect("synthetic Area_AreaLoaded packet");
        let view = MFrameView::parse(&released).expect("synthetic M parse");
        assert_eq!(view.sequence, 75);
        assert_eq!(view.ack_sequence, 30);
        assert_eq!(latest_native_client_sequence, Some(73));
        assert!(pending.is_none());
        assert!(in_flight.is_some());
        assert!(hold_gate.is_none());
    }

    #[test]
    fn named_area_rewrite_arms_area_loaded_fallback_without_env_switch() {
        let mut pending_packets = Vec::new();
        let mut pending_area_loaded = None;
        let mut server_sequence_shifts = Vec::new();

        queue_loadbar_and_area_loaded_fallback(
            &mut pending_packets,
            &mut pending_area_loaded,
            &mut server_sequence_shifts,
            22,
            74,
            Some(AreaLoadedFallbackReason::ExactEePostStaticListZeroWords),
            false,
        )
        .expect("queue area side effects");

        let pending = pending_area_loaded.expect("named area rewrite should arm fallback");
        assert_eq!(pending.server_ack_sequence, 22);
        assert_eq!(pending.release_client_ack_sequence, 22);
        assert!(pending.require_client_ack_before_release);
        assert!(pending_packets.is_empty());
    }

    #[test]
    fn consecutive_synthetic_client_packets_do_not_skip_peer_sequence() {
        let mut latest_native_client_sequence = Some(73);
        let mut client_sequence_shifts = vec![SequenceShift { base: 73, delta: 1 }];
        let mut in_flight = None;
        let mut hold_gate = None;

        let mut first_pending =
            Some(due_area_loaded(AreaLoadedFallbackReason::ExactEePostStaticListZeroWords));
        let first = maybe_build_area_loaded_client_packet(
            &mut first_pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            31,
            30,
        )
        .expect("first synthetic build")
        .expect("first synthetic packet");
        let first_view = MFrameView::parse(&first).expect("first M parse");
        assert_eq!(first_view.sequence, 75);
        assert_eq!(first_view.ack_sequence, 30);
        assert_eq!(latest_native_client_sequence, Some(73));

        in_flight = None;
        let mut second_pending =
            Some(due_area_loaded(AreaLoadedFallbackReason::LegacyHgMissingHeightRepair));
        let second = maybe_build_area_loaded_client_packet(
            &mut second_pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            40,
            39,
        )
        .expect("second synthetic build")
        .expect("second synthetic packet");
        let second_view = MFrameView::parse(&second).expect("second M parse");
        assert_eq!(second_view.sequence, 76);
        assert_eq!(second_view.ack_sequence, 39);
        assert_eq!(latest_native_client_sequence, Some(73));
        assert_eq!(shift_sequence_for_peer(&client_sequence_shifts, 74), 77);
    }

    #[test]
    fn acknowledged_synthetic_area_loaded_consumes_matching_late_native_once() {
        let mut in_flight = Some(InFlightAreaLoaded {
            packet: vec![0x4D],
            original_sequence: 74,
            shifted_sequence: 75,
            ack_sequence: 26,
            next_retransmit_at: Instant::now() + Duration::from_secs(30),
            retransmits: 0,
            reason: AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
        });
        let mut completed = None;
        let mut pending_packets = Vec::new();

        maybe_queue_area_loaded_retransmit(
            &mut in_flight,
            &mut completed,
            &mut pending_packets,
            75,
        );

        assert!(in_flight.is_none());
        assert!(pending_packets.is_empty());
        assert!(completed.is_some());
        assert!(consume_late_native_area_loaded_after_completed_synthetic(
            &mut completed,
            74,
            38
        ));
        assert!(completed.is_none());
        assert!(!consume_late_native_area_loaded_after_completed_synthetic(
            &mut completed,
            74,
            38
        ));
    }

    #[test]
    fn completed_synthetic_area_loaded_does_not_consume_different_native_sequence() {
        let mut completed = Some(CompletedAreaLoaded {
            original_sequence: 74,
            shifted_sequence: 75,
            ack_sequence: 26,
            acknowledged_at: Instant::now(),
            reason: AreaLoadedFallbackReason::ExactEePostStaticListZeroWords,
        });

        assert!(!consume_late_native_area_loaded_after_completed_synthetic(
            &mut completed,
            80,
            38
        ));
        assert!(completed.is_none());
    }

    #[test]
    fn server_hold_gate_opens_when_ee_acks_rewritten_area_window() {
        let mut hold_gate = Some(ServerHoldGate {
            release_client_ack_sequence: 31,
            reason: AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
            armed_at: Instant::now(),
            area_ack_observed_at: None,
            release_at: None,
        });

        observe_server_hold_gate_client_ack(&mut hold_gate, 30);
        assert!(hold_gate.is_some());

        observe_server_hold_gate_client_ack(&mut hold_gate, 31);
        assert!(hold_gate.is_some());
        let active = hold_gate.as_mut().expect("hold gate should wait for native grace");
        assert!(active.area_ack_observed_at.is_some());
        active.release_at = Some(Instant::now() - Duration::from_millis(1));
        observe_server_hold_gate_client_ack(&mut hold_gate, 31);
        assert!(hold_gate.is_none());
    }

    #[test]
    fn pending_verified_server_queue_collapses_reliable_replays_by_sequence() {
        let mut pending = Vec::new();
        let proof = VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject);
        let first = build_synthetic_gameplay_frame(35, 0, &[0x50, 0x05, 0x01])
            .expect("first synthetic reliable frame");
        let replay = build_synthetic_gameplay_frame(35, 0, &[0x50, 0x05, 0x01])
            .expect("replay synthetic reliable frame");
        let next = build_synthetic_gameplay_frame(36, 0, &[0x50, 0x05, 0x01])
            .expect("next synthetic reliable frame");

        assert_eq!(
            queue_pending_verified_server_packet(
                &mut pending,
                proof.clone(),
                first,
                "test reliable hold"
            ),
            PendingVerifiedServerPacketQueueResult::Queued { held_packets: 1 }
        );
        assert_eq!(
            queue_pending_verified_server_packet(
                &mut pending,
                proof.clone(),
                replay,
                "test reliable hold"
            ),
            PendingVerifiedServerPacketQueueResult::CollapsedReliableReplay {
                sequence: 35,
                held_packets: 1
            }
        );
        assert_eq!(
            queue_pending_verified_server_packet(&mut pending, proof, next, "test reliable hold"),
            PendingVerifiedServerPacketQueueResult::Queued { held_packets: 2 }
        );
        assert_eq!(pending.len(), 2);
    }
}
