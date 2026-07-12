//! Wire-derived semantic session state.
//!
//! This state is a protocol coherence cache, not a game-state authority. It is
//! fed only by verified semantic packet families and should contain only the
//! facts needed to translate future traffic safely: module/resource context,
//! area/load progress, object ids/types observed on the wire, UI packet state,
//! and proxy-owned synthetic event accounting.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Bound,
    time::Instant,
};

use crate::translate::{
    VerifiedFamily,
    area::{
        AreaPlaceableContext, AreaPlaceableContextAppearanceConflict,
        AreaPlaceableContextIdentityConflict, AreaPlaceableContextOrientationConflict,
        AreaPlaceableContextOverlap, AreaPlaceableContextPositionConflict,
        AreaPlaceableContextStateConflict, AreaPlaceableObservedOrientationSource,
        AreaPlaceableObservedState,
    },
    client_gui_event, client_gui_inventory, client_input,
    client_quickbar::{self, ClientQuickbarSetButtonKind},
    live_object_update::{
        LiveObjectQuickbarItemUseCountUpdate, area_static_row_scalar_orientation, object_ids,
    },
    player_list::PlayerListObjectIds,
    quickbar::{
        QuickbarActiveItemSignature, QuickbarPreservedActiveItemSignatures, QuickbarRewriteSummary,
        QuickbarValidatedSlotProfile,
    },
};

use super::event::{
    LiveObjectBounds, LiveObjectInventoryFeature25Reference, LiveObjectMention,
    LiveObjectOrientation, LiveObjectOrientationSource, LiveObjectPlaceableAppearance,
    LiveObjectPlaceableState, LiveObjectPosition, ProtocolEvent,
};

const MAX_RECENT_EVENTS: usize = 128;
const QUICKBAR_ITEM_REFRESH_SET_BUTTON_FALLBACK_SLOT: u8 = 0;
const ITEM_OBJECT_TYPE: u8 = 0x06;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
const PLACEABLE_POSITION_EPSILON: f32 = 0.01;

#[derive(Debug, Default)]
pub(crate) struct SemanticSessionState {
    pub(crate) auth: AuthState,
    pub(crate) resources: ResourceState,
    pub(crate) module: ModuleState,
    pub(crate) area: AreaState,
    pub(crate) objects: ObjectRegistry,
    pub(crate) ui: UiState,
    pub(crate) synthetic: SyntheticState,
    pub(crate) client_input: ClientInputState,
    pub(crate) recent_events: VecDeque<ProtocolEvent>,
}

impl SemanticSessionState {
    pub(crate) fn remember_event(&mut self, event: ProtocolEvent) {
        if self.recent_events.len() >= MAX_RECENT_EVENTS {
            self.recent_events.pop_front();
        }
        self.recent_events.push_back(event);
    }

    pub(crate) fn quickbar_item_refresh_harness_hint(
        &self,
    ) -> Option<QuickbarItemRefreshHarnessHint> {
        self.ui.quickbar_item_refresh_harness_hint()
    }

    pub(crate) fn quickbar_item_refresh_harness_idle_json(&self) -> String {
        self.ui.quickbar_item_refresh_harness_idle_json()
    }

    pub(crate) fn quickbar_item_refresh_harness_idle_reason(&self) -> &'static str {
        self.ui.quickbar_item_refresh_harness_idle_reason()
    }

    pub(crate) fn trace_unresolved_quickbar_item_refresh(&self) -> bool {
        let Some(summary) = self.ui.unresolved_pending_item_refresh() else {
            return false;
        };
        let proof_class = summary
            .proof_class
            .map(QuickbarItemRefreshProofClass::as_str)
            .unwrap_or("none");
        let action_outcome = summary.action_outcome.as_str();
        let active_property_outcome = QuickbarItemRefreshActivePropertyOutcome::from_pending_state(
            summary.first_client_action_detail,
            summary.event_breakdown_after_first_client_action,
        )
        .as_str();
        let first_followup_event = summary
            .first_followup_event
            .map(QuickbarItemRefreshEventKind::as_str)
            .unwrap_or("none");
        let first_client_action = summary
            .first_client_action
            .map(QuickbarItemRefreshEventKind::as_str)
            .unwrap_or("none");
        let first_client_action_detail = summary.first_client_action_detail;
        let first_client_action_has_object_id = first_client_action_detail
            .and_then(|detail| detail.object_id)
            .is_some();
        let first_client_action_object_id = first_client_action_detail
            .and_then(|detail| detail.object_id)
            .unwrap_or(0);
        let first_client_action_slot = first_client_action_detail
            .and_then(|detail| detail.slot)
            .unwrap_or(0);
        let first_client_action_button_type = first_client_action_detail
            .and_then(|detail| detail.button_type)
            .unwrap_or(0);
        let first_client_action_body_kind = first_client_action_detail
            .and_then(|detail| detail.body_kind)
            .map(ClientQuickbarSetButtonKind::as_str)
            .unwrap_or("none");
        let first_client_action_gui_event_known = first_client_action_detail
            .and_then(|detail| detail.gui_event_a)
            .is_some();
        let first_client_action_gui_event_a = first_client_action_detail
            .and_then(|detail| detail.gui_event_a)
            .unwrap_or(0);
        let first_client_action_gui_event_b = first_client_action_detail
            .and_then(|detail| detail.gui_event_b)
            .unwrap_or(0);
        let first_client_action_gui_event_declared_bytes = first_client_action_detail
            .and_then(|detail| detail.gui_event_declared_bytes)
            .unwrap_or(0);
        let first_client_action_gui_event_trailing_fragment_bytes = first_client_action_detail
            .and_then(|detail| detail.gui_event_trailing_fragment_bytes)
            .unwrap_or(0);
        let first_client_action_gui_event_has_vector = first_client_action_detail
            .and_then(|detail| detail.gui_event_has_vector)
            .unwrap_or(false);
        let first_client_action_gui_event_vector_bits = first_client_action_detail
            .and_then(|detail| detail.gui_event_vector_bits)
            .unwrap_or([0, 0, 0]);
        let first_client_action_gui_event_vector_zero = first_client_action_detail
            .and_then(|detail| detail.gui_event_vector_bits)
            == Some([0, 0, 0]);
        let first_client_action_candidate_known = first_client_action_detail
            .and_then(|detail| detail.candidate_object_id)
            .is_some();
        let first_client_action_candidate_object_id = first_client_action_detail
            .and_then(|detail| detail.candidate_object_id)
            .unwrap_or(0);
        let first_client_action_matches_candidate = first_client_action_detail
            .and_then(|detail| detail.matches_candidate_object)
            .unwrap_or(false);
        let (recommended_set_button_slot, _) = self.ui.quickbar_item_refresh_set_button_slot();
        let pending_candidate = summary.item_context.compact_item_emission_ready_candidate;
        let first_client_action_matches_recommended_client_quickbar_set_button =
            match (first_client_action_detail, pending_candidate) {
                (Some(detail), Some(candidate)) => detail
                    .matches_recommended_client_quickbar_set_button(
                        candidate.object_id,
                        recommended_set_button_slot,
                    ),
                _ => false,
            };
        let first_client_action_matches_recommended_client_gui_event_notify =
            match (first_client_action_detail, pending_candidate) {
                (Some(detail), Some(candidate)) => {
                    detail.matches_recommended_client_gui_event_notify(candidate.object_id)
                }
                _ => false,
            };
        let first_event_after_client_action = summary
            .first_event_after_client_action
            .map(QuickbarItemRefreshEventKind::as_str)
            .unwrap_or("none");
        let compact_item_emission_candidate = summary.item_context.compact_item_emission_candidate;
        let compact_item_emission_candidate_known = compact_item_emission_candidate.is_some();
        let compact_item_emission_candidate_object_id = compact_item_emission_candidate
            .map(|candidate| candidate.object_id)
            .unwrap_or(0);
        let compact_item_emission_candidate_proof = compact_item_emission_candidate
            .map(|candidate| candidate.proof.as_str())
            .unwrap_or("none");
        let compact_item_emission_candidate_source = compact_item_emission_candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
        let first_active_item = self
            .ui
            .last_quickbar_stream_probe
            .and_then(|probe| probe.first_preserved_active_item_signature);
        let first_active_item_first_property =
            first_active_item.and_then(|signature| signature.first_property);
        let first_active_item_known = first_active_item.is_some();
        let first_active_item_matches_candidate = match (first_active_item, pending_candidate) {
            (Some(signature), Some(candidate)) => signature.object_id == candidate.object_id,
            _ => false,
        };
        let first_active_item_object_id = first_active_item
            .map(|signature| signature.object_id)
            .unwrap_or(0);
        let first_active_item_base_item = first_active_item
            .map(|signature| signature.base_item)
            .unwrap_or(0);
        let first_active_item_appearance_type = first_active_item
            .map(|signature| signature.appearance_type)
            .unwrap_or(0);
        let first_active_item_property_count = first_active_item
            .map(|signature| signature.active_property_count)
            .unwrap_or(0);
        let first_active_item_first_property_known = first_active_item_first_property.is_some();
        let first_active_item_first_property_id = first_active_item_first_property
            .map(|property| property.property)
            .unwrap_or(0);
        let first_active_item_first_property_subtype = first_active_item_first_property
            .map(|property| property.subtype)
            .unwrap_or(0);
        let first_active_item_state_mask = first_active_item
            .map(|signature| signature.state_mask)
            .unwrap_or(0);
        let first_active_item_value_mask = first_active_item
            .map(|signature| signature.value_mask)
            .unwrap_or(0);
        let first_client_action_matches_preserved_active_item = first_client_action_detail
            .map(|detail| detail.matches_preserved_active_item(first_active_item))
            .unwrap_or(false);
        let first_client_action_match_class =
            QuickbarItemRefreshClientActionMatchClass::from_pending_state(
                first_client_action_detail,
                pending_candidate.map(|candidate| candidate.object_id),
                recommended_set_button_slot,
                first_active_item,
            )
            .as_str();
        tracing::warn!(
            updates_since_committed_quickbar = summary.updates_since_committed_quickbar,
            events_since_pending_refresh = summary.events_since_pending_refresh,
            server_to_client_events_since_pending_refresh =
                summary.event_breakdown.server_to_client_events,
            client_to_server_events_since_pending_refresh =
                summary.event_breakdown.client_to_server_events,
            live_object_events_since_pending_refresh = summary.event_breakdown.live_object_events,
            quickbar_events_since_pending_refresh = summary.event_breakdown.quickbar_events,
            server_quickbar_item_use_count_events_since_pending_refresh = summary
                .event_breakdown
                .server_quickbar_item_use_count_events,
            server_quickbar_item_use_count_records_since_pending_refresh = summary
                .event_breakdown
                .server_quickbar_item_use_count_records,
            server_quickbar_item_use_count_rows_since_pending_refresh = summary
                .event_breakdown
                .server_quickbar_item_use_count_rows,
            server_quickbar_item_use_count_candidate_rows_since_pending_refresh = summary
                .event_breakdown
                .server_quickbar_item_use_count_candidate_rows,
            server_active_item_property_events_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_events,
            server_active_item_property_uses_events_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_uses_events,
            server_active_item_property_full_events_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_full_events,
            server_active_item_property_candidate_events_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_candidate_events,
            server_active_item_property_candidate_uses_events_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_candidate_uses_events,
            server_active_item_property_candidate_full_events_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_candidate_full_events,
            server_active_item_property_candidate_changed_use_count_rows_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_candidate_changed_use_count_rows,
            server_active_item_property_candidate_full_property_rows_since_pending_refresh = summary
                .event_breakdown
                .server_active_item_property_candidate_full_property_rows,
            area_events_since_pending_refresh = summary.event_breakdown.area_events,
            inventory_events_since_pending_refresh = summary.event_breakdown.inventory_events,
            client_gui_event_events_since_pending_refresh =
                summary.event_breakdown.client_gui_event_events,
            client_input_events_since_pending_refresh = summary.event_breakdown.client_input_events,
            client_input_use_item_events_since_pending_refresh =
                summary.event_breakdown.client_input_use_item_events,
            client_input_use_object_events_since_pending_refresh =
                summary.event_breakdown.client_input_use_object_events,
            client_input_change_door_state_events_since_pending_refresh = summary
                .event_breakdown
                .client_input_change_door_state_events,
            client_input_other_events_since_pending_refresh =
                summary.event_breakdown.client_input_other_events,
            client_quickbar_events_since_pending_refresh =
                summary.event_breakdown.client_quickbar_events,
            client_quickbar_item_set_button_events_since_pending_refresh = summary
                .event_breakdown
                .client_quickbar_item_set_button_events,
            client_quickbar_other_set_button_events_since_pending_refresh = summary
                .event_breakdown
                .client_quickbar_other_set_button_events,
            chat_events_since_pending_refresh = summary.event_breakdown.chat_events,
            other_events_since_pending_refresh = summary.event_breakdown.other_events,
            pending_item_refresh_proof_class = proof_class,
            pending_item_refresh_action_outcome = action_outcome,
            pending_item_refresh_active_property_outcome = active_property_outcome,
            first_followup_event,
            first_client_action,
            first_client_action_has_object_id,
            first_client_action_object_id,
            first_client_action_slot,
            first_client_action_button_type,
            first_client_action_body_kind,
            first_client_action_gui_event_known,
            first_client_action_gui_event_a,
            first_client_action_gui_event_b,
            first_client_action_gui_event_declared_bytes,
            first_client_action_gui_event_trailing_fragment_bytes,
            first_client_action_gui_event_has_vector,
            first_client_action_gui_event_vector_zero,
            first_client_action_gui_event_vector_x_bits = %format_args!(
                "0x{:08X}",
                first_client_action_gui_event_vector_bits[0]
            ),
            first_client_action_gui_event_vector_y_bits = %format_args!(
                "0x{:08X}",
                first_client_action_gui_event_vector_bits[1]
            ),
            first_client_action_gui_event_vector_z_bits = %format_args!(
                "0x{:08X}",
                first_client_action_gui_event_vector_bits[2]
            ),
            first_client_action_candidate_known,
            first_client_action_candidate_object_id,
            first_client_action_matches_candidate,
            first_client_action_matches_preserved_active_item,
            first_client_action_match_class,
            first_client_action_matches_recommended_client_quickbar_set_button,
            first_client_action_matches_recommended_client_gui_event_notify,
            first_event_after_client_action,
            events_after_first_client_action = summary.events_after_first_client_action,
            server_to_client_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_to_client_events,
            client_to_server_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_to_server_events,
            live_object_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .live_object_events,
            quickbar_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .quickbar_events,
            server_quickbar_item_use_count_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_events,
            server_quickbar_item_use_count_records_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_records,
            server_quickbar_item_use_count_rows_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_rows,
            server_quickbar_item_use_count_candidate_rows_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_candidate_rows,
            server_active_item_property_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_events,
            server_active_item_property_uses_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_uses_events,
            server_active_item_property_full_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_full_events,
            server_active_item_property_candidate_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_candidate_events,
            server_active_item_property_candidate_uses_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_candidate_uses_events,
            server_active_item_property_candidate_full_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_candidate_full_events,
            server_active_item_property_candidate_changed_use_count_rows_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_candidate_changed_use_count_rows,
            server_active_item_property_candidate_full_property_rows_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .server_active_item_property_candidate_full_property_rows,
            area_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .area_events,
            inventory_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .inventory_events,
            client_gui_event_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_gui_event_events,
            client_input_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_input_events,
            client_input_use_item_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_input_use_item_events,
            client_input_use_object_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_input_use_object_events,
            client_input_change_door_state_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_input_change_door_state_events,
            client_input_other_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_input_other_events,
            client_quickbar_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_quickbar_events,
            client_quickbar_item_set_button_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_quickbar_item_set_button_events,
            client_quickbar_other_set_button_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .client_quickbar_other_set_button_events,
            chat_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .chat_events,
            other_events_after_first_client_action = summary
                .event_breakdown_after_first_client_action
                .other_events,
            direct_item_proof_objects = summary.item_context.direct_item_proof_objects,
            feature25_item_proof_objects = summary.item_context.feature25_item_proof_objects,
            compact_item_emission_proof_objects =
                summary.item_context.compact_item_emission_proof_objects,
            compact_item_emission_ready_objects =
                summary.item_context.compact_item_emission_ready_objects,
            compact_item_emission_candidate_known,
            compact_item_emission_candidate_object_id,
            compact_item_emission_candidate_proof,
            compact_item_emission_candidate_source,
            first_preserved_active_item_known = first_active_item_known,
            first_preserved_active_item_matches_candidate = first_active_item_matches_candidate,
            first_preserved_active_item_object_id = %format_args!(
                "0x{:08X}",
                first_active_item_object_id
            ),
            first_preserved_active_item_base_item = %format_args!(
                "0x{:08X}",
                first_active_item_base_item
            ),
            first_preserved_active_item_appearance_type = first_active_item_appearance_type,
            first_preserved_active_item_property_count = first_active_item_property_count,
            first_preserved_active_item_first_property_known = first_active_item_first_property_known,
            first_preserved_active_item_first_property = first_active_item_first_property_id,
            first_preserved_active_item_first_property_subtype =
                first_active_item_first_property_subtype,
            first_preserved_active_item_state_mask = %format_args!(
                "0x{:02X}",
                first_active_item_state_mask
            ),
            first_preserved_active_item_value_mask = %format_args!(
                "0x{:02X}",
                first_active_item_value_mask
            ),
            compact_item_emission_direct_only_proof_objects = summary
                .item_context
                .compact_item_emission_direct_only_proof_objects,
            compact_item_emission_feature25_only_proof_objects = summary
                .item_context
                .compact_item_emission_feature25_only_proof_objects,
            compact_item_emission_shared_proof_objects = summary
                .item_context
                .compact_item_emission_shared_proof_objects,
            compact_item_emission_deferred_feature25_only_objects = summary
                .item_context
                .compact_item_emission_deferred_feature25_only_objects,
            inventory_feature25_first_item_refs =
                summary.item_context.inventory_feature25_first_item_refs,
            inventory_feature25_second_item_refs =
                summary.item_context.inventory_feature25_second_item_refs,
            inventory_feature25_legacy_tail_item_refs = summary
                .item_context
                .inventory_feature25_legacy_tail_item_refs,
            "semantic state ended with unresolved pending GuiQuickbar item refresh"
        );
        true
    }
}

#[derive(Debug, Default)]
pub(crate) struct AuthState {
    pub(crate) login_packets: u64,
    pub(crate) client_input_packets: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ResourceState {
    pub(crate) module_info_seen: bool,
    pub(crate) module_resource_packets: u64,
    pub(crate) module_running_packets: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ModuleState {
    pub(crate) module_info_packets: u64,
    pub(crate) module_time_packets: u64,
    pub(crate) last_module_info_declared_len: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct AreaState {
    pub(crate) client_area_packets: u64,
    pub(crate) area_loaded_packets: u64,
    pub(crate) loadbar_packets: u64,
    pub(crate) last_client_area_declared_len: Option<usize>,
    pub(crate) current_area_object_id: Option<u32>,
}

#[derive(Debug, Default)]
pub(crate) struct ClientInputState {
    pub(crate) recent_open_door_id: Option<u32>,
    pub(crate) recent_open_at: Option<Instant>,
    pub(crate) transition_door_close_rewrites: u64,
    pub(crate) transition_door_close_rewrite_skips: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryItemObjectProof {
    ActiveObject,
    Feature25FirstList,
    Feature25SecondList,
    Feature25LegacyTail,
}

impl InventoryItemObjectProof {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ActiveObject => "active_object",
            Self::Feature25FirstList => "feature25_first_list",
            Self::Feature25SecondList => "feature25_second_list",
            Self::Feature25LegacyTail => "feature25_legacy_tail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryItemObjectStatus {
    Proven(InventoryItemObjectProof),
    DeferredFeature25(InventoryItemObjectProof),
    ClearedByItemDelete,
    ClearedByAreaReset,
    Unknown,
}

impl InventoryItemObjectStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Proven(_) => "proven",
            Self::DeferredFeature25(_) => "deferred_feature25",
            Self::ClearedByItemDelete => "cleared_by_item_delete",
            Self::ClearedByAreaReset => "cleared_by_area_reset",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn proof(self) -> Option<InventoryItemObjectProof> {
        match self {
            Self::Proven(proof) | Self::DeferredFeature25(proof) => Some(proof),
            Self::ClearedByItemDelete | Self::ClearedByAreaReset | Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryItemContextCandidateSource {
    DirectOnly,
    Shared,
    Feature25Only,
}

impl InventoryItemContextCandidateSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DirectOnly => "direct_only",
            Self::Shared => "shared",
            Self::Feature25Only => "feature25_only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InventoryItemContextCandidate {
    pub(crate) object_id: u32,
    pub(crate) proof: InventoryItemObjectProof,
    pub(crate) source: InventoryItemContextCandidateSource,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct InventoryItemObjectProvenNeighbor {
    pub(crate) object_id: u32,
    pub(crate) distance: u32,
}

impl InventoryItemObjectProvenNeighbor {
    fn new(claim_object_id: u32, item_object_id: u32) -> Self {
        Self {
            object_id: item_object_id,
            distance: item_object_id.abs_diff(claim_object_id),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct InventoryItemObjectProvenNeighborhood {
    pub(crate) lower: Option<InventoryItemObjectProvenNeighbor>,
    pub(crate) higher: Option<InventoryItemObjectProvenNeighbor>,
}

impl InventoryItemObjectProvenNeighborhood {
    pub(crate) fn closest(self) -> Option<InventoryItemObjectProvenNeighbor> {
        match (self.lower, self.higher) {
            (Some(lower), Some(higher)) if higher.distance < lower.distance => Some(higher),
            (Some(lower), Some(_)) => Some(lower),
            (Some(lower), None) => Some(lower),
            (None, Some(higher)) => Some(higher),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InventoryItemObjectClearReason {
    ItemDelete,
    AreaReset,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ObjectRegistry {
    pub(crate) live_object_packets: u64,
    pub(crate) known: BTreeMap<u32, KnownObjectState>,
    session_creature_ids_by_compact: BTreeMap<u32, u32>,
    materialized_item_object_ids: BTreeSet<u32>,
    inventory_feature25_first_item_refs: BTreeSet<u32>,
    inventory_feature25_second_item_refs: BTreeSet<u32>,
    inventory_feature25_legacy_tail_item_refs: BTreeSet<u32>,
    cleared_inventory_item_object_ids: BTreeMap<u32, InventoryItemObjectClearReason>,
    pub(crate) inventory_feature25_reference_records: u64,
    pub(crate) inventory_feature25_first_item_ref_mentions: u64,
    pub(crate) inventory_feature25_second_item_ref_mentions: u64,
    pub(crate) inventory_feature25_legacy_tail_item_ref_mentions: u64,
    pub(crate) inventory_feature25_first_materialized_item_ref_mentions: u64,
    pub(crate) inventory_feature25_first_deferred_item_ref_mentions: u64,
    pub(crate) inventory_feature25_second_materialized_item_ref_mentions: u64,
    pub(crate) inventory_feature25_second_deferred_item_ref_mentions: u64,
    pub(crate) inventory_feature25_legacy_tail_materialized_item_ref_mentions: u64,
    pub(crate) inventory_feature25_legacy_tail_deferred_item_ref_mentions: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct InventoryItemContextSummary {
    pub(crate) active_item_objects: usize,
    pub(crate) materialized_item_objects: usize,
    pub(crate) direct_item_proof_objects: usize,
    pub(crate) feature25_item_proof_objects: usize,
    pub(crate) compact_item_emission_proof_objects: usize,
    pub(crate) compact_item_emission_candidate: Option<InventoryItemContextCandidate>,
    pub(crate) compact_item_emission_ready_objects: usize,
    pub(crate) compact_item_emission_ready_candidate: Option<InventoryItemContextCandidate>,
    pub(crate) preserved_active_item_actionable_missing_use_count_slot_mask: u64,
    pub(crate) compact_item_emission_direct_only_proof_objects: usize,
    pub(crate) compact_item_emission_feature25_only_proof_objects: usize,
    pub(crate) compact_item_emission_shared_proof_objects: usize,
    pub(crate) compact_item_emission_deferred_feature25_only_objects: usize,
    pub(crate) inventory_feature25_first_item_refs: usize,
    pub(crate) inventory_feature25_second_item_refs: usize,
    pub(crate) inventory_feature25_legacy_tail_item_refs: usize,
    pub(crate) cleared_inventory_item_object_ids: usize,
    pub(crate) inventory_feature25_reference_records: u64,
    pub(crate) inventory_feature25_first_item_ref_mentions: u64,
    pub(crate) inventory_feature25_second_item_ref_mentions: u64,
    pub(crate) inventory_feature25_legacy_tail_item_ref_mentions: u64,
    pub(crate) inventory_feature25_first_materialized_item_ref_mentions: u64,
    pub(crate) inventory_feature25_first_deferred_item_ref_mentions: u64,
    pub(crate) inventory_feature25_second_materialized_item_ref_mentions: u64,
    pub(crate) inventory_feature25_second_deferred_item_ref_mentions: u64,
    pub(crate) inventory_feature25_legacy_tail_materialized_item_ref_mentions: u64,
    pub(crate) inventory_feature25_legacy_tail_deferred_item_ref_mentions: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LiveObjectInventoryMaterializationSummary {
    pub(crate) live_gui_records: u32,
    pub(crate) live_gui_fragment_bits: u32,
    pub(crate) materialized_item_object_ids: Vec<u32>,
    pub(crate) compact_item_emission_ready_objects: usize,
    pub(crate) compact_item_emission_ready_candidate: Option<InventoryItemContextCandidate>,
}

impl InventoryItemContextSummary {
    pub(crate) fn preserved_active_item_actionable_missing_use_count_slot_count(&self) -> u32 {
        self.preserved_active_item_actionable_missing_use_count_slot_mask
            .count_ones()
    }

    pub(crate) fn preserved_active_item_actionable_missing_use_count_slots_json(&self) -> String {
        quickbar_slot_mask_json(self.preserved_active_item_actionable_missing_use_count_slot_mask)
    }

    pub(crate) fn has_quickbar_item_context_evidence(&self) -> bool {
        self.direct_item_proof_objects != 0
            || self.feature25_item_proof_objects != 0
            || self.cleared_inventory_item_object_ids != 0
    }

    pub(crate) fn has_compact_quickbar_item_proof(&self) -> bool {
        self.compact_item_emission_ready_objects != 0
    }

    pub(crate) fn inventory_equipment_handoff_ready(&self) -> bool {
        self.compact_item_emission_ready_objects != 0
    }

    pub(crate) fn has_deferred_feature25_refs(&self) -> bool {
        self.inventory_feature25_deferred_item_ref_mentions() != 0
            || self.compact_item_emission_deferred_feature25_only_objects != 0
    }

    pub(crate) fn inventory_feature25_item_ref_mentions(&self) -> u64 {
        self.inventory_feature25_first_item_ref_mentions
            .saturating_add(self.inventory_feature25_second_item_ref_mentions)
            .saturating_add(self.inventory_feature25_legacy_tail_item_ref_mentions)
    }

    pub(crate) fn inventory_feature25_materialized_item_ref_mentions(&self) -> u64 {
        self.inventory_feature25_first_materialized_item_ref_mentions
            .saturating_add(self.inventory_feature25_second_materialized_item_ref_mentions)
            .saturating_add(self.inventory_feature25_legacy_tail_materialized_item_ref_mentions)
    }

    pub(crate) fn inventory_feature25_deferred_item_ref_mentions(&self) -> u64 {
        self.inventory_feature25_first_deferred_item_ref_mentions
            .saturating_add(self.inventory_feature25_second_deferred_item_ref_mentions)
            .saturating_add(self.inventory_feature25_legacy_tail_deferred_item_ref_mentions)
    }

    pub(crate) fn inventory_feature25_materialization_outcome(
        &self,
    ) -> InventoryFeature25MaterializationOutcome {
        let mentions = self.inventory_feature25_item_ref_mentions();
        let materialized = self.inventory_feature25_materialized_item_ref_mentions();
        let deferred = self.inventory_feature25_deferred_item_ref_mentions();

        if mentions == 0 {
            return if self.inventory_feature25_reference_records == 0 {
                InventoryFeature25MaterializationOutcome::None
            } else {
                InventoryFeature25MaterializationOutcome::ReferencesWithoutItemMentions
            };
        }
        if materialized == 0 && deferred == mentions {
            return InventoryFeature25MaterializationOutcome::AllItemRefsDeferred;
        }
        if deferred == 0 && materialized == mentions {
            return InventoryFeature25MaterializationOutcome::AllItemRefsMaterialized;
        }
        if materialized != 0 && deferred != 0 {
            return InventoryFeature25MaterializationOutcome::MixedItemRefs;
        }
        InventoryFeature25MaterializationOutcome::UnclassifiedItemRefs
    }

    pub(crate) fn inventory_feature25_handoff_outcome(&self) -> InventoryFeature25HandoffOutcome {
        let has_ready_item_state = self.compact_item_emission_ready_objects != 0;
        match self.inventory_feature25_materialization_outcome() {
            InventoryFeature25MaterializationOutcome::None => {
                InventoryFeature25HandoffOutcome::None
            }
            InventoryFeature25MaterializationOutcome::ReferencesWithoutItemMentions => {
                InventoryFeature25HandoffOutcome::ReferencesWithoutItemMentions
            }
            InventoryFeature25MaterializationOutcome::AllItemRefsDeferred
                if has_ready_item_state =>
            {
                InventoryFeature25HandoffOutcome::AllItemRefsDeferredWithReadyItemState
            }
            InventoryFeature25MaterializationOutcome::AllItemRefsDeferred => {
                InventoryFeature25HandoffOutcome::AllItemRefsDeferredWithoutReadyItemState
            }
            InventoryFeature25MaterializationOutcome::AllItemRefsMaterialized => {
                InventoryFeature25HandoffOutcome::AllItemRefsMaterialized
            }
            InventoryFeature25MaterializationOutcome::MixedItemRefs if has_ready_item_state => {
                InventoryFeature25HandoffOutcome::MixedItemRefsWithReadyItemState
            }
            InventoryFeature25MaterializationOutcome::MixedItemRefs => {
                InventoryFeature25HandoffOutcome::MixedItemRefsWithoutReadyItemState
            }
            InventoryFeature25MaterializationOutcome::UnclassifiedItemRefs => {
                InventoryFeature25HandoffOutcome::UnclassifiedItemRefs
            }
        }
    }

    pub(crate) fn inventory_equipment_handoff_outcome(&self) -> InventoryEquipmentHandoffOutcome {
        if self.inventory_equipment_handoff_ready() {
            return if self.has_deferred_feature25_refs() {
                InventoryEquipmentHandoffOutcome::ReadyItemStateWithDeferredFeature25Refs
            } else {
                InventoryEquipmentHandoffOutcome::ReadyItemState
            };
        }

        if self.cleared_inventory_item_object_ids != 0 {
            return InventoryEquipmentHandoffOutcome::ClearedItemState;
        }

        if self.inventory_feature25_item_ref_mentions() != 0
            || self.compact_item_emission_deferred_feature25_only_objects != 0
        {
            return InventoryEquipmentHandoffOutcome::Feature25RefsWithoutReadyItemState;
        }

        if self.has_quickbar_item_context_evidence() {
            return InventoryEquipmentHandoffOutcome::ItemEvidenceWithoutReadyItemState;
        }

        InventoryEquipmentHandoffOutcome::NoItemEvidence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryEquipmentHandoffConsumer {
    ServerInventory,
    ClientGuiInventory,
    Unknown,
}

impl InventoryEquipmentHandoffConsumer {
    pub(crate) fn from_verified_family(family: VerifiedFamily) -> Self {
        match family {
            VerifiedFamily::Inventory => Self::ServerInventory,
            VerifiedFamily::ClientGuiInventory => Self::ClientGuiInventory,
            _ => Self::Unknown,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ServerInventory => "server_inventory",
            Self::ClientGuiInventory => "client_gui_inventory",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InventoryEquipmentServerInventoryClaim {
    pub(crate) minor: u8,
    pub(crate) object_id: u32,
    pub(crate) result: bool,
    pub(crate) equip_slot: u32,
}

impl InventoryEquipmentServerInventoryClaim {
    pub(crate) fn new(minor: u8, object_id: u32, result: bool, equip_slot: u32) -> Self {
        Self {
            minor,
            object_id,
            result,
            equip_slot,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryEquipmentClientGuiInventoryClaimKind {
    Status,
    SelectPanel,
}

impl InventoryEquipmentClientGuiInventoryClaimKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::SelectPanel => "select_panel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InventoryEquipmentClientGuiInventoryClaim {
    pub(crate) kind: InventoryEquipmentClientGuiInventoryClaimKind,
    pub(crate) object_id: Option<u32>,
    pub(crate) panel: Option<u8>,
    pub(crate) player_inventory_gui: Option<bool>,
    pub(crate) rewritten_self_object_id: bool,
}

impl From<client_gui_inventory::ClientGuiInventoryClaimSummary>
    for InventoryEquipmentClientGuiInventoryClaim
{
    fn from(claim: client_gui_inventory::ClientGuiInventoryClaimSummary) -> Self {
        let kind = match claim.kind {
            client_gui_inventory::ClientGuiInventoryKind::Status => {
                InventoryEquipmentClientGuiInventoryClaimKind::Status
            }
            client_gui_inventory::ClientGuiInventoryKind::SelectPanel => {
                InventoryEquipmentClientGuiInventoryClaimKind::SelectPanel
            }
        };
        Self {
            kind,
            object_id: claim.object_id,
            panel: claim.panel,
            player_inventory_gui: claim.player_inventory_gui,
            rewritten_self_object_id: claim.rewritten_self_object_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InventoryEquipmentHandoffSnapshot {
    pub(crate) consumer: InventoryEquipmentHandoffConsumer,
    pub(crate) item_context: InventoryItemContextSummary,
    pub(crate) event_index: u64,
    pub(crate) server_inventory_claim: Option<InventoryEquipmentServerInventoryClaim>,
    pub(crate) client_gui_inventory_claim: Option<InventoryEquipmentClientGuiInventoryClaim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryEquipmentHandoffBridgeAction {
    None,
    EmitReadyItemState,
    ReadyItemStateWithoutCandidate,
}

impl InventoryEquipmentHandoffBridgeAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::EmitReadyItemState => "emit_ready_item_state",
            Self::ReadyItemStateWithoutCandidate => "ready_item_state_without_candidate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InventoryEquipmentHandoffBridgePlan {
    pub(crate) action: InventoryEquipmentHandoffBridgeAction,
    pub(crate) consumer: InventoryEquipmentHandoffConsumer,
    pub(crate) event_index: u64,
    pub(crate) outcome: InventoryEquipmentHandoffOutcome,
    pub(crate) ready_objects: usize,
    pub(crate) deferred_feature25_only_objects: usize,
    pub(crate) candidate: Option<InventoryItemContextCandidate>,
    pub(crate) server_inventory_claim: Option<InventoryEquipmentServerInventoryClaim>,
    pub(crate) client_gui_inventory_claim: Option<InventoryEquipmentClientGuiInventoryClaim>,
}

impl Default for InventoryEquipmentHandoffBridgePlan {
    fn default() -> Self {
        Self {
            action: InventoryEquipmentHandoffBridgeAction::None,
            consumer: InventoryEquipmentHandoffConsumer::Unknown,
            event_index: 0,
            outcome: InventoryEquipmentHandoffOutcome::NoItemEvidence,
            ready_objects: 0,
            deferred_feature25_only_objects: 0,
            candidate: None,
            server_inventory_claim: None,
            client_gui_inventory_claim: None,
        }
    }
}

impl InventoryEquipmentHandoffBridgePlan {
    fn from_ready_snapshot(snapshot: InventoryEquipmentHandoffSnapshot) -> Self {
        let item_context = snapshot.item_context;
        let candidate = item_context.compact_item_emission_ready_candidate;
        let action = if candidate.is_some() {
            InventoryEquipmentHandoffBridgeAction::EmitReadyItemState
        } else {
            InventoryEquipmentHandoffBridgeAction::ReadyItemStateWithoutCandidate
        };
        Self {
            action,
            consumer: snapshot.consumer,
            event_index: snapshot.event_index,
            outcome: item_context.inventory_equipment_handoff_outcome(),
            ready_objects: item_context.compact_item_emission_ready_objects,
            deferred_feature25_only_objects: item_context
                .compact_item_emission_deferred_feature25_only_objects,
            candidate,
            server_inventory_claim: snapshot.server_inventory_claim,
            client_gui_inventory_claim: snapshot.client_gui_inventory_claim,
        }
    }

    pub(crate) fn ready_to_emit(self) -> bool {
        self.action == InventoryEquipmentHandoffBridgeAction::EmitReadyItemState
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InventoryEquipmentHandoffBridgeEmission {
    pub(crate) emission_index: u64,
    pub(crate) plan: InventoryEquipmentHandoffBridgePlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InventoryEquipmentBridgeStateUpdate {
    pub(crate) update_index: u64,
    pub(crate) emission_index: u64,
    pub(crate) consumer: InventoryEquipmentHandoffConsumer,
    pub(crate) event_index: u64,
    pub(crate) candidate: InventoryItemContextCandidate,
    pub(crate) ready_objects: usize,
    pub(crate) deferred_feature25_only_objects: usize,
    pub(crate) server_inventory_claim: Option<InventoryEquipmentServerInventoryClaim>,
    pub(crate) client_gui_inventory_claim: Option<InventoryEquipmentClientGuiInventoryClaim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryFeature25MaterializationOutcome {
    None,
    ReferencesWithoutItemMentions,
    AllItemRefsDeferred,
    AllItemRefsMaterialized,
    MixedItemRefs,
    UnclassifiedItemRefs,
}

impl InventoryFeature25MaterializationOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ReferencesWithoutItemMentions => "references_without_item_mentions",
            Self::AllItemRefsDeferred => "all_item_refs_deferred",
            Self::AllItemRefsMaterialized => "all_item_refs_materialized",
            Self::MixedItemRefs => "mixed_item_refs",
            Self::UnclassifiedItemRefs => "unclassified_item_refs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryFeature25HandoffOutcome {
    None,
    ReferencesWithoutItemMentions,
    AllItemRefsDeferredWithReadyItemState,
    AllItemRefsDeferredWithoutReadyItemState,
    AllItemRefsMaterialized,
    MixedItemRefsWithReadyItemState,
    MixedItemRefsWithoutReadyItemState,
    UnclassifiedItemRefs,
}

impl InventoryFeature25HandoffOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ReferencesWithoutItemMentions => "references_without_item_mentions",
            Self::AllItemRefsDeferredWithReadyItemState => {
                "all_item_refs_deferred_with_ready_item_state"
            }
            Self::AllItemRefsDeferredWithoutReadyItemState => {
                "all_item_refs_deferred_without_ready_item_state"
            }
            Self::AllItemRefsMaterialized => "all_item_refs_materialized",
            Self::MixedItemRefsWithReadyItemState => "mixed_item_refs_with_ready_item_state",
            Self::MixedItemRefsWithoutReadyItemState => "mixed_item_refs_without_ready_item_state",
            Self::UnclassifiedItemRefs => "unclassified_item_refs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryEquipmentHandoffOutcome {
    NoItemEvidence,
    ReadyItemState,
    ReadyItemStateWithDeferredFeature25Refs,
    Feature25RefsWithoutReadyItemState,
    ClearedItemState,
    ItemEvidenceWithoutReadyItemState,
}

impl InventoryEquipmentHandoffOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoItemEvidence => "no_item_evidence",
            Self::ReadyItemState => "ready_item_state",
            Self::ReadyItemStateWithDeferredFeature25Refs => {
                "ready_item_state_with_deferred_feature25_refs"
            }
            Self::Feature25RefsWithoutReadyItemState => "feature25_refs_without_ready_item_state",
            Self::ClearedItemState => "cleared_item_state",
            Self::ItemEvidenceWithoutReadyItemState => "item_evidence_without_ready_item_state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemContextSource {
    Current,
    Prior,
    PreviousPost,
}

impl QuickbarItemContextSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Prior => "prior",
            Self::PreviousPost => "previous_post",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshOutcome {
    NoPendingRefresh,
    PendingRefreshStillBlank,
    PendingRefreshEmittedItemSlots,
    PendingRefreshObservedUseCountRows,
    PendingRefreshResolvedByUseCountState,
}

impl Default for QuickbarItemRefreshOutcome {
    fn default() -> Self {
        Self::NoPendingRefresh
    }
}

impl QuickbarItemRefreshOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoPendingRefresh => "no_pending_refresh",
            Self::PendingRefreshStillBlank => "pending_refresh_still_blank",
            Self::PendingRefreshEmittedItemSlots => "pending_refresh_emitted_item_slots",
            Self::PendingRefreshObservedUseCountRows => "pending_refresh_observed_use_count_rows",
            Self::PendingRefreshResolvedByUseCountState => {
                "pending_refresh_resolved_by_use_count_state"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshActionOutcome {
    AwaitingClientAction,
    ServerQuickbarResponseBeforeFirstClientAction,
    FirstClientActionTargetUnknown,
    FirstClientActionTargetsOtherObject,
    CandidateClientActionNoServerQuickbar,
    CandidateClientActionObservedServerQuickbar,
}

impl Default for QuickbarItemRefreshActionOutcome {
    fn default() -> Self {
        Self::AwaitingClientAction
    }
}

impl QuickbarItemRefreshActionOutcome {
    pub(crate) fn from_pending_state(
        first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
        event_breakdown_before_first_client_action: QuickbarItemRefreshEventBreakdown,
        event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown,
    ) -> Self {
        if event_breakdown_before_first_client_action.has_server_quickbar_response() {
            return Self::ServerQuickbarResponseBeforeFirstClientAction;
        }
        let Some(detail) = first_client_action_detail else {
            return Self::AwaitingClientAction;
        };
        match detail.matches_candidate_object {
            Some(true)
                if event_breakdown_after_first_client_action.has_server_quickbar_response() =>
            {
                Self::CandidateClientActionObservedServerQuickbar
            }
            Some(true) => Self::CandidateClientActionNoServerQuickbar,
            Some(false) => Self::FirstClientActionTargetsOtherObject,
            None => Self::FirstClientActionTargetUnknown,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingClientAction => "awaiting_client_action",
            Self::ServerQuickbarResponseBeforeFirstClientAction => {
                "server_quickbar_response_before_first_client_action"
            }
            Self::FirstClientActionTargetUnknown => "first_client_action_target_unknown",
            Self::FirstClientActionTargetsOtherObject => "first_client_action_targets_other_object",
            Self::CandidateClientActionNoServerQuickbar => {
                "candidate_client_action_no_server_quickbar"
            }
            Self::CandidateClientActionObservedServerQuickbar => {
                "candidate_client_action_observed_server_quickbar"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshClientActionMatchClass {
    AwaitingClientAction,
    TargetUnknown,
    OtherObject,
    CandidateObject,
    PreservedActiveItem,
    RecommendedUseItem,
    RecommendedUseItemFirstPropertySubtypeLow,
    RecommendedSetButton,
    RecommendedGuiEventNotify,
    RecommendedUseObject,
}

impl Default for QuickbarItemRefreshClientActionMatchClass {
    fn default() -> Self {
        Self::AwaitingClientAction
    }
}

impl QuickbarItemRefreshClientActionMatchClass {
    pub(crate) fn from_pending_state(
        first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
        candidate_object_id: Option<u32>,
        recommended_set_button_slot: u8,
        first_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
    ) -> Self {
        let Some(detail) = first_client_action_detail else {
            return Self::AwaitingClientAction;
        };
        if detail.object_id.is_none() {
            return Self::TargetUnknown;
        }
        if let Some(candidate_object_id) = candidate_object_id {
            if detail.matches_recommended_client_use_item(candidate_object_id) {
                return Self::RecommendedUseItem;
            }
            if detail.matches_recommended_client_use_item_first_property_subtype_low(
                candidate_object_id,
                first_preserved_active_item_signature,
            ) {
                return Self::RecommendedUseItemFirstPropertySubtypeLow;
            }
            if detail.matches_recommended_client_gui_event_notify(candidate_object_id) {
                return Self::RecommendedGuiEventNotify;
            }
            if detail.matches_recommended_client_use_object(candidate_object_id) {
                return Self::RecommendedUseObject;
            }
            if detail.matches_recommended_client_quickbar_set_button(
                candidate_object_id,
                recommended_set_button_slot,
            ) {
                return Self::RecommendedSetButton;
            }
        }
        if detail.matches_preserved_active_item(first_preserved_active_item_signature) {
            return Self::PreservedActiveItem;
        }
        match detail.matches_candidate_object {
            Some(true) => Self::CandidateObject,
            Some(false) => Self::OtherObject,
            None => Self::TargetUnknown,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingClientAction => "awaiting_client_action",
            Self::TargetUnknown => "target_unknown",
            Self::OtherObject => "other_object",
            Self::CandidateObject => "candidate_object",
            Self::PreservedActiveItem => "preserved_active_item",
            Self::RecommendedUseItem => "recommended_use_item",
            Self::RecommendedUseItemFirstPropertySubtypeLow => {
                "recommended_use_item_first_property_subtype_low"
            }
            Self::RecommendedSetButton => "recommended_set_button",
            Self::RecommendedGuiEventNotify => "recommended_gui_event_notify",
            Self::RecommendedUseObject => "recommended_use_object",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshRecommendedActionOutcome {
    AwaitingClientAction,
    NoRecommendedClientAction,
    ServerQuickbarResponseBeforeRecommendedAction,
    RecommendedUseItemNoServerQuickbar,
    RecommendedUseItemObservedServerQuickbar,
    RecommendedUseItemFirstPropertySubtypeLowNoServerQuickbar,
    RecommendedUseItemFirstPropertySubtypeLowObservedServerQuickbar,
    RecommendedSetButtonNoServerQuickbar,
    RecommendedSetButtonObservedServerQuickbar,
    RecommendedGuiEventNotifyNoServerQuickbar,
    RecommendedGuiEventNotifyObservedServerQuickbar,
    RecommendedUseObjectNoServerQuickbar,
    RecommendedUseObjectObservedServerQuickbar,
}

impl Default for QuickbarItemRefreshRecommendedActionOutcome {
    fn default() -> Self {
        Self::AwaitingClientAction
    }
}

impl QuickbarItemRefreshRecommendedActionOutcome {
    pub(crate) fn from_pending_state(
        first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
        candidate_object_id: Option<u32>,
        recommended_set_button_slot: u8,
        first_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
        event_breakdown_before_first_client_action: QuickbarItemRefreshEventBreakdown,
        event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown,
    ) -> Self {
        if event_breakdown_before_first_client_action.has_server_quickbar_response() {
            return Self::ServerQuickbarResponseBeforeRecommendedAction;
        }
        let Some(detail) = first_client_action_detail else {
            return Self::AwaitingClientAction;
        };
        let Some(candidate_object_id) = candidate_object_id else {
            return Self::NoRecommendedClientAction;
        };
        let observed_server_quickbar =
            event_breakdown_after_first_client_action.has_server_quickbar_response();
        if detail.matches_recommended_client_use_item(candidate_object_id) {
            return if observed_server_quickbar {
                Self::RecommendedUseItemObservedServerQuickbar
            } else {
                Self::RecommendedUseItemNoServerQuickbar
            };
        }
        if detail.matches_recommended_client_use_item_first_property_subtype_low(
            candidate_object_id,
            first_preserved_active_item_signature,
        ) {
            return if observed_server_quickbar {
                Self::RecommendedUseItemFirstPropertySubtypeLowObservedServerQuickbar
            } else {
                Self::RecommendedUseItemFirstPropertySubtypeLowNoServerQuickbar
            };
        }
        if detail.matches_recommended_client_gui_event_notify(candidate_object_id) {
            return if observed_server_quickbar {
                Self::RecommendedGuiEventNotifyObservedServerQuickbar
            } else {
                Self::RecommendedGuiEventNotifyNoServerQuickbar
            };
        }
        if detail.matches_recommended_client_use_object(candidate_object_id) {
            return if observed_server_quickbar {
                Self::RecommendedUseObjectObservedServerQuickbar
            } else {
                Self::RecommendedUseObjectNoServerQuickbar
            };
        }
        if detail.matches_recommended_client_quickbar_set_button(
            candidate_object_id,
            recommended_set_button_slot,
        ) {
            return if observed_server_quickbar {
                Self::RecommendedSetButtonObservedServerQuickbar
            } else {
                Self::RecommendedSetButtonNoServerQuickbar
            };
        }
        Self::NoRecommendedClientAction
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingClientAction => "awaiting_client_action",
            Self::NoRecommendedClientAction => "no_recommended_client_action",
            Self::ServerQuickbarResponseBeforeRecommendedAction => {
                "server_quickbar_response_before_recommended_action"
            }
            Self::RecommendedUseItemNoServerQuickbar => "recommended_use_item_no_server_quickbar",
            Self::RecommendedUseItemObservedServerQuickbar => {
                "recommended_use_item_observed_server_quickbar"
            }
            Self::RecommendedUseItemFirstPropertySubtypeLowNoServerQuickbar => {
                "recommended_use_item_first_property_subtype_low_no_server_quickbar"
            }
            Self::RecommendedUseItemFirstPropertySubtypeLowObservedServerQuickbar => {
                "recommended_use_item_first_property_subtype_low_observed_server_quickbar"
            }
            Self::RecommendedSetButtonNoServerQuickbar => {
                "recommended_set_button_no_server_quickbar"
            }
            Self::RecommendedSetButtonObservedServerQuickbar => {
                "recommended_set_button_observed_server_quickbar"
            }
            Self::RecommendedGuiEventNotifyNoServerQuickbar => {
                "recommended_gui_event_notify_no_server_quickbar"
            }
            Self::RecommendedGuiEventNotifyObservedServerQuickbar => {
                "recommended_gui_event_notify_observed_server_quickbar"
            }
            Self::RecommendedUseObjectNoServerQuickbar => {
                "recommended_use_object_no_server_quickbar"
            }
            Self::RecommendedUseObjectObservedServerQuickbar => {
                "recommended_use_object_observed_server_quickbar"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshActivePropertyOutcome {
    AwaitingClientAction,
    FirstClientActionTargetUnknown,
    FirstClientActionTargetsOtherObject,
    CandidateClientActionNoActivePropertyResponse,
    CandidateClientActionObservedUsesDelta,
    CandidateClientActionObservedFullRefresh,
    CandidateClientActionObservedUsesAndFullRefresh,
}

impl Default for QuickbarItemRefreshActivePropertyOutcome {
    fn default() -> Self {
        Self::AwaitingClientAction
    }
}

impl QuickbarItemRefreshActivePropertyOutcome {
    pub(crate) fn from_pending_state(
        first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
        event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown,
    ) -> Self {
        let Some(detail) = first_client_action_detail else {
            return Self::AwaitingClientAction;
        };
        match detail.matches_candidate_object {
            Some(true) => {
                let observed_uses = event_breakdown_after_first_client_action
                    .server_active_item_property_candidate_uses_events
                    != 0;
                let observed_full = event_breakdown_after_first_client_action
                    .server_active_item_property_candidate_full_events
                    != 0;
                match (observed_uses, observed_full) {
                    (true, true) => Self::CandidateClientActionObservedUsesAndFullRefresh,
                    (true, false) => Self::CandidateClientActionObservedUsesDelta,
                    (false, true) => Self::CandidateClientActionObservedFullRefresh,
                    (false, false) => Self::CandidateClientActionNoActivePropertyResponse,
                }
            }
            Some(false) => Self::FirstClientActionTargetsOtherObject,
            None => Self::FirstClientActionTargetUnknown,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingClientAction => "awaiting_client_action",
            Self::FirstClientActionTargetUnknown => "first_client_action_target_unknown",
            Self::FirstClientActionTargetsOtherObject => "first_client_action_targets_other_object",
            Self::CandidateClientActionNoActivePropertyResponse => {
                "candidate_client_action_no_active_property_response"
            }
            Self::CandidateClientActionObservedUsesDelta => {
                "candidate_client_action_observed_active_property_uses_delta"
            }
            Self::CandidateClientActionObservedFullRefresh => {
                "candidate_client_action_observed_active_property_full_refresh"
            }
            Self::CandidateClientActionObservedUsesAndFullRefresh => {
                "candidate_client_action_observed_active_property_uses_and_full_refresh"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshServerQuickbarResponseTiming {
    AwaitingClientAction,
    NoServerQuickbarResponse,
    ServerQuickbarResponseBeforeFirstClientAction,
    ServerQuickbarResponseAfterFirstClientAction,
    ServerQuickbarResponseBeforeAndAfterFirstClientAction,
}

impl Default for QuickbarItemRefreshServerQuickbarResponseTiming {
    fn default() -> Self {
        Self::AwaitingClientAction
    }
}

impl QuickbarItemRefreshServerQuickbarResponseTiming {
    pub(crate) fn from_pending_state(
        first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
        event_breakdown_before_first_client_action: QuickbarItemRefreshEventBreakdown,
        event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown,
    ) -> Self {
        if first_client_action_detail.is_none() {
            return Self::AwaitingClientAction;
        }
        match (
            event_breakdown_before_first_client_action.has_server_quickbar_response(),
            event_breakdown_after_first_client_action.has_server_quickbar_response(),
        ) {
            (false, false) => Self::NoServerQuickbarResponse,
            (true, false) => Self::ServerQuickbarResponseBeforeFirstClientAction,
            (false, true) => Self::ServerQuickbarResponseAfterFirstClientAction,
            (true, true) => Self::ServerQuickbarResponseBeforeAndAfterFirstClientAction,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingClientAction => "awaiting_client_action",
            Self::NoServerQuickbarResponse => "no_server_quickbar_response",
            Self::ServerQuickbarResponseBeforeFirstClientAction => {
                "server_quickbar_response_before_first_client_action"
            }
            Self::ServerQuickbarResponseAfterFirstClientAction => {
                "server_quickbar_response_after_first_client_action"
            }
            Self::ServerQuickbarResponseBeforeAndAfterFirstClientAction => {
                "server_quickbar_response_before_and_after_first_client_action"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshClientActionTiming {
    AwaitingClientAction,
    ImmediateAfterProof,
    DelayedAfterPendingFollowup,
}

impl Default for QuickbarItemRefreshClientActionTiming {
    fn default() -> Self {
        Self::AwaitingClientAction
    }
}

impl QuickbarItemRefreshClientActionTiming {
    pub(crate) fn from_pending_state(
        first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
        followup_events_before_first_client_action: u64,
    ) -> Self {
        if first_client_action_detail.is_none() {
            return Self::AwaitingClientAction;
        }
        if followup_events_before_first_client_action == 0 {
            Self::ImmediateAfterProof
        } else {
            Self::DelayedAfterPendingFollowup
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingClientAction => "awaiting_client_action",
            Self::ImmediateAfterProof => "immediate_after_proof",
            Self::DelayedAfterPendingFollowup => "delayed_after_pending_followup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshProofClass {
    DirectOnly,
    Feature25Only,
    Shared,
    Mixed,
}

impl QuickbarItemRefreshProofClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DirectOnly => "direct_only",
            Self::Feature25Only => "feature25_only",
            Self::Shared => "shared",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshEventKind {
    LiveObject,
    ServerQuickbar,
    ServerQuickbarItemUseCount,
    ServerActiveItemProperties,
    Area,
    Inventory,
    ClientGuiEventNotify,
    ClientInputUseItem,
    ClientInputUseObject,
    ClientInputChangeDoorState,
    ClientInputOther,
    ClientQuickbarItemSetButton,
    ClientQuickbarOtherSetButton,
    Chat,
    Other,
}

impl QuickbarItemRefreshEventKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LiveObject => "live_object",
            Self::ServerQuickbar => "server_quickbar",
            Self::ServerQuickbarItemUseCount => "server_quickbar_item_use_count",
            Self::ServerActiveItemProperties => "server_active_item_properties",
            Self::Area => "area",
            Self::Inventory => "inventory",
            Self::ClientGuiEventNotify => "client_gui_event_notify",
            Self::ClientInputUseItem => "client_input_use_item",
            Self::ClientInputUseObject => "client_input_use_object",
            Self::ClientInputChangeDoorState => "client_input_change_door_state",
            Self::ClientInputOther => "client_input_other",
            Self::ClientQuickbarItemSetButton => "client_quickbar_item_set_button",
            Self::ClientQuickbarOtherSetButton => "client_quickbar_other_set_button",
            Self::Chat => "chat",
            Self::Other => "other",
        }
    }

    pub(crate) fn is_client_action(self) -> bool {
        matches!(
            self,
            Self::ClientInputUseItem
                | Self::ClientInputUseObject
                | Self::ClientInputChangeDoorState
                | Self::ClientInputOther
                | Self::ClientGuiEventNotify
                | Self::ClientQuickbarItemSetButton
                | Self::ClientQuickbarOtherSetButton
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuickbarPendingItemRefreshSummary {
    pub(crate) item_context: InventoryItemContextSummary,
    pub(crate) updates_since_committed_quickbar: u64,
    pub(crate) events_since_pending_refresh: u64,
    pub(crate) event_breakdown: QuickbarItemRefreshEventBreakdown,
    pub(crate) events_after_first_client_action: u64,
    pub(crate) event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown,
    pub(crate) first_candidate_use_count_row: Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) first_candidate_use_count_row_before_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) first_candidate_use_count_row_after_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) action_outcome: QuickbarItemRefreshActionOutcome,
    pub(crate) followup_events_before_first_client_action: u64,
    pub(crate) proof_class: Option<QuickbarItemRefreshProofClass>,
    pub(crate) first_followup_event: Option<QuickbarItemRefreshEventKind>,
    pub(crate) first_client_action: Option<QuickbarItemRefreshEventKind>,
    pub(crate) first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
    pub(crate) first_event_after_client_action: Option<QuickbarItemRefreshEventKind>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QuickbarStreamProbeSummary {
    pub(crate) slot_records_owned: u32,
    pub(crate) item_buttons_seen: u32,
    pub(crate) item_buttons_source_compact: u32,
    pub(crate) item_buttons_preserved: u32,
    pub(crate) item_buttons_blanked: u32,
    pub(crate) item_buttons_blanked_candidate: u32,
    pub(crate) item_buttons_rejected_missing_state_proof: u32,
    pub(crate) item_buttons_rejected_missing_state_unknown: u32,
    pub(crate) item_buttons_rejected_missing_state_cleared_delete: u32,
    pub(crate) item_buttons_rejected_missing_state_cleared_area_reset: u32,
    pub(crate) item_objects_rejected_missing_state_proven: u32,
    pub(crate) item_objects_rejected_missing_state_active: u32,
    pub(crate) item_objects_rejected_missing_state_feature25_first: u32,
    pub(crate) item_objects_rejected_missing_state_feature25_second: u32,
    pub(crate) item_objects_rejected_missing_state_feature25_legacy_tail: u32,
    pub(crate) item_objects_rejected_missing_state_unknown: u32,
    pub(crate) item_objects_rejected_missing_state_cleared_delete: u32,
    pub(crate) item_objects_rejected_missing_state_cleared_area_reset: u32,
    pub(crate) item_objects_preserved_by_explicit_self_materialization: u32,
    pub(crate) item_objects_preserved_by_active_state: u32,
    pub(crate) item_objects_preserved_by_feature25_first: u32,
    pub(crate) item_objects_preserved_by_feature25_second: u32,
    pub(crate) item_objects_preserved_by_feature25_legacy_tail: u32,
    pub(crate) preserved_active_item_signatures: QuickbarPreservedActiveItemSignatures,
    pub(crate) first_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
    pub(crate) first_preserved_active_item_slot: Option<u8>,
}

impl QuickbarStreamProbeSummary {
    fn from_rewrite_summary(summary: &QuickbarRewriteSummary) -> Self {
        Self {
            slot_records_owned: summary.slot_records_owned,
            item_buttons_seen: summary.item_buttons_seen,
            item_buttons_source_compact: summary.item_buttons_source_compact,
            item_buttons_preserved: summary.item_buttons_preserved,
            item_buttons_blanked: summary.item_buttons_blanked,
            item_buttons_blanked_candidate: summary.item_buttons_blanked_candidate,
            item_buttons_rejected_missing_state_proof: summary
                .item_buttons_rejected_missing_state_proof,
            item_buttons_rejected_missing_state_unknown: summary
                .item_buttons_rejected_missing_state_unknown,
            item_buttons_rejected_missing_state_cleared_delete: summary
                .item_buttons_rejected_missing_state_cleared_delete,
            item_buttons_rejected_missing_state_cleared_area_reset: summary
                .item_buttons_rejected_missing_state_cleared_area_reset,
            item_objects_rejected_missing_state_proven: summary
                .item_objects_rejected_missing_state_proven,
            item_objects_rejected_missing_state_active: summary
                .item_objects_rejected_missing_state_active,
            item_objects_rejected_missing_state_feature25_first: summary
                .item_objects_rejected_missing_state_feature25_first,
            item_objects_rejected_missing_state_feature25_second: summary
                .item_objects_rejected_missing_state_feature25_second,
            item_objects_rejected_missing_state_feature25_legacy_tail: summary
                .item_objects_rejected_missing_state_feature25_legacy_tail,
            item_objects_rejected_missing_state_unknown: summary
                .item_objects_rejected_missing_state_unknown,
            item_objects_rejected_missing_state_cleared_delete: summary
                .item_objects_rejected_missing_state_cleared_delete,
            item_objects_rejected_missing_state_cleared_area_reset: summary
                .item_objects_rejected_missing_state_cleared_area_reset,
            item_objects_preserved_by_explicit_self_materialization: summary
                .item_objects_preserved_by_explicit_self_materialization,
            item_objects_preserved_by_active_state: summary.item_objects_preserved_by_active_state,
            item_objects_preserved_by_feature25_first: summary
                .item_objects_preserved_by_feature25_first,
            item_objects_preserved_by_feature25_second: summary
                .item_objects_preserved_by_feature25_second,
            item_objects_preserved_by_feature25_legacy_tail: summary
                .item_objects_preserved_by_feature25_legacy_tail,
            preserved_active_item_signatures: summary.preserved_active_item_signatures,
            first_preserved_active_item_signature: summary.first_preserved_active_item_signature,
            first_preserved_active_item_slot: summary.first_preserved_active_item_slot,
        }
    }

    fn preserved_active_item_for_object(
        self,
        object_id: u32,
    ) -> Option<(u8, QuickbarActiveItemSignature)> {
        self.preserved_active_item_signatures
            .0
            .iter()
            .enumerate()
            .find_map(|(slot, signature)| {
                let signature = (*signature)?;
                (signature.object_id == object_id).then_some((u8::try_from(slot).ok()?, signature))
            })
    }

    fn preserved_active_item_signature_count(self) -> usize {
        self.preserved_active_item_signatures
            .0
            .iter()
            .flatten()
            .count()
    }

    fn preserved_active_item_slots_json(self) -> String {
        let slots = self
            .preserved_active_item_signatures
            .0
            .iter()
            .enumerate()
            .filter_map(|(slot, signature)| signature.map(|_| slot.to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{slots}]")
    }

    fn preserved_active_item_use_count_coverage(
        self,
        use_count_state: &BTreeMap<QuickbarItemUseCountKey, QuickbarItemRefreshUseCountRow>,
    ) -> QuickbarPreservedActiveItemUseCountCoverage {
        let mut coverage = QuickbarPreservedActiveItemUseCountCoverage::default();
        for (slot, signature) in self
            .preserved_active_item_signatures
            .0
            .iter()
            .copied()
            .enumerate()
        {
            let Some(signature) = signature else {
                continue;
            };
            let Ok(slot) = u8::try_from(slot) else {
                continue;
            };
            // GQ is the decompile-owned quickbar use-count row. Durable state
            // satisfies an active slot only when the wire slot, item object,
            // and button type all agree; an unrelated row for the same item
            // must not suppress the action probe for this slot.
            let has_matching_use_count = use_count_state.values().any(|row| {
                row.object_id == signature.object_id
                    && row.slot == slot
                    && row.button_type == client_quickbar::ITEM_SET_BUTTON_TYPE
            });
            coverage.observe(slot, has_matching_use_count);
        }
        coverage
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QuickbarPreservedActiveItemUseCountCoverage {
    matching_use_count_slot_mask: u64,
    missing_use_count_slot_mask: u64,
}

impl QuickbarPreservedActiveItemUseCountCoverage {
    fn observe(&mut self, slot: u8, has_matching_use_count: bool) {
        let bit = 1u64.checked_shl(u32::from(slot)).unwrap_or(0);
        if has_matching_use_count {
            self.matching_use_count_slot_mask |= bit;
        } else {
            self.missing_use_count_slot_mask |= bit;
        }
    }

    fn matching_use_count_slot_count(self) -> u32 {
        self.matching_use_count_slot_mask.count_ones()
    }

    fn missing_use_count_slot_count(self) -> u32 {
        self.missing_use_count_slot_mask.count_ones()
    }

    fn matching_use_count_slots_json(self) -> String {
        quickbar_slot_mask_json(self.matching_use_count_slot_mask)
    }

    fn missing_use_count_slots_json(self) -> String {
        quickbar_slot_mask_json(self.missing_use_count_slot_mask)
    }

    fn missing_use_count_slot(self, slot: u8) -> bool {
        let bit = 1u64.checked_shl(u32::from(slot)).unwrap_or(0);
        self.missing_use_count_slot_mask & bit != 0
    }
}

fn quickbar_slot_mask_json(mask: u64) -> String {
    let slots = (0..u64::BITS)
        .filter(|slot| mask & (1u64 << slot) != 0)
        .map(|slot| slot.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{slots}]")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuickbarItemRefreshHarnessHint {
    pub(crate) candidate: InventoryItemContextCandidate,
    pub(crate) recommended_set_button_slot: u8,
    pub(crate) recommended_set_button_slot_source: &'static str,
    pub(crate) stream_probe: QuickbarStreamProbeSummary,
    pub(crate) first_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
    pub(crate) first_preserved_active_item_slot: Option<u8>,
    pub(crate) candidate_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
    pub(crate) candidate_preserved_active_item_slot: Option<u8>,
    pub(crate) candidate_use_count_state: Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) first_preserved_active_item_use_count_state: Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) quickbar_item_use_count_state_rows: usize,
    pub(crate) quickbar_item_use_count_updates_observed: u64,
    pub(crate) preserved_active_item_use_count_coverage:
        QuickbarPreservedActiveItemUseCountCoverage,
    pub(crate) preserved_active_item_actionable_missing_use_count_slot_mask: u64,
    pub(crate) observed_preserved_active_item_actionable_missing_use_count_slot_mask: u64,
    pub(crate) updates_since_committed_quickbar: u64,
    pub(crate) events_since_pending_refresh: u64,
    pub(crate) event_breakdown: QuickbarItemRefreshEventBreakdown,
    pub(crate) events_after_first_client_action: u64,
    pub(crate) event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown,
    pub(crate) first_candidate_use_count_row: Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) first_candidate_use_count_row_before_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) first_candidate_use_count_row_after_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) action_outcome: QuickbarItemRefreshActionOutcome,
    pub(crate) followup_events_before_first_client_action: u64,
    pub(crate) proof_class: Option<QuickbarItemRefreshProofClass>,
    pub(crate) first_followup_event: Option<QuickbarItemRefreshEventKind>,
    pub(crate) first_client_action: Option<QuickbarItemRefreshEventKind>,
    pub(crate) first_client_action_detail: Option<QuickbarItemRefreshClientActionDetail>,
    pub(crate) first_event_after_client_action: Option<QuickbarItemRefreshEventKind>,
    pub(crate) direct_item_proof_objects: usize,
    pub(crate) feature25_item_proof_objects: usize,
    pub(crate) compact_item_emission_proof_objects: usize,
    pub(crate) compact_item_emission_direct_only_proof_objects: usize,
    pub(crate) compact_item_emission_feature25_only_proof_objects: usize,
    pub(crate) compact_item_emission_shared_proof_objects: usize,
    pub(crate) item_context: InventoryItemContextSummary,
    pub(crate) inventory_equipment_handoff_events: u64,
    pub(crate) inventory_equipment_handoff_ready_events: u64,
    pub(crate) inventory_equipment_handoff_blocked_without_ready_state_events: u64,
    pub(crate) inventory_equipment_handoff_ready_with_deferred_feature25_events: u64,
    pub(crate) inventory_equipment_handoff_server_inventory_events: u64,
    pub(crate) inventory_equipment_handoff_server_inventory_ready_events: u64,
    pub(crate) inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events: u64,
    pub(crate) inventory_equipment_handoff_client_gui_inventory_events: u64,
    pub(crate) inventory_equipment_handoff_client_gui_inventory_ready_events: u64,
    pub(crate) inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events:
        u64,
    pub(crate) inventory_equipment_handoff_bridge_plan: InventoryEquipmentHandoffBridgePlan,
    pub(crate) inventory_equipment_bridge_handoff_emissions: u64,
    pub(crate) last_inventory_equipment_bridge_handoff_emission:
        Option<InventoryEquipmentHandoffBridgeEmission>,
    pub(crate) inventory_equipment_bridge_handoff_state_updates: u64,
    pub(crate) last_inventory_equipment_bridge_handoff_state_update:
        Option<InventoryEquipmentBridgeStateUpdate>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QuickbarItemRefreshUseCountRow {
    pub(crate) slot: u8,
    pub(crate) button_type: u8,
    pub(crate) object_id: u32,
    pub(crate) active_property_index: u8,
    pub(crate) use_count: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct QuickbarItemUseCountKey {
    pub(crate) slot: u8,
    pub(crate) button_type: u8,
    pub(crate) object_id: u32,
    pub(crate) active_property_index: u8,
}

impl QuickbarItemRefreshUseCountRow {
    fn state_key(self) -> QuickbarItemUseCountKey {
        QuickbarItemUseCountKey {
            slot: self.slot,
            button_type: self.button_type,
            object_id: self.object_id,
            active_property_index: self.active_property_index,
        }
    }
}

impl From<LiveObjectQuickbarItemUseCountUpdate> for QuickbarItemRefreshUseCountRow {
    fn from(update: LiveObjectQuickbarItemUseCountUpdate) -> Self {
        Self {
            slot: update.slot,
            button_type: update.button_type,
            object_id: update.object_id,
            active_property_index: update.active_property_index,
            use_count: update.use_count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemRefreshUseCountSlotRelation {
    NoCandidateUseCountRow,
    NoPreservedActiveItemSlot,
    MatchesPreservedActiveItemSlot,
    DiffersFromPreservedActiveItemSlot,
}

impl Default for QuickbarItemRefreshUseCountSlotRelation {
    fn default() -> Self {
        Self::NoCandidateUseCountRow
    }
}

impl QuickbarItemRefreshUseCountSlotRelation {
    pub(crate) fn from_row_and_preserved_slot(
        row: Option<QuickbarItemRefreshUseCountRow>,
        preserved_slot: Option<u8>,
    ) -> Self {
        let Some(row) = row else {
            return Self::NoCandidateUseCountRow;
        };
        let Some(preserved_slot) = preserved_slot else {
            return Self::NoPreservedActiveItemSlot;
        };
        if row.slot == preserved_slot {
            Self::MatchesPreservedActiveItemSlot
        } else {
            Self::DiffersFromPreservedActiveItemSlot
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoCandidateUseCountRow => "no_candidate_use_count_row",
            Self::NoPreservedActiveItemSlot => "no_preserved_active_item_slot",
            Self::MatchesPreservedActiveItemSlot => "matches_preserved_active_item_slot",
            Self::DiffersFromPreservedActiveItemSlot => "differs_from_preserved_active_item_slot",
        }
    }

    pub(crate) fn matches_preserved_active_item_slot(self) -> bool {
        self == Self::MatchesPreservedActiveItemSlot
    }
}

fn quickbar_item_refresh_use_count_row_timing(
    first: Option<QuickbarItemRefreshUseCountRow>,
    before_first_client_action: Option<QuickbarItemRefreshUseCountRow>,
    after_first_client_action: Option<QuickbarItemRefreshUseCountRow>,
) -> &'static str {
    match (first, before_first_client_action, after_first_client_action) {
        (None, _, _) => "none",
        (Some(first), Some(before), _) if first == before => "before_first_client_action",
        (Some(first), _, Some(after)) if first == after => "after_first_client_action",
        (Some(_), _, _) => "unknown",
    }
}

impl QuickbarItemRefreshHarnessHint {
    pub(crate) fn to_json(self) -> String {
        let first_client_action_detail = self.first_client_action_detail;
        let stream_probe = self.stream_probe;
        let preserved_active_item_signature_count =
            stream_probe.preserved_active_item_signature_count();
        let preserved_active_item_slots_json = stream_probe.preserved_active_item_slots_json();
        let use_count_coverage = self.preserved_active_item_use_count_coverage;
        let preserved_active_item_matching_use_count_slots_json =
            use_count_coverage.matching_use_count_slots_json();
        let preserved_active_item_missing_use_count_slots_json =
            use_count_coverage.missing_use_count_slots_json();
        let preserved_active_item_actionable_missing_use_count_slot_count = self
            .preserved_active_item_actionable_missing_use_count_slot_mask
            .count_ones();
        let preserved_active_item_actionable_missing_use_count_slots_json = quickbar_slot_mask_json(
            self.preserved_active_item_actionable_missing_use_count_slot_mask,
        );
        let observed_preserved_active_item_actionable_missing_use_count_slot_count = self
            .observed_preserved_active_item_actionable_missing_use_count_slot_mask
            .count_ones();
        let observed_preserved_active_item_actionable_missing_use_count_slots_json =
            quickbar_slot_mask_json(
                self.observed_preserved_active_item_actionable_missing_use_count_slot_mask,
            );
        let first_active_item = self.first_preserved_active_item_signature;
        let action_active_item = self.candidate_preserved_active_item_signature;
        let action_active_item_slot = self.candidate_preserved_active_item_slot;
        let first_active_item_first_property =
            first_active_item.and_then(|signature| signature.first_property);
        let first_active_item_known = first_active_item.is_some();
        let first_active_item_matches_candidate = first_active_item
            .map(|signature| signature.object_id == self.candidate.object_id)
            .unwrap_or(false);
        let first_active_item_slot_known = self.first_preserved_active_item_slot.is_some();
        let first_active_item_slot = self.first_preserved_active_item_slot.unwrap_or(0);
        let first_active_item_first_page_slot = self
            .first_preserved_active_item_slot
            .is_some_and(|slot| slot < 12);
        let first_active_item_slot_matches_recommended_set_button_slot = self
            .first_preserved_active_item_slot
            .map(|slot| slot == self.recommended_set_button_slot)
            .unwrap_or(false);
        let first_property_subtype_low_byte = first_property_subtype_low_byte_for_candidate(
            action_active_item,
            self.candidate.object_id,
        );
        let recommended_use_item_payload =
            crate::translate::client_input::build_self_target_use_item_payload(
                self.candidate.object_id,
            );
        let recommended_use_item_payload_available = recommended_use_item_payload.is_some();
        let recommended_use_item_payload_hex = recommended_use_item_payload
            .as_deref()
            .map(hex_encode_upper)
            .unwrap_or_default();
        let recommended_use_item_first_property_subtype_low_payload =
            first_property_subtype_low_byte.and_then(|active_property_subtype| {
                client_input::build_self_target_use_item_payload_with_active_property_byte(
                    self.candidate.object_id,
                    active_property_subtype,
                )
            });
        let recommended_use_item_first_property_subtype_low_payload_available =
            recommended_use_item_first_property_subtype_low_payload.is_some();
        let recommended_use_item_first_property_subtype_low_payload_hex =
            recommended_use_item_first_property_subtype_low_payload
                .as_deref()
                .map(hex_encode_upper)
                .unwrap_or_default();
        let recommended_use_item_first_property_subtype_low_byte_known =
            first_property_subtype_low_byte.is_some();
        let recommended_use_item_first_property_subtype_low_byte =
            first_property_subtype_low_byte.unwrap_or(0);
        let recommended_use_item_first_property_subtype_low_source =
            if recommended_use_item_first_property_subtype_low_byte_known {
                "first_preserved_active_item_first_property_subtype_low_byte"
            } else {
                "none"
            };
        let recommended_use_item_first_property_subtype_low_matches_default =
            first_property_subtype_low_byte == Some(0);
        let recommended_set_button_payload = client_quickbar::build_item_set_button_payload(
            self.recommended_set_button_slot,
            self.candidate.object_id,
            None,
        );
        let recommended_set_button_payload_available = recommended_set_button_payload.is_some();
        let recommended_set_button_payload_hex = recommended_set_button_payload
            .as_deref()
            .map(hex_encode_upper)
            .unwrap_or_default();
        let recommended_gui_event_notify_payload =
            client_gui_event::build_radial_notify_probe_payload(self.candidate.object_id);
        let recommended_gui_event_notify_payload_available =
            recommended_gui_event_notify_payload.is_some();
        let recommended_gui_event_notify_payload_hex = recommended_gui_event_notify_payload
            .as_deref()
            .map(hex_encode_upper)
            .unwrap_or_default();
        let recommended_use_object_payload =
            client_input::build_use_object_payload(self.candidate.object_id, false, false);
        let recommended_use_object_payload_available = recommended_use_object_payload.is_some();
        let recommended_use_object_payload_hex = recommended_use_object_payload
            .as_deref()
            .map(hex_encode_upper)
            .unwrap_or_default();
        let first_active_item_object_id = first_active_item
            .map(|signature| signature.object_id)
            .unwrap_or(0);
        let first_active_item_base_item = first_active_item
            .map(|signature| signature.base_item)
            .unwrap_or(0);
        let first_active_item_appearance_type = first_active_item
            .map(|signature| signature.appearance_type)
            .unwrap_or(0);
        let first_active_item_property_count = first_active_item
            .map(|signature| signature.active_property_count)
            .unwrap_or(0);
        let first_active_item_first_property_known = first_active_item_first_property.is_some();
        let first_active_item_first_property_id = first_active_item_first_property
            .map(|property| property.property)
            .unwrap_or(0);
        let first_active_item_first_property_subtype = first_active_item_first_property
            .map(|property| property.subtype)
            .unwrap_or(0);
        let first_active_item_first_property_cost_table_value = first_active_item_first_property
            .map(|property| property.cost_table_value)
            .unwrap_or(0);
        let first_active_item_first_property_param = first_active_item_first_property
            .map(|property| property.param)
            .unwrap_or(0);
        let first_active_item_has_armor_word = first_active_item
            .map(|signature| signature.has_armor_word)
            .unwrap_or(false);
        let first_active_item_name_is_locstring = first_active_item
            .map(|signature| signature.name_is_locstring)
            .unwrap_or(false);
        let first_active_item_state_mask = first_active_item
            .map(|signature| signature.state_mask)
            .unwrap_or(0);
        let first_active_item_value_mask = first_active_item
            .map(|signature| signature.value_mask)
            .unwrap_or(0);
        let first_client_action_has_object_id = first_client_action_detail
            .and_then(|detail| detail.object_id)
            .is_some();
        let first_client_action_object_id = first_client_action_detail
            .and_then(|detail| detail.object_id)
            .unwrap_or(0);
        let first_client_action_slot = first_client_action_detail
            .and_then(|detail| detail.slot)
            .unwrap_or(0);
        let first_client_action_button_type = first_client_action_detail
            .and_then(|detail| detail.button_type)
            .unwrap_or(0);
        let first_client_action_body_kind = first_client_action_detail
            .and_then(|detail| detail.body_kind)
            .map(ClientQuickbarSetButtonKind::as_str)
            .unwrap_or("none");
        let first_client_action_gui_event_known = first_client_action_detail
            .and_then(|detail| detail.gui_event_a)
            .is_some();
        let first_client_action_gui_event_a = first_client_action_detail
            .and_then(|detail| detail.gui_event_a)
            .unwrap_or(0);
        let first_client_action_gui_event_b = first_client_action_detail
            .and_then(|detail| detail.gui_event_b)
            .unwrap_or(0);
        let first_client_action_gui_event_declared_bytes = first_client_action_detail
            .and_then(|detail| detail.gui_event_declared_bytes)
            .unwrap_or(0);
        let first_client_action_gui_event_trailing_fragment_bytes = first_client_action_detail
            .and_then(|detail| detail.gui_event_trailing_fragment_bytes)
            .unwrap_or(0);
        let first_client_action_gui_event_has_vector = first_client_action_detail
            .and_then(|detail| detail.gui_event_has_vector)
            .unwrap_or(false);
        let first_client_action_gui_event_vector_bits = first_client_action_detail
            .and_then(|detail| detail.gui_event_vector_bits)
            .unwrap_or([0, 0, 0]);
        let first_client_action_gui_event_vector_zero = first_client_action_detail
            .and_then(|detail| detail.gui_event_vector_bits)
            == Some([0, 0, 0]);
        let first_client_action_use_item_known = first_client_action_detail
            .and_then(|detail| detail.use_item_active_property_subtype)
            .is_some();
        let first_client_action_use_item_active_property_subtype = first_client_action_detail
            .and_then(|detail| detail.use_item_active_property_subtype)
            .unwrap_or(0);
        let first_client_action_use_item_has_optional_byte = first_client_action_detail
            .and_then(|detail| detail.use_item_has_optional_byte)
            .unwrap_or(false);
        let first_client_action_use_item_has_target_object = first_client_action_detail
            .and_then(|detail| detail.use_item_has_target_object)
            .unwrap_or(false);
        let first_client_action_use_item_target_object_id = first_client_action_detail
            .and_then(|detail| detail.use_item_target_object_id)
            .unwrap_or(0);
        let first_client_action_use_item_target_is_self_or_legacy_self = matches!(
            first_client_action_detail.and_then(|detail| detail.use_item_target_object_id),
            Some(client_input::EE_SELF_OBJECT_ID) | Some(client_input::INVALID_OBJECT_ID)
        );
        let first_client_action_use_item_has_position = first_client_action_detail
            .and_then(|detail| detail.use_item_has_position)
            .unwrap_or(false);
        let first_client_action_candidate_known = first_client_action_detail
            .and_then(|detail| detail.candidate_object_id)
            .is_some();
        let first_client_action_candidate_object_id = first_client_action_detail
            .and_then(|detail| detail.candidate_object_id)
            .unwrap_or(0);
        let first_client_action_matches_candidate = first_client_action_detail
            .and_then(|detail| detail.matches_candidate_object)
            .unwrap_or(false);
        let first_client_action_matches_preserved_active_item = first_client_action_detail
            .map(|detail| detail.matches_preserved_active_item(action_active_item))
            .unwrap_or(false);
        let first_client_action_matches_recommended_client_quickbar_set_button =
            first_client_action_detail
                .map(|detail| {
                    detail.matches_recommended_client_quickbar_set_button(
                        self.candidate.object_id,
                        self.recommended_set_button_slot,
                    )
                })
                .unwrap_or(false);
        let first_client_action_matches_recommended_client_gui_event_notify =
            first_client_action_detail
                .map(|detail| {
                    detail.matches_recommended_client_gui_event_notify(self.candidate.object_id)
                })
                .unwrap_or(false);
        let first_client_action_matches_recommended_client_use_item = first_client_action_detail
            .map(|detail| detail.matches_recommended_client_use_item(self.candidate.object_id))
            .unwrap_or(false);
        let first_client_action_matches_recommended_client_use_item_first_property_subtype_low =
            first_client_action_detail
                .map(|detail| {
                    detail.matches_recommended_client_use_item_first_property_subtype_low(
                        self.candidate.object_id,
                        action_active_item,
                    )
                })
                .unwrap_or(false);
        let first_client_action_matches_recommended_client_use_object = first_client_action_detail
            .map(|detail| detail.matches_recommended_client_use_object(self.candidate.object_id))
            .unwrap_or(false);
        let first_client_action_match_class =
            QuickbarItemRefreshClientActionMatchClass::from_pending_state(
                first_client_action_detail,
                Some(self.candidate.object_id),
                self.recommended_set_button_slot,
                action_active_item,
            )
            .as_str();
        let first_event_after_client_action = self
            .first_event_after_client_action
            .map(QuickbarItemRefreshEventKind::as_str)
            .unwrap_or("none");
        let event_breakdown_before_first_client_action = self
            .event_breakdown
            .saturating_sub(self.event_breakdown_after_first_client_action);
        let server_quickbar_response_before_first_client_action =
            event_breakdown_before_first_client_action.has_server_quickbar_response();
        let candidate_use_count_state_slot_relation =
            QuickbarItemRefreshUseCountSlotRelation::from_row_and_preserved_slot(
                self.candidate_use_count_state,
                self.first_preserved_active_item_slot,
            );
        let first_active_item_use_count_state_slot_relation =
            QuickbarItemRefreshUseCountSlotRelation::from_row_and_preserved_slot(
                self.first_preserved_active_item_use_count_state,
                self.first_preserved_active_item_slot,
            );
        let candidate_is_preserved_active_item = action_active_item.is_some();
        let matching_candidate_use_count_state = candidate_is_preserved_active_item
            && self.candidate_use_count_state.is_some_and(|row| {
                Some(row.slot) == action_active_item_slot
                    && row.button_type == client_quickbar::ITEM_SET_BUTTON_TYPE
            });
        let matching_preserved_active_item_use_count_state = !candidate_is_preserved_active_item
            && !candidate_use_count_state_slot_relation.matches_preserved_active_item_slot()
            && first_active_item_use_count_state_slot_relation.matches_preserved_active_item_slot()
            && self
                .first_preserved_active_item_use_count_state
                .is_some_and(|row| row.button_type == client_quickbar::ITEM_SET_BUTTON_TYPE);
        let recommended_client_action_should_dispatch = candidate_is_preserved_active_item
            && !server_quickbar_response_before_first_client_action
            && !matching_candidate_use_count_state
            && !matching_preserved_active_item_use_count_state;
        let recommended_client_action_suppressed_reason = if !candidate_is_preserved_active_item {
            "candidate_not_preserved_active_item"
        } else if server_quickbar_response_before_first_client_action {
            "server_quickbar_response_before_first_client_action"
        } else if matching_candidate_use_count_state {
            "matching_quickbar_use_count_state"
        } else if matching_preserved_active_item_use_count_state {
            "preserved_active_item_quickbar_use_count_state_candidate_mismatch"
        } else {
            "none"
        };
        let action_outcome = self.action_outcome.as_str();
        let recommended_action_outcome =
            QuickbarItemRefreshRecommendedActionOutcome::from_pending_state(
                first_client_action_detail,
                Some(self.candidate.object_id),
                self.recommended_set_button_slot,
                action_active_item,
                event_breakdown_before_first_client_action,
                self.event_breakdown_after_first_client_action,
            )
            .as_str();
        let active_property_outcome = QuickbarItemRefreshActivePropertyOutcome::from_pending_state(
            first_client_action_detail,
            self.event_breakdown_after_first_client_action,
        )
        .as_str();
        let server_quickbar_response_timing =
            QuickbarItemRefreshServerQuickbarResponseTiming::from_pending_state(
                first_client_action_detail,
                event_breakdown_before_first_client_action,
                self.event_breakdown_after_first_client_action,
            )
            .as_str();
        let first_candidate_use_count_row = self.first_candidate_use_count_row.unwrap_or_default();
        let first_candidate_use_count_row_before_first_client_action = self
            .first_candidate_use_count_row_before_first_client_action
            .unwrap_or_default();
        let first_candidate_use_count_row_after_first_client_action = self
            .first_candidate_use_count_row_after_first_client_action
            .unwrap_or_default();
        let first_candidate_use_count_row_timing = quickbar_item_refresh_use_count_row_timing(
            self.first_candidate_use_count_row,
            self.first_candidate_use_count_row_before_first_client_action,
            self.first_candidate_use_count_row_after_first_client_action,
        );
        let first_candidate_use_count_row_slot_relation =
            QuickbarItemRefreshUseCountSlotRelation::from_row_and_preserved_slot(
                self.first_candidate_use_count_row,
                self.first_preserved_active_item_slot,
            );
        let candidate_use_count_state = self.candidate_use_count_state.unwrap_or_default();
        let first_active_item_use_count_state = self
            .first_preserved_active_item_use_count_state
            .unwrap_or_default();
        let first_client_action_timing = QuickbarItemRefreshClientActionTiming::from_pending_state(
            first_client_action_detail,
            self.followup_events_before_first_client_action,
        )
        .as_str();
        let bridge_plan = self.inventory_equipment_handoff_bridge_plan;
        let bridge_candidate = bridge_plan.candidate;
        let bridge_candidate_known = bridge_candidate.is_some();
        let bridge_candidate_object_id = bridge_candidate
            .map(|candidate| candidate.object_id)
            .unwrap_or(0);
        let bridge_candidate_proof = bridge_candidate
            .map(|candidate| candidate.proof.as_str())
            .unwrap_or("none");
        let bridge_candidate_source = bridge_candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
        let last_bridge_emission = self.last_inventory_equipment_bridge_handoff_emission;
        let last_bridge_emission_known = last_bridge_emission.is_some();
        let last_bridge_emission_index = last_bridge_emission
            .map(|emission| emission.emission_index)
            .unwrap_or(0);
        let last_bridge_emission_plan = last_bridge_emission
            .map(|emission| emission.plan)
            .unwrap_or_default();
        let last_bridge_emission_candidate = last_bridge_emission_plan.candidate;
        let last_bridge_emission_candidate_object_id = last_bridge_emission_candidate
            .map(|candidate| candidate.object_id)
            .unwrap_or(0);
        let last_bridge_emission_candidate_source = last_bridge_emission_candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
        let last_bridge_state_update = self.last_inventory_equipment_bridge_handoff_state_update;
        let last_bridge_state_update_known = last_bridge_state_update.is_some();
        let last_bridge_state_update_index = last_bridge_state_update
            .map(|update| update.update_index)
            .unwrap_or(0);
        let last_bridge_state_update_emission_index = last_bridge_state_update
            .map(|update| update.emission_index)
            .unwrap_or(0);
        let last_bridge_state_update_consumer = last_bridge_state_update
            .map(|update| update.consumer.as_str())
            .unwrap_or("unknown");
        let last_bridge_state_update_event_index = last_bridge_state_update
            .map(|update| update.event_index)
            .unwrap_or(0);
        let last_bridge_state_update_candidate_object_id = last_bridge_state_update
            .map(|update| update.candidate.object_id)
            .unwrap_or(0);
        let last_bridge_state_update_candidate_proof = last_bridge_state_update
            .map(|update| update.candidate.proof.as_str())
            .unwrap_or("none");
        let last_bridge_state_update_candidate_source = last_bridge_state_update
            .map(|update| update.candidate.source.as_str())
            .unwrap_or("none");
        let last_bridge_state_update_ready_objects = last_bridge_state_update
            .map(|update| update.ready_objects)
            .unwrap_or(0);
        let last_bridge_state_update_deferred_feature25_only_objects = last_bridge_state_update
            .map(|update| update.deferred_feature25_only_objects)
            .unwrap_or(0);
        format!(
            concat!(
                "{{\n",
                "  \"kind\": \"quickbar_item_refresh_candidate\",\n",
                "  \"pending_item_refresh\": true,\n",
                "  \"post_committed_item_refresh_resolution\": \"pending\",\n",
                "  \"candidate_object_id\": {},\n",
                "  \"candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"candidate_proof\": \"{}\",\n",
                "  \"candidate_source\": \"{}\",\n",
                "  \"first_preserved_active_item_known\": {},\n",
                "  \"first_preserved_active_item_matches_candidate\": {},\n",
                "  \"first_preserved_active_item_slot_known\": {},\n",
                "  \"first_preserved_active_item_slot\": {},\n",
                "  \"first_preserved_active_item_first_page_slot\": {},\n",
                "  \"first_preserved_active_item_slot_matches_recommended_set_button_slot\": {},\n",
                "  \"first_preserved_active_item_object_id\": {},\n",
                "  \"first_preserved_active_item_object_id_hex\": \"0x{:08X}\",\n",
                "  \"first_preserved_active_item_base_item\": {},\n",
                "  \"first_preserved_active_item_base_item_hex\": \"0x{:08X}\",\n",
                "  \"first_preserved_active_item_appearance_type\": {},\n",
                "  \"first_preserved_active_item_property_count\": {},\n",
                "  \"first_preserved_active_item_first_property_known\": {},\n",
                "  \"first_preserved_active_item_first_property\": {},\n",
                "  \"first_preserved_active_item_first_property_subtype\": {},\n",
                "  \"first_preserved_active_item_first_property_cost_table_value\": {},\n",
                "  \"first_preserved_active_item_first_property_param\": {},\n",
                "  \"first_preserved_active_item_has_armor_word\": {},\n",
                "  \"first_preserved_active_item_name_is_locstring\": {},\n",
                "  \"first_preserved_active_item_state_mask\": {},\n",
                "  \"first_preserved_active_item_state_mask_hex\": \"0x{:02X}\",\n",
                "  \"first_preserved_active_item_value_mask\": {},\n",
                "  \"first_preserved_active_item_value_mask_hex\": \"0x{:02X}\",\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_proof\": {},\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_unknown\": {},\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_cleared_delete\": {},\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_cleared_area_reset\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_proven\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_active\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_feature25_first\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_feature25_second\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_feature25_legacy_tail\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_unknown\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_cleared_delete\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_cleared_area_reset\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_explicit_self_materialization\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_active_state\": {},\n",
                "  \"stream_probe_preserved_active_item_signature_count\": {},\n",
                "  \"stream_probe_preserved_active_item_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_matching_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_matching_use_count_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_missing_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_missing_use_count_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_actionable_missing_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_actionable_missing_use_count_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_observed_actionable_missing_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_observed_actionable_missing_use_count_slots\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_feature25_first\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_feature25_second\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_feature25_legacy_tail\": {},\n",
                "  \"recommended_client_action\": \"target_candidate_with_use_item_use_object_quickbar_set_button_or_gui_event_notify_probe\",\n",
                "  \"recommended_client_action_should_dispatch\": {},\n",
                "  \"recommended_client_action_suppressed_reason\": \"{}\",\n",
                "  \"recommended_use_item_payload_available\": {},\n",
                "  \"recommended_use_item_payload_kind\": \"Input_UseItem\",\n",
                "  \"recommended_use_item_payload_hex\": \"{}\",\n",
                "  \"recommended_use_item_item_object_id\": {},\n",
                "  \"recommended_use_item_item_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_use_item_active_property_subtype\": 0,\n",
                "  \"recommended_use_item_has_optional_byte\": false,\n",
                "  \"recommended_use_item_has_target_object\": true,\n",
                "  \"recommended_use_item_target_object_id\": {},\n",
                "  \"recommended_use_item_target_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_use_item_target_legacy_rewrite_object_id\": {},\n",
                "  \"recommended_use_item_target_legacy_rewrite_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_use_item_has_position\": false,\n",
                "  \"recommended_use_item_first_property_subtype_low_payload_available\": {},\n",
                "  \"recommended_use_item_first_property_subtype_low_payload_kind\": \"Input_UseItem\",\n",
                "  \"recommended_use_item_first_property_subtype_low_payload_hex\": \"{}\",\n",
                "  \"recommended_use_item_first_property_subtype_low_byte_known\": {},\n",
                "  \"recommended_use_item_first_property_subtype_low_byte\": {},\n",
                "  \"recommended_use_item_first_property_subtype_low_source\": \"{}\",\n",
                "  \"recommended_use_item_first_property_subtype_low_matches_default\": {},\n",
                "  \"recommended_use_item_first_property_subtype_low_has_optional_byte\": false,\n",
                "  \"recommended_use_item_first_property_subtype_low_has_target_object\": true,\n",
                "  \"recommended_use_item_first_property_subtype_low_target_object_id\": {},\n",
                "  \"recommended_use_item_first_property_subtype_low_target_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_use_item_first_property_subtype_low_target_legacy_rewrite_object_id\": {},\n",
                "  \"recommended_use_item_first_property_subtype_low_target_legacy_rewrite_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_use_item_first_property_subtype_low_has_position\": false,\n",
                "  \"recommended_client_quickbar_set_button_payload_available\": {},\n",
                "  \"recommended_client_quickbar_set_button_payload_kind\": \"GuiQuickbar_SetButton\",\n",
                "  \"recommended_client_quickbar_set_button_payload_hex\": \"{}\",\n",
                "  \"recommended_client_quickbar_set_button_slot\": {},\n",
                "  \"recommended_client_quickbar_set_button_slot_source\": \"{}\",\n",
                "  \"recommended_client_quickbar_set_button_button_type\": {},\n",
                "  \"recommended_client_quickbar_set_button_item_object_id\": {},\n",
                "  \"recommended_client_quickbar_set_button_item_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_client_quickbar_set_button_int_param\": {},\n",
                "  \"recommended_client_quickbar_set_button_has_target_object\": false,\n",
                "  \"recommended_client_gui_event_notify_payload_available\": {},\n",
                "  \"recommended_client_gui_event_notify_payload_kind\": \"GuiEvent_Notify\",\n",
                "  \"recommended_client_gui_event_notify_payload_hex\": \"{}\",\n",
                "  \"recommended_client_gui_event_notify_event_a\": {},\n",
                "  \"recommended_client_gui_event_notify_event_b\": {},\n",
                "  \"recommended_client_gui_event_notify_object_id\": {},\n",
                "  \"recommended_client_gui_event_notify_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_client_gui_event_notify_has_vector\": true,\n",
                "  \"recommended_client_gui_event_notify_vector_x\": 0.0,\n",
                "  \"recommended_client_gui_event_notify_vector_y\": 0.0,\n",
                "  \"recommended_client_gui_event_notify_vector_z\": 0.0,\n",
                "  \"recommended_client_use_object_payload_available\": {},\n",
                "  \"recommended_client_use_object_payload_kind\": \"Input_UseObject\",\n",
                "  \"recommended_client_use_object_payload_hex\": \"{}\",\n",
                "  \"recommended_client_use_object_object_id\": {},\n",
                "  \"recommended_client_use_object_object_id_hex\": \"0x{:08X}\",\n",
                "  \"recommended_client_use_object_mark_inventory_gui_state\": false,\n",
                "  \"recommended_client_use_object_schedule_script_event\": false,\n",
                "  \"updates_since_committed_quickbar\": {},\n",
                "  \"events_since_pending_refresh\": {},\n",
                "  \"server_to_client_events_since_pending_refresh\": {},\n",
                "  \"client_to_server_events_since_pending_refresh\": {},\n",
                "  \"pending_item_refresh_proof_class\": \"{}\",\n",
                "  \"pending_item_refresh_action_outcome\": \"{}\",\n",
                "  \"pending_item_refresh_recommended_action_outcome\": \"{}\",\n",
                "  \"pending_item_refresh_active_property_outcome\": \"{}\",\n",
                "  \"pending_item_refresh_server_quickbar_response_timing\": \"{}\",\n",
                "  \"quickbar_item_use_count_state_rows\": {},\n",
                "  \"quickbar_item_use_count_updates_observed\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_known\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_slot_relation\": \"{}\",\n",
                "  \"candidate_quickbar_item_use_count_state_slot_matches_first_preserved_active_item\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_slot\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_button_type\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_object_id\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_object_id_hex\": \"0x{:08X}\",\n",
                "  \"candidate_quickbar_item_use_count_state_active_property_index\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_use_count\": {},\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_known\": {},\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_slot_relation\": \"{}\",\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_slot_matches_first_preserved_active_item\": {},\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_slot\": {},\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_button_type\": {},\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_object_id\": {},\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_object_id_hex\": \"0x{:08X}\",\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_active_property_index\": {},\n",
                "  \"first_preserved_active_item_quickbar_use_count_state_use_count\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_known\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_timing\": \"{}\",\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_slot_relation\": \"{}\",\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_slot_matches_first_preserved_active_item\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_slot\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_button_type\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_object_id\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_object_id_hex\": \"0x{:08X}\",\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_active_property_index\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_use_count\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_known\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_slot\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_button_type\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_active_property_index\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_use_count\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_known\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_slot\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_button_type\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_active_property_index\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_use_count\": {},\n",
                "  \"first_client_action_timing\": \"{}\",\n",
                "  \"followup_events_before_first_client_action\": {},\n",
                "  \"first_followup_event\": \"{}\",\n",
                "  \"first_client_action\": \"{}\",\n",
                "  \"first_client_action_has_object_id\": {},\n",
                "  \"first_client_action_object_id\": {},\n",
                "  \"first_client_action_slot\": {},\n",
                "  \"first_client_action_button_type\": {},\n",
                "  \"first_client_action_body_kind\": \"{}\",\n",
                "  \"first_client_action_gui_event_known\": {},\n",
                "  \"first_client_action_gui_event_a\": {},\n",
                "  \"first_client_action_gui_event_b\": {},\n",
                "  \"first_client_action_gui_event_declared_bytes\": {},\n",
                "  \"first_client_action_gui_event_trailing_fragment_bytes\": {},\n",
                "  \"first_client_action_gui_event_has_vector\": {},\n",
                "  \"first_client_action_gui_event_vector_zero\": {},\n",
                "  \"first_client_action_gui_event_vector_x_bits_hex\": \"0x{:08X}\",\n",
                "  \"first_client_action_gui_event_vector_y_bits_hex\": \"0x{:08X}\",\n",
                "  \"first_client_action_gui_event_vector_z_bits_hex\": \"0x{:08X}\",\n",
                "  \"first_client_action_use_item_known\": {},\n",
                "  \"first_client_action_use_item_active_property_subtype\": {},\n",
                "  \"first_client_action_use_item_has_optional_byte\": {},\n",
                "  \"first_client_action_use_item_has_target_object\": {},\n",
                "  \"first_client_action_use_item_target_object_id\": {},\n",
                "  \"first_client_action_use_item_target_object_id_hex\": \"0x{:08X}\",\n",
                "  \"first_client_action_use_item_target_is_self_or_legacy_self\": {},\n",
                "  \"first_client_action_use_item_has_position\": {},\n",
                "  \"first_client_action_candidate_known\": {},\n",
                "  \"first_client_action_candidate_object_id\": {},\n",
                "  \"first_client_action_matches_candidate\": {},\n",
                "  \"first_client_action_matches_preserved_active_item\": {},\n",
                "  \"first_client_action_match_class\": \"{}\",\n",
                "  \"first_client_action_matches_recommended_client_use_item\": {},\n",
                "  \"first_client_action_matches_recommended_client_use_item_first_property_subtype_low\": {},\n",
                "  \"first_client_action_matches_recommended_client_quickbar_set_button\": {},\n",
                "  \"first_client_action_matches_recommended_client_gui_event_notify\": {},\n",
                "  \"first_client_action_matches_recommended_client_use_object\": {},\n",
                "  \"first_event_after_client_action\": \"{}\",\n",
                "  \"events_after_first_client_action\": {},\n",
                "  \"server_to_client_events_after_first_client_action\": {},\n",
                "  \"client_to_server_events_after_first_client_action\": {},\n",
                "  \"live_object_events_after_first_client_action\": {},\n",
                "  \"quickbar_events_after_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_events_after_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_records_after_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_rows_after_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_candidate_rows_after_first_client_action\": {},\n",
                "  \"server_active_item_property_events_after_first_client_action\": {},\n",
                "  \"server_active_item_property_uses_events_after_first_client_action\": {},\n",
                "  \"server_active_item_property_full_events_after_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_events_after_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_uses_events_after_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_full_events_after_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_changed_use_count_rows_after_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_full_property_rows_after_first_client_action\": {},\n",
                "  \"area_events_after_first_client_action\": {},\n",
                "  \"inventory_events_after_first_client_action\": {},\n",
                "  \"client_gui_event_events_after_first_client_action\": {},\n",
                "  \"client_input_events_after_first_client_action\": {},\n",
                "  \"client_input_use_item_events_after_first_client_action\": {},\n",
                "  \"client_input_use_object_events_after_first_client_action\": {},\n",
                "  \"client_input_change_door_state_events_after_first_client_action\": {},\n",
                "  \"client_input_other_events_after_first_client_action\": {},\n",
                "  \"client_quickbar_events_after_first_client_action\": {},\n",
                "  \"client_quickbar_item_set_button_events_after_first_client_action\": {},\n",
                "  \"client_quickbar_other_set_button_events_after_first_client_action\": {},\n",
                "  \"chat_events_after_first_client_action\": {},\n",
                "  \"other_events_after_first_client_action\": {},\n",
                "  \"quickbar_events_before_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_events_before_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_records_before_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_rows_before_first_client_action\": {},\n",
                "  \"server_quickbar_item_use_count_candidate_rows_before_first_client_action\": {},\n",
                "  \"server_active_item_property_events_before_first_client_action\": {},\n",
                "  \"server_active_item_property_uses_events_before_first_client_action\": {},\n",
                "  \"server_active_item_property_full_events_before_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_events_before_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_uses_events_before_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_full_events_before_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_changed_use_count_rows_before_first_client_action\": {},\n",
                "  \"server_active_item_property_candidate_full_property_rows_before_first_client_action\": {},\n",
                "  \"direct_item_proof_objects\": {},\n",
                "  \"feature25_item_proof_objects\": {},\n",
                "  \"compact_item_emission_proof_objects\": {},\n",
                "  \"compact_item_emission_ready_objects\": {},\n",
                "  \"compact_item_emission_direct_only_proof_objects\": {},\n",
                "  \"compact_item_emission_feature25_only_proof_objects\": {},\n",
                "  \"compact_item_emission_shared_proof_objects\": {},\n",
                "  \"compact_item_emission_deferred_feature25_only_objects\": {},\n",
                "  \"inventory_feature25_reference_records\": {},\n",
                "  \"inventory_feature25_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_deferred_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_materialization_outcome\": \"{}\",\n",
                "  \"inventory_feature25_handoff_outcome\": \"{}\",\n",
                "  \"inventory_equipment_handoff_ready\": {},\n",
                "  \"inventory_equipment_handoff_outcome\": \"{}\",\n",
                "  \"inventory_equipment_handoff_events\": {},\n",
                "  \"inventory_equipment_handoff_ready_events\": {},\n",
                "  \"inventory_equipment_handoff_blocked_without_ready_state_events\": {},\n",
                "  \"inventory_equipment_handoff_ready_with_deferred_feature25_events\": {},\n",
                "  \"inventory_equipment_handoff_server_inventory_events\": {},\n",
                "  \"inventory_equipment_handoff_server_inventory_ready_events\": {},\n",
                "  \"inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events\": {},\n",
                "  \"inventory_equipment_handoff_client_gui_inventory_events\": {},\n",
                "  \"inventory_equipment_handoff_client_gui_inventory_ready_events\": {},\n",
                "  \"inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events\": {},\n",
                "  \"inventory_equipment_bridge_handoff_action\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_ready\": {},\n",
                "  \"inventory_equipment_bridge_handoff_consumer\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_event_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_outcome\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_ready_objects\": {},\n",
                "  \"inventory_equipment_bridge_handoff_deferred_feature25_only_objects\": {},\n",
                "  \"inventory_equipment_bridge_handoff_candidate_known\": {},\n",
                "  \"inventory_equipment_bridge_handoff_candidate_object_id\": {},\n",
                "  \"inventory_equipment_bridge_handoff_candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"inventory_equipment_bridge_handoff_candidate_proof\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_candidate_source\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_emissions\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_known\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_consumer\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_event_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_candidate_object_id\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_candidate_source\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_state_updates\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_known\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_emission_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_consumer\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_event_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_object_id\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_proof\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_source\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_ready_objects\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_deferred_feature25_only_objects\": {},\n",
                "  \"inventory_feature25_first_item_refs\": {},\n",
                "  \"inventory_feature25_first_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_first_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_first_deferred_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_second_item_refs\": {},\n",
                "  \"inventory_feature25_second_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_second_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_second_deferred_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_legacy_tail_item_refs\": {},\n",
                "  \"inventory_feature25_legacy_tail_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_legacy_tail_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_legacy_tail_deferred_item_ref_mentions\": {},\n",
                "  \"cleared_inventory_item_object_ids\": {},\n",
                "  \"live_object_events_since_pending_refresh\": {},\n",
                "  \"quickbar_events_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_events_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_records_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_rows_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_candidate_rows_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_uses_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_full_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_uses_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_full_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_changed_use_count_rows_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_full_property_rows_since_pending_refresh\": {},\n",
                "  \"area_events_since_pending_refresh\": {},\n",
                "  \"inventory_events_since_pending_refresh\": {},\n",
                "  \"client_gui_event_events_since_pending_refresh\": {},\n",
                "  \"client_input_events_since_pending_refresh\": {},\n",
                "  \"client_input_use_item_events_since_pending_refresh\": {},\n",
                "  \"client_input_use_object_events_since_pending_refresh\": {},\n",
                "  \"client_input_change_door_state_events_since_pending_refresh\": {},\n",
                "  \"client_input_other_events_since_pending_refresh\": {},\n",
                "  \"client_quickbar_events_since_pending_refresh\": {},\n",
                "  \"client_quickbar_item_set_button_events_since_pending_refresh\": {},\n",
                "  \"client_quickbar_other_set_button_events_since_pending_refresh\": {},\n",
                "  \"chat_events_since_pending_refresh\": {},\n",
                "  \"other_events_since_pending_refresh\": {}\n",
                "}}\n"
            ),
            self.candidate.object_id,
            self.candidate.object_id,
            self.candidate.proof.as_str(),
            self.candidate.source.as_str(),
            first_active_item_known,
            first_active_item_matches_candidate,
            first_active_item_slot_known,
            first_active_item_slot,
            first_active_item_first_page_slot,
            first_active_item_slot_matches_recommended_set_button_slot,
            first_active_item_object_id,
            first_active_item_object_id,
            first_active_item_base_item,
            first_active_item_base_item,
            first_active_item_appearance_type,
            first_active_item_property_count,
            first_active_item_first_property_known,
            first_active_item_first_property_id,
            first_active_item_first_property_subtype,
            first_active_item_first_property_cost_table_value,
            first_active_item_first_property_param,
            first_active_item_has_armor_word,
            first_active_item_name_is_locstring,
            first_active_item_state_mask,
            first_active_item_state_mask,
            first_active_item_value_mask,
            first_active_item_value_mask,
            stream_probe.item_buttons_rejected_missing_state_proof,
            stream_probe.item_buttons_rejected_missing_state_unknown,
            stream_probe.item_buttons_rejected_missing_state_cleared_delete,
            stream_probe.item_buttons_rejected_missing_state_cleared_area_reset,
            stream_probe.item_objects_rejected_missing_state_proven,
            stream_probe.item_objects_rejected_missing_state_active,
            stream_probe.item_objects_rejected_missing_state_feature25_first,
            stream_probe.item_objects_rejected_missing_state_feature25_second,
            stream_probe.item_objects_rejected_missing_state_feature25_legacy_tail,
            stream_probe.item_objects_rejected_missing_state_unknown,
            stream_probe.item_objects_rejected_missing_state_cleared_delete,
            stream_probe.item_objects_rejected_missing_state_cleared_area_reset,
            stream_probe.item_objects_preserved_by_explicit_self_materialization,
            stream_probe.item_objects_preserved_by_active_state,
            preserved_active_item_signature_count,
            preserved_active_item_slots_json,
            use_count_coverage.matching_use_count_slot_count(),
            preserved_active_item_matching_use_count_slots_json,
            use_count_coverage.missing_use_count_slot_count(),
            preserved_active_item_missing_use_count_slots_json,
            preserved_active_item_actionable_missing_use_count_slot_count,
            preserved_active_item_actionable_missing_use_count_slots_json,
            observed_preserved_active_item_actionable_missing_use_count_slot_count,
            observed_preserved_active_item_actionable_missing_use_count_slots_json,
            stream_probe.item_objects_preserved_by_feature25_first,
            stream_probe.item_objects_preserved_by_feature25_second,
            stream_probe.item_objects_preserved_by_feature25_legacy_tail,
            recommended_client_action_should_dispatch,
            recommended_client_action_suppressed_reason,
            recommended_use_item_payload_available,
            recommended_use_item_payload_hex,
            self.candidate.object_id,
            self.candidate.object_id,
            crate::translate::client_input::EE_SELF_OBJECT_ID,
            crate::translate::client_input::EE_SELF_OBJECT_ID,
            crate::translate::client_input::INVALID_OBJECT_ID,
            crate::translate::client_input::INVALID_OBJECT_ID,
            recommended_use_item_first_property_subtype_low_payload_available,
            recommended_use_item_first_property_subtype_low_payload_hex,
            recommended_use_item_first_property_subtype_low_byte_known,
            recommended_use_item_first_property_subtype_low_byte,
            recommended_use_item_first_property_subtype_low_source,
            recommended_use_item_first_property_subtype_low_matches_default,
            crate::translate::client_input::EE_SELF_OBJECT_ID,
            crate::translate::client_input::EE_SELF_OBJECT_ID,
            crate::translate::client_input::INVALID_OBJECT_ID,
            crate::translate::client_input::INVALID_OBJECT_ID,
            recommended_set_button_payload_available,
            recommended_set_button_payload_hex,
            self.recommended_set_button_slot,
            self.recommended_set_button_slot_source,
            client_quickbar::ITEM_SET_BUTTON_TYPE,
            self.candidate.object_id,
            self.candidate.object_id,
            client_quickbar::ITEM_SET_BUTTON_DEFAULT_INT_PARAM,
            recommended_gui_event_notify_payload_available,
            recommended_gui_event_notify_payload_hex,
            client_gui_event::RADIAL_NOTIFY_PROBE_EVENT_A,
            client_gui_event::RADIAL_NOTIFY_PROBE_EVENT_B,
            self.candidate.object_id,
            self.candidate.object_id,
            recommended_use_object_payload_available,
            recommended_use_object_payload_hex,
            self.candidate.object_id,
            self.candidate.object_id,
            self.updates_since_committed_quickbar,
            self.events_since_pending_refresh,
            self.event_breakdown.server_to_client_events,
            self.event_breakdown.client_to_server_events,
            self.proof_class
                .map(QuickbarItemRefreshProofClass::as_str)
                .unwrap_or("none"),
            action_outcome,
            recommended_action_outcome,
            active_property_outcome,
            server_quickbar_response_timing,
            self.quickbar_item_use_count_state_rows,
            self.quickbar_item_use_count_updates_observed,
            self.candidate_use_count_state.is_some(),
            candidate_use_count_state_slot_relation.as_str(),
            candidate_use_count_state_slot_relation.matches_preserved_active_item_slot(),
            candidate_use_count_state.slot,
            candidate_use_count_state.button_type,
            candidate_use_count_state.object_id,
            candidate_use_count_state.object_id,
            candidate_use_count_state.active_property_index,
            candidate_use_count_state.use_count,
            self.first_preserved_active_item_use_count_state.is_some(),
            first_active_item_use_count_state_slot_relation.as_str(),
            first_active_item_use_count_state_slot_relation.matches_preserved_active_item_slot(),
            first_active_item_use_count_state.slot,
            first_active_item_use_count_state.button_type,
            first_active_item_use_count_state.object_id,
            first_active_item_use_count_state.object_id,
            first_active_item_use_count_state.active_property_index,
            first_active_item_use_count_state.use_count,
            self.first_candidate_use_count_row.is_some(),
            first_candidate_use_count_row_timing,
            first_candidate_use_count_row_slot_relation.as_str(),
            first_candidate_use_count_row_slot_relation.matches_preserved_active_item_slot(),
            first_candidate_use_count_row.slot,
            first_candidate_use_count_row.button_type,
            first_candidate_use_count_row.object_id,
            first_candidate_use_count_row.object_id,
            first_candidate_use_count_row.active_property_index,
            first_candidate_use_count_row.use_count,
            self.first_candidate_use_count_row_before_first_client_action
                .is_some(),
            first_candidate_use_count_row_before_first_client_action.slot,
            first_candidate_use_count_row_before_first_client_action.button_type,
            first_candidate_use_count_row_before_first_client_action.active_property_index,
            first_candidate_use_count_row_before_first_client_action.use_count,
            self.first_candidate_use_count_row_after_first_client_action
                .is_some(),
            first_candidate_use_count_row_after_first_client_action.slot,
            first_candidate_use_count_row_after_first_client_action.button_type,
            first_candidate_use_count_row_after_first_client_action.active_property_index,
            first_candidate_use_count_row_after_first_client_action.use_count,
            first_client_action_timing,
            self.followup_events_before_first_client_action,
            self.first_followup_event
                .map(QuickbarItemRefreshEventKind::as_str)
                .unwrap_or("none"),
            self.first_client_action
                .map(QuickbarItemRefreshEventKind::as_str)
                .unwrap_or("none"),
            first_client_action_has_object_id,
            first_client_action_object_id,
            first_client_action_slot,
            first_client_action_button_type,
            first_client_action_body_kind,
            first_client_action_gui_event_known,
            first_client_action_gui_event_a,
            first_client_action_gui_event_b,
            first_client_action_gui_event_declared_bytes,
            first_client_action_gui_event_trailing_fragment_bytes,
            first_client_action_gui_event_has_vector,
            first_client_action_gui_event_vector_zero,
            first_client_action_gui_event_vector_bits[0],
            first_client_action_gui_event_vector_bits[1],
            first_client_action_gui_event_vector_bits[2],
            first_client_action_use_item_known,
            first_client_action_use_item_active_property_subtype,
            first_client_action_use_item_has_optional_byte,
            first_client_action_use_item_has_target_object,
            first_client_action_use_item_target_object_id,
            first_client_action_use_item_target_object_id,
            first_client_action_use_item_target_is_self_or_legacy_self,
            first_client_action_use_item_has_position,
            first_client_action_candidate_known,
            first_client_action_candidate_object_id,
            first_client_action_matches_candidate,
            first_client_action_matches_preserved_active_item,
            first_client_action_match_class,
            first_client_action_matches_recommended_client_use_item,
            first_client_action_matches_recommended_client_use_item_first_property_subtype_low,
            first_client_action_matches_recommended_client_quickbar_set_button,
            first_client_action_matches_recommended_client_gui_event_notify,
            first_client_action_matches_recommended_client_use_object,
            first_event_after_client_action,
            self.events_after_first_client_action,
            self.event_breakdown_after_first_client_action
                .server_to_client_events,
            self.event_breakdown_after_first_client_action
                .client_to_server_events,
            self.event_breakdown_after_first_client_action
                .live_object_events,
            self.event_breakdown_after_first_client_action
                .quickbar_events,
            self.event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_events,
            self.event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_records,
            self.event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_rows,
            self.event_breakdown_after_first_client_action
                .server_quickbar_item_use_count_candidate_rows,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_events,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_uses_events,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_full_events,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_candidate_events,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_candidate_uses_events,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_candidate_full_events,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_candidate_changed_use_count_rows,
            self.event_breakdown_after_first_client_action
                .server_active_item_property_candidate_full_property_rows,
            self.event_breakdown_after_first_client_action.area_events,
            self.event_breakdown_after_first_client_action
                .inventory_events,
            self.event_breakdown_after_first_client_action
                .client_gui_event_events,
            self.event_breakdown_after_first_client_action
                .client_input_events,
            self.event_breakdown_after_first_client_action
                .client_input_use_item_events,
            self.event_breakdown_after_first_client_action
                .client_input_use_object_events,
            self.event_breakdown_after_first_client_action
                .client_input_change_door_state_events,
            self.event_breakdown_after_first_client_action
                .client_input_other_events,
            self.event_breakdown_after_first_client_action
                .client_quickbar_events,
            self.event_breakdown_after_first_client_action
                .client_quickbar_item_set_button_events,
            self.event_breakdown_after_first_client_action
                .client_quickbar_other_set_button_events,
            self.event_breakdown_after_first_client_action.chat_events,
            self.event_breakdown_after_first_client_action.other_events,
            event_breakdown_before_first_client_action.quickbar_events,
            event_breakdown_before_first_client_action.server_quickbar_item_use_count_events,
            event_breakdown_before_first_client_action.server_quickbar_item_use_count_records,
            event_breakdown_before_first_client_action.server_quickbar_item_use_count_rows,
            event_breakdown_before_first_client_action
                .server_quickbar_item_use_count_candidate_rows,
            event_breakdown_before_first_client_action.server_active_item_property_events,
            event_breakdown_before_first_client_action.server_active_item_property_uses_events,
            event_breakdown_before_first_client_action.server_active_item_property_full_events,
            event_breakdown_before_first_client_action.server_active_item_property_candidate_events,
            event_breakdown_before_first_client_action
                .server_active_item_property_candidate_uses_events,
            event_breakdown_before_first_client_action
                .server_active_item_property_candidate_full_events,
            event_breakdown_before_first_client_action
                .server_active_item_property_candidate_changed_use_count_rows,
            event_breakdown_before_first_client_action
                .server_active_item_property_candidate_full_property_rows,
            self.direct_item_proof_objects,
            self.feature25_item_proof_objects,
            self.compact_item_emission_proof_objects,
            self.item_context.compact_item_emission_ready_objects,
            self.compact_item_emission_direct_only_proof_objects,
            self.compact_item_emission_feature25_only_proof_objects,
            self.compact_item_emission_shared_proof_objects,
            self.item_context
                .compact_item_emission_deferred_feature25_only_objects,
            self.item_context.inventory_feature25_reference_records,
            self.item_context.inventory_feature25_item_ref_mentions(),
            self.item_context
                .inventory_feature25_materialized_item_ref_mentions(),
            self.item_context
                .inventory_feature25_deferred_item_ref_mentions(),
            self.item_context
                .inventory_feature25_materialization_outcome()
                .as_str(),
            self.item_context
                .inventory_feature25_handoff_outcome()
                .as_str(),
            self.item_context.inventory_equipment_handoff_ready(),
            self.item_context
                .inventory_equipment_handoff_outcome()
                .as_str(),
            self.inventory_equipment_handoff_events,
            self.inventory_equipment_handoff_ready_events,
            self.inventory_equipment_handoff_blocked_without_ready_state_events,
            self.inventory_equipment_handoff_ready_with_deferred_feature25_events,
            self.inventory_equipment_handoff_server_inventory_events,
            self.inventory_equipment_handoff_server_inventory_ready_events,
            self.inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events,
            self.inventory_equipment_handoff_client_gui_inventory_events,
            self.inventory_equipment_handoff_client_gui_inventory_ready_events,
            self.inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events,
            bridge_plan.action.as_str(),
            bridge_plan.ready_to_emit(),
            bridge_plan.consumer.as_str(),
            bridge_plan.event_index,
            bridge_plan.outcome.as_str(),
            bridge_plan.ready_objects,
            bridge_plan.deferred_feature25_only_objects,
            bridge_candidate_known,
            bridge_candidate_object_id,
            bridge_candidate_object_id,
            bridge_candidate_proof,
            bridge_candidate_source,
            self.inventory_equipment_bridge_handoff_emissions,
            last_bridge_emission_known,
            last_bridge_emission_index,
            last_bridge_emission_plan.consumer.as_str(),
            last_bridge_emission_plan.event_index,
            last_bridge_emission_candidate_object_id,
            last_bridge_emission_candidate_object_id,
            last_bridge_emission_candidate_source,
            self.inventory_equipment_bridge_handoff_state_updates,
            last_bridge_state_update_known,
            last_bridge_state_update_index,
            last_bridge_state_update_emission_index,
            last_bridge_state_update_consumer,
            last_bridge_state_update_event_index,
            last_bridge_state_update_candidate_object_id,
            last_bridge_state_update_candidate_object_id,
            last_bridge_state_update_candidate_proof,
            last_bridge_state_update_candidate_source,
            last_bridge_state_update_ready_objects,
            last_bridge_state_update_deferred_feature25_only_objects,
            self.item_context.inventory_feature25_first_item_refs,
            self.item_context
                .inventory_feature25_first_item_ref_mentions,
            self.item_context
                .inventory_feature25_first_materialized_item_ref_mentions,
            self.item_context
                .inventory_feature25_first_deferred_item_ref_mentions,
            self.item_context.inventory_feature25_second_item_refs,
            self.item_context
                .inventory_feature25_second_item_ref_mentions,
            self.item_context
                .inventory_feature25_second_materialized_item_ref_mentions,
            self.item_context
                .inventory_feature25_second_deferred_item_ref_mentions,
            self.item_context.inventory_feature25_legacy_tail_item_refs,
            self.item_context
                .inventory_feature25_legacy_tail_item_ref_mentions,
            self.item_context
                .inventory_feature25_legacy_tail_materialized_item_ref_mentions,
            self.item_context
                .inventory_feature25_legacy_tail_deferred_item_ref_mentions,
            self.item_context.cleared_inventory_item_object_ids,
            self.event_breakdown.live_object_events,
            self.event_breakdown.quickbar_events,
            self.event_breakdown.server_quickbar_item_use_count_events,
            self.event_breakdown.server_quickbar_item_use_count_records,
            self.event_breakdown.server_quickbar_item_use_count_rows,
            self.event_breakdown
                .server_quickbar_item_use_count_candidate_rows,
            self.event_breakdown.server_active_item_property_events,
            self.event_breakdown.server_active_item_property_uses_events,
            self.event_breakdown.server_active_item_property_full_events,
            self.event_breakdown
                .server_active_item_property_candidate_events,
            self.event_breakdown
                .server_active_item_property_candidate_uses_events,
            self.event_breakdown
                .server_active_item_property_candidate_full_events,
            self.event_breakdown
                .server_active_item_property_candidate_changed_use_count_rows,
            self.event_breakdown
                .server_active_item_property_candidate_full_property_rows,
            self.event_breakdown.area_events,
            self.event_breakdown.inventory_events,
            self.event_breakdown.client_gui_event_events,
            self.event_breakdown.client_input_events,
            self.event_breakdown.client_input_use_item_events,
            self.event_breakdown.client_input_use_object_events,
            self.event_breakdown.client_input_change_door_state_events,
            self.event_breakdown.client_input_other_events,
            self.event_breakdown.client_quickbar_events,
            self.event_breakdown.client_quickbar_item_set_button_events,
            self.event_breakdown.client_quickbar_other_set_button_events,
            self.event_breakdown.chat_events,
            self.event_breakdown.other_events,
        )
    }
}

fn hex_encode_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0F) as usize] as char);
    }
    encoded
}

fn first_property_subtype_low_byte_for_candidate(
    signature: Option<QuickbarActiveItemSignature>,
    candidate_object_id: u32,
) -> Option<u8> {
    let signature = signature?;
    if signature.object_id != candidate_object_id {
        return None;
    }
    let property = signature.first_property?;
    Some((property.subtype & 0x00FF) as u8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuickbarItemRefreshClientActionDetail {
    pub(crate) kind: QuickbarItemRefreshEventKind,
    pub(crate) object_id: Option<u32>,
    pub(crate) slot: Option<u8>,
    pub(crate) button_type: Option<u8>,
    pub(crate) body_kind: Option<ClientQuickbarSetButtonKind>,
    pub(crate) gui_event_a: Option<u16>,
    pub(crate) gui_event_b: Option<u16>,
    pub(crate) gui_event_declared_bytes: Option<usize>,
    pub(crate) gui_event_trailing_fragment_bytes: Option<usize>,
    pub(crate) gui_event_has_vector: Option<bool>,
    pub(crate) gui_event_vector_bits: Option<[u32; 3]>,
    pub(crate) use_item_active_property_subtype: Option<u8>,
    pub(crate) use_item_has_optional_byte: Option<bool>,
    pub(crate) use_item_has_target_object: Option<bool>,
    pub(crate) use_item_target_object_id: Option<u32>,
    pub(crate) use_item_has_position: Option<bool>,
    pub(crate) use_object_mark_inventory_gui_state: Option<bool>,
    pub(crate) use_object_schedule_script_event: Option<bool>,
    pub(crate) candidate_object_id: Option<u32>,
    pub(crate) matches_candidate_object: Option<bool>,
}

impl QuickbarItemRefreshClientActionDetail {
    pub(crate) fn matches_preserved_active_item(
        self,
        first_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
    ) -> bool {
        match (self.object_id, first_preserved_active_item_signature) {
            (Some(object_id), Some(signature)) => object_id == signature.object_id,
            _ => false,
        }
    }

    pub(crate) fn matches_recommended_client_quickbar_set_button(
        self,
        candidate_object_id: u32,
        recommended_slot: u8,
    ) -> bool {
        self.kind == QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton
            && self.object_id == Some(candidate_object_id)
            && self.slot == Some(recommended_slot)
            && self.button_type == Some(client_quickbar::ITEM_SET_BUTTON_TYPE)
            && self.body_kind == Some(ClientQuickbarSetButtonKind::Item)
            && self.candidate_object_id == Some(candidate_object_id)
            && self.matches_candidate_object == Some(true)
    }

    pub(crate) fn matches_recommended_client_gui_event_notify(
        self,
        candidate_object_id: u32,
    ) -> bool {
        self.kind == QuickbarItemRefreshEventKind::ClientGuiEventNotify
            && self.object_id == Some(candidate_object_id)
            && self.gui_event_a == Some(client_gui_event::RADIAL_NOTIFY_PROBE_EVENT_A)
            && self.gui_event_b == Some(client_gui_event::RADIAL_NOTIFY_PROBE_EVENT_B)
            && self.gui_event_declared_bytes
                == Some(client_gui_event::EE_8193_35_NOTIFY_DECLARED_BYTES)
            && self.gui_event_trailing_fragment_bytes
                == Some(client_gui_event::RADIAL_NOTIFY_PROBE_TRAILING_FRAGMENT_BYTES)
            && self.gui_event_has_vector == Some(true)
            && self.gui_event_vector_bits == Some([0, 0, 0])
            && self.candidate_object_id == Some(candidate_object_id)
            && self.matches_candidate_object == Some(true)
    }

    pub(crate) fn matches_recommended_client_use_item(self, candidate_object_id: u32) -> bool {
        self.matches_recommended_client_use_item_with_active_property_byte(candidate_object_id, 0)
    }

    pub(crate) fn matches_recommended_client_use_item_first_property_subtype_low(
        self,
        candidate_object_id: u32,
        first_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
    ) -> bool {
        first_property_subtype_low_byte_for_candidate(
            first_preserved_active_item_signature,
            candidate_object_id,
        )
        .map(|active_property_subtype| {
            self.matches_recommended_client_use_item_with_active_property_byte(
                candidate_object_id,
                active_property_subtype,
            )
        })
        .unwrap_or(false)
    }

    fn matches_recommended_client_use_item_with_active_property_byte(
        self,
        candidate_object_id: u32,
        active_property_subtype: u8,
    ) -> bool {
        self.kind == QuickbarItemRefreshEventKind::ClientInputUseItem
            && self.object_id == Some(candidate_object_id)
            && self.use_item_active_property_subtype == Some(active_property_subtype)
            && self.use_item_has_optional_byte == Some(false)
            && self.use_item_has_target_object == Some(true)
            && matches!(
                self.use_item_target_object_id,
                Some(client_input::EE_SELF_OBJECT_ID) | Some(client_input::INVALID_OBJECT_ID)
            )
            && self.use_item_has_position == Some(false)
            && self.candidate_object_id == Some(candidate_object_id)
            && self.matches_candidate_object == Some(true)
    }

    pub(crate) fn matches_recommended_client_use_object(self, candidate_object_id: u32) -> bool {
        self.kind == QuickbarItemRefreshEventKind::ClientInputUseObject
            && self.object_id == Some(candidate_object_id)
            && self.use_object_mark_inventory_gui_state == Some(false)
            && self.use_object_schedule_script_event == Some(false)
            && self.candidate_object_id == Some(candidate_object_id)
            && self.matches_candidate_object == Some(true)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QuickbarItemRefreshEventBreakdown {
    pub(crate) server_to_client_events: u64,
    pub(crate) client_to_server_events: u64,
    pub(crate) live_object_events: u64,
    pub(crate) quickbar_events: u64,
    pub(crate) server_quickbar_item_use_count_events: u64,
    pub(crate) server_quickbar_item_use_count_records: u64,
    pub(crate) server_quickbar_item_use_count_rows: u64,
    pub(crate) server_quickbar_item_use_count_candidate_rows: u64,
    pub(crate) server_active_item_property_events: u64,
    pub(crate) server_active_item_property_uses_events: u64,
    pub(crate) server_active_item_property_full_events: u64,
    pub(crate) server_active_item_property_candidate_events: u64,
    pub(crate) server_active_item_property_candidate_uses_events: u64,
    pub(crate) server_active_item_property_candidate_full_events: u64,
    pub(crate) server_active_item_property_candidate_changed_use_count_rows: u64,
    pub(crate) server_active_item_property_candidate_full_property_rows: u64,
    pub(crate) area_events: u64,
    pub(crate) inventory_events: u64,
    pub(crate) client_gui_event_events: u64,
    pub(crate) client_input_events: u64,
    pub(crate) client_input_use_item_events: u64,
    pub(crate) client_input_use_object_events: u64,
    pub(crate) client_input_change_door_state_events: u64,
    pub(crate) client_input_other_events: u64,
    pub(crate) client_quickbar_events: u64,
    pub(crate) client_quickbar_item_set_button_events: u64,
    pub(crate) client_quickbar_other_set_button_events: u64,
    pub(crate) chat_events: u64,
    pub(crate) other_events: u64,
}

impl QuickbarItemRefreshEventBreakdown {
    pub(crate) fn has_server_quickbar_response(self) -> bool {
        self.quickbar_events != 0 || self.server_quickbar_item_use_count_events != 0
    }

    pub(crate) fn saturating_sub(self, rhs: Self) -> Self {
        Self {
            server_to_client_events: self
                .server_to_client_events
                .saturating_sub(rhs.server_to_client_events),
            client_to_server_events: self
                .client_to_server_events
                .saturating_sub(rhs.client_to_server_events),
            live_object_events: self
                .live_object_events
                .saturating_sub(rhs.live_object_events),
            quickbar_events: self.quickbar_events.saturating_sub(rhs.quickbar_events),
            server_quickbar_item_use_count_events: self
                .server_quickbar_item_use_count_events
                .saturating_sub(rhs.server_quickbar_item_use_count_events),
            server_quickbar_item_use_count_records: self
                .server_quickbar_item_use_count_records
                .saturating_sub(rhs.server_quickbar_item_use_count_records),
            server_quickbar_item_use_count_rows: self
                .server_quickbar_item_use_count_rows
                .saturating_sub(rhs.server_quickbar_item_use_count_rows),
            server_quickbar_item_use_count_candidate_rows: self
                .server_quickbar_item_use_count_candidate_rows
                .saturating_sub(rhs.server_quickbar_item_use_count_candidate_rows),
            server_active_item_property_events: self
                .server_active_item_property_events
                .saturating_sub(rhs.server_active_item_property_events),
            server_active_item_property_uses_events: self
                .server_active_item_property_uses_events
                .saturating_sub(rhs.server_active_item_property_uses_events),
            server_active_item_property_full_events: self
                .server_active_item_property_full_events
                .saturating_sub(rhs.server_active_item_property_full_events),
            server_active_item_property_candidate_events: self
                .server_active_item_property_candidate_events
                .saturating_sub(rhs.server_active_item_property_candidate_events),
            server_active_item_property_candidate_uses_events: self
                .server_active_item_property_candidate_uses_events
                .saturating_sub(rhs.server_active_item_property_candidate_uses_events),
            server_active_item_property_candidate_full_events: self
                .server_active_item_property_candidate_full_events
                .saturating_sub(rhs.server_active_item_property_candidate_full_events),
            server_active_item_property_candidate_changed_use_count_rows: self
                .server_active_item_property_candidate_changed_use_count_rows
                .saturating_sub(rhs.server_active_item_property_candidate_changed_use_count_rows),
            server_active_item_property_candidate_full_property_rows: self
                .server_active_item_property_candidate_full_property_rows
                .saturating_sub(rhs.server_active_item_property_candidate_full_property_rows),
            area_events: self.area_events.saturating_sub(rhs.area_events),
            inventory_events: self.inventory_events.saturating_sub(rhs.inventory_events),
            client_gui_event_events: self
                .client_gui_event_events
                .saturating_sub(rhs.client_gui_event_events),
            client_input_events: self
                .client_input_events
                .saturating_sub(rhs.client_input_events),
            client_input_use_item_events: self
                .client_input_use_item_events
                .saturating_sub(rhs.client_input_use_item_events),
            client_input_use_object_events: self
                .client_input_use_object_events
                .saturating_sub(rhs.client_input_use_object_events),
            client_input_change_door_state_events: self
                .client_input_change_door_state_events
                .saturating_sub(rhs.client_input_change_door_state_events),
            client_input_other_events: self
                .client_input_other_events
                .saturating_sub(rhs.client_input_other_events),
            client_quickbar_events: self
                .client_quickbar_events
                .saturating_sub(rhs.client_quickbar_events),
            client_quickbar_item_set_button_events: self
                .client_quickbar_item_set_button_events
                .saturating_sub(rhs.client_quickbar_item_set_button_events),
            client_quickbar_other_set_button_events: self
                .client_quickbar_other_set_button_events
                .saturating_sub(rhs.client_quickbar_other_set_button_events),
            chat_events: self.chat_events.saturating_sub(rhs.chat_events),
            other_events: self.other_events.saturating_sub(rhs.other_events),
        }
    }
}

impl ObjectRegistry {
    pub(crate) fn reset_for_area(&mut self) {
        if !self.known.is_empty()
            || !self.materialized_item_object_ids.is_empty()
            || !self.inventory_feature25_first_item_refs.is_empty()
            || !self.inventory_feature25_second_item_refs.is_empty()
            || !self.inventory_feature25_legacy_tail_item_refs.is_empty()
        {
            tracing::debug!(
                known_objects = self.known.len(),
                materialized_item_objects = self.materialized_item_object_ids.len(),
                inventory_feature25_first_item_refs =
                    self.inventory_feature25_first_item_refs.len(),
                inventory_feature25_second_item_refs =
                    self.inventory_feature25_second_item_refs.len(),
                inventory_feature25_legacy_tail_item_refs =
                    self.inventory_feature25_legacy_tail_item_refs.len(),
                cleared_inventory_item_object_ids = self.cleared_inventory_item_object_ids.len(),
                session_creature_aliases = self.session_creature_ids_by_compact.len(),
                "semantic object registry reset for new Area_ClientArea"
            );
        }
        self.remember_inventory_item_proofs_cleared_by_area_reset();
        self.known.clear();
        self.materialized_item_object_ids.clear();
        self.inventory_feature25_first_item_refs.clear();
        self.inventory_feature25_second_item_refs.clear();
        self.inventory_feature25_legacy_tail_item_refs.clear();
    }

    pub(crate) fn observe_player_list_object_ids(&mut self, object_ids: &[PlayerListObjectIds]) {
        for entry in object_ids {
            let Some(creature_object_id) = entry.creature_object_id else {
                continue;
            };
            let Some(compact_id) = compact_session_alias_from_player_list(creature_object_id)
            else {
                continue;
            };
            if let Some(previous) = self
                .session_creature_ids_by_compact
                .insert(compact_id, creature_object_id)
                .filter(|previous| *previous != creature_object_id)
            {
                tracing::warn!(
                    compact_id,
                    previous_session_id = previous,
                    new_session_id = creature_object_id,
                    player_object_id = entry.player_object_id,
                    "verified PlayerList remapped a compact creature session alias"
                );
            } else {
                tracing::debug!(
                    compact_id,
                    session_creature_id = creature_object_id,
                    player_object_id = entry.player_object_id,
                    "verified PlayerList established compact creature session alias"
                );
            }
        }
    }

    pub(crate) fn observe_mentions(&mut self, mentions: &[LiveObjectMention]) {
        self.live_object_packets = self.live_object_packets.saturating_add(1);
        for mention in mentions {
            let inventory_owner_without_type = mention.opcode == b'I' && mention.object_type == 0;
            let registry_object_id =
                self.registry_object_id_for_live_object(mention.object_type, mention.object_id);
            if (mention.object_id & 0xFFFF_FF00) == 0xFFFF_FF00 {
                tracing::debug!(
                    opcode = %char::from(mention.opcode),
                    object_type = mention.object_type,
                    object_id = mention.object_id,
                    "semantic object registry observing session-local live-object mention"
                );
            }
            let entry = self
                .known
                .entry(registry_object_id)
                .or_insert_with(|| KnownObjectState {
                    object_id: registry_object_id,
                    object_type: mention.object_type,
                    ..KnownObjectState::default()
                });
            if registry_object_id != mention.object_id {
                tracing::debug!(
                    opcode = %char::from(mention.opcode),
                    object_type = mention.object_type,
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    registry_object_id = format_args!("0x{registry_object_id:08X}"),
                    "live-object registry merged compact/external placeable alias"
                );
            }
            if entry.mentions != 0 && entry.object_type != mention.object_type {
                if inventory_owner_without_type {
                    // Live-object inventory `I` records carry an owner
                    // OBJECTID plus an inventory mask; the exact inventory
                    // parser reports object_type 0 because the packet does
                    // not carry an independent creature/placeable/etc. type
                    // field there.  Treat that as a typed owner reference,
                    // not as proof that an existing creature became object
                    // type zero.
                    tracing::debug!(
                        object_id = mention.object_id,
                        known_object_type = entry.object_type,
                        opcode = %char::from(mention.opcode),
                        "live-object registry kept known owner type for inventory record"
                    );
                } else if entry.object_type == 0 {
                    // A prior inventory-only owner mention created an
                    // unknown-type placeholder. The first typed add/update is
                    // the stronger wire-derived fact, so promote without an
                    // object-type-change warning.
                    tracing::debug!(
                        object_id = mention.object_id,
                        new_object_type = mention.object_type,
                        opcode = %char::from(mention.opcode),
                        "live-object registry promoted inventory-only owner to typed object"
                    );
                } else {
                    tracing::warn!(
                        object_id = mention.object_id,
                        old_object_type = entry.object_type,
                        new_object_type = mention.object_type,
                        opcode = %char::from(mention.opcode),
                        "live-object registry observed object type change"
                    );
                }
            }
            if !inventory_owner_without_type || entry.object_type == 0 {
                entry.object_type = mention.object_type;
            }
            entry.last_opcode = mention.opcode;
            if let Some(name) = mention.name.as_ref().filter(|name| !name.is_empty()) {
                entry.latest_name = Some(name.clone());
            }
            if let Some(position) = mention.position {
                entry.position = Some(position);
            }
            if let Some(orientation) = mention.orientation {
                entry.orientation = Some(orientation);
            }
            if let Some(bounds) = mention.bounds {
                entry.bounds = Some(bounds);
            }
            if let Some(placeable_appearance) = mention.placeable_appearance {
                entry.placeable_appearance = Some(placeable_appearance);
            }
            if let Some(placeable_state) = mention.placeable_state {
                entry.merge_placeable_state(placeable_state);
            }
            entry.mentions = entry.mentions.saturating_add(1);
            match mention.opcode {
                b'A' => {
                    if entry.active {
                        entry.duplicate_add_mentions =
                            entry.duplicate_add_mentions.saturating_add(1);
                        // `A` is an observed live-object add/update record, not
                        // a proxy-owned game-state transition. The EE server
                        // decompile for `CNWSMessage::SendServerToPlayerGameObjUpdate`
                        // shows the server recomputing object update messages
                        // from current visibility/categories, and reliable M
                        // traffic can replay the same verified payload. Treat a
                        // same-id/same-type add as an idempotent assertion that
                        // the object is present; the earlier object-type-change
                        // check remains the real warning boundary.
                        tracing::debug!(
                            object_id = mention.object_id,
                            object_type = mention.object_type,
                            duplicate_add_mentions = entry.duplicate_add_mentions,
                            "live-object registry observed idempotent duplicate add"
                        );
                    }
                    entry.active = true;
                    entry.add_mentions = entry.add_mentions.saturating_add(1);
                }
                b'D' => {
                    if !entry.active {
                        entry.delete_before_add_mentions =
                            entry.delete_before_add_mentions.saturating_add(1);
                        // The registry is wire-derived protocol context, not a
                        // game-state oracle. After area changes or late proxy
                        // startup, the server can legally delete objects that
                        // were active before this cache observed their add. Keep
                        // the fact for diagnostics, but do not surface it as a
                        // packet warning unless a future invariant proves it is
                        // harmful.
                        tracing::debug!(
                            object_id = mention.object_id,
                            object_type = mention.object_type,
                            "live-object registry observed delete before active add"
                        );
                    }
                    entry.active = false;
                    entry.clear_lifecycle_facts();
                    entry.delete_mentions = entry.delete_mentions.saturating_add(1);
                }
                b'U' | b'P' | b'I' | b'G' | b'W' => {
                    if !entry.active {
                        entry.update_before_add_mentions =
                            entry.update_before_add_mentions.saturating_add(1);
                        // Same discipline as deletes above: this is useful
                        // state for future translation decisions, but the
                        // legacy server remains authoritative and can mention
                        // objects before this proxy cache saw their add.
                        tracing::debug!(
                            object_id = mention.object_id,
                            object_type = mention.object_type,
                            opcode = %char::from(mention.opcode),
                            "live-object registry observed update before active add"
                        );
                    }
                    entry.update_mentions = entry.update_mentions.saturating_add(1);
                }
                _ => {}
            }
            if mention.opcode == b'D' && mention.object_type == ITEM_OBJECT_TYPE {
                self.forget_inventory_item_object_id(registry_object_id);
            }
        }
    }

    pub(crate) fn observe_placeable_area_context(
        &mut self,
        area_context: &AreaPlaceableContext,
        mentions: &[LiveObjectMention],
    ) {
        const PLACEABLE_STATE_OBSERVATION: u8 = 0x01;
        const PLACEABLE_ORIENTATION_OBSERVATION: u8 = 0x02;
        const PLACEABLE_APPEARANCE_OBSERVATION: u8 = 0x04;
        const PLACEABLE_POSITION_OBSERVATION: u8 = 0x08;

        let mut seen_observation_masks = BTreeMap::new();
        for mention in mentions {
            let registry_object_id =
                self.registry_object_id_for_live_object(mention.object_type, mention.object_id);
            let observation_mask = (if mention.placeable_state.is_some() {
                PLACEABLE_STATE_OBSERVATION
            } else {
                0
            }) | (if mention.orientation.is_some() {
                PLACEABLE_ORIENTATION_OBSERVATION
            } else {
                0
            }) | (if mention.placeable_appearance.is_some() {
                PLACEABLE_APPEARANCE_OBSERVATION
            } else {
                0
            }) | (if mention.position.is_some() {
                PLACEABLE_POSITION_OBSERVATION
            } else {
                0
            });
            if mention.object_type != 0x09 || observation_mask == 0 {
                continue;
            }
            let seen_mask = seen_observation_masks
                .entry(registry_object_id)
                .or_insert(0_u8);
            let new_observation_mask = observation_mask & !*seen_mask;
            if new_observation_mask == 0 {
                continue;
            }
            *seen_mask |= observation_mask;
            let observes_state = (new_observation_mask & PLACEABLE_STATE_OBSERVATION) != 0;
            let observes_orientation =
                (new_observation_mask & PLACEABLE_ORIENTATION_OBSERVATION) != 0;
            let observes_appearance =
                (new_observation_mask & PLACEABLE_APPEARANCE_OBSERVATION) != 0;
            let observes_position = (new_observation_mask & PLACEABLE_POSITION_OBSERVATION) != 0;

            let Some(known) = self.known.get(&registry_object_id) else {
                continue;
            };
            let placeable_state = known.placeable_state;
            let placeable_appearance = known.placeable_appearance;
            let live_orientation = known.orientation;
            let live_position = known.position;
            let overlap = area_context.placeable_overlap_by(|row_object_id| {
                object_ids::equivalent_legacy_external_object_ids(row_object_id, mention.object_id)
            });
            if overlap.is_empty() {
                continue;
            }

            let identity_conflict = overlap.identity_conflict();
            let conflict = if observes_state {
                let Some(placeable_state) = placeable_state else {
                    continue;
                };
                let observed = AreaPlaceableObservedState {
                    useable: placeable_state.useable,
                    trap_disarmable: placeable_state.trap_disarmable,
                    lockable: placeable_state.lockable,
                    locked: placeable_state.locked,
                };
                overlap.static_module_state_conflict(observed)
            } else {
                AreaPlaceableContextStateConflict::default()
            };
            let orientation_conflict = if observes_orientation {
                overlap.static_module_orientation_conflict(live_orientation)
            } else {
                None
            };
            let appearance_conflict = if observes_appearance {
                overlap.static_module_appearance_conflict(placeable_appearance)
            } else {
                None
            };
            let position_conflict = if observes_position {
                overlap.static_module_position_conflict(live_position)
            } else {
                None
            };
            let conflict_fields = conflict.formatted_fields();
            let area_rows = overlap.formatted_rows();
            let area_light_duplicate = overlap.has_light_row();
            let area_static_duplicate = overlap.has_static_row();
            let known_active = known.active;
            let known_mentions = known.mentions;
            let add_mentions = known.add_mentions;
            let update_mentions = known.update_mentions;
            let last_opcode = known.last_opcode;
            let prior_unresolved_conflict = known.unresolved_area_static_state_conflict;
            let prior_unresolved_conflict_fields = prior_unresolved_conflict
                .map(AreaPlaceableContextStateConflict::formatted_fields)
                .unwrap_or_else(|| "none".to_string());
            let resolved_prior_conflict =
                observes_state && prior_unresolved_conflict.is_some() && !conflict.any();
            let prior_unresolved_identity_conflict = known.unresolved_area_static_identity_conflict;
            let resolved_prior_identity_conflict =
                prior_unresolved_identity_conflict.is_some() && identity_conflict.is_none();
            let prior_unresolved_appearance_conflict =
                known.unresolved_area_static_appearance_conflict;
            let resolved_prior_appearance_conflict = observes_appearance
                && prior_unresolved_appearance_conflict.is_some()
                && appearance_conflict.is_none();
            let prior_unresolved_orientation_conflict =
                known.unresolved_area_static_orientation_conflict;
            let resolved_prior_orientation_conflict = observes_orientation
                && prior_unresolved_orientation_conflict.is_some()
                && orientation_conflict.is_none();
            let prior_unresolved_position_conflict = known.unresolved_area_static_position_conflict;
            let resolved_prior_position_conflict = observes_position
                && prior_unresolved_position_conflict.is_some()
                && position_conflict.is_none();

            if let Some(known) = self.known.get_mut(&registry_object_id) {
                known.area_placeable_context_overlaps =
                    known.area_placeable_context_overlaps.saturating_add(1);
                known.latest_area_static_identity_conflict = identity_conflict;
                if let Some(conflict) = identity_conflict {
                    known.area_static_identity_conflicts =
                        known.area_static_identity_conflicts.saturating_add(1);
                    known.unresolved_area_static_identity_conflict = Some(conflict);
                } else if known
                    .unresolved_area_static_identity_conflict
                    .take()
                    .is_some()
                {
                    known.area_static_identity_conflict_resolutions = known
                        .area_static_identity_conflict_resolutions
                        .saturating_add(1);
                }
                if observes_state {
                    known.latest_area_static_state_conflict = Some(conflict);
                    if conflict.any() {
                        known.area_static_state_conflicts =
                            known.area_static_state_conflicts.saturating_add(1);
                        known.unresolved_area_static_state_conflict = Some(conflict);
                    } else if known.unresolved_area_static_state_conflict.take().is_some() {
                        known.area_static_state_conflict_resolutions = known
                            .area_static_state_conflict_resolutions
                            .saturating_add(1);
                    }
                }
                if observes_appearance {
                    known.latest_area_static_appearance_conflict = appearance_conflict;
                    if let Some(conflict) = appearance_conflict {
                        known.area_static_appearance_conflicts =
                            known.area_static_appearance_conflicts.saturating_add(1);
                        known.unresolved_area_static_appearance_conflict = Some(conflict);
                    } else if known
                        .unresolved_area_static_appearance_conflict
                        .take()
                        .is_some()
                    {
                        known.area_static_appearance_conflict_resolutions = known
                            .area_static_appearance_conflict_resolutions
                            .saturating_add(1);
                    }
                }
                if observes_orientation {
                    known.latest_area_static_orientation_conflict = orientation_conflict;
                    if let Some(conflict) = orientation_conflict {
                        known.area_static_orientation_conflicts =
                            known.area_static_orientation_conflicts.saturating_add(1);
                        known.unresolved_area_static_orientation_conflict = Some(conflict);
                    } else if known
                        .unresolved_area_static_orientation_conflict
                        .take()
                        .is_some()
                    {
                        known.area_static_orientation_conflict_resolutions = known
                            .area_static_orientation_conflict_resolutions
                            .saturating_add(1);
                    }
                }
                if observes_position {
                    known.latest_area_static_position_conflict = position_conflict;
                    if let Some(conflict) = position_conflict {
                        known.area_static_position_conflicts =
                            known.area_static_position_conflicts.saturating_add(1);
                        known.unresolved_area_static_position_conflict = Some(conflict);
                    } else if known
                        .unresolved_area_static_position_conflict
                        .take()
                        .is_some()
                    {
                        known.area_static_position_conflict_resolutions = known
                            .area_static_position_conflict_resolutions
                            .saturating_add(1);
                    }
                }
            }

            if identity_conflict.is_some()
                || conflict.any()
                || appearance_conflict.is_some()
                || orientation_conflict.is_some()
                || position_conflict.is_some()
            {
                tracing::info!(
                    object_id = format_args!("0x{registry_object_id:08X}"),
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    area_resref = area_context.area_resref.as_str(),
                    active = known_active,
                    last_opcode = %char::from(last_opcode),
                    mentions = known_mentions,
                    add_mentions,
                    update_mentions,
                    area_light_duplicate,
                    area_static_duplicate,
                    merged_placeable_appearance = ?placeable_appearance,
                    merged_placeable_state = ?placeable_state,
                    live_orientation = ?live_orientation,
                    live_position = ?live_position,
                    area_module_identity_mismatch = ?identity_conflict,
                    area_module_state_mismatch_fields = %conflict_fields,
                    area_module_appearance_mismatch = ?appearance_conflict,
                    area_module_orientation_mismatch = ?orientation_conflict,
                    area_module_position_mismatch = ?position_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation/position conflicts with module-backed area/static context"
                );
            } else if resolved_prior_identity_conflict
                || resolved_prior_conflict
                || resolved_prior_appearance_conflict
                || resolved_prior_orientation_conflict
                || resolved_prior_position_conflict
            {
                tracing::info!(
                    object_id = format_args!("0x{registry_object_id:08X}"),
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    area_resref = area_context.area_resref.as_str(),
                    active = known_active,
                    last_opcode = %char::from(last_opcode),
                    mentions = known_mentions,
                    add_mentions,
                    update_mentions,
                    area_light_duplicate,
                    area_static_duplicate,
                    merged_placeable_appearance = ?placeable_appearance,
                    merged_placeable_state = ?placeable_state,
                    live_orientation = ?live_orientation,
                    live_position = ?live_position,
                    previous_area_module_identity_mismatch = ?prior_unresolved_identity_conflict,
                    previous_area_module_state_mismatch_fields = %prior_unresolved_conflict_fields,
                    previous_area_module_appearance_mismatch = ?prior_unresolved_appearance_conflict,
                    previous_area_module_orientation_mismatch = ?prior_unresolved_orientation_conflict,
                    previous_area_module_position_mismatch = ?prior_unresolved_position_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation/position resolved prior module-backed area/static conflict"
                );
            } else {
                tracing::debug!(
                    object_id = format_args!("0x{registry_object_id:08X}"),
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    area_resref = area_context.area_resref.as_str(),
                    active = known_active,
                    last_opcode = %char::from(last_opcode),
                    mentions = known_mentions,
                    area_light_duplicate,
                    area_static_duplicate,
                    merged_placeable_appearance = ?placeable_appearance,
                    merged_placeable_state = ?placeable_state,
                    live_orientation = ?live_orientation,
                    live_position = ?live_position,
                    area_module_identity_mismatch = ?identity_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation/position overlaps area/static context"
                );
            }
        }
    }

    fn registry_object_id_for_live_object(&self, object_type: u8, object_id: u32) -> u32 {
        if object_type != PLACEABLE_OBJECT_TYPE {
            return object_id;
        }
        if self.known.contains_key(&object_id) {
            return object_id;
        }

        self.known
            .values()
            .find(|object| {
                object.object_type == PLACEABLE_OBJECT_TYPE
                    && object_ids::equivalent_legacy_external_object_ids(
                        object.object_id,
                        object_id,
                    )
            })
            .map(|object| object.object_id)
            .unwrap_or(object_id)
    }

    pub(crate) fn get(&self, object_type: u8, object_id: u32) -> Option<&KnownObjectState> {
        let object = self.known.get(&object_id)?;
        (object.object_type == object_type).then_some(object)
    }

    pub(crate) fn observe_materialized_item_object_ids(&mut self, object_ids: &[u32]) {
        for object_id in object_ids.iter().copied() {
            if !valid_inventory_item_context_id(object_id) {
                continue;
            }
            self.materialized_item_object_ids.insert(object_id);
            self.cleared_inventory_item_object_ids.remove(&object_id);
        }
    }

    fn forget_inventory_item_object_id(&mut self, object_id: u32) {
        let removed_materialized = self.materialized_item_object_ids.remove(&object_id);
        let removed_first = self.inventory_feature25_first_item_refs.remove(&object_id);
        let removed_second = self.inventory_feature25_second_item_refs.remove(&object_id);
        let removed_legacy_tail = self
            .inventory_feature25_legacy_tail_item_refs
            .remove(&object_id);
        self.remember_inventory_item_object_clear(
            object_id,
            InventoryItemObjectClearReason::ItemDelete,
        );
        if removed_materialized || removed_first || removed_second || removed_legacy_tail {
            tracing::debug!(
                object_id = format_args!("0x{object_id:08X}"),
                removed_materialized,
                removed_feature25_first = removed_first,
                removed_feature25_second = removed_second,
                removed_feature25_legacy_tail = removed_legacy_tail,
                "live-object item delete cleared deferred inventory item proof"
            );
        }
    }

    fn remember_inventory_item_proofs_cleared_by_area_reset(&mut self) {
        self.cleared_inventory_item_object_ids.clear();
        let mut cleared_ids: Vec<u32> = self
            .materialized_item_object_ids
            .iter()
            .chain(self.inventory_feature25_first_item_refs.iter())
            .chain(self.inventory_feature25_second_item_refs.iter())
            .chain(self.inventory_feature25_legacy_tail_item_refs.iter())
            .copied()
            .collect();
        cleared_ids.extend(
            self.known
                .values()
                .filter(|object| object.active && object.object_type == ITEM_OBJECT_TYPE)
                .map(|object| object.object_id),
        );
        cleared_ids.sort_unstable();
        cleared_ids.dedup();
        for object_id in cleared_ids {
            self.remember_inventory_item_object_clear(
                object_id,
                InventoryItemObjectClearReason::AreaReset,
            );
        }
    }

    fn remember_inventory_item_object_clear(
        &mut self,
        object_id: u32,
        reason: InventoryItemObjectClearReason,
    ) {
        if valid_inventory_item_context_id(object_id) {
            self.cleared_inventory_item_object_ids
                .insert(object_id, reason);
        }
    }

    pub(crate) fn observe_inventory_feature25_references(
        &mut self,
        references: &[LiveObjectInventoryFeature25Reference],
    ) {
        for reference in references {
            self.inventory_feature25_reference_records =
                self.inventory_feature25_reference_records.saturating_add(1);
            let first_observation =
                classify_inventory_feature25_item_refs(&reference.first_object_ids, |object_id| {
                    self.has_known_inventory_item_object_id(object_id)
                });
            let second_observation =
                classify_inventory_feature25_item_refs(&reference.second_object_ids, |object_id| {
                    self.has_known_inventory_item_object_id(object_id)
                });
            let legacy_tail_observation = classify_inventory_feature25_item_refs(
                &reference.legacy_tail_object_ids,
                |object_id| self.has_known_inventory_item_object_id(object_id),
            );
            let first_refs = observe_inventory_feature25_item_refs(
                &mut self.inventory_feature25_first_item_refs,
                &mut self.cleared_inventory_item_object_ids,
                &reference.first_object_ids,
            );
            let second_refs = observe_inventory_feature25_item_refs(
                &mut self.inventory_feature25_second_item_refs,
                &mut self.cleared_inventory_item_object_ids,
                &reference.second_object_ids,
            );
            let legacy_tail_refs = observe_inventory_feature25_item_refs(
                &mut self.inventory_feature25_legacy_tail_item_refs,
                &mut self.cleared_inventory_item_object_ids,
                &reference.legacy_tail_object_ids,
            );
            debug_assert_eq!(first_refs, first_observation.accepted);
            debug_assert_eq!(second_refs, second_observation.accepted);
            debug_assert_eq!(legacy_tail_refs, legacy_tail_observation.accepted);
            self.inventory_feature25_first_item_ref_mentions = self
                .inventory_feature25_first_item_ref_mentions
                .saturating_add(first_refs);
            self.inventory_feature25_second_item_ref_mentions = self
                .inventory_feature25_second_item_ref_mentions
                .saturating_add(second_refs);
            self.inventory_feature25_legacy_tail_item_ref_mentions = self
                .inventory_feature25_legacy_tail_item_ref_mentions
                .saturating_add(legacy_tail_refs);
            self.inventory_feature25_first_materialized_item_ref_mentions = self
                .inventory_feature25_first_materialized_item_ref_mentions
                .saturating_add(first_observation.materialized);
            self.inventory_feature25_first_deferred_item_ref_mentions = self
                .inventory_feature25_first_deferred_item_ref_mentions
                .saturating_add(first_observation.deferred);
            self.inventory_feature25_second_materialized_item_ref_mentions = self
                .inventory_feature25_second_materialized_item_ref_mentions
                .saturating_add(second_observation.materialized);
            self.inventory_feature25_second_deferred_item_ref_mentions = self
                .inventory_feature25_second_deferred_item_ref_mentions
                .saturating_add(second_observation.deferred);
            self.inventory_feature25_legacy_tail_materialized_item_ref_mentions = self
                .inventory_feature25_legacy_tail_materialized_item_ref_mentions
                .saturating_add(legacy_tail_observation.materialized);
            self.inventory_feature25_legacy_tail_deferred_item_ref_mentions = self
                .inventory_feature25_legacy_tail_deferred_item_ref_mentions
                .saturating_add(legacy_tail_observation.deferred);
            if first_refs != 0 || second_refs != 0 || legacy_tail_refs != 0 {
                tracing::debug!(
                    owner_id = format_args!("0x{:08X}", reference.owner_id),
                    mask = format_args!("0x{:04X}", reference.mask),
                    first_refs,
                    first_materialized_refs = first_observation.materialized,
                    first_deferred_refs = first_observation.deferred,
                    second_refs,
                    second_materialized_refs = second_observation.materialized,
                    second_deferred_refs = second_observation.deferred,
                    legacy_tail_refs,
                    legacy_tail_materialized_refs = legacy_tail_observation.materialized,
                    legacy_tail_deferred_refs = legacy_tail_observation.deferred,
                    "semantic object registry observed deferred inventory Feature-25 item references"
                );
            }
        }
    }

    pub(crate) fn inventory_item_object_proof(
        &self,
        object_id: u32,
    ) -> Option<InventoryItemObjectProof> {
        match self.inventory_item_object_status(object_id) {
            InventoryItemObjectStatus::Proven(proof) => Some(proof),
            InventoryItemObjectStatus::DeferredFeature25(_)
            | InventoryItemObjectStatus::ClearedByItemDelete
            | InventoryItemObjectStatus::ClearedByAreaReset
            | InventoryItemObjectStatus::Unknown => None,
        }
    }

    fn inventory_feature25_item_object_proof(
        &self,
        object_id: u32,
    ) -> Option<InventoryItemObjectProof> {
        if self
            .inventory_feature25_first_item_refs
            .contains(&object_id)
        {
            return Some(InventoryItemObjectProof::Feature25FirstList);
        }
        if self
            .inventory_feature25_second_item_refs
            .contains(&object_id)
        {
            return Some(InventoryItemObjectProof::Feature25SecondList);
        }
        if self
            .inventory_feature25_legacy_tail_item_refs
            .contains(&object_id)
        {
            return Some(InventoryItemObjectProof::Feature25LegacyTail);
        }
        None
    }

    pub(crate) fn inventory_item_object_status(&self, object_id: u32) -> InventoryItemObjectStatus {
        if self.has_known_inventory_item_object_id(object_id) {
            return InventoryItemObjectStatus::Proven(InventoryItemObjectProof::ActiveObject);
        }
        if self
            .inventory_feature25_first_item_refs
            .contains(&object_id)
        {
            return InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25FirstList,
            );
        }
        if self
            .inventory_feature25_second_item_refs
            .contains(&object_id)
        {
            return InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25SecondList,
            );
        }
        if self
            .inventory_feature25_legacy_tail_item_refs
            .contains(&object_id)
        {
            return InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25LegacyTail,
            );
        }
        match self.cleared_inventory_item_object_ids.get(&object_id) {
            Some(InventoryItemObjectClearReason::ItemDelete) => {
                InventoryItemObjectStatus::ClearedByItemDelete
            }
            Some(InventoryItemObjectClearReason::AreaReset) => {
                InventoryItemObjectStatus::ClearedByAreaReset
            }
            None => InventoryItemObjectStatus::Unknown,
        }
    }

    pub(crate) fn inventory_item_object_proven_neighborhood(
        &self,
        object_id: u32,
    ) -> InventoryItemObjectProvenNeighborhood {
        let mut proven_ids = self.materialized_item_object_ids.clone();
        proven_ids.extend(
            self.known
                .values()
                .filter(|object| object.active && object.object_type == ITEM_OBJECT_TYPE)
                .map(|object| object.object_id),
        );
        let lower = proven_ids
            .range((Bound::Unbounded, Bound::Excluded(object_id)))
            .next_back()
            .copied()
            .map(|item_object_id| {
                InventoryItemObjectProvenNeighbor::new(object_id, item_object_id)
            });
        let higher = proven_ids
            .range((Bound::Excluded(object_id), Bound::Unbounded))
            .next()
            .copied()
            .map(|item_object_id| {
                InventoryItemObjectProvenNeighbor::new(object_id, item_object_id)
            });
        InventoryItemObjectProvenNeighborhood { lower, higher }
    }

    fn compact_item_emission_candidate(
        &self,
        direct_item_proof_objects: &BTreeSet<u32>,
        feature25_item_proof_objects: &BTreeSet<u32>,
        preferred_ready_object_id: Option<u32>,
    ) -> Option<InventoryItemContextCandidate> {
        if let Some(candidate) = Self::preferred_ready_item_candidate(
            direct_item_proof_objects,
            feature25_item_proof_objects,
            preferred_ready_object_id,
        ) {
            return Some(candidate);
        }

        if let Some(object_id) = direct_item_proof_objects
            .difference(feature25_item_proof_objects)
            .next()
            .copied()
        {
            return Some(InventoryItemContextCandidate {
                object_id,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            });
        }

        if let Some(object_id) = direct_item_proof_objects
            .intersection(feature25_item_proof_objects)
            .next()
            .copied()
        {
            return Some(InventoryItemContextCandidate {
                object_id,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::Shared,
            });
        }

        let object_id = feature25_item_proof_objects
            .difference(direct_item_proof_objects)
            .next()
            .copied()?;
        Some(InventoryItemContextCandidate {
            object_id,
            proof: self
                .inventory_feature25_item_object_proof(object_id)
                .unwrap_or(InventoryItemObjectProof::Feature25FirstList),
            source: InventoryItemContextCandidateSource::Feature25Only,
        })
    }

    fn compact_item_emission_ready_candidate(
        &self,
        direct_item_proof_objects: &BTreeSet<u32>,
        feature25_item_proof_objects: &BTreeSet<u32>,
        preferred_ready_object_id: Option<u32>,
    ) -> Option<InventoryItemContextCandidate> {
        if let Some(candidate) = Self::preferred_ready_item_candidate(
            direct_item_proof_objects,
            feature25_item_proof_objects,
            preferred_ready_object_id,
        ) {
            return Some(candidate);
        }

        if let Some(object_id) = direct_item_proof_objects
            .difference(feature25_item_proof_objects)
            .next()
            .copied()
        {
            return Some(InventoryItemContextCandidate {
                object_id,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            });
        }

        let object_id = direct_item_proof_objects
            .intersection(feature25_item_proof_objects)
            .next()
            .copied()?;
        Some(InventoryItemContextCandidate {
            object_id,
            proof: InventoryItemObjectProof::ActiveObject,
            source: InventoryItemContextCandidateSource::Shared,
        })
    }

    fn preferred_ready_item_candidate(
        direct_item_proof_objects: &BTreeSet<u32>,
        feature25_item_proof_objects: &BTreeSet<u32>,
        preferred_ready_object_id: Option<u32>,
    ) -> Option<InventoryItemContextCandidate> {
        let object_id = preferred_ready_object_id?;
        if !direct_item_proof_objects.contains(&object_id) {
            return None;
        }
        Some(InventoryItemContextCandidate {
            object_id,
            proof: InventoryItemObjectProof::ActiveObject,
            source: if feature25_item_proof_objects.contains(&object_id) {
                InventoryItemContextCandidateSource::Shared
            } else {
                InventoryItemContextCandidateSource::DirectOnly
            },
        })
    }

    pub(crate) fn has_known_inventory_item_object_id(&self, object_id: u32) -> bool {
        if self.materialized_item_object_ids.contains(&object_id) {
            return true;
        }
        self.known
            .get(&object_id)
            .map(|object| object.active && object.object_type == ITEM_OBJECT_TYPE)
            .unwrap_or(false)
    }

    pub(crate) fn inventory_item_context_summary(&self) -> InventoryItemContextSummary {
        self.inventory_item_context_summary_with_preferred_ready_candidate(None)
    }

    pub(crate) fn inventory_item_context_summary_with_preferred_ready_candidate(
        &self,
        preferred_ready_object_id: Option<u32>,
    ) -> InventoryItemContextSummary {
        let active_item_objects = self
            .known
            .values()
            .filter(|object| object.active && object.object_type == ITEM_OBJECT_TYPE)
            .map(|object| object.object_id)
            .collect::<BTreeSet<_>>();
        let mut direct_item_proof_objects = self.materialized_item_object_ids.clone();
        direct_item_proof_objects.extend(active_item_objects.iter().copied());
        let mut feature25_item_proof_objects = self.inventory_feature25_first_item_refs.clone();
        feature25_item_proof_objects
            .extend(self.inventory_feature25_second_item_refs.iter().copied());
        feature25_item_proof_objects.extend(
            self.inventory_feature25_legacy_tail_item_refs
                .iter()
                .copied(),
        );
        let mut compact_item_emission_proof_objects = direct_item_proof_objects.clone();
        compact_item_emission_proof_objects.extend(feature25_item_proof_objects.iter().copied());
        let compact_item_emission_direct_only_proof_objects = direct_item_proof_objects
            .difference(&feature25_item_proof_objects)
            .count();
        let compact_item_emission_feature25_only_proof_objects = feature25_item_proof_objects
            .difference(&direct_item_proof_objects)
            .count();
        let compact_item_emission_shared_proof_objects = direct_item_proof_objects
            .intersection(&feature25_item_proof_objects)
            .count();
        let compact_item_emission_candidate = self.compact_item_emission_candidate(
            &direct_item_proof_objects,
            &feature25_item_proof_objects,
            preferred_ready_object_id,
        );
        let compact_item_emission_ready_objects = direct_item_proof_objects.len();
        let compact_item_emission_ready_candidate = self.compact_item_emission_ready_candidate(
            &direct_item_proof_objects,
            &feature25_item_proof_objects,
            preferred_ready_object_id,
        );
        InventoryItemContextSummary {
            active_item_objects: active_item_objects.len(),
            materialized_item_objects: self.materialized_item_object_ids.len(),
            direct_item_proof_objects: direct_item_proof_objects.len(),
            feature25_item_proof_objects: feature25_item_proof_objects.len(),
            compact_item_emission_proof_objects: compact_item_emission_proof_objects.len(),
            compact_item_emission_candidate,
            compact_item_emission_ready_objects,
            compact_item_emission_ready_candidate,
            preserved_active_item_actionable_missing_use_count_slot_mask: 0,
            compact_item_emission_direct_only_proof_objects,
            compact_item_emission_feature25_only_proof_objects,
            compact_item_emission_shared_proof_objects,
            compact_item_emission_deferred_feature25_only_objects:
                compact_item_emission_feature25_only_proof_objects,
            inventory_feature25_first_item_refs: self.inventory_feature25_first_item_refs.len(),
            inventory_feature25_second_item_refs: self.inventory_feature25_second_item_refs.len(),
            inventory_feature25_legacy_tail_item_refs: self
                .inventory_feature25_legacy_tail_item_refs
                .len(),
            cleared_inventory_item_object_ids: self.cleared_inventory_item_object_ids.len(),
            inventory_feature25_reference_records: self.inventory_feature25_reference_records,
            inventory_feature25_first_item_ref_mentions: self
                .inventory_feature25_first_item_ref_mentions,
            inventory_feature25_second_item_ref_mentions: self
                .inventory_feature25_second_item_ref_mentions,
            inventory_feature25_legacy_tail_item_ref_mentions: self
                .inventory_feature25_legacy_tail_item_ref_mentions,
            inventory_feature25_first_materialized_item_ref_mentions: self
                .inventory_feature25_first_materialized_item_ref_mentions,
            inventory_feature25_first_deferred_item_ref_mentions: self
                .inventory_feature25_first_deferred_item_ref_mentions,
            inventory_feature25_second_materialized_item_ref_mentions: self
                .inventory_feature25_second_materialized_item_ref_mentions,
            inventory_feature25_second_deferred_item_ref_mentions: self
                .inventory_feature25_second_deferred_item_ref_mentions,
            inventory_feature25_legacy_tail_materialized_item_ref_mentions: self
                .inventory_feature25_legacy_tail_materialized_item_ref_mentions,
            inventory_feature25_legacy_tail_deferred_item_ref_mentions: self
                .inventory_feature25_legacy_tail_deferred_item_ref_mentions,
        }
    }

    pub(crate) fn has_active_object_id(&self, object_id: u32) -> bool {
        if self.materialized_item_object_ids.contains(&object_id) {
            return true;
        }
        if self
            .known
            .get(&object_id)
            .map(|object| object.active)
            .unwrap_or(false)
        {
            return true;
        }
        // Untyped live-object owner records, especially inventory `I` rows,
        // carry only an OBJECTID. If that owner is a placeable, it must share
        // the same compact/external alias rule as typed U/D/09 lifecycle
        // checks before missing-object cleanup is allowed to drop the row.
        self.placeable_object_for_record_matching(PLACEABLE_OBJECT_TYPE, object_id, |object| {
            object.active
        })
        .is_some()
    }

    pub(crate) fn has_active_typed_object(&self, object_type: u8, object_id: u32) -> bool {
        if self.materialized_item_object_ids.contains(&object_id) {
            return true;
        }
        if object_type == PLACEABLE_OBJECT_TYPE {
            // Live-object placeable records can mention the same runtime
            // object through either the legacy compact id or the external EE
            // id. Observation already merges those aliases; lifecycle checks
            // must use the same owner rule before removing missing-object rows.
            return self
                .placeable_object_for_record_matching(object_type, object_id, |object| {
                    object.active
                })
                .is_some();
        }
        self.get(object_type, object_id)
            .map(|object| object.active)
            .unwrap_or(false)
    }

    pub(crate) fn has_active_live_object_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> bool {
        // Inventory owner records carry an OBJECTID but no independent live
        // object-type marker in the packet body. The exact inventory parser
        // reports object_type 0 for that owner field, so lifecycle proof must
        // use the already-materialized object id without inventing a type.
        let active = if object_type == 0 {
            self.has_active_object_id(object_id)
        } else {
            self.has_active_typed_object(object_type, object_id)
        };
        if (object_id & 0xFFFF_FF00) == 0xFFFF_FF00 {
            tracing::debug!(
                object_type,
                object_id,
                active,
                "semantic object registry session-local lifecycle lookup"
            );
        }
        active
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextStateConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.state)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_orientation_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextOrientationConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.orientation)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_position_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextPositionConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.position)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_appearance_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextAppearanceConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.appearance)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_identity_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextIdentityConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.identity)
    }

    pub(crate) fn unresolved_area_static_placeable_conflict_snapshot_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaStaticPlaceableConflictSnapshot<'_>> {
        self.placeable_object_for_record_matching(object_type, object_id, |object| {
            unresolved_area_static_placeable_conflict_snapshot(object).is_some()
        })
        .and_then(unresolved_area_static_placeable_conflict_snapshot)
    }

    pub(crate) fn unresolved_area_static_placeable_conflict_summary_for_records<I>(
        &self,
        records: I,
    ) -> AreaStaticPlaceableConflictRecordSummary
    where
        I: IntoIterator<Item = (u8, u32)>,
    {
        let mut summary = AreaStaticPlaceableConflictRecordSummary::default();
        let mut seen_owner_ids = BTreeSet::new();
        for (object_type, object_id) in records {
            let Some(snapshot) = self
                .unresolved_area_static_placeable_conflict_snapshot_for_record(
                    object_type,
                    object_id,
                )
            else {
                continue;
            };
            if seen_owner_ids.insert(snapshot.object.object_id) {
                summary.record(snapshot);
            }
        }
        summary
    }

    pub(crate) fn unresolved_area_static_placeable_conflict_progress_for_records<I>(
        &self,
        records: I,
    ) -> AreaStaticPlaceableConflictRecordProgressSummary
    where
        I: IntoIterator<Item = AreaStaticPlaceableConflictRecordObservation>,
    {
        let mut owner_progress = BTreeMap::new();
        for record in records {
            let Some(snapshot) = self
                .unresolved_area_static_placeable_conflict_snapshot_for_record(
                    record.object_type,
                    record.object_id,
                )
            else {
                continue;
            };
            let progress = snapshot.progress_for_observation(record);
            owner_progress
                .entry(snapshot.object.object_id)
                .and_modify(|existing: &mut AreaStaticPlaceableConflictRecordProgress| {
                    existing.merge(progress);
                })
                .or_insert(progress);
        }

        let mut summary = AreaStaticPlaceableConflictRecordProgressSummary::default();
        for progress in owner_progress.values().copied() {
            summary.record(progress);
        }
        summary
    }

    #[cfg(test)]
    pub(crate) fn active_placeable_with_unresolved_area_static_context_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<&KnownObjectState> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .map(|snapshot| snapshot.object)
    }

    fn placeable_object_for_record_matching<F>(
        &self,
        object_type: u8,
        object_id: u32,
        mut predicate: F,
    ) -> Option<&KnownObjectState>
    where
        F: FnMut(&KnownObjectState) -> bool,
    {
        // Inventory owner records carry an OBJECTID without an independent
        // live-object type. Let those untyped owners reuse the same placeable
        // compact/external alias rule while keeping typed non-placeable rows out.
        if object_type != 0 && object_type != PLACEABLE_OBJECT_TYPE {
            return None;
        }

        if let Some(object) = self.known.get(&object_id) {
            if object.object_type == PLACEABLE_OBJECT_TYPE && predicate(object) {
                return Some(object);
            }
        }

        self.known.values().find(|object| {
            object.object_type == PLACEABLE_OBJECT_TYPE
                && object_ids::equivalent_legacy_external_object_ids(object.object_id, object_id)
                && predicate(object)
        })
    }

    pub(crate) fn session_creature_id_for_compact(&self, compact_id: u32) -> Option<u32> {
        self.session_creature_ids_by_compact
            .get(&compact_id)
            .copied()
    }
}

fn unresolved_area_static_placeable_conflict_snapshot(
    object: &KnownObjectState,
) -> Option<AreaStaticPlaceableConflictSnapshot<'_>> {
    if !object.active {
        return None;
    }
    let snapshot = AreaStaticPlaceableConflictSnapshot {
        object,
        identity: object.unresolved_area_static_identity_conflict,
        appearance: object.unresolved_area_static_appearance_conflict,
        state: object.unresolved_area_static_state_conflict,
        orientation: object.unresolved_area_static_orientation_conflict,
        position: object.unresolved_area_static_position_conflict,
    };
    snapshot.any().then_some(snapshot)
}

trait AreaPlaceableContextAppearanceOverlap {
    fn static_module_appearance_conflict(
        &self,
        observed: Option<LiveObjectPlaceableAppearance>,
    ) -> Option<AreaPlaceableContextAppearanceConflict>;
}

impl AreaPlaceableContextAppearanceOverlap for AreaPlaceableContextOverlap<'_> {
    fn static_module_appearance_conflict(
        &self,
        observed: Option<LiveObjectPlaceableAppearance>,
    ) -> Option<AreaPlaceableContextAppearanceConflict> {
        let observed = observed?;
        let module = self.unique_module_backed_static_row()?;
        (observed.appearance != module.appearance).then_some(
            AreaPlaceableContextAppearanceConflict {
                observed_appearance: observed.appearance,
                observed_resref: observed.resref,
                module_appearance: module.appearance,
                module_template_resref: module.module_template_resref,
            },
        )
    }
}

trait AreaPlaceableContextOrientationOverlap {
    fn static_module_orientation_conflict(
        &self,
        observed: Option<LiveObjectOrientation>,
    ) -> Option<AreaPlaceableContextOrientationConflict>;
}

impl AreaPlaceableContextOrientationOverlap for AreaPlaceableContextOverlap<'_> {
    fn static_module_orientation_conflict(
        &self,
        observed: Option<LiveObjectOrientation>,
    ) -> Option<AreaPlaceableContextOrientationConflict> {
        let observed = observed?;
        let module = area_static_row_scalar_orientation(self.unique_module_backed_static_row()?)?;
        (observed.scalar_tenths_degrees != module).then_some(
            AreaPlaceableContextOrientationConflict {
                observed_source: match observed.source {
                    LiveObjectOrientationSource::Scalar => {
                        AreaPlaceableObservedOrientationSource::Scalar
                    }
                    LiveObjectOrientationSource::Vector => {
                        AreaPlaceableObservedOrientationSource::Vector
                    }
                },
                observed_scalar_tenths_degrees: observed.scalar_tenths_degrees,
                module_scalar_tenths_degrees: module,
            },
        )
    }
}

trait AreaPlaceableContextPositionOverlap {
    fn static_module_position_conflict(
        &self,
        observed: Option<LiveObjectPosition>,
    ) -> Option<AreaPlaceableContextPositionConflict>;
}

impl AreaPlaceableContextPositionOverlap for AreaPlaceableContextOverlap<'_> {
    fn static_module_position_conflict(
        &self,
        observed: Option<LiveObjectPosition>,
    ) -> Option<AreaPlaceableContextPositionConflict> {
        let observed = observed?;
        let module = self.unique_module_backed_static_row()?;
        if !observed.x.is_finite()
            || !observed.y.is_finite()
            || !observed.z.is_finite()
            || !module.x.is_finite()
            || !module.y.is_finite()
            || !module.z.is_finite()
        {
            return None;
        }
        let differs = (observed.x - module.x).abs() > PLACEABLE_POSITION_EPSILON
            || (observed.y - module.y).abs() > PLACEABLE_POSITION_EPSILON
            || (observed.z - module.z).abs() > PLACEABLE_POSITION_EPSILON;
        differs.then_some(AreaPlaceableContextPositionConflict {
            observed_x: observed.x,
            observed_y: observed.y,
            observed_z: observed.z,
            module_x: module.x,
            module_y: module.y,
            module_z: module.z,
        })
    }
}

fn compact_session_alias_from_player_list(object_id: u32) -> Option<u32> {
    if object_id == 0 || object_id == 0x7F00_0000 || object_id == u32::MAX {
        return None;
    }
    if (object_id & 0xFFFF_FF00) != 0xFFFF_FF00 {
        return None;
    }
    let compact_id = object_id & 0xFF;
    (compact_id != 0).then_some(compact_id)
}

fn observe_inventory_feature25_item_refs(
    target: &mut BTreeSet<u32>,
    cleared: &mut BTreeMap<u32, InventoryItemObjectClearReason>,
    object_ids: &[u32],
) -> u64 {
    let mut accepted = 0_u64;
    for object_id in object_ids.iter().copied() {
        if !valid_inventory_feature25_item_ref(object_id) {
            continue;
        }
        target.insert(object_id);
        cleared.remove(&object_id);
        accepted = accepted.saturating_add(1);
    }
    accepted
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct InventoryFeature25ItemRefObservation {
    accepted: u64,
    materialized: u64,
    deferred: u64,
}

fn classify_inventory_feature25_item_refs<F>(
    object_ids: &[u32],
    mut materialized: F,
) -> InventoryFeature25ItemRefObservation
where
    F: FnMut(u32) -> bool,
{
    let mut observation = InventoryFeature25ItemRefObservation::default();
    for object_id in object_ids.iter().copied() {
        if !valid_inventory_feature25_item_ref(object_id) {
            continue;
        }
        observation.accepted = observation.accepted.saturating_add(1);
        if materialized(object_id) {
            observation.materialized = observation.materialized.saturating_add(1);
        } else {
            observation.deferred = observation.deferred.saturating_add(1);
        }
    }
    observation
}

fn valid_inventory_feature25_item_ref(object_id: u32) -> bool {
    valid_inventory_item_context_id(object_id)
}

fn valid_inventory_item_context_id(object_id: u32) -> bool {
    object_id != 0 && object_id != 0x7F00_0000 && object_id != u32::MAX
}

#[derive(Debug, Clone, Default)]
pub(crate) struct KnownObjectState {
    pub(crate) object_id: u32,
    pub(crate) object_type: u8,
    pub(crate) last_opcode: u8,
    pub(crate) active: bool,
    pub(crate) latest_name: Option<String>,
    pub(crate) position: Option<LiveObjectPosition>,
    pub(crate) orientation: Option<LiveObjectOrientation>,
    pub(crate) bounds: Option<LiveObjectBounds>,
    pub(crate) placeable_appearance: Option<LiveObjectPlaceableAppearance>,
    pub(crate) placeable_state: Option<LiveObjectPlaceableState>,
    pub(crate) mentions: u64,
    pub(crate) add_mentions: u64,
    pub(crate) update_mentions: u64,
    pub(crate) delete_mentions: u64,
    pub(crate) duplicate_add_mentions: u64,
    pub(crate) update_before_add_mentions: u64,
    pub(crate) delete_before_add_mentions: u64,
    pub(crate) area_placeable_context_overlaps: u64,
    pub(crate) area_static_identity_conflicts: u64,
    pub(crate) latest_area_static_identity_conflict: Option<AreaPlaceableContextIdentityConflict>,
    pub(crate) unresolved_area_static_identity_conflict:
        Option<AreaPlaceableContextIdentityConflict>,
    pub(crate) area_static_identity_conflict_resolutions: u64,
    pub(crate) area_static_appearance_conflicts: u64,
    pub(crate) latest_area_static_appearance_conflict:
        Option<AreaPlaceableContextAppearanceConflict>,
    pub(crate) unresolved_area_static_appearance_conflict:
        Option<AreaPlaceableContextAppearanceConflict>,
    pub(crate) area_static_appearance_conflict_resolutions: u64,
    pub(crate) area_static_state_conflicts: u64,
    pub(crate) latest_area_static_state_conflict: Option<AreaPlaceableContextStateConflict>,
    pub(crate) unresolved_area_static_state_conflict: Option<AreaPlaceableContextStateConflict>,
    pub(crate) area_static_state_conflict_resolutions: u64,
    pub(crate) area_static_orientation_conflicts: u64,
    pub(crate) latest_area_static_orientation_conflict:
        Option<AreaPlaceableContextOrientationConflict>,
    pub(crate) unresolved_area_static_orientation_conflict:
        Option<AreaPlaceableContextOrientationConflict>,
    pub(crate) area_static_orientation_conflict_resolutions: u64,
    pub(crate) area_static_position_conflicts: u64,
    pub(crate) latest_area_static_position_conflict: Option<AreaPlaceableContextPositionConflict>,
    pub(crate) unresolved_area_static_position_conflict:
        Option<AreaPlaceableContextPositionConflict>,
    pub(crate) area_static_position_conflict_resolutions: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AreaStaticPlaceableConflictSnapshot<'a> {
    pub(crate) object: &'a KnownObjectState,
    pub(crate) identity: Option<AreaPlaceableContextIdentityConflict>,
    pub(crate) appearance: Option<AreaPlaceableContextAppearanceConflict>,
    pub(crate) state: Option<AreaPlaceableContextStateConflict>,
    pub(crate) orientation: Option<AreaPlaceableContextOrientationConflict>,
    pub(crate) position: Option<AreaPlaceableContextPositionConflict>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AreaStaticPlaceableConflictRecordObservation {
    pub(crate) object_type: u8,
    pub(crate) object_id: u32,
    pub(crate) placeable_appearance: Option<LiveObjectPlaceableAppearance>,
    pub(crate) placeable_state: Option<LiveObjectPlaceableState>,
    pub(crate) orientation: Option<LiveObjectOrientation>,
    pub(crate) position: Option<LiveObjectPosition>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaStaticPlaceableConflictRecordProgress {
    pub(crate) untouched_identity: bool,
    pub(crate) resolves_appearance: bool,
    pub(crate) repeats_appearance: bool,
    pub(crate) untouched_appearance: bool,
    pub(crate) resolves_state_useable: bool,
    pub(crate) repeats_state_useable: bool,
    pub(crate) untouched_state_useable: bool,
    pub(crate) resolves_state_trap_disarmable: bool,
    pub(crate) repeats_state_trap_disarmable: bool,
    pub(crate) untouched_state_trap_disarmable: bool,
    pub(crate) resolves_state_lockable: bool,
    pub(crate) repeats_state_lockable: bool,
    pub(crate) untouched_state_lockable: bool,
    pub(crate) resolves_state_locked: bool,
    pub(crate) repeats_state_locked: bool,
    pub(crate) untouched_state_locked: bool,
    pub(crate) resolves_orientation: bool,
    pub(crate) repeats_orientation: bool,
    pub(crate) untouched_orientation: bool,
    pub(crate) resolves_position: bool,
    pub(crate) repeats_position: bool,
    pub(crate) untouched_position: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaStaticPlaceableConflictRecordProgressSummary {
    pub(crate) owners: u32,
    pub(crate) resolving_owners: u32,
    pub(crate) repeating_owners: u32,
    pub(crate) untouched_owners: u32,
    pub(crate) resolving_appearance: u32,
    pub(crate) repeating_appearance: u32,
    pub(crate) untouched_appearance: u32,
    pub(crate) resolving_state: u32,
    pub(crate) repeating_state: u32,
    pub(crate) untouched_state: u32,
    pub(crate) resolving_orientation: u32,
    pub(crate) repeating_orientation: u32,
    pub(crate) untouched_orientation: u32,
    pub(crate) resolving_position: u32,
    pub(crate) repeating_position: u32,
    pub(crate) untouched_position: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaStaticPlaceableConflictRecordSummary {
    pub(crate) owners: u32,
    pub(crate) identity: u32,
    pub(crate) appearance: u32,
    pub(crate) appearance_module_custom_target: u32,
    pub(crate) appearance_module_custom_target_with_resref: u32,
    pub(crate) appearance_module_custom_target_missing_resref: u32,
    pub(crate) appearance_module_normal_target: u32,
    pub(crate) appearance_observed_custom_source: u32,
    pub(crate) state: u32,
    pub(crate) orientation: u32,
    pub(crate) position: u32,
    pub(crate) state_useable: u32,
    pub(crate) state_trap_disarmable: u32,
    pub(crate) state_lockable: u32,
    pub(crate) state_locked: u32,
}

impl AreaStaticPlaceableConflictRecordSummary {
    pub(crate) fn any(self) -> bool {
        self.owners != 0
    }

    fn record(&mut self, snapshot: AreaStaticPlaceableConflictSnapshot<'_>) {
        self.owners = self.owners.saturating_add(1);
        if snapshot.identity.is_some() {
            self.identity = self.identity.saturating_add(1);
        }
        if snapshot.appearance.is_some() {
            self.appearance = self.appearance.saturating_add(1);
        }
        if let Some(appearance) = snapshot.appearance {
            if is_custom_placeable_appearance(appearance.module_appearance) {
                self.appearance_module_custom_target =
                    self.appearance_module_custom_target.saturating_add(1);
                if appearance.module_template_resref.is_some() {
                    self.appearance_module_custom_target_with_resref = self
                        .appearance_module_custom_target_with_resref
                        .saturating_add(1);
                } else {
                    self.appearance_module_custom_target_missing_resref = self
                        .appearance_module_custom_target_missing_resref
                        .saturating_add(1);
                }
            } else {
                self.appearance_module_normal_target =
                    self.appearance_module_normal_target.saturating_add(1);
            }
            if is_custom_placeable_appearance(appearance.observed_appearance)
                || appearance.observed_resref.is_some()
            {
                self.appearance_observed_custom_source =
                    self.appearance_observed_custom_source.saturating_add(1);
            }
        }
        if let Some(state) = snapshot.state {
            self.state = self.state.saturating_add(1);
            if state.useable {
                self.state_useable = self.state_useable.saturating_add(1);
            }
            if state.trap_disarmable {
                self.state_trap_disarmable = self.state_trap_disarmable.saturating_add(1);
            }
            if state.lockable {
                self.state_lockable = self.state_lockable.saturating_add(1);
            }
            if state.locked {
                self.state_locked = self.state_locked.saturating_add(1);
            }
        }
        if snapshot.orientation.is_some() {
            self.orientation = self.orientation.saturating_add(1);
        }
        if snapshot.position.is_some() {
            self.position = self.position.saturating_add(1);
        }
    }
}

impl AreaStaticPlaceableConflictRecordProgressSummary {
    fn record(&mut self, progress: AreaStaticPlaceableConflictRecordProgress) {
        self.owners = self.owners.saturating_add(1);
        if progress.any_resolving() {
            self.resolving_owners = self.resolving_owners.saturating_add(1);
        }
        if progress.any_repeating() {
            self.repeating_owners = self.repeating_owners.saturating_add(1);
        }
        if !progress.any_resolving() && !progress.any_repeating() && progress.any_untouched() {
            self.untouched_owners = self.untouched_owners.saturating_add(1);
        }
        if progress.resolves_appearance {
            self.resolving_appearance = self.resolving_appearance.saturating_add(1);
        }
        if progress.repeats_appearance {
            self.repeating_appearance = self.repeating_appearance.saturating_add(1);
        }
        if progress.untouched_appearance {
            self.untouched_appearance = self.untouched_appearance.saturating_add(1);
        }
        if progress.any_resolving_state() {
            self.resolving_state = self.resolving_state.saturating_add(1);
        }
        if progress.any_repeating_state() {
            self.repeating_state = self.repeating_state.saturating_add(1);
        }
        if progress.any_untouched_state() {
            self.untouched_state = self.untouched_state.saturating_add(1);
        }
        if progress.resolves_orientation {
            self.resolving_orientation = self.resolving_orientation.saturating_add(1);
        }
        if progress.repeats_orientation {
            self.repeating_orientation = self.repeating_orientation.saturating_add(1);
        }
        if progress.untouched_orientation {
            self.untouched_orientation = self.untouched_orientation.saturating_add(1);
        }
        if progress.resolves_position {
            self.resolving_position = self.resolving_position.saturating_add(1);
        }
        if progress.repeats_position {
            self.repeating_position = self.repeating_position.saturating_add(1);
        }
        if progress.untouched_position {
            self.untouched_position = self.untouched_position.saturating_add(1);
        }
    }
}

impl AreaStaticPlaceableConflictRecordProgress {
    fn merge(&mut self, other: Self) {
        self.untouched_identity |= other.untouched_identity;
        self.resolves_appearance |= other.resolves_appearance;
        self.repeats_appearance |= other.repeats_appearance;
        self.untouched_appearance |= other.untouched_appearance;
        self.resolves_state_useable |= other.resolves_state_useable;
        self.repeats_state_useable |= other.repeats_state_useable;
        self.untouched_state_useable |= other.untouched_state_useable;
        self.resolves_state_trap_disarmable |= other.resolves_state_trap_disarmable;
        self.repeats_state_trap_disarmable |= other.repeats_state_trap_disarmable;
        self.untouched_state_trap_disarmable |= other.untouched_state_trap_disarmable;
        self.resolves_state_lockable |= other.resolves_state_lockable;
        self.repeats_state_lockable |= other.repeats_state_lockable;
        self.untouched_state_lockable |= other.untouched_state_lockable;
        self.resolves_state_locked |= other.resolves_state_locked;
        self.repeats_state_locked |= other.repeats_state_locked;
        self.untouched_state_locked |= other.untouched_state_locked;
        self.resolves_orientation |= other.resolves_orientation;
        self.repeats_orientation |= other.repeats_orientation;
        self.untouched_orientation |= other.untouched_orientation;
        self.resolves_position |= other.resolves_position;
        self.repeats_position |= other.repeats_position;
        self.untouched_position |= other.untouched_position;
    }

    pub(crate) fn any_resolving(self) -> bool {
        self.resolves_appearance
            || self.any_resolving_state()
            || self.resolves_orientation
            || self.resolves_position
    }

    pub(crate) fn any_repeating(self) -> bool {
        self.repeats_appearance
            || self.any_repeating_state()
            || self.repeats_orientation
            || self.repeats_position
    }

    pub(crate) fn any_untouched(self) -> bool {
        self.untouched_identity
            || self.untouched_appearance
            || self.any_untouched_state()
            || self.untouched_orientation
            || self.untouched_position
    }

    fn any_resolving_state(self) -> bool {
        self.resolves_state_useable
            || self.resolves_state_trap_disarmable
            || self.resolves_state_lockable
            || self.resolves_state_locked
    }

    fn any_repeating_state(self) -> bool {
        self.repeats_state_useable
            || self.repeats_state_trap_disarmable
            || self.repeats_state_lockable
            || self.repeats_state_locked
    }

    fn any_untouched_state(self) -> bool {
        self.untouched_state_useable
            || self.untouched_state_trap_disarmable
            || self.untouched_state_lockable
            || self.untouched_state_locked
    }

    pub(crate) fn formatted_resolving_fields(self) -> String {
        self.format_fields(ConflictProgressFieldKind::Resolving)
    }

    pub(crate) fn formatted_repeating_fields(self) -> String {
        self.format_fields(ConflictProgressFieldKind::Repeating)
    }

    pub(crate) fn formatted_untouched_fields(self) -> String {
        self.format_fields(ConflictProgressFieldKind::Untouched)
    }

    fn format_fields(self, kind: ConflictProgressFieldKind) -> String {
        let mut fields = Vec::new();
        match kind {
            ConflictProgressFieldKind::Resolving => {
                push_if(&mut fields, self.resolves_appearance, "appearance");
                push_if(&mut fields, self.resolves_state_useable, "state.useable");
                push_if(
                    &mut fields,
                    self.resolves_state_trap_disarmable,
                    "state.trap_disarmable",
                );
                push_if(&mut fields, self.resolves_state_lockable, "state.lockable");
                push_if(&mut fields, self.resolves_state_locked, "state.locked");
                push_if(&mut fields, self.resolves_orientation, "orientation");
                push_if(&mut fields, self.resolves_position, "position");
            }
            ConflictProgressFieldKind::Repeating => {
                push_if(&mut fields, self.repeats_appearance, "appearance");
                push_if(&mut fields, self.repeats_state_useable, "state.useable");
                push_if(
                    &mut fields,
                    self.repeats_state_trap_disarmable,
                    "state.trap_disarmable",
                );
                push_if(&mut fields, self.repeats_state_lockable, "state.lockable");
                push_if(&mut fields, self.repeats_state_locked, "state.locked");
                push_if(&mut fields, self.repeats_orientation, "orientation");
                push_if(&mut fields, self.repeats_position, "position");
            }
            ConflictProgressFieldKind::Untouched => {
                push_if(&mut fields, self.untouched_identity, "identity");
                push_if(&mut fields, self.untouched_appearance, "appearance");
                push_if(&mut fields, self.untouched_state_useable, "state.useable");
                push_if(
                    &mut fields,
                    self.untouched_state_trap_disarmable,
                    "state.trap_disarmable",
                );
                push_if(&mut fields, self.untouched_state_lockable, "state.lockable");
                push_if(&mut fields, self.untouched_state_locked, "state.locked");
                push_if(&mut fields, self.untouched_orientation, "orientation");
                push_if(&mut fields, self.untouched_position, "position");
            }
        }

        if fields.is_empty() {
            "none".to_string()
        } else {
            fields.join(",")
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ConflictProgressFieldKind {
    Resolving,
    Repeating,
    Untouched,
}

fn push_if(fields: &mut Vec<&'static str>, condition: bool, field: &'static str) {
    if condition {
        fields.push(field);
    }
}

impl AreaStaticPlaceableConflictSnapshot<'_> {
    pub(crate) fn any(self) -> bool {
        self.identity.is_some()
            || self.appearance.is_some()
            || self.state.is_some()
            || self.orientation.is_some()
            || self.position.is_some()
    }

    pub(crate) fn formatted_classes(self) -> String {
        let mut classes = Vec::new();
        if self.identity.is_some() {
            classes.push("identity");
        }
        if self.appearance.is_some() {
            classes.push("appearance");
        }
        if self.state.is_some() {
            classes.push("state");
        }
        if self.orientation.is_some() {
            classes.push("orientation");
        }
        if self.position.is_some() {
            classes.push("position");
        }
        if classes.is_empty() {
            "none".to_string()
        } else {
            classes.join(",")
        }
    }

    pub(crate) fn formatted_state_fields(self) -> String {
        self.state
            .map(AreaPlaceableContextStateConflict::formatted_fields)
            .unwrap_or_else(|| "none".to_string())
    }

    pub(crate) fn progress_for_observation(
        self,
        observation: AreaStaticPlaceableConflictRecordObservation,
    ) -> AreaStaticPlaceableConflictRecordProgress {
        let mut progress = AreaStaticPlaceableConflictRecordProgress {
            untouched_identity: self.identity.is_some(),
            ..AreaStaticPlaceableConflictRecordProgress::default()
        };

        if let Some(conflict) = self.appearance {
            match observation.placeable_appearance {
                Some(observed) if observed.appearance == conflict.module_appearance => {
                    progress.resolves_appearance = true;
                }
                Some(_) => {
                    progress.repeats_appearance = true;
                }
                None => {
                    progress.untouched_appearance = true;
                }
            }
        }

        if let Some(conflict) = self.state {
            let prior = self.object.placeable_state;
            classify_state_conflict_field(
                conflict.useable,
                prior.and_then(|state| state.useable),
                observation.placeable_state.and_then(|state| state.useable),
                &mut progress.resolves_state_useable,
                &mut progress.repeats_state_useable,
                &mut progress.untouched_state_useable,
            );
            classify_state_conflict_field(
                conflict.trap_disarmable,
                prior.and_then(|state| state.trap_disarmable),
                observation
                    .placeable_state
                    .and_then(|state| state.trap_disarmable),
                &mut progress.resolves_state_trap_disarmable,
                &mut progress.repeats_state_trap_disarmable,
                &mut progress.untouched_state_trap_disarmable,
            );
            classify_state_conflict_field(
                conflict.lockable,
                prior.and_then(|state| state.lockable),
                observation.placeable_state.and_then(|state| state.lockable),
                &mut progress.resolves_state_lockable,
                &mut progress.repeats_state_lockable,
                &mut progress.untouched_state_lockable,
            );
            classify_state_conflict_field(
                conflict.locked,
                prior.and_then(|state| state.locked),
                observation.placeable_state.and_then(|state| state.locked),
                &mut progress.resolves_state_locked,
                &mut progress.repeats_state_locked,
                &mut progress.untouched_state_locked,
            );
        }

        if let Some(conflict) = self.orientation {
            match observation.orientation {
                Some(observed)
                    if observed.scalar_tenths_degrees == conflict.module_scalar_tenths_degrees =>
                {
                    progress.resolves_orientation = true;
                }
                Some(_) => {
                    progress.repeats_orientation = true;
                }
                None => {
                    progress.untouched_orientation = true;
                }
            }
        }

        if let Some(conflict) = self.position {
            match observation.position {
                Some(observed)
                    if position_matches_module(
                        observed,
                        conflict.module_x,
                        conflict.module_y,
                        conflict.module_z,
                    ) =>
                {
                    progress.resolves_position = true;
                }
                Some(_) => {
                    progress.repeats_position = true;
                }
                None => {
                    progress.untouched_position = true;
                }
            }
        }

        progress
    }
}

fn classify_state_conflict_field(
    conflicting: bool,
    prior_observed: Option<bool>,
    current_observed: Option<bool>,
    resolves: &mut bool,
    repeats: &mut bool,
    untouched: &mut bool,
) {
    if !conflicting {
        return;
    }
    match (prior_observed, current_observed) {
        (_, None) => {
            *untouched = true;
        }
        (Some(previous), Some(current)) if current != previous => {
            *resolves = true;
        }
        (Some(_), Some(_)) | (None, Some(_)) => {
            *repeats = true;
        }
    }
}

fn position_matches_module(
    position: LiveObjectPosition,
    module_x: f32,
    module_y: f32,
    module_z: f32,
) -> bool {
    (position.x - module_x).abs() <= PLACEABLE_POSITION_EPSILON
        && (position.y - module_y).abs() <= PLACEABLE_POSITION_EPSILON
        && (position.z - module_z).abs() <= PLACEABLE_POSITION_EPSILON
}

fn is_custom_placeable_appearance(appearance: u16) -> bool {
    appearance >= 0xFFFE
}

impl KnownObjectState {
    fn clear_lifecycle_facts(&mut self) {
        self.latest_name = None;
        self.position = None;
        self.orientation = None;
        self.bounds = None;
        self.placeable_appearance = None;
        self.placeable_state = None;
        self.latest_area_static_appearance_conflict = None;
        self.unresolved_area_static_appearance_conflict = None;
        self.latest_area_static_identity_conflict = None;
        self.unresolved_area_static_identity_conflict = None;
        self.latest_area_static_state_conflict = None;
        self.unresolved_area_static_state_conflict = None;
        self.latest_area_static_orientation_conflict = None;
        self.unresolved_area_static_orientation_conflict = None;
        self.latest_area_static_position_conflict = None;
        self.unresolved_area_static_position_conflict = None;
    }

    fn merge_placeable_state(&mut self, observed: LiveObjectPlaceableState) {
        let state = self.placeable_state.get_or_insert_with(Default::default);
        if observed.useable.is_some() {
            state.useable = observed.useable;
        }
        if observed.trap_disarmable.is_some() {
            state.trap_disarmable = observed.trap_disarmable;
        }
        if observed.lockable.is_some() {
            state.lockable = observed.lockable;
        }
        if observed.locked.is_some() {
            state.locked = observed.locked;
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub(crate) quickbar_packets: u64,
    pub(crate) quickbar_placeholders: u64,
    pub(crate) client_gui_event_packets: u64,
    pub(crate) client_quickbar_packets: u64,
    pub(crate) inventory_packets: u64,
    pub(crate) inventory_equipment_handoff_events: u64,
    pub(crate) inventory_equipment_handoff_ready_events: u64,
    pub(crate) inventory_equipment_handoff_blocked_without_ready_state_events: u64,
    pub(crate) inventory_equipment_handoff_ready_with_deferred_feature25_events: u64,
    pub(crate) inventory_equipment_handoff_server_inventory_events: u64,
    pub(crate) inventory_equipment_handoff_server_inventory_ready_events: u64,
    pub(crate) inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events: u64,
    pub(crate) inventory_equipment_handoff_client_gui_inventory_events: u64,
    pub(crate) inventory_equipment_handoff_client_gui_inventory_ready_events: u64,
    pub(crate) inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events:
        u64,
    pending_server_inventory_handoff_claim: Option<InventoryEquipmentServerInventoryClaim>,
    pub(crate) last_inventory_equipment_handoff: Option<InventoryEquipmentHandoffSnapshot>,
    pub(crate) inventory_equipment_bridge_handoff_emissions: u64,
    pub(crate) last_inventory_equipment_bridge_handoff_emission:
        Option<InventoryEquipmentHandoffBridgeEmission>,
    pub(crate) inventory_equipment_bridge_handoff_state_updates: u64,
    pub(crate) last_inventory_equipment_bridge_handoff_state_update:
        Option<InventoryEquipmentBridgeStateUpdate>,
    pub(crate) last_live_object_inventory_materialization:
        Option<LiveObjectInventoryMaterializationSummary>,
    pub(crate) last_quickbar_family: Option<VerifiedFamily>,
    pub(crate) quickbar_stream_probe_summaries: u64,
    pub(crate) last_quickbar_stream_probe: Option<QuickbarStreamProbeSummary>,
    pub(crate) last_quickbar_stream_probe_materialization_context:
        Option<InventoryItemContextSummary>,
    pub(crate) quickbar_item_use_count_updates_observed: u64,
    pub(crate) quickbar_item_use_count_state:
        BTreeMap<QuickbarItemUseCountKey, QuickbarItemRefreshUseCountRow>,
    pub(crate) last_quickbar_item_use_count_state: Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) last_committed_quickbar_profile: Option<QuickbarValidatedSlotProfile>,
    pub(crate) last_committed_quickbar_materialization_context: Option<InventoryItemContextSummary>,
    pub(crate) last_inventory_item_context_before_quickbar: Option<InventoryItemContextSummary>,
    pub(crate) last_committed_quickbar_prior_item_context: Option<InventoryItemContextSummary>,
    pub(crate) last_inventory_item_context_after_committed_quickbar:
        Option<InventoryItemContextSummary>,
    pub(crate) inventory_item_context_after_committed_quickbar_updates: u64,
    pub(crate) post_committed_quickbar_item_refresh_pending: bool,
    pub(crate) post_committed_quickbar_item_refresh_resolved_by_server_use_count: bool,
    pub(crate) post_committed_quickbar_item_refresh_resolved_by_prior_use_count_state: bool,
    pub(crate) post_committed_quickbar_item_refresh_pending_updates: u64,
    pub(crate) post_committed_quickbar_item_refresh_observed_actionable_missing_use_count_slot_mask:
        u64,
    pub(crate) post_committed_quickbar_item_refresh_pending_events: u64,
    pub(crate) post_committed_quickbar_item_refresh_pending_event_breakdown:
        QuickbarItemRefreshEventBreakdown,
    pub(crate) post_committed_quickbar_item_refresh_events_after_first_client_action: u64,
    pub(crate) post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action:
        QuickbarItemRefreshEventBreakdown,
    pub(crate) post_committed_quickbar_item_refresh_first_candidate_use_count_row:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) post_committed_quickbar_item_refresh_followup_events_before_first_client_action: u64,
    pub(crate) post_committed_quickbar_item_refresh_proof_class:
        Option<QuickbarItemRefreshProofClass>,
    pub(crate) post_committed_quickbar_item_refresh_first_followup_event:
        Option<QuickbarItemRefreshEventKind>,
    pub(crate) post_committed_quickbar_item_refresh_first_client_action:
        Option<QuickbarItemRefreshEventKind>,
    pub(crate) post_committed_quickbar_item_refresh_first_client_action_detail:
        Option<QuickbarItemRefreshClientActionDetail>,
    pub(crate) post_committed_quickbar_item_refresh_first_event_after_client_action:
        Option<QuickbarItemRefreshEventKind>,
    pub(crate) last_committed_quickbar_previous_post_item_context:
        Option<InventoryItemContextSummary>,
    pub(crate) last_committed_quickbar_previous_post_item_context_updates: u64,
    pub(crate) last_committed_quickbar_item_refresh_pending: bool,
    pub(crate) last_committed_quickbar_item_refresh_pending_updates: u64,
    pub(crate) last_committed_quickbar_item_refresh_pending_events: u64,
    pub(crate) last_committed_quickbar_item_refresh_pending_event_breakdown:
        QuickbarItemRefreshEventBreakdown,
    pub(crate) last_committed_quickbar_item_refresh_events_after_first_client_action: u64,
    pub(crate) last_committed_quickbar_item_refresh_event_breakdown_after_first_client_action:
        QuickbarItemRefreshEventBreakdown,
    pub(crate) last_committed_quickbar_item_refresh_first_candidate_use_count_row:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) last_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) last_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action:
        Option<QuickbarItemRefreshUseCountRow>,
    pub(crate) last_committed_quickbar_item_refresh_followup_events_before_first_client_action: u64,
    pub(crate) last_committed_quickbar_item_refresh_outcome: QuickbarItemRefreshOutcome,
    pub(crate) last_committed_quickbar_item_refresh_action_outcome:
        QuickbarItemRefreshActionOutcome,
    pub(crate) last_committed_quickbar_item_refresh_proof_class:
        Option<QuickbarItemRefreshProofClass>,
    pub(crate) last_committed_quickbar_item_refresh_first_followup_event:
        Option<QuickbarItemRefreshEventKind>,
    pub(crate) last_committed_quickbar_item_refresh_first_client_action:
        Option<QuickbarItemRefreshEventKind>,
    pub(crate) last_committed_quickbar_item_refresh_first_client_action_detail:
        Option<QuickbarItemRefreshClientActionDetail>,
    pub(crate) last_committed_quickbar_item_refresh_first_event_after_client_action:
        Option<QuickbarItemRefreshEventKind>,
    pub(crate) last_committed_quickbar_best_item_context: Option<InventoryItemContextSummary>,
    pub(crate) last_committed_quickbar_best_item_context_source: Option<QuickbarItemContextSource>,
}

impl UiState {
    pub(crate) fn inventory_equipment_handoff_bridge_plan(
        &self,
    ) -> InventoryEquipmentHandoffBridgePlan {
        self.last_inventory_equipment_handoff
            .map(InventoryEquipmentHandoffBridgePlan::from_ready_snapshot)
            .unwrap_or_default()
    }

    pub(crate) fn observe_inventory_equipment_handoff(
        &mut self,
        consumer: InventoryEquipmentHandoffConsumer,
        item_context: InventoryItemContextSummary,
        server_inventory_claim: Option<InventoryEquipmentServerInventoryClaim>,
        client_gui_inventory_claim: Option<InventoryEquipmentClientGuiInventoryClaim>,
    ) -> bool {
        self.inventory_equipment_handoff_events =
            self.inventory_equipment_handoff_events.saturating_add(1);
        match consumer {
            InventoryEquipmentHandoffConsumer::ServerInventory => {
                self.inventory_equipment_handoff_server_inventory_events = self
                    .inventory_equipment_handoff_server_inventory_events
                    .saturating_add(1);
            }
            InventoryEquipmentHandoffConsumer::ClientGuiInventory => {
                self.inventory_equipment_handoff_client_gui_inventory_events = self
                    .inventory_equipment_handoff_client_gui_inventory_events
                    .saturating_add(1);
            }
            InventoryEquipmentHandoffConsumer::Unknown => {}
        }

        if !item_context.inventory_equipment_handoff_ready() {
            if consumer == InventoryEquipmentHandoffConsumer::ServerInventory {
                self.pending_server_inventory_handoff_claim = server_inventory_claim;
            }
            self.inventory_equipment_handoff_blocked_without_ready_state_events = self
                .inventory_equipment_handoff_blocked_without_ready_state_events
                .saturating_add(1);
            match consumer {
                InventoryEquipmentHandoffConsumer::ServerInventory => {
                    self.inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events = self
                        .inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events
                        .saturating_add(1);
                }
                InventoryEquipmentHandoffConsumer::ClientGuiInventory => {
                    self.inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events = self
                        .inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events
                        .saturating_add(1);
                }
                InventoryEquipmentHandoffConsumer::Unknown => {}
            }
            return false;
        }

        if consumer == InventoryEquipmentHandoffConsumer::ServerInventory {
            self.pending_server_inventory_handoff_claim = None;
        }

        self.inventory_equipment_handoff_ready_events = self
            .inventory_equipment_handoff_ready_events
            .saturating_add(1);
        match consumer {
            InventoryEquipmentHandoffConsumer::ServerInventory => {
                self.inventory_equipment_handoff_server_inventory_ready_events = self
                    .inventory_equipment_handoff_server_inventory_ready_events
                    .saturating_add(1);
            }
            InventoryEquipmentHandoffConsumer::ClientGuiInventory => {
                self.inventory_equipment_handoff_client_gui_inventory_ready_events = self
                    .inventory_equipment_handoff_client_gui_inventory_ready_events
                    .saturating_add(1);
            }
            InventoryEquipmentHandoffConsumer::Unknown => {}
        }
        if item_context.has_deferred_feature25_refs() {
            self.inventory_equipment_handoff_ready_with_deferred_feature25_events = self
                .inventory_equipment_handoff_ready_with_deferred_feature25_events
                .saturating_add(1);
        }
        let snapshot = InventoryEquipmentHandoffSnapshot {
            consumer,
            item_context,
            event_index: self.inventory_equipment_handoff_events,
            server_inventory_claim,
            client_gui_inventory_claim,
        };
        self.last_inventory_equipment_handoff = Some(snapshot);
        if let Some(emission) = self.record_inventory_equipment_bridge_handoff_emission(snapshot) {
            self.drain_inventory_equipment_bridge_handoff_emission(emission);
        }
        true
    }

    pub(crate) fn consume_pending_server_inventory_handoff_if_ready(
        &mut self,
        item_context: InventoryItemContextSummary,
    ) -> bool {
        if !item_context.inventory_equipment_handoff_ready() {
            return false;
        }
        let Some(claim) = self.pending_server_inventory_handoff_claim else {
            return false;
        };
        self.pending_server_inventory_handoff_claim = None;
        self.observe_inventory_equipment_handoff(
            InventoryEquipmentHandoffConsumer::ServerInventory,
            item_context,
            Some(claim),
            None,
        )
    }

    pub(crate) fn record_inventory_equipment_bridge_handoff_emission(
        &mut self,
        snapshot: InventoryEquipmentHandoffSnapshot,
    ) -> Option<InventoryEquipmentHandoffBridgeEmission> {
        let plan = InventoryEquipmentHandoffBridgePlan::from_ready_snapshot(snapshot);
        if !plan.ready_to_emit() {
            return None;
        }
        if self
            .last_inventory_equipment_bridge_handoff_emission
            .is_some_and(|emission| emission.plan.event_index == plan.event_index)
        {
            return None;
        }

        self.inventory_equipment_bridge_handoff_emissions = self
            .inventory_equipment_bridge_handoff_emissions
            .saturating_add(1);
        let emission = InventoryEquipmentHandoffBridgeEmission {
            emission_index: self.inventory_equipment_bridge_handoff_emissions,
            plan,
        };
        self.last_inventory_equipment_bridge_handoff_emission = Some(emission);
        Some(emission)
    }

    pub(crate) fn drain_inventory_equipment_bridge_handoff_emission(
        &mut self,
        emission: InventoryEquipmentHandoffBridgeEmission,
    ) -> Option<InventoryEquipmentBridgeStateUpdate> {
        if !emission.plan.ready_to_emit() {
            return None;
        }
        let candidate = emission.plan.candidate?;
        if self
            .last_inventory_equipment_bridge_handoff_state_update
            .is_some_and(|update| update.emission_index == emission.emission_index)
        {
            return None;
        }

        self.inventory_equipment_bridge_handoff_state_updates = self
            .inventory_equipment_bridge_handoff_state_updates
            .saturating_add(1);
        let update = InventoryEquipmentBridgeStateUpdate {
            update_index: self.inventory_equipment_bridge_handoff_state_updates,
            emission_index: emission.emission_index,
            consumer: emission.plan.consumer,
            event_index: emission.plan.event_index,
            candidate,
            ready_objects: emission.plan.ready_objects,
            deferred_feature25_only_objects: emission.plan.deferred_feature25_only_objects,
            server_inventory_claim: emission.plan.server_inventory_claim,
            client_gui_inventory_claim: emission.plan.client_gui_inventory_claim,
        };
        self.last_inventory_equipment_bridge_handoff_state_update = Some(update);
        Some(update)
    }

    pub(crate) fn observe_quickbar_item_use_count_updates(
        &mut self,
        updates: &[LiveObjectQuickbarItemUseCountUpdate],
    ) {
        for update in updates.iter().copied() {
            let row = QuickbarItemRefreshUseCountRow::from(update);
            self.quickbar_item_use_count_updates_observed = self
                .quickbar_item_use_count_updates_observed
                .saturating_add(1);
            self.last_quickbar_item_use_count_state = Some(row);
            self.quickbar_item_use_count_state
                .insert(row.state_key(), row);
        }
    }

    fn quickbar_item_use_count_state_for_candidate(
        &self,
        candidate_object_id: u32,
        preserved_slot: Option<u8>,
    ) -> Option<QuickbarItemRefreshUseCountRow> {
        if let Some(preserved_slot) = preserved_slot {
            if let Some(row) = self
                .quickbar_item_use_count_state
                .values()
                .copied()
                .find(|row| row.object_id == candidate_object_id && row.slot == preserved_slot)
            {
                return Some(row);
            }
        }
        self.quickbar_item_use_count_state
            .values()
            .copied()
            .find(|row| row.object_id == candidate_object_id)
    }

    fn quickbar_item_use_count_state_for_preserved_active_item(
        &self,
        signature: Option<QuickbarActiveItemSignature>,
        preserved_slot: Option<u8>,
    ) -> Option<QuickbarItemRefreshUseCountRow> {
        let signature = signature?;
        let preserved_slot = preserved_slot?;
        self.quickbar_item_use_count_state
            .values()
            .copied()
            .find(|row| {
                row.object_id == signature.object_id
                    && row.slot == preserved_slot
                    && row.button_type == client_quickbar::ITEM_SET_BUTTON_TYPE
            })
    }

    pub(crate) fn preserved_active_items_without_use_count_state(
        &self,
    ) -> Vec<(u8, QuickbarActiveItemSignature)> {
        let Some(probe) = self.last_quickbar_stream_probe else {
            return Vec::new();
        };
        let coverage =
            probe.preserved_active_item_use_count_coverage(&self.quickbar_item_use_count_state);
        probe
            .preserved_active_item_signatures
            .0
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(slot, signature)| {
                let signature = signature?;
                let slot = u8::try_from(slot).ok()?;
                coverage
                    .missing_use_count_slot(slot)
                    .then_some((slot, signature))
            })
            .collect()
    }

    pub(crate) fn observe_quickbar_stream_probe(
        &mut self,
        summary: &QuickbarRewriteSummary,
        materialization_context: InventoryItemContextSummary,
    ) {
        self.quickbar_stream_probe_summaries =
            self.quickbar_stream_probe_summaries.saturating_add(1);
        self.last_quickbar_stream_probe =
            Some(QuickbarStreamProbeSummary::from_rewrite_summary(summary));
        self.last_quickbar_stream_probe_materialization_context = Some(materialization_context);
    }

    pub(crate) fn promote_quickbar_stream_probe_profile(
        &mut self,
        summary: &QuickbarRewriteSummary,
        materialization_context: InventoryItemContextSummary,
    ) -> bool {
        if crate::translate::quickbar::rewrite_summary_needs_more_quickbar_bytes(summary) {
            return false;
        }
        let Some(profile) = summary.validated_slot_profile else {
            return false;
        };
        self.commit_quickbar_profile(profile, materialization_context);
        true
    }

    pub(crate) fn commit_quickbar_profile(
        &mut self,
        profile: QuickbarValidatedSlotProfile,
        materialization_context: InventoryItemContextSummary,
    ) {
        self.quickbar_packets = self.quickbar_packets.saturating_add(1);
        self.last_quickbar_family = Some(VerifiedFamily::GuiQuickbar);

        let prior_item_context = self.last_inventory_item_context_before_quickbar;
        let previous_post_item_context = self.last_inventory_item_context_after_committed_quickbar;
        let previous_post_item_context_updates =
            self.inventory_item_context_after_committed_quickbar_updates;
        let pending_item_refresh = self.post_committed_quickbar_item_refresh_pending;
        let pending_item_refresh_updates =
            self.post_committed_quickbar_item_refresh_pending_updates;
        let pending_item_refresh_events = self.post_committed_quickbar_item_refresh_pending_events;
        let pending_item_refresh_event_breakdown =
            self.post_committed_quickbar_item_refresh_pending_event_breakdown;
        let pending_item_refresh_events_after_first_client_action =
            self.post_committed_quickbar_item_refresh_events_after_first_client_action;
        let pending_item_refresh_event_breakdown_after_first_client_action =
            self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action;
        let pending_item_refresh_first_candidate_use_count_row =
            self.post_committed_quickbar_item_refresh_first_candidate_use_count_row;
        let pending_item_refresh_first_candidate_use_count_row_before_first_client_action = self
            .post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action;
        let pending_item_refresh_first_candidate_use_count_row_after_first_client_action = self
            .post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action;
        let pending_item_refresh_followup_events_before_first_client_action =
            self.post_committed_quickbar_item_refresh_followup_events_before_first_client_action;
        let pending_item_refresh_proof_class =
            self.post_committed_quickbar_item_refresh_proof_class;
        let pending_item_refresh_first_followup_event =
            self.post_committed_quickbar_item_refresh_first_followup_event;
        let pending_item_refresh_first_client_action =
            self.post_committed_quickbar_item_refresh_first_client_action;
        let pending_item_refresh_first_client_action_detail =
            self.post_committed_quickbar_item_refresh_first_client_action_detail;
        let pending_item_refresh_first_event_after_client_action =
            self.post_committed_quickbar_item_refresh_first_event_after_client_action;
        let pending_item_refresh_action_outcome_breakdown =
            if pending_item_refresh && pending_item_refresh_first_client_action_detail.is_some() {
                let mut breakdown = pending_item_refresh_event_breakdown_after_first_client_action;
                breakdown.quickbar_events = breakdown.quickbar_events.saturating_add(1);
                breakdown
            } else {
                pending_item_refresh_event_breakdown_after_first_client_action
            };
        let pending_item_refresh_event_breakdown_before_first_client_action = self
            .post_committed_quickbar_item_refresh_pending_event_breakdown
            .saturating_sub(
                self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
            );
        let pending_item_refresh_action_outcome =
            QuickbarItemRefreshActionOutcome::from_pending_state(
                pending_item_refresh_first_client_action_detail,
                pending_item_refresh_event_breakdown_before_first_client_action,
                pending_item_refresh_action_outcome_breakdown,
            );
        let pending_item_refresh_outcome =
            quickbar_item_refresh_outcome_for_profile(pending_item_refresh, &profile);
        let (best_item_context, best_item_context_source) = best_quickbar_item_context_for_commit(
            materialization_context,
            prior_item_context,
            previous_post_item_context,
        );

        self.last_committed_quickbar_profile = Some(profile);
        self.last_committed_quickbar_materialization_context = Some(materialization_context);
        self.last_committed_quickbar_prior_item_context = prior_item_context;
        self.last_committed_quickbar_previous_post_item_context = previous_post_item_context;
        self.last_committed_quickbar_previous_post_item_context_updates =
            previous_post_item_context_updates;
        self.last_committed_quickbar_item_refresh_pending = pending_item_refresh;
        self.last_committed_quickbar_item_refresh_pending_updates = pending_item_refresh_updates;
        self.last_committed_quickbar_item_refresh_pending_events = pending_item_refresh_events;
        self.last_committed_quickbar_item_refresh_pending_event_breakdown =
            pending_item_refresh_event_breakdown;
        self.last_committed_quickbar_item_refresh_events_after_first_client_action =
            pending_item_refresh_events_after_first_client_action;
        self.last_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
            pending_item_refresh_event_breakdown_after_first_client_action;
        self.last_committed_quickbar_item_refresh_first_candidate_use_count_row =
            pending_item_refresh_first_candidate_use_count_row;
        self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action =
            pending_item_refresh_first_candidate_use_count_row_before_first_client_action;
        self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action =
            pending_item_refresh_first_candidate_use_count_row_after_first_client_action;
        self.last_committed_quickbar_item_refresh_followup_events_before_first_client_action =
            pending_item_refresh_followup_events_before_first_client_action;
        self.last_committed_quickbar_item_refresh_outcome = pending_item_refresh_outcome;
        self.last_committed_quickbar_item_refresh_action_outcome =
            pending_item_refresh_action_outcome;
        self.last_committed_quickbar_item_refresh_proof_class = pending_item_refresh_proof_class;
        self.last_committed_quickbar_item_refresh_first_followup_event =
            pending_item_refresh_first_followup_event;
        self.last_committed_quickbar_item_refresh_first_client_action =
            pending_item_refresh_first_client_action;
        self.last_committed_quickbar_item_refresh_first_client_action_detail =
            pending_item_refresh_first_client_action_detail;
        self.last_committed_quickbar_item_refresh_first_event_after_client_action =
            pending_item_refresh_first_event_after_client_action;
        self.last_committed_quickbar_best_item_context = best_item_context;
        self.last_committed_quickbar_best_item_context_source = best_item_context_source;

        self.last_inventory_item_context_after_committed_quickbar = None;
        self.inventory_item_context_after_committed_quickbar_updates = 0;
        self.post_committed_quickbar_item_refresh_pending = false;
        self.post_committed_quickbar_item_refresh_resolved_by_server_use_count = false;
        self.post_committed_quickbar_item_refresh_resolved_by_prior_use_count_state = false;
        self.post_committed_quickbar_item_refresh_pending_updates = 0;
        self.post_committed_quickbar_item_refresh_observed_actionable_missing_use_count_slot_mask =
            0;
        self.post_committed_quickbar_item_refresh_pending_events = 0;
        self.post_committed_quickbar_item_refresh_pending_event_breakdown =
            QuickbarItemRefreshEventBreakdown::default();
        self.post_committed_quickbar_item_refresh_events_after_first_client_action = 0;
        self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
            QuickbarItemRefreshEventBreakdown::default();
        self.post_committed_quickbar_item_refresh_first_candidate_use_count_row = None;
        self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action = None;
        self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action = None;
        self.post_committed_quickbar_item_refresh_followup_events_before_first_client_action = 0;
        self.post_committed_quickbar_item_refresh_proof_class = None;
        self.post_committed_quickbar_item_refresh_first_followup_event = None;
        self.post_committed_quickbar_item_refresh_first_client_action = None;
        self.post_committed_quickbar_item_refresh_first_client_action_detail = None;
        self.post_committed_quickbar_item_refresh_first_event_after_client_action = None;
    }

    fn finish_pending_quickbar_item_refresh_resolution(
        &mut self,
        item_context: InventoryItemContextSummary,
        outcome: QuickbarItemRefreshOutcome,
        resolved_by_server_use_count: bool,
        resolved_by_prior_use_count_state: bool,
    ) {
        let pending_item_refresh_event_breakdown =
            self.post_committed_quickbar_item_refresh_pending_event_breakdown;
        let pending_item_refresh_event_breakdown_after_first_client_action =
            self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action;
        let pending_item_refresh_event_breakdown_before_first_client_action =
            pending_item_refresh_event_breakdown
                .saturating_sub(pending_item_refresh_event_breakdown_after_first_client_action);
        let pending_item_refresh_action_outcome =
            QuickbarItemRefreshActionOutcome::from_pending_state(
                self.post_committed_quickbar_item_refresh_first_client_action_detail,
                pending_item_refresh_event_breakdown_before_first_client_action,
                pending_item_refresh_event_breakdown_after_first_client_action,
            );

        self.last_committed_quickbar_previous_post_item_context = Some(item_context);
        self.last_committed_quickbar_previous_post_item_context_updates =
            self.inventory_item_context_after_committed_quickbar_updates;
        self.last_committed_quickbar_item_refresh_pending = true;
        self.last_committed_quickbar_item_refresh_pending_updates =
            self.post_committed_quickbar_item_refresh_pending_updates;
        self.last_committed_quickbar_item_refresh_pending_events =
            self.post_committed_quickbar_item_refresh_pending_events;
        self.last_committed_quickbar_item_refresh_pending_event_breakdown =
            pending_item_refresh_event_breakdown;
        self.last_committed_quickbar_item_refresh_events_after_first_client_action =
            self.post_committed_quickbar_item_refresh_events_after_first_client_action;
        self.last_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
            pending_item_refresh_event_breakdown_after_first_client_action;
        self.last_committed_quickbar_item_refresh_first_candidate_use_count_row =
            self.post_committed_quickbar_item_refresh_first_candidate_use_count_row;
        self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action =
            self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action;
        self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action =
            self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action;
        self.last_committed_quickbar_item_refresh_followup_events_before_first_client_action =
            self.post_committed_quickbar_item_refresh_followup_events_before_first_client_action;
        self.last_committed_quickbar_item_refresh_outcome = outcome;
        self.last_committed_quickbar_item_refresh_action_outcome =
            pending_item_refresh_action_outcome;
        self.last_committed_quickbar_item_refresh_proof_class =
            self.post_committed_quickbar_item_refresh_proof_class;
        self.last_committed_quickbar_item_refresh_first_followup_event =
            self.post_committed_quickbar_item_refresh_first_followup_event;
        self.last_committed_quickbar_item_refresh_first_client_action =
            self.post_committed_quickbar_item_refresh_first_client_action;
        self.last_committed_quickbar_item_refresh_first_client_action_detail =
            self.post_committed_quickbar_item_refresh_first_client_action_detail;
        self.last_committed_quickbar_item_refresh_first_event_after_client_action =
            self.post_committed_quickbar_item_refresh_first_event_after_client_action;
        self.last_committed_quickbar_best_item_context = Some(item_context);
        self.last_committed_quickbar_best_item_context_source =
            Some(QuickbarItemContextSource::Current);

        self.post_committed_quickbar_item_refresh_pending = false;
        self.post_committed_quickbar_item_refresh_resolved_by_server_use_count =
            resolved_by_server_use_count;
        self.post_committed_quickbar_item_refresh_resolved_by_prior_use_count_state =
            resolved_by_prior_use_count_state;
        self.post_committed_quickbar_item_refresh_pending_updates = 0;
        self.post_committed_quickbar_item_refresh_pending_events = 0;
        self.post_committed_quickbar_item_refresh_pending_event_breakdown =
            QuickbarItemRefreshEventBreakdown::default();
        self.post_committed_quickbar_item_refresh_events_after_first_client_action = 0;
        self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
            QuickbarItemRefreshEventBreakdown::default();
        self.post_committed_quickbar_item_refresh_first_candidate_use_count_row = None;
        self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action = None;
        self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action = None;
        self.post_committed_quickbar_item_refresh_followup_events_before_first_client_action = 0;
        self.post_committed_quickbar_item_refresh_proof_class = None;
        self.post_committed_quickbar_item_refresh_first_followup_event = None;
        self.post_committed_quickbar_item_refresh_first_client_action = None;
        self.post_committed_quickbar_item_refresh_first_client_action_detail = None;
        self.post_committed_quickbar_item_refresh_first_event_after_client_action = None;
    }

    pub(crate) fn resolve_pending_quickbar_item_refresh_with_server_use_count(&mut self) -> bool {
        if !self.post_committed_quickbar_item_refresh_pending {
            return false;
        }
        if self
            .post_committed_quickbar_item_refresh_first_candidate_use_count_row
            .is_none()
        {
            return false;
        }
        let Some(item_context) = self.last_inventory_item_context_after_committed_quickbar else {
            return false;
        };

        self.finish_pending_quickbar_item_refresh_resolution(
            item_context,
            QuickbarItemRefreshOutcome::PendingRefreshObservedUseCountRows,
            true,
            false,
        );
        true
    }

    pub(crate) fn resolve_pending_quickbar_item_refresh_with_prior_use_count_state(
        &mut self,
    ) -> Option<QuickbarItemRefreshUseCountRow> {
        if !self.post_committed_quickbar_item_refresh_pending {
            return None;
        }
        if self
            .post_committed_quickbar_item_refresh_first_candidate_use_count_row
            .is_some()
        {
            return None;
        }
        let item_context = self.last_inventory_item_context_after_committed_quickbar?;
        let candidate = item_context.compact_item_emission_ready_candidate?;
        let first_active_item = self
            .last_quickbar_stream_probe
            .and_then(|probe| probe.first_preserved_active_item_signature)?;
        if first_active_item.object_id != candidate.object_id {
            return None;
        }
        let first_active_item_slot = self
            .last_quickbar_stream_probe
            .and_then(|probe| probe.first_preserved_active_item_slot)?;
        let row = self.quickbar_item_use_count_state_for_candidate(
            candidate.object_id,
            Some(first_active_item_slot),
        )?;
        if row.slot != first_active_item_slot
            || row.button_type != client_quickbar::ITEM_SET_BUTTON_TYPE
        {
            return None;
        }

        self.finish_pending_quickbar_item_refresh_resolution(
            item_context,
            QuickbarItemRefreshOutcome::PendingRefreshResolvedByUseCountState,
            false,
            true,
        );
        Some(row)
    }

    fn post_committed_quickbar_item_refresh_resolution(&self) -> &'static str {
        if self.post_committed_quickbar_item_refresh_pending {
            "pending"
        } else if self.post_committed_quickbar_item_refresh_resolved_by_server_use_count {
            "resolved_by_server_quickbar_use_count"
        } else if self.post_committed_quickbar_item_refresh_resolved_by_prior_use_count_state {
            "resolved_by_prior_quickbar_use_count_state"
        } else {
            "none"
        }
    }

    pub(crate) fn quickbar_item_refresh_harness_idle_reason(&self) -> &'static str {
        if self.last_committed_quickbar_profile.is_none() {
            if let Some(probe) = self.last_quickbar_stream_probe {
                if probe.item_buttons_seen != 0 {
                    return "stream_probe_quickbar_item_candidates_without_committed_profile";
                }
                return "stream_probe_quickbar_without_committed_profile";
            }
            return "no_committed_quickbar_profile";
        }

        let Some(context) = self.last_inventory_item_context_after_committed_quickbar else {
            return "no_post_committed_item_context";
        };

        if self.post_committed_quickbar_item_refresh_pending {
            if context.compact_item_emission_ready_candidate.is_none() {
                return "pending_refresh_without_candidate";
            }
            return "pending_refresh_hint_unavailable";
        }

        if self.post_committed_quickbar_item_refresh_resolved_by_server_use_count {
            return "post_context_resolved_by_server_quickbar_use_count";
        }

        if self.post_committed_quickbar_item_refresh_resolved_by_prior_use_count_state {
            return "post_context_resolved_by_prior_quickbar_use_count_state";
        }

        if context.cleared_inventory_item_object_ids != 0 {
            return "post_context_cleared_item_proof";
        }

        if context.has_quickbar_item_context_evidence() {
            return "post_context_without_compact_item_proof";
        }

        "post_context_without_item_evidence"
    }

    pub(crate) fn quickbar_item_refresh_harness_idle_json(&self) -> String {
        let context = self
            .last_inventory_item_context_after_committed_quickbar
            .unwrap_or_default();
        let candidate = context.compact_item_emission_candidate;
        let candidate_known = candidate.is_some();
        let candidate_object_id = candidate.map(|candidate| candidate.object_id).unwrap_or(0);
        let candidate_proof = candidate
            .map(|candidate| candidate.proof.as_str())
            .unwrap_or("none");
        let candidate_source = candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
        let proof_class = self
            .post_committed_quickbar_item_refresh_proof_class
            .map(QuickbarItemRefreshProofClass::as_str)
            .unwrap_or("none");
        let event_breakdown_before_first_client_action = self
            .post_committed_quickbar_item_refresh_pending_event_breakdown
            .saturating_sub(
                self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
            );
        let action_outcome = QuickbarItemRefreshActionOutcome::from_pending_state(
            self.post_committed_quickbar_item_refresh_first_client_action_detail,
            event_breakdown_before_first_client_action,
            self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
        )
        .as_str();
        let recommended_action_outcome =
            QuickbarItemRefreshRecommendedActionOutcome::from_pending_state(
                self.post_committed_quickbar_item_refresh_first_client_action_detail,
                candidate.map(|candidate| candidate.object_id),
                self.quickbar_item_refresh_set_button_slot().0,
                self.last_quickbar_stream_probe
                    .and_then(|probe| probe.first_preserved_active_item_signature),
                event_breakdown_before_first_client_action,
                self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
            )
            .as_str();
        let active_property_outcome = QuickbarItemRefreshActivePropertyOutcome::from_pending_state(
            self.post_committed_quickbar_item_refresh_first_client_action_detail,
            self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
        )
        .as_str();
        let server_quickbar_response_timing =
            QuickbarItemRefreshServerQuickbarResponseTiming::from_pending_state(
                self.post_committed_quickbar_item_refresh_first_client_action_detail,
                event_breakdown_before_first_client_action,
                self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
            )
            .as_str();
        let first_client_action_timing = QuickbarItemRefreshClientActionTiming::from_pending_state(
            self.post_committed_quickbar_item_refresh_first_client_action_detail,
            self.post_committed_quickbar_item_refresh_followup_events_before_first_client_action,
        )
        .as_str();
        let stream_probe = self.last_quickbar_stream_probe.unwrap_or_default();
        let stream_probe_preserved_active_item_signature_count =
            stream_probe.preserved_active_item_signature_count();
        let stream_probe_preserved_active_item_slots_json =
            stream_probe.preserved_active_item_slots_json();
        let stream_probe_preserved_active_item_use_count_coverage = stream_probe
            .preserved_active_item_use_count_coverage(&self.quickbar_item_use_count_state);
        let stream_probe_preserved_active_item_matching_use_count_slots_json =
            stream_probe_preserved_active_item_use_count_coverage.matching_use_count_slots_json();
        let stream_probe_preserved_active_item_missing_use_count_slots_json =
            stream_probe_preserved_active_item_use_count_coverage.missing_use_count_slots_json();
        let stream_probe_preserved_active_item_actionable_missing_use_count_slot_count =
            context.preserved_active_item_actionable_missing_use_count_slot_count();
        let stream_probe_preserved_active_item_actionable_missing_use_count_slots_json =
            context.preserved_active_item_actionable_missing_use_count_slots_json();
        let stream_probe_preserved_active_item_observed_actionable_missing_use_count_slot_count =
            self.post_committed_quickbar_item_refresh_observed_actionable_missing_use_count_slot_mask
                .count_ones();
        let stream_probe_preserved_active_item_observed_actionable_missing_use_count_slots_json =
            quickbar_slot_mask_json(
                self.post_committed_quickbar_item_refresh_observed_actionable_missing_use_count_slot_mask,
            );
        let stream_probe_context = self
            .last_quickbar_stream_probe_materialization_context
            .unwrap_or_default();
        let stream_probe_active_item = stream_probe.first_preserved_active_item_signature;
        let stream_probe_active_item_first_property =
            stream_probe_active_item.and_then(|signature| signature.first_property);
        let stream_probe_active_item_known = stream_probe_active_item.is_some();
        let stream_probe_active_item_slot_known =
            stream_probe.first_preserved_active_item_slot.is_some();
        let stream_probe_active_item_slot =
            stream_probe.first_preserved_active_item_slot.unwrap_or(0);
        let stream_probe_active_item_first_page_slot = stream_probe
            .first_preserved_active_item_slot
            .is_some_and(|slot| slot < 12);
        let stream_probe_active_item_object_id = stream_probe_active_item
            .map(|signature| signature.object_id)
            .unwrap_or(0);
        let stream_probe_active_item_base_item = stream_probe_active_item
            .map(|signature| signature.base_item)
            .unwrap_or(0);
        let stream_probe_active_item_appearance_type = stream_probe_active_item
            .map(|signature| signature.appearance_type)
            .unwrap_or(0);
        let stream_probe_active_item_property_count = stream_probe_active_item
            .map(|signature| signature.active_property_count)
            .unwrap_or(0);
        let stream_probe_active_item_first_property_known =
            stream_probe_active_item_first_property.is_some();
        let stream_probe_active_item_first_property_id = stream_probe_active_item_first_property
            .map(|property| property.property)
            .unwrap_or(0);
        let stream_probe_active_item_first_property_subtype =
            stream_probe_active_item_first_property
                .map(|property| property.subtype)
                .unwrap_or(0);
        let stream_probe_active_item_state_mask = stream_probe_active_item
            .map(|signature| signature.state_mask)
            .unwrap_or(0);
        let stream_probe_active_item_value_mask = stream_probe_active_item
            .map(|signature| signature.value_mask)
            .unwrap_or(0);
        let stream_probe_active_item_use_count_state = self
            .quickbar_item_use_count_state_for_preserved_active_item(
                stream_probe_active_item,
                stream_probe.first_preserved_active_item_slot,
            );
        let stream_probe_active_item_use_count_state_slot_relation =
            QuickbarItemRefreshUseCountSlotRelation::from_row_and_preserved_slot(
                stream_probe_active_item_use_count_state,
                stream_probe.first_preserved_active_item_slot,
            );
        let stream_probe_active_item_use_count_state_known =
            stream_probe_active_item_use_count_state.is_some();
        let stream_probe_active_item_use_count_state =
            stream_probe_active_item_use_count_state.unwrap_or_default();
        let first_candidate_use_count_row = self
            .post_committed_quickbar_item_refresh_first_candidate_use_count_row
            .or(self.last_committed_quickbar_item_refresh_first_candidate_use_count_row);
        let first_candidate_use_count_row_before_first_client_action = self
            .post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action
            .or(self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action);
        let first_candidate_use_count_row_after_first_client_action = self
            .post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action
            .or(self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action);
        let first_candidate_use_count_row_timing = quickbar_item_refresh_use_count_row_timing(
            first_candidate_use_count_row,
            first_candidate_use_count_row_before_first_client_action,
            first_candidate_use_count_row_after_first_client_action,
        );
        let first_candidate_use_count_row_slot_relation =
            QuickbarItemRefreshUseCountSlotRelation::from_row_and_preserved_slot(
                first_candidate_use_count_row,
                stream_probe.first_preserved_active_item_slot,
            );
        let candidate_use_count_state = candidate.and_then(|candidate| {
            self.quickbar_item_use_count_state_for_candidate(
                candidate.object_id,
                stream_probe.first_preserved_active_item_slot,
            )
        });
        let candidate_use_count_state_slot_relation =
            QuickbarItemRefreshUseCountSlotRelation::from_row_and_preserved_slot(
                candidate_use_count_state,
                stream_probe.first_preserved_active_item_slot,
            );
        let candidate_use_count_state_known = candidate_use_count_state.is_some();
        let candidate_use_count_state = candidate_use_count_state.unwrap_or_default();
        let first_candidate_use_count_row = first_candidate_use_count_row.unwrap_or_default();
        let first_candidate_use_count_row_before_first_client_action =
            first_candidate_use_count_row_before_first_client_action.unwrap_or_default();
        let first_candidate_use_count_row_after_first_client_action =
            first_candidate_use_count_row_after_first_client_action.unwrap_or_default();
        let last_inventory_equipment_handoff = self.last_inventory_equipment_handoff;
        let last_inventory_equipment_handoff_known = last_inventory_equipment_handoff.is_some();
        let last_inventory_equipment_handoff_consumer = last_inventory_equipment_handoff
            .map(|snapshot| snapshot.consumer.as_str())
            .unwrap_or("none");
        let last_inventory_equipment_handoff_event_index = last_inventory_equipment_handoff
            .map(|snapshot| snapshot.event_index)
            .unwrap_or(0);
        let last_inventory_equipment_handoff_context = last_inventory_equipment_handoff
            .map(|snapshot| snapshot.item_context)
            .unwrap_or_default();
        let last_inventory_equipment_handoff_candidate =
            last_inventory_equipment_handoff_context.compact_item_emission_ready_candidate;
        let last_inventory_equipment_handoff_candidate_known =
            last_inventory_equipment_handoff_candidate.is_some();
        let last_inventory_equipment_handoff_candidate_object_id =
            last_inventory_equipment_handoff_candidate
                .map(|candidate| candidate.object_id)
                .unwrap_or(0);
        let last_inventory_equipment_handoff_candidate_proof =
            last_inventory_equipment_handoff_candidate
                .map(|candidate| candidate.proof.as_str())
                .unwrap_or("none");
        let last_inventory_equipment_handoff_candidate_source =
            last_inventory_equipment_handoff_candidate
                .map(|candidate| candidate.source.as_str())
                .unwrap_or("none");
        let bridge_plan = self.inventory_equipment_handoff_bridge_plan();
        let bridge_candidate = bridge_plan.candidate;
        let bridge_candidate_known = bridge_candidate.is_some();
        let bridge_candidate_object_id = bridge_candidate
            .map(|candidate| candidate.object_id)
            .unwrap_or(0);
        let bridge_candidate_proof = bridge_candidate
            .map(|candidate| candidate.proof.as_str())
            .unwrap_or("none");
        let bridge_candidate_source = bridge_candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
        let last_bridge_emission = self.last_inventory_equipment_bridge_handoff_emission;
        let last_bridge_emission_known = last_bridge_emission.is_some();
        let last_bridge_emission_index = last_bridge_emission
            .map(|emission| emission.emission_index)
            .unwrap_or(0);
        let last_bridge_emission_plan = last_bridge_emission
            .map(|emission| emission.plan)
            .unwrap_or_default();
        let last_bridge_emission_candidate = last_bridge_emission_plan.candidate;
        let last_bridge_emission_candidate_object_id = last_bridge_emission_candidate
            .map(|candidate| candidate.object_id)
            .unwrap_or(0);
        let last_bridge_emission_candidate_source = last_bridge_emission_candidate
            .map(|candidate| candidate.source.as_str())
            .unwrap_or("none");
        let last_bridge_state_update = self.last_inventory_equipment_bridge_handoff_state_update;
        let last_bridge_state_update_known = last_bridge_state_update.is_some();
        let last_bridge_state_update_index = last_bridge_state_update
            .map(|update| update.update_index)
            .unwrap_or(0);
        let last_bridge_state_update_emission_index = last_bridge_state_update
            .map(|update| update.emission_index)
            .unwrap_or(0);
        let last_bridge_state_update_consumer = last_bridge_state_update
            .map(|update| update.consumer.as_str())
            .unwrap_or("unknown");
        let last_bridge_state_update_event_index = last_bridge_state_update
            .map(|update| update.event_index)
            .unwrap_or(0);
        let last_bridge_state_update_candidate_object_id = last_bridge_state_update
            .map(|update| update.candidate.object_id)
            .unwrap_or(0);
        let last_bridge_state_update_candidate_proof = last_bridge_state_update
            .map(|update| update.candidate.proof.as_str())
            .unwrap_or("none");
        let last_bridge_state_update_candidate_source = last_bridge_state_update
            .map(|update| update.candidate.source.as_str())
            .unwrap_or("none");
        let last_bridge_state_update_ready_objects = last_bridge_state_update
            .map(|update| update.ready_objects)
            .unwrap_or(0);
        let last_bridge_state_update_deferred_feature25_only_objects = last_bridge_state_update
            .map(|update| update.deferred_feature25_only_objects)
            .unwrap_or(0);
        format!(
            concat!(
                "{{\n",
                "  \"kind\": \"quickbar_item_refresh_candidate\",\n",
                "  \"pending_item_refresh\": false,\n",
                "  \"no_hint_reason\": \"{}\",\n",
                "  \"committed_quickbar_seen\": {},\n",
                "  \"stream_probe_quickbar_seen\": {},\n",
                "  \"stream_probe_quickbar_summaries\": {},\n",
                "  \"stream_probe_slot_records_owned\": {},\n",
                "  \"stream_probe_item_buttons_seen\": {},\n",
                "  \"stream_probe_item_buttons_source_compact\": {},\n",
                "  \"stream_probe_item_buttons_preserved\": {},\n",
                "  \"stream_probe_item_buttons_blanked\": {},\n",
                "  \"stream_probe_item_buttons_blanked_candidate\": {},\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_proof\": {},\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_unknown\": {},\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_cleared_delete\": {},\n",
                "  \"stream_probe_item_buttons_rejected_missing_state_cleared_area_reset\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_proven\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_active\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_feature25_first\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_feature25_second\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_feature25_legacy_tail\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_unknown\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_cleared_delete\": {},\n",
                "  \"stream_probe_item_objects_rejected_missing_state_cleared_area_reset\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_explicit_self_materialization\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_active_state\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_feature25_first\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_feature25_second\": {},\n",
                "  \"stream_probe_item_objects_preserved_by_feature25_legacy_tail\": {},\n",
                "  \"stream_probe_preserved_active_item_signature_count\": {},\n",
                "  \"stream_probe_preserved_active_item_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_matching_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_matching_use_count_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_missing_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_missing_use_count_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_actionable_missing_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_actionable_missing_use_count_slots\": {},\n",
                "  \"stream_probe_preserved_active_item_observed_actionable_missing_use_count_count\": {},\n",
                "  \"stream_probe_preserved_active_item_observed_actionable_missing_use_count_slots\": {},\n",
                "  \"stream_probe_first_preserved_active_item_known\": {},\n",
                "  \"stream_probe_first_preserved_active_item_slot_known\": {},\n",
                "  \"stream_probe_first_preserved_active_item_slot\": {},\n",
                "  \"stream_probe_first_preserved_active_item_first_page_slot\": {},\n",
                "  \"stream_probe_first_preserved_active_item_object_id\": {},\n",
                "  \"stream_probe_first_preserved_active_item_object_id_hex\": \"0x{:08X}\",\n",
                "  \"stream_probe_first_preserved_active_item_base_item\": {},\n",
                "  \"stream_probe_first_preserved_active_item_base_item_hex\": \"0x{:08X}\",\n",
                "  \"stream_probe_first_preserved_active_item_appearance_type\": {},\n",
                "  \"stream_probe_first_preserved_active_item_property_count\": {},\n",
                "  \"stream_probe_first_preserved_active_item_first_property_known\": {},\n",
                "  \"stream_probe_first_preserved_active_item_first_property\": {},\n",
                "  \"stream_probe_first_preserved_active_item_first_property_subtype\": {},\n",
                "  \"stream_probe_first_preserved_active_item_state_mask\": {},\n",
                "  \"stream_probe_first_preserved_active_item_state_mask_hex\": \"0x{:02X}\",\n",
                "  \"stream_probe_first_preserved_active_item_value_mask\": {},\n",
                "  \"stream_probe_first_preserved_active_item_value_mask_hex\": \"0x{:02X}\",\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_known\": {},\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_slot_relation\": \"{}\",\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_slot_matches_first_preserved_active_item\": {},\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_slot\": {},\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_button_type\": {},\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_object_id\": {},\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_object_id_hex\": \"0x{:08X}\",\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_active_property_index\": {},\n",
                "  \"stream_probe_first_preserved_active_item_quickbar_use_count_state_use_count\": {},\n",
                "  \"stream_probe_direct_item_proof_objects\": {},\n",
                "  \"stream_probe_feature25_item_proof_objects\": {},\n",
                "  \"stream_probe_compact_item_emission_proof_objects\": {},\n",
                "  \"stream_probe_compact_item_emission_ready_objects\": {},\n",
                "  \"stream_probe_compact_item_emission_deferred_feature25_only_objects\": {},\n",
                "  \"post_committed_item_context_known\": {},\n",
                "  \"post_committed_item_refresh_pending\": {},\n",
                "  \"post_committed_item_refresh_resolved_by_server_use_count\": {},\n",
                "  \"post_committed_item_refresh_resolved_by_prior_use_count_state\": {},\n",
                "  \"post_committed_item_refresh_resolution\": \"{}\",\n",
                "  \"updates_since_committed_quickbar\": {},\n",
                "  \"events_since_pending_refresh\": {},\n",
                "  \"server_to_client_events_since_pending_refresh\": {},\n",
                "  \"client_to_server_events_since_pending_refresh\": {},\n",
                "  \"pending_item_refresh_proof_class\": \"{}\",\n",
                "  \"pending_item_refresh_action_outcome\": \"{}\",\n",
                "  \"pending_item_refresh_recommended_action_outcome\": \"{}\",\n",
                "  \"pending_item_refresh_active_property_outcome\": \"{}\",\n",
                "  \"pending_item_refresh_server_quickbar_response_timing\": \"{}\",\n",
                "  \"quickbar_item_use_count_state_rows\": {},\n",
                "  \"quickbar_item_use_count_updates_observed\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_known\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_slot_relation\": \"{}\",\n",
                "  \"candidate_quickbar_item_use_count_state_slot_matches_first_preserved_active_item\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_slot\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_button_type\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_object_id\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_object_id_hex\": \"0x{:08X}\",\n",
                "  \"candidate_quickbar_item_use_count_state_active_property_index\": {},\n",
                "  \"candidate_quickbar_item_use_count_state_use_count\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_known\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_timing\": \"{}\",\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_slot_relation\": \"{}\",\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_slot_matches_first_preserved_active_item\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_slot\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_button_type\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_object_id\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_object_id_hex\": \"0x{:08X}\",\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_active_property_index\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_use_count\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_known\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_slot\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_button_type\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_active_property_index\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_use_count\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_known\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_slot\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_button_type\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_active_property_index\": {},\n",
                "  \"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_use_count\": {},\n",
                "  \"first_client_action_timing\": \"{}\",\n",
                "  \"followup_events_before_first_client_action\": {},\n",
                "  \"candidate_known\": {},\n",
                "  \"candidate_object_id\": {},\n",
                "  \"candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"candidate_proof\": \"{}\",\n",
                "  \"candidate_source\": \"{}\",\n",
                "  \"direct_item_proof_objects\": {},\n",
                "  \"feature25_item_proof_objects\": {},\n",
                "  \"compact_item_emission_proof_objects\": {},\n",
                "  \"compact_item_emission_ready_objects\": {},\n",
                "  \"compact_item_emission_direct_only_proof_objects\": {},\n",
                "  \"compact_item_emission_feature25_only_proof_objects\": {},\n",
                "  \"compact_item_emission_shared_proof_objects\": {},\n",
                "  \"compact_item_emission_deferred_feature25_only_objects\": {},\n",
                "  \"inventory_feature25_reference_records\": {},\n",
                "  \"inventory_feature25_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_deferred_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_materialization_outcome\": \"{}\",\n",
                "  \"inventory_feature25_handoff_outcome\": \"{}\",\n",
                "  \"inventory_equipment_handoff_ready\": {},\n",
                "  \"inventory_equipment_handoff_outcome\": \"{}\",\n",
                "  \"inventory_equipment_handoff_events\": {},\n",
                "  \"inventory_equipment_handoff_ready_events\": {},\n",
                "  \"inventory_equipment_handoff_blocked_without_ready_state_events\": {},\n",
                "  \"inventory_equipment_handoff_ready_with_deferred_feature25_events\": {},\n",
                "  \"inventory_equipment_handoff_server_inventory_events\": {},\n",
                "  \"inventory_equipment_handoff_server_inventory_ready_events\": {},\n",
                "  \"inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events\": {},\n",
                "  \"inventory_equipment_handoff_client_gui_inventory_events\": {},\n",
                "  \"inventory_equipment_handoff_client_gui_inventory_ready_events\": {},\n",
                "  \"inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events\": {},\n",
                "  \"last_inventory_equipment_handoff_known\": {},\n",
                "  \"last_inventory_equipment_handoff_consumer\": \"{}\",\n",
                "  \"last_inventory_equipment_handoff_event_index\": {},\n",
                "  \"last_inventory_equipment_handoff_outcome\": \"{}\",\n",
                "  \"last_inventory_equipment_handoff_ready_objects\": {},\n",
                "  \"last_inventory_equipment_handoff_deferred_feature25_only_objects\": {},\n",
                "  \"last_inventory_equipment_handoff_candidate_known\": {},\n",
                "  \"last_inventory_equipment_handoff_candidate_object_id\": {},\n",
                "  \"last_inventory_equipment_handoff_candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"last_inventory_equipment_handoff_candidate_proof\": \"{}\",\n",
                "  \"last_inventory_equipment_handoff_candidate_source\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_action\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_ready\": {},\n",
                "  \"inventory_equipment_bridge_handoff_consumer\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_event_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_outcome\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_ready_objects\": {},\n",
                "  \"inventory_equipment_bridge_handoff_deferred_feature25_only_objects\": {},\n",
                "  \"inventory_equipment_bridge_handoff_candidate_known\": {},\n",
                "  \"inventory_equipment_bridge_handoff_candidate_object_id\": {},\n",
                "  \"inventory_equipment_bridge_handoff_candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"inventory_equipment_bridge_handoff_candidate_proof\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_candidate_source\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_emissions\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_known\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_consumer\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_event_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_candidate_object_id\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_emitted_candidate_source\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_state_updates\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_known\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_emission_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_consumer\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_event_index\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_object_id\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_object_id_hex\": \"0x{:08X}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_proof\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_candidate_source\": \"{}\",\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_ready_objects\": {},\n",
                "  \"inventory_equipment_bridge_handoff_last_state_update_deferred_feature25_only_objects\": {},\n",
                "  \"inventory_feature25_first_item_refs\": {},\n",
                "  \"inventory_feature25_first_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_first_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_first_deferred_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_second_item_refs\": {},\n",
                "  \"inventory_feature25_second_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_second_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_second_deferred_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_legacy_tail_item_refs\": {},\n",
                "  \"inventory_feature25_legacy_tail_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_legacy_tail_materialized_item_ref_mentions\": {},\n",
                "  \"inventory_feature25_legacy_tail_deferred_item_ref_mentions\": {},\n",
                "  \"cleared_inventory_item_object_ids\": {},\n",
                "  \"live_object_events_since_pending_refresh\": {},\n",
                "  \"quickbar_events_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_events_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_records_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_rows_since_pending_refresh\": {},\n",
                "  \"server_quickbar_item_use_count_candidate_rows_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_uses_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_full_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_uses_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_full_events_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_changed_use_count_rows_since_pending_refresh\": {},\n",
                "  \"server_active_item_property_candidate_full_property_rows_since_pending_refresh\": {},\n",
                "  \"area_events_since_pending_refresh\": {},\n",
                "  \"inventory_events_since_pending_refresh\": {},\n",
                "  \"client_gui_event_events_since_pending_refresh\": {},\n",
                "  \"client_input_events_since_pending_refresh\": {},\n",
                "  \"client_quickbar_events_since_pending_refresh\": {},\n",
                "  \"chat_events_since_pending_refresh\": {},\n",
                "  \"other_events_since_pending_refresh\": {}\n",
                "}}\n"
            ),
            self.quickbar_item_refresh_harness_idle_reason(),
            self.last_committed_quickbar_profile.is_some(),
            self.last_quickbar_stream_probe.is_some(),
            self.quickbar_stream_probe_summaries,
            stream_probe.slot_records_owned,
            stream_probe.item_buttons_seen,
            stream_probe.item_buttons_source_compact,
            stream_probe.item_buttons_preserved,
            stream_probe.item_buttons_blanked,
            stream_probe.item_buttons_blanked_candidate,
            stream_probe.item_buttons_rejected_missing_state_proof,
            stream_probe.item_buttons_rejected_missing_state_unknown,
            stream_probe.item_buttons_rejected_missing_state_cleared_delete,
            stream_probe.item_buttons_rejected_missing_state_cleared_area_reset,
            stream_probe.item_objects_rejected_missing_state_proven,
            stream_probe.item_objects_rejected_missing_state_active,
            stream_probe.item_objects_rejected_missing_state_feature25_first,
            stream_probe.item_objects_rejected_missing_state_feature25_second,
            stream_probe.item_objects_rejected_missing_state_feature25_legacy_tail,
            stream_probe.item_objects_rejected_missing_state_unknown,
            stream_probe.item_objects_rejected_missing_state_cleared_delete,
            stream_probe.item_objects_rejected_missing_state_cleared_area_reset,
            stream_probe.item_objects_preserved_by_explicit_self_materialization,
            stream_probe.item_objects_preserved_by_active_state,
            stream_probe.item_objects_preserved_by_feature25_first,
            stream_probe.item_objects_preserved_by_feature25_second,
            stream_probe.item_objects_preserved_by_feature25_legacy_tail,
            stream_probe_preserved_active_item_signature_count,
            stream_probe_preserved_active_item_slots_json,
            stream_probe_preserved_active_item_use_count_coverage
                .matching_use_count_slot_count(),
            stream_probe_preserved_active_item_matching_use_count_slots_json,
            stream_probe_preserved_active_item_use_count_coverage.missing_use_count_slot_count(),
            stream_probe_preserved_active_item_missing_use_count_slots_json,
            stream_probe_preserved_active_item_actionable_missing_use_count_slot_count,
            stream_probe_preserved_active_item_actionable_missing_use_count_slots_json,
            stream_probe_preserved_active_item_observed_actionable_missing_use_count_slot_count,
            stream_probe_preserved_active_item_observed_actionable_missing_use_count_slots_json,
            stream_probe_active_item_known,
            stream_probe_active_item_slot_known,
            stream_probe_active_item_slot,
            stream_probe_active_item_first_page_slot,
            stream_probe_active_item_object_id,
            stream_probe_active_item_object_id,
            stream_probe_active_item_base_item,
            stream_probe_active_item_base_item,
            stream_probe_active_item_appearance_type,
            stream_probe_active_item_property_count,
            stream_probe_active_item_first_property_known,
            stream_probe_active_item_first_property_id,
            stream_probe_active_item_first_property_subtype,
            stream_probe_active_item_state_mask,
            stream_probe_active_item_state_mask,
            stream_probe_active_item_value_mask,
            stream_probe_active_item_value_mask,
            stream_probe_active_item_use_count_state_known,
            stream_probe_active_item_use_count_state_slot_relation.as_str(),
            stream_probe_active_item_use_count_state_slot_relation
                .matches_preserved_active_item_slot(),
            stream_probe_active_item_use_count_state.slot,
            stream_probe_active_item_use_count_state.button_type,
            stream_probe_active_item_use_count_state.object_id,
            stream_probe_active_item_use_count_state.object_id,
            stream_probe_active_item_use_count_state.active_property_index,
            stream_probe_active_item_use_count_state.use_count,
            stream_probe_context.direct_item_proof_objects,
            stream_probe_context.feature25_item_proof_objects,
            stream_probe_context.compact_item_emission_proof_objects,
            stream_probe_context.compact_item_emission_ready_objects,
            stream_probe_context.compact_item_emission_deferred_feature25_only_objects,
            self.last_inventory_item_context_after_committed_quickbar
                .is_some(),
            self.post_committed_quickbar_item_refresh_pending,
            self.post_committed_quickbar_item_refresh_resolved_by_server_use_count,
            self.post_committed_quickbar_item_refresh_resolved_by_prior_use_count_state,
            self.post_committed_quickbar_item_refresh_resolution(),
            self.inventory_item_context_after_committed_quickbar_updates,
            self.post_committed_quickbar_item_refresh_pending_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_to_client_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_to_server_events,
            proof_class,
            action_outcome,
            recommended_action_outcome,
            active_property_outcome,
            server_quickbar_response_timing,
            self.quickbar_item_use_count_state.len(),
            self.quickbar_item_use_count_updates_observed,
            candidate_use_count_state_known,
            candidate_use_count_state_slot_relation.as_str(),
            candidate_use_count_state_slot_relation.matches_preserved_active_item_slot(),
            candidate_use_count_state.slot,
            candidate_use_count_state.button_type,
            candidate_use_count_state.object_id,
            candidate_use_count_state.object_id,
            candidate_use_count_state.active_property_index,
            candidate_use_count_state.use_count,
            self.post_committed_quickbar_item_refresh_first_candidate_use_count_row
                .or(self.last_committed_quickbar_item_refresh_first_candidate_use_count_row)
                .is_some(),
            first_candidate_use_count_row_timing,
            first_candidate_use_count_row_slot_relation.as_str(),
            first_candidate_use_count_row_slot_relation.matches_preserved_active_item_slot(),
            first_candidate_use_count_row.slot,
            first_candidate_use_count_row.button_type,
            first_candidate_use_count_row.object_id,
            first_candidate_use_count_row.object_id,
            first_candidate_use_count_row.active_property_index,
            first_candidate_use_count_row.use_count,
            self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action
                .or(self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action)
                .is_some(),
            first_candidate_use_count_row_before_first_client_action.slot,
            first_candidate_use_count_row_before_first_client_action.button_type,
            first_candidate_use_count_row_before_first_client_action.active_property_index,
            first_candidate_use_count_row_before_first_client_action.use_count,
            self.post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action
                .or(self.last_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action)
                .is_some(),
            first_candidate_use_count_row_after_first_client_action.slot,
            first_candidate_use_count_row_after_first_client_action.button_type,
            first_candidate_use_count_row_after_first_client_action.active_property_index,
            first_candidate_use_count_row_after_first_client_action.use_count,
            first_client_action_timing,
            self.post_committed_quickbar_item_refresh_followup_events_before_first_client_action,
            candidate_known,
            candidate_object_id,
            candidate_object_id,
            candidate_proof,
            candidate_source,
            context.direct_item_proof_objects,
            context.feature25_item_proof_objects,
            context.compact_item_emission_proof_objects,
            context.compact_item_emission_ready_objects,
            context.compact_item_emission_direct_only_proof_objects,
            context.compact_item_emission_feature25_only_proof_objects,
            context.compact_item_emission_shared_proof_objects,
            context.compact_item_emission_deferred_feature25_only_objects,
            context.inventory_feature25_reference_records,
            context.inventory_feature25_item_ref_mentions(),
            context.inventory_feature25_materialized_item_ref_mentions(),
            context.inventory_feature25_deferred_item_ref_mentions(),
            context.inventory_feature25_materialization_outcome().as_str(),
            context.inventory_feature25_handoff_outcome().as_str(),
            context.inventory_equipment_handoff_ready(),
            context.inventory_equipment_handoff_outcome().as_str(),
            self.inventory_equipment_handoff_events,
            self.inventory_equipment_handoff_ready_events,
            self.inventory_equipment_handoff_blocked_without_ready_state_events,
            self.inventory_equipment_handoff_ready_with_deferred_feature25_events,
            self.inventory_equipment_handoff_server_inventory_events,
            self.inventory_equipment_handoff_server_inventory_ready_events,
            self.inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events,
            self.inventory_equipment_handoff_client_gui_inventory_events,
            self.inventory_equipment_handoff_client_gui_inventory_ready_events,
            self.inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events,
            last_inventory_equipment_handoff_known,
            last_inventory_equipment_handoff_consumer,
            last_inventory_equipment_handoff_event_index,
            last_inventory_equipment_handoff_context
                .inventory_equipment_handoff_outcome()
                .as_str(),
            last_inventory_equipment_handoff_context.compact_item_emission_ready_objects,
            last_inventory_equipment_handoff_context
                .compact_item_emission_deferred_feature25_only_objects,
            last_inventory_equipment_handoff_candidate_known,
            last_inventory_equipment_handoff_candidate_object_id,
            last_inventory_equipment_handoff_candidate_object_id,
            last_inventory_equipment_handoff_candidate_proof,
            last_inventory_equipment_handoff_candidate_source,
            bridge_plan.action.as_str(),
            bridge_plan.ready_to_emit(),
            bridge_plan.consumer.as_str(),
            bridge_plan.event_index,
            bridge_plan.outcome.as_str(),
            bridge_plan.ready_objects,
            bridge_plan.deferred_feature25_only_objects,
            bridge_candidate_known,
            bridge_candidate_object_id,
            bridge_candidate_object_id,
            bridge_candidate_proof,
            bridge_candidate_source,
            self.inventory_equipment_bridge_handoff_emissions,
            last_bridge_emission_known,
            last_bridge_emission_index,
            last_bridge_emission_plan.consumer.as_str(),
            last_bridge_emission_plan.event_index,
            last_bridge_emission_candidate_object_id,
            last_bridge_emission_candidate_object_id,
            last_bridge_emission_candidate_source,
            self.inventory_equipment_bridge_handoff_state_updates,
            last_bridge_state_update_known,
            last_bridge_state_update_index,
            last_bridge_state_update_emission_index,
            last_bridge_state_update_consumer,
            last_bridge_state_update_event_index,
            last_bridge_state_update_candidate_object_id,
            last_bridge_state_update_candidate_object_id,
            last_bridge_state_update_candidate_proof,
            last_bridge_state_update_candidate_source,
            last_bridge_state_update_ready_objects,
            last_bridge_state_update_deferred_feature25_only_objects,
            context.inventory_feature25_first_item_refs,
            context.inventory_feature25_first_item_ref_mentions,
            context.inventory_feature25_first_materialized_item_ref_mentions,
            context.inventory_feature25_first_deferred_item_ref_mentions,
            context.inventory_feature25_second_item_refs,
            context.inventory_feature25_second_item_ref_mentions,
            context.inventory_feature25_second_materialized_item_ref_mentions,
            context.inventory_feature25_second_deferred_item_ref_mentions,
            context.inventory_feature25_legacy_tail_item_refs,
            context.inventory_feature25_legacy_tail_item_ref_mentions,
            context.inventory_feature25_legacy_tail_materialized_item_ref_mentions,
            context.inventory_feature25_legacy_tail_deferred_item_ref_mentions,
            context.cleared_inventory_item_object_ids,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .live_object_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .quickbar_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_quickbar_item_use_count_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_quickbar_item_use_count_records,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_quickbar_item_use_count_rows,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_quickbar_item_use_count_candidate_rows,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_uses_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_full_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_candidate_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_candidate_uses_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_candidate_full_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_candidate_changed_use_count_rows,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .server_active_item_property_candidate_full_property_rows,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .area_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .inventory_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_gui_event_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_input_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_quickbar_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .chat_events,
            self.post_committed_quickbar_item_refresh_pending_event_breakdown
                .other_events,
        )
    }

    pub(crate) fn unresolved_pending_item_refresh(
        &self,
    ) -> Option<QuickbarPendingItemRefreshSummary> {
        if !self.post_committed_quickbar_item_refresh_pending {
            return None;
        }
        Some(QuickbarPendingItemRefreshSummary {
            item_context: self.last_inventory_item_context_after_committed_quickbar?,
            updates_since_committed_quickbar: self
                .inventory_item_context_after_committed_quickbar_updates,
            events_since_pending_refresh: self.post_committed_quickbar_item_refresh_pending_events,
            event_breakdown: self.post_committed_quickbar_item_refresh_pending_event_breakdown,
            events_after_first_client_action: self
                .post_committed_quickbar_item_refresh_events_after_first_client_action,
            event_breakdown_after_first_client_action: self
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
            first_candidate_use_count_row: self
                .post_committed_quickbar_item_refresh_first_candidate_use_count_row,
            first_candidate_use_count_row_before_first_client_action: self
                .post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action,
            first_candidate_use_count_row_after_first_client_action: self
                .post_committed_quickbar_item_refresh_first_candidate_use_count_row_after_first_client_action,
            action_outcome: QuickbarItemRefreshActionOutcome::from_pending_state(
                self.post_committed_quickbar_item_refresh_first_client_action_detail,
                self.post_committed_quickbar_item_refresh_pending_event_breakdown
                    .saturating_sub(
                        self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
                    ),
                self.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
            ),
            followup_events_before_first_client_action: self
                .post_committed_quickbar_item_refresh_followup_events_before_first_client_action,
            proof_class: self.post_committed_quickbar_item_refresh_proof_class,
            first_followup_event: self.post_committed_quickbar_item_refresh_first_followup_event,
            first_client_action: self.post_committed_quickbar_item_refresh_first_client_action,
            first_client_action_detail: self
                .post_committed_quickbar_item_refresh_first_client_action_detail,
            first_event_after_client_action: self
                .post_committed_quickbar_item_refresh_first_event_after_client_action,
        })
    }

    pub(crate) fn quickbar_item_refresh_harness_hint(
        &self,
    ) -> Option<QuickbarItemRefreshHarnessHint> {
        let summary = self.unresolved_pending_item_refresh()?;
        let candidate = summary.item_context.compact_item_emission_ready_candidate?;
        let (recommended_set_button_slot, recommended_set_button_slot_source) =
            self.quickbar_item_refresh_set_button_slot();
        let first_preserved_active_item_signature = self
            .last_quickbar_stream_probe
            .and_then(|probe| probe.first_preserved_active_item_signature);
        let first_preserved_active_item_slot = self
            .last_quickbar_stream_probe
            .and_then(|probe| probe.first_preserved_active_item_slot);
        let candidate_preserved_active_item = self
            .last_quickbar_stream_probe
            .and_then(|probe| probe.preserved_active_item_for_object(candidate.object_id));
        let candidate_preserved_active_item_slot =
            candidate_preserved_active_item.map(|(slot, _)| slot);
        let candidate_preserved_active_item_signature =
            candidate_preserved_active_item.map(|(_, signature)| signature);
        Some(QuickbarItemRefreshHarnessHint {
            candidate,
            recommended_set_button_slot,
            recommended_set_button_slot_source,
            stream_probe: self.last_quickbar_stream_probe.unwrap_or_default(),
            first_preserved_active_item_signature,
            first_preserved_active_item_slot,
            candidate_preserved_active_item_signature,
            candidate_preserved_active_item_slot,
            candidate_use_count_state: self.quickbar_item_use_count_state_for_candidate(
                candidate.object_id,
                candidate_preserved_active_item_slot,
            ),
            first_preserved_active_item_use_count_state: self
                .quickbar_item_use_count_state_for_preserved_active_item(
                    first_preserved_active_item_signature,
                    first_preserved_active_item_slot,
                ),
            quickbar_item_use_count_state_rows: self.quickbar_item_use_count_state.len(),
            quickbar_item_use_count_updates_observed: self.quickbar_item_use_count_updates_observed,
            preserved_active_item_use_count_coverage: self
                .last_quickbar_stream_probe
                .unwrap_or_default()
                .preserved_active_item_use_count_coverage(&self.quickbar_item_use_count_state),
            preserved_active_item_actionable_missing_use_count_slot_mask: summary
                .item_context
                .preserved_active_item_actionable_missing_use_count_slot_mask,
            observed_preserved_active_item_actionable_missing_use_count_slot_mask: self
                .post_committed_quickbar_item_refresh_observed_actionable_missing_use_count_slot_mask,
            updates_since_committed_quickbar: summary.updates_since_committed_quickbar,
            events_since_pending_refresh: summary.events_since_pending_refresh,
            event_breakdown: summary.event_breakdown,
            events_after_first_client_action: summary.events_after_first_client_action,
            event_breakdown_after_first_client_action: summary
                .event_breakdown_after_first_client_action,
            first_candidate_use_count_row: summary.first_candidate_use_count_row,
            first_candidate_use_count_row_before_first_client_action: summary
                .first_candidate_use_count_row_before_first_client_action,
            first_candidate_use_count_row_after_first_client_action: summary
                .first_candidate_use_count_row_after_first_client_action,
            action_outcome: summary.action_outcome,
            followup_events_before_first_client_action: summary
                .followup_events_before_first_client_action,
            proof_class: summary.proof_class,
            first_followup_event: summary.first_followup_event,
            first_client_action: summary.first_client_action,
            first_client_action_detail: summary.first_client_action_detail,
            first_event_after_client_action: summary.first_event_after_client_action,
            direct_item_proof_objects: summary.item_context.direct_item_proof_objects,
            feature25_item_proof_objects: summary.item_context.feature25_item_proof_objects,
            compact_item_emission_proof_objects: summary
                .item_context
                .compact_item_emission_proof_objects,
            compact_item_emission_direct_only_proof_objects: summary
                .item_context
                .compact_item_emission_direct_only_proof_objects,
            compact_item_emission_feature25_only_proof_objects: summary
                .item_context
                .compact_item_emission_feature25_only_proof_objects,
            compact_item_emission_shared_proof_objects: summary
                .item_context
                .compact_item_emission_shared_proof_objects,
            item_context: summary.item_context,
            inventory_equipment_handoff_events: self.inventory_equipment_handoff_events,
            inventory_equipment_handoff_ready_events: self.inventory_equipment_handoff_ready_events,
            inventory_equipment_handoff_blocked_without_ready_state_events: self
                .inventory_equipment_handoff_blocked_without_ready_state_events,
            inventory_equipment_handoff_ready_with_deferred_feature25_events: self
                .inventory_equipment_handoff_ready_with_deferred_feature25_events,
            inventory_equipment_handoff_server_inventory_events: self
                .inventory_equipment_handoff_server_inventory_events,
            inventory_equipment_handoff_server_inventory_ready_events: self
                .inventory_equipment_handoff_server_inventory_ready_events,
            inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events: self
                .inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events,
            inventory_equipment_handoff_client_gui_inventory_events: self
                .inventory_equipment_handoff_client_gui_inventory_events,
            inventory_equipment_handoff_client_gui_inventory_ready_events: self
                .inventory_equipment_handoff_client_gui_inventory_ready_events,
            inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events: self
                .inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events,
            inventory_equipment_handoff_bridge_plan: self.inventory_equipment_handoff_bridge_plan(),
            inventory_equipment_bridge_handoff_emissions: self
                .inventory_equipment_bridge_handoff_emissions,
            last_inventory_equipment_bridge_handoff_emission: self
                .last_inventory_equipment_bridge_handoff_emission,
            inventory_equipment_bridge_handoff_state_updates: self
                .inventory_equipment_bridge_handoff_state_updates,
            last_inventory_equipment_bridge_handoff_state_update: self
                .last_inventory_equipment_bridge_handoff_state_update,
        })
    }

    pub(super) fn quickbar_item_refresh_set_button_slot(&self) -> (u8, &'static str) {
        if let Some(profile) = self.last_committed_quickbar_profile {
            if let Some(slot) = profile.first_blank_slot {
                return (slot, "first_blank_committed_slot");
            }
            if let Some(slot) = profile.first_item_slot {
                return (slot, "first_item_committed_slot");
            }
        }
        (
            QUICKBAR_ITEM_REFRESH_SET_BUTTON_FALLBACK_SLOT,
            "fallback_slot_zero",
        )
    }
}

fn quickbar_item_refresh_outcome_for_profile(
    pending_item_refresh: bool,
    profile: &QuickbarValidatedSlotProfile,
) -> QuickbarItemRefreshOutcome {
    if !pending_item_refresh {
        return QuickbarItemRefreshOutcome::NoPendingRefresh;
    }
    if profile.item_slots == 0 {
        QuickbarItemRefreshOutcome::PendingRefreshStillBlank
    } else {
        QuickbarItemRefreshOutcome::PendingRefreshEmittedItemSlots
    }
}

fn best_quickbar_item_context_for_commit(
    current: InventoryItemContextSummary,
    prior: Option<InventoryItemContextSummary>,
    previous_post: Option<InventoryItemContextSummary>,
) -> (
    Option<InventoryItemContextSummary>,
    Option<QuickbarItemContextSource>,
) {
    if current.has_quickbar_item_context_evidence() {
        return (Some(current), Some(QuickbarItemContextSource::Current));
    }
    if let Some(previous_post) =
        previous_post.filter(|context| context.has_quickbar_item_context_evidence())
    {
        return (
            Some(previous_post),
            Some(QuickbarItemContextSource::PreviousPost),
        );
    }
    if let Some(prior) = prior.filter(|context| context.has_quickbar_item_context_evidence()) {
        return (Some(prior), Some(QuickbarItemContextSource::Prior));
    }
    (None, None)
}

#[derive(Debug, Default)]
pub(crate) struct SyntheticState {
    pub(crate) server_synthetic_packets: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        InventoryEquipmentClientGuiInventoryClaim, InventoryEquipmentClientGuiInventoryClaimKind,
        InventoryEquipmentHandoffOutcome, InventoryEquipmentServerInventoryClaim,
        InventoryFeature25HandoffOutcome, InventoryFeature25MaterializationOutcome,
    };

    use crate::translate::area::{
        AreaPlaceableContext, AreaPlaceableContextAppearanceConflict,
        AreaPlaceableContextIdentityConflict, AreaPlaceableContextObjectIdConfidence,
        AreaPlaceableContextOrientationConflict, AreaPlaceableContextPositionConflict,
        AreaPlaceableContextRow, AreaPlaceableContextState, AreaPlaceableContextStateConflict,
        AreaPlaceableObservedOrientationSource,
    };
    use crate::translate::client_gui_event;
    use crate::translate::client_input;
    use crate::translate::client_quickbar::{self, ClientQuickbarSetButtonKind};
    use crate::translate::live_object_update::LiveObjectQuickbarItemUseCountUpdate;
    use crate::translate::semantic::{
        LiveObjectInventoryFeature25Reference, LiveObjectOrientationSource,
        LiveObjectOrientationVector,
    };

    use super::{
        AreaStaticPlaceableConflictRecordObservation,
        AreaStaticPlaceableConflictRecordProgressSummary, AreaStaticPlaceableConflictRecordSummary,
        ITEM_OBJECT_TYPE, InventoryEquipmentHandoffBridgeAction, InventoryEquipmentHandoffConsumer,
        InventoryItemContextCandidate, InventoryItemContextCandidateSource,
        InventoryItemContextSummary, InventoryItemObjectProof, InventoryItemObjectProvenNeighbor,
        InventoryItemObjectStatus, LiveObjectBounds, LiveObjectMention, LiveObjectOrientation,
        LiveObjectPlaceableAppearance, LiveObjectPlaceableState, LiveObjectPosition,
        ObjectRegistry, PlayerListObjectIds, QuickbarActiveItemSignature,
        QuickbarItemRefreshActionOutcome, QuickbarItemRefreshClientActionDetail,
        QuickbarItemRefreshEventBreakdown, QuickbarItemRefreshEventKind,
        QuickbarItemRefreshHarnessHint, QuickbarItemRefreshProofClass,
        QuickbarItemRefreshUseCountRow, QuickbarPreservedActiveItemSignatures,
        QuickbarRewriteSummary, QuickbarStreamProbeSummary, QuickbarValidatedSlotProfile, UiState,
    };

    #[test]
    fn duplicate_same_type_add_is_idempotent_protocol_state() {
        let mut registry = ObjectRegistry::default();
        let mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: 0x8000_34D8,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };

        registry.observe_mentions(&[mention.clone()]);
        registry.observe_mentions(&[mention.clone()]);

        let object = registry
            .known
            .get(&mention.object_id)
            .expect("object should stay registered");
        assert!(object.active);
        assert_eq!(object.object_type, mention.object_type);
        assert_eq!(object.add_mentions, 2);
        assert_eq!(object.duplicate_add_mentions, 1);
    }

    #[test]
    fn verified_orientation_is_protocol_state() {
        let mut registry = ObjectRegistry::default();
        let mention = LiveObjectMention {
            opcode: b'U',
            object_type: 0x0A,
            object_id: 0x8000_F6AC,
            name: None,
            position: None,
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 900,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };

        registry.observe_mentions(&[mention.clone()]);

        let object = registry
            .known
            .get(&mention.object_id)
            .expect("object should stay registered");
        assert_eq!(object.orientation, mention.orientation);
    }

    #[test]
    fn verified_placeable_appearance_is_protocol_state() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_34D8;
        let add_appearance = LiveObjectPlaceableAppearance {
            appearance: 0x0011,
            resref: None,
        };
        let update_resref = *b"plc_visual_test\0";
        let update_appearance = LiveObjectPlaceableAppearance {
            appearance: 0xFFFE,
            resref: Some(update_resref),
        };

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(add_appearance),
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_appearance),
            Some(add_appearance)
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(update_appearance),
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_appearance),
            Some(update_appearance)
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_appearance),
            None,
            "delete rows clear stale placeable appearance before id reuse"
        );
    }

    #[test]
    fn delete_clears_lifecycle_fields_before_object_id_reuse() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_34D8;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: Some("first lifecycle".to_string()),
            position: Some(LiveObjectPosition {
                x: 1.25,
                y: 2.5,
                z: 3.75,
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 450,
                vector: None,
            }),
            bounds: Some(LiveObjectBounds {
                min_x: -1.0,
                min_y: -2.0,
                min_z: -3.0,
                max_x: 1.0,
                max_y: 2.0,
                max_z: 3.0,
            }),
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x0011,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        }]);

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let object = registry
            .known
            .get(&object_id)
            .expect("reused object id should stay registered");
        assert!(object.active);
        assert_eq!(object.latest_name, None);
        assert_eq!(object.position, None);
        assert_eq!(object.orientation, None);
        assert_eq!(object.bounds, None);
        assert_eq!(object.placeable_appearance, None);
        assert_eq!(object.placeable_state, None);
    }

    #[test]
    fn area_context_tracks_verified_placeable_appearance_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState::default()),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        let conflicting_add = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x2222,
                resref: None,
            }),
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_add));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&conflicting_add));

        let expected_conflict = AreaPlaceableContextAppearanceConflict {
            observed_appearance: 0x2222,
            observed_resref: None,
            module_appearance: 0x1234,
            module_template_resref: None,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should be registered after verified add");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_appearance_conflicts, 1);
        assert_eq!(
            object.latest_area_static_appearance_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_appearance_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_appearance_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external A/09 appearance conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external appearance owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(
            conflict_object.placeable_appearance,
            conflicting_add.placeable_appearance
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one appearance snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, Some(expected_conflict));
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "appearance");
        assert_eq!(snapshot.formatted_state_fields(), "none");
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_summary_for_records([(
                0x09,
                compact_object_id
            )]),
            AreaStaticPlaceableConflictRecordSummary {
                owners: 1,
                appearance: 1,
                appearance_module_normal_target: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );

        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x1234,
                resref: None,
            }),
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&resolving_update));

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact appearance update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_appearance_conflicts, 1);
        assert_eq!(object.area_static_appearance_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_appearance_conflict, None);
        assert_eq!(object.unresolved_area_static_appearance_conflict, None);
        assert_eq!(
            object.placeable_appearance,
            resolving_update.placeable_appearance
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_appearance_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_appearance_conflict_for_record(
                0x05,
                external_object_id
            ),
            None,
            "static placeable appearance conflicts must not leak to other live-object types"
        );
    }

    #[test]
    fn area_context_conflict_summary_classifies_custom_placeable_appearance_edges() {
        let mut registry = ObjectRegistry::default();
        let normal_target_compact_id = 0x0000_0003;
        let normal_target_external_id = 0x8000_0003;
        let custom_target_compact_id = 0x0000_0004;
        let custom_target_external_id = 0x8000_0004;
        let observed_resref = *b"plc_custom_one\0\0";
        let module_resref = *b"plc_custom_two\0\0";
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![
                AreaPlaceableContextRow {
                    object_id: normal_target_compact_id,
                    appearance: 0x0123,
                    object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                    module_state: Some(AreaPlaceableContextState::default()),
                    ..AreaPlaceableContextRow::default()
                },
                AreaPlaceableContextRow {
                    object_id: custom_target_compact_id,
                    appearance: 0xFFFE,
                    module_template_resref: Some(module_resref),
                    object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                    module_state: Some(AreaPlaceableContextState::default()),
                    ..AreaPlaceableContextRow::default()
                },
            ],
            ..AreaPlaceableContext::default()
        };
        let mentions = [
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: normal_target_external_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: Some(LiveObjectPlaceableAppearance {
                    appearance: 0xFFFE,
                    resref: Some(observed_resref),
                }),
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: custom_target_external_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: Some(LiveObjectPlaceableAppearance {
                    appearance: 0x0123,
                    resref: None,
                }),
                placeable_state: None,
            },
        ];

        registry.observe_mentions(&mentions);
        registry.observe_placeable_area_context(&area_context, &mentions);

        let summary = registry.unresolved_area_static_placeable_conflict_summary_for_records([
            (0x09, normal_target_compact_id),
            (0x09, custom_target_compact_id),
        ]);
        assert_eq!(
            summary,
            AreaStaticPlaceableConflictRecordSummary {
                owners: 2,
                appearance: 2,
                appearance_module_custom_target: 1,
                appearance_module_custom_target_with_resref: 1,
                appearance_module_custom_target_missing_resref: 0,
                appearance_module_normal_target: 1,
                appearance_observed_custom_source: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );
    }

    #[test]
    fn area_context_tracks_verified_placeable_orientation_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                has_direction: true,
                dir_y: 1.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState::default()),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let conflicting_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Vector,
                scalar_tenths_degrees: 900,
                vector: Some(LiveObjectOrientationVector {
                    x: -1.0,
                    y: 0.0,
                    z: 0.0,
                }),
            }),
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_update));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_update),
        );

        let expected_conflict = AreaPlaceableContextOrientationConflict {
            observed_source: AreaPlaceableObservedOrientationSource::Vector,
            observed_scalar_tenths_degrees: 900,
            module_scalar_tenths_degrees: 0,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should stay registered after verified orientation update");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_orientation_conflicts, 1);
        assert_eq!(
            object.latest_area_static_orientation_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_orientation_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.orientation,
            Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Vector,
                scalar_tenths_degrees: 900,
                vector: Some(LiveObjectOrientationVector {
                    x: -1.0,
                    y: 0.0,
                    z: 0.0,
                }),
            }),
            "vector-sourced exact U/09 orientation should remain visible to replay diagnostics"
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_orientation_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external orientation conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external conflict owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(conflict_object.orientation, conflicting_update.orientation);
        assert_eq!(
            conflict_object.unresolved_area_static_orientation_conflict,
            Some(expected_conflict)
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one orientation snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, None);
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, Some(expected_conflict));
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "orientation");
        assert_eq!(snapshot.formatted_state_fields(), "none");

        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 0,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&resolving_update));

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact orientation update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_orientation_conflicts, 1);
        assert_eq!(object.area_static_orientation_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_orientation_conflict, None);
        assert_eq!(object.unresolved_area_static_orientation_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_orientation_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_orientation_conflict_for_record(
                0x05,
                external_object_id
            ),
            None,
            "static placeable orientation conflicts must not leak to other live-object types"
        );
    }

    #[test]
    fn area_context_tracks_verified_placeable_position_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                x: 12.34,
                y: 56.78,
                z: 0.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState::default()),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let conflicting_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 1.0,
                y: 2.0,
                z: -3.0,
            }),
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_update));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_update),
        );

        let expected_conflict = AreaPlaceableContextPositionConflict {
            observed_x: 1.0,
            observed_y: 2.0,
            observed_z: -3.0,
            module_x: 12.34,
            module_y: 56.78,
            module_z: 0.0,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should stay registered after verified position update");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_position_conflicts, 1);
        assert_eq!(
            object.latest_area_static_position_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_position_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_position_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external position conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external conflict owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(conflict_object.position, conflicting_update.position);
        assert_eq!(
            conflict_object.unresolved_area_static_position_conflict,
            Some(expected_conflict)
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one position snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, None);
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, Some(expected_conflict));
        assert_eq!(snapshot.formatted_classes(), "position");
        assert_eq!(snapshot.formatted_state_fields(), "none");

        let summary = registry.unresolved_area_static_placeable_conflict_summary_for_records([
            (0x09, external_object_id),
            (0x09, compact_object_id),
        ]);
        assert_eq!(
            summary,
            AreaStaticPlaceableConflictRecordSummary {
                owners: 1,
                position: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );

        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 12.34,
                y: 56.78,
                z: 0.0,
            }),
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&resolving_update));

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact position update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_position_conflicts, 1);
        assert_eq!(object.area_static_position_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_position_conflict, None);
        assert_eq!(object.unresolved_area_static_position_conflict, None);
        assert_eq!(object.position, resolving_update.position);
        assert_eq!(
            registry.unresolved_area_static_placeable_position_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_position_conflict_for_record(
                0x05,
                external_object_id
            ),
            None,
            "static placeable position conflicts must not leak to other live-object types"
        );
    }

    #[test]
    fn area_context_tracks_placeable_identity_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let ambiguous_area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::AreaObjectAlias,
                module_state: None,
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        let add_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&add_mention));
        registry.observe_placeable_area_context(
            &ambiguous_area_context,
            std::slice::from_ref(&add_mention),
        );

        let expected_conflict = AreaPlaceableContextIdentityConflict {
            light_rows: 0,
            static_rows: 1,
            module_backed_static_rows: 0,
            module_unbacked_static_rows: 1,
            unproven_static_rows: 1,
            source_incompatible_static_rows: 0,
            source_read_mismatch_static_rows: 0,
            source_fragment_owned_static_rows: 0,
            source_read_mismatch_and_fragment_owned_static_rows: 0,
            area_alias_rows: 1,
            duplicate_object_id_rows: 0,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should be registered after verified add");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_identity_conflicts, 1);
        assert_eq!(
            object.latest_area_static_identity_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_identity_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_identity_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external A/09 identity conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external identity owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one identity snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, Some(expected_conflict));
        assert_eq!(snapshot.appearance, None);
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "identity");
        assert_eq!(snapshot.formatted_state_fields(), "none");

        let unique_area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(false),
                ..LiveObjectPlaceableState::default()
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry.observe_placeable_area_context(
            &unique_area_context,
            std::slice::from_ref(&resolving_update),
        );

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_identity_conflicts, 1);
        assert_eq!(object.area_static_identity_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_identity_conflict, None);
        assert_eq!(object.unresolved_area_static_identity_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_identity_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
    }

    #[test]
    fn verified_placeable_state_merges_add_and_update_facts() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_34D8;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(true),
                ..LiveObjectPlaceableState::default()
            }),
        }]);

        let object = registry
            .known
            .get(&object_id)
            .expect("placeable should stay registered");
        assert_eq!(
            object.placeable_state,
            Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            })
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_state),
            None,
            "delete rows clear stale placeable state before any future id reuse"
        );
    }

    #[test]
    fn area_context_conflicts_use_merged_verified_placeable_state() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let add_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        };

        registry.observe_mentions(std::slice::from_ref(&add_mention));
        registry.observe_placeable_area_context(&area_context, std::slice::from_ref(&add_mention));

        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should be registered after verified add");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_state_conflicts, 0);
        assert_eq!(
            object.latest_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict::default())
        );
        assert_eq!(object.unresolved_area_static_state_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None
        );

        let update_mention = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(true),
                ..LiveObjectPlaceableState::default()
            }),
        };

        registry.observe_mentions(std::slice::from_ref(&update_mention));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&update_mention));

        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should remain registered after verified update");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_state_conflicts, 1);
        assert_eq!(
            object.latest_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            })
        );
        assert_eq!(
            object.unresolved_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            })
        );
        assert_eq!(object.area_static_state_conflict_resolutions, 0);
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, compact_object_id),
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            }),
            "future translators may see either compact Diamond ids or canonical EE external ids"
        );
        assert_eq!(
            object.placeable_state,
            Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            })
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one state snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, None);
        assert_eq!(
            snapshot.state,
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            })
        );
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "state");
        assert_eq!(snapshot.formatted_state_fields(), "locked");

        let resolving_update_mention = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(false),
                ..LiveObjectPlaceableState::default()
            }),
        };

        registry.observe_mentions(std::slice::from_ref(&resolving_update_mention));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&resolving_update_mention),
        );

        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should remain registered after resolving update");
        assert_eq!(object.area_placeable_context_overlaps, 3);
        assert_eq!(object.area_static_state_conflicts, 1);
        assert_eq!(object.area_static_state_conflict_resolutions, 1);
        assert_eq!(
            object.latest_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict::default())
        );
        assert_eq!(object.unresolved_area_static_state_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x05, external_object_id),
            None,
            "static placeable conflict state must not leak to other live-object types"
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&external_object_id)
                .and_then(|object| object.latest_area_static_state_conflict),
            None,
            "delete rows clear stale area/static mismatch state before id reuse"
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None,
            "delete rows clear unresolved mismatch state before id reuse"
        );
    }

    #[test]
    fn area_context_conflict_summary_dedupes_alias_owners_and_counts_classes() {
        let mut registry = ObjectRegistry::default();
        let compact_conflict_id = 0x0000_0003;
        let external_conflict_id = 0x8000_0003;
        let compact_identity_id = 0x0000_0004;
        let external_identity_id = 0x8000_0004;

        let conflict_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_conflict_id,
                appearance: 0x1234,
                has_direction: true,
                dir_y: 1.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: false,
                    trap_disarmable: false,
                    lockable: false,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let conflicting_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_conflict_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 900,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x2222,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_mention));
        registry.observe_placeable_area_context(
            &conflict_context,
            std::slice::from_ref(&conflicting_mention),
        );

        let identity_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_identity_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::AreaObjectAlias,
                module_state: None,
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let identity_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_identity_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&identity_mention));
        registry.observe_placeable_area_context(
            &identity_context,
            std::slice::from_ref(&identity_mention),
        );

        let summary = registry.unresolved_area_static_placeable_conflict_summary_for_records([
            (0x09, external_conflict_id),
            (0x09, compact_conflict_id),
            (0x09, external_identity_id),
            (0x05, external_conflict_id),
            (0x09, 0x8000_DEAD),
        ]);

        assert_eq!(
            summary,
            AreaStaticPlaceableConflictRecordSummary {
                owners: 2,
                identity: 1,
                appearance: 1,
                appearance_module_custom_target: 0,
                appearance_module_custom_target_with_resref: 0,
                appearance_module_custom_target_missing_resref: 0,
                appearance_module_normal_target: 1,
                appearance_observed_custom_source: 0,
                state: 1,
                orientation: 1,
                position: 1,
                state_useable: 1,
                state_trap_disarmable: 0,
                state_lockable: 1,
                state_locked: 1,
            }
        );
    }

    #[test]
    fn untyped_placeable_owner_conflict_lookup_uses_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_placeable_id = 0x0000_0003;
        let external_placeable_id = 0x8000_0003;
        let compact_creature_id = 0x0000_0004;
        let external_creature_id = 0x8000_0004;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_placeable_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let conflicting_placeable = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_placeable_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(false),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_placeable));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_placeable),
        );
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: external_creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let expected_conflict = AreaPlaceableContextStateConflict {
            lockable: true,
            locked: true,
            ..AreaPlaceableContextStateConflict::default()
        };
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0, compact_placeable_id),
            Some(expected_conflict),
            "untyped owner rows should see an external placeable conflict through its compact alias"
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0, compact_placeable_id)
            .expect("untyped compact owner should resolve to the external placeable conflict");
        assert_eq!(snapshot.object.object_id, external_placeable_id);
        assert_eq!(snapshot.state, Some(expected_conflict));
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_summary_for_records([
                (0, compact_placeable_id),
                (0, external_placeable_id),
                (0, compact_creature_id),
                (0x05, compact_placeable_id),
            ]),
            AreaStaticPlaceableConflictRecordSummary {
                owners: 1,
                state: 1,
                state_lockable: 1,
                state_locked: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0, compact_creature_id),
            None,
            "untyped placeable conflict lookup must not claim compact creature ids"
        );
    }

    #[test]
    fn area_context_conflict_progress_classifies_current_record_fields() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                x: 12.34,
                y: 56.78,
                z: 0.0,
                has_direction: true,
                dir_x: -1.0,
                dir_y: 0.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: false,
                    trap_disarmable: false,
                    lockable: false,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let conflicting_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 1.0,
                y: 2.0,
                z: -3.0,
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 1800,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x2222,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_mention));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_mention),
        );

        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("conflicting external add should be visible through compact alias");

        let resolving_record = AreaStaticPlaceableConflictRecordObservation {
            object_type: 0x09,
            object_id: compact_object_id,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x1234,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(false),
                trap_disarmable: Some(false),
                lockable: Some(false),
                locked: Some(false),
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 900,
                vector: None,
            }),
            position: Some(LiveObjectPosition {
                x: 12.34,
                y: 56.78,
                z: 0.0,
            }),
        };
        let resolving_progress = snapshot.progress_for_observation(resolving_record);
        assert_eq!(
            resolving_progress.formatted_resolving_fields(),
            "appearance,state.useable,state.lockable,state.locked,orientation,position"
        );
        assert_eq!(resolving_progress.formatted_repeating_fields(), "none");
        assert_eq!(resolving_progress.formatted_untouched_fields(), "none");

        let repeating_record = AreaStaticPlaceableConflictRecordObservation {
            object_type: 0x09,
            object_id: external_object_id,
            placeable_appearance: conflicting_mention.placeable_appearance,
            placeable_state: conflicting_mention.placeable_state,
            orientation: conflicting_mention.orientation,
            position: conflicting_mention.position,
        };
        let repeating_progress = snapshot.progress_for_observation(repeating_record);
        assert_eq!(repeating_progress.formatted_resolving_fields(), "none");
        assert_eq!(
            repeating_progress.formatted_repeating_fields(),
            "appearance,state.useable,state.lockable,state.locked,orientation,position"
        );
        assert_eq!(repeating_progress.formatted_untouched_fields(), "none");

        let untouched_record = AreaStaticPlaceableConflictRecordObservation {
            object_type: 0x09,
            object_id: external_object_id,
            ..AreaStaticPlaceableConflictRecordObservation::default()
        };
        let untouched_progress = snapshot.progress_for_observation(untouched_record);
        assert_eq!(untouched_progress.formatted_resolving_fields(), "none");
        assert_eq!(untouched_progress.formatted_repeating_fields(), "none");
        assert_eq!(
            untouched_progress.formatted_untouched_fields(),
            "appearance,state.useable,state.lockable,state.locked,orientation,position"
        );

        let progress_summary = registry
            .unresolved_area_static_placeable_conflict_progress_for_records([
                resolving_record,
                repeating_record,
                untouched_record,
            ]);
        assert_eq!(
            progress_summary,
            AreaStaticPlaceableConflictRecordProgressSummary {
                owners: 1,
                resolving_owners: 1,
                repeating_owners: 1,
                untouched_owners: 0,
                resolving_appearance: 1,
                repeating_appearance: 1,
                untouched_appearance: 1,
                resolving_state: 1,
                repeating_state: 1,
                untouched_state: 1,
                resolving_orientation: 1,
                repeating_orientation: 1,
                untouched_orientation: 1,
                resolving_position: 1,
                repeating_position: 1,
                untouched_position: 1,
            }
        );
    }

    #[test]
    fn placeable_area_conflicts_resolve_across_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        let conflicting_add = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(false),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_add));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&conflicting_add));

        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, compact_object_id),
            Some(AreaPlaceableContextStateConflict {
                lockable: true,
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            }),
            "future compact U/09 rows should see the external A/09 conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external conflict owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(
            conflict_object.placeable_state,
            conflicting_add.placeable_state
        );

        let resolving_compact_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(false),
                ..LiveObjectPlaceableState::default()
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_compact_update));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&resolving_compact_update),
        );

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact update should merge into the external add entry");
        assert_eq!(object.area_static_state_conflicts, 1);
        assert_eq!(object.area_static_state_conflict_resolutions, 1);
        assert_eq!(object.unresolved_area_static_state_conflict, None);
        assert_eq!(
            object.placeable_state,
            Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            })
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, compact_object_id),
            None
        );
    }

    #[test]
    fn active_placeable_lifecycle_lookup_uses_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(registry.has_active_typed_object(0x09, external_object_id));
        assert!(
            registry.has_active_typed_object(0x09, compact_object_id),
            "compact U/D/09 rows should see the active external A/09 owner"
        );
        assert!(
            registry.has_active_live_object_for_record(0x09, compact_object_id),
            "lifecycle cleanup must share placeable compact/external alias ownership"
        );
        assert!(
            !registry.has_active_typed_object(0x05, compact_object_id),
            "placeable alias ownership must not leak to creature rows"
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(
            !registry.has_active_typed_object(0x09, external_object_id),
            "compact delete should clear the external owner entry"
        );
        assert!(!registry.has_active_typed_object(0x09, compact_object_id));
    }

    #[test]
    fn untyped_lifecycle_lookup_uses_placeable_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_placeable_id = 0x0000_0003;
        let external_placeable_id = 0x8000_0003;
        let compact_creature_id = 0x0000_0004;
        let external_creature_id = 0x8000_0004;

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_placeable_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: external_creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(
            registry.has_active_object_id(compact_placeable_id),
            "untyped owner rows should see an active external placeable through its compact alias"
        );
        assert!(
            registry.has_active_live_object_for_record(0, compact_placeable_id),
            "inventory owner lifecycle proof must use the same placeable alias rule"
        );
        assert!(
            !registry.has_active_object_id(compact_creature_id),
            "untyped placeable alias lookup must not claim compact creature ids"
        );
        assert!(registry.has_active_object_id(external_creature_id));
    }

    #[test]
    fn materialized_item_ids_are_protocol_state_without_live_add() {
        let mut registry = ObjectRegistry::default();
        let item_object_id = 0x4000_1234;

        assert!(!registry.has_active_object_id(item_object_id));

        registry.observe_materialized_item_object_ids(&[item_object_id]);

        assert!(registry.has_active_object_id(item_object_id));
        assert!(
            registry.known.get(&item_object_id).is_none(),
            "GUI item materialization must not invent a live-object add/type"
        );

        registry.reset_for_area();

        assert!(!registry.has_active_object_id(item_object_id));
        assert_eq!(
            registry.inventory_item_object_status(item_object_id),
            InventoryItemObjectStatus::ClearedByAreaReset,
            "area reset should explain why the prior item proof is no longer usable"
        );
    }

    #[test]
    fn inventory_item_proof_requires_item_specific_state() {
        let mut registry = ObjectRegistry::default();
        let creature_id = 0x8000_0005;
        let placeable_id = 0x8000_0009;
        let item_id = 0x8000_0006;
        let gui_materialized_item_id = 0x8000_0106;

        registry.observe_mentions(&[
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x05,
                object_id: creature_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: placeable_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: ITEM_OBJECT_TYPE,
                object_id: item_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
        ]);
        registry.observe_materialized_item_object_ids(&[gui_materialized_item_id]);

        assert!(registry.has_active_object_id(creature_id));
        assert!(registry.has_active_object_id(placeable_id));
        assert_eq!(
            registry.inventory_item_object_proof(creature_id),
            None,
            "quickbar item proof must not accept active creature lifecycle state"
        );
        assert_eq!(
            registry.inventory_item_object_proof(placeable_id),
            None,
            "quickbar item proof must not accept active placeable lifecycle state"
        );
        assert_eq!(
            registry.inventory_item_object_proof(item_id),
            Some(InventoryItemObjectProof::ActiveObject),
            "typed item live-object state remains valid quickbar item proof"
        );
        assert_eq!(
            registry.inventory_item_object_proof(gui_materialized_item_id),
            Some(InventoryItemObjectProof::ActiveObject),
            "GUI item-create materialization remains valid quickbar item proof"
        );
    }

    #[test]
    fn inventory_item_proven_neighborhood_uses_only_ready_item_state() {
        let mut registry = ObjectRegistry::default();
        let lower_item_id = 0x8000_1000;
        let claim_id = 0x8000_1100;
        let higher_item_id = 0x8000_1120;
        let deferred_feature25_id = 0x8000_1110;
        let creature_id = 0x8000_10F0;

        registry.observe_materialized_item_object_ids(&[lower_item_id]);
        registry.observe_mentions(&[
            LiveObjectMention {
                opcode: b'A',
                object_type: ITEM_OBJECT_TYPE,
                object_id: higher_item_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x05,
                object_id: creature_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
        ]);
        registry.observe_inventory_feature25_references(&[LiveObjectInventoryFeature25Reference {
            owner_id: creature_id,
            mask: 0x2000,
            first_object_ids: vec![deferred_feature25_id],
            second_object_ids: Vec::new(),
            legacy_tail_object_ids: Vec::new(),
        }]);

        let neighborhood = registry.inventory_item_object_proven_neighborhood(claim_id);

        assert_eq!(
            neighborhood.lower,
            Some(InventoryItemObjectProvenNeighbor {
                object_id: lower_item_id,
                distance: 0x100,
            })
        );
        assert_eq!(
            neighborhood.higher,
            Some(InventoryItemObjectProvenNeighbor {
                object_id: higher_item_id,
                distance: 0x20,
            })
        );
        assert_eq!(
            neighborhood.closest(),
            Some(InventoryItemObjectProvenNeighbor {
                object_id: higher_item_id,
                distance: 0x20,
            })
        );
    }

    #[test]
    fn item_delete_clears_materialized_quickbar_item_proof() {
        let mut registry = ObjectRegistry::default();
        let item_id = 0x8000_0106;

        registry.observe_materialized_item_object_ids(&[item_id]);
        assert_eq!(
            registry.inventory_item_object_proof(item_id),
            Some(InventoryItemObjectProof::ActiveObject)
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: ITEM_OBJECT_TYPE,
            object_id: item_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert_eq!(
            registry.inventory_item_object_proof(item_id),
            None,
            "D/06 must clear GUI-materialized item proof before quickbar can reuse it"
        );
        assert_eq!(
            registry.inventory_item_object_status(item_id),
            InventoryItemObjectStatus::ClearedByItemDelete,
            "diagnostics should retain that the missing proof was cleared by D/06"
        );
        assert!(
            !registry.has_active_object_id(item_id),
            "deleted item id must no longer satisfy untyped active-object checks"
        );
    }

    #[test]
    fn item_delete_clears_only_matching_feature25_quickbar_item_ref() {
        let mut registry = ObjectRegistry::default();
        let first_item_id = 0x8000_0101;
        let second_item_id = 0x8000_0102;
        let legacy_tail_item_id = 0x8000_0103;
        let survivor_item_id = 0x8000_0104;

        registry.observe_inventory_feature25_references(&[LiveObjectInventoryFeature25Reference {
            owner_id: 0x8000_0005,
            mask: 0x2000,
            first_object_ids: vec![first_item_id, survivor_item_id],
            second_object_ids: vec![second_item_id],
            legacy_tail_object_ids: vec![legacy_tail_item_id],
        }]);
        assert_eq!(
            registry.inventory_item_object_proof(first_item_id),
            None,
            "Feature-25 refs are reference-only until item materialization is observed"
        );
        assert_eq!(
            registry.inventory_item_object_status(first_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25FirstList
            )
        );
        assert_eq!(registry.inventory_item_object_proof(second_item_id), None);
        assert_eq!(
            registry.inventory_item_object_status(second_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25SecondList
            )
        );
        assert_eq!(
            registry.inventory_item_object_proof(legacy_tail_item_id),
            None
        );
        assert_eq!(
            registry.inventory_item_object_status(legacy_tail_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25LegacyTail
            )
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: ITEM_OBJECT_TYPE,
            object_id: second_item_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert_eq!(
            registry.inventory_item_object_proof(second_item_id),
            None,
            "D/06 must clear stale second-list Feature-25 item proof"
        );
        assert_eq!(
            registry.inventory_item_object_status(second_item_id),
            InventoryItemObjectStatus::ClearedByItemDelete,
            "D/06-cleared Feature-25 proof should remain visible as a diagnostic status"
        );
        assert_eq!(
            registry.inventory_item_object_proof(first_item_id),
            None,
            "deleting one item must not turn unrelated reference-only refs into proof"
        );
        assert_eq!(
            registry.inventory_item_object_status(first_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25FirstList
            ),
            "deleting one item must not clear unrelated first-list refs"
        );
        assert_eq!(
            registry.inventory_item_object_proof(legacy_tail_item_id),
            None,
            "deleting one item must not turn unrelated reference-only refs into proof"
        );
        assert_eq!(
            registry.inventory_item_object_status(legacy_tail_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25LegacyTail
            ),
            "deleting one item must not clear unrelated legacy-tail refs"
        );
        assert_eq!(
            registry.inventory_item_object_proof(survivor_item_id),
            None,
            "other refs in the same Feature-25 claim remain reference-only evidence"
        );
        assert_eq!(
            registry.inventory_item_object_status(survivor_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25FirstList
            ),
            "other refs in the same Feature-25 claim remain visible as deferred status"
        );
    }

    #[test]
    fn feature25_reference_metrics_separate_materialized_and_deferred_item_refs() {
        let mut registry = ObjectRegistry::default();
        let gui_materialized_item_id = 0x8000_0201;
        let active_item_id = 0x8000_0202;
        let first_deferred_item_id = 0x8000_0203;
        let second_deferred_item_id = 0x8000_0204;
        let legacy_tail_deferred_item_id = 0x8000_0205;

        registry.observe_materialized_item_object_ids(&[gui_materialized_item_id]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: ITEM_OBJECT_TYPE,
            object_id: active_item_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        registry.observe_inventory_feature25_references(&[LiveObjectInventoryFeature25Reference {
            owner_id: 0xFFFF_FFEC,
            mask: 0x2E00,
            first_object_ids: vec![gui_materialized_item_id, first_deferred_item_id],
            second_object_ids: vec![active_item_id, second_deferred_item_id],
            legacy_tail_object_ids: vec![legacy_tail_deferred_item_id],
        }]);

        assert_eq!(registry.inventory_feature25_first_item_ref_mentions, 2);
        assert_eq!(
            registry.inventory_feature25_first_materialized_item_ref_mentions,
            1
        );
        assert_eq!(
            registry.inventory_feature25_first_deferred_item_ref_mentions,
            1
        );
        assert_eq!(registry.inventory_feature25_second_item_ref_mentions, 2);
        assert_eq!(
            registry.inventory_feature25_second_materialized_item_ref_mentions,
            1
        );
        assert_eq!(
            registry.inventory_feature25_second_deferred_item_ref_mentions,
            1
        );
        assert_eq!(
            registry.inventory_feature25_legacy_tail_item_ref_mentions,
            1
        );
        assert_eq!(
            registry.inventory_feature25_legacy_tail_materialized_item_ref_mentions,
            0
        );
        assert_eq!(
            registry.inventory_feature25_legacy_tail_deferred_item_ref_mentions,
            1
        );
        assert_eq!(
            registry.inventory_item_object_proof(first_deferred_item_id),
            None,
            "live 2026-07-07 evidence keeps deferred Feature-25 refs reference-only for compact item emission"
        );
        assert_eq!(
            registry.inventory_item_object_status(first_deferred_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25FirstList
            )
        );
        assert_eq!(
            registry.inventory_item_object_proof(second_deferred_item_id),
            None
        );
        assert_eq!(
            registry.inventory_item_object_status(second_deferred_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25SecondList
            )
        );
        assert_eq!(
            registry.inventory_item_object_proof(legacy_tail_deferred_item_id),
            None
        );
        assert_eq!(
            registry.inventory_item_object_status(legacy_tail_deferred_item_id),
            InventoryItemObjectStatus::DeferredFeature25(
                InventoryItemObjectProof::Feature25LegacyTail
            )
        );

        let summary = registry.inventory_item_context_summary();
        assert_eq!(summary.active_item_objects, 1);
        assert_eq!(summary.materialized_item_objects, 1);
        assert_eq!(
            summary.direct_item_proof_objects, 2,
            "active live item and GUI-materialized item ids are distinct direct proofs"
        );
        assert_eq!(
            summary.feature25_item_proof_objects, 5,
            "Feature-25 proof inventory is the unique union of first, second, and legacy-tail refs"
        );
        assert_eq!(
            summary.compact_item_emission_proof_objects, 5,
            "Feature-25 refs already include the two direct-proof ids in this fixture"
        );
        assert_eq!(
            summary.compact_item_emission_ready_objects, 2,
            "only direct item proof can make compact quickbar item emission ready"
        );
        assert_eq!(
            summary.compact_item_emission_direct_only_proof_objects, 0,
            "both direct-proof ids are also present in Feature-25 refs"
        );
        assert_eq!(
            summary.compact_item_emission_feature25_only_proof_objects, 3,
            "three deferred Feature-25 refs have no direct item materialization"
        );
        assert_eq!(
            summary.compact_item_emission_shared_proof_objects, 2,
            "direct and Feature-25 proof overlap should stay explicit for quickbar policy"
        );
        assert_eq!(
            summary.compact_item_emission_candidate,
            Some(InventoryItemContextCandidate {
                object_id: gui_materialized_item_id,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::Shared,
            }),
            "the deterministic harness candidate should point at the lowest shared direct/Feature-25 proof when no direct-only proof exists"
        );
        assert_eq!(
            summary.compact_item_emission_ready_candidate,
            Some(InventoryItemContextCandidate {
                object_id: gui_materialized_item_id,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::Shared,
            }),
            "emission-ready candidate selection must ignore deferred-only Feature-25 refs"
        );
        assert_eq!(
            summary.compact_item_emission_deferred_feature25_only_objects, 3,
            "deferred-only Feature-25 refs remain diagnostic but are not emission-ready"
        );
        assert_eq!(summary.inventory_feature25_first_item_refs, 2);
        assert_eq!(summary.inventory_feature25_second_item_refs, 2);
        assert_eq!(summary.inventory_feature25_legacy_tail_item_refs, 1);
        assert_eq!(summary.inventory_feature25_reference_records, 1);
        assert_eq!(summary.inventory_feature25_first_item_ref_mentions, 2);
        assert_eq!(
            summary.inventory_feature25_first_materialized_item_ref_mentions,
            1
        );
        assert_eq!(
            summary.inventory_feature25_first_deferred_item_ref_mentions,
            1
        );
        assert_eq!(
            summary.inventory_feature25_second_materialized_item_ref_mentions,
            1
        );
        assert_eq!(
            summary.inventory_feature25_second_deferred_item_ref_mentions,
            1
        );
        assert_eq!(
            summary.inventory_feature25_legacy_tail_materialized_item_ref_mentions,
            0
        );
        assert_eq!(
            summary.inventory_feature25_legacy_tail_deferred_item_ref_mentions,
            1
        );
        assert_eq!(summary.inventory_feature25_item_ref_mentions(), 5);
        assert_eq!(
            summary.inventory_feature25_materialized_item_ref_mentions(),
            2
        );
        assert_eq!(summary.inventory_feature25_deferred_item_ref_mentions(), 3);
        assert_eq!(
            summary.inventory_feature25_materialization_outcome(),
            InventoryFeature25MaterializationOutcome::MixedItemRefs
        );
        assert_eq!(
            summary.inventory_feature25_handoff_outcome(),
            InventoryFeature25HandoffOutcome::MixedItemRefsWithReadyItemState,
            "mixed Feature-25 refs plus direct item state can hand off to the quickbar/UI writer"
        );
        assert!(summary.inventory_equipment_handoff_ready());
        assert_eq!(
            summary.inventory_equipment_handoff_outcome(),
            InventoryEquipmentHandoffOutcome::ReadyItemStateWithDeferredFeature25Refs,
            "ready direct/materialized item state drives UI handoff while deferred Feature-25 refs stay reference-only"
        );
        let live_like_deferred = InventoryItemContextSummary {
            inventory_feature25_reference_records: 13,
            inventory_feature25_first_item_ref_mentions: 6,
            inventory_feature25_first_deferred_item_ref_mentions: 6,
            inventory_feature25_second_item_ref_mentions: 7,
            inventory_feature25_second_deferred_item_ref_mentions: 7,
            ..Default::default()
        };
        assert_eq!(
            live_like_deferred.inventory_feature25_item_ref_mentions(),
            13
        );
        assert_eq!(
            live_like_deferred.inventory_feature25_materialized_item_ref_mentions(),
            0
        );
        assert_eq!(
            live_like_deferred.inventory_feature25_deferred_item_ref_mentions(),
            13
        );
        assert_eq!(
            live_like_deferred.inventory_feature25_materialization_outcome(),
            InventoryFeature25MaterializationOutcome::AllItemRefsDeferred
        );
        assert_eq!(
            live_like_deferred.inventory_feature25_handoff_outcome(),
            InventoryFeature25HandoffOutcome::AllItemRefsDeferredWithoutReadyItemState,
            "deferred-only Feature-25 refs are reference-only unless separate ready item state exists"
        );
        assert!(!live_like_deferred.inventory_equipment_handoff_ready());
        assert_eq!(
            live_like_deferred.inventory_equipment_handoff_outcome(),
            InventoryEquipmentHandoffOutcome::Feature25RefsWithoutReadyItemState,
            "Feature-25-only refs are not an inventory/equipment UI handoff source"
        );
        let live_like_deferred_with_ready_state = InventoryItemContextSummary {
            compact_item_emission_ready_objects: 18,
            ..live_like_deferred
        };
        assert_eq!(
            live_like_deferred_with_ready_state.inventory_feature25_handoff_outcome(),
            InventoryFeature25HandoffOutcome::AllItemRefsDeferredWithReadyItemState,
            "2026-07-07 live HG shape keeps Feature-25 refs deferred while direct state carries quickbar handoff"
        );
        assert!(live_like_deferred_with_ready_state.inventory_equipment_handoff_ready());
        assert_eq!(
            live_like_deferred_with_ready_state.inventory_equipment_handoff_outcome(),
            InventoryEquipmentHandoffOutcome::ReadyItemStateWithDeferredFeature25Refs,
            "2026-07-07 live HG shape consumes ready direct item state without materializing deferred Feature-25 refs"
        );
    }

    #[test]
    fn compact_item_emission_candidate_prefers_direct_then_shared_then_feature25() {
        let mut direct_only = ObjectRegistry::default();
        direct_only.observe_materialized_item_object_ids(&[0x8000_0100]);
        direct_only.observe_inventory_feature25_references(&[
            LiveObjectInventoryFeature25Reference {
                owner_id: 0xFFFF_FFEC,
                mask: 0x2000,
                first_object_ids: vec![0x8000_0200],
                second_object_ids: vec![],
                legacy_tail_object_ids: vec![],
            },
        ]);
        let direct_only_summary = direct_only.inventory_item_context_summary();
        assert_eq!(
            direct_only_summary.compact_item_emission_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8000_0100,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            })
        );
        assert_eq!(direct_only_summary.compact_item_emission_ready_objects, 1);
        assert_eq!(
            direct_only_summary.compact_item_emission_ready_candidate,
            direct_only_summary.compact_item_emission_candidate
        );
        assert_eq!(
            direct_only_summary.inventory_equipment_handoff_outcome(),
            InventoryEquipmentHandoffOutcome::ReadyItemStateWithDeferredFeature25Refs
        );

        let mut quickbar_preferred = ObjectRegistry::default();
        quickbar_preferred.observe_materialized_item_object_ids(&[0x8000_0100, 0x8000_0108]);
        let quickbar_preferred_summary = quickbar_preferred
            .inventory_item_context_summary_with_preferred_ready_candidate(Some(0x8000_0108));
        assert_eq!(
            quickbar_preferred_summary.compact_item_emission_ready_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8000_0108,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            }),
            "a directly proven preserved quickbar item must outrank an unrelated lower object id"
        );
        assert_eq!(
            quickbar_preferred_summary.compact_item_emission_candidate,
            quickbar_preferred_summary.compact_item_emission_ready_candidate
        );
        assert_eq!(
            quickbar_preferred
                .inventory_item_context_summary_with_preferred_ready_candidate(Some(0x8000_0109))
                .compact_item_emission_ready_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8000_0100,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            }),
            "an unproven preferred id must not bypass the normal proof-priority fallback"
        );

        let mut pure_ready = ObjectRegistry::default();
        pure_ready.observe_materialized_item_object_ids(&[0x8000_0100]);
        assert_eq!(
            pure_ready
                .inventory_item_context_summary()
                .inventory_equipment_handoff_outcome(),
            InventoryEquipmentHandoffOutcome::ReadyItemState,
            "ready direct/materialized state without deferred Feature-25 refs uses the simple ready outcome"
        );

        let mut shared = ObjectRegistry::default();
        shared.observe_materialized_item_object_ids(&[0x8000_0100]);
        shared.observe_inventory_feature25_references(&[LiveObjectInventoryFeature25Reference {
            owner_id: 0xFFFF_FFEC,
            mask: 0x2000,
            first_object_ids: vec![0x8000_0100, 0x8000_0200],
            second_object_ids: vec![],
            legacy_tail_object_ids: vec![],
        }]);
        let shared_summary = shared.inventory_item_context_summary();
        assert_eq!(
            shared_summary.compact_item_emission_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8000_0100,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::Shared,
            })
        );
        assert_eq!(shared_summary.compact_item_emission_ready_objects, 1);
        assert_eq!(
            shared_summary.compact_item_emission_ready_candidate,
            shared_summary.compact_item_emission_candidate
        );

        let mut feature25_only = ObjectRegistry::default();
        feature25_only.observe_inventory_feature25_references(&[
            LiveObjectInventoryFeature25Reference {
                owner_id: 0xFFFF_FFEC,
                mask: 0x2000,
                first_object_ids: vec![],
                second_object_ids: vec![0x8000_0300],
                legacy_tail_object_ids: vec![],
            },
        ]);
        let feature25_only_summary = feature25_only.inventory_item_context_summary();
        assert_eq!(
            feature25_only_summary.compact_item_emission_candidate,
            Some(InventoryItemContextCandidate {
                object_id: 0x8000_0300,
                proof: InventoryItemObjectProof::Feature25SecondList,
                source: InventoryItemContextCandidateSource::Feature25Only,
            })
        );
        assert_eq!(
            feature25_only_summary.compact_item_emission_ready_objects,
            0
        );
        assert_eq!(
            feature25_only_summary.compact_item_emission_ready_candidate, None,
            "Feature-25-only refs stay available for diagnostics but cannot drive item emission"
        );
        assert_eq!(
            feature25_only_summary.compact_item_emission_deferred_feature25_only_objects,
            1
        );
        assert_eq!(
            feature25_only_summary.inventory_equipment_handoff_outcome(),
            InventoryEquipmentHandoffOutcome::Feature25RefsWithoutReadyItemState
        );
    }

    #[test]
    fn inventory_equipment_handoff_consumes_ready_state_without_feature25_materialization() {
        let ready_with_deferred = InventoryItemContextSummary {
            direct_item_proof_objects: 18,
            compact_item_emission_proof_objects: 20,
            compact_item_emission_ready_objects: 18,
            compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                object_id: 0x8001_5219,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            }),
            compact_item_emission_deferred_feature25_only_objects: 2,
            inventory_feature25_reference_records: 7,
            inventory_feature25_first_item_ref_mentions: 4,
            inventory_feature25_first_deferred_item_ref_mentions: 4,
            inventory_feature25_second_item_ref_mentions: 3,
            inventory_feature25_second_deferred_item_ref_mentions: 3,
            ..Default::default()
        };
        let blocked_feature25_only = InventoryItemContextSummary {
            feature25_item_proof_objects: 2,
            compact_item_emission_proof_objects: 2,
            compact_item_emission_deferred_feature25_only_objects: 2,
            inventory_feature25_reference_records: 2,
            inventory_feature25_first_item_ref_mentions: 2,
            inventory_feature25_first_deferred_item_ref_mentions: 2,
            ..Default::default()
        };
        let mut ui = UiState::default();
        let client_gui_claim = InventoryEquipmentClientGuiInventoryClaim {
            kind: InventoryEquipmentClientGuiInventoryClaimKind::Status,
            object_id: Some(0x7F00_0000),
            panel: None,
            player_inventory_gui: None,
            rewritten_self_object_id: true,
        };

        assert!(ui.observe_inventory_equipment_handoff(
            InventoryEquipmentHandoffConsumer::ClientGuiInventory,
            ready_with_deferred,
            None,
            Some(client_gui_claim),
        ));
        assert!(!ui.observe_inventory_equipment_handoff(
            InventoryEquipmentHandoffConsumer::ServerInventory,
            blocked_feature25_only,
            None,
            None,
        ));

        assert_eq!(ui.inventory_equipment_handoff_events, 2);
        assert_eq!(ui.inventory_equipment_handoff_ready_events, 1);
        assert_eq!(
            ui.inventory_equipment_handoff_blocked_without_ready_state_events,
            1
        );
        assert_eq!(
            ui.inventory_equipment_handoff_ready_with_deferred_feature25_events, 1,
            "ready direct item state is consumed while deferred Feature-25 refs remain reference-only"
        );
        assert_eq!(
            ui.inventory_equipment_handoff_client_gui_inventory_events,
            1
        );
        assert_eq!(
            ui.inventory_equipment_handoff_client_gui_inventory_ready_events,
            1
        );
        assert_eq!(
            ui.inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events,
            0
        );
        assert_eq!(ui.inventory_equipment_handoff_server_inventory_events, 1);
        assert_eq!(
            ui.inventory_equipment_handoff_server_inventory_ready_events,
            0
        );
        assert_eq!(
            ui.inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events,
            1
        );
        let snapshot = ui
            .last_inventory_equipment_handoff
            .expect("ready handoff snapshot should be retained");
        assert_eq!(
            snapshot.consumer,
            InventoryEquipmentHandoffConsumer::ClientGuiInventory
        );
        assert_eq!(snapshot.event_index, 1);
        assert_eq!(snapshot.item_context, ready_with_deferred);
        assert_eq!(snapshot.client_gui_inventory_claim, Some(client_gui_claim));
        let bridge_plan = ui.inventory_equipment_handoff_bridge_plan();
        assert_eq!(
            bridge_plan.action,
            InventoryEquipmentHandoffBridgeAction::EmitReadyItemState
        );
        assert!(bridge_plan.ready_to_emit());
        assert_eq!(
            bridge_plan.consumer,
            InventoryEquipmentHandoffConsumer::ClientGuiInventory
        );
        assert_eq!(bridge_plan.event_index, 1);
        assert_eq!(
            bridge_plan.outcome,
            InventoryEquipmentHandoffOutcome::ReadyItemStateWithDeferredFeature25Refs
        );
        assert_eq!(bridge_plan.ready_objects, 18);
        assert_eq!(bridge_plan.deferred_feature25_only_objects, 2);
        assert_eq!(
            bridge_plan.client_gui_inventory_claim,
            Some(client_gui_claim)
        );
        assert_eq!(
            bridge_plan
                .candidate
                .expect("ready handoff should retain a bridge candidate")
                .object_id,
            0x8001_5219
        );
        assert_eq!(ui.inventory_equipment_bridge_handoff_emissions, 1);
        let emission = ui
            .last_inventory_equipment_bridge_handoff_emission
            .expect("ready handoff should emit one bridge-facing record");
        assert_eq!(emission.emission_index, 1);
        assert_eq!(emission.plan, bridge_plan);
        assert_eq!(ui.inventory_equipment_bridge_handoff_state_updates, 1);
        let state_update = ui
            .last_inventory_equipment_bridge_handoff_state_update
            .expect("ready handoff emission should drain into bridge state");
        assert_eq!(state_update.update_index, 1);
        assert_eq!(state_update.emission_index, emission.emission_index);
        assert_eq!(
            state_update.consumer,
            InventoryEquipmentHandoffConsumer::ClientGuiInventory
        );
        assert_eq!(state_update.event_index, 1);
        assert_eq!(state_update.candidate.object_id, 0x8001_5219);
        assert_eq!(state_update.ready_objects, 18);
        assert_eq!(state_update.deferred_feature25_only_objects, 2);
        assert_eq!(state_update.server_inventory_claim, None);
        assert_eq!(
            state_update.client_gui_inventory_claim,
            Some(client_gui_claim)
        );
        assert_eq!(
            ui.drain_inventory_equipment_bridge_handoff_emission(emission),
            None,
            "the bridge consumer must not apply the same emission twice"
        );
        assert_eq!(ui.inventory_equipment_bridge_handoff_state_updates, 1);

        let json = ui.quickbar_item_refresh_harness_idle_json();
        assert!(json.contains("\"inventory_equipment_handoff_events\": 2"));
        assert!(json.contains("\"inventory_equipment_handoff_ready_events\": 1"));
        assert!(
            json.contains("\"inventory_equipment_handoff_blocked_without_ready_state_events\": 1")
        );
        assert!(
            json.contains(
                "\"inventory_equipment_handoff_ready_with_deferred_feature25_events\": 1"
            )
        );
        assert!(json.contains("\"inventory_equipment_handoff_client_gui_inventory_events\": 1"));
        assert!(
            json.contains("\"inventory_equipment_handoff_client_gui_inventory_ready_events\": 1")
        );
        assert!(json.contains(
            "\"inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events\": 0"
        ));
        assert!(json.contains("\"inventory_equipment_handoff_server_inventory_events\": 1"));
        assert!(json.contains("\"inventory_equipment_handoff_server_inventory_ready_events\": 0"));
        assert!(json.contains(
            "\"inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events\": 1"
        ));
        assert!(json.contains("\"last_inventory_equipment_handoff_known\": true"));
        assert!(
            json.contains(
                "\"last_inventory_equipment_handoff_consumer\": \"client_gui_inventory\""
            )
        );
        assert!(json.contains(
            "\"last_inventory_equipment_handoff_outcome\": \"ready_item_state_with_deferred_feature25_refs\""
        ));
        assert!(json.contains(
            "\"last_inventory_equipment_handoff_candidate_object_id_hex\": \"0x80015219\""
        ));
        assert!(
            json.contains(
                "\"inventory_equipment_bridge_handoff_action\": \"emit_ready_item_state\""
            )
        );
        assert!(json.contains("\"inventory_equipment_bridge_handoff_ready\": true"));
        assert!(
            json.contains(
                "\"inventory_equipment_bridge_handoff_consumer\": \"client_gui_inventory\""
            )
        );
        assert!(json.contains("\"inventory_equipment_bridge_handoff_event_index\": 1"));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_outcome\": \"ready_item_state_with_deferred_feature25_refs\""
        ));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_ready_objects\": 18"));
        assert!(
            json.contains(
                "\"inventory_equipment_bridge_handoff_deferred_feature25_only_objects\": 2"
            )
        );
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_candidate_object_id_hex\": \"0x80015219\""
        ));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_emissions\": 1"));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_last_emitted_known\": true"));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_last_emitted_index\": 1"));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_emitted_consumer\": \"client_gui_inventory\""
        ));
        assert!(
            json.contains("\"inventory_equipment_bridge_handoff_last_emitted_event_index\": 1")
        );
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_emitted_candidate_object_id_hex\": \"0x80015219\""
        ));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_state_updates\": 1"));
        assert!(
            json.contains("\"inventory_equipment_bridge_handoff_last_state_update_known\": true")
        );
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_emission_index\": 1"
        ));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_consumer\": \"client_gui_inventory\""
        ));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_candidate_object_id_hex\": \"0x80015219\""
        ));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_deferred_feature25_only_objects\": 2"
        ));
    }

    #[test]
    fn pending_server_inventory_handoff_consumes_later_ready_state() {
        let ready_with_deferred = InventoryItemContextSummary {
            direct_item_proof_objects: 18,
            compact_item_emission_proof_objects: 20,
            compact_item_emission_ready_objects: 18,
            compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                object_id: 0x8001_5219,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            }),
            compact_item_emission_deferred_feature25_only_objects: 2,
            inventory_feature25_reference_records: 7,
            inventory_feature25_first_item_ref_mentions: 4,
            inventory_feature25_first_deferred_item_ref_mentions: 4,
            inventory_feature25_second_item_ref_mentions: 3,
            inventory_feature25_second_deferred_item_ref_mentions: 3,
            ..Default::default()
        };
        let blocked_before_item_state = InventoryItemContextSummary {
            feature25_item_proof_objects: 2,
            compact_item_emission_proof_objects: 2,
            compact_item_emission_deferred_feature25_only_objects: 2,
            inventory_feature25_reference_records: 2,
            inventory_feature25_first_item_ref_mentions: 2,
            inventory_feature25_first_deferred_item_ref_mentions: 2,
            ..Default::default()
        };
        let claim = InventoryEquipmentServerInventoryClaim::new(1, 0x8001_5219, true, 4);
        let mut ui = UiState::default();

        assert!(!ui.observe_inventory_equipment_handoff(
            InventoryEquipmentHandoffConsumer::ServerInventory,
            blocked_before_item_state,
            Some(claim),
            None,
        ));
        assert_eq!(ui.inventory_equipment_handoff_events, 1);
        assert_eq!(ui.inventory_equipment_handoff_ready_events, 0);
        assert_eq!(
            ui.inventory_equipment_handoff_blocked_without_ready_state_events,
            1
        );
        assert_eq!(ui.inventory_equipment_handoff_server_inventory_events, 1);
        assert_eq!(
            ui.inventory_equipment_handoff_server_inventory_ready_events,
            0
        );
        assert_eq!(
            ui.inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events,
            1
        );
        assert_eq!(ui.last_inventory_equipment_handoff, None);
        assert_eq!(ui.inventory_equipment_bridge_handoff_emissions, 0);
        assert_eq!(ui.inventory_equipment_bridge_handoff_state_updates, 0);

        assert!(
            ui.consume_pending_server_inventory_handoff_if_ready(ready_with_deferred),
            "later item-state proof should consume the retained server Inventory claim"
        );
        assert_eq!(ui.inventory_equipment_handoff_events, 2);
        assert_eq!(ui.inventory_equipment_handoff_ready_events, 1);
        assert_eq!(
            ui.inventory_equipment_handoff_blocked_without_ready_state_events,
            1
        );
        assert_eq!(ui.inventory_equipment_handoff_server_inventory_events, 2);
        assert_eq!(
            ui.inventory_equipment_handoff_server_inventory_ready_events,
            1
        );
        assert_eq!(
            ui.inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events,
            1
        );
        assert_eq!(
            ui.inventory_equipment_handoff_ready_with_deferred_feature25_events,
            1
        );

        let snapshot = ui
            .last_inventory_equipment_handoff
            .expect("pending server Inventory should become the latest ready handoff");
        assert_eq!(
            snapshot.consumer,
            InventoryEquipmentHandoffConsumer::ServerInventory
        );
        assert_eq!(snapshot.event_index, 2);
        assert_eq!(snapshot.item_context, ready_with_deferred);
        assert_eq!(snapshot.server_inventory_claim, Some(claim));

        let state_update = ui
            .last_inventory_equipment_bridge_handoff_state_update
            .expect("ready pending server Inventory should drain into bridge state");
        assert_eq!(
            state_update.consumer,
            InventoryEquipmentHandoffConsumer::ServerInventory
        );
        assert_eq!(state_update.event_index, 2);
        assert_eq!(state_update.candidate.object_id, 0x8001_5219);
        assert_eq!(state_update.ready_objects, 18);
        assert_eq!(state_update.deferred_feature25_only_objects, 2);
        assert_eq!(state_update.server_inventory_claim, Some(claim));
        assert!(!ui.consume_pending_server_inventory_handoff_if_ready(ready_with_deferred));
        assert_eq!(
            ui.inventory_equipment_bridge_handoff_state_updates, 1,
            "pending server Inventory claim must be consumed only once"
        );
    }

    fn stream_probe_rewrite_summary_with_profile(
        profile: Option<QuickbarValidatedSlotProfile>,
        trailing_read_bytes: usize,
        fragment_size: usize,
    ) -> QuickbarRewriteSummary {
        QuickbarRewriteSummary {
            old_payload_length: 1523,
            new_payload_length: 1726,
            old_declared: 1501,
            new_declared: 1702,
            read_size: 1494,
            fragment_size,
            final_cursor: 1494usize.saturating_sub(trailing_read_bytes),
            trailing_read_bytes,
            direct_opcode_stream: false,
            slot_records_owned: 36,
            item_buttons_seen: 21,
            item_buttons_source_explicit: 21,
            item_buttons_source_compact: 0,
            item_buttons_source_recovered: 0,
            item_buttons_preserved: 21,
            spells_preserved: 15,
            blank_buttons_seen: 0,
            general_buttons_preserved: 0,
            general_buttons_blanked: 0,
            item_buttons_blanked: 0,
            item_buttons_blanked_candidate: 0,
            unsupported_buttons_blanked: 0,
            item_buttons_rejected_recovered_type_tag: 0,
            item_buttons_rejected_missing_type_source: 0,
            item_buttons_rejected_no_present_item: 0,
            item_buttons_rejected_invalid_object_id: 0,
            item_buttons_rejected_missing_active_properties: 0,
            item_buttons_rejected_unsupported_appearance_type: 0,
            item_buttons_rejected_appearance_shape: 0,
            item_buttons_rejected_missing_state_proof: 0,
            item_buttons_rejected_missing_state_unknown: 0,
            item_buttons_rejected_missing_state_cleared_delete: 0,
            item_buttons_rejected_missing_state_cleared_area_reset: 0,
            item_objects_rejected_missing_state_proven: 0,
            item_objects_rejected_missing_state_active: 0,
            item_objects_rejected_missing_state_feature25_first: 0,
            item_objects_rejected_missing_state_feature25_second: 0,
            item_objects_rejected_missing_state_feature25_legacy_tail: 0,
            item_objects_rejected_missing_state_unknown: 0,
            item_objects_rejected_missing_state_cleared_delete: 0,
            item_objects_rejected_missing_state_cleared_area_reset: 0,
            item_objects_preserved_by_explicit_self_materialization: 21,
            item_objects_preserved_by_active_state: 0,
            item_objects_preserved_by_feature25_first: 0,
            item_objects_preserved_by_feature25_second: 0,
            item_objects_preserved_by_feature25_legacy_tail: 0,
            preserved_active_item_signatures: QuickbarPreservedActiveItemSignatures::default(),
            first_preserved_active_item_signature: None,
            first_preserved_active_item_slot: None,
            validated_slot_profile: profile,
        }
    }

    #[test]
    fn exact_stream_probe_quickbar_profile_promotes_committed_state() {
        let mut ui = UiState::default();
        let profile = QuickbarValidatedSlotProfile {
            slot_records: 36,
            item_slots: 21,
            spell_slots: 15,
            first_item_slot: Some(0),
            first_page_item_slots: 3,
            first_page_spell_slots: 2,
            first_page_visible_slots: 5,
            ..Default::default()
        };
        let summary = stream_probe_rewrite_summary_with_profile(Some(profile), 0, 22);
        let context = InventoryItemContextSummary {
            direct_item_proof_objects: 1,
            ..Default::default()
        };

        ui.observe_quickbar_stream_probe(&summary, context);
        assert!(ui.promote_quickbar_stream_probe_profile(&summary, context));

        assert_eq!(ui.quickbar_packets, 1);
        assert_eq!(ui.last_committed_quickbar_profile, Some(profile));
        assert_eq!(
            ui.last_committed_quickbar_materialization_context,
            Some(context)
        );
        assert_eq!(
            ui.last_committed_quickbar_best_item_context,
            Some(context),
            "stream-probe promotion should preserve the same item-context preference as a verified committed quickbar"
        );
        assert_eq!(
            ui.quickbar_item_refresh_harness_idle_reason(),
            "no_post_committed_item_context"
        );
    }

    #[test]
    fn stream_probe_quickbar_profile_waiting_for_more_bytes_does_not_promote() {
        let mut ui = UiState::default();
        let profile = QuickbarValidatedSlotProfile {
            slot_records: 36,
            blank_slots: 36,
            first_blank_slot: Some(0),
            ..Default::default()
        };
        let summary = stream_probe_rewrite_summary_with_profile(Some(profile), 11, 0);

        ui.observe_quickbar_stream_probe(&summary, InventoryItemContextSummary::default());
        assert!(!ui.promote_quickbar_stream_probe_profile(
            &summary,
            InventoryItemContextSummary::default()
        ));

        assert_eq!(ui.quickbar_packets, 0);
        assert_eq!(ui.last_committed_quickbar_profile, None);
        assert_eq!(
            ui.quickbar_item_refresh_harness_idle_reason(),
            "stream_probe_quickbar_item_candidates_without_committed_profile"
        );
    }

    #[test]
    fn quickbar_item_refresh_harness_hint_serializes_pending_candidate() {
        let mut ui = UiState::default();
        assert_eq!(
            ui.quickbar_item_refresh_harness_hint(),
            None,
            "no hint should be emitted until a verified pending refresh exists"
        );

        let item_context = InventoryItemContextSummary {
            direct_item_proof_objects: 1,
            feature25_item_proof_objects: 1,
            compact_item_emission_proof_objects: 1,
            compact_item_emission_candidate: Some(InventoryItemContextCandidate {
                object_id: 0x8000_0100,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::Shared,
            }),
            compact_item_emission_ready_objects: 1,
            compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                object_id: 0x8000_0100,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::Shared,
            }),
            compact_item_emission_shared_proof_objects: 1,
            inventory_feature25_reference_records: 2,
            inventory_feature25_first_item_refs: 1,
            inventory_feature25_first_item_ref_mentions: 3,
            inventory_feature25_first_materialized_item_ref_mentions: 1,
            inventory_feature25_first_deferred_item_ref_mentions: 2,
            inventory_feature25_second_item_refs: 1,
            inventory_feature25_second_item_ref_mentions: 4,
            inventory_feature25_second_materialized_item_ref_mentions: 3,
            inventory_feature25_second_deferred_item_ref_mentions: 1,
            inventory_feature25_legacy_tail_item_refs: 1,
            inventory_feature25_legacy_tail_item_ref_mentions: 5,
            inventory_feature25_legacy_tail_materialized_item_ref_mentions: 2,
            inventory_feature25_legacy_tail_deferred_item_ref_mentions: 3,
            cleared_inventory_item_object_ids: 1,
            ..InventoryItemContextSummary::default()
        };
        ui.last_committed_quickbar_profile =
            Some(crate::translate::quickbar::QuickbarValidatedSlotProfile {
                slot_records: 36,
                blank_slots: 34,
                item_slots: 2,
                first_blank_slot: Some(5),
                first_item_slot: Some(2),
                ..crate::translate::quickbar::QuickbarValidatedSlotProfile::default()
            });
        ui.last_inventory_item_context_after_committed_quickbar = Some(item_context);
        ui.inventory_item_context_after_committed_quickbar_updates = 7;
        ui.post_committed_quickbar_item_refresh_pending_events = 11;
        ui.post_committed_quickbar_item_refresh_pending_event_breakdown =
            QuickbarItemRefreshEventBreakdown {
                server_to_client_events: 10,
                client_to_server_events: 1,
                live_object_events: 7,
                server_quickbar_item_use_count_events: 2,
                server_quickbar_item_use_count_records: 3,
                server_quickbar_item_use_count_rows: 4,
                server_quickbar_item_use_count_candidate_rows: 1,
                server_active_item_property_events: 5,
                server_active_item_property_uses_events: 3,
                server_active_item_property_full_events: 2,
                server_active_item_property_candidate_events: 1,
                server_active_item_property_candidate_uses_events: 1,
                server_active_item_property_candidate_changed_use_count_rows: 2,
                client_input_other_events: 1,
                other_events: 3,
                ..QuickbarItemRefreshEventBreakdown::default()
            };
        ui.post_committed_quickbar_item_refresh_events_after_first_client_action = 2;
        ui.post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
            QuickbarItemRefreshEventBreakdown {
                server_to_client_events: 1,
                client_to_server_events: 1,
                live_object_events: 1,
                other_events: 1,
                ..QuickbarItemRefreshEventBreakdown::default()
            };
        let pre_action_use_count_row = QuickbarItemRefreshUseCountRow {
            slot: 2,
            button_type: 1,
            object_id: 0x8000_0100,
            active_property_index: 7,
            use_count: 4,
        };
        ui.post_committed_quickbar_item_refresh_first_candidate_use_count_row =
            Some(pre_action_use_count_row);
        ui.post_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action =
            Some(pre_action_use_count_row);
        ui.post_committed_quickbar_item_refresh_followup_events_before_first_client_action = 8;
        ui.post_committed_quickbar_item_refresh_proof_class =
            Some(QuickbarItemRefreshProofClass::Shared);
        ui.post_committed_quickbar_item_refresh_first_followup_event =
            Some(QuickbarItemRefreshEventKind::ClientInputOther);
        ui.post_committed_quickbar_item_refresh_first_client_action =
            Some(QuickbarItemRefreshEventKind::ClientInputOther);
        ui.post_committed_quickbar_item_refresh_first_client_action_detail =
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientInputOther,
                object_id: Some(0x8000_0100),
                slot: None,
                button_type: None,
                body_kind: None,
                gui_event_a: None,
                gui_event_b: None,
                gui_event_declared_bytes: None,
                gui_event_trailing_fragment_bytes: None,
                gui_event_has_vector: None,
                gui_event_vector_bits: None,
                use_item_active_property_subtype: None,
                use_item_has_optional_byte: None,
                use_item_has_target_object: None,
                use_item_target_object_id: None,
                use_item_has_position: None,
                use_object_mark_inventory_gui_state: None,
                use_object_schedule_script_event: None,
                candidate_object_id: Some(0x8000_0100),
                matches_candidate_object: Some(true),
            });
        ui.post_committed_quickbar_item_refresh_first_event_after_client_action =
            Some(QuickbarItemRefreshEventKind::LiveObject);
        ui.last_quickbar_stream_probe = Some(QuickbarStreamProbeSummary {
            slot_records_owned: 36,
            item_buttons_seen: 2,
            item_buttons_source_compact: 0,
            item_buttons_preserved: 2,
            item_buttons_blanked: 0,
            item_buttons_blanked_candidate: 0,
            item_buttons_rejected_missing_state_proof: 0,
            item_buttons_rejected_missing_state_unknown: 0,
            item_buttons_rejected_missing_state_cleared_delete: 1,
            item_buttons_rejected_missing_state_cleared_area_reset: 2,
            item_objects_rejected_missing_state_proven: 3,
            item_objects_rejected_missing_state_active: 4,
            item_objects_rejected_missing_state_feature25_first: 5,
            item_objects_rejected_missing_state_feature25_second: 6,
            item_objects_rejected_missing_state_feature25_legacy_tail: 7,
            item_objects_rejected_missing_state_unknown: 8,
            item_objects_rejected_missing_state_cleared_delete: 9,
            item_objects_rejected_missing_state_cleared_area_reset: 10,
            item_objects_preserved_by_explicit_self_materialization: 11,
            item_objects_preserved_by_active_state: 12,
            item_objects_preserved_by_feature25_first: 13,
            item_objects_preserved_by_feature25_second: 14,
            item_objects_preserved_by_feature25_legacy_tail: 15,
            first_preserved_active_item_signature: Some(QuickbarActiveItemSignature {
                object_id: 0x8000_0100,
                base_item: 0x11,
                appearance_type: 2,
                active_property_count: 1,
                first_property: Some(
                    crate::translate::quickbar::QuickbarActivePropertySignature {
                        property: 100,
                        subtype: 2,
                        cost_table_value: 3,
                        param: 4,
                    },
                ),
                has_armor_word: false,
                name_is_locstring: false,
                state_mask: 0x05,
                value_mask: 0x08,
            }),
            first_preserved_active_item_slot: Some(2),
            ..QuickbarStreamProbeSummary::default()
        });
        if let Some(probe) = ui.last_quickbar_stream_probe.as_mut() {
            probe.preserved_active_item_signatures.0[2] =
                probe.first_preserved_active_item_signature;
        }
        ui.observe_quickbar_item_use_count_updates(&[
            LiveObjectQuickbarItemUseCountUpdate {
                slot: 8,
                button_type: 1,
                object_id: 0x8000_0100,
                active_property_index: 9,
                use_count: 1,
            },
            LiveObjectQuickbarItemUseCountUpdate {
                slot: 2,
                button_type: 1,
                object_id: 0x8000_0100,
                active_property_index: 7,
                use_count: 4,
            },
        ]);
        assert!(ui.observe_inventory_equipment_handoff(
            InventoryEquipmentHandoffConsumer::ClientGuiInventory,
            item_context,
            None,
            None,
        ));
        assert!(ui.observe_inventory_equipment_handoff(
            InventoryEquipmentHandoffConsumer::ServerInventory,
            item_context,
            Some(InventoryEquipmentServerInventoryClaim::new(
                0x01,
                0x8000_0100,
                true,
                4,
            )),
            None,
        ));

        assert_eq!(
            ui.quickbar_item_refresh_harness_hint(),
            None,
            "candidate evidence alone should not emit a driver hint before the pending bit is set"
        );

        ui.post_committed_quickbar_item_refresh_pending = true;
        let hint = ui
            .quickbar_item_refresh_harness_hint()
            .expect("pending candidate should expose a harness hint");
        assert_eq!(hint.candidate.object_id, 0x8000_0100);
        assert_eq!(hint.candidate.proof, InventoryItemObjectProof::ActiveObject);
        assert_eq!(
            hint.proof_class,
            Some(QuickbarItemRefreshProofClass::Shared)
        );
        assert_eq!(hint.updates_since_committed_quickbar, 7);
        assert_eq!(hint.events_since_pending_refresh, 11);
        assert_eq!(hint.event_breakdown.server_to_client_events, 10);
        assert_eq!(hint.event_breakdown.client_to_server_events, 1);
        assert_eq!(
            hint.first_followup_event,
            Some(QuickbarItemRefreshEventKind::ClientInputOther)
        );
        assert_eq!(hint.quickbar_item_use_count_state_rows, 2);
        assert_eq!(hint.quickbar_item_use_count_updates_observed, 2);
        assert_eq!(
            hint.candidate_use_count_state,
            Some(pre_action_use_count_row)
        );
        assert_eq!(
            hint.first_preserved_active_item_use_count_state,
            Some(pre_action_use_count_row)
        );
        assert_eq!(hint.inventory_equipment_handoff_events, 2);
        assert_eq!(hint.inventory_equipment_handoff_ready_events, 2);
        assert_eq!(
            hint.inventory_equipment_handoff_blocked_without_ready_state_events,
            0
        );
        assert_eq!(
            hint.inventory_equipment_handoff_ready_with_deferred_feature25_events,
            2
        );
        assert_eq!(
            hint.inventory_equipment_handoff_client_gui_inventory_events,
            1
        );
        assert_eq!(
            hint.inventory_equipment_handoff_client_gui_inventory_ready_events,
            1
        );
        assert_eq!(
            hint.inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events,
            0
        );
        assert_eq!(hint.inventory_equipment_handoff_server_inventory_events, 1);
        assert_eq!(
            hint.inventory_equipment_handoff_server_inventory_ready_events,
            1
        );
        assert_eq!(
            hint.inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events,
            0
        );
        assert_eq!(
            hint.inventory_equipment_handoff_bridge_plan.action,
            InventoryEquipmentHandoffBridgeAction::EmitReadyItemState
        );
        assert!(hint.inventory_equipment_handoff_bridge_plan.ready_to_emit());
        assert_eq!(
            hint.inventory_equipment_handoff_bridge_plan.consumer,
            InventoryEquipmentHandoffConsumer::ServerInventory
        );
        assert_eq!(hint.inventory_equipment_handoff_bridge_plan.event_index, 2);
        assert_eq!(
            hint.inventory_equipment_handoff_bridge_plan.outcome,
            InventoryEquipmentHandoffOutcome::ReadyItemStateWithDeferredFeature25Refs
        );
        assert_eq!(
            hint.inventory_equipment_handoff_bridge_plan.ready_objects,
            1
        );
        assert_eq!(
            hint.inventory_equipment_handoff_bridge_plan
                .candidate
                .expect("pending bridge plan should retain the ready active-object candidate")
                .object_id,
            0x8000_0100
        );
        assert_eq!(hint.inventory_equipment_bridge_handoff_emissions, 2);
        let last_emission = hint
            .last_inventory_equipment_bridge_handoff_emission
            .expect("latest ready consumer should emit a bridge handoff record");
        assert_eq!(last_emission.emission_index, 2);
        assert_eq!(
            last_emission.plan,
            hint.inventory_equipment_handoff_bridge_plan
        );
        assert_eq!(hint.inventory_equipment_bridge_handoff_state_updates, 2);
        let last_state_update = hint
            .last_inventory_equipment_bridge_handoff_state_update
            .expect("latest ready bridge handoff should drain into state");
        assert_eq!(last_state_update.update_index, 2);
        assert_eq!(last_state_update.emission_index, 2);
        assert_eq!(
            last_state_update.consumer,
            InventoryEquipmentHandoffConsumer::ServerInventory
        );
        assert_eq!(last_state_update.event_index, 2);
        assert_eq!(last_state_update.candidate.object_id, 0x8000_0100);
        assert_eq!(last_state_update.ready_objects, 1);
        assert_eq!(last_state_update.deferred_feature25_only_objects, 0);
        assert_eq!(
            last_state_update.server_inventory_claim,
            Some(InventoryEquipmentServerInventoryClaim::new(
                0x01,
                0x8000_0100,
                true,
                4,
            ))
        );

        let json = hint.to_json();
        assert!(json.contains("\"post_committed_item_refresh_resolution\": \"pending\""));
        assert!(json.contains("\"candidate_object_id\": 2147483904"));
        assert!(json.contains("\"candidate_object_id_hex\": \"0x80000100\""));
        assert!(json.contains("\"candidate_proof\": \"active_object\""));
        assert!(json.contains("\"candidate_source\": \"shared\""));
        assert!(json.contains("\"first_preserved_active_item_known\": true"));
        assert!(json.contains("\"first_preserved_active_item_matches_candidate\": true"));
        assert!(json.contains("\"first_preserved_active_item_slot_known\": true"));
        assert!(json.contains("\"first_preserved_active_item_slot\": 2"));
        assert!(json.contains("\"first_preserved_active_item_first_page_slot\": true"));
        assert!(json.contains(
            "\"first_preserved_active_item_slot_matches_recommended_set_button_slot\": false"
        ));
        assert!(json.contains("\"first_preserved_active_item_object_id_hex\": \"0x80000100\""));
        assert!(json.contains("\"first_preserved_active_item_base_item_hex\": \"0x00000011\""));
        assert!(json.contains("\"first_preserved_active_item_appearance_type\": 2"));
        assert!(json.contains("\"first_preserved_active_item_property_count\": 1"));
        assert!(json.contains("\"first_preserved_active_item_first_property_known\": true"));
        assert!(json.contains("\"first_preserved_active_item_first_property\": 100"));
        assert!(json.contains("\"first_preserved_active_item_first_property_subtype\": 2"));
        assert!(json.contains("\"first_preserved_active_item_state_mask_hex\": \"0x05\""));
        assert!(json.contains("\"first_preserved_active_item_value_mask_hex\": \"0x08\""));
        assert!(
            json.contains("\"stream_probe_item_buttons_rejected_missing_state_cleared_delete\": 1")
        );
        assert!(json.contains(
            "\"stream_probe_item_buttons_rejected_missing_state_cleared_area_reset\": 2"
        ));
        assert!(json.contains("\"stream_probe_item_objects_rejected_missing_state_proven\": 3"));
        assert!(json.contains("\"stream_probe_item_objects_rejected_missing_state_active\": 4"));
        assert!(
            json.contains(
                "\"stream_probe_item_objects_rejected_missing_state_feature25_first\": 5"
            )
        );
        assert!(
            json.contains(
                "\"stream_probe_item_objects_rejected_missing_state_feature25_second\": 6"
            )
        );
        assert!(json.contains(
            "\"stream_probe_item_objects_rejected_missing_state_feature25_legacy_tail\": 7"
        ));
        assert!(json.contains("\"stream_probe_item_objects_rejected_missing_state_unknown\": 8"));
        assert!(
            json.contains("\"stream_probe_item_objects_rejected_missing_state_cleared_delete\": 9")
        );
        assert!(json.contains(
            "\"stream_probe_item_objects_rejected_missing_state_cleared_area_reset\": 10"
        ));
        assert!(json.contains(
            "\"stream_probe_item_objects_preserved_by_explicit_self_materialization\": 11"
        ));
        assert!(json.contains("\"stream_probe_item_objects_preserved_by_active_state\": 12"));
        assert!(json.contains("\"stream_probe_preserved_active_item_signature_count\": 1"));
        assert!(json.contains("\"stream_probe_preserved_active_item_slots\": [2]"));
        assert!(json.contains("\"stream_probe_item_objects_preserved_by_feature25_first\": 13"));
        assert!(json.contains("\"stream_probe_item_objects_preserved_by_feature25_second\": 14"));
        assert!(
            json.contains("\"stream_probe_item_objects_preserved_by_feature25_legacy_tail\": 15")
        );
        assert!(json.contains("\"inventory_feature25_reference_records\": 2"));
        assert!(json.contains("\"inventory_feature25_item_ref_mentions\": 12"));
        assert!(json.contains("\"inventory_feature25_materialized_item_ref_mentions\": 6"));
        assert!(json.contains("\"inventory_feature25_deferred_item_ref_mentions\": 6"));
        assert!(json.contains("\"compact_item_emission_ready_objects\": 1"));
        assert!(json.contains("\"compact_item_emission_deferred_feature25_only_objects\": 0"));
        assert!(
            json.contains("\"inventory_feature25_materialization_outcome\": \"mixed_item_refs\"")
        );
        assert!(json.contains(
            "\"inventory_feature25_handoff_outcome\": \"mixed_item_refs_with_ready_item_state\""
        ));
        assert!(json.contains("\"inventory_equipment_handoff_ready\": true"));
        assert!(json.contains(
            "\"inventory_equipment_handoff_outcome\": \"ready_item_state_with_deferred_feature25_refs\""
        ));
        assert!(json.contains("\"inventory_equipment_handoff_events\": 2"));
        assert!(json.contains("\"inventory_equipment_handoff_ready_events\": 2"));
        assert!(
            json.contains("\"inventory_equipment_handoff_blocked_without_ready_state_events\": 0")
        );
        assert!(
            json.contains(
                "\"inventory_equipment_handoff_ready_with_deferred_feature25_events\": 2"
            )
        );
        assert!(json.contains("\"inventory_equipment_handoff_client_gui_inventory_events\": 1"));
        assert!(
            json.contains("\"inventory_equipment_handoff_client_gui_inventory_ready_events\": 1")
        );
        assert!(json.contains(
            "\"inventory_equipment_handoff_client_gui_inventory_blocked_without_ready_state_events\": 0"
        ));
        assert!(json.contains("\"inventory_equipment_handoff_server_inventory_events\": 1"));
        assert!(json.contains("\"inventory_equipment_handoff_server_inventory_ready_events\": 1"));
        assert!(json.contains(
            "\"inventory_equipment_handoff_server_inventory_blocked_without_ready_state_events\": 0"
        ));
        assert!(
            json.contains(
                "\"inventory_equipment_bridge_handoff_action\": \"emit_ready_item_state\""
            )
        );
        assert!(json.contains("\"inventory_equipment_bridge_handoff_ready\": true"));
        assert!(
            json.contains("\"inventory_equipment_bridge_handoff_consumer\": \"server_inventory\"")
        );
        assert!(json.contains("\"inventory_equipment_bridge_handoff_event_index\": 2"));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_outcome\": \"ready_item_state_with_deferred_feature25_refs\""
        ));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_ready_objects\": 1"));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_candidate_object_id_hex\": \"0x80000100\""
        ));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_emissions\": 2"));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_last_emitted_known\": true"));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_last_emitted_index\": 2"));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_emitted_consumer\": \"server_inventory\""
        ));
        assert!(
            json.contains("\"inventory_equipment_bridge_handoff_last_emitted_event_index\": 2")
        );
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_emitted_candidate_object_id_hex\": \"0x80000100\""
        ));
        assert!(json.contains("\"inventory_equipment_bridge_handoff_state_updates\": 2"));
        assert!(
            json.contains("\"inventory_equipment_bridge_handoff_last_state_update_known\": true")
        );
        assert!(json.contains("\"inventory_equipment_bridge_handoff_last_state_update_index\": 2"));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_emission_index\": 2"
        ));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_consumer\": \"server_inventory\""
        ));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_candidate_object_id_hex\": \"0x80000100\""
        ));
        assert!(json.contains(
            "\"inventory_equipment_bridge_handoff_last_state_update_candidate_proof\": \"active_object\""
        ));
        assert!(
            json.contains(
                "\"inventory_equipment_bridge_handoff_last_state_update_ready_objects\": 1"
            )
        );
        assert!(json.contains("\"inventory_feature25_first_item_refs\": 1"));
        assert!(json.contains("\"inventory_feature25_first_item_ref_mentions\": 3"));
        assert!(json.contains("\"inventory_feature25_first_materialized_item_ref_mentions\": 1"));
        assert!(json.contains("\"inventory_feature25_first_deferred_item_ref_mentions\": 2"));
        assert!(json.contains("\"inventory_feature25_second_item_refs\": 1"));
        assert!(json.contains("\"inventory_feature25_second_item_ref_mentions\": 4"));
        assert!(json.contains("\"inventory_feature25_second_materialized_item_ref_mentions\": 3"));
        assert!(json.contains("\"inventory_feature25_second_deferred_item_ref_mentions\": 1"));
        assert!(json.contains("\"inventory_feature25_legacy_tail_item_refs\": 1"));
        assert!(json.contains("\"inventory_feature25_legacy_tail_item_ref_mentions\": 5"));
        assert!(
            json.contains("\"inventory_feature25_legacy_tail_materialized_item_ref_mentions\": 2")
        );
        assert!(json.contains("\"inventory_feature25_legacy_tail_deferred_item_ref_mentions\": 3"));
        assert!(json.contains("\"cleared_inventory_item_object_ids\": 1"));
        assert!(json.contains("\"recommended_use_item_payload_available\": true"));
        assert!(json.contains("\"recommended_use_item_payload_kind\": \"Input_UseItem\""));
        assert!(json.contains(
            "\"recommended_use_item_payload_hex\": \"700609100000000001008000FDFFFFFFC8\""
        ));
        assert!(json.contains("\"recommended_use_item_item_object_id\": 2147483904"));
        assert!(json.contains("\"recommended_use_item_item_object_id_hex\": \"0x80000100\""));
        assert!(json.contains("\"recommended_use_item_has_optional_byte\": false"));
        assert!(json.contains("\"recommended_use_item_has_target_object\": true"));
        assert!(json.contains("\"recommended_use_item_target_object_id\": 4294967293"));
        assert!(json.contains("\"recommended_use_item_target_object_id_hex\": \"0xFFFFFFFD\""));
        assert!(
            json.contains("\"recommended_use_item_target_legacy_rewrite_object_id\": 2130706432")
        );
        assert!(json.contains(
            "\"recommended_use_item_target_legacy_rewrite_object_id_hex\": \"0x7F000000\""
        ));
        assert!(json.contains("\"recommended_use_item_has_position\": false"));
        assert!(json.contains(
            "\"recommended_use_item_first_property_subtype_low_payload_available\": true"
        ));
        assert!(json.contains(
            "\"recommended_use_item_first_property_subtype_low_payload_kind\": \"Input_UseItem\""
        ));
        assert!(json.contains(
            "\"recommended_use_item_first_property_subtype_low_payload_hex\": \"700609100000000001008002FDFFFFFFC8\""
        ));
        assert!(
            json.contains("\"recommended_use_item_first_property_subtype_low_byte_known\": true")
        );
        assert!(json.contains("\"recommended_use_item_first_property_subtype_low_byte\": 2"));
        assert!(json.contains(
            "\"recommended_use_item_first_property_subtype_low_source\": \"first_preserved_active_item_first_property_subtype_low_byte\""
        ));
        assert!(json.contains(
            "\"recommended_use_item_first_property_subtype_low_matches_default\": false"
        ));
        assert!(json.contains(
            "\"recommended_use_item_first_property_subtype_low_has_target_object\": true"
        ));
        assert!(json.contains(
            "\"recommended_use_item_first_property_subtype_low_target_object_id_hex\": \"0xFFFFFFFD\""
        ));
        assert!(
            json.contains("\"recommended_client_quickbar_set_button_payload_available\": true")
        );
        assert!(json.contains(
            "\"recommended_client_quickbar_set_button_payload_kind\": \"GuiQuickbar_SetButton\""
        ));
        assert!(json.contains(
            "\"recommended_client_quickbar_set_button_payload_hex\": \"701E0212000000050100010080FFFFFFFF0060\""
        ));
        assert!(json.contains("\"recommended_client_quickbar_set_button_slot\": 5"));
        assert!(json.contains(
            "\"recommended_client_quickbar_set_button_slot_source\": \"first_blank_committed_slot\""
        ));
        assert!(json.contains("\"recommended_client_quickbar_set_button_button_type\": 1"));
        assert!(
            json.contains("\"recommended_client_quickbar_set_button_item_object_id\": 2147483904")
        );
        assert!(json.contains(
            "\"recommended_client_quickbar_set_button_item_object_id_hex\": \"0x80000100\""
        ));
        assert!(json.contains("\"recommended_client_quickbar_set_button_int_param\": -1"));
        assert!(
            json.contains("\"recommended_client_quickbar_set_button_has_target_object\": false")
        );
        assert!(json.contains(
            "\"recommended_client_action\": \"target_candidate_with_use_item_use_object_quickbar_set_button_or_gui_event_notify_probe\""
        ));
        assert!(json.contains("\"recommended_client_action_should_dispatch\": false"));
        assert!(json.contains(
            "\"recommended_client_action_suppressed_reason\": \"server_quickbar_response_before_first_client_action\""
        ));
        let matching_prior_state_json = QuickbarItemRefreshHarnessHint {
            event_breakdown: QuickbarItemRefreshEventBreakdown {
                server_to_client_events: 1,
                live_object_events: 1,
                ..QuickbarItemRefreshEventBreakdown::default()
            },
            event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown::default(),
            first_candidate_use_count_row: None,
            first_candidate_use_count_row_before_first_client_action: None,
            first_candidate_use_count_row_after_first_client_action: None,
            first_followup_event: Some(QuickbarItemRefreshEventKind::LiveObject),
            first_client_action: None,
            first_client_action_detail: None,
            first_event_after_client_action: None,
            action_outcome: QuickbarItemRefreshActionOutcome::AwaitingClientAction,
            followup_events_before_first_client_action: 0,
            ..hint
        }
        .to_json();
        assert!(
            matching_prior_state_json
                .contains("\"recommended_client_action_should_dispatch\": false")
        );
        assert!(matching_prior_state_json.contains(
            "\"recommended_client_action_suppressed_reason\": \"matching_quickbar_use_count_state\""
        ));
        let unbound_candidate_json = QuickbarItemRefreshHarnessHint {
            stream_probe: QuickbarStreamProbeSummary::default(),
            first_preserved_active_item_signature: None,
            first_preserved_active_item_slot: None,
            candidate_preserved_active_item_signature: None,
            candidate_preserved_active_item_slot: None,
            candidate_use_count_state: None,
            first_preserved_active_item_use_count_state: None,
            event_breakdown: QuickbarItemRefreshEventBreakdown::default(),
            event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown::default(),
            first_candidate_use_count_row: None,
            first_candidate_use_count_row_before_first_client_action: None,
            first_candidate_use_count_row_after_first_client_action: None,
            first_followup_event: None,
            first_client_action: None,
            first_client_action_detail: None,
            first_event_after_client_action: None,
            action_outcome: QuickbarItemRefreshActionOutcome::AwaitingClientAction,
            followup_events_before_first_client_action: 0,
            ..hint
        }
        .to_json();
        assert!(
            unbound_candidate_json.contains("\"recommended_client_action_should_dispatch\": false")
        );
        assert!(unbound_candidate_json.contains(
            "\"recommended_client_action_suppressed_reason\": \"candidate_not_preserved_active_item\""
        ));
        assert!(
            matching_prior_state_json
                .contains("\"pending_item_refresh_action_outcome\": \"awaiting_client_action\"")
        );
        assert!(
            matching_prior_state_json
                .contains("\"first_server_quickbar_item_use_count_candidate_row_known\": false")
        );
        assert!(json.contains("\"recommended_client_gui_event_notify_payload_available\": true"));
        assert!(
            json.contains(
                "\"recommended_client_gui_event_notify_payload_kind\": \"GuiEvent_Notify\""
            )
        );
        assert!(json.contains(
            "\"recommended_client_gui_event_notify_payload_hex\": \"7035011B000000110000000001008000000000000000000000000060\""
        ));
        assert!(json.contains("\"recommended_client_gui_event_notify_event_a\": 17"));
        assert!(json.contains("\"recommended_client_gui_event_notify_event_b\": 0"));
        assert!(json.contains("\"recommended_client_gui_event_notify_object_id\": 2147483904"));
        assert!(
            json.contains("\"recommended_client_gui_event_notify_object_id_hex\": \"0x80000100\"")
        );
        assert!(json.contains("\"recommended_client_gui_event_notify_has_vector\": true"));
        assert!(json.contains("\"recommended_client_use_object_payload_available\": true"));
        assert!(
            json.contains("\"recommended_client_use_object_payload_kind\": \"Input_UseObject\"")
        );
        assert!(json.contains(
            "\"recommended_client_use_object_payload_hex\": \"70060B0B00000000010080A0\""
        ));
        assert!(json.contains("\"recommended_client_use_object_object_id\": 2147483904"));
        assert!(json.contains("\"recommended_client_use_object_object_id_hex\": \"0x80000100\""));
        assert!(json.contains("\"recommended_client_use_object_mark_inventory_gui_state\": false"));
        assert!(json.contains("\"recommended_client_use_object_schedule_script_event\": false"));
        assert!(json.contains("\"pending_item_refresh_proof_class\": \"shared\""));
        assert!(json.contains("\"quickbar_item_use_count_state_rows\": 2"));
        assert!(json.contains("\"quickbar_item_use_count_updates_observed\": 2"));
        assert!(json.contains("\"candidate_quickbar_item_use_count_state_known\": true"));
        assert!(json.contains(
            "\"candidate_quickbar_item_use_count_state_slot_relation\": \"matches_preserved_active_item_slot\""
        ));
        assert!(json.contains(
            "\"candidate_quickbar_item_use_count_state_slot_matches_first_preserved_active_item\": true"
        ));
        assert!(json.contains("\"candidate_quickbar_item_use_count_state_slot\": 2"));
        assert!(json.contains("\"candidate_quickbar_item_use_count_state_button_type\": 1"));
        assert!(
            json.contains(
                "\"candidate_quickbar_item_use_count_state_object_id_hex\": \"0x80000100\""
            )
        );
        assert!(
            json.contains("\"candidate_quickbar_item_use_count_state_active_property_index\": 7")
        );
        assert!(json.contains("\"candidate_quickbar_item_use_count_state_use_count\": 4"));
        assert!(
            json.contains("\"first_preserved_active_item_quickbar_use_count_state_known\": true")
        );
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_slot_relation\": \"matches_preserved_active_item_slot\""
        ));
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_slot_matches_first_preserved_active_item\": true"
        ));
        assert!(json.contains("\"first_preserved_active_item_quickbar_use_count_state_slot\": 2"));
        assert!(
            json.contains(
                "\"first_preserved_active_item_quickbar_use_count_state_button_type\": 1"
            )
        );
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_object_id_hex\": \"0x80000100\""
        ));
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_active_property_index\": 7"
        ));
        assert!(
            json.contains("\"first_preserved_active_item_quickbar_use_count_state_use_count\": 4")
        );
        assert!(json.contains("\"server_to_client_events_since_pending_refresh\": 10"));
        assert!(json.contains("\"client_to_server_events_since_pending_refresh\": 1"));
        assert!(
            json.contains("\"server_quickbar_item_use_count_events_since_pending_refresh\": 2")
        );
        assert!(
            json.contains("\"server_quickbar_item_use_count_records_since_pending_refresh\": 3")
        );
        assert!(json.contains("\"server_quickbar_item_use_count_rows_since_pending_refresh\": 4"));
        assert!(json.contains(
            "\"server_quickbar_item_use_count_candidate_rows_since_pending_refresh\": 1"
        ));
        assert!(json.contains("\"server_active_item_property_events_since_pending_refresh\": 5"));
        assert!(
            json.contains("\"server_active_item_property_uses_events_since_pending_refresh\": 3")
        );
        assert!(
            json.contains("\"server_active_item_property_full_events_since_pending_refresh\": 2")
        );
        assert!(
            json.contains(
                "\"server_active_item_property_candidate_events_since_pending_refresh\": 1"
            )
        );
        assert!(json.contains(
            "\"server_active_item_property_candidate_uses_events_since_pending_refresh\": 1"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_full_events_since_pending_refresh\": 0"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_changed_use_count_rows_since_pending_refresh\": 2"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_full_property_rows_since_pending_refresh\": 0"
        ));
        assert!(json.contains(
            "\"pending_item_refresh_action_outcome\": \"server_quickbar_response_before_first_client_action\""
        ));
        assert!(json.contains(
            "\"pending_item_refresh_recommended_action_outcome\": \"server_quickbar_response_before_recommended_action\""
        ));
        assert!(json.contains(
            "\"pending_item_refresh_active_property_outcome\": \"candidate_client_action_no_active_property_response\""
        ));
        assert!(json.contains(
            "\"pending_item_refresh_server_quickbar_response_timing\": \"server_quickbar_response_before_first_client_action\""
        ));
        assert!(
            json.contains("\"first_server_quickbar_item_use_count_candidate_row_known\": true")
        );
        assert!(json.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_timing\": \"before_first_client_action\""
        ));
        assert!(json.contains("\"first_server_quickbar_item_use_count_candidate_row_slot\": 2"));
        assert!(json.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_object_id_hex\": \"0x80000100\""
        ));
        assert!(json.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_active_property_index\": 7"
        ));
        assert!(
            json.contains("\"first_server_quickbar_item_use_count_candidate_row_use_count\": 4")
        );
        assert!(json.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_slot_relation\": \"matches_preserved_active_item_slot\""
        ));
        assert!(json.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_slot_matches_first_preserved_active_item\": true"
        ));
        assert!(json.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_before_first_client_action_known\": true"
        ));
        assert!(json.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_after_first_client_action_known\": false"
        ));
        assert!(
            json.contains("\"first_client_action_timing\": \"delayed_after_pending_followup\"")
        );
        assert!(json.contains("\"followup_events_before_first_client_action\": 8"));
        assert!(json.contains("\"first_followup_event\": \"client_input_other\""));
        assert!(json.contains("\"first_client_action\": \"client_input_other\""));
        assert!(json.contains("\"first_client_action_use_item_known\": false"));
        assert!(json.contains("\"first_client_action_use_item_active_property_subtype\": 0"));
        assert!(
            json.contains("\"first_client_action_use_item_target_is_self_or_legacy_self\": false")
        );
        assert!(json.contains("\"first_client_action_matches_preserved_active_item\": true"));
        assert!(json.contains("\"first_client_action_match_class\": \"preserved_active_item\""));
        assert!(
            json.contains("\"first_client_action_matches_recommended_client_use_item\": false")
        );
        assert!(json.contains(
            "\"first_client_action_matches_recommended_client_use_item_first_property_subtype_low\": false"
        ));
        assert!(json.contains(
            "\"first_client_action_matches_recommended_client_quickbar_set_button\": false"
        ));
        assert!(json.contains(
            "\"first_client_action_matches_recommended_client_gui_event_notify\": false"
        ));
        assert!(
            json.contains("\"first_client_action_matches_recommended_client_use_object\": false")
        );
        assert!(json.contains("\"first_event_after_client_action\": \"live_object\""));
        assert!(json.contains("\"events_after_first_client_action\": 2"));
        assert!(json.contains("\"server_to_client_events_after_first_client_action\": 1"));
        assert!(json.contains("\"client_to_server_events_after_first_client_action\": 1"));
        assert!(json.contains("\"live_object_events_after_first_client_action\": 1"));
        assert!(
            json.contains("\"server_quickbar_item_use_count_events_after_first_client_action\": 0")
        );
        assert!(
            json.contains(
                "\"server_quickbar_item_use_count_records_after_first_client_action\": 0"
            )
        );
        assert!(
            json.contains("\"server_quickbar_item_use_count_rows_after_first_client_action\": 0")
        );
        assert!(json.contains(
            "\"server_quickbar_item_use_count_candidate_rows_after_first_client_action\": 0"
        ));
        assert!(
            json.contains("\"server_active_item_property_events_after_first_client_action\": 0")
        );
        assert!(json.contains(
            "\"server_active_item_property_candidate_events_after_first_client_action\": 0"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_uses_events_after_first_client_action\": 0"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_full_events_after_first_client_action\": 0"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_changed_use_count_rows_after_first_client_action\": 0"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_full_property_rows_after_first_client_action\": 0"
        ));
        assert!(json.contains("\"other_events_after_first_client_action\": 1"));
        assert!(json.contains("\"quickbar_events_before_first_client_action\": 0"));
        assert!(
            json.contains(
                "\"server_quickbar_item_use_count_events_before_first_client_action\": 2"
            )
        );
        assert!(
            json.contains(
                "\"server_quickbar_item_use_count_records_before_first_client_action\": 3"
            )
        );
        assert!(
            json.contains("\"server_quickbar_item_use_count_rows_before_first_client_action\": 4")
        );
        assert!(json.contains(
            "\"server_quickbar_item_use_count_candidate_rows_before_first_client_action\": 1"
        ));
        assert!(
            json.contains("\"server_active_item_property_events_before_first_client_action\": 5")
        );
        assert!(json.contains(
            "\"server_active_item_property_candidate_uses_events_before_first_client_action\": 1"
        ));
        assert!(json.contains(
            "\"server_active_item_property_candidate_full_events_before_first_client_action\": 0"
        ));

        let active_property_response_json = QuickbarItemRefreshHarnessHint {
            event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown {
                server_active_item_property_candidate_uses_events: 1,
                server_active_item_property_candidate_full_events: 1,
                server_active_item_property_candidate_changed_use_count_rows: 2,
                server_active_item_property_candidate_full_property_rows: 1,
                ..QuickbarItemRefreshEventBreakdown::default()
            },
            ..hint
        }
        .to_json();
        assert!(active_property_response_json.contains(
            "\"pending_item_refresh_active_property_outcome\": \"candidate_client_action_observed_active_property_uses_and_full_refresh\""
        ));
        assert!(active_property_response_json.contains(
            "\"server_active_item_property_candidate_changed_use_count_rows_after_first_client_action\": 2"
        ));
        assert!(active_property_response_json.contains(
            "\"server_active_item_property_candidate_full_property_rows_after_first_client_action\": 1"
        ));

        let quickbar_after_action_json = QuickbarItemRefreshHarnessHint {
            event_breakdown: QuickbarItemRefreshEventBreakdown {
                server_quickbar_item_use_count_events: 1,
                ..QuickbarItemRefreshEventBreakdown::default()
            },
            event_breakdown_after_first_client_action: QuickbarItemRefreshEventBreakdown {
                server_quickbar_item_use_count_events: 1,
                ..QuickbarItemRefreshEventBreakdown::default()
            },
            ..hint
        }
        .to_json();
        assert!(quickbar_after_action_json.contains(
            "\"pending_item_refresh_server_quickbar_response_timing\": \"server_quickbar_response_after_first_client_action\""
        ));
        assert!(
            quickbar_after_action_json.contains(
                "\"server_quickbar_item_use_count_events_before_first_client_action\": 0"
            )
        );

        ui.post_committed_quickbar_item_refresh_first_client_action =
            Some(QuickbarItemRefreshEventKind::ClientInputUseItem);
        ui.post_committed_quickbar_item_refresh_first_client_action_detail =
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientInputUseItem,
                object_id: Some(0x8000_0100),
                slot: None,
                button_type: None,
                body_kind: None,
                gui_event_a: None,
                gui_event_b: None,
                gui_event_declared_bytes: None,
                gui_event_trailing_fragment_bytes: None,
                gui_event_has_vector: None,
                gui_event_vector_bits: None,
                use_item_active_property_subtype: Some(0),
                use_item_has_optional_byte: Some(false),
                use_item_has_target_object: Some(true),
                use_item_target_object_id: Some(client_input::EE_SELF_OBJECT_ID),
                use_item_has_position: Some(false),
                use_object_mark_inventory_gui_state: None,
                use_object_schedule_script_event: None,
                candidate_object_id: Some(0x8000_0100),
                matches_candidate_object: Some(true),
            });
        let use_item_json = ui
            .quickbar_item_refresh_harness_hint()
            .expect("pending candidate should still expose a harness hint")
            .to_json();
        assert!(use_item_json.contains("\"first_client_action\": \"client_input_use_item\""));
        assert!(use_item_json.contains("\"first_client_action_use_item_known\": true"));
        assert!(
            use_item_json.contains("\"first_client_action_use_item_active_property_subtype\": 0")
        );
        assert!(
            use_item_json.contains("\"first_client_action_use_item_has_optional_byte\": false")
        );
        assert!(use_item_json.contains("\"first_client_action_use_item_has_target_object\": true"));
        assert!(
            use_item_json
                .contains("\"first_client_action_use_item_target_object_id_hex\": \"0xFFFFFFFD\"")
        );
        assert!(
            use_item_json
                .contains("\"first_client_action_use_item_target_is_self_or_legacy_self\": true")
        );
        assert!(use_item_json.contains("\"first_client_action_use_item_has_position\": false"));
        assert!(
            use_item_json.contains("\"first_client_action_match_class\": \"recommended_use_item\"")
        );
        assert!(use_item_json.contains(
            "\"pending_item_refresh_recommended_action_outcome\": \"server_quickbar_response_before_recommended_action\""
        ));
        assert!(
            use_item_json
                .contains("\"first_client_action_matches_recommended_client_use_item\": true")
        );
        assert!(use_item_json.contains(
            "\"first_client_action_matches_recommended_client_use_item_first_property_subtype_low\": false"
        ));
        assert!(use_item_json.contains(
            "\"first_client_action_matches_recommended_client_quickbar_set_button\": false"
        ));

        ui.post_committed_quickbar_item_refresh_first_client_action =
            Some(QuickbarItemRefreshEventKind::ClientInputUseItem);
        ui.post_committed_quickbar_item_refresh_first_client_action_detail =
            Some(QuickbarItemRefreshClientActionDetail {
                use_item_active_property_subtype: Some(2),
                ..ui.post_committed_quickbar_item_refresh_first_client_action_detail
                    .expect("UseItem detail should still be installed")
            });
        let use_item_subtype_low_json = ui
            .quickbar_item_refresh_harness_hint()
            .expect("pending candidate should still expose a harness hint")
            .to_json();
        assert!(use_item_subtype_low_json.contains(
            "\"first_client_action_match_class\": \"recommended_use_item_first_property_subtype_low\""
        ));
        assert!(use_item_subtype_low_json.contains(
            "\"pending_item_refresh_recommended_action_outcome\": \"server_quickbar_response_before_recommended_action\""
        ));
        assert!(
            use_item_subtype_low_json
                .contains("\"first_client_action_matches_recommended_client_use_item\": false")
        );
        assert!(use_item_subtype_low_json.contains(
            "\"first_client_action_matches_recommended_client_use_item_first_property_subtype_low\": true"
        ));

        ui.post_committed_quickbar_item_refresh_first_client_action =
            Some(QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton);
        ui.post_committed_quickbar_item_refresh_first_client_action_detail =
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton,
                object_id: Some(0x8000_0100),
                slot: Some(5),
                button_type: Some(client_quickbar::ITEM_SET_BUTTON_TYPE),
                body_kind: Some(ClientQuickbarSetButtonKind::Item),
                gui_event_a: None,
                gui_event_b: None,
                gui_event_declared_bytes: None,
                gui_event_trailing_fragment_bytes: None,
                gui_event_has_vector: None,
                gui_event_vector_bits: None,
                use_item_active_property_subtype: None,
                use_item_has_optional_byte: None,
                use_item_has_target_object: None,
                use_item_target_object_id: None,
                use_item_has_position: None,
                use_object_mark_inventory_gui_state: None,
                use_object_schedule_script_event: None,
                candidate_object_id: Some(0x8000_0100),
                matches_candidate_object: Some(true),
            });
        let set_button_json = ui
            .quickbar_item_refresh_harness_hint()
            .expect("pending candidate should still expose a harness hint")
            .to_json();
        assert!(set_button_json.contains(
            "\"first_client_action_matches_recommended_client_quickbar_set_button\": true"
        ));
        assert!(
            set_button_json.contains("\"first_client_action_matches_preserved_active_item\": true")
        );
        assert!(
            set_button_json
                .contains("\"first_client_action_match_class\": \"recommended_set_button\"")
        );
        assert!(set_button_json.contains(
            "\"pending_item_refresh_recommended_action_outcome\": \"server_quickbar_response_before_recommended_action\""
        ));
        assert!(set_button_json.contains(
            "\"first_client_action_matches_recommended_client_gui_event_notify\": false"
        ));
        assert!(
            set_button_json
                .contains("\"first_client_action_matches_recommended_client_use_object\": false")
        );

        ui.post_committed_quickbar_item_refresh_first_client_action =
            Some(QuickbarItemRefreshEventKind::ClientGuiEventNotify);
        ui.post_committed_quickbar_item_refresh_first_client_action_detail =
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientGuiEventNotify,
                object_id: Some(0x8000_0100),
                slot: None,
                button_type: None,
                body_kind: None,
                gui_event_a: Some(client_gui_event::RADIAL_NOTIFY_PROBE_EVENT_A),
                gui_event_b: Some(client_gui_event::RADIAL_NOTIFY_PROBE_EVENT_B),
                gui_event_declared_bytes: Some(27),
                gui_event_trailing_fragment_bytes: Some(1),
                gui_event_has_vector: Some(true),
                gui_event_vector_bits: Some([0, 0, 0]),
                use_item_active_property_subtype: None,
                use_item_has_optional_byte: None,
                use_item_has_target_object: None,
                use_item_target_object_id: None,
                use_item_has_position: None,
                use_object_mark_inventory_gui_state: None,
                use_object_schedule_script_event: None,
                candidate_object_id: Some(0x8000_0100),
                matches_candidate_object: Some(true),
            });
        let gui_json = ui
            .quickbar_item_refresh_harness_hint()
            .expect("pending candidate should still expose a harness hint")
            .to_json();
        assert!(gui_json.contains("\"first_client_action\": \"client_gui_event_notify\""));
        assert!(gui_json.contains("\"first_client_action_gui_event_known\": true"));
        assert!(gui_json.contains("\"first_client_action_gui_event_a\": 17"));
        assert!(gui_json.contains("\"first_client_action_gui_event_b\": 0"));
        assert!(gui_json.contains("\"first_client_action_gui_event_declared_bytes\": 27"));
        assert!(gui_json.contains("\"first_client_action_gui_event_trailing_fragment_bytes\": 1"));
        assert!(gui_json.contains("\"first_client_action_gui_event_has_vector\": true"));
        assert!(gui_json.contains("\"first_client_action_gui_event_vector_zero\": true"));
        assert!(
            gui_json
                .contains("\"first_client_action_gui_event_vector_x_bits_hex\": \"0x00000000\"")
        );
        assert!(gui_json.contains(
            "\"first_client_action_matches_recommended_client_quickbar_set_button\": false"
        ));
        assert!(gui_json.contains("\"first_client_action_matches_preserved_active_item\": true"));
        assert!(
            gui_json
                .contains("\"first_client_action_match_class\": \"recommended_gui_event_notify\"")
        );
        assert!(gui_json.contains(
            "\"pending_item_refresh_recommended_action_outcome\": \"server_quickbar_response_before_recommended_action\""
        ));
        assert!(
            gui_json.contains(
                "\"first_client_action_matches_recommended_client_gui_event_notify\": true"
            )
        );
        assert!(
            gui_json
                .contains("\"first_client_action_matches_recommended_client_use_object\": false")
        );

        ui.post_committed_quickbar_item_refresh_first_client_action =
            Some(QuickbarItemRefreshEventKind::ClientInputUseObject);
        ui.post_committed_quickbar_item_refresh_first_client_action_detail =
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientInputUseObject,
                object_id: Some(0x8000_0100),
                slot: None,
                button_type: None,
                body_kind: None,
                gui_event_a: None,
                gui_event_b: None,
                gui_event_declared_bytes: None,
                gui_event_trailing_fragment_bytes: None,
                gui_event_has_vector: None,
                gui_event_vector_bits: None,
                use_item_active_property_subtype: None,
                use_item_has_optional_byte: None,
                use_item_has_target_object: None,
                use_item_target_object_id: None,
                use_item_has_position: None,
                use_object_mark_inventory_gui_state: Some(false),
                use_object_schedule_script_event: Some(false),
                candidate_object_id: Some(0x8000_0100),
                matches_candidate_object: Some(true),
            });
        let use_object_json = ui
            .quickbar_item_refresh_harness_hint()
            .expect("pending candidate should still expose a harness hint")
            .to_json();
        assert!(use_object_json.contains("\"first_client_action\": \"client_input_use_object\""));
        assert!(
            use_object_json
                .contains("\"first_client_action_match_class\": \"recommended_use_object\"")
        );
        assert!(use_object_json.contains(
            "\"pending_item_refresh_recommended_action_outcome\": \"server_quickbar_response_before_recommended_action\""
        ));
        assert!(
            use_object_json
                .contains("\"first_client_action_matches_recommended_client_use_object\": true")
        );
    }

    #[test]
    fn quickbar_item_refresh_harness_hint_suppresses_candidate_mismatch_when_preserved_active_item_has_use_count_state()
     {
        let mut ui = UiState::default();
        let item_context = InventoryItemContextSummary {
            direct_item_proof_objects: 2,
            compact_item_emission_proof_objects: 2,
            compact_item_emission_candidate: Some(InventoryItemContextCandidate {
                object_id: 0x8000_0101,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            }),
            compact_item_emission_ready_objects: 2,
            compact_item_emission_ready_candidate: Some(InventoryItemContextCandidate {
                object_id: 0x8000_0101,
                proof: InventoryItemObjectProof::ActiveObject,
                source: InventoryItemContextCandidateSource::DirectOnly,
            }),
            compact_item_emission_direct_only_proof_objects: 2,
            ..InventoryItemContextSummary::default()
        };
        ui.last_committed_quickbar_profile =
            Some(crate::translate::quickbar::QuickbarValidatedSlotProfile {
                slot_records: 36,
                blank_slots: 34,
                item_slots: 2,
                first_blank_slot: Some(5),
                first_item_slot: Some(0),
                ..crate::translate::quickbar::QuickbarValidatedSlotProfile::default()
            });
        ui.last_inventory_item_context_after_committed_quickbar = Some(item_context);
        ui.inventory_item_context_after_committed_quickbar_updates = 4;
        ui.post_committed_quickbar_item_refresh_pending = true;
        ui.post_committed_quickbar_item_refresh_pending_events = 6;
        ui.post_committed_quickbar_item_refresh_pending_event_breakdown =
            QuickbarItemRefreshEventBreakdown {
                server_to_client_events: 5,
                live_object_events: 3,
                other_events: 2,
                ..QuickbarItemRefreshEventBreakdown::default()
            };
        ui.post_committed_quickbar_item_refresh_proof_class =
            Some(QuickbarItemRefreshProofClass::DirectOnly);
        ui.last_quickbar_stream_probe = Some(QuickbarStreamProbeSummary {
            slot_records_owned: 36,
            item_buttons_seen: 21,
            item_buttons_preserved: 21,
            item_objects_preserved_by_explicit_self_materialization: 21,
            first_preserved_active_item_signature: Some(QuickbarActiveItemSignature {
                object_id: 0x8000_0100,
                base_item: 0x34,
                appearance_type: 0,
                active_property_count: 1,
                first_property: Some(
                    crate::translate::quickbar::QuickbarActivePropertySignature {
                        property: 15,
                        subtype: 525,
                        cost_table_value: 0,
                        param: 0,
                    },
                ),
                has_armor_word: false,
                name_is_locstring: false,
                state_mask: 0x01,
                value_mask: 0xFF,
            }),
            first_preserved_active_item_slot: Some(0),
            ..QuickbarStreamProbeSummary::default()
        });
        let preserved_active_item_row = QuickbarItemRefreshUseCountRow {
            slot: 0,
            button_type: client_quickbar::ITEM_SET_BUTTON_TYPE,
            object_id: 0x8000_0100,
            active_property_index: 255,
            use_count: 1,
        };
        ui.observe_quickbar_item_use_count_updates(&[LiveObjectQuickbarItemUseCountUpdate {
            slot: preserved_active_item_row.slot,
            button_type: preserved_active_item_row.button_type,
            object_id: preserved_active_item_row.object_id,
            active_property_index: preserved_active_item_row.active_property_index,
            use_count: preserved_active_item_row.use_count,
        }]);

        let hint = ui
            .quickbar_item_refresh_harness_hint()
            .expect("pending candidate should expose mismatch diagnostics");
        assert_eq!(hint.candidate.object_id, 0x8000_0101);
        assert_eq!(hint.candidate_use_count_state, None);
        assert_eq!(
            hint.first_preserved_active_item_use_count_state,
            Some(preserved_active_item_row)
        );

        let json = hint.to_json();
        assert!(json.contains("\"first_preserved_active_item_matches_candidate\": false"));
        assert!(json.contains("\"candidate_quickbar_item_use_count_state_known\": false"));
        assert!(json.contains(
            "\"candidate_quickbar_item_use_count_state_slot_relation\": \"no_candidate_use_count_row\""
        ));
        assert!(
            json.contains("\"first_preserved_active_item_quickbar_use_count_state_known\": true")
        );
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_slot_relation\": \"matches_preserved_active_item_slot\""
        ));
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_slot_matches_first_preserved_active_item\": true"
        ));
        assert!(json.contains("\"first_preserved_active_item_quickbar_use_count_state_slot\": 0"));
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_object_id_hex\": \"0x80000100\""
        ));
        assert!(json.contains(
            "\"first_preserved_active_item_quickbar_use_count_state_active_property_index\": 255"
        ));
        assert!(json.contains("\"recommended_client_action_should_dispatch\": false"));
        assert!(json.contains(
            "\"recommended_client_action_suppressed_reason\": \"preserved_active_item_quickbar_use_count_state_candidate_mismatch\""
        ));
        assert!(json.contains("\"recommended_use_item_item_object_id_hex\": \"0x80000101\""));
        assert!(json.contains(
            "\"recommended_client_quickbar_set_button_item_object_id_hex\": \"0x80000101\""
        ));
    }

    #[test]
    fn quickbar_item_refresh_idle_hint_serializes_absence_reason() {
        let mut ui = UiState::default();

        let initial = ui.quickbar_item_refresh_harness_idle_json();
        assert!(initial.contains("\"pending_item_refresh\": false"));
        assert!(
            initial.contains("\"post_committed_item_refresh_resolved_by_server_use_count\": false")
        );
        assert!(
            initial.contains(
                "\"post_committed_item_refresh_resolved_by_prior_use_count_state\": false"
            )
        );
        assert!(initial.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_slot_relation\": \"no_candidate_use_count_row\""
        ));
        assert!(initial.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_known\": false"
        ));
        assert!(initial.contains("\"no_hint_reason\": \"no_committed_quickbar_profile\""));
        assert!(initial.contains("\"committed_quickbar_seen\": false"));
        assert!(initial.contains("\"stream_probe_quickbar_seen\": false"));

        ui.quickbar_stream_probe_summaries = 2;
        ui.last_quickbar_stream_probe = Some(QuickbarStreamProbeSummary {
            slot_records_owned: 36,
            item_buttons_seen: 3,
            item_buttons_source_compact: 3,
            item_buttons_preserved: 0,
            item_buttons_blanked: 10,
            item_buttons_blanked_candidate: 7,
            item_buttons_rejected_missing_state_proof: 3,
            item_buttons_rejected_missing_state_unknown: 3,
            item_buttons_rejected_missing_state_cleared_delete: 1,
            item_buttons_rejected_missing_state_cleared_area_reset: 2,
            item_objects_rejected_missing_state_proven: 4,
            item_objects_rejected_missing_state_active: 5,
            item_objects_rejected_missing_state_feature25_first: 6,
            item_objects_rejected_missing_state_feature25_second: 7,
            item_objects_rejected_missing_state_feature25_legacy_tail: 8,
            item_objects_rejected_missing_state_unknown: 9,
            item_objects_rejected_missing_state_cleared_delete: 10,
            item_objects_rejected_missing_state_cleared_area_reset: 11,
            item_objects_preserved_by_explicit_self_materialization: 12,
            item_objects_preserved_by_active_state: 13,
            item_objects_preserved_by_feature25_first: 14,
            item_objects_preserved_by_feature25_second: 15,
            item_objects_preserved_by_feature25_legacy_tail: 16,
            first_preserved_active_item_signature: Some(QuickbarActiveItemSignature {
                object_id: 0x8000_0100,
                base_item: 0x11,
                appearance_type: 2,
                active_property_count: 1,
                first_property: Some(
                    crate::translate::quickbar::QuickbarActivePropertySignature {
                        property: 100,
                        subtype: 2,
                        cost_table_value: 3,
                        param: 4,
                    },
                ),
                has_armor_word: false,
                name_is_locstring: false,
                state_mask: 0x05,
                value_mask: 0x08,
            }),
            first_preserved_active_item_slot: Some(9),
            ..QuickbarStreamProbeSummary::default()
        });
        if let Some(probe) = ui.last_quickbar_stream_probe.as_mut() {
            probe.preserved_active_item_signatures.0[9] =
                probe.first_preserved_active_item_signature;
        }
        ui.last_quickbar_stream_probe_materialization_context = Some(InventoryItemContextSummary {
            direct_item_proof_objects: 1,
            ..Default::default()
        });
        let stream_probe_no_commit = ui.quickbar_item_refresh_harness_idle_json();
        assert!(stream_probe_no_commit.contains(
            "\"no_hint_reason\": \"stream_probe_quickbar_item_candidates_without_committed_profile\""
        ));
        assert!(stream_probe_no_commit.contains("\"stream_probe_quickbar_seen\": true"));
        assert!(stream_probe_no_commit.contains("\"stream_probe_quickbar_summaries\": 2"));
        assert!(stream_probe_no_commit.contains("\"stream_probe_item_buttons_seen\": 3"));
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_buttons_rejected_missing_state_proof\": 3")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_buttons_rejected_missing_state_cleared_delete\": 1")
        );
        assert!(stream_probe_no_commit.contains(
            "\"stream_probe_item_buttons_rejected_missing_state_cleared_area_reset\": 2"
        ));
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_objects_rejected_missing_state_proven\": 4")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_objects_rejected_missing_state_active\": 5")
        );
        assert!(
            stream_probe_no_commit.contains(
                "\"stream_probe_item_objects_rejected_missing_state_feature25_first\": 6"
            )
        );
        assert!(
            stream_probe_no_commit.contains(
                "\"stream_probe_item_objects_rejected_missing_state_feature25_second\": 7"
            )
        );
        assert!(stream_probe_no_commit.contains(
            "\"stream_probe_item_objects_rejected_missing_state_feature25_legacy_tail\": 8"
        ));
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_objects_rejected_missing_state_unknown\": 9")
        );
        assert!(
            stream_probe_no_commit.contains(
                "\"stream_probe_item_objects_rejected_missing_state_cleared_delete\": 10"
            )
        );
        assert!(stream_probe_no_commit.contains(
            "\"stream_probe_item_objects_rejected_missing_state_cleared_area_reset\": 11"
        ));
        assert!(stream_probe_no_commit.contains(
            "\"stream_probe_item_objects_preserved_by_explicit_self_materialization\": 12"
        ));
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_objects_preserved_by_active_state\": 13")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_preserved_active_item_signature_count\": 1")
        );
        assert!(
            stream_probe_no_commit.contains("\"stream_probe_preserved_active_item_slots\": [9]")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_objects_preserved_by_feature25_first\": 14")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_objects_preserved_by_feature25_second\": 15")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_item_objects_preserved_by_feature25_legacy_tail\": 16")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_first_preserved_active_item_known\": true")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_first_preserved_active_item_slot_known\": true")
        );
        assert!(
            stream_probe_no_commit.contains("\"stream_probe_first_preserved_active_item_slot\": 9")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_first_preserved_active_item_first_page_slot\": true")
        );
        assert!(stream_probe_no_commit.contains(
            "\"stream_probe_first_preserved_active_item_object_id_hex\": \"0x80000100\""
        ));
        assert!(stream_probe_no_commit.contains(
            "\"stream_probe_first_preserved_active_item_base_item_hex\": \"0x00000011\""
        ));
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_first_preserved_active_item_property_count\": 1")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_first_preserved_active_item_first_property\": 100")
        );
        assert!(
            stream_probe_no_commit
                .contains("\"stream_probe_first_preserved_active_item_state_mask_hex\": \"0x05\"")
        );
        assert!(stream_probe_no_commit.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_known\": false"
        ));
        assert!(stream_probe_no_commit.contains("\"stream_probe_direct_item_proof_objects\": 1"));

        ui.last_committed_quickbar_profile =
            Some(crate::translate::quickbar::QuickbarValidatedSlotProfile {
                slot_records: 36,
                blank_slots: 36,
                ..Default::default()
            });
        let no_post_context = ui.quickbar_item_refresh_harness_idle_json();
        assert!(no_post_context.contains("\"no_hint_reason\": \"no_post_committed_item_context\""));
        assert!(no_post_context.contains("\"committed_quickbar_seen\": true"));
        assert!(no_post_context.contains("\"post_committed_item_context_known\": false"));

        ui.last_inventory_item_context_after_committed_quickbar =
            Some(InventoryItemContextSummary {
                direct_item_proof_objects: 1,
                ..Default::default()
            });
        let no_compact_proof = ui.quickbar_item_refresh_harness_idle_json();
        assert!(
            no_compact_proof
                .contains("\"no_hint_reason\": \"post_context_without_compact_item_proof\"")
        );
        assert!(no_compact_proof.contains("\"direct_item_proof_objects\": 1"));

        ui.post_committed_quickbar_item_refresh_pending = true;
        ui.post_committed_quickbar_item_refresh_proof_class =
            Some(QuickbarItemRefreshProofClass::Feature25Only);
        ui.last_inventory_item_context_after_committed_quickbar =
            Some(InventoryItemContextSummary {
                feature25_item_proof_objects: 1,
                compact_item_emission_proof_objects: 1,
                compact_item_emission_feature25_only_proof_objects: 1,
                inventory_feature25_reference_records: 1,
                inventory_feature25_first_item_refs: 2,
                inventory_feature25_first_item_ref_mentions: 6,
                inventory_feature25_first_materialized_item_ref_mentions: 4,
                inventory_feature25_first_deferred_item_ref_mentions: 2,
                ..Default::default()
            });
        let no_candidate = ui.quickbar_item_refresh_harness_idle_json();
        assert!(no_candidate.contains("\"no_hint_reason\": \"pending_refresh_without_candidate\""));
        assert!(no_candidate.contains("\"post_committed_item_refresh_pending\": true"));
        assert!(no_candidate.contains("\"post_committed_item_refresh_resolution\": \"pending\""));
        assert!(no_candidate.contains("\"pending_item_refresh_proof_class\": \"feature25_only\""));
        assert!(no_candidate.contains("\"candidate_known\": false"));
        assert!(no_candidate.contains("\"compact_item_emission_proof_objects\": 1"));
        assert!(no_candidate.contains("\"inventory_feature25_reference_records\": 1"));
        assert!(no_candidate.contains("\"inventory_feature25_item_ref_mentions\": 6"));
        assert!(no_candidate.contains("\"inventory_feature25_materialized_item_ref_mentions\": 4"));
        assert!(no_candidate.contains("\"inventory_feature25_deferred_item_ref_mentions\": 2"));
        assert!(
            no_candidate
                .contains("\"inventory_feature25_materialization_outcome\": \"mixed_item_refs\"")
        );
        assert!(no_candidate.contains(
            "\"inventory_feature25_handoff_outcome\": \"mixed_item_refs_without_ready_item_state\""
        ));
        assert!(no_candidate.contains("\"inventory_equipment_handoff_ready\": false"));
        assert!(no_candidate.contains(
            "\"inventory_equipment_handoff_outcome\": \"feature25_refs_without_ready_item_state\""
        ));
        assert!(no_candidate.contains("\"inventory_feature25_first_item_refs\": 2"));
        assert!(no_candidate.contains("\"inventory_feature25_first_item_ref_mentions\": 6"));
        assert!(
            no_candidate
                .contains("\"inventory_feature25_first_materialized_item_ref_mentions\": 4")
        );
        assert!(
            no_candidate.contains("\"inventory_feature25_first_deferred_item_ref_mentions\": 2")
        );

        ui.post_committed_quickbar_item_refresh_pending = false;
        ui.post_committed_quickbar_item_refresh_resolved_by_server_use_count = true;
        ui.last_committed_quickbar_item_refresh_first_candidate_use_count_row =
            Some(QuickbarItemRefreshUseCountRow {
                slot: 9,
                button_type: 1,
                object_id: 0x8000_0100,
                active_property_index: 2,
                use_count: 3,
            });
        ui.last_committed_quickbar_item_refresh_first_candidate_use_count_row_before_first_client_action =
            ui.last_committed_quickbar_item_refresh_first_candidate_use_count_row;
        let resolved_by_use_count = ui.quickbar_item_refresh_harness_idle_json();
        assert!(resolved_by_use_count.contains("\"pending_item_refresh\": false"));
        assert!(resolved_by_use_count.contains(
            "\"no_hint_reason\": \"post_context_resolved_by_server_quickbar_use_count\""
        ));
        assert!(
            resolved_by_use_count
                .contains("\"post_committed_item_refresh_resolved_by_server_use_count\": true")
        );
        assert!(
            resolved_by_use_count.contains(
                "\"post_committed_item_refresh_resolved_by_prior_use_count_state\": false"
            )
        );
        assert!(resolved_by_use_count.contains(
            "\"post_committed_item_refresh_resolution\": \"resolved_by_server_quickbar_use_count\""
        ));
        assert!(
            resolved_by_use_count
                .contains("\"first_server_quickbar_item_use_count_candidate_row_known\": true")
        );
        assert!(resolved_by_use_count.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_timing\": \"before_first_client_action\""
        ));
        assert!(resolved_by_use_count.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_slot_relation\": \"matches_preserved_active_item_slot\""
        ));
        assert!(resolved_by_use_count.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_slot_matches_first_preserved_active_item\": true"
        ));
        assert!(
            resolved_by_use_count
                .contains("\"first_server_quickbar_item_use_count_candidate_row_slot\": 9")
        );
        assert!(resolved_by_use_count.contains(
            "\"first_server_quickbar_item_use_count_candidate_row_object_id_hex\": \"0x80000100\""
        ));

        ui.post_committed_quickbar_item_refresh_resolved_by_server_use_count = false;
        ui.post_committed_quickbar_item_refresh_resolved_by_prior_use_count_state = true;
        ui.last_inventory_item_context_after_committed_quickbar =
            Some(InventoryItemContextSummary {
                feature25_item_proof_objects: 1,
                compact_item_emission_proof_objects: 1,
                compact_item_emission_feature25_only_proof_objects: 1,
                compact_item_emission_candidate: Some(InventoryItemContextCandidate {
                    object_id: 0x8000_0100,
                    proof: InventoryItemObjectProof::Feature25SecondList,
                    source: InventoryItemContextCandidateSource::Feature25Only,
                }),
                ..Default::default()
            });
        ui.observe_quickbar_item_use_count_updates(&[LiveObjectQuickbarItemUseCountUpdate {
            slot: 9,
            button_type: 1,
            object_id: 0x8000_0100,
            active_property_index: 2,
            use_count: 3,
        }]);
        let resolved_by_prior_state = ui.quickbar_item_refresh_harness_idle_json();
        assert!(resolved_by_prior_state.contains(
            "\"no_hint_reason\": \"post_context_resolved_by_prior_quickbar_use_count_state\""
        ));
        assert!(
            resolved_by_prior_state
                .contains("\"post_committed_item_refresh_resolved_by_server_use_count\": false")
        );
        assert!(
            resolved_by_prior_state.contains(
                "\"post_committed_item_refresh_resolved_by_prior_use_count_state\": true"
            )
        );
        assert!(resolved_by_prior_state.contains(
            "\"post_committed_item_refresh_resolution\": \"resolved_by_prior_quickbar_use_count_state\""
        ));
        assert!(
            resolved_by_prior_state
                .contains("\"candidate_quickbar_item_use_count_state_known\": true")
        );
        assert!(resolved_by_prior_state.contains(
            "\"candidate_quickbar_item_use_count_state_slot_relation\": \"matches_preserved_active_item_slot\""
        ));
        assert!(resolved_by_prior_state.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_known\": true"
        ));
        assert!(resolved_by_prior_state.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_slot_relation\": \"matches_preserved_active_item_slot\""
        ));
        assert!(resolved_by_prior_state.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_slot_matches_first_preserved_active_item\": true"
        ));
        assert!(resolved_by_prior_state.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_slot\": 9"
        ));
        assert!(resolved_by_prior_state.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_object_id_hex\": \"0x80000100\""
        ));
        assert!(resolved_by_prior_state.contains(
            "\"stream_probe_first_preserved_active_item_quickbar_use_count_state_active_property_index\": 2"
        ));
    }

    #[test]
    fn verified_player_list_creature_id_establishes_session_alias() {
        let mut registry = ObjectRegistry::default();
        let session_creature_id = 0xFFFF_FFFE;

        registry.observe_player_list_object_ids(&[PlayerListObjectIds {
            player_object_id: session_creature_id,
            creature_object_id: Some(session_creature_id),
        }]);

        assert_eq!(
            registry.session_creature_id_for_compact(0xFE),
            Some(session_creature_id)
        );

        registry.reset_for_area();
        assert_eq!(
            registry.session_creature_id_for_compact(0xFE),
            Some(session_creature_id),
            "PlayerList session aliases survive area registry resets"
        );
    }

    #[test]
    fn inventory_owner_lifecycle_uses_active_object_id_without_type() {
        let mut registry = ObjectRegistry::default();
        let creature_id = 0xFFFF_FFFE;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(registry.has_active_live_object_for_record(0, creature_id));
        assert!(registry.has_active_live_object_for_record(0x05, creature_id));
    }

    #[test]
    fn inventory_owner_mention_does_not_retype_known_creature() {
        let mut registry = ObjectRegistry::default();
        let creature_id = 0xFFFF_FFFE;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'I',
            object_type: 0,
            object_id: creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let object = registry
            .known
            .get(&creature_id)
            .expect("known creature should remain registered after inventory owner mention");
        assert_eq!(
            object.object_type, 0x05,
            "inventory owner records carry no independent object type and must not retype the creature"
        );
        assert_eq!(object.last_opcode, b'I');
        assert_eq!(object.update_mentions, 1);
    }

    #[test]
    fn later_typed_live_object_promotes_inventory_only_owner_placeholder() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_1234;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'I',
            object_type: 0,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let object = registry
            .known
            .get(&object_id)
            .expect("typed add should promote the inventory-only placeholder");
        assert_eq!(object.object_type, 0x09);
        assert!(object.active);
        assert_eq!(object.add_mentions, 1);
    }
}
