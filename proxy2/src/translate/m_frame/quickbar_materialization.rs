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
    if let Some(registry) = object_registry {
        let item_object_status =
            |object_id| quickbar_materialization_status_from_registry(registry, object_id);
        let materialization =
            quickbar::QuickbarMaterializationContext::new_with_status(&item_object_status);
        return rewrite_payload_with_context_if_possible(payload, Some(&materialization), mode);
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
