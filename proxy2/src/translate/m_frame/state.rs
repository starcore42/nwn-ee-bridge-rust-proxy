//! Typed M-frame session substates.
//!
//! Keep the root M-frame dispatcher from becoming a god-state container. Each
//! substate below owns one transport concern, so future packet-family work has
//! an obvious home instead of casually adding fields to `SessionState`.

use std::{collections::VecDeque, path::PathBuf};

use crate::translate::{
    ContinuationOwner, VerifiedProof, area, client_gui_inventory, module_resources, semantic,
};

use super::{
    client_ack, deferred_module_resources,
    deflate::PersistentServerInflater,
    live_stream, quickbar_stream,
    reassembly::{
        BufferedInterleavedServerPacket, CompletedDeflatedStreamWindow, ServerDeflatedReassembly,
    },
    sequence::{CoalescedSplitSequenceShift, SequenceElision, SequenceShift},
    synthetic_area,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OrderedSuccessorValidationToken {
    pub(super) sequence: u16,
    pub(super) server_origin_generation: u64,
    pub(super) transport_payload_identity: Vec<u8>,
}

/// Reversible engine-facing state touched while an ordered raw reliable
/// successor is being translated ahead of the outer strict emit decision.
///
/// Diamond `sub_5F3940` and EE `CNetLayerWindow::FrameReceive` retain the raw
/// reliable slot before dispatch, but the reconstructed message does not
/// become gameplay truth until its reader succeeds. Keep transport ACK and
/// reliable-generation bookkeeping live; only semantic observations and the
/// proxy-owned packets/sequence intervals derived from them belong here.
#[derive(Debug)]
pub(super) struct OrderedSuccessorEffectSnapshot {
    pub(super) server_reassembly: Option<ServerDeflatedReassembly>,
    pub(super) completed_server_stream_windows: Vec<CompletedDeflatedStreamWindow>,
    pub(super) completed_server_reliable_stream_slots:
        Vec<super::reassembly::CompletedServerReliableStreamSlot>,
    pub(super) server_zlib_stream_proxy_owned: bool,
    pub(super) server_zlib_stream_owner: Option<ContinuationOwner>,
    pub(super) server_zlib_stream_epoch: u64,
    pub(super) server_zlib_inflater: Option<PersistentServerInflater>,
    pub(super) coalesced_replay: CoalescedReplayState,
    pub(super) quickbar: QuickbarStreamState,
    pub(super) live_object: LiveObjectStreamState,
    pub(super) sequence: SequenceState,
    pub(super) direct_server_semantic_replays: DirectServerSemanticReplayState,
    pub(super) login_waypoint: LoginWaypointState,
    pub(super) inventory_equipment: InventoryEquipmentBridgeState,
    pub(super) synthetic_area: SyntheticAreaState,
    pub(super) deferred_module_resources: deferred_module_resources::DeferredModuleResourcesState,
    pub(super) area_context: AreaContextState,
    pub(super) module_resources: module_resources::ModuleResourceRuntime,
    pub(super) semantic: semantic::SemanticSessionState,
    pub(super) quickbar_item_refresh_hint_last_body: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct DeflateState {
    pub(super) server_reassembly: Option<ServerDeflatedReassembly>,
    pub(super) server_zlib_inflater: Option<PersistentServerInflater>,
    pub(super) completed_server_stream_windows: Vec<CompletedDeflatedStreamWindow>,
    pub(super) completed_server_reliable_stream_slots:
        Vec<super::reassembly::CompletedServerReliableStreamSlot>,
    pub(super) server_zlib_stream_proxy_owned: bool,
    pub(super) server_zlib_stream_owner: Option<ContinuationOwner>,
    pub(super) server_zlib_stream_epoch: u64,
    /// A bounded source-order fence armed when a post-reassembly event is
    /// withheld for retransmission. Future reliable data cannot overtake the
    /// first missing sequence merely because the predecessor transaction has
    /// already left `server_reassembly`.
    pub(super) ordered_successor_next_sequence: Option<u16>,
    pub(super) ordered_successor_final_sequence: Option<u16>,
    /// Exact raw reliable events withheld behind a stream-family predecessor.
    /// Keep the complete packet and transport epoch, not only a payload hash:
    /// retransmissions must re-enter the full dispatcher in source order and
    /// must not inherit another generation's semantic disposition.
    pub(super) ordered_successor_events: VecDeque<BufferedInterleavedServerPacket>,
    /// Candidate sequence translated by the core dispatcher but not yet
    /// accepted by the outer strict validator. The active fence and raw event
    /// remain unchanged until validation commits this candidate.
    pub(super) ordered_successor_pending_validation: Option<OrderedSuccessorValidationToken>,
    /// Speculative engine-facing effects for a direct ordered successor. The
    /// outer strict callback discards this snapshot on accept and restores it
    /// on rejection, so a retained raw slot cannot leak gameplay state.
    pub(super) ordered_successor_effect_snapshot: Option<Box<OrderedSuccessorEffectSnapshot>>,
    pub(super) last_server_core_dispatch_accepted: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct CoalescedReplayState {
    pub(super) completed_deflated_records: Vec<CompletedCoalescedDeflatedRecord>,
    pub(super) completed_direct_records: Vec<CompletedCoalescedDirectRecord>,
}

#[derive(Debug, Clone)]
pub(super) struct CompletedCoalescedDeflatedRecord {
    pub(super) sequence: u16,
    /// Wrap-safe reliable-window generation. A reused `u16` sequence must not
    /// inherit a prior record's semantic disposition or persistent-inflater
    /// replay result.
    pub(super) server_origin_generation: u64,
    pub(super) offset: usize,
    pub(super) payload_length: usize,
    pub(super) inflated_length: usize,
    pub(super) compressed: Vec<u8>,
    pub(super) proof: VerifiedProof,
    pub(super) record: Vec<u8>,
    pub(super) dropped: bool,
    pub(super) rewritten_deflated: bool,
    pub(super) abort_window_if_primary_consumed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct CompletedCoalescedDirectRecord {
    pub(super) sequence: u16,
    pub(super) server_origin_generation: u64,
    pub(super) offset: usize,
    pub(super) payload: Vec<u8>,
    pub(super) proof: VerifiedProof,
    pub(super) record: Vec<u8>,
    pub(super) dropped: bool,
    pub(super) abort_window_if_primary_consumed: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct QuickbarStreamState {
    pub(super) pending_stream: Option<quickbar_stream::PendingQuickbarStream>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct LiveObjectStreamState {
    pub(super) pending_stream: Option<live_stream::PendingLiveObjectStream>,
}

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompletedClientReliableSemanticEffect {
    pub(super) sequence: u16,
    pub(super) payload: Vec<u8>,
}

#[derive(Debug, Default)]
pub(super) struct ClientReliableSemanticEffectState {
    /// Reliable `M` retransmissions remain transport-visible, but their typed
    /// semantic and bridge effects must run once per exact sequence/payload.
    /// Exact bytes avoid treating a reused sequence carrying new data as a
    /// replay; the bounded window prevents old sequence-wrap entries from
    /// suppressing later gameplay.
    pub(super) completed: VecDeque<CompletedClientReliableSemanticEffect>,
    pub(super) duplicate_effects_suppressed: u64,
}

#[derive(Debug, Clone)]
pub(super) struct CompletedDirectServerSemanticRewrite {
    pub(super) sequence: u16,
    pub(super) origin_generation: u64,
    pub(super) source_payload: Vec<u8>,
    pub(super) rewritten_payload: Vec<u8>,
    pub(super) proof: VerifiedProof,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DirectServerSemanticReplayState {
    /// Direct server `M` retransmissions are keyed by the reliable source slot
    /// and exact source gameplay bytes. ACK/header changes are transport state,
    /// so a replay refreshes them without running semantic effects again.
    pub(super) completed: VecDeque<CompletedDirectServerSemanticRewrite>,
    pub(super) duplicates_replayed: u64,
    pub(super) latest_origin_sequence: Option<u16>,
    pub(super) origin_generation: u64,
}

#[derive(Debug, Default)]
pub(super) struct ClientAckSessionState {
    pub(super) pending: client_ack::ClientAckState,
}

#[derive(Debug, Clone, Default)]
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
    pub(super) candidate: Option<semantic::InventoryItemContextCandidate>,
    pub(super) ready_objects: usize,
    pub(super) deferred_feature25_only_objects: usize,
    pub(super) object_id: u32,
    pub(super) player_inventory_gui: bool,
    pub(super) trigger_client_sequence: u16,
    pub(super) synthetic_sequence: u16,
    pub(super) ack_sequence: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InventoryEquipmentBridgePendingConfirmedInventoryReplay {
    pub(super) update_index: u64,
    pub(super) emission_index: u64,
    pub(super) event_index: u64,
    pub(super) claim: semantic::InventoryEquipmentServerInventoryClaim,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct InventoryEquipmentBridgeClientGuiStatusResponse {
    pub(super) queued_update_index: u64,
    pub(super) server_sequence: u16,
    /// Peer-facing ACK from the legacy server before proxy-owned client
    /// sequence intervals are hidden from EE.
    pub(super) server_peer_ack_sequence: u16,
    pub(super) ack_sequence: u16,
    pub(super) live_gui_records: u32,
    pub(super) live_gui_fragment_bits: u32,
    pub(super) materialized_item_object_ids: usize,
    pub(super) materialized_item_object_id_first: u32,
    pub(super) materialized_item_object_id_last: u32,
    pub(super) materialized_item_object_id_min: u32,
    pub(super) materialized_item_object_id_max: u32,
    pub(super) materialized_item_object_ids_contain_queued_candidate: bool,
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
        if self_strength != other_strength {
            return self_strength > other_strength;
        }

        let self_evidence = (
            self.materialized_item_object_ids,
            self.materialized_item_object_ids_contain_queued_candidate,
            self.live_gui_records,
            self.live_gui_fragment_bits,
            self.queued_update_index,
            self.compact_item_emission_ready_objects,
        );
        let other_evidence = (
            other.materialized_item_object_ids,
            other.materialized_item_object_ids_contain_queued_candidate,
            other.live_gui_records,
            other.live_gui_fragment_bits,
            other.queued_update_index,
            other.compact_item_emission_ready_objects,
        );
        if self_evidence != other_evidence {
            return self_evidence > other_evidence;
        }

        // Reliable M sequences are wrapping u16 values. Numeric tuple ordering
        // would retain 0xFFFF as "newer" after the stream wrapped to 1. The
        // peer-facing ACK is the authoritative request boundary; the server
        // sequence breaks ties between equal response evidence at that ACK.
        if self.server_peer_ack_sequence != other.server_peer_ack_sequence {
            return super::sequence::sequence_at_or_after(
                self.server_peer_ack_sequence,
                other.server_peer_ack_sequence,
            );
        }
        self.server_sequence != other.server_sequence
            && super::sequence::sequence_at_or_after(self.server_sequence, other.server_sequence)
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
pub(super) enum InventoryEquipmentBridgeClientGuiStatusRequestCompletion {
    #[default]
    None,
    AwaitingServerAcknowledgement,
    AwaitingResponse,
    QueuedStatusUnavailable,
    QueuedUpdateMismatch,
    NonCurrentPlayerRequest,
    ClosedInventoryRequest,
    AwaitingMaterializedItems,
    MaterializedCurrentPlayerInventory,
}

impl InventoryEquipmentBridgeClientGuiStatusRequestCompletion {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AwaitingServerAcknowledgement => "awaiting_server_acknowledgement",
            Self::AwaitingResponse => "awaiting_response",
            Self::QueuedStatusUnavailable => "queued_status_unavailable",
            Self::QueuedUpdateMismatch => "queued_update_mismatch",
            Self::NonCurrentPlayerRequest => "non_current_player_request",
            Self::ClosedInventoryRequest => "closed_inventory_request",
            Self::AwaitingMaterializedItems => "awaiting_materialized_items",
            Self::MaterializedCurrentPlayerInventory => "materialized_current_player_inventory",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InventoryEquipmentBridgeClientGuiStatusResponseAssociation {
    #[default]
    None,
    AwaitingResponse,
    ResponseWithoutCandidate,
    QueuedStatusWithoutCandidate,
    QueuedUpdateMismatch,
    MatchesQueuedStatusCandidate,
    DiffersFromQueuedStatusCandidate,
}

impl InventoryEquipmentBridgeClientGuiStatusResponseAssociation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AwaitingResponse => "awaiting_response",
            Self::ResponseWithoutCandidate => "response_without_candidate",
            Self::QueuedStatusWithoutCandidate => "queued_status_without_candidate",
            Self::QueuedUpdateMismatch => "queued_update_mismatch",
            Self::MatchesQueuedStatusCandidate => "matches_queued_status_candidate",
            Self::DiffersFromQueuedStatusCandidate => "differs_from_queued_status_candidate",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InventoryEquipmentBridgeOutputDecisionKind {
    #[default]
    None,
    QueuedInventoryOutput,
    QueuedClientGuiStatusOutput,
    QueuedConfirmedInventoryReplay,
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
            Self::QueuedConfirmedInventoryReplay => "queued_confirmed_inventory_replay",
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
    ClientGuiStatusRefreshConfirmed,
    ClientGuiStatusInventoryReplayQueued,
    ClientGuiStatusInventoryReplayDispatched,
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
            Self::ClientGuiStatusRefreshConfirmed => "client_gui_status_refresh_confirmed",
            Self::ClientGuiStatusInventoryReplayQueued => {
                "client_gui_status_inventory_replay_queued"
            }
            Self::ClientGuiStatusInventoryReplayDispatched => {
                "client_gui_status_inventory_replay_dispatched"
            }
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

#[derive(Debug, Clone, Default)]
pub(super) struct InventoryEquipmentBridgeState {
    pub(super) last_decision_state_update_index: Option<u64>,
    pub(super) last_queued_state_update_index: Option<u64>,
    pub(super) last_queued_client_gui_status_update_index: Option<u64>,
    pub(super) queued_outputs: u64,
    pub(super) queued_client_gui_status_outputs: u64,
    pub(super) client_gui_status_request_acknowledgements: u64,
    pub(super) client_gui_status_pre_ack_live_object_packets_ignored: u64,
    pub(super) confirmed_inventory_replay_outputs: u64,
    pub(super) confirmed_inventory_replay_dispatches: u64,
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
    pub(super) last_acknowledged_client_gui_status_update_index: Option<u64>,
    pub(super) last_acknowledged_client_gui_status_server_ack_sequence: Option<u16>,
    pub(super) last_observed_client_gui_status_server_peer_ack_sequence: Option<u16>,
    pub(super) last_pre_ack_client_gui_status_live_object_server_sequence: Option<u16>,
    pub(super) last_pre_ack_client_gui_status_live_object_server_ack_sequence: Option<u16>,
    pub(super) pending_confirmed_inventory_replay:
        Option<InventoryEquipmentBridgePendingConfirmedInventoryReplay>,
    pub(super) last_completed_client_gui_status_response_update_index: Option<u64>,
    pub(super) last_confirmed_inventory_replay_update_index: Option<u64>,
    pub(super) last_confirmed_inventory_replay_dispatch_update_index: Option<u64>,
    pub(super) last_client_gui_status_response:
        Option<InventoryEquipmentBridgeClientGuiStatusResponse>,
    pub(super) best_client_gui_status_response:
        Option<InventoryEquipmentBridgeClientGuiStatusResponse>,
}

impl InventoryEquipmentBridgeState {
    pub(super) fn output_status(&self) -> InventoryEquipmentBridgeOutputStatus {
        if self.confirmed_inventory_replay_queued_for_dispatch() {
            InventoryEquipmentBridgeOutputStatus::ClientGuiStatusInventoryReplayQueued
        } else if self.confirmed_inventory_replay_dispatches > 0 {
            InventoryEquipmentBridgeOutputStatus::ClientGuiStatusInventoryReplayDispatched
        } else if self.queued_outputs > 0 {
            InventoryEquipmentBridgeOutputStatus::QueuedInventoryOutput
        } else if self.client_gui_status_refresh_confirmed() {
            InventoryEquipmentBridgeOutputStatus::ClientGuiStatusRefreshConfirmed
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

    pub(super) fn confirmed_inventory_replay_queued_for_dispatch(&self) -> bool {
        self.confirmed_inventory_replay_outputs > self.confirmed_inventory_replay_dispatches
    }

    pub(super) fn client_gui_status_response_window_complete(&self) -> bool {
        self.last_queued_client_gui_status_update_index.is_some()
            && self.last_completed_client_gui_status_response_update_index
                == self.last_queued_client_gui_status_update_index
    }

    pub(super) fn client_gui_status_request_acknowledged(&self) -> bool {
        self.last_queued_client_gui_status_update_index.is_some()
            && self.last_acknowledged_client_gui_status_update_index
                == self.last_queued_client_gui_status_update_index
    }

    pub(super) fn record_confirmed_inventory_replay_dispatch(&mut self) {
        self.confirmed_inventory_replay_dispatches =
            self.confirmed_inventory_replay_dispatches.saturating_add(1);
        self.last_confirmed_inventory_replay_dispatch_update_index =
            self.last_confirmed_inventory_replay_update_index;
    }

    pub(super) fn client_gui_status_refresh_confirmed(&self) -> bool {
        self.client_gui_status_request_completion()
            == InventoryEquipmentBridgeClientGuiStatusRequestCompletion::MaterializedCurrentPlayerInventory
    }

    pub(super) fn client_gui_status_request_completion(
        &self,
    ) -> InventoryEquipmentBridgeClientGuiStatusRequestCompletion {
        if self.queued_client_gui_status_outputs == 0 {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::None;
        }
        let Some(queued_status) = self.last_queued_client_gui_status_output else {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::QueuedStatusUnavailable;
        };
        if queued_status.object_id != client_gui_inventory::DIAMOND_CURRENT_PLAYER_OBJECT_ID {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::NonCurrentPlayerRequest;
        }
        if !queued_status.player_inventory_gui {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::ClosedInventoryRequest;
        }
        if !self.client_gui_status_request_acknowledged() {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::AwaitingServerAcknowledgement;
        }
        let Some(response) = self.best_client_gui_status_response else {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::AwaitingResponse;
        };
        if response.queued_update_index != queued_status.update_index {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::QueuedUpdateMismatch;
        }
        if response.materialized_item_object_ids == 0 {
            return InventoryEquipmentBridgeClientGuiStatusRequestCompletion::AwaitingMaterializedItems;
        }

        // EE `CNWSMessage::HandlePlayerToServerGuiInventoryMessage` minor 1
        // reads only the open BOOL and inventory-owner OBJECTID, then calls
        // `CNWSPlayerInventoryGUI::SetOpen`. No item candidate is present in
        // that request. Therefore the first nonempty typed live-GUI
        // materialization after the legacy server's reliable ACK covers the
        // exact synthetic request completes this single update-index window.
        // Candidate containment remains a stricter, independent prerequisite
        // for replaying an earlier Inventory claim.
        InventoryEquipmentBridgeClientGuiStatusRequestCompletion::MaterializedCurrentPlayerInventory
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

    pub(super) fn best_client_gui_status_response_association(
        &self,
    ) -> InventoryEquipmentBridgeClientGuiStatusResponseAssociation {
        if self.queued_client_gui_status_outputs == 0 {
            return InventoryEquipmentBridgeClientGuiStatusResponseAssociation::None;
        }
        let Some(response) = self.best_client_gui_status_response else {
            return InventoryEquipmentBridgeClientGuiStatusResponseAssociation::AwaitingResponse;
        };
        let Some(queued_status) = self.last_queued_client_gui_status_output else {
            return InventoryEquipmentBridgeClientGuiStatusResponseAssociation::QueuedStatusWithoutCandidate;
        };
        if response.queued_update_index != queued_status.update_index {
            return InventoryEquipmentBridgeClientGuiStatusResponseAssociation::QueuedUpdateMismatch;
        }
        let Some(queued_candidate) = queued_status.candidate else {
            return InventoryEquipmentBridgeClientGuiStatusResponseAssociation::QueuedStatusWithoutCandidate;
        };
        if response.materialized_item_object_ids_contain_queued_candidate {
            return InventoryEquipmentBridgeClientGuiStatusResponseAssociation::MatchesQueuedStatusCandidate;
        }
        let Some(response_candidate) = response.compact_item_emission_ready_candidate else {
            return InventoryEquipmentBridgeClientGuiStatusResponseAssociation::ResponseWithoutCandidate;
        };
        if response_candidate.object_id == queued_candidate.object_id {
            InventoryEquipmentBridgeClientGuiStatusResponseAssociation::MatchesQueuedStatusCandidate
        } else {
            InventoryEquipmentBridgeClientGuiStatusResponseAssociation::DiffersFromQueuedStatusCandidate
        }
    }

    pub(super) fn best_client_gui_status_response_candidate_delta_from_queued_status(&self) -> i64 {
        let (Some(response), Some(queued_status)) = (
            self.best_client_gui_status_response,
            self.last_queued_client_gui_status_output,
        ) else {
            return 0;
        };
        let (Some(response_candidate), Some(queued_candidate)) = (
            response.compact_item_emission_ready_candidate,
            queued_status.candidate,
        ) else {
            return 0;
        };
        i64::from(response_candidate.object_id) - i64::from(queued_candidate.object_id)
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Default)]
pub(super) struct AreaContextState {
    pub(super) latest_area_placeables: area::AreaPlaceableContext,
}

#[derive(Debug, Default)]
pub struct SessionState {
    pub(super) deflate: DeflateState,
    pub(super) coalesced_replay: CoalescedReplayState,
    pub(super) quickbar: QuickbarStreamState,
    pub(super) live_object: LiveObjectStreamState,
    pub(super) sequence: SequenceState,
    pub(super) client_reliable_semantic_effects: ClientReliableSemanticEffectState,
    pub(super) direct_server_semantic_replays: DirectServerSemanticReplayState,
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
            coalesced_replay: CoalescedReplayState::default(),
            quickbar: QuickbarStreamState::default(),
            live_object: LiveObjectStreamState::default(),
            sequence: SequenceState::default(),
            client_reliable_semantic_effects: ClientReliableSemanticEffectState::default(),
            direct_server_semantic_replays: DirectServerSemanticReplayState::default(),
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
