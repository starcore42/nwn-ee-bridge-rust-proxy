//! Semantic event layer for the protocol gateway.
//!
//! Translators still own packet-family parsing and dialect writing. This layer
//! records the typed event that a verified packet family represents so the
//! proxy can keep only the session state needed to make later protocol traffic
//! coherent. It is deliberately not a gameplay authority: the legacy server
//! remains the source of truth.

mod event;
mod reducer;
mod state;

pub(crate) use event::{
    AreaEvent, ChatEvent, ClientInputEvent, InventoryEvent, LiveObjectBounds, LiveObjectEvent,
    LiveObjectMention, LiveObjectOrientation, LiveObjectOrientationSource,
    LiveObjectOrientationVector, LiveObjectPlaceableAppearance, LiveObjectPlaceableState,
    LiveObjectPosition, LoginEvent, ModuleInfoEvent, ObservedHighLevel, PlayerListEvent,
    ProtocolEvent, QuickbarEvent, ServerStatusEvent,
};
pub(crate) use reducer::{observe_verified_payload, observe_verified_payload_with_area_context};
pub(crate) use state::{
    AreaState, AreaStaticPlaceableConflictRecordSummary, AuthState, KnownObjectState, ModuleState,
    ObjectRegistry, ResourceState, SemanticSessionState, SyntheticState, UiState,
};
