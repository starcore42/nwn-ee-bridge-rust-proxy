//! M-frame-local adapters for live-object payload rewrites.
//!
//! This module intentionally does not own direct `M` frames. Direct
//! `GameObjUpdate_LiveObject` packets must route through
//! `m_frame::server_dispatch`'s semantic registry so mixed add/update payloads
//! are claimed only after the focused add-record, update-record, fragment-bit,
//! and exact validator passes all agree.

use crate::translate::{area, live_object, live_object_update};

pub type RewriteSummary = live_object_update::LiveObjectUpdateRewriteSummary;
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
}

impl ExactLiveObjectRewriteSummary {
    fn record_update(&mut self, changed: bool) {
        if changed {
            self.update_passes_changed = self.update_passes_changed.saturating_add(1);
        }
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

pub fn rewrite_payload_if_needed(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    live_object_update::rewrite_update_records_payload_if_possible(payload)
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

    summary.record_update(
        promote_work_remaining_trailing_fragment_span_if_needed(&mut candidate).is_some(),
    );
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
    summary.record_update(rewrite_payload_if_needed(&mut candidate).is_some());
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

        let update_changed = rewrite_payload_if_needed(&mut candidate).is_some();
        summary.record_update(update_changed);
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
    fn local_cepv22_seq16_pending_stream_rewrites_to_exact_shape() {
        // Local CEP v2.2 builder harness capture from 2026-05-20. The legacy
        // server split one logical zero-declared live-object stream across
        // several deflated M windows; the stream buffer rebuilds the CNW
        // read-buffer bytes and fragment storage into this single candidate.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_cepv22_builder_seq16_pending_chunks4_20260520.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw rebuilt seq16 stream is still legacy-shaped before typed rewrites"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("CEP v2.2 seq16 pending stream should rewrite to exact EE shape");
        assert!(
            summary.changed(),
            "seq16 compatibility path must perform an explicit typed rewrite"
        );

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten CEP v2.2 seq16 stream should validate exactly");
        assert!(claim.add_records >= 1);
        assert!(claim.update_records >= 1);
        assert!(claim.live_gui_item_create_records >= 1);
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
    fn local_winds_eremor_seq16_placeable_pending_stream_rewrites_to_exact_shape() {
        // Earlier local Winds of Eremor area-entry burst from the same module:
        // four declared-zero Diamond chunks rebuilt by the pending stream
        // accumulator before a later independent live-object packet arrives.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_winds_eremor_seq16_placeable_pending_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Winds of Eremor pending stream documents the pre-rewrite Diamond shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Winds of Eremor pending placeable stream should rewrite to exact EE shape");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Winds of Eremor pending stream should validate exactly");
        assert!(claim.add_records >= 1);
        assert!(claim.creature_appearance_records >= 1);
        assert!(claim.creature_update_records >= 1);
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
    fn local_contest_seq13_creature_placeable_stream_rewrites_to_exact_shape() {
        // Local Contest Of Champions 0492 harness capture from 2026-05-21.
        // The opening inventory probe emits a mixed creature/placeable stream
        // with Diamond compact placeable add/update tails.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_contest_seq13_creature_placeable_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Contest seq13 stream documents the pre-rewrite Diamond shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Contest seq13 live-object stream should rewrite to exact EE shape");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Contest seq13 stream should validate exactly");
        assert!(claim.add_records >= 1);
        assert!(claim.creature_update_records >= 1);
        assert!(claim.update_records >= 1);
    }

    #[test]
    fn local_contest_seq14_placeable_stream_rewrites_to_exact_shape() {
        // Same local module pass as seq13. This packet is a dense placeable
        // add/update burst, useful because it repeats the compact low-tail
        // placeable family without any creature records in front.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_contest_seq14_placeable_liveobject_20260521.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Contest seq14 stream documents the pre-rewrite Diamond shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Contest seq14 live-object stream should rewrite to exact EE shape");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Contest seq14 stream should validate exactly");
        assert!(claim.add_records >= 3);
        assert!(claim.update_records >= 3);
    }

    #[test]
    fn local_prelude_seq10_pending_stream_rewrites_to_exact_shape() {
        // Local Prelude 2026-05-22 opening area stream. The pending-stream
        // accumulator rebuilt this as one `P/5/1` candidate after the compact
        // Area_ClientArea repair; it contains a creature appearance/update
        // prefix, then a compact placeable add/update pair and a world-status
        // tail that must be owned by the typed live-object passes before EE
        // sees it.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_prelude_seq10_pending_liveobject_20260522.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw Prelude seq10 stream documents the pre-rewrite Diamond shape"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("Prelude pending live-object stream should rewrite to exact EE shape");
        assert!(summary.changed());

        let claim = claim_payload_if_verified(&payload)
            .expect("rewritten Prelude pending stream should validate exactly");
        assert!(claim.add_records >= 1);
        assert!(claim.update_records >= 1);
        assert!(claim.creature_appearance_records >= 1);
        assert!(claim.creature_update_records >= 1);
        assert!(claim.world_status_records >= 1);
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
    fn pending_seq12_local_diamond_stream_uses_bounded_typed_passes_until_exact_claim() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_diamond_bw167demo_initial_live_object_seq12_20260517_unclaimed.bin"
        )
        .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "fresh pending stream capture documents the live-stream quarantine before typed passes"
        );

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None).expect(
            "bounded typed live-object passes should rewrite the fresh local Diamond stream",
        );

        assert!(summary.changed());
        assert!(
            claim_payload_if_verified(&payload).is_some(),
            "rewritten local Diamond stream must be exact EE live-object shape before emission"
        );
        assert!(
            claim_payload_if_verified_with_lifecycle(&payload, |_, _| false).is_none(),
            "shape-exact local Diamond stream documents the remaining lifecycle gap: a door update is present before EE has seen its add"
        );

        let lifecycle_summary =
            remove_unmaterialized_update_records_payload_if_possible(&mut payload, |_, _| false)
                .expect("missing-object Diamond update should be removable only after exact record-boundary proof");
        assert_eq!(lifecycle_summary.removed_update_records, 1);
        assert!(
            claim_payload_if_verified_with_lifecycle(&payload, |_, _| false).is_some(),
            "removing the proven Diamond no-op missing-object update should leave an exact EE-safe live-object payload"
        );
    }

    #[test]
    fn local_diamond_seq12_missing_object_update_cleanup_is_exactly_bounded() {
        // Local Diamond bridge capture from 2026-05-18, before the lifecycle
        // compatibility cleanup. Diamond emits a `U/10 id=2` update whose
        // generic reader consumes the mask body before discovering that no
        // object exists; EE resolves the object first and would leave the cursor
        // shifted. The M-frame live-object adapter may remove exactly that
        // missing-object update, but only after the bounded typed add/update
        // orchestration has proved the surrounding records and fragment bits.
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

        let summary = rewrite_payload_to_exact_ee_if_possible(&mut payload, None)
            .expect("bounded typed live-object passes should rewrite local Diamond seq12");
        assert!(summary.changed());

        let canonicalize = canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
            .expect("compact local Diamond door/placeable ids should canonicalize before cleanup");
        assert_eq!(canonicalize.compact_add_ids_observed, 3);
        assert_eq!(canonicalize.add_ids_rewritten, 3);
        assert_eq!(canonicalize.reference_ids_rewritten, 3);

        let exact_before_cleanup = claim_payload_if_verified(&payload)
            .expect("rewritten seq12 stream should be byte/bit exact before lifecycle cleanup");
        assert!(
            exact_before_cleanup.mentions.iter().any(|mention| {
                mention.opcode == b'U'
                    && mention.object_type == 0x0A
                    && mention.object_id == 2
                    && mention.requires_materialized_object
            }),
            "fixture must contain the orphan Diamond door update this test proves"
        );
        assert!(
            claim_payload_if_verified_with_lifecycle(&payload, |_, _| false).is_none(),
            "the orphan U/10 id=2 update must not be forwarded to EE as lifecycle-safe"
        );

        let cleanup =
            remove_unmaterialized_update_records_payload_if_possible(&mut payload, |_, _| false)
                .expect("the decompile-backed missing-object U/10 update should be removable");
        assert_eq!(cleanup.old_declared, 319);
        assert_eq!(cleanup.new_declared, 294);
        assert_eq!(cleanup.removed_update_records, 1);
        assert_eq!(cleanup.diamond_missing_object_update_records, 1);
        assert_eq!(cleanup.ee_sentinel_inventory_owner_records, 0);
        assert_eq!(cleanup.removed_bytes, 25);
        assert_eq!(cleanup.removed_fragment_bits, 13);

        let claim = claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
            .expect("seq12 stream should be exact and lifecycle-safe after bounded cleanup");
        assert_eq!(claim.add_records, 5);
        assert_eq!(claim.update_records, 5);
        assert_eq!(claim.live_bytes_length + 7, claim.declared);
        let accepted = include_bytes!(
            "../../../fixtures/live_object/local_diamond_seq12_door_placeable_stream_20260518_claimed.bin"
        );
        let accepted_claim = claim_payload_if_verified_with_lifecycle(accepted, |_, _| false)
            .expect("accepted local fixture should remain exact and lifecycle-safe");
        assert_eq!(accepted_claim.add_records, claim.add_records);
        assert_eq!(accepted_claim.update_records, claim.update_records);
        assert_eq!(accepted_claim.live_bytes_length, claim.live_bytes_length);
        assert_eq!(
            &payload[..claim.declared],
            &accepted[..accepted_claim.declared],
            "cleanup output should retain the same declared EE live-object read window as the accepted local fixture"
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
