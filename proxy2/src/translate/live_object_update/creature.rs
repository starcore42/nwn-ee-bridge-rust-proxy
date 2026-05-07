//! Creature-specific live-object update helpers.
//!
//! These helpers classify creature add/update record shapes. They deliberately
//! do not mutate bytes; transforms stay in the top-level update dispatcher and
//! writer helpers.

use super::{read_f32_le, read_u16_le, read_u32_le};

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

pub(super) fn has_ee_identity_visual_transform_map_at(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    const IDENTITY_MAP: [u8; 40] = [
        0x00, 0x00, 0x80, 0x3F,
        0x00, 0x00, 0x80, 0x3F,
        0x00, 0x00, 0x80, 0x3F,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x80, 0x3F,
    ];
    let end = offset + IDENTITY_MAP.len();
    end <= record_end && end <= bytes.len() && bytes[offset..end] == IDENTITY_MAP
}

