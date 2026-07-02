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

impl QuickbarRewriteMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::StreamProbe => "stream_probe",
        }
    }

    fn is_committed(self) -> bool {
        matches!(self, Self::Committed)
    }
}

pub(super) fn rewrite_payload_with_registry_if_possible(
    payload: &mut Vec<u8>,
    object_registry: Option<&semantic::ObjectRegistry>,
    mode: QuickbarRewriteMode,
) -> Option<quickbar::QuickbarRewriteSummary> {
    if let Some(registry) = object_registry {
        let item_object_status =
            |object_id| quickbar_materialization_status_from_registry(registry, object_id);
        let materialization =
            quickbar::QuickbarMaterializationContext::new_with_status(&item_object_status);
        let result =
            rewrite_payload_with_context_if_possible(payload, Some(&materialization), mode);
        if let Some(summary) = result.as_ref() {
            trace_quickbar_registry_materialization_context(registry, mode, summary);
        }
        return result;
    }
    rewrite_payload_with_context_if_possible(payload, None, mode)
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

fn trace_quickbar_registry_materialization_context(
    registry: &semantic::ObjectRegistry,
    mode: QuickbarRewriteMode,
    summary: &quickbar::QuickbarRewriteSummary,
) {
    let context = registry.inventory_item_context_summary();
    tracing::info!(
        trace_role = mode.as_str(),
        committed = mode.is_committed(),
        item_buttons_seen = summary.item_buttons_seen,
        item_buttons_preserved = summary.item_buttons_preserved,
        active_item_objects = context.active_item_objects,
        materialized_item_objects = context.materialized_item_objects,
        inventory_feature25_first_item_refs = context.inventory_feature25_first_item_refs,
        inventory_feature25_second_item_refs = context.inventory_feature25_second_item_refs,
        inventory_feature25_legacy_tail_item_refs =
            context.inventory_feature25_legacy_tail_item_refs,
        cleared_inventory_item_object_ids = context.cleared_inventory_item_object_ids,
        inventory_feature25_reference_records = context.inventory_feature25_reference_records,
        inventory_feature25_first_item_ref_mentions =
            context.inventory_feature25_first_item_ref_mentions,
        inventory_feature25_second_item_ref_mentions =
            context.inventory_feature25_second_item_ref_mentions,
        inventory_feature25_legacy_tail_item_ref_mentions =
            context.inventory_feature25_legacy_tail_item_ref_mentions,
        inventory_feature25_first_materialized_item_ref_mentions =
            context.inventory_feature25_first_materialized_item_ref_mentions,
        inventory_feature25_first_deferred_item_ref_mentions =
            context.inventory_feature25_first_deferred_item_ref_mentions,
        inventory_feature25_second_materialized_item_ref_mentions =
            context.inventory_feature25_second_materialized_item_ref_mentions,
        inventory_feature25_second_deferred_item_ref_mentions =
            context.inventory_feature25_second_deferred_item_ref_mentions,
        inventory_feature25_legacy_tail_materialized_item_ref_mentions =
            context.inventory_feature25_legacy_tail_materialized_item_ref_mentions,
        inventory_feature25_legacy_tail_deferred_item_ref_mentions =
            context.inventory_feature25_legacy_tail_deferred_item_ref_mentions,
        "server GuiQuickbar_SetAllButtons registry materialization context"
    );
}
