use super::facade::quickbar_has_plausible_cnw_declared;
use super::*;

// Quickbar transport normalization. This file is allowed to be conservative and
// visibly heuristic; semantic ownership still requires the reader/writer path to
// successfully parse and emit the 36 verified button records.

pub(super) fn normalize_quickbar_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    normalize_quickbar_prefixed_short_declared_payload_if_needed(payload)
        .or_else(|| normalize_quickbar_four_prefixed_fragments_payload_if_needed(payload))
        .or_else(|| normalize_quickbar_scanned_tail_payload_if_needed(payload))
}

pub(super) fn is_quickbar_family(high: HighLevel) -> bool {
    high.major == QUICKBAR_MAJOR && high.minor == SET_ALL_BUTTONS_MINOR
}

fn normalize_quickbar_prefixed_short_declared_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES + 1 {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if !is_quickbar_family(high) || quickbar_has_plausible_cnw_declared(payload) {
        return None;
    }

    let old_wire_declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] = payload
        [HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES]
        .try_into()
        .ok()?;
    let body_and_tail = payload.get(HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES..)?;
    let split = choose_quickbar_split(
        body_and_tail,
        &prefixed_fragment_bytes,
        QuickbarSplitPolicy::DecompileOwnedBoundary,
    )?;

    let mut read_buffer = Vec::with_capacity(split.read_body_len);
    // The four bytes at `P 1E 01 + 3` in this compact legacy/HG shape are not
    // a quickbar-owned read-buffer prefix. They are the displaced CNW fragment
    // bytes occupying the slot where the EE/Diamond receiver expects the CNW
    // declared length. Both decompiled quickbar receivers enter the 36-slot
    // loop immediately after that length field, so the normalized read buffer
    // must start with slot 0's type byte. Re-inserting four zero bytes here
    // shifts every button and turns a proven compact SetAllButtons packet into
    // an unclaimable quarantine.
    read_buffer.extend_from_slice(body_and_tail.get(..split.read_body_len)?);

    let mut fragments =
        Vec::with_capacity(LEGACY_PREFIXED_FRAGMENT_BYTES.checked_add(split.fragment_tail_len)?);
    fragments.extend_from_slice(&prefixed_fragment_bytes);
    fragments.extend_from_slice(body_and_tail.get(split.read_body_len..)?);

    let old_payload_length = payload.len();
    let new_declared = u32::try_from(
        HIGH_LEVEL_HEADER_BYTES
            .checked_add(CNW_LENGTH_BYTES)?
            .checked_add(read_buffer.len())?,
    )
    .ok()?;
    let mut rewritten = Vec::with_capacity(
        HIGH_LEVEL_HEADER_BYTES
            .checked_add(CNW_LENGTH_BYTES)?
            .checked_add(read_buffer.len())?
            .checked_add(fragments.len())?,
    );
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&read_buffer);
    rewritten.extend_from_slice(&fragments);
    let new_payload_length = rewritten.len();
    *payload = rewritten;

    tracing::info!(
        read_body_len = split.read_body_len,
        fragment_tail_len = split.fragment_tail_len,
        translated_item_slots = split.translated_item_slots,
        spell_slots = split.spell_slots,
        general_slots = split.general_slots,
        item_candidate_slots = split.item_candidate_slots,
        unsupported_slots = split.unsupported_slots,
        trailing_read_bytes = split.trailing_read_bytes,
        "server GuiQuickbar_SetAllButtons prefixed-fragment transport normalized after semantic split validation"
    );

    Some(PrefixedFragmentsNormalizeSummary {
        major: high.major,
        minor: high.minor,
        old_wire_declared,
        new_declared,
        old_payload_length,
        new_payload_length,
        prefixed_fragment_bytes,
        read_bytes_offset: HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES,
        read_bytes_length: read_buffer.len(),
    })
}

fn normalize_quickbar_scanned_tail_payload_if_needed(
    _payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    // Historical captures had quickbar bytes coalesced with trailing fragment
    // tails. The split selector above is the only current owner for that repair;
    // leave this as an explicit no-op rather than inventing a raw passthrough.
    None
}

fn normalize_quickbar_four_prefixed_fragments_payload_if_needed(
    _payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    // Kept as its own module seam so future decompile/capture-backed variants
    // can be added without bloating the dispatcher. Today the short-declared
    // path handles the verified four-prefix shape.
    None
}
