//! Semantic state reducer.
//!
//! Packet-family translators produce and validate bytes. The reducer only
//! consumes the already-verified family proof plus the high-level payload that
//! will be emitted. If a future translator needs richer state, add a typed event
//! here rather than reaching back into transport or byte-rewrite modules.

use crate::{
    packet::{Direction, m::HighLevel},
    translate::{
        VerifiedFamily, VerifiedProof, area, client_input, client_quickbar, gameplay_stream,
        live_object_update, player_list, quickbar,
    },
};

use super::state::{
    InventoryItemContextCandidate, QuickbarItemRefreshClientActionDetail,
    QuickbarItemRefreshEventBreakdown, QuickbarItemRefreshEventKind, QuickbarItemRefreshProofClass,
};
use super::{
    AreaEvent, ChatEvent, ClientInputEvent, ClientQuickbarEvent, InventoryEvent,
    InventoryItemContextSummary, LiveObjectEvent, LiveObjectInventoryFeature25Reference,
    LiveObjectMention, LiveObjectOrientation, LiveObjectOrientationSource,
    LiveObjectOrientationVector, LiveObjectPlaceableState, LiveObjectPosition, LoginEvent,
    ModuleInfoEvent, ObservedHighLevel, PlayerListEvent, ProtocolEvent, QuickbarEvent,
    QuickbarItemContextSource, QuickbarItemRefreshOutcome, SemanticSessionState, ServerStatusEvent,
};

#[cfg(test)]
use super::InventoryItemObjectProof;

pub(crate) fn observe_verified_payload(
    state: &mut SemanticSessionState,
    direction: Direction,
    proof: &VerifiedProof,
    payload: &[u8],
) {
    observe_verified_payload_with_area_context(state, direction, proof, payload, None);
}

pub(crate) fn observe_verified_payload_with_area_context(
    state: &mut SemanticSessionState,
    direction: Direction,
    proof: &VerifiedProof,
    payload: &[u8],
    area_context: Option<&area::AreaPlaceableContext>,
) {
    match proof {
        VerifiedProof::Family(family) => {
            observe_family_payload(state, direction, *family, payload, area_context)
        }
        VerifiedProof::GameplayStream(families) => {
            observe_gameplay_stream_payload(state, direction, families, payload, area_context);
        }
        VerifiedProof::CoalescedWindow(_) => {
            let observed = observed_high_level(direction, VerifiedFamily::CoalescedWindow, payload);
            apply_event(state, ProtocolEvent::Other(observed), area_context);
        }
    }
}

fn observe_gameplay_stream_payload(
    state: &mut SemanticSessionState,
    direction: Direction,
    families: &[VerifiedFamily],
    payload: &[u8],
    area_context: Option<&area::AreaPlaceableContext>,
) {
    let split = gameplay_stream::split_inflated_gameplay(payload);
    let mut family_iter = families.iter().copied();
    for unit in split.units {
        if let gameplay_stream::GameplayUnit::HighLevel(message) = unit {
            let family = family_iter
                .next()
                .unwrap_or(VerifiedFamily::SemanticDeflated);
            observe_family_payload(state, direction, family, message.payload, area_context);
        }
    }

    for family in family_iter {
        let observed = observed_high_level(direction, family, payload);
        apply_event(state, ProtocolEvent::Other(observed), area_context);
    }
}

fn observe_family_payload(
    state: &mut SemanticSessionState,
    direction: Direction,
    family: VerifiedFamily,
    payload: &[u8],
    area_context: Option<&area::AreaPlaceableContext>,
) {
    let observed = observed_high_level(direction, family, payload);
    let event = match family {
        VerifiedFamily::ModuleInfo => ProtocolEvent::ModuleInfo(ModuleInfoEvent { observed }),
        VerifiedFamily::ServerStatusModuleResources => {
            ProtocolEvent::ServerStatus(ServerStatusEvent::ModuleResources { observed })
        }
        VerifiedFamily::ServerStatusStatus => {
            ProtocolEvent::ServerStatus(ServerStatusEvent::ModuleRunning { observed })
        }
        VerifiedFamily::AreaClientArea => ProtocolEvent::Area(AreaEvent::ClientArea {
            observed,
            area_object_id: current_area_object_id_from_payload(payload),
        }),
        VerifiedFamily::ClientArea => ProtocolEvent::Area(AreaEvent::AreaLoaded { observed }),
        VerifiedFamily::LoadBar => ProtocolEvent::Area(AreaEvent::LoadBar { observed }),
        VerifiedFamily::GameObjUpdateLiveObject => {
            // Populate object lifecycle facts only from the exact
            // `GameObjUpdate_LiveObject` parser. This preserves the strict
            // discipline from the EE/Diamond readers: no loose byte scans, no
            // packet-family inference without proven record boundaries.
            let (mentions, materialized_item_object_ids, inventory_feature25_references) =
                live_object_observations_from_payload(payload);
            ProtocolEvent::LiveObject(LiveObjectEvent {
                observed,
                mentions,
                materialized_item_object_ids,
                inventory_feature25_references,
            })
        }
        VerifiedFamily::PlayerList => {
            let object_ids =
                player_list::object_ids_from_verified_payload(payload).unwrap_or_else(|| {
                    tracing::warn!(
                        payload_len = payload.len(),
                        "verified PlayerList payload did not expose object-id facts"
                    );
                    Vec::new()
                });
            ProtocolEvent::PlayerList(PlayerListEvent {
                observed,
                object_ids,
            })
        }
        VerifiedFamily::GuiQuickbar => ProtocolEvent::Quickbar(QuickbarEvent::Verified {
            observed,
            profile: quickbar::validated_set_all_buttons_slot_profile(payload),
            materialization_context: state.objects.inventory_item_context_summary(),
        }),
        VerifiedFamily::GuiQuickbarPlaceholder => {
            ProtocolEvent::Quickbar(QuickbarEvent::Placeholder { observed })
        }
        VerifiedFamily::Inventory | VerifiedFamily::ClientGuiInventory => {
            ProtocolEvent::Inventory(InventoryEvent { observed })
        }
        VerifiedFamily::ClientInput => ProtocolEvent::ClientInput(ClientInputEvent {
            observed,
            claim: client_input::claim_payload_if_verified(payload),
        }),
        VerifiedFamily::ClientQuickbar => ProtocolEvent::ClientQuickbar(ClientQuickbarEvent {
            observed,
            claim: client_quickbar::claim_payload_if_verified(payload),
        }),
        VerifiedFamily::Login | VerifiedFamily::ClientLogin => {
            ProtocolEvent::Login(LoginEvent { observed })
        }
        VerifiedFamily::Chat => ProtocolEvent::Chat(ChatEvent { observed }),
        VerifiedFamily::ModuleTime => ProtocolEvent::Other(observed),
        VerifiedFamily::ServerZlibStreamContinuation { .. }
        | VerifiedFamily::ServerZlibZeroFillWindow { .. }
        | VerifiedFamily::CoalescedWindow
        | VerifiedFamily::ConsumedEmptyMFrame
        | VerifiedFamily::SemanticDeflated => ProtocolEvent::Other(observed),
        _ => ProtocolEvent::Other(observed),
    };
    apply_event(state, event, area_context);
}

fn apply_event(
    state: &mut SemanticSessionState,
    event: ProtocolEvent,
    area_context: Option<&area::AreaPlaceableContext>,
) {
    let pending_item_refresh_before_event = state.ui.post_committed_quickbar_item_refresh_pending;
    match &event {
        ProtocolEvent::ModuleInfo(event) => {
            state.resources.module_info_seen = true;
            state.module.module_info_packets = state.module.module_info_packets.saturating_add(1);
            state.module.last_module_info_declared_len = event.observed.declared_len;
        }
        ProtocolEvent::ServerStatus(ServerStatusEvent::ModuleResources { .. }) => {
            state.resources.module_resource_packets =
                state.resources.module_resource_packets.saturating_add(1);
        }
        ProtocolEvent::ServerStatus(ServerStatusEvent::ModuleRunning { .. }) => {
            state.resources.module_running_packets =
                state.resources.module_running_packets.saturating_add(1);
        }
        ProtocolEvent::Area(AreaEvent::ClientArea {
            observed,
            area_object_id,
        }) => {
            state.area.client_area_packets = state.area.client_area_packets.saturating_add(1);
            state.area.last_client_area_declared_len = observed.declared_len;
            state.area.current_area_object_id = *area_object_id;
            state.objects.reset_for_area();
            remember_quickbar_item_context_if_relevant(state, "area-reset");
        }
        ProtocolEvent::Area(AreaEvent::AreaLoaded { .. }) => {
            state.area.area_loaded_packets = state.area.area_loaded_packets.saturating_add(1);
        }
        ProtocolEvent::Area(AreaEvent::LoadBar { .. }) => {
            state.area.loadbar_packets = state.area.loadbar_packets.saturating_add(1);
        }
        ProtocolEvent::LiveObject(event) => {
            state.objects.observe_mentions(&event.mentions);
            if let Some(area_context) = area_context {
                state
                    .objects
                    .observe_placeable_area_context(area_context, &event.mentions);
            }
            state
                .objects
                .observe_materialized_item_object_ids(&event.materialized_item_object_ids);
            state
                .objects
                .observe_inventory_feature25_references(&event.inventory_feature25_references);
            remember_quickbar_item_context_if_relevant(state, "live-object");
        }
        ProtocolEvent::PlayerList(event) => {
            state
                .objects
                .observe_player_list_object_ids(&event.object_ids);
            if !event.object_ids.is_empty() {
                tracing::debug!(
                    entries = event.object_ids.len(),
                    "semantic state observed verified PlayerList object ids"
                );
            }
        }
        ProtocolEvent::Quickbar(QuickbarEvent::Verified {
            observed,
            profile,
            materialization_context,
        }) => {
            state.ui.quickbar_packets = state.ui.quickbar_packets.saturating_add(1);
            state.ui.last_quickbar_family = Some(observed.family);
            if let Some(profile) = profile {
                let prior_item_context = state.ui.last_inventory_item_context_before_quickbar;
                let previous_post_item_context = state
                    .ui
                    .last_inventory_item_context_after_committed_quickbar;
                let previous_post_item_context_updates = state
                    .ui
                    .inventory_item_context_after_committed_quickbar_updates;
                let pending_item_refresh = state.ui.post_committed_quickbar_item_refresh_pending;
                let pending_item_refresh_updates = state
                    .ui
                    .post_committed_quickbar_item_refresh_pending_updates;
                let pending_item_refresh_events =
                    state.ui.post_committed_quickbar_item_refresh_pending_events;
                let pending_item_refresh_event_breakdown = state
                    .ui
                    .post_committed_quickbar_item_refresh_pending_event_breakdown;
                let pending_item_refresh_events_after_first_client_action = state
                    .ui
                    .post_committed_quickbar_item_refresh_events_after_first_client_action;
                let pending_item_refresh_event_breakdown_after_first_client_action = state
                    .ui
                    .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action;
                let pending_item_refresh_proof_class =
                    state.ui.post_committed_quickbar_item_refresh_proof_class;
                let pending_item_refresh_first_followup_event = state
                    .ui
                    .post_committed_quickbar_item_refresh_first_followup_event;
                let pending_item_refresh_first_client_action = state
                    .ui
                    .post_committed_quickbar_item_refresh_first_client_action;
                let pending_item_refresh_first_client_action_detail = state
                    .ui
                    .post_committed_quickbar_item_refresh_first_client_action_detail;
                let pending_item_refresh_first_event_after_client_action = state
                    .ui
                    .post_committed_quickbar_item_refresh_first_event_after_client_action;
                let pending_item_refresh_outcome =
                    committed_quickbar_item_refresh_outcome(pending_item_refresh, profile);
                let (best_item_context, best_item_context_source) =
                    best_committed_quickbar_item_context(
                        *materialization_context,
                        prior_item_context,
                        previous_post_item_context,
                    );
                state.ui.last_committed_quickbar_profile = Some(*profile);
                state.ui.last_committed_quickbar_materialization_context =
                    Some(*materialization_context);
                state.ui.last_committed_quickbar_prior_item_context = prior_item_context;
                state.ui.last_committed_quickbar_previous_post_item_context =
                    previous_post_item_context;
                state
                    .ui
                    .last_committed_quickbar_previous_post_item_context_updates =
                    previous_post_item_context_updates;
                state.ui.last_committed_quickbar_item_refresh_pending = pending_item_refresh;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_pending_updates =
                    pending_item_refresh_updates;
                state.ui.last_committed_quickbar_item_refresh_pending_events =
                    pending_item_refresh_events;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_pending_event_breakdown =
                    pending_item_refresh_event_breakdown;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_events_after_first_client_action =
                    pending_item_refresh_events_after_first_client_action;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
                    pending_item_refresh_event_breakdown_after_first_client_action;
                state.ui.last_committed_quickbar_item_refresh_outcome =
                    pending_item_refresh_outcome;
                state.ui.last_committed_quickbar_item_refresh_proof_class =
                    pending_item_refresh_proof_class;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_first_followup_event =
                    pending_item_refresh_first_followup_event;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_first_client_action =
                    pending_item_refresh_first_client_action;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_first_client_action_detail =
                    pending_item_refresh_first_client_action_detail;
                state
                    .ui
                    .last_committed_quickbar_item_refresh_first_event_after_client_action =
                    pending_item_refresh_first_event_after_client_action;
                state.ui.last_committed_quickbar_best_item_context = best_item_context;
                state.ui.last_committed_quickbar_best_item_context_source =
                    best_item_context_source;
                state
                    .ui
                    .last_inventory_item_context_after_committed_quickbar = None;
                state
                    .ui
                    .inventory_item_context_after_committed_quickbar_updates = 0;
                state.ui.post_committed_quickbar_item_refresh_pending = false;
                state
                    .ui
                    .post_committed_quickbar_item_refresh_pending_updates = 0;
                state.ui.post_committed_quickbar_item_refresh_pending_events = 0;
                state
                    .ui
                    .post_committed_quickbar_item_refresh_pending_event_breakdown =
                    QuickbarItemRefreshEventBreakdown::default();
                state
                    .ui
                    .post_committed_quickbar_item_refresh_events_after_first_client_action = 0;
                state
                    .ui
                    .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
                    QuickbarItemRefreshEventBreakdown::default();
                state.ui.post_committed_quickbar_item_refresh_proof_class = None;
                state
                    .ui
                    .post_committed_quickbar_item_refresh_first_followup_event = None;
                state
                    .ui
                    .post_committed_quickbar_item_refresh_first_client_action = None;
                state
                    .ui
                    .post_committed_quickbar_item_refresh_first_client_action_detail = None;
                state
                    .ui
                    .post_committed_quickbar_item_refresh_first_event_after_client_action = None;
                let prior_item_context_known = prior_item_context.is_some();
                let prior_item_context = prior_item_context.unwrap_or_default();
                let previous_post_item_context_known = previous_post_item_context.is_some();
                let previous_post_item_context = previous_post_item_context.unwrap_or_default();
                let best_item_context_known = best_item_context.is_some();
                let best_item_context_source = best_item_context_source
                    .map(QuickbarItemContextSource::as_str)
                    .unwrap_or("none");
                let best_item_context = best_item_context.unwrap_or_default();
                let pending_item_refresh_proof_class = pending_item_refresh_proof_class
                    .map(QuickbarItemRefreshProofClass::as_str)
                    .unwrap_or("none");
                let pending_item_refresh_first_followup_event =
                    pending_item_refresh_first_followup_event
                        .map(QuickbarItemRefreshEventKind::as_str)
                        .unwrap_or("none");
                let pending_item_refresh_first_client_action =
                    pending_item_refresh_first_client_action
                        .map(QuickbarItemRefreshEventKind::as_str)
                        .unwrap_or("none");
                let pending_item_refresh_first_event_after_client_action =
                    pending_item_refresh_first_event_after_client_action
                        .map(QuickbarItemRefreshEventKind::as_str)
                        .unwrap_or("none");
                let (
                    pending_item_refresh_first_client_action_has_object_id,
                    pending_item_refresh_first_client_action_object_id,
                    pending_item_refresh_first_client_action_slot,
                    pending_item_refresh_first_client_action_button_type,
                    pending_item_refresh_first_client_action_body_kind,
                    pending_item_refresh_first_client_action_candidate_known,
                    pending_item_refresh_first_client_action_candidate_object_id,
                    pending_item_refresh_first_client_action_matches_candidate,
                ) = quickbar_item_refresh_client_action_trace_fields(
                    pending_item_refresh_first_client_action_detail,
                );
                let (
                    prior_compact_item_emission_candidate_known,
                    prior_compact_item_emission_candidate_object_id,
                    prior_compact_item_emission_candidate_proof,
                    prior_compact_item_emission_candidate_source,
                ) = quickbar_item_context_candidate_trace_fields(
                    prior_item_context.compact_item_emission_candidate,
                );
                let (
                    previous_post_compact_item_emission_candidate_known,
                    previous_post_compact_item_emission_candidate_object_id,
                    previous_post_compact_item_emission_candidate_proof,
                    previous_post_compact_item_emission_candidate_source,
                ) = quickbar_item_context_candidate_trace_fields(
                    previous_post_item_context.compact_item_emission_candidate,
                );
                let (
                    pending_item_refresh_candidate_known_before_commit,
                    pending_item_refresh_candidate_object_id_before_commit,
                    pending_item_refresh_candidate_proof_before_commit,
                    pending_item_refresh_candidate_source_before_commit,
                ) = quickbar_item_context_candidate_trace_fields(if pending_item_refresh {
                    previous_post_item_context.compact_item_emission_candidate
                } else {
                    None
                });
                let (
                    best_compact_item_emission_candidate_known,
                    best_compact_item_emission_candidate_object_id,
                    best_compact_item_emission_candidate_proof,
                    best_compact_item_emission_candidate_source,
                ) = quickbar_item_context_candidate_trace_fields(
                    best_item_context.compact_item_emission_candidate,
                );
                tracing::info!(
                    slot_records = profile.slot_records,
                    blank_slots = profile.blank_slots,
                    item_slots = profile.item_slots,
                    spell_slots = profile.spell_slots,
                    general_slots = profile.general_slots,
                    first_page_visible_slots = profile.first_page_visible_slots,
                    first_page_item_slots = profile.first_page_item_slots,
                    first_page_spell_slots = profile.first_page_spell_slots,
                    active_item_objects = materialization_context.active_item_objects,
                    direct_item_proof_objects = materialization_context.direct_item_proof_objects,
                    feature25_item_proof_objects =
                        materialization_context.feature25_item_proof_objects,
                    compact_item_emission_proof_objects =
                        materialization_context.compact_item_emission_proof_objects,
                    compact_item_emission_direct_only_proof_objects =
                        materialization_context.compact_item_emission_direct_only_proof_objects,
                    compact_item_emission_feature25_only_proof_objects =
                        materialization_context.compact_item_emission_feature25_only_proof_objects,
                    compact_item_emission_shared_proof_objects =
                        materialization_context.compact_item_emission_shared_proof_objects,
                    inventory_feature25_first_item_refs =
                        materialization_context.inventory_feature25_first_item_refs,
                    inventory_feature25_second_item_refs =
                        materialization_context.inventory_feature25_second_item_refs,
                    prior_item_context_known,
                    prior_direct_item_proof_objects = prior_item_context.direct_item_proof_objects,
                    prior_feature25_item_proof_objects =
                        prior_item_context.feature25_item_proof_objects,
                    prior_compact_item_emission_proof_objects =
                        prior_item_context.compact_item_emission_proof_objects,
                    prior_compact_item_emission_candidate_known,
                    prior_compact_item_emission_candidate_object_id,
                    prior_compact_item_emission_candidate_proof,
                    prior_compact_item_emission_candidate_source,
                    prior_compact_item_emission_direct_only_proof_objects =
                        prior_item_context.compact_item_emission_direct_only_proof_objects,
                    prior_compact_item_emission_feature25_only_proof_objects =
                        prior_item_context.compact_item_emission_feature25_only_proof_objects,
                    prior_compact_item_emission_shared_proof_objects =
                        prior_item_context.compact_item_emission_shared_proof_objects,
                    prior_inventory_feature25_first_item_refs =
                        prior_item_context.inventory_feature25_first_item_refs,
                    prior_inventory_feature25_second_item_refs =
                        prior_item_context.inventory_feature25_second_item_refs,
                    prior_inventory_feature25_legacy_tail_item_refs =
                        prior_item_context.inventory_feature25_legacy_tail_item_refs,
                    prior_cleared_inventory_item_object_ids =
                        prior_item_context.cleared_inventory_item_object_ids,
                    previous_post_item_context_known,
                    previous_post_context_updates = previous_post_item_context_updates,
                    previous_post_direct_item_proof_objects =
                        previous_post_item_context.direct_item_proof_objects,
                    previous_post_feature25_item_proof_objects =
                        previous_post_item_context.feature25_item_proof_objects,
                    previous_post_compact_item_emission_proof_objects =
                        previous_post_item_context.compact_item_emission_proof_objects,
                    previous_post_compact_item_emission_candidate_known,
                    previous_post_compact_item_emission_candidate_object_id,
                    previous_post_compact_item_emission_candidate_proof,
                    previous_post_compact_item_emission_candidate_source,
                    previous_post_compact_item_emission_direct_only_proof_objects =
                        previous_post_item_context.compact_item_emission_direct_only_proof_objects,
                    previous_post_compact_item_emission_feature25_only_proof_objects =
                        previous_post_item_context
                            .compact_item_emission_feature25_only_proof_objects,
                    previous_post_compact_item_emission_shared_proof_objects =
                        previous_post_item_context.compact_item_emission_shared_proof_objects,
                    previous_post_inventory_feature25_first_item_refs =
                        previous_post_item_context.inventory_feature25_first_item_refs,
                    previous_post_inventory_feature25_second_item_refs =
                        previous_post_item_context.inventory_feature25_second_item_refs,
                    previous_post_inventory_feature25_legacy_tail_item_refs =
                        previous_post_item_context.inventory_feature25_legacy_tail_item_refs,
                    previous_post_cleared_inventory_item_object_ids =
                        previous_post_item_context.cleared_inventory_item_object_ids,
                    pending_item_refresh_before_commit = pending_item_refresh,
                    pending_item_refresh_updates_before_commit = pending_item_refresh_updates,
                    pending_item_refresh_events_before_commit = pending_item_refresh_events,
                    pending_item_refresh_live_object_events_before_commit =
                        pending_item_refresh_event_breakdown.live_object_events,
                    pending_item_refresh_quickbar_events_before_commit =
                        pending_item_refresh_event_breakdown.quickbar_events,
                    pending_item_refresh_area_events_before_commit =
                        pending_item_refresh_event_breakdown.area_events,
                    pending_item_refresh_inventory_events_before_commit =
                        pending_item_refresh_event_breakdown.inventory_events,
                    pending_item_refresh_client_input_events_before_commit =
                        pending_item_refresh_event_breakdown.client_input_events,
                    pending_item_refresh_client_input_use_item_events_before_commit =
                        pending_item_refresh_event_breakdown.client_input_use_item_events,
                    pending_item_refresh_client_input_use_object_events_before_commit =
                        pending_item_refresh_event_breakdown.client_input_use_object_events,
                    pending_item_refresh_client_input_change_door_state_events_before_commit =
                        pending_item_refresh_event_breakdown.client_input_change_door_state_events,
                    pending_item_refresh_client_input_other_events_before_commit =
                        pending_item_refresh_event_breakdown.client_input_other_events,
                    pending_item_refresh_client_quickbar_events_before_commit =
                        pending_item_refresh_event_breakdown.client_quickbar_events,
                    pending_item_refresh_client_quickbar_item_set_button_events_before_commit =
                        pending_item_refresh_event_breakdown.client_quickbar_item_set_button_events,
                    pending_item_refresh_client_quickbar_other_set_button_events_before_commit =
                        pending_item_refresh_event_breakdown
                            .client_quickbar_other_set_button_events,
                    pending_item_refresh_chat_events_before_commit =
                        pending_item_refresh_event_breakdown.chat_events,
                    pending_item_refresh_other_events_before_commit =
                        pending_item_refresh_event_breakdown.other_events,
                    pending_item_refresh_events_after_first_client_action_before_commit =
                        pending_item_refresh_events_after_first_client_action,
                    pending_item_refresh_live_object_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .live_object_events,
                    pending_item_refresh_quickbar_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .quickbar_events,
                    pending_item_refresh_area_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action.area_events,
                    pending_item_refresh_inventory_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .inventory_events,
                    pending_item_refresh_client_input_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_input_events,
                    pending_item_refresh_client_input_use_item_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_input_use_item_events,
                    pending_item_refresh_client_input_use_object_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_input_use_object_events,
                    pending_item_refresh_client_input_change_door_state_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_input_change_door_state_events,
                    pending_item_refresh_client_input_other_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_input_other_events,
                    pending_item_refresh_client_quickbar_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_quickbar_events,
                    pending_item_refresh_client_quickbar_item_set_button_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_quickbar_item_set_button_events,
                    pending_item_refresh_client_quickbar_other_set_button_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action
                            .client_quickbar_other_set_button_events,
                    pending_item_refresh_chat_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action.chat_events,
                    pending_item_refresh_other_events_after_first_client_action_before_commit =
                        pending_item_refresh_event_breakdown_after_first_client_action.other_events,
                    pending_item_refresh_proof_class,
                    pending_item_refresh_first_followup_event,
                    pending_item_refresh_first_client_action,
                    pending_item_refresh_first_client_action_has_object_id,
                    pending_item_refresh_first_client_action_object_id,
                    pending_item_refresh_first_client_action_slot,
                    pending_item_refresh_first_client_action_button_type,
                    pending_item_refresh_first_client_action_body_kind,
                    pending_item_refresh_first_client_action_candidate_known,
                    pending_item_refresh_first_client_action_candidate_object_id,
                    pending_item_refresh_first_client_action_matches_candidate,
                    pending_item_refresh_first_event_after_client_action,
                    pending_item_refresh_candidate_known_before_commit,
                    pending_item_refresh_candidate_object_id_before_commit,
                    pending_item_refresh_candidate_proof_before_commit,
                    pending_item_refresh_candidate_source_before_commit,
                    pending_item_refresh_outcome = pending_item_refresh_outcome.as_str(),
                    best_item_context_known,
                    best_item_context_source,
                    best_direct_item_proof_objects = best_item_context.direct_item_proof_objects,
                    best_feature25_item_proof_objects =
                        best_item_context.feature25_item_proof_objects,
                    best_compact_item_emission_proof_objects =
                        best_item_context.compact_item_emission_proof_objects,
                    best_compact_item_emission_candidate_known,
                    best_compact_item_emission_candidate_object_id,
                    best_compact_item_emission_candidate_proof,
                    best_compact_item_emission_candidate_source,
                    best_compact_item_emission_direct_only_proof_objects =
                        best_item_context.compact_item_emission_direct_only_proof_objects,
                    best_compact_item_emission_feature25_only_proof_objects =
                        best_item_context.compact_item_emission_feature25_only_proof_objects,
                    best_compact_item_emission_shared_proof_objects =
                        best_item_context.compact_item_emission_shared_proof_objects,
                    best_inventory_feature25_first_item_refs =
                        best_item_context.inventory_feature25_first_item_refs,
                    best_inventory_feature25_second_item_refs =
                        best_item_context.inventory_feature25_second_item_refs,
                    best_inventory_feature25_legacy_tail_item_refs =
                        best_item_context.inventory_feature25_legacy_tail_item_refs,
                    best_cleared_inventory_item_object_ids =
                        best_item_context.cleared_inventory_item_object_ids,
                    "semantic state observed committed GuiQuickbar slot profile"
                );
            } else {
                tracing::warn!(
                    payload_len = observed.payload_len,
                    declared_len = observed.declared_len,
                    "verified GuiQuickbar payload did not expose an exact EE slot profile"
                );
            }
        }
        ProtocolEvent::Quickbar(QuickbarEvent::Placeholder { observed }) => {
            state.ui.quickbar_packets = state.ui.quickbar_packets.saturating_add(1);
            state.ui.quickbar_placeholders = state.ui.quickbar_placeholders.saturating_add(1);
            state.ui.last_quickbar_family = Some(observed.family);
        }
        ProtocolEvent::Inventory(_) => {
            state.ui.inventory_packets = state.ui.inventory_packets.saturating_add(1);
        }
        ProtocolEvent::ClientInput(event) => {
            state.auth.client_input_packets = state.auth.client_input_packets.saturating_add(1);
            if let Some(claim) = event.claim {
                tracing::debug!(
                    kind = ?claim.kind,
                    packet_name = claim.packet_name,
                    object_id = %format_args!("0x{:08X}", claim.primary_object_id),
                    rewritten_self_object_id = claim.rewritten_self_object_id,
                    rewritten_transition_door_close = claim.rewritten_transition_door_close,
                    "semantic state observed verified client Input action"
                );
            } else {
                tracing::warn!(
                    payload_len = event.observed.payload_len,
                    declared_len = event.observed.declared_len,
                    "verified ClientInput payload did not expose an exact action claim"
                );
            }
        }
        ProtocolEvent::ClientQuickbar(event) => {
            state.ui.client_quickbar_packets = state.ui.client_quickbar_packets.saturating_add(1);
            if let Some(claim) = event.claim {
                tracing::debug!(
                    slot = claim.slot,
                    button_type = claim.button_type,
                    body_kind = ?claim.body_kind,
                    "semantic state observed verified client GuiQuickbar_SetButton action"
                );
            } else {
                tracing::warn!(
                    payload_len = event.observed.payload_len,
                    declared_len = event.observed.declared_len,
                    "verified ClientQuickbar payload did not expose an exact SetButton claim"
                );
            }
        }
        ProtocolEvent::Login(_) => {
            state.auth.login_packets = state.auth.login_packets.saturating_add(1);
        }
        ProtocolEvent::Chat(_) | ProtocolEvent::Other(_) => {}
    }
    record_pending_quickbar_item_refresh_event(state, &event, pending_item_refresh_before_event);
    state.remember_event(event);
}

fn best_committed_quickbar_item_context(
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

fn committed_quickbar_item_refresh_outcome(
    pending_item_refresh: bool,
    profile: &quickbar::QuickbarValidatedSlotProfile,
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

fn quickbar_item_refresh_proof_class(
    item_context: InventoryItemContextSummary,
) -> Option<QuickbarItemRefreshProofClass> {
    if !item_context.has_compact_quickbar_item_proof() {
        return None;
    }

    let direct_only = item_context.compact_item_emission_direct_only_proof_objects != 0;
    let feature25_only = item_context.compact_item_emission_feature25_only_proof_objects != 0;
    let shared = item_context.compact_item_emission_shared_proof_objects != 0;
    match (direct_only, feature25_only, shared) {
        (true, false, false) => Some(QuickbarItemRefreshProofClass::DirectOnly),
        (false, true, false) => Some(QuickbarItemRefreshProofClass::Feature25Only),
        (false, false, true) => Some(QuickbarItemRefreshProofClass::Shared),
        _ => Some(QuickbarItemRefreshProofClass::Mixed),
    }
}

fn remember_quickbar_item_context_if_relevant(
    state: &mut SemanticSessionState,
    source: &'static str,
) {
    let item_context = state.objects.inventory_item_context_summary();
    if !item_context.has_quickbar_item_context_evidence() {
        return;
    }

    if state.ui.last_inventory_item_context_before_quickbar != Some(item_context) {
        state.ui.last_inventory_item_context_before_quickbar = Some(item_context);
        tracing::debug!(
            source,
            direct_item_proof_objects = item_context.direct_item_proof_objects,
            feature25_item_proof_objects = item_context.feature25_item_proof_objects,
            compact_item_emission_proof_objects = item_context.compact_item_emission_proof_objects,
            compact_item_emission_direct_only_proof_objects =
                item_context.compact_item_emission_direct_only_proof_objects,
            compact_item_emission_feature25_only_proof_objects =
                item_context.compact_item_emission_feature25_only_proof_objects,
            compact_item_emission_shared_proof_objects =
                item_context.compact_item_emission_shared_proof_objects,
            inventory_feature25_first_item_refs = item_context.inventory_feature25_first_item_refs,
            inventory_feature25_second_item_refs =
                item_context.inventory_feature25_second_item_refs,
            inventory_feature25_legacy_tail_item_refs =
                item_context.inventory_feature25_legacy_tail_item_refs,
            cleared_inventory_item_object_ids = item_context.cleared_inventory_item_object_ids,
            "semantic state retained inventory item context for next GuiQuickbar"
        );
    }

    if state.ui.last_committed_quickbar_profile.is_some()
        && state
            .ui
            .last_inventory_item_context_after_committed_quickbar
            != Some(item_context)
    {
        state
            .ui
            .last_inventory_item_context_after_committed_quickbar = Some(item_context);
        state
            .ui
            .inventory_item_context_after_committed_quickbar_updates = state
            .ui
            .inventory_item_context_after_committed_quickbar_updates
            .saturating_add(1);
        let pending_item_refresh_proof_class = quickbar_item_refresh_proof_class(item_context);
        let pending_item_refresh = pending_item_refresh_proof_class.is_some();
        let was_pending_item_refresh = state.ui.post_committed_quickbar_item_refresh_pending;
        state.ui.post_committed_quickbar_item_refresh_pending = pending_item_refresh;
        state
            .ui
            .post_committed_quickbar_item_refresh_pending_updates = if pending_item_refresh {
            state
                .ui
                .inventory_item_context_after_committed_quickbar_updates
        } else {
            0
        };
        if !pending_item_refresh {
            state.ui.post_committed_quickbar_item_refresh_pending_events = 0;
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown =
                QuickbarItemRefreshEventBreakdown::default();
            state
                .ui
                .post_committed_quickbar_item_refresh_events_after_first_client_action = 0;
            state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
                QuickbarItemRefreshEventBreakdown::default();
            state
                .ui
                .post_committed_quickbar_item_refresh_first_followup_event = None;
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action = None;
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action_detail = None;
            state
                .ui
                .post_committed_quickbar_item_refresh_first_event_after_client_action = None;
        } else if !was_pending_item_refresh {
            state
                .ui
                .post_committed_quickbar_item_refresh_events_after_first_client_action = 0;
            state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action =
                QuickbarItemRefreshEventBreakdown::default();
            state
                .ui
                .post_committed_quickbar_item_refresh_first_followup_event = None;
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action = None;
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action_detail = None;
            state
                .ui
                .post_committed_quickbar_item_refresh_first_event_after_client_action = None;
        }
        state.ui.post_committed_quickbar_item_refresh_proof_class =
            pending_item_refresh_proof_class;
        let pending_item_refresh_proof_class = pending_item_refresh_proof_class
            .map(QuickbarItemRefreshProofClass::as_str)
            .unwrap_or("none");
        let pending_item_refresh_first_followup_event = state
            .ui
            .post_committed_quickbar_item_refresh_first_followup_event
            .map(QuickbarItemRefreshEventKind::as_str)
            .unwrap_or("none");
        let pending_item_refresh_first_client_action = state
            .ui
            .post_committed_quickbar_item_refresh_first_client_action
            .map(QuickbarItemRefreshEventKind::as_str)
            .unwrap_or("none");
        let pending_item_refresh_first_event_after_client_action = state
            .ui
            .post_committed_quickbar_item_refresh_first_event_after_client_action
            .map(QuickbarItemRefreshEventKind::as_str)
            .unwrap_or("none");
        let (
            pending_item_refresh_first_client_action_has_object_id,
            pending_item_refresh_first_client_action_object_id,
            pending_item_refresh_first_client_action_slot,
            pending_item_refresh_first_client_action_button_type,
            pending_item_refresh_first_client_action_body_kind,
            pending_item_refresh_first_client_action_candidate_known,
            pending_item_refresh_first_client_action_candidate_object_id,
            pending_item_refresh_first_client_action_matches_candidate,
        ) = quickbar_item_refresh_client_action_trace_fields(
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action_detail,
        );
        let (
            compact_item_emission_candidate_known,
            compact_item_emission_candidate_object_id,
            compact_item_emission_candidate_proof,
            compact_item_emission_candidate_source,
        ) = quickbar_item_context_candidate_trace_fields(
            item_context.compact_item_emission_candidate,
        );
        tracing::info!(
            source,
            updates_since_committed_quickbar = state
                .ui
                .inventory_item_context_after_committed_quickbar_updates,
            pending_item_refresh,
            pending_item_refresh_updates = state
                .ui
                .post_committed_quickbar_item_refresh_pending_updates,
            pending_item_refresh_events =
                state.ui.post_committed_quickbar_item_refresh_pending_events,
            pending_item_refresh_live_object_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .live_object_events,
            pending_item_refresh_quickbar_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .quickbar_events,
            pending_item_refresh_area_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .area_events,
            pending_item_refresh_inventory_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .inventory_events,
            pending_item_refresh_client_input_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_input_events,
            pending_item_refresh_client_input_use_item_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_input_use_item_events,
            pending_item_refresh_client_input_use_object_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_input_use_object_events,
            pending_item_refresh_client_input_change_door_state_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_input_change_door_state_events,
            pending_item_refresh_client_input_other_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_input_other_events,
            pending_item_refresh_client_quickbar_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_quickbar_events,
            pending_item_refresh_client_quickbar_item_set_button_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_quickbar_item_set_button_events,
            pending_item_refresh_client_quickbar_other_set_button_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .client_quickbar_other_set_button_events,
            pending_item_refresh_chat_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .chat_events,
            pending_item_refresh_other_events = state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .other_events,
            pending_item_refresh_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_events_after_first_client_action,
            pending_item_refresh_live_object_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .live_object_events,
            pending_item_refresh_quickbar_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .quickbar_events,
            pending_item_refresh_area_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .area_events,
            pending_item_refresh_inventory_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .inventory_events,
            pending_item_refresh_client_input_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .client_input_events,
            pending_item_refresh_client_input_use_item_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .client_input_use_item_events,
            pending_item_refresh_client_input_use_object_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .client_input_use_object_events,
            pending_item_refresh_client_input_change_door_state_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .client_input_change_door_state_events,
            pending_item_refresh_client_input_other_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .client_input_other_events,
            pending_item_refresh_client_quickbar_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .client_quickbar_events,
            pending_item_refresh_client_quickbar_item_set_button_events_after_first_client_action =
                state
                    .ui
                    .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                    .client_quickbar_item_set_button_events,
            pending_item_refresh_client_quickbar_other_set_button_events_after_first_client_action =
                state
                    .ui
                    .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                    .client_quickbar_other_set_button_events,
            pending_item_refresh_chat_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .chat_events,
            pending_item_refresh_other_events_after_first_client_action = state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .other_events,
            pending_item_refresh_proof_class,
            pending_item_refresh_first_followup_event,
            pending_item_refresh_first_client_action,
            pending_item_refresh_first_client_action_has_object_id,
            pending_item_refresh_first_client_action_object_id,
            pending_item_refresh_first_client_action_slot,
            pending_item_refresh_first_client_action_button_type,
            pending_item_refresh_first_client_action_body_kind,
            pending_item_refresh_first_client_action_candidate_known,
            pending_item_refresh_first_client_action_candidate_object_id,
            pending_item_refresh_first_client_action_matches_candidate,
            pending_item_refresh_first_event_after_client_action,
            direct_item_proof_objects = item_context.direct_item_proof_objects,
            feature25_item_proof_objects = item_context.feature25_item_proof_objects,
            compact_item_emission_proof_objects = item_context.compact_item_emission_proof_objects,
            compact_item_emission_candidate_known,
            compact_item_emission_candidate_object_id,
            compact_item_emission_candidate_proof,
            compact_item_emission_candidate_source,
            compact_item_emission_direct_only_proof_objects =
                item_context.compact_item_emission_direct_only_proof_objects,
            compact_item_emission_feature25_only_proof_objects =
                item_context.compact_item_emission_feature25_only_proof_objects,
            compact_item_emission_shared_proof_objects =
                item_context.compact_item_emission_shared_proof_objects,
            inventory_feature25_first_item_refs = item_context.inventory_feature25_first_item_refs,
            inventory_feature25_second_item_refs =
                item_context.inventory_feature25_second_item_refs,
            inventory_feature25_legacy_tail_item_refs =
                item_context.inventory_feature25_legacy_tail_item_refs,
            cleared_inventory_item_object_ids = item_context.cleared_inventory_item_object_ids,
            "semantic state retained inventory item context after committed GuiQuickbar"
        );
    }
}

fn record_pending_quickbar_item_refresh_event(
    state: &mut SemanticSessionState,
    event: &ProtocolEvent,
    pending_before_event: bool,
) {
    if !state.ui.post_committed_quickbar_item_refresh_pending {
        return;
    }
    state.ui.post_committed_quickbar_item_refresh_pending_events = state
        .ui
        .post_committed_quickbar_item_refresh_pending_events
        .saturating_add(1);
    let event_kind = quickbar_item_refresh_event_kind(event);
    let first_client_action_seen_before_event = state
        .ui
        .post_committed_quickbar_item_refresh_first_client_action
        .is_some();
    if pending_before_event {
        if state
            .ui
            .post_committed_quickbar_item_refresh_first_followup_event
            .is_none()
        {
            state
                .ui
                .post_committed_quickbar_item_refresh_first_followup_event = Some(event_kind);
        }
        if event_kind.is_client_action()
            && state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action
                .is_none()
        {
            let compact_candidate = state
                .ui
                .last_inventory_item_context_after_committed_quickbar
                .and_then(|context| context.compact_item_emission_candidate);
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action = Some(event_kind);
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action_detail = Some(
                quickbar_item_refresh_client_action_detail(event, event_kind, compact_candidate),
            );
        }
    }
    if first_client_action_seen_before_event {
        state
            .ui
            .post_committed_quickbar_item_refresh_events_after_first_client_action = state
            .ui
            .post_committed_quickbar_item_refresh_events_after_first_client_action
            .saturating_add(1);
        if state
            .ui
            .post_committed_quickbar_item_refresh_first_event_after_client_action
            .is_none()
        {
            state
                .ui
                .post_committed_quickbar_item_refresh_first_event_after_client_action =
                Some(event_kind);
        }
        record_quickbar_item_refresh_event_breakdown(
            &mut state
                .ui
                .post_committed_quickbar_item_refresh_event_breakdown_after_first_client_action,
            event,
        );
    }
    record_quickbar_item_refresh_event_breakdown(
        &mut state
            .ui
            .post_committed_quickbar_item_refresh_pending_event_breakdown,
        event,
    );
}

fn record_quickbar_item_refresh_event_breakdown(
    breakdown: &mut QuickbarItemRefreshEventBreakdown,
    event: &ProtocolEvent,
) {
    match event {
        ProtocolEvent::LiveObject(_) => {
            breakdown.live_object_events = breakdown.live_object_events.saturating_add(1);
        }
        ProtocolEvent::Quickbar(_) => {
            breakdown.quickbar_events = breakdown.quickbar_events.saturating_add(1);
        }
        ProtocolEvent::Area(_) => {
            breakdown.area_events = breakdown.area_events.saturating_add(1);
        }
        ProtocolEvent::Inventory(_) => {
            breakdown.inventory_events = breakdown.inventory_events.saturating_add(1);
        }
        ProtocolEvent::ClientInput(event) => {
            breakdown.client_input_events = breakdown.client_input_events.saturating_add(1);
            match event.claim.map(|claim| claim.kind) {
                Some(client_input::ClientInputKind::UseItem) => {
                    breakdown.client_input_use_item_events =
                        breakdown.client_input_use_item_events.saturating_add(1);
                }
                Some(client_input::ClientInputKind::UseObject) => {
                    breakdown.client_input_use_object_events =
                        breakdown.client_input_use_object_events.saturating_add(1);
                }
                Some(client_input::ClientInputKind::ChangeDoorState) => {
                    breakdown.client_input_change_door_state_events = breakdown
                        .client_input_change_door_state_events
                        .saturating_add(1);
                }
                _ => {
                    breakdown.client_input_other_events =
                        breakdown.client_input_other_events.saturating_add(1);
                }
            }
        }
        ProtocolEvent::ClientQuickbar(event) => {
            breakdown.client_quickbar_events = breakdown.client_quickbar_events.saturating_add(1);
            match event.claim.map(|claim| claim.body_kind) {
                Some(client_quickbar::ClientQuickbarSetButtonKind::Item) => {
                    breakdown.client_quickbar_item_set_button_events = breakdown
                        .client_quickbar_item_set_button_events
                        .saturating_add(1);
                }
                Some(_) | None => {
                    breakdown.client_quickbar_other_set_button_events = breakdown
                        .client_quickbar_other_set_button_events
                        .saturating_add(1);
                }
            }
        }
        ProtocolEvent::Chat(_) => {
            breakdown.chat_events = breakdown.chat_events.saturating_add(1);
        }
        ProtocolEvent::ModuleInfo(_)
        | ProtocolEvent::ServerStatus(_)
        | ProtocolEvent::PlayerList(_)
        | ProtocolEvent::Login(_)
        | ProtocolEvent::Other(_) => {
            breakdown.other_events = breakdown.other_events.saturating_add(1);
        }
    }
}

fn quickbar_item_refresh_client_action_detail(
    event: &ProtocolEvent,
    kind: QuickbarItemRefreshEventKind,
    compact_candidate: Option<InventoryItemContextCandidate>,
) -> QuickbarItemRefreshClientActionDetail {
    let candidate_object_id = compact_candidate.map(|candidate| candidate.object_id);
    let matches_candidate_object = |object_id: Option<u32>| {
        object_id
            .zip(candidate_object_id)
            .map(|(object_id, candidate_object_id)| object_id == candidate_object_id)
    };
    match event {
        ProtocolEvent::ClientInput(event) => {
            let object_id = event.claim.map(|claim| claim.primary_object_id);
            QuickbarItemRefreshClientActionDetail {
                kind,
                object_id,
                slot: None,
                button_type: None,
                body_kind: None,
                candidate_object_id,
                matches_candidate_object: matches_candidate_object(object_id),
            }
        }
        ProtocolEvent::ClientQuickbar(event) => {
            let object_id = event.claim.and_then(|claim| claim.item_object_id);
            QuickbarItemRefreshClientActionDetail {
                kind,
                object_id,
                slot: event.claim.map(|claim| claim.slot),
                button_type: event.claim.map(|claim| claim.button_type),
                body_kind: event.claim.map(|claim| claim.body_kind),
                candidate_object_id,
                matches_candidate_object: matches_candidate_object(object_id),
            }
        }
        _ => QuickbarItemRefreshClientActionDetail {
            kind,
            object_id: None,
            slot: None,
            button_type: None,
            body_kind: None,
            candidate_object_id,
            matches_candidate_object: None,
        },
    }
}

fn quickbar_item_refresh_client_action_trace_fields(
    detail: Option<QuickbarItemRefreshClientActionDetail>,
) -> (bool, u32, u8, u8, &'static str, bool, u32, bool) {
    let has_object_id = detail.and_then(|detail| detail.object_id).is_some();
    let object_id = detail.and_then(|detail| detail.object_id).unwrap_or(0);
    let slot = detail.and_then(|detail| detail.slot).unwrap_or(0);
    let button_type = detail.and_then(|detail| detail.button_type).unwrap_or(0);
    let body_kind = detail
        .and_then(|detail| detail.body_kind)
        .map(client_quickbar::ClientQuickbarSetButtonKind::as_str)
        .unwrap_or("none");
    let candidate_known = detail
        .and_then(|detail| detail.candidate_object_id)
        .is_some();
    let candidate_object_id = detail
        .and_then(|detail| detail.candidate_object_id)
        .unwrap_or(0);
    let matches_candidate = detail
        .and_then(|detail| detail.matches_candidate_object)
        .unwrap_or(false);
    (
        has_object_id,
        object_id,
        slot,
        button_type,
        body_kind,
        candidate_known,
        candidate_object_id,
        matches_candidate,
    )
}

fn quickbar_item_context_candidate_trace_fields(
    candidate: Option<InventoryItemContextCandidate>,
) -> (bool, u32, &'static str, &'static str) {
    let known = candidate.is_some();
    let object_id = candidate.map(|candidate| candidate.object_id).unwrap_or(0);
    let proof = candidate
        .map(|candidate| candidate.proof.as_str())
        .unwrap_or("none");
    let source = candidate
        .map(|candidate| candidate.source.as_str())
        .unwrap_or("none");
    (known, object_id, proof, source)
}

fn quickbar_item_refresh_event_kind(event: &ProtocolEvent) -> QuickbarItemRefreshEventKind {
    match event {
        ProtocolEvent::LiveObject(_) => QuickbarItemRefreshEventKind::LiveObject,
        ProtocolEvent::Quickbar(_) => QuickbarItemRefreshEventKind::ServerQuickbar,
        ProtocolEvent::Area(_) => QuickbarItemRefreshEventKind::Area,
        ProtocolEvent::Inventory(_) => QuickbarItemRefreshEventKind::Inventory,
        ProtocolEvent::ClientInput(event) => match event.claim.map(|claim| claim.kind) {
            Some(client_input::ClientInputKind::UseItem) => {
                QuickbarItemRefreshEventKind::ClientInputUseItem
            }
            Some(client_input::ClientInputKind::UseObject) => {
                QuickbarItemRefreshEventKind::ClientInputUseObject
            }
            Some(client_input::ClientInputKind::ChangeDoorState) => {
                QuickbarItemRefreshEventKind::ClientInputChangeDoorState
            }
            _ => QuickbarItemRefreshEventKind::ClientInputOther,
        },
        ProtocolEvent::ClientQuickbar(event) => match event.claim.map(|claim| claim.body_kind) {
            Some(client_quickbar::ClientQuickbarSetButtonKind::Item) => {
                QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton
            }
            Some(_) | None => QuickbarItemRefreshEventKind::ClientQuickbarOtherSetButton,
        },
        ProtocolEvent::Chat(_) => QuickbarItemRefreshEventKind::Chat,
        ProtocolEvent::ModuleInfo(_)
        | ProtocolEvent::ServerStatus(_)
        | ProtocolEvent::PlayerList(_)
        | ProtocolEvent::Login(_)
        | ProtocolEvent::Other(_) => QuickbarItemRefreshEventKind::Other,
    }
}

fn observed_high_level(
    direction: Direction,
    family: VerifiedFamily,
    payload: &[u8],
) -> ObservedHighLevel {
    let high = HighLevel::parse(payload);
    ObservedHighLevel {
        direction,
        family,
        major: high.map(|value| value.major),
        minor: high.map(|value| value.minor),
        packet_name: high.map(HighLevel::name),
        payload_len: payload.len(),
        declared_len: read_u32_le(payload, 3).and_then(|value| usize::try_from(value).ok()),
    }
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn live_object_observations_from_payload(
    payload: &[u8],
) -> (
    Vec<LiveObjectMention>,
    Vec<u32>,
    Vec<LiveObjectInventoryFeature25Reference>,
) {
    let Some(claim) = live_object_update::claim_payload_if_verified(payload) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let materialized_item_object_ids = claim.materialized_item_object_ids;
    let mut inventory_feature25_references = Vec::new();
    let mentions = claim
        .mentions
        .into_iter()
        .map(|mention| {
            if let Some(inventory) = mention.inventory_owner.as_ref() {
                if let Some(feature25) = inventory.feature25.as_ref() {
                    inventory_feature25_references.push(LiveObjectInventoryFeature25Reference {
                        owner_id: inventory.owner_id,
                        mask: inventory.mask,
                        first_object_ids: feature25.first_object_ids.clone(),
                        second_object_ids: feature25.second_object_ids.clone(),
                        legacy_tail_object_ids: feature25.legacy_tail_object_ids.clone(),
                    });
                }
            }
            LiveObjectMention {
                opcode: mention.opcode,
                object_type: mention.object_type,
                object_id: mention.object_id,
                name: mention.name,
                position: mention.position.map(|position| LiveObjectPosition {
                    x: position.x,
                    y: position.y,
                    z: position.z,
                }),
                orientation: mention
                    .orientation
                    .map(|orientation| LiveObjectOrientation {
                        source: match orientation.source {
                            live_object_update::LiveObjectRecordOrientationSource::Scalar => {
                                LiveObjectOrientationSource::Scalar
                            }
                            live_object_update::LiveObjectRecordOrientationSource::Vector => {
                                LiveObjectOrientationSource::Vector
                            }
                        },
                        scalar_tenths_degrees: orientation.scalar_tenths_degrees,
                        vector: orientation
                            .vector
                            .map(|vector| LiveObjectOrientationVector {
                                x: vector.x,
                                y: vector.y,
                                z: vector.z,
                            }),
                    }),
                bounds: mention.bounds.map(|bounds| super::LiveObjectBounds {
                    min_x: bounds.min_x,
                    min_y: bounds.min_y,
                    min_z: bounds.min_z,
                    max_x: bounds.max_x,
                    max_y: bounds.max_y,
                    max_z: bounds.max_z,
                }),
                placeable_appearance: mention.placeable_appearance.map(|appearance| {
                    super::LiveObjectPlaceableAppearance {
                        appearance: appearance.appearance,
                        resref: appearance.resref,
                    }
                }),
                placeable_state: mention
                    .placeable_state
                    .map(|state| LiveObjectPlaceableState {
                        useable: state.useable,
                        trap_disarmable: state.trap_disarmable,
                        lockable: state.lockable,
                        locked: state.locked,
                    }),
            }
        })
        .collect();
    (
        mentions,
        materialized_item_object_ids,
        inventory_feature25_references,
    )
}

fn current_area_object_id_from_payload(payload: &[u8]) -> Option<u32> {
    const AREA_OBJECT_ID_OFFSET: usize = 3 + 4 + 4 + 4 * 4;
    read_u32_le(payload, AREA_OBJECT_ID_OFFSET)
}

#[cfg(test)]
mod fixture_free_tests {
    use super::*;
    use crate::{
        packet::Direction,
        translate::{VerifiedFamily, VerifiedProof},
    };

    fn pack_msb_valid_bits(mut bits: Vec<bool>, header_bits: usize) -> Vec<u8> {
        assert!(bits.len() >= header_bits);
        let final_fragment_bits = bits.len() % 8;
        bits[0] = (final_fragment_bits & 0x04) != 0;
        bits[1] = (final_fragment_bits & 0x02) != 0;
        bits[2] = (final_fragment_bits & 0x01) != 0;

        let mut packed = vec![0u8; bits.len().div_ceil(8)];
        for (bit_index, bit) in bits.into_iter().enumerate() {
            if bit {
                packed[bit_index / 8] |= 0x80 >> (bit_index % 8);
            }
        }
        packed
    }

    fn live_object_payload_with_bits(live: &[u8], owned_bits: &[bool]) -> Vec<u8> {
        let mut payload = vec![b'P', 0x05, 0x01];
        let declared = (3 + 4 + live.len()) as u32;
        payload.extend_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(live);

        let mut fragment_bits = vec![false; 3];
        fragment_bits.extend_from_slice(owned_bits);
        payload.extend_from_slice(&pack_msb_valid_bits(fragment_bits, 3));
        payload
    }

    #[test]
    fn committed_quickbar_best_item_context_prefers_current_then_previous_post_then_prior() {
        let prior_context = InventoryItemContextSummary {
            feature25_item_proof_objects: 1,
            compact_item_emission_proof_objects: 1,
            compact_item_emission_feature25_only_proof_objects: 1,
            inventory_feature25_first_item_refs: 1,
            ..Default::default()
        };
        let previous_post_context = InventoryItemContextSummary {
            feature25_item_proof_objects: 2,
            compact_item_emission_proof_objects: 2,
            compact_item_emission_feature25_only_proof_objects: 2,
            inventory_feature25_second_item_refs: 2,
            ..Default::default()
        };
        let current_context = InventoryItemContextSummary {
            direct_item_proof_objects: 1,
            compact_item_emission_proof_objects: 1,
            compact_item_emission_direct_only_proof_objects: 1,
            ..Default::default()
        };
        let cleared_current_context = InventoryItemContextSummary {
            cleared_inventory_item_object_ids: 2,
            ..Default::default()
        };

        assert_eq!(
            best_committed_quickbar_item_context(Default::default(), None, None),
            (None, None),
            "empty current/prior/post windows should not invent quickbar item evidence"
        );
        assert_eq!(
            best_committed_quickbar_item_context(Default::default(), Some(prior_context), None),
            (Some(prior_context), Some(QuickbarItemContextSource::Prior)),
            "older prior context remains useful if no newer proof window exists"
        );
        assert_eq!(
            best_committed_quickbar_item_context(
                Default::default(),
                Some(prior_context),
                Some(previous_post_context),
            ),
            (
                Some(previous_post_context),
                Some(QuickbarItemContextSource::PreviousPost),
            ),
            "post-quickbar proof is more specific than an older prior snapshot"
        );
        assert_eq!(
            best_committed_quickbar_item_context(
                current_context,
                Some(prior_context),
                Some(previous_post_context),
            ),
            (
                Some(current_context),
                Some(QuickbarItemContextSource::Current)
            ),
            "current registry proof at commit is the strongest writer-facing evidence"
        );
        assert_eq!(
            best_committed_quickbar_item_context(
                cleared_current_context,
                Some(prior_context),
                Some(previous_post_context),
            ),
            (
                Some(cleared_current_context),
                Some(QuickbarItemContextSource::Current),
            ),
            "current cleared-state evidence must override stale proof windows"
        );
    }

    #[test]
    fn live_object_feature25_references_feed_deferred_inventory_state() {
        let owner_id = 0x8000_0010u32;
        let first_item_id = 0x8000_0100u32;
        let second_item_id = 0x8000_0101u32;
        let mut live = vec![b'I'];
        live.extend_from_slice(&owner_id.to_le_bytes());
        live.extend_from_slice(&0x2000u16.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&first_item_id.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&second_item_id.to_le_bytes());
        let payload = live_object_payload_with_bits(&live, &[false, true, false]);

        let mut state = SemanticSessionState::default();
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &payload,
        );

        assert_eq!(
            state.objects.inventory_item_object_proof(first_item_id),
            Some(InventoryItemObjectProof::Feature25FirstList),
            "Feature-25 first-list refs should retain their proof source"
        );
        assert_eq!(
            state.objects.inventory_item_object_proof(second_item_id),
            Some(InventoryItemObjectProof::Feature25SecondList),
            "Feature-25 second-list refs should stay distinguishable from first-list refs"
        );
        assert!(
            !state.objects.has_active_object_id(second_item_id),
            "deferred Feature-25 refs must not become active lifecycle materialization"
        );
    }

    #[test]
    fn committed_quickbar_profile_survives_placeholder_events() {
        let payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should build");
        let mut state = SemanticSessionState::default();

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &payload,
        );

        let profile = state
            .ui
            .last_committed_quickbar_profile
            .expect("committed quickbar should record an exact slot profile");
        assert_eq!(profile.slot_records, 36);
        assert_eq!(profile.blank_slots, 36);
        assert_eq!(profile.item_slots, 0);
        assert_eq!(profile.spell_slots, 0);
        assert_eq!(state.ui.quickbar_packets, 1);

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbarPlaceholder),
            &payload,
        );

        assert_eq!(
            state.ui.last_committed_quickbar_profile,
            Some(profile),
            "placeholder frames must not replace the last committed quickbar slot profile"
        );
        assert_eq!(state.ui.quickbar_packets, 2);
        assert_eq!(state.ui.quickbar_placeholders, 1);
    }

    #[test]
    fn committed_quickbar_records_registry_materialization_context() {
        let owner_id = 0x8000_0010u32;
        let first_item_id = 0x8000_0100u32;
        let second_item_id = 0x8000_0101u32;
        let mut live = vec![b'I'];
        live.extend_from_slice(&owner_id.to_le_bytes());
        live.extend_from_slice(&0x2000u16.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&first_item_id.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&second_item_id.to_le_bytes());
        let live_payload = live_object_payload_with_bits(&live, &[false, true, false]);
        let quickbar_payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should build");
        let mut state = SemanticSessionState::default();

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &live_payload,
        );
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );

        let context = state
            .ui
            .last_committed_quickbar_materialization_context
            .expect("committed quickbar should snapshot registry item context");
        let prior_context = state
            .ui
            .last_committed_quickbar_prior_item_context
            .expect("committed quickbar should snapshot prior item context");
        assert_eq!(context.active_item_objects, 0);
        assert_eq!(context.direct_item_proof_objects, 0);
        assert_eq!(context.feature25_item_proof_objects, 2);
        assert_eq!(context.compact_item_emission_proof_objects, 2);
        assert_eq!(context.compact_item_emission_direct_only_proof_objects, 0);
        assert_eq!(
            context.compact_item_emission_feature25_only_proof_objects,
            2
        );
        assert_eq!(context.compact_item_emission_shared_proof_objects, 0);
        assert_eq!(context.inventory_feature25_first_item_refs, 1);
        assert_eq!(context.inventory_feature25_second_item_refs, 1);
        assert_eq!(context.inventory_feature25_reference_records, 1);
        assert_eq!(
            prior_context, context,
            "a committed quickbar should retain the latest proof-bearing item context"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context,
            Some(context),
            "committed quickbar should expose the strongest item-proof context"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context_source,
            Some(QuickbarItemContextSource::Current),
            "current registry context is strongest when it already contains item proof"
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbarPlaceholder),
            &quickbar_payload,
        );

        assert_eq!(
            state.ui.last_committed_quickbar_materialization_context,
            Some(context),
            "placeholder frames must not replace the last committed quickbar materialization context"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_prior_item_context,
            Some(prior_context),
            "placeholder frames must not replace the prior-context snapshot"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context,
            Some(context),
            "placeholder frames must not replace the best-context snapshot"
        );
    }

    #[test]
    fn committed_quickbar_records_prior_cleared_item_context_after_area_reset() {
        let owner_id = 0x8000_0010u32;
        let first_item_id = 0x8000_0100u32;
        let second_item_id = 0x8000_0101u32;
        let mut live = vec![b'I'];
        live.extend_from_slice(&owner_id.to_le_bytes());
        live.extend_from_slice(&0x2000u16.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&first_item_id.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&second_item_id.to_le_bytes());
        let live_payload = live_object_payload_with_bits(&live, &[false, true, false]);
        let quickbar_payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should build");
        let mut state = SemanticSessionState::default();

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &live_payload,
        );
        assert_eq!(
            state
                .ui
                .last_inventory_item_context_before_quickbar
                .expect("Feature-25 item refs should retain prior context")
                .compact_item_emission_proof_objects,
            2
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::AreaClientArea),
            &[],
        );

        let cleared_context = state
            .ui
            .last_inventory_item_context_before_quickbar
            .expect("area reset should retain cleared prior context");
        assert_eq!(cleared_context.compact_item_emission_proof_objects, 0);
        assert_eq!(cleared_context.feature25_item_proof_objects, 0);
        assert_eq!(
            cleared_context.cleared_inventory_item_object_ids, 2,
            "area reset should explain why the prior Feature-25 refs are no longer usable"
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );

        assert_eq!(
            state.ui.last_committed_quickbar_prior_item_context,
            Some(cleared_context),
            "committed quickbar diagnostics should keep the last relevant cleared context"
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_materialization_context
                .expect("committed quickbar should snapshot current registry context")
                .cleared_inventory_item_object_ids,
            2
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context,
            Some(cleared_context),
            "current cleared context should be the best quickbar item evidence"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context_source,
            Some(QuickbarItemContextSource::Current),
            "cleared current state must override stale proof windows"
        );
    }

    #[test]
    fn item_context_after_committed_quickbar_is_tracked_until_next_profile() {
        let owner_id = 0x8000_0010u32;
        let first_item_id = 0x8000_0100u32;
        let second_item_id = 0x8000_0101u32;
        let mut live = vec![b'I'];
        live.extend_from_slice(&owner_id.to_le_bytes());
        live.extend_from_slice(&0x2000u16.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&first_item_id.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&second_item_id.to_le_bytes());
        let live_payload = live_object_payload_with_bits(&live, &[false, true, false]);
        let quickbar_payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should build");
        let mut state = SemanticSessionState::default();

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );

        assert_eq!(
            state
                .ui
                .last_inventory_item_context_after_committed_quickbar,
            None,
            "a committed quickbar opens a fresh post-quickbar item-context window"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context, None,
            "a committed quickbar with no current/prior/post item evidence should stay unmarked"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context_source, None,
            "empty best-context snapshots should not report a source"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_item_refresh_outcome,
            QuickbarItemRefreshOutcome::NoPendingRefresh,
            "the first committed quickbar has no pending item-refresh window"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_item_refresh_proof_class, None,
            "a no-pending committed quickbar should not report a proof class"
        );
        assert_eq!(
            state
                .ui
                .inventory_item_context_after_committed_quickbar_updates,
            0
        );
        assert!(
            !state.ui.post_committed_quickbar_item_refresh_pending,
            "a committed quickbar starts with no pending post-context item refresh"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_updates,
            0
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_proof_class, None,
            "a new post-quickbar window has no pending proof class"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_previous_post_item_context, None,
            "the first committed quickbar has no previous post-context window"
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_previous_post_item_context_updates,
            0
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &live_payload,
        );

        let post_context = state
            .ui
            .last_inventory_item_context_after_committed_quickbar
            .expect("later Feature-25 refs should be retained after the committed quickbar");
        assert_eq!(post_context.feature25_item_proof_objects, 2);
        assert_eq!(post_context.compact_item_emission_proof_objects, 2);
        assert_eq!(
            state
                .ui
                .inventory_item_context_after_committed_quickbar_updates,
            1
        );
        assert!(
            state.ui.post_committed_quickbar_item_refresh_pending,
            "post-quickbar compact item proof should mark the committed profile as awaiting a later item-bearing refresh"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_updates,
            1
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_proof_class,
            Some(QuickbarItemRefreshProofClass::Feature25Only),
            "the pending post-quickbar proof should preserve its Feature-25-only class"
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_pending_events, 1,
            "the live-object event that creates pending item proof should count as unresolved pending traffic"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_first_followup_event,
            None,
            "the proof-opening live-object row is not a follow-up trigger"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action,
            None,
            "no client action has occurred after the pending proof opened"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .live_object_events,
            1,
            "the event breakdown should classify the proof-creating live-object event"
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::Inventory),
            &[],
        );
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::Chat),
            &[],
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_pending_events, 3,
            "all later verified traffic should keep the pending refresh window accountable"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .inventory_events,
            1
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown
                .chat_events,
            1
        );
        let unresolved = state
            .ui
            .unresolved_pending_item_refresh()
            .expect("pending proof should expose an unresolved refresh summary");
        assert_eq!(unresolved.item_context, post_context);
        assert_eq!(unresolved.updates_since_committed_quickbar, 1);
        assert_eq!(unresolved.events_since_pending_refresh, 3);
        assert_eq!(unresolved.event_breakdown.live_object_events, 1);
        assert_eq!(unresolved.event_breakdown.inventory_events, 1);
        assert_eq!(unresolved.event_breakdown.chat_events, 1);
        assert_eq!(
            unresolved.first_followup_event,
            Some(QuickbarItemRefreshEventKind::Inventory),
            "first follow-up after proof opening should be tracked separately from aggregate buckets"
        );
        assert_eq!(
            unresolved.first_client_action, None,
            "server-only follow-up traffic should not invent a client trigger"
        );
        assert_eq!(
            unresolved.proof_class,
            Some(QuickbarItemRefreshProofClass::Feature25Only)
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );

        assert_eq!(
            state.ui.last_committed_quickbar_prior_item_context,
            Some(post_context),
            "the second committed quickbar should consume the post-quickbar context as prior evidence"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_previous_post_item_context,
            Some(post_context),
            "the second committed quickbar should preserve that prior evidence as previous-post context"
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_previous_post_item_context_updates,
            1
        );
        assert!(
            state.ui.last_committed_quickbar_item_refresh_pending,
            "the later committed quickbar should report that a post-quickbar item proof window was pending"
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_pending_updates,
            1
        );
        assert_eq!(
            state.ui.last_committed_quickbar_item_refresh_pending_events, 3,
            "the resolving committed quickbar should snapshot unresolved pending event count"
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_pending_event_breakdown
                .live_object_events,
            1
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_pending_event_breakdown
                .inventory_events,
            1
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_pending_event_breakdown
                .chat_events,
            1
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_first_followup_event,
            Some(QuickbarItemRefreshEventKind::Inventory)
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_first_client_action,
            None
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context,
            Some(post_context),
            "the second committed quickbar should expose the Feature-25 proof window"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_best_item_context_source,
            Some(QuickbarItemContextSource::Current),
            "live registry proof current at commit should win over the saved previous-post copy"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_item_refresh_outcome,
            QuickbarItemRefreshOutcome::PendingRefreshStillBlank,
            "a pending compact item refresh followed by a zero-item quickbar should remain distinguishable"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_item_refresh_proof_class,
            Some(QuickbarItemRefreshProofClass::Feature25Only),
            "the consumed pending refresh should retain the proof class seen after the prior quickbar"
        );
        assert_eq!(
            state
                .ui
                .last_inventory_item_context_after_committed_quickbar,
            None,
            "a new committed quickbar starts a new after-context window"
        );
        assert_eq!(
            state
                .ui
                .inventory_item_context_after_committed_quickbar_updates,
            0
        );
        assert!(
            !state.ui.post_committed_quickbar_item_refresh_pending,
            "a new committed quickbar consumes and clears the pending refresh window"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_updates,
            0
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_proof_class, None,
            "the next committed quickbar consumes and clears the pending proof class"
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_pending_events, 0,
            "a new committed quickbar should clear the active pending event count"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown,
            Default::default(),
            "a new committed quickbar should clear the active pending event breakdown"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_first_followup_event,
            None,
            "a new committed quickbar should clear active first-follow-up tracking"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action,
            None,
            "a new committed quickbar should clear active first-client-action tracking"
        );
        assert_eq!(
            state.ui.unresolved_pending_item_refresh(),
            None,
            "resolved pending proof should no longer expose an unresolved summary"
        );
    }

    #[test]
    fn quickbar_item_refresh_proof_class_uses_compact_proof_partition() {
        assert_eq!(
            quickbar_item_refresh_proof_class(Default::default()),
            None,
            "empty context should not create a pending proof class"
        );
        assert_eq!(
            quickbar_item_refresh_proof_class(InventoryItemContextSummary {
                compact_item_emission_proof_objects: 1,
                compact_item_emission_direct_only_proof_objects: 1,
                ..Default::default()
            }),
            Some(QuickbarItemRefreshProofClass::DirectOnly)
        );
        assert_eq!(
            quickbar_item_refresh_proof_class(InventoryItemContextSummary {
                compact_item_emission_proof_objects: 1,
                compact_item_emission_feature25_only_proof_objects: 1,
                ..Default::default()
            }),
            Some(QuickbarItemRefreshProofClass::Feature25Only)
        );
        assert_eq!(
            quickbar_item_refresh_proof_class(InventoryItemContextSummary {
                compact_item_emission_proof_objects: 1,
                compact_item_emission_shared_proof_objects: 1,
                ..Default::default()
            }),
            Some(QuickbarItemRefreshProofClass::Shared)
        );
        assert_eq!(
            quickbar_item_refresh_proof_class(InventoryItemContextSummary {
                compact_item_emission_proof_objects: 2,
                compact_item_emission_direct_only_proof_objects: 1,
                compact_item_emission_feature25_only_proof_objects: 1,
                ..Default::default()
            }),
            Some(QuickbarItemRefreshProofClass::Mixed),
            "multiple compact proof classes should stay distinguishable"
        );
    }

    #[test]
    fn committed_quickbar_item_refresh_outcome_classifies_profile_slots() {
        let blank_profile = quickbar::QuickbarValidatedSlotProfile {
            slot_records: 36,
            blank_slots: 36,
            ..Default::default()
        };
        let item_profile = quickbar::QuickbarValidatedSlotProfile {
            slot_records: 36,
            item_slots: 1,
            first_page_visible_slots: 1,
            first_page_item_slots: 1,
            ..Default::default()
        };

        assert_eq!(
            committed_quickbar_item_refresh_outcome(false, &item_profile),
            QuickbarItemRefreshOutcome::NoPendingRefresh,
            "item slots without a pending post-quickbar proof window are not a pending-refresh outcome"
        );
        assert_eq!(
            committed_quickbar_item_refresh_outcome(true, &blank_profile),
            QuickbarItemRefreshOutcome::PendingRefreshStillBlank,
            "a pending compact item refresh followed by a zero-item profile should stay distinguishable"
        );
        assert_eq!(
            committed_quickbar_item_refresh_outcome(true, &item_profile),
            QuickbarItemRefreshOutcome::PendingRefreshEmittedItemSlots,
            "a pending compact item refresh followed by item slots should be marked realized"
        );
    }

    #[test]
    fn pending_quickbar_refresh_records_typed_client_action_buckets() {
        let owner_id = 0x8000_0010u32;
        let first_item_id = 0x8000_0100u32;
        let second_item_id = 0x8000_0101u32;
        let quickbar_item_id = first_item_id;
        let mut live = vec![b'I'];
        live.extend_from_slice(&owner_id.to_le_bytes());
        live.extend_from_slice(&0x2000u16.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&first_item_id.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&second_item_id.to_le_bytes());
        let live_payload = live_object_payload_with_bits(&live, &[false, true, false]);
        let quickbar_payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should build");
        let client_use_item = client_use_item_payload(quickbar_item_id);
        let client_quickbar_item = client_quickbar_item_set_button_payload(2, quickbar_item_id);
        let mut state = SemanticSessionState::default();

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &live_payload,
        );
        observe_verified_payload(
            &mut state,
            Direction::ClientToServer,
            &VerifiedProof::Family(VerifiedFamily::ClientInput),
            &client_use_item,
        );
        observe_verified_payload(
            &mut state,
            Direction::ClientToServer,
            &VerifiedProof::Family(VerifiedFamily::ClientQuickbar),
            &client_quickbar_item,
        );

        let unresolved = state
            .ui
            .unresolved_pending_item_refresh()
            .expect("pending item proof should remain unresolved before the next server quickbar");
        assert_eq!(
            unresolved.item_context.compact_item_emission_candidate,
            Some(InventoryItemContextCandidate {
                object_id: first_item_id,
                proof: InventoryItemObjectProof::Feature25FirstList,
                source: crate::translate::semantic::state::InventoryItemContextCandidateSource::Feature25Only,
            }),
            "the pending refresh should retain the deterministic object id for the harness action"
        );
        assert_eq!(unresolved.events_since_pending_refresh, 3);
        assert_eq!(unresolved.event_breakdown.live_object_events, 1);
        assert_eq!(unresolved.event_breakdown.client_input_events, 1);
        assert_eq!(unresolved.event_breakdown.client_input_use_item_events, 1);
        assert_eq!(unresolved.event_breakdown.client_input_other_events, 0);
        assert_eq!(unresolved.event_breakdown.client_quickbar_events, 1);
        assert_eq!(
            unresolved.events_after_first_client_action, 1,
            "post-action counters should exclude the UseItem itself and count later verified traffic"
        );
        assert_eq!(
            unresolved
                .event_breakdown_after_first_client_action
                .client_input_use_item_events,
            0,
            "the first UseItem is the boundary, not an after-action event"
        );
        assert_eq!(
            unresolved
                .event_breakdown_after_first_client_action
                .client_quickbar_events,
            1
        );
        assert_eq!(
            unresolved.first_event_after_client_action,
            Some(QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton)
        );
        assert_eq!(
            unresolved.first_followup_event,
            Some(QuickbarItemRefreshEventKind::ClientInputUseItem),
            "the first event after the proof opener should identify the UseItem trigger"
        );
        assert_eq!(
            unresolved.first_client_action,
            Some(QuickbarItemRefreshEventKind::ClientInputUseItem),
            "the first client action after pending proof should be retained"
        );
        assert_eq!(
            unresolved.first_client_action_detail,
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientInputUseItem,
                object_id: Some(quickbar_item_id),
                slot: None,
                button_type: None,
                body_kind: None,
                candidate_object_id: Some(first_item_id),
                matches_candidate_object: Some(true),
            }),
            "the first client action should retain the verified UseItem object id and candidate match"
        );
        assert_eq!(
            unresolved
                .event_breakdown
                .client_quickbar_item_set_button_events,
            1
        );
        assert_eq!(
            unresolved
                .event_breakdown
                .client_quickbar_other_set_button_events,
            0
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );

        let committed_breakdown = state
            .ui
            .last_committed_quickbar_item_refresh_pending_event_breakdown;
        assert_eq!(committed_breakdown.client_input_events, 1);
        assert_eq!(committed_breakdown.client_input_use_item_events, 1);
        assert_eq!(committed_breakdown.client_quickbar_events, 1);
        assert_eq!(
            committed_breakdown.client_quickbar_item_set_button_events,
            1
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_events_after_first_client_action,
            1
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_event_breakdown_after_first_client_action
                .client_quickbar_item_set_button_events,
            1
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_first_event_after_client_action,
            Some(QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton)
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_first_followup_event,
            Some(QuickbarItemRefreshEventKind::ClientInputUseItem)
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_first_client_action,
            Some(QuickbarItemRefreshEventKind::ClientInputUseItem)
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_first_client_action_detail,
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientInputUseItem,
                object_id: Some(quickbar_item_id),
                slot: None,
                button_type: None,
                body_kind: None,
                candidate_object_id: Some(first_item_id),
                matches_candidate_object: Some(true),
            }),
            "the resolving server quickbar should snapshot the first client action details"
        );
        assert_eq!(
            state.ui.last_committed_quickbar_item_refresh_outcome,
            QuickbarItemRefreshOutcome::PendingRefreshStillBlank,
            "the resolving server quickbar should still classify the item refresh outcome separately"
        );
    }

    #[test]
    fn pending_quickbar_refresh_records_first_client_quickbar_item_detail() {
        let owner_id = 0x8000_0010u32;
        let first_item_id = 0x8000_0100u32;
        let second_item_id = 0x8000_0101u32;
        let quickbar_item_id = 0x8000_0200u32;
        let mut live = vec![b'I'];
        live.extend_from_slice(&owner_id.to_le_bytes());
        live.extend_from_slice(&0x2000u16.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&first_item_id.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&second_item_id.to_le_bytes());
        let live_payload = live_object_payload_with_bits(&live, &[false, true, false]);
        let quickbar_payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should build");
        let client_quickbar_item = client_quickbar_item_set_button_payload(7, quickbar_item_id);
        let mut state = SemanticSessionState::default();

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &live_payload,
        );
        observe_verified_payload(
            &mut state,
            Direction::ClientToServer,
            &VerifiedProof::Family(VerifiedFamily::ClientQuickbar),
            &client_quickbar_item,
        );

        let unresolved = state
            .ui
            .unresolved_pending_item_refresh()
            .expect("client quickbar item action should leave the pending refresh unresolved");
        assert_eq!(
            unresolved.item_context.compact_item_emission_candidate,
            Some(InventoryItemContextCandidate {
                object_id: first_item_id,
                proof: InventoryItemObjectProof::Feature25FirstList,
                source: crate::translate::semantic::state::InventoryItemContextCandidateSource::Feature25Only,
            })
        );
        assert_eq!(
            unresolved.first_client_action,
            Some(QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton)
        );
        assert_eq!(
            unresolved.first_client_action_detail,
            Some(QuickbarItemRefreshClientActionDetail {
                kind: QuickbarItemRefreshEventKind::ClientQuickbarItemSetButton,
                object_id: Some(quickbar_item_id),
                slot: Some(7),
                button_type: Some(1),
                body_kind: Some(client_quickbar::ClientQuickbarSetButtonKind::Item),
                candidate_object_id: Some(first_item_id),
                matches_candidate_object: Some(false),
            }),
            "the first item SetButton should preserve slot, type, object id, and candidate mismatch"
        );
    }

    #[test]
    fn cleared_context_after_committed_quickbar_cancels_pending_item_refresh() {
        let owner_id = 0x8000_0010u32;
        let first_item_id = 0x8000_0100u32;
        let second_item_id = 0x8000_0101u32;
        let mut live = vec![b'I'];
        live.extend_from_slice(&owner_id.to_le_bytes());
        live.extend_from_slice(&0x2000u16.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&first_item_id.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&second_item_id.to_le_bytes());
        let live_payload = live_object_payload_with_bits(&live, &[false, true, false]);
        let quickbar_payload = quickbar::build_blank_set_all_buttons_payload(b'P')
            .expect("blank quickbar payload should build");
        let mut state = SemanticSessionState::default();

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &live_payload,
        );

        assert!(state.ui.post_committed_quickbar_item_refresh_pending);

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::AreaClientArea),
            &[],
        );

        let cleared_context = state
            .ui
            .last_inventory_item_context_after_committed_quickbar
            .expect("area reset should retain cleared post-quickbar context");
        assert_eq!(cleared_context.compact_item_emission_proof_objects, 0);
        assert_eq!(cleared_context.cleared_inventory_item_object_ids, 2);
        assert_eq!(
            state
                .ui
                .inventory_item_context_after_committed_quickbar_updates,
            2,
            "the cleared context is still a post-quickbar update"
        );
        assert!(
            !state.ui.post_committed_quickbar_item_refresh_pending,
            "cleared post-quickbar state must cancel stale compact item proof"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_updates,
            0
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_proof_class, None,
            "cleared post-quickbar state must also clear the pending proof class"
        );
        assert_eq!(
            state.ui.post_committed_quickbar_item_refresh_pending_events, 0,
            "cleared post-quickbar state should also clear pending event accounting"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_pending_event_breakdown,
            Default::default(),
            "cleared post-quickbar state should also clear pending event buckets"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_first_followup_event,
            None,
            "cleared post-quickbar state should also clear first-follow-up tracking"
        );
        assert_eq!(
            state
                .ui
                .post_committed_quickbar_item_refresh_first_client_action,
            None,
            "cleared post-quickbar state should also clear first-client-action tracking"
        );
        assert_eq!(
            state.ui.unresolved_pending_item_refresh(),
            None,
            "area-reset-cleared proof should not remain an unresolved pending refresh"
        );

        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GuiQuickbar),
            &quickbar_payload,
        );

        assert_eq!(
            state.ui.last_committed_quickbar_previous_post_item_context,
            Some(cleared_context)
        );
        assert!(
            !state.ui.last_committed_quickbar_item_refresh_pending,
            "the next committed quickbar should not report stale proof as pending"
        );
        assert_eq!(
            state
                .ui
                .last_committed_quickbar_item_refresh_pending_updates,
            0
        );
        assert_eq!(
            state.ui.last_committed_quickbar_item_refresh_proof_class, None,
            "the later committed quickbar should not inherit a stale pending proof class"
        );
    }

    fn client_use_item_payload(item_object_id: u32) -> Vec<u8> {
        const DECLARED: usize = 12;
        let mut payload = Vec::with_capacity(DECLARED + 1);
        payload.extend_from_slice(&[0x70, 0x06, 0x09]);
        payload.extend_from_slice(&(DECLARED as u32).to_le_bytes());
        payload.extend_from_slice(&item_object_id.to_le_bytes());
        payload.push(0);
        // CNW fragment header says six final bits are owned: three header bits
        // plus UseItem's three false optional branch BOOLs.
        payload.push(0xC0);
        payload
    }

    fn client_quickbar_item_set_button_payload(slot: u8, item_object_id: u32) -> Vec<u8> {
        const DECLARED: usize = 18;
        let mut payload = Vec::with_capacity(DECLARED + 1);
        payload.extend_from_slice(&[0x70, 0x1E, 0x02]);
        payload.extend_from_slice(&(DECLARED as u32).to_le_bytes());
        payload.push(slot);
        payload.push(1);
        payload.extend_from_slice(&item_object_id.to_le_bytes());
        payload.extend_from_slice(&(-1i32).to_le_bytes());
        payload.push(0);
        payload.push(0x60);
        payload
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn exact_gui_item_create_materializes_item_ids_for_quickbar_context() {
        let mut payload =
            include_bytes!("../../../fixtures/live_object/player_hide_inventory_gui_span.bin")
                .to_vec();
        live_object_update::rewrite_update_records_payload_if_possible(&mut payload)
            .expect("fixture should rewrite legacy GUI item-create to exact EE shape");
        let claim = live_object_update::claim_payload_if_verified(&payload)
            .expect("fixture should be an exact verified live-object payload");
        assert!(
            !claim.materialized_item_object_ids.is_empty(),
            "fixture should expose GUI item-create materialization ids"
        );

        let mut state = SemanticSessionState::default();
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &payload,
        );

        assert!(
            claim
                .materialized_item_object_ids
                .iter()
                .any(|object_id| state.objects.has_active_object_id(*object_id)),
            "exact GUI item materialization should become quickbar object proof"
        );
    }

    #[test]
    fn exact_session_creature_add_materializes_playerlist_session_id() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_diamond_seq15_coalesced_liveobject_20260516_unclaimed.bin"
        )
        .to_vec();

        let _ = live_object_update::rewrite_update_records_payload_if_possible(&mut payload);
        let _ =
            crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
                &mut payload,
                None,
            );
        let _ = live_object_update::rewrite_update_records_payload_if_possible(&mut payload);
        let _ =
            live_object_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
        let _ =
            crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
                &mut payload,
                None,
            );
        let _ = live_object_update::rewrite_update_records_payload_if_possible(&mut payload);
        live_object_update::canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
            .expect("fixture should first canonicalize to EE external compact id");
        live_object_update::canonicalize_player_session_creature_ids_payload_for_ee(
            &mut payload,
            |compact_id| (compact_id == 0xFE).then_some(0xFFFF_FFFE),
        )
        .expect("fixture should canonicalize to PlayerList-proven session id");

        let mut state = SemanticSessionState::default();
        observe_verified_payload(
            &mut state,
            Direction::ServerToClient,
            &VerifiedProof::Family(VerifiedFamily::GameObjUpdateLiveObject),
            &payload,
        );

        assert!(
            state.objects.has_active_typed_object(0x05, 0xFFFF_FFFE),
            "exact live-object add should materialize the PlayerList session creature id"
        );
    }
}
