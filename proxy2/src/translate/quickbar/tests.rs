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
        final_cursor: 881,
        trailing_read_bytes: 0,
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
fn trailing_quickbar_read_bytes_still_wait_for_more_stream_data() {
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
        rewrite_summary_needs_more_quickbar_bytes(&summary),
        "trailing read-buffer bytes mean the semantic boundary is not yet proven"
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
        summary.item_buttons_preserved >= 18,
        "item slots should be emitted from typed item-object models, not blanked"
    );
    assert_eq!(summary.item_buttons_blanked, 0);
    assert_eq!(summary.unsupported_buttons_blanked, 0);
    assert!(summary.spells_preserved >= 13);
    assert!(
        payload.len() > 1300,
        "EE item appearances/properties should expand the item-bearing quickbar rather than collapse it"
    );
    assert_eq!(
        payload.len(),
        2040,
        "Starcore quickbar fixture must include the EE-only active-property BOOL for every translated item slot"
    );
    assert!(
        ee_set_all_buttons_payload_shape_valid(&payload),
        "rewritten quickbar must satisfy the exact EE SetAllButtons reader shape"
    );
}
