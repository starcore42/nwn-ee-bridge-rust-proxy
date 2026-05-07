//! Placeable-specific live-object update policy.

use super::{
    LEGACY_UPDATE_NAME_MASK, LEGACY_UPDATE_POSITION_MASK, LEGACY_UPDATE_SCALE_STATE_MASK,
    LEGACY_UPDATE_STATE_MASK,
};

pub(super) fn translate_update_mask(raw_mask: u32) -> u32 {
    raw_mask
        & (LEGACY_UPDATE_POSITION_MASK
            | LEGACY_UPDATE_SCALE_STATE_MASK
            | LEGACY_UPDATE_STATE_MASK
            | LEGACY_UPDATE_NAME_MASK)
}
