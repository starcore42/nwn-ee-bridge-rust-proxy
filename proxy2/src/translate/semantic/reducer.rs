//! Semantic state reducer.
//!
//! Packet-family translators produce and validate bytes. The reducer only
//! consumes the already-verified family proof plus the high-level payload that
//! will be emitted. If a future translator needs richer state, add a typed event
//! here rather than reaching back into transport or byte-rewrite modules.

use crate::{
    packet::{Direction, m::HighLevel},
    translate::{VerifiedFamily, VerifiedProof, gameplay_stream},
};

use super::{
    AreaEvent, ChatEvent, ClientInputEvent, InventoryEvent, LiveObjectEvent,
    LoginEvent, ModuleInfoEvent, ObservedHighLevel, ProtocolEvent, QuickbarEvent,
    SemanticSessionState, ServerStatusEvent,
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
            let observed = observed_high_level(
                direction,
                VerifiedFamily::CoalescedWindow,
                payload,
            );
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
            let family = family_iter.next().unwrap_or(VerifiedFamily::SemanticDeflated);
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
        VerifiedFamily::AreaClientArea => ProtocolEvent::Area(AreaEvent::ClientArea { observed }),
        VerifiedFamily::ClientArea => ProtocolEvent::Area(AreaEvent::AreaLoaded { observed }),
        VerifiedFamily::LoadBar => ProtocolEvent::Area(AreaEvent::LoadBar { observed }),
        VerifiedFamily::GameObjUpdateLiveObject => {
            // Do not infer object registry facts by scanning arbitrary live
            // bytes. The exact live-object add/update parsers should attach
            // typed mentions once they have proven record boundaries and
            // object ids. Until then, this event records only that a verified
            // live-object packet passed through the gateway.
            ProtocolEvent::LiveObject(LiveObjectEvent {
                observed,
                mentions: Vec::new(),
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
        ProtocolEvent::Area(AreaEvent::ClientArea { observed }) => {
            state.area.client_area_packets = state.area.client_area_packets.saturating_add(1);
            state.area.last_client_area_declared_len = observed.declared_len;
        }
        ProtocolEvent::Area(AreaEvent::AreaLoaded { .. }) => {
            state.area.area_loaded_packets = state.area.area_loaded_packets.saturating_add(1);
        }
        ProtocolEvent::Area(AreaEvent::LoadBar { .. }) => {
            state.area.loadbar_packets = state.area.loadbar_packets.saturating_add(1);
        }
        ProtocolEvent::LiveObject(event) => {
            state.objects.observe_mentions(&event.mentions);
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
