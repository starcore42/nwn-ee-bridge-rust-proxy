//! Creature-specific live-object update helpers.
//!
//! These helpers classify creature add/update record shapes. They deliberately
//! do not mutate bytes; transforms stay in the top-level update dispatcher and
//! writer helpers.

use super::{class_rows, read_f32_le, read_u16_le, read_u32_le};

const LEGACY_LIVE_CREATURE_UPDATE_ASSOCIATE_MASK: u32 = 0x0000_2000;
const LEGACY_CREATURE_UPDATE_3967_MASK: u32 = 0x0000_3967;
const LEGACY_CREATURE_APPEARANCE_NAME_ONLY_MASK: u16 = 0x0400;
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

pub(super) const EE_CREATURE_ADD_RECORD_BYTES: usize = 72;

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
    const CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET: usize = 32;
    if offset > bytes.len()
        || record_end > bytes.len()
        || record_end < offset + CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    if (object_id & 0x8000_0000) == 0 || read_u16_le(bytes, offset + 30).is_none() {
        return false;
    }

    for index in 0..6 {
        let Some(value) = read_f32_le(bytes, offset + 6 + index * 4) else {
            return false;
        };
        if !value.is_finite() || value.abs() > 1_000_000_000.0 {
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
    const IDENTITY_MAP: [u8; 40] = [
        0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F,
    ];
    let end = offset + IDENTITY_MAP.len();
    end <= record_end && end <= bytes.len() && bytes[offset..end] == IDENTITY_MAP
}

pub(super) fn advance_verified_noop_creature_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let original_bit_cursor = *bit_cursor;
    if advance_verified_noop_creature_update_record_exact_cursor(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return true;
    }
    *bit_cursor = original_bit_cursor;
    if read_u32_le(bytes, offset + 6) == Some(LEGACY_CREATURE_UPDATE_3967_MASK) {
        if let Some(repaired_cursor) = try_advance_legacy_3967_at_adjacent_bit_cursor(
            bytes,
            offset,
            record_end,
            fragment_bits,
            original_bit_cursor,
        ) {
            *bit_cursor = repaired_cursor;
            return true;
        }
    }
    false
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

fn try_advance_legacy_3967_at_adjacent_bit_cursor(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let mut accepted: Option<usize> = None;
    let candidates = [bit_cursor.checked_add(1), bit_cursor.checked_sub(1)];
    for candidate_start in candidates.into_iter().flatten() {
        if candidate_start > fragment_bits.len() {
            continue;
        }
        let mut candidate_cursor = candidate_start;
        if !simulate_legacy_live_creature_update_cursors(
            bytes,
            offset,
            record_end,
            fragment_bits,
            &mut candidate_cursor,
        ) {
            continue;
        }
        if accepted.replace(candidate_cursor).is_some() {
            return None;
        }
    }
    accepted
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

    let Some(portrait_row) = cursor.read_u16() else {
        return false;
    };
    if portrait_row >= 0xFFFE && cursor.read_cresref().is_none() {
        return false;
    }

    let Some(action_code) = simulate_legacy_creature_update_action_branch(&mut cursor) else {
        return false;
    };
    if action_code != 2 || cursor.read_u8().is_none() {
        return false;
    }
    let Some(followup_count) = cursor.read_u16() else {
        return false;
    };
    if followup_count != 1 {
        return false;
    }

    // EE/Diamond `sub_140781E80` / `sub_44ADD0` action-code 2 movement
    // follow-up reads this BOOL before the two 16-bit movement values. The HG
    // `0x3967` records documented in the alignment reference use the false
    // branch; a true value shifts the later identity branch into the following
    // read-buffer bytes and produces the exact `RVA 0x7851EB` class of faults.
    let optional_float_bit = cursor.bit_cursor;
    let Some(bit) = fragment_bits.get_mut(optional_float_bit) else {
        return false;
    };
    if !*bit {
        return false;
    }
    *bit = false;
    true
}

fn advance_verified_creature_update_c408(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    // Decompile/capture-backed HG creature state family:
    //
    //   U 05 <object-id> mask=0x0000_C408
    //   0x0008: WORD status-row count, then count*(BYTE opcode + WORD row)
    //   0x0400: four WORD scalar values
    //   0x4000: seven fragment BOOLs
    //   0x8000: three fragment BOOLs
    //
    // The BOOL values are state flags, not a permission to reinterpret this
    // compact legacy packet as the newer optional master-detail branch. Earlier
    // generic cursor simulation rejected captured HG packets when the fifth
    // 0x4000 bit was true even though no read-buffer bytes follow the four
    // scalar WORDs. Keep this as a narrow exact validator instead of broadening
    // the generic creature-update simulator.
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
    let Some(status_bytes) = count.checked_mul(3) else {
        return false;
    };
    let Some(remaining) = record_end.checked_sub(cursor) else {
        return false;
    };
    if status_bytes > remaining {
        return false;
    }
    let Some(after_status) = cursor.checked_add(status_bytes) else {
        return false;
    };
    cursor = after_status;
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
    matches!(
        object_id & 0xFF00_0000,
        0x8000_0000 | 0x8800_0000 | 0xFF00_0000 | 0x0100_0000 | 0x0500_0000
    ) || (0x0000_1000..=0x00FF_FFFF).contains(&object_id)
}

fn simulate_legacy_live_creature_update_cursors(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
        return false;
    };
    if !is_supported_legacy_creature_update_cursor_mask(raw_mask) {
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
        return false;
    }

    if (raw_mask & 0x0000_0002) != 0 {
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
    }

    if (raw_mask & 0x0000_0020) != 0 {
        let Some(portrait_row) = cursor.read_u16() else {
            return false;
        };
        if portrait_row >= 0xFFFE && cursor.read_cresref().is_none() {
            return false;
        }
    }

    if (raw_mask & 0x0000_0004) != 0 {
        let Some(action_code) = simulate_legacy_creature_update_action_branch(&mut cursor) else {
            return false;
        };
        if cursor.read_u8().is_none() {
            return false;
        }
        if !simulate_legacy_creature_update_action_post_state_followup(&mut cursor, action_code) {
            return false;
        }
    }

    if (raw_mask & 0x0000_0008) != 0
        && !simulate_legacy_creature_update_status_effect_helper_cursor(&mut cursor)
    {
        return false;
    }

    if (raw_mask & 0x0000_0040) != 0 {
        let Some(_first) = cursor.read_u16() else {
            return false;
        };
        let Some(branch_mode) = cursor.read_u8() else {
            return false;
        };
        if cursor.read_u16().is_none() || cursor.read_u8().is_none() || cursor.read_bool().is_none()
        {
            return false;
        }
        if branch_mode == 2 && cursor.read_u32().is_none() {
            return false;
        }
    }

    if (raw_mask & 0x0000_0100) != 0
        && (cursor.read_unsigned_bits(32).is_none() || cursor.read_unsigned_bits(32).is_none())
    {
        return false;
    }

    if (raw_mask & 0x0000_0200) != 0
        && (cursor.read_unsigned_bits(10).is_none() || cursor.read_unsigned_bits(10).is_none())
    {
        return false;
    }

    if (raw_mask & 0x0000_0400) != 0 {
        for _ in 0..4 {
            if cursor.read_u16().is_none() {
                return false;
            }
        }
    }

    if (raw_mask & 0x0002_0000) != 0 && cursor.read_u16().is_none() {
        return false;
    }

    if (raw_mask & 0x0000_0800) != 0 && cursor.read_u8().is_none() {
        return false;
    }

    if (raw_mask & 0x0000_1000) != 0 {
        let Some(accepted) =
            try_simulate_legacy_creature_update_identity_optional_suffix(raw_mask, cursor)
        else {
            return false;
        };
        cursor = accepted;
    } else if !simulate_legacy_creature_update_suffix_after_identity(raw_mask, &mut cursor) {
        return false;
    }

    if cursor.read_cursor != record_end {
        return false;
    }
    *bit_cursor = cursor.bit_cursor;
    true
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

fn simulate_legacy_creature_update_status_effect_helper_cursor(
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
    }
    true
}

fn try_simulate_legacy_creature_update_identity_optional_suffix(
    raw_mask: u32,
    identity_start: LegacyCreatureUpdateCursor<'_>,
) -> Option<LegacyCreatureUpdateCursor<'_>> {
    let candidates = build_legacy_creature_update_identity_branch_candidate_states(identity_start)?;
    let mut accepted: Option<LegacyCreatureUpdateCursor<'_>> = None;

    for mut candidate in candidates {
        if !simulate_legacy_creature_update_suffix_after_identity(raw_mask, &mut candidate) {
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
                continue;
            };
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
        states = next;
    }
    Some(states)
}

fn simulate_legacy_creature_update_suffix_after_identity(
    raw_mask: u32,
    cursor: &mut LegacyCreatureUpdateCursor<'_>,
) -> bool {
    if (raw_mask & LEGACY_LIVE_CREATURE_UPDATE_ASSOCIATE_MASK) != 0 {
        if cursor.read_u32().is_none()
            || cursor.read_u16().is_none()
            || cursor.read_bool().is_none()
            || cursor.read_bool().is_none()
        {
            return false;
        }
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
    if !simulate_legacy_creature_update_action_post_state_followup(&mut cursor, action_code) {
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
    action_code: u16,
) -> bool {
    let Some(followup_count) = cursor.read_u16() else {
        return false;
    };
    if followup_count > 256 {
        return false;
    }
    if followup_count == 0 {
        return true;
    }

    let Some(has_extra_float) = cursor.read_bool() else {
        return false;
    };
    if has_extra_float && cursor.read_unsigned_bits(32).is_none() {
        return false;
    }
    if !is_legacy_creature_update_movement_followup_action(action_code) {
        return true;
    }
    for _ in 0..followup_count {
        if cursor.read_unsigned_bits(16).is_none() || cursor.read_unsigned_bits(16).is_none() {
            return false;
        }
    }
    true
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
