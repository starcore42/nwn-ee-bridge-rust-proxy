//! Proxy-owned inventory/equipment bridge output.
//!
//! The semantic reducer owns the proof that retained direct/materialized item
//! state is ready for inventory/equipment handoff. This module only turns a
//! drained server-inventory handoff update into an exact EE-facing reliable
//! `Inventory_Equip`/`Inventory_EquipCancel` frame.

use std::time::Instant;

use crate::translate::{
    VerifiedFamily, client_gui_inventory, inventory,
    semantic::{
        InventoryEquipmentClientGuiInventoryClaimKind,
        InventoryEquipmentHandoffConsumer::ClientGuiInventory,
    },
};

use super::{
    sequence::{SequenceShift, shift_sequence_for_peer, trim_sequence_shifts},
    state::{
        InventoryEquipmentBridgeOutputDecision, InventoryEquipmentBridgeOutputDecisionKind,
        InventoryEquipmentBridgeQueuedClientGuiStatusOutput, InventoryEquipmentBridgeQueuedOutput,
        SessionState,
    },
    synthetic_area::{self, PendingServerPacket, PendingServerPacketPlacement},
};

const INVENTORY_EQUIPMENT_BRIDGE_REASON: &str =
    "inventory/equipment ready item-state bridge Inventory output";
const INVENTORY_EQUIPMENT_BRIDGE_INSERTED_FRAME_COUNT: u16 = 1;

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

    let player_inventory_gui = claim.player_inventory_gui.unwrap_or(true);
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

    record_output_decision(
        state,
        update,
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
