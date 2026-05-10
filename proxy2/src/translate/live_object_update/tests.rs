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
fn legacy_facing_zero_is_precompensated_for_ee_reader_basis() {
    assert_eq!(
        super::writer::encode_ee_scalar_orientation_from_legacy_facing(0),
        2700
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
    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    assert!(
        super::creature::advance_verified_noop_creature_update_record(
            live,
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
