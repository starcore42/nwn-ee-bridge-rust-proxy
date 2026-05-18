//! Typed protocol events emitted after strict semantic ownership is proven.
//!
//! These events are intentionally small. A packet-family translator may expose
//! richer typed models later, but the reducer should only receive facts already
//! proven by an exact parser/writer/validator path.

use crate::{
    packet::Direction,
    translate::{VerifiedFamily, player_list::PlayerListObjectIds},
};

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
    Inventory(InventoryEvent),
    ClientInput(ClientInputEvent),
    Login(LoginEvent),
    Chat(ChatEvent),
    Other(ObservedHighLevel),
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
    pub(crate) materialized_item_object_ids: Vec<u32>,
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LiveObjectPosition {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LiveObjectOrientation {
    pub(crate) scalar_tenths_degrees: u16,
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

#[derive(Debug, Clone)]
pub(crate) enum QuickbarEvent {
    Verified { observed: ObservedHighLevel },
    Placeholder { observed: ObservedHighLevel },
}

#[derive(Debug, Clone)]
pub(crate) struct InventoryEvent {
    pub(crate) observed: ObservedHighLevel,
}

#[derive(Debug, Clone)]
pub(crate) struct ClientInputEvent {
    pub(crate) observed: ObservedHighLevel,
}

#[derive(Debug, Clone)]
pub(crate) struct LoginEvent {
    pub(crate) observed: ObservedHighLevel,
}

#[derive(Debug, Clone)]
pub(crate) struct ChatEvent {
    pub(crate) observed: ObservedHighLevel,
}
