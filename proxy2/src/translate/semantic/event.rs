//! Typed protocol events emitted after strict semantic ownership is proven.
//!
//! These events are intentionally small. A packet-family translator may expose
//! richer typed models later, but the reducer should only receive facts already
//! proven by an exact parser/writer/validator path.

use crate::{packet::Direction, translate::VerifiedFamily};

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
    ClientArea { observed: ObservedHighLevel },
    AreaLoaded { observed: ObservedHighLevel },
    LoadBar { observed: ObservedHighLevel },
}

#[derive(Debug, Clone)]
pub(crate) struct LiveObjectEvent {
    pub(crate) observed: ObservedHighLevel,
    pub(crate) mentions: Vec<LiveObjectMention>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveObjectMention {
    pub(crate) opcode: u8,
    pub(crate) object_type: u8,
    pub(crate) object_id: u32,
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
