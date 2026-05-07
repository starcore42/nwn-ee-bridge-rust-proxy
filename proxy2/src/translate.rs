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

mod bn;
mod area;
mod char_list;
mod client_area;
mod client_char_list;
mod client_high;
mod client_login;
mod client_module;
mod client_server_status;
mod chat;
mod client_side_message;
mod cnw_message;
mod custom_token;
mod game_obj_update;
mod inventory;
mod journal;
mod loadbar;
mod m_frame;
mod module;
mod module_time;
mod module_resources;
mod login;
mod live_object;
mod live_object_update;
mod party;
mod player_list;
mod play_module_character_list;
mod profiles;
mod quickbar;

use crate::{
    config::{Config, StrictProfile},
    identity::DiamondIdentity,
    nwsync,
    packet::{Direction, Packet},
    strict::{self, Verdict},
};

#[derive(Debug, Clone)]
pub enum Emit {
    Packet(Vec<u8>),
    Packets(Vec<Vec<u8>>),
    VerifiedPackets(Vec<Vec<u8>>),
    Consumed,
    Drop,
}

#[derive(Debug, Clone)]
pub struct Translator {
    strict_translate: bool,
    strict_profile: StrictProfile,
    diamond_identity: DiamondIdentity,
    bncs_private_build: u32,
    bncs_build_field: u16,
    module_resources: module_resources::ModuleResourceRuntime,
}

#[derive(Debug)]
pub struct SessionTranslator {
    template: Translator,
    bn_state: bn::SessionState,
    m_state: m_frame::SessionState,
}

impl Translator {
    pub fn new(config: &Config, nwsync_runtime: Option<nwsync::Runtime>) -> anyhow::Result<Self> {
        let module_resource_runtime = module_resources::ModuleResourceRuntime::new(
            config.asset_profile.clone(),
            nwsync_runtime
                .as_ref()
                .map(|runtime| runtime.advertisement().clone()),
        );
        Ok(Self {
            strict_translate: config.strict_translate,
            strict_profile: config.strict_profile,
            diamond_identity: DiamondIdentity::load(config),
            bncs_private_build: config.bncs_private_build,
            bncs_build_field: config.bncs_build_field,
            module_resources: module_resource_runtime,
        })
    }

    pub fn new_session(&self) -> SessionTranslator {
        SessionTranslator {
            template: self.clone(),
            bn_state: bn::SessionState::default(),
            m_state: m_frame::SessionState::new(self.module_resources.clone()),
        }
    }
}

impl SessionTranslator {
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
                    self.template.bncs_private_build,
                    self.template.bncs_build_field,
                    &mut self.bn_state,
                )?;
                Ok(Emit::Packet(translated))
            }
            (Direction::ServerToClient, Packet::Bn(_)) => {
                let translated = bn::translate_server_to_client(
                    bytes,
                    &mut self.bn_state,
                    self.template.module_resources.nwsync_advertisement(),
                )?;
                Ok(Emit::Packet(translated))
            }
            (Direction::ClientToServer, Packet::M(_)) => {
                m_frame::translate_client_to_server(bytes, &mut self.m_state)
            }
            (Direction::ServerToClient, Packet::M(_)) => {
                m_frame::translate_server_to_client(bytes, &mut self.m_state)
            }
            (Direction::ServerToClientSynthetic, Packet::Bn(_))
            | (Direction::ServerToClientSynthetic, Packet::M(_)) => Ok(Emit::Packet(bytes.to_vec())),
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
            Emit::Packets(packets) => {
                if packets.is_empty() {
                    return Emit::Drop;
                }

                let mut validated = Vec::with_capacity(packets.len());
                for packet in packets {
                    match self.validate_packet(direction, packet) {
                        Emit::Packet(packet) => validated.push(packet),
                        Emit::Consumed | Emit::Drop | Emit::Packets(_) | Emit::VerifiedPackets(_) => {
                            return Emit::Drop;
                        }
                    }
                }
                Emit::Packets(validated)
            }
            Emit::VerifiedPackets(packets) => {
                if packets.is_empty() {
                    return Emit::Drop;
                }

                let mut validated = Vec::with_capacity(packets.len());
                for packet in packets {
                    let decision = strict::decide_verified_translated(direction, &packet);
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
            Emit::Consumed => Emit::Consumed,
            Emit::Drop => Emit::Drop,
        }
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
