//! Legacy interleaved CNW fragment-span promotion.
//!
//! Some legacy deflated live-object streams carry a proven read body, then a
//! chunk-local CNW fragment-storage span, then the next live-object submessage.
//! EE wants one normalized `P 05 01` payload with all read-buffer bytes first
//! and the CNW fragment bitstream after the declared read length. These helpers
//! perform only that bounded transport normalization; family parsers still own
//! the exact byte/bit proof.

use super::{
    CNW_FRAGMENT_HEADER_BITS, DOOR_OBJECT_TYPE, add, appearance, bits, boundary, creature, gui,
    inventory, read_u32_le, world_status,
};

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
// observed after creature updates. Local Diamond captures place item-looking
// bytes inside the span before the real following `U/5` creature update; HG
// seq31 proves the same decompile-owned shape with a 284-byte span after the
// short Northern Trader `P/5` appearance. Keep a small margin over that capture
// so mixed live-object probes remain bounded while the following creature
// update still has to validate from the promoted bits.
const MAX_APPEARANCE_FOLLOWING_CREATURE_FRAGMENT_SPAN_BYTES: usize = 320;
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
const MAX_EFFECT_ONLY_FOLLOWING_GUI_FRAGMENT_SPAN_BYTES: usize = 128;
const MAX_WORK_REMAINING_TRAILING_FRAGMENT_SPAN_BYTES: usize = 64;
const LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK: u32 = 0x0000_0008;
const LEGACY_CREATURE_UPDATE_0067_MASK: u32 = 0x0000_0067;
const LEGACY_CREATURE_UPDATE_3967_MASK: u32 = 0x0000_3967;
const LEGACY_CREATURE_UPDATE_C40F_MASK: u32 = 0x0000_C40F;
const LEGACY_CREATURE_UPDATE_C44F_MASK: u32 = 0x0000_C44F;
const LEGACY_CREATURE_UPDATE_8047_MASK: u32 = 0x0000_8047;

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
    )?;
    let read_end = proof.read_end;
    let promoted_bits = proof.promoted_bits;
    let insertion_cursor = proof.insertion_cursor;
    if proof.rewrite_bare_second_identity_string {
        let identity_rewrite = creature::rewrite_3967_bare_second_identity_string_for_ee(
            live_bytes,
            offset,
            read_end,
            fragment_bits,
            proof.start_bit_cursor,
        )?;
        if identity_rewrite.advanced_bit_cursor != proof.end_bit_cursor {
            return None;
        }
    }
    bits::insert_msb_bits(fragment_bits, insertion_cursor, &promoted_bits)?;
    live_bytes.drain(read_end..old_record_end);
    *record_end = read_end;

    trace_creature_update_interleaved_fragment_span_promotion(
        live_bytes,
        offset,
        read_end,
        old_record_end,
        insertion_cursor,
        proof.start_bit_cursor,
        proof.end_bit_cursor,
        promoted_bits.len(),
        proof.rewrite_bare_second_identity_string,
    );

    Some(PromotedCreatureUpdateFragmentSpan {
        read_end,
        old_record_end,
        bytes_promoted: old_record_end.saturating_sub(read_end),
        bits_promoted: promoted_bits.len(),
        start_bit_cursor: proof.start_bit_cursor,
        end_bit_cursor: proof.end_bit_cursor,
    })
}

pub(super) fn promote_effect_only_creature_update_following_gui_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    offset: usize,
    record_end: &mut usize,
    bit_cursor: usize,
) -> Option<PromotedCreatureUpdateFragmentSpan> {
    if offset.checked_add(10)? >= live_bytes.len()
        || live_bytes.get(offset).copied()? != b'U'
        || live_bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(live_bytes, offset + 6)? != LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK
    {
        return None;
    }

    let count = usize::from(super::read_u16_le(live_bytes, offset + 10)?);
    if count == 0 || count > 256 {
        return None;
    }
    let read_end = offset
        .checked_add(10)?
        .checked_add(2)?
        .checked_add(count.checked_mul(3)?)?;
    if read_end >= live_bytes.len() || *record_end > live_bytes.len() {
        return None;
    }

    // Local CEP v2.2 builder seq26 proves a compact feature-0x0E-false
    // creature effect row followed by a chunk-local CNW fragment span whose
    // first interior false boundary is `I 0xFE ... mask=0`. Diamond/EE do not
    // have an inventory reader for that byte shape; the next decompile-owned
    // live-object submessage is the `G Q` quickbar item-link row after the
    // fragment span. Keep this intentionally narrower than generic span
    // recovery: the shortened creature record must validate first, any
    // interior `I` candidate must fail the inventory reader/prefix proof, the
    // promoted bytes must decode as CNW fragment storage, and the following
    // `GQ` read-buffer record must prove its exact byte end.
    let mut proof_cursor = bit_cursor;
    if !creature::advance_verified_noop_creature_update_record_exact_cursor(
        live_bytes,
        offset,
        read_end,
        fragment_bits,
        &mut proof_cursor,
    ) {
        return None;
    }

    let scan_end = read_end
        .checked_add(MAX_EFFECT_ONLY_FOLLOWING_GUI_FRAGMENT_SPAN_BYTES)?
        .min(live_bytes.len());
    let mut accepted: Option<(usize, Vec<bool>)> = None;
    for span_end in read_end.checked_add(1)?..scan_end {
        if live_bytes.get(span_end).copied() != Some(b'G')
            || live_bytes.get(span_end + 1).copied() != Some(b'Q')
        {
            continue;
        }
        let Some(gui_end) = gui::try_get_legacy_live_gui_record_end_with_fragment_proof(
            live_bytes,
            span_end,
            live_bytes.len(),
            fragment_bits,
            proof_cursor,
        ) else {
            continue;
        };
        if !gui::is_verified_live_gui_read_buffer_record(live_bytes, span_end, gui_end) {
            continue;
        }
        for interior in read_end..span_end {
            if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, interior) {
                continue;
            }
            if live_bytes.get(interior).copied() == Some(b'I')
                && inventory::try_get_legacy_live_inventory_prefix_claim(
                    live_bytes, interior, span_end,
                )
                .is_none()
                && inventory::try_get_legacy_live_inventory_fragment_bit_count(
                    live_bytes, interior, span_end,
                )
                .is_none()
            {
                continue;
            }
            return None;
        }
        let span = live_bytes.get(read_end..span_end)?;
        if span.is_empty() {
            continue;
        }
        let promoted_bits = unpack_promoted_fragment_span_payload_bits(span)?;
        if promoted_bits.iter().all(|bit| !*bit) {
            continue;
        }
        if accepted.replace((span_end, promoted_bits)).is_some() {
            return None;
        }
    }

    let (span_end, promoted_bits) = accepted?;
    bits::insert_msb_bits(fragment_bits, proof_cursor, &promoted_bits)?;
    live_bytes.drain(read_end..span_end);
    *record_end = read_end;

    Some(PromotedCreatureUpdateFragmentSpan {
        read_end,
        old_record_end: span_end,
        bytes_promoted: span_end.saturating_sub(read_end),
        bits_promoted: promoted_bits.len(),
        start_bit_cursor: bit_cursor,
        end_bit_cursor: proof_cursor,
    })
}

pub(super) fn promote_legacy_creature_update_large_interleaved_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    offset: usize,
    record_end: &mut usize,
    bit_cursor: usize,
) -> Option<PromotedCreatureUpdateFragmentSpan> {
    let old_record_end = *record_end;
    let (read_end, end_bit_cursor) =
        creature::legacy_creature_update_read_end_before_fragment_span_for_span_owner(
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
    let raw_mask = read_u32_le(live_bytes, offset + 6)?;
    let minimum_large_span = if raw_mask == LEGACY_CREATURE_UPDATE_0067_MASK {
        // Prelude seq10 proves a larger 0x67 suffix after the appearance-owned
        // fragment span has been moved. The same typed cursor proof above owns
        // the shortened update; this guard only keeps small, already covered
        // adjacent spans on their older path.
        0
    } else {
        MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES
    };
    if span_bytes <= minimum_large_span {
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
    let proof = find_creature_update_3967_interleaved_fragment_span(
        live_bytes,
        offset,
        old_record_end,
        fragment_bits,
        bit_cursor,
    );
    if let Some(proof) = &proof {
        trace_creature_update_interleaved_fragment_span_proof(
            live_bytes,
            offset,
            proof.read_end,
            old_record_end,
            proof.insertion_cursor,
            proof.start_bit_cursor,
            proof.end_bit_cursor,
            proof.promoted_bits.len(),
            proof.rewrite_bare_second_identity_string,
        );
    }
    proof.map(|proof| proof.read_end)
}

pub(super) fn promote_appearance_following_creature_update_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    span_start: usize,
    bit_cursor: usize,
) -> Option<PromotedAppearanceFollowingCreatureSpan> {
    if span_start >= live_bytes.len() {
        return None;
    }
    if boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, span_start) {
        if live_bytes.get(span_start).copied() != Some(b'A') {
            return None;
        }
        let candidate_add_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            span_start,
            live_bytes.len(),
        )
        .min(live_bytes.len());
        let mut add_cursor = bit_cursor;
        if add::advance_verified_add_record(
            live_bytes,
            span_start,
            candidate_add_end,
            fragment_bits,
            &mut add_cursor,
        ) {
            return None;
        }
        // Local Prelude puts an item-add-shaped byte sequence inside the CNW
        // fragment storage between the full appearance and the real following
        // `U/5` creature update. If that apparent `A` record validates as a
        // typed add, it remains a record boundary. Otherwise the scan below may
        // cross it, but still promotes only when the decompile-owned update
        // consumes from the promoted fragment bits.
    }

    if span_start >= live_bytes.len() {
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
        || !creature::is_appearance_following_creature_span_owner_mask(read_u32_le(
            live_bytes,
            span_end + 6,
        )?)
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

pub(super) fn verified_appearance_following_creature_update_span_offset_for_ee(
    live_bytes: &[u8],
    span_start: usize,
    bit_cursor: usize,
    fragment_bits: &[bool],
) -> Option<usize> {
    // Immutable companion to the promotion helper above.  Creature appearance
    // boundary repair uses this to prove that the bytes after an exact EE
    // `P/5` record are CNW fragment storage owned by the following typed
    // creature update; the caller still leaves mutation to
    // `promote_appearance_following_creature_update_span_for_ee`.
    find_appearance_following_creature_update_span(
        live_bytes,
        span_start,
        bit_cursor,
        fragment_bits,
    )
    .map(|(span_end, _, _)| span_end)
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
            || read_u32_le(live_bytes, span_end + 6).is_none_or(|raw_mask| {
                !creature::is_appearance_following_creature_span_owner_mask(raw_mask)
            })
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
            creature::legacy_creature_update_read_end_before_fragment_span_for_span_owner(
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
        let action0_bridge_followup_rewrite_ok = if owner_ok || trailing_span_ok {
            false
        } else {
            // This appearance-adjacent span is only promoted when the following
            // creature update proves ownership. Some local Diamond captures
            // carry the decompile-owned action0 bridge WORD that EE does not
            // read; reuse the narrow action0 removal proof so the span is not
            // treated as arbitrary fragment storage.
            let mut proof_live_bytes = live_bytes.to_vec();
            let mut proof_record_end = following_end;
            let mut proof_rewrite_bits = proof_bits.clone();
            creature::remove_3967_action0_legacy_bridge_followup_for_ee(
                &mut proof_live_bytes,
                span_end,
                &mut proof_record_end,
                &mut proof_rewrite_bits,
                bit_cursor,
            )
            .is_some()
        };
        if !owner_ok && !trailing_span_ok && !action0_bridge_followup_rewrite_ok {
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

    let span_end = find_interleaved_fragment_span_end(live_bytes, span_start, live_bytes.len(), 1)
        .unwrap_or(live_bytes.len());
    let prefix = live_bytes.get(span_start..span_end)?.to_vec();
    if prefix.is_empty() || prefix.len() > MAX_TRAILING_FRAGMENT_PREFIX_BYTES {
        return None;
    }

    // Diamond/EE live-object packets keep the CNW fragment-storage stream after
    // the declared read-buffer cursor. Short-declared HG windows can leave the
    // first one or two storage bytes behind the last decompile-proven record,
    // while the remaining storage bytes already sit in the packet tail. When a
    // following live-object boundary is present, promote only the prefix before
    // that boundary and let the next family reader prove the resulting bit
    // cursor. Accept only a tiny prefix that, when prepended to the existing
    // tail, decodes as one valid CNW MSB fragment bitstream; the final strict
    // live-object claim still has to consume the resulting bit cursor exactly
    // before anything is emitted.
    let mut combined = Vec::with_capacity(prefix.len().saturating_add(fragment_bytes.len()));
    combined.extend_from_slice(&prefix);
    combined.extend_from_slice(fragment_bytes);
    let combined_bits = bits::decode_msb_valid_bits(&combined, 3)?;
    let bits_promoted = combined_bits.len().saturating_sub(fragment_bits.len());

    live_bytes.drain(span_start..span_end);
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

pub(super) fn promote_work_remaining_trailing_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
) -> Option<PromotedTrailingFragmentPrefix> {
    if live_bytes.len() <= 3
        || fragment_bytes.len() > 4
        || fragment_bits
            .iter()
            .skip(CNW_FRAGMENT_HEADER_BITS)
            .any(|bit| *bit)
    {
        return None;
    }

    let work_offset = (0..=live_bytes.len().saturating_sub(3))
        .rev()
        .find(|offset| work_remaining_offset_is_top_level_suffix(live_bytes, *offset))?;
    let span_start = work_offset.checked_add(3)?;
    if span_start >= live_bytes.len()
        || boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, span_start)
    {
        return None;
    }

    let span = live_bytes.get(span_start..)?.to_vec();
    if span.is_empty() || span.len() > MAX_WORK_REMAINING_TRAILING_FRAGMENT_SPAN_BYTES {
        return None;
    }
    if work_remaining_trailing_span_contains_top_level_boundary_after_fragment_prefix(
        live_bytes, span_start,
    ) {
        return None;
    }

    // `W current total` is the decompiled fragment-neutral work-remaining
    // record. Local Chapter1 door-transition evidence can put the CNW
    // fragment-storage byte stream immediately after that record while the
    // legacy high-level declared slot is all zero. Promote only that bounded
    // tail as the complete fragment stream; the caller still requires the
    // rewritten payload to pass the exact EE live-object validator.
    let promoted_bits = bits::decode_msb_valid_bits(&span, CNW_FRAGMENT_HEADER_BITS)?;
    if promoted_bits.len() <= CNW_FRAGMENT_HEADER_BITS {
        return None;
    }
    let bits_promoted = promoted_bits.len().saturating_sub(CNW_FRAGMENT_HEADER_BITS);

    live_bytes.truncate(span_start);
    *fragment_bytes = span;
    *fragment_bits = promoted_bits;

    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object work-remaining trailing fragment span promoted: span_start={span_start} bytes_promoted={} bits_promoted={bits_promoted}",
            fragment_bytes.len()
        );
    }

    Some(PromotedTrailingFragmentPrefix {
        bytes_promoted: fragment_bytes.len(),
        bits_promoted,
    })
}

fn work_remaining_trailing_span_contains_top_level_boundary_after_fragment_prefix(
    live_bytes: &[u8],
    span_start: usize,
) -> bool {
    let Some(span) = live_bytes.get(span_start..) else {
        return false;
    };

    for split in 1..span.len() {
        let prefix = &span[..split];
        if prefix.len() > MAX_WORK_REMAINING_TRAILING_FRAGMENT_SPAN_BYTES {
            return false;
        }
        let prefix_is_fragment_storage =
            bits::decode_msb_valid_bits(prefix, CNW_FRAGMENT_HEADER_BITS)
                .is_some_and(|decoded| decoded.len() > CNW_FRAGMENT_HEADER_BITS);
        if !prefix_is_fragment_storage {
            continue;
        }
        let boundary_offset = span_start + split;
        if top_level_boundary_walk_reaches_end(live_bytes, boundary_offset) {
            return true;
        }
    }

    false
}

fn top_level_boundary_walk_reaches_end(live_bytes: &[u8], mut offset: usize) -> bool {
    while offset < live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            return false;
        }
        let next = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        );
        if next <= offset || next > live_bytes.len() {
            return false;
        }
        offset = next;
    }

    true
}

fn work_remaining_offset_is_top_level_suffix(live_bytes: &[u8], work_offset: usize) -> bool {
    if !world_status::is_work_remaining_record_at(live_bytes, work_offset) {
        return false;
    }

    let mut offset = 0usize;
    while offset < work_offset {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            return false;
        }
        let next = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        );
        if next <= offset || next > work_offset {
            return false;
        }
        offset = next;
    }

    offset == work_offset
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

pub(super) fn promote_creature_0047_following_add_boundary_collision_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    span_start: usize,
    span_end: usize,
    bit_cursor: usize,
) -> Option<PromotedTrailingFragmentPrefix> {
    if span_start >= span_end
        || span_end >= live_bytes.len()
        || live_bytes.get(span_start).copied()? != b'I'
        || live_bytes.get(span_end).copied()? != b'A'
        || live_bytes.get(span_end + 1).copied()? != 0x05
    {
        return None;
    }

    let span = live_bytes.get(span_start..span_end)?;
    if span.is_empty() || span.len() > MAX_BOUNDARY_COLLISION_TRAILING_FRAGMENT_PREFIX_BYTES {
        return None;
    }

    // Starcore5 Sooty Crow seq35 proves a decompile-owned `U/5 0x47`
    // movement/state update whose adjacent CNW fragment-storage span begins
    // with bytes that look like an `I/FE` inventory record. The following
    // record is a real creature `A/5` add. Promote only this exact collision:
    // the caller has already proven the owning `U/5 0x47` record, the apparent
    // inventory record has failed the inventory reader, the span must decode
    // as CNW fragment storage, and the following add record must expose the
    // fixed decompile-owned creature-add byte fields.
    let mut promoted_bits = bits::decode_msb_valid_bits(span, 3)?;
    if promoted_bits.len() < 3 {
        return None;
    }
    promoted_bits.drain(0..3);
    if promoted_bits.is_empty() {
        return None;
    }

    let following_legacy_add_end =
        span_end.checked_add(creature::LEGACY_CREATURE_ADD_RECORD_BYTES)?;
    if following_legacy_add_end > live_bytes.len()
        || !creature::looks_like_legacy_creature_add_transform_fields(
            live_bytes,
            span_end,
            following_legacy_add_end,
        )
    {
        return None;
    }

    let bits_promoted = promoted_bits.len();
    bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;
    live_bytes.drain(span_start..span_end);

    Some(PromotedTrailingFragmentPrefix {
        bytes_promoted: span_end.saturating_sub(span_start),
        bits_promoted,
    })
}

pub(super) fn promote_door_add_following_missing_type_update_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    span_start: usize,
    bit_cursor: usize,
    preceding_door_object_id: u32,
) -> Option<PromotedTrailingFragmentPrefix> {
    if span_start >= live_bytes.len()
        || live_bytes.get(span_start).copied()? != b'U'
        || live_bytes.get(span_start + 1).copied()? != 0
        || read_u32_le(live_bytes, span_start + 2)? != preceding_door_object_id
    {
        return None;
    }

    let span_end = find_interleaved_fragment_span_end(live_bytes, span_start, live_bytes.len(), 1)?;
    let span = live_bytes.get(span_start..span_end)?;
    if span.is_empty() || span.len() > MAX_BOUNDARY_COLLISION_TRAILING_FRAGMENT_PREFIX_BYTES {
        return None;
    }

    let following_world_status_then_door = span_end
        .checked_add(3)
        .filter(|following_world_status_end| *following_world_status_end < live_bytes.len())
        .is_some_and(|following_world_status_end| {
            world_status::is_work_remaining_record_at(live_bytes, span_end)
                && looks_like_door_add_fixed_prefix(live_bytes, following_world_status_end)
        });
    let following_item_add = live_bytes.get(span_end).copied() == Some(b'A')
        && live_bytes.get(span_end + 1).copied() == Some(0x06)
        && boundary::looks_like_legacy_live_object_id_at(live_bytes, span_end + 2);
    if !following_world_status_then_door && !following_item_add {
        return None;
    }
    let mut promoted_bits = bits::decode_msb_valid_bits(span, 3)?;
    if promoted_bits.len() < 3 {
        return None;
    }
    promoted_bits.drain(0..3);
    if promoted_bits.is_empty() {
        return None;
    }

    let bits_promoted = promoted_bits.len();
    bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;
    live_bytes.drain(span_start..span_end);

    Some(PromotedTrailingFragmentPrefix {
        bytes_promoted: span_end.saturating_sub(span_start),
        bits_promoted,
    })
}

pub(super) fn promote_door_add_embedded_missing_type_update_fragment_span_for_ee(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    record_offset: usize,
    old_record_end: usize,
    bit_cursor: usize,
) -> Option<PromotedTrailingFragmentPrefix> {
    if old_record_end > live_bytes.len()
        || record_offset.checked_add(20)? > old_record_end
        || live_bytes.get(record_offset).copied()? != b'A'
        || live_bytes.get(record_offset + 1).copied()? != DOOR_OBJECT_TYPE
        || !boundary::looks_like_legacy_live_object_id_at(live_bytes, record_offset + 2)
    {
        return None;
    }

    let door_object_id = read_u32_le(live_bytes, record_offset + 2)?;
    let first_dword = read_u32_le(live_bytes, record_offset + 6)?;
    let _generic_door_row = read_u32_le(live_bytes, record_offset + 10)?;
    if first_dword != 0 {
        return None;
    }

    let name_offset = record_offset.checked_add(14)?;
    let span_start = name_offset.checked_add(6)?;
    if span_start > old_record_end
        || read_u32_le(live_bytes, name_offset).is_none_or(|name_token| name_token == 0)
        || live_bytes.get(span_start).copied()? != b'U'
        || live_bytes.get(span_start + 1).copied()? != 0
        || read_u32_le(live_bytes, span_start + 2)? != door_object_id
    {
        return None;
    }

    let span_end = find_interleaved_fragment_span_end(live_bytes, span_start, live_bytes.len(), 1)?;
    if span_end != old_record_end {
        return None;
    }
    let span = live_bytes.get(span_start..span_end)?;
    if span.is_empty() || span.len() > MAX_BOUNDARY_COLLISION_TRAILING_FRAGMENT_PREFIX_BYTES {
        return None;
    }

    let following_world_status_then_door = span_end
        .checked_add(3)
        .filter(|following_world_status_end| *following_world_status_end < live_bytes.len())
        .is_some_and(|following_world_status_end| {
            world_status::is_work_remaining_record_at(live_bytes, span_end)
                && looks_like_door_add_fixed_prefix(live_bytes, following_world_status_end)
        });
    let following_item_add = live_bytes.get(span_end).copied() == Some(b'A')
        && live_bytes.get(span_end + 1).copied() == Some(0x06)
        && boundary::looks_like_legacy_live_object_id_at(live_bytes, span_end + 2);
    if !following_world_status_then_door && !following_item_add {
        return None;
    }

    let mut promoted_bits = bits::decode_msb_valid_bits(span, 3)?;
    if promoted_bits.len() < 3 {
        return None;
    }
    promoted_bits.drain(0..3);
    if promoted_bits.is_empty() {
        return None;
    }

    // Diamond's compact tail-before-empty-name source shape contributes four
    // final door BOOLs. The focused add translator later inserts EE's two
    // omitted direct-name branch bits before those source bits, so inserting the
    // promoted span after the four source bits lets it shift to the exact
    // post-add cursor after visual/name normalization.
    let insertion_cursor = bit_cursor.checked_add(4)?;
    let bits_promoted = promoted_bits.len();
    bits::insert_msb_bits(fragment_bits, insertion_cursor, &promoted_bits)?;
    live_bytes.drain(span_start..span_end);

    Some(PromotedTrailingFragmentPrefix {
        bytes_promoted: span_end.saturating_sub(span_start),
        bits_promoted,
    })
}

fn looks_like_door_add_fixed_prefix(live_bytes: &[u8], offset: usize) -> bool {
    if offset + 10 > live_bytes.len()
        || live_bytes.get(offset).copied() != Some(b'A')
        || live_bytes.get(offset + 1).copied() != Some(DOOR_OBJECT_TYPE)
        || !boundary::looks_like_legacy_live_object_id_at(live_bytes, offset + 2)
    {
        return false;
    }

    let Some(first_dword) = read_u32_le(live_bytes, offset + 6) else {
        return false;
    };
    first_dword != 0 || offset + 14 <= live_bytes.len()
}

struct CreatureUpdateFragmentSpanProof {
    read_end: usize,
    promoted_bits: Vec<bool>,
    insertion_cursor: usize,
    start_bit_cursor: usize,
    end_bit_cursor: usize,
    rewrite_bare_second_identity_string: bool,
}

fn find_creature_update_3967_interleaved_fragment_span(
    live_bytes: &[u8],
    offset: usize,
    old_record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
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
            | LEGACY_CREATURE_UPDATE_0067_MASK
            | LEGACY_CREATURE_UPDATE_3967_MASK
            | LEGACY_CREATURE_UPDATE_C40F_MASK
            | LEGACY_CREATURE_UPDATE_C44F_MASK
            | LEGACY_CREATURE_UPDATE_8047_MASK
    ) {
        return None;
    }

    // Decompile/capture-backed short-declared repair for creature `U/5`
    // update masks whose reader shape is exact, but whose legacy stream window
    // can leave a bounded CNW fragment-storage suffix in the read buffer.
    // `0x3967` was the original HG capture family. Prelude seq10 adds
    // `0x0067`, the lower movement/action/state family plus the decompiled
    // `0x0020` portrait WORD branch. `0xC40F` and `0xC44F` are Starcore5 Sooty
    // Crow transition families: Diamond writes lower movement / action fields,
    // optional low `0x0040` creature-state fields, then the C408-style
    // self/status suffix. Diamond `0x4463B0..0x44649B` proves the low `0x0040`
    // branch is `WORD, BYTE, WORD, BYTE, BOOL[, OBJECTID]`.
    //
    // Local Dark Ranger adds a narrower `0x0008` effect-only family: row
    // `0x00F3` is a no-target LowLightVision visualeffects.2da row, and the
    // feature-0x0E-false reader ends before the adjacent CNW storage span.
    // Local Winds/Eremor seq24 adds the `0x8047` sibling: the shared creature
    // cursor simulator proves the movement/action/state read body, then the
    // capture carries four zero CNW storage bytes before the live-object tail.
    // In all families, bytes after the exact creature cursor are promoted only
    // after the shortened record validates from the current fragment cursor.
    let min_read_end = offset.checked_add(10)?;
    let scan_start = old_record_end
        .saturating_sub(MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES)
        .max(min_read_end);
    let mut accepted: Option<CreatureUpdateFragmentSpanProof> = None;
    for read_end in scan_start..old_record_end {
        let span = live_bytes.get(read_end..old_record_end)?;
        if span.is_empty() || span.len() > MAX_CREATURE_UPDATE_FRAGMENT_SPAN_BYTES {
            continue;
        }
        let Some(promoted_bits) = unpack_promoted_fragment_span_payload_bits(span) else {
            continue;
        };
        let mut proof_cursor = bit_cursor;
        let accepted_by_exact_parser =
            creature::advance_verified_noop_creature_update_record_exact_cursor(
                live_bytes,
                offset,
                read_end,
                fragment_bits,
                &mut proof_cursor,
            );
        if !accepted_by_exact_parser {
            if raw_mask != LEGACY_CREATURE_UPDATE_3967_MASK {
                continue;
            }
            let mut trial = live_bytes.to_vec();
            let Some(identity_rewrite) = creature::rewrite_3967_bare_second_identity_string_for_ee(
                &mut trial,
                offset,
                read_end,
                fragment_bits,
                bit_cursor,
            ) else {
                continue;
            };
            proof_cursor = identity_rewrite.advanced_bit_cursor;
            accepted = Some(CreatureUpdateFragmentSpanProof {
                read_end,
                promoted_bits,
                insertion_cursor: proof_cursor,
                start_bit_cursor: bit_cursor,
                end_bit_cursor: proof_cursor,
                rewrite_bare_second_identity_string: true,
            });
        } else {
            accepted = Some(CreatureUpdateFragmentSpanProof {
                read_end,
                promoted_bits,
                insertion_cursor: proof_cursor,
                start_bit_cursor: bit_cursor,
                end_bit_cursor: proof_cursor,
                rewrite_bare_second_identity_string: false,
            });
        }
        if accepted.is_some() {
            break;
        }
    }

    accepted
}

fn trace_creature_update_interleaved_fragment_span_promotion(
    live_bytes: &[u8],
    offset: usize,
    read_end: usize,
    old_record_end: usize,
    insertion_cursor: usize,
    start_bit_cursor: usize,
    end_bit_cursor: usize,
    bits_promoted: usize,
    rewrite_bare_second_identity_string: bool,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    let raw_mask = read_u32_le(live_bytes, offset + 6).unwrap_or(0);
    eprintln!(
        "live-object creature-update interleaved fragment span promoted: offset={offset} raw_mask=0x{raw_mask:08X} read_end={read_end} old_record_end={old_record_end} span_bytes={} bits_promoted={bits_promoted} insertion_cursor={insertion_cursor} bit_cursor={start_bit_cursor}->{end_bit_cursor} bare_second_identity_rewrite={rewrite_bare_second_identity_string}",
        old_record_end.saturating_sub(read_end)
    );
}

fn trace_creature_update_interleaved_fragment_span_proof(
    live_bytes: &[u8],
    offset: usize,
    read_end: usize,
    old_record_end: usize,
    insertion_cursor: usize,
    start_bit_cursor: usize,
    end_bit_cursor: usize,
    bits_promoted: usize,
    rewrite_bare_second_identity_string: bool,
) {
    if !debug_live_claim_enabled_for_offset(offset) {
        return;
    }
    let raw_mask = read_u32_le(live_bytes, offset + 6).unwrap_or(0);
    eprintln!(
        "live-object creature-update interleaved fragment span proof: offset={offset} raw_mask=0x{raw_mask:08X} read_end={read_end} old_record_end={old_record_end} span_bytes={} bits_promoted={bits_promoted} insertion_cursor={insertion_cursor} bit_cursor={start_bit_cursor}->{end_bit_cursor} bare_second_identity_rewrite={rewrite_bare_second_identity_string}",
        old_record_end.saturating_sub(read_end)
    );
}

fn debug_live_claim_enabled_for_offset(offset: usize) -> bool {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return false;
    }
    let Ok(filter) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM_OWNER_OFFSET") else {
        return true;
    };
    filter.split(',').any(|part| {
        part.trim()
            .parse::<usize>()
            .map(|wanted| wanted == offset)
            .unwrap_or(false)
    })
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
