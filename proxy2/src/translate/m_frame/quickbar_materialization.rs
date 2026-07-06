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
        compact_item_emission_direct_only_proof_objects: context
            .compact_item_emission_direct_only_proof_objects,
        compact_item_emission_feature25_only_proof_objects: context
            .compact_item_emission_feature25_only_proof_objects,
        compact_item_emission_shared_proof_objects: context
            .compact_item_emission_shared_proof_objects,
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
