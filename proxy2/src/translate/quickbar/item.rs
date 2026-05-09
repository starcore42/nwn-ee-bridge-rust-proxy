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

pub(super) fn parse_legacy_quickbar_compact_byte_item_payload(
    read_buffer: &[u8],
    payload_start: usize,
    record_end: usize,
    model_types: &[i8],
) -> Option<(QuickbarItemObject, QuickbarItemObject)> {
    let (primary, cursor) = parse_legacy_quickbar_compact_byte_item_object_body(
        read_buffer,
        payload_start,
        record_end,
        true,
        model_types,
    )?;
    if cursor != record_end {
        return None;
    }
    Some((primary, QuickbarItemObject::default()))
}

fn parse_legacy_quickbar_item_object(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<QuickbarItemObject> {
    let present = reader.read_bit()?;
    if !present {
        // Diamond `sub_469FD0` and EE `sub_14079DB00` both gate the item-object
        // body strictly on this BOOL.  A false presence bit owns no read-buffer
        // bytes; peeking ahead for an item-looking body is unsafe because the
        // following spell/general slot bytes can coincidentally resemble an
        // object id + base-item prefix.  If a capture ever presents a body after
        // a false bit, that packet must be quarantined and researched rather than
        // resynchronized heuristically.
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
    let active_props =
        parse_legacy_quickbar_active_item_properties(reader, base_item, appearance_type == 2)?;

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

fn parse_legacy_quickbar_compact_byte_item_object_body(
    read_buffer: &[u8],
    mut cursor: usize,
    record_end: usize,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<(QuickbarItemObject, usize)> {
    let object_id = read_u32_le(read_buffer, cursor)?;
    cursor = cursor.checked_add(CNW_LENGTH_BYTES)?;
    let int_param = if include_int_param {
        let value = i32::from_le_bytes(read_u32_le(read_buffer, cursor)?.to_le_bytes());
        cursor = cursor.checked_add(CNW_LENGTH_BYTES)?;
        value
    } else {
        -1
    };

    let appearance_start = cursor;
    let base_item = read_u32_le(read_buffer, cursor)?;
    let appearance_type = *model_types.get(usize::try_from(base_item).ok()?)?;
    let appearance_size = legacy_item_appearance_read_size(appearance_type)?;
    let appearance_end = appearance_start.checked_add(appearance_size)?;
    if appearance_end > record_end {
        return None;
    }
    let appearance_bytes = read_buffer.get(appearance_start..appearance_end)?.to_vec();
    cursor = appearance_end;
    let (active_props, cursor) = parse_legacy_quickbar_active_item_properties_compact_byte_tail(
        read_buffer,
        cursor,
        record_end,
        base_item,
        appearance_type == 2,
    )?;

    Some((
        QuickbarItemObject {
            present: true,
            object_id,
            int_param,
            base_item,
            appearance_type,
            active_props: Some(active_props),
            appearance_bytes,
        },
        cursor,
    ))
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
    let present = reader.read_bit()?;
    if !present {
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
