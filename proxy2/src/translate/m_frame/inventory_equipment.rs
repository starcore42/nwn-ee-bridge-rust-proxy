//! Proxy-owned inventory/equipment bridge output.
//!
//! The semantic reducer owns the proof that retained direct/materialized item
//! state is ready for inventory/equipment handoff. This module only turns a
//! drained server-inventory handoff update into an exact EE-facing reliable
//! `Inventory_Equip`/`Inventory_EquipCancel` frame.

use std::time::Instant;

use crate::translate::{
    VerifiedFamily, VerifiedProof, client_gui_inventory, inventory,
    semantic::{
        InventoryEquipmentClientGuiInventoryClaim, InventoryEquipmentClientGuiInventoryClaimKind,
        InventoryEquipmentHandoffConsumer::ClientGuiInventory,
    },
};

use super::{
    sequence::{
        SequenceShift, sequence_at_or_after, shift_sequence_for_peer, trim_sequence_shifts,
    },
    state::{
        InventoryEquipmentBridgeClientGuiStatusResponse, InventoryEquipmentBridgeOutputDecision,
        InventoryEquipmentBridgeOutputDecisionKind,
        InventoryEquipmentBridgePendingConfirmedInventoryReplay,
        InventoryEquipmentBridgeQueuedClientGuiStatusOutput, InventoryEquipmentBridgeQueuedOutput,
        SessionState,
    },
    synthetic_area::{self, PendingServerPacket, PendingServerPacketPlacement},
};

const INVENTORY_EQUIPMENT_BRIDGE_REASON: &str =
    "inventory/equipment ready item-state bridge Inventory output";
pub(super) const CONFIRMED_CLIENT_GUI_INVENTORY_REPLAY_REASON: &str =
    "inventory/equipment materialized ClientGui status Inventory replay";
const INVENTORY_EQUIPMENT_BRIDGE_INSERTED_FRAME_COUNT: u16 = 1;

pub(super) fn observe_server_ack_for_client_gui_status(
    state: &mut SessionState,
    server_ack_sequence: u16,
) {
    if server_ack_sequence == 0 {
        return;
    }
    let Some(queued) = state
        .inventory_equipment
        .last_queued_client_gui_status_output
    else {
        return;
    };
    state
        .inventory_equipment
        .last_observed_client_gui_status_server_peer_ack_sequence = Some(server_ack_sequence);
    if state
        .inventory_equipment
        .last_acknowledged_client_gui_status_update_index
        == Some(queued.update_index)
        || !sequence_at_or_after(server_ack_sequence, queued.synthetic_sequence)
    {
        return;
    }

    state
        .inventory_equipment
        .last_acknowledged_client_gui_status_update_index = Some(queued.update_index);
    state
        .inventory_equipment
        .last_acknowledged_client_gui_status_server_ack_sequence = Some(server_ack_sequence);
    state
        .inventory_equipment
        .client_gui_status_request_acknowledgements = state
        .inventory_equipment
        .client_gui_status_request_acknowledgements
        .saturating_add(1);
    tracing::info!(
        queued_update_index = queued.update_index,
        synthetic_sequence = queued.synthetic_sequence,
        server_ack_sequence,
        "inventory/equipment bridge observed legacy server ACK for proxy-owned ClientGuiInventory_Status request"
    );
}

pub(super) fn maybe_queue_inventory_equipment_bridge_output(
    state: &mut SessionState,
    trigger_sequence: u16,
    ack_sequence: u16,
) -> anyhow::Result<()> {
    let Some(update) = state
        .semantic
        .ui
        .last_inventory_equipment_bridge_handoff_state_update
    else {
        return Ok(());
    };

    if state
        .inventory_equipment
        .last_decision_state_update_index
        .is_some_and(|handled| handled == update.update_index)
    {
        return Ok(());
    }

    if update.consumer == ClientGuiInventory {
        maybe_queue_client_gui_status_output(state, update, Some(trigger_sequence))?;
        return Ok(());
    }

    let Some(claim) = update.server_inventory_claim else {
        record_output_decision(
            state,
            update,
            InventoryEquipmentBridgeOutputDecisionKind::DeferredMissingClaim,
        );
        state
            .inventory_equipment
            .last_deferred_missing_claim_update_index = Some(update.update_index);
        state.inventory_equipment.deferred_missing_claim_updates = state
            .inventory_equipment
            .deferred_missing_claim_updates
            .saturating_add(1);
        tracing::debug!(
            update_index = update.update_index,
            "inventory/equipment bridge output deferred: drained update lacks server Inventory claim"
        );
        return Ok(());
    };

    let claim_object_status = state
        .semantic
        .objects
        .inventory_item_object_status(claim.object_id);
    if claim.object_id != update.candidate.object_id
        && !matches!(
            claim_object_status,
            crate::translate::semantic::InventoryItemObjectStatus::Proven(_)
        )
    {
        if maybe_queue_current_player_client_gui_status_for_unknown_server_claim(
            state,
            update,
            trigger_sequence,
        )? {
            tracing::info!(
                update_index = update.update_index,
                claim_object_id = %format_args!("0x{:08X}", claim.object_id),
                claim_object_status = claim_object_status.as_str(),
                candidate_object_id = %format_args!("0x{:08X}", update.candidate.object_id),
                "inventory/equipment bridge queued ClientGui status instead of emitting unknown server Inventory claim"
            );
            return Ok(());
        }

        record_output_decision(
            state,
            update,
            InventoryEquipmentBridgeOutputDecisionKind::BlockedCandidateMismatch,
        );
        state
            .inventory_equipment
            .last_blocked_candidate_mismatch_update_index = Some(update.update_index);
        state.inventory_equipment.blocked_candidate_mismatch_updates = state
            .inventory_equipment
            .blocked_candidate_mismatch_updates
            .saturating_add(1);
        let claim_proven_neighborhood = state
            .semantic
            .objects
            .inventory_item_object_proven_neighborhood(claim.object_id);
        let closest_proven_neighbor = claim_proven_neighborhood.closest();
        tracing::warn!(
            update_index = update.update_index,
            claim_object_id = %format_args!("0x{:08X}", claim.object_id),
            claim_object_status = claim_object_status.as_str(),
            claim_object_proof = claim_object_status.proof().map(|proof| proof.as_str()).unwrap_or("none"),
            candidate_object_id = %format_args!("0x{:08X}", update.candidate.object_id),
            closest_proven_item_object_id = closest_proven_neighbor.map(|neighbor| format!("0x{:08X}", neighbor.object_id)).unwrap_or_else(|| "none".to_string()),
            closest_proven_item_distance = closest_proven_neighbor.map(|neighbor| neighbor.distance).unwrap_or(0),
            "inventory/equipment bridge output blocked: server Inventory object differs from ready item-state candidate"
        );
        return Ok(());
    }
    if claim.object_id != update.candidate.object_id {
        tracing::info!(
            update_index = update.update_index,
            claim_object_id = %format_args!("0x{:08X}", claim.object_id),
            candidate_object_id = %format_args!("0x{:08X}", update.candidate.object_id),
            "inventory/equipment bridge using server Inventory claim object with independent known item-state proof"
        );
    }

    let payload = inventory::build_ee_inventory_payload(
        claim.minor,
        claim.object_id,
        claim.result,
        claim.equip_slot,
    )
    .ok_or_else(|| {
        anyhow::anyhow!("drained inventory/equipment update did not build exact Inventory payload")
    })?;
    let shifted_trigger_sequence =
        shift_sequence_for_peer(&state.sequence.server_sequence_shifts, trigger_sequence);
    let synthetic_sequence = shifted_trigger_sequence.wrapping_add(1);
    let packet =
        synthetic_area::build_synthetic_gameplay_frame(synthetic_sequence, ack_sequence, &payload)?;

    let future_shift_base = trigger_sequence.wrapping_add(1);
    state.sequence.server_sequence_shifts.push(SequenceShift {
        base: future_shift_base,
        delta: INVENTORY_EQUIPMENT_BRIDGE_INSERTED_FRAME_COUNT,
    });
    trim_sequence_shifts(&mut state.sequence.server_sequence_shifts);
    state
        .synthetic_area
        .pending_server_to_client_packets
        .push(PendingServerPacket {
            family: VerifiedFamily::Inventory,
            packet,
            due_at: Instant::now(),
            reason: INVENTORY_EQUIPMENT_BRIDGE_REASON,
            placement: PendingServerPacketPlacement::AfterCurrentEmit,
        });
    record_output_decision(
        state,
        update,
        InventoryEquipmentBridgeOutputDecisionKind::QueuedInventoryOutput,
    );
    state.inventory_equipment.last_queued_state_update_index = Some(update.update_index);
    state.inventory_equipment.queued_outputs =
        state.inventory_equipment.queued_outputs.saturating_add(1);
    state.inventory_equipment.last_queued_output = Some(InventoryEquipmentBridgeQueuedOutput {
        update_index: update.update_index,
        emission_index: update.emission_index,
        event_index: update.event_index,
        minor: claim.minor,
        object_id: claim.object_id,
        result: claim.result,
        equip_slot: claim.equip_slot,
        trigger_sequence,
        synthetic_sequence,
    });

    tracing::info!(
        update_index = update.update_index,
        emission_index = update.emission_index,
        event_index = update.event_index,
        object_id = %format_args!("0x{:08X}", claim.object_id),
        equip_slot = claim.equip_slot,
        result = claim.result,
        trigger_sequence,
        synthetic_sequence,
        future_shift_base,
        pending_server_packets = state.synthetic_area.pending_server_to_client_packets.len(),
        "inventory/equipment bridge queued exact EE Inventory output"
    );
    Ok(())
}

pub(super) fn maybe_record_non_server_inventory_equipment_bridge_output_decision(
    state: &mut SessionState,
) {
    let Some(update) = state
        .semantic
        .ui
        .last_inventory_equipment_bridge_handoff_state_update
    else {
        return;
    };

    if state
        .inventory_equipment
        .last_decision_state_update_index
        .is_some_and(|handled| handled == update.update_index)
    {
        return;
    }

    if update.consumer == ClientGuiInventory
        && let Err(err) = maybe_queue_client_gui_status_output(state, update, None)
    {
        tracing::warn!(
            error = %err,
            update_index = update.update_index,
            "failed to queue inventory/equipment ClientGuiInventory bridge output"
        );
    }
}

pub(super) fn maybe_record_client_gui_status_live_object_response(
    state: &mut SessionState,
    proof: &VerifiedProof,
    server_sequence: u16,
    ack_sequence: u16,
) {
    if state.inventory_equipment.queued_client_gui_status_outputs == 0
        || !proof.contains_family(VerifiedFamily::GameObjUpdateLiveObject)
        || state
            .inventory_equipment
            .client_gui_status_response_window_complete()
    {
        return;
    }
    let Some(queued_synthetic_sequence) = state
        .inventory_equipment
        .last_queued_client_gui_status_output
        .map(|queued| queued.synthetic_sequence)
    else {
        return;
    };
    // `translate_server_to_client` records this frame's raw peer ACK before
    // unshifting it for EE. Historical acknowledgement of the request is not
    // sufficient here: a reordered/retransmitted frame whose own ACK still
    // precedes the synthetic request cannot be its response materialization.
    let server_peer_ack_sequence = state
        .inventory_equipment
        .last_observed_client_gui_status_server_peer_ack_sequence
        .unwrap_or(ack_sequence);
    let current_packet_acknowledges_request =
        sequence_at_or_after(server_peer_ack_sequence, queued_synthetic_sequence);
    if !state
        .inventory_equipment
        .client_gui_status_request_acknowledged()
        || !current_packet_acknowledges_request
    {
        state
            .inventory_equipment
            .client_gui_status_pre_ack_live_object_packets_ignored = state
            .inventory_equipment
            .client_gui_status_pre_ack_live_object_packets_ignored
            .saturating_add(1);
        state
            .inventory_equipment
            .last_pre_ack_client_gui_status_live_object_server_sequence = Some(server_sequence);
        state
            .inventory_equipment
            .last_pre_ack_client_gui_status_live_object_server_ack_sequence = state
            .inventory_equipment
            .last_observed_client_gui_status_server_peer_ack_sequence
            .or(Some(ack_sequence));
        tracing::debug!(
            queued_update_index = state
                .inventory_equipment
                .last_queued_client_gui_status_update_index
                .unwrap_or(0),
            server_sequence,
            client_unshifted_ack_sequence = ack_sequence,
            server_peer_ack_sequence = state
                .inventory_equipment
                .last_pre_ack_client_gui_status_live_object_server_ack_sequence
                .unwrap_or(0),
            "inventory/equipment bridge ignored live-object packet before legacy server acknowledged proxy-owned ClientGuiInventory_Status"
        );
        return;
    }
    let Some(summary) = state
        .semantic
        .ui
        .last_live_object_inventory_materialization
        .clone()
    else {
        return;
    };
    let queued_update_index = state
        .inventory_equipment
        .last_queued_client_gui_status_update_index
        .unwrap_or(0);
    let queued_candidate = state
        .inventory_equipment
        .last_queued_client_gui_status_output
        .and_then(|queued| queued.candidate);
    let materialized_item_object_ids = summary.materialized_item_object_ids.len();
    let materialized_item_object_id_first = summary
        .materialized_item_object_ids
        .first()
        .copied()
        .unwrap_or(0);
    let materialized_item_object_id_last = summary
        .materialized_item_object_ids
        .last()
        .copied()
        .unwrap_or(0);
    let materialized_item_object_id_min = summary
        .materialized_item_object_ids
        .iter()
        .copied()
        .min()
        .unwrap_or(0);
    let materialized_item_object_id_max = summary
        .materialized_item_object_ids
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    let materialized_item_object_ids_contain_queued_candidate =
        queued_candidate.is_some_and(|candidate| {
            summary
                .materialized_item_object_ids
                .contains(&candidate.object_id)
        });
    state
        .inventory_equipment
        .client_gui_status_response_live_object_packets = state
        .inventory_equipment
        .client_gui_status_response_live_object_packets
        .saturating_add(1);
    if summary.live_gui_records != 0 {
        state
            .inventory_equipment
            .client_gui_status_response_live_gui_record_packets = state
            .inventory_equipment
            .client_gui_status_response_live_gui_record_packets
            .saturating_add(1);
    }
    if materialized_item_object_ids != 0 {
        state
            .inventory_equipment
            .client_gui_status_response_materialized_item_packets = state
            .inventory_equipment
            .client_gui_status_response_materialized_item_packets
            .saturating_add(1);
    }
    // Keep the exact current-packet transport boundary alongside the EE-facing
    // ACK so a completed status window remains auditable after sequence
    // translation.
    let response = InventoryEquipmentBridgeClientGuiStatusResponse {
        queued_update_index,
        server_sequence,
        server_peer_ack_sequence,
        ack_sequence,
        live_gui_records: summary.live_gui_records,
        live_gui_fragment_bits: summary.live_gui_fragment_bits,
        materialized_item_object_ids,
        materialized_item_object_id_first,
        materialized_item_object_id_last,
        materialized_item_object_id_min,
        materialized_item_object_id_max,
        materialized_item_object_ids_contain_queued_candidate,
        compact_item_emission_ready_objects: summary.compact_item_emission_ready_objects,
        compact_item_emission_ready_candidate: summary.compact_item_emission_ready_candidate,
    };
    state.inventory_equipment.last_client_gui_status_response = Some(response);
    let update_best = match state.inventory_equipment.best_client_gui_status_response {
        Some(best) if best.queued_update_index != queued_update_index => true,
        Some(best) => response.is_stronger_than(best),
        None => true,
    };
    if update_best {
        state.inventory_equipment.best_client_gui_status_response = Some(response);
    }
    maybe_stage_confirmed_inventory_replay(state, &summary);
    if state
        .inventory_equipment
        .client_gui_status_refresh_confirmed()
    {
        state
            .inventory_equipment
            .last_completed_client_gui_status_response_update_index = Some(queued_update_index);
        tracing::info!(
            queued_update_index,
            server_sequence,
            server_peer_ack_sequence,
            ack_sequence,
            request_completion = state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            candidate_association = state
                .inventory_equipment
                .best_client_gui_status_response_association()
                .as_str(),
            materialized_item_object_ids_contain_queued_candidate,
            "inventory/equipment bridge completed proxy-owned ClientGuiInventory_Status response window"
        );
    }
    tracing::info!(
        queued_update_index,
        server_sequence,
        server_peer_ack_sequence,
        ack_sequence,
        live_gui_records = summary.live_gui_records,
        live_gui_fragment_bits = summary.live_gui_fragment_bits,
        materialized_item_object_ids,
        materialized_item_object_id_first = %format_args!("0x{:08X}", materialized_item_object_id_first),
        materialized_item_object_id_last = %format_args!("0x{:08X}", materialized_item_object_id_last),
        materialized_item_object_id_min = %format_args!("0x{:08X}", materialized_item_object_id_min),
        materialized_item_object_id_max = %format_args!("0x{:08X}", materialized_item_object_id_max),
        materialized_item_object_ids_contain_queued_candidate,
        compact_item_emission_ready_objects = summary.compact_item_emission_ready_objects,
        compact_item_emission_ready_candidate_object_id = summary
            .compact_item_emission_ready_candidate
            .map(|candidate| format!("0x{:08X}", candidate.object_id))
            .unwrap_or_else(|| "none".to_string()),
        "inventory/equipment bridge observed server live-object response after proxy-owned ClientGuiInventory_Status"
    );
}

fn maybe_stage_confirmed_inventory_replay(
    state: &mut SessionState,
    summary: &crate::translate::semantic::LiveObjectInventoryMaterializationSummary,
) {
    let Some(decision) = state.inventory_equipment.last_decision else {
        return;
    };
    let Some(queued_status) = state
        .inventory_equipment
        .last_queued_client_gui_status_output
    else {
        return;
    };
    let Some(queued_candidate) = queued_status.candidate else {
        return;
    };
    let Some(claim) = decision.server_inventory_claim else {
        return;
    };
    if decision.kind != InventoryEquipmentBridgeOutputDecisionKind::QueuedClientGuiStatusOutput
        || decision.consumer
            != crate::translate::semantic::InventoryEquipmentHandoffConsumer::ServerInventory
        || decision.update_index != queued_status.update_index
        || state
            .inventory_equipment
            .last_confirmed_inventory_replay_update_index
            == Some(decision.update_index)
        || state
            .inventory_equipment
            .pending_confirmed_inventory_replay
            .is_some()
        || !summary
            .materialized_item_object_ids
            .contains(&queued_candidate.object_id)
        || !summary
            .materialized_item_object_ids
            .contains(&claim.object_id)
        || !matches!(
            state
                .semantic
                .objects
                .inventory_item_object_status(claim.object_id),
            crate::translate::semantic::InventoryItemObjectStatus::Proven(_)
        )
    {
        return;
    }

    state.inventory_equipment.pending_confirmed_inventory_replay =
        Some(InventoryEquipmentBridgePendingConfirmedInventoryReplay {
            update_index: decision.update_index,
            emission_index: decision.emission_index,
            event_index: decision.event_index,
            claim,
        });
    tracing::info!(
        update_index = decision.update_index,
        queued_candidate_object_id = %format_args!("0x{:08X}", queued_candidate.object_id),
        claim_object_id = %format_args!("0x{:08X}", claim.object_id),
        claim_minor = claim.minor,
        claim_result = claim.result,
        claim_equip_slot = claim.equip_slot,
        "inventory/equipment bridge staged original Inventory result after associated ClientGui status materialized its claim object"
    );
}

pub(super) fn maybe_queue_confirmed_inventory_replay(
    state: &mut SessionState,
    response_last_sequence: u16,
    ack_sequence: u16,
) -> anyhow::Result<bool> {
    let Some(pending) = state
        .inventory_equipment
        .pending_confirmed_inventory_replay
        .take()
    else {
        return Ok(false);
    };
    if state
        .inventory_equipment
        .last_confirmed_inventory_replay_update_index
        == Some(pending.update_index)
    {
        return Ok(false);
    }

    // `inventory::build_ee_inventory_payload` owns the decompile-backed EE
    // writer order: OBJECTIDServer and DWORD equip slot in the CNW read
    // buffer, followed by the single MSB-owned result BOOL in the fragment
    // stream. Reusing its exact validator here prevents a materialization
    // timing repair from becoming a second, weaker packet writer.
    let claim = pending.claim;
    let payload = inventory::build_ee_inventory_payload(
        claim.minor,
        claim.object_id,
        claim.result,
        claim.equip_slot,
    )
    .ok_or_else(|| {
        anyhow::anyhow!("confirmed ClientGui status replay did not build exact Inventory payload")
    })?;
    let shifted_response_last_sequence = shift_sequence_for_peer(
        &state.sequence.server_sequence_shifts,
        response_last_sequence,
    );
    let synthetic_sequence = shifted_response_last_sequence.wrapping_add(1);
    let packet =
        synthetic_area::build_synthetic_gameplay_frame(synthetic_sequence, ack_sequence, &payload)?;

    let future_shift_base = response_last_sequence.wrapping_add(1);
    state.sequence.server_sequence_shifts.push(SequenceShift {
        base: future_shift_base,
        delta: INVENTORY_EQUIPMENT_BRIDGE_INSERTED_FRAME_COUNT,
    });
    trim_sequence_shifts(&mut state.sequence.server_sequence_shifts);
    state
        .synthetic_area
        .pending_server_to_client_packets
        .push(PendingServerPacket {
            family: VerifiedFamily::Inventory,
            packet,
            due_at: Instant::now(),
            reason: CONFIRMED_CLIENT_GUI_INVENTORY_REPLAY_REASON,
            placement: PendingServerPacketPlacement::AfterCurrentEmit,
        });

    let claim_object_status = state
        .semantic
        .objects
        .inventory_item_object_status(claim.object_id);
    if let Some(decision) = state.inventory_equipment.last_decision.as_mut()
        && decision.update_index == pending.update_index
    {
        decision.kind = InventoryEquipmentBridgeOutputDecisionKind::QueuedConfirmedInventoryReplay;
        decision.server_inventory_claim_object_status = claim_object_status;
        decision.server_inventory_claim_proven_neighborhood = state
            .semantic
            .objects
            .inventory_item_object_proven_neighborhood(claim.object_id);
    }
    state.inventory_equipment.last_queued_state_update_index = Some(pending.update_index);
    state
        .inventory_equipment
        .last_confirmed_inventory_replay_update_index = Some(pending.update_index);
    state.inventory_equipment.queued_outputs =
        state.inventory_equipment.queued_outputs.saturating_add(1);
    state.inventory_equipment.confirmed_inventory_replay_outputs = state
        .inventory_equipment
        .confirmed_inventory_replay_outputs
        .saturating_add(1);
    state.inventory_equipment.last_queued_output = Some(InventoryEquipmentBridgeQueuedOutput {
        update_index: pending.update_index,
        emission_index: pending.emission_index,
        event_index: pending.event_index,
        minor: claim.minor,
        object_id: claim.object_id,
        result: claim.result,
        equip_slot: claim.equip_slot,
        trigger_sequence: response_last_sequence,
        synthetic_sequence,
    });

    tracing::info!(
        update_index = pending.update_index,
        object_id = %format_args!("0x{:08X}", claim.object_id),
        equip_slot = claim.equip_slot,
        result = claim.result,
        response_last_sequence,
        synthetic_sequence,
        future_shift_base,
        "inventory/equipment bridge queued exact Inventory replay after materialized ClientGui status response"
    );
    Ok(true)
}

fn maybe_queue_client_gui_status_output(
    state: &mut SessionState,
    update: crate::translate::semantic::InventoryEquipmentBridgeStateUpdate,
    server_sequence_to_ack: Option<u16>,
) -> anyhow::Result<bool> {
    if update.consumer != ClientGuiInventory {
        return Ok(false);
    }

    let Some(claim) = update.client_gui_inventory_claim else {
        record_deferred_client_gui_output_decision(
            state,
            update,
            "inventory/equipment bridge output deferred: ClientGui handoff lacks exact GUI claim",
        );
        return Ok(true);
    };

    if claim.kind != InventoryEquipmentClientGuiInventoryClaimKind::Status {
        record_deferred_client_gui_output_decision(
            state,
            update,
            "inventory/equipment bridge output deferred: ClientGui handoff is not a status request",
        );
        return Ok(true);
    }

    let Some(object_id) = claim.object_id else {
        record_deferred_client_gui_output_decision(
            state,
            update,
            "inventory/equipment bridge output deferred: ClientGui status lacks object id",
        );
        return Ok(true);
    };

    if object_id != client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID {
        record_deferred_client_gui_output_decision(
            state,
            update,
            "inventory/equipment bridge output deferred: ClientGui status is not current-player inventory",
        );
        return Ok(true);
    }

    let Some(latest_client_sequence) = state.sequence.latest_client_sequence_from_client else {
        record_deferred_client_gui_output_decision(
            state,
            update,
            "inventory/equipment bridge output deferred: no client reliable sequence observed for proxy-owned ClientGui status",
        );
        return Ok(true);
    };

    queue_client_gui_status_output_with_claim(
        state,
        update,
        claim,
        latest_client_sequence,
        server_sequence_to_ack,
    )
}

fn maybe_queue_current_player_client_gui_status_for_unknown_server_claim(
    state: &mut SessionState,
    update: crate::translate::semantic::InventoryEquipmentBridgeStateUpdate,
    server_sequence_to_ack: u16,
) -> anyhow::Result<bool> {
    let Some(latest_client_sequence) = state.sequence.latest_client_sequence_from_client else {
        return Ok(false);
    };
    let claim = InventoryEquipmentClientGuiInventoryClaim {
        kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
        object_id: Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID),
        panel: None,
        player_inventory_gui: Some(true),
        rewritten_self_object_id: false,
    };
    queue_client_gui_status_output_with_claim(
        state,
        update,
        claim,
        latest_client_sequence,
        Some(server_sequence_to_ack),
    )
}

fn queue_client_gui_status_output_with_claim(
    state: &mut SessionState,
    update: crate::translate::semantic::InventoryEquipmentBridgeStateUpdate,
    claim: InventoryEquipmentClientGuiInventoryClaim,
    latest_client_sequence: u16,
    server_sequence_to_ack: Option<u16>,
) -> anyhow::Result<bool> {
    let player_inventory_gui = claim.player_inventory_gui.unwrap_or(true);
    let object_id = claim
        .object_id
        .ok_or_else(|| anyhow::anyhow!("ClientGuiInventory_Status output claim lacks object id"))?;
    if object_id != client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID {
        return Err(anyhow::anyhow!(
            "ClientGuiInventory_Status output claim is not current-player inventory"
        ));
    }
    let payload = client_gui_inventory::build_status_payload(object_id, player_inventory_gui);
    client_gui_inventory::claim_payload_if_verified(&payload).ok_or_else(|| {
        anyhow::anyhow!("built ClientGuiInventory_Status payload failed exact validator")
    })?;

    let trigger_client_sequence = latest_client_sequence.wrapping_add(1);
    let synthetic_sequence = shift_sequence_for_peer(
        &state.sequence.client_sequence_shifts,
        trigger_client_sequence,
    );
    let ack_sequence = server_sequence_to_ack
        .filter(|sequence| *sequence != 0)
        .or(state.sequence.latest_client_ack_from_client)
        .or(state.sequence.latest_server_sequence_to_client)
        .unwrap_or(0);
    let packet =
        synthetic_area::build_synthetic_gameplay_frame(synthetic_sequence, ack_sequence, &payload)?;

    state.sequence.pending_client_to_server_packets.push(packet);
    state.sequence.client_sequence_shifts.push(SequenceShift {
        base: trigger_client_sequence,
        delta: INVENTORY_EQUIPMENT_BRIDGE_INSERTED_FRAME_COUNT,
    });
    trim_sequence_shifts(&mut state.sequence.client_sequence_shifts);

    let decision_update = crate::translate::semantic::InventoryEquipmentBridgeStateUpdate {
        client_gui_inventory_claim: Some(claim),
        ..update
    };
    record_output_decision(
        state,
        decision_update,
        InventoryEquipmentBridgeOutputDecisionKind::QueuedClientGuiStatusOutput,
    );
    state
        .inventory_equipment
        .last_queued_client_gui_status_update_index = Some(update.update_index);
    state.inventory_equipment.queued_client_gui_status_outputs = state
        .inventory_equipment
        .queued_client_gui_status_outputs
        .saturating_add(1);
    state
        .inventory_equipment
        .last_queued_client_gui_status_output =
        Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
            update_index: update.update_index,
            emission_index: update.emission_index,
            event_index: update.event_index,
            candidate: Some(update.candidate),
            ready_objects: update.ready_objects,
            deferred_feature25_only_objects: update.deferred_feature25_only_objects,
            object_id,
            player_inventory_gui,
            trigger_client_sequence,
            synthetic_sequence,
            ack_sequence,
        });

    tracing::info!(
        update_index = update.update_index,
        emission_index = update.emission_index,
        event_index = update.event_index,
        object_id = %format_args!("0x{:08X}", object_id),
        player_inventory_gui,
        trigger_client_sequence,
        synthetic_sequence,
        ack_sequence,
        pending_client_packets = state.sequence.pending_client_to_server_packets.len(),
        "inventory/equipment bridge queued proxy-owned ClientGuiInventory_Status request"
    );

    Ok(true)
}

fn record_deferred_client_gui_output_decision(
    state: &mut SessionState,
    update: crate::translate::semantic::InventoryEquipmentBridgeStateUpdate,
    message: &'static str,
) {
    if update.consumer != ClientGuiInventory {
        return;
    }

    record_output_decision(
        state,
        update,
        InventoryEquipmentBridgeOutputDecisionKind::DeferredClientGui,
    );
    state
        .inventory_equipment
        .last_deferred_client_gui_update_index = Some(update.update_index);
    state.inventory_equipment.deferred_client_gui_updates = state
        .inventory_equipment
        .deferred_client_gui_updates
        .saturating_add(1);
    tracing::debug!(
        update_index = update.update_index,
        consumer = update.consumer.as_str(),
        message
    );
}

fn record_output_decision(
    state: &mut SessionState,
    update: crate::translate::semantic::InventoryEquipmentBridgeStateUpdate,
    kind: InventoryEquipmentBridgeOutputDecisionKind,
) {
    let candidate_object_status = state
        .semantic
        .objects
        .inventory_item_object_status(update.candidate.object_id);
    let server_inventory_claim_object_status = update
        .server_inventory_claim
        .map(|claim| {
            state
                .semantic
                .objects
                .inventory_item_object_status(claim.object_id)
        })
        .unwrap_or(crate::translate::semantic::InventoryItemObjectStatus::Unknown);
    let server_inventory_claim_proven_neighborhood = update
        .server_inventory_claim
        .map(|claim| {
            state
                .semantic
                .objects
                .inventory_item_object_proven_neighborhood(claim.object_id)
        })
        .unwrap_or_default();
    state.inventory_equipment.last_decision_state_update_index = Some(update.update_index);
    state.inventory_equipment.last_decision = Some(InventoryEquipmentBridgeOutputDecision {
        kind,
        update_index: update.update_index,
        emission_index: update.emission_index,
        event_index: update.event_index,
        consumer: update.consumer,
        candidate: update.candidate,
        candidate_object_status,
        ready_objects: update.ready_objects,
        deferred_feature25_only_objects: update.deferred_feature25_only_objects,
        server_inventory_claim: update.server_inventory_claim,
        server_inventory_claim_object_status,
        server_inventory_claim_proven_neighborhood,
        client_gui_inventory_claim: update.client_gui_inventory_claim,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        packet::m::MFrameView,
        translate::{
            client_gui_inventory,
            semantic::{
                InventoryEquipmentBridgeStateUpdate, InventoryEquipmentClientGuiInventoryClaim,
                InventoryEquipmentClientGuiInventoryClaimKind, InventoryEquipmentHandoffConsumer,
                InventoryEquipmentServerInventoryClaim, InventoryItemContextCandidate,
                InventoryItemContextCandidateSource, InventoryItemObjectProof,
                InventoryItemObjectProvenNeighbor, InventoryItemObjectStatus,
            },
        },
    };

    fn mark_current_status_server_acknowledged(state: &mut SessionState, server_ack_sequence: u16) {
        state
            .inventory_equipment
            .last_acknowledged_client_gui_status_update_index = state
            .inventory_equipment
            .last_queued_client_gui_status_update_index;
        state
            .inventory_equipment
            .last_acknowledged_client_gui_status_server_ack_sequence = Some(server_ack_sequence);
        state
            .inventory_equipment
            .last_observed_client_gui_status_server_peer_ack_sequence = Some(server_ack_sequence);
    }

    fn ready_server_inventory_update() -> InventoryEquipmentBridgeStateUpdate {
        InventoryEquipmentBridgeStateUpdate {
            update_index: 1,
            emission_index: 1,
            consumer: InventoryEquipmentHandoffConsumer::ServerInventory,
            event_index: 1,
            candidate: InventoryItemContextCandidate {
                object_id: 0x8000_1234,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            },
            ready_objects: 1,
            deferred_feature25_only_objects: 0,
            server_inventory_claim: Some(InventoryEquipmentServerInventoryClaim::new(
                0x01,
                0x8000_1234,
                true,
                4,
            )),
            client_gui_inventory_claim: None,
        }
    }

    #[test]
    fn queues_exact_inventory_output_after_server_inventory_state_update() {
        let mut state = SessionState::default();
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update =
            Some(ready_server_inventory_update());

        maybe_queue_inventory_equipment_bridge_output(&mut state, 10, 77)
            .expect("inventory bridge output should queue");

        assert_eq!(
            state.inventory_equipment.last_decision_state_update_index,
            Some(1)
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_decision
                .expect("decision should be recorded")
                .kind,
            InventoryEquipmentBridgeOutputDecisionKind::QueuedInventoryOutput
        );
        assert_eq!(
            state.inventory_equipment.last_queued_state_update_index,
            Some(1)
        );
        assert_eq!(state.inventory_equipment.queued_outputs, 1);
        assert_eq!(
            state.inventory_equipment.last_queued_output,
            Some(InventoryEquipmentBridgeQueuedOutput {
                update_index: 1,
                emission_index: 1,
                event_index: 1,
                minor: 0x01,
                object_id: 0x8000_1234,
                result: true,
                equip_slot: 4,
                trigger_sequence: 10,
                synthetic_sequence: 11,
            })
        );
        assert_eq!(
            state.synthetic_area.pending_server_to_client_packets.len(),
            1
        );
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.server_sequence_shifts[0].base, 11);
        assert_eq!(state.sequence.server_sequence_shifts[0].delta, 1);

        let pending = &state.synthetic_area.pending_server_to_client_packets[0];
        assert_eq!(pending.family, VerifiedFamily::Inventory);
        let view = MFrameView::parse(&pending.packet).expect("queued packet should parse");
        assert_eq!(view.sequence, 11);
        assert_eq!(view.ack_sequence, 77);
        let payload = super::super::parse_window::primary_payload(&pending.packet, &view)
            .expect("queued packet should expose primary payload");
        let claim = inventory::claim_payload_if_verified(payload)
            .expect("queued Inventory payload should be exact EE shape");
        assert_eq!(claim.object_id, 0x8000_1234);
        assert!(claim.result);
        assert_eq!(claim.equip_slot, 4);
    }

    #[test]
    fn queues_client_gui_status_output_for_current_player_inventory_update() {
        let mut update = ready_server_inventory_update();
        update.consumer = ClientGuiInventory;
        update.server_inventory_claim = None;
        update.client_gui_inventory_claim = Some(InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
            object_id: Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID),
            panel: None,
            player_inventory_gui: Some(true),
            rewritten_self_object_id: true,
        });
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(10);
        state.sequence.latest_client_ack_from_client = Some(77);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_queue_inventory_equipment_bridge_output(&mut state, 10, 77)
            .expect("client GUI status update should queue");

        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.sequence.client_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.client_sequence_shifts[0].base, 11);
        assert_eq!(state.sequence.client_sequence_shifts[0].delta, 1);
        assert_eq!(
            state.inventory_equipment.last_decision_state_update_index,
            Some(1)
        );
        let decision = state
            .inventory_equipment
            .last_decision
            .expect("decision should be recorded");
        assert_eq!(
            decision.kind,
            InventoryEquipmentBridgeOutputDecisionKind::QueuedClientGuiStatusOutput
        );
        assert_eq!(
            decision
                .client_gui_inventory_claim
                .expect("client GUI decision should retain exact claim")
                .object_id,
            Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID)
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_queued_client_gui_status_update_index,
            Some(1)
        );
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            1
        );
        assert_eq!(state.inventory_equipment.queued_outputs, 0);
        assert_eq!(
            state
                .inventory_equipment
                .last_queued_client_gui_status_output,
            Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 1,
                emission_index: 1,
                event_index: 1,
                candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8000_1234,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
                ready_objects: 1,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: true,
                trigger_client_sequence: 11,
                synthetic_sequence: 11,
                ack_sequence: 10,
            })
        );

        let pending = state.sequence.pending_client_to_server_packets.remove(0);
        let view = MFrameView::parse(&pending).expect("queued client packet should parse");
        assert_eq!(view.sequence, 11);
        assert_eq!(view.ack_sequence, 10);
        let payload = super::super::parse_window::primary_payload(&pending, &view)
            .expect("queued packet should expose primary payload");
        let claim = client_gui_inventory::claim_payload_if_verified(payload)
            .expect("queued ClientGuiInventory payload should be exact");
        assert_eq!(
            claim.object_id,
            Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID)
        );

        maybe_queue_inventory_equipment_bridge_output(&mut state, 11, 77)
            .expect("same client GUI update should remain handled");

        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 0);
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            1
        );
        assert_eq!(state.inventory_equipment.queued_outputs, 0);
    }

    #[test]
    fn records_live_object_response_after_client_gui_status_output() {
        let mut state = SessionState::default();
        state.inventory_equipment.queued_client_gui_status_outputs = 1;
        state
            .inventory_equipment
            .last_queued_client_gui_status_update_index = Some(7);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 7,
                emission_index: 7,
                event_index: 7,
                candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_56BC,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
                ready_objects: 51,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: true,
                trigger_client_sequence: 80,
                synthetic_sequence: 80,
                ack_sequence: 82,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                live_gui_records: 51,
                live_gui_fragment_bits: 348,
                materialized_item_object_ids: vec![0x8001_56BC, 0x8001_56BD],
                compact_item_emission_ready_objects: 51,
                compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_56BC,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
            },
        );
        mark_current_status_server_acknowledged(&mut state, 80);

        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            48,
            82,
        );

        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_live_object_packets,
            1
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_live_gui_record_packets,
            1
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_materialized_item_packets,
            1
        );
        let response = state
            .inventory_equipment
            .last_client_gui_status_response
            .expect("response should be retained");
        assert_eq!(response.queued_update_index, 7);
        assert_eq!(response.server_sequence, 48);
        assert_eq!(response.ack_sequence, 82);
        assert_eq!(response.live_gui_records, 51);
        assert_eq!(response.live_gui_fragment_bits, 348);
        assert_eq!(response.materialized_item_object_ids, 2);
        assert_eq!(response.materialized_item_object_id_first, 0x8001_56BC);
        assert_eq!(response.materialized_item_object_id_last, 0x8001_56BD);
        assert_eq!(response.materialized_item_object_id_min, 0x8001_56BC);
        assert_eq!(response.materialized_item_object_id_max, 0x8001_56BD);
        assert!(response.materialized_item_object_ids_contain_queued_candidate);
        assert_eq!(response.compact_item_emission_ready_objects, 51);
        assert_eq!(
            response.compact_item_emission_ready_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8001_56BC,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            })
        );
        assert_eq!(
            state.inventory_equipment.best_client_gui_status_response,
            Some(response)
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_outcome()
                .as_str(),
            "materialized_items"
        );
        assert_eq!(
            state
                .inventory_equipment
                .best_client_gui_status_response_association()
                .as_str(),
            "matches_queued_status_candidate"
        );
        assert!(
            state
                .inventory_equipment
                .client_gui_status_refresh_confirmed()
        );
        assert_eq!(
            state
                .inventory_equipment
                .best_client_gui_status_response_candidate_delta_from_queued_status(),
            0
        );
    }

    #[test]
    fn client_gui_status_response_matches_when_materialized_set_contains_queued_candidate() {
        let mut state = SessionState::default();
        state.inventory_equipment.queued_client_gui_status_outputs = 1;
        state
            .inventory_equipment
            .last_queued_client_gui_status_update_index = Some(1);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 1,
                emission_index: 1,
                event_index: 3,
                candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_538E,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
                ready_objects: 18,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: true,
                trigger_client_sequence: 79,
                synthetic_sequence: 81,
                ack_sequence: 44,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                live_gui_records: 52,
                live_gui_fragment_bits: 355,
                materialized_item_object_ids: vec![0x8001_5386, 0x8001_538E],
                compact_item_emission_ready_objects: 66,
                compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_5386,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
            },
        );
        mark_current_status_server_acknowledged(&mut state, 81);

        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            60,
            78,
        );

        let response = state
            .inventory_equipment
            .best_client_gui_status_response
            .expect("materialized response should be retained");
        assert_eq!(response.materialized_item_object_ids, 2);
        assert_eq!(response.materialized_item_object_id_first, 0x8001_5386);
        assert_eq!(response.materialized_item_object_id_last, 0x8001_538E);
        assert!(response.materialized_item_object_ids_contain_queued_candidate);
        assert_eq!(
            response.compact_item_emission_ready_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8001_5386,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            })
        );
        assert_eq!(
            state
                .inventory_equipment
                .best_client_gui_status_response_association()
                .as_str(),
            "matches_queued_status_candidate"
        );
        assert!(
            state
                .inventory_equipment
                .client_gui_status_refresh_confirmed()
        );
        assert_eq!(
            state
                .inventory_equipment
                .best_client_gui_status_response_candidate_delta_from_queued_status(),
            -8
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_completed_client_gui_status_response_update_index,
            Some(1)
        );

        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                live_gui_records: 0,
                live_gui_fragment_bits: 0,
                materialized_item_object_ids: Vec::new(),
                compact_item_emission_ready_objects: 66,
                compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_5386,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
            },
        );
        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            61,
            78,
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_live_object_packets,
            1
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_client_gui_status_response
                .expect("completed response should remain terminal")
                .server_sequence,
            60
        );

        state.inventory_equipment.queued_client_gui_status_outputs = 2;
        state
            .inventory_equipment
            .last_queued_client_gui_status_update_index = Some(2);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output
            .as_mut()
            .expect("queued status should exist")
            .update_index = 2;
        mark_current_status_server_acknowledged(&mut state, 81);
        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            62,
            79,
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_live_object_packets,
            2
        );
        assert_eq!(
            state
                .inventory_equipment
                .best_client_gui_status_response
                .expect("new response window should replace the completed best response")
                .queued_update_index,
            2
        );
        assert!(
            !state
                .inventory_equipment
                .client_gui_status_response_window_complete()
        );
    }

    #[test]
    fn current_player_status_response_completes_without_diagnostic_candidate_match() {
        let queued_candidate = InventoryItemContextCandidate {
            object_id: 0x8001_64E8,
            proof: InventoryItemObjectProof::ActiveObject,
            source: InventoryItemContextCandidateSource::DirectOnly,
        };
        let mut state = SessionState::default();
        state.inventory_equipment.queued_client_gui_status_outputs = 1;
        state
            .inventory_equipment
            .last_queued_client_gui_status_update_index = Some(1);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 1,
                emission_index: 1,
                event_index: 3,
                candidate: Some(queued_candidate),
                ready_objects: 19,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: true,
                trigger_client_sequence: 81,
                synthetic_sequence: 82,
                ack_sequence: 35,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                live_gui_records: 26,
                live_gui_fragment_bits: 178,
                materialized_item_object_ids: vec![0x8001_64CE, 0x8001_6514],
                compact_item_emission_ready_objects: 43,
                compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_64CE,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
            },
        );

        observe_server_ack_for_client_gui_status(&mut state, 81);
        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            34,
            80,
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_pre_ack_live_object_packets_ignored,
            1
        );
        assert!(
            state
                .inventory_equipment
                .best_client_gui_status_response
                .is_none()
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "awaiting_server_acknowledgement"
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_pre_ack_client_gui_status_live_object_server_ack_sequence,
            Some(81)
        );
        assert!(
            !state
                .inventory_equipment
                .client_gui_status_request_acknowledged()
        );
        observe_server_ack_for_client_gui_status(&mut state, 82);
        assert!(
            state
                .inventory_equipment
                .client_gui_status_request_acknowledged()
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_acknowledged_client_gui_status_server_ack_sequence,
            Some(82)
        );

        // A later/reordered frame can carry an older raw ACK even though the
        // session has already observed ACK 82. That frame cannot own the
        // response to synthetic sequence 82.
        observe_server_ack_for_client_gui_status(&mut state, 81);
        assert!(
            state
                .inventory_equipment
                .client_gui_status_request_acknowledged()
        );
        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            35,
            80,
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_pre_ack_live_object_packets_ignored,
            2
        );
        assert!(
            state
                .inventory_equipment
                .best_client_gui_status_response
                .is_none()
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "awaiting_response"
        );
        observe_server_ack_for_client_gui_status(&mut state, 82);

        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            36,
            80,
        );

        assert_eq!(
            state
                .inventory_equipment
                .best_client_gui_status_response_association()
                .as_str(),
            "differs_from_queued_status_candidate"
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "materialized_current_player_inventory"
        );
        let response = state
            .inventory_equipment
            .best_client_gui_status_response
            .expect("materialized response should be retained");
        assert_eq!(response.server_peer_ack_sequence, 82);
        assert_eq!(response.ack_sequence, 80);
        assert!(
            state
                .inventory_equipment
                .client_gui_status_refresh_confirmed()
        );
        assert!(
            state
                .inventory_equipment
                .client_gui_status_response_window_complete()
        );
        assert!(
            state
                .inventory_equipment
                .pending_confirmed_inventory_replay
                .is_none(),
            "a request-level completion must not relax the candidate-gated Inventory replay"
        );
    }

    #[test]
    fn client_gui_status_server_ack_gate_uses_wrapping_reliable_order() {
        let mut state = SessionState::default();
        state.inventory_equipment.queued_client_gui_status_outputs = 1;
        state
            .inventory_equipment
            .last_queued_client_gui_status_update_index = Some(9);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 9,
                emission_index: 9,
                event_index: 9,
                candidate: None,
                ready_objects: 0,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: true,
                trigger_client_sequence: u16::MAX,
                synthetic_sequence: u16::MAX,
                ack_sequence: 17,
            });

        observe_server_ack_for_client_gui_status(&mut state, u16::MAX - 1);
        assert!(
            !state
                .inventory_equipment
                .client_gui_status_request_acknowledged()
        );

        observe_server_ack_for_client_gui_status(&mut state, 1);
        observe_server_ack_for_client_gui_status(&mut state, 2);
        assert!(
            state
                .inventory_equipment
                .client_gui_status_request_acknowledged()
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_acknowledgements,
            1
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_acknowledged_client_gui_status_server_ack_sequence,
            Some(1)
        );
    }

    #[test]
    fn replays_original_inventory_result_after_status_materializes_claim_object() {
        let candidate = InventoryItemContextCandidate {
            object_id: 0x8001_5322,
            proof: InventoryItemObjectProof::ActiveObject,
            source: InventoryItemContextCandidateSource::DirectOnly,
        };
        let claim =
            InventoryEquipmentServerInventoryClaim::new(0x01, 0x8001_53D3, false, 0x0002_0000);
        let mut update = ready_server_inventory_update();
        update.candidate = candidate;
        update.ready_objects = 18;
        update.server_inventory_claim = Some(claim);

        let mut state = SessionState::default();
        record_output_decision(
            &mut state,
            update,
            InventoryEquipmentBridgeOutputDecisionKind::QueuedClientGuiStatusOutput,
        );
        state.inventory_equipment.queued_client_gui_status_outputs = 1;
        state
            .inventory_equipment
            .last_queued_client_gui_status_update_index = Some(update.update_index);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: update.update_index,
                emission_index: update.emission_index,
                event_index: update.event_index,
                candidate: Some(candidate),
                ready_objects: update.ready_objects,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: true,
                trigger_client_sequence: 80,
                synthetic_sequence: 83,
                ack_sequence: 55,
            });
        state
            .semantic
            .objects
            .observe_materialized_item_object_ids(&[candidate.object_id, claim.object_id]);
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                live_gui_records: 52,
                live_gui_fragment_bits: 355,
                materialized_item_object_ids: vec![candidate.object_id, claim.object_id],
                compact_item_emission_ready_objects: 66,
                compact_item_emission_ready_candidate: Some(candidate),
            },
        );
        mark_current_status_server_acknowledged(&mut state, 83);

        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            60,
            82,
        );
        assert_eq!(
            state.inventory_equipment.pending_confirmed_inventory_replay,
            Some(InventoryEquipmentBridgePendingConfirmedInventoryReplay {
                update_index: update.update_index,
                emission_index: update.emission_index,
                event_index: update.event_index,
                claim,
            })
        );

        assert!(
            maybe_queue_confirmed_inventory_replay(&mut state, 61, 82)
                .expect("confirmed Inventory replay should queue")
        );
        assert_eq!(
            state.inventory_equipment.confirmed_inventory_replay_outputs,
            1
        );
        assert_eq!(state.inventory_equipment.queued_outputs, 1);
        assert_eq!(
            state
                .inventory_equipment
                .last_confirmed_inventory_replay_update_index,
            Some(update.update_index)
        );
        assert_eq!(
            state.inventory_equipment.output_status().as_str(),
            "client_gui_status_inventory_replay_queued"
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_decision
                .expect("replay decision should be retained")
                .kind,
            InventoryEquipmentBridgeOutputDecisionKind::QueuedConfirmedInventoryReplay
        );
        assert_eq!(
            state.inventory_equipment.last_queued_output,
            Some(InventoryEquipmentBridgeQueuedOutput {
                update_index: update.update_index,
                emission_index: update.emission_index,
                event_index: update.event_index,
                minor: claim.minor,
                object_id: claim.object_id,
                result: claim.result,
                equip_slot: claim.equip_slot,
                trigger_sequence: 61,
                synthetic_sequence: 62,
            })
        );
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.server_sequence_shifts[0].base, 62);
        assert_eq!(state.sequence.server_sequence_shifts[0].delta, 1);

        let pending = state
            .synthetic_area
            .pending_server_to_client_packets
            .first()
            .expect("confirmed replay packet should be pending");
        let view = MFrameView::parse(&pending.packet).expect("replay frame should parse");
        assert_eq!(view.sequence, 62);
        assert_eq!(view.ack_sequence, 82);
        let payload = super::super::parse_window::primary_payload(&pending.packet, &view)
            .expect("replay packet should expose exact payload");
        let replay_claim = inventory::claim_payload_if_verified(payload)
            .expect("replayed Inventory payload should pass the exact EE validator");
        assert_eq!(replay_claim.object_id, claim.object_id);
        assert_eq!(replay_claim.result, claim.result);
        assert_eq!(replay_claim.equip_slot, claim.equip_slot);

        assert!(
            !maybe_queue_confirmed_inventory_replay(&mut state, 62, 82)
                .expect("same update must not replay twice")
        );
        assert_eq!(
            state.inventory_equipment.confirmed_inventory_replay_outputs,
            1
        );

        let emit = super::super::take_pending_server_to_client_packets(&mut state);
        assert!(matches!(
            emit,
            crate::translate::Emit::MixedVerifiedProofPacketsPreShifted(_)
        ));
        assert_eq!(
            state
                .inventory_equipment
                .confirmed_inventory_replay_dispatches,
            1
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_confirmed_inventory_replay_dispatch_update_index,
            Some(update.update_index)
        );
        assert!(
            !state
                .inventory_equipment
                .confirmed_inventory_replay_queued_for_dispatch()
        );
        assert_eq!(
            state.inventory_equipment.output_status().as_str(),
            "client_gui_status_inventory_replay_dispatched"
        );
    }

    #[test]
    fn client_gui_status_best_response_survives_generic_followup() {
        let mut state = SessionState::default();
        state.inventory_equipment.queued_client_gui_status_outputs = 1;
        state
            .inventory_equipment
            .last_queued_client_gui_status_update_index = Some(20);
        state
            .inventory_equipment
            .last_queued_client_gui_status_output =
            Some(InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
                update_index: 20,
                emission_index: 20,
                event_index: 20,
                candidate: None,
                ready_objects: 0,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                player_inventory_gui: false,
                trigger_client_sequence: 80,
                synthetic_sequence: 81,
                ack_sequence: 40,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                live_gui_records: 9,
                live_gui_fragment_bits: 72,
                materialized_item_object_ids: vec![0x8001_5211, 0x8001_5212],
                compact_item_emission_ready_objects: 66,
                compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_5211,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
            },
        );
        mark_current_status_server_acknowledged(&mut state, 81);

        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            90,
            81,
        );

        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                live_gui_records: 0,
                live_gui_fragment_bits: 0,
                materialized_item_object_ids: Vec::new(),
                compact_item_emission_ready_objects: 66,
                compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8001_5211,
                    proof: InventoryItemObjectProof::ActiveObject,
                    source: InventoryItemContextCandidateSource::DirectOnly,
                }),
            },
        );

        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            105,
            81,
        );

        let last = state
            .inventory_equipment
            .last_client_gui_status_response
            .expect("latest response should be retained");
        assert_eq!(last.server_sequence, 105);
        assert_eq!(last.live_gui_records, 0);
        assert_eq!(last.materialized_item_object_ids, 0);
        assert_eq!(last.materialized_item_object_id_first, 0);
        assert_eq!(last.materialized_item_object_id_last, 0);
        assert_eq!(last.materialized_item_object_id_min, 0);
        assert_eq!(last.materialized_item_object_id_max, 0);
        assert!(!last.materialized_item_object_ids_contain_queued_candidate);

        let best = state
            .inventory_equipment
            .best_client_gui_status_response
            .expect("best response should be retained");
        assert_eq!(best.server_sequence, 90);
        assert_eq!(best.live_gui_records, 9);
        assert_eq!(best.live_gui_fragment_bits, 72);
        assert_eq!(best.materialized_item_object_ids, 2);
        assert_eq!(best.materialized_item_object_id_first, 0x8001_5211);
        assert_eq!(best.materialized_item_object_id_last, 0x8001_5212);
        assert_eq!(best.materialized_item_object_id_min, 0x8001_5211);
        assert_eq!(best.materialized_item_object_id_max, 0x8001_5212);
        assert_eq!(
            best.compact_item_emission_ready_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8001_5211,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            })
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_outcome()
                .as_str(),
            "materialized_items"
        );
    }

    #[test]
    fn records_client_gui_writer_gap_without_server_inventory_trigger() {
        let mut update = ready_server_inventory_update();
        update.consumer = ClientGuiInventory;
        update.server_inventory_claim = None;
        update.client_gui_inventory_claim = Some(InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::SelectPanel,
            object_id: None,
            panel: Some(3),
            player_inventory_gui: Some(true),
            rewritten_self_object_id: false,
        });
        let mut state = SessionState::default();
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_record_non_server_inventory_equipment_bridge_output_decision(&mut state);
        maybe_record_non_server_inventory_equipment_bridge_output_decision(&mut state);

        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert!(state.sequence.server_sequence_shifts.is_empty());
        assert_eq!(
            state.inventory_equipment.last_decision_state_update_index,
            Some(1)
        );
        let decision = state
            .inventory_equipment
            .last_decision
            .expect("client GUI writer-gap decision should be recorded");
        assert_eq!(
            decision.kind,
            InventoryEquipmentBridgeOutputDecisionKind::DeferredClientGui
        );
        assert_eq!(
            decision.consumer,
            InventoryEquipmentHandoffConsumer::ClientGuiInventory
        );
        assert_eq!(
            decision
                .client_gui_inventory_claim
                .expect("client GUI writer-gap decision should retain exact claim")
                .panel,
            Some(3)
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_deferred_client_gui_update_index,
            Some(1)
        );
        assert_eq!(state.inventory_equipment.deferred_client_gui_updates, 1);
        assert_eq!(state.inventory_equipment.queued_outputs, 0);
    }

    #[test]
    fn handles_missing_server_inventory_claim_once_per_state_update() {
        let mut update = ready_server_inventory_update();
        update.server_inventory_claim = None;
        let mut state = SessionState::default();
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_queue_inventory_equipment_bridge_output(&mut state, 10, 77)
            .expect("missing claim should defer without error");
        maybe_queue_inventory_equipment_bridge_output(&mut state, 11, 77)
            .expect("same missing-claim update should remain handled");

        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert_eq!(
            state.inventory_equipment.last_decision_state_update_index,
            Some(1)
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_decision
                .expect("decision should be recorded")
                .kind,
            InventoryEquipmentBridgeOutputDecisionKind::DeferredMissingClaim
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_deferred_missing_claim_update_index,
            Some(1)
        );
        assert_eq!(state.inventory_equipment.deferred_missing_claim_updates, 1);
        assert_eq!(state.inventory_equipment.queued_outputs, 0);
    }

    #[test]
    fn handles_candidate_mismatch_once_per_state_update() {
        let mut update = ready_server_inventory_update();
        update.server_inventory_claim = Some(InventoryEquipmentServerInventoryClaim::new(
            0x01,
            0x8000_5678,
            true,
            4,
        ));
        let mut state = SessionState::default();
        state
            .semantic
            .objects
            .observe_materialized_item_object_ids(&[0x8000_5600, 0x8000_5800]);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_queue_inventory_equipment_bridge_output(&mut state, 10, 77)
            .expect("mismatch should block without error");
        maybe_queue_inventory_equipment_bridge_output(&mut state, 11, 77)
            .expect("same mismatch update should remain handled");

        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert_eq!(
            state.inventory_equipment.last_decision_state_update_index,
            Some(1)
        );
        let decision = state
            .inventory_equipment
            .last_decision
            .expect("decision should be recorded");
        assert_eq!(
            decision.kind,
            InventoryEquipmentBridgeOutputDecisionKind::BlockedCandidateMismatch
        );
        assert_eq!(decision.candidate.object_id, 0x8000_1234);
        assert_eq!(
            decision
                .server_inventory_claim
                .expect("mismatch decision should retain claim")
                .object_id,
            0x8000_5678
        );
        assert_eq!(
            decision.server_inventory_claim_proven_neighborhood.lower,
            Some(InventoryItemObjectProvenNeighbor {
                object_id: 0x8000_5600,
                distance: 0x78,
            })
        );
        assert_eq!(
            decision.server_inventory_claim_proven_neighborhood.higher,
            Some(InventoryItemObjectProvenNeighbor {
                object_id: 0x8000_5800,
                distance: 0x188,
            })
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_blocked_candidate_mismatch_update_index,
            Some(1)
        );
        assert_eq!(
            state.inventory_equipment.blocked_candidate_mismatch_updates,
            1
        );
        assert_eq!(state.inventory_equipment.queued_outputs, 0);
    }

    #[test]
    fn queues_client_gui_status_for_unknown_server_inventory_claim_mismatch() {
        let mut update = ready_server_inventory_update();
        update.server_inventory_claim = Some(InventoryEquipmentServerInventoryClaim::new(
            0x01,
            0x8000_5678,
            false,
            0x0002_0000,
        ));
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(30);
        state
            .semantic
            .objects
            .observe_materialized_item_object_ids(&[0x8000_5600]);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_queue_inventory_equipment_bridge_output(&mut state, 90, 77)
            .expect("unknown server claim should queue a ClientGui status request");
        maybe_queue_inventory_equipment_bridge_output(&mut state, 91, 77)
            .expect("same mismatch update should remain handled");

        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.sequence.client_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.client_sequence_shifts[0].base, 31);
        assert_eq!(state.sequence.client_sequence_shifts[0].delta, 1);
        assert_eq!(
            state.inventory_equipment.last_decision_state_update_index,
            Some(1)
        );
        let decision = state
            .inventory_equipment
            .last_decision
            .expect("decision should be recorded");
        assert_eq!(
            decision.kind,
            InventoryEquipmentBridgeOutputDecisionKind::QueuedClientGuiStatusOutput
        );
        assert_eq!(
            decision.consumer,
            InventoryEquipmentHandoffConsumer::ServerInventory
        );
        assert_eq!(decision.candidate.object_id, 0x8000_1234);
        assert_eq!(
            decision
                .server_inventory_claim
                .expect("decision should retain unknown server claim")
                .object_id,
            0x8000_5678
        );
        assert_eq!(
            decision.server_inventory_claim_object_status,
            InventoryItemObjectStatus::Unknown
        );
        let client_gui_claim = decision
            .client_gui_inventory_claim
            .expect("fallback decision should retain synthetic ClientGui status claim");
        assert_eq!(
            client_gui_claim.kind,
            InventoryEquipmentClientGuiInventoryClaimKind::Status
        );
        assert_eq!(
            client_gui_claim.object_id,
            Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID)
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_blocked_candidate_mismatch_update_index,
            None
        );
        assert_eq!(
            state.inventory_equipment.blocked_candidate_mismatch_updates,
            0
        );
        assert_eq!(state.inventory_equipment.queued_outputs, 0);
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            1
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_queued_client_gui_status_output
                .expect("ClientGui status output should be queued")
                .candidate
                .expect("queued status should preserve ready candidate")
                .object_id,
            0x8000_1234
        );

        let pending = &state.sequence.pending_client_to_server_packets[0];
        let view = MFrameView::parse(pending).expect("queued client packet should parse");
        assert_eq!(view.sequence, 31);
        assert_eq!(view.ack_sequence, 90);
        let payload = super::super::parse_window::primary_payload(pending, &view)
            .expect("queued packet should expose primary payload");
        let claim = client_gui_inventory::claim_payload_if_verified(payload)
            .expect("queued ClientGuiInventory payload should be exact");
        assert_eq!(
            claim.object_id,
            Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID)
        );
    }

    #[test]
    fn queues_inventory_output_when_mismatch_claim_object_has_known_item_state() {
        let mut update = ready_server_inventory_update();
        update.server_inventory_claim = Some(InventoryEquipmentServerInventoryClaim::new(
            0x01,
            0x8000_5678,
            false,
            0x0002_0000,
        ));
        let mut state = SessionState::default();
        state
            .semantic
            .objects
            .observe_materialized_item_object_ids(&[0x8000_5678]);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_queue_inventory_equipment_bridge_output(&mut state, 10, 77)
            .expect("known claim item should queue exact Inventory output");

        let decision = state
            .inventory_equipment
            .last_decision
            .expect("decision should be recorded");
        assert_eq!(
            decision.kind,
            InventoryEquipmentBridgeOutputDecisionKind::QueuedInventoryOutput
        );
        assert_eq!(decision.candidate.object_id, 0x8000_1234);
        assert_eq!(
            decision
                .server_inventory_claim
                .expect("queued decision should retain claim")
                .object_id,
            0x8000_5678
        );
        assert_eq!(
            decision.server_inventory_claim_object_status,
            InventoryItemObjectStatus::Proven(InventoryItemObjectProof::ActiveObject)
        );
        assert_eq!(
            state.inventory_equipment.blocked_candidate_mismatch_updates,
            0
        );
        assert_eq!(state.inventory_equipment.queued_outputs, 1);
        assert_eq!(
            state
                .inventory_equipment
                .last_queued_output
                .expect("known claim item should be queued")
                .object_id,
            0x8000_5678
        );

        let pending = &state.synthetic_area.pending_server_to_client_packets[0];
        let view = MFrameView::parse(&pending.packet).expect("queued packet should parse");
        let payload = super::super::parse_window::primary_payload(&pending.packet, &view)
            .expect("queued packet should expose primary payload");
        let claim = inventory::claim_payload_if_verified(payload)
            .expect("queued Inventory payload should be exact EE shape");
        assert_eq!(claim.object_id, 0x8000_5678);
        assert!(!claim.result);
        assert_eq!(claim.equip_slot, 0x0002_0000);
    }
}
