//! Placeable-specific live-object update policy.

use super::{
    LEGACY_UPDATE_APPEARANCE_MASK, LEGACY_UPDATE_ORIENTATION_MASK, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_SCALE_STATE_MASK, LEGACY_UPDATE_STATE_MASK,
};

pub(super) fn translate_update_mask(raw_mask: u32) -> u32 {
    // EE `sub_14079C050` handles mask bit 0x0002 by first reading a BOOL:
    // false selects the compact scalar `ReadFLOAT(10.0, 12)` facing branch,
    // true selects the three-component orientation-vector branch. Legacy
    // compact placeable updates observed in HG/local captures can carry the
    // facing WORD in the generic tail instead of the normal Diamond server
    // `0x445160` orientation-BOOL path; the typed update writer converts that
    // WORD to EE's scalar branch. Dropping this bit makes static signs/boards
    // keep stale/default orientation.
    // EE's shared `sub_14079C050` reader has no name branch, but dispatcher
    // `sub_1407B8380` sends object type 0x09 to placeable-specific
    // `sub_140797780`, which tests mask 0x0008_0000 and reads the same
    // selector + locstring/CExoString family as Diamond `sub_44EB40`. The exact
    // EE reader model therefore accepts bounded direct-name placeable updates.
    // Translation still drops this source bit until the capture-backed packed
    // CExoString control fragments are fully owned across nonterminal records.
    // Local CEP v2.2 and XP2 captures also show low 0x40/0x80 placeable update
    // bits with a bounded name/control tail after the shared generic prefix.
    // Neither EE's shared reader (`sub_14079C050`) nor its placeable-specific
    // reader (`sub_140797780`) consumes those low bits; Diamond's matching
    // client-reader pair (`sub_467AE0` / `sub_44EB40`) likewise only consumes
    // 0x10 and 0x80000 in the placeable-specific leg. The typed record rewriter
    // owns and drops that tail before this translated mask is emitted.
    raw_mask
        & (LEGACY_UPDATE_POSITION_MASK
            | LEGACY_UPDATE_ORIENTATION_MASK
            | LEGACY_UPDATE_SCALE_STATE_MASK
            | LEGACY_UPDATE_APPEARANCE_MASK
            | LEGACY_UPDATE_STATE_MASK)
}
