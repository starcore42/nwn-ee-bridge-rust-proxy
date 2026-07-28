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
    // Live-object and GUI lifecycle evidence can legitimately retain the raw
    // Diamond id, so preserve any exact status first. Only a genuinely unknown
    // raw id may fall back to the external id that the quickbar writer emits
    // and EE registers. In particular, a raw tombstone or deferred/reference
    // status must remain fail-closed rather than being hidden by high-id proof.
    let raw_status = registry.inventory_item_object_status(object_id);
    let status = if raw_status == semantic::InventoryItemObjectStatus::Unknown {
        let ee_object_id = quickbar::ee_quickbar_object_id_wire_value(object_id);
        registry.inventory_item_object_status(ee_object_id)
    } else {
        raw_status
    };
    quickbar_materialization_status_from_inventory_status(status)
}

fn quickbar_materialization_status_from_inventory_status(
    status: semantic::InventoryItemObjectStatus,
) -> quickbar::QuickbarItemMaterializationStatus {
    match status {
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
        semantic::InventoryItemObjectStatus::UnprovenFeature25Reference(_) => {
            // Feature-25 OBJECTIDs are visibility-node/source references, not
            // item materialization. Keep the writer on the same fail-closed
            // unknown path until typed item or GUI materialization exists.
            quickbar::QuickbarItemMaterializationStatus::Unknown
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

#[cfg(test)]
mod fixture_free_tests {
    use super::*;

    #[test]
    fn registry_proof_lookup_preserves_raw_low_id_proof() {
        let compact_object_id = 0x0000_0042;
        let mut registry = semantic::ObjectRegistry::default();
        registry.observe_materialized_item_object_ids(&[compact_object_id]);

        assert_eq!(
            quickbar_materialization_status_from_registry(&registry, compact_object_id),
            quickbar::QuickbarItemMaterializationStatus::Proven(
                quickbar::QuickbarItemMaterializationProof::ActiveObject,
            ),
            "raw low-id GUI/item lifecycle proof remains authoritative"
        );
    }

    #[test]
    fn registry_proof_lookup_falls_back_to_the_ee_quickbar_object_id_namespace() {
        let compact_object_id = 0x0000_0042;
        let ee_object_id = 0x8000_0042;
        let mut registry = semantic::ObjectRegistry::default();
        registry.observe_materialized_item_object_ids(&[ee_object_id]);

        assert_eq!(
            quickbar_materialization_status_from_registry(&registry, compact_object_id),
            quickbar::QuickbarItemMaterializationStatus::Proven(
                quickbar::QuickbarItemMaterializationProof::ActiveObject,
            ),
            "a compact Diamond source id must reuse the object already registered by the EE quickbar writer"
        );
        assert_eq!(
            quickbar_materialization_status_from_registry(&registry, ee_object_id),
            quickbar::QuickbarItemMaterializationStatus::Proven(
                quickbar::QuickbarItemMaterializationProof::ActiveObject,
            )
        );
        assert_eq!(
            quickbar_materialization_status_from_registry(
                &registry,
                quickbar::ee_quickbar_object_id_wire_value(0x7F00_0000),
            ),
            quickbar::QuickbarItemMaterializationStatus::Unknown,
            "the stock invalid sentinel must not alias a materialized external object"
        );
    }

    #[test]
    fn registry_proof_lookup_does_not_bypass_raw_tombstones_or_deferred_status() {
        let compact_object_id = 0x0000_0042;
        let ee_object_id = 0x8000_0042;
        let mut registry = semantic::ObjectRegistry::default();
        registry.observe_materialized_item_object_ids(&[compact_object_id, ee_object_id]);
        registry.observe_mentions(&[semantic::LiveObjectMention {
            opcode: b'D',
            object_type: 0x06,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert_eq!(
            quickbar_materialization_status_from_registry(&registry, compact_object_id),
            quickbar::QuickbarItemMaterializationStatus::ClearedByItemDelete,
            "raw item deletion must not fall through to surviving canonical-id proof"
        );
        assert_eq!(
            quickbar_materialization_status_from_inventory_status(
                semantic::InventoryItemObjectStatus::DeferredFeature25(
                    semantic::InventoryItemObjectProof::Feature25FirstList,
                ),
            ),
            quickbar::QuickbarItemMaterializationStatus::DeferredFeature25(
                quickbar::QuickbarItemMaterializationProof::InventoryFeature25FirstList,
            ),
            "deferred Feature-25 state must remain deferred and therefore fail closed"
        );
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
