//! Legacy interleaved CNW fragment-span promotion.
//!
//! Some legacy deflated live-object streams carry a proven read body, then a
//! chunk-local CNW fragment-storage span, then the next live-object submessage.
//! EE wants one normalized `P 05 01` payload with all read-buffer bytes first
//! and the CNW fragment bitstream after the declared read length. These helpers
//! perform only that bounded transport normalization; family parsers still own
//! the exact byte/bit proof.

use super::{appearance, bits, boundary, creature, inventory, read_u32_le};

const MAX_INTERLEAVED_FRAGMENT_SPAN_BYTES: usize = 4096;
// HG short-declared creature updates can leave the chunk-local CNW fragment
// storage span in the live read buffer after the decompiled `U/5 0x3967`
// reader has finished. The 2026-05 seq31 `Northern Trader` capture carries a
// 26-byte span before the next real live-object boundary, so keep this bounded
// but large enough for that proven shape. The exact creature cursor simulator
// below must still consume the shortened read buffer before any span is moved.
const MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES: usize = 32;
const MAX_LEGACY_CREATURE_UPDATE_3967_LARGE_FRAGMENT_SPAN_BYTES: usize = 512;
// Appearance-owned fragment storage can be much larger than the compact spans
// observed after creature updates.  Local Diamond seq15 places item-looking
// bytes inside the span before the real following `U/5 0x3967` creature update;
// the helper below therefore scans past false `A` item boundaries, but accepts
// only when that following creature update validates from the promoted bits.
const MAX_APPEARANCE_FOLLOWING_CREATURE_FRAGMENT_SPAN_BYTES: usize = 256;
const MAX_APPEARANCE_FOLLOWING_CREATURE_UPDATE_TRAILING_SPAN_BYTES: usize = 512;
// Local Diamond seq15 carries a creature `P/5` appearance whose exact EE cursor
// ends before a chunk-local CNW fragment-storage span, then a top-level item
// `A` record.  This span is larger than the short creature-update spans above
// because it owns item-name / active-property fragment data for the following
// item object.  Keep the cap finite and require the following typed item reader
// to prove the promoted bits before mutating the real stream.
const MAX_APPEARANCE_FOLLOWING_ITEM_FRAGMENT_SPAN_BYTES: usize = 128;
// HG seq41 carries two zero bytes after a verified creature appearance record
// on the read-buffer side. Those bytes decode as empty CNW fragment-storage
// padding and are not a live-object submessage. Keep this deliberately tiny:
// any larger or non-zero trailing span should quarantine until a capture and
// decompile-backed owner prove the exact shape.
const MAX_TRAILING_ZERO_FRAGMENT_STORAGE_BYTES: usize = 2;
const MAX_TRAILING_FRAGMENT_PREFIX_BYTES: usize = MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES;
const MAX_BOUNDARY_COLLISION_TRAILING_FRAGMENT_PREFIX_BYTES: usize = 96;
const LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK: u32 = 0x0000_0008;
const LEGACY_CREATURE_UPDATE_3967_MASK: u32 = 0x0000_3967;
const LEGACY_CREATURE_UPDATE_C40F_MASK: u32 = 0x0000_C40F;
const LEGACY_CREATURE_UPDATE_C44F_MASK: u32 = 0x0000_C44F;

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

#[derive(Debug, Clone, Copy)]
pub(super) struct PromotedAppearanceFollowingCreatureSpan {
    pub old_record_end: usize,
    pub bytes_promoted: usize,
    pub bits_promoted: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PromotedAppearanceFollowingItemSpan {
    pub old_record_end: usize,
    pub bytes_promoted: usize,
    pub bits_promoted: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RemovedTrailingFragmentStorage {
    pub bytes_removed: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PromotedTrailingFragmentPrefix {
    pub bytes_promoted: usize,
    pub bits_promoted: usize,
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
    let missing_bits = prefix.fragment_bits.saturating_sub(available_bits);
    // Legacy inventory records can carry their CNW fragment storage interleaved
    // between the read-buffer cursor and the next live-object submessage. The
    // earlier bridge only promoted that span when the trailing fragment buffer
    // was too short. The Sooty Crow `I/0x0401` capture proves a stricter case:
    // Diamond `sub_455940` / EE `sub_1407B4F70` end the read cursor after the
    // compact 0x0001 branch plus 0x0400 equipment delta, then the server places
    // three packed fragment bytes before the following `P/5` appearance record.
    // Even if the global tail has enough bits numerically, those adjacent bytes
    // are the decompile-owned inventory fragment span and must be promoted
    // before the following semantic record can be dispatched.
    let minimum_span_bytes = if missing_bits == 0 {
        1
    } else {
        missing_bits.saturating_add(7) / 8
    };
    let span_end = find_interleaved_fragment_span_end(
        live_bytes,
        prefix.read_end,
        *record_end,
        minimum_span_bytes,
    )
    .or_else(|| {
        // The live-object boundary walker reports `record_end` as an exclusive
        // cursor at the next proven submessage. For inventory records with
        // interleaved CNW storage, the storage span can therefore end exactly
        // at `record_end`; the generic scanner above only finds boundaries
        // strictly inside the range. Accept this case only when the exclusive
        // cursor is itself a verified boundary (or the end of the live stream),
        // then let the exact inventory reader below prove the shortened record.
        let candidate = *record_end;
        let span_len = candidate.checked_sub(prefix.read_end)?;
        if span_len < minimum_span_bytes || span_len > MAX_INTERLEAVED_FRAGMENT_SPAN_BYTES {
            return None;
        }
        if candidate == live_bytes.len()
            || boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, candidate)
        {
            Some(candidate)
        } else {
            None
        }
    })?;
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

pub(super) fn promote_legacy_creature_update_3967_large_interleaved_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    offset: usize,
    record_end: &mut usize,
    bit_cursor: usize,
) -> Option<PromotedCreatureUpdateFragmentSpan> {
    let old_record_end = *record_end;
    let (read_end, end_bit_cursor) =
        creature::legacy_creature_update_3967_read_end_before_fragment_span(
            live_bytes,
            offset,
            old_record_end,
            fragment_bits,
            bit_cursor,
            MAX_LEGACY_CREATURE_UPDATE_3967_LARGE_FRAGMENT_SPAN_BYTES,
        )?;
    if read_end >= old_record_end {
        return None;
    }
    let span_bytes = old_record_end.saturating_sub(read_end);
    if span_bytes <= MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES {
        return None;
    }
    let promoted_bits =
        unpack_promoted_fragment_span_payload_bits(live_bytes.get(read_end..old_record_end)?)?;
    bits::insert_msb_bits(fragment_bits, end_bit_cursor, &promoted_bits)?;
    live_bytes.drain(read_end..old_record_end);
    *record_end = read_end;

    Some(PromotedCreatureUpdateFragmentSpan {
        read_end,
        old_record_end,
        bytes_promoted: span_bytes,
        bits_promoted: promoted_bits.len(),
        start_bit_cursor: bit_cursor,
        end_bit_cursor,
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

pub(super) fn promote_appearance_following_creature_update_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    span_start: usize,
    bit_cursor: usize,
) -> Option<PromotedAppearanceFollowingCreatureSpan> {
    if span_start >= live_bytes.len()
        || boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, span_start)
    {
        return None;
    }

    let (span_end, promoted_bits, following_end) = find_appearance_following_creature_update_span(
        live_bytes,
        span_start,
        bit_cursor,
        fragment_bits,
    )?;
    if span_end <= span_start
        || live_bytes.get(span_end).copied()? != b'U'
        || live_bytes.get(span_end + 1).copied()? != 0x05
        || read_u32_le(live_bytes, span_end + 6)? != LEGACY_CREATURE_UPDATE_3967_MASK
    {
        return None;
    }

    if following_end <= span_end {
        return None;
    }

    bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;
    live_bytes.drain(span_start..span_end);
    Some(PromotedAppearanceFollowingCreatureSpan {
        old_record_end: span_end,
        bytes_promoted: span_end.saturating_sub(span_start),
        bits_promoted: promoted_bits.len(),
    })
}

fn find_appearance_following_creature_update_span(
    live_bytes: &[u8],
    span_start: usize,
    bit_cursor: usize,
    fragment_bits: &[bool],
) -> Option<(usize, Vec<bool>, usize)> {
    let search_end = live_bytes.len();
    let max_span_end = span_start
        .checked_add(MAX_APPEARANCE_FOLLOWING_CREATURE_FRAGMENT_SPAN_BYTES)?
        .min(search_end);
    for span_end in span_start.checked_add(1)?..max_span_end.saturating_sub(1) {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, span_end) {
            continue;
        }
        if live_bytes.get(span_end).copied() != Some(b'U')
            || live_bytes.get(span_end + 1).copied() != Some(0x05)
            || read_u32_le(live_bytes, span_end + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
        {
            continue;
        }
        let promoted_bits =
            unpack_promoted_fragment_span_payload_bits(live_bytes.get(span_start..span_end)?)?;
        let following_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes, span_end, search_end,
        )
        .min(search_end);
        if following_end <= span_end {
            continue;
        }
        let mut proof_bits = fragment_bits.to_vec();
        bits::insert_msb_bits(&mut proof_bits, bit_cursor, &promoted_bits)?;
        let mut proof_cursor = bit_cursor;
        let owner_ok = creature::advance_verified_legacy_creature_update_record_for_span_owner(
            live_bytes,
            span_end,
            following_end,
            &proof_bits,
            &mut proof_cursor,
        );
        let trailing_span_ok = if owner_ok {
            false
        } else {
            creature::legacy_creature_update_3967_read_end_before_fragment_span(
                live_bytes,
                span_end,
                following_end,
                &proof_bits,
                bit_cursor,
                MAX_APPEARANCE_FOLLOWING_CREATURE_UPDATE_TRAILING_SPAN_BYTES,
            )
            .and_then(|(read_end, end_cursor)| {
                unpack_promoted_fragment_span_payload_bits(live_bytes.get(read_end..following_end)?)
                    .map(|bits| (read_end, end_cursor, bits.len()))
            })
            .is_some()
        };
        if !owner_ok && !trailing_span_ok {
            continue;
        }
        return Some((span_end, promoted_bits, following_end));
    }
    None
}

pub(super) fn promote_appearance_following_item_add_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    span_start: usize,
    bit_cursor: usize,
) -> Option<PromotedAppearanceFollowingItemSpan> {
    if span_start >= live_bytes.len()
        || boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, span_start)
    {
        return None;
    }

    let span_end = find_interleaved_fragment_span_end(live_bytes, span_start, live_bytes.len(), 1)?;
    let span_len = span_end.checked_sub(span_start)?;
    if span_len == 0 || span_len > MAX_APPEARANCE_FOLLOWING_ITEM_FRAGMENT_SPAN_BYTES {
        return None;
    }
    if !appearance::looks_like_legacy_item_add_record_boundary(live_bytes, span_end) {
        return None;
    }

    let old_item_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
        live_bytes,
        span_end,
        live_bytes.len(),
    )
    .min(live_bytes.len());
    if old_item_end <= span_end {
        return None;
    }

    let promoted_bits =
        unpack_promoted_fragment_span_payload_bits(live_bytes.get(span_start..span_end)?)?;
    let mut proof_bits = fragment_bits.clone();
    bits::insert_msb_bits(&mut proof_bits, bit_cursor, &promoted_bits)?;

    let mut trial_bytes = live_bytes.clone();
    trial_bytes.drain(span_start..span_end);
    let item_offset = span_start;
    let mut item_end = old_item_end.checked_sub(span_len)?;

    // The span is accepted only when the following item `A` record owns it:
    // first the legacy item-add translator must be able to insert the EE-only
    // item shape extras from those promoted bits, then the exact EE item reader
    // must consume the resulting record from the same cursor.  This mirrors the
    // Diamond `sub_451020` item-name / active-property subobject model rather
    // than treating the bytes as an arbitrary fragment blob.
    appearance::insert_ee_item_add_extras_for_ee(
        &mut trial_bytes,
        item_offset,
        &mut item_end,
        &mut proof_bits,
        bit_cursor,
    )?;
    let mut proof_cursor = bit_cursor;
    if !appearance::advance_verified_ee_item_add_record(
        &trial_bytes,
        item_offset,
        item_end,
        &proof_bits,
        &mut proof_cursor,
    ) {
        return None;
    }

    bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;
    live_bytes.drain(span_start..span_end);
    Some(PromotedAppearanceFollowingItemSpan {
        old_record_end: span_end,
        bytes_promoted: span_len,
        bits_promoted: promoted_bits.len(),
    })
}

pub(super) fn remove_trailing_zero_fragment_storage_after_verified_record_for_ee(
    live_bytes: &mut Vec<u8>,
    span_start: usize,
) -> Option<RemovedTrailingFragmentStorage> {
    if span_start >= live_bytes.len()
        || boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, span_start)
    {
        return None;
    }

    let span_end = if live_bytes.len().saturating_sub(span_start)
        <= MAX_TRAILING_ZERO_FRAGMENT_STORAGE_BYTES
    {
        live_bytes.len()
    } else {
        find_interleaved_fragment_span_end(live_bytes, span_start, live_bytes.len(), 1)?
    };
    let span = live_bytes.get(span_start..span_end)?;
    if span.is_empty() || span.len() > MAX_TRAILING_ZERO_FRAGMENT_STORAGE_BYTES {
        return None;
    }

    let decoded_bits = bits::decode_msb_valid_bits(span, 3)?;
    if decoded_bits.iter().skip(3).any(|bit| *bit) {
        return None;
    }

    let bytes_removed = span.len();
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object zero fragment storage removed: span_start={span_start} span_end={span_end} bytes_removed={bytes_removed}"
        );
    }
    live_bytes.drain(span_start..span_end);
    Some(RemovedTrailingFragmentStorage { bytes_removed })
}

pub(super) fn promote_trailing_fragment_prefix_after_verified_record_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    span_start: usize,
) -> Option<PromotedTrailingFragmentPrefix> {
    if span_start >= live_bytes.len()
        || boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, span_start)
    {
        return None;
    }

    let prefix = live_bytes.get(span_start..)?.to_vec();
    if prefix.is_empty() || prefix.len() > MAX_TRAILING_FRAGMENT_PREFIX_BYTES {
        return None;
    }

    // Diamond/EE live-object packets keep the CNW fragment-storage stream after
    // the declared read-buffer cursor. Short-declared HG windows can leave the
    // first one or two storage bytes behind the last decompile-proven record,
    // while the remaining storage bytes already sit in the packet tail. Accept
    // only a tiny suffix that, when prepended to the existing tail, decodes as
    // one valid CNW MSB fragment bitstream; the final strict live-object claim
    // still has to consume the resulting bit cursor exactly before anything is
    // emitted.
    let mut combined = Vec::with_capacity(prefix.len().saturating_add(fragment_bytes.len()));
    combined.extend_from_slice(&prefix);
    combined.extend_from_slice(fragment_bytes);
    let combined_bits = bits::decode_msb_valid_bits(&combined, 3)?;
    let bits_promoted = combined_bits.len().saturating_sub(fragment_bits.len());

    live_bytes.drain(span_start..);
    *fragment_bytes = combined;
    *fragment_bits = combined_bits;

    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object trailing fragment prefix promoted: span_start={span_start} bytes_promoted={} bits_promoted={bits_promoted}",
            prefix.len()
        );
    }

    Some(PromotedTrailingFragmentPrefix {
        bytes_promoted: prefix.len(),
        bits_promoted,
    })
}

pub(super) fn promote_boundary_collision_trailing_fragment_prefix_after_verified_record_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    span_start: usize,
    bit_cursor: usize,
) -> Option<PromotedTrailingFragmentPrefix> {
    if span_start >= live_bytes.len() {
        return None;
    }

    let prefix = live_bytes.get(span_start..)?.to_vec();
    if prefix.is_empty() || prefix.len() > MAX_BOUNDARY_COLLISION_TRAILING_FRAGMENT_PREFIX_BYTES {
        return None;
    }

    // This is intentionally narrower than the ordinary trailing-prefix helper.
    // It is called only after the preceding record has been semantically proven
    // and the apparent `A`/`U` boundary at `span_start` has failed its exact
    // family validator. Local Diamond seq15 proves a chunk-local CNW fragment
    // prefix whose first byte is ASCII `A`; promoting it here prevents the
    // boundary scanner from turning fragment storage into a fake item add.
    let mut promoted_bits = bits::decode_msb_valid_bits(&prefix, 3)?;
    if promoted_bits.len() < 3 {
        return None;
    }
    promoted_bits.drain(0..3);
    let bits_promoted = promoted_bits.len();
    bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;

    live_bytes.drain(span_start..);

    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object boundary-collision trailing fragment prefix promoted: span_start={span_start} bytes_promoted={} bits_promoted={bits_promoted}",
            prefix.len()
        );
    }

    Some(PromotedTrailingFragmentPrefix {
        bytes_promoted: prefix.len(),
        bits_promoted,
    })
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
    {
        return None;
    }
    let raw_mask = read_u32_le(live_bytes, offset + 6)?;
    if !matches!(
        raw_mask,
        LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK
            | LEGACY_CREATURE_UPDATE_3967_MASK
            | LEGACY_CREATURE_UPDATE_C40F_MASK
            | LEGACY_CREATURE_UPDATE_C44F_MASK
    ) {
        return None;
    }

    // Decompile/capture-backed short-declared repair for creature `U/5`
    // update masks whose reader shape is exact, but whose legacy stream window
    // can leave a bounded CNW fragment-storage suffix in the read buffer.
    // `0x3967` was the original HG capture family. `0xC40F` and `0xC44F` are
    // Starcore5 Sooty Crow transition families: Diamond writes lower movement /
    // action fields, optional low `0x0040` creature-state fields, then the
    // C408-style self/status suffix. Diamond `0x4463B0..0x44649B` proves the
    // low `0x0040` branch is `WORD, BYTE, WORD, BYTE, BOOL[, OBJECTID]`.
    //
    // Local Dark Ranger adds a narrower `0x0008` effect-only family: row
    // `0x00F3` is a no-target LowLightVision visualeffects.2da row, and the
    // feature-0x0E-false reader ends before the adjacent CNW storage span.
    // In all families, bytes after the exact creature cursor are promoted only
    // after the shortened record validates from the current fragment cursor.
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
