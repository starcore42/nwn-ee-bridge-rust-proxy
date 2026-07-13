//! Strict, decompile-backed `GuiQuickbar` translation.
//!
//! This module is deliberately split by responsibility. Transport repair and
//! split selection may be heuristic while we learn from captures, but the reader
//! and writer stay decompile-owned: they parse the verified legacy shape into a
//! typed model, then emit the exact EE-side shape. Unknown item/slot layouts are
//! consumed and emitted as empty slots instead of being forwarded raw.

use crate::{crc::read_le_u32, packet::m::HighLevel};

use super::cnw_message::PrefixedFragmentsNormalizeSummary;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

mod active_props;
mod baseitems;
mod constants;
mod facade;
mod fragments;
mod general;
mod item;
mod model;
mod reader;
mod spell;
mod split;
mod transport;
mod validator;
mod wire;
mod writer;

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests;

use active_props::*;
use baseitems::*;
use constants::*;
use fragments::*;
use general::*;
use item::*;
pub(super) use model::*;
use reader::*;
use spell::*;
use split::*;
use transport::*;
use wire::*;

pub use facade::{
    full_set_all_buttons_target_length, normalize_and_rewrite_quickbar_payload_if_possible,
    normalize_and_rewrite_quickbar_payload_with_context_if_possible,
    rewrite_simple_quickbar_payload_if_possible,
    rewrite_simple_quickbar_payload_with_context_if_possible,
    rewrite_summary_needs_more_quickbar_bytes,
};
pub(crate) use facade::{
    normalize_and_rewrite_quickbar_payload_with_context_for_stream_probe_if_possible,
    quickbar_has_structurally_plausible_cnw_declared,
    rewrite_simple_quickbar_payload_with_context_for_stream_probe_if_possible,
};
pub(crate) use model::{
    QuickbarItemMaterializationProof, QuickbarItemMaterializationStatus,
    QuickbarMaterializationContext, QuickbarMaterializationContextSummary,
    QuickbarValidatedSlotProfile,
};
pub(crate) use validator::{
    ee_set_all_buttons_payload_shape_valid, validated_set_all_buttons_slot_profile,
};
pub use writer::build_blank_set_all_buttons_payload;
