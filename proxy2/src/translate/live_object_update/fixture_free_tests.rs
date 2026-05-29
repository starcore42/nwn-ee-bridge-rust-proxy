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

fn legacy_zero_count_creature_4408_payload(
    rows: &[(u8, u16)],
    extra_before_scalar: &[u8],
) -> Vec<u8> {
    legacy_zero_count_creature_4408_payload_with_bits(rows, extra_before_scalar, vec![false; 7])
}

fn legacy_zero_count_creature_4408_payload_with_bits(
    rows: &[(u8, u16)],
    extra_before_scalar: &[u8],
    owned_bits: Vec<bool>,
) -> Vec<u8> {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x05, 0x55, 0x00, 0x00, 0x80]);
    live.extend_from_slice(&0x0000_4408u32.to_le_bytes());
    live.extend_from_slice(&0u16.to_le_bytes());
    for (opcode, row) in rows {
        live.push(*opcode);
        live.extend_from_slice(&row.to_le_bytes());
    }
    live.extend_from_slice(extra_before_scalar);
    live.extend_from_slice(&[0; 8]);
    live_object_payload_with_bits(&live, owned_bits)
}

fn trigger_update_live_bytes(raw_mask: u32, tail: &[u8]) -> Vec<u8> {
    let mut live = vec![b'U', super::TRIGGER_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&raw_mask.to_le_bytes());
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    live.extend_from_slice(tail);
    live
}

fn item_update_name_live_bytes(raw_mask: u32, name: &[u8]) -> Vec<u8> {
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&raw_mask.to_le_bytes());
    live.extend_from_slice(&(name.len() as u32).to_le_bytes());
    live.extend_from_slice(name);
    live
}

fn inventory_2a00_word_list_live_bytes(
    word_entries: &[u16],
    feature25_second_ids: &[u32],
    tail_0800: Option<[u8; 12]>,
) -> Vec<u8> {
    let mut live = vec![b'I'];
    live.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    live.extend_from_slice(&0x2A00u16.to_le_bytes());
    live.extend_from_slice(&(word_entries.len() as u32).to_le_bytes());
    for entry in word_entries {
        live.extend_from_slice(&entry.to_le_bytes());
    }
    live.extend_from_slice(&0u32.to_le_bytes());
    live.extend_from_slice(&(feature25_second_ids.len() as u32).to_le_bytes());
    for object_id in feature25_second_ids {
        live.extend_from_slice(&object_id.to_le_bytes());
    }
    if let Some(tail) = tail_0800 {
        live.extend_from_slice(&tail);
    }
    live
}

fn trigger_add_live_bytes(vertex_count: u8) -> Vec<u8> {
    let mut live = vec![b'A', super::TRIGGER_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&[0; 9]);
    live.push(vertex_count);
    for index in 0..vertex_count {
        let base = f32::from(index) + 1.0;
        live.extend_from_slice(&base.to_le_bytes());
        live.extend_from_slice(&(base + 0.25).to_le_bytes());
        live.extend_from_slice(&(base + 0.5).to_le_bytes());
    }
    live
}

fn top_level_model_type2_token_name_item_add_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A'];
    live.extend_from_slice(&0x8001_69DCu32.to_le_bytes());
    live.extend_from_slice(&0x10u32.to_le_bytes());
    live.extend_from_slice(&0x01u32.to_le_bytes()); // base item with model type 2.
    for part in [0x17u16, 0x3Fu16, 0x17u16] {
        live.extend_from_slice(&part.to_le_bytes());
    }
    live.push(0);
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    live.extend_from_slice(&0x0000_380Au32.to_le_bytes()); // token-shaped item name.
    live.extend_from_slice(&0x0000_0670u32.to_le_bytes());
    live.extend_from_slice(&1u32.to_le_bytes());
    live.extend_from_slice(&[0, 0, 0xFF]);
    live.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    live
}

fn ee_door_add_with_inline_name_live_bytes(name: &[u8]) -> Vec<u8> {
    let mut live = vec![b'A', super::DOOR_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_3300u32.to_le_bytes());
    live.extend_from_slice(&1u32.to_le_bytes());
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    live.extend_from_slice(&(name.len() as u32).to_le_bytes());
    live.extend_from_slice(name);
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live
}

fn door_placeable_state_update_live_bytes(object_type: u8) -> Vec<u8> {
    let mut live = vec![b'U', object_type];
    live.extend_from_slice(&0x8000_1234u32.to_le_bytes());
    live.extend_from_slice(&super::LEGACY_UPDATE_STATE_MASK.to_le_bytes());
    live
}

fn door_state_update_live_bytes() -> Vec<u8> {
    door_placeable_state_update_live_bytes(super::DOOR_OBJECT_TYPE)
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

fn door_placeable_full_update_live_bytes(object_type: u8) -> Vec<u8> {
    let mut live = vec![b'U', object_type];
    live.extend_from_slice(&0x8000_3400u32.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_ORIENTATION_MASK
            | super::LEGACY_UPDATE_SCALE_STATE_MASK
            | super::LEGACY_UPDATE_STATE_MASK
            | super::LEGACY_UPDATE_APPEARANCE_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // position
    live.extend_from_slice(&0x0070u16.to_le_bytes()); // scalar orientation
    live.extend_from_slice(&0x0042u16.to_le_bytes()); // appearance row
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live
}

fn ee_door_placeable_full_update_live_bytes(object_type: u8) -> Vec<u8> {
    let mut live = vec![b'U', object_type];
    live.extend_from_slice(&0x8000_3400u32.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_ORIENTATION_MASK
            | super::LEGACY_UPDATE_SCALE_STATE_MASK
            | super::LEGACY_UPDATE_STATE_MASK
            | super::LEGACY_UPDATE_APPEARANCE_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // position
    live.push(0x70); // scalar orientation high byte
    live.extend_from_slice(&0x0042u16.to_le_bytes()); // appearance row
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live
}

fn scale_first_door_placeable_full_update_live_bytes(object_type: u8) -> Vec<u8> {
    let mut live = vec![b'U', object_type];
    live.extend_from_slice(&0x8000_3400u32.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_ORIENTATION_MASK
            | super::LEGACY_UPDATE_SCALE_STATE_MASK
            | super::LEGACY_UPDATE_STATE_MASK
            | super::LEGACY_UPDATE_APPEARANCE_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // position
    live.push(0x70); // scalar orientation high byte
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live.extend_from_slice(&0x0042u16.to_le_bytes()); // appearance row
    live
}

fn stale_absent_appearance_gap_door_placeable_update_live_bytes(object_type: u8) -> Vec<u8> {
    let mut live = vec![b'U', object_type];
    live.extend_from_slice(&0x8000_3400u32.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_ORIENTATION_MASK
            | super::LEGACY_UPDATE_SCALE_STATE_MASK
            | super::LEGACY_UPDATE_STATE_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // position
    live.push(0x70); // scalar orientation high byte
    live.extend_from_slice(&0x0076u16.to_le_bytes()); // stale absent-appearance gap
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live
}

fn door_placeable_named_low_tail_update_live_bytes(object_type: u8, name: &[u8]) -> Vec<u8> {
    let mut live = vec![b'U', object_type];
    live.extend_from_slice(&0x8000_3400u32.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_STATE_MASK
            | super::LEGACY_UPDATE_NAME_MASK
            | super::LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // position
    live.extend_from_slice(&(name.len() as u32).to_le_bytes());
    live.extend_from_slice(name);
    live
}

fn scalar_door_placeable_update_bits() -> Vec<bool> {
    vec![
        true, false, // position residual bits
        false, true, false, true, false, // scalar orientation selector + low bits
        true, false, true, false, true, // Diamond door/placeable state bits
    ]
}

fn exact_scalar_door_placeable_update_bits() -> Vec<bool> {
    let mut bits = scalar_door_placeable_update_bits();
    bits.push(false); // EE-only neutral door/placeable state BOOL.
    bits
}

#[test]
fn work_remaining_record_is_three_read_buffer_bytes_and_fragment_neutral() {
    // Diamond `sub_44F160` and EE `sub_1407B85A0` both read only the top-level
    // `W` opcode plus two BYTE counters, and no CNW fragment BOOLs.
    let live = [b'W', 0x02, 0x0E];
    let payload = live_object_payload_with_bits(&live, Vec::new());

    let claim = super::claim_payload_if_verified(&payload)
        .expect("work-remaining should exact-claim as a three-byte identity record");

    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());

    let shifted = live_object_payload_with_bits(&live, vec![true]);
    assert!(
        super::claim_payload_if_verified(&shifted).is_none(),
        "a work-remaining record must not consume or hide a following fragment bit"
    );
}

#[test]
fn work_remaining_record_accepts_general_counter_bytes() {
    // The reader contract is `W current total`, not the observed `W xx 0E`
    // packet family from local transition captures. Both counter bytes are
    // read-buffer BYTEs, and neither is a CNW fragment cursor guard.
    let live = [b'W', 0x10, 0x20];
    let payload = live_object_payload_with_bits(&live, Vec::new());

    let claim = super::claim_payload_if_verified(&payload)
        .expect("work-remaining must exact-claim with arbitrary counter bytes");

    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
}

#[test]
fn work_remaining_does_not_supply_missing_update_fragment_bits() {
    // The CEP v2.3 starter evidence reduces to this cursor rule: `W current
    // total` is not a fragment-storage donor for an adjacent `U/9`/`U/10`.
    // Diamond `sub_44F160` and EE `sub_1407B85A0` read only three bytes, while
    // the preceding generic door/placeable update must own its own position,
    // orientation, and state BOOLs before any following record boundary.
    let mut live = door_placeable_full_update_live_bytes(super::PLACEABLE_OBJECT_TYPE);
    live.extend_from_slice(&[b'W', 0x0C, 0x0E]);
    let mut payload = live_object_payload_with_bits(
        &live,
        vec![
            true, false, // only the position residual bits are available
        ],
    );
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "a following W row must not make a bit-short placeable update exact"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "the update pass must not borrow missing U/9 bits from W"
    );
    assert_eq!(
        payload, original,
        "bit-short U/9 before W must stay visible for quarantine/diagnostics"
    );
}

#[test]
fn work_remaining_does_not_rescue_shifted_door_placeable_37_rows() {
    // `W current total` is a fragment-neutral suffix. It can follow a
    // fully-proven update record, but it cannot turn a shifted U/9 or U/10
    // cursor into an owned family tail. The preceding record must still follow
    // Diamond `sub_467AE0` / EE `sub_14079C050`: appearance before scale/state,
    // and all position/orientation/state fragment BOOLs present before `W`.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let mut shifted_live = scale_first_door_placeable_full_update_live_bytes(object_type);
        shifted_live.extend_from_slice(&[b'W', 0x0C, 0x0E]);
        let mut shifted_payload =
            live_object_payload_with_bits(&shifted_live, exact_scalar_door_placeable_update_bits());
        let original_shifted = shifted_payload.clone();

        assert!(
            super::claim_payload_if_verified(&shifted_payload).is_none(),
            "object type {object_type:#04X} must reject scale/state before appearance even when W follows"
        );
        assert!(
            super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
            "the update pass must not use W as a shifted 0x37 row owner"
        );
        assert_eq!(
            shifted_payload, original_shifted,
            "shifted U/9 or U/10 before W must stay visible for quarantine/diagnostics"
        );

        let mut bit_short_live = ee_door_placeable_full_update_live_bytes(object_type);
        bit_short_live.extend_from_slice(&[b'W', 0x0C, 0x0E]);
        let mut bit_short_payload = live_object_payload_with_bits(
            &bit_short_live,
            vec![
                true, false, // only the position residual bits are available
            ],
        );
        let original_bit_short = bit_short_payload.clone();

        assert!(
            super::claim_payload_if_verified(&bit_short_payload).is_none(),
            "object type {object_type:#04X} must own its orientation/state bits before W"
        );
        assert!(
            super::rewrite_update_records_payload_if_possible(&mut bit_short_payload).is_none(),
            "the update pass must not borrow missing 0x37 fragment bits from W"
        );
        assert_eq!(
            bit_short_payload, original_bit_short,
            "bit-short U/9 or U/10 before W must stay visible for quarantine/diagnostics"
        );
    }
}

#[test]
fn exact_adapter_rolls_back_prior_rewrites_before_unproven_update_w_handoff() {
    // A bounded live-object adapter may stage earlier typed rewrites while it
    // searches for an exact final EE claim. If a later U/9-W handoff lacks the
    // decompile-owned update bits, the whole staged candidate must be discarded
    // instead of emitting a partially translated stream.
    let mut live = door_state_update_live_bytes();
    live.extend_from_slice(&door_placeable_full_update_live_bytes(
        super::PLACEABLE_OBJECT_TYPE,
    ));
    live.extend_from_slice(&[b'W', 0x0C, 0x0E]);
    let mut payload = live_object_payload_with_bits(
        &live,
        vec![
            true, false, true, false, true, // first door state update
            true, false, // second U/9 has only position residual bits
        ],
    );
    let original = payload.clone();

    assert!(
        !crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
            &mut payload,
            None
        ),
        "exact adapter must reject the stream until the U/9-W cursor owner is proven"
    );
    assert_eq!(
        payload, original,
        "failed exact live-object rewrite must roll back earlier staged update edits"
    );
}

#[test]
fn work_remaining_identity_pass_does_not_trim_unproven_read_tail() {
    // A byte grouped after `W current total` is not part of the Diamond/EE
    // work-remaining reader. The identity pass must leave it unclaimed unless
    // the explicit post-`W` fragment-span promoter proves a bounded CNW
    // fragment-storage stream.
    let live = [b'W', 0x02, 0x0E, 0x60];
    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "extra read-buffer bytes after W must not exact-claim"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "the update pass must not silently trim an unproven post-W byte"
    );
    assert_eq!(
        payload, original,
        "unproven post-W read bytes must stay visible for quarantine/diagnostics"
    );
}

#[test]
fn work_remaining_midstream_fragment_storage_requires_top_level_following_boundary_proof() {
    // Local transition captures can carry a bounded CNW fragment-storage byte
    // after a `W` row and before the next real live-object submessage. That is
    // not a `W` payload; it may be removed only while the top-level boundary
    // loop is sitting on the `W` row and the following live-object boundary is
    // explicit. The final exact claim below proves the emitted stream.
    let live = [b'W', 0x01, 0x0E, 0xA0, b'W', 0x02, 0x0E];
    let mut payload = live_object_payload_with_bits(&live, Vec::new());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the duplicate fragment-storage byte must block the raw exact claim"
    );
    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("bounded midstream post-W fragment storage should be removed");
    assert_eq!(summary.bytes_removed, 1);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("the post-W collision repair must leave an exact EE stream");
    assert_eq!(claim.world_status_records, 2);
}

#[test]
fn work_remaining_terminal_fragment_storage_requires_cnw_shape_and_final_exact_proof() {
    let live = [b'W', 0x10, 0x20, 0xA0];
    let mut payload = live_object_payload_with_bits(&live, Vec::new());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "terminal fragment-storage bytes after W must block the raw exact claim"
    );
    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("bounded terminal post-W fragment storage should be removed");
    assert_eq!(summary.bytes_removed, 1);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("terminal post-W collision repair must leave an exact EE stream");
    assert_eq!(claim.world_status_records, 1);
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
fn live_object_delete_records_own_exact_fragment_bits() {
    for object_type in [
        super::CREATURE_OBJECT_TYPE,
        super::ITEM_OBJECT_TYPE,
        super::PLACEABLE_OBJECT_TYPE,
    ] {
        let live = [b'D', object_type, 0x22, 0x00, 0x00, 0x80];
        let payload = live_object_payload_with_bits(&live, vec![true]);
        let claim = super::claim_payload_if_verified(&payload)
            .expect("delete record with one owned BOOL should exact-claim");
        assert_eq!(claim.delete_records, 1);

        let missing_bit = live_object_payload_with_bits(&live, Vec::new());
        assert!(
            super::claim_payload_if_verified(&missing_bit).is_none(),
            "D/{object_type:#04X} must not claim without its decompiled delete BOOL"
        );

        let extra_bit = live_object_payload_with_bits(&live, vec![true, false]);
        assert!(
            super::claim_payload_if_verified(&extra_bit).is_none(),
            "D/{object_type:#04X} must not hide a terminal extra fragment bit"
        );
    }

    for object_type in [super::TRIGGER_OBJECT_TYPE, super::DOOR_OBJECT_TYPE] {
        let live = [b'D', object_type, 0x22, 0x00, 0x00, 0x80];
        let payload = live_object_payload_with_bits(&live, Vec::new());
        let claim = super::claim_payload_if_verified(&payload)
            .expect("read-buffer-only delete record should exact-claim");
        assert_eq!(claim.delete_records, 1);

        let extra_bit = live_object_payload_with_bits(&live, vec![true]);
        assert!(
            super::claim_payload_if_verified(&extra_bit).is_none(),
            "D/{object_type:#04X} must remain read-buffer-only and reject fragment residue"
        );
    }
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
fn legacy_door_placeable_state_update_rewrite_rejects_terminal_extra_fragment_bit() {
    // Diamond door/placeable state updates own exactly five state BOOLs in
    // `sub_44E2C0`/the matching placeable reader. EE consumes those same five
    // plus one neutral object-specific BOOL. No terminal reader owns a seventh
    // bit, so the top-level live-object trim gate must not hide it after the
    // state rewrite inserts EE's neutral branch.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let live = door_placeable_state_update_live_bytes(object_type);
        let mut payload =
            live_object_payload_with_bits(&live, vec![true, false, true, false, true, true]);
        let original = payload.clone();

        assert!(
            super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
            "terminal state-only object type {object_type:#04X} must not trim an unowned bit"
        );
        assert_eq!(
            payload, original,
            "rejected terminal state-only repair must leave the source payload untouched"
        );
        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "terminal state-only residual bits must remain unclaimed"
        );
    }
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
fn ee_door_placeable_update_37_requires_appearance_before_scale_state() {
    // Diamond `sub_467AE0` and EE `sub_14079C050` both read the 0x20
    // appearance field before the 0x4 scale/state pair. The two fields have
    // the same byte total as scale/state plus a WORD appearance, so byte-end
    // validation alone is not enough proof of the reader cursor.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let live = ee_door_placeable_full_update_live_bytes(object_type);
        let payload =
            live_object_payload_with_bits(&live, exact_scalar_door_placeable_update_bits());
        let claim = super::claim_payload_if_verified(&payload)
            .expect("EE-ordered door/placeable 0x37 update should exact-claim");

        assert_eq!(claim.update_records, 1);
        assert_eq!(claim.live_bytes_length, live.len());

        let shifted = scale_first_door_placeable_full_update_live_bytes(object_type);
        assert_eq!(
            shifted.len(),
            live.len(),
            "the swapped field order is a same-length cursor hazard"
        );
        let mut shifted_payload =
            live_object_payload_with_bits(&shifted, exact_scalar_door_placeable_update_bits());
        let original = shifted_payload.clone();

        assert!(
            super::claim_payload_if_verified(&shifted_payload).is_none(),
            "object type {object_type:#04X} must reject scale/state before appearance"
        );
        assert!(
            super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
            "object type {object_type:#04X} must not rewrite a same-length shifted EE row"
        );
        assert_eq!(
            shifted_payload, original,
            "rejected shifted 0x37 row must stay visible for quarantine/diagnostics"
        );
    }
}

#[test]
fn stale_absent_appearance_gap_repair_rejects_terminal_extra_fragment_bit() {
    // The legacy `mask=0x17` gap repair removes two stale bytes at the exact
    // appearance cursor only after proving that Diamond/EE scale-state lands
    // after the gap. That does not make a later terminal fragment bit owned by
    // the door/placeable family; the decompiled readers still consume only
    // position, scalar orientation, five legacy state BOOLs, and EE's neutral
    // sixth state BOOL.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let live = stale_absent_appearance_gap_door_placeable_update_live_bytes(object_type);
        let mut exact_payload =
            live_object_payload_with_bits(&live, scalar_door_placeable_update_bits());
        let rewrite = super::rewrite_update_records_payload_if_possible(&mut exact_payload)
            .expect("bounded stale absent-appearance gap should rewrite");
        assert_eq!(rewrite.update_records_rewritten, 1);
        assert_eq!(rewrite.bytes_removed, 2);
        assert_eq!(rewrite.bits_inserted, 1);
        let claim = super::claim_payload_if_verified(&exact_payload)
            .expect("rewritten stale absent-appearance gap should exact-claim");
        assert_eq!(claim.update_records, 1);

        let mut shifted_bits = scalar_door_placeable_update_bits();
        shifted_bits.push(true);
        let mut shifted_payload = live_object_payload_with_bits(&live, shifted_bits);
        let original = shifted_payload.clone();
        assert!(
            super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
            "terminal stale absent-appearance object type {object_type:#04X} must not trim an unowned bit"
        );
        assert_eq!(
            shifted_payload, original,
            "rejected terminal stale absent-appearance repair must leave the source payload untouched"
        );
        assert!(
            super::claim_payload_if_verified(&shifted_payload).is_none(),
            "terminal stale absent-appearance residual bits must remain unclaimed"
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
fn legacy_low_tail_door_placeable_rewrite_rejects_terminal_extra_fragment_bit() {
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let live = door_placeable_low_tail_update_live_bytes(object_type, &[0x34, 0x12, 0, 0]);
        let mut bits = scalar_door_placeable_update_bits();
        bits.push(true);
        let mut payload = live_object_payload_with_bits(&live, bits);
        let original = payload.clone();

        assert!(
            super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
            "terminal low-tail object type {object_type:#04X} must not trim an unowned fragment bit"
        );
        assert_eq!(
            payload, original,
            "rejected terminal low-tail repair must leave the source payload untouched"
        );
        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "terminal low-tail residual bits must remain unclaimed"
        );
    }
}

#[test]
fn legacy_named_low_tail_door_placeable_rewrite_rejects_terminal_extra_fragment_bit() {
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let live = door_placeable_named_low_tail_update_live_bytes(object_type, b"Box");
        let mut exact_bits = vec![
            true, false, // position residual bits
            true, false, true, false, true, // Diamond door/placeable state bits
            true, // legacy name branch consumed by Diamond but not EE.
        ];
        let mut payload = live_object_payload_with_bits(&live, exact_bits.clone());
        let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
            .expect("bounded named low-tail door/placeable update should rewrite");
        assert_eq!(rewrite.update_records_rewritten, 1);
        assert_eq!(rewrite.masks_translated, 1);
        assert_eq!(rewrite.bytes_removed, 7);
        assert_eq!(rewrite.bits_inserted, 1);
        assert_eq!(rewrite.bits_removed, 1);
        assert!(
            super::claim_payload_if_verified(&payload).is_some(),
            "rewritten named low-tail update should exact-claim with no residual bits"
        );

        exact_bits.push(true);
        let mut shifted_payload = live_object_payload_with_bits(&live, exact_bits);
        let original = shifted_payload.clone();
        assert!(
            super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
            "terminal named low-tail object type {object_type:#04X} must not trim an unowned fragment bit"
        );
        assert_eq!(
            shifted_payload, original,
            "rejected terminal named low-tail repair must leave the source payload untouched"
        );
        assert!(
            super::claim_payload_if_verified(&shifted_payload).is_none(),
            "terminal named low-tail residual bits must remain unclaimed"
        );
    }
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
fn item_update_name_cursor_owns_selector_before_hidden_bool() {
    // Diamond `sub_451AF0` proves item-name mask 0x80000 as one selector BOOL
    // followed by either locstring-helper data or direct `ReadCExoString(32)`.
    // EE item body reader `sub_14076BD30` uses the same selector before the next
    // item-state BOOL, so combined name+hidden updates must advance in that
    // order and reject any shifted terminal residue.
    let mask = super::LEGACY_UPDATE_NAME_MASK | 0x0000_0040;
    let live = item_update_name_live_bytes(mask, b"Lance");

    let direct_payload = live_object_payload_with_bits(&live, vec![false, true]);
    let direct_claim = super::claim_payload_if_verified(&direct_payload)
        .expect("direct item-name plus hidden BOOL should exact-claim");
    assert_eq!(direct_claim.update_records, 1);
    assert_eq!(direct_claim.live_bytes_length, live.len());

    let loc_inline_payload = live_object_payload_with_bits(&live, vec![true, false, false]);
    let loc_inline_claim = super::claim_payload_if_verified(&loc_inline_payload)
        .expect("inline locstring item-name plus hidden BOOL should exact-claim");
    assert_eq!(loc_inline_claim.update_records, 1);
    assert_eq!(loc_inline_claim.live_bytes_length, live.len());

    let missing_hidden = live_object_payload_with_bits(&live, vec![false]);
    assert!(
        super::claim_payload_if_verified(&missing_hidden).is_none(),
        "combined name+hidden updates must not claim without the hidden-state BOOL"
    );

    let extra_terminal = live_object_payload_with_bits(&live, vec![false, true, false]);
    assert!(
        super::claim_payload_if_verified(&extra_terminal).is_none(),
        "item-name direct branch must not hide a terminal fragment bit after hidden state"
    );
}

#[test]
fn item_update_name_without_hidden_owns_only_name_selector_bits() {
    let live = item_update_name_live_bytes(super::LEGACY_UPDATE_NAME_MASK, b"Lute");

    let direct_payload = live_object_payload_with_bits(&live, vec![false]);
    let claim = super::claim_payload_if_verified(&direct_payload)
        .expect("direct item-name update should exact-claim with one selector BOOL");
    assert_eq!(claim.update_records, 1);

    let extra_bit = live_object_payload_with_bits(&live, vec![false, true]);
    assert!(
        super::claim_payload_if_verified(&extra_bit).is_none(),
        "sub_451AF0's post-name overflow check must not be modeled as a fragment BOOL"
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
fn legacy_trigger_update_rewrite_rejects_terminal_extra_fragment_bit() {
    let live = trigger_update_live_bytes(0xFFFF_FFF3, &[0xAA, 0xBB, 0xCC]);
    let mut payload = live_object_payload_with_bits(&live, vec![true, false, true]);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "terminal legacy trigger update must not trim an unowned third fragment bit"
    );
    assert_eq!(
        payload, original,
        "rejected terminal trigger repair must leave the source payload untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "terminal trigger residual bits must remain unclaimed"
    );
}

#[test]
fn final_fragment_trim_requires_family_owned_terminal_storage() {
    let trigger = trigger_update_live_bytes(0xFFFF_FFF3, &[0xAA, 0xBB, 0xCC]);
    let trigger_end = trigger.len();
    let mut live = trigger;
    let mut gui = vec![b'G', b'I', b'U'];
    gui.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    gui.extend_from_slice(&0x3344u16.to_le_bytes());
    gui.push(0x55);
    live.extend_from_slice(&gui);
    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            &live,
            0,
            live.len()
        ),
        trigger_end,
        "the trigger rewrite must hand off to the following GUI record before the terminal bit is considered"
    );

    let mut payload = live_object_payload_with_bits(
        &live,
        vec![
            true, false, // trigger position residual bits.
            true,  // terminal bit after the GUI record has no decompiled reader.
        ],
    );
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "an unrelated trigger rewrite must not trim terminal bits after a fragment-neutral GUI record"
    );
    assert_eq!(
        payload, original,
        "unowned terminal fragment bits must leave the source payload untouched"
    );
}

#[test]
fn terminal_delete_rows_do_not_inherit_prior_trim_owner() {
    // This reduces the To Heir U/5 + I/0x2A00 + GUI/delete evidence to a
    // packet-family rule. A prior typed rewrite can prove an earlier cursor,
    // but later read-buffer-only GUI rows and D/5 one-BOOL deletes must own
    // their own cursors and cannot turn final storage-looking bits into
    // family-owned terminal residue.
    let mut live = trigger_update_live_bytes(0xFFFF_FFF3, &[0xAA, 0xBB, 0xCC]);
    live.extend_from_slice(&[b'G', b'Q', 0]);
    for object_id in [0x8000_001Eu32, 0x8000_000Fu32, 0x8000_000Bu32] {
        live.extend_from_slice(&[b'D', super::CREATURE_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
    }

    let mut owned_bits = vec![true, false]; // trigger position residual bits.
    owned_bits.extend([true, false, true]); // D/5 delete BOOLs.

    let mut exact_payload = live_object_payload_with_bits(&live, owned_bits.clone());
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut exact_payload)
        .expect("valid trigger/GQ/delete stream should rewrite before exact claim");
    assert!(rewrite.update_records_rewritten >= 1);
    let claim = super::claim_payload_if_verified(&exact_payload)
        .expect("rewritten trigger/GQ/delete stream should exact-claim");
    assert_eq!(claim.delete_records, 3);
    assert_eq!(claim.live_gui_read_buffer_records, 1);

    let mut shifted_bits = owned_bits;
    shifted_bits.extend([false, true, false, false, true, false, true, false]);
    let mut shifted_payload = live_object_payload_with_bits(&live, shifted_bits);
    let original = shifted_payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
        "terminal bits after GUI/delete rows must stay unowned without a family-specific terminal storage proof"
    );
    assert_eq!(
        shifted_payload, original,
        "failed terminal-tail rewrite must leave the evidence payload untouched"
    );
}

#[test]
fn inventory_2a00_word_list_before_gq_rejects_terminal_extra_fragment_bit() {
    let mut live = inventory_2a00_word_list_live_bytes(
        &[0x0303],
        &[0xFFFF_FFFE],
        Some([
            0x0E, 0x0D, 0x0D, 0x0A, 0x13, 0x0A, 0x0C, 0x0D, 0x0F, 0x0A, 0x13, 0x0A,
        ]),
    );
    live.extend_from_slice(&[b'G', b'Q', 0]);

    let owned_inventory_bits = vec![
        false, // 0x0200 semantic BOOL; does not change the word-list cursor.
        false, // 0x0200 layout selector: false selects DWORD count + WORD rows.
        true, false, true, // one 0x2000 Feature-25 second-list row.
        true, // 0x0800 true branch owns the 12-byte read-buffer tail.
    ];
    let exact_payload = live_object_payload_with_bits(&live, owned_inventory_bits.clone());
    let exact_claim = super::claim_payload_if_verified(&exact_payload)
        .expect("I/0x2A00 word-list plus read-buffer-only GQ should claim exactly");
    assert_eq!(exact_claim.inventory_records, 1);
    assert_eq!(exact_claim.live_gui_read_buffer_records, 1);
    assert_eq!(
        exact_claim.inventory_fragment_bits, 6,
        "Diamond sub_455940 and EE sub_1407B4F70 own exactly two 0x0200 BOOLs, three Feature-25 BOOLs, and one 0x0800 BOOL"
    );

    let mut shifted_bits = owned_inventory_bits;
    shifted_bits.push(false);
    let mut shifted_payload = live_object_payload_with_bits(&live, shifted_bits);
    let original = shifted_payload.clone();

    assert!(
        super::claim_payload_if_verified(&shifted_payload).is_none(),
        "read-buffer-only GQ must not own a terminal residual fragment bit"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
        "terminal residual bits after I/0x2A00 + GQ must stay quarantined without a family owner"
    );
    assert_eq!(
        shifted_payload, original,
        "the update pass must not trim or rewrite unowned terminal evidence"
    );
}

#[test]
fn trigger_add_geometry_is_read_buffer_only() {
    // Diamond `CNWSMessage::AddTriggerGeometryToMessage` and EE's matching
    // trigger-add reader own BYTE vertex count plus XYZ FLOAT triples. They do
    // not call `ReadBOOL`, so the live-object fragment cursor must not move
    // between this add record and the next submessage.
    let live = trigger_add_live_bytes(2);
    let payload = live_object_payload_with_bits(&live, Vec::new());
    let claim = super::claim_payload_if_verified(&payload)
        .expect("trigger add geometry should exact-claim as read-buffer-only");

    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
    assert_eq!(claim.inventory_fragment_bits, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);

    let shifted = live_object_payload_with_bits(&live, vec![true]);
    assert!(
        super::claim_payload_if_verified(&shifted).is_none(),
        "trigger add geometry must not consume or hide a following fragment bit"
    );
}

#[test]
fn trigger_add_geometry_rejects_truncated_vertex_rows() {
    let mut live = trigger_add_live_bytes(1);
    live.truncate(live.len() - 1);
    let payload = live_object_payload_with_bits(&live, Vec::new());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the trigger vertex count owns complete XYZ FLOAT triples only"
    );
}

#[test]
fn top_level_item_add_token_name_repair_rewrites_selector_prefix_only() {
    // Top-level visible-equipment item adds use the same item body reader as
    // nested P/5 equipment rows. EE/Diamond read the item-name selector before
    // the active-property BOOLs, so a byte-proven token name must resize only
    // that selector prefix before exact validation advances the final cursor.
    let live = top_level_model_type2_token_name_item_add_live_bytes();
    let mut payload = live_object_payload_with_bits(&live, vec![false; 6]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "stale direct-name bits must not exact-claim a token-shaped item add"
    );

    let rewrite = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload)
        .expect("token-shaped item add should repair stale name selector bits");
    assert_eq!(rewrite.add_records_repaired, 1);
    assert_eq!(rewrite.bits_inserted, 2);
    assert_eq!(rewrite.bits_removed, 0);

    let claim =
        super::claim_payload_if_verified(&payload).expect("repaired item add should exact-claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());

    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten fragment bits");
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        &[true, true, false, false, false, false, false, false],
        "repair must materialize token selector bits before active-property bits"
    );
}

#[test]
fn door_add_name_diagnostic_uses_ee_visual_map_width() {
    // EE door adds carry the object visual-transform map as two DWORD counts,
    // not the old 40-byte scalar transform. The final claim diagnostics must
    // resolve the inline name immediately after those eight bytes.
    let live = ee_door_add_with_inline_name_live_bytes(b"Door");
    let payload = live_object_payload_with_bits(&live, vec![false; 6]);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("EE door add should exact-claim at the direct-name cursor");

    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.mentions.len(), 1);
    assert_eq!(claim.mentions[0].name.as_deref(), Some("Door"));
    assert_eq!(
        claim.mentions[0].fragment_bit_end - claim.mentions[0].fragment_bit_start,
        6,
        "door add direct-name branch owns one selector bit plus the fixed five tail bits"
    );
}

#[test]
fn door_add_name_bit_repair_uses_ee_visual_map_width() {
    // A stale legacy locstring-helper bit before a direct inline CExoString
    // must be removed at the door-name cursor. That cursor is after EE's
    // eight-byte object visual-transform map; treating it as the legacy
    // 40-byte scalar identity rejects the otherwise decompile-owned repair.
    let live = ee_door_add_with_inline_name_live_bytes(b"Door");
    let mut payload =
        live_object_payload_with_bits(&live, vec![true, false, false, false, false, false, false]);

    let rewrite = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload)
        .expect("direct-name door add should repair the stale helper bit");
    assert_eq!(rewrite.add_records_repaired, 1);
    assert_eq!(rewrite.bits_inserted, 0);
    assert_eq!(rewrite.bits_removed, 1);

    let claim =
        super::claim_payload_if_verified(&payload).expect("repaired door add should exact-claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.mentions[0].name.as_deref(), Some("Door"));

    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten fragment bits");
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        &[false, false, false, false, false, false],
        "repair must collapse the two-bit locstring helper to EE's direct-name selector"
    );
}

#[test]
fn update_rewrite_does_not_repeat_repair_exact_top_level_item_add() {
    // The update-family pass can run after the add-name pass on mixed streams.
    // Once a top-level visible-equipment add already validates at the current
    // cursor, the update pass must advance over it before trying legacy item
    // expansion; otherwise it can insert a second active-property/name repair
    // and shift the following live-object fragment cursor.
    let mut live = top_level_model_type2_token_name_item_add_live_bytes();
    live.extend_from_slice(&[b'U', super::CREATURE_OBJECT_TYPE]);
    let exact_item_bits = vec![true, true, false, false, false, false, false, false];
    let mut payload = live_object_payload_with_bits(&live, exact_item_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "trailing truncated update keeps the mixed stream from exact-claiming"
    );

    let before = payload.clone();
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "no update-family rewrite is valid after the exact add and truncated update"
    );
    assert_eq!(
        payload, before,
        "an already exact top-level item add must not be repaired a second time"
    );
}

#[test]
fn update_rewrite_can_repair_top_level_item_add_name_bits_midstream() {
    // Mixed live-object streams can expose add records only after an update
    // pass repairs earlier records. The same byte-proven item-name selector
    // repair used by the add-name pass must therefore be available here too.
    let mut live = Vec::new();
    live.extend_from_slice(&[b'W', 0x00, 0x0E]);
    live.extend_from_slice(&top_level_model_type2_token_name_item_add_live_bytes());
    live.extend_from_slice(&[b'W', 0x01, 0x0E]);
    let mut payload = live_object_payload_with_bits(&live, vec![false; 6]);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("update pass should repair the stale top-level item-add selector bits");
    assert_eq!(rewrite.bits_inserted, 2);
    assert_eq!(rewrite.bits_removed, 0);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        &[true, true, false, false, false, false, false, false],
        "update pass must rewrite only the item-name selector prefix"
    );
    let claim =
        super::claim_payload_if_verified(&payload).expect("repaired mixed stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.world_status_records, 2);
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

#[test]
fn creature_4408_zero_count_repair_infers_compact_status_row_count() {
    let mut payload =
        legacy_zero_count_creature_4408_payload(&[(b'A', 0x00F3), (b'D', 0x00B6)], &[]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "count-zero 0x4408 input must not exact-claim before the compact row count is repaired"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("zero-count compact 0x4408 status rows should rewrite through the typed path");
    assert_eq!(
        rewrite.bytes_inserted, 16,
        "two compact rows should receive two EE identity transform maps"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("repaired 0x4408 status rows should exact-claim");
    let live = &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..claim.declared];
    assert_eq!(
        super::read_u16_le(live, super::LEGACY_UPDATE_HEADER_BYTES),
        Some(2),
        "the repaired count comes from the compact row list, not a captured row table"
    );
    assert_eq!(claim.creature_update_records, 1);
}

#[test]
fn creature_4408_zero_count_repair_rejects_non_triplet_status_rows() {
    let payload = legacy_zero_count_creature_4408_payload(&[(b'A', 0x00F3)], &[0xAA]);
    let declared =
        usize::try_from(super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES).unwrap())
            .unwrap();
    let mut live =
        payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared].to_vec();
    let original = live.clone();

    assert!(
        super::creature::repair_legacy_4408_visual_effect_count_for_ee(
            &mut live,
            0,
            original.len()
        )
        .is_none(),
        "a shifted status-effect row body must not rewrite the count"
    );
    assert_eq!(live, original);
}

#[test]
fn creature_4408_zero_count_repair_rejects_without_partial_payload_write() {
    let mut payload = legacy_zero_count_creature_4408_payload_with_bits(
        &[(b'A', 0x00F3), (b'D', 0x00B6)],
        &[],
        vec![false; 6],
    );
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "zero-count repair must not commit unless the final 0x4000 BOOL cursor is exact"
    );
    assert_eq!(
        payload, original,
        "failed staged count/map repair must leave the source payload untouched"
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
