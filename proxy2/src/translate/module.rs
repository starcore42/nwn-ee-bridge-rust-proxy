//! Module-info (`P 03/01`) semantic rewrites.
//!
//! This file deliberately knows nothing about UDP, reliable-window headers,
//! compression, or CRCs. It answers one narrow question:
//!
//! Given a verified Diamond-era Module_Info payload, what exact EE-dialect
//! Module_Info payload should be emitted?
//!
//! Decompile anchor:
//! - EE `CNWSMessage::SendServerToPlayerModule_Info` delegates to
//!   `CNWSModule::PackModuleIntoMessage` and sends major/minor `3/1`.
//! - EE `CNWSModule::PackModuleIntoMessage` writes module name as
//!   `WriteCExoString(..., 0x20)`, module description through
//!   `WriteCExoLocStringServer(..., 0)`, one module flag byte, custom TLK as
//!   `WriteCResRef(..., 0x10)`, module area count, area ids/names, and a
//!   short EE tail. It does not write Diamond's legacy
//!   `hak_count + hak[16]... + module_resref[16]` block before the resource /
//!   area-name table.
//! - Diamond/1.69 module-info streams observed from HG include that legacy hak
//!   block. Removing it moves the following resource/area table to the offset
//!   where EE expects it.

const MAX_MODULE_INFO_STRING: usize = 4096;
const MAX_MODULE_INFO_PREFIX_SCAN: usize = 1024;
const MAX_LEGACY_HAK_BLOCK_LOOKAHEAD: usize = 128;
const MAX_LEGACY_HAK_COUNT: usize = 64;
// EE writes the module area count here, then one OBJECTID + CExoString area
// name per entry. This is not an arbitrary resource-table count, so keep it
// tight enough to reject accidental 32-bit values found while scanning.
const MAX_MODULE_RESOURCE_COUNT: u32 = 4096;
const MAX_AREA_NAME_LENGTH: usize = 512;
const ZERO_NAME_TERMINATOR_MIN_ENTRIES: u32 = 32;
const RESREF_BYTES: usize = 16;

#[derive(Debug, Clone)]
pub struct RewriteSummary {
    pub offset: usize,
    pub hak_count: u8,
    pub removed_hak_bytes: usize,
    pub legacy_tail_removed: usize,
    pub old_declared: u32,
    pub new_declared: u32,
    pub resource_count: u32,
    pub resource_name_count: u32,
    pub zero_length_name_repairs: u32,
    pub zero_length_name_terminator: bool,
}

#[derive(Debug, Clone)]
struct ModuleInfoView {
    declared: u32,
    hak_search_start: usize,
}

#[derive(Debug, Clone)]
struct LegacyHakBlock {
    offset: usize,
    hak_count: u8,
    skipped_bytes: usize,
    resource_count: u32,
}

#[derive(Debug, Clone)]
struct TableRewrite {
    new_count: u32,
    old_declared: u32,
    new_declared: u32,
    tail_removed: usize,
    zero_length_name_repairs: u32,
    zero_length_name_terminator: bool,
}

pub fn rewrite_module_info_payload(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    if let Some(summary) = rewrite_module_info_payload_at_zero(payload) {
        return Some(summary);
    }

    for offset in module_info_candidate_offsets(payload) {
        if offset == 0 {
            continue;
        }

        let mut tail = payload[offset..].to_vec();
        let mut summary = rewrite_module_info_payload_at_zero(&mut tail)?;
        summary.offset = offset;
        payload.truncate(offset);
        payload.extend_from_slice(&tail);
        return Some(summary);
    }

    None
}

pub fn first_module_info_candidate_offset(payload: &[u8]) -> Option<usize> {
    module_info_candidate_offsets(payload).into_iter().next()
}

fn rewrite_module_info_payload_at_zero(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    let view = parse_module_info(payload)?;
    let hak_block = find_legacy_hak_block(payload, &view)?;

    if hak_block.skipped_bytes == 0
        || hak_block.skipped_bytes >= view.declared as usize
        || hak_block.offset + hak_block.skipped_bytes > view.declared as usize
        || hak_block.offset + hak_block.skipped_bytes > payload.len()
    {
        return None;
    }

    let old_declared = view.declared;
    let new_declared = old_declared.checked_sub(hak_block.skipped_bytes as u32)?;
    let erase_begin = hak_block.offset;
    let erase_end = erase_begin + hak_block.skipped_bytes;
    payload.drain(erase_begin..erase_end);
    write_le_u32(payload, 3, new_declared)?;

    let table_rewrite = rewrite_load_module_resource_name_table_tail(payload, erase_begin);
    let final_declared = read_le_u32(payload, 3)?;

    Some(RewriteSummary {
        offset: 0,
        hak_count: hak_block.hak_count,
        removed_hak_bytes: hak_block.skipped_bytes,
        legacy_tail_removed: table_rewrite
            .as_ref()
            .map(|rewrite| rewrite.tail_removed)
            .unwrap_or(0),
        old_declared,
        new_declared: final_declared,
        resource_count: hak_block.resource_count,
        resource_name_count: table_rewrite
            .as_ref()
            .map(|rewrite| rewrite.new_count)
            .unwrap_or(hak_block.resource_count),
        zero_length_name_repairs: table_rewrite
            .as_ref()
            .map(|rewrite| rewrite.zero_length_name_repairs)
            .unwrap_or(0),
        zero_length_name_terminator: table_rewrite
            .as_ref()
            .map(|rewrite| rewrite.zero_length_name_terminator)
            .unwrap_or(false),
    })
}

fn module_info_candidate_offsets(payload: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    if payload.len() < 16 {
        return offsets;
    }

    for offset in 0..=payload.len() - 16 {
        if !is_high_level_envelope(payload[offset])
            || payload.get(offset + 1).copied() != Some(3)
            || payload.get(offset + 2).copied() != Some(1)
        {
            continue;
        }
        let Some(declared) = read_le_u32(payload, offset + 3) else {
            continue;
        };
        if declared >= 7 && declared as usize <= payload.len().saturating_sub(offset) {
            offsets.push(offset);
        }
    }
    offsets
}

fn parse_module_info(payload: &[u8]) -> Option<ModuleInfoView> {
    if payload.len() < 16 || !is_high_level_envelope(*payload.first()?) {
        return None;
    }
    if payload.get(1).copied()? != 3 || payload.get(2).copied()? != 1 {
        return None;
    }

    let declared = read_le_u32(payload, 3)?;
    if declared < 4 || declared as usize > payload.len() {
        return None;
    }

    let hak_search_start = module_info_hak_search_start(payload, declared as usize)?;

    Some(ModuleInfoView {
        declared,
        hak_search_start,
    })
}

fn find_legacy_hak_block(payload: &[u8], view: &ModuleInfoView) -> Option<LegacyHakBlock> {
    if view.hak_search_start >= payload.len() {
        return None;
    }

    let search_start = view.hak_search_start;
    let search_end = (view.declared as usize)
        .min(payload.len())
        .min(search_start + MAX_LEGACY_HAK_BLOCK_LOOKAHEAD);
    best_legacy_hak_block_in_range(payload, search_start, search_end, view.declared as usize)
}

fn module_info_hak_search_start(payload: &[u8], declared: usize) -> Option<usize> {
    let mut starts = Vec::with_capacity(4);

    if let Some(cursor) = parse_legacy_string_module_info_prefix(payload, declared, true) {
        starts.push(cursor);
    }
    if let Some(cursor) = parse_legacy_string_module_info_prefix(payload, declared, false) {
        starts.push(cursor);
    }
    if let Some(cursor) = parse_ee_module_info_prefix(payload, declared) {
        starts.push(cursor);
    }

    for start in starts.into_iter().filter(|cursor| *cursor < declared) {
        if valid_legacy_hak_block_near(payload, start, declared).is_some() {
            return Some(start);
        }
    }

    scan_for_valid_legacy_hak_block_after_module_name(payload, declared)
}

fn parse_ee_module_info_prefix(payload: &[u8], declared: usize) -> Option<usize> {
    let mut cursor = 7;
    read_raw_string_bounded(payload, &mut cursor, declared)?;
    skip_loc_string_server(payload, &mut cursor, declared)?;
    skip_exact(payload, &mut cursor, 1, declared)?;
    skip_exact(payload, &mut cursor, RESREF_BYTES, declared)?;
    Some(cursor)
}

fn parse_legacy_string_module_info_prefix(
    payload: &[u8],
    declared: usize,
    custom_tlk_is_resref: bool,
) -> Option<usize> {
    let mut cursor = 7;
    read_raw_string_bounded(payload, &mut cursor, declared)?;
    read_raw_string_bounded(payload, &mut cursor, declared)?;
    skip_exact(payload, &mut cursor, 1, declared)?;
    if custom_tlk_is_resref {
        skip_exact(payload, &mut cursor, RESREF_BYTES, declared)?;
    } else {
        read_raw_string_bounded(payload, &mut cursor, declared)?;
    }
    Some(cursor)
}

fn skip_loc_string_server(payload: &[u8], cursor: &mut usize, declared: usize) -> Option<()> {
    let has_strref_or_raw_string = *payload.get(*cursor)?;
    *cursor += 1;

    if has_strref_or_raw_string == 0 {
        read_raw_string_bounded(payload, cursor, declared)
    } else if has_strref_or_raw_string == 1 {
        skip_exact(payload, cursor, 1 + 4, declared)
    } else {
        None
    }
}

fn scan_for_valid_legacy_hak_block_after_module_name(
    payload: &[u8],
    declared: usize,
) -> Option<usize> {
    let mut cursor = 7;
    read_raw_string_bounded(payload, &mut cursor, declared)?;

    let scan_end = declared.min(cursor.saturating_add(MAX_MODULE_INFO_PREFIX_SCAN));
    (cursor..scan_end).find(|offset| valid_legacy_hak_block_at(payload, *offset, declared).is_some())
}

fn valid_legacy_hak_block_near(
    payload: &[u8],
    offset: usize,
    declared: usize,
) -> Option<LegacyHakBlock> {
    let search_end = declared
        .min(payload.len())
        .min(offset + MAX_LEGACY_HAK_BLOCK_LOOKAHEAD);
    best_legacy_hak_block_in_range(payload, offset, search_end, declared)
}

fn best_legacy_hak_block_in_range(
    payload: &[u8],
    start: usize,
    end: usize,
    declared: usize,
) -> Option<LegacyHakBlock> {
    let mut first_empty_block = None;

    for candidate in start..end {
        let Some(block) = valid_legacy_hak_block_at(payload, candidate, declared) else {
            continue;
        };

        // Diamond PackModuleIntoMessage writes the hak count byte immediately
        // before hak_count fixed 16-byte resrefs, then a fixed 16-byte module
        // resref, then the area count. In live HG module-info streams that
        // count is 23. Because the preceding localized-string/custom-tlk area
        // contains byte-looking zeros, a forward scan can also find plausible
        // zero-hak shapes before the real block. Prefer the decompile-shaped
        // non-empty block, only falling back to a zero-hak block for modules
        // that genuinely declare no haks.
        if block.hak_count != 0 {
            return Some(block);
        }

        if first_empty_block.is_none() {
            first_empty_block = Some(block);
        }
    }

    first_empty_block
}

fn valid_legacy_hak_block_at(payload: &[u8], offset: usize, declared: usize) -> Option<LegacyHakBlock> {
    let count = *payload.get(offset)? as usize;
    if count > MAX_LEGACY_HAK_COUNT {
        return None;
    }

    let skipped_bytes = 1 + count * RESREF_BYTES + RESREF_BYTES;
    if skipped_bytes > declared.saturating_sub(offset)
        || skipped_bytes > payload.len().saturating_sub(offset)
        || declared.saturating_sub(offset + skipped_bytes) < 4
    {
        return None;
    }

    let resource_count = read_le_u32(payload, offset + skipped_bytes)?;
    if resource_count == 0 || resource_count > MAX_MODULE_RESOURCE_COUNT {
        return None;
    }

    for index in 0..count {
        let start = offset + 1 + index * RESREF_BYTES;
        if !is_fixed_resref(&payload[start..start + RESREF_BYTES], false) {
            return None;
        }
    }

    let module_start = offset + 1 + count * RESREF_BYTES;
    if !is_fixed_resref(&payload[module_start..module_start + RESREF_BYTES], true) {
        return None;
    }

    valid_area_name_table_prefix(payload, offset + skipped_bytes + 4, resource_count, declared)?;

    Some(LegacyHakBlock {
        offset,
        hak_count: count as u8,
        skipped_bytes,
        resource_count,
    })
}

fn valid_area_name_table_prefix(
    payload: &[u8],
    cursor: usize,
    resource_count: u32,
    declared: usize,
) -> Option<()> {
    if resource_count == 0 || cursor + 4 > declared || cursor + 4 > payload.len() {
        return None;
    }

    let first_area_id = read_le_u32(payload, cursor)?;
    is_likely_area_resource_id(first_area_id).then_some(())
}

fn rewrite_load_module_resource_name_table_tail(
    payload: &mut Vec<u8>,
    count_offset: usize,
) -> Option<TableRewrite> {
    if payload.len() < 8
        || count_offset + 4 > payload.len()
        || !is_high_level_envelope(*payload.first()?)
        || payload.get(1).copied()? != 3
        || payload.get(2).copied()? != 1
    {
        return None;
    }

    let declared = read_le_u32(payload, 3)?;
    if declared < 7 || declared as usize > payload.len() || count_offset + 4 > declared as usize {
        return None;
    }

    let old_count = read_le_u32(payload, count_offset)?;
    if old_count == 0 || old_count > MAX_MODULE_RESOURCE_COUNT {
        return None;
    }

    let mut cursor = count_offset + 4;
    let mut valid_count = 0_u32;
    let mut zero_length_name_repairs = 0_u32;
    let mut zero_length_name_terminator = false;

    while valid_count < old_count {
        if cursor + 8 > declared as usize {
            break;
        }

        let mut name_length = read_le_u32(payload, cursor + 4)? as usize;
        if name_length == 0 {
            if valid_count >= ZERO_NAME_TERMINATOR_MIN_ENTRIES {
                // The legacy payload can carry a zero-length resource-name sentinel after the
                // area-name records. EE's CNWSModule::PackModuleIntoMessage does not emit that
                // sentinel: it writes the area count, then exactly that many
                // OBJECTID/CExoString(area-name) pairs, followed immediately by the EE tail
                // fields. Treating the sentinel as an area makes the EE client consume one fake
                // area row and shifts the following fragment/tail bytes, which shows up as
                // nwmessage fragment-offset overflows during module load.
                zero_length_name_terminator = true;
                break;
            }

            if let Some(inferred) = infer_zero_length_area_name(payload, cursor, declared as usize)
            {
                write_le_u32(payload, cursor + 4, inferred as u32)?;
                name_length = inferred;
                zero_length_name_repairs += 1;
            }
        }

        if name_length > MAX_AREA_NAME_LENGTH
            || name_length > (declared as usize).saturating_sub(cursor + 8)
        {
            break;
        }

        cursor += 8 + name_length;
        valid_count += 1;
    }

    if valid_count == 0 || !zero_length_name_terminator {
        return None;
    }

    let fragment_offset = declared as usize;
    if cursor + 1 > fragment_offset {
        return None;
    }

    let fragments = payload[fragment_offset..].to_vec();
    let new_declared = (cursor + 1) as u32;
    let tail_removed = declared.checked_sub(new_declared)? as usize;
    if tail_removed == 0 {
        return None;
    }

    write_le_u32(payload, count_offset, valid_count)?;
    payload.truncate(cursor);
    payload.push(0);
    payload.extend_from_slice(&fragments);
    write_le_u32(payload, 3, new_declared)?;

    Some(TableRewrite {
        new_count: valid_count,
        old_declared: declared,
        new_declared,
        tail_removed,
        zero_length_name_repairs,
        zero_length_name_terminator,
    })
}

fn read_raw_string_bounded(payload: &[u8], cursor: &mut usize, bound: usize) -> Option<()> {
    if *cursor > bound || bound > payload.len() {
        return None;
    }
    if payload.len().saturating_sub(*cursor) < 4 {
        return None;
    }
    let length = read_le_u32(payload, *cursor)? as usize;
    *cursor += 4;
    if *cursor > bound
        || length > MAX_MODULE_INFO_STRING
        || length > payload.len().saturating_sub(*cursor)
        || length > bound.saturating_sub(*cursor)
    {
        return None;
    }
    *cursor += length;
    Some(())
}

fn skip_exact(payload: &[u8], cursor: &mut usize, length: usize, bound: usize) -> Option<()> {
    if *cursor > bound
        || bound > payload.len()
        || length > payload.len().saturating_sub(*cursor)
        || length > bound.saturating_sub(*cursor)
    {
        return None;
    }
    *cursor += length;
    Some(())
}

fn infer_zero_length_area_name(
    payload: &[u8],
    entry_offset: usize,
    declared: usize,
) -> Option<usize> {
    if entry_offset + 8 >= declared || declared > payload.len() {
        return None;
    }

    let name_offset = entry_offset + 8;
    let max_candidate = MAX_AREA_NAME_LENGTH.min(declared.saturating_sub(name_offset));
    for candidate_length in 1..=max_candidate {
        if !is_likely_area_name(payload, name_offset, candidate_length, declared) {
            break;
        }

        let next_entry_offset = name_offset + candidate_length;
        if next_entry_offset + 8 > declared {
            continue;
        }

        let next_id = read_le_u32(payload, next_entry_offset)?;
        let next_name_length = read_le_u32(payload, next_entry_offset + 4)? as usize;
        if !is_likely_area_resource_id(next_id)
            || next_name_length > MAX_AREA_NAME_LENGTH
            || next_name_length > declared.saturating_sub(next_entry_offset + 8)
        {
            continue;
        }
        if next_name_length != 0
            && !is_likely_area_name(payload, next_entry_offset + 8, next_name_length, declared)
        {
            continue;
        }

        return Some(candidate_length);
    }

    None
}

fn is_likely_area_name(payload: &[u8], offset: usize, length: usize, declared: usize) -> bool {
    if length == 0
        || length > MAX_AREA_NAME_LENGTH
        || offset > declared
        || length > declared.saturating_sub(offset)
    {
        return false;
    }

    payload[offset..offset + length]
        .iter()
        .all(|byte| matches!(*byte, b'\t' | b'\r' | b'\n' | 0x20..=0x7e))
}

fn is_likely_area_resource_id(id: u32) -> bool {
    (id & 0x8000_0000) != 0 && id != 0xffff_ffff
}

fn is_fixed_resref(bytes: &[u8], allow_empty: bool) -> bool {
    if bytes.len() != RESREF_BYTES {
        return false;
    }

    let mut length = 0;
    while length < RESREF_BYTES && bytes[length] != 0 {
        if !is_resref_char(bytes[length]) {
            return false;
        }
        length += 1;
    }
    if length == 0 && !allow_empty {
        return false;
    }
    // Diamond's `sub_4FC600` copies the requested width from the CResRef
    // storage. The decompile proves the field width is 16 bytes, but live HG
    // haks show bytes after the first NUL are not guaranteed to be sanitized.
    // Validate the meaningful leading resref segment and let the surrounding
    // decompile-ordered count/module/area checks prove this is the hak block.
    true
}

fn is_resref_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn is_high_level_envelope(byte: u8) -> bool {
    byte == b'P' || byte == 0x70
}

fn read_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn write_le_u32(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let slice = bytes.get_mut(offset..offset + 4)?;
    slice.copy_from_slice(&value.to_le_bytes());
    Some(())
}
