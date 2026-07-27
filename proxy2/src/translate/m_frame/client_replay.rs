//! Immutable client reliable-data slot ownership and deterministic replay.
//!
//! The original reliable window stores one type-0 datagram per sequence slot
//! before CNW gameplay dispatch. A retransmit may refresh the CRC, ACK, and
//! FrameSend-owned bit 6, but it cannot replace the stored packetized shape or
//! gameplay bytes. Keep that transport identity separate from semantic state:
//! a strict reader rejection leaves the source slot pinned, while an exact
//! retry may translate again from the rolled-back semantic boundary. Once a
//! translation and its Diamond-facing output pass the outer strict owner, the
//! contiguous source slot is released and its immutable identity moves to the
//! bounded retired ledger.

use std::collections::VecDeque;

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{MFrameType, MFrameView},
    translate::VerifiedFamily,
};

use super::transport_identity::SEND_WINDOW_BIT6_MASK;

/// Diamond initializes both reliable receive intervals with 16 slots at lines
/// 750687-750694 and 750769-750775; EE does the same at lines 891083-891086 and
/// 891172-891173.
pub(super) const MAX_CLIENT_RELIABLE_SLOTS: usize = 16;
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
    /// Latest exact datagram for this immutable reliable identity. ACK, CRC,
    /// and FrameSend-owned bit 6 may refresh on retransmission, so the
    /// canonical identity below remains the conflict authority.
    pub(super) packet: Vec<u8>,
    pub(super) transport_identity: ClientReliableTransportIdentity,
    /// `None` means the transport slot is pinned but its semantic disposition
    /// is retryable (for example after an outer strict rejection or the
    /// Module_Loaded resource gate deliberately consumes an early attempt).
    pub(super) replay: Option<ClientReliableTranslationReplay>,
    /// An in-window source behind a missing predecessor is retained without
    /// entering CNW semantics. Once the predecessor commits, the network loop
    /// dispatches this exact packet once through the ordinary strict path.
    pub(super) deferred_behind_gap: bool,
    /// The source reached the receive frontier, but its translated batch
    /// needed more Diamond output slots than were available. The original
    /// windows leave queued frames pending until ACK retirement lets
    /// `LoadWindowWithFrames` move them into the retained send interval.
    /// Proxy2 keeps the raw source pinned and re-enters the ordinary strict
    /// translator only after the same bounded capacity condition becomes true.
    pub(super) deferred_output_capacity_slots: Option<usize>,
}

#[derive(Debug, Clone)]
struct RetiredClientReliableSlot {
    key: ClientReliableSlotKey,
    transport_identity: ClientReliableTransportIdentity,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ClientReliableReplayState {
    pub(super) slots: VecDeque<ClientReliableSlot>,
    /// First source sequence in the circular half-open receive interval
    /// `[receive_start, receive_start + 16)`. Isolated replay fixtures anchor
    /// this on their first validated type-0 source.
    pub(super) receive_start: Option<u16>,
    /// Generation owning `receive_start`; an admitted slot after an in-window
    /// `0xFFFF -> 0x0000` wrap belongs to the following generation.
    pub(super) origin_generation: u64,
    /// The originals free cumulatively acknowledged receive slots and never
    /// dispatch an older sequence again. Retain only their immutable identity
    /// in a diagnostic side ledger so proxy2 can distinguish an exact delayed
    /// EE retransmit from conflicting bytes without reopening engine effects.
    /// This is bounded to the same 16-slot interval as the live window.
    retired_history: VecDeque<RetiredClientReliableSlot>,
    pub(super) exact_replays: u64,
}

/// Exact client receive-retention state at a strict semantic commit boundary.
///
/// Diamond `sub_5F3940` lines 751482-751549/751605-751724 and EE
/// `CNetLayerWindow::FrameReceive` lines 878891-878952/879029-879135 prove
/// separate 16-slot receive release and destination send-window retirement.
/// This diagnostic remains useful for proving that source release tracks the
/// strict contiguous dispatch frontier rather than downstream Diamond ACK
/// latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClientReliableRetentionDiagnostic {
    pub(super) receive_start: Option<u16>,
    pub(super) retained_slots: usize,
    pub(super) translated_disposition_slots: usize,
    pub(super) retryable_slots: usize,
    pub(super) contiguous_translated_prefix: usize,
    pub(super) at_full_translated_retention: bool,
}

#[derive(Debug, Clone)]
pub(super) enum PreparedClientReliableSource {
    Excluded,
    Pending(ClientReliableSlotKey),
    Conflict(ClientReliableSlotKey),
    RetiredReplay(ClientReliableSlotKey),
    RetiredConflict(ClientReliableSlotKey),
    OutsideWindow(ClientReliableSlotKey),
    Replay {
        key: ClientReliableSlotKey,
        replay: ClientReliableTranslationReplay,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientReliableDispatchAdmission {
    Excluded,
    Ready(ClientReliableSlotKey),
    Future {
        key: ClientReliableSlotKey,
        expected: ClientReliableSlotKey,
    },
}

impl PreparedClientReliableSource {
    pub(super) fn key(&self) -> Option<ClientReliableSlotKey> {
        match self {
            Self::Excluded => None,
            Self::Pending(key)
            | Self::Conflict(key)
            | Self::RetiredReplay(key)
            | Self::RetiredConflict(key)
            | Self::OutsideWindow(key)
            | Self::Replay { key, .. } => Some(*key),
        }
    }
}

pub(super) fn retention_diagnostic(
    state: &ClientReliableReplayState,
) -> ClientReliableRetentionDiagnostic {
    let translated_disposition_slots = state
        .slots
        .iter()
        .filter(|slot| slot.replay.is_some())
        .count();
    let retryable_slots = state
        .slots
        .len()
        .saturating_sub(translated_disposition_slots);
    let contiguous_translated_prefix = state.receive_start.map_or(0, |receive_start| {
        (0..MAX_CLIENT_RELIABLE_SLOTS)
            .take_while(|offset| {
                let sequence = receive_start.wrapping_add(*offset as u16);
                let generation = generation_for_sequence(state, receive_start, sequence, *offset);
                state.slots.iter().any(|slot| {
                    slot.key.sequence == sequence
                        && slot.key.origin_generation == generation
                        && slot.replay.is_some()
                })
            })
            .count()
    });
    let retained_slots = state.slots.len();
    ClientReliableRetentionDiagnostic {
        receive_start: state.receive_start,
        retained_slots,
        translated_disposition_slots,
        retryable_slots,
        contiguous_translated_prefix,
        at_full_translated_retention: retained_slots == MAX_CLIENT_RELIABLE_SLOTS
            && contiguous_translated_prefix == MAX_CLIENT_RELIABLE_SLOTS,
    }
}

/// Pin or match the immutable source identity before any semantic mutation.
///
/// Diamond `sub_5F3940` lines 751482-751549 and EE
/// `CNetLayerWindow::FrameReceive` lines 878891-878952 admit kind 0 only inside
/// the circular 16-slot interval and never replace an occupied slot. Controls
/// are deliberately excluded even when their unused sequence field is nonzero.
pub(super) fn prepare_source_slot(
    state: &mut ClientReliableReplayState,
    packet: &[u8],
    view: &MFrameView,
) -> anyhow::Result<PreparedClientReliableSource> {
    if view.frame_kind() != Some(MFrameType::ReliableData) {
        return Ok(PreparedClientReliableSource::Excluded);
    }

    let transport_identity = transport_identity(packet, view)?;
    let receive_start = *state.receive_start.get_or_insert(view.sequence);
    let distance = view.sequence.wrapping_sub(receive_start) as usize;
    let key = ClientReliableSlotKey {
        lane: MFrameType::ReliableData,
        sequence: view.sequence,
        origin_generation: generation_for_sequence(state, receive_start, view.sequence, distance),
    };
    if distance >= MAX_CLIENT_RELIABLE_SLOTS {
        if let Some(retired) = state
            .retired_history
            .iter()
            .rev()
            .find(|retired| retired.key == key)
        {
            return Ok(if retired.transport_identity == transport_identity {
                PreparedClientReliableSource::RetiredReplay(key)
            } else {
                PreparedClientReliableSource::RetiredConflict(key)
            });
        }
        return Ok(PreparedClientReliableSource::OutsideWindow(key));
    }

    let existing = state.slots.iter_mut().find(|slot| slot.key == key);
    if let Some(existing) = existing {
        if existing.transport_identity != transport_identity {
            return Ok(PreparedClientReliableSource::Conflict(key));
        }
        existing.packet = packet.to_vec();
        return Ok(match existing.replay.clone() {
            Some(replay) => PreparedClientReliableSource::Replay { key, replay },
            None => PreparedClientReliableSource::Pending(key),
        });
    }

    state.slots.push_back(ClientReliableSlot {
        key,
        packet: packet.to_vec(),
        transport_identity,
        replay: None,
        deferred_behind_gap: false,
        deferred_output_capacity_slots: None,
    });
    debug_assert!(state.slots.len() <= MAX_CLIENT_RELIABLE_SLOTS);
    tracing::trace!(
        sequence = key.sequence,
        origin_generation = key.origin_generation,
        receive_start,
        retained_slots = state.slots.len(),
        "client reliable-data slot pinned inside the 16-frame receive window"
    );
    Ok(PreparedClientReliableSource::Pending(key))
}

/// Admit one pinned type-0 client source to semantic dispatch in reliable
/// receive order.
///
/// Diamond `sub_5F3940` lines 751571-751673 and EE `FrameReceive` lines
/// 879029-879088 dispatch only the occupied receive-frontier slot and then walk
/// its contiguous successors. A future in-window source is transport truth,
/// but it cannot touch packetized/CNW state before its predecessor commits.
pub(super) fn prepare_source_dispatch(
    state: &mut ClientReliableReplayState,
    prepared: &PreparedClientReliableSource,
) -> anyhow::Result<ClientReliableDispatchAdmission> {
    let key = match prepared {
        PreparedClientReliableSource::Excluded => {
            return Ok(ClientReliableDispatchAdmission::Excluded);
        }
        PreparedClientReliableSource::Pending(key)
        | PreparedClientReliableSource::Replay { key, .. } => *key,
        PreparedClientReliableSource::Conflict(key)
        | PreparedClientReliableSource::RetiredReplay(key)
        | PreparedClientReliableSource::RetiredConflict(key)
        | PreparedClientReliableSource::OutsideWindow(key) => {
            anyhow::bail!(
                "client reliable source {} generation {} reached dispatch admission after transport rejection",
                key.sequence,
                key.origin_generation
            );
        }
    };
    let receive_start = state
        .receive_start
        .ok_or_else(|| anyhow::anyhow!("client dispatch has no receive frontier"))?;
    let expected = ClientReliableSlotKey {
        lane: MFrameType::ReliableData,
        sequence: receive_start,
        origin_generation: state.origin_generation,
    };
    if key == expected {
        if let Some(slot) = state.slots.iter_mut().find(|slot| slot.key == key) {
            slot.deferred_behind_gap = false;
            slot.deferred_output_capacity_slots = None;
        }
        return Ok(ClientReliableDispatchAdmission::Ready(key));
    }

    let Some(slot) = state.slots.iter_mut().find(|slot| slot.key == key) else {
        anyhow::bail!(
            "future client reliable dispatch {} generation {} lost its retained raw slot",
            key.sequence,
            key.origin_generation
        );
    };
    slot.deferred_behind_gap = true;
    tracing::trace!(
        sequence = key.sequence,
        origin_generation = key.origin_generation,
        expected_sequence = expected.sequence,
        expected_origin_generation = expected.origin_generation,
        retained_slots = state.slots.len(),
        "client reliable source retained behind its missing receive-frontier predecessor"
    );
    Ok(ClientReliableDispatchAdmission::Future { key, expected })
}

/// Retain the exact receive-frontier source until enough Diamond send-window
/// slots exist for the already-proven translated batch shape.
pub(super) fn defer_frontier_for_output_capacity(
    state: &mut ClientReliableReplayState,
    key: ClientReliableSlotKey,
    required_slots: usize,
) -> bool {
    if required_slots == 0 {
        return false;
    }
    let expected = state.receive_start.map(|sequence| ClientReliableSlotKey {
        lane: MFrameType::ReliableData,
        sequence,
        origin_generation: state.origin_generation,
    });
    if expected != Some(key) {
        return false;
    }
    let Some(slot) = state.slots.iter_mut().find(|slot| slot.key == key) else {
        return false;
    };
    slot.deferred_output_capacity_slots = Some(required_slots);
    true
}

/// Take one raw client source that became contiguous after its predecessor
/// committed or whose exact Diamond output capacity is now available.
/// Clearing the marker makes this an at-most-once internal handoff; strict
/// rejection waits for a real exact retransmit unless it proves another
/// capacity deferral.
pub(super) fn take_deferred_frontier_packet(
    state: &mut ClientReliableReplayState,
    available_output_slots: usize,
) -> Option<(ClientReliableSlotKey, Vec<u8>)> {
    let receive_start = state.receive_start?;
    let expected = ClientReliableSlotKey {
        lane: MFrameType::ReliableData,
        sequence: receive_start,
        origin_generation: state.origin_generation,
    };
    let slot = state.slots.iter_mut().find(|slot| slot.key == expected)?;
    let output_capacity_ready = slot
        .deferred_output_capacity_slots
        .is_some_and(|required| required <= available_output_slots);
    if !slot.deferred_behind_gap && !output_capacity_ready {
        return None;
    }
    slot.deferred_behind_gap = false;
    slot.deferred_output_capacity_slots = None;
    Some((slot.key, slot.packet.clone()))
}

/// Release the strict-committed contiguous source prefix only after the
/// associated Diamond-facing output batch has committed to its independent
/// reliable send window.
///
/// The current dispatch key must own the receive frontier. A later in-window
/// source stays pinned, even if its immutable datagram already arrived. Each
/// released identity moves into the bounded retired ledger before the local
/// cumulative ACK is published toward EE.
pub(super) fn release_committed_prefix(
    state: &mut ClientReliableReplayState,
    committed: ClientReliableSlotKey,
) -> Option<ClientReliableSlotKey> {
    let receive_start = state.receive_start?;
    let expected = ClientReliableSlotKey {
        lane: MFrameType::ReliableData,
        sequence: receive_start,
        origin_generation: state.origin_generation,
    };
    if committed != expected {
        return None;
    }

    let slot_index = state
        .slots
        .iter()
        .position(|slot| slot.key == committed && slot.replay.is_some())?;
    let slot = state
        .slots
        .remove(slot_index)
        .expect("located strict-committed client receive slot");
    state.retired_history.push_back(RetiredClientReliableSlot {
        key: slot.key,
        transport_identity: slot.transport_identity,
    });
    while state.retired_history.len() > MAX_CLIENT_RELIABLE_SLOTS {
        let _ = state.retired_history.pop_front();
    }

    let next_sequence = committed.sequence.wrapping_add(1);
    let next_generation = committed
        .origin_generation
        .saturating_add(u64::from(committed.sequence == u16::MAX));
    state.receive_start = Some(next_sequence);
    state.origin_generation = next_generation;
    tracing::trace!(
        sequence = committed.sequence,
        origin_generation = committed.origin_generation,
        next_sequence,
        next_origin_generation = next_generation,
        retained_slots = state.slots.len(),
        retired_history = state.retired_history.len(),
        "strict-committed client source released after Diamond output ownership committed"
    );
    Some(committed)
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

fn generation_for_sequence(
    state: &ClientReliableReplayState,
    receive_start: u16,
    sequence: u16,
    forward_distance: usize,
) -> u64 {
    if forward_distance < 0x8000 && sequence < receive_start {
        state.origin_generation.saturating_add(1)
    } else if forward_distance >= 0x8000 && sequence > receive_start {
        state.origin_generation.saturating_sub(1)
    } else {
        state.origin_generation
    }
}
