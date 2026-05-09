//! Shared quickbar data model.
//!
//! The quickbar translator is one of the highest-risk heuristic surfaces in the
//! bridge.  Keeping the parsed shapes in this small module helps preserve the
//! intended layering: transport and split search may discover a complete legacy
//! record, but reader/writer code owns the typed shape and exact EE emission.

use super::constants::NWN_OBJECT_INVALID;

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
pub(in crate::translate::quickbar) struct QuickbarParse {
    pub(in crate::translate::quickbar) envelope: u8,
    pub(in crate::translate::quickbar) declared: u32,
    pub(in crate::translate::quickbar) read_size: usize,
    pub(in crate::translate::quickbar) fragment_size: usize,
    pub(in crate::translate::quickbar) final_cursor: usize,
    pub(in crate::translate::quickbar) buttons: Vec<QuickbarButton>,
    pub(in crate::translate::quickbar) direct_opcode_stream: bool,
}

#[derive(Debug, Clone)]
pub(in crate::translate::quickbar) struct QuickbarButton {
    pub(in crate::translate::quickbar) kind: QuickbarButtonKind,
}

#[derive(Debug, Clone)]
pub(in crate::translate::quickbar) enum QuickbarButtonKind {
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
pub(in crate::translate::quickbar) struct QuickbarLocStringField {
    pub(in crate::translate::quickbar) custom_tlk: bool,
    pub(in crate::translate::quickbar) language_id: u8,
    pub(in crate::translate::quickbar) string_ref: u32,
    pub(in crate::translate::quickbar) text: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::translate::quickbar) struct QuickbarActivePropertyEntry {
    pub(in crate::translate::quickbar) property: u16,
    pub(in crate::translate::quickbar) subtype: u16,
    pub(in crate::translate::quickbar) cost_table_value: u16,
    pub(in crate::translate::quickbar) param: u8,
}

#[derive(Debug, Clone, Default)]
pub(in crate::translate::quickbar) struct QuickbarActiveItemProperties {
    pub(in crate::translate::quickbar) has_armor_word: bool,
    pub(in crate::translate::quickbar) armor_word: u16,
    pub(in crate::translate::quickbar) name_is_locstring: bool,
    pub(in crate::translate::quickbar) locstring_name: QuickbarLocStringField,
    pub(in crate::translate::quickbar) string_name: Vec<u8>,
    pub(in crate::translate::quickbar) post_name_bool1: bool,
    pub(in crate::translate::quickbar) cost: u32,
    pub(in crate::translate::quickbar) stack_or_charges: u32,
    pub(in crate::translate::quickbar) post_name_bool2: bool,
    pub(in crate::translate::quickbar) post_name_bool3: bool,
    pub(in crate::translate::quickbar) post_name_bool4: bool,
    pub(in crate::translate::quickbar) properties: Vec<QuickbarActivePropertyEntry>,
    pub(in crate::translate::quickbar) state_mask: u8,
    pub(in crate::translate::quickbar) value_mask: u8,
    pub(in crate::translate::quickbar) value_mask_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(in crate::translate::quickbar) struct QuickbarItemObject {
    pub(in crate::translate::quickbar) present: bool,
    pub(in crate::translate::quickbar) object_id: u32,
    pub(in crate::translate::quickbar) int_param: i32,
    pub(in crate::translate::quickbar) base_item: u32,
    pub(in crate::translate::quickbar) appearance_type: i8,
    pub(in crate::translate::quickbar) active_props: Option<QuickbarActiveItemProperties>,
    pub(in crate::translate::quickbar) appearance_bytes: Vec<u8>,
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
