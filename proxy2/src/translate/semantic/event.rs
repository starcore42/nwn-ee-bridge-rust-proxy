//! Typed protocol events emitted after strict semantic ownership is proven.
//!
//! These events are intentionally small. A packet-family translator may expose
//! richer typed models later, but the reducer should only receive facts already
//! proven by an exact parser/writer/validator path.

use crate::{
    packet::Direction,
    translate::{
        VerifiedFamily, client_gui_event::ClientGuiEventClaimSummary,
        client_gui_inventory::ClientGuiInventoryClaimSummary,
        client_input::ClientInputClaimSummary, client_quickbar::ClientQuickbarClaimSummary,
        inventory::InventoryClaimSummary,
        item_update_active_props::ActiveItemPropertiesClaimSummary,
        live_object_update::LiveObjectQuickbarItemUseCountUpdate, player_list::PlayerListObjectIds,
        quickbar::QuickbarValidatedSlotProfile,
    },
};

use super::state::InventoryItemContextSummary;

#[derive(Debug, Clone)]
pub(crate) struct ObservedHighLevel {
    pub(crate) direction: Direction,
    pub(crate) family: VerifiedFamily,
    pub(crate) major: Option<u8>,
    pub(crate) minor: Option<u8>,
    pub(crate) packet_name: Option<&'static str>,
    pub(crate) payload_len: usize,
    pub(crate) declared_len: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) enum ProtocolEvent {
    ModuleInfo(ModuleInfoEvent),
    ServerStatus(ServerStatusEvent),
    Area(AreaEvent),
    LiveObject(LiveObjectEvent),
    PlayerList(PlayerListEvent),
    Quickbar(QuickbarEvent),
    ActiveItemProperties(ActiveItemPropertiesEvent),
    Inventory(InventoryEvent),
    ClientGuiEvent(ClientGuiEventEvent),
    ClientInput(ClientInputEvent),
    ClientQuickbar(ClientQuickbarEvent),
    Login(LoginEvent),
    Chat(ChatEvent),
    Other(ObservedHighLevel),
}

impl ProtocolEvent {
    pub(crate) fn observed(&self) -> &ObservedHighLevel {
        match self {
            ProtocolEvent::ModuleInfo(event) => &event.observed,
            ProtocolEvent::ServerStatus(ServerStatusEvent::ModuleResources { observed })
            | ProtocolEvent::ServerStatus(ServerStatusEvent::ModuleRunning { observed }) => {
                observed
            }
            ProtocolEvent::Area(AreaEvent::ClientArea { observed, .. })
            | ProtocolEvent::Area(AreaEvent::AreaLoaded { observed })
            | ProtocolEvent::Area(AreaEvent::LoadBar { observed }) => observed,
            ProtocolEvent::LiveObject(event) => &event.observed,
            ProtocolEvent::PlayerList(event) => &event.observed,
            ProtocolEvent::Quickbar(QuickbarEvent::Verified { observed, .. })
            | ProtocolEvent::Quickbar(QuickbarEvent::Placeholder { observed }) => observed,
            ProtocolEvent::ActiveItemProperties(event) => &event.observed,
            ProtocolEvent::Inventory(event) => &event.observed,
            ProtocolEvent::ClientGuiEvent(event) => &event.observed,
            ProtocolEvent::ClientInput(event) => &event.observed,
            ProtocolEvent::ClientQuickbar(event) => &event.observed,
            ProtocolEvent::Login(event) => &event.observed,
            ProtocolEvent::Chat(event) => &event.observed,
            ProtocolEvent::Other(observed) => observed,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ModuleInfoEvent {
    pub(crate) observed: ObservedHighLevel,
}

#[derive(Debug, Clone)]
pub(crate) enum ServerStatusEvent {
    ModuleResources { observed: ObservedHighLevel },
    ModuleRunning { observed: ObservedHighLevel },
}

#[derive(Debug, Clone)]
pub(crate) enum AreaEvent {
    ClientArea {
        observed: ObservedHighLevel,
        area_object_id: Option<u32>,
    },
    AreaLoaded {
        observed: ObservedHighLevel,
    },
    LoadBar {
        observed: ObservedHighLevel,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct LiveObjectEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) mentions: Vec<LiveObjectMention>,
    pub(crate) live_gui_records: u32,
    pub(crate) live_gui_fragment_bits: u32,
    pub(crate) materialized_item_object_ids: Vec<u32>,
    pub(crate) inventory_feature25_references: Vec<LiveObjectInventoryFeature25Reference>,
    pub(crate) quickbar_item_use_count_records: u32,
    pub(crate) quickbar_item_use_count_rows: u32,
    pub(crate) quickbar_item_use_count_updates: Vec<LiveObjectQuickbarItemUseCountUpdate>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlayerListEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) object_ids: Vec<PlayerListObjectIds>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LiveObjectMention {
    pub(crate) opcode: u8,
    pub(crate) object_type: u8,
    pub(crate) object_id: u32,
    pub(crate) name: Option<String>,
    pub(crate) position: Option<LiveObjectPosition>,
    pub(crate) orientation: Option<LiveObjectOrientation>,
    pub(crate) bounds: Option<LiveObjectBounds>,
    pub(crate) placeable_appearance: Option<LiveObjectPlaceableAppearance>,
    pub(crate) placeable_state: Option<LiveObjectPlaceableState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveObjectInventoryFeature25Reference {
    pub(crate) owner_id: u32,
    pub(crate) mask: u16,
    pub(crate) first_object_ids: Vec<u32>,
    pub(crate) second_object_ids: Vec<u32>,
    pub(crate) legacy_tail_object_ids: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LiveObjectPosition {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LiveObjectOrientation {
    pub(crate) source: LiveObjectOrientationSource,
    pub(crate) scalar_tenths_degrees: u16,
    pub(crate) vector: Option<LiveObjectOrientationVector>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveObjectOrientationSource {
    Scalar,
    Vector,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LiveObjectOrientationVector {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LiveObjectBounds {
    pub(crate) min_x: f32,
    pub(crate) min_y: f32,
    pub(crate) min_z: f32,
    pub(crate) max_x: f32,
    pub(crate) max_y: f32,
    pub(crate) max_z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveObjectPlaceableAppearance {
    pub(crate) appearance: u16,
    pub(crate) resref: Option<[u8; 16]>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LiveObjectPlaceableState {
    pub(crate) useable: Option<bool>,
    pub(crate) trap_disarmable: Option<bool>,
    pub(crate) lockable: Option<bool>,
    pub(crate) locked: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) enum QuickbarEvent {
    Verified {
        observed: ObservedHighLevel,
        profile: Option<QuickbarValidatedSlotProfile>,
        materialization_context: InventoryItemContextSummary,
    },
    Placeholder {
        observed: ObservedHighLevel,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveItemPropertiesEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) claim: ActiveItemPropertiesClaimSummary,
}

#[derive(Debug, Clone)]
pub(crate) struct InventoryEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) inventory_claim: Option<InventoryClaimSummary>,
    pub(crate) client_gui_inventory_claim: Option<ClientGuiInventoryClaimSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClientGuiEventEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) claim: Option<ClientGuiEventClaimSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClientInputEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) claim: Option<ClientInputClaimSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClientQuickbarEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) claim: Option<ClientQuickbarClaimSummary>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoginEvent {
    pub(crate) observed: ObservedHighLevel,
}

#[derive(Debug, Clone)]
pub(crate) struct ChatEvent {
    pub(crate) observed: ObservedHighLevel,
}
