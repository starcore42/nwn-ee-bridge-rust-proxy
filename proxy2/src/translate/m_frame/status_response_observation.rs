//! Candidate-aware correlation for tracked `ClientGuiInventory_Status` responses.
//!
//! The bridge deliberately keeps this evidence separate from response
//! completion and current-player authority. A LiveObject unit proves that the
//! queued candidate was materialized only when that unit's exact typed parser
//! included the candidate object id; the cumulative object-registry candidate
//! is not a substitute. This reducer gives the harness a bounded, wire-ordered
//! view from which a later decompile/live-backed completion rule can be made
//! without changing today's fail-closed consumers.

use super::state::{
    InventoryEquipmentBridgeClientGuiStatusResponseObservation, InventoryEquipmentBridgeState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObservationContextKey {
    queued_update_index: u64,
    current_player_object_id: Option<u32>,
    area_client_area_packets: u64,
    control_epoch: u64,
}

impl From<InventoryEquipmentBridgeClientGuiStatusResponseObservation> for ObservationContextKey {
    fn from(observation: InventoryEquipmentBridgeClientGuiStatusResponseObservation) -> Self {
        Self {
            queued_update_index: observation.queued_update_index,
            current_player_object_id: observation.current_player_object_id,
            area_client_area_packets: observation.area_client_area_packets,
            control_epoch: observation.control_epoch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransportKey {
    server_sequence: u16,
    server_peer_ack_sequence: u16,
    ack_sequence: u16,
}

impl From<InventoryEquipmentBridgeClientGuiStatusResponseObservation> for TransportKey {
    fn from(observation: InventoryEquipmentBridgeClientGuiStatusResponseObservation) -> Self {
        Self {
            server_sequence: observation.server_sequence,
            server_peer_ack_sequence: observation.server_peer_ack_sequence,
            ack_sequence: observation.ack_sequence,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum CandidateChronology {
    #[default]
    None,
    OwnerOnly,
    QueuedCandidateMaterializationOnly,
    SameUnit,
    OwnerBeforeCandidateMaterialization,
    OwnerBeforePostCompletionCandidateMaterialization,
    CandidateMaterializationBeforeOwner,
}

impl CandidateChronology {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::OwnerOnly => "owner_only",
            Self::QueuedCandidateMaterializationOnly => "queued_candidate_materialization_only",
            Self::SameUnit => "same_unit",
            Self::OwnerBeforeCandidateMaterialization => "owner_before_candidate_materialization",
            Self::OwnerBeforePostCompletionCandidateMaterialization => {
                "owner_before_post_completion_candidate_materialization"
            }
            Self::CandidateMaterializationBeforeOwner => "candidate_materialization_before_owner",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum CandidateCorrelationBlockedReason {
    #[default]
    None,
    QueuedCandidateMaterializationUnobserved,
    PreCompletionCurrentPlayerOwnerUnobserved,
    PreCompletionOwnerEvictedContextUnknown,
    QueuedCandidateMaterializationEvictedContextUnknown,
    AuthorityContextMismatch,
    CrossUnitAuthorityUnproven,
}

impl CandidateCorrelationBlockedReason {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::QueuedCandidateMaterializationUnobserved => {
                "queued_candidate_materialization_unobserved"
            }
            Self::PreCompletionCurrentPlayerOwnerUnobserved => {
                "pre_completion_current_player_owner_unobserved"
            }
            Self::PreCompletionOwnerEvictedContextUnknown => {
                "pre_completion_owner_evicted_context_unknown"
            }
            Self::QueuedCandidateMaterializationEvictedContextUnknown => {
                "queued_candidate_materialization_evicted_context_unknown"
            }
            Self::AuthorityContextMismatch => "authority_context_mismatch",
            Self::CrossUnitAuthorityUnproven => "cross_unit_authority_unproven",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum CandidateTransportRelation {
    #[default]
    Unavailable,
    SameUnit,
    SameTransport,
    DifferentTransport,
}

impl CandidateTransportRelation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::SameUnit => "same_unit",
            Self::SameTransport => "same_transport",
            Self::DifferentTransport => "different_transport",
        }
    }

    pub(super) fn same_transport(self) -> bool {
        matches!(self, Self::SameUnit | Self::SameTransport)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct CandidateCorrelationSummary {
    pub(super) queued_candidate_materialization_retained: bool,
    pub(super) pre_completion_owner_retained: bool,
    pub(super) same_authority_context: bool,
    pub(super) owner_unit_ordinal: u64,
    pub(super) candidate_materialization_unit_ordinal: u64,
    pub(super) transport_relation: CandidateTransportRelation,
    pub(super) candidate_materialization_post_completion: bool,
    pub(super) chronology: CandidateChronology,
    pub(super) blocked_reason: CandidateCorrelationBlockedReason,
}

#[derive(Debug, Clone, Copy)]
struct RetainedUnit {
    index: usize,
    ordinal: u64,
    observation: InventoryEquipmentBridgeClientGuiStatusResponseObservation,
}

impl RetainedUnit {
    fn context(self) -> ObservationContextKey {
        self.observation.into()
    }

    fn transport(self) -> TransportKey {
        self.observation.into()
    }
}

fn transport_relation(owner: RetainedUnit, candidate: RetainedUnit) -> CandidateTransportRelation {
    if owner.index == candidate.index {
        CandidateTransportRelation::SameUnit
    } else if owner.transport() == candidate.transport() {
        CandidateTransportRelation::SameTransport
    } else {
        CandidateTransportRelation::DifferentTransport
    }
}

fn pair_rank(owner: RetainedUnit, candidate: RetainedUnit) -> (u8, u8, usize, usize, usize, usize) {
    let relation_rank = match transport_relation(owner, candidate) {
        CandidateTransportRelation::SameUnit => 0,
        CandidateTransportRelation::SameTransport => 1,
        CandidateTransportRelation::DifferentTransport => 2,
        CandidateTransportRelation::Unavailable => 3,
    };
    let post_completion_rank = u8::from(
        candidate
            .observation
            .response_window_complete_before_observation,
    );
    let completion_index = owner.index.max(candidate.index);
    let distance = owner.index.abs_diff(candidate.index);
    (
        post_completion_rank,
        relation_rank,
        completion_index,
        distance,
        owner.index,
        candidate.index,
    )
}

fn chronology(owner: Option<RetainedUnit>, candidate: Option<RetainedUnit>) -> CandidateChronology {
    match (owner, candidate) {
        (None, None) => CandidateChronology::None,
        (Some(_), None) => CandidateChronology::OwnerOnly,
        (None, Some(_)) => CandidateChronology::QueuedCandidateMaterializationOnly,
        (Some(owner), Some(candidate)) if owner.index == candidate.index => {
            CandidateChronology::SameUnit
        }
        (Some(owner), Some(candidate)) if owner.index < candidate.index => {
            if candidate
                .observation
                .response_window_complete_before_observation
            {
                CandidateChronology::OwnerBeforePostCompletionCandidateMaterialization
            } else {
                CandidateChronology::OwnerBeforeCandidateMaterialization
            }
        }
        (Some(_), Some(_)) => CandidateChronology::CandidateMaterializationBeforeOwner,
    }
}

/// Reduce the retained request-scoped FIFO into candidate-specific evidence.
///
/// This method is observation-only. In particular, it does not feed
/// `client_gui_status_request_completion`, current-player binding, Inventory
/// replay, or the EquipToggle planner.
pub(super) fn summarize(bridge: &InventoryEquipmentBridgeState) -> CandidateCorrelationSummary {
    let retained = &bridge.client_gui_status_response_observations;
    let retained_unit =
        |index: usize, observation: &InventoryEquipmentBridgeClientGuiStatusResponseObservation| {
            RetainedUnit {
                index,
                ordinal: bridge
                    .client_gui_status_response_observations_evicted
                    .saturating_add(u64::try_from(index).unwrap_or(u64::MAX))
                    .saturating_add(1),
                observation: *observation,
            }
        };
    let owners: Vec<_> = retained
        .iter()
        .enumerate()
        .filter(|(_, observation)| {
            observation.current_player_inventory_records != 0
                && !observation.response_window_complete_before_observation
        })
        .map(|(index, observation)| retained_unit(index, observation))
        .collect();
    let candidates: Vec<_> = retained
        .iter()
        .enumerate()
        .filter(|(_, observation)| {
            observation.materialized_item_object_ids_contain_queued_candidate
        })
        .map(|(index, observation)| retained_unit(index, observation))
        .collect();

    let mut selected_pair: Option<(RetainedUnit, RetainedUnit)> = None;
    for owner in &owners {
        for candidate in &candidates {
            if owner.context() != candidate.context() {
                continue;
            }
            let pair = (*owner, *candidate);
            if selected_pair.is_none_or(|selected| {
                pair_rank(pair.0, pair.1) < pair_rank(selected.0, selected.1)
            }) {
                selected_pair = Some(pair);
            }
        }
    }

    let (selected_owner, selected_candidate, same_authority_context) = match selected_pair {
        Some((owner, candidate)) => (Some(owner), Some(candidate), true),
        None => (owners.first().copied(), candidates.first().copied(), false),
    };
    let relation = match (selected_owner, selected_candidate) {
        (Some(owner), Some(candidate)) => transport_relation(owner, candidate),
        _ => CandidateTransportRelation::Unavailable,
    };
    let candidate_materialization_post_completion = selected_candidate.is_some_and(|candidate| {
        candidate
            .observation
            .response_window_complete_before_observation
    });
    let blocked_reason = if retained.is_empty()
        && bridge.client_gui_status_response_observations_evicted == 0
    {
        CandidateCorrelationBlockedReason::None
    } else if selected_owner.is_none() && selected_candidate.is_none() {
        if bridge.client_gui_status_response_queued_candidate_observations_evicted != 0 {
            CandidateCorrelationBlockedReason::QueuedCandidateMaterializationEvictedContextUnknown
        } else {
            CandidateCorrelationBlockedReason::QueuedCandidateMaterializationUnobserved
        }
    } else if selected_owner.is_none() {
        if bridge.client_gui_status_response_pre_completion_owner_observations_evicted != 0 {
            CandidateCorrelationBlockedReason::PreCompletionOwnerEvictedContextUnknown
        } else {
            CandidateCorrelationBlockedReason::PreCompletionCurrentPlayerOwnerUnobserved
        }
    } else if selected_candidate.is_none() {
        if bridge.client_gui_status_response_queued_candidate_observations_evicted != 0 {
            CandidateCorrelationBlockedReason::QueuedCandidateMaterializationEvictedContextUnknown
        } else {
            CandidateCorrelationBlockedReason::QueuedCandidateMaterializationUnobserved
        }
    } else if !same_authority_context {
        CandidateCorrelationBlockedReason::AuthorityContextMismatch
    } else if relation != CandidateTransportRelation::SameUnit {
        CandidateCorrelationBlockedReason::CrossUnitAuthorityUnproven
    } else {
        CandidateCorrelationBlockedReason::None
    };

    CandidateCorrelationSummary {
        queued_candidate_materialization_retained: selected_candidate.is_some(),
        pre_completion_owner_retained: selected_owner.is_some(),
        same_authority_context,
        owner_unit_ordinal: selected_owner.map(|owner| owner.ordinal).unwrap_or(0),
        candidate_materialization_unit_ordinal: selected_candidate
            .map(|candidate| candidate.ordinal)
            .unwrap_or(0),
        transport_relation: relation,
        candidate_materialization_post_completion,
        chronology: chronology(selected_owner, selected_candidate),
        blocked_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::m_frame::state::{
        CLIENT_GUI_STATUS_RESPONSE_OBSERVATION_CAPACITY,
        InventoryEquipmentBridgeClientGuiStatusResponseObservation, InventoryEquipmentBridgeState,
    };

    const OWNER: u32 = 0xFFFF_FFEF;

    fn observation(
        server_sequence: u16,
    ) -> InventoryEquipmentBridgeClientGuiStatusResponseObservation {
        InventoryEquipmentBridgeClientGuiStatusResponseObservation {
            queued_update_index: 7,
            server_sequence,
            server_peer_ack_sequence: 80,
            ack_sequence: 60,
            current_player_object_id: Some(OWNER),
            area_client_area_packets: 3,
            control_epoch: 5,
            ..Default::default()
        }
    }

    #[test]
    fn fresh_live_queued_candidate_materialization_only_keeps_binding_blocked() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                // The fresh HG Status sample materialized fifty upper-case
                // self LiveGUI objects, including the queued candidate, but
                // carried no top-level current-player Inventory owner row.
                materialized_item_object_ids: 50,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(40)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(
            summary.chronology,
            CandidateChronology::QueuedCandidateMaterializationOnly
        );
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::PreCompletionCurrentPlayerOwnerUnobserved
        );
        assert_eq!(summary.owner_unit_ordinal, 0);
        assert_eq!(summary.candidate_materialization_unit_ordinal, 1);
        assert_eq!(
            summary.transport_relation,
            CandidateTransportRelation::Unavailable
        );
        assert!(!summary.same_authority_context);
    }

    #[test]
    fn nonqueued_materialization_is_not_candidate_evidence() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                materialized_item_object_ids: 50,
                ..observation(40)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(summary.chronology, CandidateChronology::None);
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::QueuedCandidateMaterializationUnobserved
        );
        assert!(!summary.queued_candidate_materialization_retained);
        assert_eq!(summary.candidate_materialization_unit_ordinal, 0);
    }

    #[test]
    fn empty_ledger_has_no_candidate_correlation_diagnosis() {
        let summary = summarize(&InventoryEquipmentBridgeState::default());

        assert_eq!(summary.chronology, CandidateChronology::None);
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::None
        );
        assert_eq!(summary.owner_unit_ordinal, 0);
        assert_eq!(summary.candidate_materialization_unit_ordinal, 0);
    }

    #[test]
    fn same_unit_owner_and_candidate_is_the_strongest_exact_correlation() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                current_player_inventory_records: 1,
                materialized_item_object_ids: 1,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(41)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(summary.chronology, CandidateChronology::SameUnit);
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::None
        );
        assert_eq!(summary.owner_unit_ordinal, 1);
        assert_eq!(summary.candidate_materialization_unit_ordinal, 1);
        assert_eq!(
            summary.transport_relation,
            CandidateTransportRelation::SameUnit
        );
        assert!(summary.transport_relation.same_transport());
        assert!(summary.same_authority_context);
    }

    #[test]
    fn cross_unit_candidate_correlation_retains_wire_order_and_transport() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                current_player_inventory_records: 1,
                ..observation(42)
            },
        );
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                materialized_item_object_ids: 1,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(42)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(
            summary.chronology,
            CandidateChronology::OwnerBeforeCandidateMaterialization
        );
        assert_eq!(summary.owner_unit_ordinal, 1);
        assert_eq!(summary.candidate_materialization_unit_ordinal, 2);
        assert_eq!(
            summary.transport_relation,
            CandidateTransportRelation::SameTransport
        );
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::CrossUnitAuthorityUnproven
        );
        assert!(summary.same_authority_context);
    }

    #[test]
    fn every_observation_context_key_component_mismatch_blocks_correlation() {
        let mut mismatches = [observation(43); 4];
        mismatches[0].queued_update_index += 1;
        mismatches[1].current_player_object_id = Some(0xFFFF_FFEE);
        mismatches[2].area_client_area_packets += 1;
        mismatches[3].control_epoch += 1;

        for mut candidate in mismatches {
            candidate.materialized_item_object_ids = 1;
            candidate.materialized_item_object_ids_contain_queued_candidate = true;
            let mut bridge = InventoryEquipmentBridgeState::default();
            bridge.record_client_gui_status_response_observation(
                InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                    current_player_inventory_records: 1,
                    ..observation(43)
                },
            );
            bridge.record_client_gui_status_response_observation(candidate);

            let summary = summarize(&bridge);

            assert_eq!(summary.owner_unit_ordinal, 1);
            assert_eq!(summary.candidate_materialization_unit_ordinal, 2);
            assert_eq!(
                summary.blocked_reason,
                CandidateCorrelationBlockedReason::AuthorityContextMismatch
            );
            assert!(!summary.same_authority_context);
        }
    }

    #[test]
    fn post_completion_candidate_is_retained_as_diagnostic_only() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                current_player_inventory_records: 1,
                ..observation(44)
            },
        );
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                response_window_complete_before_observation: true,
                materialized_item_object_ids: 1,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(44)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(
            summary.chronology,
            CandidateChronology::OwnerBeforePostCompletionCandidateMaterialization
        );
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::CrossUnitAuthorityUnproven
        );
        assert!(summary.candidate_materialization_post_completion);
        assert!(summary.same_authority_context);
    }

    #[test]
    fn candidate_only_post_completion_state_describes_the_selected_unit() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                response_window_complete_before_observation: true,
                materialized_item_object_ids: 1,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(44)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::PreCompletionCurrentPlayerOwnerUnobserved
        );
        assert!(summary.candidate_materialization_post_completion);
        assert_eq!(summary.owner_unit_ordinal, 0);
        assert_eq!(summary.candidate_materialization_unit_ordinal, 1);
    }

    #[test]
    fn different_transport_cross_unit_authority_remains_unproven() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                current_player_inventory_records: 1,
                ..observation(45)
            },
        );
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                materialized_item_object_ids: 1,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(46)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(
            summary.transport_relation,
            CandidateTransportRelation::DifferentTransport
        );
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::CrossUnitAuthorityUnproven
        );
        assert!(!summary.transport_relation.same_transport());
        assert!(summary.same_authority_context);
    }

    #[test]
    fn fifo_eviction_preserves_ordinal_without_claiming_candidate_context() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                current_player_inventory_records: 1,
                ..observation(47)
            },
        );
        for _ in 0..CLIENT_GUI_STATUS_RESPONSE_OBSERVATION_CAPACITY {
            bridge.record_client_gui_status_response_observation(observation(47));
        }
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                materialized_item_object_ids: 1,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(47)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(
            bridge.client_gui_status_response_pre_completion_owner_observations_evicted,
            1
        );
        assert_eq!(summary.owner_unit_ordinal, 0);
        assert_eq!(
            summary.candidate_materialization_unit_ordinal,
            u64::try_from(CLIENT_GUI_STATUS_RESPONSE_OBSERVATION_CAPACITY).unwrap() + 2
        );
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::PreCompletionOwnerEvictedContextUnknown
        );
    }

    #[test]
    fn evicted_candidate_does_not_inherit_retained_owner_context() {
        let mut bridge = InventoryEquipmentBridgeState::default();
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                materialized_item_object_ids: 1,
                materialized_item_object_ids_contain_queued_candidate: true,
                ..observation(48)
            },
        );
        for _ in 0..CLIENT_GUI_STATUS_RESPONSE_OBSERVATION_CAPACITY {
            bridge.record_client_gui_status_response_observation(observation(48));
        }
        bridge.record_client_gui_status_response_observation(
            InventoryEquipmentBridgeClientGuiStatusResponseObservation {
                current_player_inventory_records: 1,
                ..observation(48)
            },
        );

        let summary = summarize(&bridge);

        assert_eq!(
            bridge.client_gui_status_response_queued_candidate_observations_evicted,
            1
        );
        assert_eq!(summary.candidate_materialization_unit_ordinal, 0);
        assert_eq!(
            summary.owner_unit_ordinal,
            u64::try_from(CLIENT_GUI_STATUS_RESPONSE_OBSERVATION_CAPACITY).unwrap() + 2
        );
        assert_eq!(
            summary.blocked_reason,
            CandidateCorrelationBlockedReason::QueuedCandidateMaterializationEvictedContextUnknown
        );
    }
}
