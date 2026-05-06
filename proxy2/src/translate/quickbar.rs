//! Quickbar packet transforms.
//!
//! The EE decompile identifies high-level `0x1E01` as
//! `GuiQuickbar_SetAllButtons` and `0x1E02` as `GuiQuickbar_SetButton`.
//! `CNWSMessage::SendServerToPlayerGuiQuickbar_SetButton` builds a
//! `CNWMessage`, obtains its write buffer with `GetWriteMessage`, then sends
//! family `0x1E` with minor `1` for the full bar or `2` for one button.
//!
//! Some 1.69 server captures carry the first four CNW fragment bytes directly
//! after `P 1E minor` instead of at EE's declared fragment offset. Given that
//! verified legacy quickbar transport shape, emit the EE CNW shape and leave
//! the button semantics untouched.

use crate::packet::m::HighLevel;

use super::cnw_message::{self, PrefixedFragmentsNormalizeSummary};

pub fn normalize_quickbar_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    cnw_message::normalize_prefixed_fragments_payload_for(payload, is_quickbar_family)
}

fn is_quickbar_family(high: HighLevel) -> bool {
    matches!(
        (high.major, high.minor),
        (0x1E, 0x01) | (0x1E, 0x02)
    )
}
