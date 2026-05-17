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
//! Some visual-effect rows take a compact target payload before the EE transform
//! map. EE `HandleServerToPlayerUpdateVisualEffects` (`sub_1407B1F00`) performs
//! the row-type check, optionally reads the target payload at
//! `0x1407B214A..0x1407B2165`, then always checks build `2001/0x0E` and reads
//! `ObjectVisualTransformData` at `0x1407B218F..0x1407B21AE`. Until resource
//! state exposes the client `visualeffects.2da` row type, only the exact
//! single-entry compact-target shape observed in Starcore5 captures is accepted;
//! multi-entry target-payload lists remain quarantined rather than guessed.

use super::{
    CREATURE_OBJECT_TYPE, DOOR_OBJECT_TYPE, LEGACY_UPDATE_HEADER_BYTES, PLACEABLE_OBJECT_TYPE,
    read_u16_le, read_u32_le,
};

pub(super) const LOOPING_VISUAL_EFFECT_UPDATE_MASK: u32 = 0x0000_0008;
const MAX_REASONABLE_LOOPING_EFFECT_CHANGES: u16 = 256;
const LOOPING_EFFECT_SHORT_ENTRY_BYTES: usize = 3;
const LOOPING_EFFECT_COMPACT_TARGET_PAYLOAD_BYTES: usize = 3;
const MAX_COMPACT_TARGET_PAYLOAD_ENTRIES_WITHOUT_2DA: u16 = 1;
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
    try_get_verified_ee_looping_visual_effect_entries_end(bytes, cursor, count, scan_end, 0)
        .or_else(|| {
            if count <= MAX_COMPACT_TARGET_PAYLOAD_ENTRIES_WITHOUT_2DA {
                return try_get_verified_ee_looping_visual_effect_entries_end(
                    bytes,
                    cursor,
                    count,
                    scan_end,
                    LOOPING_EFFECT_COMPACT_TARGET_PAYLOAD_BYTES,
                );
            }
            None
        })
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

    let cursor = offset + LEGACY_UPDATE_HEADER_BYTES + 2;
    if let Some(shape) = parse_looping_visual_effect_entries(bytes, cursor, count, record_end, 0)
    {
        return Some(shape);
    }

    if count <= MAX_COMPACT_TARGET_PAYLOAD_ENTRIES_WITHOUT_2DA {
        return parse_looping_visual_effect_entries(
            bytes,
            cursor,
            count,
            record_end,
            LOOPING_EFFECT_COMPACT_TARGET_PAYLOAD_BYTES,
        );
    }

    None
}

fn parse_looping_visual_effect_entries(
    bytes: &[u8],
    mut cursor: usize,
    count: u16,
    record_end: usize,
    compact_target_payload_bytes: usize,
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
        if compact_target_payload_bytes != 0 {
            cursor = cursor.checked_add(compact_target_payload_bytes)?;
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
    compact_target_payload_bytes: usize,
) -> Option<usize> {
    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
        if compact_target_payload_bytes != 0 {
            cursor = cursor.checked_add(compact_target_payload_bytes)?;
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

fn has_identity_transform_at(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    let end = offset.saturating_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len());
    end <= record_end
        && end <= bytes.len()
        && bytes[offset..end] == LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES
}
