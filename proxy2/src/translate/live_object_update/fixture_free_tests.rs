//! Fixture-free live-object update regression anchors.

fn live_gui_character_sheet_payload(mask: u32, body: &[u8], owned_bits: Vec<bool>) -> Vec<u8> {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'G', b'S']);
    live.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    live.extend_from_slice(&mask.to_le_bytes());
    live.extend_from_slice(body);

    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);

    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    fragment_bits.extend(owned_bits);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

fn live_gui_read_buffer_payload(live: &[u8]) -> Vec<u8> {
    live_object_payload_with_bits(live, Vec::new())
}

fn live_object_payload_with_bits(live: &[u8], owned_bits: Vec<bool>) -> Vec<u8> {
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(live);
    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    fragment_bits.extend(owned_bits);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

fn push_msb_bits(bits: &mut Vec<bool>, value: u32, width: usize) {
    for shift in (0..width).rev() {
        bits.push(((value >> shift) & 1) != 0);
    }
}

fn creature_status_effect_4008_payload(rows: &[(u16, Option<&[u8]>)]) -> Vec<u8> {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x05, 0x55, 0x00, 0x00, 0x80]);
    live.extend_from_slice(&0x0000_4008u32.to_le_bytes());
    live.extend_from_slice(&(rows.len() as u16).to_le_bytes());
    for (row, target_payload) in rows {
        live.push(b'A');
        live.extend_from_slice(&row.to_le_bytes());
        if let Some(payload) = target_payload {
            live.extend_from_slice(payload);
        }
        live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    }

    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false; super::CNW_FRAGMENT_HEADER_BITS + 7],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

fn trigger_update_live_bytes(raw_mask: u32, tail: &[u8]) -> Vec<u8> {
    let mut live = vec![b'U', super::TRIGGER_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&raw_mask.to_le_bytes());
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    live.extend_from_slice(tail);
    live
}

fn door_state_update_live_bytes() -> Vec<u8> {
    let mut live = vec![b'U', super::DOOR_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_1234u32.to_le_bytes());
    live.extend_from_slice(&super::LEGACY_UPDATE_STATE_MASK.to_le_bytes());
    live
}

fn door_placeable_low_tail_update_live_bytes(object_type: u8, tail: &[u8]) -> Vec<u8> {
    let mut live = vec![b'U', object_type];
    live.extend_from_slice(&0x8000_3400u32.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_ORIENTATION_MASK
            | super::LEGACY_UPDATE_SCALE_STATE_MASK
            | super::LEGACY_UPDATE_STATE_MASK
            | super::LEGACY_UPDATE_APPEARANCE_MASK
            | super::LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // position
    live.push(0x70); // scalar orientation high byte
    live.extend_from_slice(&0x0042u16.to_le_bytes()); // appearance row
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live.extend_from_slice(tail);
    live
}

fn scalar_door_placeable_update_bits() -> Vec<bool> {
    vec![
        true, false, // position residual bits
        false, true, false, true, false, // scalar orientation selector + low bits
        true, false, true, false, true, // Diamond door/placeable state bits
    ]
}

#[test]
fn live_gui_inventory_update_row_is_ten_read_buffer_bytes() {
    // Diamond `sub_4589A0` and EE `sub_1407B3F30` read inventory `G I/i U` as
    // inner opcode, OBJECTID/INT32, SHORT, BYTE. Unlike repository `G R/r U`,
    // it does not own two trailing DWORDs or any CNW fragment bits.
    let mut live = vec![b'G', b'I', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x3344u16.to_le_bytes());
    live.push(0x55);

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("inventory GUI update row should exact-claim at ten bytes");

    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 10);
}

#[test]
fn live_gui_inventory_update_splits_before_following_gui_row() {
    let mut live = vec![b'G', b'i', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x3344u16.to_le_bytes());
    live.push(0x55);
    live.extend_from_slice(&[b'G', b'Q', 0]);

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("inventory GUI update should hand off before the following GQ row");

    assert_eq!(claim.live_gui_read_buffer_records, 2);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 13);
}

#[test]
fn live_gui_inventory_update_rejects_repository_width_tail() {
    let mut live = vec![b'G', b'I', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x3344u16.to_le_bytes());
    live.push(0x55);
    live.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);

    let payload = live_gui_read_buffer_payload(&live);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "inventory G I U must not swallow five repository-width tail bytes"
    );
}

#[test]
fn live_gui_inventory_delete_row_is_read_buffer_only() {
    // Diamond `sub_4589A0` / EE `sub_1407B3F30` dispatch `G I/i D` as a raw
    // OBJECTID delete row. It consumes seven read-buffer bytes and no fragment
    // BOOLs before handing off to the next GUI row.
    let mut live = vec![b'G', b'i', b'D'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&[b'G', b'Q', 0]);

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("inventory GUI delete should hand off before the following GQ row");

    assert_eq!(claim.live_gui_read_buffer_records, 2);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 10);
}

#[test]
fn live_gui_repository_update_remains_fifteen_read_buffer_bytes() {
    let mut live = vec![b'G', b'R', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x1122_3344u32.to_le_bytes());
    live.extend_from_slice(&0x5566_7788u32.to_le_bytes());

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("repository GUI update row should keep the wider exact shape");

    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 15);
}

#[test]
fn live_gui_repository_move_row_owns_two_slot_bytes_then_object_id() {
    // EE `sub_1407B4620` and Diamond's repository GUI reader consume `G R/r M`
    // as inner opcode, two BYTE slot/container fields, then OBJECTID. There is
    // no ReadBOOL between this row and a following GUI row.
    let mut live = vec![b'G', b'r', b'M', 0x04, 0x09];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&[b'G', b'Q', 0]);

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("repository move row should exact-claim before the following GQ row");

    assert_eq!(claim.live_gui_read_buffer_records, 2);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 12);
}

#[test]
fn live_gui_repository_move_rejects_object_id_at_update_offset() {
    let mut live = vec![b'G', b'R', b'M'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&[0xAA, 0xBB]);

    let payload = live_gui_read_buffer_payload(&live);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "repository move must not treat an object id at the update/delete cursor as a valid row"
    );
}

#[test]
fn legacy_door_state_update_rewrites_five_diamond_bools_to_ee_six_bool_shape() {
    // Diamond door update `sub_44E2C0` reads five state BOOLs for mask 0x10.
    // EE door update `sub_140797780` reads those same five in order, then one
    // EE-only neutral `sam` BOOL. The proxy may insert only that sixth bit.
    let live = door_state_update_live_bytes();
    let legacy_state_bits = vec![true, false, true, false, true];
    let mut payload = live_object_payload_with_bits(&live, legacy_state_bits.clone());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "five Diamond door state bits are not already an exact EE door update"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("door state update should insert EE's neutral sixth BOOL");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(rewrite.bits_removed, 0);
    assert_eq!(rewrite.bytes_inserted, 0);
    assert_eq!(rewrite.bytes_removed, 0);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten door state update should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.live_bytes_length, super::LEGACY_UPDATE_HEADER_BYTES);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("rewritten declared length") as usize;
    let fragment = &payload[declared..];
    let fragment_bits =
        super::bits::decode_msb_valid_bits(fragment, super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits should decode");
    let mut expected_state_bits = legacy_state_bits;
    expected_state_bits.push(false);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected_state_bits.as_slice(),
        "door state bits must keep Diamond order and append only the EE neutral bit"
    );
}

#[test]
fn ee_door_state_update_requires_neutral_sixth_bool_and_no_extra_bits() {
    let live = door_state_update_live_bytes();
    let exact_payload =
        live_object_payload_with_bits(&live, vec![true, false, true, false, true, false]);
    assert!(
        super::claim_payload_if_verified(&exact_payload).is_some(),
        "EE door state update owns exactly five legacy state BOOLs plus a false sixth BOOL"
    );

    let true_sixth_payload =
        live_object_payload_with_bits(&live, vec![true, false, true, false, true, true]);
    assert!(
        super::claim_payload_if_verified(&true_sixth_payload).is_none(),
        "the EE-only sixth door state BOOL must be neutral false"
    );

    let extra_bit_payload =
        live_object_payload_with_bits(&live, vec![true, false, true, false, true, false, false]);
    assert!(
        super::claim_payload_if_verified(&extra_bit_payload).is_none(),
        "a byte-complete door state update with an extra unowned fragment bit is not exact"
    );
}

#[test]
fn ee_door_placeable_update_rejects_low_tail_mask_bits() {
    // EE `sub_14079C050` plus the door/placeable-specific `sub_140797780`
    // readers own position, orientation, appearance, scale/state, and the
    // object state BOOLs. They have no 0x40/0x80 consumer, so an already-EE
    // byte shape with those low bits still set is not exact.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let live = door_placeable_low_tail_update_live_bytes(object_type, &[]);
        let mut bits = scalar_door_placeable_update_bits();
        bits.push(false); // EE-only neutral state BOOL.
        let payload = live_object_payload_with_bits(&live, bits);

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "object type {object_type:#04X} must reject Diamond-only 0x40/0x80 update mask bits"
        );
    }
}

#[test]
fn legacy_low_tail_door_placeable_updates_drop_only_bounded_control_suffix() {
    // Diamond `sub_467AE0` feeds the same shared generic update prefix for
    // doors/placeables, while the object-specific readers do not consume the
    // low 0x40/0x80 bits. The bridge may drop only the bounded WORD + mode
    // suffix after the prefix and append EE's neutral sixth state BOOL.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let live = door_placeable_low_tail_update_live_bytes(object_type, &[0x34, 0x12, 0, 0]);
        let mut payload = live_object_payload_with_bits(&live, scalar_door_placeable_update_bits());

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "legacy low-tail object type {object_type:#04X} is not already exact EE"
        );
        let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
            .expect("bounded low-tail door/placeable update should rewrite");

        assert_eq!(rewrite.update_records_rewritten, 1);
        assert_eq!(rewrite.masks_translated, 1);
        assert_eq!(rewrite.bytes_removed, 4);
        assert_eq!(rewrite.bits_inserted, 1);

        let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
            .expect("rewritten declared length") as usize;
        let live = &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared];
        assert_eq!(
            super::read_u32_le(live, 6),
            Some(
                super::LEGACY_UPDATE_POSITION_MASK
                    | super::LEGACY_UPDATE_ORIENTATION_MASK
                    | super::LEGACY_UPDATE_SCALE_STATE_MASK
                    | super::LEGACY_UPDATE_STATE_MASK
                    | super::LEGACY_UPDATE_APPEARANCE_MASK
            ),
            "Diamond-only low tail bits must be removed from the EE mask"
        );
        assert_eq!(
            live.len(),
            super::LEGACY_UPDATE_HEADER_BYTES
                + super::LEGACY_UPDATE_POSITION_READ_BYTES
                + super::EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES
                + super::EE_UPDATE_APPEARANCE_WORD_READ_BYTES
                + super::EE_UPDATE_SCALE_STATE_READ_BYTES,
            "only the bounded legacy control suffix should be removed"
        );

        let claim = super::claim_payload_if_verified(&payload)
            .expect("rewritten low-tail update should exact-claim");
        assert_eq!(claim.update_records, 1);
        let fragment_bits = super::bits::decode_msb_valid_bits(
            &payload[claim.declared..],
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .expect("rewritten fragment bits");
        assert_eq!(
            fragment_bits.len(),
            super::CNW_FRAGMENT_HEADER_BITS
                + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS
                + super::EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                + super::LEGACY_UPDATE_STATE_FRAGMENT_BITS
                + 1
        );
        assert!(
            !fragment_bits[fragment_bits.len() - 1],
            "EE's extra door/placeable state BOOL must be neutral false"
        );
    }
}

#[test]
fn legacy_low_tail_door_placeable_rewrite_requires_bounded_suffix() {
    let live = door_placeable_low_tail_update_live_bytes(super::DOOR_OBJECT_TYPE, &[0x34, 0x12, 0]);
    let mut payload = live_object_payload_with_bits(&live, scalar_door_placeable_update_bits());

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "three-byte 0x40/0x80 tail has no decompile-backed door/placeable reader boundary"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "malformed low-tail update must remain unclaimed"
    );
}

#[test]
fn legacy_trigger_update_tail_rewrites_to_position_only_exact_shape() {
    // Trigger updates use the shared Diamond/EE generic position cursor:
    // mask 0x0001 owns three WORD read-buffer fields plus two CNW fragment
    // bits. The observed legacy all-bits trigger row is accepted only with
    // its bounded three-byte legacy trigger tail, then collapsed to that EE
    // position-only shape.
    let live = trigger_update_live_bytes(0xFFFF_FFF3, &[0xAA, 0xBB, 0xCC]);
    let mut payload = live_object_payload_with_bits(&live, vec![true, false]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the all-bits legacy trigger tail is not already an exact EE update"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("bounded legacy trigger tail should rewrite to EE position-only");

    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.masks_translated, 1);
    assert_eq!(rewrite.bytes_removed, 3);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("rewritten declared length") as usize;
    let live = &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared];
    assert_eq!(live.len(), super::LEGACY_UPDATE_HEADER_BYTES + 6);
    assert_eq!(
        super::read_u32_le(live, 6),
        Some(super::LEGACY_UPDATE_POSITION_MASK)
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten trigger update should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(
        claim.live_bytes_length,
        super::LEGACY_UPDATE_HEADER_BYTES + 6
    );
}

#[test]
fn trigger_update_exact_shape_owns_only_position_fragment_bits() {
    let live = trigger_update_live_bytes(super::LEGACY_UPDATE_POSITION_MASK, &[]);
    let payload = live_object_payload_with_bits(&live, vec![false, true]);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("position-only trigger update should claim exactly");

    assert_eq!(claim.update_records, 1);
    assert_eq!(
        claim.live_bytes_length,
        super::LEGACY_UPDATE_HEADER_BYTES + 6
    );

    let extra_bit_payload = live_object_payload_with_bits(&live, vec![false, true, false]);
    assert!(
        super::claim_payload_if_verified(&extra_bit_payload).is_none(),
        "a byte-complete trigger update with an extra unowned fragment bit is not exact"
    );
}

#[test]
fn legacy_trigger_update_rewrite_requires_tail_and_position_bits() {
    let missing_tail = trigger_update_live_bytes(0xFFFF_FFF3, &[]);
    let mut missing_tail_payload = live_object_payload_with_bits(&missing_tail, vec![false, true]);
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut missing_tail_payload).is_none(),
        "legacy all-bits trigger updates must carry the bounded three-byte tail before rewrite"
    );

    let short_bits = trigger_update_live_bytes(0xFFFF_FFF3, &[0xAA, 0xBB, 0xCC]);
    let mut short_bits_payload = live_object_payload_with_bits(&short_bits, vec![true]);
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut short_bits_payload).is_none(),
        "the trigger position branch owns exactly two CNW fragment bits"
    );
}

#[test]
fn live_gui_character_sheet_effect_icons_word_ids_do_not_split_on_legacy_prefix() {
    // EE build 8193.37 widened character-sheet effect-icon counts and ids to
    // WORDs. The leading zero removed-count byte is also a valid legacy prefix,
    // so exact ownership must pick the full word-id branch before final cursor
    // validation.
    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0x1234u16.to_le_bytes());

    let payload = live_gui_character_sheet_payload(0x0000_0100, &body, vec![true]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("word-id character-sheet effect icons should exact-claim");
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 1);
    assert_eq!(claim.live_bytes_length, 16);
}

#[test]
fn live_gui_character_sheet_effect_icons_word_ids_require_changed_row_bool() {
    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0x1234u16.to_le_bytes());

    let payload = live_gui_character_sheet_payload(0x0000_0100, &body, Vec::new());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the changed effect-icon row owns one CNW BOOL after the WORD id"
    );
}

#[test]
fn live_gui_character_sheet_combat_build_8193_35_owns_five_bit_actions() {
    // EE build 8193.35 widened the second combat-info list action field from
    // four bits to five. The byte window is identical to the legacy shape, so
    // the candidate selector has to use final fragment ownership, not first
    // parse success.
    let mut body = Vec::new();
    let mut bits = Vec::new();

    push_msb_bits(&mut bits, 0, 3);
    body.extend_from_slice(&[0x11, 0x22, 0x33]);
    push_msb_bits(&mut bits, 0, 7);
    push_msb_bits(&mut bits, 0, 5);
    push_msb_bits(&mut bits, 0, 5);
    push_msb_bits(&mut bits, 0, 5);
    for row in 0..3 {
        push_msb_bits(&mut bits, 0, 5);
        push_msb_bits(&mut bits, 0, 5);
        body.push(0x40 + row);
    }
    push_msb_bits(&mut bits, 0, 4);
    push_msb_bits(&mut bits, 0, 3);
    bits.push(false);
    body.push(0);
    body.push(1);
    body.push(0x77);
    push_msb_bits(&mut bits, 0b1_0001, 5);
    push_msb_bits(&mut bits, 0b101, 3);
    bits.extend_from_slice(&[false, false, false]);

    let payload = live_gui_character_sheet_payload(0x0000_0040, &body, bits.clone());

    let claim = super::claim_payload_if_verified(&payload)
        .expect("five-bit character-sheet combat action should exact-claim");
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_fragment_bits, bits.len() as u32);
}

#[test]
fn live_gui_character_sheet_isolated_record_must_consume_all_fragment_bits() {
    // The EE character-sheet reader (`sub_1407B2740`) owns only the BOOLs
    // selected by the mask branches. With no following live-object boundary, a
    // byte-complete `G S` record that leaves an extra fragment bit is not an
    // exact isolated record and must stay unclaimed.
    let live = [b'G', b'S', 0xFE, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
    let fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS + 1];

    assert!(
        super::gui::try_get_verified_ee_live_gui_record_end(
            &live,
            0,
            live.len(),
            &fragment_bits,
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .is_none(),
        "isolated G S must not claim a max-consumed candidate that leaves fragment bits behind"
    );
}

#[test]
fn creature_status_effect_single_target_payload_is_exact_ee_shape() {
    let payload =
        creature_status_effect_4008_payload(&[(0x1234, Some(&[0x44, 0x33, 0x22, 0x80, 0x66]))]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("single target-payload status-effect row should be exact-owned");
    assert_eq!(claim.creature_update_records, 1);
}

#[test]
fn creature_status_effect_three_byte_target_payload_is_not_exact_ee_shape() {
    let payload = creature_status_effect_4008_payload(&[(0x1234, Some(&[0x44, 0x33, 0x22]))]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "EE sub_1407B1F00 and Diamond sub_44ED20 own DWORD target id plus one BYTE, not a three-byte target payload"
    );
}

#[test]
fn creature_status_effect_multi_target_payload_stays_unclaimed_without_2da() {
    let payload = creature_status_effect_4008_payload(&[
        (0x1234, Some(&[0x44, 0x33, 0x22, 0x80, 0x66])),
        (0x1235, Some(&[0x55, 0x44, 0x33, 0x80, 0x77])),
    ]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "without visualeffects.2da row-type proof, multi-row target payload boundaries are ambiguous"
    );
}

#[test]
fn creature_status_effect_mixed_target_payload_rows_stay_unclaimed_without_2da() {
    let payload = creature_status_effect_4008_payload(&[
        (0x00F3, None),
        (0x1234, Some(&[0x44, 0x33, 0x22, 0x80, 0x66])),
    ]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "without visualeffects.2da row-type proof, mixed target/no-target rows cannot be exact-owned"
    );
}

fn creature_update_with_adjacent_fragment_span(
    raw_mask: u32,
) -> (Vec<u8>, usize, Vec<bool>, Vec<u8>) {
    let mut live = vec![b'U', 0x05, 0x55, 0x00, 0x00, 0x80];
    live.extend_from_slice(&raw_mask.to_le_bytes());
    live.extend_from_slice(&[0; 6]); // 0x0001 position: WORD, WORD, WORD + 2 bits.
    live.push(0); // 0x0002 scalar orientation: one BYTE + four bits.
    live.extend_from_slice(&0u32.to_le_bytes()); // 0x0004 action scalar.
    live.extend_from_slice(&0u16.to_le_bytes()); // 0x0004 action code.
    if !matches!(raw_mask, 0x0000_C40F | 0x0000_C44F) {
        live.push(0); // 0x0004 action state byte.
        live.extend_from_slice(&0u16.to_le_bytes()); // 0x0004 action follow-up count.
    }
    if (raw_mask & 0x0000_0008) != 0 {
        live.extend_from_slice(&0u16.to_le_bytes()); // status-effect count.
    }
    if (raw_mask & 0x0000_0040) != 0 {
        live.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // WORD, BYTE, WORD, BYTE.
    }
    if (raw_mask & 0x0000_0400) != 0 {
        live.extend_from_slice(&[0; 8]); // 0x0400 four WORD scalar/status values.
    }
    let read_end = live.len();
    let span = super::bits::pack_msb_valid_bits(
        vec![false, false, false, true, false, true],
        super::CNW_FRAGMENT_HEADER_BITS,
    );
    live.extend_from_slice(&span);

    let current_record_bits = 2 // 0x0001 residual position bits.
        + 6 // 0x0002 scalar branch: selector, four scalar bits, target guard.
        + if (raw_mask & 0x0000_0040) != 0 { 1 } else { 0 }
        + if (raw_mask & 0x0000_4000) != 0 { 7 } else { 0 }
        + if (raw_mask & 0x0000_8000) != 0 { 3 } else { 0 };
    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS + current_record_bits];
    // Bit order at the decompiled C40F/C44F/8047 cursor:
    //   0x0001 owns two residual position bits,
    //   0x0002 owns scalar-orientation selector + one BYTE + four scalar bits,
    //   the optional orientation target guard can own one BOOL,
    //   0x0040 owns one state BOOL when present,
    //   0x4000 owns seven status BOOLs,
    //   0x8000 owns three self-visibility BOOLs.
    // The first scalar bit is true so starting one bit late makes the
    // orientation selector look like the vector branch and fail exact proof.
    fragment_bits[super::CNW_FRAGMENT_HEADER_BITS + 3] = true;

    (live, read_end, fragment_bits, span)
}

#[test]
fn creature_interleaved_fragment_span_requires_exact_bit_cursor() {
    for (raw_mask, name) in [
        (0x0000_C40F, "C40F"),
        (0x0000_C44F, "C44F"),
        (0x0000_8047, "8047"),
    ] {
        let (mut live, read_end, mut fragment_bits, span) =
            creature_update_with_adjacent_fragment_span(raw_mask);
        let old_record_end = live.len();
        let mut record_end = old_record_end;

        let shifted_cursor = super::CNW_FRAGMENT_HEADER_BITS + 1;
        assert!(
            super::fragment_spans::promote_creature_update_interleaved_fragment_span_for_ee(
                &mut live,
                &mut fragment_bits,
                0,
                &mut record_end,
                shifted_cursor,
            )
            .is_none(),
            "{name} span promoter must not retry at a neighboring fragment cursor"
        );
        assert_eq!(record_end, old_record_end, "{name} record_end changed");
        assert_eq!(live.len(), old_record_end, "{name} live bytes changed");
        assert_eq!(read_end + span.len(), old_record_end, "{name} span length");
    }
}

#[test]
fn creature_interleaved_fragment_span_promotes_from_exact_bit_cursor() {
    for (raw_mask, name) in [
        (0x0000_C40F, "C40F"),
        (0x0000_C44F, "C44F"),
        (0x0000_8047, "8047"),
    ] {
        let (mut live, read_end, mut fragment_bits, span) =
            creature_update_with_adjacent_fragment_span(raw_mask);
        let old_record_end = live.len();
        let mut record_end = old_record_end;

        let promoted =
            super::fragment_spans::promote_creature_update_interleaved_fragment_span_for_ee(
                &mut live,
                &mut fragment_bits,
                0,
                &mut record_end,
                super::CNW_FRAGMENT_HEADER_BITS,
            )
            .unwrap_or_else(|| {
                panic!("{name} span should promote from the exact inherited bit cursor")
            });

        assert_eq!(promoted.read_end, read_end, "{name} read_end");
        assert_eq!(
            promoted.old_record_end, old_record_end,
            "{name} old_record_end"
        );
        assert_eq!(promoted.bytes_promoted, span.len(), "{name} span bytes");
        assert_eq!(promoted.bits_promoted, 3, "{name} payload bits");
        assert_eq!(record_end, read_end, "{name} record_end");
        assert_eq!(live.len(), read_end, "{name} live length");
    }
}
