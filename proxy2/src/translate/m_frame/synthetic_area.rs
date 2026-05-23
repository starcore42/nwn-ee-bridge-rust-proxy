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
// Synthetic LoadBar is a stateful EE-facing UI compatibility event, not game
// truth. EE decompiles show LoadBar as the server-owned stall-event UI family:
// module load sends LoadBar_Start, then module-load completion sends
// LoadBar_End followed by ServerStatus_Status. A legacy 1.69 server that is
// already running a module may enter the first area without emitting that EE
// module-load UI pair. Local Diamond bridge capture
// `local-diamond-bridge-20260517-161322` confirmed that the verified
// proxy-owned load-completion sequence lets EE leave `Loading Area...
// Sunshine_Vill` after the rewritten Area_ClientArea.
// LoadBar still must not become the authoritative area stream gate. Driver-only
// evidence on 2026-05-12 showed that ACKing LoadBar_End alone is not a safe
// release condition: EE can ACK a transport frame before the area object
// registry is ready. The EE client decompile routes native `Area_AreaLoaded`
// through the client gameplay sender only after the area-load reader returns, so
// native completion remains the strongest proof. The proxy's fallback therefore
// waits for both sides of the narrower transport proof before proxy-owning the
// client event: EE must ACK the rewritten Area_ClientArea window, the audited
// synthetic LoadBar completion timeout must pass, and then a native-completion
// grace expires. LoadBar_End and ServerStatus_Status are UI compatibility
// frames; they are not game-truth, and local Diamond driver-only capture
// `local-diamond-bridge-20260518-043859` showed EE can remain at the area/load
// ACK while never ACKing those synthetic completion frames. This keeps the
// fallback late enough to avoid racing the area reader without making UI cleanup
// packets the authoritative release gate. If the native packet never appears,
// the proxy has no decompile-backed signal equivalent to EE's in-process
// `CNWCArea::LoadArea` success return, so the fallback remains deliberately
// conservative. Earlier driver-only captures proved that releasing synthetic
// `Area_AreaLoaded` immediately after `LoadBar_End` ACK can leave the EE client
// in a permanent black/fade state. A live HG Starcore5 Docks driver-only run on
// 2026-05-18 showed the previous 14.5s completion stall plus 5s native grace held
// exact-validated live-object/quickbar traffic for about 19.5s after the area
// ACK. The decompile-owned invariant is ordering, not that long wall-clock wait:
// the synthetic End/Status pair must follow the rewritten Area_ClientArea window
// and have enough time for EE's screen-fade panel to exist before the proxy-owned
// client fallback is emitted. Keep that as a named panel-arm delay, then give
// native completion one short grace window. Server-authored gameplay packets are
// held behind the same
// native-or-fallback Area_AreaLoaded proof; only the rewritten Area_ClientArea
// window and proxy-owned area-load UI/control packets are allowed through while
// the client is still loading the area.
const AREA_LOADED_FALLBACK_AFTER_LOADBAR_ACK_GRACE: Duration = Duration::from_millis(5_000);
// When all audited fallback preconditions are satisfied, the proxy is allowed
// to proxy-own `Area_AreaLoaded`. Local Diamond driver-only capture
// `local-diamond-bridge-20260518-174721` showed a narrower stateful path: the
// EE client produced the native `Area_AreaLoaded` roughly 430ms after the proxy
// opened the held post-area gameplay stream. That means the post-area packets
// can be the final input needed for EE's native area-load sender to run.
//
// Keep synthetic ownership as the last resort. At the moment the old fallback
// would have emitted a client packet, open the exact-validated hold gate once,
// then give the EE client this short native-probe window. If native completion
// appears, it cancels the pending fallback through the normal native-wins path.
// If it does not, the proxy emits the same audited fallback packet a moment
// later. This does not relax validation or release gameplay earlier than the
// previous fallback time; it only lets the real client acknowledgement beat the
// synthetic packet after the stream is already safe to release.
const AREA_LOADED_NATIVE_PROBE_AFTER_GATE_OPEN_GRACE: Duration = Duration::from_millis(1_000);
// When synthetic LoadBar is disabled for driver-only isolation, the proxy has a
// narrower state proof: an exact Area_ClientArea rewrite was emitted and the EE
// client ACKed the final rewritten area frame. That ACK is still transport
// proof, not in-process load completion. EE/Diamond decompiles route native
// `Area_AreaLoaded` through the client gameplay sender, so that native packet is
// the stronger semantic proof that the area reader returned and gameplay packets
// can resume. Driver-only captures also showed that holding post-area packets
// forever can deadlock native completion. Keep the gate stateful: an area ACK
// records the ACK for diagnostics, native Area_AreaLoaded opens the gate
// immediately, and the audited proxy-owned fallback opens it if native
// completion does not arrive. A 2026-05-18 Starcore5 Docks driver-only run
// showed that releasing live-object/quickbar packets on a short ACK grace before
// LoadBar_End/Area_AreaLoaded can leave the EE world view black even though the
// UI is alive.
const AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE: Duration = Duration::from_millis(5_000);
const SERVER_HOLD_GATE_ACK_DIAGNOSTIC_GRACE: Duration = Duration::from_millis(5_000);
const AREA_LOADED_RETRANSMIT_DELAY: Duration = Duration::from_millis(1250);
const LOADBAR_FRAME_COUNT: u16 = 2;
const LOADBAR_WITH_STATUS_FRAME_COUNT: u16 = LOADBAR_FRAME_COUNT + 1;
const LOADBAR_COMPLETION_FALLBACK_DELAY: Duration = Duration::from_millis(2_500);
// EE `CServerExoAppInternal::LoadModule` calls
// `SendServerToPlayerLoadBar_StartStallEvent(1)` for the normal module-load
// progress path and later sends `SendServerToPlayerLoadBar_EndStallEvent(1, 0)`
// immediately before `ServerStatus_Status`. Area entry through the bridge is a
// different UI state: the EE client is already sitting behind the load/screen
// fade panel. EE `CNWCMessage::HandleServerToPlayerLoadBar` (`sub_1407A2D40`)
// has an explicit stall id 2 branch that marks the active `CPanelScreenFade`
// for cleanup on `LoadBar_End`, while stall id 1 only records module-load
// status in the active app state and can leave the `LOAD_SCR` modal covering the
// rendered area. The old in-process bridge used this same default area stall id
// when no real server LoadBar was outstanding.
const LOADBAR_AREA_SCREEN_FADE_STALL_EVENT_ID: u32 = 2;
const AREA_LOADBAR_END_REASON: &str = "Area_ClientArea synthetic LoadBar_End";
const AREA_LOADBAR_STATUS_REASON: &str =
    "Area_ClientArea synthetic ServerStatus_Status after LoadBar_End";

#[derive(Debug, Clone)]
pub(super) struct PendingAreaLoaded {
    pub(super) server_ack_sequence: u16,
    pub(super) release_client_ack_sequence: u16,
    pub(super) release_at: Instant,
    pub(super) require_client_ack_before_release: bool,
    pub(super) native_completion_grace_after_ack: Duration,
    pub(super) client_ack_observed_at: Option<Instant>,
    pub(super) gate_opened_for_native_probe_at: Option<Instant>,
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
    Queued { held_packets: usize },
    CollapsedReliableReplay { sequence: u16, held_packets: usize },
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
    pub(super) area_first_sequence: u16,
    pub(super) release_client_ack_sequence: u16,
    pub(super) reason: AreaLoadedFallbackReason,
    pub(super) armed_at: Instant,
    pub(super) area_window_released_at: Option<Instant>,
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
        .contains(&area::AreaRewriteKind::ExactEeAreaNameModeCExoStringBit)
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

pub(super) fn release_pending_loadbar_completion_after_native_area_loaded(
    pending_packets: &mut [PendingServerPacket],
) {
    let now = Instant::now();
    for pending in pending_packets {
        if matches!(
            pending.reason,
            AREA_LOADBAR_END_REASON | AREA_LOADBAR_STATUS_REASON
        ) && pending.due_at > now
        {
            pending.due_at = now;
            tracing::info!(
                reason = pending.reason,
                "pending synthetic loadbar completion released by native Area_AreaLoaded"
            );
        }
    }
}

pub(super) fn clear_in_flight_area_loaded(in_flight: &mut Option<InFlightAreaLoaded>) {
    *in_flight = None;
}

pub(super) fn clear_server_hold_gate(gate: &mut Option<ServerHoldGate>, trigger: &'static str) {
    let Some(active) = gate.take() else {
        return;
    };
    tracing::info!(
        area_first_sequence = active.area_first_sequence,
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
    area_first_sequence: u16,
    release_client_ack_sequence: u16,
    reason: Option<AreaLoadedFallbackReason>,
) {
    let Some(reason) = reason else {
        return;
    };
    *gate = Some(ServerHoldGate {
        area_first_sequence,
        release_client_ack_sequence,
        reason,
        armed_at: Instant::now(),
        area_window_released_at: None,
        area_ack_observed_at: None,
        release_at: None,
    });
    tracing::info!(
        area_first_sequence,
        release_client_ack_sequence,
        reason = reason.as_str(),
        diagnostic_grace_ms = SERVER_HOLD_GATE_ACK_DIAGNOSTIC_GRACE.as_millis(),
        "server-to-client post-area hold gate armed until native or proxy-owned Area_AreaLoaded proof"
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
    let ack_satisfied =
        sequence_at_or_after(observed_client_ack, active.release_client_ack_sequence);
    if !ack_satisfied {
        return;
    }
    if active.area_ack_observed_at.is_none() {
        active.area_ack_observed_at = Some(now);
        active.release_at = Some(now + SERVER_HOLD_GATE_ACK_DIAGNOSTIC_GRACE);
        tracing::info!(
            observed_client_ack,
            release_client_ack_sequence = active.release_client_ack_sequence,
            diagnostic_grace_ms = SERVER_HOLD_GATE_ACK_DIAGNOSTIC_GRACE.as_millis(),
            held_ms = now.saturating_duration_since(active.armed_at).as_millis(),
            reason = active.reason.as_str(),
            "area-load ACK observed; holding post-area gameplay packets until native/proxy-owned Area_AreaLoaded proof"
        );
        return;
    }
    let Some(release_at) = active.release_at else {
        return;
    };
    if now < release_at {
        return;
    }
    active.release_at = None;
    tracing::info!(
        observed_client_ack,
        release_client_ack_sequence = active.release_client_ack_sequence,
        held_ms = now.saturating_duration_since(active.armed_at).as_millis(),
        reason = active.reason.as_str(),
        "area-load ACK diagnostic grace elapsed; post-area gameplay remains held until Area_AreaLoaded proof"
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
    original_first_sequence: u16,
    original_last_sequence: u16,
    ack_sequence: u16,
    area_loaded_fallback_reason: Option<AreaLoadedFallbackReason>,
    synthesize_loadbar: bool,
) -> anyhow::Result<()> {
    let shifted_first_sequence =
        shift_sequence_for_peer(server_sequence_shifts, original_first_sequence);
    let shifted_last_sequence =
        shift_sequence_for_peer(server_sequence_shifts, original_last_sequence);
    let now = Instant::now();
    let (
        release_client_ack_sequence,
        release_at,
        start_sequence,
        end_sequence,
        status_sequence,
        shift_base,
        shift_delta,
        future_shift_base,
        future_shift_delta,
        end_delay_ms,
    ) = if synthesize_loadbar {
        // EE's area-screen-fade stall id 2 path only latches if the active
        // `CPanelScreenFade` already exists. The EE client creates that panel
        // while handling the area transition, so the synthetic Start/End pair
        // belongs after the complete Area_ClientArea reliable window, not
        // before it. This mirrors the old in-process bridge/proxy evidence:
        // queue the LoadBar_Start at `shifted_after + 1` and LoadBar_End at
        // `shifted_after + 2`.
        //
        // original first..last -> Area_ClientArea, contiguous and not split
        // original last+1      -> synthetic LoadBar_Start
        // original last+2      -> synthetic LoadBar_End
        // original last+3      -> synthetic ServerStatus_Status
        //
        // This is deliberately window-aware. It keeps synthetic UI packets out
        // of the deflated Area_ClientArea stream while still giving EE's id-2
        // screen-fade branch a live panel to complete.
        let start_sequence = shifted_last_sequence.wrapping_add(1);
        let end_sequence = shifted_last_sequence.wrapping_add(2);
        let status_sequence = shifted_last_sequence.wrapping_add(3);

        let start_payload = loadbar::start_payload(LOADBAR_AREA_SCREEN_FADE_STALL_EVENT_ID);
        let end_payload = loadbar::end_success_payload(LOADBAR_AREA_SCREEN_FADE_STALL_EVENT_ID);
        // EE `CServerExoAppInternal::MainLoop` sets server mode to 1 after
        // `CNWSModule::LoadModuleFinish`, then sends `LoadBar_End` immediately
        // followed by `CNWSMessage::SendServerToPlayerServerStatus_Status`.
        // `SendServerToPlayerServerStatus_Status` maps mode 1 to high-level
        // `0x01/0x01` with no CNW read buffer. This is a typed protocol-status
        // transition, not the later mode-2 `0x01/0x03` module-resource packet.
        let status_payload = server_status_status_payload();
        let start_packet =
            build_synthetic_gameplay_frame(start_sequence, ack_sequence, &start_payload)?;
        let end_packet = build_synthetic_gameplay_frame(end_sequence, ack_sequence, &end_payload)?;
        let status_packet =
            build_synthetic_gameplay_frame(status_sequence, ack_sequence, &status_payload)?;

        let end_due_at = now + LOADBAR_COMPLETION_FALLBACK_DELAY;
        let area_loaded_due_at = end_due_at + AREA_LOADED_FALLBACK_AFTER_LOADBAR_ACK_GRACE;
        server_sequence_shifts.push(SequenceShift {
            base: original_last_sequence.wrapping_add(1),
            delta: LOADBAR_WITH_STATUS_FRAME_COUNT,
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
            reason: AREA_LOADBAR_END_REASON,
            placement: PendingServerPacketPlacement::AfterCurrentEmit,
        });
        pending_packets.push(PendingServerPacket {
            family: VerifiedFamily::ServerStatusStatus,
            packet: status_packet,
            due_at: end_due_at,
            reason: AREA_LOADBAR_STATUS_REASON,
            placement: PendingServerPacketPlacement::AfterCurrentEmit,
        });
        (
            shifted_last_sequence,
            area_loaded_due_at,
            Some(start_sequence),
            Some(end_sequence),
            Some(status_sequence),
            Some(original_last_sequence.wrapping_add(1)),
            LOADBAR_WITH_STATUS_FRAME_COUNT,
            Some(original_last_sequence.wrapping_add(1)),
            LOADBAR_WITH_STATUS_FRAME_COUNT,
            Some(LOADBAR_COMPLETION_FALLBACK_DELAY.as_millis()),
        )
    } else {
        (
            shifted_last_sequence,
            now + AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE,
            None,
            None,
            None,
            None,
            0,
            None,
            0,
            None,
        )
    };
    if let Some(reason) = area_loaded_fallback_reason {
        arm_area_loaded_fallback(
            pending_area_loaded,
            original_last_sequence,
            release_client_ack_sequence,
            release_at,
            true,
            if synthesize_loadbar {
                AREA_LOADED_FALLBACK_AFTER_LOADBAR_ACK_GRACE
            } else {
                AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE
            },
            reason,
        );
    } else {
        *pending_area_loaded = None;
        tracing::info!(
            original_first_sequence,
            original_last_sequence,
            release_client_ack_sequence,
            "client synthetic Area_AreaLoaded fallback not armed: no named compatibility reason"
        );
    }

    tracing::info!(
        original_first_sequence,
        original_last_sequence,
        shifted_first_sequence,
        shifted_last_sequence,
        start_sequence,
        end_sequence,
        status_sequence,
        ack_sequence,
        shift_base,
        shift_delta,
        future_shift_base,
        future_shift_delta,
        end_delay_ms,
        fallback_after_loadbar_ack_grace_ms =
            AREA_LOADED_FALLBACK_AFTER_LOADBAR_ACK_GRACE.as_millis(),
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

fn server_status_status_payload() -> [u8; 3] {
    [b'P', 0x01, 0x01]
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
        let ack_grace_release_at = now + active.native_completion_grace_after_ack;
        if ack_grace_release_at > active.release_at {
            active.release_at = ack_grace_release_at;
        }
        tracing::info!(
            observed_client_ack,
            release_client_ack_sequence = active.release_client_ack_sequence,
            native_grace_ms = active.native_completion_grace_after_ack.as_millis(),
            release_delay_ms = active.release_at.saturating_duration_since(now).as_millis(),
            reason = active.reason.as_str(),
            "area-load release ACK observed; synthetic Area_AreaLoaded fallback is waiting for native EE completion or audited release time"
        );
        return Ok(None);
    }
    let release_due = now >= active.release_at;
    if !release_due {
        return Ok(None);
    }

    if active.require_client_ack_before_release
        && client_ack_satisfied
        && active.client_ack_observed_at.is_some()
        && active.gate_opened_for_native_probe_at.is_none()
        && server_hold_gate.is_some()
    {
        active.gate_opened_for_native_probe_at = Some(now);
        active.release_at = now + AREA_LOADED_NATIVE_PROBE_AFTER_GATE_OPEN_GRACE;
        let reason = active.reason;
        clear_server_hold_gate(
            server_hold_gate,
            "synthetic Area_AreaLoaded fallback due; opening gate for native completion probe",
        );
        tracing::info!(
            observed_client_ack,
            release_client_ack_sequence = active.release_client_ack_sequence,
            native_probe_ms = AREA_LOADED_NATIVE_PROBE_AFTER_GATE_OPEN_GRACE.as_millis(),
            reason = reason.as_str(),
            "synthetic Area_AreaLoaded fallback deferred after opening held post-area stream so native EE completion can win"
        );
        return Ok(None);
    }

    let pending = active.clone();

    let release_trigger = if client_ack_satisfied {
        if pending.gate_opened_for_native_probe_at.is_some() {
            "native completion probe elapsed after opening held post-area stream"
        } else {
            "client ACKed area-load release sequence"
        }
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
    native_completion_grace_after_ack: Duration,
    reason: AreaLoadedFallbackReason,
) {
    *pending = Some(PendingAreaLoaded {
        server_ack_sequence,
        release_client_ack_sequence,
        release_at,
        require_client_ack_before_release,
        native_completion_grace_after_ack,
        client_ack_observed_at: None,
        gate_opened_for_native_probe_at: None,
        reason,
    });
    tracing::info!(
        server_ack_sequence,
        release_client_ack_sequence,
        require_client_ack_before_release,
        native_completion_grace_after_ack_ms = native_completion_grace_after_ack.as_millis(),
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
            native_completion_grace_after_ack: AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE,
            client_ack_observed_at: None,
            gate_opened_for_native_probe_at: None,
            reason,
        }
    }

    fn ack_gated_area_loaded(reason: AreaLoadedFallbackReason) -> PendingAreaLoaded {
        PendingAreaLoaded {
            server_ack_sequence: 30,
            release_client_ack_sequence: 31,
            release_at: Instant::now(),
            require_client_ack_before_release: true,
            native_completion_grace_after_ack: AREA_LOADED_FALLBACK_AFTER_AREA_ACK_GRACE,
            client_ack_observed_at: None,
            gate_opened_for_native_probe_at: None,
            reason,
        }
    }

    #[test]
    fn area_loaded_fallback_waits_for_native_grace_after_area_ack_without_loadbar() {
        let mut latest_native_client_sequence = Some(73);
        let mut client_sequence_shifts = vec![SequenceShift { base: 73, delta: 1 }];
        let mut in_flight = None;
        let mut hold_gate = Some(ServerHoldGate {
            area_first_sequence: 30,
            release_client_ack_sequence: 31,
            reason: AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
            armed_at: Instant::now(),
            area_window_released_at: None,
            area_ack_observed_at: None,
            release_at: None,
        });
        let mut pending = Some(ack_gated_area_loaded(
            AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
        ));

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
        assert!(
            pending
                .as_ref()
                .and_then(|pending| pending.client_ack_observed_at)
                .is_some()
        );
        assert!(in_flight.is_none());
        assert!(hold_gate.is_some());

        pending.as_mut().expect("pending fallback").release_at =
            Instant::now() - Duration::from_millis(1);
        let gate_opened_for_native_probe = maybe_build_area_loaded_client_packet(
            &mut pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            31,
            30,
        )
        .expect("native grace elapsed fallback release");
        assert!(gate_opened_for_native_probe.is_none());
        assert!(pending.is_some());
        assert!(
            pending
                .as_ref()
                .and_then(|pending| pending.gate_opened_for_native_probe_at)
                .is_some()
        );
        assert!(in_flight.is_none());
        assert!(hold_gate.is_none());

        pending.as_mut().expect("pending native probe").release_at =
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
        .expect("post-gate native probe elapsed fallback release")
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
    fn synthetic_loadbar_uses_area_screen_fade_stall_id_and_status_status() {
        let mut pending_packets = Vec::new();
        let mut pending_area_loaded = None;
        let mut server_sequence_shifts = Vec::new();
        let queued_at = Instant::now();

        queue_loadbar_and_area_loaded_fallback(
            &mut pending_packets,
            &mut pending_area_loaded,
            &mut server_sequence_shifts,
            22,
            22,
            74,
            Some(AreaLoadedFallbackReason::ExactEePostStaticListZeroWords),
            true,
        )
        .expect("queue synthetic loadbar side effects");

        assert_eq!(pending_packets.len(), 3);
        assert_eq!(pending_packets[0].family, VerifiedFamily::LoadBar);
        assert_eq!(pending_packets[1].family, VerifiedFamily::LoadBar);
        assert_eq!(
            pending_packets[2].family,
            VerifiedFamily::ServerStatusStatus
        );
        assert_eq!(server_sequence_shifts.len(), 1);
        assert_eq!(server_sequence_shifts[0].base, 23);
        assert_eq!(
            server_sequence_shifts[0].delta,
            LOADBAR_WITH_STATUS_FRAME_COUNT
        );
        assert_eq!(
            pending_packets[0].placement,
            PendingServerPacketPlacement::AfterCurrentEmit
        );
        assert_eq!(
            pending_packets[1].placement,
            PendingServerPacketPlacement::AfterCurrentEmit
        );
        assert_eq!(
            pending_packets[2].placement,
            PendingServerPacketPlacement::AfterCurrentEmit
        );

        let start = MFrameView::parse(&pending_packets[0].packet).expect("start frame");
        let end = MFrameView::parse(&pending_packets[1].packet).expect("end frame");
        let status = MFrameView::parse(&pending_packets[2].packet).expect("status frame");
        assert!(start.crc_valid);
        assert!(end.crc_valid);
        assert!(status.crc_valid);
        assert_eq!(start.sequence, 23);
        assert_eq!(end.sequence, 24);
        assert_eq!(status.sequence, 25);
        assert_eq!(status.ack_sequence, 74);
        assert_eq!(
            u32::from_le_bytes(
                pending_packets[0].packet[19..23]
                    .try_into()
                    .expect("start stall id bytes")
            ),
            LOADBAR_AREA_SCREEN_FADE_STALL_EVENT_ID
        );
        assert_eq!(
            u32::from_le_bytes(
                pending_packets[1].packet[19..23]
                    .try_into()
                    .expect("end stall id bytes")
            ),
            LOADBAR_AREA_SCREEN_FADE_STALL_EVENT_ID
        );
        assert_eq!(
            status.high.map(|high| (high.major, high.minor)),
            Some((0x01, 0x01))
        );

        let pending = pending_area_loaded.expect("fallback should be armed");
        assert_eq!(pending.release_client_ack_sequence, 22);
        assert!(pending.require_client_ack_before_release);
        assert!(
            pending.release_at
                >= queued_at
                    + LOADBAR_COMPLETION_FALLBACK_DELAY
                    + AREA_LOADED_FALLBACK_AFTER_LOADBAR_ACK_GRACE
        );
        assert!(
            pending.release_at > pending_packets[1].due_at,
            "synthetic Area_AreaLoaded must remain a late fallback, not race LoadBar_End"
        );
    }

    #[test]
    fn synthetic_loadbar_does_not_split_multiframe_area_window() {
        let mut pending_packets = Vec::new();
        let mut pending_area_loaded = None;
        let mut server_sequence_shifts = vec![SequenceShift { base: 16, delta: 1 }];

        queue_loadbar_and_area_loaded_fallback(
            &mut pending_packets,
            &mut pending_area_loaded,
            &mut server_sequence_shifts,
            22,
            26,
            74,
            Some(AreaLoadedFallbackReason::ExactEePostStaticListZeroWords),
            true,
        )
        .expect("queue synthetic loadbar side effects");

        let start = MFrameView::parse(&pending_packets[0].packet).expect("start frame");
        let end = MFrameView::parse(&pending_packets[1].packet).expect("end frame");
        let status = MFrameView::parse(&pending_packets[2].packet).expect("status frame");
        let shifted_area_window = (22..=26)
            .map(|sequence| shift_sequence_for_peer(&server_sequence_shifts, sequence))
            .collect::<Vec<_>>();

        assert_eq!(start.sequence, 28);
        assert_eq!(shifted_area_window, vec![23, 24, 25, 26, 27]);
        assert_eq!(end.sequence, 29);
        assert_eq!(status.sequence, 30);
        assert_eq!(
            pending_area_loaded
                .as_ref()
                .map(|pending| pending.release_client_ack_sequence),
            Some(27)
        );
    }

    #[test]
    fn synthetic_loadbar_area_loaded_fallback_does_not_require_loadbar_completion_ack() {
        let mut pending_packets = Vec::new();
        let mut pending_area_loaded = None;
        let mut server_sequence_shifts = Vec::new();

        queue_loadbar_and_area_loaded_fallback(
            &mut pending_packets,
            &mut pending_area_loaded,
            &mut server_sequence_shifts,
            22,
            22,
            74,
            Some(AreaLoadedFallbackReason::ExactEePostStaticListZeroWords),
            true,
        )
        .expect("queue synthetic loadbar side effects");

        let mut pending = pending_area_loaded;
        let mut in_flight = None;
        let mut hold_gate = None;
        let mut latest_native_client_sequence = Some(73);
        let mut client_sequence_shifts = Vec::new();

        let acked_but_waiting_for_native = maybe_build_area_loaded_client_packet(
            &mut pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            22,
            74,
        )
        .expect("area ack starts native grace");
        assert!(acked_but_waiting_for_native.is_none());
        assert!(pending.as_ref().unwrap().client_ack_observed_at.is_some());

        pending.as_mut().unwrap().release_at = Instant::now() - Duration::from_millis(1);
        let released = maybe_build_area_loaded_client_packet(
            &mut pending,
            &mut in_flight,
            &mut hold_gate,
            &mut latest_native_client_sequence,
            &mut client_sequence_shifts,
            22,
            74,
        )
        .expect("fallback releases without waiting for LoadBar_End/Status ACK")
        .expect("synthetic Area_AreaLoaded packet");

        let view = MFrameView::parse(&released).expect("synthetic M parse");
        assert_eq!(view.sequence, 74);
        assert_eq!(view.ack_sequence, 74);
        assert!(pending.is_none());
        assert!(in_flight.is_some());
    }

    #[test]
    fn consecutive_synthetic_client_packets_do_not_skip_peer_sequence() {
        let mut latest_native_client_sequence = Some(73);
        let mut client_sequence_shifts = vec![SequenceShift { base: 73, delta: 1 }];
        let mut in_flight = None;
        let mut hold_gate = None;

        let mut first_pending = Some(due_area_loaded(
            AreaLoadedFallbackReason::ExactEePostStaticListZeroWords,
        ));
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
        let mut second_pending = Some(due_area_loaded(
            AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
        ));
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
    fn server_hold_gate_records_ack_but_waits_for_area_loaded_proof() {
        let mut hold_gate = Some(ServerHoldGate {
            area_first_sequence: 30,
            release_client_ack_sequence: 31,
            reason: AreaLoadedFallbackReason::LegacyHgMissingHeightRepair,
            armed_at: Instant::now(),
            area_window_released_at: None,
            area_ack_observed_at: None,
            release_at: None,
        });

        observe_server_hold_gate_client_ack(&mut hold_gate, 30);
        assert!(hold_gate.is_some());

        observe_server_hold_gate_client_ack(&mut hold_gate, 31);
        assert!(hold_gate.is_some());
        let active = hold_gate
            .as_mut()
            .expect("hold gate should wait for native/proxy-owned Area_AreaLoaded");
        assert!(active.area_ack_observed_at.is_some());
        active.release_at = Some(Instant::now() - Duration::from_millis(1));
        observe_server_hold_gate_client_ack(&mut hold_gate, 31);
        assert!(hold_gate.is_some());
        assert!(
            hold_gate
                .as_ref()
                .expect("hold gate remains armed")
                .release_at
                .is_none()
        );
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
