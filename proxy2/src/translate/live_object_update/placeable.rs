//! Placeable-specific live-object update policy.

use super::{
    LEGACY_UPDATE_APPEARANCE_MASK, LEGACY_UPDATE_ORIENTATION_MASK, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_SCALE_STATE_MASK, LEGACY_UPDATE_STATE_MASK,
};

pub(super) fn translate_update_mask(raw_mask: u32) -> u32 {
    // EE `sub_14079C050` handles mask bit 0x0002 by first reading a BOOL:
    // false selects the compact scalar `ReadFLOAT(10.0, 12)` facing branch,
    // true selects the three-component orientation-vector branch. Diamond/HG
    // placeable updates carry the legacy facing WORD in the generic tail; the
    // typed update writer converts that WORD to EE's scalar branch. Dropping
    // this bit makes static signs/boards keep stale/default orientation.
    // Diamond's generic update writer has a legacy 0x0008_0000 name/locstring
    // branch (`wserver` around 0x446061), but EE's generic update reader/writer
    // pair (`sub_14079C050` / `WriteGameObjUpdate_UpdateObject`) has no bit-13
    // consumer in this packet family. Keep that legacy field as input-only until
    // a separate EE semantic packet is proven from the decompile.
    // Local CEP v2.2 and XP2 captures also show low 0x40/0x80 placeable update
    // bits with a bounded name/control tail after the shared generic prefix.
    // Neither EE's shared reader (`sub_14079C050`) nor its placeable-specific
    // reader (`sub_140797780`) consumes those low bits; Diamond's matching
    // reader pair (`sub_467AE0` / `sub_44EB40`) likewise only consumes 0x10 and
    // 0x80000 in the placeable-specific leg. The typed record rewriter owns and
    // drops that tail before this translated mask is emitted.
    raw_mask
        & (LEGACY_UPDATE_POSITION_MASK
            | LEGACY_UPDATE_ORIENTATION_MASK
            | LEGACY_UPDATE_SCALE_STATE_MASK
            | LEGACY_UPDATE_APPEARANCE_MASK
            | LEGACY_UPDATE_STATE_MASK)
}
