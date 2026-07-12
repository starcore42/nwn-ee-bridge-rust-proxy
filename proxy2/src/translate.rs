//! Semantic packet translation layer.
//!
//! The bridge has two separate responsibilities:
//!
//! 1. Convert packets from one protocol dialect into the other dialect.
//! 2. Validate the converted packet and emit only known-good shapes.
//!
//! Keeping those steps separate matters. A packet can have a known top-level
//! tag and still be unsafe to forward if it is still in the wrong version's
//! layout. `BNCS` is the first example: stock EE sends a longer connection
//! packet, while HG/1.69 expects Diamond's shorter two-string form.

pub(crate) mod ambient;
pub(crate) mod area;
pub(crate) mod area_change_day_night;
pub(crate) mod area_visual_effect;
pub(crate) mod baseitems;
mod bm;
mod bn;
pub(crate) mod camera;
pub(crate) mod char_list;
pub(crate) mod chat;
pub(crate) mod client_area;
pub(crate) mod client_char_list;
pub(crate) mod client_character_sheet;
pub(crate) mod client_device;
pub(crate) mod client_gui_event;
pub(crate) mod client_gui_inventory;
mod client_high;
pub(crate) mod client_input;
pub(crate) mod client_login;
pub(crate) mod client_module;
pub(crate) mod client_quickbar;
pub(crate) mod client_server_admin;
pub(crate) mod client_server_status;
pub(crate) mod client_side_message;
mod cnw_message;
pub(crate) mod custom_token;
pub(crate) mod cutscene;
pub(crate) mod diagnostics;
pub(crate) mod dialog;
pub(crate) mod game_obj_update;
pub(crate) mod gameplay_stream;
pub(crate) mod genericdoors;
pub(crate) mod gui_timing_event;
pub(crate) mod inventory;
pub(crate) mod item_update_active_props;
pub(crate) mod journal;
mod live_object;
pub(crate) mod live_object_update;
pub(crate) mod loadbar;
pub(crate) mod login;
mod m_frame;
pub(crate) mod module;
pub(crate) mod module_resources;
pub(crate) mod module_time;
pub(crate) mod party;
pub(crate) mod placeables;
pub(crate) mod play_module_character_list;
pub(crate) mod player_list;
mod profiles;
pub(crate) mod quickbar;
pub(crate) mod resource_config;
pub(crate) mod safe_projectile;
pub(crate) mod semantic;
pub(crate) mod server_status;
pub(crate) mod sound;

use crate::{
    config::{Config, StrictProfile},
    identity::DiamondIdentity,
    nwsync,
    packet::{Direction, Packet},
    strict::{self, Verdict},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationOwner {
    AreaClientArea,
    CharList,
    GameObjUpdateLiveObject,
    GuiQuickbar,
    ModuleInfo,
    PlayModuleCharacterList,
    PlayerList,
    ServerStatusModuleResources,
    UnknownProxyOwned,
}

impl ContinuationOwner {
    pub fn from_verified_family(family: VerifiedFamily) -> Self {
        match family {
            VerifiedFamily::AreaClientArea => Self::AreaClientArea,
            VerifiedFamily::CharList => Self::CharList,
            VerifiedFamily::GameObjUpdateLiveObject => Self::GameObjUpdateLiveObject,
            VerifiedFamily::GuiQuickbar | VerifiedFamily::GuiQuickbarPlaceholder => {
                Self::GuiQuickbar
            }
            VerifiedFamily::ModuleInfo => Self::ModuleInfo,
            VerifiedFamily::PlayModuleCharacterList => Self::PlayModuleCharacterList,
            VerifiedFamily::PlayerList => Self::PlayerList,
            VerifiedFamily::ServerStatusModuleResources => Self::ServerStatusModuleResources,
            _ => Self::UnknownProxyOwned,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AreaClientArea => "Area_ClientArea",
            Self::CharList => "CharList",
            Self::GameObjUpdateLiveObject => "GameObjUpdate_LiveObject",
            Self::GuiQuickbar => "GuiQuickbar",
            Self::ModuleInfo => "Module_Info",
            Self::PlayModuleCharacterList => "PlayModuleCharacterList",
            Self::PlayerList => "PlayerList",
            Self::ServerStatusModuleResources => "ServerStatus_ModuleResources",
            Self::UnknownProxyOwned => "UnknownProxyOwned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedFamily {
    Ambient,
    AreaClientArea,
    AreaChangeDayNight,
    AreaVisualEffect,
    CharList,
    Chat,
    Camera,
    Cutscene,
    ClientArea,
    ClientCharList,
    ClientChat,
    ClientCharacterSheet,
    ClientDialog,
    ClientGuiEvent,
    ClientGuiInventory,
    ClientInput,
    ClientJournal,
    ClientLogin,
    ClientModule,
    ClientParty,
    ClientPlayModuleCharacterList,
    ClientQuickbar,
    ClientServerAdmin,
    ClientServerStatus,
    ClientSideMessage,
    CoalescedWindow,
    ConsumedEmptyMFrame,
    Dialog,
    GameObjUpdateLiveObject,
    GameObjUpdateObjectControl,
    GameObjUpdateVisEffect,
    GameObjUpdateDestroyItem,
    GuiTimingEvent,
    GuiQuickbar,
    GuiQuickbarPlaceholder,
    Inventory,
    ItemUpdateActiveProperties,
    Journal,
    LoadBar,
    Login,
    ModuleEndGame,
    ModuleInfo,
    ModuleTime,
    Party,
    PlayModuleCharacterList,
    PlayerList,
    SemanticDeflated,
    SetCustomToken,
    ServerStatusStatus,
    ServerZlibStreamContinuation {
        owner: ContinuationOwner,
        stream_epoch: u64,
        first_sequence: u16,
    },
    ServerZlibZeroFillWindow {
        first_sequence: u16,
        inflated_length: usize,
        compressed_length: usize,
    },
    ServerStatusModuleResources,
    SafeProjectile,
    Sound,
}

impl VerifiedFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ambient => "Ambient",
            Self::AreaClientArea => "Area_ClientArea",
            Self::AreaChangeDayNight => "Area_ChangeDayNight",
            Self::AreaVisualEffect => "Area_VisualEffect",
            Self::CharList => "CharList",
            Self::Chat => "Chat",
            Self::Camera => "Camera",
            Self::Cutscene => "Cutscene",
            Self::ClientArea => "ClientArea",
            Self::ClientCharList => "ClientCharList",
            Self::ClientChat => "ClientChat",
            Self::ClientCharacterSheet => "ClientCharacterSheet",
            Self::ClientDialog => "ClientDialog",
            Self::ClientGuiEvent => "ClientGuiEvent",
            Self::ClientGuiInventory => "ClientGuiInventory",
            Self::ClientInput => "ClientInput",
            Self::ClientJournal => "ClientJournal",
            Self::ClientLogin => "ClientLogin",
            Self::ClientModule => "ClientModule",
            Self::ClientParty => "ClientParty",
            Self::ClientPlayModuleCharacterList => "ClientPlayModuleCharacterList",
            Self::ClientQuickbar => "ClientQuickbar",
            Self::ClientServerAdmin => "ClientServerAdmin",
            Self::ClientServerStatus => "ClientServerStatus",
            Self::ClientSideMessage => "ClientSideMessage",
            Self::CoalescedWindow => "CoalescedWindow",
            Self::ConsumedEmptyMFrame => "ConsumedEmptyMFrame",
            Self::Dialog => "Dialog",
            Self::GameObjUpdateLiveObject => "GameObjUpdate_LiveObject",
            Self::GameObjUpdateObjectControl => "GameObjUpdate_ObjectControl",
            Self::GameObjUpdateVisEffect => "GameObjUpdate_VisEffect",
            Self::GameObjUpdateDestroyItem => "GameObjUpdate_DestroyItem",
            Self::GuiTimingEvent => "GuiTimingEvent_Info",
            Self::GuiQuickbar => "GuiQuickbar",
            Self::GuiQuickbarPlaceholder => "GuiQuickbarPlaceholder",
            Self::Inventory => "Inventory",
            Self::ItemUpdateActiveProperties => "ItemUpdate_ActiveProperties",
            Self::Journal => "Journal",
            Self::LoadBar => "LoadBar",
            Self::Login => "Login",
            Self::ModuleEndGame => "Module_EndGame",
            Self::ModuleInfo => "Module_Info",
            Self::ModuleTime => "Module_Time",
            Self::Party => "Party",
            Self::PlayModuleCharacterList => "PlayModuleCharacterList",
            Self::PlayerList => "PlayerList",
            Self::SemanticDeflated => "SemanticDeflated",
            Self::SetCustomToken => "SetCustomToken",
            Self::ServerStatusStatus => "ServerStatus_Status",
            Self::ServerZlibStreamContinuation { .. } => "ServerZlibStreamContinuation",
            Self::ServerZlibZeroFillWindow { .. } => "ServerZlibZeroFillWindow",
            Self::ServerStatusModuleResources => "ServerStatus_ModuleResources",
            Self::SafeProjectile => "SafeProjectile",
            Self::Sound => "Sound",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedPacket {
    pub proof: VerifiedProof,
    pub packet: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedProof {
    Family(VerifiedFamily),
    GameplayStream(Vec<VerifiedFamily>),
    CoalescedWindow(Vec<VerifiedProof>),
}

impl VerifiedProof {
    pub fn family(family: VerifiedFamily) -> Self {
        Self::Family(family)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Family(family) => family.as_str(),
            Self::GameplayStream(_) => "GameplayStream",
            Self::CoalescedWindow(_) => "CoalescedWindow",
        }
    }

    pub fn primary_family(&self) -> Option<VerifiedFamily> {
        match self {
            Self::Family(family) => Some(*family),
            Self::GameplayStream(families) => families.first().copied(),
            Self::CoalescedWindow(records) => records.first().and_then(Self::primary_family),
        }
    }

    pub fn contains_family(&self, needle: VerifiedFamily) -> bool {
        match self {
            Self::Family(family) => *family == needle,
            Self::GameplayStream(families) => families.contains(&needle),
            Self::CoalescedWindow(records) => {
                records.iter().any(|record| record.contains_family(needle))
            }
        }
    }

    pub fn from_unit_families(families: Vec<VerifiedFamily>) -> Self {
        match families.as_slice() {
            [family] => Self::Family(*family),
            [] => Self::Family(VerifiedFamily::SemanticDeflated),
            _ => Self::GameplayStream(families),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Emit {
    Packet(Vec<u8>),
    PacketRetireSession {
        packet: Vec<u8>,
        reason: &'static str,
    },
    Packets(Vec<Vec<u8>>),
    PacketsPreShifted(Vec<Vec<u8>>),
    VerifiedPackets {
        family: VerifiedFamily,
        packets: Vec<Vec<u8>>,
    },
    VerifiedPacketsPreShifted {
        family: VerifiedFamily,
        packets: Vec<Vec<u8>>,
    },
    MixedVerifiedPackets(Vec<(VerifiedFamily, Vec<u8>)>),
    VerifiedProofPackets {
        proof: VerifiedProof,
        packets: Vec<Vec<u8>>,
    },
    VerifiedProofPacketsPreShifted {
        proof: VerifiedProof,
        packets: Vec<Vec<u8>>,
    },
    MixedVerifiedProofPackets(Vec<(VerifiedProof, Vec<u8>)>),
    MixedVerifiedProofPacketsPreShifted(Vec<(VerifiedProof, Vec<u8>)>),
    Consumed,
    ConsumedRetireSession {
        reason: &'static str,
    },
    Drop,
}

#[derive(Debug, Clone)]
pub struct Translator {
    strict_translate: bool,
    strict_profile: StrictProfile,
    diamond_identity: DiamondIdentity,
    bncs_private_build: u32,
    bncs_build_field: u16,
    bnxr_nwsync_advertisement: Option<nwsync::Advertisement>,
    server_port: u16,
    discovery_session_name: &'static str,
    discovery_module_name: &'static str,
    module_resources: module_resources::ModuleResourceRuntime,
    synthetic_area_loadbar: bool,
    quickbar_item_refresh_hint: Option<std::path::PathBuf>,
}

#[derive(Debug)]
pub struct SessionTranslator {
    template: Translator,
    bn_state: bn::SessionState,
    m_state: m_frame::SessionState,
    legacy_udp_port: u16,
}

impl Translator {
    pub fn new(config: &Config, nwsync_runtime: Option<nwsync::Runtime>) -> anyhow::Result<Self> {
        let nwsync_advertisement = nwsync_runtime
            .as_ref()
            .map(|runtime| runtime.advertisement().clone());
        let module_resource_runtime = module_resources::ModuleResourceRuntime::new(
            config.asset_profile.clone(),
            nwsync_advertisement
                .clone()
                .filter(|_| config.nwsync_advertise_mode.advertises_module_resources()),
        );
        let profile = profiles::module_resources_profile(&config.asset_profile);
        Ok(Self {
            strict_translate: config.strict_translate,
            strict_profile: config.strict_profile,
            diamond_identity: DiamondIdentity::load(config),
            bncs_private_build: config.bncs_private_build,
            bncs_build_field: config.bncs_build_field,
            bnxr_nwsync_advertisement: nwsync_advertisement
                .filter(|_| config.nwsync_advertise_mode.advertises_bnxr()),
            server_port: config.server.port(),
            discovery_session_name: profile.discovery_session_name,
            discovery_module_name: profile.discovery_module_name,
            module_resources: module_resource_runtime,
            synthetic_area_loadbar: config.synthetic_area_loadbar_enabled(),
            quickbar_item_refresh_hint: config.quickbar_item_refresh_hint.clone(),
        })
    }

    pub fn new_session(&self, legacy_udp_port: u16) -> SessionTranslator {
        SessionTranslator {
            template: self.clone(),
            bn_state: bn::SessionState::default(),
            m_state: m_frame::SessionState::new(
                self.module_resources.for_new_session(),
                self.synthetic_area_loadbar,
                self.quickbar_item_refresh_hint.clone(),
            ),
            legacy_udp_port,
        }
    }
}

impl SessionTranslator {
    pub fn take_pending_client_to_server_packets(&mut self) -> Vec<Vec<u8>> {
        m_frame::take_pending_client_to_server_packets(&mut self.m_state)
    }

    pub fn take_pending_server_to_client_packets(&mut self) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        for packet in self.bn_state.take_pending_server_to_client_packets() {
            packets.extend(packets_from_emit(self.validate_emit(
                Direction::ServerToClientSynthetic,
                Emit::Packet(packet),
            )));
        }
        let emit = m_frame::take_pending_server_to_client_packets(&mut self.m_state);
        packets.extend(packets_from_emit(
            self.validate_emit(Direction::ServerToClientSynthetic, emit),
        ));
        packets
    }

    pub fn translate(&mut self, direction: Direction, bytes: &[u8]) -> Emit {
        // Translation happens before strict validation. This prevents an
        // untranslated-but-recognized packet from slipping through simply
        // because its top-level tag is known.
        let emit = match self.translate_known(direction, bytes) {
            Ok(translated) => translated,
            Err(err) => {
                tracing::warn!(
                    direction = direction.as_str(),
                    error = %err,
                    "strict semantic translation failed"
                );
                return Emit::Drop;
            }
        };

        self.validate_emit(direction, emit)
    }

    fn translate_known(&mut self, direction: Direction, bytes: &[u8]) -> anyhow::Result<Emit> {
        match (direction, Packet::classify(bytes)) {
            (Direction::ClientToServer, Packet::Bn(_)) => {
                let translated = bn::translate_client_to_server(
                    bytes,
                    &self.template.diamond_identity,
                    self.legacy_udp_port,
                    self.template.bncs_private_build,
                    self.template.bncs_build_field,
                    self.template.server_port,
                    self.template.discovery_session_name,
                    self.template.discovery_module_name,
                    self.template.bnxr_nwsync_advertisement.as_ref(),
                    &mut self.bn_state,
                )?;
                match translated {
                    bn::ClientTranslation::Packet(packet) => Ok(Emit::Packet(packet)),
                    bn::ClientTranslation::PacketRetireSession { packet, reason } => {
                        Ok(Emit::PacketRetireSession { packet, reason })
                    }
                    bn::ClientTranslation::Consumed => Ok(Emit::Consumed),
                    bn::ClientTranslation::ConsumedRetireSession { reason } => {
                        Ok(Emit::ConsumedRetireSession { reason })
                    }
                }
            }
            (Direction::ServerToClient, Packet::Bn(_)) => {
                let translated = bn::translate_server_to_client(
                    bytes,
                    &mut self.bn_state,
                    self.template.bnxr_nwsync_advertisement.as_ref(),
                )?;
                match translated {
                    bn::ServerTranslation::Packet(packet) => Ok(Emit::Packet(packet)),
                }
            }
            (Direction::ClientToServer, Packet::M(_)) => {
                self.bn_state.remember_reliable_gameplay_seen();
                m_frame::translate_client_to_server(bytes, &mut self.m_state)
            }
            (Direction::ServerToClient, Packet::M(_)) => {
                self.bn_state.remember_reliable_gameplay_seen();
                m_frame::translate_server_to_client(bytes, &mut self.m_state)
            }
            (Direction::ServerToClientSynthetic, Packet::Bn(_))
            | (Direction::ServerToClientSynthetic, Packet::M(_)) => {
                Ok(Emit::Packet(bytes.to_vec()))
            }
            (Direction::ServerToClient, Packet::UnknownTopLevel(bytes)) => {
                if let Some(claim) = bm::claim_legacy_server_master_control(bytes) {
                    tracing::info!(
                        tag = claim.tag,
                        len = bytes.len(),
                        account_name_len = claim.account_name_len,
                        cd_key_count = claim.cd_key_count.unwrap_or(0),
                        "legacy BM master/auth control consumed before EE client"
                    );
                    return Ok(Emit::Consumed);
                }
                anyhow::bail!(
                    "unclassified top-level packet in {} direction",
                    direction.as_str()
                )
            }
            (_, Packet::UnknownTopLevel(_)) => {
                anyhow::bail!(
                    "unclassified top-level packet in {} direction",
                    direction.as_str()
                )
            }
        }
    }

    fn validate_emit(&self, direction: Direction, emit: Emit) -> Emit {
        match emit {
            Emit::Packet(packet) => self.validate_packet(direction, packet),
            Emit::PacketRetireSession { packet, reason } => {
                match self.validate_packet(direction, packet) {
                    Emit::Packet(packet) => Emit::PacketRetireSession { packet, reason },
                    Emit::Consumed
                    | Emit::ConsumedRetireSession { .. }
                    | Emit::Drop
                    | Emit::Packets(_)
                    | Emit::PacketsPreShifted(_)
                    | Emit::MixedVerifiedPackets(_)
                    | Emit::MixedVerifiedProofPackets(_)
                    | Emit::MixedVerifiedProofPacketsPreShifted(_)
                    | Emit::PacketRetireSession { .. }
                    | Emit::VerifiedPackets { .. }
                    | Emit::VerifiedPacketsPreShifted { .. }
                    | Emit::VerifiedProofPackets { .. }
                    | Emit::VerifiedProofPacketsPreShifted { .. } => Emit::Drop,
                }
            }
            Emit::Packets(packets) | Emit::PacketsPreShifted(packets) => {
                if packets.is_empty() {
                    return Emit::Drop;
                }

                let mut validated = Vec::with_capacity(packets.len());
                for packet in packets {
                    match self.validate_packet(direction, packet) {
                        Emit::Packet(packet) => validated.push(packet),
                        Emit::Consumed
                        | Emit::ConsumedRetireSession { .. }
                        | Emit::Drop
                        | Emit::PacketRetireSession { .. }
                        | Emit::Packets(_)
                        | Emit::PacketsPreShifted(_)
                        | Emit::MixedVerifiedPackets(_)
                        | Emit::MixedVerifiedProofPackets(_)
                        | Emit::MixedVerifiedProofPacketsPreShifted(_)
                        | Emit::VerifiedPackets { .. }
                        | Emit::VerifiedPacketsPreShifted { .. }
                        | Emit::VerifiedProofPackets { .. }
                        | Emit::VerifiedProofPacketsPreShifted { .. } => {
                            return Emit::Drop;
                        }
                    }
                }
                Emit::Packets(validated)
            }
            Emit::MixedVerifiedPackets(packets) => {
                if packets.is_empty() {
                    return Emit::Drop;
                }

                self.validate_mixed_verified_packets(direction, packets)
            }
            Emit::MixedVerifiedProofPackets(packets)
            | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
                if packets.is_empty() {
                    return Emit::Drop;
                }

                self.validate_mixed_verified_proof_packets(direction, packets)
            }
            Emit::VerifiedPackets { family, packets }
            | Emit::VerifiedPacketsPreShifted { family, packets } => {
                if packets.is_empty() {
                    return Emit::Drop;
                }

                if let Some(decision) =
                    strict::decide_verified_translated_batch(direction, family, &packets)
                {
                    strict::log_decision(
                        direction,
                        packets.first().map(Vec::as_slice).unwrap_or_default(),
                        &decision,
                        self.template.strict_translate,
                    );
                    if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
                        return Emit::Drop;
                    }
                    return Emit::Packets(packets);
                }

                let has_batch_prefix = match Packet::classify(
                    packets.first().map(Vec::as_slice).unwrap_or_default(),
                ) {
                    Packet::M(frame) => frame
                        .parsed
                        .as_ref()
                        .map(|view| {
                            let expected = usize::from(view.packetized_sequence);
                            expected > 1 && expected < packets.len()
                        })
                        .unwrap_or(false),
                    _ => false,
                };
                if has_batch_prefix {
                    return self.validate_verified_packet_batch_prefix(direction, family, packets);
                }

                let mut validated = Vec::with_capacity(packets.len());
                for packet in packets {
                    let decision = strict::decide_verified_translated(direction, family, &packet);
                    strict::log_decision(
                        direction,
                        &packet,
                        &decision,
                        self.template.strict_translate,
                    );
                    if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
                        return Emit::Drop;
                    }
                    validated.push(packet);
                }
                Emit::Packets(validated)
            }
            Emit::VerifiedProofPackets { proof, packets }
            | Emit::VerifiedProofPacketsPreShifted { proof, packets } => {
                self.validate_verified_proof_packets(direction, proof, packets)
            }
            Emit::Consumed => Emit::Consumed,
            Emit::ConsumedRetireSession { reason } => Emit::ConsumedRetireSession { reason },
            Emit::Drop => Emit::Drop,
        }
    }

    fn validate_verified_proof_packets(
        &self,
        direction: Direction,
        proof: VerifiedProof,
        packets: Vec<Vec<u8>>,
    ) -> Emit {
        if packets.is_empty() {
            return Emit::Drop;
        }

        if let Some(decision) =
            strict::decide_verified_proof_translated_batch(direction, &proof, &packets)
        {
            strict::log_decision(
                direction,
                packets.first().map(Vec::as_slice).unwrap_or_default(),
                &decision,
                self.template.strict_translate,
            );
            if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
                return Emit::Drop;
            }
            return Emit::Packets(packets);
        }

        let has_batch_prefix =
            match Packet::classify(packets.first().map(Vec::as_slice).unwrap_or_default()) {
                Packet::M(frame) => frame
                    .parsed
                    .as_ref()
                    .map(|view| {
                        let expected = usize::from(view.packetized_sequence);
                        expected > 1 && expected < packets.len()
                    })
                    .unwrap_or(false),
                _ => false,
            };
        if has_batch_prefix {
            return self.validate_verified_proof_packet_batch_prefix(direction, proof, packets);
        }

        let mut validated = Vec::with_capacity(packets.len());
        for packet in packets {
            let decision = strict::decide_verified_proof_translated(direction, &proof, &packet);
            strict::log_decision(
                direction,
                &packet,
                &decision,
                self.template.strict_translate,
            );
            if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
                return Emit::Drop;
            }
            validated.push(packet);
        }
        Emit::Packets(validated)
    }

    fn validate_verified_packet_batch_prefix(
        &self,
        direction: Direction,
        family: VerifiedFamily,
        mut packets: Vec<Vec<u8>>,
    ) -> Emit {
        let Some(first) = packets.first() else {
            return Emit::Drop;
        };
        let expected = match Packet::classify(first.as_slice()) {
            Packet::M(frame) => {
                let Some(view) = frame.parsed.as_ref() else {
                    return Emit::Drop;
                };
                usize::from(view.packetized_sequence)
            }
            _ => return Emit::Drop,
        };
        if expected <= 1 || expected >= packets.len() {
            return Emit::Drop;
        }

        let suffix = packets.split_off(expected);
        let Some(decision) = strict::decide_verified_translated_batch(direction, family, &packets)
        else {
            return Emit::Drop;
        };
        strict::log_decision(
            direction,
            packets.first().map(Vec::as_slice).unwrap_or_default(),
            &decision,
            self.template.strict_translate,
        );
        if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
            return Emit::Drop;
        }

        let mut validated_suffix = Vec::with_capacity(suffix.len());
        for packet in suffix {
            match self.validate_packet(direction, packet) {
                Emit::Packet(packet) => validated_suffix.push(packet),
                Emit::Consumed
                | Emit::ConsumedRetireSession { .. }
                | Emit::Drop
                | Emit::PacketRetireSession { .. }
                | Emit::Packets(_)
                | Emit::PacketsPreShifted(_)
                | Emit::MixedVerifiedPackets(_)
                | Emit::MixedVerifiedProofPackets(_)
                | Emit::MixedVerifiedProofPacketsPreShifted(_)
                | Emit::VerifiedPackets { .. }
                | Emit::VerifiedPacketsPreShifted { .. }
                | Emit::VerifiedProofPackets { .. }
                | Emit::VerifiedProofPacketsPreShifted { .. } => {
                    return Emit::Drop;
                }
            }
        }
        packets.extend(validated_suffix);
        Emit::Packets(packets)
    }

    fn validate_verified_proof_packet_batch_prefix(
        &self,
        direction: Direction,
        proof: VerifiedProof,
        mut packets: Vec<Vec<u8>>,
    ) -> Emit {
        let Some(first) = packets.first() else {
            return Emit::Drop;
        };
        let expected = match Packet::classify(first.as_slice()) {
            Packet::M(frame) => {
                let Some(view) = frame.parsed.as_ref() else {
                    return Emit::Drop;
                };
                usize::from(view.packetized_sequence)
            }
            _ => return Emit::Drop,
        };
        if expected <= 1 || expected >= packets.len() {
            return Emit::Drop;
        }

        let suffix = packets.split_off(expected);
        let Some(decision) =
            strict::decide_verified_proof_translated_batch(direction, &proof, &packets)
        else {
            return Emit::Drop;
        };
        strict::log_decision(
            direction,
            packets.first().map(Vec::as_slice).unwrap_or_default(),
            &decision,
            self.template.strict_translate,
        );
        if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
            return Emit::Drop;
        }

        let mut validated_suffix = Vec::with_capacity(suffix.len());
        for packet in suffix {
            match self.validate_packet(direction, packet) {
                Emit::Packet(packet) => validated_suffix.push(packet),
                Emit::Consumed
                | Emit::ConsumedRetireSession { .. }
                | Emit::Drop
                | Emit::PacketRetireSession { .. }
                | Emit::Packets(_)
                | Emit::PacketsPreShifted(_)
                | Emit::MixedVerifiedPackets(_)
                | Emit::MixedVerifiedProofPackets(_)
                | Emit::MixedVerifiedProofPacketsPreShifted(_)
                | Emit::VerifiedPackets { .. }
                | Emit::VerifiedPacketsPreShifted { .. }
                | Emit::VerifiedProofPackets { .. }
                | Emit::VerifiedProofPacketsPreShifted { .. } => {
                    return Emit::Drop;
                }
            }
        }
        packets.extend(validated_suffix);
        Emit::Packets(packets)
    }

    fn validate_mixed_verified_packets(
        &self,
        direction: Direction,
        packets: Vec<(VerifiedFamily, Vec<u8>)>,
    ) -> Emit {
        let mut validated = Vec::with_capacity(packets.len());
        let mut index = 0;

        while index < packets.len() {
            let (family, packet) = &packets[index];
            let batch_len = match Packet::classify(packet.as_slice()) {
                Packet::M(frame) => frame
                    .parsed
                    .as_ref()
                    .map(|view| usize::from(view.packetized_sequence))
                    .unwrap_or(0),
                _ => 0,
            };

            if batch_len > 1 && index + batch_len <= packets.len() {
                let same_family_batch = packets[index..index + batch_len]
                    .iter()
                    .all(|(candidate_family, _)| candidate_family == family);
                if same_family_batch {
                    let batch_packets = packets[index..index + batch_len]
                        .iter()
                        .map(|(_, packet)| packet.clone())
                        .collect::<Vec<_>>();
                    if let Some(decision) =
                        strict::decide_verified_translated_batch(direction, *family, &batch_packets)
                    {
                        strict::log_decision(
                            direction,
                            batch_packets.first().map(Vec::as_slice).unwrap_or_default(),
                            &decision,
                            self.template.strict_translate,
                        );
                        if self.template.strict_translate && decision.verdict == Verdict::Quarantine
                        {
                            return Emit::Drop;
                        }
                        validated.extend(batch_packets);
                        index += batch_len;
                        continue;
                    }
                }
            }

            let decision = strict::decide_verified_translated(direction, *family, packet);
            strict::log_decision(direction, packet, &decision, self.template.strict_translate);
            if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
                return Emit::Drop;
            }
            validated.push(packet.clone());
            index += 1;
        }

        Emit::Packets(validated)
    }

    fn validate_mixed_verified_proof_packets(
        &self,
        direction: Direction,
        packets: Vec<(VerifiedProof, Vec<u8>)>,
    ) -> Emit {
        let mut validated = Vec::with_capacity(packets.len());
        let mut index = 0;

        while index < packets.len() {
            let (proof, packet) = &packets[index];
            let batch_len = match Packet::classify(packet.as_slice()) {
                Packet::M(frame) => frame
                    .parsed
                    .as_ref()
                    .map(|view| usize::from(view.packetized_sequence))
                    .unwrap_or(0),
                _ => 0,
            };

            if batch_len > 1 && index + batch_len <= packets.len() {
                let same_proof_batch = packets[index..index + batch_len]
                    .iter()
                    .all(|(candidate_proof, _)| candidate_proof == proof);
                if same_proof_batch {
                    let batch_packets = packets[index..index + batch_len]
                        .iter()
                        .map(|(_, packet)| packet.clone())
                        .collect::<Vec<_>>();
                    if let Some(decision) = strict::decide_verified_proof_translated_batch(
                        direction,
                        proof,
                        &batch_packets,
                    ) {
                        strict::log_decision(
                            direction,
                            batch_packets.first().map(Vec::as_slice).unwrap_or_default(),
                            &decision,
                            self.template.strict_translate,
                        );
                        if self.template.strict_translate && decision.verdict == Verdict::Quarantine
                        {
                            return Emit::Drop;
                        }
                        validated.extend(batch_packets);
                        index += batch_len;
                        continue;
                    }
                }
            }

            let decision = strict::decide_verified_proof_translated(direction, proof, packet);
            strict::log_decision(direction, packet, &decision, self.template.strict_translate);
            if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
                return Emit::Drop;
            }
            validated.push(packet.clone());
            index += 1;
        }

        Emit::Packets(validated)
    }

    fn validate_packet(&self, direction: Direction, packet: Vec<u8>) -> Emit {
        let decision = strict::decide(direction, &packet, self.template.strict_profile);
        strict::log_decision(
            direction,
            &packet,
            &decision,
            self.template.strict_translate,
        );
        if self.template.strict_translate && decision.verdict == Verdict::Quarantine {
            Emit::Drop
        } else {
            Emit::Packet(packet)
        }
    }
}

fn packets_from_emit(emit: Emit) -> Vec<Vec<u8>> {
    match emit {
        Emit::Packet(packet) => vec![packet],
        Emit::PacketRetireSession { packet, .. } => vec![packet],
        Emit::Packets(packets)
        | Emit::PacketsPreShifted(packets)
        | Emit::VerifiedPackets { packets, .. }
        | Emit::VerifiedPacketsPreShifted { packets, .. }
        | Emit::VerifiedProofPackets { packets, .. }
        | Emit::VerifiedProofPacketsPreShifted { packets, .. } => packets,
        Emit::MixedVerifiedPackets(packets) => {
            packets.into_iter().map(|(_, packet)| packet).collect()
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            packets.into_iter().map(|(_, packet)| packet).collect()
        }
        Emit::Consumed | Emit::ConsumedRetireSession { .. } | Emit::Drop => Vec::new(),
    }
}
