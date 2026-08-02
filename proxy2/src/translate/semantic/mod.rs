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
    ActiveItemPropertiesEvent, AreaEvent, ChatEvent, ClientGuiEventEvent, ClientInputEvent,
    ClientInventoryEvent, ClientQuickbarEvent, InventoryEvent, LiveObjectBounds, LiveObjectEvent,
    LiveObjectInventoryFeature25Reference, LiveObjectInventoryOwner, LiveObjectMention,
    LiveObjectOrientation, LiveObjectOrientationSource, LiveObjectOrientationVector,
    LiveObjectPlaceableAppearance, LiveObjectPlaceableState, LiveObjectPosition, LoginEvent,
    ModuleInfoEvent, ObjectControlEvent, ObservedHighLevel, PlayerListEvent, ProtocolEvent,
    QuickbarEvent, ServerStatusEvent,
};
pub(crate) use reducer::{
    CommittedQuickbarUnitProbe, LiveObjectInventoryMaterializationObservation,
    observe_verified_payload, observe_verified_payload_with_area_context,
    observe_verified_payload_with_area_context_report_and_committed_quickbar_probes,
};
pub(crate) use state::{
    AreaState, AreaStaticPlaceableConflictRecordObservation,
    AreaStaticPlaceableConflictRecordProgressSummary, AreaStaticPlaceableConflictRecordSummary,
    AuthState, InventoryEquipmentBridgeStateUpdate, InventoryEquipmentClientGuiInventoryClaim,
    InventoryEquipmentClientGuiInventoryClaimKind, InventoryEquipmentHandoffConsumer,
    InventoryEquipmentProtocolState, InventoryEquipmentServerInventoryClaim,
    InventoryItemContextCandidate, InventoryItemContextCandidateSource,
    InventoryItemContextSummary, InventoryItemObjectProof, InventoryItemObjectProvenNeighbor,
    InventoryItemObjectProvenNeighborhood, InventoryItemObjectStatus, KnownObjectState,
    LiveObjectInventoryMaterializationSummary, MAX_VISIBLE_EQUIPMENT_UPDATE_OBSERVATIONS_PER_AREA,
    ModuleState, ObjectRegistry, QuickbarItemContextSource, QuickbarItemRefreshOutcome,
    ResourceState, SemanticSessionState, StatusAuthorizedVisibleEquipmentProbeAuthorization,
    SyntheticState, UiState,
};
