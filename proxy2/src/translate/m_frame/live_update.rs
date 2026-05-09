//! M-frame-local adapters for live-object payload rewrites.
//!
//! This module intentionally does not own direct `M` frames. Direct
//! `GameObjUpdate_LiveObject` packets must route through
//! `m_frame::server_dispatch`'s semantic registry so mixed add/update payloads
//! are claimed only after the focused add-record, update-record, fragment-bit,
//! and exact validator passes all agree.

use crate::translate::live_object_update;

pub type RewriteSummary = live_object_update::LiveObjectUpdateRewriteSummary;
pub type ClaimSummary = live_object_update::LiveObjectUpdateClaimSummary;
pub type AddNameBitRewriteSummary = live_object_update::LiveObjectAddNameBitRewriteSummary;

pub fn rewrite_payload_if_needed(payload: &mut Vec<u8>) -> Option<RewriteSummary> {
    live_object_update::rewrite_update_records_payload_if_possible(payload)
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClaimSummary> {
    live_object_update::claim_payload_if_verified(payload)
}

pub fn rewrite_add_name_fragment_bits_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<AddNameBitRewriteSummary> {
    live_object_update::rewrite_add_name_fragment_bits_payload_if_possible(payload)
}
