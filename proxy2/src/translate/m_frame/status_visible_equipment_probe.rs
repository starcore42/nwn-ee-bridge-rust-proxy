//! Stable diagnostics for Status-authorized P/5 equipment state.
//!
//! This retains both the focused EquipToggle transaction and the most recent
//! exact visible-equipment `U` observation. Keeping the serializer beside those
//! models prevents the already transport-heavy M-frame dispatcher from owning
//! another large field projection. The values are copied only from typed
//! semantic state; this module does not parse packets, reinterpret the `U`
//! status BYTE, or infer missing equipment rows.

use std::fmt::Write as _;

use crate::translate::{live_object_update::LiveObjectVisibleEquipmentOperation, semantic};

fn visible_equipment_update_ledger_json(
    protocol: &semantic::InventoryEquipmentProtocolState,
) -> String {
    let mut json = String::from("[");
    for (index, observation) in protocol
        .visible_equipment_update_observation_ledger
        .iter()
        .enumerate()
    {
        if index != 0 {
            json.push(',');
        }
        write!(
            json,
            concat!(
                "{{\"area_update_ordinal\":{},",
                "\"claim_ordinal\":{},",
                "\"owner_object_id\":{},",
                "\"appearance_mask\":{},",
                "\"all_fields_appearance\":{},",
                "\"record_offset\":{},",
                "\"record_end\":{},",
                "\"claim_fragment_bit_start\":{},",
                "\"claim_fragment_bit_end\":{},",
                "\"row_ordinal\":{},",
                "\"row_offset\":{},",
                "\"object_id_offset\":{},",
                "\"visible_slot_offset\":{},",
                "\"status_offset\":{},",
                "\"visual_transform_map_offset\":{},",
                "\"visual_transform_map_end\":{},",
                "\"row_end\":{},",
                "\"row_fragment_bit_start\":{},",
                "\"row_fragment_bit_end\":{},",
                "\"row_fragment_cursor_unchanged\":{},",
                "\"row_object_id\":{},",
                "\"carried_visible_slot\":{},",
                "\"status_known\":{},",
                "\"status\":{},",
                "\"matching_visible_slot_count\":{},",
                "\"unique_matching_visible_slot_known\":{},",
                "\"unique_matching_visible_slot\":{},",
                "\"legacy_equivalent_visible_slot_count\":{},",
                "\"exact_object_id_match\":{},",
                "\"legacy_equivalence_only_match\":{}}}"
            ),
            observation.area_update_ordinal,
            observation.claim_ordinal,
            observation.owner_object_id,
            observation.appearance_mask,
            observation.all_fields_appearance,
            observation.record_offset,
            observation.record_end,
            observation.fragment_bit_start,
            observation.fragment_bit_end,
            observation.row_ordinal,
            observation.row_offset,
            observation.object_id_offset,
            observation.visible_slot_offset,
            observation.status_offset,
            observation.visual_transform_map_offset,
            observation.visual_transform_map_end,
            observation.row_end,
            observation.row_fragment_bit_start,
            observation.row_fragment_bit_end,
            observation.row_fragment_bit_start == observation.row_fragment_bit_end,
            observation.row_object_id,
            observation.carried_visible_slot,
            observation.update_status.is_some(),
            observation.update_status.unwrap_or(0),
            observation.matching_visible_slot_count,
            observation.unique_matching_visible_slot.is_some(),
            observation.unique_matching_visible_slot.unwrap_or(0),
            observation.legacy_equivalent_visible_slot_count,
            observation.exact_object_id_match,
            observation.legacy_equivalence_only_match,
        )
        .expect("writing visible-equipment U ledger JSON to String cannot fail");
    }
    json.push(']');
    json
}

pub(super) fn json_fields(protocol: &semantic::InventoryEquipmentProtocolState) -> String {
    let stage = protocol
        .status_authorized_visible_equipment_probe_stage()
        .as_str();
    let completed = protocol.last_completed_status_authorized_visible_equipment_probe;
    let terminal = protocol.last_terminal_status_authorized_visible_equipment_probe;
    let active = protocol.active_status_authorized_visible_equipment_probe;
    let authorization = completed
        .map(|transaction| transaction.authorization)
        .or_else(|| terminal.map(|transaction| transaction.authorization))
        .or_else(|| active.map(|transaction| transaction.authorization))
        .or(protocol.offered_status_authorized_visible_equipment_probe);
    let action_epoch = completed
        .map(|transaction| transaction.action_epoch)
        .or_else(|| terminal.map(|transaction| transaction.action_epoch))
        .or_else(|| active.map(|transaction| transaction.action_epoch));
    let action = completed
        .map(|transaction| transaction.action)
        .or_else(|| terminal.map(|transaction| transaction.action))
        .or_else(|| active.map(|transaction| transaction.action));
    let response = completed
        .map(|transaction| transaction.response)
        .or_else(|| terminal.map(|transaction| transaction.response))
        .or_else(|| active.and_then(|transaction| transaction.matching_response));
    let delta = completed.map(|transaction| transaction.delta);
    let last_update = protocol.last_visible_equipment_update_observation;
    let delta_operation = delta
        .map(|delta| match delta.operation {
            LiveObjectVisibleEquipmentOperation::Add => "add",
            LiveObjectVisibleEquipmentOperation::Delete => "delete",
            LiveObjectVisibleEquipmentOperation::Update => "update",
            LiveObjectVisibleEquipmentOperation::IgnoredLegacyZero => "ignored_legacy_zero",
        })
        .unwrap_or("none");

    let mut fields = format!(
        concat!(
            ",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_stage\": \"{}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_completed\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_terminal\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_status_update_index\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_status_server_sequence\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_area_client_area_packets\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_control_epoch\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_owner_object_id\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_owner_object_id_hex\": \"0x{:08X}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_object_id\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_object_id_hex\": \"0x{:08X}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_visible_slot\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_epoch\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_primary_object_id\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_primary_object_id_hex\": \"0x{:08X}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_secondary_object_known\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_secondary_object_id\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_secondary_object_id_hex\": \"0x{:08X}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_declared\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_action_fragment_bytes\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_ordinal\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_operation\": \"{}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_object_id\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_object_id_hex\": \"0x{:08X}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_alternate_inventory_context\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_equip_slot_known\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_equip_slot\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_matches_client_primary\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_response_matches_client_secondary\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_claim_ordinal\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_state_update_index\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_appearance_mask\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_appearance_mask_hex\": \"0x{:04X}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_all_fields_appearance\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_operation\": \"{}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_row_object_id\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_row_object_id_hex\": \"0x{:08X}\",\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_visible_slot\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_update_status_known\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_update_status\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_record_offset\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_record_end\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_fragment_bit_start\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_fragment_bit_end\": {},\n",
            "  \"status_authorized_visible_equipment_probe_transaction_delta_row_ordinal\": {},\n",
            "  \"last_visible_equipment_update_observed\": {},\n",
            "  \"last_visible_equipment_update_claim_ordinal\": {},\n",
            "  \"last_visible_equipment_update_owner_object_id\": {},\n",
            "  \"last_visible_equipment_update_owner_object_id_hex\": \"0x{:08X}\",\n",
            "  \"last_visible_equipment_update_appearance_mask\": {},\n",
            "  \"last_visible_equipment_update_appearance_mask_hex\": \"0x{:04X}\",\n",
            "  \"last_visible_equipment_update_all_fields_appearance\": {},\n",
            "  \"last_visible_equipment_update_record_offset\": {},\n",
            "  \"last_visible_equipment_update_record_end\": {},\n",
            "  \"last_visible_equipment_update_fragment_bit_start\": {},\n",
            "  \"last_visible_equipment_update_fragment_bit_end\": {},\n",
            "  \"last_visible_equipment_update_row_ordinal\": {},\n",
            "  \"last_visible_equipment_update_row_object_id\": {},\n",
            "  \"last_visible_equipment_update_row_object_id_hex\": \"0x{:08X}\",\n",
            "  \"last_visible_equipment_update_carried_visible_slot\": {},\n",
            "  \"last_visible_equipment_update_status_known\": {},\n",
            "  \"last_visible_equipment_update_status\": {},\n",
            "  \"last_visible_equipment_update_matching_visible_slot_count\": {},\n",
            "  \"last_visible_equipment_update_unique_matching_visible_slot_known\": {},\n",
            "  \"last_visible_equipment_update_unique_matching_visible_slot\": {},\n",
            "  \"last_visible_equipment_update_carried_slot_matches_unique_mapping\": {},\n",
            "  \"last_visible_equipment_update_legacy_equivalent_visible_slot_count\": {},\n",
            "  \"last_visible_equipment_update_exact_object_id_match\": {},\n",
            "  \"last_visible_equipment_update_legacy_equivalence_only_match\": {}"
        ),
        stage,
        completed.is_some(),
        terminal.is_some(),
        authorization
            .map(|value| value.status_update_index)
            .unwrap_or(0),
        authorization
            .map(|value| value.status_server_sequence)
            .unwrap_or(0),
        authorization
            .map(|value| value.area_client_area_packets)
            .unwrap_or(0),
        authorization.map(|value| value.control_epoch).unwrap_or(0),
        authorization
            .map(|value| value.owner_object_id)
            .unwrap_or(0),
        authorization
            .map(|value| value.owner_object_id)
            .unwrap_or(0),
        authorization.map(|value| value.object_id).unwrap_or(0),
        authorization.map(|value| value.object_id).unwrap_or(0),
        authorization.map(|value| value.visible_slot).unwrap_or(0),
        action_epoch.unwrap_or(0),
        action.map(|value| value.primary_object_id).unwrap_or(0),
        action.map(|value| value.primary_object_id).unwrap_or(0),
        action.is_some_and(|value| value.secondary_object_id.is_some()),
        action
            .and_then(|value| value.secondary_object_id)
            .unwrap_or(0),
        action
            .and_then(|value| value.secondary_object_id)
            .unwrap_or(0),
        action.map(|value| value.declared).unwrap_or(0),
        action.map(|value| value.fragment_bytes).unwrap_or(0),
        response.map(|value| value.response_ordinal).unwrap_or(0),
        response
            .map(|value| value.operation.as_str())
            .unwrap_or("none"),
        response.map(|value| value.object_id).unwrap_or(0),
        response.map(|value| value.object_id).unwrap_or(0),
        response.is_some_and(|value| value.alternate_inventory_context),
        response.is_some_and(|value| value.equip_slot.is_some()),
        response.and_then(|value| value.equip_slot).unwrap_or(0),
        response.is_some_and(|value| value.matches_client_primary),
        response.is_some_and(|value| value.matches_client_secondary),
        delta.map(|value| value.claim_ordinal).unwrap_or(0),
        delta.map(|value| value.state_update_index).unwrap_or(0),
        delta.map(|value| value.appearance_mask).unwrap_or(0),
        delta.map(|value| value.appearance_mask).unwrap_or(0),
        delta.is_some_and(|value| value.all_fields_appearance),
        delta_operation,
        delta.map(|value| value.row_object_id).unwrap_or(0),
        delta.map(|value| value.row_object_id).unwrap_or(0),
        delta.map(|value| value.visible_slot).unwrap_or(0),
        delta.is_some_and(|value| value.update_status.is_some()),
        delta.and_then(|value| value.update_status).unwrap_or(0),
        delta.map(|value| value.record_offset).unwrap_or(0),
        delta.map(|value| value.record_end).unwrap_or(0),
        delta.map(|value| value.fragment_bit_start).unwrap_or(0),
        delta.map(|value| value.fragment_bit_end).unwrap_or(0),
        delta.map(|value| value.row_ordinal).unwrap_or(0),
        last_update.is_some(),
        last_update.map(|value| value.claim_ordinal).unwrap_or(0),
        last_update.map(|value| value.owner_object_id).unwrap_or(0),
        last_update.map(|value| value.owner_object_id).unwrap_or(0),
        last_update.map(|value| value.appearance_mask).unwrap_or(0),
        last_update.map(|value| value.appearance_mask).unwrap_or(0),
        last_update.is_some_and(|value| value.all_fields_appearance),
        last_update.map(|value| value.record_offset).unwrap_or(0),
        last_update.map(|value| value.record_end).unwrap_or(0),
        last_update
            .map(|value| value.fragment_bit_start)
            .unwrap_or(0),
        last_update.map(|value| value.fragment_bit_end).unwrap_or(0),
        last_update.map(|value| value.row_ordinal).unwrap_or(0),
        last_update.map(|value| value.row_object_id).unwrap_or(0),
        last_update.map(|value| value.row_object_id).unwrap_or(0),
        last_update
            .map(|value| value.carried_visible_slot)
            .unwrap_or(0),
        last_update.is_some_and(|value| value.update_status.is_some()),
        last_update
            .and_then(|value| value.update_status)
            .unwrap_or(0),
        last_update
            .map(|value| value.matching_visible_slot_count)
            .unwrap_or(0),
        last_update.is_some_and(|value| value.unique_matching_visible_slot.is_some()),
        last_update
            .and_then(|value| value.unique_matching_visible_slot)
            .unwrap_or(0),
        last_update.is_some_and(|value| {
            value.unique_matching_visible_slot == Some(value.carried_visible_slot)
        }),
        last_update
            .map(|value| value.legacy_equivalent_visible_slot_count)
            .unwrap_or(0),
        last_update.is_some_and(|value| value.exact_object_id_match),
        last_update.is_some_and(|value| value.legacy_equivalence_only_match),
    );
    let ledger_json = visible_equipment_update_ledger_json(protocol);
    write!(
        fields,
        concat!(
            ",\n",
            "  \"last_visible_equipment_update_area_update_ordinal\": {},\n",
            "  \"last_visible_equipment_update_row_offset\": {},\n",
            "  \"last_visible_equipment_update_object_id_offset\": {},\n",
            "  \"last_visible_equipment_update_visible_slot_offset\": {},\n",
            "  \"last_visible_equipment_update_status_offset\": {},\n",
            "  \"last_visible_equipment_update_visual_transform_map_offset\": {},\n",
            "  \"last_visible_equipment_update_visual_transform_map_end\": {},\n",
            "  \"last_visible_equipment_update_row_end\": {},\n",
            "  \"last_visible_equipment_update_row_fragment_bit_start\": {},\n",
            "  \"last_visible_equipment_update_row_fragment_bit_end\": {},\n",
            "  \"last_visible_equipment_update_row_fragment_cursor_unchanged\": {},\n",
            "  \"visible_equipment_update_ledger_capacity\": {},\n",
            "  \"visible_equipment_update_ledger_observed_in_area\": {},\n",
            "  \"visible_equipment_update_ledger_retained\": {},\n",
            "  \"visible_equipment_update_ledger_evicted_in_area\": {},\n",
            "  \"visible_equipment_update_ledger\": {}"
        ),
        last_update
            .map(|value| value.area_update_ordinal)
            .unwrap_or(0),
        last_update.map(|value| value.row_offset).unwrap_or(0),
        last_update.map(|value| value.object_id_offset).unwrap_or(0),
        last_update
            .map(|value| value.visible_slot_offset)
            .unwrap_or(0),
        last_update.map(|value| value.status_offset).unwrap_or(0),
        last_update
            .map(|value| value.visual_transform_map_offset)
            .unwrap_or(0),
        last_update
            .map(|value| value.visual_transform_map_end)
            .unwrap_or(0),
        last_update.map(|value| value.row_end).unwrap_or(0),
        last_update
            .map(|value| value.row_fragment_bit_start)
            .unwrap_or(0),
        last_update
            .map(|value| value.row_fragment_bit_end)
            .unwrap_or(0),
        last_update
            .is_some_and(|value| { value.row_fragment_bit_start == value.row_fragment_bit_end }),
        semantic::MAX_VISIBLE_EQUIPMENT_UPDATE_OBSERVATIONS_PER_AREA,
        protocol.visible_equipment_update_observations_in_area,
        protocol.visible_equipment_update_observation_ledger.len(),
        protocol.visible_equipment_update_observations_evicted_in_area,
        ledger_json,
    )
    .expect("writing visible-equipment U ledger fields to String cannot fail");
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::{
        client_inventory, inventory,
        live_object_update::{
            LiveObjectCreatureVisibleEquipmentClaim, LiveObjectVisibleEquipmentRow,
            LiveObjectVisibleEquipmentUpdateProvenance,
        },
    };

    fn authorization(
        status_update_index: u64,
        owner_object_id: u32,
        object_id: u32,
        visible_slot: u32,
    ) -> semantic::StatusAuthorizedVisibleEquipmentProbeAuthorization {
        semantic::StatusAuthorizedVisibleEquipmentProbeAuthorization {
            status_update_index,
            status_server_sequence: status_update_index + 10,
            area_client_area_packets: status_update_index + 20,
            control_epoch: status_update_index + 30,
            owner_object_id,
            visible_slot,
            object_id,
        }
    }

    fn action(primary_object_id: u32) -> client_inventory::ClientInventoryClaimSummary {
        client_inventory::ClientInventoryClaimSummary {
            packet_name: "Inventory_EquipToggle",
            primary_object_id,
            secondary_object_id: None,
            declared: 11,
            fragment_bytes: 1,
        }
    }

    fn observe_matching_unequip(
        protocol: &mut semantic::InventoryEquipmentProtocolState,
        object_id: u32,
    ) {
        let payload = inventory::build_ee_inventory_unequip_payload(0x07, object_id, false)
            .expect("exact false-context Unequip payload");
        let claim = inventory::claim_payload_if_verified(&payload)
            .expect("exact false-context Unequip claim");
        protocol.observe_server_inventory_response(claim);
    }

    fn visible_equipment_claim(
        owner_id: u32,
        appearance_mask: u16,
        operation: LiveObjectVisibleEquipmentOperation,
        object_id: u32,
        visible_slot: u32,
    ) -> LiveObjectCreatureVisibleEquipmentClaim {
        let row_offset = 77 + 8 + 1;
        let update_row_provenance = (operation == LiveObjectVisibleEquipmentOperation::Update)
            .then(|| {
                let status_offset = row_offset + 1 + 4 + 4;
                let visual_transform_map_offset = status_offset + 1;
                let visual_transform_map_end = visual_transform_map_offset + 8;
                LiveObjectVisibleEquipmentUpdateProvenance {
                    row_offset,
                    object_id_offset: row_offset + 1,
                    visible_slot_offset: row_offset + 1 + 4,
                    status_offset,
                    visual_transform_map_offset,
                    visual_transform_map_end,
                    row_end: visual_transform_map_end,
                    fragment_bit_start: 11,
                    fragment_bit_end: 11,
                }
            })
            .into_iter()
            .collect::<Vec<_>>();
        LiveObjectCreatureVisibleEquipmentClaim {
            owner_id,
            appearance_mask,
            all_fields_appearance: appearance_mask == u16::MAX,
            record_offset: 77,
            record_end: update_row_provenance
                .first()
                .map(|provenance| provenance.row_end)
                .unwrap_or(row_offset + 1 + 4 + 4),
            fragment_bit_start: 11,
            fragment_bit_end: 11,
            rows: vec![LiveObjectVisibleEquipmentRow {
                operation,
                object_id,
                visible_slot,
                update_status: None,
            }],
            update_row_provenance,
        }
    }

    #[test]
    fn offered_transaction_serializes_authorization_before_client_action() {
        let offered = authorization(7, 0xffff_ffef, 0x8000_0044, 2);
        let protocol = semantic::InventoryEquipmentProtocolState {
            offered_status_authorized_visible_equipment_probe: Some(offered),
            ..Default::default()
        };

        let fields = json_fields(&protocol);

        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_stage\": \"offered\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_completed\": false"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_status_update_index\": 7"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_owner_object_id_hex\": \"0xFFFFFFEF\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_object_id_hex\": \"0x80000044\""
        ));
        assert!(
            fields.contains(
                "\"status_authorized_visible_equipment_probe_transaction_action_epoch\": 0"
            )
        );
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_response_operation\": \"none\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_delta_operation\": \"none\""
        ));
    }

    #[test]
    fn active_transaction_serializes_action_and_matching_response() {
        let active_authorization = authorization(8, 0xffff_ffee, 0x8000_0055, 0x10);
        let mut protocol = semantic::InventoryEquipmentProtocolState::default();
        protocol.visible_equipment_slots_by_owner.insert(
            (
                active_authorization.owner_object_id,
                active_authorization.visible_slot,
            ),
            active_authorization.object_id,
        );
        protocol.client_equip_toggle_events = 2;
        protocol.offer_status_authorized_visible_equipment_probe(active_authorization);
        protocol.observe_client_equip_toggle(action(active_authorization.object_id));
        protocol.server_responses_since_last_client_equip_toggle = 3;
        observe_matching_unequip(&mut protocol, active_authorization.object_id);

        let fields = json_fields(&protocol);

        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_stage\": \"typed_response_observed\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_completed\": false"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_status_update_index\": 8"
        ));
        assert!(
            fields.contains(
                "\"status_authorized_visible_equipment_probe_transaction_action_epoch\": 3"
            )
        );
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_action_primary_object_id_hex\": \"0x80000055\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_response_ordinal\": 4"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_response_operation\": \"unequip\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_delta_operation\": \"none\""
        ));
    }

    #[test]
    fn terminal_transaction_serializes_exact_response_without_delta() {
        let terminal_authorization = authorization(8, 0xffff_ffee, 0x8000_0055, 0x10);
        let mut protocol = semantic::InventoryEquipmentProtocolState::default();
        protocol.visible_equipment_slots_by_owner.insert(
            (
                terminal_authorization.owner_object_id,
                terminal_authorization.visible_slot,
            ),
            terminal_authorization.object_id,
        );
        protocol.offer_status_authorized_visible_equipment_probe(terminal_authorization);
        protocol.observe_client_equip_toggle(action(terminal_authorization.object_id));
        let response_payload = inventory::build_ee_inventory_payload(
            0x02,
            terminal_authorization.object_id,
            false,
            0x0002_0000,
        )
        .expect("exact false-context EquipCancel payload");
        protocol.observe_server_inventory_response(
            inventory::claim_payload_if_verified(&response_payload)
                .expect("exact false-context EquipCancel claim"),
        );

        let fields = json_fields(&protocol);

        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_stage\": \"terminal_response_observed\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_completed\": false"
        ));
        assert!(
            fields.contains(
                "\"status_authorized_visible_equipment_probe_transaction_terminal\": true"
            )
        );
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_object_id_hex\": \"0x80000055\""
        ));
        assert!(
            fields.contains(
                "\"status_authorized_visible_equipment_probe_transaction_action_epoch\": 1"
            )
        );
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_response_operation\": \"equip_cancel\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_response_equip_slot_known\": true"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_response_equip_slot\": 131072"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_delta_operation\": \"none\""
        ));
    }

    #[test]
    fn completed_transaction_takes_precedence_and_alone_serializes_delta() {
        let offered = authorization(1, 0x1111_1111, 0x8000_0011, 1);
        let active_authorization = authorization(2, 0x2222_2222, 0x8000_0022, 2);
        let completed_authorization = authorization(9, 0xffff_ffef, 0x8000_0099, 0x40);
        let mut active_protocol = semantic::InventoryEquipmentProtocolState::default();
        active_protocol.visible_equipment_slots_by_owner.insert(
            (
                active_authorization.owner_object_id,
                active_authorization.visible_slot,
            ),
            active_authorization.object_id,
        );
        active_protocol.client_equip_toggle_events = 4;
        active_protocol.offer_status_authorized_visible_equipment_probe(active_authorization);
        active_protocol.observe_client_equip_toggle(action(active_authorization.object_id));
        active_protocol.server_responses_since_last_client_equip_toggle = 5;
        observe_matching_unequip(&mut active_protocol, active_authorization.object_id);
        let active = active_protocol
            .active_status_authorized_visible_equipment_probe
            .expect("active typed-response transaction");

        let mut protocol = semantic::InventoryEquipmentProtocolState::default();
        protocol.observe_creature_visible_equipment_claims(&[visible_equipment_claim(
            completed_authorization.owner_object_id,
            u16::MAX,
            LiveObjectVisibleEquipmentOperation::Add,
            completed_authorization.object_id,
            completed_authorization.visible_slot,
        )]);
        protocol.client_equip_toggle_events = 9;
        protocol.offer_status_authorized_visible_equipment_probe(completed_authorization);
        protocol.observe_client_equip_toggle(action(completed_authorization.object_id));
        protocol.server_responses_since_last_client_equip_toggle = 10;
        observe_matching_unequip(&mut protocol, completed_authorization.object_id);
        protocol.observe_creature_visible_equipment_claims(&[visible_equipment_claim(
            completed_authorization.owner_object_id,
            0x0200,
            LiveObjectVisibleEquipmentOperation::Delete,
            0x7f00_0000,
            completed_authorization.visible_slot,
        )]);
        assert!(
            protocol
                .last_completed_status_authorized_visible_equipment_probe
                .is_some(),
            "exact response plus matching D row must complete the transaction"
        );
        protocol.offered_status_authorized_visible_equipment_probe = Some(offered);
        protocol.active_status_authorized_visible_equipment_probe = Some(active);

        let fields = json_fields(&protocol);

        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_stage\": \"completed_visible_equipment_delta\""
        ));
        assert!(
            fields.contains(
                "\"status_authorized_visible_equipment_probe_transaction_completed\": true"
            )
        );
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_status_update_index\": 9"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_object_id_hex\": \"0x80000099\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_action_epoch\": 10"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_response_ordinal\": 11"
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_delta_appearance_mask_hex\": \"0x0200\""
        ));
        assert!(fields.contains(
            "\"status_authorized_visible_equipment_probe_transaction_delta_operation\": \"delete\""
        ));
    }

    #[test]
    fn last_visible_equipment_update_serializes_object_only_resolution() {
        let owner_id = 0xffff_ffef;
        let object_id = 0x8000_0077;
        let mapped_slot = 0x10;
        let carried_slot = 0x40;
        let mut protocol = semantic::InventoryEquipmentProtocolState::default();
        protocol.observe_creature_visible_equipment_claims(&[visible_equipment_claim(
            owner_id,
            u16::MAX,
            LiveObjectVisibleEquipmentOperation::Add,
            object_id,
            mapped_slot,
        )]);
        let mut update = visible_equipment_claim(
            owner_id,
            0x0200,
            LiveObjectVisibleEquipmentOperation::Update,
            object_id,
            carried_slot,
        );
        update.rows[0].update_status = Some(0x7f);
        protocol.observe_creature_visible_equipment_claims(&[update]);

        let fields = json_fields(&protocol);

        assert!(fields.contains("\"last_visible_equipment_update_observed\": true"));
        assert!(
            fields
                .contains("\"last_visible_equipment_update_owner_object_id_hex\": \"0xFFFFFFEF\"")
        );
        assert!(
            fields.contains("\"last_visible_equipment_update_appearance_mask_hex\": \"0x0200\"")
        );
        assert!(
            fields.contains("\"last_visible_equipment_update_row_object_id_hex\": \"0x80000077\"")
        );
        assert!(fields.contains("\"last_visible_equipment_update_carried_visible_slot\": 64"));
        assert!(fields.contains("\"last_visible_equipment_update_status\": 127"));
        assert!(fields.contains("\"last_visible_equipment_update_area_update_ordinal\": 1"));
        assert!(fields.contains("\"last_visible_equipment_update_row_offset\": 86"));
        assert!(fields.contains("\"last_visible_equipment_update_object_id_offset\": 87"));
        assert!(fields.contains("\"last_visible_equipment_update_visible_slot_offset\": 91"));
        assert!(fields.contains("\"last_visible_equipment_update_status_offset\": 95"));
        assert!(
            fields.contains("\"last_visible_equipment_update_visual_transform_map_offset\": 96")
        );
        assert!(fields.contains("\"last_visible_equipment_update_visual_transform_map_end\": 104"));
        assert!(fields.contains("\"last_visible_equipment_update_row_end\": 104"));
        assert!(fields.contains("\"last_visible_equipment_update_row_fragment_bit_start\": 11"));
        assert!(fields.contains("\"last_visible_equipment_update_row_fragment_bit_end\": 11"));
        assert!(
            fields
                .contains("\"last_visible_equipment_update_row_fragment_cursor_unchanged\": true")
        );
        assert!(fields.contains("\"visible_equipment_update_ledger_capacity\": 64"));
        assert!(fields.contains("\"visible_equipment_update_ledger_observed_in_area\": 1"));
        assert!(fields.contains("\"visible_equipment_update_ledger_retained\": 1"));
        assert!(fields.contains("\"visible_equipment_update_ledger_evicted_in_area\": 0"));
        assert!(fields.contains(concat!(
            "\"visible_equipment_update_ledger\": [",
            "{\"area_update_ordinal\":1,\"claim_ordinal\":2,",
            "\"owner_object_id\":4294967279"
        )));
        assert!(fields.contains(
            "\"row_fragment_bit_start\":11,\"row_fragment_bit_end\":11,\"row_fragment_cursor_unchanged\":true"
        ));
        assert!(
            fields.contains("\"last_visible_equipment_update_matching_visible_slot_count\": 1")
        );
        assert!(
            fields.contains("\"last_visible_equipment_update_unique_matching_visible_slot\": 16")
        );
        assert!(fields.contains(
            "\"last_visible_equipment_update_carried_slot_matches_unique_mapping\": false"
        ));
        assert!(
            fields.contains(
                "\"last_visible_equipment_update_legacy_equivalent_visible_slot_count\": 0"
            )
        );
        assert!(fields.contains("\"last_visible_equipment_update_exact_object_id_match\": true"));
        assert!(
            fields
                .contains("\"last_visible_equipment_update_legacy_equivalence_only_match\": false")
        );
    }

    #[test]
    fn visible_equipment_update_ledger_serializes_two_rows_in_observation_order() {
        let owner_id = 0xffff_ffef;
        let first_object_id = 0x8000_0077;
        let second_object_id = 0x8000_0088;
        let mut protocol = semantic::InventoryEquipmentProtocolState::default();
        let mut first = visible_equipment_claim(
            owner_id,
            0x0200,
            LiveObjectVisibleEquipmentOperation::Update,
            first_object_id,
            0x20,
        );
        first.rows[0].update_status = Some(0x11);
        let mut second = visible_equipment_claim(
            owner_id,
            0x0200,
            LiveObjectVisibleEquipmentOperation::Update,
            second_object_id,
            0x40,
        );
        second.rows[0].update_status = Some(0x22);

        protocol.observe_creature_visible_equipment_claims(&[first, second]);

        let ledger = visible_equipment_update_ledger_json(&protocol);
        assert!(ledger.starts_with("[{") && ledger.ends_with("}]"));
        assert_eq!(
            ledger.matches("},{").count(),
            1,
            "two retained rows must be adjacent JSON objects with one comma"
        );
        let separator = ledger
            .find("},{")
            .expect("two-entry ledger object separator");
        let (first_json, second_json) = ledger.split_at(separator);
        assert!(first_json.contains(concat!("\"area_update_ordinal\":1,", "\"claim_ordinal\":1")));
        assert!(first_json.contains("\"row_object_id\":2147483767"));
        assert!(first_json.contains("\"status\":17"));
        assert!(second_json.contains(concat!("\"area_update_ordinal\":2,", "\"claim_ordinal\":2")));
        assert!(second_json.contains("\"row_object_id\":2147483784"));
        assert!(second_json.contains("\"status\":34"));

        let fields = json_fields(&protocol);
        let wrapped = format!("{{\"base\":true{fields}\n}}");
        assert!(wrapped.contains(&format!("\"visible_equipment_update_ledger\": {ledger}")));
    }
}
