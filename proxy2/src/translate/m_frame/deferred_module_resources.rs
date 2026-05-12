//! Deferred `ServerStatus_ModuleResources` emission.
//!
//! Higher Ground's 1.69 server can send the short `ServerStatus_ModuleRunning`
//! status before the later legacy `Module_Info` packet carries the Diamond
//! HAK/TLK declaration. EE expects the resource block in the server-status
//! family instead. Strict translation therefore cannot pass that early packet
//! through and also cannot invent the resource list yet.
//!
//! This module owns that narrow transport gap:
//!
//! 1. capture only the decompile-backed legacy status shape,
//! 2. wait until `Module_Info` records the exact server-provided HAK/TLK list,
//! 3. rewrite the captured status through `module_resources`, and
//! 4. inject one verified EE server-status resources M frame with sequence
//!    repair.
//!
//! It deliberately does not know how to parse `Module_Info`; that remains in
//! `translate::module`. It also does not own generic synthetic packets; that
//! remains in `synthetic_area`.

use std::time::Instant;

use crate::{
    packet::m::MFrameView,
    translate::{VerifiedFamily, VerifiedProof, module_resources},
};

use super::{
    parse_window,
    sequence::{SequenceShift, sequence_at_or_after, shift_sequence_for_peer, trim_sequence_shifts},
    synthetic_area::{self, PendingServerPacket, PendingServerPacketPlacement},
};

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const SERVER_STATUS_MAJOR: u8 = 0x01;
const MODULE_RUNNING_MINOR: u8 = 0x03;
const MAX_SERVER_STATUS_STRING: usize = 4096;
const MAX_FRAGMENT_TAIL_BYTES: usize = 64;
const MODULE_RESOURCES_INSERTED_FRAME_COUNT: u16 = 1;

#[derive(Debug, Default)]
pub(super) struct DeferredModuleResourcesState {
    pending_status: Option<DeferredStatusPayload>,
    hold_gate: Option<ModuleResourceHoldGate>,
    held_server_to_client_packets: Vec<PendingModuleResourceServerPacket>,
}

/// Gate server traffic that follows the proxy-owned module-resource status.
///
/// The legacy server remains authoritative: this gate does not synthesize game
/// truth or reorder semantic events. It only holds later server frames until EE
/// ACKs the exact synthetic `ServerStatus_ModuleResources` frame that the proxy
/// emitted from decompile-backed legacy module resource data.
#[derive(Debug, Clone)]
pub(super) struct ModuleResourceHoldGate {
    pub(super) release_client_ack_sequence: u16,
    pub(super) armed_at: Instant,
}

#[derive(Debug, Clone)]
pub(super) struct PendingModuleResourceServerPacket {
    pub(super) proof: VerifiedProof,
    pub(super) packet: Vec<u8>,
    pub(super) reason: &'static str,
}

#[derive(Debug, Clone)]
struct DeferredStatusPayload {
    payload: Vec<u8>,
    sequence: u16,
    ack_sequence: u16,
    declared: usize,
    status_string_len: usize,
    fragment_tail_len: usize,
}

pub(super) fn capture_early_server_status_if_needed(
    bytes: &[u8],
    view: &MFrameView,
    runtime: &module_resources::ModuleResourceRuntime,
    state: &mut DeferredModuleResourcesState,
) {
    let Some(high) = view.high else {
        return;
    };
    if high.major != SERVER_STATUS_MAJOR || high.minor != MODULE_RUNNING_MINOR {
        return;
    }

    let Some(payload) = parse_window::primary_payload(bytes, view) else {
        return;
    };

    let _ = capture_early_status_payload_if_needed(
        payload,
        view.sequence,
        view.ack_sequence,
        runtime,
        state,
    );
}

pub(super) fn capture_early_status_payload_if_needed(
    payload: &[u8],
    sequence: u16,
    ack_sequence: u16,
    runtime: &module_resources::ModuleResourceRuntime,
    state: &mut DeferredModuleResourcesState,
) -> Option<LegacyStatusShape> {
    // If the module-resource runtime can already rewrite this packet, the
    // normal semantic translator should own it immediately. Deferral is only
    // for the strict startup gap before legacy Module_Info has supplied the
    // Diamond HAK/TLK declaration.
    let mut immediate_probe = payload.to_vec();
    if module_resources::rewrite_server_status_module_resources_payload(
        &mut immediate_probe,
        runtime,
    )
    .is_some()
    {
        return None;
    }

    let Some(shape) = LegacyStatusShape::parse(payload) else {
        // This helper is a narrow startup-gap owner, not the generic
        // ServerStatus dispatcher. Non-resource ModuleRunning payloads are
        // expected later in normal play and are claimed by their focused
        // translators/validators; failing this deferred-resource probe is not a
        // protocol warning by itself.
        tracing::debug!(
            sequence,
            ack_sequence,
            payload_len = payload.len(),
            "early ServerStatus_ModuleRunning was not deferred: payload is not the legacy short status shape"
        );
        return None;
    };

    if state.pending_status.is_some() {
        tracing::debug!(
            sequence,
            ack_sequence,
            declared = shape.declared,
            "early ServerStatus_ModuleRunning deferral already has a pending status payload"
        );
        return Some(shape);
    }

    state.pending_status = Some(DeferredStatusPayload {
        payload: payload.to_vec(),
        sequence,
        ack_sequence,
        declared: shape.declared,
        status_string_len: shape.status_string_len,
        fragment_tail_len: shape.fragment_tail_len,
    });
    tracing::info!(
        sequence,
        ack_sequence,
        declared = shape.declared,
        status_string_len = shape.status_string_len,
        fragment_tail_len = shape.fragment_tail_len,
        "early ServerStatus_ModuleRunning captured for deferred EE module-resource rewrite"
    );
    Some(shape)
}

pub(super) fn queue_after_module_info_if_ready(
    state: &mut DeferredModuleResourcesState,
    pending_packets: &mut Vec<PendingServerPacket>,
    server_sequence_shifts: &mut Vec<SequenceShift>,
    original_first_sequence: u16,
    original_after_sequence: u16,
    ack_sequence: u16,
    runtime: &module_resources::ModuleResourceRuntime,
) -> anyhow::Result<()> {
    let Some(pending) = state.pending_status.take() else {
        return Ok(());
    };

    let mut payload = pending.payload.clone();
    let Some(summary) =
        module_resources::rewrite_server_status_module_resources_payload(&mut payload, runtime)
    else {
        state.pending_status = Some(pending);
        tracing::warn!(
            original_after_sequence,
            ack_sequence,
            "deferred ServerStatus_ModuleRunning could not be rewritten after Module_Info; keeping it quarantined pending more evidence"
        );
        return Ok(());
    };

    let synthetic_sequence = shift_sequence_for_peer(server_sequence_shifts, original_first_sequence);
    let packet =
        synthetic_area::build_synthetic_gameplay_frame(synthetic_sequence, ack_sequence, &payload)?;

    server_sequence_shifts.push(SequenceShift {
        base: original_first_sequence,
        delta: MODULE_RESOURCES_INSERTED_FRAME_COUNT,
    });
    trim_sequence_shifts(server_sequence_shifts);
    let shifted_after_sequence =
        shift_sequence_for_peer(server_sequence_shifts, original_after_sequence);

    let now = Instant::now();

    pending_packets.push(PendingServerPacket {
        family: VerifiedFamily::ServerStatusModuleResources,
        packet,
        due_at: now,
        reason: "deferred ServerStatus_ModuleResources before Module_Info",
        placement: PendingServerPacketPlacement::BeforeCurrentEmit,
    });
    state.hold_gate = Some(ModuleResourceHoldGate {
        release_client_ack_sequence: shifted_after_sequence,
        armed_at: now,
    });

    tracing::info!(
        captured_sequence = pending.sequence,
        captured_ack_sequence = pending.ack_sequence,
        captured_declared = pending.declared,
        captured_status_string_len = pending.status_string_len,
        captured_fragment_tail_len = pending.fragment_tail_len,
        original_first_sequence,
        original_after_sequence,
        shifted_after_sequence,
        synthetic_sequence,
        ack_sequence,
        release_client_ack_sequence = shifted_after_sequence,
        shift_base = original_first_sequence,
        shift_delta = MODULE_RESOURCES_INSERTED_FRAME_COUNT,
        old_declared = summary.old_declared,
        new_declared = summary.new_declared,
        hak_count = summary.hak_count,
        custom_tlk = ?summary.custom_tlk,
        profile = %summary.profile_name,
        nwsync_advertised = summary.nwsync_advertised,
        "deferred ServerStatus_ModuleResources queued before Module_Info proved legacy resources"
    );
    tracing::info!(
        release_client_ack_sequence = shifted_after_sequence,
        "server-to-client module-resource hold gate armed until EE ACKs resource-prefixed Module_Info window"
    );

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyStatusShape {
    pub(super) declared: usize,
    pub(super) status_string_len: usize,
    pub(super) fragment_tail_len: usize,
}

impl LegacyStatusShape {
    pub(super) fn parse(payload: &[u8]) -> Option<Self> {
        if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
            || !matches!(payload[0], b'P' | 0x70)
            || payload[1] != SERVER_STATUS_MAJOR
            || payload[2] != MODULE_RUNNING_MINOR
        {
            return None;
        }

        let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)? as usize;
        if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + CNW_LENGTH_BYTES
            || declared > payload.len()
        {
            return None;
        }

        let string_len_offset = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
        let status_string_len = read_u32_le(payload, string_len_offset)? as usize;
        if status_string_len > MAX_SERVER_STATUS_STRING {
            return None;
        }

        let status_string_end = string_len_offset
            .checked_add(CNW_LENGTH_BYTES)?
            .checked_add(status_string_len)?;
        if status_string_end > declared {
            return None;
        }

        let fragment_tail_len = payload.len().saturating_sub(declared);
        if fragment_tail_len > MAX_FRAGMENT_TAIL_BYTES {
            return None;
        }

        Some(Self {
            declared,
            status_string_len,
            fragment_tail_len,
        })
    }
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub(super) fn module_resource_hold_gate_release_sequence(
    state: &DeferredModuleResourcesState,
) -> Option<u16> {
    state
        .hold_gate
        .as_ref()
        .map(|gate| gate.release_client_ack_sequence)
}

pub(super) fn observe_resource_hold_gate_client_ack(
    state: &mut DeferredModuleResourcesState,
    observed_client_ack: u16,
) {
    let Some(gate) = state.hold_gate.as_ref() else {
        return;
    };

    if observed_client_ack == 0
        || !sequence_at_or_after(observed_client_ack, gate.release_client_ack_sequence)
    {
        return;
    }

    tracing::info!(
        observed_client_ack,
        release_client_ack_sequence = gate.release_client_ack_sequence,
        held_packets = state.held_server_to_client_packets.len(),
        armed_elapsed_ms = gate.armed_at.elapsed().as_millis(),
        "server-to-client module-resource hold gate opened by EE ACK"
    );
    state.hold_gate = None;
}

pub(super) fn client_ack_would_release_held_server_packets(
    state: &DeferredModuleResourcesState,
    observed_client_ack: u16,
) -> bool {
    let Some(gate) = state.hold_gate.as_ref() else {
        return false;
    };

    observed_client_ack != 0
        && !state.held_server_to_client_packets.is_empty()
        && sequence_at_or_after(observed_client_ack, gate.release_client_ack_sequence)
}

pub(super) fn hold_server_packet(
    state: &mut DeferredModuleResourcesState,
    proof: VerifiedProof,
    packet: Vec<u8>,
    reason: &'static str,
) {
    let sequence = MFrameView::parse(&packet).map(|view| view.sequence);
    tracing::info!(
        ?sequence,
        proof = proof.as_str(),
        release_client_ack_sequence = state
            .hold_gate
            .as_ref()
            .map(|gate| gate.release_client_ack_sequence),
        len = packet.len(),
        reason,
        held_packets = state.held_server_to_client_packets.len() + 1,
        "server-to-client verified packet held behind module-resource ACK gate"
    );
    state
        .held_server_to_client_packets
        .push(PendingModuleResourceServerPacket {
            proof,
            packet,
            reason,
        });
}

pub(super) fn take_releasable_held_server_packets(
    state: &mut DeferredModuleResourcesState,
) -> Vec<(VerifiedProof, Vec<u8>)> {
    if state.hold_gate.is_some() || state.held_server_to_client_packets.is_empty() {
        return Vec::new();
    }

    state
        .held_server_to_client_packets
        .drain(..)
        .map(|pending| {
            let sequence = MFrameView::parse(&pending.packet).map(|view| view.sequence);
            tracing::info!(
                ?sequence,
                proof = pending.proof.as_str(),
                reason = pending.reason,
                len = pending.packet.len(),
                "server-to-client held packet released after module-resource ACK gate opened"
            );
            (pending.proof, pending.packet)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::packet::m::MFrameView;

    use super::*;

    #[test]
    fn validates_captured_legacy_short_status_shape() {
        let payload = [
            b'P', 0x01, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x79,
        ];
        let shape = LegacyStatusShape::parse(&payload).expect("legacy status should parse");

        assert_eq!(shape.declared, 0x0B);
        assert_eq!(shape.status_string_len, 0);
        assert_eq!(shape.fragment_tail_len, 1);
    }

    #[test]
    fn queues_verified_module_resources_after_module_info() {
        let runtime = module_resources::ModuleResourceRuntime::default();
        assert!(runtime.observe_legacy_module_info_resources(
            &["cep2_custom".to_string(), "cep2_top_v23".to_string()],
            Some("cep23_v1"),
        ));
        let mut state = DeferredModuleResourcesState {
            pending_status: Some(DeferredStatusPayload {
                payload: vec![
                    b'P', 0x01, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x79,
                ],
                sequence: 0,
                ack_sequence: 0,
                declared: 0x0B,
                status_string_len: 0,
                fragment_tail_len: 1,
            }),
            hold_gate: None,
            held_server_to_client_packets: Vec::new(),
        };
        let mut pending_packets = Vec::new();
        let mut shifts = Vec::new();

        queue_after_module_info_if_ready(
            &mut state,
            &mut pending_packets,
            &mut shifts,
            13,
            20,
            7,
            &runtime,
        )
        .expect("deferred resource packet should queue");

        assert!(state.pending_status.is_none());
        assert_eq!(shifts.len(), 1);
        assert_eq!(shifts[0].base, 13);
        assert_eq!(shifts[0].delta, 1);
        assert_eq!(pending_packets.len(), 1);
        assert_eq!(
            pending_packets[0].family,
            VerifiedFamily::ServerStatusModuleResources
        );
        assert_eq!(
            pending_packets[0].placement,
            PendingServerPacketPlacement::BeforeCurrentEmit
        );
        let view = MFrameView::parse(&pending_packets[0].packet)
            .expect("synthetic module resources M frame should parse");
        assert!(view.crc_valid);
        assert_eq!(view.sequence, 13);
        assert_eq!(view.ack_sequence, 7);
        assert_eq!(view.high.map(|high| (high.major, high.minor)), Some((1, 3)));
        assert_eq!(
            state
                .hold_gate
                .as_ref()
                .expect("module-resource hold gate should be armed")
                .release_client_ack_sequence,
            21
        );
    }
}
