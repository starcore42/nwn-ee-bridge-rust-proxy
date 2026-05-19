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
//! - EE `CNWSModule::PackModuleIntoMessage` writes a leading module string,
//!   module name through `WriteCExoLocStringServer(..., 0)`, one module byte,
//!   the module `CResRef`, module area count, area ids/names, and a short EE
//!   tail. It does not write Diamond's legacy custom-TLK/HAK resource block in
//!   this packet.
//! - EE `CNWCModule::LoadModule` mirrors that order and reads the field after
//!   the module byte with `ReadCResRef(..., 0x10)`. Feeding the custom TLK there
//!   shifts the rest of the cursor and returns the `0x1AFCC` overflow/underflow
//!   failure seen by the harness.
//! - Diamond/1.69 module-info streams observed from HG place
//!   `custom_tlk_string + optional legacy NWM-resource CExoString + hak_count +
//!   hak[16]... + module_resref[16]` before the area table. The Diamond client
//!   decompile reads that optional NWM-resource string with a 0x20 bound before
//!   the HAK count; HG sends it as an empty string. EE has no `Module_Info`
//!   reader slot for it, so the correct rewrite removes it with the custom-TLK
//!   and HAK bytes, then moves the legacy module resref into the EE module-resref
//!   slot.
//! - The custom TLK is preserved only in the rewrite summary. EE consumes that
//!   value later from `CNWCModule::LoadModuleResources`, not from `Module_Info`.

use std::sync::{OnceLock, RwLock};

const MAX_MODULE_INFO_STRING: usize = 4096;
const MAX_LEGACY_NWM_RESOURCE_STRING: usize = 32;
const MAX_LEGACY_HAK_BLOCK_LOOKAHEAD: usize = 128;
const MAX_LEGACY_HAK_COUNT: usize = 64;
// EE writes the module area count here, then one OBJECTID + CExoString area
// name per entry. This is not an arbitrary resource-table count, so keep it
// tight enough to reject accidental 32-bit values found while scanning.
const MAX_MODULE_RESOURCE_COUNT: u32 = 4096;
const MAX_AREA_NAME_LENGTH: usize = 512;
const ZERO_NAME_TERMINATOR_MIN_ENTRIES: u32 = 32;
const RESREF_BYTES: usize = 16;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const MAX_COMPACT_DECLARED_ZERO_BYTES: usize = 512;
const MAX_COMPACT_DECLARED_ZERO_AREA_COUNT: u32 = 64;
const COMPACT_DECLARED_ZERO_FRAGMENT_BYTES: usize = 1;
const MODULE_MAJOR: u8 = 0x03;
const MODULE_END_GAME_MINOR: u8 = 0x0E;
const MODULE_END_GAME_MAX_TEXT_BYTES: usize = 4096;
const MODULE_END_GAME_SHA1_HEX_BYTES: usize = 40;
const FINAL_EMPTY_FRAGMENT_BYTE: u8 = 0x60;

static OBSERVED_MODULE_CONTEXT: OnceLock<RwLock<Option<ObservedModuleContext>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct ObservedModuleContext {
    pub(crate) localized_name: String,
    pub(crate) module_resref: String,
    pub(crate) areas: Vec<ObservedModuleArea>,
}

#[derive(Debug, Clone)]
pub(crate) struct ObservedModuleArea {
    pub(crate) object_id: u32,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub struct RewriteSummary {
    pub offset: usize,
    pub hak_count: u8,
    pub hak_order_top_first: Vec<String>,
    pub module_resref: Option<String>,
    pub custom_tlk: Option<String>,
    pub custom_tlk_converted_to_resref: bool,
    pub removed_hak_bytes: usize,
    pub legacy_tail_removed: usize,
    pub old_declared: u32,
    pub new_declared: u32,
    pub resource_count: u32,
    pub resource_name_count: u32,
    pub zero_length_name_repairs: u32,
    pub zero_length_name_terminator: bool,
    pub compact_legacy_no_resource: bool,
}

#[derive(Debug, Clone)]
struct ModuleInfoView {
    declared: u32,
    hak_search_start: usize,
    custom_tlk: Option<CustomTlkField>,
}

#[derive(Debug, Clone)]
struct LegacyHakBlock {
    offset: usize,
    hak_count: u8,
    skipped_bytes: usize,
    module_resref_start: usize,
    area_count_offset: usize,
    resource_count: u32,
    hak_order_top_first: Vec<String>,
    module_resref: Option<String>,
}

#[derive(Debug, Clone)]
struct ModuleInfoPrefix {
    cursor: usize,
    custom_tlk: Option<CustomTlkField>,
}

#[derive(Debug, Clone)]
struct CustomTlkField {
    value: Option<String>,
    start: usize,
    end: usize,
    legacy_string: bool,
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

pub fn module_end_game_shape_valid(payload: &[u8]) -> bool {
    // Decompile anchor: `CNWSMessage::SendServerToPlayerModule_EndGame`
    // creates a CNW write message, writes the end-game `CExoString`, and for
    // clients satisfying build `0x2001/0x1C` appends a second `CExoString`
    // containing the module SHA1 hex string. Both Diamond and EE use the same
    // read-buffer order, so this is an identity translator with an exact cursor
    // proof rather than a byte patch.
    if !is_high_level_envelope(payload.first().copied().unwrap_or_default())
        || payload.get(1) != Some(&MODULE_MAJOR)
        || payload.get(2) != Some(&MODULE_END_GAME_MINOR)
    {
        return false;
    }

    let Some(declared) =
        read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };

    if declared <= HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload.len() != declared + 1
        || payload[declared] != FINAL_EMPTY_FRAGMENT_BYTE
    {
        return false;
    }

    let mut cursor = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    let Some((next, _message_len)) =
        read_c_exo_string_shape(payload, cursor, declared, MODULE_END_GAME_MAX_TEXT_BYTES)
    else {
        return false;
    };
    cursor = next;
    if cursor == declared {
        return true;
    }

    let Some((next, hash_len)) =
        read_c_exo_string_shape(payload, cursor, declared, MODULE_END_GAME_SHA1_HEX_BYTES)
    else {
        return false;
    };
    next == declared
        && (hash_len == 0
            || (hash_len == MODULE_END_GAME_SHA1_HEX_BYTES
                && payload[cursor + 4..next].iter().copied().all(is_ascii_hex)))
}

fn rewrite_module_info_payload_at_zero(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    if let Some(summary) = rewrite_legacy_hak_module_info_payload_at_zero(payload) {
        return Some(summary);
    }

    rewrite_compact_legacy_no_resource_module_info_payload_at_zero(payload)
}

fn rewrite_legacy_hak_module_info_payload_at_zero(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    let view = parse_module_info(payload)?;
    let hak_block = find_legacy_hak_block(payload, &view)?;
    let custom_tlk = view.custom_tlk.as_ref()?;

    if hak_block.skipped_bytes == 0
        || hak_block.skipped_bytes >= view.declared as usize
        || hak_block.offset + hak_block.skipped_bytes > view.declared as usize
        || hak_block.offset + hak_block.skipped_bytes > payload.len()
        || !custom_tlk.legacy_string
    {
        return None;
    }
    parse_legacy_nwm_resource_preamble(payload, custom_tlk.end, hak_block.offset)?;

    let old_declared = view.declared;
    let replacement_start = custom_tlk.start;
    let replacement_end = hak_block.area_count_offset;
    let module_resref = payload
        .get(hak_block.module_resref_start..hak_block.module_resref_start + RESREF_BYTES)?
        .to_vec();
    if module_resref.len() != RESREF_BYTES {
        return None;
    }

    let replaced_len = replacement_end.checked_sub(replacement_start)?;
    if replaced_len <= RESREF_BYTES || replacement_end > payload.len() {
        return None;
    }
    payload.splice(replacement_start..replacement_end, module_resref);

    let removed_resource_bytes = replaced_len.checked_sub(RESREF_BYTES)?;
    let new_declared = old_declared.checked_sub(u32::try_from(removed_resource_bytes).ok()?)?;
    write_le_u32(payload, 3, new_declared)?;

    let area_count_offset = replacement_start.checked_add(RESREF_BYTES)?;
    let table_rewrite = rewrite_load_module_resource_name_table_tail(payload, area_count_offset);
    let final_declared = read_le_u32(payload, 3)?;

    Some(RewriteSummary {
        offset: 0,
        hak_count: hak_block.hak_count,
        hak_order_top_first: hak_block.hak_order_top_first,
        module_resref: hak_block.module_resref,
        custom_tlk: view.custom_tlk.and_then(|field| field.value),
        custom_tlk_converted_to_resref: false,
        removed_hak_bytes: removed_resource_bytes,
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
        compact_legacy_no_resource: false,
    })
}

#[derive(Debug, Clone)]
struct CompactLegacyModuleInfo {
    old_declared: u32,
    module_name: Option<String>,
    localized_name: String,
    module_byte: u8,
    module_resref: String,
    areas: Vec<CompactLegacyArea>,
    official_campaign: u8,
    fragment_tail: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CompactLegacyArea {
    object_id: u32,
    name: String,
}

#[derive(Debug, Clone)]
struct PrintableRun {
    start: usize,
    end: usize,
    value: String,
}

fn rewrite_compact_legacy_no_resource_module_info_payload_at_zero(
    payload: &mut Vec<u8>,
) -> Option<RewriteSummary> {
    let compact = parse_compact_legacy_no_resource_module_info(payload)?;
    let old_declared = compact.old_declared;
    let old_payload_length = payload.len();

    let mut rewritten = Vec::with_capacity(payload.len().saturating_add(64));
    rewritten.extend_from_slice(&[payload[0], 0x03, 0x01, 0, 0, 0, 0]);
    write_string(&mut rewritten, compact.module_name.as_deref().unwrap_or(""));
    write_string(&mut rewritten, &compact.localized_name);
    rewritten.push(compact.module_byte);
    write_resref16(&mut rewritten, &compact.module_resref)?;
    rewritten.extend_from_slice(&(compact.areas.len() as u32).to_le_bytes());
    for area in &compact.areas {
        rewritten.extend_from_slice(&area.object_id.to_le_bytes());
        write_string(&mut rewritten, &area.name);
    }
    rewritten.push(compact.official_campaign);

    let new_declared = u32::try_from(rewritten.len()).ok()?;
    rewritten[3..7].copy_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&compact.fragment_tail);

    tracing::info!(
        old_declared,
        new_declared,
        old_payload_length,
        new_payload_length = rewritten.len(),
        module_resref = compact.module_resref.as_str(),
        area_count = compact.areas.len(),
        "server Module_Info compact Diamond no-resource shape rewritten to EE read-window layout"
    );

    remember_compact_module_context(&compact);
    *payload = rewritten;

    Some(RewriteSummary {
        offset: 0,
        hak_count: 0,
        hak_order_top_first: Vec::new(),
        module_resref: Some(compact.module_resref),
        custom_tlk: None,
        custom_tlk_converted_to_resref: false,
        removed_hak_bytes: 0,
        legacy_tail_removed: old_declared.saturating_sub(new_declared) as usize,
        old_declared,
        new_declared,
        resource_count: compact.areas.len() as u32,
        resource_name_count: compact.areas.len() as u32,
        zero_length_name_repairs: 0,
        zero_length_name_terminator: false,
        compact_legacy_no_resource: true,
    })
}

pub(crate) fn observed_module_context() -> Option<ObservedModuleContext> {
    let lock = OBSERVED_MODULE_CONTEXT.get()?;
    let guard = lock.read().ok()?;
    guard.clone()
}

#[cfg(all(test, hgbridge_private_fixtures))]
pub(crate) fn remember_observed_module_context_for_tests(context: ObservedModuleContext) {
    let lock = OBSERVED_MODULE_CONTEXT.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = Some(context);
    }
}

fn remember_compact_module_context(compact: &CompactLegacyModuleInfo) {
    let context = ObservedModuleContext {
        localized_name: compact.localized_name.clone(),
        module_resref: compact.module_resref.clone(),
        areas: compact
            .areas
            .iter()
            .map(|area| ObservedModuleArea {
                object_id: area.object_id,
                name: area.name.clone(),
            })
            .collect(),
    };
    let area_count = context.areas.len();
    let lock = OBSERVED_MODULE_CONTEXT.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = Some(context.clone());
    }
    tracing::debug!(
        module_name = context.localized_name.as_str(),
        module_resref = context.module_resref.as_str(),
        area_count,
        "observed compact Module_Info context for later resource-backed packet translation"
    );
}

fn parse_compact_legacy_no_resource_module_info(payload: &[u8]) -> Option<CompactLegacyModuleInfo> {
    if payload.len() < 32 || !is_high_level_envelope(*payload.first()?) {
        return None;
    }
    if payload.get(1).copied()? != 0x03 || payload.get(2).copied()? != 0x01 {
        return None;
    }

    let old_declared = read_le_u32(payload, 3)?;
    if old_declared == 0 {
        return parse_compact_legacy_declared_zero_module_info(payload);
    }

    let declared = usize::try_from(old_declared).ok()?;
    if declared < 7 || declared >= payload.len() || payload.len().saturating_sub(declared) > 8 {
        return None;
    }

    // Do not compete with the decompile-proven legacy HAK/custom-TLK path. The
    // compact path exists for Diamond modules that declare no legacy resource
    // stack: Diamond still emits the same high-level family, but the locstring
    // discriminator and no-resource fields leave the read window in a shape EE
    // will not accept as a raw `CNWSModule::PackModuleIntoMessage` stream.
    if parse_module_info(payload).is_some() {
        return None;
    }

    let mut cursor = 7;
    let module_name = read_raw_string_bounded_value(payload, &mut cursor, declared)?;
    if cursor >= declared {
        return None;
    }

    let module_resref_start = find_compact_sanitized_module_resref(payload, cursor, declared)?;
    let module_resref = fixed_resref16_to_string(payload, module_resref_start)?;
    let localized_name = module_resref.clone();
    let module_byte = 0;
    let official_campaign = 0;
    let fragment_tail = payload.get(declared..)?.to_vec();
    if !compact_fragment_tail_has_module_info_bits(&fragment_tail) {
        return None;
    }

    let area_runs = compact_area_name_runs(payload, module_resref_start + RESREF_BYTES, declared);
    if area_runs.is_empty() || area_runs.len() > MAX_MODULE_RESOURCE_COUNT as usize {
        return None;
    }

    let mut areas = Vec::with_capacity(area_runs.len());
    for (index, run) in area_runs.into_iter().enumerate() {
        let object_id = compact_area_object_id_near(payload, run.start)
            .unwrap_or(0x8000_0000u32.saturating_add(index as u32));
        if (object_id & 0x8000_0000) == 0 || object_id == 0xffff_ffff {
            return None;
        }
        areas.push(CompactLegacyArea {
            object_id,
            name: run.value,
        });
    }

    Some(CompactLegacyModuleInfo {
        old_declared,
        module_name,
        localized_name,
        module_byte,
        module_resref,
        areas,
        official_campaign,
        fragment_tail,
    })
}

fn parse_compact_legacy_declared_zero_module_info(
    payload: &[u8],
) -> Option<CompactLegacyModuleInfo> {
    if payload.len() < 48 || payload.len() > MAX_COMPACT_DECLARED_ZERO_BYTES {
        return None;
    }

    // Local Diamond no-resource modules can emit a compact `Module_Info` stream
    // with a zero declared read-window and one final fragment byte. The observed
    // 1.69 bytes still carry the decompile-owned module metadata in order:
    // leading empty module CExoString storage, compact locstring text fragments,
    // area count, then one OBJECTID-framed area-name fragment group per area.
    // EE has no zero-declared `CNWCModule::LoadModule` branch, so rebuild the
    // same semantic fields into the EE read-window layout and let strict
    // validation prove the result before dispatch can claim it.
    let fragment_start = payload
        .len()
        .checked_sub(COMPACT_DECLARED_ZERO_FRAGMENT_BYTES)?;
    let fragment_tail = payload.get(fragment_start..)?.to_vec();
    if !compact_fragment_tail_has_module_info_bits(&fragment_tail) {
        return None;
    }

    let (area_count_offset, areas) = compact_declared_zero_area_table(
        payload,
        HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES,
        fragment_start,
    )?;
    let localized_name = compact_declared_zero_module_name(
        payload,
        HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES,
        area_count_offset,
    )?;
    let module_resref = compact_resref_from_observed_text(&localized_name)?;

    Some(CompactLegacyModuleInfo {
        old_declared: 0,
        module_name: None,
        localized_name,
        module_byte: 0,
        module_resref,
        areas,
        official_campaign: 0,
        fragment_tail,
    })
}

fn compact_declared_zero_area_table(
    payload: &[u8],
    search_start: usize,
    declared_end: usize,
) -> Option<(usize, Vec<CompactLegacyArea>)> {
    if search_start >= declared_end || declared_end > payload.len() {
        return None;
    }

    let last_count_offset = declared_end.checked_sub(8)?;
    for count_offset in search_start..=last_count_offset {
        let count = read_le_u32(payload, count_offset)?;
        if count == 0 || count > MAX_COMPACT_DECLARED_ZERO_AREA_COUNT {
            continue;
        }
        let table_start = count_offset.checked_add(CNW_LENGTH_BYTES)?;
        let Some(areas) =
            compact_declared_zero_areas_at(payload, table_start, declared_end, count as usize)
        else {
            continue;
        };
        return Some((count_offset, areas));
    }

    compact_declared_zero_countless_area_table(payload, search_start, declared_end)
}

fn compact_declared_zero_areas_at(
    payload: &[u8],
    table_start: usize,
    declared_end: usize,
    area_count: usize,
) -> Option<Vec<CompactLegacyArea>> {
    if area_count == 0
        || area_count > MAX_COMPACT_DECLARED_ZERO_AREA_COUNT as usize
        || table_start + 4 > declared_end
        || declared_end > payload.len()
    {
        return None;
    }

    if !is_likely_area_resource_id(read_le_u32(payload, table_start)?) {
        return None;
    }

    let mut object_offsets = Vec::with_capacity(area_count);
    let mut cursor = table_start;
    while cursor + 4 <= declared_end {
        let raw = read_le_u32(payload, cursor)?;
        if is_likely_area_resource_id(raw) {
            object_offsets.push(cursor);
            cursor += 4;
            if object_offsets.len() == area_count {
                break;
            }
            continue;
        }
        cursor += 1;
    }

    if object_offsets.len() != area_count {
        return None;
    }
    if cursor + 4 <= declared_end {
        for offset in cursor..=declared_end - 4 {
            if is_likely_area_resource_id(read_le_u32(payload, offset)?) {
                return None;
            }
        }
    }

    let mut areas = Vec::with_capacity(area_count);
    for (index, object_offset) in object_offsets.iter().copied().enumerate() {
        let object_id = read_le_u32(payload, object_offset)?;
        let name_start = object_offset.checked_add(4)?;
        let name_end = object_offsets
            .get(index + 1)
            .copied()
            .unwrap_or(declared_end);
        if name_start >= name_end {
            return None;
        }
        let name = compact_declared_zero_fragment_text(payload, name_start, name_end)?;
        if name.len() > MAX_AREA_NAME_LENGTH {
            return None;
        }
        areas.push(CompactLegacyArea { object_id, name });
    }

    Some(areas)
}

fn compact_declared_zero_countless_area_table(
    payload: &[u8],
    search_start: usize,
    declared_end: usize,
) -> Option<(usize, Vec<CompactLegacyArea>)> {
    if search_start >= declared_end || declared_end > payload.len() {
        return None;
    }

    let mut object_offsets = Vec::new();
    let mut cursor = search_start;
    while cursor + 4 <= declared_end {
        let raw = read_le_u32(payload, cursor)?;
        if is_likely_area_resource_id(raw) {
            object_offsets.push(cursor);
            cursor += 4;
        } else {
            cursor += 1;
        }
    }

    if object_offsets.len() < 2
        || object_offsets.len() > MAX_COMPACT_DECLARED_ZERO_AREA_COUNT as usize
    {
        return None;
    }

    let module_prefix_has_text =
        compact_printable_runs(payload, search_start, object_offsets[0], 2)
            .iter()
            .any(|run| run.value.chars().any(|ch| ch.is_ascii_alphabetic()));
    if !module_prefix_has_text {
        return None;
    }

    let mut areas = Vec::with_capacity(object_offsets.len());
    for (index, object_offset) in object_offsets.iter().copied().enumerate() {
        let object_id = read_le_u32(payload, object_offset)?;
        let name_start = object_offset.checked_add(4)?;
        let name_end = object_offsets
            .get(index + 1)
            .copied()
            .unwrap_or(declared_end);
        if name_start >= name_end {
            return None;
        }
        let name = compact_declared_zero_fragment_text(payload, name_start, name_end)?;
        if name.len() > MAX_AREA_NAME_LENGTH {
            return None;
        }
        areas.push(CompactLegacyArea { object_id, name });
    }

    Some((object_offsets[0], areas))
}

fn compact_declared_zero_module_name(payload: &[u8], start: usize, end: usize) -> Option<String> {
    compact_printable_runs(payload, start, end, 2)
        .into_iter()
        .find(|run| run.value.chars().any(|ch| ch.is_ascii_alphabetic()))
        .map(|run| run.value)
}

fn compact_declared_zero_fragment_text(payload: &[u8], start: usize, end: usize) -> Option<String> {
    let runs = compact_printable_runs(payload, start, end, 1);
    if runs.is_empty() {
        return None;
    }

    let mut value = String::new();
    for run in runs {
        let fragment = run.value.trim();
        if fragment.is_empty() {
            continue;
        }
        if !value.is_empty() {
            value.push(' ');
        }
        value.push_str(fragment);
    }

    if value.is_empty() || !value.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(value)
}

fn compact_resref_from_observed_text(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut last_was_separator = false;
    for byte in value.bytes() {
        if is_resref_char(byte) {
            out.push(char::from(byte));
            last_was_separator = false;
        } else if byte.is_ascii_whitespace() && !out.is_empty() && !last_was_separator {
            out.push('_');
            last_was_separator = true;
        }
        if out.len() == RESREF_BYTES {
            break;
        }
    }

    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() || !out.as_bytes().iter().any(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    Some(out)
}

fn find_compact_sanitized_module_resref(
    payload: &[u8],
    start: usize,
    declared: usize,
) -> Option<usize> {
    if start >= declared || declared > payload.len() {
        return None;
    }

    let mut best = None;
    let mut best_len = 0usize;
    let end = declared.saturating_sub(RESREF_BYTES);
    for offset in start..=end {
        if let Some(length) = sanitized_fixed_resref16_len(payload, offset, false) {
            if length > best_len {
                best_len = length;
                best = Some(offset);
            }
        } else if sanitized_fixed_resref16(payload, offset, false) {
            best = Some(offset);
        }
    }
    best
}

fn sanitized_fixed_resref16(payload: &[u8], offset: usize, allow_empty: bool) -> bool {
    sanitized_fixed_resref16_len(payload, offset, allow_empty).is_some()
}

fn sanitized_fixed_resref16_len(payload: &[u8], offset: usize, allow_empty: bool) -> Option<usize> {
    let Some(bytes) = payload.get(offset..offset + RESREF_BYTES) else {
        return None;
    };
    let Some(value) = fixed_resref16_value(payload, offset, allow_empty) else {
        return None;
    };
    if value.is_none() {
        return allow_empty.then_some(0);
    }
    let meaningful_len = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(RESREF_BYTES);
    bytes[meaningful_len..]
        .iter()
        .all(|byte| *byte == 0)
        .then_some(meaningful_len)
}

fn compact_fragment_tail_has_module_info_bits(fragment: &[u8]) -> bool {
    const CNW_FRAGMENT_HEADER_BITS: usize = 3;
    const LEGACY_LOCSTRING_AND_EE_TAIL_BITS: usize = 3;

    let Some(first) = fragment.first().copied() else {
        return false;
    };
    let final_bits = usize::from((first & 0xE0) >> 5);
    let valid_bits = if final_bits == 0 {
        fragment.len().saturating_mul(8)
    } else {
        fragment
            .len()
            .saturating_sub(1)
            .saturating_mul(8)
            .saturating_add(final_bits)
    };
    valid_bits >= CNW_FRAGMENT_HEADER_BITS + LEGACY_LOCSTRING_AND_EE_TAIL_BITS
}

fn compact_area_name_runs(payload: &[u8], start: usize, declared: usize) -> Vec<PrintableRun> {
    compact_printable_runs(payload, start, declared, 4)
}

fn compact_printable_runs(
    payload: &[u8],
    start: usize,
    declared: usize,
    min_len: usize,
) -> Vec<PrintableRun> {
    let mut runs = Vec::new();
    if start >= declared || declared > payload.len() {
        return runs;
    }

    let mut cursor = start;
    while cursor < declared {
        while cursor < declared && !compact_area_name_byte(payload[cursor]) {
            cursor += 1;
        }
        let run_start = cursor;
        while cursor < declared && compact_area_name_byte(payload[cursor]) {
            cursor += 1;
        }
        let run_end = cursor;
        if run_end.saturating_sub(run_start) >= min_len {
            let value = String::from_utf8_lossy(&payload[run_start..run_end])
                .trim()
                .to_string();
            if value.len() >= min_len && value.chars().any(|ch| ch.is_ascii_alphabetic()) {
                runs.push(PrintableRun {
                    start: run_start,
                    end: run_end,
                    value,
                });
            }
        }
    }
    runs
}

fn compact_area_name_byte(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\r' | b'\n' | 0x20..=0x7e)
}

fn compact_area_object_id_near(payload: &[u8], name_start: usize) -> Option<u32> {
    let window_start = name_start.saturating_sub(16);
    let window_end = name_start.saturating_sub(4);
    let mut best = None;
    for offset in window_start..=window_end {
        let Some(raw) = read_le_u32(payload, offset) else {
            continue;
        };
        if raw == 0 || raw == 0xffff_ffff {
            continue;
        }
        let normalized = raw | 0x8000_0000;
        if normalized == 0xffff_ffff {
            continue;
        }
        if (raw & 0x8000_0000) != 0 {
            return Some(raw);
        }
        if best.is_none()
            && payload
                .get(offset..offset + 4)
                .is_some_and(|bytes| bytes.iter().any(|byte| *byte == 0x80))
        {
            best = Some(normalized);
        }
    }
    best
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
        hak_search_start: hak_search_start.cursor,
        custom_tlk: hak_search_start.custom_tlk,
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

fn module_info_hak_search_start(payload: &[u8], declared: usize) -> Option<ModuleInfoPrefix> {
    let prefix = parse_legacy_string_module_info_prefix(payload, declared)?;
    valid_legacy_hak_block_near(payload, prefix.cursor, declared).map(|_| prefix)
}

fn parse_legacy_string_module_info_prefix(
    payload: &[u8],
    declared: usize,
) -> Option<ModuleInfoPrefix> {
    let mut cursor = 7;
    read_raw_string_bounded(payload, &mut cursor, declared)?;
    read_raw_string_bounded(payload, &mut cursor, declared)?;
    skip_exact(payload, &mut cursor, 1, declared)?;
    let custom_tlk_start = cursor;
    let value = read_raw_string_bounded_value(payload, &mut cursor, declared)?;
    Some(ModuleInfoPrefix {
        cursor,
        custom_tlk: Some(CustomTlkField {
            value,
            start: custom_tlk_start,
            end: cursor,
            legacy_string: true,
        }),
    })
}

fn parse_legacy_nwm_resource_preamble(
    payload: &[u8],
    offset: usize,
    hak_count_offset: usize,
) -> Option<Option<String>> {
    if offset == hak_count_offset {
        return Some(None);
    }
    if offset > hak_count_offset || hak_count_offset > payload.len() {
        return None;
    }

    let mut cursor = offset;
    let value = read_raw_string_bounded_value(payload, &mut cursor, hak_count_offset)?;
    if cursor == hak_count_offset
        && value.as_deref().map(str::len).unwrap_or(0) <= MAX_LEGACY_NWM_RESOURCE_STRING
    {
        Some(value)
    } else {
        None
    }
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

fn valid_legacy_hak_block_at(
    payload: &[u8],
    offset: usize,
    declared: usize,
) -> Option<LegacyHakBlock> {
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
    let module_resref = fixed_resref16_to_string(payload, module_start);
    let area_count_offset = offset + skipped_bytes;

    valid_area_name_table_prefix(payload, area_count_offset + 4, resource_count, declared)?;

    let mut hak_order_top_first = Vec::with_capacity(count);
    for index in 0..count {
        let start = offset + 1 + index * RESREF_BYTES;
        hak_order_top_first.push(fixed_resref16_to_string(payload, start)?);
    }

    Some(LegacyHakBlock {
        offset,
        hak_count: count as u8,
        skipped_bytes,
        module_resref_start: module_start,
        area_count_offset,
        resource_count,
        hak_order_top_first,
        module_resref,
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
    read_raw_string_bounded_value(payload, cursor, bound).map(|_| ())
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn write_resref16(out: &mut Vec<u8>, value: &str) -> Option<()> {
    if value.len() > RESREF_BYTES || !value.as_bytes().iter().copied().all(is_resref_char) {
        return None;
    }
    let mut bytes = [0u8; RESREF_BYTES];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    out.extend_from_slice(&bytes);
    Some(())
}

fn read_raw_string_bounded_value(
    payload: &[u8],
    cursor: &mut usize,
    bound: usize,
) -> Option<Option<String>> {
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
    let value = if length == 0 {
        None
    } else {
        Some(String::from_utf8_lossy(&payload[*cursor..*cursor + length]).to_string())
    };
    *cursor += length;
    Some(value)
}

fn read_c_exo_string_shape(
    payload: &[u8],
    cursor: usize,
    declared: usize,
    max_len: usize,
) -> Option<(usize, usize)> {
    if cursor > declared || declared > payload.len() || declared.saturating_sub(cursor) < 4 {
        return None;
    }
    let length = usize::try_from(read_le_u32(payload, cursor)?).ok()?;
    if length > max_len || length > declared.saturating_sub(cursor + 4) {
        return None;
    }
    Some((cursor + 4 + length, length))
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

fn fixed_resref16_to_string(payload: &[u8], offset: usize) -> Option<String> {
    fixed_resref16_value(payload, offset, false).flatten()
}

fn fixed_resref16_value(
    payload: &[u8],
    offset: usize,
    allow_empty: bool,
) -> Option<Option<String>> {
    let bytes = payload.get(offset..offset + RESREF_BYTES)?;
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(RESREF_BYTES);
    if length == 0 {
        return allow_empty.then_some(None);
    }
    if !bytes[..length].iter().copied().all(is_resref_char) {
        return None;
    }
    Some(Some(String::from_utf8_lossy(&bytes[..length]).to_string()))
}

fn is_resref_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn is_ascii_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_legacy_custom_tlk_string_before_hak_block() {
        let mut payload = legacy_module_info_payload(
            "Path of Ascension CEP Legends",
            "Path of Ascension CEP Legends",
            0x02,
            "cep23_v1",
            &["cep2_top_v23", "cep2_custom"],
            "poa_mod",
            &[(0x8000_0001, "Armor Shop")],
        );

        let summary = rewrite_module_info_payload(&mut payload)
            .expect("legacy Module_Info should be claimed and rewritten");

        assert_eq!(summary.custom_tlk.as_deref(), Some("cep23_v1"));
        assert!(!summary.custom_tlk_converted_to_resref);
        assert_eq!(summary.hak_count, 2);
        assert_eq!(
            summary.hak_order_top_first,
            vec!["cep2_top_v23".to_string(), "cep2_custom".to_string()]
        );
        assert_eq!(summary.module_resref.as_deref(), Some("poa_mod"));
        assert_eq!(summary.resource_count, 1);
        assert_eq!(summary.resource_name_count, 1);
        assert!(summary.removed_hak_bytes > 0);

        let mut cursor = 7;
        assert_eq!(
            read_raw_string_bounded_value(&payload, &mut cursor, payload.len())
                .unwrap()
                .as_deref(),
            Some("Path of Ascension CEP Legends")
        );
        assert_eq!(
            read_raw_string_bounded_value(&payload, &mut cursor, payload.len())
                .unwrap()
                .as_deref(),
            Some("Path of Ascension CEP Legends")
        );
        assert_eq!(payload[cursor], 0x02);
        cursor += 1;
        assert_eq!(
            fixed_resref16_to_string(&payload, cursor).as_deref(),
            Some("poa_mod")
        );
        cursor += RESREF_BYTES;
        assert_eq!(read_le_u32(&payload, cursor), Some(1));
    }

    #[test]
    fn claims_decompile_backed_module_end_game_single_string_shape() {
        let payload = module_end_game_payload(&["The End"]);

        assert!(module_end_game_shape_valid(&payload));
    }

    #[test]
    fn claims_decompile_backed_module_end_game_with_sha1_shape() {
        let payload =
            module_end_game_payload(&["The End", "0123456789abcdef0123456789ABCDEF01234567"]);

        assert!(module_end_game_shape_valid(&payload));
    }

    #[test]
    fn rejects_module_end_game_with_unverified_second_string_shape() {
        let payload = module_end_game_payload(&["The End", "not-a-sha1"]);

        assert!(!module_end_game_shape_valid(&payload));
    }

    #[test]
    fn parses_legacy_custom_tlk_as_string_not_module_resref() {
        let payload = legacy_module_info_payload(
            "Path of Ascension CEP Legends",
            "Path of Ascension CEP Legends",
            0x02,
            "cep23_v1",
            &["cep2_top_v23"],
            "poa_mod",
            &[(0x8000_0001, "Armor Shop")],
        );
        let declared = read_le_u32(&payload, 3).unwrap() as usize;

        let mut cursor = 7;
        read_raw_string_bounded(&payload, &mut cursor, declared).unwrap();
        read_raw_string_bounded(&payload, &mut cursor, declared).unwrap();
        skip_exact(&payload, &mut cursor, 1, declared).unwrap();
        assert_eq!(payload[cursor], 8);
        let prefix = parse_legacy_string_module_info_prefix(&payload, declared).unwrap();
        let field = prefix.custom_tlk.unwrap();
        assert_eq!(field.value.as_deref(), Some("cep23_v1"));
        assert!(field.legacy_string);
    }

    #[test]
    fn rewrites_compact_diamond_no_resource_module_info_to_exact_ee_shape() {
        let mut payload = vec![
            0x50, 0x03, 0x01, 0x81, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x62, 0x77, 0x31,
            0x36, 0x37, 0x64, 0x65, 0x6D, 0x6F, 0x00, 0x00, 0x00, 0x00, 0x62, 0x77, 0x31, 0x36,
            0x37, 0x64, 0x65, 0x6D, 0x6F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00,
            0x00, 0x53, 0x75, 0x6E, 0x73, 0x68, 0x69, 0x6E, 0x65, 0x20, 0x56, 0x69, 0x6C, 0x6C,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x54, 0x00, 0x00,
            0x00, 0x4D, 0x61, 0x67, 0x69, 0x63, 0x20, 0x54, 0x00, 0x00, 0x00, 0x74, 0x00, 0x00,
            0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x52, 0x00, 0x00, 0x00, 0x00, 0x20, 0x48, 0x6F,
            0x6D, 0x65, 0x00, 0xC0,
        ];

        let summary = rewrite_module_info_payload(&mut payload)
            .expect("compact Diamond no-resource Module_Info should be rewritten");

        assert!(summary.compact_legacy_no_resource);
        assert_eq!(summary.module_resref.as_deref(), Some("bw167demo"));
        assert_eq!(summary.hak_count, 0);
        assert_eq!(summary.resource_name_count, 3);
        assert!(crate::strict::module_info_shape_valid(&payload));
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn rewrites_declared_zero_compact_module_info_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../fixtures/module_info/local_diamond_to_heir_declared_zero_module_info_20260519.bin"
        )
        .to_vec();

        let summary = rewrite_module_info_payload(&mut payload)
            .expect("declared-zero compact Module_Info should be rewritten");

        assert!(summary.compact_legacy_no_resource);
        assert_eq!(summary.old_declared, 0);
        assert_eq!(summary.hak_count, 0);
        assert_eq!(summary.module_resref.as_deref(), Some("To_H"));
        assert_eq!(summary.resource_name_count, 5);
        assert!(crate::strict::module_info_shape_valid(&payload));
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn rewrites_declared_zero_countless_compact_module_info_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../fixtures/module_info/local_dark_ranger_declared_zero_countless_module_info_20260519.bin"
        )
        .to_vec();

        let summary = rewrite_module_info_payload(&mut payload)
            .expect("countless declared-zero compact Module_Info should be rewritten");

        assert!(summary.compact_legacy_no_resource);
        assert_eq!(summary.old_declared, 0);
        assert_eq!(summary.hak_count, 0);
        assert_eq!(summary.module_resref.as_deref(), Some("Dark_R"));
        assert_eq!(summary.resource_name_count, 3);
        assert!(crate::strict::module_info_shape_valid(&payload));
    }

    fn legacy_module_info_payload(
        name: &str,
        description: &str,
        flag: u8,
        custom_tlk: &str,
        haks: &[&str],
        module_resref: &str,
        areas: &[(u32, &str)],
    ) -> Vec<u8> {
        let mut payload = vec![b'P', 0x03, 0x01, 0, 0, 0, 0];
        write_string(&mut payload, name);
        write_string(&mut payload, description);
        payload.push(flag);
        write_string(&mut payload, custom_tlk);
        write_string(&mut payload, "");
        payload.push(haks.len() as u8);
        for hak in haks {
            write_resref16(&mut payload, hak);
        }
        write_resref16(&mut payload, module_resref);
        payload.extend_from_slice(&(areas.len() as u32).to_le_bytes());
        for (area_id, area_name) in areas {
            payload.extend_from_slice(&area_id.to_le_bytes());
            write_string(&mut payload, area_name);
        }
        payload.push(0);
        let declared = (payload.len() - 1) as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload
    }

    fn module_end_game_payload(strings: &[&str]) -> Vec<u8> {
        let mut payload = vec![b'P', MODULE_MAJOR, MODULE_END_GAME_MINOR, 0, 0, 0, 0];
        for value in strings {
            write_string(&mut payload, value);
        }
        let declared = payload.len() as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload.push(FINAL_EMPTY_FRAGMENT_BYTE);
        payload
    }

    fn write_string(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn write_resref16(out: &mut Vec<u8>, value: &str) {
        assert!(value.len() <= RESREF_BYTES);
        let mut bytes = [0u8; RESREF_BYTES];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        out.extend_from_slice(&bytes);
    }
}
