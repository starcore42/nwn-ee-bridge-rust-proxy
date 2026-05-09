//! Creature `A/5` live-object add-record rewrite.
//!
//! Diamond/1.69 creature adds carry the `A`, creature type, object id, six
//! transform floats, and a final WORD in 32 read-buffer bytes. EE's matching
//! reader continues at that cursor into `ObjectVisualTransformData::Read`, so
//! the bridge inserts the legacy-build identity visual-transform map before
//! strict validation can claim the record.

use super::creature;

const LEGACY_CREATURE_ADD_RECORD_BYTES: usize = 32;
const EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES: [u8; 40] = [
    0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureAddVisualTransformRewrite {
    pub bytes_inserted: usize,
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

    bytes.splice(
        visual_offset..visual_offset,
        EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES,
    );
    *record_end = (*record_end).checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;

    Some(CreatureAddVisualTransformRewrite {
        bytes_inserted: EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len(),
    })
}
