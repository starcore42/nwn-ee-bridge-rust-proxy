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

fn live_object_read_window(payload: &[u8]) -> &[u8] {
    let declared = usize::try_from(
        super::read_u32_le(payload, super::HIGH_LEVEL_HEADER_BYTES)
            .expect("fixture should have a declared CNW read length"),
    )
    .expect("declared CNW read length should fit usize");
    &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared]
}

fn assert_no_full_creature_appearance_has_longer_legacy_shape(payload: &[u8]) {
    let live = live_object_read_window(payload);
    for offset in live.windows(2).enumerate().filter_map(|(offset, pair)| {
        (pair == [b'P', super::CREATURE_OBJECT_TYPE]).then_some(offset)
    }) {
        let Some(ee_end) =
            super::appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
                live,
                offset,
                live.len(),
            )
        else {
            continue;
        };
        let Some(legacy_end) = super::appearance::try_get_legacy_creature_appearance_record_end(
            live,
            offset,
            live.len(),
        ) else {
            continue;
        };
        assert!(
            legacy_end <= ee_end,
            "translated P/5 at offset {offset} still has a longer Diamond full-appearance shape than its EE byte shape: ee_end={ee_end} legacy_end={legacy_end}"
        );
    }
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
    fragment_bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(
        0x2E00,
    ));
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
    fragment_bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(
        0x2E00,
    ));
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
    assert_eq!(rewrite.bits_inserted, 6);
    assert_eq!(rewrite.bits_removed, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated door mask");
    assert_eq!(translated_mask, 0x17);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("Diamond inline-name update should become an exact EE door update");
    assert_eq!(
        payload[7 + super::LEGACY_UPDATE_HEADER_BYTES + super::LEGACY_UPDATE_POSITION_READ_BYTES],
        0xD1,
        "legacy 8-bit Diamond orientation byte must remain the high EE scalar byte"
    );
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    );
    let fragment_bits = fragment_bits.expect("rewritten door fragment bits");
    let orientation_bit_offset =
        super::CNW_FRAGMENT_HEADER_BITS + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    assert_eq!(
        &fragment_bits[orientation_bit_offset..orientation_bit_offset + 5],
        &[false, false, false, false, false],
        "Diamond has no orientation low-nibble fragment bits; EE scalar low bits are proxy-owned zero padding"
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
fn captured_len416_door_placeable_stream_with_nonrecord_leadin_stays_quarantined() {
    // Driver-only Starcore5 capture from 2026-05-13.
    //
    // This was originally quarantined as
    // `live-object-claimed-records-rejected-door-placeable-requires-translator`:
    // later bytes contain plausible door/placeable add/update records, but the
    // declared read window starts with `44 05 F7 20 50 72`, which is not a
    // decompile-backed live-object delete record because the following DWORD is
    // not an EE/Diamond object id.  The semantic translators must not scan past
    // that unowned lead-in and mutate later records; a future transport or
    // gameplay-stream owner must first prove where those leading bytes belong.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_legacy_door_placeable_len416_rejected_20260513.bin"
    )
    .to_vec();

    let update_pre = super::rewrite_update_records_payload_if_possible(&mut payload);
    let add = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let update_post = super::rewrite_update_records_payload_if_possible(&mut payload);
    let add_name_bits = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let add_after_name =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let update_after_name = super::rewrite_update_records_payload_if_possible(&mut payload);
    assert!(update_pre.is_none());
    assert!(add.is_none());
    assert!(update_post.is_none());
    assert!(add_name_bits.is_none());
    assert!(add_after_name.is_none());
    assert!(update_after_name.is_none());
    assert!(
        super::payload_contains_door_or_placeable_add_update_record(&payload),
        "fixture should still document the quarantined door/placeable-looking records"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "non-record lead-in must keep this payload quarantined until a stream owner proves it"
    );
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
        0xC4, 0x22, 0x84, 0x03, 0xA9, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x49, 0x00,
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
fn starcore_town_greeter_trader_stream_rewrites_to_exact_ee_live_object() {
    // HG Docks capture: the stream contains a creature add followed by a full
    // `P/5` creature appearance for Town Greeter and adjacent inventory/GUI
    // live-object records. Diamond writes this as a coherent live-object burst;
    // EE must only receive it after the add/update/appearance translators have
    // proven exact record boundaries and fragment-bit ownership.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/starcore_npc_town_greeter_trader_stream_claimed_but_ee_rejects.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Diamond burst must not be claimed as already-EE-shaped"
    );

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
        .expect("Town Greeter live-object burst should rewrite to exact EE shape");
    assert!(claim.add_records >= 1);
    assert!(claim.creature_appearance_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
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
        !super::creature::advance_verified_noop_creature_update_record(
            live,
            update_offset,
            inventory_offset,
            &fragment_bits,
            &mut stock_bit_cursor,
        ),
        "legacy c408 short status rows must be translated before EE exact validation"
    );
    assert_eq!(stock_bit_cursor, 3);

    let mut rewritten_live = live.to_vec();
    let mut rewritten_record_end = inventory_offset;
    let status_rewrite =
        super::creature::insert_creature_update_status_effect_identity_maps_for_ee(
            &mut rewritten_live,
            update_offset,
            &mut rewritten_record_end,
            &fragment_bits,
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .expect("legacy c408 status rows should receive EE identity transform maps");
    assert_eq!(status_rewrite.entries, 3);
    assert_eq!(status_rewrite.bytes_inserted, 24);
    let mut translated_bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    assert!(
        super::creature::advance_verified_noop_creature_update_record(
            &rewritten_live,
            update_offset,
            rewritten_record_end,
            &fragment_bits,
            &mut translated_bit_cursor,
        ),
        "translated c408 status rows should consume exactly before following inventory"
    );
    assert_eq!(translated_bit_cursor, fragment_bits.len());

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
    assert_eq!(
        &rewritten_live[update_offset + 10..update_offset + 12],
        &[3, 0]
    );
    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    let mut rewritten_record_end = inventory_offset;
    let status_rewrite =
        super::creature::insert_creature_update_status_effect_identity_maps_for_ee(
            &mut rewritten_live,
            update_offset,
            &mut rewritten_record_end,
            &fragment_bits,
            bit_cursor,
        )
        .expect("repaired c408 rows should receive EE identity transform maps");
    assert_eq!(status_rewrite.entries, 3);
    assert_eq!(status_rewrite.bytes_inserted, 24);
    assert!(
        super::creature::advance_verified_noop_creature_update_record(
            &rewritten_live,
            update_offset,
            rewritten_record_end,
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
fn inventory_0401_delta_accepts_true_state_bool_before_equipment_delta() {
    // Live HG sequence 38 begins with this player-owned inventory state record
    // before the Town Greeter/Northern Trader creature records:
    //
    //   I ECFFFFFF 0104 9500 D4D9E005 EB0A0000 02 1E6B 01 6B
    //
    // EE `sub_1407B4F70` and Diamond `sub_455940` both consume the 0x0001
    // branch as SHORT, DWORD, INT, BOOL.  That BOOL does not select an extra
    // read-buffer branch; the reader continues into the 0x0400 equipment delta.
    let live = [
        b'I', 0xEC, 0xFF, 0xFF, 0xFF, // player inventory owner id
        0x01, 0x04, // mask 0x0401: 0x0001 state + 0x0400 equipment delta
        0x95, 0x00, // 0x0001 state SHORT
        0xD4, 0xD9, 0xE0, 0x05, // 0x0001 state DWORD
        0xEB, 0x0A, 0x00, 0x00, // 0x0001 state INT
        0x02, 0x1E, 0x6B, // 0x0400 clear-count and clear slots
        0x01, 0x6B, // 0x0400 set-count and set slot
    ];

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            true,  // 0x0001 state BOOL: valid true branch, no extra bytes
            true,  // 0x0400 set-slot BOOL
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim = super::claim_payload_if_verified(&payload)
        .expect("0x0401 inventory delta should accept true state BOOL");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 2);
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
fn local_diamond_inventory_0200_counted_cells_require_exact_branch_bits() {
    // Local Diamond door/placeable click capture from 2026-05-18:
    //
    //   I FFFEFFFF 0002 03000000 0202 0302 0402 | 07
    //
    // Diamond `sub_455940` and EE `sub_1407B4F70` both read mask 0x0200 as
    // two CNW BOOLs followed by a branch.  For this capture the branch bits
    // are false/false, so the read-buffer body is DWORD count plus count
    // two-byte cells, and the fragment stream owns one BOOL per cell.
    let payload = local_diamond_inventory_0200_payload(&[(2, 2), (3, 2), (4, 2)]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("local Diamond I/0x0200 counted-cell inventory should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 5);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);

    let fragment_offset = u32::from_le_bytes(payload[3..7].try_into().unwrap()) as usize;
    let mut wrong_branch = payload.clone();
    wrong_branch[fragment_offset] |= 0x08;
    assert!(
        super::claim_payload_if_verified(&wrong_branch).is_none(),
        "0x0200 counted-cell branch must require the decompiled second BOOL=false path"
    );
}

#[test]
fn local_diamond_inventory_0200_counted_cells_second_capture_claims_exactly() {
    let payload = local_diamond_inventory_0200_payload(&[(1, 2), (1, 3), (1, 4)]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("second local Diamond I/0x0200 counted-cell inventory should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 5);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
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

fn local_diamond_inventory_0200_payload(cells: &[(u8, u8)]) -> Vec<u8> {
    let mut live = vec![
        b'I', 0xFE, 0xFF, 0xFF, 0xFF, // sentinel/self inventory owner id
        0x00, 0x02, // mask 0x0200
    ];
    live.extend_from_slice(&(cells.len() as u32).to_le_bytes());
    for (x, y) in cells {
        live.push(*x);
        live.push(*y);
    }

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            false, // 0x0200 first branch BOOL: counted cells carry per-cell BOOLs
            false, // 0x0200 second branch BOOL: false selects DWORD-count body
            true, true, true, // one semantic cell BOOL for each counted cell
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

#[test]
fn placeable_looping_effect_updates_expand_identity_visual_maps() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/placeable_effect_list_two_records_legacy.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy placeable looping-effect update list should expand for EE");
    assert_eq!(rewrite.update_records_rewritten, 2);
    assert_eq!(rewrite.bytes_inserted, 16);

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

    assert_eq!(rewrite.bytes_inserted, 8);
    assert_eq!(rewrite.bytes_removed, 0);
    assert_eq!(record_end, 15);
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
    assert_eq!(summary.bytes_inserted, 199);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten captured creature stream should validate exactly");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.creature_appearance_records, 2);
    assert_eq!(claim.creature_update_records, 2);
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.read_buffer_only_records, 0);
}

#[test]
fn hg_starc5_sooty_3967_action0_bridge_followup_rewrites_to_ee_shape() {
    let raw = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_transition_3967_action0_bridge_followup_20260514.bin"
    );
    assert!(
        super::claim_payload_if_verified(raw).is_none(),
        "legacy bridge action-state/follow-up bytes must not be accepted as EE-readable"
    );

    let mut payload = raw.to_vec();
    let old_declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("legacy declared length");
    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("0x3967 action-0 bridge follow-up rewrite");
    assert_eq!(summary.update_records_rewritten, 1);
    assert_eq!(summary.bytes_inserted, 29);
    assert_eq!(summary.bytes_removed, 42);

    let new_declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("rewritten declared length");
    assert_eq!(new_declared, old_declared - 13);
    assert_eq!(payload.len(), raw.len() - 13);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten 0x3967 action-0 stream should validate exactly");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_update_records, 1);
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.read_buffer_only_records, 0);

    let marker = [b'U', 0x05, 0xF7, 0x4C, 0x01, 0x80, 0x67, 0x39, 0x00, 0x00];
    let u_offset = raw
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("captured 0x3967 update marker");
    let mut buggy_missing_state_byte = raw.to_vec();
    super::write_u32_le(
        &mut buggy_missing_state_byte,
        super::HIGH_LEVEL_HEADER_BYTES,
        old_declared - 3,
    )
    .expect("adjust buggy declared length");
    buggy_missing_state_byte.drain(u_offset + 25..u_offset + 28);
    assert!(
        super::claim_payload_if_verified(&buggy_missing_state_byte).is_none(),
        "removing the EE action-state byte plus follow-up WORD recreates the crash shape"
    );
}

#[test]
fn area_entry_door_pairs_rewrite_to_exact_ee_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_area_entry_door_pairs_claimed_records.bin"
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
fn hg_seq41_creature_captain_mixed_add_update_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_seq41_creature_captain_mixed_add_update.bin"
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
        .expect("HG Captain mixed creature appearance/update stream should claim exactly");
    assert!(claim.creature_appearance_records >= 1);
    assert!(claim.creature_update_records >= 1);
}

#[test]
fn captured_hg_self_c408_inventory_stream_is_claimed_after_boundary_floor() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/hg_self_c408_inventory_unclaimed.bin")
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
fn local_diamond_bw167demo_u5_4408_inventory_stream_rewrites_to_exact_shape() {
    // Local Diamond server harness capture from bw167demo on 2026-05-16. The
    // stream starts with `U/5 0x00004408`: a compact legacy status-effect delta,
    // four scalar/status WORDs, and the fragment-only self/status suffix, then
    // immediately continues with an `I` inventory record. The interior status
    // effect opcode byte is `A`, so the generic live-object boundary scanner must
    // defer to the decompile-backed creature cursor instead of splitting early.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_bw167demo_u5_4408_inventory_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("local Diamond 0x4408 status stream should rewrite through creature translator");
    assert!(
        rewrite.update_records_rewritten >= 1 || rewrite.bytes_inserted > 0,
        "0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten local Diamond 0x4408 stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
}

#[test]
fn local_diamond_auto_inventory_u5_4408_gui_rows_stream_rewrites_to_exact_shape() {
    // Local Diamond harness capture from 2026-05-19 after the driver opened
    // inventory immediately after Area_AreaLoaded. It starts with the same
    // `U/5 0x00004408` compact creature status family as the shorter bw167demo
    // fixture, then continues with the GUI inventory/repository row block
    // emitted by the inventory panel. The live-object stream owner must keep
    // this whole packet in the typed 0x4408 + GUI-row path instead of buffering
    // it as an unclaimed continuation.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq18_auto_inventory_u5_4408_gui_rows_20260519_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw local Diamond auto-inventory stream lacks the exact EE live-object opcode shape"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("local Diamond auto-inventory 0x4408 stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1
            || rewrite.bytes_inserted > 0
            || rewrite.bytes_removed > 0
            || rewrite.interleaved_fragment_spans_promoted > 0,
        "auto-inventory 0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten local Diamond auto-inventory stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(
        claim.live_gui_item_create_records >= 1,
        "GUI inventory/repository rows should remain owned by exact live-object claim"
    );
}

#[test]
fn local_to_heir_u5_4408_inventory_2a00_word_list_rewrites_to_exact_shape() {
    // Local To Heir harness capture from 2026-05-19 after Area_ClientArea was
    // repaired from the module ARE. The stream starts with the decompile-owned
    // compact `U/5 0x00004408` self/status record, then an `I/0x2A00` current
    // player inventory body. This module takes the 0x0200 false branch as a
    // nonzero DWORD count followed by WORD entries before the Feature-25 lists.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_to_heir_seq16_u5_4408_inventory_2a00_20260519_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw To Heir 0x4408 + 0x2A00 stream is still legacy-shaped"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("To Heir 0x4408 + inventory stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1 || rewrite.bytes_inserted > 0,
        "To Heir 0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten To Heir inventory stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.live_gui_read_buffer_records >= 1);
}

#[test]
fn hg_starc5_seq37_creature_effect_delta_stream_rewrites_to_exact_shape() {
    // Live HG Starcore5 driver-only capture from 2026-05-18. The stream starts
    // with a standalone creature `U/5 0x00000008` looping visual-effect delta,
    // then continues with creature appearance/add/update records in the same
    // `GameObjUpdate_LiveObject` payload. EE's current reader consumes an
    // ObjectVisualTransformData map after each status-effect entry, so this
    // fixture must be owned by the focused effects/creature path and then
    // exact-claimed. It must not fall back to raw zlib or generic passthrough.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq37_creature_effect_delta_20260518.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured seq37 creature status-effect stream should rewrite");
    assert!(
        rewrite.bytes_inserted >= 32,
        "four status-effect entries should receive EE identity transform maps: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq37 creature status-effect stream should validate exactly");
    assert!(claim.update_records >= 1);
    assert!(claim.creature_appearance_records >= 1 || claim.creature_update_records >= 1);
}

#[test]
fn hg_starc5_seq37_otis_appearance_stream_rewrites_without_legacy_false_positive() {
    // Live HG Starcore5 driver-only capture from 2026-05-18 after the
    // decompile-backed full-appearance guard was added. This stream carries an
    // NPC full `P/5` appearance for "Otis"; the legacy body/equipment shape
    // must be widened into EE's 0x2001/0x23 dialect, not accepted as a shorter
    // already-EE record.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq37_otis_appearance_20260518.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured seq37 Otis appearance stream should rewrite");
    assert!(
        rewrite.bytes_inserted > 0 || rewrite.bits_inserted > 0,
        "Otis full appearance should require an explicit typed EE rewrite: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq37 Otis appearance stream should validate exactly");
    assert!(claim.creature_appearance_records >= 1);
    assert_no_full_creature_appearance_has_longer_legacy_shape(&payload);
}

#[test]
fn hg_starc5_seq36_town_npc_appearance_stream_rewrites_to_exact_shape() {
    // Live HG Starcore5 driver-only capture from 2026-05-18. This post-area
    // burst adds and updates several creature NPCs; the first unclaimed family
    // is a `P/5` creature appearance for "Town Greeter", followed by another
    // appearance/update pair for "Northern Trader". The appearance parser must
    // own the embedded visible-equipment subobjects and exact fragment cursor,
    // instead of letting the generic live-object scanner split on interior
    // `D`/`U`-looking bytes.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq36_town_greeter_trader_appearance_20260518.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured seq36 town NPC appearance stream should rewrite");
    assert!(
        rewrite.bytes_inserted > 0
            || rewrite.bits_inserted > 0
            || rewrite.update_records_rewritten > 0,
        "seq36 stream should make typed appearance/update progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq36 town NPC appearance stream should validate exactly");
    assert!(claim.creature_appearance_records >= 2);
    assert!(claim.creature_update_records >= 1);
    assert_no_full_creature_appearance_has_longer_legacy_shape(&payload);
}

#[test]
fn raw_legacy_prefixed_town_trader_appearance_is_not_claimed_as_ee_shape() {
    // Live HG Starcore5 driver-only capture from 2026-05-18. The Northern
    // Trader `P/5` full appearance has a Diamond body-table prefix whose second
    // byte is `0x13`. EE's widened reader can otherwise mistake that byte for
    // an already-EE full-body selector and stop before the counted visible
    // equipment list. Diamond's full appearance reader proves the longer
    // semantic record, so the strict EE byte-shape probe must reject the short
    // shifted candidate and force the typed LegacyDiamond -> EE rewrite.
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq36_town_greeter_trader_appearance_20260518.bin"
    );
    let live = live_object_read_window(payload);
    let trader_offset = live
        .windows(2)
        .enumerate()
        .filter_map(|(offset, pair)| {
            (pair == [b'P', super::CREATURE_OBJECT_TYPE]).then_some(offset)
        })
        .last()
        .expect("fixture should contain the Northern Trader P/5 appearance");
    let legacy_end = super::appearance::try_get_legacy_creature_appearance_record_end(
        live,
        trader_offset,
        live.len(),
    )
    .expect("Diamond reader should prove the full trader appearance");
    let ee_end = super::appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
        live,
        trader_offset,
        live.len(),
    );

    assert!(
        ee_end.is_none_or(|ee_end| ee_end >= legacy_end),
        "short shifted EE byte-shape candidate must not hide longer Diamond P/5 appearance: ee_end={ee_end:?} legacy_end={legacy_end}"
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

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload).expect(
        "captured self D5FF + GUI-row stream should rewrite through typed live-object path",
    );
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
fn hg_starc5_area_entry_d5ff_creature_inventory_candidate_claims_exactly() {
    // Starcore5 driver-only 2026-05-17 area-entry capture, quarantined after
    // the live-object rewrite inserted EE creature-add visual-transform bytes
    // but before the exact validator could claim the self creature inventory
    // `I/0xD5FF` record. This fixture keeps the fix packet-family owned:
    // Diamond `sub_455940` and EE `sub_1407B4F70` both drive the inventory
    // mask branches, so the record must be parsed as typed inventory rather
    // than relaxed as raw zlib or a generic live-object blob.
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_area_entry_d5ff_creature_inventory_20260517_rejected.bin"
    );

    let _claim = super::claim_payload_if_verified(payload)
        .expect("area-entry D5FF creature inventory stream should claim exactly");
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
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_transition_seq48_delete_inventory_door.bin"
    )
    .to_vec();
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
    let final_rewrite = super::rewrite_update_records_payload_if_possible(&mut payload);
    if second_add_rewrite.is_none() && final_rewrite.is_none() {
        assert!(
            super::claim_payload_if_verified(&payload).is_some(),
            "captured Starcore5 door/sign stream must be exact-claimable if final add/update passes are no-ops"
        );
    }

    let claim = super::claim_payload_if_verified(&payload)
        .expect("captured Starcore5 door/sign transition live-object stream should claim");
    assert!(claim.add_records >= 7);
    assert!(claim.update_records >= 7);
}

#[test]
fn hg_inventory_2e00_followed_by_gq_rows_claims_exactly() {
    let payload =
        include_bytes!("../../fixtures/live_object/hg_inventory_2e00_gq_rows_u_updates.bin");
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
fn hg_inventory_2e00_gui_hand_trap_sentinel_claims_exactly() {
    let mut payload =
        include_bytes!("../../fixtures/live_object/hg_inventory_2e00_gq_rows_u_updates.bin")
            .to_vec();
    let inventory_offset = 7usize;
    assert_eq!(payload[inventory_offset], b'I');
    payload[inventory_offset + 1..inventory_offset + 5]
        .copy_from_slice(&0xFFFF_FFECu32.to_le_bytes());

    let claim = super::claim_payload_if_verified(&payload)
        .expect("HG I/0x2E00 inventory with 0xFFFFFFEC GUI sentinel should be exactly claimed");

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 22);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.records_examined, 6);
}

#[test]
fn hg_inventory_2e00_gui_hand_trap_false_0800_tail_claims_exactly() {
    let mut payload =
        include_bytes!("../../fixtures/live_object/hg_inventory_2e00_gq_rows_u_updates.bin")
            .to_vec();
    let inventory_offset = 7usize;
    assert_eq!(payload[inventory_offset], b'I');
    payload[inventory_offset + 1..inventory_offset + 5]
        .copy_from_slice(&0xFFFF_FFECu32.to_le_bytes());

    let declared = usize::try_from(super::read_u32_le(&payload, 3).unwrap()).unwrap();
    let mut fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();
    let branch_0800_bit = super::CNW_FRAGMENT_HEADER_BITS + 1 + 2 + (6 * 3);
    fragment_bits[branch_0800_bit] = false;
    let repacked = super::bits::pack_msb_valid_bits(fragment_bits, super::CNW_FRAGMENT_HEADER_BITS);
    payload.truncate(declared);
    payload.extend_from_slice(&repacked);

    let claim = super::claim_payload_if_verified(&payload).expect(
        "HG I/0x2E00 0xFFFFFFEC inventory with false 0x0800 interleaved tail should claim exactly",
    );

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 22);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.records_examined, 6);
}

#[test]
fn hg_inventory_2e00_gui_hand_trap_interleaved_tail_promotes_before_gq() {
    // Live HG 2026-05-17 exposed this precise area-entry sibling:
    //
    //   I FFFFFFEC mask=0x2E00
    //     0x0400 clear/set one slot
    //     0x0200 false second branch + DWORD zero
    //     0x2000 second object-list count five
    //     0x0800 false
    //     twelve bytes of adjacent CNW fragment storage
    //   G Q ...
    //
    // Diamond `sub_455940` and EE `sub_1407B4F70` both read the 0x0800
    // fixed 12-byte branch only when its BOOL is true. Therefore the false
    // branch owns no read bytes; the twelve bytes immediately before `GQ`
    // must be promoted to the fragment bitstream before strict validation.
    let mut live = Vec::new();
    live.extend_from_slice(&[
        b'I', 0xEC, 0xFF, 0xFF, 0xFF, 0x00, 0x2E, 0x01, 0x6B, 0x01, 0x6B, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xEC, 0xFF, 0xFF, 0xFF, 0xF4, 0x71, 0x01,
        0x80, 0xE7, 0x71, 0x01, 0x80, 0xE2, 0x71, 0x01, 0x80, 0xFB, 0x71, 0x01, 0x80,
    ]);
    live.extend_from_slice(&[
        0x0E, 0x16, 0x14, 0x12, 0x40, 0x0E, 0x1E, 0x26, 0x24, 0x22, 0x50, 0x1E,
    ]);
    live.extend_from_slice(&[b'G', b'Q', 0x00]);

    let declared = super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len();
    let mut payload = vec![
        b'P',
        super::GAME_OBJECT_UPDATE_MAJOR,
        super::LIVE_OBJECT_MINOR,
    ];
    payload.extend_from_slice(&(declared as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false; super::CNW_FRAGMENT_HEADER_BITS],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw interleaved fragment storage must not claim before promotion"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("inventory-owned interleaved fragment storage should be promoted");
    assert_eq!(rewrite.records_examined, 2);
    assert_eq!(rewrite.interleaved_fragment_spans_promoted, 1);
    assert_eq!(rewrite.interleaved_fragment_bytes_promoted, 12);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("promoted hand-trap inventory plus GQ stream should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 19);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.records_examined, 2);
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

    let inventory_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
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
        rewrite.bytes_inserted >= 8,
        "legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("captured Starcore5 0x2E01 inventory/GQ live-object stream should claim exactly after rewrite");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 13);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
}

#[test]
fn hg_docks_inventory_2e01_missing_feature25_second_count_rewrites_before_claim() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_docks_seq36_inventory_2e01_missing_feature25_second_count_len454.bin"
    );
    assert!(
        super::claim_payload_if_verified(payload).is_none(),
        "raw legacy I/0x2E01 missing Feature-25 second_count must not claim as EE-shaped"
    );

    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let live = &payload[7..declared];
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();

    let inventory_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        live,
        0,
        live.len(),
    );
    assert_eq!(
        inventory_end, 66,
        "raw HG I/0x2E01 quickbar-link record should split before the following GQ rows"
    );

    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    let inventory_claim = super::inventory::advance_verified_inventory_record(
        live,
        0,
        inventory_end,
        &fragment_bits,
        &mut bit_cursor,
    )
    .expect("captured I/0x2E01 quickbar-link inventory record should claim exactly");
    assert_eq!(inventory_claim.fragment_bits, 20);
    assert_eq!(bit_cursor, super::CNW_FRAGMENT_HEADER_BITS + 20);

    let mut rewritten = payload.to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("captured Docks live-object stream should rewrite before strict claim");
    assert!(
        rewrite.bytes_inserted >= 8,
        "the following legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("rewritten Docks live-object stream should claim with exact family validators");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 20);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_update_records, 1);
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
        rewrite.bytes_inserted >= 8,
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
        .expect(
            "Sooty Crow live-object span should prove the final CNW fragment tail at offset 604",
        );
    rewritten[3..7].copy_from_slice(&repair.new_declared.to_le_bytes());

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("Sooty Crow I/0x2E01 extended live-object span should have a focused rewrite");

    assert!(
        rewrite.bytes_inserted >= 8,
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
        let _ =
            crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
                &mut candidate,
                None,
            );
        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);
        let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut candidate);
        let _ =
            crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
                &mut candidate,
                None,
            );
        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);

        if let Some(claim) = super::claim_payload_if_verified(&candidate) {
            accepted = Some((repair, claim));
            break;
        }
    }

    let (repair, claim) = accepted.expect(
        "current Starcore5 Sooty Crow U/5 visual-transform span should repair and claim exactly",
    );
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

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload).expect(
        "second live Starcore5 GUI inventory item-create stream should have a focused rewrite",
    );
    assert!(
        rewrite.bits_inserted > 0 || rewrite.bytes_inserted > 0,
        "legacy GUI item-create rows should receive EE item-create extras"
    );

    let claim = super::claim_payload_if_verified(&payload).expect(
        "rewritten second live Starcore5 GUI inventory item-create stream should claim exactly",
    );
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

#[test]
fn local_diamond_seq15_coalesced_liveobject_burst_rewrites_and_claims_exactly() {
    // Local Diamond bridge capture from 2026-05-16, quarantined from coalesced
    // server sequence 15 as an unclaimed `GameObjUpdate_LiveObject` payload.
    // The first live-object record is a legacy `A/5` creature add followed by
    // creature appearance/update records.  This fixture keeps the coalesced
    // path honest: transport repair alone must not own it, and the payload may
    // be emitted only after the typed live-object add/update translators make
    // the stream match exact EE reader shape.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq15_coalesced_liveobject_20260516_unclaimed.bin"
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
        .expect("local Diamond seq15 coalesced live-object burst should claim exactly");

    assert!(claim.add_records >= 1);
    assert!(claim.creature_appearance_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn local_diamond_seq15_compact_creature_ids_canonicalize_to_ee_external_namespace() {
    // EE creature add (`sub_14077F870`) calls
    // `CGameObjectArray::AddExternalObject(&id, creature, ...)`, which stores
    // the object in the external high-bit namespace. EE creature appearance
    // (`sub_14077FE10`) then resolves the appearance `OBJECTID` through that
    // object array before consuming the body. The local Diamond seq15 capture
    // used compact creature id `0x000000FE` for both `A/5` and later `P/5`
    // records; leaving that compact id in the EE-facing stream reproduces the
    // client log warning `HandleServerToPlayerCreatureUpdate_Appearance:
    // EXOWARNING: pCreature`.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq15_coalesced_liveobject_20260516_unclaimed.bin"
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

    let before = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq15 creature burst should be exact before id canonicalization");
    assert!(before.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x0000_00FE
    }));
    assert!(before.mentions.iter().any(|mention| {
        mention.opcode == b'P'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x0000_00FE
    }));

    let summary = super::canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
        .expect("compact creature add/appearance ids should canonicalize for EE");
    assert_eq!(summary.compact_add_ids_observed, 1);
    assert_eq!(summary.add_ids_rewritten, 1);
    assert!(
        summary.reference_ids_rewritten >= 1,
        "at least the P/5 appearance reference must be rewritten"
    );

    let after = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("canonicalized seq15 creature burst should remain exact and lifecycle-safe");
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x8000_00FE
    }));
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'P'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x8000_00FE
    }));
    assert!(!after.mentions.iter().any(|mention| {
        matches!(mention.opcode, b'A' | b'P' | b'U' | b'D')
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x0000_00FE
    }));
}

#[test]
fn local_diamond_seq15_creature_ids_can_use_playerlist_proven_session_alias() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq15_coalesced_liveobject_20260516_unclaimed.bin"
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

    super::canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
        .expect("generic compact canonicalization documents the pre-session state");

    let summary = super::canonicalize_player_session_creature_ids_payload_for_ee(
        &mut payload,
        |compact_id| (compact_id == 0xFE).then_some(0xFFFF_FFFE),
    )
    .expect("PlayerList-proven compact creature alias should rewrite the add/appearance pair");
    assert_eq!(summary.compact_add_ids_observed, 1);
    assert_eq!(summary.add_ids_rewritten, 1);
    assert!(
        summary.reference_ids_rewritten >= 1,
        "at least the P/5 appearance reference must be rewritten to the session id"
    );

    let after = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("session-canonicalized seq15 creature burst should remain exact");
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0xFFFF_FFFE
    }));
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'P'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0xFFFF_FFFE
    }));
    assert!(!after.mentions.iter().any(|mention| {
        matches!(mention.opcode, b'A' | b'P' | b'U' | b'D')
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x8000_00FE
    }));
}

#[test]
fn local_diamond_seq17_inventory_2a00_requires_exact_branch_bits() {
    // Local Diamond bridge capture from 2026-05-17 after the preceding
    // Diamond-only missing-object `U/5` update was removed. The remaining
    // `I/0x2A00` record is not raw passthrough: Diamond `sub_455940` and EE
    // `sub_1407B4F70` both read it as `0x0200 | 0x2000 | 0x0800`, with BOOL
    // branch bits choosing the read-buffer layout. The exact claim must
    // therefore prove those branch bits before the following `GQ` record can
    // be treated as the next live-object submessage.
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq17_inventory_2a00_20260517_rewritten.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("local Diamond seq17 I/0x2A00 + GQ stream should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);

    let fragment_offset = u32::from_le_bytes(payload[3..7].try_into().unwrap()) as usize;
    assert!(fragment_offset + 1 < payload.len());

    let mut wrong_0200_branch = payload.to_vec();
    wrong_0200_branch[fragment_offset] &= !0x08;
    assert!(
        super::claim_payload_if_verified(&wrong_0200_branch).is_none(),
        "0x0200 byte-mask shape must require its second BOOL branch bit"
    );

    let mut wrong_0800_branch = payload.to_vec();
    wrong_0800_branch[fragment_offset + 1] &= !0x80;
    assert!(
        super::claim_payload_if_verified(&wrong_0800_branch).is_none(),
        "0x0800 12-byte tail shape must require its present BOOL branch bit"
    );
}

#[test]
fn local_diamond_seq17_sentinel_inventory_owner_is_removed_with_missing_update() {
    // Same local Diamond 2026-05-17 direct M payload before lifecycle cleanup.
    // EE's inventory reader checks the resolved object pointer before consuming
    // the `0x2000` Feature-25 body, so the sentinel-owner `I/0x2A00` record
    // must be removed together with the preceding missing-object `U/5` record.
    // Leaving it in place reproduces EE's `Unknown Update sub-message` because
    // the reader bails before the following `GQ` bytes are consumed.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq17_liveobject_sentinel_inventory_20260517.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let rewrite =
        super::remove_unmaterialized_update_records_payload_if_possible(&mut payload, |_, _| false)
            .expect("sentinel U/I records should be removed after exact lifecycle proof");
    assert_eq!(rewrite.removed_update_records, 2);

    let claim = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("remaining GQ live-object record should claim after sentinel removals");
    assert_eq!(claim.inventory_records, 0);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn local_diamond_seq12_door_placeable_stream_claims_with_materialized_adds() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq12_door_placeable_stream_20260517_rewritten.bin"
    );

    let claim = super::claim_payload_if_verified_with_lifecycle(payload, |_, _| false)
        .expect("local Diamond seq12 door/placeable stream should claim after exact rewrite");

    assert_eq!(claim.add_records, 5);
    assert_eq!(claim.update_records, 5);
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::DOOR_OBJECT_TYPE
            && mention.object_id == 3
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_0006
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'U'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_000E
            && mention.requires_materialized_object
    }));
}

#[test]
fn local_diamond_seq12_compact_door_ids_canonicalize_to_ee_external_namespace() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq12_door_placeable_stream_20260517_rewritten.bin"
    )
    .to_vec();

    let summary = super::canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
        .expect("seq12 compact Diamond door ids should canonicalize for EE");

    assert_eq!(summary.compact_add_ids_observed, 3);
    assert_eq!(summary.add_ids_rewritten, 3);
    assert_eq!(summary.reference_ids_rewritten, 3);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("canonicalized payload remains exact EE live-object shape");
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A' && mention.object_type == 0x0A && mention.object_id == 0x8000_0003
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'U'
            && mention.object_type == 0x0A
            && mention.object_id == 0x8000_0003
            && mention.requires_materialized_object
    }));
    assert!(
        super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false).is_some(),
        "canonicalized add/update ids must lifecycle-proof within the same payload"
    );
}

#[test]
fn local_diamond_seq12_rebuilt_high_bit_door_placeable_stream_claims_exactly() {
    // Local Diamond bridge capture from 2026-05-18, dumped after the
    // live-object high-level fragment stream was rebuilt and canonicalized for
    // EE.  It is deliberately separate from the compact-id 2026-05-17 fixture:
    // this pins the emitted EE-facing external-object namespace shape.
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq12_door_placeable_stream_20260518_claimed.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("rebuilt local Diamond seq12 stream should claim as exact EE live-object shape");
    assert_eq!(claim.add_records, 5);
    assert_eq!(claim.update_records, 5);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::DOOR_OBJECT_TYPE
            && mention.object_id == 0x8000_0003
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_0006
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'U'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_000E
            && mention.requires_materialized_object
    }));

    let lifecycle_claim = super::claim_payload_if_verified_with_lifecycle(payload, |_, _| false)
        .expect("add/update pairs in the same rebuilt stream should lifecycle-proof internally");
    assert_eq!(lifecycle_claim.add_records, 5);
    assert_eq!(lifecycle_claim.update_records, 5);
}

#[test]
fn hg_live_empty_live_object_shell_claims_as_exact_noop() {
    // Live Higher Ground driver-only capture from 2026-05-17.  This is a
    // GameObjUpdate_LiveObject high-level packet whose declared length ends at
    // the CNW header and whose fragment stream contains only the three
    // final-bit header bits (`0x60` => valid bit count 3).  The EE client
    // decompile path (`MessageMoreDataToRead` before submessage dispatch)
    // treats this as no live-object work to read, so the strict translator
    // claims it as a typed semantic no-op instead of quarantining it as an
    // unverified lifecycle packet.
    let payload =
        include_bytes!("../../../fixtures/live_object/hg_live_empty_live_object_noop_20260517.bin");

    let claim =
        super::claim_payload_if_verified(payload).expect("empty live-object shell should claim");
    assert_eq!(claim.declared, 7);
    assert_eq!(claim.live_bytes_length, 0);
    assert_eq!(claim.fragment_bytes, 1);
    assert_eq!(claim.records_examined, 0);
    assert!(claim.mentions.is_empty());

    let lifecycle_claim = super::claim_payload_if_verified_with_lifecycle(payload, |_, _| false)
        .expect("empty no-op shell should not require object lifecycle state");
    assert_eq!(lifecycle_claim.records_examined, 0);
    assert!(lifecycle_claim.mentions.is_empty());
}

#[test]
fn hg_starc5_seq34_current_player_inventory_gq_stream_claims_exactly() {
    // Live HG Starcore5 2026-05-18 seq34: current-player inventory owner
    // 0xFFFFFFFE with mask 0x2E00, followed by a GQ row stream and P/U creature
    // records. Diamond's inventory traversal uses this current-player sentinel;
    // the proxy must classify it through the exact inventory/GQ parser instead
    // of waiting on retries or treating the zlib window as raw traffic.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq34_current_player_inventory_gq_20260518.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Diamond seq34 stream must not be claimed before typed rewrites"
    );

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
        .expect("HG seq34 current-player I/0x2E00 + GQ stream should rewrite to exact EE shape");

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 19);
    assert!(claim.live_gui_read_buffer_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}
