//! Packet-level live-object update regression anchors.

fn ee_scalar_orientation_read_byte_from_legacy_facing(facing: u16) -> u8 {
    let scalar12 = super::writer::encode_ee_scalar_orientation_from_legacy_facing(facing);
    ((scalar12 >> 4) & 0xFF) as u8
}

fn ee_scalar_orientation_fragment_bits_from_legacy_facing(facing: u16) -> [bool; 5] {
    let scalar12 = super::writer::encode_ee_scalar_orientation_from_legacy_facing(facing);
    [
        false,
        ((scalar12 >> 3) & 1) != 0,
        ((scalar12 >> 2) & 1) != 0,
        ((scalar12 >> 1) & 1) != 0,
        (scalar12 & 1) != 0,
    ]
}

#[test]
fn legacy_facing_zero_preserves_shared_diamond_ee_packet_basis() {
    assert_eq!(
        super::writer::encode_ee_scalar_orientation_from_legacy_facing(0),
        0
    );
}

#[test]
fn legacy_facing_wraps_inside_ee_scalar_range() {
    assert!(super::writer::encode_ee_scalar_orientation_from_legacy_facing(u16::MAX) <= 0x0FFF);
}

#[test]
fn translated_door_update_record_is_exactly_claimed() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0x17, 0x00, 0x00, 0x00]);
    live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
    live.push(ee_scalar_orientation_read_byte_from_legacy_facing(0x2E00));
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());

    let mut fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained from the legacy stream.
    ];
    fragment_bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(0x2E00));
    fragment_bits.extend_from_slice(&[
        false, false, false, false, false, // five legacy door state bits.
        false, // EE-only neutral door state branch.
    ]);
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim = super::claim_payload_if_verified(&payload).expect("translated update claim");
    assert_eq!(claim.update_records, 1);
}

#[test]
fn legacy_name_bit_is_not_valid_in_ee_door_placeable_update_claims() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0x17, 0x00, 0x08, 0x00]);
    live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
    live.push(ee_scalar_orientation_read_byte_from_legacy_facing(0x2E00));
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let mut fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained from the legacy stream.
    ];
    fragment_bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(0x2E00));
    fragment_bits.extend_from_slice(&[
        false, false, false, false, false, // five legacy door state bits.
        false, // EE-only neutral door state branch.
        false, // Legacy name bit payload that EE generic updates must not claim.
    ]);
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(super::claim_payload_if_verified(&payload).is_none());
}

#[test]
fn diamond_inline_name_door_update_drops_only_legacy_name_branch() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0x17, 0x00, 0x08, 0x00]);
    live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
    live.push(0xD1);
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained from the Diamond stream.
        true, false, false, true, // Diamond scalar orientation low bits.
        false, false, false, false, false, // five Diamond door state bits.
        false, // Diamond-only name-presence branch.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("Diamond inline-name door update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert!(rewrite.bytes_removed >= 8);
    assert_eq!(rewrite.bits_inserted, 2);
    assert_eq!(rewrite.bits_removed, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated door mask");
    assert_eq!(translated_mask, 0x17);
    assert!(
        super::claim_payload_if_verified(&payload).is_some(),
        "Diamond inline-name update should become an exact EE door update"
    );
}

#[test]
fn raw_legacy_all_bits_door_update_is_not_exactly_claimed() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E, 0x00, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x16, 0x00,
    ]);
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let fragment_bits = vec![
        false, false, false, false, true, false, false, false, false, false,
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(super::claim_payload_if_verified(&payload).is_none());
}

#[test]
fn legacy_all_bits_door_update_rewrites_to_exact_ee_claim() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E, 0x00, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x16, 0x00,
    ]);
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained.
        false, false, false, false, false, // five legacy door state bits.
        false, // Diamond name-presence BOOL removed with the legacy name branch.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy door update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated door mask");
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_ORIENTATION_MASK,
        super::LEGACY_UPDATE_ORIENTATION_MASK,
        "door orientation must survive mask translation so EE reads scalar facing"
    );
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_NAME_MASK,
        0,
        "Diamond's legacy 0x0008_0000 name bit is not an EE generic update bit"
    );
    let claim = super::claim_payload_if_verified(&payload).expect("translated update claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(
        payload[7 + super::LEGACY_UPDATE_HEADER_BYTES + super::LEGACY_UPDATE_POSITION_READ_BYTES],
        ee_scalar_orientation_read_byte_from_legacy_facing(0x2E00),
        "rewritten door update must carry the high eight CNW WriteUnsigned yaw bits in the read buffer"
    );
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten door fragment bits");
    let orientation_bit_offset =
        super::CNW_FRAGMENT_HEADER_BITS + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    assert_eq!(
        &fragment_bits[orientation_bit_offset..orientation_bit_offset + 5],
        &ee_scalar_orientation_fragment_bits_from_legacy_facing(0x2E00),
        "rewritten door update must carry the low four CNW WriteUnsigned yaw bits in the fragment stream"
    );
}

#[test]
fn legacy_all_bits_placeable_update_preserves_orientation_mask_when_rewritten() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0xC8, 0x35, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0xC4, 0x22, 0x84, 0x03, 0xA9, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x49,
        0x00,
    ]);
    live.extend_from_slice(&22u32.to_le_bytes());
    live.extend_from_slice(b"Portal to Loot Testing");

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained.
        false, false, false, false, false, // five legacy placeable state bits.
        false, // Diamond name-presence BOOL removed with the legacy name branch.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy placeable update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated placeable mask");
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_ORIENTATION_MASK,
        super::LEGACY_UPDATE_ORIENTATION_MASK,
        "placeable orientation must survive mask translation so signs keep facing"
    );
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_NAME_MASK,
        0,
        "Diamond's legacy 0x0008_0000 name bit is not an EE generic update bit"
    );
    let claim = super::claim_payload_if_verified(&payload).expect("translated placeable claim");
    assert_eq!(claim.update_records, 1);
    // The first six bytes after the legacy all-bits mask are packed position.
    // In this captured portal record the legacy facing WORD starts after that
    // block and is 0x0000, so this fixture proves the placeable path emits the
    // EE scalar-orientation field without inventing yaw from position bytes.
    assert_eq!(
        payload[7 + super::LEGACY_UPDATE_HEADER_BYTES + super::LEGACY_UPDATE_POSITION_READ_BYTES],
        ee_scalar_orientation_read_byte_from_legacy_facing(0x0000),
        "rewritten placeable update must carry the high eight CNW WriteUnsigned yaw bits in the read buffer"
    );
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten placeable fragment bits");
    let orientation_bit_offset =
        super::CNW_FRAGMENT_HEADER_BITS + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    assert_eq!(
        &fragment_bits[orientation_bit_offset..orientation_bit_offset + 5],
        &ee_scalar_orientation_fragment_bits_from_legacy_facing(0x0000),
        "rewritten placeable update must carry the low four CNW WriteUnsigned yaw bits in the fragment stream"
    );
}

#[test]
fn placeable_update_boundary_skips_anchored_tail_before_name() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0xC8, 0x35, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0xC4, 0x22, 0x84, 0x03, 0xA9, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x49, 0x00,
    ]);
    live.extend_from_slice(&22u32.to_le_bytes());
    live.extend_from_slice(b"Portal to Loot Testing");
    live.extend_from_slice(&[b'A', 0x09, 0xFB, 0x4C, 0x01, 0x80]);

    let record_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        &live,
        0,
        live.len(),
    );
    assert_eq!(record_end, 51);
}

#[test]
fn anchored_tail_rejects_non_string_four_byte_name_candidate() {
    let bytes = [
        0x38, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x16, 0x00, 0xE5, 0x14, 0x00, 0x00,
    ];

    assert!(
        !super::reader::legacy_named_update_tail_following_payload_ready(&bytes, 0, bytes.len(),)
    );
}

#[test]
fn appearance_update_with_embedded_equipment_does_not_split_on_false_u09() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/player_appearance_false_u09.bin").to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("appearance rewrite");
    assert!(rewrite.bytes_inserted > 0);
    assert!(rewrite.bits_inserted > 0);
    let claim = super::claim_payload_if_verified(&payload).expect("appearance claim");
    assert_eq!(claim.records_examined, 3);
    assert_eq!(claim.creature_appearance_records, 1);
}

#[test]
fn appearance_update_slot0_visible_equipment_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/player_appearance_slot0_visible_equipment.bin"
    )
    .to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("slot0 appearance rewrite");
    assert!(rewrite.bytes_inserted > 0);
    assert!(rewrite.bits_inserted > 0);
    let claim = super::claim_payload_if_verified(&payload).expect("slot0 appearance claim");
    assert_eq!(claim.records_examined, 3);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_visual_transform_update_records, 0);
    assert_eq!(claim.creature_update_records, 1);
}

#[test]
fn unsupported_all_fields_appearance_does_not_fall_through_to_noop_claim() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/player_appearance_slot0_visible_equipment.bin"
    )
    .to_vec();
    // Corrupt the first visible-equipment add slot. The exact appearance parser
    // must reject it, and the narrow P/5 name-only no-op fallback must not claim
    // this complex 0xFFFF appearance mask with only three fragment bits.
    let first_visible_add_slot_in_payload = 7 + 157 + 5;
    payload[first_visible_add_slot_in_payload] = 0x77;
    assert!(super::claim_payload_if_verified(&payload).is_none());
}

#[test]
fn appearance_fixture_tail_cursor_anchor() {
    let payload = include_bytes!("../../../fixtures/live_object/player_appearance_false_u09.bin");
    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();
    let live = &payload[7..declared];
    let update_offset = 495;
    let update_end = live.len();
    let mut accepted = Vec::new();
    for start in super::CNW_FRAGMENT_HEADER_BITS..fragment_bits.len() {
        let mut cursor = start;
        if super::creature::advance_verified_noop_creature_update_record(
            live,
            update_offset,
            update_end,
            &fragment_bits,
            &mut cursor,
        ) {
            accepted.push((start, cursor));
        }
    }
    assert!(accepted.contains(&(29, 42)));
}

#[test]
fn creature_status_visibility_update_does_not_swallow_following_inventory_record() {
    let payload =
        include_bytes!("../../../fixtures/live_object/pending_live_object_seq31_chunks5.bin");
    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let live = &payload[7..declared];
    let update_offset = 597;
    let inventory_offset = 626;

    assert_eq!(&live[update_offset..update_offset + 2], &[b'U', 0x05]);
    assert_eq!(live[inventory_offset], b'I');
    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live,
            update_offset,
            live.len(),
        ),
        inventory_offset
    );

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header.
        false, true, false, false, false, // 0x4000 first five BOOLs.
        false, false, // 0x4000 final two BOOLs.
        false, false, false, // 0x8000 self-visibility BOOLs.
    ];
    let mut stock_bit_cursor = 3;
    assert!(
        super::creature::advance_verified_noop_creature_update_record(
            live,
            update_offset,
            inventory_offset,
            &fragment_bits,
            &mut stock_bit_cursor,
        ),
        "valid stock c408 record should consume exactly to the following inventory record"
    );
    assert_eq!(stock_bit_cursor, 13);

    let mut rewritten_live = live.to_vec();
    rewritten_live[update_offset + 10] = 0;
    rewritten_live[update_offset + 11] = 0;
    let c408_rewrite = super::creature::repair_legacy_c408_visual_effect_count_for_ee(
        &mut rewritten_live,
        update_offset,
        inventory_offset,
    )
    .expect("HG malformed c408 zero-count capture should be normalized before EE validation");
    assert_eq!(c408_rewrite.entries, 3);
    assert_eq!(c408_rewrite.bytes_rewritten, 2);
    assert_eq!(&rewritten_live[update_offset + 10..update_offset + 12], &[3, 0]);
    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    assert!(
        super::creature::advance_verified_noop_creature_update_record(
            &rewritten_live,
            update_offset,
            inventory_offset,
            &fragment_bits,
            &mut bit_cursor,
        )
    );
    assert_eq!(bit_cursor, fragment_bits.len());
}

#[test]
fn inventory_delta_followed_by_gui_item_add_rewrites_and_claims_exactly() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/player_hide_inventory_gui_span.bin").to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("embedded GUI item-create active-property bit should be rewritten");
    assert_eq!(rewrite.bits_inserted, 1);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("inventory delta plus GIA item-create should be exactly claimed");
    assert_eq!(claim.records_examined, 2);
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 1);
    // EE `sub_14076BD30` consumes the same two item-name selector bits as the
    // legacy inline-locstring branch, then one shared pre-DWORD active-property
    // BOOL plus four post-DWORD BOOLs. Diamond `sub_451020` has only three
    // post-DWORD BOOLs, so the translated GUI item-create owns seven fragment
    // bits after insertion.
    assert_eq!(claim.live_gui_fragment_bits, 7);
}

#[test]
fn inventory_0400_delta_requires_exact_fragment_cursor_consumption() {
    let live = [
        b'I', 0xDC, 0xFF, 0xFF, 0xFF, // live inventory owner id
        0x00, 0x04, // mask 0x0400: clear slots, then set slots
        0x01, 0x6B, // one cleared slot
        0x01, 0x6B, // one set slot, therefore one CNW fragment BOOL
    ];

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false, false, false, true],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim =
        super::claim_payload_if_verified(&payload).expect("exact 0x0400 inventory delta claim");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 1);

    let mut payload_with_extra_fragment_bit = vec![b'P', 0x05, 0x01];
    payload_with_extra_fragment_bit.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload_with_extra_fragment_bit.extend_from_slice(&live);
    payload_with_extra_fragment_bit.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false, false, false, true, false],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(super::claim_payload_if_verified(&payload_with_extra_fragment_bit).is_none());
}

#[test]
fn inventory_2700_zero_count_branch_requires_matching_fragment_bool() {
    let live = [
        b'I', 0xB0, 0xFF, 0xFF, 0xFF, // inventory/GUI owner id
        0x00, 0x27, // mask 0x2700: 0x0400, 0x0200, 0x0100, 0x2000
        0x00, 0x01, 0x6B, // 0x0400: no clears, one set slot
        0x00, 0x00, 0x00, 0x00, // 0x0200: zero-count DWORD branch
        0x00, // 0x0100: empty opcode stream
        0x00, 0x00, 0x00, 0x00, // 0x2000: first object-list count
        0x00, 0x00, 0x00, 0x00, // 0x2000: second object-list count
    ];

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            true,  // 0x0400 set-slot BOOL
            false, // 0x0200 first branch BOOL; does not affect read cursor here
            false, // 0x0200 second branch BOOL: false selects the DWORD branch
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim = super::claim_payload_if_verified(&payload)
        .expect("0x2700 zero-count DWORD branch should validate with matching BOOLs");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 3);

    let mut payload_with_wrong_branch_bit = vec![b'P', 0x05, 0x01];
    payload_with_wrong_branch_bit.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload_with_wrong_branch_bit.extend_from_slice(&live);
    payload_with_wrong_branch_bit.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            true,  // 0x0400 set-slot BOOL
            false, // 0x0200 first branch BOOL
            true,  // 0x0200 second branch BOOL: true selects the byte-mask branch
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(
        super::claim_payload_if_verified(&payload_with_wrong_branch_bit).is_none(),
        "the byte cursor alone is not an exact proof when the 0x0200 BOOL selects another branch"
    );
}

#[test]
fn inventory_2000_zero_count_trailing_pair_keeps_following_live_boundary() {
    let live = [
        b'I', 0xAD, 0xFF, 0xFF, 0xFF, // inventory owner object id
        0x00, 0x20, // mask 0x2000: feature-25 inventory object lists
        0x00, 0x00, 0x00, 0x00, // first list count
        0x00, 0x00, 0x00, 0x00, // second list count
        0x8B, 0x75, 0x01, 0x80, // captured Diamond compatibility tail object
        0x91, 0x75, 0x01, 0x80, // captured Diamond compatibility tail object
        b'U', 0x05, 0x9D, 0x75, 0x01, 0x80, // next live-object update boundary
        0x00, 0x00, 0x00, 0x00,
    ];

    assert_eq!(
        super::inventory::try_get_legacy_live_inventory_fragment_bit_count(&live, 0, 23),
        Some(0),
        "zero-count 0x2000 legacy compatibility tail owns no fragment BOOLs"
    );
    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            &live,
            0,
            live.len(),
        ),
        23,
        "validated inventory 0x2000 tail must not swallow the following U/5 record"
    );
}

#[test]
fn placeable_looping_effect_updates_expand_identity_visual_maps() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/placeable_effect_list_two_records_legacy.bin")
            .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy placeable looping-effect update list should expand for EE");
    assert_eq!(rewrite.update_records_rewritten, 2);
    assert_eq!(rewrite.bytes_inserted, 80);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("expanded looping-effect updates should validate exactly");
    assert_eq!(claim.records_examined, 2);
    assert_eq!(claim.update_records, 2);
}

#[test]
fn creature_update_mask_0047_is_not_rewritten_as_visual_transform_selector() {
    let mut live = vec![
        b'U', 0x05, 0x82, 0x67, 0x01, 0x80, // creature object id
        0x47, 0x00, 0x00, 0x00, // decompile-supported creature update mask
    ];
    live.extend_from_slice(&[
        0x00, 0x00, 0x80, 0x3F, // plausible read-buffer body bytes
        0x00, 0x00, 0x80, 0x3F,
    ]);
    let mut fragment_bits = vec![false, false, false, false, true, false, false, false];
    let mut record_end = live.len();

    assert!(
        super::appearance::rewrite_creature_visual_transform_update_for_ee(
            &mut live,
            0,
            &mut record_end,
            &mut fragment_bits,
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .is_none()
    );
    assert_eq!(&live[6..10], &[0x47, 0x00, 0x00, 0x00]);
}

#[test]
fn quarantined_creature_update_mask_0047_claims_exactly() {
    let payload =
        include_bytes!("../../../fixtures/live_object/creature_update_mask_0047_direct.bin");

    let claim = super::claim_payload_if_verified(payload)
        .expect("decompile-shaped U/5 mask 0x47 update should be exactly claimed");
    assert_eq!(claim.records_examined, 1);
    assert_eq!(claim.creature_update_records, 1);
    assert_eq!(claim.live_bytes_length, 36);
    assert_eq!(claim.fragment_bytes, 2);
}

#[test]
fn driver_direct_creature_update_mask_0047_variants_claim_exactly() {
    // Driver-only HG captures from the Starcore5 run after area load. Diamond
    // and EE both walk these `U/5` creature movement/status records through the
    // same mask-ordered reader branches modelled by `creature.rs`: position,
    // orientation, action, and branch-0x40 data. These fixtures are identity
    // translations only after the bounded creature cursor consumes the record
    // and its CNW fragment bits exactly; they must not fall through to the
    // generic door/placeable update path or the legacy visual-transform shim.
    let cases: &[(&[u8], usize)] = &[
        (
            include_bytes!(
                "../../fixtures/live_object/hg_direct_creature_update_0047_move_idle_33.bin"
            ),
            32,
        ),
        (
            include_bytes!(
                "../../fixtures/live_object/hg_direct_creature_update_0047_move_action2_37.bin"
            ),
            36,
        ),
        (
            include_bytes!(
                "../../fixtures/live_object/hg_direct_creature_update_0047_move_action2_49.bin"
            ),
            48,
        ),
    ];

    for (idx, (payload, live_bytes)) in cases.iter().enumerate() {
        let claim = super::claim_payload_if_verified(payload).unwrap_or_else(|| {
            panic!(
                "driver-captured U/5 mask 0x47 movement/status update case {idx} should claim exactly"
            )
        });
        assert_eq!(claim.records_examined, 1);
        assert_eq!(claim.creature_update_records, 1);
        assert_eq!(claim.update_records, 0);
        assert_eq!(claim.creature_visual_transform_update_records, 0);
        assert_eq!(claim.live_bytes_length, *live_bytes);
        assert_eq!(claim.fragment_bytes, 2);
    }
}

#[test]
fn driver_direct_creature_update_mask_0040_then_0047_pair_claims_exactly() {
    // Starcore5 driver-only capture after area load:
    //
    //   U/5 mask 0x40: compact creature state branch, mode 1, one fragment BOOL.
    //   U/5 mask 0x47: movement/action/state branch already modelled above.
    //
    // Diamond/EE consume the first record at the exact branch-0x40 read cursor.
    // The following `U` byte is a real live-object boundary, not inline string
    // data, so the boundary scanner must classify 0x40 as a numeric creature
    // update before the exact creature cursor validates both records.
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_direct_creature_update_0040_then_0047_pair.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("driver-captured U/5 0x40 + U/5 0x47 pair should claim exactly");
    assert_eq!(claim.records_examined, 2);
    assert_eq!(claim.creature_update_records, 2);
    assert_eq!(claim.update_records, 0);
    assert_eq!(claim.live_bytes_length, 48);
    assert_eq!(claim.fragment_bytes, 2);
}

#[test]
fn legacy_creature_visual_transform_selector_still_gets_identity_map() {
    let mut live = vec![
        b'U', 0x05, 0x82, 0x67, 0x01, 0x80, // creature object id
        0x01, // legacy selector byte, not a four-byte creature update mask
    ];
    let mut fragment_bits = vec![false, false, false];
    let mut record_end = live.len();

    let rewrite = super::appearance::rewrite_creature_visual_transform_update_for_ee(
        &mut live,
        0,
        &mut record_end,
        &mut fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("selector-only legacy visual-transform update should rewrite");

    assert_eq!(rewrite.bytes_inserted, 40);
    assert_eq!(rewrite.bytes_removed, 0);
    assert_eq!(record_end, 47);
    assert!(
        super::appearance::is_verified_ee_creature_visual_transform_update_record(
            &live, 0, record_end
        )
    );
}

#[test]
fn captured_creature_pair_3967_short_stream_rewrites_to_exact_ee_shape() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/creature_pair_3967_short_stream.bin")
            .to_vec();

    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured A/5 + P/5 + U/5 0x3967 stream should rewrite");
    assert_eq!(summary.records_examined, 7);
    assert_eq!(summary.bytes_inserted, 234);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten captured creature stream should validate exactly");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.creature_appearance_records, 2);
    assert_eq!(claim.creature_update_records, 2);
    assert_eq!(claim.read_buffer_only_records, 1);
}

#[test]
fn area_entry_door_pairs_rewrite_to_exact_ee_shape() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/hg_area_entry_door_pairs_claimed_records.bin")
            .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten area-entry door pairs should validate exactly");
    assert!(claim.add_records >= 2);
    assert!(claim.update_records >= 2);
}

#[test]
fn area_entry_door_and_signs_rewrite_to_exact_ee_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_area_entry_door_signs_mixed_liveobject.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten area-entry door/sign burst should validate exactly");
    assert!(claim.add_records >= 7);
    assert!(claim.update_records >= 7);
}

#[test]
fn trigger_add_mentions_expose_verified_geometry_bounds() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/hg_seq29_trigger_door_mixed_add_update.bin")
            .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("trigger/door mixed live-object burst should claim exactly");
    let trigger = claim
        .mentions
        .iter()
        .find(|mention| mention.opcode == b'A' && mention.object_type == super::TRIGGER_OBJECT_TYPE)
        .expect("exact claim should expose the verified trigger add mention");
    let bounds = trigger
        .bounds
        .expect("verified trigger geometry should derive finite protocol bounds");
    assert!(bounds.min_x.is_finite());
    assert!(bounds.min_y.is_finite());
    assert!(bounds.min_z.is_finite());
    assert!(bounds.max_x.is_finite());
    assert!(bounds.max_y.is_finite());
    assert!(bounds.max_z.is_finite());
    assert!(bounds.min_x <= bounds.max_x);
    assert!(bounds.min_y <= bounds.max_y);
    assert!(bounds.min_z <= bounds.max_z);
}

#[test]
fn captured_hg_self_c408_inventory_stream_is_claimed_after_boundary_floor() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_self_c408_inventory_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload);
    assert!(
        rewrite.is_some(),
        "captured HG C408 self/status stream should rewrite through the exact live-object path"
    );

    let claim = super::claim_payload_if_verified(&payload);
    assert!(
        claim.is_some(),
        "rewritten HG C408 self/status stream should be exactly claimable"
    );
}

#[test]
fn hg_starc5_self_d5ff_gui_rows_seq49_rewrites_and_claims_exactly() {
    // Starcore5 driver-only 2026-05-13 capture, quarantined as
    // `GameObjUpdate_LiveObject` seq49. The stream begins with a self `U/5`
    // C408 status update, then a deterministic self `I/0xD5FF` inventory
    // record, then GUI inventory rows (`G I A`) in the same high-level
    // live-object payload. This proves the D5FF inventory read cursor and any
    // adjacent fragment storage are handled by the inventory/span translators
    // before the GUI row family is allowed to claim its nested item records.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_self_d5ff_gui_rows_seq49_live_20260513_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured self D5FF + GUI-row stream should rewrite through typed live-object path");
    assert!(
        rewrite.update_records_examined > 0
            || rewrite.interleaved_fragment_spans_promoted > 0
            || rewrite.bytes_inserted > 0
            || rewrite.bytes_removed > 0,
        "D5FF GUI-row stream should make typed rewrite progress: {rewrite:?}"
    );

    let _claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten self D5FF + GUI-row stream should be exactly claimable");
}

#[test]
fn captured_hg_sooty_transition_c40f_self_status_stream_rewrites_to_exact_shape() {
    // Starcore5 driver-only Sooty Crow transition capture, quarantined as
    // `GameObjUpdate_LiveObject` seq41 on 2026-05-12. The first record is a
    // `U/5` self-creature update with mask `0x0000_C40F`: Diamond writes the
    // lower position/orientation/action branches first, then the same
    // self/status suffix used by `0xC408`, followed by a three-byte adjacent
    // CNW fragment-storage span before the next inventory record. This fixture
    // proves the packet is not raw-passed: the live-object translator must
    // promote that span and then the exact EE-shape claim must own the result.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_transition_seq41_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured HG C40F self/status stream should rewrite");
    assert!(rewrite.interleaved_fragment_spans_promoted >= 1);
    assert!(rewrite.interleaved_fragment_bytes_promoted >= 3);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten HG C40F self/status stream should be exactly claimable");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
}

#[test]
fn captured_hg_transition_seq48_delete_inventory_door_stream_is_claimed() {
    let mut payload = include_bytes!("../../../fixtures/live_object/hg_transition_seq48_delete_inventory_door.bin").to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload);
    eprintln!("rewrite={rewrite:?}");
    assert!(
        rewrite
            .as_ref()
            .map(|summary| summary.bytes_removed > 0 && summary.update_records_rewritten > 0)
            .unwrap_or(false),
        "captured transition stream should first normalize its legacy inventory/update records"
    );
    let add_rewrite =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    assert!(
        add_rewrite
            .as_ref()
            .map(|summary| summary.maps_inserted > 0)
            .unwrap_or(false),
        "captured transition door add should receive the EE visual-transform map"
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let claim = super::claim_payload_if_verified(&payload);
    assert!(
        claim.is_some(),
        "captured transition delete/inventory/door live-object stream should rewrite and claim"
    );
}

#[test]
fn captured_hg_starc5_seq48_door_sign_transition_stream_is_claimed() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_seq48_door_sign_transition_unclaimed.bin"
    )
    .to_vec();

    let first_rewrite = super::rewrite_update_records_payload_if_possible(&mut payload);
    assert!(
        first_rewrite
            .as_ref()
            .map(|summary| summary.update_records_rewritten >= 7 && summary.bytes_removed > 0)
            .unwrap_or(false),
        "captured Starcore5 door/sign stream should first normalize legacy update records"
    );
    let first_add_rewrite =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    assert!(
        first_add_rewrite
            .as_ref()
            .map(|summary| summary.maps_inserted >= 7)
            .unwrap_or(false),
        "captured Starcore5 door/sign adds should receive EE visual-transform maps"
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let second_add_rewrite =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    assert!(
        second_add_rewrite.is_some(),
        "captured Starcore5 door/sign add bits should be revisited after update bit repair"
    );
    let final_rewrite = super::rewrite_update_records_payload_if_possible(&mut payload);
    assert!(
        final_rewrite
            .as_ref()
            .map(|summary| summary.update_records_rewritten > 0)
            .unwrap_or(false),
        "captured Starcore5 door/sign stream should finish update bit repair"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("captured Starcore5 door/sign transition live-object stream should claim");
    assert!(claim.add_records >= 7);
    assert!(claim.update_records >= 7);
}

#[test]
fn hg_inventory_2e00_followed_by_gq_rows_claims_exactly() {
    let payload = include_bytes!("../../fixtures/live_object/hg_inventory_2e00_gq_rows_u_updates.bin");
    let claim = super::claim_payload_if_verified(payload)
        .expect("HG I/0x2E00 inventory plus GQ row stream should be exactly claimed");

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 22);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.creature_update_records, 3);
    assert_eq!(claim.delete_records, 1);
    assert_eq!(claim.records_examined, 6);
}

#[test]
fn hg_inventory_2e01_followed_by_gq_rows_splits_on_decompiled_cursor() {
    let payload = include_bytes!(
        "../../fixtures/live_object/hg_starc5_seq42_mixed_door_placeable_unclaimed_len519.bin"
    );
    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let live = &payload[7..declared];
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();

    let inventory_end =
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live,
            0,
            live.len(),
        );
    assert_eq!(
        inventory_end, 56,
        "0x2E01 should consume the compact 0x0001 branch, then 0x0400/0x0200/0x2000/0x0800, and stop at GQ"
    );

    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    let inventory_claim = super::inventory::advance_verified_inventory_record(
        live,
        0,
        inventory_end,
        &fragment_bits,
        &mut bit_cursor,
    )
    .expect("0x2E01 inventory prefix should be exactly claimed before GQ");
    assert_eq!(inventory_claim.fragment_bits, 13);
    assert_eq!(bit_cursor, super::CNW_FRAGMENT_HEADER_BITS + 13);

    let gui_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        live,
        inventory_end,
        live.len(),
    );
    assert_eq!(gui_end, 221);
    let gui_claim = super::gui::advance_verified_live_gui_record(
        live,
        inventory_end,
        gui_end,
        &fragment_bits,
        &mut bit_cursor,
    )
    .expect("GQ quickbar-link row stream should be exactly claimed after inventory");
    assert_eq!(gui_claim.fragment_bits, 0);

    let mut rewritten = payload.to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("captured Starcore5 stream should rewrite legacy A/5 creature adds before claim");
    assert!(
        rewrite.bytes_inserted >= 40,
        "legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("captured Starcore5 0x2E01 inventory/GQ live-object stream should claim exactly after rewrite");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 13);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
}

#[test]
fn hg_sooty_crow_npc_creature_span_rewrites_and_claims_exactly() {
    let payload =
        include_bytes!("../../../fixtures/live_object/hg_sooty_crow_npc_creature_span_len495.bin");
    let mut rewritten = payload.to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("Sooty Crow NPC creature live-object span should have a focused rewrite");

    assert!(rewrite.interleaved_fragment_spans_promoted >= 1);
    assert!(rewrite.interleaved_fragment_bytes_promoted >= 3);
    assert!(
        rewrite.bytes_inserted >= 40,
        "the real legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("Sooty Crow NPC creature live-object span should claim exactly after rewrite");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.creature_update_records, 3);
    assert_eq!(claim.creature_appearance_records, 2);
    assert_eq!(claim.add_records, 1);
}

#[test]
fn hg_sooty_crow_inventory_mask_2e01_extended_span_rewrites_and_claims_exactly() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_sooty_crow_inventory_mask_2e01_span_len611.bin"
    );
    let mut rewritten = payload.to_vec();
    let repair = crate::translate::live_object::declared_length_repair_candidates(&rewritten)
        .into_iter()
        .find(|candidate| usize::try_from(candidate.new_declared).ok() == Some(604))
        .expect("Sooty Crow live-object span should prove the final CNW fragment tail at offset 604");
    rewritten[3..7].copy_from_slice(&repair.new_declared.to_le_bytes());

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("Sooty Crow I/0x2E01 extended live-object span should have a focused rewrite");

    assert!(
        rewrite.bytes_inserted >= 40,
        "the following legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("Sooty Crow I/0x2E01 extended live-object span should claim exactly after rewrite");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_update_records, 2);
}

#[test]
fn hg_starc5_sooty_current_seq35_u5_visual_transform_update_rewrites_and_claims_exactly() {
    // Starcore5 driver-only Sooty Crow capture from 2026-05-12. The server sends
    // a stale `P 05 01` declared length (`0x5D`) that overruns the decompile-
    // proven legacy A/5 creature-add read cursor by six CNW fragment-storage
    // bytes. This fixture must go through the declared-length repair search,
    // then the focused live-object record translators, then the exact EE-shape
    // claim. It is intentionally not raw-passthrough.
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_current_seq35_u5_inventory_20260512_unclaimed.bin"
    );

    let mut accepted = None;
    for repair in crate::translate::live_object::declared_length_repair_candidates(payload) {
        let mut candidate = payload.to_vec();
        candidate[3..7].copy_from_slice(&repair.new_declared.to_le_bytes());

        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);
        let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut candidate,
            None,
        );
        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);
        let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut candidate);
        let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut candidate,
            None,
        );
        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);

        if let Some(claim) = super::claim_payload_if_verified(&candidate) {
            accepted = Some((repair, claim));
            break;
        }
    }

    let (repair, claim) = accepted
        .expect("current Starcore5 Sooty Crow U/5 visual-transform span should repair and claim exactly");
    assert_ne!(repair.new_declared, repair.old_declared);
    // The quarantined capture was initially named from the visible inventory
    // symptom, but the decompile-backed live-object opcode is `U` for object
    // type 5 and the exact accepted subfamily is EE creature visual-transform
    // update, not the `I` inventory-record reader.
    assert_eq!(claim.inventory_records, 0);
    assert!(claim.creature_visual_transform_update_records >= 1);
}

#[test]
fn hg_starc5_seq54_coalesced_liveobject_burst_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_seq54_coalesced_liveobject_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("Starcore5 coalesced live-object burst should have an exact semantic claim");

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.creature_appearance_records, 2);
    assert_eq!(claim.creature_update_records, 3);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq53_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq53_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("Starcore5 GUI inventory item-create stream should have a focused rewrite");
    assert!(
        rewrite.bits_inserted > 0 || rewrite.bytes_inserted > 0,
        "legacy GUI item-create rows should receive EE item-create extras"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten Starcore5 GUI inventory item-create stream should claim exactly");
    assert!(claim.live_gui_item_create_records > 0);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq53_live_20260512_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq53_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("live Starcore5 GUI inventory item-create stream should have a focused rewrite");
    assert!(
        rewrite.bits_inserted > 0 || rewrite.bytes_inserted > 0,
        "legacy GUI item-create rows should receive EE item-create extras"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten live Starcore5 GUI inventory item-create stream should claim exactly");
    assert!(claim.live_gui_item_create_records > 0);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq54_live_20260512_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq54_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("second live Starcore5 GUI inventory item-create stream should have a focused rewrite");
    assert!(
        rewrite.bits_inserted > 0 || rewrite.bytes_inserted > 0,
        "legacy GUI item-create rows should receive EE item-create extras"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten second live Starcore5 GUI inventory item-create stream should claim exactly");
    assert!(claim.live_gui_item_create_records > 0);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq54_live_20260512_survives_dispatch_rewrite_sequence() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq54_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("dispatch rewrite sequence should preserve exact GUI inventory claim");
    assert!(claim.live_gui_item_create_records > 0);
}

#[test]
fn hg_starc5_sooty_transition_seq34_creature_update_c44f_claims_exactly() {
    // Starcore5 driver-only Sooty Crow transition capture from 2026-05-13.
    // This was quarantined as `GameObjUpdate_LiveObject` seq34 after the area
    // transition was otherwise stable. The first record is a legacy `U/5`
    // creature update with mask `0x0000_C44F`: the already verified `0xC40F`
    // self/status family plus the low `0x0040` creature state branch.
    //
    // EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` keeps `0x0040` as a
    // typed creature state branch: optional pre-state BOOL, then WORD/BYTE/
    // WORD/BYTE/BOOL with an optional target OBJECTID when mode is 2. The proxy
    // must prove that full cursor shape instead of treating the deflated live
    // object payload as a raw zlib blob or generic passthrough.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_transition_seq34_creature_update_c44f_20260513_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("Sooty transition U/5 0xC44F creature stream should claim exactly");
    assert!(claim.creature_update_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}
