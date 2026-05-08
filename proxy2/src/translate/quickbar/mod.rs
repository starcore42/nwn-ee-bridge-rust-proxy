//! Strict, decompile-backed `GuiQuickbar` translation.
//!
//! This module is deliberately split by responsibility. Transport repair and
//! split selection may be heuristic while we learn from captures, but the reader
//! and writer stay decompile-owned: they parse the verified legacy shape into a
//! typed model, then emit the exact EE-side shape. Unknown item/slot layouts are
//! consumed and emitted as empty slots instead of being forwarded raw.

use crate::{
    crc::read_le_u32,
    packet::m::{HighLevel, MAX_REASONABLE_GAMEPLAY_PAYLOAD},
};

use super::cnw_message::PrefixedFragmentsNormalizeSummary;
use std::{
    fs,
    path::PathBuf,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
const QUICKBAR_MAJOR: u8 = 0x1E;
const SET_ALL_BUTTONS_MINOR: u8 = 0x01;
const LEGACY_QUICKBAR_BUTTON_COUNT: usize = 36;
const LEGACY_QUICKBAR_READ_CURSOR_START: usize = 4;
const C_RESREF_TEXT_BYTES: usize = 16;
const MAX_REASONABLE_QUICKBAR_STRING_BYTES: usize = 4096;
const MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES: usize = 32 * 1024;
const MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES: u8 = 128;
const MAX_QUICKBAR_ITEM_PRESENCE_RESYNC_BITS: u8 = 5;
const MAX_QUICKBAR_BARE_ACTIVE_ITEM_NAME_BYTES: usize = 128;
const MAX_QUICKBAR_FOUR_PREFIX_FRAGMENT_TAIL_BYTES: usize = 512;
const QUICKBAR_BAD_SCORE: i32 = -1_000_000;
const QUICKBAR_UNKNOWN_SCORE: i32 = i32::MIN;
const EE_SERVER_OBJECT_ID_MARKER_BIT: u32 = 0x8000_0000;
const NWN_OBJECT_INVALID: u32 = 0x7F00_0000;
const EE_QUICKBAR_ANIMATION_ICON_COUNT: u32 = 23;
const NWN_BASE_ITEM_ARMOR: u32 = 0x10;
const EE_QUICKBAR_ARMOR_EXACT_COPY_BYTES: usize = 4 + 19 + 6;
const EE_QUICKBAR_ARMOR_EXTRA_TABLE_ZERO_DWORDS: usize = 18;
const EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_DWORDS: [u32; 3] = [0, 0x3F80_0000, 0];
const BASEITEMS_2DA_NAME: &str = "baseitems.2da";
const HG_REQUIRED_FILES_DIR: &str = "HG REQUIRED FILES";

static QUICKBAR_BASE_ITEM_MODEL_TYPES: OnceLock<Option<Vec<i8>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct QuickbarRewriteSummary {
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub old_declared: u32,
    pub new_declared: u32,
    pub read_size: usize,
    pub fragment_size: usize,
    pub final_cursor: usize,
    pub trailing_read_bytes: usize,
    pub direct_opcode_stream: bool,
    pub item_buttons_preserved: u32,
    pub spells_preserved: u32,
    pub general_buttons_preserved: u32,
    pub general_buttons_blanked: u32,
    pub item_buttons_blanked: u32,
    pub unsupported_buttons_blanked: u32,
}

#[derive(Debug, Clone)]
struct QuickbarParse {
    envelope: u8,
    declared: u32,
    read_size: usize,
    fragment_size: usize,
    final_cursor: usize,
    buttons: Vec<QuickbarButton>,
    direct_opcode_stream: bool,
}

#[derive(Debug, Clone)]
struct QuickbarButton {
    kind: QuickbarButtonKind,
}

#[derive(Debug, Clone)]
enum QuickbarButtonKind {
    Item {
        primary: QuickbarItemObject,
        secondary: QuickbarItemObject,
        recovered_type_tag: bool,
    },
    Spell {
        spell_class: u8,
        spell_id: u32,
        metamagic: u8,
        domain: u8,
    },
    General {
        bytes: Vec<u8>,
    },
    ItemCandidate,
    Unsupported,
}

#[derive(Debug, Clone, Default)]
struct QuickbarLocStringField {
    custom_tlk: bool,
    language_selector: bool,
    string_ref: u32,
    text: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct QuickbarActivePropertyEntry {
    property: u16,
    subtype: u16,
    cost_table_value: u16,
    param: u8,
}

#[derive(Debug, Clone, Default)]
struct QuickbarActiveItemProperties {
    has_armor_word: bool,
    armor_word: u16,
    name_is_locstring: bool,
    locstring_name: QuickbarLocStringField,
    string_name: Vec<u8>,
    post_name_bool1: bool,
    cost: u32,
    stack_or_charges: u32,
    post_name_bool2: bool,
    post_name_bool3: bool,
    post_name_bool4: bool,
    properties: Vec<QuickbarActivePropertyEntry>,
    state_mask: u8,
    value_mask: u8,
    value_mask_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct QuickbarItemObject {
    present: bool,
    object_id: u32,
    int_param: i32,
    base_item: u32,
    appearance_type: i8,
    active_props: Option<QuickbarActiveItemProperties>,
    appearance_bytes: Vec<u8>,
}

impl Default for QuickbarItemObject {
    fn default() -> Self {
        Self {
            present: false,
            object_id: NWN_OBJECT_INVALID,
            int_param: -1,
            base_item: 0,
            appearance_type: 0,
            active_props: None,
            appearance_bytes: Vec::new(),
        }
    }
}

mod active_props;
mod baseitems;
mod facade;
mod fragments;
mod item;
mod reader;
mod split;
mod transport;
mod wire;
mod writer;

#[cfg(test)]
mod tests;

use active_props::*;
use baseitems::*;
use fragments::*;
use item::*;
use reader::*;
use split::*;
use transport::*;
use wire::*;

pub use facade::{
    full_set_all_buttons_target_length, normalize_and_rewrite_quickbar_payload_if_possible,
    rewrite_simple_quickbar_payload_if_possible, rewrite_summary_needs_more_quickbar_bytes,
};
pub use writer::build_blank_set_all_buttons_payload;
