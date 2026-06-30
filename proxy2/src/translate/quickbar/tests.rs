use super::facade::parse_cnw_quickbar_payload;
use super::*;

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.split_whitespace()
        .map(|token| u8::from_str_radix(token, 16).expect("valid hex byte"))
        .collect()
}

#[test]
fn blank_placeholder_is_exact_36_slot_set_all_buttons_shape() {
    let payload =
        build_blank_set_all_buttons_payload(b'P').expect("blank quickbar placeholder should build");

    assert_eq!(
        payload.get(0..3),
        Some(&[b'P', QUICKBAR_MAJOR, SET_ALL_BUTTONS_MINOR][..])
    );
    assert_eq!(read_u32_le(&payload, 3), Some(43));
    assert_eq!(payload.len(), 44);
    assert!(
        payload[7..43].iter().all(|byte| *byte == 0),
        "placeholder read buffer must be exactly 36 blank slots with no synthetic prefix"
    );
    assert_eq!(payload[43], 0x60);
    assert!(ee_set_all_buttons_payload_shape_valid(&payload));
}

#[test]
fn owned_quickbar_boundary_with_many_blanks_does_not_wait_for_placeholder() {
    let summary = QuickbarRewriteSummary {
        old_payload_length: 1340,
        new_payload_length: 64,
        old_declared: 884,
        new_declared: 43,
        read_size: 881,
        fragment_size: 459,
        final_cursor: 870,
        trailing_read_bytes: 11,
        direct_opcode_stream: false,
        item_buttons_seen: 0,
        item_buttons_source_explicit: 0,
        item_buttons_source_compact: 0,
        item_buttons_source_recovered: 0,
        item_buttons_preserved: 0,
        spells_preserved: 1,
        blank_buttons_seen: 0,
        general_buttons_preserved: 0,
        general_buttons_blanked: 3,
        item_buttons_blanked: 19,
        item_buttons_blanked_candidate: 19,
        unsupported_buttons_blanked: 13,
        item_buttons_rejected_recovered_type_tag: 0,
        item_buttons_rejected_missing_type_source: 0,
        item_buttons_rejected_no_present_item: 0,
        item_buttons_rejected_invalid_object_id: 0,
        item_buttons_rejected_missing_active_properties: 0,
        item_buttons_rejected_unsupported_appearance_type: 0,
        item_buttons_rejected_appearance_shape: 0,
        item_buttons_rejected_missing_state_proof: 0,
        item_objects_preserved_by_explicit_self_materialization: 0,
        item_objects_preserved_by_active_state: 0,
        item_objects_preserved_by_feature25_first: 0,
        item_objects_preserved_by_feature25_second: 0,
        item_objects_preserved_by_feature25_legacy_tail: 0,
    };

    assert!(
        !rewrite_summary_needs_more_quickbar_bytes(&summary),
        "a decompile-owned 36-slot boundary should emit translated/blanked slots instead of falling back to visible placeholder frames"
    );
}

#[test]
fn owned_quickbar_boundary_with_only_blank_slots_does_not_wait_for_placeholder() {
    let summary = QuickbarRewriteSummary {
        old_payload_length: 96,
        new_payload_length: 44,
        old_declared: 43,
        new_declared: 43,
        read_size: 40,
        fragment_size: 2,
        final_cursor: 36,
        trailing_read_bytes: 4,
        direct_opcode_stream: false,
        item_buttons_seen: 0,
        item_buttons_source_explicit: 0,
        item_buttons_source_compact: 0,
        item_buttons_source_recovered: 0,
        item_buttons_preserved: 0,
        spells_preserved: 0,
        blank_buttons_seen: 36,
        general_buttons_preserved: 0,
        general_buttons_blanked: 0,
        item_buttons_blanked: 0,
        item_buttons_blanked_candidate: 0,
        unsupported_buttons_blanked: 0,
        item_buttons_rejected_recovered_type_tag: 0,
        item_buttons_rejected_missing_type_source: 0,
        item_buttons_rejected_no_present_item: 0,
        item_buttons_rejected_invalid_object_id: 0,
        item_buttons_rejected_missing_active_properties: 0,
        item_buttons_rejected_unsupported_appearance_type: 0,
        item_buttons_rejected_appearance_shape: 0,
        item_buttons_rejected_missing_state_proof: 0,
        item_objects_preserved_by_explicit_self_materialization: 0,
        item_objects_preserved_by_active_state: 0,
        item_objects_preserved_by_feature25_first: 0,
        item_objects_preserved_by_feature25_second: 0,
        item_objects_preserved_by_feature25_legacy_tail: 0,
    };

    assert!(
        !rewrite_summary_needs_more_quickbar_bytes(&summary),
        "type-0 slots are decompile-owned one-byte records, so all-blank quickbars with fragment-tail proof should not be replaced by placeholders"
    );
}

#[test]
fn unproven_trailing_quickbar_read_bytes_still_wait_for_more_stream_data() {
    let summary = QuickbarRewriteSummary {
        old_payload_length: 1340,
        new_payload_length: 64,
        old_declared: 884,
        new_declared: 43,
        read_size: 881,
        fragment_size: 0,
        final_cursor: 870,
        trailing_read_bytes: 11,
        direct_opcode_stream: false,
        item_buttons_seen: 0,
        item_buttons_source_explicit: 0,
        item_buttons_source_compact: 0,
        item_buttons_source_recovered: 0,
        item_buttons_preserved: 0,
        spells_preserved: 1,
        blank_buttons_seen: 0,
        general_buttons_preserved: 0,
        general_buttons_blanked: 3,
        item_buttons_blanked: 19,
        item_buttons_blanked_candidate: 19,
        unsupported_buttons_blanked: 13,
        item_buttons_rejected_recovered_type_tag: 0,
        item_buttons_rejected_missing_type_source: 0,
        item_buttons_rejected_no_present_item: 0,
        item_buttons_rejected_invalid_object_id: 0,
        item_buttons_rejected_missing_active_properties: 0,
        item_buttons_rejected_unsupported_appearance_type: 0,
        item_buttons_rejected_appearance_shape: 0,
        item_buttons_rejected_missing_state_proof: 0,
        item_objects_preserved_by_explicit_self_materialization: 0,
        item_objects_preserved_by_active_state: 0,
        item_objects_preserved_by_feature25_first: 0,
        item_objects_preserved_by_feature25_second: 0,
        item_objects_preserved_by_feature25_legacy_tail: 0,
    };

    assert!(
        rewrite_summary_needs_more_quickbar_bytes(&summary),
        "trailing read-buffer bytes without a fragment-tail proof mean the semantic boundary is not yet proven"
    );
}

#[test]
fn local_diamond_zero_declared_quickbar_uses_typed_boundary_not_placeholder() {
    // Captured from the local Diamond `bw167demo` bridge harness on
    // 2026-05-17, sequence 16. Diamond emits a GuiQuickbar_SetAllButtons
    // window whose CNW declared value is zero, so the four bytes after
    // `P 1E 01` are treated as prefixed fragment bytes by the transport
    // normalizer. The decompile-backed 36-slot reader still proves the
    // complete slot model: 17 spell buttons, 18 general buttons, and one
    // unmaterialized item candidate. This must be claimed by the typed
    // quickbar translator, not replaced by a visible blank placeholder frame.
    let mut payload = include_bytes!(
        "../../../fixtures/quickbar/local_diamond_bw167demo_zero_declared_seq16_set_all_buttons.bin"
    )
    .to_vec();

    assert_eq!(
        read_u32_le(&payload, 3),
        Some(0),
        "fixture documents the local-Diamond zero-declared transport shape"
    );

    let (_normalization, summary) =
        normalize_and_rewrite_quickbar_payload_if_possible(&mut payload)
            .expect("zero-declared local Diamond quickbar should be semantically owned");

    println!("{summary:?}");
    assert_eq!(summary.spells_preserved, 17);
    assert_eq!(
        summary.item_buttons_preserved + summary.item_buttons_blanked,
        1
    );
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert_eq!(summary.trailing_read_bytes, 42);
    assert!(
        !rewrite_summary_needs_more_quickbar_bytes(&summary),
        "typed 36-slot ownership should emit the real quickbar instead of buffering into a placeholder"
    );
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten zero-declared quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten zero-declared quickbar should expose validated EE slot types");
    assert_eq!(
        slot_types,
        vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0,
            2, 2, 2, 2, 2, 2, 2,
        ],
        "local Diamond zero-declared fixture keeps the source spell slots in their proven positions"
    );
    assert_eq!(
        slot_types
            .iter()
            .filter(|slot_type| **slot_type == 2)
            .count(),
        17,
        "all proven spell buttons should remain visible after typed rewrite"
    );
}

#[test]
fn local_diamond_zero_declared_quickbar_split_stops_at_tail_zero_boundary() {
    // This pins the fast path used by the local Diamond harness. The first
    // four bytes after `P 1E 01` are the prefixed fragment bytes; the 36-slot
    // reader can already prove the complete quickbar at fragment_tail_len=0.
    // Later tail candidates are scoring artifacts, not better semantic proof.
    let payload = include_bytes!(
        "../../../fixtures/quickbar/local_diamond_bw167demo_zero_declared_seq16_set_all_buttons.bin"
    );

    assert_eq!(
        read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES),
        Some(0),
        "fixture documents the zero-declared source transport shape"
    );

    let prefixed_fragment_bytes = payload
        .get(HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES)
        .expect("fixture should contain prefixed fragment bytes");
    let body_and_tail = payload
        .get(HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..)
        .expect("fixture should contain a quickbar body");

    let split = super::split::choose_quickbar_split(
        body_and_tail,
        prefixed_fragment_bytes,
        super::split::QuickbarSplitPolicy::DecompileOwnedBoundary,
    )
    .expect("zero-declared quickbar should have a decompile-owned split");

    assert_eq!(split.fragment_tail_len, 0);
    assert_eq!(split.read_body_len, body_and_tail.len());
    assert_eq!(split.spell_slots, 17);
    assert_eq!(split.item_candidate_slots, 1);
    assert_eq!(split.unsupported_slots, 0);
    assert_eq!(split.trailing_read_bytes, 42);
}

#[test]
fn starcore_druid60_initial_quickbar_rewrites_item_slots_from_msb_fragments() {
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/starcore_druid60_initial_set_all_buttons.bin")
            .to_vec();
    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("Starcore initial quickbar fixture should parse before rewriting");
    let visible_slots = parsed
        .buttons
        .iter()
        .take(12)
        .filter(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::Item { .. } | QuickbarButtonKind::Spell { .. }
            )
        })
        .count();
    assert!(
        visible_slots >= 6,
        "the visible F1-F12 page must contain real item/spell buttons before rewriting"
    );

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("Starcore initial quickbar capture should be semantically owned");

    println!("{summary:?}");
    assert!(!summary.direct_opcode_stream);
    assert_eq!(summary.old_declared, 1321);
    assert_eq!(summary.read_size, 1314);
    assert_eq!(summary.fragment_size, 19);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert!(
        summary.item_buttons_preserved + summary.item_buttons_blanked >= 18,
        "item slots should be either emitted from proven item-object models or deliberately blanked after boundary proof"
    );
    assert!(
        summary.item_buttons_preserved > 0,
        "explicit type-1 item bodies are self-materializing in EE once the item-object model is exact"
    );
    assert!(
        summary.item_buttons_blanked < 18,
        "only compact/recovered or otherwise unproven item bodies should remain deliberate blanks"
    );
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert!(summary.spells_preserved >= 13);
    assert!(
        payload.len() > 1000,
        "emitting exact EE item-object bodies should keep the item materialization payload instead of shrinking to spell-only quickbar slots"
    );
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten quickbar should expose validated EE slot types");
    let visible_first_page = slot_types
        .iter()
        .take(12)
        .filter(|slot_type| **slot_type != 0)
        .count();
    assert!(
        visible_first_page >= 6,
        "rewritten Starcore initial quickbar should keep visible F1-F12 records populated: {:?}",
        &slot_types[..12]
    );
}

#[test]
fn state_proven_quickbar_item_objects_emit_typed_ee_item_slots() {
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/starcore_druid60_initial_set_all_buttons.bin")
            .to_vec();
    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("Starcore initial quickbar fixture should parse before rewriting");
    let mut known_item_objects = std::collections::BTreeSet::new();
    for button in &parsed.buttons {
        if let QuickbarButtonKind::Item {
            primary,
            secondary,
            recovered_type_tag: false,
            ..
        } = &button.kind
        {
            if primary.present {
                known_item_objects.insert(primary.object_id);
            }
            if secondary.present {
                known_item_objects.insert(secondary.object_id);
            }
        }
    }
    assert!(
        !known_item_objects.is_empty(),
        "fixture must contain explicit type-1 item slots for state-backed materialization proof"
    );

    let item_object_proof = |object_id| {
        known_item_objects
            .contains(&object_id)
            .then_some(QuickbarItemMaterializationProof::ActiveObject)
    };
    let materialization = QuickbarMaterializationContext::new_with_proof(&item_object_proof);
    let summary = rewrite_simple_quickbar_payload_with_context_if_possible(
        &mut payload,
        Some(&materialization),
    )
    .expect("state-backed Starcore initial quickbar capture should be semantically owned");

    assert!(
        summary.item_buttons_preserved > 0,
        "verified object-registry proof should allow explicit item slots to be emitted"
    );
    assert!(
        summary.item_buttons_blanked < 18,
        "state proof should reduce deliberate item-slot blanking without allowing recovered compact slots"
    );
    assert!(ee_set_all_buttons_payload_shape_valid(&payload));
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten quickbar should expose validated EE slot types");
    assert!(
        slot_types.iter().any(|slot_type| *slot_type == 1),
        "rewritten quickbar should contain at least one validated EE item slot"
    );
}

#[test]
fn starcore5_compact_item_body_without_source_type_preserves_spells_and_blanks_unverified_items() {
    let mut payload = include_bytes!(
        "../../../fixtures/quickbar/starcore5_compact_missing_item_type_set_all_buttons.bin"
    )
    .to_vec();
    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("Starcore5 compact quickbar fixture should parse before rewriting");
    let recovered_item_slots = parsed
        .buttons
        .iter()
        .filter(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::Item {
                    recovered_type_tag: true,
                    ..
                }
            )
        })
        .count();
    assert!(
        recovered_item_slots > 0,
        "at least one compact item body must be consumed to prove the quickbar boundary"
    );

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("Starcore5 compact quickbar capture should be semantically owned");

    println!("{summary:?}");
    assert!(!summary.direct_opcode_stream);
    assert_eq!(summary.old_payload_length, 1340);
    assert!(
        summary.new_payload_length < 1494,
        "compact item bodies prove the boundary but must not be emitted until EE item materialization is state-proven"
    );
    assert_eq!(summary.old_declared, 1321);
    assert_eq!(summary.read_size, 1314);
    assert_eq!(summary.fragment_size, 19);
    assert_eq!(summary.final_cursor, 1314);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert!(
        summary.item_buttons_preserved > 0,
        "explicit type-1 item bodies should self-materialize while compact/recovered bodies remain blank"
    );
    assert!(
        summary.item_buttons_blanked >= recovered_item_slots as u32,
        "recovered compact item branches should remain deliberate blanks"
    );
    assert_eq!(summary.spells_preserved, 15);
    assert_eq!(
        summary.general_buttons_blanked, 0,
        "this compact missing-type fixture has no unproven general command slot to blank"
    );
    assert!(
        summary.item_buttons_preserved + summary.item_buttons_blanked >= 18,
        "all item-like slots must be either emitted from explicit decompile-owned bodies or deliberately blanked"
    );
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten quickbar should expose validated EE slot types");
    let visible_first_page = slot_types
        .iter()
        .take(12)
        .filter(|slot_type| **slot_type != 0)
        .count();
    assert!(
        visible_first_page >= 6,
        "rewritten Starcore5 quickbar should keep visible F1-F12 records populated: {:?}",
        &slot_types[..12]
    );
}

#[test]
fn starcore5_live_driver_only_capture_keeps_visible_quickbar_page_populated() {
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/starcore5_live_20260510_set_all_buttons.bin")
            .to_vec();
    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("latest Starcore5 live quickbar fixture should parse before rewriting");
    let visible_before = parsed
        .buttons
        .iter()
        .take(12)
        .filter(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::Item { .. } | QuickbarButtonKind::Spell { .. }
            )
        })
        .count();
    let first_page_items_before = parsed
        .buttons
        .iter()
        .take(12)
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Item { .. }))
        .count();
    let first_page_spells_before = parsed
        .buttons
        .iter()
        .take(12)
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
        .count();
    println!(
        "first page before: visible={visible_before} items={first_page_items_before} spells={first_page_spells_before}"
    );
    assert!(
        visible_before >= 6,
        "the live driver-only capture should contain visible F1-F12 quickbar content before rewriting"
    );

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("latest Starcore5 live quickbar capture should be semantically owned");

    println!("{summary:?}");
    assert!(!summary.direct_opcode_stream);
    assert_eq!(summary.old_payload_length, 1340);
    assert_eq!(summary.old_declared, 1321);
    assert_eq!(summary.read_size, 1314);
    assert_eq!(summary.fragment_size, 19);
    assert_eq!(summary.final_cursor, 1314);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert_eq!(
        summary.item_buttons_preserved, 18,
        "the live fixture carries full explicit type-1 item bodies that EE can self-materialize"
    );
    assert_eq!(summary.spells_preserved, 15);
    assert_eq!(summary.general_buttons_preserved, 0);
    assert_eq!(
        summary.general_buttons_blanked, 1,
        "the live Starcore5 command slot must not be emitted as byte-identical after the EE reader overflow proof"
    );
    assert_eq!(
        summary.blank_buttons_seen, 2,
        "the live Starcore5 quickbar should account for the two real blank slots separately from item/spell/general records"
    );
    assert_eq!(summary.item_buttons_blanked, 0);
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten quickbar should expose validated EE slot types");
    let visible_after = slot_types
        .iter()
        .take(12)
        .filter(|slot_type| **slot_type != 0)
        .count();
    let first_page_items_after = slot_types
        .iter()
        .take(12)
        .filter(|slot_type| **slot_type == 1)
        .count();
    let first_page_spells_after = slot_types
        .iter()
        .take(12)
        .filter(|slot_type| **slot_type == 2)
        .count();
    println!(
        "first page after: visible={visible_after} items={first_page_items_after} spells={first_page_spells_after} slot_types={:?}",
        &slot_types[..12]
    );
    assert_eq!(
        first_page_items_after, first_page_items_before,
        "full explicit quickbar item bodies should self-materialize through the EE client reader"
    );
    assert_eq!(first_page_spells_after, first_page_spells_before);
    assert_eq!(
        slot_types[35], 0,
        "the final type-18 command slot is intentionally blanked until a focused EE command-slot writer is proven"
    );
    assert!(
        visible_after >= first_page_spells_before,
        "rewritten Starcore5 live quickbar should keep visible F1-F12 records populated: {:?}",
        &slot_types[..12]
    );
}

#[test]
fn starcore5_compact_quickbar_with_valid_declared_offset_claims_spells() {
    // Captured from the post-area Starcore5 driver-only coalesced stream on
    // 2026-05-13. Rechecking EE `CNWMessage::SetReadMessage` showed that the
    // DWORD at `P 1E 01 + 3` is a valid declared fragment offset for this
    // compact packet, not a fragment-prefix artifact. The quickbar semantic
    // translator should therefore claim it directly and the transport
    // normalizer must not move those bytes to the fragment tail.
    let mut payload = hex_bytes(
        "50 1E 01 56 00 00 00 \
         0A 01 00 00 00 04 2F 01 00 00 \
         02 00 18 00 00 00 00 00 \
         02 00 32 00 00 00 00 00 \
         02 00 25 00 00 00 00 00 \
         02 00 64 00 00 00 00 00 \
         02 00 90 00 00 00 00 00 \
         00 00 00 00 00 00 00 00 00 00 \
         00 00 00 00 00 00 00 00 00 00 \
         00 00 00 00 00 00 00 00 \
         00 70",
    );

    assert!(
        normalize_and_rewrite_quickbar_payload_if_possible(&mut payload.clone()).is_none(),
        "a decompile-valid SetReadMessage declared offset must not be treated as prefixed fragments"
    );

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("compact Starcore5 quickbar should be semantically owned");

    assert_eq!(summary.old_declared, 0x56);
    assert_eq!(summary.old_payload_length, 87);
    assert_eq!(summary.read_size, 79);
    assert_eq!(summary.fragment_size, 1);
    assert_eq!(summary.spells_preserved, 5);
    assert_eq!(
        summary.blank_buttons_seen, 29,
        "compact quickbar diagnostics should prove this stream contained no hidden item slots"
    );
    assert_eq!(summary.general_buttons_preserved, 2);
    assert_eq!(summary.item_buttons_blanked, 0);
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten compact quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten compact quickbar should expose validated EE slot types");
    assert_eq!(&slot_types[..7], &[10, 4, 2, 2, 2, 2, 2]);
}

#[test]
fn local_xp2_chapter3_compact_declared_quickbar_claims_spells() {
    let mut payload = include_bytes!(
        "../../../fixtures/quickbar/local_xp2_chapter3_seq16_set_all_buttons_20260523.bin"
    )
    .to_vec();

    assert_eq!(
        read_u32_le(&payload, HIGH_LEVEL_HEADER_BYTES),
        Some(0xFD),
        "fixture documents a compact local XP2 Chapter 3 packet with a valid declared offset"
    );

    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("local XP2 Chapter 3 compact quickbar source should parse before rewriting");
    assert_eq!(parsed.read_size, 246);
    assert_eq!(parsed.fragment_size, 2);
    assert_eq!(parsed.final_cursor, 246);

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("local XP2 Chapter 3 compact quickbar should be semantically owned");

    println!("{summary:?}");
    assert_eq!(summary.old_payload_length, 255);
    assert_eq!(summary.old_declared, 0xFD);
    assert_eq!(summary.read_size, 246);
    assert_eq!(summary.fragment_size, 2);
    assert_eq!(summary.spells_preserved, 22);
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten local XP2 Chapter 3 quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten local XP2 Chapter 3 quickbar should expose validated EE slot types");
    assert!(
        slot_types
            .iter()
            .filter(|slot_type| **slot_type == 2)
            .count()
            >= 20,
        "XP2 Chapter 3 quickbar should keep its proven spell slots visible"
    );
}

#[test]
fn local_cepv22_crp_starter_quickbar_keeps_unverified_items_blanked() {
    // Captured from the local Diamond bridge harness against the CEP v2.2 CRP
    // starter module on 2026-05-23 after opening inventory. The decompile-owned
    // reader proves the SetAllButtons boundary and the spell/general slots, but
    // the two item-like branches do not yet have a verified EE materialization
    // shape: one is an unowned explicit item candidate and one is a compact body
    // recovered without its source type tag. They must stay deliberate blanks.
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/local_cepv22_crp_starter_quickbar_20260523.bin")
            .to_vec();

    assert_eq!(
        read_u32_le(&payload, HIGH_LEVEL_HEADER_BYTES),
        Some(0xFD),
        "fixture documents a compact local CEPv22 CRP packet with a valid declared offset"
    );

    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("local CEPv22 CRP quickbar source should parse before rewriting");
    let recovered_item_slots = parsed
        .buttons
        .iter()
        .filter(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::Item {
                    recovered_type_tag: true,
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        recovered_item_slots, 1,
        "fixture must keep the compact missing-type item branch visible to the reader"
    );
    assert_eq!(parsed.read_size, 246);
    assert_eq!(parsed.fragment_size, 2);
    assert_eq!(parsed.final_cursor, 246);

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("local CEPv22 CRP quickbar should be semantically owned");

    println!("{summary:?}");
    assert_eq!(summary.old_payload_length, 255);
    assert_eq!(summary.old_declared, 0xFD);
    assert_eq!(summary.read_size, 246);
    assert_eq!(summary.fragment_size, 2);
    assert_eq!(summary.final_cursor, 246);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert_eq!(summary.spells_preserved, 22);
    assert_eq!(
        summary.blank_buttons_seen, 8,
        "local CEPv22 CRP quickbar diagnostics should distinguish true blank slots from unverified item slots"
    );
    assert_eq!(summary.general_buttons_preserved, 4);
    assert_eq!(summary.item_buttons_preserved, 0);
    assert_eq!(
        summary.item_buttons_blanked, 2,
        "compact or unowned item branches stay blanked until EE materialization is proven"
    );
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten local CEPv22 CRP quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten local CEPv22 CRP quickbar should expose validated EE slot types");
    assert_eq!(
        slot_types
            .iter()
            .filter(|slot_type| **slot_type == 1)
            .count(),
        0,
        "unverified item materialization must not leak into the emitted EE quickbar"
    );
    assert_eq!(
        slot_types
            .iter()
            .filter(|slot_type| **slot_type == 2)
            .count(),
        22,
        "all proven CEPv22 CRP spell slots should remain visible"
    );
}

#[test]
fn local_kingmaker_quickbar_keeps_repeated_compact_policy_pinned() {
    // Captured from the local Diamond bridge harness against the Kingmaker
    // premium NWM on 2026-05-24, sequence 14. This byte-distinct packet has
    // the same decompile-owned policy as the CEPv22 CRP startup quickbar:
    // preserve proven spell/general slots, but keep the compact/unowned item
    // branches blank until EE item materialization has independent proof.
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/local_kingmaker_seq14_quickbar_20260524.bin")
            .to_vec();

    assert_eq!(
        read_u32_le(&payload, HIGH_LEVEL_HEADER_BYTES),
        Some(0xFD),
        "fixture documents a compact Kingmaker packet with a valid declared offset"
    );

    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("local Kingmaker quickbar source should parse before rewriting");
    assert_eq!(parsed.read_size, 246);
    assert_eq!(parsed.fragment_size, 2);
    assert_eq!(parsed.final_cursor, 246);

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("local Kingmaker quickbar should be semantically owned");

    println!("{summary:?}");
    assert_eq!(summary.old_payload_length, 255);
    assert_eq!(summary.old_declared, 0xFD);
    assert_eq!(summary.read_size, 246);
    assert_eq!(summary.fragment_size, 2);
    assert_eq!(summary.final_cursor, 246);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert_eq!(summary.spells_preserved, 22);
    assert_eq!(summary.general_buttons_preserved, 4);
    assert_eq!(summary.item_buttons_preserved, 0);
    assert_eq!(summary.item_buttons_blanked, 2);
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten local Kingmaker quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten local Kingmaker quickbar should expose validated EE slot types");
    assert_eq!(
        slot_types
            .iter()
            .filter(|slot_type| **slot_type == 1)
            .count(),
        0,
        "unverified item materialization must remain blanked for the Kingmaker capture"
    );
    assert_eq!(
        slot_types
            .iter()
            .filter(|slot_type| **slot_type == 2)
            .count(),
        22,
        "all proven Kingmaker spell slots should remain visible"
    );
}

#[test]
fn starcore5_live_absent_fragment_presence_bits_recover_only_exact_byte_owned_items() {
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/starcore5_live_20260510_set_all_buttons.bin")
            .to_vec();
    let declared = read_u32_le(&payload, 3).expect("quickbar declared length") as usize;
    let fragment_start = declared;
    let fragments = payload
        .get_mut(fragment_start..)
        .expect("fixture has quickbar fragment tail");
    let final_bits = fragments[0] & 0xE0;
    fragments[0] = final_bits;
    for byte in fragments.iter_mut().skip(1) {
        *byte = 0;
    }

    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("byte-owned item bodies should prove exact quickbar shape");
    let recovered_items = parsed
        .buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Item { .. }))
        .count();
    assert!(
        recovered_items >= 13,
        "compact byte-owned item bodies should be materialized when the item model and 36-slot boundary are exact"
    );
    assert!(
        recovered_items < 18,
        "the translator must not invent all item slots from absent fragment presence bits; non-byte-owned items remain deliberate blanks"
    );

    let summary = rewrite_simple_quickbar_payload_if_possible(&mut payload)
        .expect("mutated Starcore5 quickbar capture should still be semantically owned");
    assert_eq!(summary.item_buttons_preserved, 0);
    assert!(
        summary.item_buttons_blanked >= recovered_items as u32,
        "absent fragment presence bits must not be promoted into unproven item materialization"
    );
    assert!(
        summary.item_buttons_blanked <= 18,
        "byte-owned recovery may prove ownership, but unproven EE materialization must stay bounded to known item slots"
    );
    assert_eq!(summary.spells_preserved, 15);
    assert!(ee_set_all_buttons_payload_shape_valid(&payload));
}

#[test]
fn state_proven_compact_byte_owned_quickbar_items_emit_typed_ee_slots() {
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/starcore5_live_20260510_set_all_buttons.bin")
            .to_vec();
    let declared = read_u32_le(&payload, 3).expect("quickbar declared length") as usize;
    let fragments = payload
        .get_mut(declared..)
        .expect("fixture has quickbar fragment tail");
    let final_bits = fragments[0] & 0xE0;
    fragments[0] = final_bits;
    for byte in fragments.iter_mut().skip(1) {
        *byte = 0;
    }

    let parsed = parse_cnw_quickbar_payload(&payload)
        .expect("byte-owned item bodies should prove exact quickbar shape");
    let mut known_item_objects = std::collections::BTreeSet::new();
    let compact_item_slots = parsed
        .buttons
        .iter()
        .filter_map(|button| match &button.kind {
            QuickbarButtonKind::Item {
                primary,
                secondary,
                source: QuickbarItemSource::CompactByteOwnedWithSourceType,
                recovered_type_tag: false,
            } => {
                if primary.present {
                    known_item_objects.insert(primary.object_id);
                }
                if secondary.present {
                    known_item_objects.insert(secondary.object_id);
                }
                Some(())
            }
            _ => None,
        })
        .count() as u32;
    assert!(
        compact_item_slots > 0,
        "fixture mutation must recover compact byte-owned item bodies"
    );
    assert!(
        !known_item_objects.is_empty(),
        "state-backed compact item proof needs concrete object ids"
    );

    let item_object_proof = |object_id| {
        known_item_objects
            .contains(&object_id)
            .then_some(QuickbarItemMaterializationProof::ActiveObject)
    };
    let materialization = QuickbarMaterializationContext::new_with_proof(&item_object_proof);
    let summary = rewrite_simple_quickbar_payload_with_context_if_possible(
        &mut payload,
        Some(&materialization),
    )
    .expect("state-backed compact quickbar capture should be semantically owned");

    assert_eq!(
        summary.item_buttons_preserved, compact_item_slots,
        "known inventory object ids should promote only compact byte-owned item slots"
    );
    assert_eq!(summary.spells_preserved, 15);
    assert!(ee_set_all_buttons_payload_shape_valid(&payload));
    let slot_types = super::validator::ee_set_all_buttons_slot_types_if_valid(&payload)
        .expect("rewritten quickbar should expose validated EE slot types");
    assert_eq!(
        slot_types
            .iter()
            .filter(|slot_type| **slot_type == 1)
            .count() as u32,
        compact_item_slots
    );
}
