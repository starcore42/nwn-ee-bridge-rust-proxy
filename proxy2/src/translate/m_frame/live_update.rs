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
pub type ClaimDiagnostics = live_object_update::LiveObjectPayloadClaimDiagnostics;
pub type AddNameBitRewriteSummary = live_object_update::LiveObjectAddNameBitRewriteSummary;
pub type ExternalObjectIdCanonicalizeSummary =
    live_object_update::LiveObjectExternalObjectIdCanonicalizeSummary;
pub type LifecycleRewriteSummary = live_object_update::LiveObjectLifecycleRewriteSummary;

pub fn claim_payload_diagnostics(payload: &[u8]) -> ClaimDiagnostics {
    live_object_update::claim_payload_diagnostics(payload)
}

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
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_rewrite_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_unchanged_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_rewrite_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_unchanged_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_rewrite_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_unchanged_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_rewrite_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_unchanged_targets:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocker_slots:
        live_object_update::ExactPlaceableUnprovenCustomCarrierSourceBlockerSlots,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_synthesis_gate_slots:
        live_object_update::ExactPlaceableUnprovenCustomCarrierSynthesisGateSlots,
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
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_update: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_normal_update:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_custom_update:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_normal_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_custom_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_add_only: u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_update:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_normal_update:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_custom_update:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_normal_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_custom_update_only:
        u32,
    pub exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_add_only:
        u32,
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
    pub(crate) fn exact_placeable_unproven_custom_carrier_disposition(
        &self,
    ) -> live_object_update::ExactPlaceableUnprovenCustomCarrierDisposition {
        live_object_update::ExactPlaceableUnprovenCustomCarrierDisposition::from_counts(
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_skipped,
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked,
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_rewrite_targets,
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_unchanged_targets,
        )
    }

    pub(crate) fn exact_placeable_unproven_custom_carrier_writer_gap_slots(
        &self,
    ) -> live_object_update::ExactPlaceableUnprovenCustomCarrierWriterGapSlotSummary {
        live_object_update::ExactPlaceableUnprovenCustomCarrierWriterGapSlotSummary::from_slots(
            live_object_update::ExactPlaceableUnprovenCustomCarrierWriterGapSlots {
                with_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_update,
                with_normal_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_normal_update,
                with_custom_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_custom_update,
                pre_add_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_update_only,
                pre_add_normal_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_normal_update_only,
                pre_add_custom_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_custom_update_only,
                add_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_add_only,
            },
            live_object_update::ExactPlaceableUnprovenCustomCarrierWriterGapSlots {
                with_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_update,
                with_normal_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_normal_update,
                with_custom_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_custom_update,
                pre_add_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_update_only,
                pre_add_normal_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_normal_update_only,
                pre_add_custom_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_custom_update_only,
                add_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_add_only,
            },
        )
    }

    pub(crate) fn exact_placeable_unproven_custom_carrier_slot_disposition(
        &self,
    ) -> live_object_update::ExactPlaceableUnprovenCustomCarrierSlotDisposition {
        let writer_gap_slots = self.exact_placeable_unproven_custom_carrier_writer_gap_slots();
        live_object_update::ExactPlaceableUnprovenCustomCarrierSlotDisposition::from_slots(
            live_object_update::ExactPlaceableUnprovenCustomCarrierWriterGapSlots {
                with_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_update,
                with_normal_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_normal_update,
                with_custom_update: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_with_custom_update,
                pre_add_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_update_only,
                pre_add_normal_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_normal_update_only,
                pre_add_custom_update_only: self
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_pre_add_custom_update_only,
                add_only: self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_add_only,
            },
            writer_gap_slots.all,
            writer_gap_slots.source_blocked,
        )
    }

    pub(crate) fn exact_placeable_unproven_custom_carrier_synthesis_resolution(
        &self,
    ) -> live_object_update::ExactPlaceableUnprovenCustomCarrierSynthesisResolution {
        live_object_update::ExactPlaceableUnprovenCustomCarrierSynthesisResolution::from_gate_slots(
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_synthesis_gate_slots,
        )
    }

    pub(crate) fn exact_placeable_custom_carrier_target_unavailable_resolution(
        &self,
    ) -> live_object_update::ExactPlaceableCustomCarrierTargetUnavailableResolution {
        live_object_update::ExactPlaceableCustomCarrierTargetUnavailableResolution::from_scoped(
            self.exact_placeable_custom_carrier_selected_target_unavailable_reasons_by_scope(),
            self.exact_placeable_custom_carrier_committed_target_unavailable_reasons_by_scope(),
            self.exact_placeable_custom_carrier_satisfied_target_unavailable_reasons_by_scope(),
        )
    }

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
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked = self
            .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked
            .saturating_add(
                rewrite
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked,
            );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_rewrite_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_rewrite_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_rewrite_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_unchanged_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_unchanged_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_field_unchanged_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_rewrite_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_rewrite_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_rewrite_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_unchanged_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_unchanged_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_field_unchanged_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_rewrite_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_rewrite_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_rewrite_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_unchanged_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_unchanged_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_fragment_owned_field_unchanged_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_rewrite_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_rewrite_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_rewrite_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_unchanged_targets =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_unchanged_targets
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_read_mismatch_and_fragment_owned_field_unchanged_targets,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocker_slots
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocker_slots,
            );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_synthesis_gate_slots
            .saturating_add_assign(
                rewrite
                    .exact_placeable_add_module_custom_fixed_width_unproven_carrier_synthesis_gate_slots,
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
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_normal_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_normal_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_normal_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_custom_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_custom_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_with_custom_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_normal_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_normal_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_normal_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_custom_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_custom_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_pre_add_custom_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_add_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_add_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_writer_gap_add_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_normal_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_normal_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_normal_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_custom_update =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_custom_update
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_with_custom_update,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_normal_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_normal_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_normal_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_custom_update_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_custom_update_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_pre_add_custom_update_only,
                );
        self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_add_only =
            self.exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_add_only
                .saturating_add(
                    rewrite
                        .exact_placeable_add_module_custom_fixed_width_unproven_carrier_source_blocked_writer_gap_add_only,
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

    // Diamond `CNWSMessage::SendServerToPlayerGameObjUpdate` may interleave a
    // legacy `A/0A, U/0A, A/09, U/09, ...` stream in one CNW read window.
    // The update-family pass owns the exact source `U` field order and shared
    // MSB-first fragment cursor, and can use a following same-object update to
    // bound the preceding add while inserting EE's visual-transform identity.
    // Run that typed pass before the standalone add-map pass: inserting every
    // add map first can move the read boundaries while the legacy `U` masks
    // are still present, leaving neither focused pass able to commit its safe
    // intermediate form. The final exact EE claim below remains mandatory.
    summary.record_update_summary(rewrite_payload_if_needed_with_area_context(
        &mut candidate,
        latest_area_placeables,
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

#[cfg(test)]
pub(super) fn alternating_legacy_door_placeable_test_payload() -> Vec<u8> {
    fixture_free_tests::alternating_legacy_door_placeable_payload()
}

#[cfg(test)]
mod fixture_free_tests {
    use super::*;

    pub(super) fn alternating_legacy_door_placeable_payload() -> Vec<u8> {
        let door_id = 0x8000_1001u32;
        let first_placeable_id = 0x8000_1002u32;
        let second_placeable_id = 0x8000_1003u32;
        let first_name = b"Storage Drum";
        let second_name = b"Generic Placeable Interaction Gate";
        assert_eq!(first_name.len(), 12);
        assert_eq!(second_name.len(), 34);

        let mut live = Vec::new();

        // Diamond door add: A/type/id, empty name branch, fixed tail. EE's
        // reader inserts the eight-byte object visual-transform identity
        // before the final model token WORD.
        live.extend_from_slice(&[b'A', 10]);
        live.extend_from_slice(&door_id.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&0x0000_14E5u32.to_le_bytes());
        live.extend_from_slice(&0x0033u16.to_le_bytes());

        // Diamond door U/0A all-bits mask. Both readers consume appearance
        // before scale/state; the EE writer narrows this source mask to 0x17
        // and inserts the sixth door/placeable state BOOL.
        live.extend_from_slice(&[b'U', 10]);
        live.extend_from_slice(&door_id.to_le_bytes());
        live.extend_from_slice(&0xFFFF_FFF7u32.to_le_bytes());
        live.extend_from_slice(&[
            0x0C, 0x17, 0x66, 0x1C, 0x0F, 0x0F, 0x00, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x33,
            0x00, 0xE5, 0x14, 0x00, 0x00,
        ]);

        for (object_id, name, appearance, update_body) in [
            (
                first_placeable_id,
                first_name.as_slice(),
                0x01CFu16,
                [
                    0x43, 0x19, 0x1A, 0x1D, 0x11, 0x0F, 0x00, 0xE7, 0x03, 0x00, 0x00, 0x80, 0x3F,
                    0x00, 0x00,
                ],
            ),
            (
                second_placeable_id,
                second_name.as_slice(),
                0x0090u16,
                [
                    0x5A, 0x14, 0xFC, 0x1B, 0x0F, 0x0F, 0x00, 0xF6, 0x01, 0x00, 0x00, 0x80, 0x3F,
                    0x00, 0x00,
                ],
            ),
        ] {
            live.extend_from_slice(&[b'A', 9]);
            live.extend_from_slice(&object_id.to_le_bytes());
            live.extend_from_slice(&(name.len() as u32).to_le_bytes());
            live.extend_from_slice(name);
            live.push(0x05);
            live.extend_from_slice(&appearance.to_le_bytes());
            live.extend_from_slice(&0u16.to_le_bytes());

            live.extend_from_slice(&[b'U', 9]);
            live.extend_from_slice(&object_id.to_le_bytes());
            live.extend_from_slice(&0xFFFF_FFF7u32.to_le_bytes());
            live.extend_from_slice(&update_body);
            live.extend_from_slice(&(name.len() as u32).to_le_bytes());
            live.extend_from_slice(name);
        }

        let mut payload = vec![b'P', 0x05, 0x01];
        let declared = u32::try_from(7usize + live.len()).expect("fixture declared length");
        payload.extend_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(&live);
        // CNW fragments are MSB-first: the high three bits carry the final
        // valid-bit count, then the alternating add/update BOOLs continue in
        // source reader order across all six records.
        payload.extend_from_slice(&[0x9A, 0x60, 0x23, 0xAB, 0x88, 0x08, 0xD5, 0xC4, 0x04, 0x62]);
        payload
    }

    #[test]
    fn alternating_legacy_door_placeable_pairs_report_exact_terminal_residual() {
        let mut payload = alternating_legacy_door_placeable_payload();
        let original = payload.clone();
        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "Diamond add/update stream must not already pass the EE reader"
        );

        let attempt = rewrite_payload_if_needed_with_area_context_attempt(&mut payload, None);
        assert!(attempt.summary.is_none());
        let failure = attempt
            .failure
            .expect("typed update walk should retain the terminal cursor blocker");
        assert_eq!(
            failure.kind,
            live_object_update::LiveObjectUpdateRewriteFailureKind::
                DoorPlaceableTail9TerminalResidualFragmentBits
        );
        let evidence = failure
            .terminal_door_placeable_tail9_residual
            .expect("terminal tail9 failure should expose exact fragment evidence");
        assert_eq!(evidence.object_type, 0x09);
        assert_eq!(evidence.object_id, 0x8000_1003);
        assert_eq!(evidence.raw_mask, 0xFFFF_FFF7);
        assert_eq!(evidence.translated_mask, 0x0008_0017);
        assert_eq!(
            evidence.rewritten_fragment_bit_count - evidence.rewritten_bit_cursor,
            evidence.residual_fragment_bits
        );
        assert_eq!(evidence.source_fragment_bit_count, 76);
        assert_eq!(evidence.source_bit_cursor, 50);
        assert_eq!(evidence.source_reader_bit_cursor, 59);
        assert_eq!(evidence.source_reader_bits_consumed, 9);
        assert_eq!(evidence.source_name_selector_bit_cursor, Some(57));
        assert_eq!(evidence.source_name_selector, Some(true));
        assert_eq!(evidence.source_name_locstring_selector_bit_cursor, Some(58));
        assert_eq!(evidence.source_name_locstring_selector, Some(false));
        assert_eq!(
            evidence.source_name_kind,
            Some("locstring-inline-cexostring")
        );
        assert_eq!(evidence.source_reader_residual.bit_start, 59);
        assert_eq!(evidence.source_reader_residual.bit_end, 76);
        assert_eq!(evidence.source_reader_residual.bit_count, 17);
        assert_eq!(evidence.source_reader_residual.bits_retained, 16);
        assert_eq!(
            evidence.source_reader_residual.bits,
            [
                Some(false),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(true),
            ]
        );
        assert_eq!(evidence.emitted_bit_cursor, 57);
        assert_eq!(evidence.emitted_fragment_bit_count, 83);
        assert_eq!(evidence.rewritten_bit_cursor, 71);
        assert_eq!(evidence.rewritten_fragment_bit_count, 88);
        assert_eq!(evidence.residual_fragment_bits, 17);
        assert_eq!(evidence.rewritten_residual.bit_start, 71);
        assert_eq!(evidence.rewritten_residual.bit_end, 88);
        assert_eq!(
            evidence.rewritten_residual.bits,
            evidence.source_reader_residual.bits
        );
        let rewritten_residual_exact = evidence
            .rewritten_residual_exact
            .expect("all 17 emitted terminal bits should be retained compactly");
        assert_eq!(
            (
                rewritten_residual_exact.bit_start,
                rewritten_residual_exact.bit_end,
                rewritten_residual_exact.bit_count(),
            ),
            (71, 88, 17)
        );
        let expected_emitted_bits = [
            false, false, true, false, false, false, false, false, false, false, true, false,
            false, false, true, true, false,
        ];
        for (offset, expected) in expected_emitted_bits.into_iter().enumerate() {
            assert_eq!(
                rewritten_residual_exact.bit(71 + offset),
                Some(expected),
                "emitted residual bit {offset} must preserve MSB-first reader order"
            );
        }
        assert_eq!(evidence.proven_terminal_packed_name_bits, 0);
        let precursor = evidence
            .precursor_tail
            .expect("terminal evidence should retain the committed rewrite ledger");
        assert_eq!(precursor.entry_count, 5);
        assert_eq!(precursor.source_bit_end, 50);
        assert_eq!(precursor.emitted_bit_end, 57);
        assert_eq!(precursor.emitted_source_delta, 7);
        let preceding_add = precursor
            .entries
            .iter()
            .flatten()
            .last()
            .expect("second A/09 should be the preceding ledger owner");
        assert_eq!((preceding_add.offset, preceding_add.record_end), (125, 182));
        assert_eq!((preceding_add.opcode, preceding_add.marker), (b'A', 9));
        assert_eq!(
            (preceding_add.source_bit_start, preceding_add.source_bit_end),
            (40, 50)
        );
        assert_eq!(
            (
                preceding_add.emitted_bit_start,
                preceding_add.emitted_bit_end
            ),
            (46, 57)
        );
        assert_eq!(preceding_add.bits_inserted, 1);
        assert_eq!(preceding_add.bits_removed, 0);
        assert_eq!(preceding_add.family, "add-compact-rewrite");

        let stock_source = evidence
            .stock_diamond_source
            .expect("the same bytes should retain the competing stock Diamond reader walk");
        assert_eq!(stock_source.raw_mask, 0xFFFF_FFF7);
        assert_eq!(stock_source.effective_mask, 0x0008_0037);
        assert_eq!(stock_source.ignored_mask, 0xFFF7_FFC0);
        assert_eq!(stock_source.read_end, 245);
        assert_eq!(stock_source.source_bit_cursor, 50);
        assert_eq!(stock_source.source_reader_bit_cursor, 63);
        assert_eq!(stock_source.source_reader_bits_consumed, 13);
        assert_eq!(stock_source.source_orientation_vector, Some(false));
        assert_eq!(stock_source.source_state_bit_cursor, Some(57));
        assert_eq!(stock_source.source_name_selector_bit_cursor, Some(62));
        assert_eq!(stock_source.source_name_selector, Some(false));
        assert_eq!(stock_source.source_name_locstring_selector_bit_cursor, None);
        assert_eq!(stock_source.source_name_locstring_selector, None);
        assert_eq!(stock_source.source_name_kind, Some("direct-cexostring"));
        assert_eq!(stock_source.source_reader_bits.bit_start, 50);
        assert_eq!(stock_source.source_reader_bits.bit_end, 63);
        assert_eq!(stock_source.source_reader_bits.packed_msb, 0x0AE2);
        assert_eq!(stock_source.source_reader_residual.bit_start, 63);
        assert_eq!(stock_source.source_reader_residual.bit_end, 76);
        assert_eq!(stock_source.source_reader_residual.bit_count, 13);

        let continuation = evidence.terminal_reader_continuation;
        assert_eq!(
            (
                continuation.source_read_buffer_cursor,
                continuation.source_read_buffer_end,
                continuation.source_fragment_bit_cursor,
                continuation.source_fragment_bit_end,
            ),
            (245, 245, 63, 76)
        );
        assert_eq!(
            continuation.source_more_data_source,
            live_object_update::LiveObjectUpdateReaderContinuationSource::FragmentOnly
        );
        assert!(continuation.source_next_opcode_read_overflows);
        assert_eq!(
            evidence.source_fragment_ownership_verdict(),
            live_object_update::LiveObjectUpdateTerminalFragmentOwnershipVerdict::
                FragmentWriterOwnerUnproven
        );
        assert_eq!(
            (
                continuation.emitted_read_buffer_cursor,
                continuation.emitted_read_buffer_end,
                continuation.emitted_fragment_bit_cursor,
                continuation.emitted_fragment_bit_end,
            ),
            (243, 243, 71, 88),
            "emitted read-buffer cursors must come from the staged seven-byte EE tail"
        );
        assert_eq!(
            continuation.emitted_more_data_source,
            live_object_update::LiveObjectUpdateReaderContinuationSource::FragmentOnly
        );
        assert!(continuation.emitted_next_opcode_read_overflows);
        assert_eq!(
            evidence.emitted_fragment_ownership_verdict(),
            live_object_update::LiveObjectUpdateTerminalFragmentOwnershipVerdict::
                FragmentWriterOwnerUnproven
        );

        let handoff = evidence
            .terminal_fragment_handoff_correlation
            .expect("stock residual should be compared with exact prior ledger spans");
        assert_eq!(handoff.anchored_source_bit_cursor, 63);
        assert_eq!(handoff.source_fragment_bit_count, 76);
        assert_eq!(handoff.unresolved_source_bits.bit_start, 63);
        assert_eq!(handoff.unresolved_source_bits.bit_end, 76);
        assert_eq!(handoff.unresolved_source_bits.bit_count, 13);
        assert_eq!(
            handoff.ambiguity_count,
            handoff.candidate_count.saturating_sub(1)
        );
        assert_eq!(
            handoff.candidates_retained,
            handoff.candidate_count.min(
                live_object_update::LIVE_OBJECT_UPDATE_TERMINAL_FRAGMENT_REPLAY_CANDIDATE_LIMIT
            )
        );
        let replay = handoff
            .candidates
            .iter()
            .flatten()
            .find(|candidate| {
                candidate.prior_source_bit_start == 40
                    && candidate.prior_source_bit_end == 50
                    && candidate.replayed_source_bits.bit_start == 65
                    && candidate.replayed_source_bits.bit_end == 75
            })
            .expect("terminal residual should retain the preceding A/09 source-span replay");
        assert_eq!((replay.prior_offset, replay.prior_record_end), (125, 182));
        assert_eq!((replay.prior_opcode, replay.prior_marker), (b'A', 9));
        assert_eq!(replay.prior_object_id, Some(0x8000_1003));
        assert_eq!(replay.focus_offset, 182);
        assert_eq!(replay.focus_object_id, Some(0x8000_1003));
        assert!(replay.same_object);
        assert!(replay.immediately_precedes_focus);
        assert_eq!(replay.prior_source_bit_count, 10);
        assert_eq!(
            (
                replay.unresolved_prefix.bit_start,
                replay.unresolved_prefix.bit_end,
                replay.unresolved_prefix.bit_count,
            ),
            (63, 65, 2)
        );
        assert_eq!(replay.replayed_source_bits.bit_count, 10);
        assert_eq!(
            (
                replay.unresolved_suffix.bit_start,
                replay.unresolved_suffix.bit_end,
                replay.unresolved_suffix.bit_count,
            ),
            (75, 76, 1)
        );
        assert_eq!(
            replay.replayed_source_bits.bits[..10],
            preceding_add.source_bits.bits[..10],
            "the correlation must compare the complete immutable ledger row"
        );
        let semantic_replay = replay
            .direct_name_placeable_add_replay
            .expect("exact same-object A/09 replay should retain its decompile-backed semantics");
        assert_eq!(
            (
                semantic_replay.prior_emitted_bit_start,
                semantic_replay.prior_emitted_bit_end,
                semantic_replay.prior_emitted_bit_count,
            ),
            (46, 57, 11)
        );
        assert_eq!(semantic_replay.prior_bits_inserted, 1);
        assert_eq!(semantic_replay.prior_bits_removed, 0);
        assert_eq!(semantic_replay.source_name_selector_bit_cursor, 40);
        assert_eq!(semantic_replay.emitted_name_selector_bit_cursor, 46);
        assert_eq!(semantic_replay.emitted_post_name_bit_cursor, 47);
        assert_eq!(semantic_replay.emitted_next_bit_cursor, 57);
        assert_eq!(
            semantic_replay.emitted_bits.bits[..11],
            preceding_add.emitted_bits.bits[..11],
            "semantic classification must validate the exact emitted EE add span"
        );

        assert_eq!(evidence.end_aligned_diamond_reader_candidate_count, 1);
        let mut end_aligned_candidates = evidence
            .end_aligned_diamond_reader_candidates
            .iter()
            .flatten();
        let end_aligned = end_aligned_candidates
            .next()
            .expect("the stock-reader residual should itself be an exact terminal reader walk");
        assert_eq!(end_aligned.raw_mask, 0xFFFF_FFF7);
        assert_eq!(end_aligned.effective_mask, 0x0008_0037);
        assert_eq!(end_aligned.ignored_mask, 0xFFF7_FFC0);
        assert_eq!(end_aligned.read_end, 245);
        assert_eq!(end_aligned.source_bit_cursor, 63);
        assert_eq!(end_aligned.source_reader_bit_cursor, 76);
        assert_eq!(end_aligned.source_reader_bits_consumed, 13);
        assert_eq!(end_aligned.source_orientation_vector, Some(false));
        assert_eq!(end_aligned.source_state_bit_cursor, Some(70));
        assert_eq!(end_aligned.source_name_selector_bit_cursor, Some(75));
        assert_eq!(end_aligned.source_name_selector, Some(false));
        assert_eq!(end_aligned.source_name_locstring_selector_bit_cursor, None);
        assert_eq!(end_aligned.source_name_locstring_selector, None);
        assert_eq!(end_aligned.source_name_kind, Some("direct-cexostring"));
        assert_eq!(
            (
                end_aligned.source_gap_from_ledger_cursor.bit_start,
                end_aligned.source_gap_from_ledger_cursor.bit_end,
                end_aligned.source_gap_from_ledger_cursor.bit_count,
            ),
            (50, 63, 13)
        );
        let anchored_gap = end_aligned
            .source_gap_from_anchored_reader
            .expect("end-aligned candidate should start exactly at the anchored reader end");
        assert_eq!(
            (
                anchored_gap.bit_start,
                anchored_gap.bit_end,
                anchored_gap.bit_count,
                anchored_gap.bits_retained,
            ),
            (63, 63, 0, 0)
        );
        assert_eq!(
            (
                end_aligned.source_bits.bit_start,
                end_aligned.source_bits.bit_end,
                end_aligned.source_bits.bit_count,
                end_aligned.source_bits.bits_retained,
            ),
            (63, 76, 13, 13)
        );
        assert_eq!(
            end_aligned.source_bits.bits[..13],
            [
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(true),
                Some(false),
            ]
        );
        assert!(end_aligned_candidates.next().is_none());

        let reused = evidence
            .reused_record_reader_interpretation()
            .expect("contiguous equal-topology walk should retain reused-record evidence");
        assert_eq!(reused.candidate_index, 0);
        assert_eq!(reused.record_end, 245);
        assert_eq!(
            (reused.read_buffer_cursor, reused.read_buffer_end),
            (245, 245)
        );
        assert_eq!(reused.required_second_row_header_bytes, 10);
        assert_eq!(reused.available_second_row_header_bytes, 0);
        assert_eq!(
            (
                reused.stock_fragment_bit_start,
                reused.stock_fragment_bit_end,
                reused.candidate_fragment_bit_start,
                reused.candidate_fragment_bit_end,
            ),
            (50, 63, 63, 76)
        );
        assert_eq!(reused.fragment_gap_bits, 0);
        assert_eq!(reused.reader_shape_bits, 13);
        assert!(!reused.second_stock_row_dispatch_possible);

        let mut gapped_interpretation = evidence;
        gapped_interpretation.source_fragment_bit_count += 1;
        gapped_interpretation
            .terminal_reader_continuation
            .source_fragment_bit_end += 1;
        let gapped_candidate = gapped_interpretation.end_aligned_diamond_reader_candidates[0]
            .as_mut()
            .expect("fixture should retain its first end-aligned candidate");
        gapped_candidate.source_bit_cursor += 1;
        gapped_candidate.source_reader_bit_cursor += 1;
        gapped_candidate.source_state_bit_cursor = gapped_candidate
            .source_state_bit_cursor
            .map(|cursor| cursor + 1);
        gapped_candidate.source_name_selector_bit_cursor = gapped_candidate
            .source_name_selector_bit_cursor
            .map(|cursor| cursor + 1);
        gapped_candidate.source_name_locstring_selector_bit_cursor = gapped_candidate
            .source_name_locstring_selector_bit_cursor
            .map(|cursor| cursor + 1);
        let gapped_span = gapped_candidate
            .source_gap_from_anchored_reader
            .as_mut()
            .expect("fixture candidate should retain its anchored gap");
        gapped_span.bit_end += 1;
        gapped_span.bit_count = 1;
        assert!(
            gapped_interpretation
                .reused_record_reader_interpretation()
                .is_none(),
            "even a one-bit gap must reject reused-record classification"
        );

        let mut nonterminal_read_buffer = evidence;
        nonterminal_read_buffer
            .terminal_reader_continuation
            .source_read_buffer_end += 1;
        nonterminal_read_buffer
            .terminal_reader_continuation
            .source_more_data_source =
            live_object_update::LiveObjectUpdateReaderContinuationSource::ReadBufferAndFragment;
        nonterminal_read_buffer
            .terminal_reader_continuation
            .source_next_opcode_read_overflows = false;
        assert!(
            nonterminal_read_buffer
                .reused_record_reader_interpretation()
                .is_none(),
            "remaining read bytes must use ordinary row dispatch instead of reused-record evidence"
        );

        assert_eq!(evidence.source_suffix_candidate_count, 2);
        let mut suffix_candidates = evidence.source_suffix_candidates.iter().flatten();
        let locstring_candidate = suffix_candidates
            .next()
            .expect("nine-bit locstring-selected terminal suffix");
        assert_eq!(locstring_candidate.source_bit_cursor, 67);
        assert_eq!(locstring_candidate.source_reader_bit_cursor, 76);
        assert_eq!(locstring_candidate.source_reader_bits_consumed, 9);
        assert_eq!(locstring_candidate.source_name_selector, Some(true));
        assert_eq!(
            locstring_candidate.source_name_locstring_selector,
            Some(false)
        );
        assert_eq!(
            locstring_candidate.source_name_kind,
            Some("locstring-inline-cexostring")
        );
        assert_eq!(locstring_candidate.source_bits.bit_count, 9);
        assert_eq!(
            (
                locstring_candidate
                    .source_gap_from_selected_reader
                    .bit_start,
                locstring_candidate.source_gap_from_selected_reader.bit_end,
                locstring_candidate
                    .source_gap_from_selected_reader
                    .bit_count,
            ),
            (59, 67, 8)
        );
        assert_eq!(
            locstring_candidate.source_bits.bits[..9],
            [
                Some(false),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(true),
                Some(false),
            ]
        );
        let direct_candidate = suffix_candidates
            .next()
            .expect("eight-bit direct-name terminal suffix");
        assert_eq!(direct_candidate.source_bit_cursor, 68);
        assert_eq!(direct_candidate.source_reader_bit_cursor, 76);
        assert_eq!(direct_candidate.source_reader_bits_consumed, 8);
        assert_eq!(direct_candidate.source_name_selector, Some(false));
        assert_eq!(direct_candidate.source_name_kind, Some("direct-cexostring"));
        assert_eq!(
            (
                direct_candidate.source_gap_from_selected_reader.bit_start,
                direct_candidate.source_gap_from_selected_reader.bit_end,
                direct_candidate.source_gap_from_selected_reader.bit_count,
            ),
            (59, 68, 9)
        );
        assert!(suffix_candidates.next().is_none());
        assert_eq!(
            payload, original,
            "failed typed rewrite must be transactional"
        );

        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "unowned terminal bits must keep the strict dispatcher quarantined"
        );
        assert_eq!(
            payload, original,
            "exact orchestration must not commit a partial alternating rewrite"
        );
    }

    #[test]
    fn alternating_tail9_stock_field_map_includes_localized_name_selector() {
        let mut payload = alternating_legacy_door_placeable_payload();
        let declared = usize::try_from(u32::from_le_bytes(
            payload[3..7].try_into().expect("declared length bytes"),
        ))
        .expect("declared length");
        let outer_name_selector = 62usize;
        let inner_name_selector = 63usize;
        payload[declared + outer_name_selector / 8] |= 0x80u8 >> (outer_name_selector % 8);
        payload[declared + inner_name_selector / 8] &= !(0x80u8 >> (inner_name_selector % 8));
        let original = payload.clone();

        let attempt = rewrite_payload_if_needed_with_area_context_attempt(&mut payload, None);
        assert!(attempt.summary.is_none());
        let failure = attempt
            .failure
            .expect("localized stock walk should remain a typed terminal failure");
        let evidence = failure
            .terminal_door_placeable_tail9_residual
            .expect("localized stock walk should retain typed residual evidence");
        let stock = evidence
            .stock_diamond_source
            .expect("localized stock walk should retain exact Diamond evidence");
        assert_eq!(stock.source_reader_bit_cursor, 64);
        assert_eq!(stock.source_name_selector, Some(true));
        assert_eq!(stock.source_name_locstring_selector, Some(false));
        assert_eq!(stock.source_name_kind, Some("locstring-inline-cexostring"));
        assert_eq!(stock.source_reader_bits.bit_start, 50);
        assert_eq!(stock.source_reader_bits.bit_end, 64);
        assert!(
            evidence.reused_record_reader_interpretation().is_none(),
            "localized selector width changes must not match the anchored reader topology"
        );
        let capture = live_object_update::format_live_object_update_terminal_tail9_handoff_capture(
            "localized-stock-field-map",
            &payload,
            failure,
        )
        .expect("localized typed failure should format bounded field evidence");
        assert!(capture.lines().any(|line| {
            line.starts_with(
                "stock_diamond_fragment_field\t9\tkind\tname-locstring-selector\tdialect\tdiamond",
            ) && line.contains("source\t63..64:0\tprobe_cursor\t63..64")
                && line.ends_with("claimable\tfalse\trewrite_authorized\tfalse")
        }));
        assert_eq!(
            payload, original,
            "diagnostic field mapping must be transactional"
        );
    }

    #[test]
    fn alternating_tail9_diagnostics_require_an_exact_end_aligned_diamond_byte_walk() {
        let mut payload = alternating_legacy_door_placeable_payload();
        let terminal_update_offset = payload
            .windows(2)
            .rposition(|window| window == [b'U', 9])
            .expect("fixture should end with a U/09 record");
        // The compact tail9 reader treats these bytes as the facing high byte
        // and an ignored state byte. The Diamond generic reader treats them as
        // its appearance WORD. 0xFFFE selects a mandatory 16-byte resref branch,
        // making the byte walk inexact without disturbing the compact terminal
        // evidence used to diagnose the residual fragment bits.
        payload[terminal_update_offset + 17..terminal_update_offset + 19]
            .copy_from_slice(&0xFFFEu16.to_le_bytes());
        let original = payload.clone();

        let attempt = rewrite_payload_if_needed_with_area_context_attempt(&mut payload, None);
        assert!(attempt.summary.is_none());
        let failure = attempt
            .failure
            .expect("compact terminal walk should still retain residual evidence");
        assert_eq!(
            failure.kind,
            live_object_update::LiveObjectUpdateRewriteFailureKind::
                DoorPlaceableTail9TerminalResidualFragmentBits
        );
        let evidence = failure
            .terminal_door_placeable_tail9_residual
            .expect("tail9 residual evidence should survive the byte-shape discriminator");
        assert!(evidence.stock_diamond_source.is_none());
        assert_eq!(evidence.end_aligned_diamond_reader_candidate_count, 0);
        assert!(
            evidence
                .end_aligned_diamond_reader_candidates
                .iter()
                .all(Option::is_none)
        );
        assert!(
            evidence.reused_record_reader_interpretation().is_none(),
            "a reused-record interpretation requires an exact anchored stock walk"
        );
        assert_eq!(
            payload, original,
            "diagnostic candidate rejection must not commit a staged rewrite"
        );
        assert!(rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none());
        assert_eq!(
            payload, original,
            "strict orchestration must preserve the rejected terminal stream"
        );
    }

    #[test]
    fn alternating_tail9_terminal_capture_is_bounded_and_non_claiming() {
        assert!(
            std::mem::size_of::<live_object_update::LiveObjectUpdatePackedFragmentBitSpanEvidence>(
            ) <= 24,
            "packed reader evidence must not grow into per-field bit arrays"
        );
        assert!(
            std::mem::size_of::<
                live_object_update::LiveObjectUpdateDoorPlaceableTail9ResidualEvidence,
            >() <= 5_376,
            "terminal residual evidence is copied through retry paths and must remain stack-bounded"
        );
        assert!(
            std::mem::size_of::<live_object_update::LiveObjectUpdateRewriteFailure>() <= 15_872,
            "the complete retry-path failure value must remain stack-bounded"
        );
        assert!(
            std::mem::size_of::<live_object_update::LiveObjectUpdateTerminalWriterHandoffRequirement>(
            ) <= 256,
            "writer handoff requirements must remain compact in copied failure evidence"
        );
        assert!(
            std::mem::size_of::<live_object_update::LiveObjectUpdateDoorPlaceableStockSourceEvidence>(
            ) <= 256,
            "terminal evidence is copied through retry paths and must remain stack-bounded"
        );
        assert!(
            std::mem::size_of::<
                live_object_update::LiveObjectUpdateTerminalReusedRecordReaderInterpretationEvidence,
            >() <= 128,
            "one reused-record interpretation must remain compact"
        );
        let mut payload = alternating_legacy_door_placeable_payload();
        let attempt = rewrite_payload_if_needed_with_area_context_attempt(&mut payload, None);
        assert!(attempt.summary.is_none());
        let failure = attempt
            .failure
            .expect("alternating terminal stream should retain typed failure evidence");
        let evidence = failure
            .terminal_door_placeable_tail9_residual
            .expect("alternating terminal stream should retain typed residual evidence");
        let writer_requirement = evidence
            .writer_handoff_requirement()
            .expect("all 13 source handoff bits should fit the bounded exact join contract");
        assert_eq!(writer_requirement.object_type, 0x09);
        assert_eq!(writer_requirement.object_id, 0x8000_1003);
        assert_eq!(writer_requirement.raw_mask, 0xFFFF_FFF7);
        assert_eq!(
            (
                writer_requirement.source_read_buffer_cursor,
                writer_requirement.source_read_buffer_end,
            ),
            (245, 245)
        );
        assert_eq!(
            (
                writer_requirement.source_fragment_bits.bit_start,
                writer_requirement.source_fragment_bits.bit_end,
                writer_requirement.source_fragment_bits.bit_count,
                writer_requirement.source_fragment_bits.bits_retained,
            ),
            (63, 76, 13, 13)
        );
        assert!(writer_requirement.source_next_opcode_read_overflows);
        assert_eq!(
            (
                writer_requirement.emitted_read_buffer_cursor,
                writer_requirement.emitted_read_buffer_end,
                writer_requirement.emitted_fragment_bit_start,
                writer_requirement.emitted_fragment_bit_end,
                writer_requirement.emitted_fragment_bit_count,
                writer_requirement.emitted_fragment_bits_retained,
            ),
            (243, 243, 71, 88, 17, 17),
            "the emitted side must retain the complete compact final-claim obligation"
        );
        assert_eq!(
            writer_requirement.emitted_fragment_bits,
            evidence
                .rewritten_residual_exact
                .expect("writer requirement must carry exact emitted values")
        );
        assert_eq!(writer_requirement.emitted_fragment_bits.packed_msb, 0x4046);
        assert!(writer_requirement.emitted_next_opcode_read_overflows);
        let mut nonterminal_read_buffer = evidence;
        nonterminal_read_buffer
            .terminal_reader_continuation
            .source_read_buffer_cursor = 244;
        nonterminal_read_buffer
            .terminal_reader_continuation
            .source_more_data_source =
            live_object_update::LiveObjectUpdateReaderContinuationSource::ReadBufferAndFragment;
        assert!(
            nonterminal_read_buffer
                .writer_handoff_requirement()
                .is_none(),
            "writer join requirements are terminal fragment-only contracts"
        );
        let mut incomplete_emitted_continuation = evidence;
        incomplete_emitted_continuation
            .terminal_reader_continuation
            .emitted_next_opcode_read_overflows = false;
        assert!(
            incomplete_emitted_continuation
                .writer_handoff_requirement()
                .is_none(),
            "both source and emitted terminal continuation invariants are required"
        );
        let exact_candidate_capture =
            live_object_update::format_live_object_update_terminal_tail9_handoff_capture(
                "live-object-update-records",
                &payload,
                failure,
            )
            .expect("the exact failure payload should support a bounded EE candidate audit");
        assert!(exact_candidate_capture.contains(
            "terminal_ee_writer_candidate\tstatus\texact-payload-rejected\ttyped_row_exact\ttrue\tcandidate_payload_len\t261\tcandidate_read_buffer\t243..243\ttyped_row_read_buffer\t243..243\tunconsumed_fragment\t71..88\tcandidate_fragment_end\t88\texact_payload_validator_accepted\tfalse\treject_stage\tfragment-cursor\treject_read_buffer\t243..243\treject_fragment_cursor\t71\tclaimable\tfalse\trewrite_authorized\tfalse\tcursor_advance_authorized\tfalse\tfragment_trim_authorized\tfalse"
        ));
        let mut capture_payload = payload.clone();
        // `failure.offset` belongs to the staged mutable read buffer after
        // earlier rows grew. Mutate the immutable source header bound by the
        // writer requirement so the diagnostic rerun must reject a genuinely
        // different packet rather than unrelated bytes at the staged offset.
        let capture_record_offset = 7usize.saturating_add(writer_requirement.source_record_offset);
        capture_payload[capture_record_offset + 1] = 0x0A;
        capture_payload[capture_record_offset + 2..capture_record_offset + 6]
            .copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let capture = live_object_update::format_live_object_update_terminal_tail9_handoff_capture(
            "live-object-update-records",
            &capture_payload,
            failure,
        )
        .expect("terminal tail9 failure should emit a machine-readable artifact");

        assert!(capture.starts_with("capture\tlive-object-terminal-tail9-handoff\tversion\t11\n"));
        let payload_md5_hint = format!("{:x}", md5::compute(&capture_payload));
        assert!(capture.contains(&format!("payload_md5_hint\t{payload_md5_hint}")));
        assert!(capture.contains(
            "ownership\tstatus\tunproven-source-owner\tsource_fragment_ownership\tfragment-writer-owner-unproven\temitted_fragment_ownership\tfragment-writer-owner-unproven\tclaimable\tfalse\trewrite_authorized\tfalse\tcursor_advance_authorized\tfalse\tfragment_trim_authorized\tfalse\trequired_proof\tsource-writer-or-list-handoff"
        ));
        assert!(capture.contains(
            "writer_handoff_requirement\tobject_type\t0x09\tsource_record_offset\t166\tobject_id\t0x80001003\traw_mask\t0xFFFFFFF7\temitted_record_offset\t182\temitted_mask\t0x00080017\tsource_read_buffer\t245..245\tsource_fragment\t63..76:"
        ), "capture omitted exact source record binding:\n{capture}");
        assert!(capture.contains(
            "source_next_opcode_read_overflows\ttrue\temitted_read_buffer\t243..243\temitted_fragment_obligation\t71..88\temitted_fragment_bits\t17\temitted_fragment_bits_retained\t17\temitted_fragment_exact\t71..88:00100000001000110"
        ));
        assert!(capture.contains(
            "emitted_fragment_values_complete\ttrue\temitted_next_opcode_read_overflows\ttrue"
        ));
        assert!(capture.contains(
            "packet_correlation_required\texact-payload-bytes\tfinal_ee_claim_required\ttrue\tclaimable\tfalse\trewrite_authorized\tfalse\tfragment_trim_authorized\tfalse"
        ));
        assert!(capture.contains(
            "writer_handoff_correlation\tartifact_status\tnot-configured\tverdict\tincomplete-trace\twriter_handoff_observed\tfalse\tclaimable\tfalse\ttrace_id\tnone\tmessage_id\tnone\tcomponent_sha256\tnone"
        ));
        assert!(capture.contains(
            "terminal_ee_writer_candidate\tstatus\tunavailable-bounded-rerun-mismatch\tclaimable\tfalse\trewrite_authorized\tfalse\tcursor_advance_authorized\tfalse\tfragment_trim_authorized\tfalse"
        ), "capture omitted mismatch audit row:\n{capture}");
        assert!(capture.contains(
            "ee_final_claim_readiness\tobservation\tnone\tverdict\tincomplete-typed-ee-writer\tready\tfalse\tclaimable\tfalse\trewrite_authorized\tfalse\tcursor_advance_authorized\tfalse\tfragment_trim_authorized\tfalse"
        ));
        assert!(capture.contains(
            "terminal_proof_join\tsource_handoff_token\tfalse\tee_final_claim_token\tfalse\tverdict\tincomplete-source-and-ee-proof\tready\tfalse\tclaimable\tfalse\trewrite_authorized\tfalse\tcursor_advance_authorized\tfalse\tfragment_trim_authorized\tfalse"
        ));
        assert!(capture.contains(
            "stock_diamond_reader\traw_mask\t0xFFFFFFF7\teffective_mask\t0x00080037\tignored_mask\t0xFFF7FFC0\tread_end\t245\tstart\t50\tend\t63\tconsumed\t13"
        ));
        assert!(capture.contains(
            "reader_continuation\tsource_read_buffer\t245..245\tsource_fragment\t63..76\tsource_more_data\tfragment-only\tsource_next_opcode_read_overflows\ttrue\temitted_read_buffer\t243..243\temitted_fragment\t71..88\temitted_more_data\tfragment-only\temitted_next_opcode_read_overflows\ttrue\tclaimable\tfalse"
        ));
        assert!(capture.contains(
            "reused_record_reader_summary\tcandidates\t1\tretained\t1\townership\tunknown\tclaimable\tfalse"
        ));
        assert!(capture.contains(
            "reused_record_reader_interpretation\t0\tdialect\tdiamond\trecord_end\t245\tread_buffer\t245..245\trequired_second_row_header_bytes\t10\tavailable_second_row_header_bytes\t0\tstock_fragment\t50..63\tcandidate_fragment\t63..76\tfragment_gap_bits\t0\treader_shape_bits\t13\tsame_ordered_field_topology\ttrue\tsecond_stock_row_dispatch_possible\tfalse\twriter_replay_proven\tfalse\tclaimable\tfalse\trewrite_authorized\tfalse\tfragment_trim_authorized\tfalse"
        ));
        let expected_stock_fields = [
            ("position-z-low", 50, 52),
            ("orientation-selector", 52, 53),
            ("scalar-orientation-low", 53, 57),
            ("state-visual-selector", 57, 58),
            ("state-visual-active", 58, 59),
            ("state-locked", 59, 60),
            ("state-lockable", 60, 61),
            ("state-visual-payload", 61, 62),
            ("name-selector", 62, 63),
        ];
        for (index, (kind, bit_start, bit_end)) in expected_stock_fields.into_iter().enumerate() {
            let prefix =
                format!("stock_diamond_fragment_field\t{index}\tkind\t{kind}\tdialect\tdiamond");
            let line = capture
                .lines()
                .find(|line| line.starts_with(&prefix))
                .expect("stock field map must retain every exact reader field");
            assert!(line.contains("object_type\t0x09\tobject_id\t0x80001003\tmask\t0xFFFFFFF7"));
            assert!(line.contains(&format!("source\t{bit_start}..{bit_end}:")));
            assert!(line.contains(&format!("probe_cursor\t{bit_start}..{bit_end}")));
            assert!(line.ends_with("claimable\tfalse\trewrite_authorized\tfalse"));
        }
        assert!(
            !capture.contains("object_type\t0x0A\tobject_id\t0xDEADBEEF"),
            "the formatter must use failure-time typed identity, not a later staged payload"
        );
        let expected_end_fields = [
            ("position-z-low", 63, 65),
            ("orientation-selector", 65, 66),
            ("scalar-orientation-low", 66, 70),
            ("state-visual-selector", 70, 71),
            ("state-visual-active", 71, 72),
            ("state-locked", 72, 73),
            ("state-lockable", 73, 74),
            ("state-visual-payload", 74, 75),
            ("name-selector", 75, 76),
        ];
        for (field, (kind, bit_start, bit_end)) in expected_end_fields.into_iter().enumerate() {
            let prefix = format!(
                "end_aligned_fragment_field\t0\tfield\t{field}\tkind\t{kind}\tdialect\tdiamond"
            );
            let line = capture
                .lines()
                .find(|line| line.starts_with(&prefix))
                .expect("end-aligned field map must retain every exact reader field");
            assert!(line.contains("object_type\t0x09\tobject_id\t0x80001003\tmask\t0xFFFFFFF7"));
            assert!(line.contains(&format!("source\t{bit_start}..{bit_end}:")));
            assert!(line.contains(&format!("probe_cursor\t{bit_start}..{bit_end}")));
            assert!(line.ends_with("claimable\tfalse\trewrite_authorized\tfalse"));
        }
        assert!(capture.contains(
            "terminal_handoff\tanchored_cursor\t63\tfragment_bits\t76\tunresolved\t63..76:"
        ));
        let replay = capture
            .lines()
            .find(|line| {
                line.starts_with("terminal_replay_candidate\t")
                    && line.contains("prior_source\t40..50")
                    && line.contains("replayed\t65..75:")
            })
            .expect("artifact should retain the exact preceding A/09 replay candidate");
        assert!(replay.contains("same_object\ttrue"));
        assert!(replay.contains("immediately_precedes\ttrue"));
        assert!(replay.contains("unresolved_prefix\t63..65:"));
        assert!(replay.contains("unresolved_suffix\t75..76:"));
        assert!(replay.ends_with("claimable\tfalse"));
        assert!(capture.lines().any(|line| {
            line.starts_with("terminal_semantic_replay\t")
                && line.contains("kind\tdirect-name-placeable-add")
                && line.contains("source_name_selector\t40")
                && line.contains("emitted_name_selector\t46")
                && line.contains("prior_emitted\t46..57")
                && line.contains("post_name\t47\tnext\t57")
                && line.ends_with("claimable\tfalse\trewrite_authorized\tfalse")
        }));
        assert!(capture.lines().any(|line| {
            line.starts_with("end_aligned_candidate\t")
                && line.contains("start\t63\tend\t76\tconsumed\t13")
                && line.ends_with("claimable\tfalse")
        }));
        assert!(capture.contains(
            "contiguous_tail\tentries\t5\tretained\t5\tsource_span\t3..50\temitted_span\t3..57"
        ));
        assert!(
            !capture.contains("claimable\ttrue"),
            "candidate evidence must never promote source ownership"
        );
        assert!(
            capture.lines().count()
                <= 15
                    + 1
                    + live_object_update::LIVE_OBJECT_UPDATE_END_ALIGNED_DIAMOND_READER_CANDIDATE_LIMIT
                    + live_object_update::LIVE_OBJECT_UPDATE_TERMINAL_FRAGMENT_REPLAY_CANDIDATE_LIMIT
                    + live_object_update::LIVE_OBJECT_UPDATE_TERMINAL_FRAGMENT_REPLAY_CANDIDATE_LIMIT
                    + live_object_update::LIVE_OBJECT_UPDATE_END_ALIGNED_DIAMOND_READER_CANDIDATE_LIMIT
                    + live_object_update::LIVE_OBJECT_UPDATE_TAIL9_SOURCE_CANDIDATE_LIMIT
                    + live_object_update::LIVE_OBJECT_UPDATE_DOOR_PLACEABLE_FRAGMENT_FIELD_LIMIT
                    + live_object_update::LIVE_OBJECT_UPDATE_END_ALIGNED_DIAMOND_READER_CANDIDATE_LIMIT
                        * live_object_update::LIVE_OBJECT_UPDATE_DOOR_PLACEABLE_FRAGMENT_FIELD_LIMIT
                    + live_object_update::LIVE_OBJECT_UPDATE_REWRITE_TAIL_EVIDENCE_ENTRY_LIMIT,
            "artifact row count must stay bounded by retained typed-evidence limits"
        );
    }

    #[test]
    fn alternating_tail9_handoff_correlation_rejects_a_one_bit_near_match() {
        let mut payload = alternating_legacy_door_placeable_payload();
        let declared = usize::try_from(u32::from_le_bytes(
            payload[3..7].try_into().expect("declared length bytes"),
        ))
        .expect("declared length");
        let replay_bit_cursor = 69usize;
        payload[declared + replay_bit_cursor / 8] ^= 0x80 >> (replay_bit_cursor % 8);
        let original = payload.clone();

        let attempt = rewrite_payload_if_needed_with_area_context_attempt(&mut payload, None);
        assert!(attempt.summary.is_none());
        let failure = attempt
            .failure
            .expect("one changed residual bit must remain a strict terminal failure");
        assert_eq!(
            failure.kind,
            live_object_update::LiveObjectUpdateRewriteFailureKind::
                DoorPlaceableTail9TerminalResidualFragmentBits
        );
        let evidence = failure
            .terminal_door_placeable_tail9_residual
            .expect("near-match failure should retain terminal evidence");
        assert!(evidence.stock_diamond_source.is_some());
        let handoff = evidence
            .terminal_fragment_handoff_correlation
            .expect("stock residual should still receive bounded correlation analysis");
        assert!(
            handoff.candidates.iter().flatten().all(|candidate| {
                !(candidate.prior_source_bit_start == 40
                    && candidate.prior_source_bit_end == 50
                    && candidate.replayed_source_bits.bit_start == 65
                    && candidate.replayed_source_bits.bit_end == 75)
            }),
            "a one-bit mismatch must not be reported as an exact prior-row replay"
        );
        assert!(
            evidence.reused_record_reader_interpretation().is_some(),
            "reader topology evidence must remain independent of prior-row bit equality"
        );
        let capture = live_object_update::format_live_object_update_terminal_tail9_handoff_capture(
            "live-object-update-records",
            &payload,
            failure,
        )
        .expect("near-match terminal failure should still emit bounded evidence");
        assert!(capture.contains(
            "ownership\tstatus\tunproven-source-owner\tsource_fragment_ownership\tfragment-writer-owner-unproven\temitted_fragment_ownership\tfragment-writer-owner-unproven\tclaimable\tfalse"
        ));
        assert!(
            capture
                .lines()
                .filter(|line| {
                    line.starts_with("terminal_replay_candidate\t")
                        && line.contains("prior_source\t40..50")
                        && line.contains("replayed\t65..75:")
                })
                .next()
                .is_none()
        );
        assert!(!capture.contains("claimable\ttrue"));
        assert_eq!(
            payload, original,
            "diagnostic near-match analysis must not mutate the staged packet"
        );
        assert!(rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none());
        assert_eq!(
            payload, original,
            "strict orchestration must keep the near-match packet quarantined"
        );
    }

    #[test]
    fn alternating_tail9_semantic_replay_requires_a_single_false_suffix() {
        let mut payload = alternating_legacy_door_placeable_payload();
        let declared = usize::try_from(u32::from_le_bytes(
            payload[3..7].try_into().expect("declared length bytes"),
        ))
        .expect("declared length");
        let suffix_bit_cursor = 75usize;
        payload[declared + suffix_bit_cursor / 8] ^= 0x80 >> (suffix_bit_cursor % 8);
        let original = payload.clone();

        let attempt = rewrite_payload_if_needed_with_area_context_attempt(&mut payload, None);
        assert!(attempt.summary.is_none());
        let failure = attempt
            .failure
            .expect("changed terminal suffix must remain a strict terminal failure");
        assert_eq!(
            failure.kind,
            live_object_update::LiveObjectUpdateRewriteFailureKind::
                DoorPlaceableTail9TerminalResidualFragmentBits
        );
        let evidence = failure
            .terminal_door_placeable_tail9_residual
            .expect("changed suffix should retain terminal evidence");
        let handoff = evidence
            .terminal_fragment_handoff_correlation
            .expect("stock residual should still receive bounded correlation analysis");
        let replay = handoff
            .candidates
            .iter()
            .flatten()
            .find(|candidate| {
                candidate.prior_source_bit_start == 40
                    && candidate.prior_source_bit_end == 50
                    && candidate.replayed_source_bits.bit_start == 65
                    && candidate.replayed_source_bits.bit_end == 75
            })
            .expect("changing the suffix must preserve the exact raw A/09 replay");
        assert_eq!(replay.unresolved_suffix.bit_count, 1);
        assert_eq!(replay.unresolved_suffix.bits[0], Some(true));
        assert!(
            replay.direct_name_placeable_add_replay.is_none(),
            "a non-neutral trailing bit must prevent semantic envelope classification"
        );
        assert_eq!(payload, original, "diagnostics must remain transactional");
        assert!(rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none());
        assert_eq!(
            payload, original,
            "strict orchestration must retain quarantine"
        );
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn local_diamond_seq26_gui_inventory_missing_add_opcodes_stays_quarantined() {
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

        let normalized = payload.clone();
        assert!(
            rewrite_payload_to_exact_ee_if_possible(&mut payload, None).is_none(),
            "later G I 00 rows still lack decompile-backed item-name/active-property bit proof"
        );
        assert_eq!(
            payload, normalized,
            "failed GUI item-create proof must roll back the proven first-row repair"
        );
        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "shifted local Diamond GUI inventory burst remains quarantine evidence"
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
