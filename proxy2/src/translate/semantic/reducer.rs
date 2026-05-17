//! Semantic state reducer.
//!
//! Packet-family translators produce and validate bytes. The reducer only
//! consumes the already-verified family proof plus the high-level payload that
//! will be emitted. If a future translator needs richer state, add a typed event
//! here rather than reaching back into transport or byte-rewrite modules.

use crate::{
    packet::{Direction, m::HighLevel},
    translate::{VerifiedFamily, VerifiedProof, gameplay_stream, live_object_update},
};

use super::{
    AreaEvent, ChatEvent, ClientInputEvent, InventoryEvent, LiveObjectEvent, LiveObjectMention,
    LiveObjectOrientation, LiveObjectPosition, LoginEvent, ModuleInfoEvent, ObservedHighLevel,
    ProtocolEvent, QuickbarEvent, SemanticSessionState, ServerStatusEvent,
};

pub(crate) fn observe_verified_payload(
    state: &mut SemanticSessionState,
    direction: Direction,
    proof: &VerifiedProof,
    payload: &[u8],
) {
    match proof {
        VerifiedProof::Family(family) => observe_family_payload(state, direction, *family, payload),
        VerifiedProof::GameplayStream(families) => {
            observe_gameplay_stream_payload(state, direction, families, payload);
        }
        VerifiedProof::CoalescedWindow(_) => {
            let observed = observed_high_level(direction, VerifiedFamily::CoalescedWindow, payload);
            apply_event(state, ProtocolEvent::Other(observed));
        }
    }
}

fn observe_gameplay_stream_payload(
    state: &mut SemanticSessionState,
    direction: Direction,
    families: &[VerifiedFamily],
    payload: &[u8],
) {
    let split = gameplay_stream::split_inflated_gameplay(payload);
    let mut family_iter = families.iter().copied();
    for unit in split.units {
        if let gameplay_stream::GameplayUnit::HighLevel(message) = unit {
            let family = family_iter
                .next()
                .unwrap_or(VerifiedFamily::SemanticDeflated);
            observe_family_payload(state, direction, family, message.payload);
        }
    }

    for family in family_iter {
        let observed = observed_high_level(direction, family, payload);
        apply_event(state, ProtocolEvent::Other(observed));
    }
}

fn observe_family_payload(
    state: &mut SemanticSessionState,
    direction: Direction,
    family: VerifiedFamily,
    payload: &[u8],
) {
    let observed = observed_high_level(direction, family, payload);
    let event = match family {
        VerifiedFamily::ModuleInfo => ProtocolEvent::ModuleInfo(ModuleInfoEvent { observed }),
        VerifiedFamily::ServerStatusModuleResources => {
            ProtocolEvent::ServerStatus(ServerStatusEvent::ModuleResources { observed })
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
            let (mentions, materialized_item_object_ids) =
                live_object_observations_from_payload(payload);
            ProtocolEvent::LiveObject(LiveObjectEvent {
                observed,
                mentions,
                materialized_item_object_ids,
            })
        }
        VerifiedFamily::GuiQuickbar => {
            ProtocolEvent::Quickbar(QuickbarEvent::Verified { observed })
        }
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
    apply_event(state, event);
}

fn apply_event(state: &mut SemanticSessionState, event: ProtocolEvent) {
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
        }
        ProtocolEvent::Area(AreaEvent::AreaLoaded { .. }) => {
            state.area.area_loaded_packets = state.area.area_loaded_packets.saturating_add(1);
        }
        ProtocolEvent::Area(AreaEvent::LoadBar { .. }) => {
            state.area.loadbar_packets = state.area.loadbar_packets.saturating_add(1);
        }
        ProtocolEvent::LiveObject(event) => {
            state.objects.observe_mentions(&event.mentions);
            state
                .objects
                .observe_materialized_item_object_ids(&event.materialized_item_object_ids);
        }
        ProtocolEvent::Quickbar(QuickbarEvent::Verified { observed }) => {
            state.ui.quickbar_packets = state.ui.quickbar_packets.saturating_add(1);
            state.ui.last_quickbar_family = Some(observed.family);
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

fn live_object_observations_from_payload(payload: &[u8]) -> (Vec<LiveObjectMention>, Vec<u32>) {
    let Some(claim) = live_object_update::claim_payload_if_verified(payload) else {
        return (Vec::new(), Vec::new());
    };
    let materialized_item_object_ids = claim.materialized_item_object_ids;
    let mentions = claim
        .mentions
        .into_iter()
        .map(|mention| LiveObjectMention {
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
                    scalar_tenths_degrees: orientation.scalar_tenths_degrees,
                }),
            bounds: mention.bounds.map(|bounds| super::LiveObjectBounds {
                min_x: bounds.min_x,
                min_y: bounds.min_y,
                min_z: bounds.min_z,
                max_x: bounds.max_x,
                max_y: bounds.max_y,
                max_z: bounds.max_z,
            }),
        })
        .collect();
    (mentions, materialized_item_object_ids)
}

fn current_area_object_id_from_payload(payload: &[u8]) -> Option<u32> {
    const AREA_OBJECT_ID_OFFSET: usize = 3 + 4 + 4 + 4 * 4;
    read_u32_le(payload, AREA_OBJECT_ID_OFFSET)
}

#[cfg(test)]
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
}
