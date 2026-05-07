//! EE live-object update writers.

use super::{
    reader::LegacyNamedUpdateTail, EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES,
    EE_UPDATE_SCALE_STATE_READ_BYTES, LEGACY_UPDATE_ORIENTATION_MASK,
    LEGACY_UPDATE_SCALE_STATE_MASK,
};

pub(super) fn build_ee_door_placeable_generic_update_bytes(
    legacy_tail: LegacyNamedUpdateTail,
    translated_mask: u32,
) -> Vec<u8> {
    let mut rewritten = Vec::with_capacity(
        EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES + EE_UPDATE_SCALE_STATE_READ_BYTES,
    );
    if (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let scalar12 = encode_ee_scalar_orientation_from_legacy_facing(legacy_tail.facing);
        rewritten.push(((scalar12 >> 4) & 0xFF) as u8);
    }
    if (translated_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        rewritten.extend_from_slice(&legacy_tail.scale_raw.to_le_bytes());
        rewritten.extend_from_slice(&legacy_tail.generic_state_word.to_le_bytes());
    }
    rewritten
}

pub(super) fn encode_ee_scalar_orientation_from_legacy_facing(facing: u16) -> u16 {
    let degrees = f64::from(facing) * 360.0 / 65536.0;
    let raw = (degrees * 10.0 + 0.000001).floor() as u32;
    raw.min(0x0FFF) as u16
}
