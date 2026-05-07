//! `BNXR` NWSync advertisement rewrite.
//!
//! EE probes an optional extended section immediately after the BNXR module
//! name. The legacy server either omits that section or has a different
//! description byte in that slot, so the bridge inserts a known-good NWSync
//! section and preserves the remaining suffix after the consumed probe byte.

use crate::nwsync::Advertisement;

const MODULE_NAME_LENGTH_OFFSET: usize = 19;
const MODULE_NAME_OFFSET: usize = 20;
const EXTENDED_MARKER_OFFSET: usize = 6;
const LENGTH_HINT_OFFSET: usize = 18;
const EXTENDED_MARKER: u8 = 0xFD;
const NWSYNC_SECTION_TAG: u8 = 0x02;

pub(super) fn rewrite_server_to_ee(
    bytes: &[u8],
    advertisement: &Advertisement,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some((module_name, module_end)) = parse_module_name(bytes) else {
        return Ok(None);
    };
    let advert_section = advertisement.build_bnxr_section()?;
    let Some((suffix_start, replaced_existing, consumed_section_tag)) =
        find_suffix_after_advert_slot(bytes, module_end)
    else {
        return Ok(None);
    };

    let mut rewritten = Vec::with_capacity(module_end + advert_section.len() + bytes.len().saturating_sub(suffix_start));
    rewritten.extend_from_slice(&bytes[..module_end]);
    rewritten.extend_from_slice(&advert_section);
    rewritten.extend_from_slice(&bytes[suffix_start..]);

    tracing::info!(
        module = %module_name,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        module_end,
        inserted = advert_section.len(),
        suffix_start,
        suffix_preserved = bytes.len().saturating_sub(suffix_start),
        consumed_section_tag,
        replaced_existing,
        root_hash = %advertisement.root_hash(),
        url = %advertisement.url(),
        manifests = advertisement.manifests().len(),
        "server BNXR NWSync advert rewritten for EE"
    );

    Ok(Some(rewritten))
}

pub(super) fn claim_server_to_ee_if_verified(bytes: &[u8]) -> Option<()> {
    let Some((_module_name, module_end)) = parse_module_name(bytes) else {
        return None;
    };
    if module_end == bytes.len() {
        return Some(());
    }

    // EE probes optional tagged sections immediately after the module name.
    // Only an absent section or an already-valid NWSync section is a true
    // no-op. Legacy HG description-byte suffixes are handled by
    // `rewrite_server_to_ee` when an advertisement is configured; without that
    // rewrite they remain unclaimed so strict mode cannot hide the mismatch.
    if bytes[module_end] != NWSYNC_SECTION_TAG {
        return None;
    }
    (skip_existing_nwsync_advert(bytes, module_end)? == bytes.len()).then_some(())
}

fn parse_module_name(bytes: &[u8]) -> Option<(String, usize)> {
    if bytes.len() < MODULE_NAME_OFFSET
        || !bytes.starts_with(b"BNXR")
        || bytes.get(EXTENDED_MARKER_OFFSET).copied()? != EXTENDED_MARKER
    {
        return None;
    }

    let length_hint_end = MODULE_NAME_OFFSET.checked_add(*bytes.get(LENGTH_HINT_OFFSET)? as usize)?;
    if bytes.len() < length_hint_end {
        return None;
    }

    let name_len = *bytes.get(MODULE_NAME_LENGTH_OFFSET)? as usize;
    let module_end = MODULE_NAME_OFFSET.checked_add(name_len)?;
    if module_end > bytes.len() {
        return None;
    }
    let module_name = String::from_utf8_lossy(&bytes[MODULE_NAME_OFFSET..module_end]).to_string();
    Some((module_name, module_end))
}

fn find_suffix_after_advert_slot(bytes: &[u8], module_end: usize) -> Option<(usize, bool, u8)> {
    if module_end == bytes.len() {
        return Some((module_end, false, 0));
    }

    let consumed_section_tag = bytes[module_end];
    if consumed_section_tag == NWSYNC_SECTION_TAG {
        let suffix_start = skip_existing_nwsync_advert(bytes, module_end)?;
        return Some((suffix_start, true, consumed_section_tag));
    }

    Some((module_end.checked_add(1)?, false, consumed_section_tag))
}

fn skip_existing_nwsync_advert(bytes: &[u8], module_end: usize) -> Option<usize> {
    let mut cursor = module_end.checked_add(1)?;
    let enabled = *bytes.get(cursor)?;
    cursor += 1;
    if enabled == 0 {
        return Some(cursor);
    }

    skip_counted(bytes, &mut cursor)?;
    skip_counted(bytes, &mut cursor)?;
    let manifest_count = *bytes.get(cursor)? as usize;
    cursor += 1;
    for _ in 0..manifest_count {
        cursor = cursor.checked_add(2)?;
        if cursor > bytes.len() {
            return None;
        }
        skip_counted(bytes, &mut cursor)?;
    }
    Some(cursor)
}

fn skip_counted(bytes: &[u8], cursor: &mut usize) -> Option<()> {
    let len = *bytes.get(*cursor)? as usize;
    *cursor += 1;
    *cursor = (*cursor).checked_add(len)?;
    if *cursor > bytes.len() {
        return None;
    }
    Some(())
}
