//! Immutable client reliable-data slot ownership and deterministic replay.
//!
//! The original reliable window stores one type-0 datagram per sequence slot
//! before CNW gameplay dispatch. A retransmit may refresh the CRC, ACK, and
//! FrameSend-owned bit 6, but it cannot replace the stored packetized shape or
//! gameplay bytes. Keep that transport identity separate from semantic state:
//! a strict reader rejection leaves the source slot pinned, while an exact
//! retry may translate again from the rolled-back semantic boundary. Once a
//! translation passes the outer strict owner, later retransmits replay that
//! first disposition without running engine-facing effects again.

use std::collections::VecDeque;

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{MFrameType, MFrameView},
    translate::VerifiedFamily,
};

use super::transport_identity::SEND_WINDOW_BIT6_MASK;

pub(super) const MAX_CLIENT_RELIABLE_SLOTS: usize = 64;
const FRAME_SEND_OWNED_FLAG_MASK: u8 = 0x70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClientReliableSlotKey {
    pub(super) lane: MFrameType,
    pub(super) sequence: u16,
    pub(super) origin_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientReliableTransportIdentity {
    /// Keep length explicit even though the exact suffix also makes differing
    /// lengths unequal. This mirrors the receive-window allocation boundary.
    pub(super) datagram_len: usize,
    /// Diamond `sub_5F36E0` lines 751251-751266 and EE `FrameSend` lines
    /// 879868-879880 overwrite only bit 6 and the frame-kind bits at send
    /// time. The lane key carries kind 0 separately; bit 6 is the only one of
    /// those writer-owned bits that may refresh within that data-lane key.
    pub(super) immutable_flags: u8,
    /// Packetized sequence/length, gameplay payload, and any trailing storage
    /// are exact. CRC, source sequence, and ACK occupy bytes before offset 8.
    pub(super) bytes_from_offset_8: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClientReliableTranslationReplay {
    pub(super) family: VerifiedFamily,
    pub(super) packet: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(super) struct ClientReliableSlot {
    pub(super) key: ClientReliableSlotKey,
    pub(super) transport_identity: ClientReliableTransportIdentity,
    /// `None` means the transport slot is pinned but its semantic disposition
    /// is retryable (for example after an outer strict rejection or the
    /// Module_Loaded resource gate deliberately consumes an early attempt).
    pub(super) replay: Option<ClientReliableTranslationReplay>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ClientReliableReplayState {
    pub(super) slots: VecDeque<ClientReliableSlot>,
    pub(super) latest_origin_sequence: Option<u16>,
    pub(super) origin_generation: u64,
    pub(super) exact_replays: u64,
}

#[derive(Debug, Clone)]
pub(super) enum PreparedClientReliableSource {
    Excluded,
    Pending(ClientReliableSlotKey),
    Conflict(ClientReliableSlotKey),
    Replay {
        key: ClientReliableSlotKey,
        replay: ClientReliableTranslationReplay,
    },
}

impl PreparedClientReliableSource {
    pub(super) fn key(&self) -> Option<ClientReliableSlotKey> {
        match self {
            Self::Excluded => None,
            Self::Pending(key) | Self::Conflict(key) | Self::Replay { key, .. } => Some(*key),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OriginGenerationObservation {
    generation: u64,
    latest_origin_sequence: u16,
    origin_generation: u64,
}

/// Pin or match the immutable source identity before any semantic mutation.
///
/// Diamond `sub_5F3940` lines 751460-751763 and EE
/// `CNetLayerWindow::FrameReceive` lines 878825-879146 route only kind 0 into
/// reliable-data storage and advance its cursor modulo `u16`. Controls are
/// deliberately excluded from this ledger even when their sequence field is
/// nonzero.
pub(super) fn prepare_source_slot(
    state: &mut ClientReliableReplayState,
    packet: &[u8],
    view: &MFrameView,
) -> anyhow::Result<PreparedClientReliableSource> {
    if view.frame_kind() != Some(MFrameType::ReliableData) {
        return Ok(PreparedClientReliableSource::Excluded);
    }

    let transport_identity = transport_identity(packet, view)?;
    let generation = preview_origin_generation(state, view.sequence);
    let key = ClientReliableSlotKey {
        lane: MFrameType::ReliableData,
        sequence: view.sequence,
        origin_generation: generation.generation,
    };
    let existing = state.slots.iter().find(|slot| slot.key == key).cloned();
    if let Some(existing) = existing {
        if existing.transport_identity != transport_identity {
            return Ok(PreparedClientReliableSource::Conflict(key));
        }
        apply_origin_generation(state, generation);
        return Ok(match existing.replay {
            Some(replay) => PreparedClientReliableSource::Replay { key, replay },
            None => PreparedClientReliableSource::Pending(key),
        });
    }

    apply_origin_generation(state, generation);
    state.slots.push_back(ClientReliableSlot {
        key,
        transport_identity,
        replay: None,
    });
    while state.slots.len() > MAX_CLIENT_RELIABLE_SLOTS {
        state.slots.pop_front();
    }
    tracing::trace!(
        sequence = key.sequence,
        origin_generation = key.origin_generation,
        retained_slots = state.slots.len(),
        "client reliable-data slot pinned to its first immutable transport identity"
    );
    Ok(PreparedClientReliableSource::Pending(key))
}

pub(super) fn stage_translation(
    state: &mut ClientReliableReplayState,
    key: ClientReliableSlotKey,
    family: VerifiedFamily,
    packet: Option<Vec<u8>>,
) -> anyhow::Result<()> {
    let slot = state
        .slots
        .iter_mut()
        .find(|slot| slot.key == key)
        .ok_or_else(|| {
            anyhow::anyhow!("client reliable slot was evicted before translation commit")
        })?;
    let replay = ClientReliableTranslationReplay { family, packet };
    if let Some(existing) = slot.replay.as_ref() {
        if existing == &replay {
            return Ok(());
        }
        anyhow::bail!("client reliable slot already committed a different translated disposition");
    }
    slot.replay = Some(replay);
    Ok(())
}

/// Rebuild an accepted translation with only decompile-proven send-time fields
/// refreshed. The translated sequence, immutable flags, packetized metadata,
/// payload, trailing storage, family, and consume/forward disposition remain
/// exactly the first strict-accepted result.
pub(super) fn replay_translation(
    state: &mut ClientReliableReplayState,
    key: ClientReliableSlotKey,
    replay: ClientReliableTranslationReplay,
    current_server_facing_source: &[u8],
) -> anyhow::Result<ClientReliableTranslationReplay> {
    let source_view = MFrameView::parse(current_server_facing_source)
        .ok_or_else(|| anyhow::anyhow!("client reliable replay source failed to parse"))?;
    if source_view.frame_kind() != Some(MFrameType::ReliableData) {
        anyhow::bail!("client reliable replay source left the type-0 data lane");
    }

    let mut replay = replay;
    if let Some(packet) = replay.packet.as_mut() {
        let cached_view = MFrameView::parse(packet)
            .ok_or_else(|| anyhow::anyhow!("cached client reliable translation failed to parse"))?;
        if cached_view.frame_kind() != Some(MFrameType::ReliableData) {
            anyhow::bail!("cached client reliable translation left the type-0 data lane");
        }
        packet[7] =
            (packet[7] & !SEND_WINDOW_BIT6_MASK) | (source_view.flags & SEND_WINDOW_BIT6_MASK);
        write_be_u16(packet, 5, source_view.ack_sequence)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to refresh cached client replay ACK"))?;
        encode_legacy_m_crc(packet)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair cached client replay CRC"))?;
    }

    state.exact_replays = state.exact_replays.saturating_add(1);
    tracing::info!(
        sequence = key.sequence,
        origin_generation = key.origin_generation,
        ack_sequence = source_view.ack_sequence,
        send_window_bit6 = (source_view.flags & SEND_WINDOW_BIT6_MASK) != 0,
        family = replay.family.as_str(),
        emitted = replay.packet.is_some(),
        exact_replays = state.exact_replays,
        "client reliable M retransmission replayed from first accepted translation without engine-facing effects"
    );
    Ok(replay)
}

fn transport_identity(
    packet: &[u8],
    view: &MFrameView,
) -> anyhow::Result<ClientReliableTransportIdentity> {
    if view.frame_kind() != Some(MFrameType::ReliableData) {
        anyhow::bail!("client reliable transport identity requires a type-0 data frame");
    }
    let bytes_from_offset_8 = packet
        .get(8..)
        .ok_or_else(|| anyhow::anyhow!("client reliable frame ended before immutable offset 8"))?
        .to_vec();
    Ok(ClientReliableTransportIdentity {
        datagram_len: packet.len(),
        immutable_flags: view.flags & !FRAME_SEND_OWNED_FLAG_MASK,
        bytes_from_offset_8,
    })
}

fn preview_origin_generation(
    state: &ClientReliableReplayState,
    sequence: u16,
) -> OriginGenerationObservation {
    let Some(latest) = state.latest_origin_sequence else {
        return OriginGenerationObservation {
            generation: state.origin_generation,
            latest_origin_sequence: sequence,
            origin_generation: state.origin_generation,
        };
    };
    if sequence == latest {
        return OriginGenerationObservation {
            generation: state.origin_generation,
            latest_origin_sequence: latest,
            origin_generation: state.origin_generation,
        };
    }

    let forward_distance = sequence.wrapping_sub(latest);
    if forward_distance < 0x8000 {
        let origin_generation = if sequence < latest {
            state.origin_generation.saturating_add(1)
        } else {
            state.origin_generation
        };
        OriginGenerationObservation {
            generation: origin_generation,
            latest_origin_sequence: sequence,
            origin_generation,
        }
    } else if sequence > latest && state.origin_generation > 0 {
        OriginGenerationObservation {
            generation: state.origin_generation - 1,
            latest_origin_sequence: latest,
            origin_generation: state.origin_generation,
        }
    } else {
        OriginGenerationObservation {
            generation: state.origin_generation,
            latest_origin_sequence: latest,
            origin_generation: state.origin_generation,
        }
    }
}

fn apply_origin_generation(
    state: &mut ClientReliableReplayState,
    observation: OriginGenerationObservation,
) {
    state.latest_origin_sequence = Some(observation.latest_origin_sequence);
    state.origin_generation = observation.origin_generation;
}
