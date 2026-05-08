use super::*;

// Decompile-owned quickbar item-object and item-appearance parser.

pub(super) fn parse_legacy_quickbar_item_payload(
    reader: &mut QuickbarPacketReader<'_>,
    model_types: &[i8],
) -> Option<(QuickbarItemObject, QuickbarItemObject)> {
    let before = reader.clone();
    let primary = match parse_legacy_quickbar_item_object(reader, true, model_types) {
        Some(item) => item,
        None => {
            *reader = before;
            return None;
        }
    };
    let secondary = match parse_legacy_quickbar_item_object(reader, false, model_types) {
        Some(item) => item,
        None => {
            *reader = before;
            return None;
        }
    };
    Some((primary, secondary))
}

fn parse_legacy_quickbar_item_object(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<QuickbarItemObject> {
    let before_present = reader.clone();
    let present = reader.read_bit()?;
    if !present {
        if looks_like_quickbar_item_object_body_at(reader, include_int_param, model_types) {
            // EE and Diamond both read primary/secondary item-object presence
            // BOOLs before any item object body. HG captures can have the CNW
            // fragment cursor shifted by a few already-consumed source bits;
            // only resync when the byte-side object body and active-property
            // tail validate completely.
            for skipped_bits in 1..=MAX_QUICKBAR_ITEM_PRESENCE_RESYNC_BITS {
                let mut trial = before_present.clone();
                let mut candidate_present = false;
                let mut bits_ok = true;
                for _ in 0..=skipped_bits {
                    match trial.read_bit() {
                        Some(bit) => candidate_present = bit,
                        None => {
                            bits_ok = false;
                            break;
                        }
                    }
                }
                if !bits_ok || !candidate_present {
                    continue;
                }
                if let Some(mut item) = parse_legacy_quickbar_item_object_body(
                    &mut trial,
                    include_int_param,
                    model_types,
                ) {
                    item.present = true;
                    tracing::info!(
                        skipped_bits,
                        cursor = before_present.cursor,
                        fragment_cursor = before_present.fragment_cursor,
                        fragment_bit = before_present.fragment_bit,
                        object_id = %format_args!("0x{:08X}", item.object_id),
                        base_item = item.base_item,
                        "server GuiQuickbar_SetAllButtons item presence bit resynced"
                    );
                    *reader = trial;
                    return Some(item);
                }
            }
            return None;
        }
        return Some(QuickbarItemObject::default());
    }

    let mut item = parse_legacy_quickbar_item_object_body(reader, include_int_param, model_types)?;
    item.present = true;
    Some(item)
}

fn parse_legacy_quickbar_item_object_body(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<QuickbarItemObject> {
    let object_id = reader.read_dword()?;
    let int_param = if include_int_param {
        reader.read_i32()?
    } else {
        -1
    };
    let (base_item, appearance_type, appearance_bytes) =
        parse_legacy_quickbar_item_appearance(reader, model_types)?;
    let active_props = parse_legacy_quickbar_active_item_properties(reader, base_item)?;

    Some(QuickbarItemObject {
        present: true,
        object_id,
        int_param,
        base_item,
        appearance_type,
        active_props: Some(active_props),
        appearance_bytes,
    })
}

fn parse_legacy_quickbar_item_appearance(
    reader: &mut QuickbarPacketReader<'_>,
    model_types: &[i8],
) -> Option<(u32, i8, Vec<u8>)> {
    let start = reader.cursor;
    let base_item_id = reader.read_dword()?;
    let model_type = *model_types.get(usize::try_from(base_item_id).ok()?)?;
    let appearance_size = legacy_item_appearance_read_size(model_type)?;
    let end = start.checked_add(appearance_size)?;
    let appearance_bytes = reader.read_buffer.get(start..end)?.to_vec();
    reader.cursor = end;
    Some((base_item_id, model_type, appearance_bytes))
}

fn skip_legacy_quickbar_item_payload(
    reader: &mut QuickbarPacketReader<'_>,
    model_types: &[i8],
) -> Option<()> {
    let before = reader.clone();
    if skip_legacy_quickbar_item_object(reader, true, model_types).is_some()
        && skip_legacy_quickbar_item_object(reader, false, model_types).is_some()
    {
        return Some(());
    }
    *reader = before;
    None
}

fn skip_legacy_quickbar_item_object(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<()> {
    let before_present = reader.clone();
    let present = reader.read_bit()?;
    if !present {
        if looks_like_quickbar_item_object_body_at(reader, include_int_param, model_types) {
            for skipped_bits in 1..=MAX_QUICKBAR_ITEM_PRESENCE_RESYNC_BITS {
                let mut trial = before_present.clone();
                let mut candidate_present = false;
                let mut bits_ok = true;
                for _ in 0..=skipped_bits {
                    match trial.read_bit() {
                        Some(bit) => candidate_present = bit,
                        None => {
                            bits_ok = false;
                            break;
                        }
                    }
                }
                if !bits_ok || !candidate_present {
                    continue;
                }
                if skip_legacy_quickbar_item_object_body(&mut trial, include_int_param, model_types)
                    .is_some()
                {
                    *reader = trial;
                    return Some(());
                }
            }
            return None;
        }
        return Some(());
    }

    skip_legacy_quickbar_item_object_body(reader, include_int_param, model_types)
}

fn skip_legacy_quickbar_item_object_body(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<()> {
    let _object_id = reader.read_dword()?;
    if include_int_param {
        let _int_param = reader.read_dword()?;
    }
    let base_item_id = reader.read_dword()?;
    let model_type = *model_types.get(usize::try_from(base_item_id).ok()?)?;
    let appearance_size = legacy_item_appearance_read_size(model_type)?;
    reader.skip_bytes(appearance_size.checked_sub(CNW_LENGTH_BYTES)?)?;
    skip_legacy_quickbar_active_item_properties(reader, base_item_id)
}

pub(super) fn looks_like_quickbar_item_object_body_at(
    reader: &QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> bool {
    let minimum = CNW_LENGTH_BYTES
        + if include_int_param {
            CNW_LENGTH_BYTES
        } else {
            0
        }
        + CNW_LENGTH_BYTES;
    if reader.cursor > reader.read_buffer.len()
        || reader.read_buffer.len().saturating_sub(reader.cursor) < minimum
    {
        return false;
    }

    let mut cursor = reader.cursor;
    let Some(_object_id) = read_u32_le(reader.read_buffer, cursor) else {
        return false;
    };
    // Diamond's decompiled `sub_53E690` is just a bounded four-byte read. Older
    // bridge heuristics required the high bit here, but HG compact object ids
    // can be low. Keep this lookahead semantic by validating the following
    // decompile-backed base-item/appearance shape instead of rejecting low ids.
    cursor += CNW_LENGTH_BYTES;
    if include_int_param {
        cursor += CNW_LENGTH_BYTES;
    }

    let Some(base_item_id) = read_u32_le(reader.read_buffer, cursor) else {
        return false;
    };
    let Some(model_type) = usize::try_from(base_item_id)
        .ok()
        .and_then(|index| model_types.get(index))
        .copied()
    else {
        return false;
    };
    let Some(legacy_size) = legacy_item_appearance_read_size(model_type) else {
        return false;
    };
    reader.read_buffer.len().saturating_sub(cursor) >= legacy_size
}

pub(super) fn legacy_item_appearance_read_size(model_type: i8) -> Option<usize> {
    match model_type {
        0 => Some(CNW_LENGTH_BYTES + 1),
        1 => Some(CNW_LENGTH_BYTES + 1 + 6),
        2 => Some(CNW_LENGTH_BYTES + 3 + 1),
        3 => Some(CNW_LENGTH_BYTES + 19 + 6),
        _ => None,
    }
}

pub(super) fn legacy_quickbar_base_item_requires_active_property_word(base_item_id: u32) -> bool {
    base_item_id == 0x10
}
