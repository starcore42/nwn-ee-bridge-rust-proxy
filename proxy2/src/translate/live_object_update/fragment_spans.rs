//! Legacy interleaved CNW fragment-span promotion.
//!
//! Some legacy deflated live-object streams carry a proven read body, then a
//! chunk-local CNW fragment-storage span, then the next live-object submessage.
//! EE wants one normalized `P 05 01` payload with all read-buffer bytes first
//! and the CNW fragment bitstream after the declared read length. These helpers
//! perform only that bounded transport normalization; family parsers still own
//! the exact byte/bit proof.

use super::{bits, boundary, creature, inventory, read_u32_le};

const MAX_INTERLEAVED_FRAGMENT_SPAN_BYTES: usize = 4096;
// HG short-declared creature updates can leave the chunk-local CNW fragment
// storage span in the live read buffer after the decompiled `U/5 0x3967`
// reader has finished. The 2026-05 seq31 `Northern Trader` capture carries a
// 26-byte span before the next real live-object boundary, so keep this bounded
// but large enough for that proven shape. The exact creature cursor simulator
// below must still consume the shortened read buffer before any span is moved.
const MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES: usize = 32;
const LEGACY_CREATURE_UPDATE_3967_MASK: u32 = 0x0000_3967;

#[derive(Debug, Clone, Copy)]
enum CreatureUpdateFragmentSpanCursorPolicy {
    Exact,
    AdjacentRecovery,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PromotedInventoryFragmentSpan {
    pub read_end: usize,
    pub old_record_end: usize,
    pub bytes_promoted: usize,
    pub bits_promoted: usize,
    pub inventory_bits: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PromotedCreatureUpdateFragmentSpan {
    pub read_end: usize,
    pub old_record_end: usize,
    pub bytes_promoted: usize,
    pub bits_promoted: usize,
    pub start_bit_cursor: usize,
    pub end_bit_cursor: usize,
}

pub(super) fn promote_inventory_interleaved_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    offset: usize,
    record_end: &mut usize,
    bit_cursor: usize,
) -> Option<PromotedInventoryFragmentSpan> {
    if offset >= *record_end || *record_end > live_bytes.len() {
        return None;
    }

    let prefix =
        inventory::try_get_legacy_live_inventory_prefix_claim(live_bytes, offset, *record_end)?;
    let available_bits = fragment_bits.len().saturating_sub(bit_cursor);
    if prefix.fragment_bits <= available_bits {
        return None;
    }

    let minimum_span_bytes = prefix
        .fragment_bits
        .saturating_sub(available_bits)
        .saturating_add(7)
        / 8;
    let span_end = find_interleaved_fragment_span_end(
        live_bytes,
        prefix.read_end,
        *record_end,
        minimum_span_bytes,
    )?;
    let span = live_bytes.get(prefix.read_end..span_end)?;
    let span_len = span.len();
    if span.is_empty() || span_len > MAX_INTERLEAVED_FRAGMENT_SPAN_BYTES {
        return None;
    }

    let promoted_bits = unpack_all_msb_bits(span);
    if promoted_bits.len() < prefix.fragment_bits.saturating_sub(available_bits) {
        return None;
    }

    let mut proof_bits = fragment_bits.clone();
    bits::insert_msb_bits(&mut proof_bits, bit_cursor, &promoted_bits)?;
    let mut proof_cursor = bit_cursor;
    inventory::advance_verified_inventory_record(
        live_bytes,
        offset,
        prefix.read_end,
        &proof_bits,
        &mut proof_cursor,
    )?;

    bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;
    let old_record_end = span_end;
    live_bytes.drain(prefix.read_end..old_record_end);
    *record_end = prefix.read_end;

    Some(PromotedInventoryFragmentSpan {
        read_end: prefix.read_end,
        old_record_end,
        bytes_promoted: span_len,
        bits_promoted: promoted_bits.len(),
        inventory_bits: prefix.fragment_bits,
    })
}

pub(super) fn promote_creature_update_interleaved_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    offset: usize,
    record_end: &mut usize,
    bit_cursor: usize,
) -> Option<PromotedCreatureUpdateFragmentSpan> {
    let old_record_end = *record_end;
    let proof = find_creature_update_3967_interleaved_fragment_span(
        live_bytes,
        offset,
        old_record_end,
        fragment_bits,
        bit_cursor,
        CreatureUpdateFragmentSpanCursorPolicy::AdjacentRecovery,
    )?;
    let read_end = proof.read_end;
    let promoted_bits = proof.promoted_bits;
    let insertion_cursor = proof.insertion_cursor;
    bits::insert_msb_bits(fragment_bits, insertion_cursor, &promoted_bits)?;
    live_bytes.drain(read_end..old_record_end);
    *record_end = read_end;

    Some(PromotedCreatureUpdateFragmentSpan {
        read_end,
        old_record_end,
        bytes_promoted: old_record_end.saturating_sub(read_end),
        bits_promoted: promoted_bits.len(),
        start_bit_cursor: proof.start_bit_cursor,
        end_bit_cursor: proof.end_bit_cursor,
    })
}

pub(super) fn verified_creature_update_3967_read_end_before_interleaved_fragment_span(
    live_bytes: &[u8],
    offset: usize,
    old_record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    find_creature_update_3967_interleaved_fragment_span(
        live_bytes,
        offset,
        old_record_end,
        fragment_bits,
        bit_cursor,
        CreatureUpdateFragmentSpanCursorPolicy::Exact,
    )
    .map(|proof| proof.read_end)
}

struct CreatureUpdateFragmentSpanProof {
    read_end: usize,
    promoted_bits: Vec<bool>,
    insertion_cursor: usize,
    start_bit_cursor: usize,
    end_bit_cursor: usize,
}

fn find_creature_update_3967_interleaved_fragment_span(
    live_bytes: &[u8],
    offset: usize,
    old_record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    cursor_policy: CreatureUpdateFragmentSpanCursorPolicy,
) -> Option<CreatureUpdateFragmentSpanProof> {
    if offset.checked_add(10)? >= old_record_end
        || old_record_end > live_bytes.len()
        || live_bytes.get(offset).copied()? != b'U'
        || live_bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(live_bytes, offset + 6)? != LEGACY_CREATURE_UPDATE_3967_MASK
    {
        return None;
    }

    // Decompile/capture-backed short-declared repair for creature `U/5`
    // mask `0x3967`: the reader shape is exact, but some legacy stream windows
    // leave a bounded CNW fragment-storage suffix in the read buffer. Try only
    // suffix splits that decode as CNW fragment storage and accept exactly one
    // split whose shortened read buffer is consumed by the focused creature
    // update simulator.
    let min_read_end = offset.checked_add(10)?;
    let scan_start = old_record_end
        .saturating_sub(MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES)
        .max(min_read_end);
    let mut accepted: Option<CreatureUpdateFragmentSpanProof> = None;
    let mut candidate_bit_cursors = Vec::with_capacity(3);
    candidate_bit_cursors.push(bit_cursor);
    if matches!(
        cursor_policy,
        CreatureUpdateFragmentSpanCursorPolicy::AdjacentRecovery
    ) {
        if bit_cursor != 0 {
            candidate_bit_cursors.push(bit_cursor - 1);
        }
        if bit_cursor < fragment_bits.len() {
            candidate_bit_cursors.push(bit_cursor + 1);
        }
    }
    for read_end in scan_start..old_record_end {
        let span = live_bytes.get(read_end..old_record_end)?;
        if span.is_empty() || span.len() > MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES {
            continue;
        }
        let Some(promoted_bits) = unpack_promoted_fragment_span_payload_bits(span) else {
            continue;
        };
        for candidate_bit_cursor in candidate_bit_cursors.iter().copied() {
            let mut proof_cursor = candidate_bit_cursor;
            let accepted_by_exact_parser =
                creature::advance_verified_noop_creature_update_record_exact_cursor(
                    live_bytes,
                    offset,
                    read_end,
                    fragment_bits,
                    &mut proof_cursor,
                );
            if !accepted_by_exact_parser {
                continue;
            }
            accepted = Some(CreatureUpdateFragmentSpanProof {
                read_end,
                promoted_bits,
                insertion_cursor: proof_cursor,
                start_bit_cursor: candidate_bit_cursor,
                end_bit_cursor: proof_cursor,
            });
            break;
        }
        if accepted.is_some() {
            break;
        }
    }

    accepted
}

fn unpack_promoted_fragment_span_payload_bits(bytes: &[u8]) -> Option<Vec<bool>> {
    let mut bits = bits::decode_msb_valid_bits(bytes, 3)?;
    if bits.len() < 3 {
        return None;
    }
    bits.drain(0..3);
    Some(bits)
}

fn unpack_all_msb_bits(bytes: &[u8]) -> Vec<bool> {
    let mut out = Vec::with_capacity(bytes.len().saturating_mul(8));
    for byte in bytes {
        for bit_index in 0..8 {
            out.push((byte & (0x80 >> bit_index)) != 0);
        }
    }
    out
}

fn find_interleaved_fragment_span_end(
    live_bytes: &[u8],
    span_start: usize,
    search_end: usize,
    minimum_span_bytes: usize,
) -> Option<usize> {
    let scan_start = span_start.checked_add(minimum_span_bytes)?;
    if scan_start >= search_end {
        return None;
    }
    (scan_start..search_end.saturating_sub(1)).find(|candidate| {
        boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, *candidate)
    })
}
