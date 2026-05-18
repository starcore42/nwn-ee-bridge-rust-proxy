use super::*;

// Inventory feature-25 object-list parser and mask integration.

pub(super) fn apply_2000(
    bytes: &[u8],
    candidates: &[GenericInventoryCandidate],
    record_end: usize,
) -> Vec<GenericInventoryCandidate> {
    let mut next = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let Some(feature25) = try_parse_inventory_2000_at(bytes, candidate.cursor, record_end)
        else {
            continue;
        };
        next.push(
            candidate.advanced(
                feature25.block_end,
                candidate.bits.saturating_add(
                    usize::try_from(feature25.second_count)
                        .unwrap_or(usize::MAX)
                        .saturating_mul(3),
                ),
            ),
        );
    }
    next
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Inventory2000Shape {
    pub second_count: u32,
    pub block_end: usize,
}

pub(super) fn try_parse_inventory_2000_record(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<Inventory2000Shape> {
    if record_end - record_offset < 11 || read_u16_le(bytes, record_offset + 5)? != 0x2000 {
        return None;
    }
    try_parse_inventory_2000_at(bytes, record_offset + 7, record_end)
}

pub(super) fn try_parse_inventory_2000_at(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<Inventory2000Shape> {
    // EE's CNWSMessage inventory writer for mask 0x2000 emits:
    //
    //   DWORD first_count, first_count * OBJECTID,
    //   DWORD second_count, second_count * OBJECTID,
    //   then three CNW BOOL fragment bits for each second-list OBJECTID.
    //
    // The BOOLs are fragment bits, so the read-buffer cursor ends at
    // Feature25Shape::block_end. HG's Diamond stream has also produced a
    // strictly bounded zero/zero-count compatibility shape with two trailing
    // OBJECTIDs before the next live-object record. Accept that only when the
    // EE-owned lists are both empty and the extra read-buffer tail is exactly
    // those two typed OBJECTIDs; this keeps the branch classified without
    // turning it into an inventory resync heuristic.
    if let Some(shape) = try_parse_zero_first_sentinel_tail_at(bytes, cursor, record_end) {
        return Some(shape);
    }

    let feature25 = try_parse_feature25_at(bytes, cursor, record_end)?;
    if feature25.missing_second_count || feature25.block_end > record_end {
        return None;
    }
    if feature25.block_end == record_end {
        return Some(Inventory2000Shape {
            second_count: feature25.second_count,
            block_end: feature25.block_end,
        });
    }
    if feature25.second_count != 0 || record_end - feature25.block_end != 8 {
        return None;
    }
    let first_tail_object = read_u32_le(bytes, feature25.block_end)?;
    let second_tail_object = read_u32_le(bytes, feature25.block_end + 4)?;
    if !looks_like_legacy_live_object_id_value(first_tail_object)
        || !looks_like_legacy_live_object_id_value(second_tail_object)
    {
        return None;
    }

    Some(Inventory2000Shape {
        second_count: 0,
        block_end: record_end,
    })
}

pub(super) fn try_parse_inventory_2000_prefix_at(
    bytes: &[u8],
    cursor: usize,
    scan_end: usize,
) -> Option<Inventory2000Shape> {
    // Prefix-only companion for Diamond `sub_455940` / EE `sub_1407B4F70`
    // inventory mask 0x2000 handling. The decompiled reader consumes the two
    // OBJECTID lists and then returns to the enclosing inventory mask reader;
    // it does not require the enclosing live-object stream to end here. Use
    // this only when another exact inventory branch owns the following bytes.
    let feature25 = try_parse_feature25_at(bytes, cursor, scan_end)?;
    if feature25.missing_second_count || feature25.block_end > scan_end {
        return None;
    }
    Some(Inventory2000Shape {
        second_count: feature25.second_count,
        block_end: feature25.block_end,
    })
}

pub(super) fn normalize_legacy_feature25_tail_for_ee(
    bytes: &mut Vec<u8>,
    cursor: usize,
    record_end: &mut usize,
) -> Option<usize> {
    if cursor > bytes.len() || *record_end > bytes.len() || cursor + 8 > *record_end {
        return None;
    }
    if read_u32_le(bytes, cursor)? != 0 {
        return None;
    }

    // Legacy zero/zero plus bounded captured tail:
    //
    //   first_count=0, second_count=0, OBJECTID...
    //
    // EE's decompiled reader stops at the second count when it is zero, so any
    // trailing object ids must be removed before the packet is emitted.
    if read_u32_le(bytes, cursor + 4)? == 0 && cursor + 8 < *record_end {
        let tail_start = cursor + 8;
        if feature25_tail_all_object_ids(bytes, tail_start, *record_end)? {
            let removed = *record_end - tail_start;
            bytes.drain(tail_start..*record_end);
            *record_end = tail_start;
            return Some(removed);
        }
    }

    // HG transition capture shape:
    //
    //   first_count=0, OBJECTID..., sentinel_zero
    //
    // Neither Diamond nor EE's client-side `0x2000` reader consumes a
    // null-terminated object vector here; both expect a second DWORD count.
    // The exact zero sentinel is therefore promoted to EE's `second_count=0`
    // and the bounded object tail is removed.
    if try_parse_zero_first_sentinel_tail_at(bytes, cursor, *record_end).is_some() {
        let tail_start = cursor + 4;
        let sentinel_start = *record_end - 4;
        let removed = sentinel_start - tail_start;
        bytes.drain(tail_start..sentinel_start);
        *record_end = cursor + 8;
        return Some(removed);
    }

    None
}

pub(super) fn insert_missing_feature25_second_count_zero_for_ee(
    bytes: &mut Vec<u8>,
    cursor: usize,
    record_end: &mut usize,
) -> Option<usize> {
    if cursor > bytes.len() || *record_end > bytes.len() || cursor > *record_end {
        return None;
    }

    let feature25 = try_parse_feature25_at(bytes, cursor, *record_end)?;
    if !feature25.missing_second_count || feature25.block_end != *record_end {
        return None;
    }

    bytes.splice(*record_end..*record_end, [0, 0, 0, 0]);
    *record_end = record_end.checked_add(4)?;
    Some(4)
}

pub(super) fn try_parse_inventory_2a00_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    let branch_cursor = record_offset.checked_add(7)?;

    let try_parse_after_0200 = |candidate: GenericInventoryCandidate,
                                cursor: usize|
     -> Option<GenericInventoryCandidate> {
        let feature25 = try_parse_inventory_2a00_feature25_at(bytes, cursor, record_end)?;
        if feature25.missing_second_count || feature25.block_end > record_end {
            return None;
        }
        let feature25_bits = usize::try_from(feature25.second_count)
            .ok()?
            .saturating_mul(3);
        let candidate = candidate.advanced(
            feature25.block_end,
            candidate.bits.saturating_add(feature25_bits),
        );

        // Diamond `sub_455940` and EE `sub_1407B4F70` both process mask
        // `0x2A00` as `0x0200 | 0x2000 | 0x0800`. After the Feature-25
        // `0x2000` object-list reader, `0x0800` consumes one BOOL. A false
        // BOOL owns no read-buffer bytes; a true BOOL owns the following
        // 12-byte appearance/status block. Proving the BOOL value here keeps
        // this shape exact instead of merely counting enough fragment bits.
        let branch_0800_bit = candidate.bits;
        if feature25.block_end == record_end {
            return Some(
                candidate
                    .require_fragment_bit(branch_0800_bit, false)?
                    .advanced(record_end, candidate.bits.saturating_add(1)),
            );
        }
        if record_end - feature25.block_end == 12 {
            return Some(
                candidate
                    .require_fragment_bit(branch_0800_bit, true)?
                    .advanced(record_end, candidate.bits.saturating_add(1)),
            );
        }
        None
    };

    if record_end - branch_cursor >= 4 && read_u32_le(bytes, branch_cursor)? == 0 {
        // For the `0x0200` branch, both clients read two BOOLs before the
        // read-buffer branch body. The second BOOL selects the layout: false
        // means the following DWORD-count branch, while true means the
        // byte-mask-list branch. The first BOOL is semantic state and does not
        // change the cursor shape, so this exact verifier only constrains the
        // layout-selecting second bit.
        let candidate =
            GenericInventoryCandidate::new(branch_cursor + 4, 2).require_fragment_bit(1, false)?;
        if let Some(candidate) = try_parse_after_0200(candidate, branch_cursor + 4) {
            return Some(candidate);
        }
    }

    if branch_cursor < record_end {
        let byte_mask_count = usize::from(bytes[branch_cursor]);
        let masks_offset = branch_cursor + 1;
        if byte_mask_count <= 64
            && masks_offset <= record_end
            && byte_mask_count <= record_end - masks_offset
        {
            let candidate = GenericInventoryCandidate::new(masks_offset + byte_mask_count, 2)
                .require_fragment_bit(1, true)?;
            return try_parse_after_0200(candidate, masks_offset + byte_mask_count);
        }
    }

    None
}

#[derive(Debug, Clone, Copy)]
struct Inventory2a00ZeroFeature25Prefix {
    read_end: usize,
    missing_second_count: bool,
}

pub(super) fn inventory_2a00_object_id_is_allowed(object_id: u32) -> bool {
    // `I/0x2A00` is read by Diamond `sub_455940` and EE `sub_1407B4F70` as an
    // OBJECTID owner plus the fixed 0x0200 -> 0x2000 -> 0x0800 mask order. The
    // clients do not require the owner to look like a materialized server oid
    // before advancing the read buffer; they resolve it for game-state updates
    // separately. HG can therefore send negative session-local sentinels such
    // as 0xFFFFFF8E in this family. Keep that allowance local to this exact
    // mask so other inventory packets still need their own decompile proof.
    matches!(object_id, 0xFFFF_FFFD | 0xFFFF_FFFE)
        || looks_like_legacy_live_object_id_value(object_id)
        || (object_id & 0xFFFF_FF00) == 0xFFFF_FF00
}

pub(super) fn try_parse_inventory_2a00_prefix_shape(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> Option<InventoryRecordPrefixClaim> {
    let prefix = try_parse_inventory_2a00_zero_feature25_prefix(bytes, record_offset, scan_end)?;
    Some(InventoryRecordPrefixClaim {
        read_end: prefix.read_end,
        fragment_bits: 3,
        interleaved_fragment_tail_allowed: prefix.read_end < scan_end,
    })
}

pub(super) fn insert_missing_inventory_2a00_feature25_second_count_zero_for_ee(
    bytes: &mut Vec<u8>,
    record_offset: usize,
    record_end: &mut usize,
) -> Option<usize> {
    let prefix = try_parse_inventory_2a00_zero_feature25_prefix(bytes, record_offset, *record_end)?;
    if !prefix.missing_second_count {
        return None;
    }

    bytes.splice(prefix.read_end..prefix.read_end, [0, 0, 0, 0]);
    *record_end = record_end.checked_add(4)?;
    Some(4)
}

fn try_parse_inventory_2a00_zero_feature25_prefix(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> Option<Inventory2a00ZeroFeature25Prefix> {
    if record_offset > bytes.len()
        || scan_end > bytes.len()
        || scan_end <= record_offset
        || scan_end - record_offset < 15
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u16_le(bytes, record_offset + 5)? != 0x2A00
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    if !inventory_2a00_object_id_is_allowed(object_id) {
        return None;
    }

    let branch_cursor = record_offset.checked_add(7)?;
    if read_u32_le(bytes, branch_cursor)? != 0 {
        return None;
    }
    let feature25_cursor = branch_cursor.checked_add(4)?;
    let first_count = read_u32_le(bytes, feature25_cursor)?;
    if first_count > MAX_REASONABLE_FEATURE25_OBJECTS {
        return None;
    }
    let first_objects = feature25_cursor.checked_add(4)?;
    let first_end =
        first_objects.checked_add(usize::try_from(first_count).ok()?.checked_mul(4)?)?;
    if first_end > scan_end
        || !looks_like_inventory_2a00_feature25_object_list(
            bytes,
            first_objects,
            first_count,
            first_end,
        )
    {
        return None;
    }

    if first_end == scan_end || scan_end - first_end < 4 {
        return Some(Inventory2a00ZeroFeature25Prefix {
            read_end: first_end,
            missing_second_count: true,
        });
    }

    let second_count = read_u32_le(bytes, first_end)?;
    if second_count > MAX_REASONABLE_FEATURE25_OBJECTS {
        return Some(Inventory2a00ZeroFeature25Prefix {
            read_end: first_end,
            missing_second_count: true,
        });
    }
    let second_objects = first_end.checked_add(4)?;
    let second_end =
        second_objects.checked_add(usize::try_from(second_count).ok()?.checked_mul(4)?)?;
    if second_end > scan_end
        || !looks_like_inventory_2a00_feature25_object_list(
            bytes,
            second_objects,
            second_count,
            second_end,
        )
    {
        return Some(Inventory2a00ZeroFeature25Prefix {
            read_end: first_end,
            missing_second_count: true,
        });
    }

    Some(Inventory2a00ZeroFeature25Prefix {
        read_end: second_end,
        missing_second_count: false,
    })
}

fn try_parse_inventory_2a00_feature25_at(
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
        || !looks_like_inventory_2a00_feature25_object_list(
            bytes,
            first_objects,
            first_count,
            first_end,
        )
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
        || !looks_like_inventory_2a00_feature25_object_list(
            bytes,
            second_objects,
            second_count,
            second_end,
        )
    {
        return None;
    }
    Some(Feature25Shape {
        second_count,
        block_end: second_end,
        missing_second_count: false,
    })
}

fn looks_like_inventory_2a00_feature25_object_list(
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
        if !inventory_2a00_object_id_is_allowed(object_id) {
            return false;
        }
    }
    true
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

fn try_parse_zero_first_sentinel_tail_at(
    bytes: &[u8],
    cursor: usize,
    record_end: usize,
) -> Option<Inventory2000Shape> {
    if cursor > bytes.len() || record_end > bytes.len() || cursor + 12 > record_end {
        return None;
    }
    if read_u32_le(bytes, cursor)? != 0 || read_u32_le(bytes, record_end - 4)? != 0 {
        return None;
    }
    let tail_start = cursor + 4;
    let sentinel_start = record_end - 4;
    if tail_start >= sentinel_start || (sentinel_start - tail_start) % 4 != 0 {
        return None;
    }
    if !feature25_tail_all_object_ids(bytes, tail_start, sentinel_start)? {
        return None;
    }
    Some(Inventory2000Shape {
        second_count: 0,
        block_end: record_end,
    })
}

fn feature25_tail_all_object_ids(bytes: &[u8], offset: usize, record_end: usize) -> Option<bool> {
    if offset >= record_end || record_end > bytes.len() || (record_end - offset) % 4 != 0 {
        return None;
    }
    let count = (record_end - offset) / 4;
    if count == 0 || count > usize::try_from(MAX_REASONABLE_FEATURE25_OBJECTS).ok()? {
        return None;
    }
    for index in 0..count {
        let object_id = read_u32_le(bytes, offset + index * 4)?;
        if !looks_like_legacy_live_object_id_value(object_id) {
            return None;
        }
    }
    Some(true)
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
