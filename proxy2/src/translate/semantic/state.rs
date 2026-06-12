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
        AreaPlaceableContextOverlap, AreaPlaceableContextStateConflict,
        AreaPlaceableObservedOrientationSource, AreaPlaceableObservedState,
    },
    live_object_update::{area_static_row_scalar_orientation, object_ids},
    player_list::PlayerListObjectIds,
};

use super::event::{
    LiveObjectBounds, LiveObjectMention, LiveObjectOrientation, LiveObjectOrientationSource,
    LiveObjectPlaceableAppearance, LiveObjectPlaceableState, LiveObjectPosition, ProtocolEvent,
};

const MAX_RECENT_EVENTS: usize = 128;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;

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

#[derive(Debug, Default)]
pub(crate) struct ObjectRegistry {
    pub(crate) live_object_packets: u64,
    pub(crate) known: BTreeMap<u32, KnownObjectState>,
    session_creature_ids_by_compact: BTreeMap<u32, u32>,
    materialized_item_object_ids: BTreeSet<u32>,
}

impl ObjectRegistry {
    pub(crate) fn reset_for_area(&mut self) {
        if !self.known.is_empty() || !self.materialized_item_object_ids.is_empty() {
            tracing::debug!(
                known_objects = self.known.len(),
                materialized_item_objects = self.materialized_item_object_ids.len(),
                session_creature_aliases = self.session_creature_ids_by_compact.len(),
                "semantic object registry reset for new Area_ClientArea"
            );
        }
        self.known.clear();
        self.materialized_item_object_ids.clear();
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
                    entry.placeable_appearance = None;
                    entry.placeable_state = None;
                    entry.latest_area_static_appearance_conflict = None;
                    entry.unresolved_area_static_appearance_conflict = None;
                    entry.latest_area_static_identity_conflict = None;
                    entry.unresolved_area_static_identity_conflict = None;
                    entry.latest_area_static_state_conflict = None;
                    entry.unresolved_area_static_state_conflict = None;
                    entry.latest_area_static_orientation_conflict = None;
                    entry.unresolved_area_static_orientation_conflict = None;
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

            let Some(known) = self.known.get(&registry_object_id) else {
                continue;
            };
            let placeable_state = known.placeable_state;
            let placeable_appearance = known.placeable_appearance;
            let live_orientation = known.orientation;
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
            }

            if identity_conflict.is_some()
                || conflict.any()
                || appearance_conflict.is_some()
                || orientation_conflict.is_some()
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
                    area_module_identity_mismatch = ?identity_conflict,
                    area_module_state_mismatch_fields = %conflict_fields,
                    area_module_appearance_mismatch = ?appearance_conflict,
                    area_module_orientation_mismatch = ?orientation_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation conflicts with module-backed area/static context"
                );
            } else if resolved_prior_identity_conflict
                || resolved_prior_conflict
                || resolved_prior_appearance_conflict
                || resolved_prior_orientation_conflict
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
                    previous_area_module_identity_mismatch = ?prior_unresolved_identity_conflict,
                    previous_area_module_state_mismatch_fields = %prior_unresolved_conflict_fields,
                    previous_area_module_appearance_mismatch = ?prior_unresolved_appearance_conflict,
                    previous_area_module_orientation_mismatch = ?prior_unresolved_orientation_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation resolved prior module-backed area/static conflict"
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
                    area_module_identity_mismatch = ?identity_conflict,
                    area_rows = %area_rows,
                    "semantic live-object placeable identity/appearance/state/orientation overlaps area/static context"
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
            if object_id == 0 || object_id == 0x7F00_0000 || object_id == u32::MAX {
                continue;
            }
            self.materialized_item_object_ids.insert(object_id);
        }
    }

    pub(crate) fn has_active_object_id(&self, object_id: u32) -> bool {
        self.materialized_item_object_ids.contains(&object_id)
            || self
                .known
                .get(&object_id)
                .map(|object| object.active)
                .unwrap_or(false)
    }

    pub(crate) fn has_active_typed_object(&self, object_type: u8, object_id: u32) -> bool {
        self.materialized_item_object_ids.contains(&object_id)
            || self
                .get(object_type, object_id)
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
        if object_type != PLACEABLE_OBJECT_TYPE {
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

#[derive(Debug, Default)]
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
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AreaStaticPlaceableConflictSnapshot<'a> {
    pub(crate) object: &'a KnownObjectState,
    pub(crate) identity: Option<AreaPlaceableContextIdentityConflict>,
    pub(crate) appearance: Option<AreaPlaceableContextAppearanceConflict>,
    pub(crate) state: Option<AreaPlaceableContextStateConflict>,
    pub(crate) orientation: Option<AreaPlaceableContextOrientationConflict>,
}

impl AreaStaticPlaceableConflictSnapshot<'_> {
    pub(crate) fn any(self) -> bool {
        self.identity.is_some()
            || self.appearance.is_some()
            || self.state.is_some()
            || self.orientation.is_some()
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
}

impl KnownObjectState {
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
        AreaPlaceableContextOrientationConflict, AreaPlaceableContextRow,
        AreaPlaceableContextState, AreaPlaceableContextStateConflict,
        AreaPlaceableObservedOrientationSource,
    };
    use crate::translate::semantic::{LiveObjectOrientationSource, LiveObjectOrientationVector};

    use super::{
        LiveObjectMention, LiveObjectOrientation, LiveObjectPlaceableAppearance,
        LiveObjectPlaceableState, ObjectRegistry, PlayerListObjectIds,
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
        assert_eq!(snapshot.formatted_classes(), "appearance");
        assert_eq!(snapshot.formatted_state_fields(), "none");

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
            unproven_static_rows: 1,
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
