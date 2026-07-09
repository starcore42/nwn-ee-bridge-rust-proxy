//! Typed M-frame session substates.
//!
//! Keep the root M-frame dispatcher from becoming a god-state container. Each
//! substate below owns one transport concern, so future packet-family work has
//! an obvious home instead of casually adding fields to `SessionState`.

use std::path::PathBuf;

use flate2::Decompress;

use crate::translate::{ContinuationOwner, VerifiedProof, area, module_resources, semantic};

use super::{
    client_ack, deferred_module_resources, live_stream, quickbar_stream,
    reassembly::{CompletedDeflatedStreamWindow, ServerDeflatedReassembly},
    sequence::{CoalescedSplitSequenceShift, SequenceElision, SequenceShift},
    synthetic_area,
};

#[derive(Debug, Default)]
pub(super) struct DeflateState {
    pub(super) server_reassembly: Option<ServerDeflatedReassembly>,
    pub(super) server_zlib_inflater: Option<Decompress>,
    pub(super) completed_server_stream_windows: Vec<CompletedDeflatedStreamWindow>,
    pub(super) completed_coalesced_stream_records: Vec<CompletedCoalescedStreamRecord>,
    pub(super) server_zlib_stream_proxy_owned: bool,
    pub(super) server_zlib_stream_owner: Option<ContinuationOwner>,
    pub(super) server_zlib_stream_epoch: u64,
}

#[derive(Debug, Clone)]
pub(super) struct CompletedCoalescedStreamRecord {
    pub(super) sequence: u16,
    pub(super) offset: usize,
    pub(super) payload_length: usize,
    pub(super) inflated_length: usize,
    pub(super) compressed: Vec<u8>,
    pub(super) proof: VerifiedProof,
    pub(super) record: Vec<u8>,
    pub(super) dropped: bool,
    pub(super) rewritten_deflated: bool,
}

#[derive(Debug, Default)]
pub(super) struct QuickbarStreamState {
    pub(super) pending_stream: Option<quickbar_stream::PendingQuickbarStream>,
}

#[derive(Debug, Default)]
pub(super) struct LiveObjectStreamState {
    pub(super) pending_stream: Option<live_stream::PendingLiveObjectStream>,
}

#[derive(Debug, Default)]
pub(super) struct SequenceState {
    pub(super) latest_client_sequence_from_client: Option<u16>,
    pub(super) latest_client_ack_from_client: Option<u16>,
    /// Latest server-origin reliable sequence emitted toward the EE client,
    /// after any proxy-owned sequence shifts have been applied. This is
    /// gateway transport state, not game truth: synthetic client M frames use
    /// it only to carry a coherent receive-window ACK when no native client
    /// packet is available to piggyback on.
    pub(super) latest_server_sequence_to_client: Option<u16>,
    pub(super) client_sequence_shifts: Vec<SequenceShift>,
    pub(super) client_sequence_elisions: Vec<SequenceElision>,
    pub(super) server_sequence_shifts: Vec<SequenceShift>,
    pub(super) coalesced_split_sequence_shifts: Vec<CoalescedSplitSequenceShift>,
    pub(super) pending_client_to_server_packets: Vec<Vec<u8>>,
}

#[derive(Debug, Default)]
pub(super) struct ClientAckSessionState {
    pub(super) pending: client_ack::ClientAckState,
}

#[derive(Debug, Default)]
pub(super) struct LoginWaypointState {
    pub(super) last_server_get_waypoint_sequence: Option<u16>,
    pub(super) synthetic_empty_response_count: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct InventoryEquipmentBridgeQueuedOutput {
    pub(super) update_index: u64,
    pub(super) emission_index: u64,
    pub(super) event_index: u64,
    pub(super) minor: u8,
    pub(super) object_id: u32,
    pub(super) result: bool,
    pub(super) equip_slot: u32,
    pub(super) trigger_sequence: u16,
    pub(super) synthetic_sequence: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct InventoryEquipmentBridgeQueuedClientGuiStatusOutput {
    pub(super) update_index: u64,
    pub(super) emission_index: u64,
    pub(super) event_index: u64,
    pub(super) object_id: u32,
    pub(super) player_inventory_gui: bool,
    pub(super) trigger_client_sequence: u16,
    pub(super) synthetic_sequence: u16,
    pub(super) ack_sequence: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct InventoryEquipmentBridgeClientGuiStatusResponse {
    pub(super) queued_update_index: u64,
    pub(super) server_sequence: u16,
    pub(super) ack_sequence: u16,
    pub(super) live_gui_records: u32,
    pub(super) live_gui_fragment_bits: u32,
    pub(super) materialized_item_object_ids: usize,
    pub(super) compact_item_emission_ready_objects: usize,
    pub(super) compact_item_emission_ready_candidate:
        Option<semantic::InventoryItemContextCandidate>,
}

impl InventoryEquipmentBridgeClientGuiStatusResponse {
    fn strength(self) -> u8 {
        if self.materialized_item_object_ids != 0 {
            3
        } else if self.live_gui_records != 0 {
            2
        } else {
            1
        }
    }

    pub(super) fn is_stronger_than(self, other: Self) -> bool {
        let self_strength = self.strength();
        let other_strength = other.strength();
        self_strength > other_strength
            || (self_strength == other_strength
                && (
                    self.materialized_item_object_ids,
                    self.live_gui_records,
                    self.live_gui_fragment_bits,
                    self.server_sequence,
                ) > (
                    other.materialized_item_object_ids,
                    other.live_gui_records,
                    other.live_gui_fragment_bits,
                    other.server_sequence,
                ))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InventoryEquipmentBridgeClientGuiStatusResponseOutcome {
    #[default]
    None,
    AwaitingResponse,
    LiveObjectOnly,
    LiveGuiRecords,
    MaterializedItems,
}

impl InventoryEquipmentBridgeClientGuiStatusResponseOutcome {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AwaitingResponse => "awaiting_response",
            Self::LiveObjectOnly => "live_object_only",
            Self::LiveGuiRecords => "live_gui_records",
            Self::MaterializedItems => "materialized_items",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InventoryEquipmentBridgeOutputDecisionKind {
    #[default]
    None,
    QueuedInventoryOutput,
    QueuedClientGuiStatusOutput,
    DeferredClientGui,
    DeferredMissingClaim,
    BlockedCandidateMismatch,
}

impl InventoryEquipmentBridgeOutputDecisionKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::QueuedInventoryOutput => "queued_inventory_output",
            Self::QueuedClientGuiStatusOutput => "queued_client_gui_status_output",
            Self::DeferredClientGui => "deferred_client_gui",
            Self::DeferredMissingClaim => "deferred_missing_claim",
            Self::BlockedCandidateMismatch => "blocked_candidate_mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InventoryEquipmentBridgeOutputStatus {
    #[default]
    AwaitingBridgeStateUpdate,
    QueuedInventoryOutput,
    QueuedClientGuiStatusOutput,
    BlockedCandidateMismatch,
    DeferredMissingClaim,
    AwaitingClientGuiWriter,
    DecisionRecordedWithoutDetail,
}

impl InventoryEquipmentBridgeOutputStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingBridgeStateUpdate => "awaiting_bridge_state_update",
            Self::QueuedInventoryOutput => "queued_inventory_output",
            Self::QueuedClientGuiStatusOutput => "queued_client_gui_status_output",
            Self::BlockedCandidateMismatch => "blocked_candidate_mismatch",
            Self::DeferredMissingClaim => "deferred_missing_claim",
            Self::AwaitingClientGuiWriter => "awaiting_client_gui_writer",
            Self::DecisionRecordedWithoutDetail => "decision_recorded_without_detail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InventoryEquipmentBridgeOutputDecision {
    pub(super) kind: InventoryEquipmentBridgeOutputDecisionKind,
    pub(super) update_index: u64,
    pub(super) emission_index: u64,
    pub(super) event_index: u64,
    pub(super) consumer: semantic::InventoryEquipmentHandoffConsumer,
    pub(super) candidate: semantic::InventoryItemContextCandidate,
    pub(super) candidate_object_status: semantic::InventoryItemObjectStatus,
    pub(super) ready_objects: usize,
    pub(super) deferred_feature25_only_objects: usize,
    pub(super) server_inventory_claim: Option<semantic::InventoryEquipmentServerInventoryClaim>,
    pub(super) server_inventory_claim_object_status: semantic::InventoryItemObjectStatus,
    pub(super) server_inventory_claim_proven_neighborhood:
        semantic::InventoryItemObjectProvenNeighborhood,
    pub(super) client_gui_inventory_claim:
        Option<semantic::InventoryEquipmentClientGuiInventoryClaim>,
}

#[derive(Debug, Default)]
pub(super) struct InventoryEquipmentBridgeState {
    pub(super) last_decision_state_update_index: Option<u64>,
    pub(super) last_queued_state_update_index: Option<u64>,
    pub(super) last_queued_client_gui_status_update_index: Option<u64>,
    pub(super) queued_outputs: u64,
    pub(super) queued_client_gui_status_outputs: u64,
    pub(super) client_gui_status_response_live_object_packets: u64,
    pub(super) client_gui_status_response_live_gui_record_packets: u64,
    pub(super) client_gui_status_response_materialized_item_packets: u64,
    pub(super) deferred_client_gui_updates: u64,
    pub(super) deferred_missing_claim_updates: u64,
    pub(super) blocked_candidate_mismatch_updates: u64,
    pub(super) last_deferred_client_gui_update_index: Option<u64>,
    pub(super) last_deferred_missing_claim_update_index: Option<u64>,
    pub(super) last_blocked_candidate_mismatch_update_index: Option<u64>,
    pub(super) last_decision: Option<InventoryEquipmentBridgeOutputDecision>,
    pub(super) last_queued_output: Option<InventoryEquipmentBridgeQueuedOutput>,
    pub(super) last_queued_client_gui_status_output:
        Option<InventoryEquipmentBridgeQueuedClientGuiStatusOutput>,
    pub(super) last_client_gui_status_response:
        Option<InventoryEquipmentBridgeClientGuiStatusResponse>,
    pub(super) best_client_gui_status_response:
        Option<InventoryEquipmentBridgeClientGuiStatusResponse>,
}

impl InventoryEquipmentBridgeState {
    pub(super) fn output_status(&self) -> InventoryEquipmentBridgeOutputStatus {
        if self.queued_outputs > 0 {
            InventoryEquipmentBridgeOutputStatus::QueuedInventoryOutput
        } else if self.queued_client_gui_status_outputs > 0 {
            InventoryEquipmentBridgeOutputStatus::QueuedClientGuiStatusOutput
        } else if self.blocked_candidate_mismatch_updates > 0 {
            InventoryEquipmentBridgeOutputStatus::BlockedCandidateMismatch
        } else if self.deferred_missing_claim_updates > 0 {
            InventoryEquipmentBridgeOutputStatus::DeferredMissingClaim
        } else if self.deferred_client_gui_updates > 0 {
            InventoryEquipmentBridgeOutputStatus::AwaitingClientGuiWriter
        } else if self.last_decision_state_update_index.is_some() {
            InventoryEquipmentBridgeOutputStatus::DecisionRecordedWithoutDetail
        } else {
            InventoryEquipmentBridgeOutputStatus::AwaitingBridgeStateUpdate
        }
    }

    pub(super) fn requires_client_gui_writer(&self) -> bool {
        self.output_status() == InventoryEquipmentBridgeOutputStatus::AwaitingClientGuiWriter
    }

    pub(super) fn client_gui_status_response_outcome(
        &self,
    ) -> InventoryEquipmentBridgeClientGuiStatusResponseOutcome {
        if self.client_gui_status_response_materialized_item_packets > 0 {
            InventoryEquipmentBridgeClientGuiStatusResponseOutcome::MaterializedItems
        } else if self.client_gui_status_response_live_gui_record_packets > 0 {
            InventoryEquipmentBridgeClientGuiStatusResponseOutcome::LiveGuiRecords
        } else if self.client_gui_status_response_live_object_packets > 0 {
            InventoryEquipmentBridgeClientGuiStatusResponseOutcome::LiveObjectOnly
        } else if self.queued_client_gui_status_outputs > 0 {
            InventoryEquipmentBridgeClientGuiStatusResponseOutcome::AwaitingResponse
        } else {
            InventoryEquipmentBridgeClientGuiStatusResponseOutcome::None
        }
    }
}

#[derive(Debug)]
pub(super) struct SyntheticAreaState {
    pub(super) pending_server_to_client_packets: Vec<synthetic_area::PendingServerPacket>,
    pub(super) pending_area_loaded: Option<synthetic_area::PendingAreaLoaded>,
    pub(super) in_flight_area_loaded: Option<synthetic_area::InFlightAreaLoaded>,
    pub(super) completed_area_loaded: Option<synthetic_area::CompletedAreaLoaded>,
    pub(super) server_hold_gate: Option<synthetic_area::ServerHoldGate>,
    pub(super) held_server_to_client_packets: Vec<synthetic_area::PendingVerifiedServerPacket>,
    pub(super) synthesize_loadbar: bool,
}

impl Default for SyntheticAreaState {
    fn default() -> Self {
        Self {
            pending_server_to_client_packets: Vec::new(),
            pending_area_loaded: None,
            in_flight_area_loaded: None,
            completed_area_loaded: None,
            server_hold_gate: None,
            held_server_to_client_packets: Vec::new(),
            synthesize_loadbar: true,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct DeferredModuleResourcesSessionState {
    pub(super) pending: deferred_module_resources::DeferredModuleResourcesState,
}

#[derive(Debug, Default)]
pub(super) struct AreaContextState {
    pub(super) latest_area_placeables: area::AreaPlaceableContext,
}

#[derive(Debug, Default)]
pub struct SessionState {
    pub(super) deflate: DeflateState,
    pub(super) quickbar: QuickbarStreamState,
    pub(super) live_object: LiveObjectStreamState,
    pub(super) sequence: SequenceState,
    pub(super) client_ack: ClientAckSessionState,
    pub(super) login_waypoint: LoginWaypointState,
    pub(super) inventory_equipment: InventoryEquipmentBridgeState,
    pub(super) synthetic_area: SyntheticAreaState,
    pub(super) deferred_module_resources: DeferredModuleResourcesSessionState,
    pub(super) area_context: AreaContextState,
    pub(super) module_resources: module_resources::ModuleResourceRuntime,
    pub(super) semantic: semantic::SemanticSessionState,
    pub(super) quickbar_item_refresh_hint_path: Option<PathBuf>,
    pub(super) quickbar_item_refresh_hint_last_body: Option<String>,
}

impl SessionState {
    pub fn new(
        module_resources: module_resources::ModuleResourceRuntime,
        synthesize_area_loadbar: bool,
        quickbar_item_refresh_hint_path: Option<PathBuf>,
    ) -> Self {
        Self {
            deflate: DeflateState::default(),
            quickbar: QuickbarStreamState::default(),
            live_object: LiveObjectStreamState::default(),
            sequence: SequenceState::default(),
            client_ack: ClientAckSessionState::default(),
            login_waypoint: LoginWaypointState::default(),
            inventory_equipment: InventoryEquipmentBridgeState::default(),
            module_resources,
            synthetic_area: SyntheticAreaState {
                synthesize_loadbar: synthesize_area_loadbar,
                pending_server_to_client_packets: Vec::new(),
                pending_area_loaded: None,
                in_flight_area_loaded: None,
                completed_area_loaded: None,
                server_hold_gate: None,
                held_server_to_client_packets: Vec::new(),
            },
            deferred_module_resources: DeferredModuleResourcesSessionState::default(),
            area_context: AreaContextState::default(),
            semantic: semantic::SemanticSessionState::default(),
            quickbar_item_refresh_hint_path,
            quickbar_item_refresh_hint_last_body: None,
        }
    }
}

impl Drop for SessionState {
    fn drop(&mut self) {
        self.semantic.trace_unresolved_quickbar_item_refresh();
    }
}
