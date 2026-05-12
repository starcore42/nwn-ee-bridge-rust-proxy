use super::*;

/// Exact EE-side `GuiQuickbar_SetAllButtons` validator for packets emitted by
/// the quickbar translator.
///
/// This intentionally validates the receiver shape from EE decompiles:
/// `sub_14079DB00` loops 36 slots, type 1 calls `sub_14079FAC0` for each item
/// object, `sub_14079FAC0` widens the Diamond item model-part bytes to the
/// EE feature-0x23 WORD fields, and then calls `sub_140973160` for the EE
/// visual-transform maps. The validator is therefore a post-translation proof,
/// not a generic "known opcode" allow-list.
pub(crate) fn ee_set_all_buttons_payload_shape_valid(payload: &[u8]) -> bool {
    ee_set_all_buttons_slot_types_if_valid(payload).is_some()
}

pub(in crate::translate::quickbar) fn ee_set_all_buttons_slot_types_if_valid(
    payload: &[u8],
) -> Option<Vec<u8>> {
    let Some(high) = HighLevel::parse(payload) else {
        return None;
    };
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return None;
    }

    let Some(declared) =
        read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return None;
    };
    if declared < HIGH_LEVEL_HEADER_BYTES {
        return None;
    }

    let read_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    let Some(read_size) = declared.checked_sub(HIGH_LEVEL_HEADER_BYTES) else {
        return None;
    };
    let Some(read_end) = read_start.checked_add(read_size) else {
        return None;
    };
    if read_end >= payload.len() {
        return None;
    }

    let Some(read_buffer) = payload.get(read_start..read_end) else {
        return None;
    };
    let Some(fragments) = payload.get(read_end..) else {
        return None;
    };
    if fragments.is_empty() {
        return None;
    }

    let Some(model_types) = quickbar_base_item_model_types() else {
        return None;
    };
    let mut reader = QuickbarPacketReader {
        read_buffer,
        fragments,
        cursor: 0,
        fragment_cursor: 0,
        fragment_bit: 0,
        final_fragment_bits: 0,
    };
    let Some(final_fragment_bits) = reader.read_bits(3).and_then(|bits| u8::try_from(bits).ok())
    else {
        return None;
    };
    reader.final_fragment_bits = final_fragment_bits;

    let mut slot_types = Vec::with_capacity(LEGACY_QUICKBAR_BUTTON_COUNT);
    for _slot in 0..LEGACY_QUICKBAR_BUTTON_COUNT {
        let Some(ty) = reader.read_byte() else {
            return None;
        };
        slot_types.push(ty);
        match ty {
            0 => {}
            1 => {
                if !validate_ee_quickbar_item_object(&mut reader, true, model_types) {
                    return None;
                }
                if !validate_ee_quickbar_item_object(&mut reader, false, model_types) {
                    return None;
                }
            }
            2 => {
                if reader.skip_bytes(1 + CNW_LENGTH_BYTES + 1 + 1).is_none() {
                    return None;
                }
            }
            _ => {
                if !validate_ee_quickbar_general_button(&mut reader, ty) {
                    return None;
                }
            }
        }
    }

    if reader.cursor != reader.read_buffer.len() {
        return None;
    }
    let Some(consumed_fragment_bits) = reader
        .fragment_cursor
        .checked_mul(8)
        .and_then(|bits| bits.checked_add(usize::from(reader.fragment_bit)))
    else {
        return None;
    };
    let consumed_fragment_bytes = reader.fragment_cursor + usize::from(reader.fragment_bit != 0);
    if consumed_fragment_bytes == reader.fragments.len()
        && reader.final_fragment_bits == u8::try_from(consumed_fragment_bits % 8).unwrap_or(0)
    {
        Some(slot_types)
    } else {
        None
    }
}

fn validate_ee_quickbar_item_object(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> bool {
    let Some(present) = reader.read_bit() else {
        return false;
    };
    if !present {
        return true;
    }
    if reader.read_dword().is_none() {
        return false;
    }
    if include_int_param && reader.read_i32().is_none() {
        return false;
    }
    let Some(base_item) = validate_ee_quickbar_item_appearance(reader, model_types) else {
        return false;
    };
    validate_empty_ee_visual_transform_map(reader)
        && validate_ee_quickbar_active_item_properties(reader, base_item)
}

fn validate_ee_quickbar_item_appearance(
    reader: &mut QuickbarPacketReader<'_>,
    model_types: &[i8],
) -> Option<u32> {
    let base_item = reader.read_dword()?;
    let model_type = *model_types.get(usize::try_from(base_item).ok()?)?;
    match model_type {
        0 => {
            reader.read_word()?;
        }
        1 => {
            reader.skip_bytes(2 + 6)?;
        }
        2 => {
            reader.skip_bytes(2 + 2 + 2 + 1)?;
        }
        3 => {
            reader.skip_bytes((19 * 2) + 6)?;
            reader.skip_bytes(EE_QUICKBAR_ARMOR_LAYERED_COLOR_BYTES)?;
        }
        _ => return None,
    }
    Some(base_item)
}

fn validate_empty_ee_visual_transform_map(reader: &mut QuickbarPacketReader<'_>) -> bool {
    // EE `sub_140973160` takes the feature-0x23 count-prefixed path in this
    // EE-facing session. The translator currently owns only the neutral
    // transform state, represented by two empty maps.
    let Some(first_count) = reader.read_dword() else {
        return false;
    };
    let Some(second_count) = reader.read_dword() else {
        return false;
    };
    first_count == 0 && second_count == 0
}

fn validate_ee_quickbar_active_item_properties(
    reader: &mut QuickbarPacketReader<'_>,
    base_item: u32,
) -> bool {
    if legacy_quickbar_base_item_requires_active_property_word(base_item)
        && reader.read_word().is_none()
    {
        return false;
    }

    let Some(name_is_locstring) = reader.read_bit() else {
        return false;
    };
    if name_is_locstring {
        if reader.skip_loc_string().is_none() {
            return false;
        }
    } else if reader.skip_string().is_none() {
        return false;
    }

    if reader.read_bit().is_none() || reader.read_dword().is_none() || reader.read_dword().is_none()
    {
        return false;
    }

    let Some(ee_only_can_use_item) = reader.read_bit() else {
        return false;
    };
    if ee_only_can_use_item {
        // Diamond has no source bit for this EE-only field. The translator owns
        // the semantic expansion and must emit the same neutral false value used
        // by the live-object equipment item translator.
        return false;
    }

    if reader.read_bit().is_none() || reader.read_bit().is_none() || reader.read_bit().is_none() {
        return false;
    }

    let Some(property_count) = reader.read_byte() else {
        return false;
    };
    if property_count > MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES {
        return false;
    }
    for _ in 0..property_count {
        if reader.skip_bytes(2 + 2 + 2 + 1).is_none() {
            return false;
        }
    }

    if reader.read_byte().is_none() {
        return false;
    }
    let Some(value_mask) = reader.read_byte() else {
        return false;
    };
    for bit in 0..8 {
        if (value_mask & (1u8 << bit)) != 0 && reader.read_byte().is_none() {
            return false;
        }
    }
    true
}
