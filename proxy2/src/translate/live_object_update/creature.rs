//! Creature-specific live-object update helpers.
//!
//! These helpers classify creature add/update record shapes and expose only
//! narrow, decompile-backed creature-family rewrites. The top-level dispatcher
//! still owns packet routing, declared-length repair, and fragment repacking.

use super::{bits, class_rows, read_f32_le, read_u16_le, read_u32_le};

const LEGACY_LIVE_CREATURE_UPDATE_ASSOCIATE_MASK: u32 = 0x0000_2000;
const LEGACY_CREATURE_UPDATE_3967_MASK: u32 = 0x0000_3967;
const LEGACY_CREATURE_UPDATE_C40F_MASK: u32 = 0x0000_C40F;
const LEGACY_CREATURE_UPDATE_C44F_MASK: u32 = 0x0000_C44F;
const LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES: [u8; 2] = [0, 0];
const LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_OFFSET: usize = 26;
const LEGACY_CREATURE_APPEARANCE_NAME_ONLY_MASK: u16 = 0x0400;
const MAX_CREATURE_UPDATE_ADJACENT_FRAGMENT_SPAN_BYTES: usize = 32;
const LEGACY_LIVE_CREATURE_UPDATE_UNSUPPORTED_FEATURE_MASK: u32 =
    0x0010_0000 | 0x0020_0000 | 0x0040_0000 | 0x0080_0000 | 0x0100_0000;
const SUPPORTED_LEGACY_CREATURE_UPDATE_CURSOR_MASK: u32 = 0x0000_0001
    | 0x0000_0002
    | 0x0000_0004
    | 0x0000_0008
    | 0x0000_0020
    | 0x0000_0040
    | 0x0000_0100
    | 0x0000_0200
    | 0x0000_0400
    | 0x0000_0800
    | 0x0000_1000
    | LEGACY_LIVE_CREATURE_UPDATE_ASSOCIATE_MASK
    | 0x0000_4000
    | 0x0000_8000
    | 0x0002_0000;

pub(super) const LEGACY_CREATURE_ADD_RECORD_BYTES: usize = 32;
pub(super) const EE_CREATURE_ADD_RECORD_BYTES: usize = LEGACY_CREATURE_ADD_RECORD_BYTES
    + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;

pub(super) fn looks_like_ee_creature_add_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let Some(visual_offset) = offset.checked_add(32) else {
        return false;
    };
    offset
        .checked_add(EE_CREATURE_ADD_RECORD_BYTES)
        .map(|expected_end| expected_end == record_end)
        .unwrap_or(false)
        && looks_like_legacy_creature_add_transform_fields(bytes, offset, record_end)
        && has_ee_identity_visual_transform_map_at(bytes, visual_offset, record_end)
}

pub(super) fn looks_like_legacy_creature_add_transform_fields(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if offset > bytes.len()
        || record_end > bytes.len()
        || record_end < offset + LEGACY_CREATURE_ADD_RECORD_BYTES
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    if object_id == u32::MAX || read_u16_le(bytes, offset + 30).is_none() {
        return false;
    }

    for index in 0..6 {
        // Diamond `sub_4489F0` and EE's creature-add writer consume these as
        // raw FLOAT slots. They do not reject NaN/sentinel bit patterns, so the
        // proxy validator must only prove the six fields are present.
        if read_f32_le(bytes, offset + 6 + index * 4).is_none() {
            return false;
        }
    }
    true
}

pub(super) fn has_ee_identity_visual_transform_map_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    super::visual_transform::has_ee_object_visual_transform_identity_at(bytes, offset, record_end)
}

pub(super) fn advance_verified_noop_creature_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    // EE's `HandleServerToPlayerGameObjectUpdate` dispatcher reads live-object
    // records sequentially and never retries a creature-update record from a
    // neighboring fragment cursor. This helper is used by final strict claims
    // and by transport tail repair, so it must prove the record from the exact
    // cursor the previous semantic reader left behind. Older development builds
    // allowed the HG `U/5 0x3967` probe to try `cursor +/- 1`; that could mark a
    // packet as verified even though the real EE reader would desynchronize and
    // report "Unknown Update sub-message" on the following byte.
    advance_verified_noop_creature_update_record_exact_cursor(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(super) fn advance_verified_noop_creature_update_record_exact_cursor(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
        return false;
    };
    if !looks_like_legacy_creature_object_id(object_id) {
        return false;
    }

    if raw_mask == 0x0000_C408 {
        return advance_verified_creature_update_c408(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
        );
    }
    if raw_mask == LEGACY_CREATURE_UPDATE_3967_MASK {
        let original_bit_cursor = *bit_cursor;
        if advance_verified_creature_update_3967_action0_ee_record(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
        ) {
            return true;
        }
        *bit_cursor = original_bit_cursor;
        if advance_verified_creature_update_3967_action_ffff_ee_record(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
        ) {
            return true;
        }
        *bit_cursor = original_bit_cursor;
        if advance_verified_creature_update_3967_action_fffd_ee_record(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
        ) {
            return true;
        }
        *bit_cursor = original_bit_cursor;
        if find_legacy_3967_action0_bridge_followup_removal(
            bytes,
            offset,
            record_end,
            fragment_bits,
            original_bit_cursor,
        )
        .is_some()
        {
            return false;
        }
    }

    let original_bit_cursor = *bit_cursor;
    let advanced = simulate_legacy_live_creature_update_cursors(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    );

    if !advanced {
        *bit_cursor = original_bit_cursor;
    }
    advanced
}

pub(super) fn advance_verified_legacy_creature_update_record_for_span_owner(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
        return false;
    };
    if raw_mask != LEGACY_CREATURE_UPDATE_3967_MASK
        || !looks_like_legacy_creature_object_id(object_id)
    {
        return false;
    }

    // This is not a final EE-shape validator. It is a narrow transport proof
    // for interleaved CNW fragment-span ownership: Local Diamond seq15 carries a
    // chunk-local fragment-storage span immediately before a legacy
    // `U/5 mask=0x3967` creature update. The span must be promoted before the
    // normal `U` translator can see and rewrite that record, so this helper
    // proves only the Diamond writer cursor shape. The post-rewrite exact claim
    // still owns the EE reader proof before the packet can be emitted.
    let original_bit_cursor = *bit_cursor;
    let advanced = simulate_legacy_live_creature_update_cursors(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    );
    if !advanced {
        *bit_cursor = original_bit_cursor;
    }
    advanced
}

pub(super) fn legacy_creature_update_3967_read_end_before_fragment_span(
    bytes: &[u8],
    offset: usize,
    old_record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    max_span_bytes: usize,
) -> Option<(usize, usize)> {
    if offset + 10 >= old_record_end
        || old_record_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(bytes, offset + 6)? != LEGACY_CREATURE_UPDATE_3967_MASK
    {
        return None;
    }

    let min_read_end = offset.checked_add(10)?;
    let scan_start = old_record_end
        .saturating_sub(max_span_bytes)
        .max(min_read_end);
    for read_end in scan_start..old_record_end {
        let mut proof_cursor = bit_cursor;
        if advance_verified_legacy_creature_update_record_for_span_owner(
            bytes,
            offset,
            read_end,
            fragment_bits,
            &mut proof_cursor,
        ) {
            return Some((read_end, proof_cursor));
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureC408CountRepair {
    pub entries: u16,
    pub bytes_rewritten: usize,
}

pub(super) fn repair_legacy_c408_visual_effect_count_for_ee(
    bytes: &mut [u8],
    offset: usize,
    record_end: usize,
) -> Option<CreatureC408CountRepair> {
    // Stock Diamond 1.69 and EE both write the `U/5 0x0000_C408` core as:
    //
    //   0x0008: WORD looping-visual-effect delta count, then count entries.
    //   0x0400: four signed SHORT scalar/status values.
    //   0x4000: five BOOLs, optional dominated/master details, then two BOOLs.
    //   0x8000: three visibility BOOLs.
    //
    // The seq36 HG quarantine contains the same three looping-effect entries
    // seen in the earlier valid fixture, but its count WORD is zero. That is
    // invalid for both stock writers: a zero count would leave the three
    // entry triplets to be misread as scalar fields and shift the following
    // `I` inventory record. Repair only this proven malformed shape, then let
    // the normal exact EE-shaped validator consume the record.
    const HG_C408_THREE_EFFECT_ENTRIES: [u8; 9] =
        [b'A', 0xB6, 0x00, b'A', 0xB1, 0x00, b'A', 0xFE, 0x07];
    const HG_C408_THREE_EFFECT_COUNT: u16 = 3;

    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(0x0000_C408)
    {
        return None;
    }

    let mut cursor = offset.checked_add(10)?;
    let count = usize::from(read_u16_le(bytes, cursor)?);
    if count != 0 {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    let entries_end = cursor.checked_add(HG_C408_THREE_EFFECT_ENTRIES.len())?;
    let scalar_end = entries_end.checked_add(8)?;
    if scalar_end != record_end || scalar_end > bytes.len() {
        return None;
    }
    if bytes.get(cursor..entries_end)? != HG_C408_THREE_EFFECT_ENTRIES {
        return None;
    }

    let count_bytes = HG_C408_THREE_EFFECT_COUNT.to_le_bytes();
    bytes[offset + 10] = count_bytes[0];
    bytes[offset + 11] = count_bytes[1];
    Some(CreatureC408CountRepair {
        entries: HG_C408_THREE_EFFECT_COUNT,
        bytes_rewritten: 2,
    })
}

pub(super) fn try_get_ee_creature_update_c408_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // EE `sub_140781E80` reaches the visual-effect update reader for the
    // `0x0008` branch, and current EE builds read an
    // `ObjectVisualTransformData` map after each three-byte effect delta. This
    // boundary helper exists only for already-EE-shaped C408 records after the
    // semantic rewriter has inserted those maps; compact Diamond records remain
    // owned by the legacy scanner/rewrite pass.
    if offset + 12 > scan_end
        || scan_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(bytes, offset + 6)? != 0x0000_C408
    {
        return None;
    }

    let count = usize::from(read_u16_le(bytes, offset + 10)?);
    if count == 0 || count > 256 {
        return None;
    }

    let mut cursor = offset.checked_add(12)?;
    for _ in 0..count {
        if scan_end.saturating_sub(cursor)
            < 3 + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
        {
            return None;
        }
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') || read_u16_le(bytes, cursor + 1).is_none() {
            return None;
        }
        cursor = cursor.checked_add(3)?;
        let after_visual_transform = cursor
            .checked_add(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
        if !super::visual_transform::has_ee_object_visual_transform_identity_at(
            bytes,
            cursor,
            after_visual_transform,
        ) {
            return None;
        }
        cursor = after_visual_transform;
    }

    cursor = cursor.checked_add(8)?;
    (cursor <= scan_end).then_some(cursor)
}

pub(super) fn try_get_ee_creature_update_c40f_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // C40F is the C408 self/status suffix with the lower position,
    // orientation, and action branches present. Diamond 1.69 writes those
    // lower branches before the status-effect list; EE then expects the same
    // per-effect identity maps as C408. Boundary detection does not own the
    // fragment cursor, so it tries only the two decompile-owned read-buffer
    // starts for the orientation branch: scalar one-byte orientation or vector
    // six-byte orientation. The final exact creature validator still proves
    // the fragment bits and chosen branch.
    if offset + 12 > scan_end
        || scan_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(bytes, offset + 6)? != LEGACY_CREATURE_UPDATE_C40F_MASK
    {
        return None;
    }

    let lower_prefix = offset.checked_add(10)?.checked_add(6)?;
    let action_branch_bytes = 6usize;
    let scalar_status_start = lower_prefix.checked_add(1)?.checked_add(action_branch_bytes)?;
    let vector_status_start = lower_prefix.checked_add(6)?.checked_add(action_branch_bytes)?;

    let mut accepted = None;
    for status_start in [scalar_status_start, vector_status_start] {
        if let Some(record_end) =
            try_get_ee_creature_status_effect_suffix_record_end(bytes, status_start, scan_end, 0)
        {
            accepted = match accepted {
                Some(existing) if existing != record_end => return None,
                Some(existing) => Some(existing),
                None => Some(record_end),
            };
        }
    }
    accepted
}

pub(super) fn try_get_ee_creature_update_c44f_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // C44F is the same lower movement/action + C408 self/status family as
    // C40F, with the additional decompile-owned `0x0040` creature state branch
    // between the visual-effect list and the scalar/status suffix. That branch
    // consumes WORD, BYTE, WORD, BYTE from the read buffer and one BOOL from
    // the fragment stream; the boundary scanner can prove only the six bytes,
    // while the exact creature validator proves the BOOL cursor.
    if offset + 12 > scan_end
        || scan_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(bytes, offset + 6)? != LEGACY_CREATURE_UPDATE_C44F_MASK
    {
        return None;
    }

    let lower_prefix = offset.checked_add(10)?.checked_add(6)?;
    let action_branch_bytes = 6usize;
    let scalar_status_start = lower_prefix.checked_add(1)?.checked_add(action_branch_bytes)?;
    let vector_status_start = lower_prefix.checked_add(6)?.checked_add(action_branch_bytes)?;

    let mut accepted = None;
    for status_start in [scalar_status_start, vector_status_start] {
        if let Some(record_end) =
            try_get_ee_creature_status_effect_suffix_record_end(bytes, status_start, scan_end, 6)
        {
            accepted = match accepted {
                Some(existing) if existing != record_end => return None,
                Some(existing) => Some(existing),
                None => Some(record_end),
            };
        }
    }
    accepted
}

fn try_get_ee_creature_status_effect_suffix_record_end(
    bytes: &[u8],
    status_start: usize,
    scan_end: usize,
    extra_read_bytes_before_scalar_suffix: usize,
) -> Option<usize> {
    if status_start + 2 > scan_end {
        return None;
    }
    let count = usize::from(read_u16_le(bytes, status_start)?);
    if count == 0 || count > 256 {
        return None;
    }

    let mut cursor = status_start.checked_add(2)?;
    for _ in 0..count {
        if scan_end.saturating_sub(cursor)
            < 3 + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
        {
            return None;
        }
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') || read_u16_le(bytes, cursor + 1).is_none() {
            return None;
        }
        cursor = cursor.checked_add(3)?;
        let after_visual_transform = cursor
            .checked_add(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
        if !super::visual_transform::has_ee_object_visual_transform_identity_at(
            bytes,
            cursor,
            after_visual_transform,
        ) {
            return None;
        }
        cursor = after_visual_transform;
    }

    cursor = cursor.checked_add(extra_read_bytes_before_scalar_suffix)?;
    cursor = cursor.checked_add(8)?;
    (cursor <= scan_end).then_some(cursor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureStatusEffectTransformRewrite {
    pub entries: u16,
    pub bytes_inserted: usize,
}

pub(super) fn insert_creature_update_status_effect_identity_maps_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<CreatureStatusEffectTransformRewrite> {
    // EE `HandleServerToPlayerUpdateVisualEffects` (`sub_1407B1F00`) is reached
    // from the creature update reader when mask `0x0008` is set
    // (`sub_140781E80+0x1126`). For current EE builds it reads an
    // `ObjectVisualTransformData` map after each visual-effect entry
    // (`0x1407B218F..0x1407B21AE`). Diamond/HG creature update writers emit the
    // compact count/opcode/row form. This compatibility rewrite is deliberately
    // transactional: locate the status-effect list with the same bounded cursor
    // model used by the final validator, insert identity maps, and commit only
    // if the full EE-shaped creature update then validates exactly.
    let raw_mask = read_u32_le(bytes, offset + 6)?;
    if (raw_mask & 0x0000_0008) == 0 {
        return None;
    }

    let start_states = legacy_creature_update_status_effect_start_states(
        bytes,
        offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    )?;
    if debug_creature_update_cursor_trace_enabled(raw_mask) {
        eprintln!(
            "live-object creature status-effect identity-map rewrite candidates: raw_mask=0x{raw_mask:08X} offset={offset} record_end={} start_states={}",
            *record_end,
            start_states.len()
        );
    }
    let mut accepted: Option<(Vec<u8>, usize, u16, usize)> = None;

    for state in start_states {
        let count = read_u16_le(bytes, state.read_cursor)?;
        if debug_creature_update_cursor_trace_enabled(raw_mask) {
            eprintln!(
                "live-object creature status-effect identity-map rewrite state: raw_mask=0x{raw_mask:08X} status_start={} bit_cursor={} count={count}",
                state.read_cursor,
                state.bit_cursor
            );
        }
        if count == 0 || count > 256 {
            continue;
        }
        let mut cursor = state.read_cursor.checked_add(2)?;
        let mut insert_offsets = Vec::with_capacity(usize::from(count));
        let mut already_ee_shaped = false;
        for _ in 0..usize::from(count) {
            if (*record_end).saturating_sub(cursor) < 3 {
                insert_offsets.clear();
                break;
            }
            cursor = cursor.checked_add(3)?;
            if super::visual_transform::has_ee_object_visual_transform_identity_at(
                bytes,
                cursor,
                *record_end,
            ) {
                cursor = cursor.checked_add(
                    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN,
                )?;
                already_ee_shaped = true;
                continue;
            }
            insert_offsets.push(cursor);
        }
        if insert_offsets.is_empty() || already_ee_shaped {
            continue;
        }
        if debug_creature_update_cursor_trace_enabled(raw_mask) {
            eprintln!(
                "live-object creature status-effect identity-map rewrite trial: raw_mask=0x{raw_mask:08X} insert_offsets={insert_offsets:?}"
            );
        }

        let mut candidate = bytes.clone();
        let mut candidate_record_end = *record_end;
        for insert_at in insert_offsets.iter().rev().copied() {
            candidate.splice(
                insert_at..insert_at,
                super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
            );
            candidate_record_end = candidate_record_end.checked_add(
                super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN,
            )?;
        }

        let mut candidate_bit_cursor = bit_cursor;
        let exact_full_record = advance_verified_noop_creature_update_record_exact_cursor(
            &candidate,
            offset,
            candidate_record_end,
            fragment_bits,
            &mut candidate_bit_cursor,
        );
        let exact_before_adjacent_span = !exact_full_record
            && matches!(
                raw_mask,
                LEGACY_CREATURE_UPDATE_3967_MASK
                    | LEGACY_CREATURE_UPDATE_C40F_MASK
                    | LEGACY_CREATURE_UPDATE_C44F_MASK
            )
            && creature_update_record_valid_before_adjacent_fragment_span(
                &candidate,
                offset,
                candidate_record_end,
                fragment_bits,
                bit_cursor,
            );
        if !exact_full_record && !exact_before_adjacent_span {
            if debug_creature_update_cursor_trace_enabled(raw_mask) {
                eprintln!(
                    "live-object creature status-effect identity-map rewrite rejected: raw_mask=0x{raw_mask:08X} candidate_record_end={candidate_record_end}"
                );
            }
            continue;
        }
        let bytes_inserted = insert_offsets.len()
            * super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;
        if let Some((accepted_candidate, accepted_record_end, accepted_count, accepted_inserted)) =
            accepted.as_ref()
        {
            if *accepted_record_end == candidate_record_end
                && *accepted_count == count
                && *accepted_inserted == bytes_inserted
                && accepted_candidate == &candidate
            {
                continue;
            }
            return None;
        }
        accepted = Some((
            candidate,
            candidate_record_end,
            count,
            bytes_inserted,
        ));
    }

    let (candidate, candidate_record_end, entries, bytes_inserted) = accepted?;
    *bytes = candidate;
    *record_end = candidate_record_end;
    Some(CreatureStatusEffectTransformRewrite {
        entries,
        bytes_inserted,
    })
}

fn creature_update_record_valid_before_adjacent_fragment_span(
    bytes: &[u8],
    offset: usize,
    old_record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> bool {
    // C40F/C44F Sooty transition captures can carry a short CNW fragment
    // storage span between the exact creature read cursor and the following
    // inventory submessage. This proof mirrors the stricter promotion pass:
    // the shortened creature record must validate with the EE cursor model,
    // and the suffix must decode as bounded CNW fragment-storage bytes before
    // the status-effect identity-map rewrite may commit.
    let Some(min_read_end) = offset.checked_add(10) else {
        return false;
    };
    if old_record_end > bytes.len() || min_read_end >= old_record_end {
        return false;
    }
    let scan_start = old_record_end
        .saturating_sub(MAX_CREATURE_UPDATE_ADJACENT_FRAGMENT_SPAN_BYTES)
        .max(min_read_end);

    for read_end in scan_start..old_record_end {
        let Some(span) = bytes.get(read_end..old_record_end) else {
            continue;
        };
        if !looks_like_adjacent_fragment_storage_span(span) {
            continue;
        }
        let mut proof_cursor = bit_cursor;
        if advance_verified_noop_creature_update_record_exact_cursor(
            bytes,
            offset,
            read_end,
            fragment_bits,
            &mut proof_cursor,
        ) {
            return true;
        }
    }
    false
}

fn looks_like_adjacent_fragment_storage_span(span: &[u8]) -> bool {
    if span.is_empty() || span.len() > MAX_CREATURE_UPDATE_ADJACENT_FRAGMENT_SPAN_BYTES {
        return false;
    }
    bits::decode_msb_valid_bits(span, 3).is_some_and(|decoded| decoded.len() >= 3)
}

pub(super) fn legacy_3967_update_was_already_consumed_to_cursor(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> bool {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return false;
    }

    // Full creature `P/5` appearance records can prove the immediately
    // following `U/5 0x3967` cursor as part of their decompile-shaped
    // following-update fence check. If the update pass later reaches that same
    // `U` with the cursor already sitting at the exact post-update bit, do not
    // mark the whole live-object stream unreliable. This is intentionally not a
    // passthrough: the earlier start cursor must be found in a small bounded
    // window, the focused `0x3967` simulator must consume the record exactly,
    // and exactly one prior cursor may land on the current cursor.
    const MAX_ALREADY_CONSUMED_REWIND_BITS: usize = 32;
    let start = bit_cursor.saturating_sub(MAX_ALREADY_CONSUMED_REWIND_BITS);
    let mut accepted = false;
    for candidate_start in start..bit_cursor {
        let mut candidate_cursor = candidate_start;
        if advance_verified_noop_creature_update_record_exact_cursor(
            bytes,
            offset,
            record_end,
            fragment_bits,
            &mut candidate_cursor,
        ) && candidate_cursor == bit_cursor
        {
            if accepted {
                return false;
            }
            accepted = true;
        }
    }
    accepted
}

pub(super) fn legacy_3967_update_was_already_consumed_from_cursor(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    start_bit_cursor: usize,
    bit_cursor: usize,
) -> bool {
    if start_bit_cursor >= bit_cursor
        || offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return false;
    }

    let mut candidate_cursor = start_bit_cursor;
    advance_verified_noop_creature_update_record_exact_cursor(
        bytes,
        offset,
        record_end,
        fragment_bits,
        &mut candidate_cursor,
    ) && candidate_cursor == bit_cursor
}

pub(super) fn advance_verified_translated_hg_creature_update_3967_omitted_action_code_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    // This validator is intentionally for the already-rewritten EE shape only.
    // EE `sub_140781E80` reads mask `0x0004` as FLOAT followed by an action-code
    // WORD (`0x140782CD4..0x140782CFE`) and later reads the action-state BYTE
    // (`0x140782FD1`). The HG Starcore5 capture can omit the action-code WORD;
    // that legacy shape must go through
    // `insert_3967_hg_action_ffff_omitted_code_for_ee` before any final strict
    // claim can accept it.
    advance_verified_creature_update_3967_action_ffff_ee_record(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

fn advance_verified_creature_update_3967_action_ffff_ee_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return false;
    }

    let start_bit_cursor = *bit_cursor;
    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor: *bit_cursor,
        fragment_bits,
    };

    if cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(18).is_none()
    {
        return false;
    }

    let Some(candidate_cursors) = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    ) else {
        return false;
    };

    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;
    for candidate in candidate_cursors {
        let Some(candidate) = simulate_creature_update_3967_action_ffff_ee_tail_cursor(candidate)
        else {
            continue;
        };
        let Some(candidate) = try_simulate_ee_creature_update_identity_optional_suffix(
            LEGACY_CREATURE_UPDATE_3967_MASK,
            candidate,
        ) else {
            continue;
        };
        if candidate.read_cursor != record_end {
            continue;
        }
        if accepted
            .as_ref()
            .is_none_or(|accepted| orientation_candidate_is_more_specific(candidate, *accepted))
        {
            accepted = Some(candidate);
        }
    }

    let Some(cursor) = accepted else {
        *bit_cursor = start_bit_cursor;
        return false;
    };
    trace_creature_update_cursor_accept(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor.read_cursor,
        start_bit_cursor,
        cursor.bit_cursor,
        record_end,
    );
    *bit_cursor = cursor.bit_cursor;
    true
}

fn simulate_creature_update_3967_action_ffff_ee_tail_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let portrait_row = cursor.read_u16()?;
    if portrait_row >= 0xFFFE {
        cursor.read_cresref()?;
    }

    let action_code = simulate_legacy_creature_update_action_branch(&mut cursor)?;
    if action_code != 0xFFFF {
        return None;
    }

    // EE reads this mask-0x0004 action-state byte at `0x140782FD1` after the
    // action-code branch, regardless of the non-special `0xFFFF` action code.
    cursor.read_u8()?;

    let branch_mode = {
        cursor.read_u16()?;
        cursor.read_u8()?
    };
    cursor.read_u16()?;
    cursor.read_u8()?;
    cursor.read_bool()?;
    if branch_mode == 2 {
        cursor.read_u32()?;
    }

    cursor.read_unsigned_bits(32)?;
    cursor.read_unsigned_bits(32)?;
    cursor.read_u8()?;
    Some(cursor)
}

fn advance_verified_creature_update_3967_action_fffd_ee_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return false;
    }

    let start_bit_cursor = *bit_cursor;
    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor: *bit_cursor,
        fragment_bits,
    };

    if cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(18).is_none()
    {
        return false;
    }

    let Some(candidate_cursors) = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    ) else {
        return false;
    };

    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;
    for candidate in candidate_cursors {
        let Some(candidate) = simulate_creature_update_3967_action_fffd_ee_tail_cursor(candidate)
        else {
            continue;
        };
        let Some(candidate) = try_simulate_ee_creature_update_identity_optional_suffix(
            LEGACY_CREATURE_UPDATE_3967_MASK,
            candidate,
        ) else {
            continue;
        };
        if candidate.read_cursor != record_end {
            continue;
        }
        if accepted
            .as_ref()
            .is_none_or(|accepted| orientation_candidate_is_more_specific(candidate, *accepted))
        {
            accepted = Some(candidate);
        }
    }

    let Some(cursor) = accepted else {
        *bit_cursor = start_bit_cursor;
        return false;
    };
    trace_creature_update_cursor_accept(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor.read_cursor,
        start_bit_cursor,
        cursor.bit_cursor,
        record_end,
    );
    *bit_cursor = cursor.bit_cursor;
    true
}

fn simulate_creature_update_3967_action_fffd_ee_tail_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let portrait_row = cursor.read_u16()?;
    if portrait_row >= 0xFFFE {
        cursor.read_cresref()?;
    }

    // Diamond server `0x445437..0x44546C` writes mask `0x0004` as an action
    // scalar followed by a WORD action code. EE `sub_140781E80` reads the same
    // pair at `0x140782CD4..0x140782CFE`. For non-special codes such as the
    // Starcore5 `0xFFFD` sentinel, neither side consumes a legacy movement
    // follow-up WORD; EE falls through to mask `0x0008`, then reads the
    // action-state BYTE at `0x140782FD1`.
    let action_code = simulate_legacy_creature_update_action_branch(&mut cursor)?;
    if action_code != 0xFFFD {
        return None;
    }
    if !simulate_ee_creature_update_status_effect_helper_cursor(&mut cursor) {
        return None;
    }
    cursor.read_u8()?;

    let branch_mode = {
        cursor.read_u16()?;
        cursor.read_u8()?
    };
    cursor.read_u16()?;
    cursor.read_u8()?;
    cursor.read_bool()?;
    if branch_mode == 2 {
        cursor.read_u32()?;
    }

    cursor.read_unsigned_bits(32)?;
    cursor.read_unsigned_bits(32)?;
    cursor.read_u8()?;
    Some(cursor)
}

fn advance_verified_creature_update_3967_action0_ee_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return false;
    }

    let start_bit_cursor = *bit_cursor;
    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor: *bit_cursor,
        fragment_bits,
    };

    if cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(18).is_none()
    {
        return false;
    }

    let Some(candidate_cursors) = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    ) else {
        return false;
    };

    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;
    for candidate in candidate_cursors {
        let Some(candidate) =
            simulate_creature_update_3967_action0_ee_tail_cursor(candidate)
        else {
            continue;
        };
        if candidate.read_cursor != record_end {
            continue;
        }
        if accepted
            .as_ref()
            .is_none_or(|accepted| orientation_candidate_is_more_specific(candidate, *accepted))
        {
            accepted = Some(candidate);
        }
    }

    let Some(cursor) = accepted else {
        *bit_cursor = start_bit_cursor;
        return false;
    };
    trace_creature_update_cursor_accept(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor.read_cursor,
        start_bit_cursor,
        cursor.bit_cursor,
        record_end,
    );
    *bit_cursor = cursor.bit_cursor;
    true
}

fn simulate_creature_update_3967_action0_ee_tail_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let cursor = simulate_creature_update_3967_action0_pre_identity_ee_cursor(cursor)?;
    try_simulate_ee_creature_update_identity_optional_suffix(LEGACY_CREATURE_UPDATE_3967_MASK, cursor)
}

fn simulate_creature_update_3967_action0_pre_identity_ee_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let portrait_row = cursor.read_u16()?;
    if portrait_row >= 0xFFFE {
        cursor.read_cresref()?;
    }

    let action_code = simulate_legacy_creature_update_action_branch(&mut cursor)?;
    if action_code != 0 {
        return None;
    }

    // EE `sub_140781E80` reads the mask-0x0004 action state byte after the
    // action-code branch, then falls through to the next enabled mask branch.
    // What action code 0 does not read is the legacy/HG movement follow-up
    // count WORD that Diamond bridge captures carry after that state byte.
    cursor.read_u8()?;

    let branch_mode = {
        cursor.read_u16()?;
        cursor.read_u8()?
    };
    cursor.read_u16()?;
    cursor.read_u8()?;
    cursor.read_bool()?;
    if branch_mode == 2 {
        cursor.read_u32()?;
    }

    cursor.read_unsigned_bits(32)?;
    cursor.read_unsigned_bits(32)?;
    cursor.read_u8()?;
    Some(cursor)
}

fn simulate_creature_update_3967_action0_missing_damage_pre_identity_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<(LegacyCreatureUpdateCursor<'_>, usize)> {
    let portrait_row = cursor.read_u16()?;
    if portrait_row >= 0xFFFE {
        cursor.read_cresref()?;
    }

    let action_code = simulate_legacy_creature_update_action_branch(&mut cursor)?;
    if action_code != 0 {
        return None;
    }

    // Companion to the legacy bridge-followup removal path above. Stock Diamond
    // and EE both write/read one damage BYTE for mask bit `0x0800`; the
    // Starcore5 action0 bridge capture reaches the identity branch immediately
    // after the two `0x0100` FLOATs. Insert a conservative zero damage byte only
    // when the following legacy identity/associate suffix proves the exact end.
    cursor.read_u8()?;

    let branch_mode = {
        cursor.read_u16()?;
        cursor.read_u8()?
    };
    cursor.read_u16()?;
    cursor.read_u8()?;
    cursor.read_bool()?;
    if branch_mode == 2 {
        cursor.read_u32()?;
    }

    cursor.read_unsigned_bits(32)?;
    cursor.read_unsigned_bits(32)?;
    let damage_insert_offset = cursor.read_cursor;
    Some((cursor, damage_insert_offset))
}

fn simulate_creature_update_3967_hg_action_ffff_omitted_code_pre_identity_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let portrait_row = cursor.read_u16()?;
    if portrait_row >= 0xFFFE {
        cursor.read_cresref()?;
    }

    // Stock Diamond `CNWSMessage::WriteGameObjUpdate_UpdateObject`
    // `0x445437..0x44546C` writes the action scalar and then the action-code
    // WORD for mask bit `0x0004`. The Starcore5 Sooty Crow `0x3967` capture is
    // a narrower HG server shape: it carries the scalar, while the following
    // bytes prove the decompiled `0x0040` creature-state branch starts where
    // the action-code WORD would be. Accept only this exact family, and only
    // when the later identity/associate suffix proves the whole record.
    cursor.read_unsigned_bits(32)?;
    if read_u16_le(cursor.bytes, cursor.read_cursor)? != 0xFFFF {
        return None;
    }

    let branch_mode = {
        cursor.read_u16()?;
        cursor.read_u8()?
    };
    cursor.read_u16()?;
    cursor.read_u8()?;
    cursor.read_bool()?;
    if branch_mode == 2 {
        cursor.read_u32()?;
    }

    cursor.read_unsigned_bits(32)?;
    cursor.read_unsigned_bits(32)?;
    cursor.read_u8()?;
    Some(cursor)
}

pub(super) fn repair_3967_action2_optional_float_bool_for_ee(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut [bool],
    bit_cursor: usize,
) -> bool {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return false;
    }

    let Some(optional_float_bit) = find_legacy_3967_action2_optional_float_bit_for_repair(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) else {
        return false;
    };
    let Some(bit) = fragment_bits.get_mut(optional_float_bit) else {
        return false;
    };
    if !*bit {
        return false;
    }
    *bit = false;
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967Action0BridgeFollowupRewrite {
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
    pub bits_inserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967Action0EeBuild1fBoolRewrite {
    pub bits_inserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967OmittedActionCodeRewrite {
    pub bytes_inserted: usize,
}

pub(super) fn insert_3967_hg_action_ffff_omitted_code_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<Creature3967OmittedActionCodeRewrite> {
    let insertion_offset = find_legacy_3967_hg_action_ffff_omitted_code_insertion(
        bytes.as_slice(),
        offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    )?;

    let mut trial_bytes = bytes.clone();
    trial_bytes.splice(insertion_offset..insertion_offset, [0xFF, 0xFF]);
    let trial_record_end = record_end.checked_add(2)?;
    let mut trial_bit_cursor = bit_cursor;
    if !advance_verified_creature_update_3967_action_ffff_ee_record(
        &trial_bytes,
        offset,
        trial_record_end,
        fragment_bits,
        &mut trial_bit_cursor,
    ) {
        return None;
    }

    *bytes = trial_bytes;
    *record_end = trial_record_end;
    Some(Creature3967OmittedActionCodeRewrite { bytes_inserted: 2 })
}

pub(super) fn remove_3967_action0_legacy_bridge_followup_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<Creature3967Action0BridgeFollowupRewrite> {
    let Some(removal_start) = find_legacy_3967_action0_bridge_followup_removal(
        bytes.as_slice(),
        offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    ) else {
        if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
            eprintln!(
                "live-object creature update 0x3967 action0 bridge followup not found: offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
            );
        }
        return None;
    };
    let removal_end = removal_start
        .checked_add(LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.len())?;

    let mut trial_bytes_after_removal = bytes.clone();
    trial_bytes_after_removal.drain(removal_start..removal_end);
    let trial_record_end_after_removal =
        record_end.checked_sub(LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.len())?;
    let (trial_bytes, trial_record_end, insert_bit, bytes_inserted, bytes_removed) = {
        let mut trial_bit_cursor = bit_cursor;
        if advance_verified_creature_update_3967_action0_ee_record(
            &trial_bytes_after_removal,
            offset,
            trial_record_end_after_removal,
            fragment_bits,
            &mut trial_bit_cursor,
        ) {
            (
                trial_bytes_after_removal,
                trial_record_end_after_removal,
                None,
                0,
                LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.len(),
            )
        } else if let Some((damage_insert_offset, _insert_bit)) =
            find_legacy_3967_action0_missing_damage_byte_ee_repair(
                &trial_bytes_after_removal,
                offset,
                trial_record_end_after_removal,
                fragment_bits,
                bit_cursor,
            )
        {
            let mut trial_bytes = trial_bytes_after_removal;
            trial_bytes.insert(damage_insert_offset, 0);
            let trial_record_end = trial_record_end_after_removal.checked_add(1)?;
            let mut trial_bit_cursor = bit_cursor;
            if !advance_verified_creature_update_3967_action0_ee_record(
                &trial_bytes,
                offset,
                trial_record_end,
                fragment_bits,
                &mut trial_bit_cursor,
            ) {
                return None;
            }
            (
                trial_bytes,
                trial_record_end,
                None,
                1,
                LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.len(),
            )
        } else if let Some((read_end, _insert_bit)) =
            find_legacy_3967_action0_reader_end_before_empty_fragment_storage(
                &trial_bytes_after_removal,
                offset,
                trial_record_end_after_removal,
                fragment_bits,
                bit_cursor,
            )
        {
            let mut trial_bytes = trial_bytes_after_removal;
            let trailing_storage_bytes = trial_record_end_after_removal.checked_sub(read_end)?;
            trial_bytes.drain(read_end..trial_record_end_after_removal);
            let mut trial_bit_cursor = bit_cursor;
            if !advance_verified_creature_update_3967_action0_ee_record(
                &trial_bytes,
                offset,
                read_end,
                fragment_bits,
                &mut trial_bit_cursor,
            ) {
                return None;
            }
            (
                trial_bytes,
                read_end,
                None,
                0,
                LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES
                    .len()
                    .saturating_add(trailing_storage_bytes),
            )
        } else if let Some(insert_bit) =
            find_legacy_3967_action0_missing_second_associate_bool_insert_bit(
                &trial_bytes_after_removal,
                offset,
                trial_record_end_after_removal,
                fragment_bits,
                bit_cursor,
            )
        {
            (
                trial_bytes_after_removal,
                trial_record_end_after_removal,
                Some(insert_bit),
                0,
                LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.len(),
            )
        } else {
            if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
                eprintln!(
                    "live-object creature update 0x3967 action0 bridge followup trial failed exact suffix repair: offset={offset} removal_start={removal_start} trial_record_end={trial_record_end_after_removal} bit_cursor={bit_cursor}"
                );
            }
            return None;
        }
    };
    let mut trial_fragment_bits = fragment_bits.clone();
    if let Some(insert_bit) = insert_bit {
        bits::insert_msb_bit(&mut trial_fragment_bits, insert_bit, false)?;
    }
    let mut trial_bit_cursor = bit_cursor;
    if !advance_verified_creature_update_3967_action0_ee_record(
        &trial_bytes,
        offset,
        trial_record_end,
        &trial_fragment_bits,
        &mut trial_bit_cursor,
    ) {
        if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
            eprintln!(
                "live-object creature update 0x3967 action0 bridge followup trial failed exact EE validator: offset={offset} removal_start={removal_start} insert_bit={insert_bit:?} trial_record_end={trial_record_end} trial_bit_cursor={trial_bit_cursor}"
            );
        }
        return None;
    }

    *bytes = trial_bytes;
    *fragment_bits = trial_fragment_bits;
    *record_end = trial_record_end;
    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        eprintln!(
            "live-object creature update 0x3967 action0 bridge followup removed: offset={offset} removal_start={removal_start} bytes_inserted={bytes_inserted} missing_associate_bool_insert_bit={insert_bit:?} new_record_end={record_end}"
        );
    }
    Some(Creature3967Action0BridgeFollowupRewrite {
        bytes_inserted,
        bytes_removed,
        bits_inserted: usize::from(insert_bit.is_some()),
    })
}

fn find_legacy_3967_action0_reader_end_before_empty_fragment_storage(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<(usize, usize)> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return None;
    }

    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor,
        fragment_bits,
    };
    cursor.read_unsigned_bits(16)?;
    cursor.read_unsigned_bits(16)?;
    cursor.read_unsigned_bits(18)?;

    let candidates = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    )?;
    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;
    for candidate in candidates {
        let Some(identity_start) =
            simulate_creature_update_3967_action0_pre_identity_ee_cursor(candidate)
        else {
            continue;
        };
        let Some(identity_candidates) =
            build_legacy_creature_update_identity_branch_candidate_states(identity_start)
        else {
            continue;
        };
        for mut suffix in identity_candidates {
            // Same decompile-owned associate suffix as
            // `find_legacy_3967_action0_empty_fragment_tail_takes_commands_insert_bit`.
            // This variant additionally allows a tiny all-zero CNW fragment
            // storage tail after the proven creature reader end. Those bytes are
            // not creature fields; they are removed only after
            // `empty_fragment_storage_tail_after_creature_update` proves the
            // storage header/payload is empty and the final EE cursor below
            // validates the shortened record.
            if suffix.read_u32().is_none()
                || suffix.read_u16().is_none()
                || suffix.read_bool().is_none()
                || suffix.read_bool().is_none()
            {
                continue;
            }
            if !empty_fragment_storage_tail_after_creature_update(
                bytes,
                suffix.read_cursor,
                record_end,
            ) {
                continue;
            }
            if accepted
                .as_ref()
                .is_none_or(|accepted| orientation_candidate_is_more_specific(suffix, *accepted))
            {
                accepted = Some(suffix);
            }
        }
    }

    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        eprintln!(
            "live-object creature update 0x3967 action0 empty-fragment-storage repair result: offset={offset} accepted={:?} record_end={record_end}",
            accepted.map(|cursor| (cursor.read_cursor, cursor.bit_cursor))
        );
    }

    accepted.map(|cursor| (cursor.read_cursor, cursor.bit_cursor))
}

fn find_legacy_3967_action0_missing_second_associate_bool_insert_bit(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return None;
    }

    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor,
        fragment_bits,
    };
    cursor.read_unsigned_bits(16)?;
    cursor.read_unsigned_bits(16)?;
    cursor.read_unsigned_bits(18)?;

    let candidates = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    )?;
    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;
    for candidate in candidates {
        let Some(identity_start) =
            simulate_creature_update_3967_action0_pre_identity_ee_cursor(candidate)
        else {
            continue;
        };
        let Some(identity_candidates) =
            build_legacy_creature_update_identity_branch_candidate_states(identity_start)
        else {
            continue;
        };
        for mut suffix in identity_candidates {
            // Diamond `sub_44ADD0` mask bit 0x2000 reads:
            //
            //   raw OBJECTID, WORD, BOOL, BOOL
            //
            // EE `sub_140781E80` consumes the same associate-state suffix in
            // the proxy-owned build-35 dialect. Some Starcore5 action0 bridge
            // spans end exactly after the first associate BOOL once the legacy
            // movement follow-up WORD has been removed. Insert a conservative
            // false second BOOL only when the read-buffer cursor is already at
            // the decompiled record end and the fragment cursor is exactly at
            // the current fragment stream end; the caller then proves the full
            // EE-shaped record with the inserted bit.
            if suffix.read_u32().is_none()
                || suffix.read_u16().is_none()
                || suffix.read_bool().is_none()
            {
                continue;
            }
            if suffix.read_cursor != record_end || suffix.bit_cursor != fragment_bits.len() {
                continue;
            }
            if accepted
                .as_ref()
                .is_none_or(|accepted| orientation_candidate_is_more_specific(suffix, *accepted))
            {
                accepted = Some(suffix);
            }
        }
    }

    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        eprintln!(
            "live-object creature update 0x3967 action0 missing-associate-bool repair result: offset={offset} accepted={:?} record_end={record_end}",
            accepted.map(|cursor| cursor.bit_cursor)
        );
    }

    accepted.map(|cursor| cursor.bit_cursor)
}

fn empty_fragment_storage_tail_after_creature_update(
    bytes: &[u8],
    read_end: usize,
    record_end: usize,
) -> bool {
    let Some(span) = bytes.get(read_end..record_end) else {
        return false;
    };
    if span.is_empty() || span.len() > 2 {
        return false;
    }
    let Some(decoded_bits) = bits::decode_msb_valid_bits(span, super::CNW_FRAGMENT_HEADER_BITS)
    else {
        return false;
    };
    decoded_bits
        .iter()
        .skip(super::CNW_FRAGMENT_HEADER_BITS)
        .all(|bit| !*bit)
}

fn find_legacy_3967_action0_missing_damage_byte_ee_repair(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<(usize, usize)> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return None;
    }

    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor,
        fragment_bits,
    };
    cursor.read_unsigned_bits(16)?;
    cursor.read_unsigned_bits(16)?;
    cursor.read_unsigned_bits(18)?;

    let candidates = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    )?;
    let mut accepted: Option<(usize, usize)> = None;
    for candidate in candidates {
        let Some((identity_start, damage_insert_offset)) =
            simulate_creature_update_3967_action0_missing_damage_pre_identity_cursor(candidate)
        else {
            continue;
        };
        let Some(suffix) = try_simulate_legacy_creature_update_identity_optional_suffix(
            LEGACY_CREATURE_UPDATE_3967_MASK,
            identity_start,
        ) else {
            continue;
        };
        if suffix.read_cursor != record_end {
            continue;
        }

        let repair = (damage_insert_offset, suffix.bit_cursor);
        if accepted.replace(repair).is_some_and(|old| old != repair) {
            return None;
        }
    }
    accepted
}

fn simulate_legacy_creature_update_3967_pre_identity_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let portrait_row = cursor.read_u16()?;
    if portrait_row >= 0xFFFE {
        cursor.read_cresref()?;
    }

    let action_code = simulate_legacy_creature_update_action_branch(&mut cursor)?;
    cursor.read_u8()?;
    if !simulate_legacy_creature_update_action_post_state_followup(
        &mut cursor,
        LEGACY_CREATURE_UPDATE_3967_MASK,
        action_code,
    ) {
        return None;
    }

    let branch_mode = {
        cursor.read_u16()?;
        cursor.read_u8()?
    };
    cursor.read_u16()?;
    cursor.read_u8()?;
    cursor.read_bool()?;
    if branch_mode == 2 {
        cursor.read_u32()?;
    }

    cursor.read_unsigned_bits(32)?;
    cursor.read_unsigned_bits(32)?;
    cursor.read_u8()?;
    Some(cursor)
}

fn find_legacy_3967_action0_bridge_followup_removal(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return None;
    }

    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor,
        fragment_bits,
    };

    if cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(18).is_none()
    {
        return None;
    }

    let candidates = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    )?;

    let mut accepted: Option<usize> = None;
    for mut candidate in candidates {
        let Some(portrait_row) = candidate.read_u16() else {
            continue;
        };
        if portrait_row >= 0xFFFE && candidate.read_cresref().is_none() {
            continue;
        }

        let Some(action_code) = simulate_legacy_creature_update_action_branch(&mut candidate)
        else {
            continue;
        };
        if action_code != 0 {
            continue;
        }

        // Diamond/HG bridge traffic can carry the EE-owned action-state byte
        // followed by a legacy zero movement-followup WORD after action code 0.
        // EE `sub_140781E80` reads the state byte for mask bit 0x0004, but does
        // not read the movement-followup WORD for non-movement action code 0:
        // after the state byte it falls through to the mask-0x0040
        // creature-state branch. The proxy may therefore remove exactly that
        // all-zero WORD, while preserving the state byte.
        if candidate.read_u8().is_none() {
            continue;
        }
        let removal_start = candidate.read_cursor;
        if removal_start
            != offset.checked_add(LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_OFFSET)?
        {
            continue;
        }
        let removal_end = removal_start
            .checked_add(LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.len())?;
        if removal_end > record_end {
            continue;
        }
        if bytes.get(removal_start..removal_end)
            != Some(LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.as_slice())
        {
            continue;
        }

        accepted = Some(accepted.map_or(removal_start, |existing| existing.min(removal_start)));
    }
    accepted
}

fn find_legacy_3967_hg_action_ffff_omitted_code_insertion(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return None;
    }

    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor,
        fragment_bits,
    };

    if cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(18).is_none()
    {
        return None;
    }

    let candidates = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    )?;

    let mut accepted: Option<usize> = None;
    for candidate in candidates {
        let Some(insertion_offset) =
            find_legacy_3967_hg_action_ffff_omitted_code_insertion_after_orientation(candidate)
        else {
            continue;
        };
        if accepted
            .replace(insertion_offset)
            .is_some_and(|old| old != insertion_offset)
        {
            return None;
        }
    }
    accepted
}

fn find_legacy_3967_hg_action_ffff_omitted_code_insertion_after_orientation(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<usize> {
    let portrait_row = cursor.read_u16()?;
    if portrait_row >= 0xFFFE {
        cursor.read_cresref()?;
    }

    cursor.read_unsigned_bits(32)?;
    let insertion_offset = cursor.read_cursor;
    if read_u16_le(cursor.bytes, insertion_offset)? != 0xFFFF {
        return None;
    }
    Some(insertion_offset)
}

fn find_legacy_3967_action2_optional_float_bit_for_repair(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor,
        fragment_bits,
    };

    if cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(18).is_none()
    {
        return None;
    }

    let candidates = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    )?;

    let mut accepted: Option<usize> = None;
    for mut candidate in candidates {
        let Some(portrait_row) = candidate.read_u16() else {
            continue;
        };
        if portrait_row >= 0xFFFE && candidate.read_cresref().is_none() {
            continue;
        }

        let Some(action_code) = simulate_legacy_creature_update_action_branch(&mut candidate)
        else {
            continue;
        };
        if action_code != 2 || candidate.read_u8().is_none() {
            continue;
        }
        let Some(followup_count) = candidate.read_u16() else {
            continue;
        };
        if followup_count != 1 {
            continue;
        }

        // EE/Diamond `sub_140781E80` / `sub_44ADD0` action-code 2 movement
        // follow-up reads this BOOL before the two 16-bit movement values. HG
        // legacy captures proving this branch use the false path; when the
        // existing bit is true, EE consumes four extra read-buffer bytes and
        // shifts the later identity branch into the wrong subobject. Only
        // repair the bit when the complete record then validates through the
        // same exact cursor proof used by strict live-object claiming.
        let optional_float_bit = candidate.bit_cursor;
        if fragment_bits.get(optional_float_bit).copied() != Some(true) {
            continue;
        }
        let mut trial_fragment_bits = fragment_bits.to_vec();
        trial_fragment_bits[optional_float_bit] = false;
        let mut trial_bit_cursor = bit_cursor;
        let exact_record = advance_verified_noop_creature_update_record_exact_cursor(
            bytes,
            offset,
            record_end,
            &trial_fragment_bits,
            &mut trial_bit_cursor,
        );
        let interleaved_span_record = if exact_record {
            false
        } else {
            super::fragment_spans::verified_creature_update_3967_read_end_before_interleaved_fragment_span(
                bytes,
                offset,
                record_end,
                &trial_fragment_bits,
                bit_cursor,
            )
            .is_some()
        };
        if !exact_record && !interleaved_span_record {
            continue;
        }
        if accepted.replace(optional_float_bit).is_some() {
            return None;
        }
    }
    accepted
}

fn advance_verified_creature_update_c408(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    // Decompile-backed stock EE creature state family after legacy/HG count
    // normalization:
    //
    //   U 05 <object-id> mask=0x0000_C408
    //   0x0008: WORD looping-effect delta count, then count entries with
    //           current-build ObjectVisualTransformData identity maps
    //   0x0400: four WORD scalar values
    //   0x4000: seven fragment BOOLs
    //   0x8000: three fragment BOOLs
    //
    // This validator intentionally accepts only the EE-shaped record. Captured
    // HG legacy records with a zero looping-effect count followed by the known
    // three encoded entries must first go through
    // `repair_legacy_c408_visual_effect_count_for_ee`; otherwise the packet
    // remains unclaimed instead of leaking a malformed read layout to the EE
    // client.
    let Some(mut cursor) = offset.checked_add(10) else {
        return false;
    };
    let Some(count_word) = read_u16_le(bytes, cursor) else {
        return false;
    };
    let count = usize::from(count_word);
    if count > 256 {
        return false;
    }
    let Some(after_count) = cursor.checked_add(2) else {
        return false;
    };
    cursor = after_count;
    for _ in 0..count {
        if record_end.saturating_sub(cursor)
            < 3 + super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
        {
            return false;
        }
        let Some(change_opcode) = bytes.get(cursor).copied() else {
            return false;
        };
        if !matches!(change_opcode, b'A' | b'D') || read_u16_le(bytes, cursor + 1).is_none() {
            return false;
        }
        let Some(after_status_entry) = cursor.checked_add(3) else {
            return false;
        };
        cursor = after_status_entry;
        if !super::visual_transform::has_ee_object_visual_transform_identity_at(
            bytes, cursor, record_end,
        ) {
            return false;
        }
        let Some(after_visual_transform) = cursor
            .checked_add(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)
        else {
            return false;
        };
        cursor = after_visual_transform;
    }
    let Some(remaining) = record_end.checked_sub(cursor) else {
        return false;
    };
    if remaining != 8 {
        return false;
    }
    let Some(after_scalars) = cursor.checked_add(8) else {
        return false;
    };
    cursor = after_scalars;
    if cursor != record_end {
        return false;
    }
    advance_fragment_bits(fragment_bits, bit_cursor, 10)
}

pub(super) fn advance_verified_noop_creature_appearance_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if offset + 8 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'P')
        || bytes.get(offset + 1).copied() != Some(0x05)
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    let Some(flags) = read_u16_le(bytes, offset + 6) else {
        return false;
    };
    if !looks_like_legacy_creature_object_id(object_id) {
        return false;
    }
    if flags != LEGACY_CREATURE_APPEARANCE_NAME_ONLY_MASK {
        return false;
    }

    // EE `CNWSMessage::WriteGameObjUpdate_UpdateAppearance` writes the same
    // P/creature header as Diamond: CHAR 'P', BYTE object type, object id, and
    // a WORD appearance-update mask. This fallback is intentionally narrow:
    // complex masks such as `0xFFFF` must be claimed by the focused appearance
    // parser, including visible-equipment item records, or quarantined. Without
    // this guard, a failed exact appearance parse could still be accepted while
    // advancing only the name bits, shifting the following `U/5` update.
    let original_bit_cursor = *bit_cursor;
    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 8,
        bit_cursor: *bit_cursor,
        fragment_bits,
    };

    let Some(locstring_pair_shape) = cursor.read_bool() else {
        *bit_cursor = original_bit_cursor;
        return false;
    };
    if locstring_pair_shape {
        if cursor.read_bool().is_none()
            || cursor.read_bool().is_none()
            || cursor.read_cexo_string().is_none()
            || cursor.read_cexo_string().is_none()
        {
            *bit_cursor = original_bit_cursor;
            return false;
        }
    } else if cursor.read_cexo_string().is_none() {
        *bit_cursor = original_bit_cursor;
        return false;
    }

    if cursor.read_cursor != record_end {
        *bit_cursor = original_bit_cursor;
        return false;
    }

    *bit_cursor = cursor.bit_cursor;
    true
}

fn looks_like_legacy_creature_object_id(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }
    if object_id <= 0x00FF_FFFF {
        // Diamond and EE reader code treats OBJECTID values as opaque DWORDs.
        // This predicate is only a false-positive guard before the typed
        // creature-update parser proves the exact mask/cursor shape. Local
        // Diamond seq15 uses compact creature id `0x000000FE` for both `A/5`
        // and following `U/5 0x3967`, so do not impose the older 0x1000 floor
        // here.
        return true;
    }
    matches!(
        object_id & 0xFF00_0000,
        // Diamond/EE readers treat object ids as opaque DWORDs; this list is
        // only a false-positive guard for live-object stream scanning. HG's
        // 2026-05-12 Starcore5 Sooty Crow capture proves a creature add/update
        // namespace at 0xACxxxxxx immediately after an exact `I/0x2B00`
        // inventory cursor, so allow the focused creature parser to validate
        // those records instead of rejecting the id before semantic proof.
        0x8000_0000 | 0x8800_0000 | 0xFF00_0000 | 0x0100_0000 | 0x0500_0000 | 0xAC00_0000
    )
}

fn simulate_legacy_live_creature_update_cursors(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let start_bit_cursor = *bit_cursor;
    let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
        trace_creature_update_cursor_reject("missing-mask", 0, offset + 6, *bit_cursor, record_end);
        return false;
    };
    if !is_supported_legacy_creature_update_cursor_mask(raw_mask) {
        trace_creature_update_cursor_reject(
            "unsupported-mask",
            raw_mask,
            offset + 10,
            *bit_cursor,
            record_end,
        );
        return false;
    }

    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor: *bit_cursor,
        fragment_bits,
    };

    if (raw_mask & 0x0000_0001) != 0
        && (cursor.read_unsigned_bits(16).is_none()
            || cursor.read_unsigned_bits(16).is_none()
            || cursor.read_unsigned_bits(18).is_none())
    {
        trace_creature_update_cursor_reject(
            "position",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return false;
    }

    let candidate_cursors = if (raw_mask & 0x0000_0002) != 0 {
        let Some(candidates) =
            build_legacy_creature_orientation_branch_candidate_states(raw_mask, cursor)
        else {
            return false;
        };
        candidates
    } else {
        vec![cursor]
    };

    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;
    for candidate in candidate_cursors {
        let Some(candidate) = simulate_legacy_live_creature_update_tail_cursor(raw_mask, candidate)
        else {
            continue;
        };
        if accepted
            .as_ref()
            .is_none_or(|accepted| orientation_candidate_is_more_specific(candidate, *accepted))
        {
            accepted = Some(candidate);
        }
    }
    let Some(cursor) = accepted else {
        return false;
    };

    trace_creature_update_cursor_accept(
        raw_mask,
        cursor.read_cursor,
        start_bit_cursor,
        cursor.bit_cursor,
        record_end,
    );
    *bit_cursor = cursor.bit_cursor;
    true
}

fn legacy_creature_update_status_effect_start_states<'a>(
    bytes: &'a [u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &'a [bool],
    bit_cursor: usize,
) -> Option<Vec<LegacyCreatureUpdateCursor<'a>>> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
    {
        return None;
    }
    let raw_mask = read_u32_le(bytes, offset + 6)?;
    if !is_supported_legacy_creature_update_cursor_mask(raw_mask) || (raw_mask & 0x0000_0008) == 0 {
        return None;
    }

    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor,
        fragment_bits,
    };

    if (raw_mask & 0x0000_0001) != 0 {
        cursor.read_unsigned_bits(16)?;
        cursor.read_unsigned_bits(16)?;
        cursor.read_unsigned_bits(18)?;
    }

    let candidate_cursors = if (raw_mask & 0x0000_0002) != 0 {
        build_legacy_creature_orientation_branch_candidate_states(raw_mask, cursor)?
    } else {
        vec![cursor]
    };

    let mut states = Vec::new();
    for mut candidate in candidate_cursors {
        if (raw_mask & 0x0000_0004) != 0 {
            let action_code = simulate_legacy_creature_update_action_branch(&mut candidate)?;
            if !legacy_creature_update_action_branch_omits_bridge_followup(raw_mask) {
                if candidate.read_u8().is_none()
                    || !simulate_legacy_creature_update_action_post_state_followup(
                        &mut candidate,
                        raw_mask,
                        action_code,
                    )
                {
                    continue;
                }
            }
        }
        states.push(candidate);
    }

    if states.is_empty() {
        None
    } else {
        Some(states)
    }
}

fn orientation_candidate_is_more_specific(
    candidate: LegacyCreatureUpdateCursor<'_>,
    accepted: LegacyCreatureUpdateCursor<'_>,
) -> bool {
    // The Diamond server writer has two exact 0x2-orientation shapes:
    // target-accessor omitted entirely, or target-accessor present with a
    // BOOL plus optional object id. If both full-record parses succeed, prefer
    // the more specific branch that consumed the explicit target subbranch.
    // The omitted branch remains available for records where that target
    // subbranch cannot itself produce a complete decompile-backed parse.
    (candidate.bit_cursor, candidate.read_cursor) > (accepted.bit_cursor, accepted.read_cursor)
}

fn build_legacy_creature_orientation_branch_candidate_states<'a>(
    raw_mask: u32,
    mut cursor: LegacyCreatureUpdateCursor<'a>,
) -> Option<Vec<LegacyCreatureUpdateCursor<'a>>> {
    let Some(vector_branch) = cursor.read_bool() else {
        trace_creature_update_cursor_reject(
            "orientation-branch-bit",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            cursor.record_end,
        );
        return None;
    };
    if vector_branch {
        if cursor.read_unsigned_bits(16).is_none()
            || cursor.read_unsigned_bits(16).is_none()
            || cursor.read_unsigned_bits(16).is_none()
        {
            trace_creature_update_cursor_reject(
                "orientation-vector",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                cursor.record_end,
            );
            return None;
        }
    } else if cursor.read_unsigned_bits(12).is_none() {
        trace_creature_update_cursor_reject(
            "orientation-scalar",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            cursor.record_end,
        );
        return None;
    }

    // Diamond server sub_44525B..sub_4453B3 writes the vector/scalar
    // orientation branch unconditionally, but only writes the target
    // BOOL/object-id subbranch when the server object exposes that target
    // accessor. Model both exact server-emitted shapes and let the full
    // bounded record parser below select a single complete parse.
    let mut candidates = vec![cursor];
    let mut target_cursor = cursor;
    if let Some(has_target) = target_cursor.read_bool() {
        if !has_target || target_cursor.read_u32().is_some() {
            candidates.push(target_cursor);
        } else {
            trace_creature_update_cursor_reject(
                "orientation-target-object",
                raw_mask,
                target_cursor.read_cursor,
                target_cursor.bit_cursor,
                target_cursor.record_end,
            );
        }
    }

    Some(candidates)
}

fn simulate_legacy_live_creature_update_tail_cursor(
    raw_mask: u32,
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let record_end = cursor.record_end;

    if (raw_mask & 0x0000_0020) != 0 {
        let Some(portrait_row) = cursor.read_u16() else {
            trace_creature_update_cursor_reject(
                "portrait-row",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        };
        if portrait_row >= 0xFFFE && cursor.read_cresref().is_none() {
            trace_creature_update_cursor_reject(
                "portrait-resref",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        }
    }

    if (raw_mask & 0x0000_0004) != 0 {
        let Some(action_code) = simulate_legacy_creature_update_action_branch(&mut cursor) else {
            trace_creature_update_cursor_reject(
                "action-branch",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        };
        // Starcore5 Sooty Crow `0xC40F` and `0xC44F` self/status captures
        // match EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` at
        // `loc_1404EBD88`: mask bit `0x0004` writes only the 32-bit action
        // scalar and 16-bit action code, plus the decompiled action-specific
        // subobjects parsed above, before falling through to the `0x0008`
        // status-effect list. `0xC44F` is the same family with the low
        // `0x0040` creature-state branch enabled later in the writer. The
        // older movement fixtures still carry the bridge-modelled post-state /
        // followup shape, so keep that parser for those proven masks instead of
        // broadening this exact self/status family.
        if !legacy_creature_update_action_branch_omits_bridge_followup(raw_mask) {
            if cursor.read_u8().is_none() {
                trace_creature_update_cursor_reject(
                    "action-state-byte",
                    raw_mask,
                    cursor.read_cursor,
                    cursor.bit_cursor,
                    record_end,
                );
                return None;
            }
            if !simulate_legacy_creature_update_action_post_state_followup(
                &mut cursor,
                raw_mask,
                action_code,
            ) {
                trace_creature_update_cursor_reject(
                    "action-followup",
                    raw_mask,
                    cursor.read_cursor,
                    cursor.bit_cursor,
                    record_end,
                );
                return None;
            }
        }
    }

    if (raw_mask & 0x0000_0008) != 0
        && !simulate_ee_creature_update_status_effect_helper_cursor(&mut cursor)
    {
        trace_creature_update_cursor_reject(
            "status-effects",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return None;
    }

    if (raw_mask & 0x0000_0040) != 0 {
        let Some(_first) = cursor.read_u16() else {
            trace_creature_update_cursor_reject(
                "branch40-first",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        };
        let Some(branch_mode) = cursor.read_u8() else {
            trace_creature_update_cursor_reject(
                "branch40-mode",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        };
        if cursor.read_u16().is_none() || cursor.read_u8().is_none() || cursor.read_bool().is_none()
        {
            trace_creature_update_cursor_reject(
                "branch40-body",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        }
        if branch_mode == 2 && cursor.read_u32().is_none() {
            trace_creature_update_cursor_reject(
                "branch40-object",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        }
    }

    if (raw_mask & 0x0000_0100) != 0
        && (cursor.read_unsigned_bits(32).is_none() || cursor.read_unsigned_bits(32).is_none())
    {
        trace_creature_update_cursor_reject(
            "branch100",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return None;
    }

    if (raw_mask & 0x0000_0200) != 0
        && (cursor.read_unsigned_bits(10).is_none() || cursor.read_unsigned_bits(10).is_none())
    {
        trace_creature_update_cursor_reject(
            "branch200",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return None;
    }

    if (raw_mask & 0x0000_0400) != 0 {
        for _ in 0..4 {
            if cursor.read_u16().is_none() {
                trace_creature_update_cursor_reject(
                    "branch400",
                    raw_mask,
                    cursor.read_cursor,
                    cursor.bit_cursor,
                    record_end,
                );
                return None;
            }
        }
    }

    if (raw_mask & 0x0002_0000) != 0 && cursor.read_u16().is_none() {
        trace_creature_update_cursor_reject(
            "branch20000",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return None;
    }

    if (raw_mask & 0x0000_0800) != 0 && cursor.read_u8().is_none() {
        trace_creature_update_cursor_reject(
            "branch800",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return None;
    }

    if (raw_mask & 0x0000_1000) != 0 {
        let Some(accepted) =
            try_simulate_legacy_creature_update_identity_optional_suffix(raw_mask, cursor)
        else {
            trace_creature_update_cursor_reject(
                "identity",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        };
        cursor = accepted;
    } else if !simulate_legacy_creature_update_suffix_after_identity(raw_mask, &mut cursor) {
        trace_creature_update_cursor_reject(
            "suffix",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return None;
    }

    if cursor.read_cursor != record_end {
        trace_creature_update_cursor_reject(
            "record-end",
            raw_mask,
            cursor.read_cursor,
            cursor.bit_cursor,
            record_end,
        );
        return None;
    }
    Some(cursor)
}

fn legacy_creature_update_action_branch_omits_bridge_followup(raw_mask: u32) -> bool {
    matches!(
        raw_mask,
        LEGACY_CREATURE_UPDATE_C40F_MASK | LEGACY_CREATURE_UPDATE_C44F_MASK
    )
}

fn trace_creature_update_cursor_reject(
    stage: &'static str,
    raw_mask: u32,
    read_cursor: usize,
    bit_cursor: usize,
    record_end: usize,
) {
    if !debug_creature_update_cursor_trace_enabled(raw_mask) {
        return;
    }
    eprintln!(
        "live-object creature update cursor rejected: stage={stage} raw_mask=0x{raw_mask:08X} read_cursor={read_cursor} bit_cursor={bit_cursor} record_end={record_end}"
    );
}

fn trace_creature_update_cursor_accept(
    raw_mask: u32,
    read_cursor: usize,
    start_bit_cursor: usize,
    bit_cursor: usize,
    record_end: usize,
) {
    if !debug_creature_update_cursor_trace_enabled(raw_mask) {
        return;
    }
    eprintln!(
        "live-object creature update cursor accepted: raw_mask=0x{raw_mask:08X} read_cursor={read_cursor} start_bit_cursor={start_bit_cursor} bit_cursor={bit_cursor} record_end={record_end}"
    );
}

fn debug_creature_update_cursor_trace_enabled(raw_mask: u32) -> bool {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return false;
    }
    let Ok(filter) = std::env::var("HGBRIDGE_PROXY2_DEBUG_CREATURE_UPDATE_MASK") else {
        return true;
    };
    filter.split(',').any(|part| {
        let part = part.trim();
        let parsed = part
            .strip_prefix("0x")
            .or_else(|| part.strip_prefix("0X"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| part.parse::<u32>().ok());
        parsed.map(|value| value == raw_mask).unwrap_or(false)
    })
}

pub(super) fn is_supported_legacy_creature_update_cursor_mask(raw_mask: u32) -> bool {
    raw_mask != 0
        && (raw_mask & LEGACY_LIVE_CREATURE_UPDATE_UNSUPPORTED_FEATURE_MASK) == 0
        && (raw_mask & !SUPPORTED_LEGACY_CREATURE_UPDATE_CURSOR_MASK) == 0
}

fn simulate_legacy_creature_update_status_effect_helper(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let mut cursor = offset + 10;
    let Some(count) = read_u16_le(bytes, cursor) else {
        return false;
    };
    if count > 256 {
        return false;
    }
    cursor += 2;

    for _ in 0..count {
        // Observed HG rows follow the feature-0x0E-false Diamond/EE legacy path:
        // compact status/effect opcode byte plus a 16-bit 2DA row. If a future
        // server row requires target-object payload, this exact validator fails
        // and quarantines the packet until that branch is decompile-backed.
        if record_end.saturating_sub(cursor) < 3 {
            return false;
        }
        cursor += 3;
    }

    cursor == record_end
}

fn simulate_ee_creature_update_status_effect_helper_cursor(
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
) -> bool {
    let Some(count) = cursor.read_u16() else {
        return false;
    };
    if count > 256 {
        return false;
    }

    for _ in 0..count {
        if cursor.advance_read(3).is_none() {
            return false;
        }
        if super::visual_transform::has_ee_object_visual_transform_identity_at(
            cursor.bytes,
            cursor.read_cursor,
            cursor.record_end,
        ) {
            if cursor
                .advance_read(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)
                .is_none()
            {
                return false;
            }
            continue;
        }

        let Some(after_compact_target) = cursor.read_cursor.checked_add(3) else {
            return false;
        };
        if !super::visual_transform::has_ee_object_visual_transform_identity_at(
            cursor.bytes,
            after_compact_target,
            cursor.record_end,
        ) {
            return false;
        }
        if cursor.advance_read(3).is_none()
            || cursor
                .advance_read(super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)
                .is_none()
        {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CreatureUpdateSuffixDialect {
    LegacyDiamond,
    EeBuild8193,
}

fn try_simulate_legacy_creature_update_identity_optional_suffix(
    raw_mask: u32,
    identity_start: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    try_simulate_creature_update_identity_optional_suffix(
        raw_mask,
        identity_start,
        CreatureUpdateSuffixDialect::LegacyDiamond,
    )
}

fn try_simulate_ee_creature_update_identity_optional_suffix(
    raw_mask: u32,
    identity_start: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    try_simulate_creature_update_identity_optional_suffix(
        raw_mask,
        identity_start,
        CreatureUpdateSuffixDialect::EeBuild8193,
    )
}

fn try_simulate_creature_update_identity_optional_suffix(
    raw_mask: u32,
    identity_start: LegacyCreatureUpdateCursor<'_>,
    dialect: CreatureUpdateSuffixDialect,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let candidates = build_legacy_creature_update_identity_branch_candidate_states(identity_start)?;
    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;

    for mut candidate in candidates {
        if !simulate_creature_update_suffix_after_identity(raw_mask, &mut candidate, dialect) {
            continue;
        }
        if candidate.read_cursor != candidate.record_end {
            continue;
        }
        if accepted.is_some() {
            return None;
        }
        accepted = Some(candidate);
    }

    accepted
}

fn build_legacy_creature_update_identity_branch_candidate_states(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<Vec<LegacyCreatureUpdateCursor<'_>>> {
    let debug_identity = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_IDENTITY_ROWS").is_some();
    let identity_start_read_cursor = cursor.read_cursor;
    cursor.read_u16()?;
    cursor.read_cexo_string()?;
    cursor.read_cexo_string()?;
    cursor.read_u8()?;
    cursor.read_u16()?;
    cursor.read_u16()?;
    cursor.read_bool()?;
    cursor.read_bool()?;
    let row_count = usize::from(cursor.read_u8()?);
    if row_count > 32 {
        if debug_identity {
            eprintln!(
                "live-object creature identity rows rejected: start_read_cursor={identity_start_read_cursor} row_count={row_count}"
            );
        }
        return None;
    }

    let mut states = vec![cursor];
    for _ in 0..row_count {
        let mut next = Vec::new();
        for state in states {
            let mut row_cursor = state;
            let Some(row_id) = row_cursor.read_u8() else {
                continue;
            };
            if row_cursor.read_u8().is_none() {
                continue;
            }

            let Some(optional_extra_byte_counts) =
                class_rows::creature_identity_row_optional_extra_byte_counts(row_id)
            else {
                if debug_identity {
                    eprintln!(
                        "live-object creature identity row rejected: start_read_cursor={identity_start_read_cursor} row_id=0x{row_id:02X} reason=unknown-optional-count state_read_cursor={}",
                        row_cursor.read_cursor
                    );
                }
                continue;
            };
            if debug_identity {
                eprintln!(
                    "live-object creature identity row candidates: start_read_cursor={identity_start_read_cursor} row_id=0x{row_id:02X} counts={optional_extra_byte_counts:?} state_read_cursor={}",
                    row_cursor.read_cursor
                );
            }
            for optional_extra_bytes in optional_extra_byte_counts {
                let mut candidate = row_cursor;
                if candidate
                    .advance_read(usize::from(*optional_extra_bytes))
                    .is_some()
                {
                    next.push(candidate);
                }
            }
        }
        if next.len() > 4096 {
            return None;
        }
        if next.is_empty() && debug_identity {
            eprintln!(
                "live-object creature identity rows exhausted: start_read_cursor={identity_start_read_cursor}"
            );
        }
        states = next;
    }
    Some(states)
}

fn simulate_legacy_creature_update_suffix_after_identity(
    raw_mask: u32,
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
) -> bool {
    simulate_creature_update_suffix_after_identity(
        raw_mask,
        cursor,
        CreatureUpdateSuffixDialect::LegacyDiamond,
    )
}

fn simulate_creature_update_suffix_after_identity(
    raw_mask: u32,
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
    dialect: CreatureUpdateSuffixDialect,
) -> bool {
    if (raw_mask & LEGACY_LIVE_CREATURE_UPDATE_ASSOCIATE_MASK) != 0 {
        if cursor.read_u32().is_none()
            || cursor.read_u16().is_none()
            || cursor.read_bool().is_none()
            || cursor.read_bool().is_none()
        {
            return false;
        }
        let _ = dialect;
    }

    if (raw_mask & 0x0000_4000) != 0 {
        // EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` at
        // `loc_1404ED5B5` writes this branch as five status BOOLs, then an
        // optional dominated/master detail block guarded by the fifth BOOL,
        // then two final BOOLs. The previous parser incorrectly treated the
        // second BOOL as the optional-string guard, which shifted the read
        // cursor into the following live-object `I` record for HG
        // `0x0000_C408` creature updates.
        let Some(_possessed_or_custom_ai) = cursor.read_bool() else {
            return false;
        };
        let Some(_subtle_visibility_flag) = cursor.read_bool() else {
            return false;
        };
        let Some(_singleton_party) = cursor.read_bool() else {
            return false;
        };
        let Some(_party_leader) = cursor.read_bool() else {
            return false;
        };
        let Some(has_master_detail_strings) = cursor.read_bool() else {
            return false;
        };
        if has_master_detail_strings {
            // The optional branch writes OBJECTID plus two
            // `WriteCExoLocStringServer` values. We have not yet captured that
            // legacy shape in HG traffic, so keep strict mode honest: packets
            // taking this branch remain unclaimed until a decompile-backed
            // locstring-server cursor parser is added.
            return false;
        }
        if cursor.read_bool().is_none() || cursor.read_bool().is_none() {
            return false;
        }
    }

    if (raw_mask & 0x0000_8000) != 0 {
        for _ in 0..3 {
            if cursor.read_bool().is_none() {
                return false;
            }
        }
    }

    true
}

fn simulate_legacy_creature_update_mask_0x47(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let mut cursor = LegacyCreatureUpdateCursor {
        bytes,
        record_end,
        read_cursor: offset + 10,
        bit_cursor: *bit_cursor,
        fragment_bits,
    };

    if cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(16).is_none()
        || cursor.read_unsigned_bits(18).is_none()
    {
        return false;
    }

    let Some(vector_branch) = cursor.read_bool() else {
        return false;
    };
    if vector_branch {
        if cursor.read_unsigned_bits(16).is_none()
            || cursor.read_unsigned_bits(16).is_none()
            || cursor.read_unsigned_bits(16).is_none()
        {
            return false;
        }
    } else if cursor.read_unsigned_bits(12).is_none() {
        return false;
    }

    let Some(has_target) = cursor.read_bool() else {
        return false;
    };
    if has_target && cursor.read_u32().is_none() {
        return false;
    }

    let Some(action_code) = simulate_legacy_creature_update_action_branch(&mut cursor) else {
        return false;
    };
    if cursor.read_u8().is_none() {
        return false;
    }
    if !simulate_legacy_creature_update_action_post_state_followup(
        &mut cursor,
        0x0000_0047,
        action_code,
    ) {
        return false;
    }

    let Some(_first) = cursor.read_u16() else {
        return false;
    };
    let Some(branch_mode) = cursor.read_u8() else {
        return false;
    };
    if cursor.read_u16().is_none() || cursor.read_u8().is_none() || cursor.read_bool().is_none() {
        return false;
    }
    if branch_mode == 2 && cursor.read_u32().is_none() {
        return false;
    }

    if cursor.read_cursor != record_end {
        return false;
    }
    *bit_cursor = cursor.bit_cursor;
    true
}

fn simulate_legacy_creature_update_action_branch(
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
) -> Option<u16> {
    cursor.read_unsigned_bits(32)?;
    cursor.read_unsigned_bits(16)?;
    let action_code = read_u16_le(cursor.bytes, cursor.read_cursor.checked_sub(2)?)?;
    trace_creature_update_action_branch(action_code, cursor.read_cursor, cursor.bit_cursor);

    if action_code == 9 {
        let attack_count = cursor.read_unsigned_bits(2)?;
        if attack_count > 3 {
            return None;
        }
        for _ in 0..attack_count {
            cursor.read_u32()?;
            cursor.read_unsigned_bits(16)?;
            cursor.read_unsigned_bits(16)?;
            cursor.read_unsigned_bits(16)?;
            cursor.read_unsigned_bits(4)?;
            cursor.read_unsigned_bits(16)?;
            cursor.read_unsigned_bits(9)?;
            cursor.read_bool()?;
            cursor.read_bool()?;
            cursor.read_unsigned_bits(4)?;
        }
        return Some(action_code);
    }

    if (0x0F..=0x14).contains(&action_code) || action_code == 0x3D {
        cursor.read_unsigned_bits(32)?;
        cursor.read_u32()?;
        cursor.read_bool()?;
        if (0x11..=0x14).contains(&action_code) || action_code == 0x3D {
            let mode = cursor.read_u8()?;
            if mode == 1 {
                cursor.read_u32()?;
            } else if mode == 2 {
                cursor.read_unsigned_bits(32)?;
                cursor.read_unsigned_bits(32)?;
                cursor.read_unsigned_bits(32)?;
            }
            cursor.read_unsigned_bits(32)?;
        }
    }

    Some(action_code)
}

fn simulate_legacy_creature_update_action_post_state_followup(
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
    raw_mask: u32,
    action_code: u16,
) -> bool {
    let start_read_cursor = cursor.read_cursor;
    let start_bit_cursor = cursor.bit_cursor;
    let Some(followup_count) = cursor.read_u16() else {
        return false;
    };
    trace_creature_update_action_followup(
        action_code,
        followup_count,
        start_read_cursor,
        start_bit_cursor,
        cursor.read_cursor,
        cursor.bit_cursor,
    );
    if followup_count > 256 {
        return false;
    }
    if !is_legacy_creature_update_movement_followup_action(action_code) {
        return followup_count == 0;
    }

    // Diamond and EE both read the action-code movement guard before walking
    // the per-point coordinate list.  HG Starcore5 captures prove the guard can
    // be present with a zero follow-up count; returning early on count zero
    // left the cursor four bytes before the decompiled identity branch and made
    // the following `U/5 0x3967` record look malformed.
    let Some(has_extra_float) = cursor.read_bool() else {
        return false;
    };
    if has_extra_float && cursor.read_unsigned_bits(32).is_none() {
        return false;
    }
    if followup_count == 0 {
        if action_code == 4 && (raw_mask & 0x0000_0040) != 0 {
            // Diamond/EE both classify action code 4 as a movement follow-up
            // family. The 2026-05-12 Starcore5 Sooty Crow `U/5 0x47`
            // capture proves the zero-count action-4 shape can still carry
            // one implicit 2D point before the decompile-owned six-byte
            // `0x0040` state tail. Keep this narrow: only consume that point
            // when enough read bytes remain for the point plus the state tail,
            // and let the final exact record cursor prove the whole shape.
            let remaining = cursor.record_end.saturating_sub(cursor.read_cursor);
            if remaining >= 10 {
                let original = *cursor;
                if cursor.read_unsigned_bits(16).is_some()
                    && cursor.read_unsigned_bits(16).is_some()
                {
                    return true;
                }
                *cursor = original;
            }
        }
        return true;
    }

    for _ in 0..followup_count {
        if cursor.read_unsigned_bits(16).is_none() || cursor.read_unsigned_bits(16).is_none() {
            return false;
        }
    }
    true
}

fn trace_creature_update_action_branch(action_code: u16, read_cursor: usize, bit_cursor: usize) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object creature update action branch: action_code=0x{action_code:04X} read_cursor={read_cursor} bit_cursor={bit_cursor}"
    );
}

fn trace_creature_update_action_followup(
    action_code: u16,
    followup_count: u16,
    start_read_cursor: usize,
    start_bit_cursor: usize,
    read_cursor: usize,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object creature update action followup: action_code=0x{action_code:04X} followup_count={followup_count} start_read_cursor={start_read_cursor} start_bit_cursor={start_bit_cursor} read_cursor={read_cursor} bit_cursor={bit_cursor}"
    );
}

fn is_legacy_creature_update_movement_followup_action(action_code: u16) -> bool {
    matches!(action_code, 2 | 3 | 4 | 0x4E | 0x4F) || (0x54..=0x57).contains(&action_code)
}

fn advance_fragment_bits(bits: &[bool], bit_cursor: &mut usize, count: usize) -> bool {
    if count > bits.len() || *bit_cursor > bits.len().saturating_sub(count) {
        return false;
    }
    *bit_cursor += count;
    true
}

#[derive(Clone, Copy)]
struct LegacyCreatureUpdateCursor<'a> {
    bytes: &'a [u8],
    record_end: usize,
    read_cursor: usize,
    bit_cursor: usize,
    fragment_bits: &'a [bool],
}

impl LegacyCreatureUpdateCursor<'_> {
    fn advance_read(&mut self, count: usize) -> Option<()> {
        if count > self.record_end.checked_sub(self.read_cursor)? {
            return None;
        }
        self.read_cursor = self.read_cursor.checked_add(count)?;
        Some(())
    }

    fn read_u8(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.read_cursor)?;
        self.read_cursor = self.read_cursor.checked_add(1)?;
        Some(value)
    }

    fn read_u16(&mut self) -> Option<u16> {
        if self.record_end.saturating_sub(self.read_cursor) < 2 {
            return None;
        }
        let value = read_u16_le(self.bytes, self.read_cursor)?;
        self.read_cursor = self.read_cursor.checked_add(2)?;
        Some(value)
    }

    fn read_u32(&mut self) -> Option<u32> {
        if self.record_end.saturating_sub(self.read_cursor) < 4 {
            return None;
        }
        let value = read_u32_le(self.bytes, self.read_cursor)?;
        self.read_cursor = self.read_cursor.checked_add(4)?;
        Some(value)
    }

    fn read_cresref(&mut self) -> Option<()> {
        self.advance_read(16)
    }

    fn read_cexo_string(&mut self) -> Option<()> {
        let len = usize::try_from(self.read_u32()?).ok()?;
        if len > 4096 {
            return None;
        }
        self.advance_read(len)
    }

    fn read_bool(&mut self) -> Option<bool> {
        Some(self.read_fragment_bits(1)? != 0)
    }

    fn read_fragment_bits(&mut self, count: usize) -> Option<u64> {
        if count > 64 || count > self.fragment_bits.len().checked_sub(self.bit_cursor)? {
            return None;
        }
        let mut value = 0u64;
        for _ in 0..count {
            value = (value << 1) | u64::from(self.fragment_bits[self.bit_cursor]);
            self.bit_cursor += 1;
        }
        Some(value)
    }

    fn read_unsigned_bits(&mut self, bit_count: u8) -> Option<u64> {
        let mut value = 0u64;
        let mut remaining = bit_count;
        while remaining >= 32 {
            value = (value << 32) | u64::from(self.read_u32()?);
            remaining -= 32;
        }
        while remaining >= 16 {
            value = (value << 16) | u64::from(self.read_u16()?);
            remaining -= 16;
        }
        while remaining >= 8 {
            value = (value << 8) | u64::from(self.read_u8()?);
            remaining -= 8;
        }
        if remaining != 0 {
            value = (value << remaining) | self.read_fragment_bits(usize::from(remaining))?;
        }
        Some(value)
    }
}
