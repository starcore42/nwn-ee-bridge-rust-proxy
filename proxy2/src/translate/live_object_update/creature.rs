//! Creature-specific live-object update helpers.
//!
//! These helpers classify creature add/update record shapes and expose only
//! narrow, decompile-backed creature-family rewrites. The top-level dispatcher
//! still owns packet routing, declared-length repair, and fragment repacking.

use super::{bits, class_rows, read_f32_le, read_u16_le, read_u32_le, visual_effect_rows};

const LEGACY_LIVE_CREATURE_UPDATE_ASSOCIATE_MASK: u32 = 0x0000_2000;
const LEGACY_CREATURE_UPDATE_0067_MASK: u32 = 0x0000_0067;
const LEGACY_CREATURE_UPDATE_3967_MASK: u32 = 0x0000_3967;
const LEGACY_CREATURE_UPDATE_C40F_MASK: u32 = 0x0000_C40F;
const LEGACY_CREATURE_UPDATE_C44F_MASK: u32 = 0x0000_C44F;
const LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK: u32 = 0x0000_0008;
const VFX_DUR_LOWLIGHTVISION_ROW: u16 = 0x00F3;
const CREATURE_STATUS_EFFECT_TARGET_PAYLOAD_BYTES: usize = 5;
const MAX_CREATURE_STATUS_EFFECT_TARGET_AMBIGUITY_PROBE_ENTRIES_WITHOUT_2DA: usize = 1;
const EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN: usize =
    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;
const LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES: [u8; 2] = [0, 0];
const LEGACY_CREATURE_UPDATE_3967_ACTION0_WOE_BRIDGE_BLOCK_BYTES: [u8; 11] =
    [0, 0, 0, 0, 0x80, 0x3F, 1, 0, 0, 0, 0];
const LEGACY_CREATURE_APPEARANCE_NOOP_MASK: u16 = 0x0000;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompactStatusEffectNoTablePolicy {
    AssumeNoTarget,
    KnownFeature0eFalseRowsOnly,
}

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

pub(super) fn verified_creature_update_claim_for_ee(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    expected_next_bit_cursor: usize,
) -> Option<super::LiveObjectCreatureUpdateClaim> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != super::CREATURE_OBJECT_TYPE
    {
        return None;
    }

    let raw_mask = read_u32_le(bytes, offset + 6)?;
    let mut proof_cursor = bit_cursor;
    if !advance_verified_noop_creature_update_record_exact_cursor(
        bytes,
        offset,
        record_end,
        fragment_bits,
        &mut proof_cursor,
    ) || proof_cursor != expected_next_bit_cursor
    {
        return None;
    }

    // Diamond and EE read the creature-update mask before the ordered optional
    // body. The `0x0001` position branch consumes six read-buffer bytes plus
    // two fragment bits; the `0x0002` orientation branch then reads one
    // scalar/vector selector BOOL. Reuse the exact validator above for the
    // complete record, then expose only these decompile-fixed cursor facts.
    let has_position = (raw_mask & 0x0000_0001) != 0;
    let position_bit_cursor = has_position.then_some(bit_cursor);
    let orientation_bit_cursor = if (raw_mask & 0x0000_0002) != 0 {
        Some(bit_cursor + if has_position { 2 } else { 0 })
    } else {
        None
    };
    let orientation_source = if let Some(cursor) = orientation_bit_cursor {
        Some(if fragment_bits.get(cursor).copied()? {
            super::LiveObjectRecordOrientationSource::Vector
        } else {
            super::LiveObjectRecordOrientationSource::Scalar
        })
    } else {
        None
    };

    Some(super::LiveObjectCreatureUpdateClaim {
        raw_mask,
        has_position,
        position_bit_cursor,
        orientation_source,
        orientation_bit_cursor,
    })
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

    if raw_mask == 0 {
        // Diamond and EE both read the creature update mask before walking the
        // ordered optional branches. A zero mask consumes no read-buffer body
        // and no CNW fragment bits, so it is an exact no-op record once the
        // ten-byte `U/5` header+mask boundary is proven. Lifecycle cleanup may
        // still remove it when the target object is absent on the EE side.
        return record_end == offset + 10;
    }

    if raw_mask == LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK {
        return advance_verified_legacy_feature0e_false_effect_only_update_record(
            bytes, offset, record_end,
        );
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
        let mut trial_live_bytes = bytes.to_vec();
        let mut trial_record_end = record_end;
        let mut trial_fragment_bits = fragment_bits.to_vec();
        if remove_3967_action0_legacy_bridge_followup_for_ee(
            &mut trial_live_bytes,
            offset,
            &mut trial_record_end,
            &mut trial_fragment_bits,
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
    if !is_appearance_following_creature_span_owner_mask(raw_mask)
        || !looks_like_legacy_creature_object_id(object_id)
    {
        return false;
    }

    // This is not a final EE-shape validator. It is a narrow transport proof
    // for interleaved CNW fragment-span ownership. Local Diamond captures carry
    // chunk-local fragment-storage spans immediately before creature updates:
    // the older HG path used `U/5 mask=0x3967`, while Prelude seq10 proves the
    // same transport shape before `U/5 mask=0x67` (position/orientation/action,
    // portrait WORD, and low 0x0040 creature-state branch). The span must be
    // promoted before the normal `U` translator can see and rewrite that record,
    // so this helper proves only the Diamond writer cursor shape. The
    // post-rewrite exact claim still owns the EE reader proof before the packet
    // can be emitted.
    let original_bit_cursor = *bit_cursor;
    let advanced = simulate_legacy_live_creature_update_cursors(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    );
    if !advanced {
        let mut trial = bytes.to_vec();
        if let Some(identity_rewrite) = rewrite_3967_bare_second_identity_string_for_ee(
            &mut trial,
            offset,
            record_end,
            fragment_bits,
            original_bit_cursor,
        ) {
            *bit_cursor = identity_rewrite.advanced_bit_cursor;
            return true;
        }
        *bit_cursor = original_bit_cursor;
    }
    advanced
}

pub(super) fn is_appearance_following_creature_span_owner_mask(raw_mask: u32) -> bool {
    matches!(
        raw_mask,
        LEGACY_CREATURE_UPDATE_0067_MASK | LEGACY_CREATURE_UPDATE_3967_MASK
    )
}

pub(super) fn legacy_creature_update_read_end_before_fragment_span_for_span_owner(
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
    {
        return None;
    }
    let raw_mask = read_u32_le(bytes, offset + 6)?;
    if !is_appearance_following_creature_span_owner_mask(raw_mask) {
        return None;
    }

    let min_read_end = offset.checked_add(10)?;
    let scan_start = old_record_end
        .saturating_sub(max_span_bytes)
        .max(min_read_end);
    let mut accepted = None;
    for read_end in scan_start..old_record_end {
        let mut proof_cursor = bit_cursor;
        if advance_verified_legacy_creature_update_record_for_span_owner(
            bytes,
            offset,
            read_end,
            fragment_bits,
            &mut proof_cursor,
        ) {
            accepted = Some((read_end, proof_cursor));
            break;
        }
    }
    if debug_creature_update_cursor_trace_enabled(raw_mask) {
        eprintln!(
            "live-object creature update span-owner scan: offset={offset} raw_mask=0x{raw_mask:08X} old_record_end={old_record_end} scan_start={scan_start} max_span_bytes={max_span_bytes} start_bit_cursor={bit_cursor} accepted_read_end={:?} accepted_bit_cursor={:?} accepted_span_bytes={:?}",
            accepted.map(|(read_end, _)| read_end),
            accepted.map(|(_, cursor)| cursor),
            accepted.map(|(read_end, _)| old_record_end.saturating_sub(read_end))
        );
    }
    accepted
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

    legacy_creature_update_read_end_before_fragment_span_for_span_owner(
        bytes,
        offset,
        old_record_end,
        fragment_bits,
        bit_cursor,
        max_span_bytes,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureC408CountRepair {
    pub entries: u16,
    pub bytes_rewritten: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureStatusEffectZeroCountRepair {
    pub bytes_inserted: usize,
}

fn infer_compact_legacy_status_effect_row_count_with_policy(
    bytes: &[u8],
    mut cursor: usize,
    entries_end: usize,
    loaded_rows: Option<&[Option<usize>]>,
    no_table_policy: CompactStatusEffectNoTablePolicy,
) -> Option<u16> {
    if cursor >= entries_end || entries_end > bytes.len() {
        return None;
    }

    let mut count = 0u16;
    while cursor < entries_end {
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        let row = read_u16_le(bytes, cursor + 1)?;
        if !compact_status_effect_row_has_no_target_payload(row, loaded_rows, no_table_policy) {
            return None;
        }
        cursor = cursor.checked_add(3)?;
        if cursor > entries_end {
            return None;
        }
        count = count.checked_add(1)?;
    }

    (count > 0 && count <= 256).then_some(count)
}

fn compact_status_effect_row_has_no_target_payload(
    row: u16,
    loaded_rows: Option<&[Option<usize>]>,
    no_table_policy: CompactStatusEffectNoTablePolicy,
) -> bool {
    if let Some(rows) = loaded_rows {
        return visual_effect_rows::target_payload_bytes_for_loaded_row(rows, row) == Some(0);
    }

    match no_table_policy {
        CompactStatusEffectNoTablePolicy::AssumeNoTarget => true,
        CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly => {
            legacy_feature0e_false_known_no_target_row(row)
        }
    }
}

fn repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee(
    bytes: &mut [u8],
    offset: usize,
    record_end: usize,
    raw_mask: u32,
    suffix_read_bytes: usize,
) -> Option<CreatureC408CountRepair> {
    repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee_with_policy(
        bytes,
        offset,
        record_end,
        raw_mask,
        suffix_read_bytes,
        CompactStatusEffectNoTablePolicy::AssumeNoTarget,
    )
}

fn repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee_with_policy(
    bytes: &mut [u8],
    offset: usize,
    record_end: usize,
    raw_mask: u32,
    suffix_read_bytes: usize,
    no_table_policy: CompactStatusEffectNoTablePolicy,
) -> Option<CreatureC408CountRepair> {
    let loaded_rows = visual_effect_rows::loaded_visual_effect_target_payload_bytes();
    repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee_with_rows(
        bytes,
        offset,
        record_end,
        raw_mask,
        suffix_read_bytes,
        loaded_rows.as_deref(),
        no_table_policy,
    )
}

fn repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee_with_rows(
    bytes: &mut [u8],
    offset: usize,
    record_end: usize,
    raw_mask: u32,
    suffix_read_bytes: usize,
    loaded_rows: Option<&[Option<usize>]>,
    no_table_policy: CompactStatusEffectNoTablePolicy,
) -> Option<CreatureC408CountRepair> {
    if offset + 12 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(raw_mask)
        || read_u16_le(bytes, offset + 10) != Some(0)
    {
        return None;
    }

    let entries_start = offset.checked_add(12)?;
    let entries_end = record_end.checked_sub(suffix_read_bytes)?;
    if entries_end < entries_start {
        return None;
    }
    let inferred_count = infer_compact_legacy_status_effect_row_count_with_policy(
        bytes,
        entries_start,
        entries_end,
        loaded_rows,
        no_table_policy,
    )?;
    bytes[offset + 10..offset + 12].copy_from_slice(&inferred_count.to_le_bytes());
    Some(CreatureC408CountRepair {
        entries: inferred_count,
        bytes_rewritten: 2,
    })
}

pub(super) fn repair_legacy_effect_only_visual_effect_count_for_ee(
    bytes: &mut [u8],
    offset: usize,
    record_end: usize,
) -> Option<CreatureC408CountRepair> {
    // EE `sub_140781E80` reaches `sub_1407B1F00` behind mask bit `0x0008`;
    // that helper reads the WORD count before the compact opcode/row triplets.
    // A stale zero count leaves real effect rows to be misread as later
    // live-object opcodes. Repair the count for the same no-target row family
    // accepted by the effect-only validator: loaded `visualeffects.2da`
    // Type_FD policy wins when available, while no-table mode stays limited to
    // the previously proven feature-0x0E-false no-target row.
    repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee_with_policy(
        bytes,
        offset,
        record_end,
        LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK,
        0,
        CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
    )
}

pub(super) fn repair_legacy_4408_visual_effect_count_for_ee(
    bytes: &mut [u8],
    offset: usize,
    record_end: usize,
) -> Option<CreatureC408CountRepair> {
    // Local XP2 Chapter 1 capture, verified against the same EE/Diamond
    // `WriteGameObjUpdate_UpdateObject` reader order as the existing 0x4408
    // fixtures:
    //
    //   0x0008: WORD looping-visual-effect delta count, then count entries.
    //   0x0400: four signed SHORT scalar/status values.
    //   0x4000: fragment BOOL status suffix.
    //
    // The malformed source record leaves the preceding count as zero. A zero
    // count shifts the compact effect rows into the 0x0400 scalar reader and
    // strands the following record. Repair the general compact no-target row
    // rule before the four-WORD scalar/status suffix; the caller still inserts
    // EE's per-effect identity maps and proves the final live-object cursor.
    repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee(
        bytes,
        offset,
        record_end,
        0x0000_4408,
        8,
    )
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
    // Count-zero C408 captures prove the same malformed family as 0x4408, with
    // an extra visibility suffix in the CNW fragment stream. The read-buffer
    // repair is still only the compact effect-row count before the four-WORD
    // scalar/status suffix; exact EE validation owns the later fragment BOOLs.
    repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee(
        bytes,
        offset,
        record_end,
        0x0000_C408,
        8,
    )
}

pub(super) fn repair_legacy_zero_count_status_effect_record_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<CreatureStatusEffectZeroCountRepair> {
    let mut candidate = bytes.clone();
    let candidate_count_repair =
        repair_legacy_c408_visual_effect_count_for_ee(&mut candidate, offset, *record_end)
            .or_else(|| {
                repair_legacy_4408_visual_effect_count_for_ee(&mut candidate, offset, *record_end)
            })
            .or_else(|| {
                repair_legacy_effect_only_visual_effect_count_for_ee(
                    &mut candidate,
                    offset,
                    *record_end,
                )
            })?;

    let mut exact_bit_cursor = bit_cursor;
    if advance_verified_noop_creature_update_record_exact_cursor(
        &candidate,
        offset,
        *record_end,
        fragment_bits,
        &mut exact_bit_cursor,
    ) {
        *bytes = candidate;
        return Some(CreatureStatusEffectZeroCountRepair { bytes_inserted: 0 });
    }

    let mut candidate_record_end = *record_end;
    let transform_rewrite =
        insert_compact_legacy_creature_update_status_effect_identity_maps_for_ee(
            &mut candidate,
            offset,
            &mut candidate_record_end,
            fragment_bits,
            bit_cursor,
        )?;
    if transform_rewrite.entries != candidate_count_repair.entries {
        return None;
    }

    let mut exact_bit_cursor = bit_cursor;
    if !advance_verified_noop_creature_update_record_exact_cursor(
        &candidate,
        offset,
        candidate_record_end,
        fragment_bits,
        &mut exact_bit_cursor,
    ) {
        return None;
    }

    *bytes = candidate;
    *record_end = candidate_record_end;
    Some(CreatureStatusEffectZeroCountRepair {
        bytes_inserted: transform_rewrite.bytes_inserted,
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

    let count = read_u16_le(bytes, offset + 10)?;
    if count == 0 || count > 256 {
        return None;
    }

    let cursor = try_get_ee_creature_status_effect_entries_end(
        bytes,
        offset.checked_add(12)?,
        count,
        scan_end,
    )?;
    let cursor = cursor.checked_add(8)?;
    (cursor <= scan_end).then_some(cursor)
}

pub(super) fn try_get_ee_creature_update_0008_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // EE `sub_140781E80` reaches the same visual-effect update reader for a
    // standalone `0x0008` status-effect delta. After the focused rewrite inserts
    // `ObjectVisualTransformData` after each A/D row, the row bytes are no
    // longer live-object boundaries; the count-derived EE cursor owns them.
    if offset + 12 > scan_end
        || scan_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(bytes, offset + 6)? != 0x0000_0008
    {
        return None;
    }

    let count = read_u16_le(bytes, offset + 10)?;
    if count == 0 || count > 256 {
        return None;
    }

    let cursor = try_get_ee_creature_status_effect_entries_end(
        bytes,
        offset.checked_add(12)?,
        count,
        scan_end,
    )?;
    (cursor <= scan_end).then_some(cursor)
}

pub(super) fn try_get_ee_creature_update_4008_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` handles mask bit
    // `0x0008` by writing the visual-effect delta count and current-build
    // ObjectVisualTransformData after each three-byte effect entry. Mask bit
    // `0x4000` then consumes only seven fragment BOOLs in the no-master branch
    // proven by the paired cursor validator. This boundary helper owns only the
    // byte span after the focused rewrite inserted identity maps.
    if offset + 12 > scan_end
        || scan_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(bytes, offset + 6)? != 0x0000_4008
    {
        return None;
    }

    let count = read_u16_le(bytes, offset + 10)?;
    if count == 0 || count > 256 {
        return None;
    }

    let cursor = try_get_ee_creature_status_effect_entries_end(
        bytes,
        offset.checked_add(12)?,
        count,
        scan_end,
    )?;
    (cursor <= scan_end).then_some(cursor)
}

pub(super) fn try_get_ee_creature_update_8008_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // Local Diamond XP2 Chapter 2 proves the status-effect/visibility family:
    //
    //   0x0008: WORD visual-effect delta count, then compact effect entries.
    //   0x8000: three visibility BOOLs in the CNW fragment stream.
    //
    // EE `sub_140781E80` reaches the same visual-effect helper used by 0x4008
    // and current builds read ObjectVisualTransformData after each short
    // effect entry. The visibility suffix has no read-buffer bytes, so this
    // boundary helper owns only the already-rewritten EE effect list and leaves
    // the final creature cursor validator to prove the three fragment BOOLs.
    if offset + 12 > scan_end
        || scan_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != 0x05
        || read_u32_le(bytes, offset + 6)? != 0x0000_8008
    {
        return None;
    }

    let count = read_u16_le(bytes, offset + 10)?;
    if count == 0 || count > 256 {
        return None;
    }

    let cursor = try_get_ee_creature_status_effect_entries_end(
        bytes,
        offset.checked_add(12)?,
        count,
        scan_end,
    )?;
    (cursor <= scan_end).then_some(cursor)
}

fn try_get_ee_creature_status_effect_entries_end(
    bytes: &[u8],
    cursor: usize,
    count: u16,
    scan_end: usize,
) -> Option<usize> {
    if let Some(rows) = visual_effect_rows::loaded_visual_effect_target_payload_bytes() {
        return try_get_ee_creature_status_effect_entries_end_with_rows(
            bytes, cursor, count, scan_end, &rows,
        );
    }
    try_get_ee_creature_status_effect_entries_end_without_2da(bytes, cursor, count, scan_end)
}

fn try_get_ee_creature_status_effect_entries_end_without_2da(
    bytes: &[u8],
    cursor: usize,
    count: u16,
    scan_end: usize,
) -> Option<usize> {
    let no_target_end = try_get_ee_creature_status_effect_entries_end_with_fixed_target_width(
        bytes, cursor, count, scan_end, 0,
    );
    let target_end = if usize::from(count)
        <= MAX_CREATURE_STATUS_EFFECT_TARGET_AMBIGUITY_PROBE_ENTRIES_WITHOUT_2DA
    {
        try_get_ee_creature_status_effect_entries_end_with_fixed_target_width(
            bytes,
            cursor,
            count,
            scan_end,
            CREATURE_STATUS_EFFECT_TARGET_PAYLOAD_BYTES,
        )
    } else {
        None
    };

    match (no_target_end, target_end) {
        (Some(no_target_end), Some(target_end)) if no_target_end != target_end => None,
        (Some(end), _) => Some(end),
        (None, Some(_)) => None,
        (None, None) => None,
    }
}

fn try_get_ee_creature_status_effect_entries_end_with_fixed_target_width(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    scan_end: usize,
    target_payload_bytes: usize,
) -> Option<usize> {
    for _ in 0..usize::from(count) {
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') || read_u16_le(bytes, cursor + 1).is_none() {
            return None;
        }
        cursor = cursor.checked_add(3)?.checked_add(target_payload_bytes)?;
        if cursor > scan_end
            || !super::visual_transform::has_ee_object_visual_transform_identity_at(
                bytes, cursor, scan_end,
            )
        {
            return None;
        }
        cursor = cursor.checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
    }
    (cursor <= scan_end).then_some(cursor)
}

fn try_get_ee_creature_status_effect_entries_end_with_rows(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    scan_end: usize,
    rows: &[Option<usize>],
) -> Option<usize> {
    for _ in 0..usize::from(count) {
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        let row = read_u16_le(bytes, cursor + 1)?;
        let target_payload_bytes =
            visual_effect_rows::target_payload_bytes_for_loaded_row(rows, row)?;
        cursor = cursor.checked_add(3)?.checked_add(target_payload_bytes)?;
        if cursor > scan_end
            || !super::visual_transform::has_ee_object_visual_transform_identity_at(
                bytes, cursor, scan_end,
            )
        {
            return None;
        }
        cursor = cursor.checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
    }
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
    let scalar_status_start = lower_prefix
        .checked_add(1)?
        .checked_add(action_branch_bytes)?;
    let vector_status_start = lower_prefix
        .checked_add(6)?
        .checked_add(action_branch_bytes)?;

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
    let scalar_status_start = lower_prefix
        .checked_add(1)?
        .checked_add(action_branch_bytes)?;
    let vector_status_start = lower_prefix
        .checked_add(6)?
        .checked_add(action_branch_bytes)?;

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
    let count = read_u16_le(bytes, status_start)?;
    if count == 0 || count > 256 {
        return None;
    }

    let mut cursor = try_get_ee_creature_status_effect_entries_end(
        bytes,
        status_start.checked_add(2)?,
        count,
        scan_end,
    )?;

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
                state.read_cursor, state.bit_cursor
            );
        }
        if count == 0 || count > 256 {
            continue;
        }
        let insert_offset_plans = legacy_creature_status_effect_identity_map_insert_plans(
            bytes,
            state.read_cursor.checked_add(2)?,
            count,
            *record_end,
        );
        if insert_offset_plans.is_empty() {
            continue;
        }

        for insert_offsets in insert_offset_plans {
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
                candidate_record_end = candidate_record_end
                    .checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
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
            let bytes_inserted =
                insert_offsets.len() * EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;
            if let Some((
                accepted_candidate,
                accepted_record_end,
                accepted_count,
                accepted_inserted,
            )) = accepted.as_ref()
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
            accepted = Some((candidate, candidate_record_end, count, bytes_inserted));
        }
    }

    let (candidate, candidate_record_end, entries, bytes_inserted) = accepted?;
    *bytes = candidate;
    *record_end = candidate_record_end;
    Some(CreatureStatusEffectTransformRewrite {
        entries,
        bytes_inserted,
    })
}

fn insert_compact_legacy_creature_update_status_effect_identity_maps_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<CreatureStatusEffectTransformRewrite> {
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
    let mut accepted: Option<(Vec<u8>, usize, u16, usize)> = None;

    for state in start_states {
        let count = read_u16_le(bytes, state.read_cursor)?;
        if count == 0 || count > 256 {
            continue;
        }
        let insert_offset_plans = compact_legacy_creature_status_effect_identity_map_insert_plans(
            bytes,
            state.read_cursor.checked_add(2)?,
            count,
            *record_end,
        );
        if insert_offset_plans.is_empty() {
            continue;
        }

        for insert_offsets in insert_offset_plans {
            let mut candidate = bytes.clone();
            let mut candidate_record_end = *record_end;
            for insert_at in insert_offsets.iter().rev().copied() {
                candidate.splice(
                    insert_at..insert_at,
                    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
                );
                candidate_record_end = candidate_record_end
                    .checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
            }

            let mut candidate_bit_cursor = bit_cursor;
            if !advance_verified_noop_creature_update_record_exact_cursor(
                &candidate,
                offset,
                candidate_record_end,
                fragment_bits,
                &mut candidate_bit_cursor,
            ) {
                continue;
            }

            let bytes_inserted =
                insert_offsets.len() * EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;
            if let Some((
                accepted_candidate,
                accepted_record_end,
                accepted_count,
                accepted_inserted,
            )) = accepted.as_ref()
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
            accepted = Some((candidate, candidate_record_end, count, bytes_inserted));
        }
    }

    let (candidate, candidate_record_end, entries, bytes_inserted) = accepted?;
    *bytes = candidate;
    *record_end = candidate_record_end;
    Some(CreatureStatusEffectTransformRewrite {
        entries,
        bytes_inserted,
    })
}

fn legacy_creature_status_effect_identity_map_insert_plans(
    bytes: &[u8],
    cursor: usize,
    count: u16,
    record_end: usize,
) -> Vec<Vec<usize>> {
    if let Some(rows) = visual_effect_rows::loaded_visual_effect_target_payload_bytes() {
        return legacy_creature_status_effect_identity_map_insert_plan_with_rows(
            bytes, cursor, count, record_end, &rows,
        )
        .into_iter()
        .collect();
    }

    legacy_creature_status_effect_identity_map_insert_plan_with_fixed_target_width(
        bytes, cursor, count, record_end, 0,
    )
    .into_iter()
    .collect()
}

fn legacy_creature_status_effect_identity_map_insert_plan_with_fixed_target_width(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    record_end: usize,
    target_payload_bytes: usize,
) -> Option<Vec<usize>> {
    let mut insert_offsets = Vec::with_capacity(usize::from(count));
    let mut transform_maps_seen = 0usize;
    for _ in 0..usize::from(count) {
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') || read_u16_le(bytes, cursor + 1).is_none() {
            return None;
        }
        cursor = cursor.checked_add(3)?.checked_add(target_payload_bytes)?;
        if cursor > record_end {
            return None;
        }
        if super::visual_transform::has_ee_object_visual_transform_identity_at(
            bytes, cursor, record_end,
        ) {
            cursor = cursor.checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
            transform_maps_seen = transform_maps_seen.saturating_add(1);
            continue;
        }
        insert_offsets.push(cursor);
    }
    if insert_offsets.is_empty() || transform_maps_seen != 0 {
        return None;
    }
    Some(insert_offsets)
}

fn legacy_creature_status_effect_identity_map_insert_plan_with_rows(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    record_end: usize,
    rows: &[Option<usize>],
) -> Option<Vec<usize>> {
    let mut insert_offsets = Vec::with_capacity(usize::from(count));
    let mut transform_maps_seen = 0usize;
    for _ in 0..usize::from(count) {
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        let row = read_u16_le(bytes, cursor + 1)?;
        let target_payload_bytes =
            visual_effect_rows::target_payload_bytes_for_loaded_row(rows, row)?;
        cursor = cursor.checked_add(3)?.checked_add(target_payload_bytes)?;
        if cursor > record_end {
            return None;
        }
        if super::visual_transform::has_ee_object_visual_transform_identity_at(
            bytes, cursor, record_end,
        ) {
            cursor = cursor.checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
            transform_maps_seen = transform_maps_seen.saturating_add(1);
            continue;
        }
        insert_offsets.push(cursor);
    }
    if insert_offsets.is_empty() || transform_maps_seen != 0 {
        return None;
    }
    Some(insert_offsets)
}

fn compact_legacy_creature_status_effect_identity_map_insert_plans(
    bytes: &[u8],
    cursor: usize,
    count: u16,
    record_end: usize,
) -> Vec<Vec<usize>> {
    if let Some(rows) = visual_effect_rows::loaded_visual_effect_target_payload_bytes() {
        return compact_legacy_creature_status_effect_identity_map_insert_plan_with_rows(
            bytes, cursor, count, record_end, &rows,
        )
        .into_iter()
        .collect();
    }

    compact_legacy_creature_status_effect_identity_map_insert_plan_with_fixed_target_width(
        bytes, cursor, count, record_end, 0,
    )
    .into_iter()
    .collect()
}

fn compact_legacy_creature_status_effect_identity_map_insert_plan_with_fixed_target_width(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    record_end: usize,
    target_payload_bytes: usize,
) -> Option<Vec<usize>> {
    let mut insert_offsets = Vec::with_capacity(usize::from(count));
    for _ in 0..usize::from(count) {
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') || read_u16_le(bytes, cursor + 1).is_none() {
            return None;
        }
        cursor = cursor.checked_add(3)?.checked_add(target_payload_bytes)?;
        if cursor > record_end {
            return None;
        }
        insert_offsets.push(cursor);
    }
    (!insert_offsets.is_empty()).then_some(insert_offsets)
}

fn compact_legacy_creature_status_effect_identity_map_insert_plan_with_rows(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    record_end: usize,
    rows: &[Option<usize>],
) -> Option<Vec<usize>> {
    let mut insert_offsets = Vec::with_capacity(usize::from(count));
    for _ in 0..usize::from(count) {
        let change_opcode = bytes.get(cursor).copied()?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        let row = read_u16_le(bytes, cursor + 1)?;
        let target_payload_bytes =
            visual_effect_rows::target_payload_bytes_for_loaded_row(rows, row)?;
        cursor = cursor.checked_add(3)?.checked_add(target_payload_bytes)?;
        if cursor > record_end {
            return None;
        }
        insert_offsets.push(cursor);
    }
    (!insert_offsets.is_empty()).then_some(insert_offsets)
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
        let Some(candidate) = simulate_creature_update_3967_action0_ee_tail_cursor(candidate)
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
    cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let cursor = simulate_creature_update_3967_action0_pre_identity_ee_cursor(cursor)?;
    try_simulate_ee_creature_update_identity_optional_suffix(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    )
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

    // EE `sub_140781E80` action code 0 reaches `loc_140782FA0`: it has no
    // action-specific subobject. The reader then consumes mask-0x0008 status
    // effects at `loc_140782FA6` before the mask-0x0004 action-state byte at
    // `loc_140782FD1`.
    if !simulate_ee_creature_update_status_effect_helper_cursor(&mut cursor) {
        return None;
    }
    cursor.read_u8()?;

    simulate_creature_update_3967_action0_fixed_tail_after_state_cursor(cursor)
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

    if !simulate_ee_creature_update_status_effect_helper_cursor(&mut cursor) {
        return None;
    }
    cursor.read_u8()?;

    // Companion to the action0 bridge removal path above. Stock EE reads one
    // mask-0x0800 BYTE after the two mask-0x0100 FLOATs; the Starcore5 action0
    // bridge capture reaches the identity branch immediately after those
    // FLOATs. Insert zero only when the identity/associate suffix proves the
    // exact record end.
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

fn simulate_creature_update_3967_action0_fixed_tail_after_state_cursor(
    mut cursor: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
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
) -> Option<Creature3967Action2OptionalFloatBoolRewrite> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return None;
    }

    let mut already_valid_cursor = bit_cursor;
    if advance_verified_noop_creature_update_record_exact_cursor(
        bytes,
        offset,
        record_end,
        fragment_bits,
        &mut already_valid_cursor,
    ) || super::fragment_spans::verified_creature_update_3967_read_end_before_interleaved_fragment_span(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
    .is_some()
    {
        return None;
    }

    let Some(optional_float_bit) = find_legacy_3967_action2_optional_float_bit_for_repair(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) else {
        return None;
    };
    let Some(bit) = fragment_bits.get_mut(optional_float_bit) else {
        return None;
    };
    if !*bit {
        return None;
    }
    *bit = false;
    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        eprintln!(
            "live-object creature update 0x3967 action2 optional-float BOOL cleared: offset={offset} record_end={record_end} bit_cursor={bit_cursor} optional_float_bit={optional_float_bit}"
        );
    }
    Some(Creature3967Action2OptionalFloatBoolRewrite {
        bit_rewritten: optional_float_bit,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967Action0BridgeFollowupRewrite {
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
    pub bits_inserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967Action0MissingDamageByteRewrite {
    pub bytes_inserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967Action0ShortAssociateSuffixRewrite {
    pub bytes_inserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Creature3967Action0BridgeRemoval {
    start: usize,
    len: usize,
}

struct Creature3967Action0BridgeRewriteTrial {
    bytes: Vec<u8>,
    fragment_bits: Vec<bool>,
    record_end: usize,
    bytes_inserted: usize,
    bytes_removed: usize,
    bits_inserted: usize,
    removal_start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967ActionFfffBridgeFollowupRewrite {
    pub bytes_removed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967Action0EeBuild1fBoolRewrite {
    pub bits_inserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967Action2OptionalFloatBoolRewrite {
    pub bit_rewritten: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967OmittedActionCodeRewrite {
    pub bytes_inserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Creature3967BareSecondIdentityStringRewrite {
    pub identity_offset: usize,
    pub text_len: usize,
    pub advanced_bit_cursor: usize,
}

pub(super) fn rewrite_3967_bare_second_identity_string_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<Creature3967BareSecondIdentityStringRewrite> {
    let (candidate, rewrite) = build_3967_bare_second_identity_string_candidate(
        bytes.as_slice(),
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )?;
    *bytes = candidate;
    Some(rewrite)
}

fn build_3967_bare_second_identity_string_candidate(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<(Vec<u8>, Creature3967BareSecondIdentityStringRewrite)> {
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
    let mut accepted: Option<(Vec<u8>, Creature3967BareSecondIdentityStringRewrite)> = None;
    for candidate in candidates {
        let Some(identity_start) =
            simulate_legacy_creature_update_3967_pre_identity_cursor(candidate)
        else {
            continue;
        };
        let identity_offset = identity_start.read_cursor;
        let Some(candidate) = build_3967_bare_second_identity_string_candidate_at_offset(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
            identity_offset,
        ) else {
            continue;
        };
        if accepted
            .as_ref()
            .is_some_and(|accepted| accepted.1 != candidate.1 || accepted.0 != candidate.0)
        {
            return None;
        }
        accepted = Some(candidate);
    }

    accepted
}

fn build_3967_bare_second_identity_string_candidate_at_offset(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    identity_offset: usize,
) -> Option<(Vec<u8>, Creature3967BareSecondIdentityStringRewrite)> {
    // EE `sub_140781E80` and Diamond `sub_44ADD0` both read mask `0x1000` as:
    //
    //   WORD, CExoString, CExoString, BYTE, WORD, WORD, BOOL, BOOL, BYTE rows...
    //
    // HG Starcore5 captures can encode the second CExoString as bare printable
    // bytes followed by the four zero bytes that would have been the missing
    // CExoString length field. Rewrite only this decompiled identity cursor,
    // then require the focused creature-update validator to consume the exact
    // record. This keeps the translator semantic rather than a scan-and-trim
    // fallback.
    if identity_offset + 6 >= record_end
        || read_u16_le(bytes, identity_offset).is_none()
        || read_u32_le(bytes, identity_offset + 2) != Some(0)
    {
        return None;
    }

    let text_start = identity_offset.checked_add(6)?;
    let text_limit = record_end.min(text_start.checked_add(32)?);
    let mut accepted: Option<(Vec<u8>, Creature3967BareSecondIdentityStringRewrite)> = None;
    for text_end in text_start.checked_add(1)?..=text_limit {
        if !bytes
            .get(text_end.checked_sub(1)?)
            .copied()
            .is_some_and(is_legacy_bare_identity_string_byte)
        {
            break;
        }
        let Some(padding_end) = text_end.checked_add(4) else {
            continue;
        };
        if padding_end > record_end
            || bytes
                .get(text_end..padding_end)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
        {
            continue;
        }
        let text_len = text_end.checked_sub(text_start)?;
        if text_len == 0 || text_len > 32 {
            continue;
        }
        let mut candidate = bytes.to_vec();
        let text = candidate.get(text_start..text_end)?.to_vec();
        let encoded_len = u32::try_from(text_len).ok()?.to_le_bytes();
        candidate
            .get_mut(text_start..text_start.checked_add(4)?)?
            .copy_from_slice(&encoded_len);
        candidate
            .get_mut(text_start.checked_add(4)?..text_start.checked_add(4 + text_len)?)?
            .copy_from_slice(text.as_slice());

        let mut exact_cursor = bit_cursor;
        if !advance_verified_noop_creature_update_record_exact_cursor(
            candidate.as_slice(),
            offset,
            record_end,
            fragment_bits,
            &mut exact_cursor,
        ) {
            continue;
        }
        let rewrite = Creature3967BareSecondIdentityStringRewrite {
            identity_offset,
            text_len,
            advanced_bit_cursor: exact_cursor,
        };
        if accepted
            .as_ref()
            .is_some_and(|accepted| accepted.1 != rewrite || accepted.0 != candidate)
        {
            return None;
        }
        accepted = Some((candidate, rewrite));
    }

    accepted
}

fn is_legacy_bare_identity_string_byte(byte: u8) -> bool {
    (0x20..=0x7E).contains(&byte)
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

pub(super) fn remove_3967_action_ffff_legacy_bridge_followup_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<Creature3967ActionFfffBridgeFollowupRewrite> {
    let Some(removal_start) = find_legacy_3967_action_ffff_bridge_followup_removal(
        bytes.as_slice(),
        offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    ) else {
        if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
            eprintln!(
                "live-object creature update 0x3967 actionffff bridge followup not found: offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
            );
        }
        return None;
    };
    let removal_end = removal_start.checked_add(2)?;

    let mut trial_bytes = bytes.clone();
    trial_bytes.drain(removal_start..removal_end);
    let trial_record_end = record_end.checked_sub(2)?;
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
    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        eprintln!(
            "live-object creature update 0x3967 actionffff bridge followup removed: offset={offset} removal_start={removal_start} new_record_end={record_end}"
        );
    }
    Some(Creature3967ActionFfffBridgeFollowupRewrite { bytes_removed: 2 })
}

pub(super) fn remove_3967_action0_legacy_bridge_followup_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<Creature3967Action0BridgeFollowupRewrite> {
    let removals = find_legacy_3967_action0_bridge_followup_removals(
        bytes.as_slice(),
        offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    );
    if removals.is_empty() {
        if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
            eprintln!(
                "live-object creature update 0x3967 action0 bridge followup not found: offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
            );
        }
        return None;
    }

    let mut accepted: Option<Creature3967Action0BridgeRewriteTrial> = None;
    for removal in removals {
        let Some(trial) = build_3967_action0_bridge_rewrite_trial(
            bytes.as_slice(),
            offset,
            *record_end,
            fragment_bits,
            bit_cursor,
            removal,
        ) else {
            continue;
        };
        if accepted.replace(trial).is_some() {
            if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
                eprintln!(
                    "live-object creature update 0x3967 action0 bridge followup ambiguous repairs: offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
                );
            }
            return None;
        }
    }

    let Some(trial) = accepted else {
        if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
            eprintln!(
                "live-object creature update 0x3967 action0 bridge followup trial failed exact suffix repair: offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
            );
        }
        return None;
    };

    *bytes = trial.bytes;
    *fragment_bits = trial.fragment_bits;
    *record_end = trial.record_end;
    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        let removal_start = trial.removal_start;
        let bytes_inserted = trial.bytes_inserted;
        let bytes_removed = trial.bytes_removed;
        let bits_inserted = trial.bits_inserted;
        eprintln!(
            "live-object creature update 0x3967 action0 bridge followup removed: offset={offset} removal_start={removal_start} bytes_inserted={bytes_inserted} bytes_removed={bytes_removed} bits_inserted={bits_inserted} new_record_end={record_end}"
        );
    }
    Some(Creature3967Action0BridgeFollowupRewrite {
        bytes_inserted: trial.bytes_inserted,
        bytes_removed: trial.bytes_removed,
        bits_inserted: trial.bits_inserted,
    })
}

pub(super) fn insert_3967_action0_missing_damage_byte_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<Creature3967Action0MissingDamageByteRewrite> {
    let (damage_insert_offset, _advanced_bit_cursor) =
        find_legacy_3967_action0_missing_damage_byte_ee_repair(
            bytes.as_slice(),
            offset,
            *record_end,
            fragment_bits,
            bit_cursor,
        )?;

    let mut trial_bytes = bytes.clone();
    trial_bytes.insert(damage_insert_offset, 0);
    let trial_record_end = (*record_end).checked_add(1)?;
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

    *bytes = trial_bytes;
    *record_end = trial_record_end;
    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        eprintln!(
            "live-object creature update 0x3967 action0 missing damage byte inserted: offset={offset} insertion_offset={damage_insert_offset} new_record_end={record_end}"
        );
    }
    Some(Creature3967Action0MissingDamageByteRewrite { bytes_inserted: 1 })
}

pub(super) fn insert_3967_action0_short_associate_suffix_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<Creature3967Action0ShortAssociateSuffixRewrite> {
    let insertion_offset = find_legacy_3967_action0_short_associate_suffix_insertion(
        bytes.as_slice(),
        offset,
        *record_end,
        fragment_bits,
        bit_cursor,
    )?;

    let mut trial_bytes = bytes.clone();
    trial_bytes.splice(insertion_offset..insertion_offset, [0, 0, 0, 0]);
    let trial_record_end = (*record_end).checked_add(4)?;
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

    *bytes = trial_bytes;
    *record_end = trial_record_end;
    if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
        eprintln!(
            "live-object creature update 0x3967 action0 short associate suffix expanded: offset={offset} insertion_offset={insertion_offset} new_record_end={record_end}"
        );
    }
    Some(Creature3967Action0ShortAssociateSuffixRewrite { bytes_inserted: 4 })
}

fn build_3967_action0_bridge_rewrite_trial(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    removal: Creature3967Action0BridgeRemoval,
) -> Option<Creature3967Action0BridgeRewriteTrial> {
    let removal_end = removal.start.checked_add(removal.len)?;
    if removal_end > record_end {
        return None;
    }

    let mut trial_bytes_after_removal = bytes.to_vec();
    trial_bytes_after_removal.drain(removal.start..removal_end);
    let trial_record_end_after_removal = record_end.checked_sub(removal.len)?;
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
                removal.len,
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
            (trial_bytes, trial_record_end, None, 1, removal.len)
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
                removal.len.saturating_add(trailing_storage_bytes),
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
                removal.len,
            )
        } else {
            if debug_creature_update_cursor_trace_enabled(LEGACY_CREATURE_UPDATE_3967_MASK) {
                eprintln!(
                    "live-object creature update 0x3967 action0 bridge followup candidate failed exact suffix repair: offset={offset} removal_start={} removal_len={} trial_record_end={trial_record_end_after_removal} bit_cursor={bit_cursor}",
                    removal.start, removal.len
                );
            }
            return None;
        }
    };
    let mut trial_fragment_bits = fragment_bits.to_vec();
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
                "live-object creature update 0x3967 action0 bridge followup candidate failed exact EE validator: offset={offset} removal_start={} removal_len={} insert_bit={insert_bit:?} trial_record_end={trial_record_end} trial_bit_cursor={trial_bit_cursor}",
                removal.start, removal.len
            );
        }
        return None;
    }

    Some(Creature3967Action0BridgeRewriteTrial {
        bytes: trial_bytes,
        fragment_bits: trial_fragment_bits,
        record_end: trial_record_end,
        bytes_inserted,
        bytes_removed,
        bits_inserted: usize::from(insert_bit.is_some()),
        removal_start: removal.start,
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

fn find_legacy_3967_action0_short_associate_suffix_insertion(
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
    let mut accepted: Option<usize> = None;
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
        for suffix in identity_candidates {
            // EE `sub_140781E80` and Diamond `sub_44ADD0` read mask `0x2000`
            // as raw OBJECTID + WORD before the two associate BOOLs. This HG
            // action0 shape ends after the two low zero bytes; materialize the
            // remaining four zero read-buffer bytes only when the full EE
            // validator accepts the expanded record.
            let remaining = record_end.checked_sub(suffix.read_cursor)?;
            if remaining != 2 {
                continue;
            }
            if bytes.get(suffix.read_cursor..record_end) != Some([0, 0].as_slice()) {
                continue;
            }
            if accepted
                .replace(record_end)
                .is_some_and(|old| old != record_end)
            {
                return None;
            }
        }
    }
    accepted
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

fn find_legacy_3967_action0_bridge_followup_removals(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Vec<Creature3967Action0BridgeRemoval> {
    if offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(0x05)
        || read_u32_le(bytes, offset + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
    {
        return Vec::new();
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
        return Vec::new();
    }

    let Some(candidates) = build_legacy_creature_orientation_branch_candidate_states(
        LEGACY_CREATURE_UPDATE_3967_MASK,
        cursor,
    ) else {
        return Vec::new();
    };

    let mut accepted = Vec::new();
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

        if !simulate_ee_creature_update_status_effect_helper_cursor(&mut candidate) {
            continue;
        }
        if candidate.read_u8().is_none() {
            continue;
        }
        push_unique_action0_bridge_removal(
            bytes,
            record_end,
            &mut accepted,
            candidate.read_cursor,
            LEGACY_CREATURE_UPDATE_3967_ACTION0_WOE_BRIDGE_BLOCK_BYTES.as_slice(),
        );
        push_unique_action0_bridge_removal(
            bytes,
            record_end,
            &mut accepted,
            candidate.read_cursor,
            LEGACY_CREATURE_UPDATE_3967_ACTION0_BRIDGE_FOLLOWUP_BYTES.as_slice(),
        );
    }
    accepted
}

fn push_unique_action0_bridge_removal(
    bytes: &[u8],
    record_end: usize,
    accepted: &mut Vec<Creature3967Action0BridgeRemoval>,
    removal_start: usize,
    expected: &[u8],
) {
    let Some(removal_end) = removal_start.checked_add(expected.len()) else {
        return;
    };
    if removal_end > record_end || bytes.get(removal_start..removal_end) != Some(expected) {
        return;
    }
    let removal = Creature3967Action0BridgeRemoval {
        start: removal_start,
        len: expected.len(),
    };
    if !accepted.contains(&removal) {
        accepted.push(removal);
    }
}

fn find_legacy_3967_action_ffff_bridge_followup_removal(
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

    // Diamond/HG can carry a legacy zero WORD after the non-special 0xFFFF
    // action branch. EE `sub_140781E80` reads the action-code WORD and the
    // following action-state BYTE for mask 0x0004, then continues directly
    // into the mask 0x0040 fields; it does not consume this bridge WORD. The
    // candidate locator below is deliberately only a locator: the transform is
    // claimed only when removing one exact zero WORD yields a unique record that
    // the decompile-owned EE 0x3967/action-FFFF validator consumes exactly.
    let mut accepted: Option<usize> = None;
    for removal_start in offset + 10..record_end.saturating_sub(1) {
        if bytes.get(removal_start..removal_start + 2) != Some(&[0, 0][..]) {
            continue;
        }

        let mut trial_bytes = bytes.to_vec();
        trial_bytes.drain(removal_start..removal_start + 2);
        let trial_record_end = record_end.checked_sub(2)?;
        let mut trial_bit_cursor = bit_cursor;
        if !advance_verified_creature_update_3967_action_ffff_ee_record(
            &trial_bytes,
            offset,
            trial_record_end,
            fragment_bits,
            &mut trial_bit_cursor,
        ) {
            continue;
        }

        if accepted.replace(removal_start).is_some() {
            return None;
        }
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
        if accepted
            .replace(optional_float_bit)
            .is_some_and(|old| old != optional_float_bit)
        {
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
    if count_word > 256 {
        return false;
    }
    let Some(after_count) = cursor.checked_add(2) else {
        return false;
    };
    let Some(status_end) = record_end.checked_sub(8) else {
        return false;
    };
    let Some(status_cursor) =
        try_get_ee_creature_status_effect_entries_end(bytes, after_count, count_word, status_end)
    else {
        return false;
    };
    if status_cursor != status_end {
        return false;
    }
    let Some(after_scalars) = status_cursor.checked_add(8) else {
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
    if flags == LEGACY_CREATURE_APPEARANCE_NOOP_MASK {
        // Diamond `sub_448E30` and EE `sub_14077FE10` both read the `P/5`
        // object id and WORD appearance mask before the mask-gated body. A
        // zero mask has no body branches and consumes no CNW fragment BOOLs.
        // Keep this as a true zero-length body, not a fallback for failed
        // complex appearance parses.
        return record_end == offset + 8 && *bit_cursor <= fragment_bits.len();
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
        if cursor
            .read_creature_appearance_locstring_component()
            .is_none()
            || cursor
                .read_creature_appearance_locstring_component()
                .is_none()
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

pub(super) fn try_get_zero_mask_creature_appearance_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    let record_end = offset.checked_add(8)?;
    if record_end > scan_end
        || record_end > bytes.len()
        || bytes.get(offset).copied()? != b'P'
        || bytes.get(offset + 1).copied()? != 0x05
        || !looks_like_legacy_creature_object_id(read_u32_le(bytes, offset + 2)?)
        || read_u16_le(bytes, offset + 6)? != LEGACY_CREATURE_APPEARANCE_NOOP_MASK
    {
        return None;
    }
    Some(record_end)
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
    super::object_ids::has_known_legacy_live_object_id_namespace(object_id)
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
            if legacy_creature_update_status_precedes_action_followup(raw_mask) {
                states.push(candidate);
                continue;
            }
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
    let status_precedes_action_followup =
        legacy_creature_update_status_precedes_action_followup(raw_mask);

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
        if status_precedes_action_followup
            && (raw_mask & 0x0000_0008) != 0
            && !simulate_ee_creature_update_status_effect_helper_cursor(&mut cursor)
        {
            trace_creature_update_cursor_reject(
                "status-effects-before-action-followup",
                raw_mask,
                cursor.read_cursor,
                cursor.bit_cursor,
                record_end,
            );
            return None;
        }
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

    if !status_precedes_action_followup
        && (raw_mask & 0x0000_0008) != 0
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

fn legacy_creature_update_status_precedes_action_followup(raw_mask: u32) -> bool {
    matches!(raw_mask, 0x0000_000F)
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

fn advance_verified_legacy_feature0e_false_effect_only_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    // EE `sub_1407B1F00` first reads the compact status/effect row count and
    // one opcode/WORD row per entry. The negotiated legacy feature-0x0E path
    // used by this bridge does not read an ObjectVisualTransformData map here,
    // but Diamond/EE still use `visualeffects.2da` to decide whether a row owns
    // the target payload. Loaded Type_FD policy is authoritative; without that
    // table, keep only the previously proven no-target legacy row.
    let Some(mut cursor) = offset.checked_add(10) else {
        return false;
    };
    let Some(count) = read_u16_le(bytes, cursor) else {
        return false;
    };
    if count == 0 || count > 256 {
        return false;
    }
    cursor += 2;
    let loaded_rows = visual_effect_rows::loaded_visual_effect_target_payload_bytes();

    for _ in 0..usize::from(count) {
        if record_end.saturating_sub(cursor) < 3 {
            return false;
        }
        let Some(change_opcode) = bytes.get(cursor).copied() else {
            return false;
        };
        if !matches!(change_opcode, b'A' | b'D') {
            return false;
        }
        let Some(row) = read_u16_le(bytes, cursor + 1) else {
            return false;
        };
        if !compact_status_effect_row_has_no_target_payload(
            row,
            loaded_rows.as_deref(),
            CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
        ) {
            return false;
        }
        cursor += 3;
    }

    cursor == record_end
}

fn legacy_feature0e_false_known_no_target_row(row: u16) -> bool {
    matches!(row, VFX_DUR_LOWLIGHTVISION_ROW)
}

fn simulate_ee_creature_update_status_effect_helper_cursor(
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
) -> bool {
    if let Some(rows) = visual_effect_rows::loaded_visual_effect_target_payload_bytes() {
        return simulate_ee_creature_update_status_effect_helper_cursor_with_rows(cursor, &rows);
    }
    simulate_ee_creature_update_status_effect_helper_cursor_without_2da(cursor)
}

fn simulate_ee_creature_update_status_effect_helper_cursor_with_rows(
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
    rows: &[Option<usize>],
) -> bool {
    let Some(count) = cursor.read_u16() else {
        return false;
    };
    if count > 256 {
        return false;
    }

    let Some(read_cursor) = try_get_ee_creature_status_effect_entries_end_with_rows(
        cursor.bytes,
        cursor.read_cursor,
        count,
        cursor.record_end,
        rows,
    ) else {
        return false;
    };
    cursor.read_cursor = read_cursor;
    true
}

fn simulate_ee_creature_update_status_effect_helper_cursor_without_2da(
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
) -> bool {
    let Some(count) = cursor.read_u16() else {
        return false;
    };
    if count > 256 {
        return false;
    }

    let Some(read_cursor) = try_get_ee_creature_status_effect_entries_end_without_2da(
        cursor.bytes,
        cursor.read_cursor,
        count,
        cursor.record_end,
    ) else {
        // EE `sub_1407B1F00` and Diamond `sub_44ED20` check the resolved
        // visualeffects.2da row before optionally reading a DWORD object id
        // plus one BYTE for `Type_FD` `P`/`B` rows. Until proxy2 has row-type
        // state at this cursor, target-width byte shapes are only ambiguity
        // evidence; they are not exact-owned by the creature reader.
        return false;
    };
    cursor.read_cursor = read_cursor;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_only_creature_appearance_token_inline_bytes() -> Vec<u8> {
        let mut bytes = vec![b'P', 0x05];
        bytes.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        bytes.extend_from_slice(&LEGACY_CREATURE_APPEARANCE_NAME_ONLY_MASK.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // First locstring component token ref.
        bytes.extend_from_slice(&0u32.to_le_bytes()); // Second locstring component empty string.
        bytes
    }

    fn creature_4008_live_bytes_with_status_row(target_payload: Option<&[u8]>) -> Vec<u8> {
        let mut bytes = vec![b'U', 0x05, 0x55, 0x00, 0x00, 0x80];
        bytes.extend_from_slice(&0x0000_4008u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.push(b'A');
        bytes.extend_from_slice(&0x1234u16.to_le_bytes());
        if let Some(payload) = target_payload {
            bytes.extend_from_slice(payload);
        }
        bytes
    }

    fn creature_update_0047_action4_zero_followup_live_bytes(
        include_implicit_point: bool,
    ) -> Vec<u8> {
        let mut bytes = vec![b'U', 0x05];
        bytes.extend_from_slice(&0x8000_000Au32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_0047u32.to_le_bytes());
        bytes.extend_from_slice(&0x0DA1u16.to_le_bytes()); // position X low 16 bits.
        bytes.extend_from_slice(&0x0F8Bu16.to_le_bytes()); // position Y low 16 bits.
        bytes.extend_from_slice(&0x0FD1u16.to_le_bytes()); // position Z low 16 bits.
        bytes.push(0x6F); // scalar orientation low 8 bits.
        bytes.extend_from_slice(&1.0f32.to_le_bytes()); // action scalar.
        bytes.extend_from_slice(&4u16.to_le_bytes()); // movement action code.
        bytes.push(1); // action state byte.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // zero follow-up count.
        if include_implicit_point {
            bytes.extend_from_slice(&0x0D9Au16.to_le_bytes());
            bytes.extend_from_slice(&0x0E0Eu16.to_le_bytes());
        }
        bytes.extend_from_slice(&0xFFFFu16.to_le_bytes()); // 0x0040 branch first field.
        bytes.push(1); // 0x0040 branch mode without optional object id.
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.push(1);
        bytes
    }

    fn creature_update_0047_action4_fragment_bits() -> Vec<bool> {
        let mut bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        bits.extend_from_slice(&[
            false, false, // position Z high bits.
            false, // scalar orientation branch.
            false, false, false, false, // scalar orientation residual bits.
            false, // no orientation target object.
            false, // no post-state extra float.
            false, // 0x0040 state BOOL.
        ]);
        bits
    }

    fn creature_update_0047_action4_omitted_target_fragment_bits() -> Vec<bool> {
        let mut bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        bits.extend_from_slice(&[
            false, false, // position Z high bits.
            false, // scalar orientation branch.
            false, false, false, false, // scalar orientation residual bits.
            false, // no post-state extra float; no orientation-target guard bit was emitted.
            false, // 0x0040 state BOOL.
        ]);
        bits
    }

    fn creature_update_0047_action4_vector_target_live_bytes() -> Vec<u8> {
        let mut bytes = vec![b'U', 0x05];
        bytes.extend_from_slice(&0x8000_000Au32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_0047u32.to_le_bytes());
        bytes.extend_from_slice(&0x0DA1u16.to_le_bytes()); // position X low 16 bits.
        bytes.extend_from_slice(&0x0F8Bu16.to_le_bytes()); // position Y low 16 bits.
        bytes.extend_from_slice(&0x0FD1u16.to_le_bytes()); // position Z low 16 bits.
        bytes.extend_from_slice(&0x0101u16.to_le_bytes()); // vector orientation X.
        bytes.extend_from_slice(&0x0202u16.to_le_bytes()); // vector orientation Y.
        bytes.extend_from_slice(&0x0303u16.to_le_bytes()); // vector orientation Z.
        bytes.extend_from_slice(&0x8000_000Bu32.to_le_bytes()); // orientation target object.
        bytes.extend_from_slice(&1.0f32.to_le_bytes()); // action scalar.
        bytes.extend_from_slice(&4u16.to_le_bytes()); // movement action code.
        bytes.push(1); // action state byte.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // zero follow-up count.
        bytes.extend_from_slice(&0x0D9Au16.to_le_bytes()); // implicit 2D point X.
        bytes.extend_from_slice(&0x0E0Eu16.to_le_bytes()); // implicit 2D point Y.
        bytes.extend_from_slice(&0xFFFFu16.to_le_bytes()); // 0x0040 branch first field.
        bytes.push(2); // 0x0040 branch mode with optional object id.
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&0x8000_000Cu32.to_le_bytes()); // 0x0040 optional object id.
        bytes
    }

    fn creature_update_0047_action4_vector_target_fragment_bits() -> Vec<bool> {
        let mut bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        bits.extend_from_slice(&[
            false, false, // position Z high bits.
            true,  // vector orientation branch.
            true,  // orientation target object guard.
            false, // no post-state extra float.
            false, // 0x0040 state BOOL.
        ]);
        bits
    }

    fn creature_update_3967_action0_scalar_live_bytes() -> Vec<u8> {
        let mut bytes = vec![b'U', 0x05];
        bytes.extend_from_slice(&0x8000_000Au32.to_le_bytes());
        bytes.extend_from_slice(&LEGACY_CREATURE_UPDATE_3967_MASK.to_le_bytes());
        bytes.extend_from_slice(&0x1111u16.to_le_bytes()); // position X low 16 bits.
        bytes.extend_from_slice(&0x2222u16.to_le_bytes()); // position Y low 16 bits.
        bytes.extend_from_slice(&0x3333u16.to_le_bytes()); // position Z low 16 bits.
        bytes.push(0x44); // scalar orientation low 8 bits.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // portrait row, no CResRef.
        bytes.extend_from_slice(&0u32.to_le_bytes()); // action scalar.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // action code 0.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // no status/effect rows.
        bytes.push(0); // action state byte.
        bytes.extend_from_slice(&0x1234u16.to_le_bytes()); // 0x0040 branch first field.
        bytes.push(1); // 0x0040 branch mode without optional object id.
        bytes.extend_from_slice(&0x5678u16.to_le_bytes());
        bytes.push(2);
        bytes.extend_from_slice(&0x1111_1111u32.to_le_bytes()); // 0x0100 first field.
        bytes.extend_from_slice(&0x2222_2222u32.to_le_bytes()); // 0x0100 second field.
        bytes.push(0); // 0x0800 byte.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // identity row prefix.
        bytes.extend_from_slice(&0u32.to_le_bytes()); // first identity CExoString length.
        bytes.extend_from_slice(&0u32.to_le_bytes()); // second identity CExoString length.
        bytes.push(0);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.push(0); // identity row count after two identity BOOLs.
        bytes.extend_from_slice(&0x8000_000Bu32.to_le_bytes()); // associate object id.
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    fn creature_update_3967_action0_scalar_fragment_bits() -> Vec<bool> {
        let mut bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        bits.extend_from_slice(&[
            true, false, // position Z high bits.
            false, // scalar orientation branch.
            true, false, true, false, // scalar orientation residual bits.
            true,  // 0x0040 state BOOL.
            false, true, // identity branch BOOLs.
            true, false, // associate suffix BOOLs.
        ]);
        bits
    }

    fn creature_update_3967_action0_scalar_target_false_fragment_bits() -> Vec<bool> {
        let mut bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        bits.extend_from_slice(&[
            true, false, // position Z high bits.
            false, // scalar orientation branch.
            true, false, true, false, // scalar orientation residual bits.
            false, // explicit orientation-target guard: no target object id.
            true,  // 0x0040 state BOOL after the action/status read-buffer body.
            false, true, // identity branch BOOLs.
            true, false, // associate suffix BOOLs.
        ]);
        bits
    }

    fn creature_update_3967_action2_scalar_live_bytes() -> Vec<u8> {
        let mut bytes = vec![b'U', 0x05];
        bytes.extend_from_slice(&0x8000_000Au32.to_le_bytes());
        bytes.extend_from_slice(&LEGACY_CREATURE_UPDATE_3967_MASK.to_le_bytes());
        bytes.extend_from_slice(&0x1111u16.to_le_bytes()); // position X low 16 bits.
        bytes.extend_from_slice(&0x2222u16.to_le_bytes()); // position Y low 16 bits.
        bytes.extend_from_slice(&0x3333u16.to_le_bytes()); // position Z low 16 bits.
        bytes.push(0x44); // scalar orientation low 8 bits.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // portrait row, no CResRef.
        bytes.extend_from_slice(&0u32.to_le_bytes()); // action scalar.
        bytes.extend_from_slice(&2u16.to_le_bytes()); // movement action code 2.
        bytes.push(0); // action state byte.
        bytes.extend_from_slice(&1u16.to_le_bytes()); // one movement follow-up point.
        bytes.extend_from_slice(&0x0101u16.to_le_bytes()); // movement point X.
        bytes.extend_from_slice(&0x0202u16.to_le_bytes()); // movement point Y.
        bytes.extend_from_slice(&0x1234u16.to_le_bytes()); // 0x0040 branch first field.
        bytes.push(1); // 0x0040 branch mode without optional object id.
        bytes.extend_from_slice(&0x5678u16.to_le_bytes());
        bytes.push(2);
        bytes.extend_from_slice(&0x1111_1111u32.to_le_bytes()); // 0x0100 first field.
        bytes.extend_from_slice(&0x2222_2222u32.to_le_bytes()); // 0x0100 second field.
        bytes.push(0); // 0x0800 byte.
        bytes.extend_from_slice(&0u16.to_le_bytes()); // identity row prefix.
        bytes.extend_from_slice(&0u32.to_le_bytes()); // first identity CExoString length.
        bytes.extend_from_slice(&0u32.to_le_bytes()); // second identity CExoString length.
        bytes.push(0);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.push(0); // identity row count after two identity BOOLs.
        bytes.extend_from_slice(&0x8000_000Bu32.to_le_bytes()); // associate object id.
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    fn creature_update_3967_action2_scalar_fragment_bits(optional_float: bool) -> Vec<bool> {
        let mut bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        bits.extend_from_slice(&[
            true,
            false, // position Z high bits.
            false, // scalar orientation branch.
            true,
            false,
            true,
            false,          // scalar orientation residual bits.
            optional_float, // action-code 2 movement follow-up optional-float guard.
            true,           // 0x0040 state BOOL.
            false,
            true, // identity branch BOOLs.
            true,
            false, // associate suffix BOOLs.
        ]);
        bits
    }

    fn creature_update_4000_live_bytes() -> Vec<u8> {
        let mut bytes = vec![b'U', 0x05];
        bytes.extend_from_slice(&0x8000_000Au32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_4000u32.to_le_bytes());
        bytes
    }

    fn append_cexo_string(bytes: &mut Vec<u8>, text: &[u8]) {
        bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
        bytes.extend_from_slice(text);
    }

    fn creature_update_4000_master_detail_direct_live_bytes() -> Vec<u8> {
        let mut bytes = creature_update_4000_live_bytes();
        bytes.extend_from_slice(&0x8000_000Bu32.to_le_bytes());
        append_cexo_string(&mut bytes, b"Dominated");
        append_cexo_string(&mut bytes, b"Master");
        bytes
    }

    fn creature_update_4000_master_detail_tlk_live_bytes() -> Vec<u8> {
        let mut bytes = creature_update_4000_live_bytes();
        bytes.extend_from_slice(&0x8000_000Bu32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_1234u32.to_le_bytes());
        append_cexo_string(&mut bytes, b"Master");
        bytes
    }

    fn creature_update_4000_fragment_bits(body_bits: &[bool]) -> Vec<bool> {
        let mut bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        bits.extend_from_slice(body_bits);
        bits
    }

    #[test]
    fn loaded_visualeffects_rows_drive_creature_status_target_payload_width() {
        let mut rows = vec![None; 0x1235];
        rows[0x00F3] = Some(0);
        rows[0x1234] = Some(CREATURE_STATUS_EFFECT_TARGET_PAYLOAD_BYTES);

        let mut bytes = vec![0x02, 0x00, b'A', 0xF3, 0x00];
        bytes.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        bytes.extend_from_slice(&[b'D', 0x34, 0x12, 0x44, 0x33, 0x22, 0x80, 0x66]);
        bytes.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );

        let mut cursor = LegacyCreatureUpdateCursor {
            bytes: &bytes,
            record_end: bytes.len(),
            read_cursor: 0,
            bit_cursor: 0,
            fragment_bits: &[],
        };

        assert!(
            simulate_ee_creature_update_status_effect_helper_cursor_with_rows(&mut cursor, &rows,)
        );
        assert_eq!(cursor.read_cursor, bytes.len());
    }

    #[test]
    fn loaded_visualeffects_rows_reject_missing_creature_status_target_payload() {
        let mut rows = vec![None; 0x1235];
        rows[0x1234] = Some(CREATURE_STATUS_EFFECT_TARGET_PAYLOAD_BYTES);

        let mut bytes = vec![0x01, 0x00, b'A', 0x34, 0x12];
        bytes.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );

        let mut cursor = LegacyCreatureUpdateCursor {
            bytes: &bytes,
            record_end: bytes.len(),
            read_cursor: 0,
            bit_cursor: 0,
            fragment_bits: &[],
        };

        assert!(
            !simulate_ee_creature_update_status_effect_helper_cursor_with_rows(&mut cursor, &rows),
            "a P/B visualeffects row must own DWORD target id plus one BYTE before the map"
        );
    }

    #[test]
    fn feature0e_false_effect_rows_use_loaded_visualeffects_policy() {
        let mut rows = vec![None; 0x1236];
        rows[0x00F3] = Some(0);
        rows[0x1234] = Some(0);
        rows[0x1235] = Some(CREATURE_STATUS_EFFECT_TARGET_PAYLOAD_BYTES);

        assert!(
            compact_status_effect_row_has_no_target_payload(
                0x1234,
                Some(&rows),
                CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
            ),
            "loaded non-P/B Type_FD rows own no target payload"
        );
        assert!(
            !compact_status_effect_row_has_no_target_payload(
                0x1235,
                Some(&rows),
                CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
            ),
            "loaded P/B Type_FD rows still own the target payload"
        );
        assert!(
            !compact_status_effect_row_has_no_target_payload(
                0x1234,
                None,
                CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
            ),
            "without loaded row state, arbitrary effect-only rows must stay unclaimed"
        );
        assert!(
            compact_status_effect_row_has_no_target_payload(
                VFX_DUR_LOWLIGHTVISION_ROW,
                None,
                CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
            ),
            "the old no-table fallback remains limited to the proven no-target row"
        );
    }

    #[test]
    fn effect_only_zero_count_repair_uses_loaded_no_target_rows() {
        let mut rows = vec![None; 0x1236];
        rows[0x1234] = Some(0);
        rows[0x1235] = Some(0);
        let mut bytes = vec![b'U', 0x05, 0x55, 0x00, 0x00, 0x80];
        bytes.extend_from_slice(&LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[b'A', 0x34, 0x12, b'D', 0x35, 0x12]);
        let record_end = bytes.len();

        let repair = repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee_with_rows(
            &mut bytes,
            0,
            record_end,
            LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK,
            0,
            Some(&rows),
            CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
        )
        .expect("loaded no-target rows should repair the stale effect-only count");

        assert_eq!(repair.entries, 2);
        assert_eq!(read_u16_le(&bytes, 10), Some(2));
    }

    #[test]
    fn effect_only_zero_count_repair_rejects_loaded_target_rows() {
        let mut rows = vec![None; 0x1235];
        rows[0x1234] = Some(CREATURE_STATUS_EFFECT_TARGET_PAYLOAD_BYTES);
        let mut bytes = vec![b'U', 0x05, 0x55, 0x00, 0x00, 0x80];
        bytes.extend_from_slice(&LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[b'A', 0x34, 0x12]);
        let record_end = bytes.len();

        assert!(
            repair_legacy_zero_visual_effect_count_from_compact_rows_for_ee_with_rows(
                &mut bytes,
                0,
                record_end,
                LEGACY_CREATURE_UPDATE_EFFECT_ONLY_MASK,
                0,
                Some(&rows),
                CompactStatusEffectNoTablePolicy::KnownFeature0eFalseRowsOnly,
            )
            .is_none(),
            "effect-only count repair must not erase a target-payload row"
        );
        assert_eq!(read_u16_le(&bytes, 10), Some(0));
    }

    #[test]
    fn creature_update_0047_action4_zero_followup_consumes_optional_point_before_state_tail() {
        let bits = creature_update_0047_action4_fragment_bits();
        let mut with_point = creature_update_0047_action4_zero_followup_live_bytes(true);
        let mut bit_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &with_point,
                0,
                with_point.len(),
                &bits,
                &mut bit_cursor,
            ),
            "action-4 zero-followup records may own one implicit 2D point before the 0x0040 state tail"
        );
        assert_eq!(
            bit_cursor,
            bits.len(),
            "the record owns only position/orientation/target, extra-float, and state BOOLs"
        );

        with_point.truncate(with_point.len() - 2);
        let mut truncated_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            !advance_verified_noop_creature_update_record_exact_cursor(
                &with_point,
                0,
                with_point.len(),
                &bits,
                &mut truncated_cursor,
            ),
            "the implicit point is accepted only when the remaining bytes still include the full 0x0040 state tail"
        );
        assert_eq!(
            truncated_cursor,
            super::super::CNW_FRAGMENT_HEADER_BITS,
            "failed 0x47 cursor proof must restore the fragment cursor"
        );
    }

    #[test]
    fn creature_update_0047_action4_zero_followup_does_not_require_implicit_point() {
        let bytes = creature_update_0047_action4_zero_followup_live_bytes(false);
        let bits = creature_update_0047_action4_fragment_bits();
        let mut bit_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;

        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut bit_cursor,
            ),
            "zero follow-up count remains valid without the optional implicit point when the 0x0040 tail begins immediately"
        );
        assert_eq!(bit_cursor, bits.len());
    }

    #[test]
    fn creature_update_0047_action4_zero_followup_accepts_omitted_target_guard() {
        let bytes = creature_update_0047_action4_zero_followup_live_bytes(false);
        let bits = creature_update_0047_action4_omitted_target_fragment_bits();
        let mut bit_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;

        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut bit_cursor,
            ),
            "Diamond may omit the orientation target guard entirely before the action/state tail"
        );
        assert_eq!(
            bit_cursor,
            bits.len(),
            "the action extra-float bit must not be misread as a target guard"
        );
    }

    #[test]
    fn creature_update_0047_action4_zero_followup_accepts_vector_target_branch() {
        let bytes = creature_update_0047_action4_vector_target_live_bytes();
        let bits = creature_update_0047_action4_vector_target_fragment_bits();
        let mut bit_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;

        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut bit_cursor,
            ),
            "vector orientation, target object, implicit point, and mode-2 0x0040 object must share one exact 0x47 cursor"
        );
        assert_eq!(bit_cursor, bits.len());
    }

    #[test]
    fn creature_update_3967_scalar_orientation_rejects_shifted_fragment_cursor() {
        let bytes = creature_update_3967_action0_scalar_live_bytes();
        let bits = creature_update_3967_action0_scalar_fragment_bits();
        let mut exact_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut exact_cursor,
            ),
            "0x3967 action-0 scalar orientation owns position bits, scalar selector/residual bits, 0x0040 state, identity, and associate BOOLs from the exact cursor"
        );
        assert_eq!(exact_cursor, bits.len());

        let mut shifted_cursor = super::super::CNW_FRAGMENT_HEADER_BITS + 2;
        assert!(
            !advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut shifted_cursor,
            ),
            "a following 0x3967 row must not be rescued by retrying two bits before the caller-owned cursor"
        );
        assert_eq!(
            shifted_cursor,
            super::super::CNW_FRAGMENT_HEADER_BITS + 2,
            "failed shifted 0x3967 cursor proof must restore the caller cursor"
        );

        let mut missing_prefix_bits = vec![false; super::super::CNW_FRAGMENT_HEADER_BITS];
        missing_prefix_bits.extend_from_slice(&bits[super::super::CNW_FRAGMENT_HEADER_BITS + 2..]);
        let mut missing_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            !advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &missing_prefix_bits,
                &mut missing_cursor,
            ),
            "a tail that starts after the two position-owned bits is not a valid compact source for this 0x3967 row"
        );
        assert_eq!(
            missing_cursor,
            super::super::CNW_FRAGMENT_HEADER_BITS,
            "failed truncated-tail 0x3967 cursor proof must restore the caller cursor"
        );
    }

    #[test]
    fn creature_update_3967_scalar_orientation_target_guard_advances_before_state() {
        let bytes = creature_update_3967_action0_scalar_live_bytes();
        let bits = creature_update_3967_action0_scalar_target_false_fragment_bits();
        let mut cursor = super::super::CNW_FRAGMENT_HEADER_BITS;

        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut cursor,
            ),
            "explicit false target ownership must validate before the later 0x0040 state BOOL"
        );
        assert_eq!(
            cursor,
            bits.len(),
            "the preferred decompile-backed 0x3967 action-0 shape owns position, scalar orientation, explicit false target, 0x0040, identity, and associate BOOLs"
        );
        assert_eq!(
            cursor - super::super::CNW_FRAGMENT_HEADER_BITS,
            13,
            "the Sooty offset-218 shape advances 13 CNW bits when the orientation target guard is emitted"
        );
    }

    #[test]
    fn creature_update_3967_action2_optional_float_repair_clears_exact_guard_bit() {
        let bytes = creature_update_3967_action2_scalar_live_bytes();
        let mut bits = creature_update_3967_action2_scalar_fragment_bits(true);
        let optional_float_bit = super::super::CNW_FRAGMENT_HEADER_BITS + 2 + 1 + 4;
        let mut unrepaired_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;

        assert!(
            !advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut unrepaired_cursor,
            ),
            "without repair, EE reads an absent optional FLOAT before the movement point"
        );

        let repair = repair_3967_action2_optional_float_bool_for_ee(
            &bytes,
            0,
            bytes.len(),
            &mut bits,
            super::super::CNW_FRAGMENT_HEADER_BITS,
        )
        .expect("action-code 2 false optional-float branch should repair");
        assert_eq!(repair.bit_rewritten, optional_float_bit);
        assert!(
            !bits[optional_float_bit],
            "the repair must clear only the action-followup optional-float BOOL"
        );

        let mut cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut cursor,
            ),
            "the repaired action-code 2 row must exact-validate from the caller cursor"
        );
        assert_eq!(cursor, bits.len());
    }

    #[test]
    fn creature_update_3967_action2_optional_float_repair_leaves_false_guard_untouched() {
        let bytes = creature_update_3967_action2_scalar_live_bytes();
        let mut bits = creature_update_3967_action2_scalar_fragment_bits(false);
        let original_bits = bits.clone();

        assert!(
            repair_3967_action2_optional_float_bool_for_ee(
                &bytes,
                0,
                bytes.len(),
                &mut bits,
                super::super::CNW_FRAGMENT_HEADER_BITS,
            )
            .is_none(),
            "an already-false movement optional-float guard is not a rewrite"
        );
        assert_eq!(bits, original_bits);

        let mut cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut cursor,
            ),
            "the false optional-float branch is already decompile-owned"
        );
        assert_eq!(cursor, bits.len());
    }

    #[test]
    fn creature_update_4000_master_detail_locstrings_follow_guarded_branch() {
        let no_master_bytes = creature_update_4000_live_bytes();
        let no_master_bits =
            creature_update_4000_fragment_bits(&[false, false, false, false, false, false, false]);
        let mut no_master_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &no_master_bytes,
                0,
                no_master_bytes.len(),
                &no_master_bits,
                &mut no_master_cursor,
            ),
            "the 0x4000 status suffix without master detail strings owns only seven BOOLs"
        );
        assert_eq!(no_master_cursor, no_master_bits.len());

        let direct_bytes = creature_update_4000_master_detail_direct_live_bytes();
        let direct_bits = creature_update_4000_fragment_bits(&[
            false, false, false, false, true, false, false, false, false,
        ]);
        let mut direct_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &direct_bytes,
                0,
                direct_bytes.len(),
                &direct_bits,
                &mut direct_cursor,
            ),
            "direct-string server locstrings are read between the guarded object id and final 0x4000 BOOLs"
        );
        assert_eq!(direct_cursor, direct_bits.len());

        let tlk_bytes = creature_update_4000_master_detail_tlk_live_bytes();
        let tlk_bits = creature_update_4000_fragment_bits(&[
            false, false, false, false, true, true, false, false, false, false,
        ]);
        let mut tlk_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;
        assert!(
            advance_verified_noop_creature_update_record_exact_cursor(
                &tlk_bytes,
                0,
                tlk_bytes.len(),
                &tlk_bits,
                &mut tlk_cursor,
            ),
            "TLK-ref server locstrings consume selector BOOL, language BOOL, then DWORD ref before the next locstring"
        );
        assert_eq!(tlk_cursor, tlk_bits.len());
    }

    #[test]
    fn creature_update_4000_master_detail_rejects_shifted_locstring_bits() {
        let bytes = creature_update_4000_master_detail_direct_live_bytes();
        let bits = creature_update_4000_fragment_bits(&[
            false, false, false, false, true, false, false, false,
        ]);
        let mut bit_cursor = super::super::CNW_FRAGMENT_HEADER_BITS;

        assert!(
            !advance_verified_noop_creature_update_record_exact_cursor(
                &bytes,
                0,
                bytes.len(),
                &bits,
                &mut bit_cursor,
            ),
            "the final two status BOOLs must not be borrowed to satisfy missing locstring selector bits"
        );
        assert_eq!(
            bit_cursor,
            super::super::CNW_FRAGMENT_HEADER_BITS,
            "failed 0x4000 cursor proof must restore the caller's fragment cursor"
        );
    }

    #[test]
    fn name_only_creature_appearance_locstring_token_requires_component_bits() {
        let bytes = name_only_creature_appearance_token_inline_bytes();
        let mut short_cursor = 0usize;
        assert!(
            !advance_verified_noop_creature_appearance_record(
                &bytes,
                0,
                bytes.len(),
                &[true, true, false],
                &mut short_cursor,
            ),
            "outer locstring + token component needs the token language bit and the second component selector"
        );
        assert_eq!(
            short_cursor, 0,
            "rejected fallback must restore the fragment cursor"
        );

        let mut exact_cursor = 0usize;
        assert!(
            advance_verified_noop_creature_appearance_record(
                &bytes,
                0,
                bytes.len(),
                &[true, true, false, false],
                &mut exact_cursor,
            ),
            "token first component plus inline second component consumes the decompiled four BOOLs"
        );
        assert_eq!(exact_cursor, 4);
    }

    #[test]
    fn creature_status_effect_boundary_rejects_target_payload_without_2da() {
        let mut bytes =
            creature_4008_live_bytes_with_status_row(Some(&[0x44, 0x33, 0x22, 0x80, 0x66]));
        bytes.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );

        assert_eq!(
            try_get_ee_creature_update_4008_record_end(&bytes, 0, bytes.len()),
            None,
            "without visualeffects.2da row proof, target-payload shape is not boundary proof"
        );
    }

    #[test]
    fn creature_status_effect_boundary_rejects_same_row_target_map_ambiguity_without_2da() {
        let mut bytes = creature_4008_live_bytes_with_status_row(Some(&[0, 0, 0, 0, 0]));
        bytes.extend_from_slice(
            &super::super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );

        assert_eq!(
            try_get_ee_creature_update_4008_record_end(&bytes, 0, bytes.len()),
            None,
            "without row proof, zero target bytes plus the EE map can also look like an immediate no-target map"
        );
    }

    #[test]
    fn creature_status_effect_rewrite_rejects_target_payload_without_2da() {
        let mut bytes =
            creature_4008_live_bytes_with_status_row(Some(&[0x44, 0x33, 0x22, 0x80, 0x66]));
        let original = bytes.clone();
        let mut record_end = bytes.len();
        let fragment_bits = vec![false; 7];

        assert!(
            insert_creature_update_status_effect_identity_maps_for_ee(
                &mut bytes,
                0,
                &mut record_end,
                &fragment_bits,
                0,
            )
            .is_none(),
            "target-payload map insertion requires loaded visualeffects.2da row policy"
        );
        assert_eq!(bytes, original);
        assert_eq!(record_end, original.len());
    }

    #[test]
    fn creature_status_effect_rewrite_rejects_ambiguous_target_payload_cursor_without_2da() {
        let mut bytes = creature_4008_live_bytes_with_status_row(Some(&[0, 0, 0, 0, 0]));
        let original = bytes.clone();
        let mut record_end = bytes.len();
        let fragment_bits = vec![false; 7];

        assert!(
            insert_creature_update_status_effect_identity_maps_for_ee(
                &mut bytes,
                0,
                &mut record_end,
                &fragment_bits,
                0,
            )
            .is_none(),
            "same-row no-target/target ambiguity must stay unrewritten without visualeffects.2da row proof"
        );
        assert_eq!(bytes, original);
        assert_eq!(record_end, original.len());
    }
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
            // EE `loc_1404ED5B5` writes the guarded dominated/master detail
            // block as OBJECTID, then two `WriteCExoLocStringServer` values.
            // Diamond's matching locstring helper and EE's shared
            // `WriteCExoLocStringServer` shape are bit-fronted: one selector
            // BOOL, then either a second selector BOOL plus DWORD string-ref or
            // an inline length-prefixed CExoString. Keep the final two status
            // BOOLs after both locstrings so this branch cannot borrow their
            // bits as string selectors.
            if cursor.read_u32().is_none()
                || cursor.read_server_locstring().is_none()
                || cursor.read_server_locstring().is_none()
            {
                return false;
            }
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

    fn read_creature_appearance_locstring_component(&mut self) -> Option<()> {
        let token_branch = self.read_bool()?;
        if token_branch {
            // Diamond `sub_53E700` and the EE locstring helper first read the
            // component token selector, then the client-TLK/language selector,
            // before the DWORD token reference. Do not collapse this to the
            // inline CExoString shape; the read bytes can be identical while
            // the fragment cursor is two bits wider.
            self.read_bool()?;
            self.read_u32()?;
        } else {
            self.read_cexo_string()?;
        }
        Some(())
    }

    fn read_server_locstring(&mut self) -> Option<()> {
        if self.read_bool()? {
            self.read_bool()?;
            self.read_u32()?;
        } else {
            self.read_cexo_string()?;
        }
        Some(())
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
