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

#[cfg(test)]
use super::output_reliability;
use super::{
    sequence::{
        SequenceShift, ServerSequenceInsertionProducer, record_server_sequence_insertion,
        sequence_at_or_after, shift_sequence_for_peer, trim_sequence_shifts,
    },
    state::{
        InventoryEquipmentBridgeClientGuiStatusResponse, InventoryEquipmentBridgeOutputDecision,
        InventoryEquipmentBridgeOutputDecisionKind,
        InventoryEquipmentBridgePendingConfirmedInventoryReplay,
        InventoryEquipmentBridgeQueuedClientGuiStatusOutput, InventoryEquipmentBridgeQueuedOutput,
        InventoryEquipmentCurrentPlayerStatusBinding, PendingClientPacket, SessionState,
    },
    synthetic_area::{
        self, PendingServerInsertionSequence, PendingServerPacket, PendingServerPacketPlacement,
    },
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
        forwarded_request = state
            .inventory_equipment
            .client_gui_status_request_is_forwarded(),
        "inventory/equipment bridge observed legacy server ACK for tracked ClientGuiInventory_Status request"
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
        maybe_queue_client_gui_status_output(state, update, Some(trigger_sequence), None)?;
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
    if claim.native_object_was_proven {
        // The exact native packet is emitted before any AfterCurrentEmit
        // insertion. If its item object was already materialized when the
        // reducer observed it, the EE client can consume that original outcome
        // directly and a synthetic copy would repeat the state transition.
        // Claims proven only later deliberately remain on the materialize and
        // replay path because their first native outcome may have been ignored.
        record_output_decision(
            state,
            update,
            InventoryEquipmentBridgeOutputDecisionKind::NativeInventoryOutcomeSufficient,
        );
        tracing::info!(
            update_index = update.update_index,
            emission_index = update.emission_index,
            event_index = update.event_index,
            claim_object_id = %format_args!("0x{:08X}", claim.object_id),
            claim_object_status = claim_object_status.as_str(),
            claim_object_proof = claim_object_status
                .proof()
                .map(|proof| proof.as_str())
                .unwrap_or("none"),
            candidate_object_id = %format_args!("0x{:08X}", update.candidate.object_id),
            minor = claim.minor,
            equip_slot = claim.equip_slot,
            alternate_inventory_context = claim.alternate_inventory_context,
            "inventory/equipment bridge retained sufficient native Inventory outcome without a synthetic duplicate"
        );
        return Ok(());
    }
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
        claim.alternate_inventory_context,
        claim.equip_slot,
    )
    .ok_or_else(|| {
        anyhow::anyhow!("drained inventory/equipment update did not build exact Inventory payload")
    })?;
    let future_shift_base = trigger_sequence.wrapping_add(1);
    let producer = ServerSequenceInsertionProducer::InventoryEquipment {
        update_index: update.update_index,
        trigger_sequence,
    };
    record_server_sequence_insertion(
        &mut state.sequence.pending_server_sequence_insertions,
        producer,
        future_shift_base,
        INVENTORY_EQUIPMENT_BRIDGE_INSERTED_FRAME_COUNT,
    )?;
    let insertion_owner = state.sequence.current_server_insertion_owner(producer)?;
    let prospective_epochs = state
        .sequence
        .prospective_ordered_server_sequence_epochs()?;
    let synthetic_sequence = prospective_epochs
        .insertion_range(insertion_owner)
        .ok_or_else(|| anyhow::anyhow!("inventory/equipment insertion range is absent"))?
        .destination_first
        .sequence;
    let packet =
        synthetic_area::build_synthetic_gameplay_frame(synthetic_sequence, ack_sequence, &payload)?;
    state
        .synthetic_area
        .pending_server_to_client_packets
        .push(PendingServerPacket {
            family: VerifiedFamily::Inventory,
            packet,
            insertion_sequence: Some(PendingServerInsertionSequence {
                owner: insertion_owner,
                offset: 0,
            }),
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
        alternate_inventory_context: claim.alternate_inventory_context,
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
        alternate_inventory_context = claim.alternate_inventory_context,
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
    forwarded_client_frame: Option<(u16, u16, InventoryEquipmentClientGuiInventoryClaim)>,
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
        && let Err(err) =
            maybe_queue_client_gui_status_output(state, update, None, forwarded_client_frame)
    {
        tracing::warn!(
            error = %err,
            update_index = update.update_index,
            "failed to queue inventory/equipment ClientGuiInventory bridge output"
        );
    }
}

pub(super) fn maybe_record_client_gui_status_live_object_frame_response(
    state: &mut SessionState,
    proof: &VerifiedProof,
    server_sequence: u16,
    server_peer_ack_sequence: u16,
    ack_sequence: u16,
    frame_materialization: Option<
        &crate::translate::semantic::LiveObjectInventoryMaterializationSummary,
    >,
    current_controlled_object_id_at_observation: Option<u32>,
) -> bool {
    if state
        .inventory_equipment
        .last_queued_client_gui_status_output
        .is_none()
        || !proof.contains_family(VerifiedFamily::GameObjUpdateLiveObject)
        || frame_materialization.is_none()
        || state
            .inventory_equipment
            .client_gui_status_response_window_complete()
    {
        return false;
    }
    let Some(queued_request_sequence) = state
        .inventory_equipment
        .last_queued_client_gui_status_output
        .map(|queued| queued.synthetic_sequence)
    else {
        return false;
    };
    // The transport caller supplies this frame's raw peer ACK explicitly,
    // before sequence unshifting hides proxy-owned client intervals from EE.
    // Historical acknowledgement is not sufficient: a reordered or
    // retransmitted frame whose own ACK precedes the synthetic request cannot
    // be its response materialization.
    let current_packet_acknowledges_request =
        sequence_at_or_after(server_peer_ack_sequence, queued_request_sequence);
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
            .last_pre_ack_client_gui_status_live_object_server_ack_sequence =
            Some(server_peer_ack_sequence);
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
        return false;
    }
    let queued_candidate = state
        .inventory_equipment
        .last_queued_client_gui_status_output
        .and_then(|queued| queued.candidate);
    // Raw peer ACK provenance is frame-local. Carry the typed semantic summary
    // produced while reducing this exact server frame as part of the same
    // context, rather than proving only a boolean and then reading mutable
    // session history. A later frame cannot substitute its summary under this
    // frame's ACK.
    let Some(summary) = frame_materialization.cloned() else {
        return false;
    };
    let forwarded_request = state
        .inventory_equipment
        .client_gui_status_request_is_forwarded();
    let current_player_object_id = state
        .inventory_equipment
        .last_queued_client_gui_status_output
        .and_then(|queued| {
            if queued.object_id == client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID {
                // Diamond resolves the sentinel dynamically when the handler
                // consumes it. Bind it to ObjControl at this exact live-object
                // unit, not request time or the end of a multi-unit frame.
                current_controlled_object_id_at_observation
            } else {
                // A concrete Status names one exact creature. Preserve the
                // request-time ObjControl proof even if control changes before
                // its response arrives.
                queued
                    .resolved_current_player_object_id
                    .filter(|resolved| *resolved == queued.object_id)
            }
        });
    let current_player_inventory_claims: Vec<_> = current_player_object_id
        .map(|object_id| {
            summary
                .inventory_owner_claims
                .iter()
                .copied()
                .filter(|claim| claim.owner_id == object_id)
                .collect()
        })
        .unwrap_or_default();
    let current_player_inventory_records =
        u32::try_from(current_player_inventory_claims.len()).unwrap_or(u32::MAX);
    let first_current_player_inventory_mask = current_player_inventory_claims
        .first()
        .map(|claim| claim.mask);
    let current_player_inventory_mask_union = current_player_inventory_claims
        .iter()
        .fold(0u16, |mask, claim| mask | claim.mask);
    let typed_response_relevant = if forwarded_request {
        summary.inventory_records != 0
    } else {
        summary.inventory_records != 0
            || summary.live_gui_records != 0
            || !summary.materialized_item_object_ids.is_empty()
    };
    if !typed_response_relevant {
        state
            .inventory_equipment
            .client_gui_status_non_inventory_live_object_packets_ignored = state
            .inventory_equipment
            .client_gui_status_non_inventory_live_object_packets_ignored
            .saturating_add(1);
        tracing::debug!(
            queued_update_index = state
                .inventory_equipment
                .last_queued_client_gui_status_update_index
                .unwrap_or(0),
            server_sequence,
            server_peer_ack_sequence,
            forwarded_request,
            "inventory/equipment bridge ignored non-inventory live-object frame outside the tracked ClientGuiInventory_Status response"
        );
        return false;
    }
    let queued_update_index = state
        .inventory_equipment
        .last_queued_client_gui_status_update_index
        .unwrap_or(0);
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
    if summary.inventory_records != 0 {
        state
            .inventory_equipment
            .client_gui_status_response_inventory_record_packets = state
            .inventory_equipment
            .client_gui_status_response_inventory_record_packets
            .saturating_add(1);
    }
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
        inventory_records: summary.inventory_records,
        inventory_owner_claims: u32::try_from(summary.inventory_owner_claims.len())
            .unwrap_or(u32::MAX),
        current_player_inventory_records,
        first_current_player_inventory_mask,
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
        .client_gui_status_response_window_satisfied()
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
            current_player_object_id = current_player_object_id
                .map(|object_id| format!("0x{object_id:08X}"))
                .unwrap_or_else(|| "none".to_string()),
            current_player_inventory_records,
            first_current_player_inventory_mask = first_current_player_inventory_mask
                .map(|mask| format!("0x{mask:04X}"))
                .unwrap_or_else(|| "none".to_string()),
            current_player_inventory_mask_union =
                %format_args!("0x{:04X}", current_player_inventory_mask_union),
            forwarded_request = state
                .inventory_equipment
                .client_gui_status_request_is_forwarded(),
            "inventory/equipment bridge completed tracked ClientGuiInventory_Status response window"
        );
    }
    tracing::info!(
        queued_update_index,
        server_sequence,
        server_peer_ack_sequence,
        ack_sequence,
        inventory_records = summary.inventory_records,
        inventory_owner_claims = summary.inventory_owner_claims.len(),
        current_player_object_id = current_player_object_id
            .map(|object_id| format!("0x{object_id:08X}"))
            .unwrap_or_else(|| "none".to_string()),
        current_player_inventory_records,
        first_current_player_inventory_mask = first_current_player_inventory_mask
            .map(|mask| format!("0x{mask:04X}"))
            .unwrap_or_else(|| "none".to_string()),
        current_player_inventory_mask_union =
            %format_args!("0x{:04X}", current_player_inventory_mask_union),
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
        "inventory/equipment bridge observed typed server response to tracked ClientGuiInventory_Status"
    );
    true
}

pub(super) fn maybe_bind_current_player_status_response_authority(
    state: &mut SessionState,
    server_sequence: u16,
    observation: &crate::translate::semantic::LiveObjectInventoryMaterializationObservation,
) {
    // Use the same request identity rule as the response recorder. A concrete
    // Status remains bound to its request-time controlled object; only the
    // Diamond current-player sentinel resolves dynamically at this exact
    // observation. Count equality alone is not identity proof because one
    // live-object unit may carry rows for multiple owners.
    let Some(owner_object_id) = state
        .inventory_equipment
        .last_queued_client_gui_status_output
        .and_then(|queued| {
            if queued.object_id == client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID {
                observation.current_controlled_object_id
            } else {
                queued
                    .resolved_current_player_object_id
                    .filter(|resolved| *resolved == queued.object_id)
            }
        })
    else {
        return;
    };
    let Some(response) = state.inventory_equipment.last_client_gui_status_response else {
        return;
    };
    if response.server_sequence != server_sequence
        || response.current_player_inventory_records == 0
        || !state
            .inventory_equipment
            .client_gui_status_response_window_satisfied()
    {
        return;
    }
    let owner_claims: Vec<_> = observation
        .summary
        .inventory_owner_claims
        .iter()
        .copied()
        .filter(|claim| claim.owner_id == owner_object_id)
        .collect();
    let owner_record_count = u32::try_from(owner_claims.len()).unwrap_or(u32::MAX);
    if owner_record_count != response.current_player_inventory_records {
        tracing::warn!(
            server_sequence,
            response_current_player_records = response.current_player_inventory_records,
            exact_unit_owner_records = owner_record_count,
            "current-player Status authority did not match its exact live-object unit"
        );
        return;
    }
    let owner_mask_union = owner_claims
        .iter()
        .fold(0u16, |mask, claim| mask | claim.mask);
    state.inventory_equipment.current_player_status_binding =
        Some(InventoryEquipmentCurrentPlayerStatusBinding {
            queued_update_index: response.queued_update_index,
            area_client_area_packets: observation.area_client_area_packets,
            control_epoch: observation.control_epoch,
            server_sequence,
            owner_object_id,
            owner_record_count,
            owner_mask_union,
        });
    tracing::info!(
        queued_update_index = response.queued_update_index,
        server_sequence,
        area_client_area_packets = observation.area_client_area_packets,
        control_epoch = observation.control_epoch,
        owner_object_id = %format_args!("0x{:08X}", owner_object_id),
        owner_record_count,
        owner_mask_union = %format_args!("0x{:04X}", owner_mask_union),
        "inventory/equipment bridge bound exact current-player Status authority"
    );
}

#[cfg(test)]
fn maybe_record_client_gui_status_live_object_response(
    state: &mut SessionState,
    proof: &VerifiedProof,
    server_sequence: u16,
    server_peer_ack_sequence: u16,
    ack_sequence: u16,
    observed_live_object_inventory_materialization: bool,
) {
    let frame_materialization = observed_live_object_inventory_materialization
        .then(|| {
            state
                .semantic
                .ui
                .last_live_object_inventory_materialization
                .clone()
        })
        .flatten();
    let current_controlled_object_id_at_observation =
        state.semantic.player_control.current_controlled_object_id;
    maybe_record_client_gui_status_live_object_frame_response(
        state,
        proof,
        server_sequence,
        server_peer_ack_sequence,
        ack_sequence,
        frame_materialization.as_ref(),
        current_controlled_object_id_at_observation,
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
        claim_alternate_inventory_context = claim.alternate_inventory_context,
        claim_equip_slot = claim.equip_slot,
        "inventory/equipment bridge staged original Inventory context after associated ClientGui status materialized its claim object"
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
    // buffer, followed by the single MSB-owned inventory-context BOOL in the fragment
    // stream. Reusing its exact validator here prevents a materialization
    // timing repair from becoming a second, weaker packet writer.
    let claim = pending.claim;
    let payload = inventory::build_ee_inventory_payload(
        claim.minor,
        claim.object_id,
        claim.alternate_inventory_context,
        claim.equip_slot,
    )
    .ok_or_else(|| {
        anyhow::anyhow!("confirmed ClientGui status replay did not build exact Inventory payload")
    })?;
    let future_shift_base = response_last_sequence.wrapping_add(1);
    let producer = ServerSequenceInsertionProducer::ConfirmedInventoryReplay {
        update_index: pending.update_index,
        response_last_sequence,
    };
    record_server_sequence_insertion(
        &mut state.sequence.pending_server_sequence_insertions,
        producer,
        future_shift_base,
        INVENTORY_EQUIPMENT_BRIDGE_INSERTED_FRAME_COUNT,
    )?;
    let insertion_owner = state.sequence.current_server_insertion_owner(producer)?;
    let prospective_epochs = state
        .sequence
        .prospective_ordered_server_sequence_epochs()?;
    let synthetic_sequence = prospective_epochs
        .insertion_range(insertion_owner)
        .ok_or_else(|| anyhow::anyhow!("confirmed inventory replay insertion range is absent"))?
        .destination_first
        .sequence;
    let packet =
        synthetic_area::build_synthetic_gameplay_frame(synthetic_sequence, ack_sequence, &payload)?;
    state
        .synthetic_area
        .pending_server_to_client_packets
        .push(PendingServerPacket {
            family: VerifiedFamily::Inventory,
            packet,
            insertion_sequence: Some(PendingServerInsertionSequence {
                owner: insertion_owner,
                offset: 0,
            }),
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
        alternate_inventory_context: claim.alternate_inventory_context,
        equip_slot: claim.equip_slot,
        trigger_sequence: response_last_sequence,
        synthetic_sequence,
    });

    tracing::info!(
        update_index = pending.update_index,
        object_id = %format_args!("0x{:08X}", claim.object_id),
        equip_slot = claim.equip_slot,
        alternate_inventory_context = claim.alternate_inventory_context,
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
    forwarded_client_frame: Option<(u16, u16, InventoryEquipmentClientGuiInventoryClaim)>,
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

    let resolved_current_player_object_id = current_player_status_object_id(state, object_id);
    if object_id != client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID
        && resolved_current_player_object_id != Some(object_id)
    {
        record_deferred_client_gui_output_decision(
            state,
            update,
            "inventory/equipment bridge output deferred: ClientGui status object does not match exact ObjControl authority",
        );
        return Ok(true);
    }

    let Some(player_inventory_gui) = claim.player_inventory_gui else {
        record_deferred_client_gui_output_decision(
            state,
            update,
            "inventory/equipment bridge output deferred: exact ClientGui status lacks its BOOL",
        );
        return Ok(true);
    };

    if let Some((forwarded_sequence, forwarded_ack_sequence, forwarded_claim)) =
        forwarded_client_frame
    {
        if forwarded_claim != claim {
            record_deferred_client_gui_output_decision(
                state,
                update,
                "inventory/equipment bridge output deferred: forwarded ClientGui status does not match the current semantic update",
            );
            return Ok(true);
        }
        return adopt_forwarded_client_gui_status_request(
            state,
            update,
            claim,
            player_inventory_gui,
            forwarded_sequence,
            forwarded_ack_sequence,
        );
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

fn current_player_status_object_id(state: &SessionState, object_id: u32) -> Option<u32> {
    let controlled_object_id = state.semantic.player_control.current_controlled_object_id;
    if object_id == client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID {
        // The legacy sentinel remains a valid self request even if the
        // ObjControl packet has not arrived yet. Once authority exists, retain
        // its concrete object id so an exact inventory-owner response can be
        // correlated without treating the sentinel itself as an owner.
        return controlled_object_id;
    }
    (controlled_object_id == Some(object_id)).then_some(object_id)
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

fn adopt_forwarded_client_gui_status_request(
    state: &mut SessionState,
    update: crate::translate::semantic::InventoryEquipmentBridgeStateUpdate,
    claim: InventoryEquipmentClientGuiInventoryClaim,
    player_inventory_gui: bool,
    forwarded_sequence: u16,
    forwarded_ack_sequence: u16,
) -> anyhow::Result<bool> {
    let object_id = claim
        .object_id
        .ok_or_else(|| anyhow::anyhow!("forwarded ClientGuiInventory_Status lacks object id"))?;
    let resolved_current_player_object_id = current_player_status_object_id(state, object_id);
    if object_id != client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID
        && resolved_current_player_object_id != Some(object_id)
    {
        return Err(anyhow::anyhow!(
            "forwarded ClientGuiInventory_Status does not match exact ObjControl authority"
        ));
    }

    // The exact client translator has already rewritten EE self to Diamond's
    // current-player sentinel and the outgoing M frame already owns a real
    // reliable sequence. Treat that forwarded writer result as the request
    // boundary. Emitting the same BOOL+OBJECTID body again would duplicate
    // SetOpen and can invert a close request if the final MSB-first BOOL is
    // discarded.
    state
        .inventory_equipment
        .begin_client_gui_status_request_window();
    record_output_decision(
        state,
        update,
        InventoryEquipmentBridgeOutputDecisionKind::ForwardedClientGuiStatusRequest,
    );
    state
        .inventory_equipment
        .last_queued_client_gui_status_update_index = Some(update.update_index);
    state
        .inventory_equipment
        .last_forwarded_client_gui_status_update_index = Some(update.update_index);
    state
        .inventory_equipment
        .forwarded_client_gui_status_requests = state
        .inventory_equipment
        .forwarded_client_gui_status_requests
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
            resolved_current_player_object_id,
            player_inventory_gui,
            trigger_client_sequence: forwarded_sequence,
            synthetic_sequence: forwarded_sequence,
            ack_sequence: forwarded_ack_sequence,
        });
    if !player_inventory_gui {
        state
            .inventory_equipment
            .last_completed_client_gui_status_response_update_index = Some(update.update_index);
    }

    tracing::info!(
        update_index = update.update_index,
        emission_index = update.emission_index,
        event_index = update.event_index,
        object_id = %format_args!("0x{:08X}", object_id),
        resolved_current_player_object_id = resolved_current_player_object_id
            .map(|object_id| format!("0x{object_id:08X}"))
            .unwrap_or_else(|| "none".to_string()),
        player_inventory_gui,
        forwarded_sequence,
        forwarded_ack_sequence,
        pending_client_packets = state.sequence.pending_client_to_server_packets.len(),
        "inventory/equipment bridge adopted forwarded ClientGuiInventory_Status request without a duplicate"
    );

    Ok(true)
}

fn queue_client_gui_status_output_with_claim(
    state: &mut SessionState,
    update: crate::translate::semantic::InventoryEquipmentBridgeStateUpdate,
    claim: InventoryEquipmentClientGuiInventoryClaim,
    latest_client_sequence: u16,
    server_sequence_to_ack: Option<u16>,
) -> anyhow::Result<bool> {
    let player_inventory_gui = claim.player_inventory_gui.ok_or_else(|| {
        anyhow::anyhow!("ClientGuiInventory_Status output claim lacks its exact BOOL")
    })?;
    let object_id = claim
        .object_id
        .ok_or_else(|| anyhow::anyhow!("ClientGuiInventory_Status output claim lacks object id"))?;
    let resolved_current_player_object_id = current_player_status_object_id(state, object_id);
    if object_id != client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID
        && resolved_current_player_object_id != Some(object_id)
    {
        return Err(anyhow::anyhow!(
            "ClientGuiInventory_Status output claim does not match exact ObjControl authority"
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
    // Both candidates below are EE-facing destination sequences. Convert the
    // selected destination ACK as one value so the latest emitted fallback
    // cannot leak an inserted EE sequence into the Diamond ACK field. An
    // explicit `server_sequence_to_ack` is already source-domain state.
    let destination_ack = state
        .sequence
        .latest_client_ack_from_client
        .or(state.sequence.latest_server_sequence_to_client);
    let mapped_destination_ack = destination_ack
        .map(|ack| super::map_client_destination_ack_to_server_source(state, ack))
        .transpose()?;
    let ack_sequence = server_sequence_to_ack
        .or(mapped_destination_ack)
        .unwrap_or(u16::MAX);
    let packet =
        synthetic_area::build_synthetic_gameplay_frame(synthetic_sequence, ack_sequence, &payload)?;

    state
        .inventory_equipment
        .begin_client_gui_status_request_window();
    state
        .sequence
        .pending_client_to_server_packets
        .push(PendingClientPacket {
            family: VerifiedFamily::ClientGuiInventory,
            packet,
            reason: "inventory/equipment ClientGuiInventory_Status bridge output",
        });
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
            resolved_current_player_object_id,
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

    fn session_state_with_server_source(sequence: u16) -> SessionState {
        let mut state = SessionState::default();
        state.sequence.current_server_translation_source =
            Some(super::super::server_replay::ServerReliableSlotKey {
                sequence,
                origin_generation: 0,
            });
        state
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
    fn retains_native_inventory_outcome_when_object_was_already_proven() {
        for minor in [0x01, 0x02] {
            let mut update = ready_server_inventory_update();
            update.server_inventory_claim = Some(
                InventoryEquipmentServerInventoryClaim::new(minor, 0x8000_1234, true, 4)
                    .with_native_object_was_proven(true),
            );
            let mut state = session_state_with_server_source(10);
            state
                .semantic
                .objects
                .observe_materialized_item_object_ids(&[0x8000_1234]);
            state
                .semantic
                .ui
                .last_inventory_equipment_bridge_handoff_state_update = Some(update);

            maybe_queue_inventory_equipment_bridge_output(&mut state, 10, 77)
                .expect("the exact native Inventory outcome should be sufficient");

            assert!(
                state
                    .synthetic_area
                    .pending_server_to_client_packets
                    .is_empty()
            );
            assert!(state.sequence.pending_server_sequence_insertions.is_empty());
            assert_eq!(state.inventory_equipment.queued_outputs, 0);
            assert_eq!(state.inventory_equipment.last_queued_output, None);
            let decision = state
                .inventory_equipment
                .last_decision
                .expect("native-outcome decision should be recorded");
            assert_eq!(
                decision.kind,
                InventoryEquipmentBridgeOutputDecisionKind::NativeInventoryOutcomeSufficient
            );
            assert_eq!(
                decision
                    .server_inventory_claim
                    .expect("decision should preserve the native claim")
                    .minor,
                minor
            );
            assert_eq!(
                decision.server_inventory_claim_object_status,
                InventoryItemObjectStatus::Proven(InventoryItemObjectProof::ActiveObject)
            );
            assert_eq!(
                state.inventory_equipment.output_status(),
                super::super::state::InventoryEquipmentBridgeOutputStatus::NativeInventoryOutcomeSufficient
            );
        }
    }

    #[test]
    fn queues_exact_inventory_output_after_server_inventory_state_update() {
        let mut state = session_state_with_server_source(10);
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
                alternate_inventory_context: true,
                equip_slot: 4,
                trigger_sequence: 10,
                synthetic_sequence: 11,
            })
        );
        assert_eq!(
            state.synthetic_area.pending_server_to_client_packets.len(),
            1
        );
        assert_eq!(state.sequence.pending_server_sequence_insertions.len(), 1);

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
        assert!(claim.alternate_inventory_context);
        assert_eq!(claim.shape.equip_slot(), Some(4));
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
                resolved_current_player_object_id: None,
                player_inventory_gui: true,
                trigger_client_sequence: 11,
                synthetic_sequence: 11,
                ack_sequence: 10,
            })
        );

        let pending = state.sequence.pending_client_to_server_packets.remove(0);
        assert_eq!(pending.family, VerifiedFamily::ClientGuiInventory);
        let view = MFrameView::parse(&pending.packet).expect("queued client packet should parse");
        assert_eq!(view.sequence, 11);
        assert_eq!(view.ack_sequence, 10);
        let payload = super::super::parse_window::primary_payload(&pending.packet, &view)
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
    fn adopts_forwarded_client_gui_status_without_duplicate_or_sequence_shift() {
        let claim = InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
            object_id: Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID),
            panel: None,
            player_inventory_gui: Some(true),
            rewritten_self_object_id: false,
        };
        let mut update = ready_server_inventory_update();
        update.consumer = ClientGuiInventory;
        update.server_inventory_claim = None;
        update.client_gui_inventory_claim = Some(claim);

        let mut state = SessionState::default();
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);
        state
            .inventory_equipment
            .client_gui_status_response_live_object_packets = 12;
        state
            .inventory_equipment
            .client_gui_status_response_live_gui_record_packets = 3;

        maybe_record_non_server_inventory_equipment_bridge_output_decision(
            &mut state,
            Some((84, 50, claim)),
        );

        assert!(state.sequence.pending_client_to_server_packets.is_empty());
        assert!(state.sequence.client_sequence_shifts.is_empty());
        assert_eq!(
            state.inventory_equipment.queued_client_gui_status_outputs,
            0
        );
        assert_eq!(
            state
                .inventory_equipment
                .forwarded_client_gui_status_requests,
            1
        );
        assert!(
            state
                .inventory_equipment
                .client_gui_status_request_is_forwarded()
        );
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
                resolved_current_player_object_id: None,
                player_inventory_gui: true,
                trigger_client_sequence: 84,
                synthetic_sequence: 84,
                ack_sequence: 50,
            })
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_decision
                .expect("forwarded request decision")
                .kind,
            InventoryEquipmentBridgeOutputDecisionKind::ForwardedClientGuiStatusRequest
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_live_object_packets,
            0,
            "a forwarded request starts a fresh response window"
        );

        observe_server_ack_for_client_gui_status(&mut state, 84);
        let generic_live_object =
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
                live_gui_records: 2,
                live_gui_fragment_bits: 17,
                materialized_item_object_ids: vec![0x8000_1234],
                compact_item_emission_ready_objects: 1,
                compact_item_emission_ready_candidate: Some(update.candidate),
            };
        maybe_record_client_gui_status_live_object_frame_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            55,
            84,
            51,
            Some(&generic_live_object),
            None,
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_non_inventory_live_object_packets_ignored,
            1
        );
        assert!(
            state
                .inventory_equipment
                .last_client_gui_status_response
                .is_none()
        );

        let inventory_response =
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 1,
                inventory_owner_claims: vec![
                    crate::translate::semantic::LiveObjectInventoryOwner {
                        owner_id: 0x8000_5678,
                        mask: 0x1000,
                    },
                ],
                materialized_item_object_ids: vec![0x8000_1234],
                ..Default::default()
            };
        maybe_record_client_gui_status_live_object_frame_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            56,
            84,
            51,
            Some(&inventory_response),
            None,
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "observed_inventory_record"
        );
        assert!(
            !state
                .inventory_equipment
                .client_gui_status_response_window_complete()
        );
        assert!(
            !state
                .inventory_equipment
                .client_gui_status_refresh_confirmed(),
            "an uncorrelated inventory owner must not be promoted to current-player proof"
        );
        assert_eq!(
            state
                .inventory_equipment
                .best_client_gui_status_response
                .expect("foreign inventory response should remain diagnostic evidence")
                .materialized_item_object_ids,
            1
        );

        const CURRENT_CREATURE_ID: u32 = 0xFFFF_FFEF;
        state
            .semantic
            .player_control
            .observe_object_control(0, CURRENT_CREATURE_ID);
        let current_player_inventory_response =
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 3,
                inventory_owner_claims: vec![
                    crate::translate::semantic::LiveObjectInventoryOwner {
                        owner_id: CURRENT_CREATURE_ID,
                        mask: 0x2000,
                    },
                    crate::translate::semantic::LiveObjectInventoryOwner {
                        owner_id: 0x8000_5678,
                        mask: 0x1000,
                    },
                    crate::translate::semantic::LiveObjectInventoryOwner {
                        owner_id: CURRENT_CREATURE_ID,
                        mask: 0x0400,
                    },
                ],
                ..Default::default()
            };
        let recorded_current_player_response =
            maybe_record_client_gui_status_live_object_frame_response(
                &mut state,
                &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
                57,
                84,
                51,
                Some(&current_player_inventory_response),
                Some(CURRENT_CREATURE_ID),
            );
        assert!(recorded_current_player_response);
        if recorded_current_player_response {
            maybe_bind_current_player_status_response_authority(
                &mut state,
                57,
                &crate::translate::semantic::LiveObjectInventoryMaterializationObservation {
                    summary: current_player_inventory_response.clone(),
                    current_controlled_object_id: Some(CURRENT_CREATURE_ID),
                    area_client_area_packets: 0,
                    control_epoch: 1,
                },
            );
        }
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "confirmed_current_player_inventory_record"
        );
        assert!(
            state
                .inventory_equipment
                .client_gui_status_response_window_complete()
        );
        assert!(
            state
                .inventory_equipment
                .client_gui_status_refresh_confirmed()
        );
        let response = state
            .inventory_equipment
            .best_client_gui_status_response
            .expect("matching current-player response should outrank generic inventory");
        assert_eq!(response.current_player_inventory_records, 2);
        assert_eq!(response.first_current_player_inventory_mask, Some(0x2000));
        let expected_binding = InventoryEquipmentCurrentPlayerStatusBinding {
            queued_update_index: update.update_index,
            area_client_area_packets: 0,
            control_epoch: 1,
            server_sequence: 57,
            owner_object_id: CURRENT_CREATURE_ID,
            owner_record_count: 2,
            owner_mask_union: 0x2400,
        };
        assert_eq!(
            state
                .inventory_equipment
                .current_player_status_binding
                .expect("matching rows mint exact current-player status provenance"),
            expected_binding
        );
        let later_sibling =
            crate::translate::semantic::LiveObjectInventoryMaterializationObservation {
                summary: crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                    inventory_records: 2,
                    inventory_owner_claims: vec![
                        crate::translate::semantic::LiveObjectInventoryOwner {
                            owner_id: CURRENT_CREATURE_ID,
                            mask: 0x1000,
                        },
                        crate::translate::semantic::LiveObjectInventoryOwner {
                            owner_id: CURRENT_CREATURE_ID,
                            mask: 0x8000,
                        },
                    ],
                    ..Default::default()
                },
                current_controlled_object_id: Some(CURRENT_CREATURE_ID),
                area_client_area_packets: 9,
                control_epoch: 9,
            };
        let recorded_later_sibling = maybe_record_client_gui_status_live_object_frame_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            57,
            84,
            51,
            Some(&later_sibling.summary),
            later_sibling.current_controlled_object_id,
        );
        assert!(
            !recorded_later_sibling,
            "a sibling after response-window completion must not mint authority"
        );
        if recorded_later_sibling {
            maybe_bind_current_player_status_response_authority(&mut state, 57, &later_sibling);
        }
        assert_eq!(
            state.inventory_equipment.current_player_status_binding,
            Some(expected_binding),
            "later same-frame observations must not overwrite the exact completing unit"
        );
        assert_eq!(
            response.materialized_item_object_ids, 0,
            "exact current-owner proof must outrank an earlier foreign materialization"
        );
    }

    #[test]
    fn adopts_forwarded_concrete_current_player_status_from_obj_control_authority() {
        const CURRENT_CREATURE_ID: u32 = 0xFFFF_FFEF;
        let claim = InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
            object_id: Some(CURRENT_CREATURE_ID),
            panel: None,
            player_inventory_gui: Some(true),
            rewritten_self_object_id: false,
        };
        let mut update = ready_server_inventory_update();
        update.consumer = ClientGuiInventory;
        update.server_inventory_claim = None;
        update.client_gui_inventory_claim = Some(claim);

        let mut state = SessionState::default();
        state
            .semantic
            .player_control
            .observe_object_control(0, CURRENT_CREATURE_ID);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_record_non_server_inventory_equipment_bridge_output_decision(
            &mut state,
            Some((85, 52, claim)),
        );

        assert!(state.sequence.pending_client_to_server_packets.is_empty());
        assert!(state.sequence.client_sequence_shifts.is_empty());
        assert_eq!(
            state
                .inventory_equipment
                .forwarded_client_gui_status_requests,
            1
        );
        let queued = state
            .inventory_equipment
            .last_queued_client_gui_status_output
            .expect("concrete request matching ObjControl should be adopted");
        assert_eq!(queued.object_id, CURRENT_CREATURE_ID);
        assert_eq!(
            queued.resolved_current_player_object_id,
            Some(CURRENT_CREATURE_ID)
        );
        assert_eq!(queued.synthetic_sequence, 85);
        assert_eq!(queued.ack_sequence, 52);
    }

    #[test]
    fn sentinel_status_resolves_control_at_each_inventory_observation() {
        const CREATURE_A: u32 = 0xFFFF_FFEF;
        const CREATURE_B: u32 = 0xFFFF_FFEE;
        let claim = InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
            object_id: Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID),
            panel: None,
            player_inventory_gui: Some(true),
            rewritten_self_object_id: false,
        };
        let mut update = ready_server_inventory_update();
        update.consumer = ClientGuiInventory;
        update.server_inventory_claim = None;
        update.client_gui_inventory_claim = Some(claim);
        let mut state = SessionState::default();
        state
            .semantic
            .player_control
            .observe_object_control(0, CREATURE_A);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);
        maybe_record_non_server_inventory_equipment_bridge_output_decision(
            &mut state,
            Some((84, 50, claim)),
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_queued_client_gui_status_output
                .expect("sentinel request should be adopted")
                .resolved_current_player_object_id,
            Some(CREATURE_A)
        );
        observe_server_ack_for_client_gui_status(&mut state, 84);

        state
            .semantic
            .player_control
            .observe_object_control(0, CREATURE_B);
        let response = crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
            inventory_records: 1,
            inventory_owner_claims: vec![crate::translate::semantic::LiveObjectInventoryOwner {
                owner_id: CREATURE_B,
                mask: 0x2000,
            }],
            ..Default::default()
        };
        maybe_record_client_gui_status_live_object_frame_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            58,
            84,
            51,
            Some(&response),
            Some(CREATURE_B),
        );

        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "confirmed_current_player_inventory_record"
        );
    }

    #[test]
    fn concrete_status_keeps_request_time_control_identity() {
        const CREATURE_A: u32 = 0xFFFF_FFEF;
        const CREATURE_B: u32 = 0xFFFF_FFEE;
        let claim = InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
            object_id: Some(CREATURE_A),
            panel: None,
            player_inventory_gui: Some(true),
            rewritten_self_object_id: false,
        };
        let mut update = ready_server_inventory_update();
        update.consumer = ClientGuiInventory;
        update.server_inventory_claim = None;
        update.client_gui_inventory_claim = Some(claim);
        let mut state = SessionState::default();
        state
            .semantic
            .player_control
            .observe_object_control(0, CREATURE_A);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);
        maybe_record_non_server_inventory_equipment_bridge_output_decision(
            &mut state,
            Some((85, 52, claim)),
        );
        observe_server_ack_for_client_gui_status(&mut state, 85);
        state
            .semantic
            .player_control
            .observe_object_control(0, CREATURE_B);

        let response_for_new_control =
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 1,
                inventory_owner_claims: vec![
                    crate::translate::semantic::LiveObjectInventoryOwner {
                        owner_id: CREATURE_B,
                        mask: 0x1000,
                    },
                ],
                ..Default::default()
            };
        maybe_record_client_gui_status_live_object_frame_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            58,
            85,
            53,
            Some(&response_for_new_control),
            Some(CREATURE_B),
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "observed_inventory_record"
        );

        let response_for_requested_object =
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 2,
                inventory_owner_claims: vec![
                    crate::translate::semantic::LiveObjectInventoryOwner {
                        owner_id: CREATURE_A,
                        mask: 0x2000,
                    },
                    crate::translate::semantic::LiveObjectInventoryOwner {
                        owner_id: CREATURE_B,
                        mask: 0x1000,
                    },
                ],
                ..Default::default()
            };
        maybe_record_client_gui_status_live_object_frame_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            59,
            85,
            53,
            Some(&response_for_requested_object),
            Some(CREATURE_B),
        );
        maybe_bind_current_player_status_response_authority(
            &mut state,
            59,
            &crate::translate::semantic::LiveObjectInventoryMaterializationObservation {
                summary: response_for_requested_object,
                current_controlled_object_id: Some(CREATURE_B),
                area_client_area_packets: 0,
                control_epoch: 2,
            },
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_request_completion()
                .as_str(),
            "confirmed_current_player_inventory_record"
        );
        assert_eq!(
            state
                .inventory_equipment
                .current_player_status_binding
                .expect("concrete Status must preserve request-time owner identity")
                .owner_object_id,
            CREATURE_A
        );
    }

    #[test]
    fn defers_forwarded_concrete_status_without_matching_obj_control() {
        const CURRENT_CREATURE_ID: u32 = 0xFFFF_FFEF;
        const FOREIGN_CREATURE_ID: u32 = 0xFFFF_FFEE;
        let claim = InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
            object_id: Some(FOREIGN_CREATURE_ID),
            panel: None,
            player_inventory_gui: Some(true),
            rewritten_self_object_id: false,
        };
        let mut update = ready_server_inventory_update();
        update.consumer = ClientGuiInventory;
        update.server_inventory_claim = None;
        update.client_gui_inventory_claim = Some(claim);
        let mut state = SessionState::default();
        state
            .semantic
            .player_control
            .observe_object_control(0, CURRENT_CREATURE_ID);
        state
            .semantic
            .ui
            .last_inventory_equipment_bridge_handoff_state_update = Some(update);

        maybe_record_non_server_inventory_equipment_bridge_output_decision(
            &mut state,
            Some((85, 52, claim)),
        );

        assert_eq!(
            state
                .inventory_equipment
                .forwarded_client_gui_status_requests,
            0
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_decision
                .expect("mismatched concrete request should be explicitly deferred")
                .kind,
            InventoryEquipmentBridgeOutputDecisionKind::DeferredClientGui
        );
        assert!(
            state
                .inventory_equipment
                .last_queued_client_gui_status_output
                .is_none()
        );
    }

    #[test]
    fn non_server_client_gui_output_maps_ee_facing_ack_through_expansion_span() {
        let queued_ack = |observed_ee_ack: Option<u16>, latest_server_sequence: Option<u16>| {
            let mut update = ready_server_inventory_update();
            update.consumer = ClientGuiInventory;
            update.server_inventory_claim = None;
            let claim = InventoryEquipmentClientGuiInventoryClaim {
                kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
                object_id: Some(client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID),
                panel: None,
                player_inventory_gui: Some(true),
                rewritten_self_object_id: true,
            };
            update.client_gui_inventory_claim = Some(claim);

            let mut state = SessionState::default();
            state.sequence.latest_client_sequence_from_client = Some(10);
            state.sequence.latest_client_ack_from_client = observed_ee_ack;
            state.sequence.latest_server_sequence_to_client = latest_server_sequence;
            let source = super::super::sequence::SequenceEpochKey::new(61, 4);
            let mut epochs =
                super::super::sequence::OrderedServerSequenceEpochs::identity_at(source);
            epochs
                .insert_before(
                    super::super::sequence::ServerSequenceInsertionOwner::new(
                        source,
                        ServerSequenceInsertionProducer::Test { operation: 1 },
                    ),
                    super::super::sequence::SequenceEpochKey::new(62, 4),
                    1,
                )
                .expect("seed exact expanded output insertion");
            state.sequence.ordered_server_sequence_epochs = epochs;
            output_reliability::register_server_output_ack_span(
                &mut state.sequence.server_output_ack_spans,
                super::super::server_replay::ServerReliableSlotKey {
                    sequence: 61,
                    origin_generation: 4,
                },
                61,
                62,
            )
            .expect("register expanded server output span");
            let expanded = crate::translate::Emit::Packets(vec![
                synthetic_area::build_synthetic_gameplay_frame(61, 10, &[0x01, 0x01])
                    .expect("build first expanded destination"),
                synthetic_area::build_synthetic_gameplay_frame(62, 10, &[0x01, 0x01])
                    .expect("build terminal expanded destination"),
            ]);
            super::super::stage_direct_server_send_window(&mut state, &expanded)
                .expect("stage exact expanded EE destination interval");
            assert_eq!(
                super::super::ee_send_window::finish(
                    &mut state.ee_server_send_window,
                    super::super::ee_send_window::EeServerSendOwner::DirectServer,
                    true,
                ),
                2
            );

            queue_client_gui_status_output_with_claim(&mut state, update, claim, 10, None)
                .expect("queue non-server ClientGui status output");
            let pending = state
                .sequence
                .pending_client_to_server_packets
                .pop()
                .expect("queued client packet");
            MFrameView::parse(&pending.packet)
                .expect("queued client packet should parse")
                .ack_sequence
        };

        assert_eq!(
            queued_ack(Some(61), None),
            60,
            "the first rebuilt destination frame is only a partial source ACK"
        );
        assert_eq!(
            queued_ack(Some(62), None),
            61,
            "the terminal rebuilt destination frame completes the source ACK"
        );
        assert_eq!(
            queued_ack(None, Some(62)),
            61,
            "the latest emitted EE destination fallback must also enter the legacy sequence domain"
        );
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
                resolved_current_player_object_id: None,
                player_inventory_gui: true,
                trigger_client_sequence: 80,
                synthetic_sequence: 80,
                ack_sequence: 82,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
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
        let frame_materialization = state
            .semantic
            .ui
            .last_live_object_inventory_materialization
            .clone();

        maybe_record_client_gui_status_live_object_frame_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            48,
            80,
            82,
            frame_materialization.as_ref(),
            None,
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
                resolved_current_player_object_id: None,
                player_inventory_gui: true,
                trigger_client_sequence: 79,
                synthetic_sequence: 81,
                ack_sequence: 44,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
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
            81,
            78,
            true,
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
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
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
            81,
            78,
            true,
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

        state.inventory_equipment.current_player_status_binding =
            Some(InventoryEquipmentCurrentPlayerStatusBinding {
                queued_update_index: 1,
                area_client_area_packets: 0,
                control_epoch: 1,
                server_sequence: 60,
                owner_object_id: 0xFFFF_FFEF,
                owner_record_count: 1,
                owner_mask_union: 0x2000,
            });
        state
            .inventory_equipment
            .begin_client_gui_status_request_window();
        assert!(
            state
                .inventory_equipment
                .current_player_status_binding
                .is_none(),
            "a new Status request must invalidate the prior response authority"
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
            81,
            79,
            true,
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_response_live_object_packets,
            0
        );
        assert!(
            state
                .inventory_equipment
                .best_client_gui_status_response
                .is_none(),
            "an empty live-object summary must not become the new window's response"
        );
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_non_inventory_live_object_packets_ignored,
            1
        );
        assert!(
            !state
                .inventory_equipment
                .client_gui_status_response_window_complete()
        );
    }

    #[test]
    fn response_window_does_not_reuse_prior_materialization_without_current_frame_observation() {
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
                candidate: None,
                ready_objects: 19,
                deferred_feature25_only_objects: 0,
                object_id: client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID,
                resolved_current_player_object_id: None,
                player_inventory_gui: true,
                trigger_client_sequence: 81,
                synthetic_sequence: 82,
                ack_sequence: 35,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
                live_gui_records: 26,
                live_gui_fragment_bits: 178,
                materialized_item_object_ids: vec![0x8001_64CE, 0x8001_6514],
                compact_item_emission_ready_objects: 43,
                compact_item_emission_ready_candidate: None,
            },
        );
        state
            .semantic
            .ui
            .live_object_inventory_materialization_observations = 1;
        observe_server_ack_for_client_gui_status(&mut state, 82);

        let proof = VerifiedProof::GameplayStream(vec![VerifiedFamily::GameObjUpdateLiveObject]);
        let current_materialization =
            super::super::observe_verified_server_payload_semantics(&mut state, &proof, &[]);
        assert!(
            current_materialization.is_empty(),
            "a proof entry without a current gameplay unit must not adopt the previous summary"
        );
        maybe_record_client_gui_status_live_object_frame_response(
            &mut state, &proof, 36, 82, 80, None, None,
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
                resolved_current_player_object_id: None,
                player_inventory_gui: true,
                trigger_client_sequence: 81,
                synthetic_sequence: 82,
                ack_sequence: 35,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
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
            81,
            80,
            true,
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

        // A later/reordered frame can carry raw ACK 81 even if session-level
        // diagnostics have advanced beyond ACK 82. The explicit current-frame
        // ACK must win over that mutable historical state.
        observe_server_ack_for_client_gui_status(&mut state, 90);
        assert!(
            state
                .inventory_equipment
                .client_gui_status_request_acknowledged()
        );
        assert_eq!(
            state
                .inventory_equipment
                .last_observed_client_gui_status_server_peer_ack_sequence,
            Some(90)
        );
        maybe_record_client_gui_status_live_object_response(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::GameObjUpdateLiveObject),
            35,
            81,
            80,
            true,
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
            82,
            80,
            true,
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
                resolved_current_player_object_id: None,
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
    fn replays_original_inventory_cancel_after_status_materializes_claim_object() {
        let candidate = InventoryItemContextCandidate {
            object_id: 0x8001_5322,
            proof: InventoryItemObjectProof::ActiveObject,
            source: InventoryItemContextCandidateSource::DirectOnly,
        };
        let claim =
            InventoryEquipmentServerInventoryClaim::new(0x02, 0x8001_53D3, false, 0x0002_0000);
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
                resolved_current_player_object_id: None,
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
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
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
            83,
            82,
            true,
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

        state.sequence.current_server_translation_source =
            Some(super::super::server_replay::ServerReliableSlotKey {
                sequence: 61,
                origin_generation: 0,
            });
        super::super::begin_ordinary_server_emit_effect_transaction(&mut state)
            .expect("begin source-owned confirmed replay transaction");
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
                alternate_inventory_context: claim.alternate_inventory_context,
                equip_slot: claim.equip_slot,
                trigger_sequence: 61,
                synthetic_sequence: 62,
            })
        );
        assert_eq!(state.sequence.pending_server_sequence_insertions.len(), 1);

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
        assert_eq!(
            replay_claim.operation,
            inventory::InventoryOperation::EquipCancel
        );
        assert_eq!(replay_claim.object_id, claim.object_id);
        assert_eq!(
            replay_claim.alternate_inventory_context,
            claim.alternate_inventory_context
        );
        assert_eq!(replay_claim.shape.equip_slot(), Some(claim.equip_slot));

        assert!(
            !maybe_queue_confirmed_inventory_replay(&mut state, 62, 82)
                .expect("same update must not replay twice")
        );
        assert_eq!(
            state.inventory_equipment.confirmed_inventory_replay_outputs,
            1
        );

        let emit = super::super::finalize_server_to_client_emit(
            &mut state,
            crate::translate::Emit::Consumed,
            0,
        )
        .expect("confirmed inventory replay joins its source emit");
        assert!(matches!(
            emit,
            crate::translate::Emit::MixedVerifiedPackets(_)
        ));
        super::super::finish_server_to_client_emit_validation(&mut state, true);
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
                resolved_current_player_object_id: None,
                player_inventory_gui: false,
                trigger_client_sequence: 80,
                synthetic_sequence: 81,
                ack_sequence: 40,
            });
        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
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
            81,
            true,
        );

        state.semantic.ui.last_live_object_inventory_materialization = Some(
            crate::translate::semantic::LiveObjectInventoryMaterializationSummary {
                inventory_records: 0,
                inventory_owner_claims: Vec::new(),
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
            81,
            true,
        );

        let last = state
            .inventory_equipment
            .last_client_gui_status_response
            .expect("typed response should remain the latest retained response");
        assert_eq!(last.server_sequence, 90);
        assert_eq!(last.live_gui_records, 9);
        assert_eq!(last.materialized_item_object_ids, 2);
        assert_eq!(
            state
                .inventory_equipment
                .client_gui_status_non_inventory_live_object_packets_ignored,
            1
        );

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

        maybe_record_non_server_inventory_equipment_bridge_output_decision(&mut state, None);
        maybe_record_non_server_inventory_equipment_bridge_output_decision(&mut state, None);

        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .is_empty()
        );
        assert!(state.sequence.pending_server_sequence_insertions.is_empty());
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
        assert_eq!(pending.family, VerifiedFamily::ClientGuiInventory);
        let view = MFrameView::parse(&pending.packet).expect("queued client packet should parse");
        assert_eq!(view.sequence, 31);
        assert_eq!(view.ack_sequence, 90);
        let payload = super::super::parse_window::primary_payload(&pending.packet, &view)
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
        let mut state = session_state_with_server_source(10);
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
        assert!(!claim.alternate_inventory_context);
        assert_eq!(claim.shape.equip_slot(), Some(0x0002_0000));
    }
}
