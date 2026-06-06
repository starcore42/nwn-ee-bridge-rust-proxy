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

fn ee_creature_effect_only_update_live(rows: &[(u8, u16)]) -> Vec<u8> {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x05, 0x55, 0x00, 0x00, 0x80]);
    live.extend_from_slice(&0x0000_0008u32.to_le_bytes());
    live.extend_from_slice(&(rows.len() as u16).to_le_bytes());
    for (opcode, row) in rows {
        live.push(*opcode);
        live.extend_from_slice(&row.to_le_bytes());
        live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    }
    live
}

fn ee_creature_visual_transform_update_live_bytes(object_id: u32, selector: u8) -> Vec<u8> {
    let mut live = vec![b'U', super::CREATURE_OBJECT_TYPE];
    live.extend_from_slice(&object_id.to_le_bytes());
    live.push(selector);
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    live
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
    let live = legacy_zero_count_creature_4408_live_bytes(rows, extra_before_scalar);
    live_object_payload_with_bits(&live, owned_bits)
}

fn legacy_zero_count_creature_4408_live_bytes(
    rows: &[(u8, u16)],
    extra_before_scalar: &[u8],
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
    live
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

fn item_update_locstring_token_name_live_bytes(raw_mask: u32, selector: u8, token: u32) -> Vec<u8> {
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&raw_mask.to_le_bytes());
    live.push(selector);
    live.extend_from_slice(&token.to_le_bytes());
    live
}

fn item_update_position_live_bytes(position_bytes: [u8; 6]) -> Vec<u8> {
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&super::LEGACY_UPDATE_POSITION_MASK.to_le_bytes());
    live.extend_from_slice(&position_bytes);
    live
}

fn item_update_full_mask_scalar_direct_name_live_bytes(name: &[u8]) -> Vec<u8> {
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
    live.extend_from_slice(&0xFFFF_FFF3u32.to_le_bytes());
    live.extend_from_slice(&[0xB7, 0x05, 0xC1, 0x04, 0x0F, 0x0F]); // position read bytes.
    live.push(0); // scalar-orientation read byte.
    live.extend_from_slice(&0xFFFFu16.to_le_bytes()); // appearance word with resref sentinel.
    live.extend_from_slice(&[0; 16]); // empty appearance resref.
    live.extend_from_slice(&(name.len() as u32).to_le_bytes());
    live.extend_from_slice(name);
    live
}

fn item_update_full_mask_scalar_locstring_token_live_bytes(selector: u8, token: u32) -> Vec<u8> {
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
    live.extend_from_slice(&0xFFFF_FFF3u32.to_le_bytes());
    live.extend_from_slice(&[0xB7, 0x05, 0xC1, 0x04, 0x0F, 0x0F]); // position read bytes.
    live.push(0); // scalar-orientation read byte.
    live.extend_from_slice(&0xFFFFu16.to_le_bytes()); // appearance word with resref sentinel.
    live.extend_from_slice(&[0; 16]); // empty appearance resref.
    live.push(selector);
    live.extend_from_slice(&token.to_le_bytes());
    live
}

fn item_update_full_mask_vector_direct_name_live_bytes(name: &[u8]) -> Vec<u8> {
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
    live.extend_from_slice(&0xFFFF_FFF3u32.to_le_bytes());
    live.extend_from_slice(&[0xB7, 0x05, 0xC1, 0x04, 0x0F, 0x0F]); // position read bytes.
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // vector orientation read bytes.
    live.extend_from_slice(&0xFFFFu16.to_le_bytes()); // appearance word with resref sentinel.
    live.extend_from_slice(&[0; 16]); // empty appearance resref.
    live.extend_from_slice(&(name.len() as u32).to_le_bytes());
    live.extend_from_slice(name);
    live
}

fn item_update_full_mask_scalar_direct_name_bits() -> Vec<bool> {
    vec![
        false, true, // position residual bits.
        false, true, false, true, false, // scalar orientation selector plus residual bits.
        true, false, true, false, true,  // state bits.
        false, // direct CExoString item name.
        true,  // EE hidden-state BOOL after item name.
    ]
}

fn item_update_full_mask_vector_direct_name_bits() -> Vec<bool> {
    vec![
        false, true, // position residual bits.
        true, // vector orientation selector.
        true, false, true, false, true,  // state bits.
        false, // direct CExoString item name.
        true,  // EE hidden-state BOOL after item name.
    ]
}

fn item_update_full_mask_scalar_locstring_inline_bits() -> Vec<bool> {
    vec![
        false, true, // position residual bits.
        false, true, false, true, false, // scalar orientation selector plus residual bits.
        true, false, true, false, true,  // state bits.
        true,  // locstring item name helper.
        false, // inline CExoString component, not TLK token.
        true,  // EE hidden-state BOOL after item name.
    ]
}

fn item_update_full_mask_scalar_locstring_token_bits() -> Vec<bool> {
    vec![
        false, true, // position residual bits.
        false, true, false, true, false, // scalar orientation selector plus residual bits.
        true, false, true, false, true, // state bits.
        true, // locstring item name helper.
        true, // client-TLK/token component.
        true, // EE hidden-state BOOL after the token payload.
    ]
}

fn legacy_tail9_door_update_without_name_payload_live_bytes() -> Vec<u8> {
    let mut live = vec![b'U', super::DOOR_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_0004u32.to_le_bytes());
    live.extend_from_slice(&0xFFFF_FFF7u32.to_le_bytes());
    live.extend_from_slice(&[0xD0, 0x07, 0xF4, 0x01, 0x0F, 0x0F]); // position read bytes.
    live.extend_from_slice(&0x2EA8u16.to_le_bytes()); // legacy facing scalar.
    live.push(0x02); // legacy generic state byte.
    live.extend_from_slice(&1.0f32.to_le_bytes()); // scale.
    live.extend_from_slice(&0x0016u16.to_le_bytes()); // legacy animation/state word.
    live.extend_from_slice(&0x0000_14E5u32.to_le_bytes()); // legacy-only suffix, not CExoString.
    live
}

fn legacy_tail9_door_update_source_bits() -> Vec<bool> {
    vec![
        false, true, // position residual bits.
        true, false, true, false, true,  // Diamond door state bits.
        false, // Diamond legacy name branch bit removed before EE emission.
    ]
}

fn legacy_tail9_door_update_cep_name_suffix_source_bits() -> Vec<bool> {
    vec![
        false, true, // position residual bits.
        true, false, false, false, true, // Diamond door state bits from the CEP v2.3 stream.
        true, // legacy name branch owning the following four-byte suffix.
    ]
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

fn legacy_tail9_door_update_expected_ee_bits() -> Vec<bool> {
    let mut bits = vec![false, true]; // position residual bits.
    bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(
        0x2EA8,
    ));
    bits.extend_from_slice(&[true, false, true, false, true]); // Diamond door state bits.
    bits.push(false); // EE-only neutral door/placeable state BOOL.
    bits
}

fn legacy_tail9_door_update_cep_name_suffix_expected_ee_bits() -> Vec<bool> {
    let mut bits = vec![false, true]; // position residual bits.
    bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(
        0x2EA8,
    ));
    bits.extend_from_slice(&[true, false, false, false, true]); // CEP v2.3 Diamond door state bits.
    bits.push(false); // EE-only neutral door/placeable state BOOL.
    bits
}

fn legacy_short_strref_door_add_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A', super::DOOR_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_0004u32.to_le_bytes());
    live.extend_from_slice(&0u32.to_le_bytes()); // generic door row follows.
    live.extend_from_slice(&0x0000_000Du32.to_le_bytes());
    live.extend_from_slice(&0x0000_14E5u32.to_le_bytes()); // legacy short-name token.
    live.extend_from_slice(&0x0016u16.to_le_bytes()); // door state tail.
    live
}

fn legacy_short_strref_door_add_source_bits_with_state(state_bits: [bool; 4]) -> Vec<bool> {
    let mut bits = vec![true]; // Diamond short-name/locstring branch.
    bits.extend_from_slice(&state_bits);
    bits
}

fn legacy_short_strref_door_add_source_bits() -> Vec<bool> {
    legacy_short_strref_door_add_source_bits_with_state([true, false, true, false])
}

fn legacy_short_strref_door_add_expected_ee_bits_with_state(state_bits: [bool; 4]) -> Vec<bool> {
    let mut bits = vec![
        false, // canonical EE direct empty-name branch.
        false, // inserted first post-name state BOOL for the normalized empty name.
    ];
    bits.extend_from_slice(&state_bits);
    bits
}

fn legacy_short_strref_door_add_expected_ee_bits() -> Vec<bool> {
    legacy_short_strref_door_add_expected_ee_bits_with_state([true, false, true, false])
}

fn ee_shaped_generic_door_add_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A', super::DOOR_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_0004u32.to_le_bytes());
    live.extend_from_slice(&0u32.to_le_bytes()); // generic door selector.
    live.extend_from_slice(&0x0000_000Du32.to_le_bytes());
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    live.extend_from_slice(&0u32.to_le_bytes()); // direct empty name.
    live.extend_from_slice(&0x0016u16.to_le_bytes()); // door state tail.
    live
}

fn ee_shaped_generic_door_add_bits() -> Vec<bool> {
    vec![
        false, // direct empty-name branch.
        false, false, false, false, false, // six EE door state/name bits total.
    ]
}

fn item_update_hidden_live_bytes() -> Vec<u8> {
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&0x0000_0040u32.to_le_bytes());
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

fn direct_name_trigger_add_live_bytes(name: &[u8], vertex_count: u8) -> Vec<u8> {
    let mut live = vec![b'A', super::TRIGGER_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&(name.len() as u32).to_le_bytes());
    live.extend_from_slice(name);
    live.push(0); // cursor byte
    live.extend_from_slice(&0.0f32.to_le_bytes()); // height
    live.push(vertex_count);
    for index in 0..vertex_count {
        let base = f32::from(index) + 1.0;
        live.extend_from_slice(&base.to_le_bytes());
        live.extend_from_slice(&(base + 0.25).to_le_bytes());
        live.extend_from_slice(&(base + 0.5).to_le_bytes());
    }
    live
}

fn ambiguous_direct_name_trigger_add_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A', super::TRIGGER_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2200u32.to_le_bytes());
    live.extend_from_slice(&1u32.to_le_bytes());
    live.push(0); // one-byte direct name; also byte-plausible as the short cursor.
    live.push(0); // direct cursor byte.
    live.extend_from_slice(&[0, 0, 0, 1]); // finite direct height; short vertex count = 1.
    live.push(1); // direct vertex count.
    live.extend_from_slice(&[0; 12]); // one direct XYZ vertex; also finite as short geometry.
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

fn ee_shaped_model_type2_typed_item_create_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
    live.extend_from_slice(&0x01u32.to_le_bytes()); // stock base item with model type 2.
    for part in [0x0Cu16, 0x0Bu16, 0x0Bu16] {
        live.extend_from_slice(&part.to_le_bytes());
    }
    live.push(0);
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    live.extend_from_slice(&5u32.to_le_bytes());
    live.extend_from_slice(b"Lance");
    live.extend_from_slice(&2u32.to_le_bytes());
    live.extend_from_slice(&1u32.to_le_bytes());
    live.extend_from_slice(&[0, 0, 0xFF]);
    live.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    live
}

fn ee_shaped_gui_inventory_model_type2_item_create_live_bytes() -> Vec<u8> {
    let typed = ee_shaped_model_type2_typed_item_create_live_bytes();
    let mut live = vec![b'G', b'I', b'A'];
    live.extend_from_slice(&0u32.to_le_bytes()); // inventory slot/container payload.
    live.extend_from_slice(&typed[2..]); // GUI rows start at the item OBJECTID.
    live
}

fn legacy_width_gui_inventory_model_type2_item_create_live_bytes() -> Vec<u8> {
    let typed = legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes();
    let mut live = vec![b'G', b'I', b'A'];
    live.extend_from_slice(&0u32.to_le_bytes()); // inventory slot/container payload.
    live.extend_from_slice(&typed[2..]); // GUI rows start at the item OBJECTID.
    live
}

fn inject_live_boundary_lookalike_into_item_property_values(live: &mut [u8]) {
    let name_start = live
        .windows(b"Lance".len())
        .position(|window| window == b"Lance")
        .expect("item name in typed item-create fixture");
    let active_property_tail_start = name_start + b"Lance".len();
    let value_bytes_start = active_property_tail_start + 11;
    live[value_bytes_start..value_bytes_start + 8].copy_from_slice(&[
        b'U',
        super::ITEM_OBJECT_TYPE,
        0xB8,
        0x00,
        0x00,
        0x80,
        0xF3,
        0xFF,
    ]);
}

fn legacy_width_model_type2_typed_item_create_with_visual_map_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
    live.extend_from_slice(&0x01u32.to_le_bytes()); // stock base item with model type 2.
    live.extend_from_slice(&[0x0C, 0x0B, 0x0B]); // Diamond BYTE model parts.
    live.push(0);
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    live.extend_from_slice(&5u32.to_le_bytes());
    live.extend_from_slice(b"Lance");
    live.extend_from_slice(&2u32.to_le_bytes());
    live.extend_from_slice(&1u32.to_le_bytes());
    live.extend_from_slice(&[0, 0, 0xFF]);
    live.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    live
}

fn legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
    live.extend_from_slice(&0x01u32.to_le_bytes()); // stock base item with model type 2.
    live.extend_from_slice(&[0x0C, 0x0B, 0x0B]); // Diamond BYTE model parts.
    live.push(0); // fourth Diamond model-type-2 appearance byte.
    live.extend_from_slice(&5u32.to_le_bytes());
    live.extend_from_slice(b"Lance");
    live.extend_from_slice(&2u32.to_le_bytes());
    live.extend_from_slice(&1u32.to_le_bytes());
    live.extend_from_slice(&[0, 0, 0xFF]);
    live.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    live
}

fn compact_placeable_token_name_add_live_bytes() -> Vec<u8> {
    let mut live = vec![b'A', super::PLACEABLE_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_18CAu32.to_le_bytes());
    live.extend_from_slice(&0x0000_62A9u32.to_le_bytes());
    live.push(0x05);
    live.extend_from_slice(&0u16.to_le_bytes());
    live.extend_from_slice(&0u16.to_le_bytes());
    live
}

fn ee_empty_placeable_add_live_bytes(appearance: u16) -> Vec<u8> {
    let mut live = vec![b'A', super::PLACEABLE_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_18C2u32.to_le_bytes());
    live.extend_from_slice(&0u32.to_le_bytes());
    live.push(0x05);
    live.extend_from_slice(&appearance.to_le_bytes());
    live.extend_from_slice(&0u16.to_le_bytes());
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    live
}

fn placeable_stale_gap_update_live_bytes_for_object(object_id: u32) -> Vec<u8> {
    let mut live = vec![b'U', super::PLACEABLE_OBJECT_TYPE];
    live.extend_from_slice(&object_id.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_ORIENTATION_MASK
            | super::LEGACY_UPDATE_SCALE_STATE_MASK
            | super::LEGACY_UPDATE_STATE_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0xB3, 0x0C, 0x11, 0x06, 0x0F, 0x0F]);
    live.push(0x61);
    live.extend_from_slice(&0x0076u16.to_le_bytes());
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live
}

fn door_direct_name_add_live_bytes_without_visual_map(object_id: u32) -> Vec<u8> {
    let mut live = vec![b'A', super::DOOR_OBJECT_TYPE];
    live.extend_from_slice(&object_id.to_le_bytes());
    live.extend_from_slice(&0u32.to_le_bytes());
    live.extend_from_slice(&0x0000_000Cu32.to_le_bytes());
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live
}

fn door_update_0x17_live_bytes_for_object(object_id: u32) -> Vec<u8> {
    let mut live = vec![b'U', super::DOOR_OBJECT_TYPE];
    live.extend_from_slice(&object_id.to_le_bytes());
    live.extend_from_slice(
        &(super::LEGACY_UPDATE_POSITION_MASK
            | super::LEGACY_UPDATE_ORIENTATION_MASK
            | super::LEGACY_UPDATE_SCALE_STATE_MASK
            | super::LEGACY_UPDATE_STATE_MASK)
            .to_le_bytes(),
    );
    live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
    live.push(0x28);
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
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

fn ee_door_placeable_full_vector_update_live_bytes(object_type: u8) -> Vec<u8> {
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
    live.extend_from_slice(&[0x20, 0x01, 0xE0, 0xFF, 0x00, 0x10]); // vector orientation
    live.extend_from_slice(&0x0042u16.to_le_bytes()); // appearance row
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live
}

fn with_live_update_object_id(mut live: Vec<u8>, object_id: u32) -> Vec<u8> {
    live[2..6].copy_from_slice(&object_id.to_le_bytes());
    live
}

fn work_remaining_compact_pairs_with_storage(object_ids: &[u32], source_bits: &[bool]) -> Vec<u8> {
    let mut storage_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    storage_bits.extend_from_slice(source_bits);
    let storage = super::bits::pack_msb_valid_bits(storage_bits, super::CNW_FRAGMENT_HEADER_BITS);

    let mut live = vec![b'W', 0x0C, 0x0E];
    live.extend_from_slice(&storage);
    for object_id in object_ids.iter().copied() {
        let compact_object_id = object_id & !0x8000_0000;
        live.extend_from_slice(&with_live_update_object_id(
            compact_placeable_token_name_add_live_bytes(),
            object_id,
        ));
        live.extend_from_slice(&with_live_update_object_id(
            door_placeable_low_tail_update_live_bytes(
                super::PLACEABLE_OBJECT_TYPE,
                &[0x7B, 0x74, 0x01, 0x00],
            ),
            compact_object_id,
        ));
    }
    live
}

fn work_remaining_compact_stale_gap_pairs_with_storage(
    object_ids: &[u32],
    source_bits: &[bool],
) -> Vec<u8> {
    let mut storage_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    storage_bits.extend_from_slice(source_bits);
    let storage = super::bits::pack_msb_valid_bits(storage_bits, super::CNW_FRAGMENT_HEADER_BITS);

    let mut live = vec![b'W', 0x0C, 0x0E];
    live.extend_from_slice(&storage);
    for object_id in object_ids.iter().copied() {
        live.extend_from_slice(&with_live_update_object_id(
            compact_placeable_token_name_add_live_bytes(),
            object_id,
        ));
        live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(object_id));
    }
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

fn exact_vector_door_placeable_update_bits() -> Vec<bool> {
    vec![
        true, false, // position residual bits
        true,  // vector orientation selector; vector branch has no scalar low bits
        true, false, true, false, true,  // Diamond door/placeable state bits
        false, // EE-only neutral door/placeable state BOOL
    ]
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
fn work_remaining_preserves_exact_vector_door_placeable_37_rows() {
    // This is the vector-orientation sibling of the scalar U/9-W audit.
    // Diamond `sub_467AE0` and EE `sub_14079C050` both branch on the
    // orientation BOOL before reading either one scalar byte or six vector
    // bytes; `W current total` is still a separate three-byte, zero-BOOL
    // suffix after the update owns its own position/state bits.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let mut live = ee_door_placeable_full_vector_update_live_bytes(object_type);
        live.extend_from_slice(&[b'W', 0x0C, 0x20]);
        let payload =
            live_object_payload_with_bits(&live, exact_vector_door_placeable_update_bits());

        let claim = super::claim_payload_if_verified(&payload)
            .expect("exact vector U/9 or U/10 followed by W should claim");
        assert_eq!(claim.update_records, 1);
        assert_eq!(claim.world_status_records, 1);
        assert_eq!(
            claim.live_bytes_length,
            live.len(),
            "W remains a fragment-neutral suffix after the vector update"
        );
    }
}

#[test]
fn work_remaining_does_not_supply_missing_vector_update_fragment_bits() {
    // A vector update consumes only the orientation selector bit for the
    // branch itself, but it still owns position and state BOOLs before any
    // following W row. A bit-short vector-shaped record must remain visible
    // for quarantine instead of borrowing from the W suffix.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let mut live = ee_door_placeable_full_vector_update_live_bytes(object_type);
        live.extend_from_slice(&[b'W', 0x0C, 0x20]);
        let mut payload = live_object_payload_with_bits(
            &live,
            vec![
                true, false, // position residual bits
                true,  // vector orientation selector only
            ],
        );
        let original = payload.clone();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "object type {object_type:#04X} must own vector update state bits before W"
        );
        assert!(
            super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
            "the update pass must not borrow missing vector U/9 bits from W"
        );
        assert_eq!(
            payload, original,
            "bit-short vector U/9 or U/10 before W must stay visible for quarantine/diagnostics"
        );
    }
}

#[test]
fn work_remaining_does_not_supply_missing_item_hidden_bit() {
    // Diamond `sub_451AF0` has no low-0x40 read-buffer tail; EE
    // `sub_14076BD30` owns one hidden-state BOOL for item mask 0x40. `W current
    // total` (`sub_44F160` / `sub_1407B85A0`) owns only its three read-buffer
    // bytes, so it cannot donate the missing item BOOL or hide an extra one.
    let mut live = item_update_hidden_live_bytes();
    live.extend_from_slice(&[b'W', 0x0C, 0x20]);

    let payload = live_object_payload_with_bits(&live, vec![true]);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("item hidden update followed by W should exact-claim with one item BOOL");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());

    let mut missing = live_object_payload_with_bits(&live, Vec::new());
    let original_missing = missing.clone();
    assert!(
        super::claim_payload_if_verified(&missing).is_none(),
        "W must not supply the missing item hidden-state BOOL"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut missing).is_none(),
        "rewrite must not borrow the missing item bit from W"
    );
    assert_eq!(
        missing, original_missing,
        "bit-short U/6 before W must stay visible for quarantine/diagnostics"
    );

    let extra = live_object_payload_with_bits(&live, vec![true, false]);
    assert!(
        super::claim_payload_if_verified(&extra).is_none(),
        "W must not hide an extra terminal fragment bit after item hidden state"
    );
}

#[test]
fn work_remaining_does_not_rescue_shifted_full_item_update_cursor() {
    // This is the item sibling of the CEP v2.3 U/6-W boundary evidence.
    // Diamond `sub_467AE0` and EE `sub_14079C050` branch on the orientation
    // BOOL before reading orientation bytes, while `W current total`
    // (`sub_44F160` / `sub_1407B85A0`) owns no CNW fragment bits. A following W
    // can therefore follow a fully proven item update, but it cannot relabel a
    // vector-selected, scalar-shaped U/6 row.
    let mut exact_live = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    exact_live.extend_from_slice(&[b'W', 0x0C, 0x20]);
    let mut exact_payload =
        live_object_payload_with_bits(&exact_live, item_update_full_mask_scalar_direct_name_bits());

    assert!(
        super::claim_payload_if_verified(&exact_payload).is_none(),
        "the raw Diamond all-bits item mask is not an exact EE update before W"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut exact_payload)
        .expect("decompile-shaped full item update should translate before W");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.masks_translated, 1);
    let exact_claim = super::claim_payload_if_verified(&exact_payload)
        .expect("translated full item update followed by W should exact-claim");
    assert_eq!(exact_claim.update_records, 1);
    assert_eq!(exact_claim.world_status_records, 1);
    assert_eq!(exact_claim.live_bytes_length, exact_live.len());

    let mut shifted_live = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    shifted_live.extend_from_slice(&[b'W', 0x0C, 0x20]);
    let shifted_bits = vec![
        false, true, // position residual bits.
        true, // vector orientation selector, despite scalar-shaped bytes.
        true, false, true, false, true,  // state bits if the vector cursor were valid.
        false, // direct CExoString item name if the scalar cursor were valid.
        true,  // hidden BOOL if the scalar cursor were valid.
    ];
    let mut shifted_payload = live_object_payload_with_bits(&shifted_live, shifted_bits);
    let original_shifted = shifted_payload.clone();

    assert!(
        super::claim_payload_if_verified(&shifted_payload).is_none(),
        "W must not make a shifted full item U/6 cursor exact"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
        "rewrite must not borrow U/6 orientation/name proof from a following W row"
    );
    assert_eq!(
        shifted_payload, original_shifted,
        "shifted full item U/6 before W must stay visible for quarantine/diagnostics"
    );
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
fn work_remaining_fragment_storage_before_compact_add_does_not_supply_low_tail_bits() {
    // XP2-style streams can carry a bounded post-W CNW storage span before the
    // next compact `A/09` row. `W current total` remains fragment-neutral
    // (`sub_44F160` / `sub_1407B85A0`), so removing that top-level storage
    // collision must not manufacture the following compact add's EE guard run
    // or the same-object low-tail update cursor.
    let object_id = 0x8000_18CAu32;
    let mut live = vec![b'W', 0x01, 0x0E, 0xA0];
    live.extend_from_slice(&compact_placeable_token_name_add_live_bytes());
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(super::PLACEABLE_OBJECT_TYPE, &[0x00, 0x00]),
        object_id,
    ));

    let mut payload = live_object_payload_with_bits(&live, vec![false; 5]);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "post-W storage removal must not prove compact-add/low-tail source bits"
    );
    assert_eq!(
        payload, original,
        "failed W/add/low-tail proof must leave the evidence payload untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the raw stream stays active evidence until the upstream bit owner is proven"
    );
}

#[test]
fn work_remaining_midstream_storage_promotes_bits_before_compact_add_update() {
    // `W current total` is fragment-neutral, but a bounded CNW storage span can
    // sit between that read-buffer row and the next top-level bit-owning record.
    // When the following compact add/update consumes those bits exactly, the
    // span must be promoted into the fragment cursor instead of discarded.
    let object_id = 0x8000_18CAu32;
    let compact_object_id = object_id & !0x8000_0000;
    let mut source_bits = vec![false; 6];
    source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut storage_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    storage_bits.extend_from_slice(&source_bits);
    let storage = super::bits::pack_msb_valid_bits(storage_bits, super::CNW_FRAGMENT_HEADER_BITS);

    let mut live = vec![b'W', 0x01, 0x0E];
    live.extend_from_slice(&storage);
    live.extend_from_slice(&compact_placeable_token_name_add_live_bytes());
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        compact_object_id,
    ));

    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw post-W storage bytes are not owned by the fragment-neutral W row"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("post-W storage should promote into the following compact add/update cursor");
    assert_eq!(
        rewrite.interleaved_fragment_bytes_promoted,
        storage.len() as u32
    );
    assert_eq!(
        rewrite.interleaved_fragment_bits_promoted,
        source_bits.len() as u32
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("promoted post-W storage plus compact add/update should exact-claim");
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn work_remaining_storage_rolls_back_when_later_compact_pair_is_shifted() {
    // Generalized from the XP2 seq19 trace after the second post-W storage
    // promotion: the CNW span can be valid storage for earlier compact
    // add/update pairs, but a later shifted compact add must still roll the
    // whole transaction back instead of committing the earlier rewrites.
    let good_pairs = [0x8000_1103u32, 0x8000_1119u32];
    let shifted_pair = 0x8000_1101u32;

    let mut good_source_bits = Vec::new();
    for _ in good_pairs {
        good_source_bits.extend_from_slice(&[false; 4]);
        good_source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }
    let good_live = work_remaining_compact_pairs_with_storage(&good_pairs, &good_source_bits);
    let mut good_payload = live_object_payload_with_bits(&good_live, Vec::new());

    let good_rewrite = super::rewrite_update_records_payload_if_possible(&mut good_payload)
        .expect("bounded post-W storage should feed the valid compact pairs");
    assert_eq!(good_rewrite.interleaved_fragment_spans_promoted, 1);
    let good_claim = super::claim_payload_if_verified(&good_payload)
        .expect("valid compact pairs after promoted W storage should exact-claim");
    assert_eq!(good_claim.world_status_records, 1);
    assert_eq!(good_claim.add_records, good_pairs.len() as u32);
    assert_eq!(good_claim.update_records, good_pairs.len() as u32);

    let mut shifted_source_bits = good_source_bits;
    shifted_source_bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut all_pairs = good_pairs.to_vec();
    all_pairs.push(shifted_pair);
    let shifted_live = work_remaining_compact_pairs_with_storage(&all_pairs, &shifted_source_bits);
    let mut shifted_payload = live_object_payload_with_bits(&shifted_live, Vec::new());
    let original = shifted_payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
        "a later shifted compact pair must not commit earlier post-W storage rewrites"
    );
    assert_eq!(
        shifted_payload, original,
        "failed promoted-storage compact-pair proof must leave bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&shifted_payload).is_none(),
        "the shifted post-W compact handoff remains active cursor evidence"
    );
}

#[test]
fn work_remaining_storage_rolls_back_after_stale_gap_pair_run_before_shifted_low_tail() {
    // The XP2 seq19 replay reaches a post-`W` CNW storage span before a run of
    // compact token-name `A/09` plus same-object `U/09 mask=0x17` stale-gap
    // pairs. `W` itself is fragment-neutral; the storage span can feed those
    // exact pairs, but it does not create a later resync point for a shifted
    // compact add followed by `U/09 mask=0xF7`.
    let good_pairs = [0x8000_1072u32, 0x8000_1120u32];
    let shifted_object_id = 0x8000_0001u32;

    let mut good_source_bits = Vec::new();
    for _ in good_pairs {
        good_source_bits.extend_from_slice(&[true, false, true, false]);
        good_source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }
    let good_live =
        work_remaining_compact_stale_gap_pairs_with_storage(&good_pairs, &good_source_bits);
    let mut good_payload = live_object_payload_with_bits(&good_live, Vec::new());

    let good_rewrite = super::rewrite_update_records_payload_if_possible(&mut good_payload)
        .expect("post-W storage should feed exact compact/stale-gap pairs");
    assert_eq!(good_rewrite.interleaved_fragment_spans_promoted, 1);
    assert_eq!(
        good_rewrite.interleaved_fragment_bits_promoted,
        good_source_bits.len() as u32
    );
    let good_claim = super::claim_payload_if_verified(&good_payload)
        .expect("compact/stale-gap pairs after promoted W storage should exact-claim");
    assert_eq!(good_claim.world_status_records, 1);
    assert_eq!(good_claim.add_records, good_pairs.len() as u32);
    assert_eq!(good_claim.update_records, good_pairs.len() as u32);

    let mut shifted_source_bits = good_source_bits;
    shifted_source_bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut shifted_live =
        work_remaining_compact_stale_gap_pairs_with_storage(&good_pairs, &shifted_source_bits);
    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());
    shifted_live.extend_from_slice(&shifted_add);
    shifted_live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));
    let mut shifted_payload = live_object_payload_with_bits(&shifted_live, Vec::new());
    let original = shifted_payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
        "post-W storage plus exact stale-gap pairs must roll back before shifted compact low-tail bits"
    );
    assert_eq!(
        shifted_payload, original,
        "failed storage/stale-gap/low-tail proof must leave bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&shifted_payload).is_none(),
        "the shifted post-W stale-gap handoff remains active cursor evidence"
    );
}

#[test]
fn work_remaining_long_storage_span_claims_only_when_stale_gap_run_consumes_every_bit() {
    // The XP2 seq19 private trace uses a much larger post-`W` storage span than
    // the minimal public siblings above. Span length is not ownership proof:
    // Diamond `sub_44E4A0` owns four compact add BOOLs per `A/09`, and each
    // stale-gap `U/09 mask=0x17` owns only its decompiled update cursor. The
    // promoted span may commit only when those exact rows consume every bit.
    let object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1200u32 + index).collect();
    let mut source_bits = Vec::new();
    for _ in &object_ids {
        source_bits.extend_from_slice(&[true, false, true, false]);
        source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }

    let live = work_remaining_compact_stale_gap_pairs_with_storage(&object_ids, &source_bits);
    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("long post-W storage should feed the exact compact/stale-gap run");

    assert_eq!(rewrite.interleaved_fragment_spans_promoted, 1);
    assert_eq!(
        rewrite.interleaved_fragment_bytes_promoted, 31,
        "3 CNW header bits plus 240 row-owned bits pack into the long XP2-sized span"
    );
    assert_eq!(
        rewrite.interleaved_fragment_bits_promoted,
        source_bits.len() as u32
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("long promoted storage plus exact stale-gap run should claim");
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.add_records, object_ids.len() as u32);
    assert_eq!(claim.update_records, object_ids.len() as u32);
}

#[test]
fn work_remaining_long_storage_span_accepts_mixed_compact_add_prefix_bits() {
    // The XP2 seq19 private replay shows the same long post-`W` span carrying
    // mixed compact-add source prefixes (`1101`, `0001`, `0010`, `1110`) before
    // exact `U/09 mask=0x17` stale-gap cursors. Those four bits are still owned
    // by Diamond `sub_44E4A0` and drained as source-only compact add state; their
    // values must not be reinterpreted as EE guard bits or a cursor resync point.
    let object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1400u32 + index).collect();
    let compact_prefixes: &[&[bool]] = &[
        &[true, true, false, true],
        &[false, false, false, true],
        &[false, false, true, false],
        &[true, true, true, false],
    ];
    let mut source_bits = Vec::new();
    for (index, _) in object_ids.iter().enumerate() {
        source_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }

    let live = work_remaining_compact_stale_gap_pairs_with_storage(&object_ids, &source_bits);
    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("mixed-prefix long post-W storage should feed exact compact/stale-gap rows");

    assert_eq!(rewrite.interleaved_fragment_spans_promoted, 1);
    assert_eq!(
        rewrite.interleaved_fragment_bytes_promoted, 31,
        "3 CNW header bits plus 240 mixed-prefix row bits pack into the XP2-sized span"
    );
    assert_eq!(
        rewrite.interleaved_fragment_bits_promoted,
        source_bits.len() as u32
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("mixed-prefix long promoted storage plus exact stale-gap run should claim");
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.add_records, object_ids.len() as u32);
    assert_eq!(claim.update_records, object_ids.len() as u32);
}

#[test]
fn work_remaining_long_storage_span_rolls_back_before_shifted_low_tail_handoff() {
    // The XP2 seq19 private replay combines the long post-`W` storage span with
    // a later compact `A/09` plus same-object `U/09 mask=0xF7` handoff. The long
    // span can feed the preceding compact/stale-gap run, but it must not make
    // the later shifted `1000_11_101101` bit run look owned.
    let object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1500u32 + index).collect();
    let compact_prefixes: &[&[bool]] = &[
        &[true, true, false, true],
        &[false, false, false, true],
        &[false, false, true, false],
        &[true, true, true, false],
    ];
    let mut source_bits = Vec::new();
    for (index, _) in object_ids.iter().enumerate() {
        source_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }

    let shifted_object_id = 0x8000_0001u32;
    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut live = work_remaining_compact_stale_gap_pairs_with_storage(&object_ids, &source_bits);
    live.extend_from_slice(&shifted_add);
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));

    let mut payload = live_object_payload_with_bits(
        &live,
        vec![
            true, false, false, false, true, true, true, false, true, true, false, true,
        ],
    );
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "long storage plus exact stale-gap rows must roll back before the shifted compact low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed long-storage/add/low-tail proof must leave source bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the later shifted low-tail handoff remains active cursor evidence"
    );
}

#[test]
fn work_remaining_long_storage_span_rolls_back_before_shifted_low_tail_with_following_boundary() {
    // In the XP2 seq19 private replay the shifted compact add/low-tail update
    // is followed by another top-level compact row. That later plausible
    // boundary is not a stream resync proof: the shifted pair still owns only
    // its decompiled compact-add and low-tail update cursors, so the whole
    // promoted-storage transaction must roll back unchanged.
    let object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1600u32 + index).collect();
    let compact_prefixes: &[&[bool]] = &[
        &[true, true, false, true],
        &[false, false, false, true],
        &[false, false, true, false],
        &[true, true, true, false],
    ];
    let mut source_bits = Vec::new();
    for (index, _) in object_ids.iter().enumerate() {
        source_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }

    let following_object_id = 0x8000_1700u32;
    let mut following_live = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        following_object_id,
    );
    following_live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(
        following_object_id,
    ));
    let mut following_bits = vec![false, false, false, true];
    following_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut following_payload =
        live_object_payload_with_bits(&following_live, following_bits.clone());
    let following_rewrite =
        super::rewrite_update_records_payload_if_possible(&mut following_payload)
            .expect("the following compact/stale-gap boundary should be valid by itself");
    assert_eq!(following_rewrite.update_records_rewritten, 1);
    let following_claim = super::claim_payload_if_verified(&following_payload)
        .expect("the following compact/stale-gap boundary should exact-claim by itself");
    assert_eq!(following_claim.add_records, 1);
    assert_eq!(following_claim.update_records, 1);

    let shifted_object_id = 0x8000_0001u32;
    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut live = work_remaining_compact_stale_gap_pairs_with_storage(&object_ids, &source_bits);
    live.extend_from_slice(&shifted_add);
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));
    live.extend_from_slice(&following_live);

    let mut bits = vec![
        true, false, false, false, true, true, true, false, true, true, false, true,
    ];
    bits.extend_from_slice(&following_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "a following compact boundary must not resync a shifted low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed low-tail/following-boundary proof must leave source bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the shifted handoff remains active even with a plausible following boundary"
    );
}

#[test]
fn work_remaining_long_storage_span_rolls_back_after_preceding_pair_before_shifted_low_tail() {
    // Move one boundary upstream from the XP2 seq19 long-span proof: the row
    // before the `W`/storage span can itself be a compact token-name `A/09`
    // plus same-object stale-gap `U/09 mask=0x17`. That pair owns only its
    // decompiled add/update cursor. `W current total` remains fragment-neutral,
    // so the following promoted storage span and shifted low-tail handoff must
    // still be exact at their own cursors.
    let pre_w_object_id = 0x8000_1720u32;
    let mut pre_w_live = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        pre_w_object_id,
    );
    pre_w_live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(
        pre_w_object_id,
    ));
    let mut pre_w_bits = vec![true, false, true, false];
    pre_w_bits.extend_from_slice(&scalar_door_placeable_update_bits());

    let mut pre_w_payload = live_object_payload_with_bits(&pre_w_live, pre_w_bits.clone());
    let pre_w_rewrite = super::rewrite_update_records_payload_if_possible(&mut pre_w_payload)
        .expect("the pre-W compact/stale-gap pair should own its source cursor");
    assert_eq!(pre_w_rewrite.update_records_rewritten, 1);
    let pre_w_claim = super::claim_payload_if_verified(&pre_w_payload)
        .expect("the pre-W compact/stale-gap pair should exact-claim by itself");
    assert_eq!(pre_w_claim.add_records, 1);
    assert_eq!(pre_w_claim.update_records, 1);

    let object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1800u32 + index).collect();
    let compact_prefixes: &[&[bool]] = &[
        &[true, true, false, true],
        &[false, false, false, true],
        &[false, false, true, false],
        &[true, true, true, false],
    ];
    let mut source_bits = Vec::new();
    for (index, _) in object_ids.iter().enumerate() {
        source_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }

    let storage_live =
        work_remaining_compact_stale_gap_pairs_with_storage(&object_ids, &source_bits);
    let mut positive_live = pre_w_live.clone();
    positive_live.extend_from_slice(&storage_live);
    let mut positive_payload = live_object_payload_with_bits(&positive_live, pre_w_bits.clone());
    let positive_rewrite = super::rewrite_update_records_payload_if_possible(&mut positive_payload)
        .expect("pre-W pair plus long storage/stale-gap run should exact-rewrite");
    assert_eq!(positive_rewrite.interleaved_fragment_spans_promoted, 1);
    let positive_claim = super::claim_payload_if_verified(&positive_payload)
        .expect("pre-W pair plus long promoted storage should exact-claim");
    assert_eq!(positive_claim.world_status_records, 1);
    assert_eq!(positive_claim.add_records, (object_ids.len() + 1) as u32);
    assert_eq!(positive_claim.update_records, (object_ids.len() + 1) as u32);

    let shifted_object_id = 0x8000_0001u32;
    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut shifted_live = positive_live;
    shifted_live.extend_from_slice(&shifted_add);
    shifted_live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));

    let mut bits = pre_w_bits;
    bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut payload = live_object_payload_with_bits(&shifted_live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "a pre-W compact/stale-gap pair must not resync a later shifted low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed pre-W/storage/low-tail proof must leave source bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the shifted handoff remains active even with a proven pre-W pair"
    );
}

#[test]
fn pre_w_full_update_run_does_not_resync_shifted_low_tail() {
    // Move the XP2 seq19 proof upstream across the long run before the first
    // `W`: compact token-name `A/09` rows followed by full `U/09 mask=0x37`
    // updates own only their decompiled add/update cursors. Even a long exact
    // run before `W` cannot donate, skip, or resync bits for the later shifted
    // compact add plus low-tail update handoff.
    let pre_w_object_ids: Vec<u32> = (0..8).map(|index| 0x8000_1900u32 + index).collect();
    let compact_prefixes: &[&[bool]] = &[
        &[true, false, true, false],
        &[false, false, false, true],
        &[true, true, false, true],
        &[false, false, true, false],
    ];

    let mut pre_w_live = Vec::new();
    let mut pre_w_bits = Vec::new();
    for (index, object_id) in pre_w_object_ids.iter().copied().enumerate() {
        pre_w_live.extend_from_slice(&with_live_update_object_id(
            compact_placeable_token_name_add_live_bytes(),
            object_id,
        ));
        pre_w_live.extend_from_slice(&with_live_update_object_id(
            ee_door_placeable_full_update_live_bytes(super::PLACEABLE_OBJECT_TYPE),
            object_id,
        ));
        pre_w_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        pre_w_bits.extend_from_slice(&exact_scalar_door_placeable_update_bits());
    }

    let mut pre_w_payload = live_object_payload_with_bits(&pre_w_live, pre_w_bits.clone());
    let pre_w_rewrite = super::rewrite_update_records_payload_if_possible(&mut pre_w_payload)
        .expect("the pre-W compact/full-update run should own its exact source cursor");
    assert_eq!(pre_w_rewrite.update_records_rewritten, 0);
    let pre_w_claim = super::claim_payload_if_verified(&pre_w_payload)
        .expect("the pre-W compact/full-update run should exact-claim by itself");
    assert_eq!(pre_w_claim.add_records, pre_w_object_ids.len() as u32);
    assert_eq!(pre_w_claim.update_records, pre_w_object_ids.len() as u32);

    let storage_object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1A00u32 + index).collect();
    let mut storage_bits = Vec::new();
    for (index, _) in storage_object_ids.iter().enumerate() {
        storage_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        storage_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }
    let storage_live =
        work_remaining_compact_stale_gap_pairs_with_storage(&storage_object_ids, &storage_bits);

    let mut positive_live = pre_w_live.clone();
    positive_live.extend_from_slice(&storage_live);
    let mut positive_payload = live_object_payload_with_bits(&positive_live, pre_w_bits.clone());
    let positive_rewrite = super::rewrite_update_records_payload_if_possible(&mut positive_payload)
        .expect("pre-W full-update run plus long storage/stale-gap run should exact-rewrite");
    assert_eq!(positive_rewrite.interleaved_fragment_spans_promoted, 1);
    let positive_claim = super::claim_payload_if_verified(&positive_payload)
        .expect("pre-W full-update run plus long promoted storage should exact-claim");
    assert_eq!(positive_claim.world_status_records, 1);
    assert_eq!(
        positive_claim.add_records,
        (pre_w_object_ids.len() + storage_object_ids.len()) as u32
    );
    assert_eq!(
        positive_claim.update_records,
        (pre_w_object_ids.len() + storage_object_ids.len()) as u32
    );

    let shifted_object_id = 0x8000_0001u32;
    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut shifted_live = positive_live;
    shifted_live.extend_from_slice(&shifted_add);
    shifted_live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));

    let mut bits = pre_w_bits;
    bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut payload = live_object_payload_with_bits(&shifted_live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "a pre-W full-update run must not resync a later shifted low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed pre-W/full-update/storage proof must leave source bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the shifted handoff remains active even after a proven pre-W full-update run"
    );
}

#[test]
fn leading_creature_and_door_run_does_not_resync_shifted_low_tail() {
    // The XP2 seq19 private replay starts with two EE-shaped `U/05` creature
    // visual-transform rows and a door add/full-update pair before the compact
    // placeable full-update run. Those rows own no compact-placeable source
    // bits, so they cannot donate, skip, or resync the later shifted
    // compact-add plus low-tail update handoff.
    let mut leading_live = ee_creature_visual_transform_update_live_bytes(0x8000_123C, 0);
    leading_live.extend_from_slice(&ee_creature_visual_transform_update_live_bytes(
        0x8000_1250,
        0,
    ));
    let door_object_id = 0x8000_11FDu32;
    leading_live.extend_from_slice(&door_direct_name_add_live_bytes_without_visual_map(
        door_object_id,
    ));
    leading_live.extend_from_slice(&with_live_update_object_id(
        ee_door_placeable_full_update_live_bytes(super::DOOR_OBJECT_TYPE),
        door_object_id,
    ));

    let mut leading_bits = vec![true, false, true, false, false, true, true];
    leading_bits.extend_from_slice(&exact_scalar_door_placeable_update_bits());
    let mut leading_payload = live_object_payload_with_bits(&leading_live, leading_bits.clone());
    let leading_rewrite = super::rewrite_update_records_payload_if_possible(&mut leading_payload)
        .expect("leading creature/door run should own its exact cursor");
    assert_eq!(
        leading_rewrite.bytes_inserted,
        super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN as u32
    );
    let leading_claim = super::claim_payload_if_verified(&leading_payload)
        .expect("leading creature/door run should exact-claim by itself");
    assert_eq!(leading_claim.creature_visual_transform_update_records, 2);
    assert_eq!(leading_claim.add_records, 1);
    assert_eq!(leading_claim.update_records, 1);

    let pre_w_object_ids: Vec<u32> = (0..8).map(|index| 0x8000_1900u32 + index).collect();
    let compact_prefixes: &[&[bool]] = &[
        &[true, false, true, false],
        &[false, false, false, true],
        &[true, true, false, true],
        &[false, false, true, false],
    ];

    let mut pre_w_live = Vec::new();
    let mut pre_w_bits = Vec::new();
    for (index, object_id) in pre_w_object_ids.iter().copied().enumerate() {
        pre_w_live.extend_from_slice(&with_live_update_object_id(
            compact_placeable_token_name_add_live_bytes(),
            object_id,
        ));
        pre_w_live.extend_from_slice(&with_live_update_object_id(
            ee_door_placeable_full_update_live_bytes(super::PLACEABLE_OBJECT_TYPE),
            object_id,
        ));
        pre_w_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        pre_w_bits.extend_from_slice(&exact_scalar_door_placeable_update_bits());
    }

    let storage_object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1A00u32 + index).collect();
    let mut storage_bits = Vec::new();
    for (index, _) in storage_object_ids.iter().enumerate() {
        storage_bits.extend_from_slice(compact_prefixes[index % compact_prefixes.len()]);
        storage_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }
    let storage_live =
        work_remaining_compact_stale_gap_pairs_with_storage(&storage_object_ids, &storage_bits);

    let mut positive_live = leading_live.clone();
    positive_live.extend_from_slice(&pre_w_live);
    positive_live.extend_from_slice(&storage_live);
    let mut positive_bits = leading_bits.clone();
    positive_bits.extend_from_slice(&pre_w_bits);
    let mut positive_payload = live_object_payload_with_bits(&positive_live, positive_bits);
    let positive_rewrite = super::rewrite_update_records_payload_if_possible(&mut positive_payload)
        .expect("leading run plus pre-W/storage rows should exact-rewrite");
    assert_eq!(positive_rewrite.interleaved_fragment_spans_promoted, 1);
    let positive_claim = super::claim_payload_if_verified(&positive_payload)
        .expect("leading run plus pre-W/storage rows should exact-claim");
    assert_eq!(positive_claim.creature_visual_transform_update_records, 2);
    assert_eq!(
        positive_claim.add_records,
        (1 + pre_w_object_ids.len() + storage_object_ids.len()) as u32
    );
    assert_eq!(
        positive_claim.update_records,
        (1 + pre_w_object_ids.len() + storage_object_ids.len()) as u32
    );
    assert_eq!(positive_claim.world_status_records, 1);

    let shifted_object_id = 0x8000_0001u32;
    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut shifted_live = positive_live;
    shifted_live.extend_from_slice(&shifted_add);
    shifted_live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));

    let mut shifted_bits = leading_bits;
    shifted_bits.extend_from_slice(&pre_w_bits);
    shifted_bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut payload = live_object_payload_with_bits(&shifted_live, shifted_bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "leading creature/door rows must not resync a later shifted low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed leading/pre-W/storage/low-tail proof must leave source bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the shifted handoff remains active even after the leading creature/door rows"
    );
}

#[test]
fn work_remaining_long_storage_span_rolls_back_with_one_unowned_bit() {
    // A long bounded post-`W` storage span must still be exact at the bit
    // cursor. One extra bit after the repeated compact/stale-gap run is active
    // evidence for an upstream owner or stream-boundary artifact, not terminal
    // storage that the `W` row or the repeated rows may trim.
    let object_ids: Vec<u32> = (0..15).map(|index| 0x8000_1300u32 + index).collect();
    let mut source_bits = Vec::new();
    for _ in &object_ids {
        source_bits.extend_from_slice(&[true, false, true, false]);
        source_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }
    source_bits.push(true);

    let live = work_remaining_compact_stale_gap_pairs_with_storage(&object_ids, &source_bits);
    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "one unowned bit must roll back the long post-W storage candidate"
    );
    assert_eq!(
        payload, original,
        "failed long-storage proof must leave bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the extra long-storage bit remains active cursor evidence"
    );
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
fn work_remaining_terminal_storage_rejects_nonzero_unowned_bits() {
    // `W current total` remains fragment-neutral even when the trailing bytes
    // decode as a CNW fragment-storage span. Terminal cleanup may drop an empty
    // all-zero storage shell, but nonzero payload bits need a following
    // decompile-owned family reader; otherwise they are active cursor evidence.
    let live = [b'W', 0x10, 0x20, 0xF8];
    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "terminal nonzero storage bytes after W must block raw exact claim"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "terminal W storage cleanup must not trim nonzero unowned fragment bits"
    );
    assert_eq!(
        payload, original,
        "nonzero terminal W storage evidence must stay visible for quarantine"
    );
}

#[test]
fn work_remaining_terminal_storage_after_source_update_rewrite_is_bounded_transport_tail() {
    // The final `W current total` still owns no BOOLs. This shape is accepted
    // only because the immediately preceding placeable update removes a
    // decompile-unowned legacy low-tail byte suffix, reaches an exact cursor,
    // and the W-legal candidate exact-claims after the bounded CNW storage span
    // is dropped.
    let mut live = door_placeable_low_tail_update_live_bytes(
        super::PLACEABLE_OBJECT_TYPE,
        &[0x7B, 0x74, 0x01, 0x00],
    );
    live.extend_from_slice(&[b'W', 0x0E, 0x0E]);
    let storage_payload_bits = [true, false, true, true, false, true, false, true, false];
    let mut storage_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    storage_bits.extend_from_slice(&storage_payload_bits);
    let storage = super::bits::pack_msb_valid_bits(storage_bits, super::CNW_FRAGMENT_HEADER_BITS);
    live.extend_from_slice(&storage);

    let mut source_bits = scalar_door_placeable_update_bits();
    source_bits.extend_from_slice(&storage_payload_bits);
    let mut payload = live_object_payload_with_bits(&live, source_bits);

    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("terminal W storage should trim only after the preceding update rewrite is exact");
    assert!(summary.fragment_bits_trimmed >= storage_payload_bits.len() as u32);
    assert!(summary.bytes_removed >= storage.len() as u32);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("terminal W storage trim must leave an exact EE stream");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.world_status_records, 1);
}

#[test]
fn work_remaining_terminal_storage_after_exact_cursor_update_rewrite_is_byte_tail() {
    // CEP-style zero-declared streams can carry bounded storage bytes after a
    // terminal `W current total` even when the preceding source rewrite lands
    // exactly at the end of the real fragment cursor. The storage bytes are
    // still not W payload; they can be dropped only after the W-legal stream
    // exact-claims with the already-consumed cursor.
    let mut live = door_placeable_low_tail_update_live_bytes(
        super::PLACEABLE_OBJECT_TYPE,
        &[0x7B, 0x74, 0x01, 0x00],
    );
    live.extend_from_slice(&[b'W', 0x0E, 0x0E]);
    let storage_payload_bits = [true, false, true, true, false, true, false, true, false];
    let mut storage_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    storage_bits.extend_from_slice(&storage_payload_bits);
    let storage = super::bits::pack_msb_valid_bits(storage_bits, super::CNW_FRAGMENT_HEADER_BITS);
    live.extend_from_slice(&storage);

    let mut payload = live_object_payload_with_bits(&live, scalar_door_placeable_update_bits());

    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("terminal W storage bytes should drop after an exact-cursor source rewrite");
    assert_eq!(
        summary.fragment_bits_trimmed, 0,
        "the exact-cursor case removes only terminal storage bytes"
    );
    assert!(summary.bytes_removed >= storage.len() as u32);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("byte-only terminal W storage cleanup must leave an exact EE stream");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.world_status_records, 1);
}

#[test]
fn work_remaining_fragment_span_promoter_ignores_w_inside_gui_read_buffer() {
    // The pre-loop post-W span repair may only use a top-level `W current total`
    // boundary. Diamond `sub_4589A0` / EE `sub_1407B3F30` read `G I U` as one
    // ten-byte GUI row, and bytes inside its OBJECTID can legally spell
    // `W current total`; those bytes are not a work-remaining suffix and must
    // not be truncated as fragment storage.
    let live = [
        b'G', b'I', b'U', 0x57, 0x10, 0x20, 0x80, 0x44, 0x33, 0x55, 0xA0,
    ];
    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the extra terminal byte after GUI must block the raw exact claim"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "post-W repair must not fire on W-shaped bytes inside a GUI row"
    );
    assert_eq!(
        payload, original,
        "W-shaped GUI object-id bytes must remain visible for quarantine"
    );
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
fn live_gui_quickbar_link_row_requires_object_id_at_row_offset_two() {
    // `G Q` is read-buffer-only, but its row boundary is still byte-exact:
    // Diamond `sub_4589A0` / EE `sub_1407B3F30` read count, then nine-byte rows
    // whose OBJECTID begins at row offset +2. A row whose only plausible id is
    // one byte earlier is shifted cursor evidence, not proof of a quickbar-link
    // row.
    let mut exact = vec![b'G', b'Q', 1, 0xAA, 0xBB];
    exact.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    exact.extend_from_slice(&[0x10, 0x20, 0x30]);
    let exact_payload = live_gui_read_buffer_payload(&exact);
    let exact_claim = super::claim_payload_if_verified(&exact_payload)
        .expect("GQ row should claim when object id starts at row offset +2");
    assert_eq!(exact_claim.live_gui_read_buffer_records, 1);
    assert_eq!(exact_claim.live_gui_fragment_bits, 0);
    assert_eq!(exact_claim.live_bytes_length, 12);

    let mut shifted = vec![b'G', b'Q', 1, 0xAA];
    shifted.extend_from_slice(&0x0100_0000u32.to_le_bytes());
    shifted.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]);
    let shifted_payload = live_gui_read_buffer_payload(&shifted);
    assert!(
        super::claim_payload_if_verified(&shifted_payload).is_none(),
        "GQ must not accept a row whose only plausible object id begins at row offset +1"
    );
}

#[test]
fn live_gui_quickbar_link_auxiliary_row_bytes_are_length_only() {
    // Diamond `sub_458850` and EE `sub_1407B4390` both read each `GQ` row as
    // two discarded BYTEs, raw object id, BYTE quickbar button, WORD use count.
    // The client-side rejection path is overflow/object-lookup based; these
    // auxiliary bytes are not range gates for the live-object boundary claim.
    let mut live = vec![b'G', b'Q', 2];
    live.extend_from_slice(&[0x00, 0xFF]);
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.push(0xFF);
    live.extend_from_slice(&0xFFFFu16.to_le_bytes());
    live.extend_from_slice(&[0xFF, 0x00]);
    live.extend_from_slice(&0x8001_2346u32.to_le_bytes());
    live.push(0x00);
    live.extend_from_slice(&0x0000u16.to_le_bytes());

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("GQ auxiliary row bytes should not reject the fixed row boundary");

    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 21);
    assert_eq!(
        super::gui::verified_item_materialization_object_ids(&live, 0, live.len()),
        vec![0x8001_2345, 0x8001_2346],
        "materialization must still extract only the row-offset +2 object ids"
    );
}

#[test]
fn live_gui_missing_inventory_add_opcode_rejects_unproven_item_name_bits() {
    // This is the generalized terminal-GI hazard from local XP2 evidence: the
    // row bytes can expose plausible no-name and token-name item endpoints, but
    // the nested item body still owns at least four source BOOLs. If the
    // inherited fragment cursor has fewer bits available, neither Diamond nor EE
    // may promote nearby bytes or choose a neighboring cursor just to make the
    // row validate.
    let live = [
        b'G', b'I', 0x00, 0x00, 0x00, 0x00, 0x00, 0x76, 0x12, 0x00, 0x80, 0x10, 0x00, 0x00, 0x00,
        0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x08, 0x10, 0x0B, 0x01, 0x10, 0x10, 0x04, 0x04, 0x0F,
        0x0F, 0x01, 0x01, 0x00, 0x0F, 0x0C, 0x0F, 0x14, 0x00, 0x00, 0x00, 0x00, 0x0B, 0x36, 0x00,
        0x00, 0xA6, 0x1A, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, b'G', b'I', 0x00, 0x10, 0x00, 0x00, 0x00, 0x77, 0x12, 0x00,
        0x80,
    ];
    let mut payload = live_object_payload_with_bits(&live, vec![false, false, false]);
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw shifted GUI item-create row must stay unclaimed"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "GUI item-create rewrite must not invent name-branch proof from shifted bits"
    );
    assert_eq!(
        payload, original,
        "failed GUI item-create proof must leave bytes and fragment bits untouched"
    );
}

#[test]
fn live_gui_missing_inventory_add_opcode_is_not_byte_only_boundary() {
    // `G I/i 00` is a repairable Diamond capture quirk only after the focused
    // item-create parser proves the row's fragment bits. A byte-only boundary
    // scan must not treat the zero inner opcode as equivalent to the explicit
    // `G I/i A` row, or shifted terminal evidence can become false alignment
    // proof for following live-object records.
    let explicit = legacy_width_gui_inventory_model_type2_item_create_live_bytes();
    assert_eq!(
        super::gui::try_get_legacy_live_gui_record_end(&explicit, 0, explicit.len()),
        Some(explicit.len()),
        "explicit G I A item-create rows remain valid byte-only GUI boundaries"
    );

    let mut missing = explicit;
    missing[2] = 0x00;
    assert!(
        super::gui::looks_like_legacy_live_gui_rewrite_boundary(&missing, 0),
        "the focused rewrite path still recognizes the missing-opcode candidate"
    );
    assert_eq!(
        super::gui::try_get_legacy_live_gui_record_end(&missing, 0, missing.len()),
        None,
        "unproven G I 00 rows must not be claimed by byte-only GUI boundary scans"
    );
    assert_eq!(
        super::gui::try_get_legacy_live_gui_item_create_read_end(&missing, 0, missing.len()),
        None,
        "the GUI item read-end fallback also needs an explicit inner A without fragment proof"
    );
    assert!(
        !super::boundary::looks_like_legacy_live_object_sub_message_boundary(&missing, 0),
        "generic live-object boundary scans must leave G I 00 to the focused proof path"
    );
}

#[test]
fn live_gui_missing_inventory_add_opcode_rewrites_only_with_item_bit_proof() {
    // Diamond/EE GUI handlers dispatch `G I/i A` into the shared item-create
    // reader. Local Diamond captures can lose that inner `A`, but the zero byte
    // is repairable only when the inherited CNW cursor proves the item body and
    // active-property bits at the exact row boundary.
    let mut live = ee_shaped_gui_inventory_model_type2_item_create_live_bytes();
    live[2] = 0x00;
    let item_bits = vec![false, false, false, true, false, false];
    let mut payload = live_object_payload_with_bits(&live, item_bits.clone());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the exact EE GUI validator must not accept a missing inner add opcode"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("proven missing GUI add opcode should be repaired");
    assert_eq!(rewrite.live_gui_missing_add_opcodes_repaired, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("rewritten live-object declared length") as usize;
    assert_eq!(
        payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + 2],
        b'A'
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("repaired GUI item-create row should exact-claim");
    assert_eq!(claim.live_gui_item_create_records, 1);
    assert_eq!(claim.live_gui_fragment_bits, item_bits.len() as u32);
    assert_eq!(
        claim.live_bytes_length + super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES,
        declared
    );
}

#[test]
fn exact_adapter_rolls_back_prior_update_before_terminal_gui_missing_item_bits() {
    // Generalized XP2 seq19 terminal handoff: a prior legacy update may stage
    // a valid EE bit insertion, and `W current total` may follow it, but W is
    // fragment-neutral (`sub_44F160` / `sub_1407B85A0`). A following `G I 00`
    // still needs exact item-create bits at its own row boundary.
    let mut live = door_state_update_live_bytes();
    live.extend_from_slice(&[b'W', 0x0E, 0x0E]);
    let mut gui = ee_shaped_gui_inventory_model_type2_item_create_live_bytes();
    gui[2] = 0x00;
    live.extend_from_slice(&gui);

    let mut payload = live_object_payload_with_bits(
        &live,
        vec![true, false, true, false, true], // Diamond door state bits only.
    );
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw stream is missing EE's door state bit and GUI item proof"
    );
    assert!(
        !crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
            &mut payload,
            None
        ),
        "adapter must not emit staged update rewrites when terminal G I 00 has no item bits"
    );
    assert_eq!(
        payload, original,
        "failed terminal GUI proof must roll back the earlier update rewrite"
    );
}

#[test]
fn live_gui_terminal_item_fragment_span_promotes_already_ee_shaped_item_bits() {
    // GUI inventory item-create rows use the same item-create helper as typed
    // `A/6` rows after the `G I A` prefix. Interleaved CNW fragment storage can
    // therefore carry only the item-name/active-property bits even when the
    // item body already has EE appearance and visual-transform bytes; promotion
    // is still valid only after exact EE item validation consumes those bits.
    let mut live = ee_shaped_gui_inventory_model_type2_item_create_live_bytes();
    let gui_item_len = live.len();
    let item_bits = [false, false, false, true, false, false];
    let mut storage_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    storage_bits.extend_from_slice(&item_bits);
    let storage = super::bits::pack_msb_valid_bits(storage_bits, super::CNW_FRAGMENT_HEADER_BITS);
    live.extend_from_slice(&storage);

    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw GUI row has its item fragment bits stranded in the read buffer"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("terminal GUI item fragment storage should promote through exact item proof");
    assert_eq!(rewrite.interleaved_fragment_spans_promoted, 1);
    assert_eq!(
        rewrite.interleaved_fragment_bytes_promoted,
        storage.len() as u32
    );
    assert_eq!(
        rewrite.interleaved_fragment_bits_promoted,
        item_bits.len() as u32
    );
    assert_eq!(rewrite.new_live_bytes_length, gui_item_len);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("promoted GUI item-create row should exact-claim");
    assert_eq!(claim.live_gui_item_create_records, 1);
    assert_eq!(claim.live_gui_fragment_bits, item_bits.len() as u32);
    assert_eq!(claim.live_bytes_length, gui_item_len);
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
fn legacy_low_tail_update_splits_before_following_compact_add() {
    // XP2-style door/placeable streams can put another live-object row
    // immediately after the bounded low 0x40/0x80 control suffix. The suffix
    // is still owned by the same typed low-tail rule as terminal rows; it must
    // not be treated as an inline-string span that swallows the following add.
    // The compact add itself is only source-backed once the next same-object
    // update proves the cursor handoff, so keep the update/add/update shape.
    let object_id = 0x8000_18CAu32;
    let mut live = with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        object_id,
    );
    live.extend_from_slice(&compact_placeable_token_name_add_live_bytes());
    live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(object_id));

    let mut bits = scalar_door_placeable_update_bits();
    bits.extend_from_slice(&[true, false, true, false]);
    bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("low-tail update should split before the following compact add");
    assert!(
        rewrite.update_records_rewritten >= 2,
        "both door/placeable updates should be rewritten without losing the compact add boundary"
    );
    assert!(
        rewrite.bytes_removed >= 4,
        "only the bounded low-tail suffix should be removed from the update"
    );
    assert!(
        rewrite.bytes_inserted
            >= super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN as u32,
        "following compact add should receive EE's visual-transform map"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten low-tail update plus compact add should exact-claim");
    assert_eq!(claim.update_records, 2);
    assert_eq!(claim.add_records, 1);
}

#[test]
fn compact_placeable_token_add_can_own_selector_bits_before_low_tail_update() {
    // XP2 placeable rows can encode a four-byte short-name/token slot, then
    // carry two source-only selector bits before the four compact tail BOOLs
    // hand off to the following same-object low-tail update. The add repair may
    // drop those two extra source bits only when the update exact-validates at
    // the resulting cursor. The same-object proof accepts Diamond's compact
    // OBJECTID form because EE canonicalization runs after exact claim.
    let object_id = 0x8000_18CAu32;
    let compact_object_id = object_id & !0x8000_0000;
    let mut live = with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        object_id,
    );
    live.extend_from_slice(&compact_placeable_token_name_add_live_bytes());
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        compact_object_id,
    ));

    let mut bits = scalar_door_placeable_update_bits();
    bits.extend_from_slice(&[false, false, false, false, false, false]);
    bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("token add should prove the six-bit compact source before the low-tail update");
    assert_eq!(
        rewrite.bits_inserted, 8,
        "two low-tail updates insert one EE state bit each, while the six-bit compact add nets six EE bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten low-tail update/add/update sequence should exact-claim");
    assert_eq!(claim.update_records, 2);
    assert_eq!(claim.add_records, 1);
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
fn work_remaining_suffix_does_not_let_low_tail_update_trim_extra_fragment_bit() {
    // `W current total` is fragment-neutral in Diamond `sub_44F160` and EE
    // `sub_1407B85A0`. A bounded legacy low-tail `U/9` or `U/10` may rewrite
    // before W when its own source cursor is exact, but the following W row does
    // not make an extra post-update fragment bit terminal-family storage.
    for object_type in [super::DOOR_OBJECT_TYPE, super::PLACEABLE_OBJECT_TYPE] {
        let mut live = door_placeable_low_tail_update_live_bytes(object_type, &[0x34, 0x12, 0, 0]);
        live.extend_from_slice(&[b'W', 0x0C, 0x20]);

        let mut exact_payload =
            live_object_payload_with_bits(&live, scalar_door_placeable_update_bits());
        let rewrite = super::rewrite_update_records_payload_if_possible(&mut exact_payload)
            .expect("bounded low-tail update followed by W should rewrite when bits are exact");
        assert_eq!(rewrite.update_records_rewritten, 1);
        let claim = super::claim_payload_if_verified(&exact_payload)
            .expect("rewritten low-tail update followed by W should exact-claim");
        assert_eq!(claim.update_records, 1);
        assert_eq!(claim.world_status_records, 1);

        let mut shifted_bits = scalar_door_placeable_update_bits();
        shifted_bits.push(true);
        let mut shifted_payload = live_object_payload_with_bits(&live, shifted_bits);
        let original = shifted_payload.clone();

        assert!(
            super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
            "W must not let object type {object_type:#04X} trim an unowned post-update bit"
        );
        assert_eq!(
            shifted_payload, original,
            "failed U/9-W low-tail rewrite must leave the evidence payload unchanged"
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
fn item_update_position_body_w_bytes_stay_inside_update_record() {
    // Diamond `sub_459700 -> sub_467AE0` consumes the full six-byte position
    // body before the live-object dispatcher can see the next opcode. The first
    // three position bytes can legally spell a `W current total` row, but they
    // are not a top-level work-remaining record.
    let live = item_update_position_live_bytes([b'W', 0x10, 0x20, 0xAA, 0xBB, 0xCC]);
    let payload = live_object_payload_with_bits(&live, vec![true, false]);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("item position update should own W-shaped position bytes");

    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.world_status_records, 0);
    assert_eq!(claim.live_bytes_length, live.len());

    let bit_short = live_object_payload_with_bits(&live, vec![true]);
    assert!(
        super::claim_payload_if_verified(&bit_short).is_none(),
        "the transport boundary must not replace the two position fragment bits"
    );
}

#[test]
fn item_update_position_hands_off_after_full_position_body() {
    let mut live = item_update_position_live_bytes([b'W', 0x10, 0x20, 0xAA, 0xBB, 0xCC]);
    live.extend_from_slice(&[b'D', super::ITEM_OBJECT_TYPE]);
    live.extend_from_slice(&0x8000_2201u32.to_le_bytes());

    let payload = live_object_payload_with_bits(
        &live,
        vec![
            true, false, // item position residual bits.
            true,  // D/6 owns one delete BOOL.
        ],
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("item update should hand off only after all position bytes");

    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.delete_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
}

#[test]
fn item_update_scalar_vector_boundary_ambiguity_stays_unclaimed() {
    // The transport scanner has no orientation BOOL. A scalar item cursor can
    // land on bytes that look exactly like `W current total` while the vector
    // cursor lands on the real following `D/6` boundary. Diamond `sub_467AE0`
    // and EE `sub_14079C050` choose that branch from the fragment bit before
    // reading orientation bytes, so byte-only splitting must keep this
    // ambiguity visible instead of claiming the shorter scalar-looking record.
    let mask = super::LEGACY_UPDATE_POSITION_MASK | super::LEGACY_UPDATE_ORIENTATION_MASK;
    let mut live = vec![b'U', super::ITEM_OBJECT_TYPE];
    live.extend_from_slice(&0x8000_2201u32.to_le_bytes());
    live.extend_from_slice(&mask.to_le_bytes());
    live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    live.extend_from_slice(&[0x70, b'W', 0x0C, 0x0E, 0x88, 0x99]);
    live.extend_from_slice(&[b'D', super::ITEM_OBJECT_TYPE]);
    live.extend_from_slice(&0x8000_2201u32.to_le_bytes());

    let mut payload = live_object_payload_with_bits(
        &live,
        vec![
            true, false, // item position residual bits.
            true,  // vector orientation selector.
            false, // D/6 delete BOOL if the vector boundary were proven.
        ],
    );
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "scalar/vector item boundary ambiguity must not exact-claim through an internal W row"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "rewrite must not split a vector-selected item update at scalar-looking W bytes"
    );
    assert_eq!(
        payload, original,
        "failed ambiguous item boundary proof must leave source bytes and bits untouched"
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
fn item_update_locstring_token_name_owns_token_selector_before_hidden_bool() {
    // Diamond `sub_451AF0` and EE `sub_14076BD30` both read the outer item-name
    // selector first. When it selects the locstring helper, the next fragment
    // bit selects the client-TLK/token branch before the item hidden-state
    // BOOL. The read-buffer payload is the selector BYTE plus DWORD token.
    let mask = super::LEGACY_UPDATE_NAME_MASK | 0x0000_0040;
    let live = item_update_locstring_token_name_live_bytes(mask, 1, 0x0100_75D6);

    let payload = live_object_payload_with_bits(&live, vec![true, true, false]);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("locstring-token item-name plus hidden BOOL should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());

    let missing_hidden = live_object_payload_with_bits(&live, vec![true, true]);
    assert!(
        super::claim_payload_if_verified(&missing_hidden).is_none(),
        "the hidden-state BOOL is read after the locstring token selector"
    );

    let extra_terminal = live_object_payload_with_bits(&live, vec![true, true, false, true]);
    assert!(
        super::claim_payload_if_verified(&extra_terminal).is_none(),
        "token item-name updates must not hide a terminal fragment bit after hidden state"
    );
}

#[test]
fn item_update_locstring_token_name_hands_off_after_token_payload() {
    let mut live =
        item_update_locstring_token_name_live_bytes(super::LEGACY_UPDATE_NAME_MASK, 0, 0x0100_380A);
    live.extend_from_slice(&[b'D', super::ITEM_OBJECT_TYPE]);
    live.extend_from_slice(&0x8000_2201u32.to_le_bytes());

    let payload = live_object_payload_with_bits(
        &live,
        vec![
            true, // item name uses the locstring helper.
            true, // locstring helper uses selector BYTE + DWORD token.
            true, // following D/6 delete owns one BOOL.
        ],
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("item token-name update should hand off only after the token payload");

    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.delete_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
}

#[test]
fn item_full_update_scalar_direct_name_rewrites_mask_without_moving_cursor() {
    // Diamond `sub_459700 -> sub_467AE0 -> sub_451AF0` and EE
    // `sub_1407B8380 -> sub_14079C050 -> sub_1407A08F0` agree on the low
    // update-body order: position, orientation selector/body, appearance, state
    // bits, item name, then EE's hidden-state BOOL. The raw Diamond full mask
    // is translated to that EE mask only when the same fragment cursor proves
    // every branch.
    let live = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    let mut payload =
        live_object_payload_with_bits(&live, item_update_full_mask_scalar_direct_name_bits());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the raw Diamond all-bits item mask is not an exact EE update"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("decompile-shaped scalar full item update should translate its mask");

    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.masks_translated, 1);
    assert_eq!(rewrite.bits_inserted, 0);
    assert_eq!(rewrite.bits_removed, 0);
    assert_eq!(rewrite.bytes_inserted, 0);
    assert_eq!(rewrite.bytes_removed, 0);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let rewritten_live =
        &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared];
    assert_eq!(
        super::read_u32_le(rewritten_live, 6),
        Some(0x0008_0073),
        "translated EE mask keeps position/orientation/appearance/state/name/hidden only"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("translated scalar full item update should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
}

#[test]
fn item_full_update_scalar_locstring_inline_rewrites_mask_without_moving_cursor() {
    // The full-mask item update uses the same decompiled name branch as the
    // narrower U/6 name tests. The outer locstring selector owns one extra
    // fragment bit before the inline CExoString bytes; the following hidden BOOL
    // remains after that component selector, not at the direct-name cursor.
    let live = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    let following_bits = item_update_full_mask_scalar_locstring_inline_bits();
    let mut payload = live_object_payload_with_bits(&live, following_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the raw Diamond all-bits item mask is not an exact EE update"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("decompile-shaped locstring-inline full item update should translate its mask");

    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.masks_translated, 1);
    assert_eq!(rewrite.bits_inserted, 0);
    assert_eq!(rewrite.bits_removed, 0);
    assert_eq!(rewrite.bytes_inserted, 0);
    assert_eq!(rewrite.bytes_removed, 0);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let rewritten_live =
        &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared];
    assert_eq!(
        super::read_u32_le(rewritten_live, 6),
        Some(0x0008_0073),
        "translated EE mask keeps position/orientation/appearance/state/name/hidden only"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("translated locstring-inline full item update should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
}

#[test]
fn item_full_update_scalar_locstring_token_rewrites_mask_without_moving_cursor() {
    // Diamond `sub_451AF0` and EE `sub_14076BD30` read the full item name
    // branch as outer locstring selector, token/client-TLK selector bit, one
    // read-buffer selector BYTE, and a DWORD token before EE's hidden-state
    // BOOL. The token payload must not be mistaken for direct CExoString bytes
    // or for fragment storage owned by a neighboring record.
    let live = item_update_full_mask_scalar_locstring_token_live_bytes(1, 0x0100_75D6);
    let following_bits = item_update_full_mask_scalar_locstring_token_bits();
    let mut payload = live_object_payload_with_bits(&live, following_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the raw Diamond all-bits item mask is not an exact EE update"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("decompile-shaped locstring-token full item update should translate its mask");

    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.masks_translated, 1);
    assert_eq!(rewrite.bits_inserted, 0);
    assert_eq!(rewrite.bits_removed, 0);
    assert_eq!(rewrite.bytes_inserted, 0);
    assert_eq!(rewrite.bytes_removed, 0);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let rewritten_live =
        &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared];
    assert_eq!(
        super::read_u32_le(rewritten_live, 6),
        Some(0x0008_0073),
        "translated EE mask keeps position/orientation/appearance/state/name/hidden only"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("translated locstring-token full item update should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
}

#[test]
fn item_full_update_vector_direct_name_rewrites_mask_without_moving_cursor() {
    // The vector branch is selected by the orientation BOOL before any
    // orientation bytes are read. Diamond `sub_467AE0` and EE `sub_14079C050`
    // both consume the six vector bytes, then resume at appearance/state/name
    // without the scalar branch's four residual orientation bits.
    let live = item_update_full_mask_vector_direct_name_live_bytes(b"Lance");
    let following_bits = item_update_full_mask_vector_direct_name_bits();
    let mut payload = live_object_payload_with_bits(&live, following_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the raw Diamond all-bits item mask is not an exact EE update"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("decompile-shaped vector full item update should translate its mask");

    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.masks_translated, 1);
    assert_eq!(rewrite.bits_inserted, 0);
    assert_eq!(rewrite.bits_removed, 0);
    assert_eq!(rewrite.bytes_inserted, 0);
    assert_eq!(rewrite.bytes_removed, 0);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let rewritten_live =
        &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared];
    assert_eq!(
        super::read_u32_le(rewritten_live, 6),
        Some(0x0008_0073),
        "translated EE mask keeps position/orientation/appearance/state/name/hidden only"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("translated vector full item update should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());
}

#[test]
fn item_full_update_vector_selector_cannot_claim_scalar_direct_name_bytes() {
    // The same read-buffer bytes can look scalar by inspection, but the
    // decompiled Diamond/EE generic update reader branches on the orientation
    // BOOL first. A true selector must consume the six vector bytes; the item
    // translator must not relabel that bit to rescue a later direct-name shape.
    let live = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    let vector_selector_bits = vec![
        false, true, // position residual bits.
        true, // vector orientation selector.
        true, false, true, false, true,  // state bits that would follow vector orientation.
        false, // direct name if the read cursor were still plausible.
        true,  // hidden BOOL.
    ];
    let mut payload = live_object_payload_with_bits(&live, vector_selector_bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "orientation BOOL order is decompile-owned and cannot be repaired from scalar-looking bytes"
    );
    assert_eq!(
        payload, original,
        "rejected shifted item update must leave bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "vector-selector/scalar-byte item update must remain quarantinable"
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
fn creature_4408_inventory_2a00_gq_terminal_bit_rolls_back_prior_rewrite() {
    // Generalizes the XP1/To Heir terminal-tail captures without fixture bytes.
    // The first record is a compact Diamond `U/5 0x4408` status update that
    // the typed creature repair can rewrite exactly. The following
    // `I/0x2A00` and `GQ` records then own their own decompile-backed cursors.
    // A final extra bit after `GQ` is still unowned by those families, so the
    // earlier creature rewrite must be rolled back with the original payload
    // left visible for quarantine.
    let mut live =
        legacy_zero_count_creature_4408_live_bytes(&[(b'A', 0x00F3), (b'D', 0x00B6)], &[]);
    live.extend_from_slice(&inventory_2a00_word_list_live_bytes(
        &[0x0303],
        &[0xFFFF_FFFE],
        Some([
            0x0E, 0x0D, 0x0D, 0x0A, 0x13, 0x0A, 0x0C, 0x0D, 0x0F, 0x0A, 0x13, 0x0A,
        ]),
    ));
    live.extend_from_slice(&[b'G', b'Q', 0]);

    let mut owned_bits = vec![false; 7]; // Repaired `U/5 0x4408` status/scalar bits.
    owned_bits.extend([
        false, // 0x0200 semantic BOOL.
        false, // 0x0200 layout selector: DWORD count + WORD rows.
        true, false, true, // one Feature-25 second-list object.
        true, // 0x0800 present branch owns the twelve-byte tail.
    ]);

    let mut exact_payload = live_object_payload_with_bits(&live, owned_bits.clone());
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut exact_payload)
        .expect("creature 0x4408 plus exact inventory/GQ stream should rewrite");
    assert_eq!(
        rewrite.bytes_inserted, 16,
        "the leading compact creature update should receive two EE identity maps: {rewrite:?}"
    );
    let exact_claim = super::claim_payload_if_verified(&exact_payload)
        .expect("rewritten creature/inventory/GQ stream should exact-claim");
    assert_eq!(exact_claim.creature_update_records, 1);
    assert_eq!(exact_claim.inventory_records, 1);
    assert_eq!(exact_claim.live_gui_read_buffer_records, 1);

    let mut shifted_bits = owned_bits;
    shifted_bits.push(false);
    let mut shifted_payload = live_object_payload_with_bits(&live, shifted_bits);
    let original = shifted_payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
        "a prior U/5 repair must not authorize trimming the terminal bit after I/0x2A00 + GQ"
    );
    assert_eq!(
        shifted_payload, original,
        "failed terminal-tail proof must roll back the earlier creature rewrite"
    );
}

#[test]
fn trigger_add_owns_name_state_bits_before_geometry() {
    // Diamond `sub_4552E0` and EE `sub_1407B1670` read the same trigger add
    // order: name selector/payload, two state BOOLs, an optional third state
    // BOOL gated by the first state BOOL, then cursor/height/vertex geometry.
    // The proxy preserves the bytes but must still advance that fragment span.
    let live = trigger_add_live_bytes(2);
    let payload = live_object_payload_with_bits(
        &live,
        vec![
            true,  // locstring/token name branch
            false, // client-TLK selector bit
            true,  // first state BOOL gates the third state BOOL
            false, // second state BOOL
            true,  // optional third state BOOL
        ],
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("trigger add locstring/state bits should exact-claim");

    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.live_bytes_length, live.len());

    let missing_optional = live_object_payload_with_bits(&live, vec![true, false, true, false]);
    assert!(
        super::claim_payload_if_verified(&missing_optional).is_none(),
        "the first trigger state BOOL gates one more source BOOL"
    );

    let direct = direct_name_trigger_add_live_bytes(b"Gate", 1);
    let direct_payload = live_object_payload_with_bits(
        &direct,
        vec![
            false, // direct CExoString name branch
            false, // first state BOOL, so no optional third state BOOL
            true,  // second state BOOL
        ],
    );
    let direct_claim = super::claim_payload_if_verified(&direct_payload)
        .expect("direct-name trigger add should exact-claim with its dynamic geometry cursor");
    assert_eq!(direct_claim.add_records, 1);
    assert_eq!(direct_claim.live_bytes_length, direct.len());

    let shifted = live_object_payload_with_bits(&direct, vec![false, false, true, false]);
    assert!(
        super::claim_payload_if_verified(&shifted).is_none(),
        "trigger add must not hide a terminal fragment bit after its state span"
    );
}

#[test]
fn trigger_add_name_bits_must_match_byte_cursor_branch() {
    // The four-byte trigger token branch and direct CExoString branch can be
    // byte-plausible at different cursors. Diamond `sub_4552E0` and EE
    // `sub_1407B1670` choose that cursor from the name selector bit first, so
    // exact validation must reject a locstring selector that borrows the longer
    // direct-name geometry boundary.
    let live = ambiguous_direct_name_trigger_add_live_bytes();

    let direct_payload = live_object_payload_with_bits(&live, vec![false, false, true]);
    let direct_claim = super::claim_payload_if_verified(&direct_payload)
        .expect("direct-name trigger add should exact-claim on its CExoString cursor");
    assert_eq!(direct_claim.add_records, 1);
    assert_eq!(direct_claim.live_bytes_length, live.len());

    let mismatched_locstring = live_object_payload_with_bits(&live, vec![true, false, false, true]);
    assert!(
        super::claim_payload_if_verified(&mismatched_locstring).is_none(),
        "locstring/token trigger bits must not claim a direct-name byte boundary"
    );
}

#[test]
fn trigger_add_geometry_rejects_truncated_vertex_rows() {
    let mut live = trigger_add_live_bytes(1);
    live.truncate(live.len() - 1);
    let payload = live_object_payload_with_bits(&live, vec![true, false, false, true]);

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
fn compact_placeable_token_name_add_advances_legacy_tail_cursor_only() {
    let live = compact_placeable_token_name_add_live_bytes();
    let legacy_bits = vec![true, false, true, false];
    let mut bit_cursor = 0usize;

    assert_eq!(
        super::boundary::try_get_legacy_placeable_short_name_add_record_end_for_transport(
            &live,
            0,
            live.len(),
        ),
        Some(live.len()),
        "the compact token-name placeable add owns the byte cursor"
    );
    assert!(
        super::cursor::advance_legacy_add_record_bit_cursor_for_update_pass(
            &live,
            &legacy_bits,
            0,
            live.len(),
            &mut bit_cursor,
        )
    );
    assert_eq!(
        bit_cursor, 4,
        "Diamond compact placeable adds consume only the four legacy tail BOOLs"
    );

    bit_cursor = 0;
    assert!(
        !super::cursor::advance_live_add_record_bit_cursor(
            &live,
            &legacy_bits,
            0,
            live.len(),
            &mut bit_cursor,
        ),
        "raw token-name compact placeable adds are not already exact EE records"
    );
}

#[test]
fn empty_placeable_add_guard_repair_drains_compact_source_residue() {
    for residue_bits in 0usize..=4 {
        let live = ee_empty_placeable_add_live_bytes(0x0071);
        let residue = (0..residue_bits)
            .map(|index| index % 2 == 0)
            .collect::<Vec<_>>();
        let mut payload = live_object_payload_with_bits(&live, residue);

        let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
            .expect("compact placeable residue should rewrite to neutral EE add guards");
        assert!(
            rewrite.bits_inserted >= 7,
            "residue {residue_bits} should grow to the 11-bit EE guard run"
        );

        let claim =
            super::claim_payload_if_verified(&payload).expect("rewritten add should exact-claim");
        assert_eq!(claim.add_records, 1);
        let fragment_bits = super::bits::decode_msb_valid_bits(
            &payload[claim.declared..],
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .expect("rewritten placeable add fragment bits");
        assert_eq!(
            fragment_bits.len(),
            super::CNW_FRAGMENT_HEADER_BITS + 11,
            "residue {residue_bits} should be replaced, not appended"
        );
        assert!(
            fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..]
                .iter()
                .all(|bit| !*bit),
            "residue {residue_bits} must not leak into EE guard bits"
        );
    }
}

#[test]
fn empty_placeable_add_direct_name_repair_reuses_stale_inner_bit_as_state() {
    let live = ee_empty_placeable_add_live_bytes(0x0071);
    let mut payload = live_object_payload_with_bits(
        &live,
        vec![
            true, true, true, false, true, false, true, false, true, false, true,
        ],
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "outer=true/inner=true would route EE into the TLK helper, not the empty CExoString"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("direct empty-name placeable add bits should repair at the add cursor");
    assert!(
        rewrite.bits_inserted == 0 && rewrite.bits_removed == 0,
        "direct-name branch repair should change existing guard bits in place"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("repaired empty-name placeable add should exact-claim");
    assert_eq!(claim.add_records, 1);
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten placeable add fragment bits");
    let add_bits = &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..];
    assert_eq!(add_bits.len(), 11);
    assert!(
        !add_bits[0],
        "outer=false selects the direct CExoString reader"
    );
    assert!(
        add_bits[1],
        "the former stale inner selector remains the first post-name state bit"
    );
    assert!(
        !add_bits[2] && !add_bits[10],
        "absent optional-object guard and EE-only visual guard stay neutral"
    );
}

#[test]
fn ee_empty_placeable_add_does_not_borrow_following_update_bits() {
    let object_id = 0x8000_18C2u32;
    let mut live = ee_empty_placeable_add_live_bytes(0x0009);
    live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(object_id));

    let mut bits = vec![false, true, false, true];
    bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "exact add validation can otherwise borrow the following update's source bits"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("compact-source add bits should be repaired before the following update");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert!(
        rewrite.bits_inserted >= 8,
        "compact add repair plus 0x17 update repair should grow the bitstream"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("add/update pair should claim after bounded compact-source repair");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn compact_placeable_token_add_rewrites_before_following_same_object_update() {
    let object_id = 0x8000_18CAu32;
    let mut live = compact_placeable_token_name_add_live_bytes();
    live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(object_id));

    let mut bits = vec![true, false, true, false];
    bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw compact token-name placeable add is not an exact EE add row"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("compact placeable add should rewrite before the following update");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert!(
        rewrite.bytes_inserted
            >= super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN as u32,
        "compact add should receive EE's visual-transform map"
    );
    assert!(
        rewrite.bits_inserted >= 8,
        "compact add source bits should net-grow into the EE name/guard run"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("compact add/update pair should claim after bounded rewrite");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn compact_placeable_token_add_accepts_following_exact_37_update() {
    // Compact `A/09` add expansion may use the following same-object
    // `U/09 mask=0x37` only after that update's own EE reader proves
    // appearance before scale/state and all position/orientation/state bits at
    // the post-add cursor.
    let object_id = 0x8000_18CAu32;
    let mut live = compact_placeable_token_name_add_live_bytes();
    live.extend_from_slice(&with_live_update_object_id(
        ee_door_placeable_full_update_live_bytes(super::PLACEABLE_OBJECT_TYPE),
        object_id,
    ));

    let mut bits = vec![true, false, true, false];
    bits.extend_from_slice(&exact_scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("compact placeable add should be bounded by the exact following 0x37 update");
    assert!(
        rewrite.bytes_inserted
            >= super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN as u32,
        "compact placeable add should receive EE's visual-transform map"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("compact placeable add plus exact 0x37 update should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn compact_placeable_token_add_rejects_stream_start_bit_shift_before_exact_37_update() {
    // Generalized from the XP2 seq19 stream-start audit: the first live-object
    // row starts at CNW fragment cursor 3. An extra bit before the first
    // compact add's four Diamond tail BOOLs is not a packet-boundary artifact
    // the add/update translator may skip, even when the following U/09
    // `mask=0x37` row is byte-exact and would otherwise prove its own cursor.
    let object_id = 0x8000_18CAu32;
    let mut live = compact_placeable_token_name_add_live_bytes();
    live.extend_from_slice(&with_live_update_object_id(
        ee_door_placeable_full_update_live_bytes(super::PLACEABLE_OBJECT_TYPE),
        object_id,
    ));

    let mut exact_bits = vec![true, false, true, false];
    exact_bits.extend_from_slice(&exact_scalar_door_placeable_update_bits());
    let mut exact_payload = live_object_payload_with_bits(&live, exact_bits.clone());
    super::rewrite_update_records_payload_if_possible(&mut exact_payload)
        .expect("unshifted compact add plus exact update should rewrite");
    let exact_claim = super::claim_payload_if_verified(&exact_payload)
        .expect("unshifted compact add plus exact update should claim");
    assert_eq!(exact_claim.add_records, 1);
    assert_eq!(exact_claim.update_records, 1);

    let mut shifted_bits = vec![true];
    shifted_bits.extend_from_slice(&exact_bits);
    let mut shifted_payload = live_object_payload_with_bits(&live, shifted_bits);
    let original = shifted_payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut shifted_payload).is_none(),
        "stream-start bit shift must not be resynchronized before compact add/update rows"
    );
    assert_eq!(
        shifted_payload, original,
        "failed stream-start bit proof must leave the evidence payload untouched"
    );
    assert!(
        super::claim_payload_if_verified(&shifted_payload).is_none(),
        "the extra stream-start bit remains active cursor evidence"
    );
}

#[test]
fn compact_placeable_token_add_rejects_shifted_or_bit_short_37_update() {
    // Same-length scale/state-before-appearance rows and fragment-short rows
    // are still shifted-cursor evidence. They must not prove the preceding
    // compact add's EE visual-map and guard-bit expansion.
    let object_id = 0x8000_18CAu32;
    let add = compact_placeable_token_name_add_live_bytes();
    for (label, update, update_bits) in [
        (
            "scale-first",
            with_live_update_object_id(
                scale_first_door_placeable_full_update_live_bytes(super::PLACEABLE_OBJECT_TYPE),
                object_id,
            ),
            exact_scalar_door_placeable_update_bits(),
        ),
        (
            "bit-short",
            with_live_update_object_id(
                ee_door_placeable_full_update_live_bytes(super::PLACEABLE_OBJECT_TYPE),
                object_id,
            ),
            vec![true, false],
        ),
    ] {
        let mut live = add.clone();
        live.extend_from_slice(&update);
        let mut bits = vec![true, false, true, false];
        bits.extend_from_slice(&update_bits);
        let mut payload = live_object_payload_with_bits(&live, bits);
        let original = payload.clone();

        assert!(
            super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
            "{label} 0x37 update must not gate compact placeable add expansion"
        );
        assert_eq!(
            payload, original,
            "{label} 0x37 evidence must stay visible for quarantine/diagnostics"
        );
    }
}

#[test]
fn compact_placeable_token_add_rejects_five_bit_residue_before_low_tail_update() {
    // Generalized XP2 seq19 proof: after many prior door/placeable rewrites, a
    // compact token-name A/09 can reach the cursor with five all-zero bits before
    // a same-object U/09 mask=0xF7 low-tail row. Diamond owns four compact add
    // tail BOOLs, but the single remaining bit cannot prove the following
    // position/orientation/state cursor. The bridge must leave the stream
    // unclaimed instead of materializing a full EE add guard run and starving the
    // low-tail update.
    let object_id = 0x8000_18CAu32;
    let mut live = compact_placeable_token_name_add_live_bytes();
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(super::PLACEABLE_OBJECT_TYPE, &[0x00, 0x00]),
        object_id,
    ));

    let mut payload = live_object_payload_with_bits(&live, vec![false; 5]);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "five residual bits do not prove compact add expansion plus following low-tail update"
    );
    assert_eq!(
        payload, original,
        "failed compact-add/low-tail proof must leave source bytes and bits untouched"
    );
}

#[test]
fn compact_placeable_token_add_rejects_unowned_bit_before_low_tail_update_bits() {
    // The two source-only compact selector bits are an exact count, not a cue to
    // resync the following update. If one extra bit sits between that compact
    // add cursor and a same-object low-tail update's own source bits, the update
    // can still look byte-plausible from a shifted cursor. It must stay
    // unclaimed instead of borrowing the residue.
    let object_id = 0x8000_18CAu32;
    let mut live = compact_placeable_token_name_add_live_bytes();
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        object_id,
    ));

    let mut bits = vec![false; 7];
    bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "compact add repair must not skip one residue bit before the following low-tail update"
    );
    assert_eq!(
        payload, original,
        "failed shifted compact-add/low-tail proof must leave source bytes and bits untouched"
    );
}

#[test]
fn prior_low_tail_rewrite_rolls_back_when_compact_alias_add_has_only_five_bits() {
    // Generalized from the XP2 seq19 rollback trace: an earlier low-tail
    // `U/09` may be independently rewriteable, but a later compact token-name
    // `A/09` with external OBJECTID followed by a same-object compact-id
    // `U/09 mask=0xF7` still needs its own exact cursor proof. Five all-zero
    // bits are enough to look plausible but not enough to cover Diamond's four
    // compact add BOOLs plus the following update cursor, so the whole pass must
    // roll back rather than committing the preceding low-tail repair.
    let prior_object_id = 0x8000_0019u32;
    let external_object_id = 0x8000_11FFu32;
    let compact_object_id = external_object_id & !0x8000_0000;
    let mut live = with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        prior_object_id,
    );
    live.extend_from_slice(&with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        external_object_id,
    ));
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        compact_object_id,
    ));

    let mut bits = scalar_door_placeable_update_bits();
    bits.extend_from_slice(&[false; 5]);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "a valid earlier low-tail row must not commit before a failed compact alias handoff"
    );
    assert_eq!(
        payload, original,
        "failed low-tail/add/compact-id proof must roll the whole candidate back"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the shifted compact alias handoff remains active evidence"
    );
}

#[test]
fn prior_low_tail_rewrite_rolls_back_when_compact_add_has_shifted_xp2_low_tail_bits() {
    // Generalized from the later XP2 seq19 rollback trace: after a prior
    // low-tail row rewrites, a compact token-name `A/09` may be followed by a
    // same-object `U/09 mask=0xF7` whose first twelve source bits look
    // plentiful but are shifted for both legal compact-add handoffs. Diamond
    // `sub_44E4A0` owns exactly four compact add BOOLs, with the two extra
    // token selector bits allowed only when the following update exact-proves
    // its own cursor. The traced `1000_11_101101` run proves neither update
    // cursor, so the whole candidate must roll back unchanged.
    let prior_object_id = 0x8000_002Au32;
    let compact_object_id = 0x8000_0001u32;
    let mut add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        compact_object_id,
    );
    add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut live = with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        prior_object_id,
    );
    live.extend_from_slice(&add);
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        compact_object_id,
    ));

    let mut bits = scalar_door_placeable_update_bits();
    bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "shifted XP2 compact-add/low-tail bits must not commit a prior low-tail rewrite"
    );
    assert_eq!(
        payload, original,
        "failed shifted low-tail proof must leave all source bytes and bits untouched"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the shifted compact low-tail handoff remains active cursor evidence"
    );
}

#[test]
fn prior_stale_gap_rewrite_rolls_back_when_compact_add_has_shifted_low_tail_bits() {
    // The sibling `U/09 mask=0x17` stale absent-appearance repair owns the
    // decompiled position, scalar-orientation, and five Diamond state BOOLs
    // only. It must not donate or skip bits for a later compact add/low-tail
    // pair whose source cursor is shifted.
    let prior_object_id = 0x8000_0055u32;
    let compact_object_id = 0x8000_0001u32;
    let mut add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        compact_object_id,
    );
    add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut live = placeable_stale_gap_update_live_bytes_for_object(prior_object_id);
    live.extend_from_slice(&add);
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        compact_object_id,
    ));

    let mut bits = scalar_door_placeable_update_bits();
    bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "a valid stale-gap row must not commit before a failed compact low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed stale-gap/add/low-tail proof must roll the whole candidate back"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the stale-gap row is not the source-bit owner for the shifted compact pair"
    );
}

#[test]
fn prior_compact_stale_gap_pair_rolls_back_before_shifted_compact_low_tail_bits() {
    // Generalized from the XP2 seq19 neighbor before the offset-1145 rollback:
    // a compact token-name `A/09` plus same-object `U/09 mask=0x17` stale-gap
    // update can consume its own decompiled add/update bits exactly. That
    // complete pair still must not donate, skip, or resync bits for a following
    // shifted compact add/low-tail handoff.
    let prior_object_id = 0x8000_1101u32;
    let shifted_object_id = 0x8000_0001u32;

    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut live = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        prior_object_id,
    );
    live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(
        prior_object_id,
    ));
    live.extend_from_slice(&shifted_add);
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));

    let mut bits = vec![true, false, true, false];
    bits.extend_from_slice(&scalar_door_placeable_update_bits());
    bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "a prior compact/stale-gap pair must not commit before a shifted compact low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed compact/stale-gap/add/low-tail proof must roll the whole candidate back"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the preceding compact/stale-gap pair is not the shifted pair's bit owner"
    );
}

#[test]
fn prior_compact_stale_gap_pair_run_rolls_back_before_shifted_compact_low_tail_bits() {
    // The XP2 seq19 replay shows a run of compact token-name `A/09` plus
    // same-object `U/09 mask=0x17` stale-gap pairs before the offset-1145
    // shifted low-tail handoff. Each pair owns only its own four Diamond add
    // BOOLs and stale-gap update cursor; repeating the pair does not create a
    // source-bit reservoir or resync point for the later compact add/update.
    let prior_object_ids = [0x8000_1072u32, 0x8000_1120u32];
    let shifted_object_id = 0x8000_0001u32;

    let mut good_live = Vec::new();
    let mut good_bits = Vec::new();
    for object_id in prior_object_ids {
        good_live.extend_from_slice(&with_live_update_object_id(
            compact_placeable_token_name_add_live_bytes(),
            object_id,
        ));
        good_live.extend_from_slice(&placeable_stale_gap_update_live_bytes_for_object(object_id));
        good_bits.extend_from_slice(&[true, false, true, false]);
        good_bits.extend_from_slice(&scalar_door_placeable_update_bits());
    }
    let mut good_payload = live_object_payload_with_bits(&good_live, good_bits.clone());
    let good_rewrite = super::rewrite_update_records_payload_if_possible(&mut good_payload)
        .expect("the compact/stale-gap pair run should own its exact source bits");
    assert_eq!(
        good_rewrite.update_records_rewritten,
        prior_object_ids.len() as u32
    );
    let good_claim = super::claim_payload_if_verified(&good_payload)
        .expect("the compact/stale-gap pair run should exact-claim after rewrite");
    assert_eq!(good_claim.add_records, prior_object_ids.len() as u32);
    assert_eq!(good_claim.update_records, prior_object_ids.len() as u32);

    let mut shifted_add = with_live_update_object_id(
        compact_placeable_token_name_add_live_bytes(),
        shifted_object_id,
    );
    shifted_add[6..10].copy_from_slice(&0x0001_747Bu32.to_le_bytes());

    let mut live = good_live;
    live.extend_from_slice(&shifted_add);
    live.extend_from_slice(&with_live_update_object_id(
        door_placeable_low_tail_update_live_bytes(
            super::PLACEABLE_OBJECT_TYPE,
            &[0x7B, 0x74, 0x01, 0x00],
        ),
        shifted_object_id,
    ));

    let mut bits = good_bits;
    bits.extend_from_slice(&[
        true, false, false, false, true, true, true, false, true, true, false, true,
    ]);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "a compact/stale-gap run must not commit before a shifted compact low-tail handoff"
    );
    assert_eq!(
        payload, original,
        "failed repeated-pair/add/low-tail proof must roll the whole candidate back"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the preceding compact/stale-gap run is not the shifted pair's bit owner"
    );
}

#[test]
fn door_add_visual_map_repair_is_gated_by_following_same_object_update() {
    let object_id = 0x8000_34D1u32;
    let mut live = door_direct_name_add_live_bytes_without_visual_map(object_id);
    live.extend_from_slice(&door_update_0x17_live_bytes_for_object(object_id));

    let mut bits = vec![true, false, true, false, false, true, true];
    bits.extend_from_slice(&scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "legacy door add lacks EE's visual-transform map before repair"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("door add map insertion should be bounded by the following update");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(
        rewrite.bytes_inserted,
        super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN as u32
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("door add/update pair should claim after bounded map repair");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn door_add_visual_map_repair_accepts_following_exact_37_update() {
    // Door `A/0A` map insertion may use a following same-object `U/0A 0x37`
    // as a cursor proof only after the update's own decompiled reader proves
    // appearance before scale/state and all position/orientation/state bits at
    // the post-add cursor.
    let object_id = 0x8000_34D2u32;
    let mut live = door_direct_name_add_live_bytes_without_visual_map(object_id);
    live.extend_from_slice(&with_live_update_object_id(
        ee_door_placeable_full_update_live_bytes(super::DOOR_OBJECT_TYPE),
        object_id,
    ));

    let mut bits = vec![true, false, true, false, false, true, true];
    bits.extend_from_slice(&exact_scalar_door_placeable_update_bits());
    let mut payload = live_object_payload_with_bits(&live, bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("door add map insertion should be bounded by the exact following 0x37 update");
    assert_eq!(rewrite.update_records_rewritten, 0);
    assert_eq!(
        rewrite.bytes_inserted,
        super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN as u32
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("door add plus exact 0x37 update should claim after bounded map repair");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn door_add_visual_map_repair_rejects_shifted_or_bit_short_37_update() {
    // Same-length scale/state-before-appearance bytes and bit-short updates
    // remain active cursor hazards. The add pass must not use them as proof for
    // inserting a visual-transform map into the preceding door add.
    let object_id = 0x8000_34D3u32;
    let add = door_direct_name_add_live_bytes_without_visual_map(object_id);
    for (label, update, update_bits) in [
        (
            "scale-first",
            with_live_update_object_id(
                scale_first_door_placeable_full_update_live_bytes(super::DOOR_OBJECT_TYPE),
                object_id,
            ),
            exact_scalar_door_placeable_update_bits(),
        ),
        (
            "bit-short",
            with_live_update_object_id(
                ee_door_placeable_full_update_live_bytes(super::DOOR_OBJECT_TYPE),
                object_id,
            ),
            vec![true, false],
        ),
    ] {
        let mut live = add.clone();
        live.extend_from_slice(&update);
        let mut bits = vec![true, false, true, false, false, true, true];
        bits.extend_from_slice(&update_bits);
        let mut payload = live_object_payload_with_bits(&live, bits);
        let original = payload.clone();

        assert!(
            super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
            "{label} 0x37 update must not gate door add map insertion"
        );
        assert_eq!(
            payload, original,
            "{label} 0x37 evidence must stay visible for quarantine/diagnostics"
        );
    }
}

#[test]
fn compact_placeable_token_add_with_no_source_bits_stays_unclaimed_without_prior_cursor_owner() {
    let live = compact_placeable_token_name_add_live_bytes();
    let mut payload = live_object_payload_with_bits(&live, Vec::new());
    let original = payload.clone();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw compact placeable add cannot exact-claim without its four Diamond source BOOLs"
    );
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "zero-source compact add expansion needs a prior update-repair cursor owner"
    );
    assert_eq!(
        payload, original,
        "unowned compact add bits must stay visible for quarantine/diagnostics"
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
fn update_rewrite_typed_item_create_preserves_following_update_bits() {
    // `A/6` is a typed live-object item create: after the object id, Diamond
    // and EE hand off to the shared item body reader. It must not fall through
    // to top-level visible-equipment add cursor fallback, because the EE-only
    // active-property BOOL belongs inside the item-create row before the next
    // `U/6` position record sees its residual bits.
    let mut live = ee_shaped_model_type2_typed_item_create_live_bytes();
    live.extend_from_slice(&item_update_position_live_bytes([
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
    ]));
    let mut payload = live_object_payload_with_bits(
        &live,
        vec![
            false, false, true, false, false, // item name + Diamond active-property bits.
            false, true, // following U/6 position residual bits.
        ],
    );

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw typed item create is missing EE's active-property bit"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("typed item-create rewrite should insert the missing EE bit");
    assert_eq!(rewrite.bits_inserted, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        &[false, false, false, true, false, false, false, true],
        "the inserted EE bit must precede, not consume, the following update bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten item-create/update should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn update_rewrite_typed_item_create_preserves_following_full_item_update_bits() {
    // Extend the local CEP v2.3 `A/6` handoff proof beyond the following
    // update's first two position bits. When the next `U/6` carries the
    // decompiled scalar-orientation/name/hidden cursor, the typed item-create
    // repair inserts only its EE active-property bit and the later full-mask
    // item update translates normally.
    let mut live = ee_shaped_model_type2_typed_item_create_live_bytes();
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    let mut owned_bits = source_item_create_bits.to_vec();
    owned_bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, owned_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw typed item create still lacks EE's active-property bit"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("typed item-create repair should preserve a full following item-update cursor");
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(rewrite.masks_translated, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = vec![false, false, false, true, false, false];
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "the A/6 active-property insert must not steal any following U/6 bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten typed item-create/full-update stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn typed_item_create_boundary_uses_item_body_proof_over_property_opcode_bytes() {
    // Diamond `sub_451020` and EE `sub_14076BD30` both consume the counted
    // active-property tail inside the typed `A/6` item body. Bytes in that tail
    // can resemble a top-level `U/6`, so the boundary must be selected by the
    // item parser plus fragment proof before the following update cursor is
    // validated.
    let mut item_create = ee_shaped_model_type2_typed_item_create_live_bytes();
    inject_live_boundary_lookalike_into_item_property_values(&mut item_create);
    let item_create_len = item_create.len();

    let mut live = item_create;
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let generic_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        &live,
        0,
        live.len(),
    );
    assert!(
        generic_end < item_create_len,
        "the byte-only boundary scanner should see the interior U/6 lookalike"
    );

    let source_item_create_bits = [false, false, true, false, false];
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    let mut owned_bits = source_item_create_bits.to_vec();
    owned_bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, owned_bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("fragment-proven A/6 endpoint should beat the interior opcode lookalike");
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(rewrite.masks_translated, 1);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten item-create/update stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn legacy_width_typed_item_create_preserves_following_full_item_update_bits() {
    // This is the Diamond-body sibling of the CEP v2.3 `A/6` handoff audit.
    // At this layer the EE object visual-map may already be present, while the
    // typed item-create row still has Diamond-width model-type-2 BYTE parts and
    // lacks EE's active-property BOOL. Widening those bytes must not move the
    // following full `U/6` source bit.
    let mut live = legacy_width_model_type2_typed_item_create_with_visual_map_live_bytes();
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    let mut owned_bits = source_item_create_bits.to_vec();
    owned_bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, owned_bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy typed item-create widening should preserve the following U/6 cursor");
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(
        rewrite.bytes_inserted, 3,
        "model-type-2 BYTE parts widen to WORDs before the existing EE visual-map"
    );
    assert_eq!(rewrite.masks_translated, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = vec![false, false, false, true, false, false];
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "item-create byte widening must not steal any following U/6 bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten legacy item-create/full-update stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn legacy_width_typed_item_create_without_visual_map_preserves_following_full_item_update_bits() {
    // Generalized CEP v2.3 `A/6` sibling: Diamond `sub_451020` stops after the
    // model-type-2 BYTE appearance fields, while EE `sub_14079FAC0` widens
    // those parts and reads an object visual-transform map before active item
    // properties. Both byte insertions are inside the item-create row and must
    // preserve the following `U/6` fragment cursor.
    let mut live = legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes();
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    let mut owned_bits = source_item_create_bits.to_vec();
    owned_bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, owned_bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload).expect(
        "legacy typed item-create visual-map insertion should preserve the following U/6 cursor",
    );
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(
        rewrite.bytes_inserted,
        3 + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN as u32,
        "model-type-2 BYTE parts widen and EE's item visual-transform map is inserted"
    );
    assert_eq!(rewrite.masks_translated, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = vec![false, false, false, true, false, false];
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "item-create byte insertion must not steal any following U/6 bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten legacy item-create/no-map full-update stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn tail9_door_update_before_typed_item_create_preserves_following_full_item_update_bits() {
    // Pin the CEP v2.3 upstream cursor: a preceding all-bits door `U/10`
    // tail9 row owns eight Diamond source bits and emits thirteen EE bits
    // before the typed `A/6` item create hands off to the following full `U/6`.
    let mut live = legacy_tail9_door_update_without_name_payload_live_bytes();
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut bits = legacy_tail9_door_update_source_bits();
    bits.extend_from_slice(&[false, false, true, false, false]); // typed item-create source bits.
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("tail9 door update plus typed item-create handoff should rewrite when all source cursors are exact");
    assert_eq!(rewrite.update_records_rewritten, 2);
    assert_eq!(rewrite.masks_translated, 2);
    assert_eq!(rewrite.bits_inserted, 7);
    assert_eq!(rewrite.bits_removed, 1);
    assert_eq!(rewrite.bytes_removed, 6);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = legacy_tail9_door_update_expected_ee_bits();
    expected.extend_from_slice(&[false, false, false, true, false, false]); // A/6 plus EE BOOL.
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "tail9 and A/6 rewrites must preserve every following U/6 cursor bit"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten tail9/A6/U6 stream should exact-claim");
    assert_eq!(claim.update_records, 2);
    assert_eq!(claim.add_records, 1);
}

#[test]
fn tail9_door_update_before_typed_item_create_rejects_shifted_following_item_update() {
    // CEP-like negative proof: after tail9's eight source bits and A/6's
    // source bits, the following U/6 selects vector orientation while the
    // bytes are scalar-shaped. That cursor remains unproven; it must not be
    // rescued by the preceding generalized repairs.
    let mut live = legacy_tail9_door_update_without_name_payload_live_bytes();
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut bits = legacy_tail9_door_update_source_bits();
    bits.extend_from_slice(&[false, false, true, false, true]); // CEP-like typed A/6 source bits.
    bits.extend_from_slice(&[
        false, false, // position residual bits.
        true,  // vector orientation selector, despite scalar-shaped bytes.
        true, false, true, false, true,  // state bits if the cursor were valid.
        false, // direct name if the scalar cursor were valid.
        true,  // hidden BOOL if the scalar cursor were valid.
    ]);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "tail9/A6 repairs must not commit when the following U/6 cursor is shifted"
    );
    assert_eq!(
        payload, original,
        "failed tail9/A6/U6 proof must leave source bytes and bits untouched"
    );
}

#[test]
fn cep_tail9_name_suffix_before_typed_item_create_preserves_following_full_item_update_bits() {
    // The live CEP v2.3 stream differs from the older constructed tail9 sibling:
    // the U/10 source bits have the legacy name branch set and the read buffer
    // carries a four-byte legacy suffix after the tail9 state WORD. Diamond owns
    // only that one name BOOL before returning to the next A/6 row; it does not
    // donate extra fragment bits to the following U/6.
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut bits = legacy_short_strref_door_add_source_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);

    super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("CEP-like tail9 name suffix plus A/6/U6 stream should rewrite exactly");

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = legacy_short_strref_door_add_expected_ee_bits();
    expected.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_expected_ee_bits());
    expected.extend_from_slice(&[false, false, false, true, false, false]); // A/6 plus EE BOOL.
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "CEP-like tail9 name suffix must not move the following U/6 cursor"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten CEP-like door/tail9/A6/U6 stream should exact-claim");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.update_records, 2);
}

#[test]
fn cep_tail9_name_suffix_before_legacy_width_item_create_without_visual_map_preserves_u6_bits() {
    // This matches the generalized private CEP v2.3 handoff shape without
    // depending on that fixture: a legacy door add, `U/10` tail9 with its
    // four-byte legacy name suffix, a Diamond-width typed item create missing
    // EE's item visual map, then the following full item update.
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(
        &legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes(),
    );
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut bits = legacy_short_strref_door_add_source_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);

    super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("CEP-like tail9/no-map A6/U6 stream should rewrite exactly");

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = legacy_short_strref_door_add_expected_ee_bits();
    expected.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_expected_ee_bits());
    expected.extend_from_slice(&[false, false, false, true, false, false]); // A/6 plus EE BOOL.
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "door/tail9/A6 rewrites must preserve the following no-map U/6 cursor"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten CEP-like no-map door/tail9/A6/U6 stream should exact-claim");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.update_records, 2);
}

#[test]
fn cep_tail9_name_suffix_with_actual_short_strref_state_preserves_no_map_u6_bits() {
    // The private CEP v2.3 starter stream's leading short-strref A/10 carries
    // post-name state bits 1011. Normalizing the legacy short-name row to EE's
    // direct empty-name shape preserves those state values; it still does not
    // move the later no-map A/6 to full U/6 cursor handoff.
    let actual_short_strref_state_bits = [true, false, true, true];
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(
        &legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes(),
    );
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut bits =
        legacy_short_strref_door_add_source_bits_with_state(actual_short_strref_state_bits);
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);

    super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("actual-state short-strref/no-map A6/U6 stream should rewrite exactly");

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected =
        legacy_short_strref_door_add_expected_ee_bits_with_state(actual_short_strref_state_bits);
    expected.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_expected_ee_bits());
    expected.extend_from_slice(&[false, false, false, true, false, false]); // A/6 plus EE BOOL.
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "actual short-strref state bits must be preserved without moving the following U/6"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("actual-state no-map door/tail9/A6/U6 stream should exact-claim");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.update_records, 2);
}

#[test]
fn cep_tail9_name_suffix_does_not_supply_two_residue_bits_before_item_update() {
    // Negative sibling for the exact CEP tail9 bit pattern. Even with the
    // legacy name branch true and the four-byte suffix removed from the read
    // buffer, the next U/6 may validate at cursor +2 only if some separate
    // decompile-backed owner consumed those two bits first.
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut shifted_item_bits = vec![false, true];
    shifted_item_bits.extend_from_slice(&item_update_full_mask_scalar_direct_name_bits());
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept the U/6 only after an external two-bit owner"
    );

    let mut bits = legacy_short_strref_door_add_source_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "CEP-like tail9 name suffix must not skip unowned bits before the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed CEP-like residue proof must leave the source stream untouched"
    );
}

#[test]
fn cep_tail9_name_suffix_with_actual_short_strref_state_does_not_supply_residue_bits() {
    // Negative sibling for the actual CEP v2.3 leading A/10 state values. The
    // short-name normalization preserves those four bits but cannot donate two
    // extra source bits before the later no-map A/6 to U/6 handoff.
    let actual_short_strref_state_bits = [true, false, true, true];
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(
        &legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes(),
    );
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut shifted_item_bits = vec![false, true];
    shifted_item_bits.extend_from_slice(&item_update_full_mask_scalar_direct_name_bits());
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept the U/6 only after an external two-bit owner"
    );

    let mut bits =
        legacy_short_strref_door_add_source_bits_with_state(actual_short_strref_state_bits);
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "actual-state short-strref/no-map A6 must not skip unowned bits before the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed actual-state residue proof must leave source bytes and bits untouched"
    );
}

#[test]
fn cep_tail9_name_suffix_before_legacy_width_item_create_without_visual_map_does_not_supply_residue_bits()
 {
    // Negative sibling for the no-visual-map typed item-create branch above.
    // The EE visual-map bytes and active-property BOOL are inserted
    // transactionally inside `A/6`; neither insertion can consume two unrelated
    // source bits before the following `U/6`.
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(
        &legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes(),
    );
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut shifted_item_bits = vec![false, true];
    shifted_item_bits.extend_from_slice(&item_update_full_mask_scalar_direct_name_bits());
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept the U/6 only after an external two-bit owner"
    );

    let mut bits = legacy_short_strref_door_add_source_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "no-map A/6 byte/bit insertion must not skip unowned bits before the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed no-map residue proof must leave the source stream untouched"
    );
}

#[test]
fn cep_tail9_name_suffix_no_map_replays_raw_neighbor_u6_bits_without_repair() {
    // Replay the public shape of the raw CEP v2.3 no-map handoff observed in
    // the private stream: A/10 `11011`, U/10 `01100011`, A/6 `00100`, then a
    // following U/6 whose first bits are `01110101100000`. The U/6 reader fits
    // only at cursor +2; without a decompile-backed owner for those two leading
    // bits, the packet-level rewrite must leave the stream unclaimed.
    let actual_short_strref_state_bits = [true, false, true, true];
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(
        &legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes(),
    );
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let shifted_item_bits = vec![
        false, true, // unowned pre-cursor residue.
        true, true, // position residuals if a prior owner consumed the residue.
        false, true, false, true, true, // scalar orientation selector plus residual bits.
        false, false, false, false, false, // item state bits.
        false, // direct CExoString item name.
        false, // EE hidden-state BOOL after item name.
    ];
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            0,
        )
        .is_none(),
        "at the true cursor the item row selects vector orientation but has scalar-shaped bytes"
    );
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept the raw U/6 bits only after an external two-bit owner"
    );

    let mut bits =
        legacy_short_strref_door_add_source_bits_with_state(actual_short_strref_state_bits);
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "raw CEP no-map handoff must not skip two unowned bits before the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed raw CEP handoff proof must leave the source stream untouched"
    );
}

#[test]
fn ee_shaped_door_add_cep_tail9_no_map_replays_raw_neighbor_u6_bits_without_repair() {
    // The private CEP v2.3 debug pass first normalizes the leading A/10 to
    // EE-shaped direct-empty/state bits, then reaches the same U/10 name suffix,
    // no-map A/6, and raw U/6 bits. The normalized prefix is still just a
    // boundary proof; it cannot own the two bits needed by the item update.
    let mut live = ee_shaped_generic_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(
        &legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes(),
    );
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let shifted_item_bits = vec![
        false, true, // unowned pre-cursor residue.
        true, true, // position residuals if a prior owner consumed the residue.
        false, true, false, true, true, // scalar orientation selector plus residual bits.
        false, false, false, false, false, // item state bits.
        false, // direct CExoString item name.
        false, // EE hidden-state BOOL after item name.
    ];
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            0,
        )
        .is_none(),
        "at the true cursor the raw U/6 bits do not match the scalar-shaped read bytes"
    );
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept only after a separate two-bit owner"
    );

    let mut bits = ee_shaped_generic_door_add_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "normalized A/10 plus tail9/A6 repairs must not skip into the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed normalized-prefix handoff proof must leave the source stream untouched"
    );
}

#[test]
fn cep_no_map_raw_u6_neighboring_cursor_fits_are_not_ownership_proof() {
    // The private CEP v2.3 trace reaches the Lance U/6 after the normalized
    // A/10, tail9 U/10, and no-map A/6 rewrites at bit cursor 28. Several
    // neighboring cursors can validate the scalar-shaped item bytes, but
    // Diamond `sub_467AE0` / EE `sub_14079C050` still branch on the current
    // orientation bit before reading those bytes. A neighboring fit is only
    // evidence that some prior reader would need to own the skipped bits.
    let actual_short_strref_state_bits = [true, false, true, true];
    let mut prefix_bits =
        legacy_short_strref_door_add_expected_ee_bits_with_state(actual_short_strref_state_bits);
    prefix_bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_expected_ee_bits());
    prefix_bits.extend_from_slice(&[false, false, false, true, false, false]); // no-map A/6 after EE repair.

    let shifted_item_bits = vec![
        false, true, // unowned pre-cursor residue.
        true, true, // position residuals if a prior owner consumed the residue.
        false, true, false, true, true, // scalar orientation selector plus residual bits.
        false, false, false, false, false, // item state bits.
        false, // direct CExoString item name.
        false, // EE hidden-state BOOL after item name.
        false, false, // following-stream bits available in the private trace.
    ];

    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    fragment_bits.extend_from_slice(&prefix_bits);
    let item_cursor = fragment_bits.len();
    assert_eq!(
        item_cursor, 28,
        "public CEP-style prefix should match the private trace cursor"
    );
    fragment_bits.extend_from_slice(&shifted_item_bits);

    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());

    let nearby_verified: Vec<isize> = (-4..=4)
        .filter(|delta| *delta != 0)
        .filter(|delta| {
            let cursor = item_cursor.saturating_add_signed(*delta);
            super::item::advance_verified_ee_item_update_record(
                &translated_item_update,
                0,
                translated_item_update.len(),
                &fragment_bits,
                cursor,
            )
            .is_some()
        })
        .collect();
    assert_eq!(
        nearby_verified,
        vec![-4, -3, -2, 2, 4],
        "neighboring scalar-shaped fits must be treated as ambiguity until a prior owner is proven"
    );
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &fragment_bits,
            item_cursor,
        )
        .is_none(),
        "the true cursor still selects vector orientation for scalar-shaped bytes"
    );

    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(
        &legacy_width_model_type2_typed_item_create_without_visual_map_live_bytes(),
    );
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut source_bits =
        legacy_short_strref_door_add_source_bits_with_state(actual_short_strref_state_bits);
    source_bits.extend_from_slice(&legacy_tail9_door_update_cep_name_suffix_source_bits());
    source_bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    source_bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, source_bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "multiple neighboring U/6 fits must not make the shifted source cursor claimable"
    );
    assert_eq!(
        payload, original,
        "ambiguous neighboring cursor proof must leave bytes and bits untouched"
    );
}

#[test]
fn tail9_item_create_handoff_does_not_skip_two_unowned_bits_before_item_update() {
    // The CEP v2.3 cursor-neighbor evidence is not limited to an isolated U/6.
    // Even after the preceding U/10 tail9 row and typed A/6 item-create row are
    // both decompile-owned, neither row owns two extra fragment bits before the
    // following full item update. A neighboring item cursor may validate only if
    // some separate reader has consumed those bits first.
    let mut live = legacy_tail9_door_update_without_name_payload_live_bytes();
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut shifted_item_bits = vec![false, true];
    shifted_item_bits.extend_from_slice(&item_update_full_mask_scalar_direct_name_bits());
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept the U/6 only after an external two-bit owner"
    );

    let mut bits = legacy_tail9_door_update_source_bits();
    bits.extend_from_slice(&[false, false, true, false, false]); // typed A/6 source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "tail9/A6 repairs must not skip unowned bits before the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed residue proof must leave the source stream untouched"
    );
}

#[test]
fn short_strref_door_add_before_tail9_item_handoff_preserves_full_item_update_bits() {
    // The full CEP-like prefix includes a short-strref `A/10` door add before
    // the `U/10` tail9 row. Diamond owns five source bits for that add; EE
    // receives the canonical six-bit empty-name/state shape after one inserted
    // BOOL. That normalization must not move the following item `U/6` cursor.
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut bits = legacy_short_strref_door_add_source_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed item-create source bits.
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);

    super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("short-strref door add plus tail9/A6/U6 stream should rewrite exactly");

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = legacy_short_strref_door_add_expected_ee_bits();
    expected.extend_from_slice(&legacy_tail9_door_update_expected_ee_bits());
    expected.extend_from_slice(&[false, false, false, true, false, false]); // A/6 plus EE BOOL.
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "door-add, tail9, and A/6 rewrites must preserve the following U/6 cursor"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten short-strref door/tail9/A6/U6 stream should exact-claim");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.update_records, 2);
}

#[test]
fn short_strref_door_add_before_tail9_item_handoff_does_not_supply_two_residue_bits() {
    // Negative sibling for the CEP v2.3 handoff: the initial `A/10` short-name
    // row owns exactly five Diamond bits, not seven. The later U/6 may validate
    // at cursor + 2 only if some separate reader consumed those two bits first.
    let mut live = legacy_short_strref_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut shifted_item_bits = vec![false, true];
    shifted_item_bits.extend_from_slice(&item_update_full_mask_scalar_direct_name_bits());
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept the U/6 only after an external two-bit owner"
    );

    let mut bits = legacy_short_strref_door_add_source_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed item-create source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "A/10 door-add normalization must not supply unowned bits before the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed short-strref door/tail9/A6 residue proof must leave the source stream untouched"
    );
}

#[test]
fn ee_shaped_door_add_before_tail9_item_handoff_preserves_full_item_update_bits() {
    // CEP v2.3 debug also shows an already EE-shaped generic `A/10` prefix:
    // Diamond/EE both read the two door DWORDs, EE visual-map, direct empty
    // CExoString, state WORD, and six fragment BOOLs. That exact add row is a
    // boundary proof, not a license to move the later item `U/6` cursor.
    let mut live = ee_shaped_generic_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut bits = ee_shaped_generic_door_add_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed item-create source bits.
    let following_update_bits = item_update_full_mask_scalar_direct_name_bits();
    bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);

    super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("EE-shaped door add plus tail9/A6/U6 stream should rewrite exactly");

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = ee_shaped_generic_door_add_bits();
    expected.extend_from_slice(&legacy_tail9_door_update_expected_ee_bits());
    expected.extend_from_slice(&[false, false, false, true, false, false]); // A/6 plus EE BOOL.
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "already-EE door add, tail9, and A/6 rewrites must preserve the following U/6 cursor"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten EE-shaped door/tail9/A6/U6 stream should exact-claim");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.update_records, 2);
}

#[test]
fn ee_shaped_door_add_before_tail9_item_handoff_does_not_supply_two_residue_bits() {
    // Negative sibling for the actual direct-empty `A/10` prefix shape: an
    // already EE-shaped door add owns its six decompile-backed bits exactly.
    // The following item update may validate at cursor + 2 only if a different
    // reader consumed those two bits first.
    let mut live = ee_shaped_generic_door_add_live_bytes();
    live.extend_from_slice(&legacy_tail9_door_update_without_name_payload_live_bytes());
    live.extend_from_slice(&ee_shaped_model_type2_typed_item_create_live_bytes());
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let mut shifted_item_bits = vec![false, true];
    shifted_item_bits.extend_from_slice(&item_update_full_mask_scalar_direct_name_bits());
    let mut translated_item_update = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    translated_item_update[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_item_update,
            0,
            translated_item_update.len(),
            &shifted_item_bits,
            2,
        )
        .is_some(),
        "the item reader would accept the U/6 only after an external two-bit owner"
    );

    let mut bits = ee_shaped_generic_door_add_bits();
    bits.extend_from_slice(&legacy_tail9_door_update_source_bits());
    bits.extend_from_slice(&[false, false, true, false, false]); // typed item-create source bits.
    bits.extend_from_slice(&shifted_item_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "EE-shaped A/10 door add must not supply unowned bits before the following U/6"
    );
    assert_eq!(
        payload, original,
        "failed EE-shaped door/tail9/A6 residue proof must leave the source stream untouched"
    );
}

#[test]
fn full_item_update_does_not_skip_unowned_pre_cursor_residue() {
    // Cursor-neighbor proof for the CEP v2.3 item handoff risk. A full `U/6`
    // byte row can still look valid if two unowned fragment bits are read as
    // its position residuals; that does not make those bits part of the item
    // update. The packet-level rewrite must leave the stream unclaimed instead
    // of retrying the item parser at a neighboring cursor that happens to fit.
    let live = item_update_full_mask_scalar_direct_name_live_bytes(b"Lance");
    let exact_update_bits = item_update_full_mask_scalar_direct_name_bits();
    let mut shifted_bits = vec![false, true];
    shifted_bits.extend_from_slice(&exact_update_bits);

    let mut translated_live = live.clone();
    translated_live[6..10]
        .copy_from_slice(&super::item::translate_update_mask(0xFFFF_FFF3).to_le_bytes());
    assert!(
        super::item::advance_verified_ee_item_update_record(
            &translated_live,
            0,
            translated_live.len(),
            &shifted_bits,
            2,
        )
        .is_some(),
        "the decompiled item reader would validate if an external owner had consumed the two residue bits"
    );

    let mut payload = live_object_payload_with_bits(&live, shifted_bits);
    let original = payload.clone();
    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "item update rewriting must not skip unowned pre-cursor bits to make a full U/6 validate"
    );
    assert_eq!(
        payload, original,
        "failed neighboring-cursor proof must leave bytes and fragment bits untouched"
    );
}

#[test]
fn update_rewrite_typed_item_create_preserves_following_full_item_update_locstring_inline_bits() {
    // This is the locstring-inline sibling of the CEP v2.3 typed A/6 handoff
    // audit. The A/6 active-property insertion is allowed only if the following
    // U/6 owns its own position/orientation/state/name/hidden bits exactly.
    let mut live = ee_shaped_model_type2_typed_item_create_live_bytes();
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let following_update_bits = item_update_full_mask_scalar_locstring_inline_bits();
    let mut owned_bits = source_item_create_bits.to_vec();
    owned_bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, owned_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw typed item create still lacks EE's active-property bit"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("typed item-create repair should preserve a locstring-inline full U/6 cursor");
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(rewrite.masks_translated, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = vec![false, false, false, true, false, false];
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "the A/6 active-property insert must not steal locstring-inline U/6 bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten typed item-create/locstring-inline full-update stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn update_rewrite_typed_item_create_preserves_following_full_item_update_locstring_token_bits() {
    // Token names use an extra locstring selector bit and a five-byte read
    // payload inside the following U/6. The A/6 active-property insertion must
    // leave that cursor intact instead of treating token bytes or selector bits
    // as shared handoff residue.
    let mut live = ee_shaped_model_type2_typed_item_create_live_bytes();
    live.extend_from_slice(&item_update_full_mask_scalar_locstring_token_live_bytes(
        1,
        0x0100_75D6,
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let following_update_bits = item_update_full_mask_scalar_locstring_token_bits();
    let mut owned_bits = source_item_create_bits.to_vec();
    owned_bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, owned_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw typed item create still lacks EE's active-property bit"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("typed item-create repair should preserve a locstring-token full U/6 cursor");
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(rewrite.masks_translated, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = vec![false, false, false, true, false, false];
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "the A/6 active-property insert must not steal locstring-token U/6 bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten typed item-create/locstring-token full-update stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn update_rewrite_typed_item_create_preserves_following_full_item_update_vector_bits() {
    // Positive sibling for the CEP v2.3 A/6 handoff: if the following full
    // item update really selects vector orientation and carries six vector
    // bytes, the A/6 active-property insertion must still leave those U/6
    // cursor bits untouched.
    let mut live = ee_shaped_model_type2_typed_item_create_live_bytes();
    live.extend_from_slice(&item_update_full_mask_vector_direct_name_live_bytes(
        b"Lance",
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let following_update_bits = item_update_full_mask_vector_direct_name_bits();
    let mut owned_bits = source_item_create_bits.to_vec();
    owned_bits.extend_from_slice(&following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, owned_bits);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw typed item create still lacks EE's active-property bit"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("typed item-create repair should preserve a vector full U/6 cursor");
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(rewrite.masks_translated, 1);

    let declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("declared length") as usize;
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .expect("rewritten fragment bits");
    let mut expected = vec![false, false, false, true, false, false];
    expected.extend_from_slice(&following_update_bits);
    assert_eq!(
        &fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..],
        expected.as_slice(),
        "the A/6 active-property insert must not steal vector U/6 bits"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten typed item-create/vector full-update stream should claim");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.update_records, 1);
}

#[test]
fn typed_item_create_handoff_rejects_vector_selected_full_item_update() {
    // This is the negative sibling of the CEP v2.3 typed A/6 handoff audit.
    // The A/6 row may insert EE's active-property BOOL only when the following
    // U/6 owns its own cursor. Diamond `sub_467AE0` and EE `sub_14079C050`
    // branch on the orientation BOOL before reading orientation bytes, so a
    // vector selector cannot be relabeled to fit scalar-looking item bytes.
    let mut live = ee_shaped_model_type2_typed_item_create_live_bytes();
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let shifted_following_update_bits = [
        false, true, // position residual bits.
        true, // vector orientation selector, despite scalar-shaped bytes.
        true, false, true, false, true,  // state bits if the cursor were valid.
        false, // direct name if the scalar cursor were valid.
        true,  // hidden BOOL if the scalar cursor were valid.
    ];
    let mut bits = source_item_create_bits.to_vec();
    bits.extend_from_slice(&shifted_following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "A/6 repair must not commit when the following U/6 cursor is shifted"
    );
    assert_eq!(
        payload, original,
        "failed handoff proof must leave the source bytes and fragment bits untouched"
    );
}

#[test]
fn legacy_width_typed_item_create_handoff_rejects_vector_selected_full_item_update() {
    // Byte widening inside the preceding Diamond `A/6` body is still
    // transactional. If the following `U/6` bits select vector orientation while
    // the bytes are scalar-shaped, the item-create repair must roll back instead
    // of committing a plausible but shifted cursor.
    let mut live = legacy_width_model_type2_typed_item_create_with_visual_map_live_bytes();
    live.extend_from_slice(&item_update_full_mask_scalar_direct_name_live_bytes(
        b"Lance",
    ));

    let source_item_create_bits = [false, false, true, false, false];
    let shifted_following_update_bits = [
        false, true, // position residual bits.
        true, // vector orientation selector, despite scalar-shaped bytes.
        true, false, true, false, true,  // state bits if the cursor were valid.
        false, // direct name if the scalar cursor were valid.
        true,  // hidden BOOL if the scalar cursor were valid.
    ];
    let mut bits = source_item_create_bits.to_vec();
    bits.extend_from_slice(&shifted_following_update_bits);
    let mut payload = live_object_payload_with_bits(&live, bits);
    let original = payload.clone();

    assert!(
        super::rewrite_update_records_payload_if_possible(&mut payload).is_none(),
        "legacy A/6 byte/bit repair must not commit when following U/6 bits are shifted"
    );
    assert_eq!(
        payload, original,
        "failed Diamond-body handoff proof must leave bytes and bits untouched"
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
fn live_gui_character_sheet_mask20_owns_one_fragment_bool() {
    // EE `sub_1407B2740` reads the mask 0x20 character-sheet branch as one
    // read-buffer BYTE followed by one CNW BOOL. It is a short `G S` row, but
    // still not read-buffer-only.
    let payload = live_gui_character_sheet_payload(0x0000_0020, &[0x7A], vec![true]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("short character-sheet mask 0x20 row should exact-claim");
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 1);

    let shifted = live_gui_character_sheet_payload(0x0000_0020, &[0x7A], Vec::new());
    assert!(
        super::claim_payload_if_verified(&shifted).is_none(),
        "the mask 0x20 character-sheet row must not claim without its BOOL"
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
fn live_gui_character_sheet_build_mode_boundary_ambiguity_stays_unclaimed() {
    // With a following record boundary, both the legacy four-bit and EE
    // build-8193.35 five-bit combat-list branches can land on the same byte.
    // That is not proof that either bit cursor is correct; the bridge must
    // leave the row unclaimed until the build-width evidence is unique.
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
    push_msb_bits(&mut bits, 0b100, 3);
    bits.extend_from_slice(&[false, false, false]);

    let isolated = live_gui_character_sheet_payload(0x0000_0040, &body, bits.clone());
    let isolated_claim = super::claim_payload_if_verified(&isolated)
        .expect("isolated five-bit combat action should exact-claim");
    assert_eq!(isolated_claim.live_gui_fragment_bits, bits.len() as u32);

    let mut live = Vec::new();
    live.extend_from_slice(&[b'G', b'S']);
    live.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    live.extend_from_slice(&0x0000_0040u32.to_le_bytes());
    live.extend_from_slice(&body);
    let first_record_end = live.len();
    live.extend_from_slice(&[b'W', 0x10, 0x20]);

    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    fragment_bits.extend(bits.clone());
    assert!(
        super::gui::try_get_verified_ee_live_gui_record_end(
            &live,
            0,
            live.len(),
            &fragment_bits,
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .is_none(),
        "same-byte character-sheet boundary with different legacy/EE bit widths must not be guessed"
    );
    assert!(
        super::gui::try_get_verified_ee_live_gui_record_end(
            &live[..first_record_end],
            0,
            first_record_end,
            &fragment_bits,
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .is_some(),
        "the same bytes are claimable only when the isolated final bit cursor proves the five-bit branch"
    );

    let payload = live_object_payload_with_bits(&live, bits);
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "a following W boundary cannot disambiguate the character-sheet combat build-width cursor"
    );
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
fn creature_status_effect_single_target_payload_requires_2da_row_policy() {
    let payload =
        creature_status_effect_4008_payload(&[(0x1234, Some(&[0x44, 0x33, 0x22, 0x80, 0x66]))]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "without visualeffects.2da Type_FD proof, target-payload shape must stay unclaimed"
    );
}

#[test]
fn creature_effect_only_status_update_boundary_owns_embedded_add_rows() {
    // Standalone `U/5 mask=0x0008` uses the same EE status-effect row writer as
    // the 0x4008/0x8008/C408 families: each A/D row owns a following
    // ObjectVisualTransformData identity map before the next live-object record.
    let mut live = ee_creature_effect_only_update_live(&[(b'A', 0x00F3), (b'D', 0x00B6)]);
    let status_end = live.len();
    live.extend_from_slice(&[b'W', 0x10, 0x20]);

    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            &live,
            0,
            live.len()
        ),
        status_end,
        "the transport scanner must not split at embedded A/D effect rows"
    );

    let claim = super::claim_payload_if_verified(&live_object_payload_with_bits(&live, Vec::new()))
        .expect("effect-only status update followed by W should exact-claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(claim.world_status_records, 1);
}

#[test]
fn zero_mask_looking_creature_selector_storage_waits_for_following_boundary() {
    // `U/5 + OBJECTID + 00 00 00 00` is ambiguous without a stream boundary:
    // the creature update reader treats the four zero bytes as a mask and owns
    // no body, while the legacy visual-transform selector branch owns only the
    // first zero byte and treats the following bytes as CNW fragment storage.
    // Do not split at the ten-byte mask cursor unless it is a real boundary.
    let mut live = vec![b'U', super::CREATURE_OBJECT_TYPE];
    live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
    live.push(0x00);
    let storage =
        super::bits::pack_msb_valid_bits(vec![false; 64], super::CNW_FRAGMENT_HEADER_BITS);
    assert_eq!(storage.len(), 8);
    live.extend_from_slice(&storage);
    let visual_selector_end = live.len();
    live.extend_from_slice(&[b'W', 0x10, 0x20]);

    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            &live,
            0,
            live.len()
        ),
        visual_selector_end,
        "zero-looking selector storage belongs to the visual-transform bridge candidate"
    );

    let exact_zero_update = super::claim_payload_if_verified(&live_object_payload_with_bits(
        &live[..super::LEGACY_UPDATE_HEADER_BYTES],
        Vec::new(),
    ))
    .expect("isolated ten-byte zero-mask U/5 should exact-claim");
    assert_eq!(exact_zero_update.creature_update_records, 1);
}

#[test]
fn creature_effect_only_target_shape_yields_to_shorter_live_boundary() {
    let mut live = ee_creature_effect_only_update_live(&[(b'A', 0x00F3)]);
    live.truncate(12 + 3);
    let legacy_effect_end = live.len();
    live.extend_from_slice(&[b'A', super::DOOR_OBJECT_TYPE, 0xB4, 0x18, 0x00]);
    live.extend_from_slice(
        &[0; super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN],
    );

    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            &live,
            0,
            live.len()
        ),
        legacy_effect_end,
        "a target-payload-shaped single effect row must not swallow a following live-object boundary"
    );
    assert!(
        super::boundary::try_get_ee_creature_update_record_end_for_transport(&live, 0, live.len())
            .is_none(),
        "byte-only EE status proof must yield to the shorter legacy boundary when 2DA target proof is absent"
    );
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
