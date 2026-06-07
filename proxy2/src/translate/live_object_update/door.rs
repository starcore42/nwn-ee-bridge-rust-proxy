//! Door-specific live-object update policy.

use super::{
    LEGACY_UPDATE_APPEARANCE_MASK, LEGACY_UPDATE_ORIENTATION_MASK, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_SCALE_STATE_MASK, LEGACY_UPDATE_STATE_MASK,
};

pub(super) fn translate_update_mask(raw_mask: u32) -> u32 {
    // EE `sub_14079C050` handles mask bit 0x0002 by first reading a BOOL:
    // false selects the compact scalar `ReadFLOAT(10.0, 12)` facing branch,
    // true selects the three-component orientation-vector branch. Legacy
    // compact door updates observed in HG/local captures can carry the facing
    // WORD in the generic tail instead of the normal Diamond server
    // `0x445160` orientation-BOOL path; the typed update writer converts that
    // WORD to EE's scalar branch. Do not drop this bit here, or doors keep
    // stale/default facing and visibly miss their frame.
    // Diamond's generic update writer has a legacy 0x0008_0000 name/locstring
    // branch (`nwserver` around 0x446061), but EE's generic update reader/writer
    // pair (`sub_14079C050` / `WriteGameObjUpdate_UpdateObject`) has no bit-13
    // consumer in this packet family. Keep that legacy field as input-only until
    // a separate EE semantic packet is proven from the decompile.
    // Local XP2 captures also show low 0x40/0x80 door update bits with the same
    // bounded control tail used by placeables. EE's shared generic reader and
    // door-specific reader (`sub_140797780`) consume neither bit; Diamond's
    // matching client-reader pair (`sub_467AE0` / `sub_44E2C0`) consumes only
    // the documented generic bits plus five state BOOLs. The record rewriter
    // drops the suffix only after the exact typed prefix and following boundary
    // are proven.
    raw_mask
        & (LEGACY_UPDATE_POSITION_MASK
            | LEGACY_UPDATE_ORIENTATION_MASK
            | LEGACY_UPDATE_SCALE_STATE_MASK
            | LEGACY_UPDATE_APPEARANCE_MASK
            | LEGACY_UPDATE_STATE_MASK)
}
