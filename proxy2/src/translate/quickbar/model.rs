//! Shared quickbar data model.
//!
//! The quickbar translator is one of the highest-risk heuristic surfaces in the
//! bridge.  Keeping the parsed shapes in this small module helps preserve the
//! intended layering: transport and split search may discover a complete legacy
//! record, but reader/writer code owns the typed shape and exact EE emission.

use super::constants::NWN_OBJECT_INVALID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemMaterializationProof {
    ExplicitSelfMaterialization,
    ActiveObject,
    InventoryFeature25FirstList,
    InventoryFeature25SecondList,
    InventoryFeature25LegacyTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuickbarItemMaterializationStatus {
    Proven(QuickbarItemMaterializationProof),
    DeferredFeature25(QuickbarItemMaterializationProof),
    ClearedByItemDelete,
    ClearedByAreaReset,
    Unknown,
}

/// Session-state proof used by the EE quickbar writer when deciding whether a
/// parsed legacy item button may be emitted as an EE item button.
///
/// The quickbar reader/writer remains packet-pure: it does not own the object
/// registry and it does not decide game truth. The gateway may provide this
/// narrow predicate after verified live-object, GUI item-create, or exact
/// inventory Feature-25 packets have populated a wire-derived item context.
/// Compact byte-owned item slots require that state proof before emission. Full
/// explicit type-1 item bodies may also be self-materializing: EE
/// `sub_14079DB00` calls `sub_14079FAC0`, constructs a client item object when
/// the id is not already present, then registers it with
/// `CGameObjectArray::AddExternalObject` before applying the quickbar slot.
/// Missing-source-type recovered item bodies still remain blanked by policy.
pub struct QuickbarMaterializationContext<'a> {
    item_object_proof: Option<&'a dyn Fn(u32) -> Option<QuickbarItemMaterializationProof>>,
    item_object_status: Option<&'a dyn Fn(u32) -> QuickbarItemMaterializationStatus>,
    context_summary: Option<QuickbarMaterializationContextSummary>,
}

impl<'a> QuickbarMaterializationContext<'a> {
    pub fn new_with_proof(
        item_object_proof: &'a dyn Fn(u32) -> Option<QuickbarItemMaterializationProof>,
    ) -> Self {
        Self {
            item_object_proof: Some(item_object_proof),
            item_object_status: None,
            context_summary: None,
        }
    }

    pub fn new_with_status(
        item_object_status: &'a dyn Fn(u32) -> QuickbarItemMaterializationStatus,
    ) -> Self {
        Self {
            item_object_proof: None,
            item_object_status: Some(item_object_status),
            context_summary: None,
        }
    }

    pub fn new_with_status_and_summary(
        item_object_status: &'a dyn Fn(u32) -> QuickbarItemMaterializationStatus,
        context_summary: QuickbarMaterializationContextSummary,
    ) -> Self {
        Self {
            item_object_proof: None,
            item_object_status: Some(item_object_status),
            context_summary: Some(context_summary),
        }
    }

    pub(in crate::translate::quickbar) fn item_object_materialization_proof(
        &self,
        object_id: u32,
    ) -> Option<QuickbarItemMaterializationProof> {
        match self.item_object_materialization_status(object_id) {
            QuickbarItemMaterializationStatus::Proven(proof) => Some(proof),
            QuickbarItemMaterializationStatus::DeferredFeature25(_)
            | QuickbarItemMaterializationStatus::ClearedByItemDelete
            | QuickbarItemMaterializationStatus::ClearedByAreaReset
            | QuickbarItemMaterializationStatus::Unknown => None,
        }
    }

    pub(in crate::translate::quickbar) fn item_object_materialization_status(
        &self,
        object_id: u32,
    ) -> QuickbarItemMaterializationStatus {
        if let Some(item_object_status) = self.item_object_status {
            return item_object_status(object_id);
        }
        self.item_object_proof
            .and_then(|item_object_proof| item_object_proof(object_id))
            .map(QuickbarItemMaterializationStatus::Proven)
            .unwrap_or(QuickbarItemMaterializationStatus::Unknown)
    }

    pub(in crate::translate::quickbar) fn context_summary(
        &self,
    ) -> Option<QuickbarMaterializationContextSummary> {
        self.context_summary
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuickbarMaterializationContextSummary {
    pub active_item_objects: usize,
    pub materialized_item_objects: usize,
    pub direct_item_proof_objects: usize,
    pub feature25_item_proof_objects: usize,
    pub compact_item_emission_proof_objects: usize,
    pub compact_item_emission_direct_only_proof_objects: usize,
    pub compact_item_emission_feature25_only_proof_objects: usize,
    pub compact_item_emission_shared_proof_objects: usize,
    pub inventory_feature25_first_item_refs: usize,
    pub inventory_feature25_second_item_refs: usize,
    pub inventory_feature25_legacy_tail_item_refs: usize,
    pub cleared_inventory_item_object_ids: usize,
    pub inventory_feature25_reference_records: u64,
    pub inventory_feature25_first_item_ref_mentions: u64,
    pub inventory_feature25_second_item_ref_mentions: u64,
    pub inventory_feature25_legacy_tail_item_ref_mentions: u64,
    pub inventory_feature25_first_materialized_item_ref_mentions: u64,
    pub inventory_feature25_first_deferred_item_ref_mentions: u64,
    pub inventory_feature25_second_materialized_item_ref_mentions: u64,
    pub inventory_feature25_second_deferred_item_ref_mentions: u64,
    pub inventory_feature25_legacy_tail_materialized_item_ref_mentions: u64,
    pub inventory_feature25_legacy_tail_deferred_item_ref_mentions: u64,
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
    pub slot_records_owned: u32,
    pub item_buttons_seen: u32,
    pub item_buttons_source_explicit: u32,
    pub item_buttons_source_compact: u32,
    pub item_buttons_source_recovered: u32,
    pub item_buttons_preserved: u32,
    pub spells_preserved: u32,
    pub blank_buttons_seen: u32,
    pub general_buttons_preserved: u32,
    pub general_buttons_blanked: u32,
    pub item_buttons_blanked: u32,
    pub item_buttons_blanked_candidate: u32,
    pub unsupported_buttons_blanked: u32,
    pub item_buttons_rejected_recovered_type_tag: u32,
    pub item_buttons_rejected_missing_type_source: u32,
    pub item_buttons_rejected_no_present_item: u32,
    pub item_buttons_rejected_invalid_object_id: u32,
    pub item_buttons_rejected_missing_active_properties: u32,
    pub item_buttons_rejected_unsupported_appearance_type: u32,
    pub item_buttons_rejected_appearance_shape: u32,
    pub item_buttons_rejected_missing_state_proof: u32,
    pub item_buttons_rejected_missing_state_unknown: u32,
    pub item_buttons_rejected_missing_state_cleared_delete: u32,
    pub item_buttons_rejected_missing_state_cleared_area_reset: u32,
    pub item_objects_rejected_missing_state_proven: u32,
    pub item_objects_rejected_missing_state_active: u32,
    pub item_objects_rejected_missing_state_feature25_first: u32,
    pub item_objects_rejected_missing_state_feature25_second: u32,
    pub item_objects_rejected_missing_state_feature25_legacy_tail: u32,
    pub item_objects_rejected_missing_state_unknown: u32,
    pub item_objects_rejected_missing_state_cleared_delete: u32,
    pub item_objects_rejected_missing_state_cleared_area_reset: u32,
    pub item_objects_preserved_by_explicit_self_materialization: u32,
    pub item_objects_preserved_by_active_state: u32,
    pub item_objects_preserved_by_feature25_first: u32,
    pub item_objects_preserved_by_feature25_second: u32,
    pub item_objects_preserved_by_feature25_legacy_tail: u32,
    pub(crate) first_preserved_active_item_signature: Option<QuickbarActiveItemSignature>,
    pub(crate) first_preserved_active_item_slot: Option<u8>,
    pub(crate) validated_slot_profile: Option<QuickbarValidatedSlotProfile>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QuickbarActivePropertySignature {
    pub(crate) property: u16,
    pub(crate) subtype: u16,
    pub(crate) cost_table_value: u16,
    pub(crate) param: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QuickbarActiveItemSignature {
    pub(crate) object_id: u32,
    pub(crate) base_item: u32,
    pub(crate) appearance_type: i8,
    pub(crate) active_property_count: u32,
    pub(crate) first_property: Option<QuickbarActivePropertySignature>,
    pub(crate) has_armor_word: bool,
    pub(crate) name_is_locstring: bool,
    pub(crate) state_mask: u8,
    pub(crate) value_mask: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct QuickbarValidatedSlotProfile {
    pub(crate) slot_records: u32,
    pub(crate) blank_slots: u32,
    pub(crate) item_slots: u32,
    pub(crate) spell_slots: u32,
    pub(crate) general_slots: u32,
    pub(crate) first_blank_slot: Option<u8>,
    pub(crate) first_item_slot: Option<u8>,
    pub(crate) first_page_visible_slots: u32,
    pub(crate) first_page_item_slots: u32,
    pub(crate) first_page_spell_slots: u32,
}

impl QuickbarValidatedSlotProfile {
    pub(in crate::translate::quickbar) fn from_slot_types(slot_types: &[u8]) -> Self {
        let mut profile = Self {
            slot_records: u32::try_from(slot_types.len()).unwrap_or(u32::MAX),
            ..Self::default()
        };
        for (index, slot_type) in slot_types.iter().copied().enumerate() {
            match slot_type {
                0 => {
                    profile.blank_slots = profile.blank_slots.saturating_add(1);
                    if profile.first_blank_slot.is_none() {
                        profile.first_blank_slot = u8::try_from(index).ok();
                    }
                }
                1 => {
                    profile.item_slots = profile.item_slots.saturating_add(1);
                    if profile.first_item_slot.is_none() {
                        profile.first_item_slot = u8::try_from(index).ok();
                    }
                }
                2 => profile.spell_slots = profile.spell_slots.saturating_add(1),
                _ => profile.general_slots = profile.general_slots.saturating_add(1),
            }
            if index < 12 && slot_type != 0 {
                profile.first_page_visible_slots =
                    profile.first_page_visible_slots.saturating_add(1);
                if slot_type == 1 {
                    profile.first_page_item_slots = profile.first_page_item_slots.saturating_add(1);
                } else if slot_type == 2 {
                    profile.first_page_spell_slots =
                        profile.first_page_spell_slots.saturating_add(1);
                }
            }
        }
        profile
    }
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
