//! Shared M-frame quickbar materialization policy.
//!
//! Direct dispatch and buffered zlib-stream handling both rewrite
//! `GuiQuickbar_SetAllButtons`. Keep their semantic item-proof mapping and
//! normalized/simple retry order here so probes and committed rewrites cannot
//! drift.

use crate::translate::{quickbar, semantic};

#[derive(Debug, Clone, Copy)]
pub(super) enum QuickbarRewriteMode {
    Committed,
    StreamProbe,
}

pub(super) fn rewrite_payload_with_registry_if_possible(
    payload: &mut Vec<u8>,
    object_registry: Option<&semantic::ObjectRegistry>,
    mode: QuickbarRewriteMode,
) -> Option<quickbar::QuickbarRewriteSummary> {
    with_registry_materialization_context(object_registry, |materialization| {
        rewrite_payload_with_context_if_possible(payload, materialization, mode)
    })
}

pub(super) fn with_registry_materialization_context<R>(
    object_registry: Option<&semantic::ObjectRegistry>,
    f: impl FnOnce(Option<&quickbar::QuickbarMaterializationContext<'_>>) -> R,
) -> R {
    if let Some(registry) = object_registry {
        let item_object_status =
            |object_id| quickbar_materialization_status_from_registry(registry, object_id);
        let materialization = quickbar::QuickbarMaterializationContext::new_with_status_and_summary(
            &item_object_status,
            quickbar_materialization_context_summary_from_registry(registry),
        );
        f(Some(&materialization))
    } else {
        f(None)
    }
}

fn rewrite_payload_with_context_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&quickbar::QuickbarMaterializationContext<'_>>,
    mode: QuickbarRewriteMode,
) -> Option<quickbar::QuickbarRewriteSummary> {
    // A structurally valid CNW declared offset is the normal Diamond/EE
    // SetReadMessage transport shape. Try its exact 36-slot reader first. The
    // transport normalizer used to prove that same reader shape as a guard and
    // then the simple path parsed all 36 slots again, which made candidate-heavy
    // live quickbars occupy the reliable-window thread for multiple seconds.
    //
    // This is only retry ordering: the direct path still requires the exact
    // byte reader, shared MSB-first fragment cursor, typed writer, and EE
    // validator. Source-only prefixed-fragment forms (including declared zero)
    // remain structurally ineligible here and continue through normalization.
    let direct_declared = quickbar::quickbar_has_structurally_plausible_cnw_declared(payload);
    if direct_declared {
        if let Some(summary) =
            rewrite_direct_payload_with_context_if_possible(payload, materialization, mode)
        {
            return Some(summary);
        }
    }

    let normalized = match mode {
        QuickbarRewriteMode::Committed => {
            quickbar::normalize_and_rewrite_quickbar_payload_with_context_if_possible(
                payload,
                materialization,
            )
        }
        QuickbarRewriteMode::StreamProbe => {
            quickbar::normalize_and_rewrite_quickbar_payload_with_context_for_stream_probe_if_possible(
                payload,
                materialization,
            )
        }
    };
    if let Some((_, summary)) = normalized {
        return Some(summary);
    }

    if direct_declared {
        return None;
    }

    rewrite_direct_payload_with_context_if_possible(payload, materialization, mode)
}

fn rewrite_direct_payload_with_context_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&quickbar::QuickbarMaterializationContext<'_>>,
    mode: QuickbarRewriteMode,
) -> Option<quickbar::QuickbarRewriteSummary> {
    match mode {
        QuickbarRewriteMode::Committed => {
            quickbar::rewrite_simple_quickbar_payload_with_context_if_possible(
                payload,
                materialization,
            )
        }
        QuickbarRewriteMode::StreamProbe => {
            quickbar::rewrite_simple_quickbar_payload_with_context_for_stream_probe_if_possible(
                payload,
                materialization,
            )
        }
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn valid_declared_quickbar_takes_direct_exact_reader_path() {
        let mut payload = include_bytes!(
            "../../../fixtures/quickbar/starcore5_live_20260510_set_all_buttons.bin"
        )
        .to_vec();
        assert!(quickbar::quickbar_has_structurally_plausible_cnw_declared(
            &payload
        ));

        let summary = rewrite_payload_with_context_if_possible(
            &mut payload,
            None,
            QuickbarRewriteMode::StreamProbe,
        )
        .expect("valid declared live quickbar should use the exact direct reader");

        assert_eq!(summary.slot_records_owned, 36);
        assert!(summary.validated_slot_profile.is_some());
        assert!(quickbar::ee_set_all_buttons_payload_shape_valid(&payload));
    }

    #[test]
    fn zero_declared_quickbar_still_uses_transport_normalization() {
        let mut payload = include_bytes!(
            "../../../fixtures/quickbar/local_diamond_bw167demo_zero_declared_seq16_set_all_buttons.bin"
        )
        .to_vec();
        assert!(!quickbar::quickbar_has_structurally_plausible_cnw_declared(
            &payload
        ));

        let summary = rewrite_payload_with_context_if_possible(
            &mut payload,
            None,
            QuickbarRewriteMode::StreamProbe,
        )
        .expect("zero-declared source form should remain owned by normalization");

        assert_eq!(summary.slot_records_owned, 36);
        assert!(summary.validated_slot_profile.is_some());
        assert!(quickbar::ee_set_all_buttons_payload_shape_valid(&payload));
    }
}

fn quickbar_materialization_status_from_registry(
    registry: &semantic::ObjectRegistry,
    object_id: u32,
) -> quickbar::QuickbarItemMaterializationStatus {
    match registry.inventory_item_object_status(object_id) {
        semantic::InventoryItemObjectStatus::Proven(proof) => {
            quickbar::QuickbarItemMaterializationStatus::Proven(
                quickbar_materialization_proof_from_registry(proof),
            )
        }
        semantic::InventoryItemObjectStatus::DeferredFeature25(proof) => {
            quickbar::QuickbarItemMaterializationStatus::DeferredFeature25(
                quickbar_materialization_proof_from_registry(proof),
            )
        }
        semantic::InventoryItemObjectStatus::ClearedByItemDelete => {
            quickbar::QuickbarItemMaterializationStatus::ClearedByItemDelete
        }
        semantic::InventoryItemObjectStatus::ClearedByAreaReset => {
            quickbar::QuickbarItemMaterializationStatus::ClearedByAreaReset
        }
        semantic::InventoryItemObjectStatus::Unknown => {
            quickbar::QuickbarItemMaterializationStatus::Unknown
        }
    }
}

fn quickbar_materialization_proof_from_registry(
    proof: semantic::InventoryItemObjectProof,
) -> quickbar::QuickbarItemMaterializationProof {
    match proof {
        semantic::InventoryItemObjectProof::ActiveObject => {
            quickbar::QuickbarItemMaterializationProof::ActiveObject
        }
        semantic::InventoryItemObjectProof::Feature25FirstList => {
            quickbar::QuickbarItemMaterializationProof::InventoryFeature25FirstList
        }
        semantic::InventoryItemObjectProof::Feature25SecondList => {
            quickbar::QuickbarItemMaterializationProof::InventoryFeature25SecondList
        }
        semantic::InventoryItemObjectProof::Feature25LegacyTail => {
            quickbar::QuickbarItemMaterializationProof::InventoryFeature25LegacyTail
        }
    }
}

fn quickbar_materialization_context_summary_from_registry(
    registry: &semantic::ObjectRegistry,
) -> quickbar::QuickbarMaterializationContextSummary {
    let context = registry.inventory_item_context_summary();
    quickbar::QuickbarMaterializationContextSummary {
        active_item_objects: context.active_item_objects,
        materialized_item_objects: context.materialized_item_objects,
        direct_item_proof_objects: context.direct_item_proof_objects,
        feature25_item_proof_objects: context.feature25_item_proof_objects,
        compact_item_emission_proof_objects: context.compact_item_emission_proof_objects,
        compact_item_emission_ready_objects: context.compact_item_emission_ready_objects,
        compact_item_emission_direct_only_proof_objects: context
            .compact_item_emission_direct_only_proof_objects,
        compact_item_emission_feature25_only_proof_objects: context
            .compact_item_emission_feature25_only_proof_objects,
        compact_item_emission_shared_proof_objects: context
            .compact_item_emission_shared_proof_objects,
        compact_item_emission_deferred_feature25_only_objects: context
            .compact_item_emission_deferred_feature25_only_objects,
        inventory_feature25_first_item_refs: context.inventory_feature25_first_item_refs,
        inventory_feature25_second_item_refs: context.inventory_feature25_second_item_refs,
        inventory_feature25_legacy_tail_item_refs: context
            .inventory_feature25_legacy_tail_item_refs,
        cleared_inventory_item_object_ids: context.cleared_inventory_item_object_ids,
        inventory_feature25_reference_records: context.inventory_feature25_reference_records,
        inventory_feature25_first_item_ref_mentions: context
            .inventory_feature25_first_item_ref_mentions,
        inventory_feature25_second_item_ref_mentions: context
            .inventory_feature25_second_item_ref_mentions,
        inventory_feature25_legacy_tail_item_ref_mentions: context
            .inventory_feature25_legacy_tail_item_ref_mentions,
        inventory_feature25_first_materialized_item_ref_mentions: context
            .inventory_feature25_first_materialized_item_ref_mentions,
        inventory_feature25_first_deferred_item_ref_mentions: context
            .inventory_feature25_first_deferred_item_ref_mentions,
        inventory_feature25_second_materialized_item_ref_mentions: context
            .inventory_feature25_second_materialized_item_ref_mentions,
        inventory_feature25_second_deferred_item_ref_mentions: context
            .inventory_feature25_second_deferred_item_ref_mentions,
        inventory_feature25_legacy_tail_materialized_item_ref_mentions: context
            .inventory_feature25_legacy_tail_materialized_item_ref_mentions,
        inventory_feature25_legacy_tail_deferred_item_ref_mentions: context
            .inventory_feature25_legacy_tail_deferred_item_ref_mentions,
    }
}
