//! Looping visual-effect update-list translation for generic live objects.
//!
//! EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` handles update-mask
//! `0x00000008` as a compact change list:
//!
//! * WORD change count
//! * for each entry: BYTE change opcode (`A`/`D`) and WORD visual-effect row
//! * for current EE builds, `ObjectVisualTransformData::Write` after each entry
//!
//! Diamond/HG legacy captures carry the same count/opcode/row short form but
//! do not carry the EE visual-transform map.  This module keeps that semantic
//! expansion isolated from the generic door/placeable update-mask logic.

use super::{
    DOOR_OBJECT_TYPE, LEGACY_UPDATE_HEADER_BYTES, PLACEABLE_OBJECT_TYPE, read_u16_le,
    read_u32_le,
};

pub(super) const LOOPING_VISUAL_EFFECT_UPDATE_MASK: u32 = 0x0000_0008;
const MAX_REASONABLE_LOOPING_EFFECT_CHANGES: u16 = 256;
const LOOPING_EFFECT_SHORT_ENTRY_BYTES: usize = 3;
const LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES: [u8; 40] = [
    0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F,
];

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
    for insert_at in shape.insert_offsets_after_short_entries.iter().rev().copied() {
        bytes.splice(insert_at..insert_at, LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES);
        *record_end = record_end.checked_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len())?;
        bytes_inserted = bytes_inserted
            .saturating_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len() as u32);
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
            PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
        )
        || read_u32_le(bytes, offset + 6)? != LOOPING_VISUAL_EFFECT_UPDATE_MASK
    {
        return None;
    }

    let count = read_u16_le(bytes, offset + LEGACY_UPDATE_HEADER_BYTES)?;
    if count > MAX_REASONABLE_LOOPING_EFFECT_CHANGES {
        return None;
    }

    let mut cursor = offset + LEGACY_UPDATE_HEADER_BYTES + 2;
    let mut insert_offsets_after_short_entries = Vec::with_capacity(usize::from(count));
    let mut transform_maps_seen = 0usize;

    for _ in 0..usize::from(count) {
        let change_opcode = *bytes.get(cursor)?;
        if !matches!(change_opcode, b'A' | b'D') {
            return None;
        }
        read_u16_le(bytes, cursor + 1)?;
        cursor = cursor.checked_add(LOOPING_EFFECT_SHORT_ENTRY_BYTES)?;
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

fn has_identity_transform_at(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    let end = offset.saturating_add(LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES.len());
    end <= record_end && end <= bytes.len() && bytes[offset..end] == LOOPING_EFFECT_IDENTITY_TRANSFORM_BYTES
}
