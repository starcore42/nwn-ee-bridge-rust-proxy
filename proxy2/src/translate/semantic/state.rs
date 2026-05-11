//! Wire-derived semantic session state.
//!
//! This state is a protocol coherence cache, not a game-state authority. It is
//! fed only by verified semantic packet families and should contain only the
//! facts needed to translate future traffic safely: module/resource context,
//! area/load progress, object ids/types observed on the wire, UI packet state,
//! and proxy-owned synthetic event accounting.

use std::collections::{BTreeMap, VecDeque};

use crate::translate::VerifiedFamily;

use super::event::{LiveObjectMention, ProtocolEvent};

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
}

#[derive(Debug, Default)]
pub(crate) struct ObjectRegistry {
    pub(crate) live_object_packets: u64,
    pub(crate) known: BTreeMap<u32, KnownObjectState>,
}

impl ObjectRegistry {
    pub(crate) fn observe_mentions(&mut self, mentions: &[LiveObjectMention]) {
        self.live_object_packets = self.live_object_packets.saturating_add(1);
        for mention in mentions {
            let entry = self
                .known
                .entry(mention.object_id)
                .or_insert_with(|| KnownObjectState {
                    object_type: mention.object_type,
                    ..KnownObjectState::default()
                });
            entry.object_type = mention.object_type;
            entry.last_opcode = mention.opcode;
            entry.mentions = entry.mentions.saturating_add(1);
            match mention.opcode {
                b'A' => entry.add_mentions = entry.add_mentions.saturating_add(1),
                b'U' => entry.update_mentions = entry.update_mentions.saturating_add(1),
                b'D' => entry.delete_mentions = entry.delete_mentions.saturating_add(1),
                _ => {}
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct KnownObjectState {
    pub(crate) object_type: u8,
    pub(crate) last_opcode: u8,
    pub(crate) mentions: u64,
    pub(crate) add_mentions: u64,
    pub(crate) update_mentions: u64,
    pub(crate) delete_mentions: u64,
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
