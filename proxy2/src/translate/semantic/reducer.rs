//! Semantic state reducer.
//!
//! Packet-family translators produce and validate bytes. The reducer only
//! consumes the already-verified family proof plus the high-level payload that
//! will be emitted. If a future translator needs richer state, add a typed event
//! here rather than reaching back into transport or byte-rewrite modules.

use crate::{
    packet::{Direction, m::HighLevel},
    translate::{
        VerifiedFamily, VerifiedProof, area, gameplay_stream, live_object_update, player_list,
        quickbar,
    },
};

use super::{
    AreaEvent, ChatEvent, ClientInputEvent, InventoryEvent, LiveObjectEvent,
    LiveObjectInventoryFeature25Reference, LiveObjectMention, LiveObjectOrientation,
    LiveObjectOrientationSource, LiveObjectOrientationVector, LiveObjectPlaceableState,
    LiveObjectPosition, LoginEvent, ModuleInfoEvent, ObservedHighLevel, PlayerListEvent,
    ProtocolEvent, QuickbarEvent, SemanticSessionState, ServerStatusEvent,
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
        VerifiedFamily::ClientInput => ProtocolEvent::ClientInput(ClientInputEvent { observed }),
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
                state.ui.last_committed_quickbar_profile = Some(*profile);
                state.ui.last_committed_quickbar_materialization_context =
                    Some(*materialization_context);
                state.ui.last_committed_quickbar_prior_item_context = prior_item_context;
                state
                    .ui
                    .last_inventory_item_context_after_committed_quickbar = None;
                state
                    .ui
                    .inventory_item_context_after_committed_quickbar_updates = 0;
                let prior_item_context_known = prior_item_context.is_some();
                let prior_item_context = prior_item_context.unwrap_or_default();
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
        ProtocolEvent::ClientInput(_) => {
            state.auth.client_input_packets = state.auth.client_input_packets.saturating_add(1);
        }
        ProtocolEvent::Login(_) => {
            state.auth.login_packets = state.auth.login_packets.saturating_add(1);
        }
        ProtocolEvent::Chat(_) | ProtocolEvent::Other(_) => {}
    }
    state.remember_event(event);
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
        tracing::info!(
            source,
            updates_since_committed_quickbar = state
                .ui
                .inventory_item_context_after_committed_quickbar_updates,
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
            "semantic state retained inventory item context after committed GuiQuickbar"
        );
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
            state
                .ui
                .inventory_item_context_after_committed_quickbar_updates,
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
