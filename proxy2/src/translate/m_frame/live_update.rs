//! M-frame-local adapters for live-object payload rewrites.
//!
//! This module intentionally does not own direct `M` frames. Direct
//! `GameObjUpdate_LiveObject` packets must route through
//! `m_frame::server_dispatch`'s semantic registry so mixed add/update payloads
//! are claimed only after the focused add-record, update-record, fragment-bit,
//! and exact validator passes all agree.

use crate::translate::{area, live_object, live_object_update};

pub type RewriteSummary = live_object_update::LiveObjectUpdateRewriteSummary;
pub type RewriteAttempt = live_object_update::LiveObjectUpdateRewriteAttempt;
pub type RewriteFailure = live_object_update::LiveObjectUpdateRewriteFailure;
pub type ClaimSummary = live_object_update::LiveObjectUpdateClaimSummary;
pub type AddNameBitRewriteSummary = live_object_update::LiveObjectAddNameBitRewriteSummary;
pub type ExternalObjectIdCanonicalizeSummary =
    live_object_update::LiveObjectExternalObjectIdCanonicalizeSummary;
pub type LifecycleRewriteSummary = live_object_update::LiveObjectLifecycleRewriteSummary;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct ExactLiveObjectRewriteSummary {
    pub update_passes_changed: u8,
    pub add_passes_changed: u8,
    pub add_name_bit_passes_changed: u8,
    pub exact_placeable_add_unique_targets: u32,
    pub exact_placeable_update_unique_targets: u32,
    pub exact_placeable_add_identity_blocked: u32,
    pub exact_placeable_update_identity_blocked: u32,
    pub exact_placeable_add_identity_resolved_by_fixed_fields: u32,
    pub exact_placeable_add_identity_resolved_by_fixed_field_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_divergent: u32,
    pub exact_placeable_add_identity_resolved_by_following_position: u32,
    pub exact_placeable_add_identity_resolved_by_following_position_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_following_position_fixed_output_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_following_position_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_identity_resolved_by_following_position_fixed_output_divergent: u32,
    pub exact_placeable_add_identity_resolved_by_preceding_position: u32,
    pub exact_placeable_add_identity_resolved_by_preceding_position_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_divergent: u32,
    pub exact_placeable_add_identity_resolved_by_surrounding_position: u32,
    pub exact_placeable_add_identity_resolved_by_surrounding_position_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_equivalence: u32,
    pub exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_divergent: u32,
    pub exact_placeable_add_identity_surrounding_position_conflicts: u32,
    pub exact_placeable_add_identity_surrounding_position_conflict_output_unavailable: u32,
    pub exact_placeable_add_identity_surrounding_position_conflict_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_identity_surrounding_position_conflict_output_divergent: u32,
    pub exact_placeable_add_identity_resolved_by_add_output_equivalence: u32,
    pub exact_placeable_update_identity_resolved_by_position: u32,
    pub exact_placeable_update_identity_resolved_by_position_output_equivalence: u32,
    pub exact_placeable_add_identity_blocked_following_position_missing: u32,
    pub exact_placeable_add_identity_blocked_following_position_lifecycle_blocked: u32,
    pub exact_placeable_add_identity_blocked_following_position_no_static_match: u32,
    pub exact_placeable_add_identity_blocked_following_position_ambiguous_matches: u32,
    pub exact_placeable_add_identity_blocked_following_position_ambiguous_match_rows: u32,
    pub exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_rows: u32,
    pub exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_missing_resref_rows:
        u32,
    pub exact_placeable_add_identity_blocked_following_position_ambiguous_output_unavailable_rows:
        u32,
    pub exact_placeable_add_identity_blocked_following_position_ambiguous_output_divergent_matches:
        u32,
    pub exact_placeable_add_identity_blocked_preceding_position_missing: u32,
    pub exact_placeable_add_identity_blocked_preceding_position_lifecycle_blocked: u32,
    pub exact_placeable_add_identity_blocked_preceding_position_no_static_match: u32,
    pub exact_placeable_add_identity_blocked_preceding_position_ambiguous_matches: u32,
    pub exact_placeable_add_identity_blocked_preceding_position_ambiguous_match_rows: u32,
    pub exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_rows: u32,
    pub exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_missing_resref_rows:
        u32,
    pub exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_unavailable_rows:
        u32,
    pub exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_divergent_matches:
        u32,
    pub exact_placeable_add_identity_blocked_module_custom_rows: u32,
    pub exact_placeable_add_identity_blocked_module_custom_missing_resref_rows: u32,
    pub exact_placeable_add_identity_blocked_fixed_field_matches: u32,
    pub exact_placeable_add_identity_blocked_fixed_field_module_custom_matches: u32,
    pub exact_placeable_add_identity_blocked_fixed_field_module_custom_missing_resref_matches: u32,
    pub exact_placeable_add_identity_blocked_fixed_field_ambiguous_matches: u32,
    pub exact_placeable_add_identity_blocked_fixed_field_ambiguous_match_rows: u32,
    pub exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_rows: u32,
    pub exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_missing_resref_rows:
        u32,
    pub exact_placeable_update_identity_blocked_module_custom_rows: u32,
    pub exact_placeable_update_identity_blocked_module_custom_missing_resref_rows: u32,
    pub exact_placeable_add_no_overlap: u32,
    pub exact_placeable_update_no_overlap: u32,
    pub exact_placeable_add_unique_unchanged: u32,
    pub exact_placeable_update_unique_unchanged: u32,
    pub exact_placeable_appearance_custom_skipped: u32,
    pub exact_placeable_add_module_custom_appearance_skipped: u32,
    pub exact_placeable_update_module_custom_appearance_skipped: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_skipped: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_update: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_update_position_output_equivalence:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_ready:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_blocked:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_ready:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_blocked:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only_position_output_equivalence:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_ready:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_blocked:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_ready:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_blocked:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_add_only: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_planned:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_insert_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_start_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_end_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_placeable_add_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_normal_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_custom_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_after_add_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_boundary_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_source_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_duplicate_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_placeable_add_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_normal_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_custom_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_after_add_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_emit_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_payload_build_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_header_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_declared_length_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_fragment_bits_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_boundary_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_record_validator_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_cursor_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_placeable_add_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_normal_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_custom_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_add_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_before_synthesized_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_inside_synthesized_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_synthesized_update_rejected:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_placeable_add:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_normal_update:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_custom_update:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_after_add:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update: u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_without_carrier:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_ready:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_blocked:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_match:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_match:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_mismatch:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable:
        u32,
    pub exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable_reasons:
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_skipped: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_divergent:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_divergent:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_only_fixed_output:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_divergent:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_divergent:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_only_fixed_output:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_missing_template_resref_rows:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_output_divergent: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_update: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_normal_update: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_custom_update: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_update_only: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_position_only_fixed_output:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_normal_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_custom_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_add_only: u32,
    pub exact_placeable_add_module_custom_template_resref_missing: u32,
    pub exact_placeable_update_module_custom_template_resref_missing: u32,
    pub exact_placeable_add_source_custom_appearance_rewritten: u32,
    pub exact_placeable_update_source_custom_appearance_rewritten: u32,
    pub exact_placeable_update_module_custom_appearance_rewritten: u32,
    pub exact_placeable_add_appearance_rewritten: u32,
    pub exact_placeable_add_state_rewritten: u32,
    pub exact_placeable_update_position_rewritten: u32,
    pub exact_placeable_update_appearance_rewritten: u32,
    pub exact_placeable_update_appearance_exact_rejected: u32,
    pub exact_placeable_update_orientation_rewritten: u32,
    pub exact_placeable_update_state_rewritten: u32,
}

impl ExactLiveObjectRewriteSummary {
    pub(crate) fn exact_placeable_custom_carrier_selected_target_unavailable_reasons_by_scope(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
        live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
            following_normal: self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable_reasons,
            following_custom: self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_reasons,
            pre_add_normal: self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable_reasons,
            pre_add_custom: self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable_reasons,
        }
    }

    pub(crate) fn exact_placeable_custom_carrier_committed_target_unavailable_reasons_by_scope(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
        live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
            following_normal: self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable_reasons,
            following_custom: self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable_reasons,
            pre_add_normal: self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable_reasons,
            pre_add_custom: self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable_reasons,
        }
    }

    pub(crate) fn exact_placeable_custom_carrier_satisfied_target_unavailable_reasons_by_scope(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
        live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
            following_custom: self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier_reasons,
            ..live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary::default()
        }
    }

    pub(crate) fn exact_placeable_custom_carrier_uncommitted_target_unavailable_reasons_by_scope(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
        self.exact_placeable_custom_carrier_selected_target_unavailable_reasons_by_scope()
            .saturating_sub(
                self.exact_placeable_custom_carrier_committed_target_unavailable_reasons_by_scope(),
            )
    }

    pub(crate) fn exact_placeable_custom_carrier_unresolved_target_unavailable_reasons_by_scope(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierScopedTargetUnavailableReasonSummary {
        self.exact_placeable_custom_carrier_uncommitted_target_unavailable_reasons_by_scope()
            .saturating_sub(
                self.exact_placeable_custom_carrier_satisfied_target_unavailable_reasons_by_scope(),
            )
    }

    pub(crate) fn exact_placeable_custom_carrier_selected_target_unavailable_reasons(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary {
        self.exact_placeable_custom_carrier_selected_target_unavailable_reasons_by_scope()
            .total_reasons()
    }

    pub(crate) fn exact_placeable_custom_carrier_committed_target_unavailable_reasons(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary {
        self.exact_placeable_custom_carrier_committed_target_unavailable_reasons_by_scope()
            .total_reasons()
    }

    pub(crate) fn exact_placeable_custom_carrier_satisfied_target_unavailable_reasons(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary {
        self.exact_placeable_custom_carrier_satisfied_target_unavailable_reasons_by_scope()
            .total_reasons()
    }

    pub(crate) fn exact_placeable_custom_carrier_uncommitted_target_unavailable_reasons(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary {
        self.exact_placeable_custom_carrier_uncommitted_target_unavailable_reasons_by_scope()
            .total_reasons()
    }

    pub(crate) fn exact_placeable_custom_carrier_unresolved_target_unavailable_reasons(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierTargetUnavailableReasonSummary {
        self.exact_placeable_custom_carrier_unresolved_target_unavailable_reasons_by_scope()
            .total_reasons()
    }

    fn record_update(&mut self, changed: bool) {
        if changed {
            self.update_passes_changed = self.update_passes_changed.saturating_add(1);
        }
    }

    fn record_update_summary(&mut self, rewrite: Option<RewriteSummary>) -> bool {
        let Some(rewrite) = rewrite else {
            return false;
        };
        self.record_update(true);
        self.exact_placeable_add_unique_targets = self
            .exact_placeable_add_unique_targets
            .saturating_add(rewrite.exact_placeable_add_unique_targets);
        self.exact_placeable_update_unique_targets = self
            .exact_placeable_update_unique_targets
            .saturating_add(rewrite.exact_placeable_update_unique_targets);
        self.exact_placeable_add_identity_blocked = self
            .exact_placeable_add_identity_blocked
            .saturating_add(rewrite.exact_placeable_add_identity_blocked);
        self.exact_placeable_update_identity_blocked = self
            .exact_placeable_update_identity_blocked
            .saturating_add(rewrite.exact_placeable_update_identity_blocked);
        self.exact_placeable_add_identity_resolved_by_fixed_fields = self
            .exact_placeable_add_identity_resolved_by_fixed_fields
            .saturating_add(rewrite.exact_placeable_add_identity_resolved_by_fixed_fields);
        self.exact_placeable_add_identity_resolved_by_fixed_field_equivalence = self
            .exact_placeable_add_identity_resolved_by_fixed_field_equivalence
            .saturating_add(
                rewrite.exact_placeable_add_identity_resolved_by_fixed_field_equivalence,
            );
        self.exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_equivalence = self
            .exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_equivalence
            .saturating_add(
                rewrite
                    .exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_equivalence,
            );
        self
            .exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_missing_template_resref_rows =
            self
                .exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_missing_template_resref_rows,
                );
        self.exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_divergent = self
            .exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_divergent
            .saturating_add(
                rewrite.exact_placeable_add_identity_resolved_by_fixed_field_fixed_output_divergent,
            );
        self.exact_placeable_add_identity_resolved_by_following_position = self
            .exact_placeable_add_identity_resolved_by_following_position
            .saturating_add(rewrite.exact_placeable_add_identity_resolved_by_following_position);
        self.exact_placeable_add_identity_resolved_by_following_position_equivalence = self
            .exact_placeable_add_identity_resolved_by_following_position_equivalence
            .saturating_add(
                rewrite.exact_placeable_add_identity_resolved_by_following_position_equivalence,
            );
        self
            .exact_placeable_add_identity_resolved_by_following_position_fixed_output_equivalence =
            self
                .exact_placeable_add_identity_resolved_by_following_position_fixed_output_equivalence
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_following_position_fixed_output_equivalence,
                );
        self
            .exact_placeable_add_identity_resolved_by_following_position_fixed_output_missing_template_resref_rows =
            self
                .exact_placeable_add_identity_resolved_by_following_position_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_following_position_fixed_output_missing_template_resref_rows,
                );
        self
            .exact_placeable_add_identity_resolved_by_following_position_fixed_output_divergent =
            self
                .exact_placeable_add_identity_resolved_by_following_position_fixed_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_following_position_fixed_output_divergent,
                );
        self.exact_placeable_add_identity_resolved_by_preceding_position = self
            .exact_placeable_add_identity_resolved_by_preceding_position
            .saturating_add(rewrite.exact_placeable_add_identity_resolved_by_preceding_position);
        self.exact_placeable_add_identity_resolved_by_preceding_position_equivalence = self
            .exact_placeable_add_identity_resolved_by_preceding_position_equivalence
            .saturating_add(
                rewrite.exact_placeable_add_identity_resolved_by_preceding_position_equivalence,
            );
        self
            .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_equivalence =
            self
                .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_equivalence
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_equivalence,
                );
        self
            .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_missing_template_resref_rows =
            self
                .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_missing_template_resref_rows,
                );
        self
            .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_divergent =
            self
                .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_preceding_position_fixed_output_divergent,
                );
        self.exact_placeable_add_identity_resolved_by_surrounding_position = self
            .exact_placeable_add_identity_resolved_by_surrounding_position
            .saturating_add(rewrite.exact_placeable_add_identity_resolved_by_surrounding_position);
        self.exact_placeable_add_identity_resolved_by_surrounding_position_equivalence = self
            .exact_placeable_add_identity_resolved_by_surrounding_position_equivalence
            .saturating_add(
                rewrite.exact_placeable_add_identity_resolved_by_surrounding_position_equivalence,
            );
        self
            .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_equivalence =
            self
                .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_equivalence
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_equivalence,
                );
        self
            .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_missing_template_resref_rows =
            self
                .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_missing_template_resref_rows,
                );
        self
            .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_divergent =
            self
                .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_resolved_by_surrounding_position_fixed_output_divergent,
                );
        self.exact_placeable_add_identity_surrounding_position_conflicts = self
            .exact_placeable_add_identity_surrounding_position_conflicts
            .saturating_add(rewrite.exact_placeable_add_identity_surrounding_position_conflicts);
        self.exact_placeable_add_identity_surrounding_position_conflict_output_unavailable = self
            .exact_placeable_add_identity_surrounding_position_conflict_output_unavailable
            .saturating_add(
                rewrite
                    .exact_placeable_add_identity_surrounding_position_conflict_output_unavailable,
            );
        self
            .exact_placeable_add_identity_surrounding_position_conflict_output_missing_template_resref_rows =
            self
                .exact_placeable_add_identity_surrounding_position_conflict_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_surrounding_position_conflict_output_missing_template_resref_rows,
                );
        self.exact_placeable_add_identity_surrounding_position_conflict_output_divergent = self
            .exact_placeable_add_identity_surrounding_position_conflict_output_divergent
            .saturating_add(
                rewrite.exact_placeable_add_identity_surrounding_position_conflict_output_divergent,
            );
        self.exact_placeable_add_identity_resolved_by_add_output_equivalence = self
            .exact_placeable_add_identity_resolved_by_add_output_equivalence
            .saturating_add(
                rewrite.exact_placeable_add_identity_resolved_by_add_output_equivalence,
            );
        self.exact_placeable_update_identity_resolved_by_position = self
            .exact_placeable_update_identity_resolved_by_position
            .saturating_add(rewrite.exact_placeable_update_identity_resolved_by_position);
        self.exact_placeable_update_identity_resolved_by_position_output_equivalence = self
            .exact_placeable_update_identity_resolved_by_position_output_equivalence
            .saturating_add(
                rewrite.exact_placeable_update_identity_resolved_by_position_output_equivalence,
            );
        self.exact_placeable_add_identity_blocked_following_position_missing = self
            .exact_placeable_add_identity_blocked_following_position_missing
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_following_position_missing,
            );
        self.exact_placeable_add_identity_blocked_following_position_lifecycle_blocked = self
            .exact_placeable_add_identity_blocked_following_position_lifecycle_blocked
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_following_position_lifecycle_blocked,
            );
        self.exact_placeable_add_identity_blocked_following_position_no_static_match = self
            .exact_placeable_add_identity_blocked_following_position_no_static_match
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_following_position_no_static_match,
            );
        self.exact_placeable_add_identity_blocked_following_position_ambiguous_matches = self
            .exact_placeable_add_identity_blocked_following_position_ambiguous_matches
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_following_position_ambiguous_matches,
            );
        self.exact_placeable_add_identity_blocked_following_position_ambiguous_match_rows = self
            .exact_placeable_add_identity_blocked_following_position_ambiguous_match_rows
            .saturating_add(
                rewrite
                    .exact_placeable_add_identity_blocked_following_position_ambiguous_match_rows,
            );
        self.exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_rows =
            self.exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_rows,
                );
        self.exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_missing_resref_rows =
            self.exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_missing_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_following_position_ambiguous_module_custom_missing_resref_rows,
                );
        self.exact_placeable_add_identity_blocked_following_position_ambiguous_output_unavailable_rows =
            self.exact_placeable_add_identity_blocked_following_position_ambiguous_output_unavailable_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_following_position_ambiguous_output_unavailable_rows,
                );
        self.exact_placeable_add_identity_blocked_following_position_ambiguous_output_divergent_matches =
            self.exact_placeable_add_identity_blocked_following_position_ambiguous_output_divergent_matches
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_following_position_ambiguous_output_divergent_matches,
                );
        self.exact_placeable_add_identity_blocked_preceding_position_missing = self
            .exact_placeable_add_identity_blocked_preceding_position_missing
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_preceding_position_missing,
            );
        self.exact_placeable_add_identity_blocked_preceding_position_lifecycle_blocked = self
            .exact_placeable_add_identity_blocked_preceding_position_lifecycle_blocked
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_preceding_position_lifecycle_blocked,
            );
        self.exact_placeable_add_identity_blocked_preceding_position_no_static_match = self
            .exact_placeable_add_identity_blocked_preceding_position_no_static_match
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_preceding_position_no_static_match,
            );
        self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_matches = self
            .exact_placeable_add_identity_blocked_preceding_position_ambiguous_matches
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_preceding_position_ambiguous_matches,
            );
        self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_match_rows = self
            .exact_placeable_add_identity_blocked_preceding_position_ambiguous_match_rows
            .saturating_add(
                rewrite
                    .exact_placeable_add_identity_blocked_preceding_position_ambiguous_match_rows,
            );
        self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_rows =
            self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_rows,
                );
        self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_missing_resref_rows =
            self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_missing_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_preceding_position_ambiguous_module_custom_missing_resref_rows,
                );
        self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_unavailable_rows =
            self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_unavailable_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_unavailable_rows,
                );
        self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_divergent_matches =
            self.exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_divergent_matches
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_preceding_position_ambiguous_output_divergent_matches,
                );
        self.exact_placeable_add_identity_blocked_module_custom_rows = self
            .exact_placeable_add_identity_blocked_module_custom_rows
            .saturating_add(rewrite.exact_placeable_add_identity_blocked_module_custom_rows);
        self.exact_placeable_add_identity_blocked_module_custom_missing_resref_rows = self
            .exact_placeable_add_identity_blocked_module_custom_missing_resref_rows
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_module_custom_missing_resref_rows,
            );
        self.exact_placeable_add_identity_blocked_fixed_field_matches = self
            .exact_placeable_add_identity_blocked_fixed_field_matches
            .saturating_add(rewrite.exact_placeable_add_identity_blocked_fixed_field_matches);
        self.exact_placeable_add_identity_blocked_fixed_field_module_custom_matches = self
            .exact_placeable_add_identity_blocked_fixed_field_module_custom_matches
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_fixed_field_module_custom_matches,
            );
        self.exact_placeable_add_identity_blocked_fixed_field_module_custom_missing_resref_matches =
            self.exact_placeable_add_identity_blocked_fixed_field_module_custom_missing_resref_matches
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_fixed_field_module_custom_missing_resref_matches,
                );
        self.exact_placeable_add_identity_blocked_fixed_field_ambiguous_matches = self
            .exact_placeable_add_identity_blocked_fixed_field_ambiguous_matches
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_fixed_field_ambiguous_matches,
            );
        self.exact_placeable_add_identity_blocked_fixed_field_ambiguous_match_rows = self
            .exact_placeable_add_identity_blocked_fixed_field_ambiguous_match_rows
            .saturating_add(
                rewrite.exact_placeable_add_identity_blocked_fixed_field_ambiguous_match_rows,
            );
        self.exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_rows = self
            .exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_rows
            .saturating_add(
                rewrite
                    .exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_rows,
            );
        self.exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_missing_resref_rows =
            self.exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_missing_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_identity_blocked_fixed_field_ambiguous_module_custom_missing_resref_rows,
                );
        self.exact_placeable_update_identity_blocked_module_custom_rows = self
            .exact_placeable_update_identity_blocked_module_custom_rows
            .saturating_add(rewrite.exact_placeable_update_identity_blocked_module_custom_rows);
        self.exact_placeable_update_identity_blocked_module_custom_missing_resref_rows = self
            .exact_placeable_update_identity_blocked_module_custom_missing_resref_rows
            .saturating_add(
                rewrite.exact_placeable_update_identity_blocked_module_custom_missing_resref_rows,
            );
        self.exact_placeable_add_no_overlap = self
            .exact_placeable_add_no_overlap
            .saturating_add(rewrite.exact_placeable_add_no_overlap);
        self.exact_placeable_update_no_overlap = self
            .exact_placeable_update_no_overlap
            .saturating_add(rewrite.exact_placeable_update_no_overlap);
        self.exact_placeable_add_unique_unchanged = self
            .exact_placeable_add_unique_unchanged
            .saturating_add(rewrite.exact_placeable_add_unique_unchanged);
        self.exact_placeable_update_unique_unchanged = self
            .exact_placeable_update_unique_unchanged
            .saturating_add(rewrite.exact_placeable_update_unique_unchanged);
        self.exact_placeable_appearance_custom_skipped = self
            .exact_placeable_appearance_custom_skipped
            .saturating_add(rewrite.exact_placeable_appearance_custom_skipped);
        self.exact_placeable_add_module_custom_appearance_skipped = self
            .exact_placeable_add_module_custom_appearance_skipped
            .saturating_add(rewrite.exact_placeable_add_module_custom_appearance_skipped);
        self.exact_placeable_update_module_custom_appearance_skipped = self
            .exact_placeable_update_module_custom_appearance_skipped
            .saturating_add(rewrite.exact_placeable_update_module_custom_appearance_skipped);
        self.exact_placeable_add_module_custom_template_resref_fixed_width_skipped = self
            .exact_placeable_add_module_custom_template_resref_fixed_width_skipped
            .saturating_add(
                rewrite.exact_placeable_add_module_custom_template_resref_fixed_width_skipped,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_update = self
            .exact_placeable_add_module_custom_template_resref_fixed_width_with_update
            .saturating_add(
                rewrite.exact_placeable_add_module_custom_template_resref_fixed_width_with_update,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_update_position_output_equivalence =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_update_position_output_equivalence
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_update_position_output_equivalence,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update = self
            .exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update
            .saturating_add(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_ready =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_ready
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_ready,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_blocked =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_blocked
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_blocked,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_with_normal_update_custom_rewrite_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update = self
            .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update
            .saturating_add(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_ready =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_ready
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_ready,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_blocked =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_blocked
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_blocked,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_with_custom_update_custom_rewrite_unavailable_satisfied_by_matching_carrier_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only_position_output_equivalence =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only_position_output_equivalence
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_update_only_position_output_equivalence,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_ready =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_ready
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_ready,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_blocked =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_blocked
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_blocked,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_normal_update_only_custom_rewrite_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_ready =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_ready
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_ready,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_blocked =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_blocked
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_blocked,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_pre_add_custom_update_only_custom_rewrite_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_add_only = self
            .exact_placeable_add_module_custom_template_resref_fixed_width_add_only
            .saturating_add(
                rewrite.exact_placeable_add_module_custom_template_resref_fixed_width_add_only,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_planned =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_planned
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_planned,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_insert_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_insert_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_insert_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_start_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_start_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_start_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_end_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_end_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_anchor_end_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_placeable_add_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_placeable_add_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_placeable_add_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_normal_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_normal_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_normal_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_custom_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_custom_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_custom_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_after_add_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_after_add_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_offset_after_add_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_boundary_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_boundary_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_boundary_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_source_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_source_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_source_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_duplicate_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_duplicate_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_duplicate_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_placeable_add_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_placeable_add_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_placeable_add_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_normal_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_normal_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_normal_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_custom_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_custom_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_custom_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_after_add_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_after_add_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_plan_anchor_after_add_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_emit_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_emit_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_emit_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_payload_build_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_payload_build_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_payload_build_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_header_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_header_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_header_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_declared_length_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_declared_length_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_declared_length_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_fragment_bits_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_fragment_bits_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_fragment_bits_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_boundary_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_boundary_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_boundary_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_record_validator_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_record_validator_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_record_validator_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_cursor_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_cursor_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_cursor_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_placeable_add_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_placeable_add_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_placeable_add_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_normal_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_normal_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_normal_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_custom_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_custom_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_custom_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_add_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_add_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_add_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_before_synthesized_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_before_synthesized_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_before_synthesized_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_inside_synthesized_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_inside_synthesized_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_inside_synthesized_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_synthesized_update_rejected =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_synthesized_update_rejected
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_after_synthesized_update_rejected,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_placeable_add =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_placeable_add
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_placeable_add,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_normal_update =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_normal_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_normal_update,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_custom_update =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_custom_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_custom_update,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_after_add =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_after_add
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_batch_claim_row_dropped_after_add,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_without_carrier =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_without_carrier
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_without_carrier,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_ready =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_ready
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_ready,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_blocked =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_blocked
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_blocked,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_match =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_match
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_match,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_normal_rewrite_target_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_match =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_match
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_match,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_pre_add_custom_rewrite_target_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_normal_rewrite_target_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_mismatch =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_mismatch,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable =
            self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable,
                );
        self.exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable_reasons
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_template_resref_fixed_width_synthesized_update_after_add_following_custom_rewrite_target_unavailable_reasons,
            );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_skipped = self
            .exact_placeable_add_module_custom_fixed_width_unproven_carrier_skipped
            .saturating_add(
                rewrite.exact_placeable_add_module_custom_fixed_width_unproven_carrier_skipped,
            );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_missing_template_resref_rows =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_missing_template_resref_rows,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_divergent =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_fixed_field_fixed_output_divergent,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_missing_template_resref_rows =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_missing_template_resref_rows,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_divergent =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_fixed_output_divergent,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_only_fixed_output =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_only_fixed_output
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_following_position_only_fixed_output,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_missing_template_resref_rows =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_missing_template_resref_rows,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_divergent =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_preceding_position_fixed_output_divergent,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_missing_template_resref_rows =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_missing_template_resref_rows,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_divergent =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_fixed_output_divergent,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_only_fixed_output =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_only_fixed_output
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_surrounding_position_only_fixed_output,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_missing_template_resref_rows =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_missing_template_resref_rows
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_missing_template_resref_rows,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_output_divergent =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_output_divergent
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_output_divergent,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_update = self
            .exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_update
            .saturating_add(
                rewrite.exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_update,
            );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_normal_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_normal_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_normal_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_custom_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_custom_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_custom_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_position_only_fixed_output =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_position_only_fixed_output
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_position_only_fixed_output,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_normal_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_normal_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_normal_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_custom_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_custom_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_custom_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_add_only = self
            .exact_placeable_add_module_custom_fixed_width_unproven_carrier_add_only
            .saturating_add(
                rewrite.exact_placeable_add_module_custom_fixed_width_unproven_carrier_add_only,
            );
        self.exact_placeable_add_module_custom_template_resref_missing = self
            .exact_placeable_add_module_custom_template_resref_missing
            .saturating_add(rewrite.exact_placeable_add_module_custom_template_resref_missing);
        self.exact_placeable_update_module_custom_template_resref_missing = self
            .exact_placeable_update_module_custom_template_resref_missing
            .saturating_add(rewrite.exact_placeable_update_module_custom_template_resref_missing);
        self.exact_placeable_add_source_custom_appearance_rewritten = self
            .exact_placeable_add_source_custom_appearance_rewritten
            .saturating_add(rewrite.exact_placeable_add_source_custom_appearance_rewritten);
        self.exact_placeable_update_source_custom_appearance_rewritten = self
            .exact_placeable_update_source_custom_appearance_rewritten
            .saturating_add(rewrite.exact_placeable_update_source_custom_appearance_rewritten);
        self.exact_placeable_update_module_custom_appearance_rewritten = self
            .exact_placeable_update_module_custom_appearance_rewritten
            .saturating_add(rewrite.exact_placeable_update_module_custom_appearance_rewritten);
        self.exact_placeable_add_appearance_rewritten = self
            .exact_placeable_add_appearance_rewritten
            .saturating_add(rewrite.exact_placeable_add_appearance_rewritten);
        self.exact_placeable_add_state_rewritten = self
            .exact_placeable_add_state_rewritten
            .saturating_add(rewrite.exact_placeable_add_state_rewritten);
        self.exact_placeable_update_position_rewritten = self
            .exact_placeable_update_position_rewritten
            .saturating_add(rewrite.exact_placeable_update_position_rewritten);
        self.exact_placeable_update_appearance_rewritten = self
            .exact_placeable_update_appearance_rewritten
            .saturating_add(rewrite.exact_placeable_update_appearance_rewritten);
        self.exact_placeable_update_appearance_exact_rejected = self
            .exact_placeable_update_appearance_exact_rejected
            .saturating_add(rewrite.exact_placeable_update_appearance_exact_rejected);
        self.exact_placeable_update_orientation_rewritten = self
            .exact_placeable_update_orientation_rewritten
            .saturating_add(rewrite.exact_placeable_update_orientation_rewritten);
        self.exact_placeable_update_state_rewritten = self
            .exact_placeable_update_state_rewritten
            .saturating_add(rewrite.exact_placeable_update_state_rewritten);
        true
    }

    fn record_add(&mut self, changed: bool) {
        if changed {
            self.add_passes_changed = self.add_passes_changed.saturating_add(1);
        }
    }

    fn record_add_name_bits(&mut self, changed: bool) {
        if changed {
            self.add_name_bit_passes_changed = self.add_name_bit_passes_changed.saturating_add(1);
        }
    }

    pub fn changed(self) -> bool {
        self.update_passes_changed != 0
            || self.add_passes_changed != 0
            || self.add_name_bit_passes_changed != 0
    }
}

pub fn rewrite_payload_if_needed_with_area_context(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> Option<RewriteSummary> {
    live_object_update::rewrite_update_records_payload_with_area_context_if_possible(
        payload,
        latest_area_placeables,
    )
}

pub fn rewrite_payload_if_needed_with_area_context_attempt(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> RewriteAttempt {
    live_object_update::rewrite_update_records_payload_with_area_context_attempt(
        payload,
        latest_area_placeables,
    )
}

pub fn promote_work_remaining_trailing_fragment_span_if_needed(
    payload: &mut Vec<u8>,
) -> Option<RewriteSummary> {
    live_object_update::promote_work_remaining_trailing_fragment_span_payload_if_possible(payload)
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClaimSummary> {
    live_object_update::claim_payload_if_verified(payload)
}

pub fn claim_payload_if_verified_with_lifecycle<F>(
    payload: &[u8],
    is_already_materialized: F,
) -> Option<ClaimSummary>
where
    F: FnMut(u8, u32) -> bool,
{
    live_object_update::claim_payload_if_verified_with_lifecycle(payload, is_already_materialized)
}

pub fn remove_unmaterialized_update_records_payload_if_possible<F>(
    payload: &mut Vec<u8>,
    is_already_materialized: F,
) -> Option<LifecycleRewriteSummary>
where
    F: FnMut(u8, u32) -> bool,
{
    live_object_update::remove_unmaterialized_update_records_payload_if_possible(
        payload,
        is_already_materialized,
    )
}

pub fn rewrite_add_name_fragment_bits_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<AddNameBitRewriteSummary> {
    live_object_update::rewrite_add_name_fragment_bits_payload_if_possible(payload)
}

pub fn canonicalize_compact_external_object_ids_payload_for_ee(
    payload: &mut Vec<u8>,
) -> Option<ExternalObjectIdCanonicalizeSummary> {
    live_object_update::canonicalize_compact_external_object_ids_payload_for_ee(payload)
}

pub fn canonicalize_player_session_creature_ids_payload_for_ee<F>(
    payload: &mut Vec<u8>,
    session_creature_id_for_compact: F,
) -> Option<ExternalObjectIdCanonicalizeSummary>
where
    F: FnMut(u32) -> Option<u32>,
{
    live_object_update::canonicalize_player_session_creature_ids_payload_for_ee(
        payload,
        session_creature_id_for_compact,
    )
}

pub fn rewrite_payload_to_exact_ee_if_possible(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> Option<ExactLiveObjectRewriteSummary> {
    // This adapter deliberately owns orchestration only. The actual record
    // semantics remain in the focused decompile-backed live-object modules:
    // add-record visual/name translation, update-record translation, and the
    // exact EE reader-shape validator. Local Diamond captures can alternate
    // `A` and `U` records such that one focused pass exposes the next typed
    // record boundary. Use a bounded sequence matching direct dispatch rather
    // than a raw passthrough or unbounded resync loop.
    let mut candidate = payload.clone();
    let mut summary = ExactLiveObjectRewriteSummary::default();
    let mut add_name_bits_attempted = false;
    let max_alternating_add_update_passes = candidate
        .len()
        .saturating_div(10)
        .saturating_add(4)
        .min(128)
        .max(16);

    summary.record_update_summary(promote_work_remaining_trailing_fragment_span_if_needed(
        &mut candidate,
    ));
    if exact_after_changed(&candidate, summary) {
        *payload = candidate;
        return Some(summary);
    }

    summary.record_add(
        live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut candidate,
            latest_area_placeables,
        )
        .is_some(),
    );
    summary.record_update_summary(rewrite_payload_if_needed_with_area_context(
        &mut candidate,
        latest_area_placeables,
    ));
    if exact_after_changed(&candidate, summary) {
        *payload = candidate;
        return Some(summary);
    }

    for _ in 0..max_alternating_add_update_passes {
        let add_changed =
            live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
                &mut candidate,
                latest_area_placeables,
            )
            .is_some();
        summary.record_add(add_changed);
        if exact_after_changed(&candidate, summary) {
            *payload = candidate;
            return Some(summary);
        }

        let update_changed = summary.record_update_summary(
            rewrite_payload_if_needed_with_area_context(&mut candidate, latest_area_placeables),
        );
        if exact_after_changed(&candidate, summary) {
            *payload = candidate;
            return Some(summary);
        }

        let mut add_name_bits_changed = false;
        if !add_name_bits_attempted {
            add_name_bits_changed =
                rewrite_add_name_fragment_bits_payload_if_possible(&mut candidate).is_some();
            summary.record_add_name_bits(add_name_bits_changed);
            add_name_bits_attempted = true;
            if exact_after_changed(&candidate, summary) {
                *payload = candidate;
                return Some(summary);
            }
        }

        if !add_changed && !update_changed && !add_name_bits_changed {
            break;
        }
    }

    // Candidate passes are intentionally speculative. If a later record, such
    // as a bit-short U/9 before a read-buffer-only W row, prevents the exact EE
    // reader claim, discard every staged edit instead of emitting a mixed stream.
    if !summary.changed() || claim_payload_if_verified(&candidate).is_none() {
        return None;
    }

    *payload = candidate;
    Some(summary)
}

fn exact_after_changed(candidate: &[u8], summary: ExactLiveObjectRewriteSummary) -> bool {
    summary.changed() && claim_payload_if_verified(candidate).is_some()
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn local_diamond_seq26_gui_inventory_missing_add_opcodes_rewrite_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_diamond_seq26_gui_inventory_missing_add_opcode_20260518_legacy.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw local Diamond GUI inventory fixture documents the zero-declared pre-EE shape"
        );
        let normalize = live_object::normalize_prefixed_fragments_payload_if_needed(&mut payload)
            .expect("zero-declared local Diamond GUI inventory stream should normalize");
        assert_eq!(normalize.old_wire_declared, 0);
        assert_eq!(
            normalize.dropped_leadin_bytes, 0,
            "normalization must not salvage past the first unclaimed G I 00 row"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("typed GUI item-add repair should rewrite missing inner add opcodes");
        assert!(
            summary.changed(),
            "the GUI compatibility row must be an explicit semantic rewrite"
        );
        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten GUI inventory/repository burst must be exact EE live-object shape");
        assert!(
            claim.live_gui_item_create_records >= 8,
            "fixture should retain the five inventory and three repository item-create rows"
        );
        assert_eq!(
            claim.live_bytes_length + 7,
            claim.declared,
            "declared CNW read window should exactly cover the rewritten GUI live bytes"
        );
    }

    #[test]
    fn local_cepv22_seq16_pending_stream_stays_quarantined_after_boundary_audit() {
        // Local CEP v2.2 builder harness capture from 2026-05-20. The legacy
        // server split one logical zero-declared live-object stream across
        // several deflated M windows; the stream buffer rebuilds the CNW
        // read-buffer bytes and fragment storage into this single candidate.
        // Keep it quarantined until the post-rewrite U/0x55 boundary has a
        // decompile-backed cursor owner rather than relying on the old broad
        // exact-rewrite expectation.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_cepv22_builder_seq16_pending_chunks4_20260520.bin"
        )
        .to_vec();
        let original = payload.clone();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw rebuilt seq16 stream is still legacy-shaped before typed rewrites"
        );

        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "CEP v2.2 seq16 pending stream must not exact-rewrite without a proven record boundary"
        );
        assert_eq!(
            payload, original,
            "failed exact rewrite must leave rebuilt boundary evidence unchanged"
        );
    }

    #[test]
    fn local_dark_ranger_seq13_current_player_stream_rewrites_to_exact_shape() {
        // Local Dark Ranger harness capture from 2026-05-21. The Diamond
        // server emitted a stale declared read-window over a current-player
        // add/appearance/update stream; strict dispatch quarantined it until
        // the typed live-object translators owned the full EE reader shape.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_dark_ranger_seq13_current_player_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Dark Ranger current-player stream documents the pre-rewrite Diamond shape"
        );
        let repair = live_object::declared_length_repair_candidates(&payload)
            .into_iter()
            .filter(|candidate| {
                live_object::declared_length_repair_tail_contains_live_object_read_boundary(
                    &payload, candidate,
                ) && live_object::declared_length_repair_read_window_ends_after_creature_appearance_update_pair(
                    &payload, candidate,
                )
            })
            .max_by_key(|candidate| candidate.read_bytes_length)
            .expect("fixture should contain a bounded current-player appearance/update split");
        payload[3..7].copy_from_slice(&repair.new_declared.to_le_bytes());

        let started = std::time::Instant::now();
        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Dark Ranger current-player stream should rewrite to exact EE shape");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "Dark Ranger current-player rewrite must stay bounded"
        );

        assert!(summary.changed());
        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Dark Ranger current-player stream must validate exactly");
        assert!(claim.add_records >= 1);
        assert!(claim.creature_appearance_records >= 1);
        assert!(claim.creature_update_records >= 1);
    }

    #[test]
    fn local_dark_ranger_seq15_pending_stream_rewrites_to_exact_shape() {
        // Pending stream from the same Dark Ranger harness pass. It preserves
        // the legacy action-0 0x3967 missing-damage-byte shape, so the bridge
        // must repair it before exact EE validation.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_dark_ranger_seq15_pending_claimed_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(claim_payload_if_verified(&payload).is_none());
        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Dark Ranger pending live-object stream should rewrite");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("Dark Ranger pending live-object stream must validate exactly");
        assert_eq!(claim.live_bytes_length + 7, claim.declared);
        assert!(claim.records_examined >= 1);
        assert!(claim.add_records >= 1);
    }

    #[test]
    fn local_winds_eremor_seq22_placeable_stream_rewrites_to_exact_shape() {
        // Local Winds of Eremor harness capture from 2026-05-21. The module's
        // opening area emits several placeable adds in a coalesced live-object
        // stream after the area gate opens.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_winds_eremor_seq22_placeable_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Winds of Eremor placeable burst documents the pre-rewrite Diamond shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Winds of Eremor placeable stream should rewrite to exact EE shape");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Winds of Eremor stream should validate exactly");
        assert!(claim.add_records >= 1);
        assert!(claim.creature_update_records >= 1);
        assert!(claim.inventory_records >= 1);
    }

    #[test]
    fn local_winds_eremor_seq16_placeable_pending_stream_rewrites_after_37_fragment_audit() {
        // Earlier local Winds of Eremor area-entry burst from the same module:
        // four declared-zero Diamond chunks rebuilt by the pending stream
        // accumulator before a later independent live-object packet arrives.
        // After the 0x37 cursor audit and declared-window repairs, this stream
        // now proves each final U/9 row from the decompiled scalar/state cursor:
        // Diamond source bits provide position + scalar orientation + five
        // state BOOLs, the bridge inserts EE's neutral sixth placeable-state
        // BOOL, and the following W rows remain fragment-neutral identity
        // records rather than donating any cursor bits.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_winds_eremor_seq16_placeable_pending_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Winds of Eremor pending stream documents the pre-rewrite Diamond shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Winds of Eremor pending stream should rewrite after 0x37 cursor proof");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Winds of Eremor pending stream must validate exactly");
        assert!(claim.add_records >= 1);
        assert!(claim.update_records >= 1);
        assert!(
            claim.world_status_records >= 1,
            "W records must stay as fragment-neutral identity records"
        );
    }

    #[test]
    fn local_winds_eremor_seq24_creature_stream_rewrites_to_exact_shape() {
        // Follow-up live-object stream from the same local module after the
        // placeable burst. It is kept separate because the root quarantine
        // captured it as a standalone deflated payload.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_winds_eremor_seq24_creature_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Winds of Eremor creature burst documents the pre-rewrite Diamond shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Winds of Eremor creature stream should rewrite to exact EE shape");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Winds of Eremor creature stream should validate exactly");
        assert!(claim.creature_update_records >= 1);
        assert!(claim.live_gui_read_buffer_records >= 1);
    }

    #[test]
    fn local_contest_seq13_creature_placeable_stream_stays_unclaimed_after_37_fragment_audit() {
        // Local Contest Of Champions 0492 harness capture from 2026-05-21.
        // The opening inventory probe emits a mixed creature/placeable stream
        // with Diamond compact placeable add/update tails. The final 0x37
        // placeable update still lacks the EE scalar/state fragment bits needed
        // to own the row exactly.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_contest_seq13_creature_placeable_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Contest seq13 stream documents the pre-rewrite Diamond shape"
        );

        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "Contest seq13 must remain unclaimed until the terminal 0x37 fragment owner is proven"
        );
        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn local_contest_seq14_placeable_stream_stays_unclaimed_after_37_fragment_audit() {
        // Same local module pass as seq13. This packet is a dense placeable
        // add/update burst, useful because it repeats the compact low-tail
        // placeable family without any creature records in front. It is now
        // negative coverage for the same terminal 0x37 fragment shortage.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_contest_seq14_placeable_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Contest seq14 stream documents the pre-rewrite Diamond shape"
        );

        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "Contest seq14 must remain unclaimed until the terminal 0x37 fragment owner is proven"
        );
        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn local_prelude_seq10_pending_stream_stays_unclaimed_after_37_fragment_audit() {
        // Local Prelude 2026-05-22 opening area stream. The pending-stream
        // accumulator rebuilt this as one `P/5/1` candidate after the compact
        // Area_ClientArea repair; it contains a creature appearance/update
        // prefix, then a compact placeable add/update pair. The final
        // placeable 0x37 row still has no proven fragment owner before the
        // world-status tail, so the stream stays quarantined.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_prelude_seq10_pending_liveobject_20260522.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Prelude seq10 stream documents the pre-rewrite Diamond shape"
        );

        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "Prelude seq10 must remain unclaimed until the terminal 0x37 fragment owner is proven"
        );
        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn local_diamond_seq20_auto_inventory_gia_gra_claims_exact_ee_shape() {
        let payload = include_bytes!(
            "../../../fixtures/live_object/local_diamond_seq20_auto_inventory_gia_gra_20260519.bin"
        )
        .as_slice();

        let started = std::time::Instant::now();
        let claim = claim_payload_if_verified(payload)
            .expect("rebuilt local auto-inventory GUI stream must be exact EE live-object shape");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "auto-inventory GUI live-object claim must stay bounded"
        );

        assert!(
            claim.live_gui_item_create_records >= 5,
            "fixture should retain the inventory/repository GUI item-create rows"
        );
        assert_eq!(
            claim.live_bytes_length + 7,
            claim.declared,
            "declared CNW read window should exactly cover the rewritten GUI live bytes"
        );
    }

    #[test]
    fn pending_seq12_local_diamond_stream_stays_unclaimed_after_37_order_audit() {
        // This local seq12 evidence predates the 0x37 bit-order audit and can
        // only be made to pass by accepting scale-before-appearance
        // door/placeable update rows. Keep it as negative coverage until the
        // source side of those rows is re-captured or proven from decompiles.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_diamond_bw167demo_initial_live_object_seq12_20260517_unclaimed.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "fresh pending stream capture documents the live-stream quarantine before typed passes"
        );

        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "bounded typed live-object passes must reject the shifted seq12 0x37 evidence"
        );
        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "stale seq12 evidence remains unclaimed after the rejected rewrite attempt"
        );
    }

    #[test]
    fn local_diamond_seq12_missing_object_update_stays_unclaimed_after_37_order_audit() {
        // Local Diamond bridge capture from 2026-05-18. The older accepted
        // fixture carried door/placeable `U/9`/`U/10` 0x37 rows whose scale
        // bytes are in the pre-audit, scale-before-appearance position. The
        // decompiled Diamond and EE generic readers both consume appearance
        // before scale, so the bounded adapter must reject this capture until a
        // decompile-owned source shape is available.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_diamond_seq12_door_placeable_missing_object_update_20260518_legacy.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw local Diamond seq12 stream must not be accepted before typed rewrites"
        );

        let normalize = live_object::normalize_prefixed_fragments_payload_if_needed(&mut payload)
            .expect("raw local Diamond seq12 should normalize from zero-declared fragments");
        assert_eq!(normalize.old_wire_declared, 0);

        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "bounded typed live-object passes must not emit a stream with shifted 0x37 scale fields"
        );
        assert!(
            canonicalize_compact_external_object_ids_payload_for_ee(&mut payload).is_none(),
            "object-id canonicalization is gated by exact live-object cursor proof"
        );
        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "normalized seq12 evidence remains unclaimed after rejecting shifted 0x37 rows"
        );
    }

    #[test]
    fn hg_starc5_seq28_northern_trader_burst_rewrites_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_starc5_docks_seq28_northern_trader_visible_equipment_slow_20260518.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw HG Northern Trader burst documents the pre-rewrite Diamond live-object shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("bounded typed live-object passes should rewrite the HG Northern Trader burst");

        assert!(summary.changed());
        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten HG Northern Trader burst must be exact EE live-object shape");
        assert!(
            claim.creature_appearance_records >= 1,
            "fixture should retain the creature appearance record that carries visible equipment"
        );
        assert!(
            claim.creature_update_records >= 1,
            "fixture should retain the following typed creature update record"
        );
    }

    #[test]
    fn hg_starc5_seq36_town_greeter_trader_burst_rewrites_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_starc5_docks_seq36_town_greeter_northern_trader_slow_20260518.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw HG Town Greeter/Northern Trader burst documents the pre-rewrite Diamond live-object shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None).expect(
            "bounded typed live-object passes should rewrite the HG Town Greeter/Northern Trader burst",
        );

        assert!(summary.changed());
        let claim = claim_payload_if_verified(&payload).expect(
            "rewritten HG Town Greeter/Northern Trader burst must be exact EE live-object shape",
        );
        assert!(
            claim.creature_appearance_records >= 2,
            "fixture should retain both NPC creature appearance records"
        );
        assert!(
            claim.creature_update_records >= 1,
            "fixture should retain the following typed creature update record"
        );
    }

    #[test]
    fn hg_starc5_seq37_creature_update_burst_rewrites_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_starc5_docks_seq37_creature_update_slow_20260518.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw HG seq37 burst documents the pre-rewrite Diamond live-object shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("bounded typed live-object passes should rewrite the HG seq37 creature burst");

        assert!(summary.changed());
        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten HG seq37 creature burst must be exact EE live-object shape");
        assert!(
            claim.creature_appearance_records >= 1,
            "fixture should retain the creature appearance/update context"
        );
        assert!(
            claim.creature_update_records >= 1,
            "fixture should retain the typed creature update record"
        );
    }

    #[test]
    fn hg_live_seq37_creature_4008_burst_rewrites_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_live_seq37_creature_4008_captain_ogric_20260519.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw HG seq37 0x4008 creature burst documents the pre-rewrite Diamond live-object shape"
        );

        let started = std::time::Instant::now();
        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None).expect(
            "bounded typed live-object passes should rewrite the HG seq37 0x4008 creature burst",
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "seq37 0x4008 rewrite must stay bounded enough for the reliable window"
        );

        assert!(summary.changed());
        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten HG seq37 0x4008 creature burst must be exact EE live-object shape");
        assert!(
            claim.creature_update_records >= 1,
            "fixture should retain the typed 0x4008 creature update record"
        );
        assert!(
            claim.add_records >= 1 && claim.creature_appearance_records >= 1,
            "fixture should retain the following Captain/Ogric add and appearance records"
        );
    }

    #[test]
    fn hg_live_town_greeter_northern_trader_bursts_rewrite_bounded_and_exact() {
        for payload in [
            include_bytes!(
                "../../../fixtures/live_object/hg_live_seq38_town_greeter_northern_trader_20260519.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/hg_live_seq39_town_greeter_northern_trader_20260519.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/hg_live_seq40_town_greeter_northern_trader_20260519.bin"
            )
            .as_slice(),
        ] {
            let mut payload = payload.to_vec();
            assert!(
                claim_payload_if_verified(&payload).is_none(),
                "raw HG Town Greeter/Northern Trader burst documents the pre-rewrite Diamond live-object shape"
            );

            let started = std::time::Instant::now();
            let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None).expect(
                "bounded typed live-object passes should rewrite the current HG greeter/trader burst",
            );
            let elapsed = started.elapsed();
            assert!(
                elapsed < std::time::Duration::from_secs(2),
                "current HG greeter/trader rewrite must stay bounded enough for the reliable window, elapsed={elapsed:?}"
            );

            assert!(summary.changed());
            let claim = claim_payload_if_verified(&payload)
                .expect("rewritten current HG greeter/trader burst must be exact EE shape");
            assert!(
                claim.creature_appearance_records >= 2,
                "fixture should retain both NPC creature appearance records"
            );
            assert!(
                claim.creature_update_records >= 1,
                "fixture should retain the following typed creature update record"
            );
        }
    }

    #[test]
    fn hg_starc5_seq38_creature_update_rewrite_is_bounded_and_exact() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_starc5_docks_seq38_creature_update_unacked_20260518.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw HG seq38 burst documents the slow pre-rewrite Diamond live-object shape"
        );

        let started = std::time::Instant::now();
        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("bounded typed live-object passes should rewrite the HG seq38 creature burst");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "seq38 rewrite must stay bounded enough for the reliable window"
        );

        assert!(summary.changed());
        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten HG seq38 creature burst must be exact EE live-object shape");
        assert!(
            claim.creature_appearance_records >= 1,
            "fixture should retain Otis' creature appearance record"
        );
        assert!(
            claim.creature_update_records >= 1,
            "fixture should retain the typed creature update record"
        );
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod mixed_live_object_regression_tests {
    use super::*;
    use crate::translate::live_object_update::claim_payload_if_verified;
    use std::time::{Duration, Instant};

    #[test]
    fn hg_seq40_creature_otis_mixed_burst_rewrite_is_bounded_and_exact() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_seq40_creature_otis_mixed_add_update.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "legacy HG fixture should not already be EE-exact before translation"
        );

        let started = Instant::now();
        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None).expect(
            "bounded live-object orchestrator should rewrite the mixed Otis/Elrawiel burst",
        );
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(3),
            "mixed live-object rewrite should be bounded, elapsed={elapsed:?}"
        );
        assert!(
            summary.changed(),
            "expected at least one typed live-object rewrite"
        );

        let claim = claim_payload_if_verified(&payload)
            .expect("translated mixed live-object burst should have an exact EE claim");
        assert!(
            claim.add_records > 0,
            "expected at least one fixed live-object add record in the exact claim"
        );
        assert!(
            claim.creature_appearance_records > 0,
            "expected at least one EE creature appearance/name record in the exact claim"
        );
        assert!(
            claim.creature_update_records > 0,
            "expected at least one EE creature update record in the exact claim"
        );
    }
}
