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

use crate::translate::VerifiedFamily;

use super::event::{
    LiveObjectBounds, LiveObjectMention, LiveObjectOrientation, LiveObjectPosition, ProtocolEvent,
};

const MAX_RECENT_EVENTS: usize = 128;

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
    materialized_item_object_ids: BTreeSet<u32>,
}

impl ObjectRegistry {
    pub(crate) fn reset_for_area(&mut self) {
        if !self.known.is_empty() || !self.materialized_item_object_ids.is_empty() {
            tracing::debug!(
                known_objects = self.known.len(),
                materialized_item_objects = self.materialized_item_object_ids.len(),
                "semantic object registry reset for new Area_ClientArea"
            );
        }
        self.known.clear();
        self.materialized_item_object_ids.clear();
    }

    pub(crate) fn observe_mentions(&mut self, mentions: &[LiveObjectMention]) {
        self.live_object_packets = self.live_object_packets.saturating_add(1);
        for mention in mentions {
            let entry = self
                .known
                .entry(mention.object_id)
                .or_insert_with(|| KnownObjectState {
                    object_id: mention.object_id,
                    object_type: mention.object_type,
                    ..KnownObjectState::default()
                });
            if entry.mentions != 0 && entry.object_type != mention.object_type {
                tracing::warn!(
                    object_id = mention.object_id,
                    old_object_type = entry.object_type,
                    new_object_type = mention.object_type,
                    opcode = %char::from(mention.opcode),
                    "live-object registry observed object type change"
                );
            }
            entry.object_type = mention.object_type;
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

    pub(crate) fn nearby_transition_anchor_for_door(
        &self,
        door_id: u32,
    ) -> Option<NearbyTransitionAnchor<'_>> {
        const DOOR_OBJECT_TYPE: u8 = 0x0A;
        const TRIGGER_OBJECT_TYPE: u8 = 0x07;
        const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
        const MAX_DISTANCE: f32 = 3.5;

        let door = self.get(DOOR_OBJECT_TYPE, door_id)?;
        let door_position = door.click_point()?;
        if !door.active {
            return None;
        }

        self.known
            .values()
            .filter(|entry| {
                let transition_like = match entry.object_type {
                    // Trigger adds carry decompile-owned geometry but not always a
                    // useful display name. Treat a verified nearby active trigger
                    // as a transition anchor without requiring a name heuristic.
                    TRIGGER_OBJECT_TYPE => true,
                    PLACEABLE_OBJECT_TYPE => entry
                        .latest_name
                        .as_deref()
                        .is_some_and(name_looks_transition_related),
                    _ => false,
                };
                transition_like && entry.active && entry.click_point().is_some()
            })
            .filter_map(|entry| {
                let position = entry.click_point()?;
                let dx = position.x - door_position.x;
                let dy = position.y - door_position.y;
                let dz = position.z - door_position.z;
                let distance = (dx * dx + dy * dy + dz * dz).sqrt();
                (distance <= MAX_DISTANCE).then_some(NearbyTransitionAnchor {
                    object_id: entry.object_id,
                    object_type: entry.object_type,
                    name: entry.latest_name.as_deref().unwrap_or(""),
                    distance,
                })
            })
            .min_by(|left, right| left.distance.total_cmp(&right.distance))
    }
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
    pub(crate) mentions: u64,
    pub(crate) add_mentions: u64,
    pub(crate) update_mentions: u64,
    pub(crate) delete_mentions: u64,
    pub(crate) duplicate_add_mentions: u64,
    pub(crate) update_before_add_mentions: u64,
    pub(crate) delete_before_add_mentions: u64,
}

impl KnownObjectState {
    fn click_point(&self) -> Option<LiveObjectPosition> {
        if let Some(position) = self.position {
            return Some(position);
        }
        let bounds = self.bounds?;
        if !bounds.min_x.is_finite()
            || !bounds.min_y.is_finite()
            || !bounds.min_z.is_finite()
            || !bounds.max_x.is_finite()
            || !bounds.max_y.is_finite()
            || !bounds.max_z.is_finite()
            || bounds.min_x > bounds.max_x
            || bounds.min_y > bounds.max_y
            || bounds.min_z > bounds.max_z
        {
            return None;
        }
        Some(LiveObjectPosition {
            x: (bounds.min_x + bounds.max_x) * 0.5,
            y: (bounds.min_y + bounds.max_y) * 0.5,
            z: (bounds.min_z + bounds.max_z) * 0.5,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NearbyTransitionAnchor<'a> {
    pub(crate) object_id: u32,
    pub(crate) object_type: u8,
    pub(crate) name: &'a str,
    pub(crate) distance: f32,
}

fn name_looks_transition_related(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "door",
        "portal",
        "transition",
        "inn",
        "tavern",
        "crow",
        "moon",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
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
    use super::{
        LiveObjectBounds, LiveObjectMention, LiveObjectOrientation, LiveObjectPosition,
        ObjectRegistry,
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
                scalar_tenths_degrees: 900,
            }),
            bounds: None,
        };

        registry.observe_mentions(&[mention.clone()]);

        let object = registry
            .known
            .get(&mention.object_id)
            .expect("object should stay registered");
        assert_eq!(object.orientation, mention.orientation);
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
    fn nearby_transition_anchor_accepts_verified_trigger_bounds_without_name() {
        let mut registry = ObjectRegistry::default();
        registry.observe_mentions(&[
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x0A,
                object_id: 0x8000_F6AC,
                name: Some("Door".to_string()),
                position: None,
                orientation: None,
                bounds: None,
            },
            LiveObjectMention {
                opcode: b'U',
                object_type: 0x0A,
                object_id: 0x8000_F6AC,
                name: None,
                position: Some(LiveObjectPosition {
                    x: 15.0,
                    y: 3.33,
                    z: 0.0,
                }),
                orientation: None,
                bounds: None,
            },
            LiveObjectMention {
                opcode: b'A',
                object_type: 0x07,
                object_id: 0x8000_F700,
                name: None,
                position: None,
                orientation: None,
                bounds: Some(LiveObjectBounds {
                    min_x: 14.5,
                    min_y: 3.0,
                    min_z: 0.0,
                    max_x: 15.5,
                    max_y: 4.0,
                    max_z: 0.0,
                }),
            },
        ]);

        let anchor = registry
            .nearby_transition_anchor_for_door(0x8000_F6AC)
            .expect("verified trigger bounds should provide a transition anchor");
        assert_eq!(anchor.object_id, 0x8000_F700);
        assert_eq!(anchor.object_type, 0x07);
    }
}
