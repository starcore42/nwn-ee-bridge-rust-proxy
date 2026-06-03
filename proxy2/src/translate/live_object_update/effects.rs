//! Looping visual-effect update-list translation for live-object updates.
//!
//! EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` handles update-mask
//! `0x00000008` as a compact change list:
//!
//! * WORD change count
//! * for each entry: BYTE change opcode (`A`/`D`) and WORD visual-effect row
//! * for current EE builds, `ObjectVisualTransformData::Write` after each entry
//!
//! Diamond/HG legacy captures carry the same count/opcode/row short form but
//! do not carry the EE visual-transform map. For EE build `2001/0x23`, an
//! identity object-level transform is two zero DWORD map counts. This module
//! keeps that semantic expansion isolated from the generic update-mask logic.
//!
//! Some visual-effect rows take a target payload before the EE transform map.
//! EE `HandleServerToPlayerUpdateVisualEffects` (`sub_1407B1F00`) and Diamond
//! `sub_44ED20` both check `visualeffects.2da` `Type_FD`; `P`/`B` rows read a
//! DWORD object id plus one BYTE at `0x1407B214A..0x1407B2165`, then EE always
//! checks build `2001/0x0E` and reads `ObjectVisualTransformData` at
//! `0x1407B218F..0x1407B21AE`. Without loaded row state, target-width shapes
//! are only ambiguity evidence for the stream scanner; they are not positive
//! packet-boundary proof.

use super::{
    CREATURE_OBJECT_TYPE, DOOR_OBJECT_TYPE, LEGACY_UPDATE_HEADER_BYTES, PLACEABLE_OBJECT_TYPE,
    read_u16_le, read_u32_le, visual_effect_rows,
};

pub(super) const LOOPING_VISUAL_EFFECT_UPDATE_MASK: u32 = 0x0000_0008;
const MAX_REASONABLE_LOOPING_EFFECT_CHANGES: u16 = 256;
const LOOPING_EFFECT_SHORT_ENTRY_BYTES: usize = 3;
const LOOPING_EFFECT_TARGET_PAYLOAD_BYTES: usize = 5;
const MAX_TARGET_PAYLOAD_AMBIGUITY_PROBE_ENTRIES_WITHOUT_2DA: u16 = 1;
const LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES: [u8;
    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN] =
    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES;

#[derive(Debug, Clone, Default)]
pub(super) struct LoopingVisualEffectRewrite {
    pub bytes_inserted: u32,
}

#[derive(Debug, Clone)]
struct LoopingVisualEffectList {
    insert_offsets_after_short_entries: Vec<usize>,
    already_ee_shaped: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct LoopingVisualEffectRecordEndCandidates {
    pub proven: Vec<usize>,
    pub ambiguous: Vec<usize>,
}

pub(super) fn rewrite_legacy_looping_visual_effect_update_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
) -> Option<LoopingVisualEffectRewrite> {
    let shape = parse_looping_visual_effect_update(bytes, offset, *record_end)?;
    if shape.already_ee_shaped {
        return Some(LoopingVisualEffectRewrite::default());
    }

    let mut bytes_inserted = 0u32;
    for insert_at in shape
        .insert_offsets_after_short_entries
        .iter()
        .rev()
        .copied()
    {
        bytes.splice(
            insert_at..insert_at,
            LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES,
        );
        *record_end = record_end.checked_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len())?;
        bytes_inserted =
            bytes_inserted.saturating_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len() as u32);
    }

    Some(LoopingVisualEffectRewrite { bytes_inserted })
}

pub(super) fn is_verified_ee_looping_visual_effect_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    parse_looping_visual_effect_update(bytes, offset, record_end)
        .is_some_and(|shape| shape.already_ee_shaped)
}

pub(super) fn has_legacy_looping_visual_effect_body_without_mask(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES).is_none_or(|count| count == 0) {
        return false;
    }
    parse_looping_visual_effect_body(bytes, offset, record_end)
        .is_some_and(|shape| !shape.already_ee_shaped)
}

pub(super) fn try_get_verified_ee_looping_visual_effect_update_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > scan_end || scan_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U'
        || !matches!(
            bytes.get(offset + 1).copied()?,
            PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE | CREATURE_OBJECT_TYPE
        )
        || read_u32_le(bytes, offset + 6)? != LOOPING_VISUAL_EFFECT_UPDATE_MASK
    {
        return None;
    }

    let count = read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES)?;
    if count > MAX_REASONABLE_LOOPING_EFFECT_CHANGES {
        return None;
    }

    let cursor = offset + LEGACY_UPDATE_HEADER_BYTES + 2;
    if let Some(rows) = visual_effect_rows::loaded_visual_effect_target_payload_bytes() {
        return try_get_verified_ee_looping_visual_effect_entries_end_with_rows(
            bytes, cursor, count, scan_end, &rows,
        );
    }
    let no_target_end =
        try_get_verified_ee_looping_visual_effect_entries_end(bytes, cursor, count, scan_end, 0);
    let target_end = if count <= MAX_TARGET_PAYLOAD_AMBIGUITY_PROBE_ENTRIES_WITHOUT_2DA {
        try_get_verified_ee_looping_visual_effect_entries_end(
            bytes,
            cursor,
            count,
            scan_end,
            LOOPING_EFFECT_TARGET_PAYLOAD_BYTES,
        )
    } else {
        None
    };

    match (no_target_end, target_end) {
        (Some(no_target_end), Some(target_end)) if no_target_end != target_end => {
            // Without visualeffects.2da row proof, a stream boundary that can be
            // both "no target + map" and "five-byte target + map" is not owned
            // by either decompiled branch.
            None
        }
        (Some(end), _) => Some(end),
        (None, Some(_)) => None,
        (None, None) => None,
    }
}

pub(super) fn legacy_looping_visual_effect_update_record_end_candidates(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<LoopingVisualEffectRecordEndCandidates> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > scan_end || scan_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U'
        || !matches!(
            bytes.get(offset + 1).copied()?,
            PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE | CREATURE_OBJECT_TYPE
        )
        || read_u32_le(bytes, offset + 6)? != LOOPING_VISUAL_EFFECT_UPDATE_MASK
    {
        return None;
    }

    let count = read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES)?;
    if count > MAX_REASONABLE_LOOPING_EFFECT_CHANGES {
        return None;
    }

    let cursor = offset + LEGACY_UPDATE_HEADER_BYTES + 2;
    if let Some(rows) = visual_effect_rows::loaded_visual_effect_target_payload_bytes() {
        return legacy_looping_visual_effect_entries_end_with_rows(
            bytes, cursor, count, scan_end, &rows,
        )
        .map(|end| LoopingVisualEffectRecordEndCandidates {
            proven: vec![end],
            ambiguous: Vec::new(),
        });
    }

    let mut candidates = LoopingVisualEffectRecordEndCandidates::default();
    if let Some(end) = legacy_looping_visual_effect_entries_end(bytes, cursor, count, scan_end, 0) {
        candidates.proven.push(end);
    }
    if count <= MAX_TARGET_PAYLOAD_AMBIGUITY_PROBE_ENTRIES_WITHOUT_2DA {
        if let Some(end) = legacy_looping_visual_effect_entries_end(
            bytes,
            cursor,
            count,
            scan_end,
            LOOPING_EFFECT_TARGET_PAYLOAD_BYTES,
        ) {
            if !candidates.proven.contains(&end) {
                candidates.ambiguous.push(end);
            }
        }
    }

    (!candidates.proven.is_empty() || !candidates.ambiguous.is_empty()).then_some(candidates)
}

fn parse_looping_visual_effect_update(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<LoopingVisualEffectList> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U'
        || !matches!(
            bytes.get(offset + 1).copied()?,
            PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE | CREATURE_OBJECT_TYPE
        )
        || read_u32_le(bytes, offset + 6)? != LOOPING_VISUAL_EFFECT_UPDATE_MASK
    {
        return None;
    }

    let count = read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES)?;
    if count > MAX_REASONABLE_LOOPING_EFFECT_CHANGES {
        return None;
    }

    parse_looping_visual_effect_body(bytes, offset, record_end)
}

fn parse_looping_visual_effect_body(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<LoopingVisualEffectList> {
    let count = read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES)?;
    if count > MAX_REASONABLE_LOOPING_EFFECT_CHANGES {
        return None;
    }

    let cursor = offset + LEGACY_UPDATE_HEADER_BYTES + 2;
    if let Some(rows) = visual_effect_rows::loaded_visual_effect_target_payload_bytes() {
        return parse_looping_visual_effect_entries_with_rows(
            bytes, cursor, count, record_end, &rows,
        );
    }
    if let Some(shape) = parse_looping_visual_effect_entries(bytes, cursor, count, record_end, 0) {
        return Some(shape);
    }

    None
}

fn parse_looping_visual_effect_entries(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    record_end: usize,
    target_payload_bytes: usize,
) -> Option<LoopingVisualEffectList> {
    let mut insert_offsets_after_short_entries = Vec::with_capacity(usize::from(count));
    let mut transform_maps_seen = 0usize;

    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
        if target_payload_bytes != 0 {
            cursor = cursor.checked_add(target_payload_bytes)?;
            if cursor > record_end {
                return None;
            }
        }
        insert_offsets_after_short_entries.push(cursor);

        if has_identity_transform_at(bytes, cursor, record_end) {
            cursor = cursor.checked_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len())?;
            transform_maps_seen = transform_maps_seen.saturating_add(1);
        }
    }

    if cursor != record_end {
        return None;
    }

    let count = usize::from(count);
    if transform_maps_seen != 0 && transform_maps_seen != count {
        // EE build-0x23 owns one ObjectVisualTransformData map after every row,
        // while the Diamond/HG legacy path owns none. A mixed list means the
        // next row boundary is already ambiguous, so quarantine it instead of
        // duplicating a map before rows that were already EE-shaped.
        return None;
    }
    Some(LoopingVisualEffectList {
        insert_offsets_after_short_entries,
        already_ee_shaped: transform_maps_seen == count,
    })
}

fn parse_looping_visual_effect_entries_with_rows(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    record_end: usize,
    rows: &[Option<usize>],
) -> Option<LoopingVisualEffectList> {
    let mut insert_offsets_after_short_entries = Vec::with_capacity(usize::from(count));
    let mut transform_maps_seen = 0usize;

    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        let row = read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
        let target_payload_bytes =
            visual_effect_rows::target_payload_bytes_for_loaded_row(rows, row)?;
        cursor = cursor.checked_add(target_payload_bytes)?;
        if cursor > record_end {
            return None;
        }
        insert_offsets_after_short_entries.push(cursor);

        if has_identity_transform_at(bytes, cursor, record_end) {
            cursor = cursor.checked_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len())?;
            transform_maps_seen = transform_maps_seen.saturating_add(1);
        }
    }

    if cursor != record_end {
        return None;
    }

    let count = usize::from(count);
    if transform_maps_seen != 0 && transform_maps_seen != count {
        return None;
    }
    Some(LoopingVisualEffectList {
        insert_offsets_after_short_entries,
        already_ee_shaped: transform_maps_seen == count,
    })
}

fn try_get_verified_ee_looping_visual_effect_entries_end(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    scan_end: usize,
    target_payload_bytes: usize,
) -> Option<usize> {
    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
        if target_payload_bytes != 0 {
            cursor = cursor.checked_add(target_payload_bytes)?;
            if cursor > scan_end {
                return None;
            }
        }
        if !has_identity_transform_at(bytes, cursor, scan_end) {
            return None;
        }
        cursor = cursor.checked_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len())?;
    }
    if cursor > scan_end {
        return None;
    }
    Some(cursor)
}

fn legacy_looping_visual_effect_entries_end(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    scan_end: usize,
    target_payload_bytes: usize,
) -> Option<usize> {
    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
        if target_payload_bytes != 0 {
            cursor = cursor.checked_add(target_payload_bytes)?;
        }
        if cursor > scan_end {
            return None;
        }
    }
    Some(cursor)
}

fn legacy_looping_visual_effect_entries_end_with_rows(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    scan_end: usize,
    rows: &[Option<usize>],
) -> Option<usize> {
    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        let row = read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
        let target_payload_bytes =
            visual_effect_rows::target_payload_bytes_for_loaded_row(rows, row)?;
        cursor = cursor.checked_add(target_payload_bytes)?;
        if cursor > scan_end {
            return None;
        }
    }
    Some(cursor)
}

fn try_get_verified_ee_looping_visual_effect_entries_end_with_rows(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    scan_end: usize,
    rows: &[Option<usize>],
) -> Option<usize> {
    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        let row = read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
        let target_payload_bytes =
            visual_effect_rows::target_payload_bytes_for_loaded_row(rows, row)?;
        cursor = cursor.checked_add(target_payload_bytes)?;
        if cursor > scan_end {
            return None;
        }
        if !has_identity_transform_at(bytes, cursor, scan_end) {
            return None;
        }
        cursor = cursor.checked_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len())?;
    }
    if cursor > scan_end {
        return None;
    }
    Some(cursor)
}

fn has_identity_transform_at(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    let end = offset.saturating_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len());
    end <= record_end
        && end <= bytes.len()
        && bytes[offset..end] == LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creature_effect_body_without_mask_rewrites_to_exact_ee_shape() {
        let mut record = vec![
            b'U',
            CREATURE_OBJECT_TYPE,
            0x0F,
            0x00,
            0x00,
            0x80,
            0x00,
            0x00,
            0x00,
            0x00,
            0x02,
            0x00,
            b'A',
            0xB6,
            0x00,
            b'A',
            0xF3,
            0x00,
        ];
        let mut record_end = record.len();
        assert!(has_legacy_looping_visual_effect_body_without_mask(
            &record, 0, record_end
        ));

        record[6..10].copy_from_slice(&LOOPING_VISUAL_EFFECT_UPDATE_MASK.to_le_bytes());
        let rewrite =
            rewrite_legacy_looping_visual_effect_update_for_ee(&mut record, 0, &mut record_end)
                .expect("legacy short effect rows should expand");

        assert_eq!(rewrite.bytes_inserted, 16);
        assert_eq!(record_end, record.len());
        assert!(is_verified_ee_looping_visual_effect_update_record(
            &record, 0, record_end
        ));
    }

    #[test]
    fn mixed_looping_effect_transform_rows_remain_unclaimed() {
        let mut record = vec![
            b'U',
            CREATURE_OBJECT_TYPE,
            0x0F,
            0x00,
            0x00,
            0x80,
            0x08,
            0x00,
            0x00,
            0x00,
            0x02,
            0x00,
            b'A',
            0xB6,
            0x00,
        ];
        record.extend_from_slice(&LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES);
        record.extend_from_slice(&[b'A', 0xF3, 0x00]);
        let mut record_end = record.len();

        assert!(
            rewrite_legacy_looping_visual_effect_update_for_ee(&mut record, 0, &mut record_end)
                .is_none(),
            "a partial EE/legacy effect list has no decompile-backed row boundary"
        );
        assert!(
            !is_verified_ee_looping_visual_effect_update_record(&record, 0, record_end),
            "the same mixed list is not exact EE because the second row lacks its map"
        );
        assert!(
            !has_legacy_looping_visual_effect_body_without_mask(&record, 0, record_end),
            "the mixed list must not be treated as the all-legacy Diamond shape"
        );
    }

    #[test]
    fn looping_effect_target_payload_requires_loaded_row_policy() {
        let mut record = vec![
            b'U',
            CREATURE_OBJECT_TYPE,
            0x0F,
            0x00,
            0x00,
            0x80,
            0x08,
            0x00,
            0x00,
            0x00,
            0x01,
            0x00,
            b'A',
            0x34,
            0x12,
            0x44,
            0x33,
            0x22,
            0x80,
            0x66,
        ];
        record.extend_from_slice(&LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES);

        assert!(
            !is_verified_ee_looping_visual_effect_update_record(&record, 0, record.len()),
            "without visualeffects.2da row proof, target-payload byte shape is not boundary proof"
        );

        let mut rows = vec![None; 0x1235];
        rows[0x1234] = Some(LOOPING_EFFECT_TARGET_PAYLOAD_BYTES);
        let shape = parse_looping_visual_effect_entries_with_rows(
            &record,
            LEGACY_UPDATE_HEADER_BYTES + 2,
            1,
            record.len(),
            &rows,
        )
        .expect("loaded P/B visualeffects row should prove the target-payload cursor");
        assert!(
            shape.already_ee_shaped,
            "P/B visualeffects rows own DWORD target id plus one BYTE before the transform map"
        );

        let mut stale_short_target = record.clone();
        stale_short_target.drain(18..20);
        assert!(
            parse_looping_visual_effect_entries_with_rows(
                &stale_short_target,
                LEGACY_UPDATE_HEADER_BYTES + 2,
                1,
                stale_short_target.len(),
                &rows,
            )
            .is_none(),
            "the old three-byte target-payload shape must not exact-claim"
        );
    }

    #[test]
    fn looping_effect_stream_boundary_rejects_ambiguous_target_fallback() {
        let mut record = vec![
            b'U',
            CREATURE_OBJECT_TYPE,
            0x0F,
            0x00,
            0x00,
            0x80,
            0x08,
            0x00,
            0x00,
            0x00,
            0x01,
            0x00,
            b'A',
            0x34,
            0x12,
        ];
        record.extend_from_slice(&[0; LOOPING_EFFECT_TARGET_PAYLOAD_BYTES]);
        record.extend_from_slice(&LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES);

        assert!(
            !is_verified_ee_looping_visual_effect_update_record(&record, 0, record.len()),
            "exact record validation must not own target rows without visualeffects.2da row proof"
        );
        assert_eq!(
            try_get_verified_ee_looping_visual_effect_update_record_end(&record, 0, record.len()),
            None,
            "stream boundary scanning must not choose between no-target and target cursors without row proof"
        );
    }

    #[test]
    fn loaded_visualeffects_rows_own_mixed_target_boundaries() {
        let mut rows = vec![None; 0x1235];
        rows[0x00F3] = Some(0);
        rows[0x1234] = Some(LOOPING_EFFECT_TARGET_PAYLOAD_BYTES);

        let mut record = vec![
            b'U',
            CREATURE_OBJECT_TYPE,
            0x0F,
            0x00,
            0x00,
            0x80,
            0x08,
            0x00,
            0x00,
            0x00,
            0x02,
            0x00,
            b'A',
            0xF3,
            0x00,
        ];
        record.extend_from_slice(&LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES);
        record.extend_from_slice(&[b'D', 0x34, 0x12, 0x44, 0x33, 0x22, 0x80, 0x66]);
        record.extend_from_slice(&LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES);

        let shape = parse_looping_visual_effect_entries_with_rows(
            &record,
            LEGACY_UPDATE_HEADER_BYTES + 2,
            2,
            record.len(),
            &rows,
        )
        .expect("loaded visualeffects.2da rows should prove mixed target/no-target boundaries");
        assert!(shape.already_ee_shaped);
        assert_eq!(
            try_get_verified_ee_looping_visual_effect_entries_end_with_rows(
                &record,
                LEGACY_UPDATE_HEADER_BYTES + 2,
                2,
                record.len(),
                &rows,
            ),
            Some(record.len())
        );
    }

    #[test]
    fn loaded_visualeffects_rows_reject_absent_row_policy() {
        let rows = vec![Some(0)];
        let mut record = vec![
            b'U',
            CREATURE_OBJECT_TYPE,
            0x0F,
            0x00,
            0x00,
            0x80,
            0x08,
            0x00,
            0x00,
            0x00,
            0x01,
            0x00,
            b'A',
            0x34,
            0x12,
        ];
        record.extend_from_slice(&LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES);

        assert!(
            parse_looping_visual_effect_entries_with_rows(
                &record,
                LEGACY_UPDATE_HEADER_BYTES + 2,
                1,
                record.len(),
                &rows,
            )
            .is_none(),
            "once visualeffects.2da is loaded, absent rows are not guessed"
        );
    }
}
