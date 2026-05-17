//! Creature `A/5` live-object add-record rewrite.
//!
//! Diamond/1.69 creature adds carry the `A`, creature type, object id, six
//! transform floats, and a final WORD in 32 read-buffer bytes. EE's matching
//! reader continues at that cursor into `ObjectVisualTransformData::Read`
//! (`sub_140973160`). For EE build `2001/0x23`, the decompiled writer emits an
//! identity object transform as two zero map counts, not as the legacy 40-byte
//! `CAurObjectVisualTransformData` scalar block.

use super::creature;

const LEGACY_CREATURE_ADD_RECORD_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureAddVisualTransformRewrite {
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
}

pub(super) fn insert_ee_visual_transform_for_legacy_creature_add(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
) -> Option<CreatureAddVisualTransformRewrite> {
    if creature::looks_like_ee_creature_add_record(bytes, offset, *record_end) {
        return None;
    }
    if bytes.get(offset).copied()? != b'A' || bytes.get(offset + 1).copied()? != 0x05 {
        return None;
    }

    let visual_offset = offset.checked_add(LEGACY_CREATURE_ADD_RECORD_BYTES)?;
    if *record_end != visual_offset
        || !creature::looks_like_legacy_creature_add_transform_fields(bytes, offset, *record_end)
    {
        return None;
    }

    if super::visual_transform::has_legacy_scalar_visual_transform_identity_at(
        bytes,
        visual_offset,
        bytes.len(),
    ) {
        let bytes_removed =
            super::visual_transform::replace_legacy_scalar_identity_with_ee_object_identity(
                bytes,
                visual_offset,
                bytes.len(),
            )?;
        *record_end = visual_offset.checked_add(
            super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN,
        )?;
        return Some(CreatureAddVisualTransformRewrite {
            bytes_inserted: super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN,
            bytes_removed,
        });
    }

    let bytes_inserted =
        super::visual_transform::insert_ee_object_visual_transform_identity(bytes, visual_offset, record_end)?;

    Some(CreatureAddVisualTransformRewrite {
        bytes_inserted,
        bytes_removed: 0,
    })
}
