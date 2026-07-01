//! Wire-derived semantic session state.
//!
//! This state is a protocol coherence cache, not a game-state authority. It is
//! fed only by verified semantic packet families and should contain only the
//! facts needed to translate future traffic safely: module/resource context,
//! area/load progress, object ids/types observed on the wire, UI packet state,
//! and proxy-owned synthetic event accounting.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Instant,
};

use crate::translate::{
    VerifiedFamily,
    area::{
        AreaPlaceableContext, AreaPlaceableContextAppearanceConflict,
        AreaPlaceableContextIdentityConflict, AreaPlaceableContextOrientationConflict,
        AreaPlaceableContextOverlap, AreaPlaceableContextPositionConflict,
        AreaPlaceableContextStateConflict, AreaPlaceableObservedOrientationSource,
        AreaPlaceableObservedState,
    },
    live_object_update::{area_static_row_scalar_orientation, object_ids},
    player_list::PlayerListObjectIds,
    quickbar::QuickbarValidatedSlotProfile,
};

use super::event::{
    LiveObjectBounds, LiveObjectInventoryFeature25Reference, LiveObjectMention,
    LiveObjectOrientation, LiveObjectOrientationSource, LiveObjectPlaceableAppearance,
    LiveObjectPlaceableState, LiveObjectPosition, ProtocolEvent,
};

const MAX_RECENT_EVENTS: usize = 128;
const ITEM_OBJECT_TYPE: u8 = 0x06;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
const PLACEABLE_POSITION_EPSILON: f32 = 0.01;

#[derive(Debug, Default)]
pub(crate) struct SemanticSessionState {
    pub(crate) auth: AuthState,
    pub(crate) resources: ResourceState,
    pub(crate) module: ModuleState,
    pub(crate) area: AreaState,
    pub(crate) objects: ObjectRegistry,
    pub(crate) ui: UiState,
    pub(crate) synthetic: SyntheticState,
    pub(crate) client_input: ClientInputState,
    pub(crate) recent_events: VecDeque<ProtocolEvent>,
}

impl SemanticSessionState {
    pub(crate) fn remember_event(&mut self, event: ProtocolEvent) {
        if self.recent_events.len() >= MAX_RECENT_EVENTS {
            self.recent_events.pop_front();
        }
        self.recent_events.push_back(event);
    }
}

#[derive(Debug, Default)]
pub(crate) struct AuthState {
    pub(crate) login_packets: u64,
    pub(crate) client_input_packets: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ResourceState {
    pub(crate) module_info_seen: bool,
    pub(crate) module_resource_packets: u64,
    pub(crate) module_running_packets: u64,
}

#[derive(Debug, Default)]
pub(crate) struct ModuleState {
    pub(crate) module_info_packets: u64,
    pub(crate) module_time_packets: u64,
    pub(crate) last_module_info_declared_len: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct AreaState {
    pub(crate) client_area_packets: u64,
    pub(crate) area_loaded_packets: u64,
    pub(crate) loadbar_packets: u64,
    pub(crate) last_client_area_declared_len: Option<usize>,
    pub(crate) current_area_object_id: Option<u32>,
}

#[derive(Debug, Default)]
pub(crate) struct ClientInputState {
    pub(crate) recent_open_door_id: Option<u32>,
    pub(crate) recent_open_at: Option<Instant>,
    pub(crate) transition_door_close_rewrites: u64,
    pub(crate) transition_door_close_rewrite_skips: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryItemObjectProof {
    ActiveObject,
    Feature25FirstList,
    Feature25SecondList,
    Feature25LegacyTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InventoryItemObjectStatus {
    Proven(InventoryItemObjectProof),
    ClearedByItemDelete,
    ClearedByAreaReset,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InventoryItemObjectClearReason {
    ItemDelete,
    AreaReset,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ObjectRegistry {
    pub(crate) live_object_packets: u64,
    pub(crate) known: BTreeMap<u32, KnownObjectState>,
    session_creature_ids_by_compact: BTreeMap<u32, u32>,
    materialized_item_object_ids: BTreeSet<u32>,
    inventory_feature25_first_item_refs: BTreeSet<u32>,
    inventory_feature25_second_item_refs: BTreeSet<u32>,
    inventory_feature25_legacy_tail_item_refs: BTreeSet<u32>,
    cleared_inventory_item_object_ids: BTreeMap<u32, InventoryItemObjectClearReason>,
    pub(crate) inventory_feature25_reference_records: u64,
    pub(crate) inventory_feature25_first_item_ref_mentions: u64,
    pub(crate) inventory_feature25_second_item_ref_mentions: u64,
    pub(crate) inventory_feature25_legacy_tail_item_ref_mentions: u64,
}

impl ObjectRegistry {
    pub(crate) fn reset_for_area(&mut self) {
        if !self.known.is_empty()
            || !self.materialized_item_object_ids.is_empty()
            || !self.inventory_feature25_first_item_refs.is_empty()
            || !self.inventory_feature25_second_item_refs.is_empty()
            || !self.inventory_feature25_legacy_tail_item_refs.is_empty()
        {
            tracing::debug!(
                known_objects = self.known.len(),
                materialized_item_objects = self.materialized_item_object_ids.len(),
                inventory_feature25_first_item_refs =
                    self.inventory_feature25_first_item_refs.len(),
                inventory_feature25_second_item_refs =
                    self.inventory_feature25_second_item_refs.len(),
                inventory_feature25_legacy_tail_item_refs =
                    self.inventory_feature25_legacy_tail_item_refs.len(),
                cleared_inventory_item_object_ids = self.cleared_inventory_item_object_ids.len(),
                session_creature_aliases = self.session_creature_ids_by_compact.len(),
                "semantic object registry reset for new Area_ClientArea"
            );
        }
        self.remember_inventory_item_proofs_cleared_by_area_reset();
        self.known.clear();
        self.materialized_item_object_ids.clear();
        self.inventory_feature25_first_item_refs.clear();
        self.inventory_feature25_second_item_refs.clear();
        self.inventory_feature25_legacy_tail_item_refs.clear();
    }

    pub(crate) fn observe_player_list_object_ids(&mut self, object_ids: &[PlayerListObjectIds]) {
        for entry in object_ids {
            let Some(creature_object_id) = entry.creature_object_id else {
                continue;
            };
            let Some(compact_id) = compact_session_alias_from_player_list(creature_object_id)
            else {
                continue;
            };
            if let Some(previous) = self
                .session_creature_ids_by_compact
                .insert(compact_id, creature_object_id)
                .filter(|previous| *previous != creature_object_id)
            {
                tracing::warn!(
                    compact_id,
                    previous_session_id = previous,
                    new_session_id = creature_object_id,
                    player_object_id = entry.player_object_id,
                    "verified PlayerList remapped a compact creature session alias"
                );
            } else {
                tracing::debug!(
                    compact_id,
                    session_creature_id = creature_object_id,
                    player_object_id = entry.player_object_id,
                    "verified PlayerList established compact creature session alias"
                );
            }
        }
    }

    pub(crate) fn observe_mentions(&mut self, mentions: &[LiveObjectMention]) {
        self.live_object_packets = self.live_object_packets.saturating_add(1);
        for mention in mentions {
            let inventory_owner_without_type = mention.opcode == b'I' && mention.object_type == 0;
            let registry_object_id =
                self.registry_object_id_for_live_object(mention.object_type, mention.object_id);
            if (mention.object_id & 0xFFFF_FF00) == 0xFFFF_FF00 {
                tracing::debug!(
                    opcode = %char::from(mention.opcode),
                    object_type = mention.object_type,
                    object_id = mention.object_id,
                    "semantic object registry observing session-local live-object mention"
                );
            }
            let entry = self
                .known
                .entry(registry_object_id)
                .or_insert_with(|| KnownObjectState {
                    object_id: registry_object_id,
                    object_type: mention.object_type,
                    ..KnownObjectState::default()
                });
            if registry_object_id != mention.object_id {
                tracing::debug!(
                    opcode = %char::from(mention.opcode),
                    object_type = mention.object_type,
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    registry_object_id = format_args!("0x{registry_object_id:08X}"),
                    "live-object registry merged compact/external placeable alias"
                );
            }
            if entry.mentions != 0 && entry.object_type != mention.object_type {
                if inventory_owner_without_type {
                    // Live-object inventory `I` records carry an owner
                    // OBJECTID plus an inventory mask; the exact inventory
                    // parser reports object_type 0 because the packet does
                    // not carry an independent creature/placeable/etc. type
                    // field there.  Treat that as a typed owner reference,
                    // not as proof that an existing creature became object
                    // type zero.
                    tracing::debug!(
                        object_id = mention.object_id,
                        known_object_type = entry.object_type,
                        opcode = %char::from(mention.opcode),
                        "live-object registry kept known owner type for inventory record"
                    );
                } else if entry.object_type == 0 {
                    // A prior inventory-only owner mention created an
                    // unknown-type placeholder. The first typed add/update is
                    // the stronger wire-derived fact, so promote without an
                    // object-type-change warning.
                    tracing::debug!(
                        object_id = mention.object_id,
                        new_object_type = mention.object_type,
                        opcode = %char::from(mention.opcode),
                        "live-object registry promoted inventory-only owner to typed object"
                    );
                } else {
                    tracing::warn!(
                        object_id = mention.object_id,
                        old_object_type = entry.object_type,
                        new_object_type = mention.object_type,
                        opcode = %char::from(mention.opcode),
                        "live-object registry observed object type change"
                    );
                }
            }
            if !inventory_owner_without_type || entry.object_type == 0 {
                entry.object_type = mention.object_type;
            }
            entry.last_opcode = mention.opcode;
            if let Some(name) = mention.name.as_ref().filter(|name| !name.is_empty()) {
                entry.latest_name = Some(name.clone());
            }
            if let Some(position) = mention.position {
                entry.position = Some(position);
            }
            if let Some(orientation) = mention.orientation {
                entry.orientation = Some(orientation);
            }
            if let Some(bounds) = mention.bounds {
                entry.bounds = Some(bounds);
            }
            if let Some(placeable_appearance) = mention.placeable_appearance {
                entry.placeable_appearance = Some(placeable_appearance);
            }
            if let Some(placeable_state) = mention.placeable_state {
                entry.merge_placeable_state(placeable_state);
            }
            entry.mentions = entry.mentions.saturating_add(1);
            match mention.opcode {
                b'A' => {
                    if entry.active {
                        entry.duplicate_add_mentions =
                            entry.duplicate_add_mentions.saturating_add(1);
                        // `A` is an observed live-object add/update record, not
                        // a proxy-owned game-state transition. The EE server
                        // decompile for `CNWSMessage::SendServerToPlayerGameObjUpdate`
                        // shows the server recomputing object update messages
                        // from current visibility/categories, and reliable M
                        // traffic can replay the same verified payload. Treat a
                        // same-id/same-type add as an idempotent assertion that
                        // the object is present; the earlier object-type-change
                        // check remains the real warning boundary.
                        tracing::debug!(
                            object_id = mention.object_id,
                            object_type = mention.object_type,
                            duplicate_add_mentions = entry.duplicate_add_mentions,
                            "live-object registry observed idempotent duplicate add"
                        );
                    }
                    entry.active = true;
                    entry.add_mentions = entry.add_mentions.saturating_add(1);
                }
                b'D' => {
                    if !entry.active {
                        entry.delete_before_add_mentions =
                            entry.delete_before_add_mentions.saturating_add(1);
                        // The registry is wire-derived protocol context, not a
                        // game-state oracle. After area changes or late proxy
                        // startup, the server can legally delete objects that
                        // were active before this cache observed their add. Keep
                        // the fact for diagnostics, but do not surface it as a
                        // packet warning unless a future invariant proves it is
                        // harmful.
                        tracing::debug!(
                            object_id = mention.object_id,
                            object_type = mention.object_type,
                            "live-object registry observed delete before active add"
                        );
                    }
                    entry.active = false;
                    entry.clear_lifecycle_facts();
                    entry.delete_mentions = entry.delete_mentions.saturating_add(1);
                }
                b'U' | b'P' | b'I' | b'G' | b'W' => {
                    if !entry.active {
                        entry.update_before_add_mentions =
                            entry.update_before_add_mentions.saturating_add(1);
                        // Same discipline as deletes above: this is useful
                        // state for future translation decisions, but the
                        // legacy server remains authoritative and can mention
                        // objects before this proxy cache saw their add.
                        tracing::debug!(
                            object_id = mention.object_id,
                            object_type = mention.object_type,
                            opcode = %char::from(mention.opcode),
                            "live-object registry observed update before active add"
                        );
                    }
                    entry.update_mentions = entry.update_mentions.saturating_add(1);
                }
                _ => {}
            }
            if mention.opcode == b'D' && mention.object_type == ITEM_OBJECT_TYPE {
                self.forget_inventory_item_object_id(registry_object_id);
            }
        }
    }

    pub(crate) fn observe_placeable_area_context(
        &mut self,
        area_context: &AreaPlaceableContext,
        mentions: &[LiveObjectMention],
    ) {
        const PLACEABLE_STATE_OBSERVATION: u8 = 0x01;
        const PLACEABLE_ORIENTATION_OBSERVATION: u8 = 0x02;
        const PLACEABLE_APPEARANCE_OBSERVATION: u8 = 0x04;
        const PLACEABLE_POSITION_OBSERVATION: u8 = 0x08;

        let mut seen_observation_masks = BTreeMap::new();
        for mention in mentions {
            let registry_object_id =
                self.registry_object_id_for_live_object(mention.object_type, mention.object_id);
            let observation_mask = (if mention.placeable_state.is_some() {
                PLACEABLE_STATE_OBSERVATION
            } else {
                0
            }) | (if mention.orientation.is_some() {
                PLACEABLE_ORIENTATION_OBSERVATION
            } else {
                0
            }) | (if mention.placeable_appearance.is_some() {
                PLACEABLE_APPEARANCE_OBSERVATION
            } else {
                0
            }) | (if mention.position.is_some() {
                PLACEABLE_POSITION_OBSERVATION
            } else {
                0
            });
            if mention.object_type != 0x09 || observation_mask == 0 {
                continue;
            }
            let seen_mask = seen_observation_masks
                .entry(registry_object_id)
                .or_insert(0_u8);
            let new_observation_mask = observation_mask & !*seen_mask;
            if new_observation_mask == 0 {
                continue;
            }
            *seen_mask |= observation_mask;
            let observes_state = (new_observation_mask & PLACEABLE_STATE_OBSERVATION) != 0;
            let observes_orientation =
                (new_observation_mask & PLACEABLE_ORIENTATION_OBSERVATION) != 0;
            let observes_appearance =
                (new_observation_mask & PLACEABLE_APPEARANCE_OBSERVATION) != 0;
            let observes_position = (new_observation_mask & PLACEABLE_POSITION_OBSERVATION) != 0;

            let Some(known) = self.known.get(&registry_object_id) else {
                continue;
            };
            let placeable_state = known.placeable_state;
            let placeable_appearance = known.placeable_appearance;
            let live_orientation = known.orientation;
            let live_position = known.position;
            let overlap = area_context.placeable_overlap_by(|row_object_id| {
                object_ids::equivalent_legacy_external_object_ids(row_object_id, mention.object_id)
            });
            if overlap.is_empty() {
                continue;
            }

            let identity_conflict = overlap.identity_conflict();
            let conflict = if observes_state {
                let Some(placeable_state) = placeable_state else {
                    continue;
                };
                let observed = AreaPlaceableObservedState {
                    useable: placeable_state.useable,
                    trap_disarmable: placeable_state.trap_disarmable,
                    lockable: placeable_state.lockable,
                    locked: placeable_state.locked,
                };
                overlap.static_module_state_conflict(observed)
            } else {
                AreaPlaceableContextStateConflict::default()
            };
            let orientation_conflict = if observes_orientation {
                overlap.static_module_orientation_conflict(live_orientation)
            } else {
                None
            };
            let appearance_conflict = if observes_appearance {
                overlap.static_module_appearance_conflict(placeable_appearance)
            } else {
                None
            };
            let position_conflict = if observes_position {
                overlap.static_module_position_conflict(live_position)
            } else {
                None
            };
            let conflict_fields = conflict.formatted_fields();
            let area_rows = overlap.formatted_rows();
            let area_light_duplicate = overlap.has_light_row();
            let area_static_duplicate = overlap.has_static_row();
            let known_active = known.active;
            let known_mentions = known.mentions;
            let add_mentions = known.add_mentions;
            let update_mentions = known.update_mentions;
            let last_opcode = known.last_opcode;
            let prior_unresolved_conflict = known.unresolved_area_static_state_conflict;
            let prior_unresolved_conflict_fields = prior_unresolved_conflict
                .map(AreaPlaceableContextStateConflict::formatted_fields)
                .unwrap_or_else(|| "none".to_string());
            let resolved_prior_conflict =
                observes_state && prior_unresolved_conflict.is_some() && !conflict.any();
            let prior_unresolved_identity_conflict = known.unresolved_area_static_identity_conflict;
            let resolved_prior_identity_conflict =
                prior_unresolved_identity_conflict.is_some() && identity_conflict.is_none();
            let prior_unresolved_appearance_conflict =
                known.unresolved_area_static_appearance_conflict;
            let resolved_prior_appearance_conflict = observes_appearance
                && prior_unresolved_appearance_conflict.is_some()
                && appearance_conflict.is_none();
            let prior_unresolved_orientation_conflict =
                known.unresolved_area_static_orientation_conflict;
            let resolved_prior_orientation_conflict = observes_orientation
                && prior_unresolved_orientation_conflict.is_some()
                && orientation_conflict.is_none();
            let prior_unresolved_position_conflict = known.unresolved_area_static_position_conflict;
            let resolved_prior_position_conflict = observes_position
                && prior_unresolved_position_conflict.is_some()
                && position_conflict.is_none();

            if let Some(known) = self.known.get_mut(&registry_object_id) {
                known.area_placeable_context_overlaps =
                    known.area_placeable_context_overlaps.saturating_add(1);
                known.latest_area_static_identity_conflict = identity_conflict;
                if let Some(conflict) = identity_conflict {
                    known.area_static_identity_conflicts =
                        known.area_static_identity_conflicts.saturating_add(1);
                    known.unresolved_area_static_identity_conflict = Some(conflict);
                } else if known
                    .unresolved_area_static_identity_conflict
                    .take()
                    .is_some()
                {
                    known.area_static_identity_conflict_resolutions = known
                        .area_static_identity_conflict_resolutions
                        .saturating_add(1);
                }
                if observes_state {
                    known.latest_area_static_state_conflict = Some(conflict);
                    if conflict.any() {
                        known.area_static_state_conflicts =
                            known.area_static_state_conflicts.saturating_add(1);
                        known.unresolved_area_static_state_conflict = Some(conflict);
                    } else if known.unresolved_area_static_state_conflict.take().is_some() {
                        known.area_static_state_conflict_resolutions = known
                            .area_static_state_conflict_resolutions
                            .saturating_add(1);
                    }
                }
                if observes_appearance {
                    known.latest_area_static_appearance_conflict = appearance_conflict;
                    if let Some(conflict) = appearance_conflict {
                        known.area_static_appearance_conflicts =
                            known.area_static_appearance_conflicts.saturating_add(1);
                        known.unresolved_area_static_appearance_conflict = Some(conflict);
                    } else if known
                        .unresolved_area_static_appearance_conflict
                        .take()
                        .is_some()
                    {
                        known.area_static_appearance_conflict_resolutions = known
                            .area_static_appearance_conflict_resolutions
                            .saturating_add(1);
                    }
                }
                if observes_orientation {
                    known.latest_area_static_orientation_conflict = orientation_conflict;
                    if let Some(conflict) = orientation_conflict {
                        known.area_static_orientation_conflicts =
                            known.area_static_orientation_conflicts.saturating_add(1);
                        known.unresolved_area_static_orientation_conflict = Some(conflict);
                    } else if known
                        .unresolved_area_static_orientation_conflict
                        .take()
                        .is_some()
                    {
                        known.area_static_orientation_conflict_resolutions = known
                            .area_static_orientation_conflict_resolutions
                            .saturating_add(1);
                    }
                }
                if observes_position {
                    known.latest_area_static_position_conflict = position_conflict;
                    if let Some(conflict) = position_conflict {
                        known.area_static_position_conflicts =
                            known.area_static_position_conflicts.saturating_add(1);
                        known.unresolved_area_static_position_conflict = Some(conflict);
                    } else if known
                        .unresolved_area_static_position_conflict
                        .take()
                        .is_some()
                    {
                        known.area_static_position_conflict_resolutions = known
                            .area_static_position_conflict_resolutions
                            .saturating_add(1);
                    }
                }
            }

            if identity_conflict.is_some()
                || conflict.any()
                || appearance_conflict.is_some()
                || orientation_conflict.is_some()
                || position_conflict.is_some()
            {
                tracing::info!(
                    object_id = format_args!("0x{registry_object_id:08X}"),
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    area_resref = area_context.area_resref.as_str(),
                    active = known_active,
                    last_opcode = %char::from(last_opcode),
                    mentions = known_mentions,
                    add_mentions,
                    update_mentions,
                    area_light_duplicate,
                    area_static_duplicate,
                    merged_placeable_appearance = ?placeable_appearance,
                    merged_placeable_state = ?placeable_state,
                    live_orientation = ?live_orientation,
                    live_position = ?live_position,
                    area_module_identity_mismatch = ?identity_conflict,
                    area_module_state_mismatch_fields = %conflict_fields,
                    area_module_appearance_mismatch = ?appearance_conflict,
                    area_module_orientation_mismatch = ?orientation_conflict,
                    area_module_position_mismatch = ?position_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation/position conflicts with module-backed area/static context"
                );
            } else if resolved_prior_identity_conflict
                || resolved_prior_conflict
                || resolved_prior_appearance_conflict
                || resolved_prior_orientation_conflict
                || resolved_prior_position_conflict
            {
                tracing::info!(
                    object_id = format_args!("0x{registry_object_id:08X}"),
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    area_resref = area_context.area_resref.as_str(),
                    active = known_active,
                    last_opcode = %char::from(last_opcode),
                    mentions = known_mentions,
                    add_mentions,
                    update_mentions,
                    area_light_duplicate,
                    area_static_duplicate,
                    merged_placeable_appearance = ?placeable_appearance,
                    merged_placeable_state = ?placeable_state,
                    live_orientation = ?live_orientation,
                    live_position = ?live_position,
                    previous_area_module_identity_mismatch = ?prior_unresolved_identity_conflict,
                    previous_area_module_state_mismatch_fields = %prior_unresolved_conflict_fields,
                    previous_area_module_appearance_mismatch = ?prior_unresolved_appearance_conflict,
                    previous_area_module_orientation_mismatch = ?prior_unresolved_orientation_conflict,
                    previous_area_module_position_mismatch = ?prior_unresolved_position_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation/position resolved prior module-backed area/static conflict"
                );
            } else {
                tracing::debug!(
                    object_id = format_args!("0x{registry_object_id:08X}"),
                    mention_object_id = format_args!("0x{:08X}", mention.object_id),
                    area_resref = area_context.area_resref.as_str(),
                    active = known_active,
                    last_opcode = %char::from(last_opcode),
                    mentions = known_mentions,
                    area_light_duplicate,
                    area_static_duplicate,
                    merged_placeable_appearance = ?placeable_appearance,
                    merged_placeable_state = ?placeable_state,
                    live_orientation = ?live_orientation,
                    live_position = ?live_position,
                    area_module_identity_mismatch = ?identity_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation/position overlaps area/static context"
                );
            }
        }
    }

    fn registry_object_id_for_live_object(&self, object_type: u8, object_id: u32) -> u32 {
        if object_type != PLACEABLE_OBJECT_TYPE {
            return object_id;
        }
        if self.known.contains_key(&object_id) {
            return object_id;
        }

        self.known
            .values()
            .find(|object| {
                object.object_type == PLACEABLE_OBJECT_TYPE
                    && object_ids::equivalent_legacy_external_object_ids(
                        object.object_id,
                        object_id,
                    )
            })
            .map(|object| object.object_id)
            .unwrap_or(object_id)
    }

    pub(crate) fn get(&self, object_type: u8, object_id: u32) -> Option<&KnownObjectState> {
        let object = self.known.get(&object_id)?;
        (object.object_type == object_type).then_some(object)
    }

    pub(crate) fn observe_materialized_item_object_ids(&mut self, object_ids: &[u32]) {
        for object_id in object_ids.iter().copied() {
            if !valid_inventory_item_context_id(object_id) {
                continue;
            }
            self.materialized_item_object_ids.insert(object_id);
            self.cleared_inventory_item_object_ids.remove(&object_id);
        }
    }

    fn forget_inventory_item_object_id(&mut self, object_id: u32) {
        let removed_materialized = self.materialized_item_object_ids.remove(&object_id);
        let removed_first = self.inventory_feature25_first_item_refs.remove(&object_id);
        let removed_second = self.inventory_feature25_second_item_refs.remove(&object_id);
        let removed_legacy_tail = self
            .inventory_feature25_legacy_tail_item_refs
            .remove(&object_id);
        self.remember_inventory_item_object_clear(
            object_id,
            InventoryItemObjectClearReason::ItemDelete,
        );
        if removed_materialized || removed_first || removed_second || removed_legacy_tail {
            tracing::debug!(
                object_id = format_args!("0x{object_id:08X}"),
                removed_materialized,
                removed_feature25_first = removed_first,
                removed_feature25_second = removed_second,
                removed_feature25_legacy_tail = removed_legacy_tail,
                "live-object item delete cleared deferred inventory item proof"
            );
        }
    }

    fn remember_inventory_item_proofs_cleared_by_area_reset(&mut self) {
        self.cleared_inventory_item_object_ids.clear();
        let mut cleared_ids: Vec<u32> = self
            .materialized_item_object_ids
            .iter()
            .chain(self.inventory_feature25_first_item_refs.iter())
            .chain(self.inventory_feature25_second_item_refs.iter())
            .chain(self.inventory_feature25_legacy_tail_item_refs.iter())
            .copied()
            .collect();
        cleared_ids.extend(
            self.known
                .values()
                .filter(|object| object.active && object.object_type == ITEM_OBJECT_TYPE)
                .map(|object| object.object_id),
        );
        cleared_ids.sort_unstable();
        cleared_ids.dedup();
        for object_id in cleared_ids {
            self.remember_inventory_item_object_clear(
                object_id,
                InventoryItemObjectClearReason::AreaReset,
            );
        }
    }

    fn remember_inventory_item_object_clear(
        &mut self,
        object_id: u32,
        reason: InventoryItemObjectClearReason,
    ) {
        if valid_inventory_item_context_id(object_id) {
            self.cleared_inventory_item_object_ids
                .insert(object_id, reason);
        }
    }

    pub(crate) fn observe_inventory_feature25_references(
        &mut self,
        references: &[LiveObjectInventoryFeature25Reference],
    ) {
        for reference in references {
            self.inventory_feature25_reference_records =
                self.inventory_feature25_reference_records.saturating_add(1);
            let first_refs = observe_inventory_feature25_item_refs(
                &mut self.inventory_feature25_first_item_refs,
                &mut self.cleared_inventory_item_object_ids,
                &reference.first_object_ids,
            );
            let second_refs = observe_inventory_feature25_item_refs(
                &mut self.inventory_feature25_second_item_refs,
                &mut self.cleared_inventory_item_object_ids,
                &reference.second_object_ids,
            );
            let legacy_tail_refs = observe_inventory_feature25_item_refs(
                &mut self.inventory_feature25_legacy_tail_item_refs,
                &mut self.cleared_inventory_item_object_ids,
                &reference.legacy_tail_object_ids,
            );
            self.inventory_feature25_first_item_ref_mentions = self
                .inventory_feature25_first_item_ref_mentions
                .saturating_add(first_refs);
            self.inventory_feature25_second_item_ref_mentions = self
                .inventory_feature25_second_item_ref_mentions
                .saturating_add(second_refs);
            self.inventory_feature25_legacy_tail_item_ref_mentions = self
                .inventory_feature25_legacy_tail_item_ref_mentions
                .saturating_add(legacy_tail_refs);
            if second_refs != 0 {
                tracing::debug!(
                    owner_id = format_args!("0x{:08X}", reference.owner_id),
                    mask = format_args!("0x{:04X}", reference.mask),
                    first_refs,
                    second_refs,
                    legacy_tail_refs,
                    "semantic object registry observed deferred inventory Feature-25 item references"
                );
            }
        }
    }

    pub(crate) fn inventory_item_object_proof(
        &self,
        object_id: u32,
    ) -> Option<InventoryItemObjectProof> {
        match self.inventory_item_object_status(object_id) {
            InventoryItemObjectStatus::Proven(proof) => Some(proof),
            InventoryItemObjectStatus::ClearedByItemDelete
            | InventoryItemObjectStatus::ClearedByAreaReset
            | InventoryItemObjectStatus::Unknown => None,
        }
    }

    pub(crate) fn inventory_item_object_status(&self, object_id: u32) -> InventoryItemObjectStatus {
        if self.has_known_inventory_item_object_id(object_id) {
            return InventoryItemObjectStatus::Proven(InventoryItemObjectProof::ActiveObject);
        }
        if self
            .inventory_feature25_first_item_refs
            .contains(&object_id)
        {
            return InventoryItemObjectStatus::Proven(InventoryItemObjectProof::Feature25FirstList);
        }
        if self
            .inventory_feature25_second_item_refs
            .contains(&object_id)
        {
            return InventoryItemObjectStatus::Proven(
                InventoryItemObjectProof::Feature25SecondList,
            );
        }
        if self
            .inventory_feature25_legacy_tail_item_refs
            .contains(&object_id)
        {
            return InventoryItemObjectStatus::Proven(
                InventoryItemObjectProof::Feature25LegacyTail,
            );
        }
        match self.cleared_inventory_item_object_ids.get(&object_id) {
            Some(InventoryItemObjectClearReason::ItemDelete) => {
                InventoryItemObjectStatus::ClearedByItemDelete
            }
            Some(InventoryItemObjectClearReason::AreaReset) => {
                InventoryItemObjectStatus::ClearedByAreaReset
            }
            None => InventoryItemObjectStatus::Unknown,
        }
    }

    pub(crate) fn has_known_inventory_item_object_id(&self, object_id: u32) -> bool {
        if self.materialized_item_object_ids.contains(&object_id) {
            return true;
        }
        self.known
            .get(&object_id)
            .map(|object| object.active && object.object_type == ITEM_OBJECT_TYPE)
            .unwrap_or(false)
    }

    pub(crate) fn has_active_object_id(&self, object_id: u32) -> bool {
        if self.materialized_item_object_ids.contains(&object_id) {
            return true;
        }
        if self
            .known
            .get(&object_id)
            .map(|object| object.active)
            .unwrap_or(false)
        {
            return true;
        }
        // Untyped live-object owner records, especially inventory `I` rows,
        // carry only an OBJECTID. If that owner is a placeable, it must share
        // the same compact/external alias rule as typed U/D/09 lifecycle
        // checks before missing-object cleanup is allowed to drop the row.
        self.placeable_object_for_record_matching(PLACEABLE_OBJECT_TYPE, object_id, |object| {
            object.active
        })
        .is_some()
    }

    pub(crate) fn has_active_typed_object(&self, object_type: u8, object_id: u32) -> bool {
        if self.materialized_item_object_ids.contains(&object_id) {
            return true;
        }
        if object_type == PLACEABLE_OBJECT_TYPE {
            // Live-object placeable records can mention the same runtime
            // object through either the legacy compact id or the external EE
            // id. Observation already merges those aliases; lifecycle checks
            // must use the same owner rule before removing missing-object rows.
            return self
                .placeable_object_for_record_matching(object_type, object_id, |object| {
                    object.active
                })
                .is_some();
        }
        self.get(object_type, object_id)
            .map(|object| object.active)
            .unwrap_or(false)
    }

    pub(crate) fn has_active_live_object_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> bool {
        // Inventory owner records carry an OBJECTID but no independent live
        // object-type marker in the packet body. The exact inventory parser
        // reports object_type 0 for that owner field, so lifecycle proof must
        // use the already-materialized object id without inventing a type.
        let active = if object_type == 0 {
            self.has_active_object_id(object_id)
        } else {
            self.has_active_typed_object(object_type, object_id)
        };
        if (object_id & 0xFFFF_FF00) == 0xFFFF_FF00 {
            tracing::debug!(
                object_type,
                object_id,
                active,
                "semantic object registry session-local lifecycle lookup"
            );
        }
        active
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextStateConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.state)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_orientation_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextOrientationConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.orientation)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_position_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextPositionConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.position)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_appearance_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextAppearanceConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.appearance)
    }

    #[cfg(test)]
    pub(crate) fn unresolved_area_static_placeable_identity_conflict_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaPlaceableContextIdentityConflict> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .and_then(|snapshot| snapshot.identity)
    }

    pub(crate) fn unresolved_area_static_placeable_conflict_snapshot_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<AreaStaticPlaceableConflictSnapshot<'_>> {
        self.placeable_object_for_record_matching(object_type, object_id, |object| {
            unresolved_area_static_placeable_conflict_snapshot(object).is_some()
        })
        .and_then(unresolved_area_static_placeable_conflict_snapshot)
    }

    pub(crate) fn unresolved_area_static_placeable_conflict_summary_for_records<I>(
        &self,
        records: I,
    ) -> AreaStaticPlaceableConflictRecordSummary
    where
        I: IntoIterator<Item = (u8, u32)>,
    {
        let mut summary = AreaStaticPlaceableConflictRecordSummary::default();
        let mut seen_owner_ids = BTreeSet::new();
        for (object_type, object_id) in records {
            let Some(snapshot) = self
                .unresolved_area_static_placeable_conflict_snapshot_for_record(
                    object_type,
                    object_id,
                )
            else {
                continue;
            };
            if seen_owner_ids.insert(snapshot.object.object_id) {
                summary.record(snapshot);
            }
        }
        summary
    }

    pub(crate) fn unresolved_area_static_placeable_conflict_progress_for_records<I>(
        &self,
        records: I,
    ) -> AreaStaticPlaceableConflictRecordProgressSummary
    where
        I: IntoIterator<Item = AreaStaticPlaceableConflictRecordObservation>,
    {
        let mut owner_progress = BTreeMap::new();
        for record in records {
            let Some(snapshot) = self
                .unresolved_area_static_placeable_conflict_snapshot_for_record(
                    record.object_type,
                    record.object_id,
                )
            else {
                continue;
            };
            let progress = snapshot.progress_for_observation(record);
            owner_progress
                .entry(snapshot.object.object_id)
                .and_modify(|existing: &mut AreaStaticPlaceableConflictRecordProgress| {
                    existing.merge(progress);
                })
                .or_insert(progress);
        }

        let mut summary = AreaStaticPlaceableConflictRecordProgressSummary::default();
        for progress in owner_progress.values().copied() {
            summary.record(progress);
        }
        summary
    }

    #[cfg(test)]
    pub(crate) fn active_placeable_with_unresolved_area_static_context_for_record(
        &self,
        object_type: u8,
        object_id: u32,
    ) -> Option<&KnownObjectState> {
        self.unresolved_area_static_placeable_conflict_snapshot_for_record(object_type, object_id)
            .map(|snapshot| snapshot.object)
    }

    fn placeable_object_for_record_matching<F>(
        &self,
        object_type: u8,
        object_id: u32,
        mut predicate: F,
    ) -> Option<&KnownObjectState>
    where
        F: FnMut(&KnownObjectState) -> bool,
    {
        // Inventory owner records carry an OBJECTID without an independent
        // live-object type. Let those untyped owners reuse the same placeable
        // compact/external alias rule while keeping typed non-placeable rows out.
        if object_type != 0 && object_type != PLACEABLE_OBJECT_TYPE {
            return None;
        }

        if let Some(object) = self.known.get(&object_id) {
            if object.object_type == PLACEABLE_OBJECT_TYPE && predicate(object) {
                return Some(object);
            }
        }

        self.known.values().find(|object| {
            object.object_type == PLACEABLE_OBJECT_TYPE
                && object_ids::equivalent_legacy_external_object_ids(object.object_id, object_id)
                && predicate(object)
        })
    }

    pub(crate) fn session_creature_id_for_compact(&self, compact_id: u32) -> Option<u32> {
        self.session_creature_ids_by_compact
            .get(&compact_id)
            .copied()
    }
}

fn unresolved_area_static_placeable_conflict_snapshot(
    object: &KnownObjectState,
) -> Option<AreaStaticPlaceableConflictSnapshot<'_>> {
    if !object.active {
        return None;
    }
    let snapshot = AreaStaticPlaceableConflictSnapshot {
        object,
        identity: object.unresolved_area_static_identity_conflict,
        appearance: object.unresolved_area_static_appearance_conflict,
        state: object.unresolved_area_static_state_conflict,
        orientation: object.unresolved_area_static_orientation_conflict,
        position: object.unresolved_area_static_position_conflict,
    };
    snapshot.any().then_some(snapshot)
}

trait AreaPlaceableContextAppearanceOverlap {
    fn static_module_appearance_conflict(
        &self,
        observed: Option<LiveObjectPlaceableAppearance>,
    ) -> Option<AreaPlaceableContextAppearanceConflict>;
}

impl AreaPlaceableContextAppearanceOverlap for AreaPlaceableContextOverlap<'_> {
    fn static_module_appearance_conflict(
        &self,
        observed: Option<LiveObjectPlaceableAppearance>,
    ) -> Option<AreaPlaceableContextAppearanceConflict> {
        let observed = observed?;
        let module = self.unique_module_backed_static_row()?;
        (observed.appearance != module.appearance).then_some(
            AreaPlaceableContextAppearanceConflict {
                observed_appearance: observed.appearance,
                observed_resref: observed.resref,
                module_appearance: module.appearance,
                module_template_resref: module.module_template_resref,
            },
        )
    }
}

trait AreaPlaceableContextOrientationOverlap {
    fn static_module_orientation_conflict(
        &self,
        observed: Option<LiveObjectOrientation>,
    ) -> Option<AreaPlaceableContextOrientationConflict>;
}

impl AreaPlaceableContextOrientationOverlap for AreaPlaceableContextOverlap<'_> {
    fn static_module_orientation_conflict(
        &self,
        observed: Option<LiveObjectOrientation>,
    ) -> Option<AreaPlaceableContextOrientationConflict> {
        let observed = observed?;
        let module = area_static_row_scalar_orientation(self.unique_module_backed_static_row()?)?;
        (observed.scalar_tenths_degrees != module).then_some(
            AreaPlaceableContextOrientationConflict {
                observed_source: match observed.source {
                    LiveObjectOrientationSource::Scalar => {
                        AreaPlaceableObservedOrientationSource::Scalar
                    }
                    LiveObjectOrientationSource::Vector => {
                        AreaPlaceableObservedOrientationSource::Vector
                    }
                },
                observed_scalar_tenths_degrees: observed.scalar_tenths_degrees,
                module_scalar_tenths_degrees: module,
            },
        )
    }
}

trait AreaPlaceableContextPositionOverlap {
    fn static_module_position_conflict(
        &self,
        observed: Option<LiveObjectPosition>,
    ) -> Option<AreaPlaceableContextPositionConflict>;
}

impl AreaPlaceableContextPositionOverlap for AreaPlaceableContextOverlap<'_> {
    fn static_module_position_conflict(
        &self,
        observed: Option<LiveObjectPosition>,
    ) -> Option<AreaPlaceableContextPositionConflict> {
        let observed = observed?;
        let module = self.unique_module_backed_static_row()?;
        if !observed.x.is_finite()
            || !observed.y.is_finite()
            || !observed.z.is_finite()
            || !module.x.is_finite()
            || !module.y.is_finite()
            || !module.z.is_finite()
        {
            return None;
        }
        let differs = (observed.x - module.x).abs() > PLACEABLE_POSITION_EPSILON
            || (observed.y - module.y).abs() > PLACEABLE_POSITION_EPSILON
            || (observed.z - module.z).abs() > PLACEABLE_POSITION_EPSILON;
        differs.then_some(AreaPlaceableContextPositionConflict {
            observed_x: observed.x,
            observed_y: observed.y,
            observed_z: observed.z,
            module_x: module.x,
            module_y: module.y,
            module_z: module.z,
        })
    }
}

fn compact_session_alias_from_player_list(object_id: u32) -> Option<u32> {
    if object_id == 0 || object_id == 0x7F00_0000 || object_id == u32::MAX {
        return None;
    }
    if (object_id & 0xFFFF_FF00) != 0xFFFF_FF00 {
        return None;
    }
    let compact_id = object_id & 0xFF;
    (compact_id != 0).then_some(compact_id)
}

fn observe_inventory_feature25_item_refs(
    target: &mut BTreeSet<u32>,
    cleared: &mut BTreeMap<u32, InventoryItemObjectClearReason>,
    object_ids: &[u32],
) -> u64 {
    let mut accepted = 0_u64;
    for object_id in object_ids.iter().copied() {
        if !valid_inventory_feature25_item_ref(object_id) {
            continue;
        }
        target.insert(object_id);
        cleared.remove(&object_id);
        accepted = accepted.saturating_add(1);
    }
    accepted
}

fn valid_inventory_feature25_item_ref(object_id: u32) -> bool {
    valid_inventory_item_context_id(object_id)
}

fn valid_inventory_item_context_id(object_id: u32) -> bool {
    object_id != 0 && object_id != 0x7F00_0000 && object_id != u32::MAX
}

#[derive(Debug, Clone, Default)]
pub(crate) struct KnownObjectState {
    pub(crate) object_id: u32,
    pub(crate) object_type: u8,
    pub(crate) last_opcode: u8,
    pub(crate) active: bool,
    pub(crate) latest_name: Option<String>,
    pub(crate) position: Option<LiveObjectPosition>,
    pub(crate) orientation: Option<LiveObjectOrientation>,
    pub(crate) bounds: Option<LiveObjectBounds>,
    pub(crate) placeable_appearance: Option<LiveObjectPlaceableAppearance>,
    pub(crate) placeable_state: Option<LiveObjectPlaceableState>,
    pub(crate) mentions: u64,
    pub(crate) add_mentions: u64,
    pub(crate) update_mentions: u64,
    pub(crate) delete_mentions: u64,
    pub(crate) duplicate_add_mentions: u64,
    pub(crate) update_before_add_mentions: u64,
    pub(crate) delete_before_add_mentions: u64,
    pub(crate) area_placeable_context_overlaps: u64,
    pub(crate) area_static_identity_conflicts: u64,
    pub(crate) latest_area_static_identity_conflict: Option<AreaPlaceableContextIdentityConflict>,
    pub(crate) unresolved_area_static_identity_conflict:
        Option<AreaPlaceableContextIdentityConflict>,
    pub(crate) area_static_identity_conflict_resolutions: u64,
    pub(crate) area_static_appearance_conflicts: u64,
    pub(crate) latest_area_static_appearance_conflict:
        Option<AreaPlaceableContextAppearanceConflict>,
    pub(crate) unresolved_area_static_appearance_conflict:
        Option<AreaPlaceableContextAppearanceConflict>,
    pub(crate) area_static_appearance_conflict_resolutions: u64,
    pub(crate) area_static_state_conflicts: u64,
    pub(crate) latest_area_static_state_conflict: Option<AreaPlaceableContextStateConflict>,
    pub(crate) unresolved_area_static_state_conflict: Option<AreaPlaceableContextStateConflict>,
    pub(crate) area_static_state_conflict_resolutions: u64,
    pub(crate) area_static_orientation_conflicts: u64,
    pub(crate) latest_area_static_orientation_conflict:
        Option<AreaPlaceableContextOrientationConflict>,
    pub(crate) unresolved_area_static_orientation_conflict:
        Option<AreaPlaceableContextOrientationConflict>,
    pub(crate) area_static_orientation_conflict_resolutions: u64,
    pub(crate) area_static_position_conflicts: u64,
    pub(crate) latest_area_static_position_conflict: Option<AreaPlaceableContextPositionConflict>,
    pub(crate) unresolved_area_static_position_conflict:
        Option<AreaPlaceableContextPositionConflict>,
    pub(crate) area_static_position_conflict_resolutions: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AreaStaticPlaceableConflictSnapshot<'a> {
    pub(crate) object: &'a KnownObjectState,
    pub(crate) identity: Option<AreaPlaceableContextIdentityConflict>,
    pub(crate) appearance: Option<AreaPlaceableContextAppearanceConflict>,
    pub(crate) state: Option<AreaPlaceableContextStateConflict>,
    pub(crate) orientation: Option<AreaPlaceableContextOrientationConflict>,
    pub(crate) position: Option<AreaPlaceableContextPositionConflict>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AreaStaticPlaceableConflictRecordObservation {
    pub(crate) object_type: u8,
    pub(crate) object_id: u32,
    pub(crate) placeable_appearance: Option<LiveObjectPlaceableAppearance>,
    pub(crate) placeable_state: Option<LiveObjectPlaceableState>,
    pub(crate) orientation: Option<LiveObjectOrientation>,
    pub(crate) position: Option<LiveObjectPosition>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaStaticPlaceableConflictRecordProgress {
    pub(crate) untouched_identity: bool,
    pub(crate) resolves_appearance: bool,
    pub(crate) repeats_appearance: bool,
    pub(crate) untouched_appearance: bool,
    pub(crate) resolves_state_useable: bool,
    pub(crate) repeats_state_useable: bool,
    pub(crate) untouched_state_useable: bool,
    pub(crate) resolves_state_trap_disarmable: bool,
    pub(crate) repeats_state_trap_disarmable: bool,
    pub(crate) untouched_state_trap_disarmable: bool,
    pub(crate) resolves_state_lockable: bool,
    pub(crate) repeats_state_lockable: bool,
    pub(crate) untouched_state_lockable: bool,
    pub(crate) resolves_state_locked: bool,
    pub(crate) repeats_state_locked: bool,
    pub(crate) untouched_state_locked: bool,
    pub(crate) resolves_orientation: bool,
    pub(crate) repeats_orientation: bool,
    pub(crate) untouched_orientation: bool,
    pub(crate) resolves_position: bool,
    pub(crate) repeats_position: bool,
    pub(crate) untouched_position: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaStaticPlaceableConflictRecordProgressSummary {
    pub(crate) owners: u32,
    pub(crate) resolving_owners: u32,
    pub(crate) repeating_owners: u32,
    pub(crate) untouched_owners: u32,
    pub(crate) resolving_appearance: u32,
    pub(crate) repeating_appearance: u32,
    pub(crate) untouched_appearance: u32,
    pub(crate) resolving_state: u32,
    pub(crate) repeating_state: u32,
    pub(crate) untouched_state: u32,
    pub(crate) resolving_orientation: u32,
    pub(crate) repeating_orientation: u32,
    pub(crate) untouched_orientation: u32,
    pub(crate) resolving_position: u32,
    pub(crate) repeating_position: u32,
    pub(crate) untouched_position: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaStaticPlaceableConflictRecordSummary {
    pub(crate) owners: u32,
    pub(crate) identity: u32,
    pub(crate) appearance: u32,
    pub(crate) appearance_module_custom_target: u32,
    pub(crate) appearance_module_custom_target_with_resref: u32,
    pub(crate) appearance_module_custom_target_missing_resref: u32,
    pub(crate) appearance_module_normal_target: u32,
    pub(crate) appearance_observed_custom_source: u32,
    pub(crate) state: u32,
    pub(crate) orientation: u32,
    pub(crate) position: u32,
    pub(crate) state_useable: u32,
    pub(crate) state_trap_disarmable: u32,
    pub(crate) state_lockable: u32,
    pub(crate) state_locked: u32,
}

impl AreaStaticPlaceableConflictRecordSummary {
    pub(crate) fn any(self) -> bool {
        self.owners != 0
    }

    fn record(&mut self, snapshot: AreaStaticPlaceableConflictSnapshot<'_>) {
        self.owners = self.owners.saturating_add(1);
        if snapshot.identity.is_some() {
            self.identity = self.identity.saturating_add(1);
        }
        if snapshot.appearance.is_some() {
            self.appearance = self.appearance.saturating_add(1);
        }
        if let Some(appearance) = snapshot.appearance {
            if is_custom_placeable_appearance(appearance.module_appearance) {
                self.appearance_module_custom_target =
                    self.appearance_module_custom_target.saturating_add(1);
                if appearance.module_template_resref.is_some() {
                    self.appearance_module_custom_target_with_resref = self
                        .appearance_module_custom_target_with_resref
                        .saturating_add(1);
                } else {
                    self.appearance_module_custom_target_missing_resref = self
                        .appearance_module_custom_target_missing_resref
                        .saturating_add(1);
                }
            } else {
                self.appearance_module_normal_target =
                    self.appearance_module_normal_target.saturating_add(1);
            }
            if is_custom_placeable_appearance(appearance.observed_appearance)
                || appearance.observed_resref.is_some()
            {
                self.appearance_observed_custom_source =
                    self.appearance_observed_custom_source.saturating_add(1);
            }
        }
        if let Some(state) = snapshot.state {
            self.state = self.state.saturating_add(1);
            if state.useable {
                self.state_useable = self.state_useable.saturating_add(1);
            }
            if state.trap_disarmable {
                self.state_trap_disarmable = self.state_trap_disarmable.saturating_add(1);
            }
            if state.lockable {
                self.state_lockable = self.state_lockable.saturating_add(1);
            }
            if state.locked {
                self.state_locked = self.state_locked.saturating_add(1);
            }
        }
        if snapshot.orientation.is_some() {
            self.orientation = self.orientation.saturating_add(1);
        }
        if snapshot.position.is_some() {
            self.position = self.position.saturating_add(1);
        }
    }
}

impl AreaStaticPlaceableConflictRecordProgressSummary {
    fn record(&mut self, progress: AreaStaticPlaceableConflictRecordProgress) {
        self.owners = self.owners.saturating_add(1);
        if progress.any_resolving() {
            self.resolving_owners = self.resolving_owners.saturating_add(1);
        }
        if progress.any_repeating() {
            self.repeating_owners = self.repeating_owners.saturating_add(1);
        }
        if !progress.any_resolving() && !progress.any_repeating() && progress.any_untouched() {
            self.untouched_owners = self.untouched_owners.saturating_add(1);
        }
        if progress.resolves_appearance {
            self.resolving_appearance = self.resolving_appearance.saturating_add(1);
        }
        if progress.repeats_appearance {
            self.repeating_appearance = self.repeating_appearance.saturating_add(1);
        }
        if progress.untouched_appearance {
            self.untouched_appearance = self.untouched_appearance.saturating_add(1);
        }
        if progress.any_resolving_state() {
            self.resolving_state = self.resolving_state.saturating_add(1);
        }
        if progress.any_repeating_state() {
            self.repeating_state = self.repeating_state.saturating_add(1);
        }
        if progress.any_untouched_state() {
            self.untouched_state = self.untouched_state.saturating_add(1);
        }
        if progress.resolves_orientation {
            self.resolving_orientation = self.resolving_orientation.saturating_add(1);
        }
        if progress.repeats_orientation {
            self.repeating_orientation = self.repeating_orientation.saturating_add(1);
        }
        if progress.untouched_orientation {
            self.untouched_orientation = self.untouched_orientation.saturating_add(1);
        }
        if progress.resolves_position {
            self.resolving_position = self.resolving_position.saturating_add(1);
        }
        if progress.repeats_position {
            self.repeating_position = self.repeating_position.saturating_add(1);
        }
        if progress.untouched_position {
            self.untouched_position = self.untouched_position.saturating_add(1);
        }
    }
}

impl AreaStaticPlaceableConflictRecordProgress {
    fn merge(&mut self, other: Self) {
        self.untouched_identity |= other.untouched_identity;
        self.resolves_appearance |= other.resolves_appearance;
        self.repeats_appearance |= other.repeats_appearance;
        self.untouched_appearance |= other.untouched_appearance;
        self.resolves_state_useable |= other.resolves_state_useable;
        self.repeats_state_useable |= other.repeats_state_useable;
        self.untouched_state_useable |= other.untouched_state_useable;
        self.resolves_state_trap_disarmable |= other.resolves_state_trap_disarmable;
        self.repeats_state_trap_disarmable |= other.repeats_state_trap_disarmable;
        self.untouched_state_trap_disarmable |= other.untouched_state_trap_disarmable;
        self.resolves_state_lockable |= other.resolves_state_lockable;
        self.repeats_state_lockable |= other.repeats_state_lockable;
        self.untouched_state_lockable |= other.untouched_state_lockable;
        self.resolves_state_locked |= other.resolves_state_locked;
        self.repeats_state_locked |= other.repeats_state_locked;
        self.untouched_state_locked |= other.untouched_state_locked;
        self.resolves_orientation |= other.resolves_orientation;
        self.repeats_orientation |= other.repeats_orientation;
        self.untouched_orientation |= other.untouched_orientation;
        self.resolves_position |= other.resolves_position;
        self.repeats_position |= other.repeats_position;
        self.untouched_position |= other.untouched_position;
    }

    pub(crate) fn any_resolving(self) -> bool {
        self.resolves_appearance
            || self.any_resolving_state()
            || self.resolves_orientation
            || self.resolves_position
    }

    pub(crate) fn any_repeating(self) -> bool {
        self.repeats_appearance
            || self.any_repeating_state()
            || self.repeats_orientation
            || self.repeats_position
    }

    pub(crate) fn any_untouched(self) -> bool {
        self.untouched_identity
            || self.untouched_appearance
            || self.any_untouched_state()
            || self.untouched_orientation
            || self.untouched_position
    }

    fn any_resolving_state(self) -> bool {
        self.resolves_state_useable
            || self.resolves_state_trap_disarmable
            || self.resolves_state_lockable
            || self.resolves_state_locked
    }

    fn any_repeating_state(self) -> bool {
        self.repeats_state_useable
            || self.repeats_state_trap_disarmable
            || self.repeats_state_lockable
            || self.repeats_state_locked
    }

    fn any_untouched_state(self) -> bool {
        self.untouched_state_useable
            || self.untouched_state_trap_disarmable
            || self.untouched_state_lockable
            || self.untouched_state_locked
    }

    pub(crate) fn formatted_resolving_fields(self) -> String {
        self.format_fields(ConflictProgressFieldKind::Resolving)
    }

    pub(crate) fn formatted_repeating_fields(self) -> String {
        self.format_fields(ConflictProgressFieldKind::Repeating)
    }

    pub(crate) fn formatted_untouched_fields(self) -> String {
        self.format_fields(ConflictProgressFieldKind::Untouched)
    }

    fn format_fields(self, kind: ConflictProgressFieldKind) -> String {
        let mut fields = Vec::new();
        match kind {
            ConflictProgressFieldKind::Resolving => {
                push_if(&mut fields, self.resolves_appearance, "appearance");
                push_if(&mut fields, self.resolves_state_useable, "state.useable");
                push_if(
                    &mut fields,
                    self.resolves_state_trap_disarmable,
                    "state.trap_disarmable",
                );
                push_if(&mut fields, self.resolves_state_lockable, "state.lockable");
                push_if(&mut fields, self.resolves_state_locked, "state.locked");
                push_if(&mut fields, self.resolves_orientation, "orientation");
                push_if(&mut fields, self.resolves_position, "position");
            }
            ConflictProgressFieldKind::Repeating => {
                push_if(&mut fields, self.repeats_appearance, "appearance");
                push_if(&mut fields, self.repeats_state_useable, "state.useable");
                push_if(
                    &mut fields,
                    self.repeats_state_trap_disarmable,
                    "state.trap_disarmable",
                );
                push_if(&mut fields, self.repeats_state_lockable, "state.lockable");
                push_if(&mut fields, self.repeats_state_locked, "state.locked");
                push_if(&mut fields, self.repeats_orientation, "orientation");
                push_if(&mut fields, self.repeats_position, "position");
            }
            ConflictProgressFieldKind::Untouched => {
                push_if(&mut fields, self.untouched_identity, "identity");
                push_if(&mut fields, self.untouched_appearance, "appearance");
                push_if(&mut fields, self.untouched_state_useable, "state.useable");
                push_if(
                    &mut fields,
                    self.untouched_state_trap_disarmable,
                    "state.trap_disarmable",
                );
                push_if(&mut fields, self.untouched_state_lockable, "state.lockable");
                push_if(&mut fields, self.untouched_state_locked, "state.locked");
                push_if(&mut fields, self.untouched_orientation, "orientation");
                push_if(&mut fields, self.untouched_position, "position");
            }
        }

        if fields.is_empty() {
            "none".to_string()
        } else {
            fields.join(",")
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ConflictProgressFieldKind {
    Resolving,
    Repeating,
    Untouched,
}

fn push_if(fields: &mut Vec<&'static str>, condition: bool, field: &'static str) {
    if condition {
        fields.push(field);
    }
}

impl AreaStaticPlaceableConflictSnapshot<'_> {
    pub(crate) fn any(self) -> bool {
        self.identity.is_some()
            || self.appearance.is_some()
            || self.state.is_some()
            || self.orientation.is_some()
            || self.position.is_some()
    }

    pub(crate) fn formatted_classes(self) -> String {
        let mut classes = Vec::new();
        if self.identity.is_some() {
            classes.push("identity");
        }
        if self.appearance.is_some() {
            classes.push("appearance");
        }
        if self.state.is_some() {
            classes.push("state");
        }
        if self.orientation.is_some() {
            classes.push("orientation");
        }
        if self.position.is_some() {
            classes.push("position");
        }
        if classes.is_empty() {
            "none".to_string()
        } else {
            classes.join(",")
        }
    }

    pub(crate) fn formatted_state_fields(self) -> String {
        self.state
            .map(AreaPlaceableContextStateConflict::formatted_fields)
            .unwrap_or_else(|| "none".to_string())
    }

    pub(crate) fn progress_for_observation(
        self,
        observation: AreaStaticPlaceableConflictRecordObservation,
    ) -> AreaStaticPlaceableConflictRecordProgress {
        let mut progress = AreaStaticPlaceableConflictRecordProgress {
            untouched_identity: self.identity.is_some(),
            ..AreaStaticPlaceableConflictRecordProgress::default()
        };

        if let Some(conflict) = self.appearance {
            match observation.placeable_appearance {
                Some(observed) if observed.appearance == conflict.module_appearance => {
                    progress.resolves_appearance = true;
                }
                Some(_) => {
                    progress.repeats_appearance = true;
                }
                None => {
                    progress.untouched_appearance = true;
                }
            }
        }

        if let Some(conflict) = self.state {
            let prior = self.object.placeable_state;
            classify_state_conflict_field(
                conflict.useable,
                prior.and_then(|state| state.useable),
                observation.placeable_state.and_then(|state| state.useable),
                &mut progress.resolves_state_useable,
                &mut progress.repeats_state_useable,
                &mut progress.untouched_state_useable,
            );
            classify_state_conflict_field(
                conflict.trap_disarmable,
                prior.and_then(|state| state.trap_disarmable),
                observation
                    .placeable_state
                    .and_then(|state| state.trap_disarmable),
                &mut progress.resolves_state_trap_disarmable,
                &mut progress.repeats_state_trap_disarmable,
                &mut progress.untouched_state_trap_disarmable,
            );
            classify_state_conflict_field(
                conflict.lockable,
                prior.and_then(|state| state.lockable),
                observation.placeable_state.and_then(|state| state.lockable),
                &mut progress.resolves_state_lockable,
                &mut progress.repeats_state_lockable,
                &mut progress.untouched_state_lockable,
            );
            classify_state_conflict_field(
                conflict.locked,
                prior.and_then(|state| state.locked),
                observation.placeable_state.and_then(|state| state.locked),
                &mut progress.resolves_state_locked,
                &mut progress.repeats_state_locked,
                &mut progress.untouched_state_locked,
            );
        }

        if let Some(conflict) = self.orientation {
            match observation.orientation {
                Some(observed)
                    if observed.scalar_tenths_degrees == conflict.module_scalar_tenths_degrees =>
                {
                    progress.resolves_orientation = true;
                }
                Some(_) => {
                    progress.repeats_orientation = true;
                }
                None => {
                    progress.untouched_orientation = true;
                }
            }
        }

        if let Some(conflict) = self.position {
            match observation.position {
                Some(observed)
                    if position_matches_module(
                        observed,
                        conflict.module_x,
                        conflict.module_y,
                        conflict.module_z,
                    ) =>
                {
                    progress.resolves_position = true;
                }
                Some(_) => {
                    progress.repeats_position = true;
                }
                None => {
                    progress.untouched_position = true;
                }
            }
        }

        progress
    }
}

fn classify_state_conflict_field(
    conflicting: bool,
    prior_observed: Option<bool>,
    current_observed: Option<bool>,
    resolves: &mut bool,
    repeats: &mut bool,
    untouched: &mut bool,
) {
    if !conflicting {
        return;
    }
    match (prior_observed, current_observed) {
        (_, None) => {
            *untouched = true;
        }
        (Some(previous), Some(current)) if current != previous => {
            *resolves = true;
        }
        (Some(_), Some(_)) | (None, Some(_)) => {
            *repeats = true;
        }
    }
}

fn position_matches_module(
    position: LiveObjectPosition,
    module_x: f32,
    module_y: f32,
    module_z: f32,
) -> bool {
    (position.x - module_x).abs() <= PLACEABLE_POSITION_EPSILON
        && (position.y - module_y).abs() <= PLACEABLE_POSITION_EPSILON
        && (position.z - module_z).abs() <= PLACEABLE_POSITION_EPSILON
}

fn is_custom_placeable_appearance(appearance: u16) -> bool {
    appearance >= 0xFFFE
}

impl KnownObjectState {
    fn clear_lifecycle_facts(&mut self) {
        self.latest_name = None;
        self.position = None;
        self.orientation = None;
        self.bounds = None;
        self.placeable_appearance = None;
        self.placeable_state = None;
        self.latest_area_static_appearance_conflict = None;
        self.unresolved_area_static_appearance_conflict = None;
        self.latest_area_static_identity_conflict = None;
        self.unresolved_area_static_identity_conflict = None;
        self.latest_area_static_state_conflict = None;
        self.unresolved_area_static_state_conflict = None;
        self.latest_area_static_orientation_conflict = None;
        self.unresolved_area_static_orientation_conflict = None;
        self.latest_area_static_position_conflict = None;
        self.unresolved_area_static_position_conflict = None;
    }

    fn merge_placeable_state(&mut self, observed: LiveObjectPlaceableState) {
        let state = self.placeable_state.get_or_insert_with(Default::default);
        if observed.useable.is_some() {
            state.useable = observed.useable;
        }
        if observed.trap_disarmable.is_some() {
            state.trap_disarmable = observed.trap_disarmable;
        }
        if observed.lockable.is_some() {
            state.lockable = observed.lockable;
        }
        if observed.locked.is_some() {
            state.locked = observed.locked;
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct UiState {
    pub(crate) quickbar_packets: u64,
    pub(crate) quickbar_placeholders: u64,
    pub(crate) inventory_packets: u64,
    pub(crate) last_quickbar_family: Option<VerifiedFamily>,
    pub(crate) last_committed_quickbar_profile: Option<QuickbarValidatedSlotProfile>,
}

#[derive(Debug, Default)]
pub(crate) struct SyntheticState {
    pub(crate) server_synthetic_packets: u64,
}

#[cfg(test)]
mod tests {
    use crate::translate::area::{
        AreaPlaceableContext, AreaPlaceableContextAppearanceConflict,
        AreaPlaceableContextIdentityConflict, AreaPlaceableContextObjectIdConfidence,
        AreaPlaceableContextOrientationConflict, AreaPlaceableContextPositionConflict,
        AreaPlaceableContextRow, AreaPlaceableContextState, AreaPlaceableContextStateConflict,
        AreaPlaceableObservedOrientationSource,
    };
    use crate::translate::semantic::{
        LiveObjectInventoryFeature25Reference, LiveObjectOrientationSource,
        LiveObjectOrientationVector,
    };

    use super::{
        AreaStaticPlaceableConflictRecordObservation,
        AreaStaticPlaceableConflictRecordProgressSummary, AreaStaticPlaceableConflictRecordSummary,
        ITEM_OBJECT_TYPE, InventoryItemObjectProof, InventoryItemObjectStatus, LiveObjectBounds,
        LiveObjectMention, LiveObjectOrientation, LiveObjectPlaceableAppearance,
        LiveObjectPlaceableState, LiveObjectPosition, ObjectRegistry, PlayerListObjectIds,
    };

    #[test]
    fn duplicate_same_type_add_is_idempotent_protocol_state() {
        let mut registry = ObjectRegistry::default();
        let mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: 0x8000_34D8,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };

        registry.observe_mentions(&[mention.clone()]);
        registry.observe_mentions(&[mention.clone()]);

        let object = registry
            .known
            .get(&mention.object_id)
            .expect("object should stay registered");
        assert!(object.active);
        assert_eq!(object.object_type, mention.object_type);
        assert_eq!(object.add_mentions, 2);
        assert_eq!(object.duplicate_add_mentions, 1);
    }

    #[test]
    fn verified_orientation_is_protocol_state() {
        let mut registry = ObjectRegistry::default();
        let mention = LiveObjectMention {
            opcode: b'U',
            object_type: 0x0A,
            object_id: 0x8000_F6AC,
            name: None,
            position: None,
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 900,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };

        registry.observe_mentions(&[mention.clone()]);

        let object = registry
            .known
            .get(&mention.object_id)
            .expect("object should stay registered");
        assert_eq!(object.orientation, mention.orientation);
    }

    #[test]
    fn verified_placeable_appearance_is_protocol_state() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_34D8;
        let add_appearance = LiveObjectPlaceableAppearance {
            appearance: 0x0011,
            resref: None,
        };
        let update_resref = *b"plc_visual_test\0";
        let update_appearance = LiveObjectPlaceableAppearance {
            appearance: 0xFFFE,
            resref: Some(update_resref),
        };

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(add_appearance),
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_appearance),
            Some(add_appearance)
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(update_appearance),
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_appearance),
            Some(update_appearance)
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_appearance),
            None,
            "delete rows clear stale placeable appearance before id reuse"
        );
    }

    #[test]
    fn delete_clears_lifecycle_fields_before_object_id_reuse() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_34D8;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: Some("first lifecycle".to_string()),
            position: Some(LiveObjectPosition {
                x: 1.25,
                y: 2.5,
                z: 3.75,
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 450,
                vector: None,
            }),
            bounds: Some(LiveObjectBounds {
                min_x: -1.0,
                min_y: -2.0,
                min_z: -3.0,
                max_x: 1.0,
                max_y: 2.0,
                max_z: 3.0,
            }),
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x0011,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        }]);

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let object = registry
            .known
            .get(&object_id)
            .expect("reused object id should stay registered");
        assert!(object.active);
        assert_eq!(object.latest_name, None);
        assert_eq!(object.position, None);
        assert_eq!(object.orientation, None);
        assert_eq!(object.bounds, None);
        assert_eq!(object.placeable_appearance, None);
        assert_eq!(object.placeable_state, None);
    }

    #[test]
    fn area_context_tracks_verified_placeable_appearance_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState::default()),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        let conflicting_add = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x2222,
                resref: None,
            }),
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_add));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&conflicting_add));

        let expected_conflict = AreaPlaceableContextAppearanceConflict {
            observed_appearance: 0x2222,
            observed_resref: None,
            module_appearance: 0x1234,
            module_template_resref: None,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should be registered after verified add");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_appearance_conflicts, 1);
        assert_eq!(
            object.latest_area_static_appearance_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_appearance_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_appearance_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external A/09 appearance conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external appearance owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(
            conflict_object.placeable_appearance,
            conflicting_add.placeable_appearance
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one appearance snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, Some(expected_conflict));
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "appearance");
        assert_eq!(snapshot.formatted_state_fields(), "none");
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_summary_for_records([(
                0x09,
                compact_object_id
            )]),
            AreaStaticPlaceableConflictRecordSummary {
                owners: 1,
                appearance: 1,
                appearance_module_normal_target: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );

        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x1234,
                resref: None,
            }),
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&resolving_update));

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact appearance update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_appearance_conflicts, 1);
        assert_eq!(object.area_static_appearance_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_appearance_conflict, None);
        assert_eq!(object.unresolved_area_static_appearance_conflict, None);
        assert_eq!(
            object.placeable_appearance,
            resolving_update.placeable_appearance
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_appearance_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_appearance_conflict_for_record(
                0x05,
                external_object_id
            ),
            None,
            "static placeable appearance conflicts must not leak to other live-object types"
        );
    }

    #[test]
    fn area_context_conflict_summary_classifies_custom_placeable_appearance_edges() {
        let mut registry = ObjectRegistry::default();
        let normal_target_compact_id = 0x0000_0003;
        let normal_target_external_id = 0x8000_0003;
        let custom_target_compact_id = 0x0000_0004;
        let custom_target_external_id = 0x8000_0004;
        let observed_resref = *b"plc_custom_one\0\0";
        let module_resref = *b"plc_custom_two\0\0";
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![
                AreaPlaceableContextRow {
                    object_id: normal_target_compact_id,
                    appearance: 0x0123,
                    object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                    module_state: Some(AreaPlaceableContextState::default()),
                    ..AreaPlaceableContextRow::default()
                },
                AreaPlaceableContextRow {
                    object_id: custom_target_compact_id,
                    appearance: 0xFFFE,
                    module_template_resref: Some(module_resref),
                    object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                    module_state: Some(AreaPlaceableContextState::default()),
                    ..AreaPlaceableContextRow::default()
                },
            ],
            ..AreaPlaceableContext::default()
        };
        let mentions = [
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: normal_target_external_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: Some(LiveObjectPlaceableAppearance {
                    appearance: 0xFFFE,
                    resref: Some(observed_resref),
                }),
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: custom_target_external_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: Some(LiveObjectPlaceableAppearance {
                    appearance: 0x0123,
                    resref: None,
                }),
                placeable_state: None,
            },
        ];

        registry.observe_mentions(&mentions);
        registry.observe_placeable_area_context(&area_context, &mentions);

        let summary = registry.unresolved_area_static_placeable_conflict_summary_for_records([
            (0x09, normal_target_compact_id),
            (0x09, custom_target_compact_id),
        ]);
        assert_eq!(
            summary,
            AreaStaticPlaceableConflictRecordSummary {
                owners: 2,
                appearance: 2,
                appearance_module_custom_target: 1,
                appearance_module_custom_target_with_resref: 1,
                appearance_module_custom_target_missing_resref: 0,
                appearance_module_normal_target: 1,
                appearance_observed_custom_source: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );
    }

    #[test]
    fn area_context_tracks_verified_placeable_orientation_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                has_direction: true,
                dir_y: 1.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState::default()),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let conflicting_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Vector,
                scalar_tenths_degrees: 900,
                vector: Some(LiveObjectOrientationVector {
                    x: -1.0,
                    y: 0.0,
                    z: 0.0,
                }),
            }),
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_update));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_update),
        );

        let expected_conflict = AreaPlaceableContextOrientationConflict {
            observed_source: AreaPlaceableObservedOrientationSource::Vector,
            observed_scalar_tenths_degrees: 900,
            module_scalar_tenths_degrees: 0,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should stay registered after verified orientation update");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_orientation_conflicts, 1);
        assert_eq!(
            object.latest_area_static_orientation_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_orientation_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.orientation,
            Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Vector,
                scalar_tenths_degrees: 900,
                vector: Some(LiveObjectOrientationVector {
                    x: -1.0,
                    y: 0.0,
                    z: 0.0,
                }),
            }),
            "vector-sourced exact U/09 orientation should remain visible to replay diagnostics"
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_orientation_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external orientation conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external conflict owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(conflict_object.orientation, conflicting_update.orientation);
        assert_eq!(
            conflict_object.unresolved_area_static_orientation_conflict,
            Some(expected_conflict)
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one orientation snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, None);
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, Some(expected_conflict));
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "orientation");
        assert_eq!(snapshot.formatted_state_fields(), "none");

        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 0,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&resolving_update));

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact orientation update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_orientation_conflicts, 1);
        assert_eq!(object.area_static_orientation_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_orientation_conflict, None);
        assert_eq!(object.unresolved_area_static_orientation_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_orientation_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_orientation_conflict_for_record(
                0x05,
                external_object_id
            ),
            None,
            "static placeable orientation conflicts must not leak to other live-object types"
        );
    }

    #[test]
    fn area_context_tracks_verified_placeable_position_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                x: 12.34,
                y: 56.78,
                z: 0.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState::default()),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let conflicting_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 1.0,
                y: 2.0,
                z: -3.0,
            }),
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_update));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_update),
        );

        let expected_conflict = AreaPlaceableContextPositionConflict {
            observed_x: 1.0,
            observed_y: 2.0,
            observed_z: -3.0,
            module_x: 12.34,
            module_y: 56.78,
            module_z: 0.0,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should stay registered after verified position update");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_position_conflicts, 1);
        assert_eq!(
            object.latest_area_static_position_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_position_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_position_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external position conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external conflict owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(conflict_object.position, conflicting_update.position);
        assert_eq!(
            conflict_object.unresolved_area_static_position_conflict,
            Some(expected_conflict)
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one position snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, None);
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, Some(expected_conflict));
        assert_eq!(snapshot.formatted_classes(), "position");
        assert_eq!(snapshot.formatted_state_fields(), "none");

        let summary = registry.unresolved_area_static_placeable_conflict_summary_for_records([
            (0x09, external_object_id),
            (0x09, compact_object_id),
        ]);
        assert_eq!(
            summary,
            AreaStaticPlaceableConflictRecordSummary {
                owners: 1,
                position: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );

        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 12.34,
                y: 56.78,
                z: 0.0,
            }),
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&resolving_update));

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact position update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_position_conflicts, 1);
        assert_eq!(object.area_static_position_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_position_conflict, None);
        assert_eq!(object.unresolved_area_static_position_conflict, None);
        assert_eq!(object.position, resolving_update.position);
        assert_eq!(
            registry.unresolved_area_static_placeable_position_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_position_conflict_for_record(
                0x05,
                external_object_id
            ),
            None,
            "static placeable position conflicts must not leak to other live-object types"
        );
    }

    #[test]
    fn area_context_tracks_placeable_identity_conflicts() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let ambiguous_area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::AreaObjectAlias,
                module_state: None,
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        let add_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&add_mention));
        registry.observe_placeable_area_context(
            &ambiguous_area_context,
            std::slice::from_ref(&add_mention),
        );

        let expected_conflict = AreaPlaceableContextIdentityConflict {
            light_rows: 0,
            static_rows: 1,
            module_backed_static_rows: 0,
            module_unbacked_static_rows: 1,
            unproven_static_rows: 1,
            source_incompatible_static_rows: 0,
            source_read_mismatch_static_rows: 0,
            source_fragment_owned_static_rows: 0,
            source_read_mismatch_and_fragment_owned_static_rows: 0,
            area_alias_rows: 1,
            duplicate_object_id_rows: 0,
        };
        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should be registered after verified add");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_identity_conflicts, 1);
        assert_eq!(
            object.latest_area_static_identity_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            object.unresolved_area_static_identity_conflict,
            Some(expected_conflict)
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_identity_conflict_for_record(
                0x09,
                compact_object_id
            ),
            Some(expected_conflict),
            "future compact U/09 rows should see the external A/09 identity conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external identity owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one identity snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, Some(expected_conflict));
        assert_eq!(snapshot.appearance, None);
        assert_eq!(snapshot.state, None);
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "identity");
        assert_eq!(snapshot.formatted_state_fields(), "none");

        let unique_area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let resolving_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(false),
                ..LiveObjectPlaceableState::default()
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_update));
        registry.observe_placeable_area_context(
            &unique_area_context,
            std::slice::from_ref(&resolving_update),
        );

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact update should merge into the external add entry");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_identity_conflicts, 1);
        assert_eq!(object.area_static_identity_conflict_resolutions, 1);
        assert_eq!(object.latest_area_static_identity_conflict, None);
        assert_eq!(object.unresolved_area_static_identity_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_identity_conflict_for_record(
                0x09,
                external_object_id
            ),
            None
        );
    }

    #[test]
    fn verified_placeable_state_merges_add_and_update_facts() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_34D8;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(true),
                ..LiveObjectPlaceableState::default()
            }),
        }]);

        let object = registry
            .known
            .get(&object_id)
            .expect("placeable should stay registered");
        assert_eq!(
            object.placeable_state,
            Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            })
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&object_id)
                .and_then(|object| object.placeable_state),
            None,
            "delete rows clear stale placeable state before any future id reuse"
        );
    }

    #[test]
    fn area_context_conflicts_use_merged_verified_placeable_state() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let add_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        };

        registry.observe_mentions(std::slice::from_ref(&add_mention));
        registry.observe_placeable_area_context(&area_context, std::slice::from_ref(&add_mention));

        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should be registered after verified add");
        assert_eq!(object.area_placeable_context_overlaps, 1);
        assert_eq!(object.area_static_state_conflicts, 0);
        assert_eq!(
            object.latest_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict::default())
        );
        assert_eq!(object.unresolved_area_static_state_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None
        );

        let update_mention = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(true),
                ..LiveObjectPlaceableState::default()
            }),
        };

        registry.observe_mentions(std::slice::from_ref(&update_mention));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&update_mention));

        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should remain registered after verified update");
        assert_eq!(object.area_placeable_context_overlaps, 2);
        assert_eq!(object.area_static_state_conflicts, 1);
        assert_eq!(
            object.latest_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            })
        );
        assert_eq!(
            object.unresolved_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            })
        );
        assert_eq!(object.area_static_state_conflict_resolutions, 0);
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, compact_object_id),
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            }),
            "future translators may see either compact Diamond ids or canonical EE external ids"
        );
        assert_eq!(
            object.placeable_state,
            Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            })
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("compact diagnostics should expose one state snapshot");
        assert_eq!(snapshot.object.object_id, external_object_id);
        assert_eq!(snapshot.identity, None);
        assert_eq!(snapshot.appearance, None);
        assert_eq!(
            snapshot.state,
            Some(AreaPlaceableContextStateConflict {
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            })
        );
        assert_eq!(snapshot.orientation, None);
        assert_eq!(snapshot.position, None);
        assert_eq!(snapshot.formatted_classes(), "state");
        assert_eq!(snapshot.formatted_state_fields(), "locked");

        let resolving_update_mention = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(false),
                ..LiveObjectPlaceableState::default()
            }),
        };

        registry.observe_mentions(std::slice::from_ref(&resolving_update_mention));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&resolving_update_mention),
        );

        let object = registry
            .known
            .get(&external_object_id)
            .expect("placeable should remain registered after resolving update");
        assert_eq!(object.area_placeable_context_overlaps, 3);
        assert_eq!(object.area_static_state_conflicts, 1);
        assert_eq!(object.area_static_state_conflict_resolutions, 1);
        assert_eq!(
            object.latest_area_static_state_conflict,
            Some(AreaPlaceableContextStateConflict::default())
        );
        assert_eq!(object.unresolved_area_static_state_conflict, None);
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x05, external_object_id),
            None,
            "static placeable conflict state must not leak to other live-object types"
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        assert_eq!(
            registry
                .known
                .get(&external_object_id)
                .and_then(|object| object.latest_area_static_state_conflict),
            None,
            "delete rows clear stale area/static mismatch state before id reuse"
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None,
            "delete rows clear unresolved mismatch state before id reuse"
        );
    }

    #[test]
    fn area_context_conflict_summary_dedupes_alias_owners_and_counts_classes() {
        let mut registry = ObjectRegistry::default();
        let compact_conflict_id = 0x0000_0003;
        let external_conflict_id = 0x8000_0003;
        let compact_identity_id = 0x0000_0004;
        let external_identity_id = 0x8000_0004;

        let conflict_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_conflict_id,
                appearance: 0x1234,
                has_direction: true,
                dir_y: 1.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: false,
                    trap_disarmable: false,
                    lockable: false,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let conflicting_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_conflict_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 900,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x2222,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_mention));
        registry.observe_placeable_area_context(
            &conflict_context,
            std::slice::from_ref(&conflicting_mention),
        );

        let identity_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_identity_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::AreaObjectAlias,
                module_state: None,
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let identity_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_identity_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&identity_mention));
        registry.observe_placeable_area_context(
            &identity_context,
            std::slice::from_ref(&identity_mention),
        );

        let summary = registry.unresolved_area_static_placeable_conflict_summary_for_records([
            (0x09, external_conflict_id),
            (0x09, compact_conflict_id),
            (0x09, external_identity_id),
            (0x05, external_conflict_id),
            (0x09, 0x8000_DEAD),
        ]);

        assert_eq!(
            summary,
            AreaStaticPlaceableConflictRecordSummary {
                owners: 2,
                identity: 1,
                appearance: 1,
                appearance_module_custom_target: 0,
                appearance_module_custom_target_with_resref: 0,
                appearance_module_custom_target_missing_resref: 0,
                appearance_module_normal_target: 1,
                appearance_observed_custom_source: 0,
                state: 1,
                orientation: 1,
                position: 1,
                state_useable: 1,
                state_trap_disarmable: 0,
                state_lockable: 1,
                state_locked: 1,
            }
        );
    }

    #[test]
    fn untyped_placeable_owner_conflict_lookup_uses_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_placeable_id = 0x0000_0003;
        let external_placeable_id = 0x8000_0003;
        let compact_creature_id = 0x0000_0004;
        let external_creature_id = 0x8000_0004;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_placeable_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let conflicting_placeable = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_placeable_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(false),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_placeable));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_placeable),
        );
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: external_creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let expected_conflict = AreaPlaceableContextStateConflict {
            lockable: true,
            locked: true,
            ..AreaPlaceableContextStateConflict::default()
        };
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0, compact_placeable_id),
            Some(expected_conflict),
            "untyped owner rows should see an external placeable conflict through its compact alias"
        );
        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0, compact_placeable_id)
            .expect("untyped compact owner should resolve to the external placeable conflict");
        assert_eq!(snapshot.object.object_id, external_placeable_id);
        assert_eq!(snapshot.state, Some(expected_conflict));
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_summary_for_records([
                (0, compact_placeable_id),
                (0, external_placeable_id),
                (0, compact_creature_id),
                (0x05, compact_placeable_id),
            ]),
            AreaStaticPlaceableConflictRecordSummary {
                owners: 1,
                state: 1,
                state_lockable: 1,
                state_locked: 1,
                ..AreaStaticPlaceableConflictRecordSummary::default()
            }
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0, compact_creature_id),
            None,
            "untyped placeable conflict lookup must not claim compact creature ids"
        );
    }

    #[test]
    fn area_context_conflict_progress_classifies_current_record_fields() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                x: 12.34,
                y: 56.78,
                z: 0.0,
                has_direction: true,
                dir_x: -1.0,
                dir_y: 0.0,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: false,
                    trap_disarmable: false,
                    lockable: false,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };
        let conflicting_mention = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: Some(LiveObjectPosition {
                x: 1.0,
                y: 2.0,
                z: -3.0,
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 1800,
                vector: None,
            }),
            bounds: None,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x2222,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_mention));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&conflicting_mention),
        );

        let snapshot = registry
            .unresolved_area_static_placeable_conflict_snapshot_for_record(0x09, compact_object_id)
            .expect("conflicting external add should be visible through compact alias");

        let resolving_record = AreaStaticPlaceableConflictRecordObservation {
            object_type: 0x09,
            object_id: compact_object_id,
            placeable_appearance: Some(LiveObjectPlaceableAppearance {
                appearance: 0x1234,
                resref: None,
            }),
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(false),
                trap_disarmable: Some(false),
                lockable: Some(false),
                locked: Some(false),
            }),
            orientation: Some(LiveObjectOrientation {
                source: LiveObjectOrientationSource::Scalar,
                scalar_tenths_degrees: 900,
                vector: None,
            }),
            position: Some(LiveObjectPosition {
                x: 12.34,
                y: 56.78,
                z: 0.0,
            }),
        };
        let resolving_progress = snapshot.progress_for_observation(resolving_record);
        assert_eq!(
            resolving_progress.formatted_resolving_fields(),
            "appearance,state.useable,state.lockable,state.locked,orientation,position"
        );
        assert_eq!(resolving_progress.formatted_repeating_fields(), "none");
        assert_eq!(resolving_progress.formatted_untouched_fields(), "none");

        let repeating_record = AreaStaticPlaceableConflictRecordObservation {
            object_type: 0x09,
            object_id: external_object_id,
            placeable_appearance: conflicting_mention.placeable_appearance,
            placeable_state: conflicting_mention.placeable_state,
            orientation: conflicting_mention.orientation,
            position: conflicting_mention.position,
        };
        let repeating_progress = snapshot.progress_for_observation(repeating_record);
        assert_eq!(repeating_progress.formatted_resolving_fields(), "none");
        assert_eq!(
            repeating_progress.formatted_repeating_fields(),
            "appearance,state.useable,state.lockable,state.locked,orientation,position"
        );
        assert_eq!(repeating_progress.formatted_untouched_fields(), "none");

        let untouched_record = AreaStaticPlaceableConflictRecordObservation {
            object_type: 0x09,
            object_id: external_object_id,
            ..AreaStaticPlaceableConflictRecordObservation::default()
        };
        let untouched_progress = snapshot.progress_for_observation(untouched_record);
        assert_eq!(untouched_progress.formatted_resolving_fields(), "none");
        assert_eq!(untouched_progress.formatted_repeating_fields(), "none");
        assert_eq!(
            untouched_progress.formatted_untouched_fields(),
            "appearance,state.useable,state.lockable,state.locked,orientation,position"
        );

        let progress_summary = registry
            .unresolved_area_static_placeable_conflict_progress_for_records([
                resolving_record,
                repeating_record,
                untouched_record,
            ]);
        assert_eq!(
            progress_summary,
            AreaStaticPlaceableConflictRecordProgressSummary {
                owners: 1,
                resolving_owners: 1,
                repeating_owners: 1,
                untouched_owners: 0,
                resolving_appearance: 1,
                repeating_appearance: 1,
                untouched_appearance: 1,
                resolving_state: 1,
                repeating_state: 1,
                untouched_state: 1,
                resolving_orientation: 1,
                repeating_orientation: 1,
                untouched_orientation: 1,
                resolving_position: 1,
                repeating_position: 1,
                untouched_position: 1,
            }
        );
    }

    #[test]
    fn placeable_area_conflicts_resolve_across_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;
        let area_context = AreaPlaceableContext {
            area_resref: "testarea".to_string(),
            static_rows: vec![AreaPlaceableContextRow {
                object_id: compact_object_id,
                appearance: 0x1234,
                object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
                module_state: Some(AreaPlaceableContextState {
                    useable: true,
                    trap_disarmable: false,
                    lockable: true,
                    locked: false,
                    ..AreaPlaceableContextState::default()
                }),
                ..AreaPlaceableContextRow::default()
            }],
            ..AreaPlaceableContext::default()
        };

        let conflicting_add = LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(false),
                locked: Some(true),
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&conflicting_add));
        registry
            .observe_placeable_area_context(&area_context, std::slice::from_ref(&conflicting_add));

        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, compact_object_id),
            Some(AreaPlaceableContextStateConflict {
                lockable: true,
                locked: true,
                ..AreaPlaceableContextStateConflict::default()
            }),
            "future compact U/09 rows should see the external A/09 conflict"
        );
        let conflict_object = registry
            .active_placeable_with_unresolved_area_static_context_for_record(
                0x09,
                compact_object_id,
            )
            .expect("compact diagnostics should resolve to the external conflict owner");
        assert_eq!(conflict_object.object_id, external_object_id);
        assert_eq!(
            conflict_object.placeable_state,
            conflicting_add.placeable_state
        );

        let resolving_compact_update = LiveObjectMention {
            opcode: b'U',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: Some(LiveObjectPlaceableState {
                lockable: Some(true),
                locked: Some(false),
                ..LiveObjectPlaceableState::default()
            }),
        };
        registry.observe_mentions(std::slice::from_ref(&resolving_compact_update));
        registry.observe_placeable_area_context(
            &area_context,
            std::slice::from_ref(&resolving_compact_update),
        );

        assert!(
            !registry.known.contains_key(&compact_object_id),
            "compact/external aliases should not create parallel placeable registry entries"
        );
        let object = registry
            .known
            .get(&external_object_id)
            .expect("compact update should merge into the external add entry");
        assert_eq!(object.area_static_state_conflicts, 1);
        assert_eq!(object.area_static_state_conflict_resolutions, 1);
        assert_eq!(object.unresolved_area_static_state_conflict, None);
        assert_eq!(
            object.placeable_state,
            Some(LiveObjectPlaceableState {
                useable: Some(true),
                trap_disarmable: Some(false),
                lockable: Some(true),
                locked: Some(false),
            })
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, external_object_id),
            None
        );
        assert_eq!(
            registry.unresolved_area_static_placeable_conflict_for_record(0x09, compact_object_id),
            None
        );
    }

    #[test]
    fn active_placeable_lifecycle_lookup_uses_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_object_id = 0x0000_0003;
        let external_object_id = 0x8000_0003;

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(registry.has_active_typed_object(0x09, external_object_id));
        assert!(
            registry.has_active_typed_object(0x09, compact_object_id),
            "compact U/D/09 rows should see the active external A/09 owner"
        );
        assert!(
            registry.has_active_live_object_for_record(0x09, compact_object_id),
            "lifecycle cleanup must share placeable compact/external alias ownership"
        );
        assert!(
            !registry.has_active_typed_object(0x05, compact_object_id),
            "placeable alias ownership must not leak to creature rows"
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: 0x09,
            object_id: compact_object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(
            !registry.has_active_typed_object(0x09, external_object_id),
            "compact delete should clear the external owner entry"
        );
        assert!(!registry.has_active_typed_object(0x09, compact_object_id));
    }

    #[test]
    fn untyped_lifecycle_lookup_uses_placeable_compact_external_aliases() {
        let mut registry = ObjectRegistry::default();
        let compact_placeable_id = 0x0000_0003;
        let external_placeable_id = 0x8000_0003;
        let compact_creature_id = 0x0000_0004;
        let external_creature_id = 0x8000_0004;

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id: external_placeable_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: external_creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(
            registry.has_active_object_id(compact_placeable_id),
            "untyped owner rows should see an active external placeable through its compact alias"
        );
        assert!(
            registry.has_active_live_object_for_record(0, compact_placeable_id),
            "inventory owner lifecycle proof must use the same placeable alias rule"
        );
        assert!(
            !registry.has_active_object_id(compact_creature_id),
            "untyped placeable alias lookup must not claim compact creature ids"
        );
        assert!(registry.has_active_object_id(external_creature_id));
    }

    #[test]
    fn materialized_item_ids_are_protocol_state_without_live_add() {
        let mut registry = ObjectRegistry::default();
        let item_object_id = 0x4000_1234;

        assert!(!registry.has_active_object_id(item_object_id));

        registry.observe_materialized_item_object_ids(&[item_object_id]);

        assert!(registry.has_active_object_id(item_object_id));
        assert!(
            registry.known.get(&item_object_id).is_none(),
            "GUI item materialization must not invent a live-object add/type"
        );

        registry.reset_for_area();

        assert!(!registry.has_active_object_id(item_object_id));
        assert_eq!(
            registry.inventory_item_object_status(item_object_id),
            InventoryItemObjectStatus::ClearedByAreaReset,
            "area reset should explain why the prior item proof is no longer usable"
        );
    }

    #[test]
    fn inventory_item_proof_requires_item_specific_state() {
        let mut registry = ObjectRegistry::default();
        let creature_id = 0x8000_0005;
        let placeable_id = 0x8000_0009;
        let item_id = 0x8000_0006;
        let gui_materialized_item_id = 0x8000_0106;

        registry.observe_mentions(&[
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x05,
                object_id: creature_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x09,
                object_id: placeable_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: ITEM_OBJECT_TYPE,
                object_id: item_id,
                name: None,
                position: None,
                orientation: None,
                bounds: None,
                placeable_appearance: None,
                placeable_state: None,
            },
        ]);
        registry.observe_materialized_item_object_ids(&[gui_materialized_item_id]);

        assert!(registry.has_active_object_id(creature_id));
        assert!(registry.has_active_object_id(placeable_id));
        assert_eq!(
            registry.inventory_item_object_proof(creature_id),
            None,
            "quickbar item proof must not accept active creature lifecycle state"
        );
        assert_eq!(
            registry.inventory_item_object_proof(placeable_id),
            None,
            "quickbar item proof must not accept active placeable lifecycle state"
        );
        assert_eq!(
            registry.inventory_item_object_proof(item_id),
            Some(InventoryItemObjectProof::ActiveObject),
            "typed item live-object state remains valid quickbar item proof"
        );
        assert_eq!(
            registry.inventory_item_object_proof(gui_materialized_item_id),
            Some(InventoryItemObjectProof::ActiveObject),
            "GUI item-create materialization remains valid quickbar item proof"
        );
    }

    #[test]
    fn item_delete_clears_materialized_quickbar_item_proof() {
        let mut registry = ObjectRegistry::default();
        let item_id = 0x8000_0106;

        registry.observe_materialized_item_object_ids(&[item_id]);
        assert_eq!(
            registry.inventory_item_object_proof(item_id),
            Some(InventoryItemObjectProof::ActiveObject)
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: ITEM_OBJECT_TYPE,
            object_id: item_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert_eq!(
            registry.inventory_item_object_proof(item_id),
            None,
            "D/06 must clear GUI-materialized item proof before quickbar can reuse it"
        );
        assert_eq!(
            registry.inventory_item_object_status(item_id),
            InventoryItemObjectStatus::ClearedByItemDelete,
            "diagnostics should retain that the missing proof was cleared by D/06"
        );
        assert!(
            !registry.has_active_object_id(item_id),
            "deleted item id must no longer satisfy untyped active-object checks"
        );
    }

    #[test]
    fn item_delete_clears_only_matching_feature25_quickbar_item_ref() {
        let mut registry = ObjectRegistry::default();
        let first_item_id = 0x8000_0101;
        let second_item_id = 0x8000_0102;
        let legacy_tail_item_id = 0x8000_0103;
        let survivor_item_id = 0x8000_0104;

        registry.observe_inventory_feature25_references(&[LiveObjectInventoryFeature25Reference {
            owner_id: 0x8000_0005,
            mask: 0x2000,
            first_object_ids: vec![first_item_id, survivor_item_id],
            second_object_ids: vec![second_item_id],
            legacy_tail_object_ids: vec![legacy_tail_item_id],
        }]);
        assert_eq!(
            registry.inventory_item_object_proof(first_item_id),
            Some(InventoryItemObjectProof::Feature25FirstList)
        );
        assert_eq!(
            registry.inventory_item_object_proof(second_item_id),
            Some(InventoryItemObjectProof::Feature25SecondList)
        );
        assert_eq!(
            registry.inventory_item_object_proof(legacy_tail_item_id),
            Some(InventoryItemObjectProof::Feature25LegacyTail)
        );

        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'D',
            object_type: ITEM_OBJECT_TYPE,
            object_id: second_item_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert_eq!(
            registry.inventory_item_object_proof(second_item_id),
            None,
            "D/06 must clear stale second-list Feature-25 item proof"
        );
        assert_eq!(
            registry.inventory_item_object_status(second_item_id),
            InventoryItemObjectStatus::ClearedByItemDelete,
            "D/06-cleared Feature-25 proof should remain visible as a diagnostic status"
        );
        assert_eq!(
            registry.inventory_item_object_proof(first_item_id),
            Some(InventoryItemObjectProof::Feature25FirstList),
            "deleting one item must not clear unrelated first-list refs"
        );
        assert_eq!(
            registry.inventory_item_object_proof(legacy_tail_item_id),
            Some(InventoryItemObjectProof::Feature25LegacyTail),
            "deleting one item must not clear unrelated legacy-tail refs"
        );
        assert_eq!(
            registry.inventory_item_object_proof(survivor_item_id),
            Some(InventoryItemObjectProof::Feature25FirstList),
            "other refs in the same Feature-25 claim remain usable evidence"
        );
    }

    #[test]
    fn verified_player_list_creature_id_establishes_session_alias() {
        let mut registry = ObjectRegistry::default();
        let session_creature_id = 0xFFFF_FFFE;

        registry.observe_player_list_object_ids(&[PlayerListObjectIds {
            player_object_id: session_creature_id,
            creature_object_id: Some(session_creature_id),
        }]);

        assert_eq!(
            registry.session_creature_id_for_compact(0xFE),
            Some(session_creature_id)
        );

        registry.reset_for_area();
        assert_eq!(
            registry.session_creature_id_for_compact(0xFE),
            Some(session_creature_id),
            "PlayerList session aliases survive area registry resets"
        );
    }

    #[test]
    fn inventory_owner_lifecycle_uses_active_object_id_without_type() {
        let mut registry = ObjectRegistry::default();
        let creature_id = 0xFFFF_FFFE;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        assert!(registry.has_active_live_object_for_record(0, creature_id));
        assert!(registry.has_active_live_object_for_record(0x05, creature_id));
    }

    #[test]
    fn inventory_owner_mention_does_not_retype_known_creature() {
        let mut registry = ObjectRegistry::default();
        let creature_id = 0xFFFF_FFFE;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x05,
            object_id: creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'I',
            object_type: 0,
            object_id: creature_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let object = registry
            .known
            .get(&creature_id)
            .expect("known creature should remain registered after inventory owner mention");
        assert_eq!(
            object.object_type, 0x05,
            "inventory owner records carry no independent object type and must not retype the creature"
        );
        assert_eq!(object.last_opcode, b'I');
        assert_eq!(object.update_mentions, 1);
    }

    #[test]
    fn later_typed_live_object_promotes_inventory_only_owner_placeholder() {
        let mut registry = ObjectRegistry::default();
        let object_id = 0x8000_1234;
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'I',
            object_type: 0,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);
        registry.observe_mentions(&[LiveObjectMention {
            opcode: b'A',
            object_type: 0x09,
            object_id,
            name: None,
            position: None,
            orientation: None,
            bounds: None,
            placeable_appearance: None,
            placeable_state: None,
        }]);

        let object = registry
            .known
            .get(&object_id)
            .expect("typed add should promote the inventory-only placeholder");
        assert_eq!(object.object_type, 0x09);
        assert!(object.active);
        assert_eq!(object.add_mentions, 1);
    }
}
