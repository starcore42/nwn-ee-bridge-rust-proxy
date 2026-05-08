use super::*;

// Inventory feature-25 object-list parser and mask integration.

pub(super) fn apply_2000(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let Some(feature25) = try_parse_feature25_at(bytes, candidate.cursor, record_end) else {
            continue;
        };
        if feature25.missing_second_count || feature25.block_end > record_end {
            continue;
        }
        next.push(GenericInventoryCandidate {
            cursor: feature25.block_end,
            bits: candidate.bits.saturating_add(
                usize::try_from(feature25.second_count)
                    .unwrap_or(usize::MAX)
                    .saturating_mul(3),
            ),
        });
    }
    next
}

pub(super) fn try_parse_inventory_2a00_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let branch_cursor = record_offset.checked_add(7)?;
    let try_parse_after_0200 = |cursor: usize| -> Option<usize> {
        let feature25 = try_parse_feature25_at(bytes, cursor, record_end)?;
        if feature25.missing_second_count || feature25.block_end > record_end {
            return None;
        }
        if feature25.block_end != record_end && record_end - feature25.block_end != 12 {
            return None;
        }
        Some(
            2usize
                .saturating_add(
                    usize::try_from(feature25.second_count)
                        .ok()?
                        .saturating_mul(3),
                )
                .saturating_add(1),
        )
    };

    if record_end - branch_cursor >= 4 && read_u32_le(bytes, branch_cursor)? == 0 {
        if let Some(bits) = try_parse_after_0200(branch_cursor + 4) {
            return Some(bits);
        }
    }

    if branch_cursor < record_end {
        let byte_mask_count = usize::from(bytes[branch_cursor]);
        let masks_offset = branch_cursor + 1;
        if byte_mask_count <= 64
            && masks_offset <= record_end
            && byte_mask_count <= record_end - masks_offset
        {
            return try_parse_after_0200(masks_offset + byte_mask_count);
        }
    }

    None
}

pub(super) fn try_parse_feature25_record(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<Feature25Shape> {
    if record_end - record_offset < 11 || read_u16_le(bytes, record_offset + 5)? != 0x2000 {
        return None;
    }
    let shape = try_parse_feature25_at(bytes, record_offset + 7, record_end)?;
    (shape.block_end == record_end).then_some(shape)
}

fn try_parse_feature25_at(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<Feature25Shape> {
    if cursor > bytes.len() || record_end > bytes.len() || cursor > record_end {
        return None;
    }
    let first_count = read_u32_le(bytes, cursor)?;
    if first_count > MAX_REASONABLE_FEATURE25_OBJECTS {
        return None;
    }
    let first_objects = cursor.checked_add(4)?;
    let first_end =
        first_objects.checked_add(usize::try_from(first_count).ok()?.checked_mul(4)?)?;
    if first_end > record_end
        || !looks_like_feature25_object_list(bytes, first_objects, first_count, first_end)
    {
        return None;
    }
    if first_end == record_end {
        return Some(Feature25Shape {
            block_end: record_end,
            missing_second_count: true,
            ..Feature25Shape::default()
        });
    }
    if record_end - first_end < 4 {
        return None;
    }
    let second_count = read_u32_le(bytes, first_end)?;
    if second_count > MAX_REASONABLE_FEATURE25_OBJECTS {
        return None;
    }
    let second_objects = first_end.checked_add(4)?;
    let second_end =
        second_objects.checked_add(usize::try_from(second_count).ok()?.checked_mul(4)?)?;
    if second_end > record_end
        || !looks_like_feature25_object_list(bytes, second_objects, second_count, second_end)
    {
        return None;
    }
    Some(Feature25Shape {
        second_count,
        block_end: second_end,
        missing_second_count: false,
    })
}

fn looks_like_feature25_object_list(
    bytes: &[u8],
    offset: usize,
    count: u32,
    record_end: usize,
) -> bool {
    if count > MAX_REASONABLE_FEATURE25_OBJECTS
        || offset > record_end
        || record_end > bytes.len()
        || usize::try_from(count)
            .ok()
            .is_none_or(|count| count > (record_end - offset) / 4)
    {
        return false;
    }
    for index in 0..usize::try_from(count).unwrap_or(usize::MAX) {
        let Some(object_id) = read_u32_le(bytes, offset + index * 4) else {
            return false;
        };
        if !looks_like_legacy_live_object_id_value(object_id) {
            return false;
        }
    }
    true
}
