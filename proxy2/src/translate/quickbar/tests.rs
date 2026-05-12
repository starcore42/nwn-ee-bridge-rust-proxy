use super::facade::parse_cnw_quickbar_payload;
use super::*;

#[test]
fn blank_placeholder_is_exact_36_slot_set_all_buttons_shape() {
    let payload =
        build_blank_set_all_buttons_payload(b'P').expect("blank quickbar placeholder should build");

    assert_eq!(
        payload.get(0..3),
        Some(&[b'P', QUICKBAR_MAJOR, SET_ALL_BUTTONS_MINOR][..])
    );
    assert_eq!(read_u32_le(&payload, 3), Some(39));
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
        item_buttons_preserved: 0,
        spells_preserved: 1,
        general_buttons_preserved: 0,
        general_buttons_blanked: 3,
        item_buttons_blanked: 19,
        unsupported_buttons_blanked: 13,
    };

    assert!(
        !rewrite_summary_needs_more_quickbar_bytes(&summary),
        "a decompile-owned 36-slot boundary should emit translated/blanked slots instead of falling back to visible placeholder frames"
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
        item_buttons_preserved: 0,
        spells_preserved: 1,
        general_buttons_preserved: 0,
        general_buttons_blanked: 3,
        item_buttons_blanked: 19,
        unsupported_buttons_blanked: 13,
    };

    assert!(
        rewrite_summary_needs_more_quickbar_bytes(&summary),
        "trailing read-buffer bytes without a fragment-tail proof mean the semantic boundary is not yet proven"
    );
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
    assert_eq!(summary.read_size, 1318);
    assert_eq!(summary.fragment_size, 15);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert!(
        summary.item_buttons_preserved + summary.item_buttons_blanked >= 18,
        "item slots should be either emitted from proven item-object models or deliberately blanked after boundary proof"
    );
    assert_eq!(
        summary.item_buttons_preserved, 0,
        "item-bearing slots should stay blank until the state-aware object registry can prove EE client-item materialization"
    );
    assert_eq!(summary.item_buttons_blanked, 18);
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert!(summary.spells_preserved >= 13);
    assert!(
        payload.len() < 1515,
        "unproven item materialization should be removed from the EE-facing packet while spell/general slots remain"
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
    assert_eq!(summary.read_size, 1318);
    assert_eq!(summary.fragment_size, 15);
    assert_eq!(summary.final_cursor, 1318);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert_eq!(summary.item_buttons_preserved, 0);
    assert_eq!(summary.item_buttons_blanked, 18);
    assert_eq!(summary.spells_preserved, 15);
    assert_eq!(
        summary.general_buttons_blanked, 0,
        "verified byte-identical general/blank records should no longer be counted as translator-blanked"
    );
    assert!(
        summary.item_buttons_blanked >= recovered_item_slots as u32,
        "recovered compact item branches should prove ownership but remain blank until typed EE item materialization proof exists"
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
    assert_eq!(summary.read_size, 1318);
    assert_eq!(summary.fragment_size, 15);
    assert_eq!(summary.final_cursor, 1318);
    assert_eq!(summary.trailing_read_bytes, 0);
    assert_eq!(summary.item_buttons_preserved, 0);
    assert_eq!(summary.spells_preserved, 15);
    assert_eq!(summary.general_buttons_preserved, 1);
    assert_eq!(summary.item_buttons_blanked, 18);
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
        first_page_items_after, 0,
        "item slots are intentionally blanked until the proxy can prove EE client item materialization"
    );
    assert_eq!(first_page_spells_after, first_page_spells_before);
    assert!(
        visible_after >= first_page_spells_before,
        "rewritten Starcore5 live quickbar should keep visible F1-F12 records populated: {:?}",
        &slot_types[..12]
    );
}

#[test]
fn starcore5_live_absent_fragment_presence_bits_recover_only_exact_byte_owned_items() {
    let mut payload =
        include_bytes!("../../../fixtures/quickbar/starcore5_live_20260510_set_all_buttons.bin")
            .to_vec();
    let declared = read_u32_le(&payload, 3).expect("quickbar declared length") as usize;
    let read_size = declared
        .checked_sub(HIGH_LEVEL_HEADER_BYTES)
        .expect("declared includes high-level header");
    let fragment_start = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)
        .and_then(|start| start.checked_add(read_size))
        .expect("fragment start");
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
