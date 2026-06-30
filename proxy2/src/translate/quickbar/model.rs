//! Shared quickbar data model.
//!
//! The quickbar translator is one of the highest-risk heuristic surfaces in the
//! bridge.  Keeping the parsed shapes in this small module helps preserve the
//! intended layering: transport and split search may discover a complete legacy
//! record, but reader/writer code owns the typed shape and exact EE emission.

use super::constants::NWN_OBJECT_INVALID;

/// Session-state proof used by the EE quickbar writer when deciding whether a
/// parsed legacy item button may be emitted as an EE item button.
///
/// The quickbar reader/writer remains packet-pure: it does not own the object
/// registry and it does not decide game truth. The gateway may provide this
/// narrow predicate after verified live-object, GUI item-create, or exact
/// inventory Feature-25 packets have populated a wire-derived item context. Full
/// explicit type-1 item bodies may also be self-materializing: EE
/// `sub_14079DB00` calls `sub_14079FAC0`, constructs a client item object when
/// the id is not already present, then registers it with
/// `CGameObjectArray::AddExternalObject` before applying the quickbar slot.
/// Ambiguous/recovered compact item bodies still remain blanked by policy.
pub struct QuickbarMaterializationContext<'a> {
    item_object_is_known: &'a dyn Fn(u32) -> bool,
}

impl<'a> QuickbarMaterializationContext<'a> {
    pub fn new(item_object_is_known: &'a dyn Fn(u32) -> bool) -> Self {
        Self {
            item_object_is_known,
        }
    }

    pub(in crate::translate::quickbar) fn item_object_is_known(&self, object_id: u32) -> bool {
        (self.item_object_is_known)(object_id)
    }
}

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
        source: QuickbarItemSource,
        recovered_type_tag: bool,
    },
    Spell {
        class_byte: u8,
        spell_id: u32,
        legacy_metamagic: u8,
        legacy_level: u8,
    },
    General {
        bytes: Vec<u8>,
    },
    ItemCandidate,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::translate::quickbar) enum QuickbarItemSource {
    /// Normal decompile-owned source: the slot carried type byte `1`, then the
    /// primary/secondary item BOOL/object bodies were read through the CNW
    /// fragment cursor.
    ExplicitTypeAndFragmentBits,

    /// Compatibility source: the slot carried type byte `1`, but the legacy
    /// source only became bounded through the compact byte-owned item parser
    /// after fragment-bit ownership failed. This proves the quickbar boundary
    /// but not EE-visible item materialization.
    CompactByteOwnedWithSourceType,

    /// Compatibility source: the item body was recovered at a slot boundary
    /// without a source type byte. This proves the quickbar boundary only.
    RecoveredMissingType,
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
