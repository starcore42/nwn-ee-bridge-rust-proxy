//! Deflated reliable-window reassembly data types.
//!
//! This module owns deflated-window collection, duplicate replay, safe
//! consumed-frame emission, and reconstruction of repaired deflated output
//! frames. Semantic packet translation remains outside this module.

use flate2::{Decompress, FlushDecompress};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{
        LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MAX_REASONABLE_GAMEPLAY_PAYLOAD, MFrameView,
        parse_packetized_spans,
    },
    translate::{ContinuationOwner, Emit, VerifiedFamily, VerifiedPacket, VerifiedProof},
};

// Decompile note: EE's CNetLayerWindow::FrameReceive stores reliable data frames
// by the incoming datagram size and only advances the receive window through
// accepted in-order frames. When a semantic rewrite makes a deflated payload
// larger than the legacy packet's original datagram budget, we must packetize it
// into additional reliable M frames instead of emitting one oversized datagram.
//
// The observed accepted HG/EE gameplay window cap is a 960-byte datagram
// (12-byte legacy M header + 948 payload bytes). Keeping rewritten deflated
// output at or below that cap avoids ACK starvation while preserving the exact
// translated high-level payload.
pub(super) const EE_SAFE_M_FRAME_DATAGRAM_BYTES: usize = 960;
pub(super) const EE_SAFE_M_FRAME_PAYLOAD_BYTES: usize =
    EE_SAFE_M_FRAME_DATAGRAM_BYTES - LEGACY_GAMEPLAY_PAYLOAD_OFFSET;

use super::{
    MAX_INTERLEAVED_PACKETS, MAX_REASSEMBLY_FRAMES, SessionState,
    deflate::{inflate_with_server_stream, inflate_with_window, looks_like_zlib_wrapped_deflate},
    transport_identity,
};

#[derive(Debug, Clone)]
pub(super) struct ServerDeflatedReassembly {
    pub(super) inflated_length: usize,
    pub(super) expected_frames: usize,
    pub(super) first_sequence: u16,
    /// Wrap-safe generation of `first_sequence` in the server-origin reliable
    /// window. Sequence numbers alone are reusable after `u16` wrap, so a
    /// completed persistent-stream disposition must never match without this
    /// transport epoch.
    pub(super) server_origin_generation: u64,
    pub(super) packetized_sequence: u16,
    pub(super) zlib_stream: bool,
    pub(super) frames: Vec<BufferedFrame>,
    pub(super) interleaved_packets: Vec<VerifiedPacket>,
    /// Exact reliable frames that arrived after this deflated window while one
    /// of its packetized members was still missing. `interleaved_packets`
    /// carries fail-closed empty data-frame placeholders until the predecessor
    /// window has a successful typed disposition; only then may the parent
    /// dispatcher translate these events in source-sequence order.
    pub(super) interleaved_events: Vec<BufferedInterleavedServerPacket>,
}

#[derive(Debug, Clone)]
pub(super) struct BufferedInterleavedServerPacket {
    pub(super) packet: Vec<u8>,
    pub(super) sequence: u16,
    pub(super) server_peer_ack_sequence: u16,
    pub(super) server_origin_generation: u64,
    /// Header bytes from flags through the exact payload/trailing window. CRC,
    /// sequence, and ACK are intentionally excluded so a retransmit can refresh
    /// transport state without becoming a second semantic event.
    pub(super) transport_payload_identity: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct BufferedFrame {
    pub(super) packet: Vec<u8>,
    pub(super) payload_length: usize,
    pub(super) sequence: u16,
    pub(super) server_peer_ack_sequence: u16,
    pub(super) ack_sequence: u16,
    pub(super) compressed_chunk: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct CompletedDeflatedStreamWindow {
    pub(super) first_sequence: u16,
    pub(super) server_origin_generation: u64,
    pub(super) expected_frames: usize,
    pub(super) packetized_sequence: u16,
    pub(super) inflated_length: usize,
    /// Exact immutable compressed bytes from every source frame, in reliable
    /// order. A length-only match is unsafe because a different payload in the
    /// same reliable slot must not inherit cached semantics or skip inflation.
    pub(super) compressed: Vec<u8>,
    /// Per-frame immutable transport bytes from flags through the exact
    /// datagram tail. CRC, sequence, and ACK are deliberately excluded:
    /// retransmit ACKs may advance, but flags, packetized lengths, chunk
    /// boundaries, payload, and any trailing bytes may not change within one
    /// reliable generation.
    pub(super) frame_transport_identities: Vec<Vec<u8>>,
    /// Expanded replacements are shifted before their future source-sequence
    /// shift is recorded. Remember that disposition so retransmission cannot
    /// pass through the ordinary finalizer and apply the same shift twice.
    pub(super) pre_shifted: bool,
    pub(super) replay: CompletedDeflatedReplay,
}

#[derive(Debug, Clone)]
pub(super) enum CompletedDeflatedStreamWindowMatch {
    Miss,
    Exact(CompletedDeflatedStreamWindow),
    /// The reliable slot/generation was already committed, but the immutable
    /// source shape or compressed bytes differ. This is not a replay.
    Conflict,
}

#[derive(Debug, Clone)]
pub(super) enum CompletedDeflatedReplay {
    /// The inflated payload was understood as already EE-safe, so duplicates can
    /// replay the same reliable-window records without touching the inflater.
    Packets(Vec<Vec<u8>>),
    /// The inflated payload was either translated or deliberately quarantined.
    /// Duplicates must preserve that exact safe disposition; raw legacy bytes
    /// must never leak through on retransmit.
    VerifiedPackets {
        family: VerifiedFamily,
        packets: Vec<Vec<u8>>,
    },
    VerifiedProofPackets {
        proof: VerifiedProof,
        packets: Vec<Vec<u8>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompletedServerReliableStreamRoute {
    StreamWindow,
    CoalescedWindow,
}

impl CompletedServerReliableStreamRoute {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::StreamWindow => "stream-window",
            Self::CoalescedWindow => "coalesced-window",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CompletedServerReliableStreamSlot {
    pub(super) sequence: u16,
    pub(super) server_origin_generation: u64,
    pub(super) transport_identity: Vec<u8>,
    pub(super) route: CompletedServerReliableStreamRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompletedServerReliableStreamSlotMatch {
    Miss,
    Exact(CompletedServerReliableStreamRoute),
    Conflict(CompletedServerReliableStreamRoute),
}

#[derive(Debug, Clone)]
pub(super) struct InflatedGameplayPayload {
    pub(super) bytes: Vec<u8>,
    pub(super) used_server_stream: bool,
}

pub(super) fn emit_family_packets_with_interleaved(
    family: VerifiedFamily,
    packets: Vec<Vec<u8>>,
    interleaved: Vec<VerifiedPacket>,
) -> Emit {
    emit_proof_packets_with_interleaved(VerifiedProof::family(family), packets, interleaved)
}

pub(super) fn emit_proof_packets_with_interleaved(
    proof: VerifiedProof,
    packets: Vec<Vec<u8>>,
    interleaved: Vec<VerifiedPacket>,
) -> Emit {
    if interleaved.is_empty() {
        return Emit::VerifiedProofPackets { proof, packets };
    }

    let mut mixed = Vec::with_capacity(packets.len() + interleaved.len());
    mixed.extend(packets.into_iter().map(|packet| (proof.clone(), packet)));
    mixed.extend(
        interleaved
            .into_iter()
            .map(|packet| (packet.proof, packet.packet)),
    );
    Emit::MixedVerifiedProofPackets(mixed)
}

pub(super) fn should_start_server_deflated_reassembly(view: &MFrameView) -> bool {
    view.deflated
        .as_ref()
        .map(|deflated| deflated.plausible && view.payload_length >= 4)
        .unwrap_or(false)
}

pub(super) fn start_server_deflated_reassembly(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
    server_peer_ack_sequence: u16,
    server_origin_generation: u64,
) -> anyhow::Result<Emit> {
    let deflated = view
        .deflated
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing deflated envelope"))?;
    let expected_frames = if view.packetized_sequence > 1 {
        usize::from(view.packetized_sequence)
    } else {
        1
    };
    if expected_frames == 0 || expected_frames > MAX_REASSEMBLY_FRAMES {
        tracing::warn!(
            expected_frames,
            sequence = view.sequence,
            packetized_sequence = view.packetized_sequence,
            "server deflated M reassembly quarantined: implausible expected frame count"
        );
        return Ok(Emit::Drop);
    }

    let frame = buffered_frame_from_view(bytes, view, server_peer_ack_sequence, true)?;
    let mut reassembly = ServerDeflatedReassembly {
        inflated_length: deflated.inflated_length,
        expected_frames,
        first_sequence: view.sequence,
        server_origin_generation,
        packetized_sequence: view.packetized_sequence,
        zlib_stream: (view.flags & 0x01) != 0,
        frames: Vec::with_capacity(expected_frames),
        interleaved_packets: Vec::new(),
        interleaved_events: Vec::new(),
    };
    reassembly.frames.push(frame);
    state.deflate.server_reassembly = Some(reassembly);

    tracing::info!(
        inflated_length = deflated.inflated_length,
        expected_frames,
        sequence = view.sequence,
        packetized_sequence = view.packetized_sequence,
        zlib_stream = (view.flags & 0x01) != 0,
        "server deflated M reassembly started"
    );

    if expected_frames == 1 {
        super::emit_completed_server_deflated_reassembly(state)
    } else {
        // Strict translation discipline: a multi-frame deflated window is not
        // EE-safe until the full inflated payload has been classified and
        // claimed by a semantic translator. Hold the partial legacy frame
        // instead of leaking a transport placeholder to the client. Because the
        // proxy is now the reliable-window endpoint for this consumed frame, it
        // also sends a verified empty ACK/control shell upstream so Diamond can
        // continue the packetized window.
        queue_reassembly_progress_ack(state, "server deflated M initial frame buffered")?;
        Ok(Emit::Consumed)
    }
}

pub(super) fn continue_server_deflated_reassembly(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
    server_peer_ack_sequence: u16,
    server_origin_generation: u64,
) -> anyhow::Result<Emit> {
    let Some(snapshot) = state.deflate.server_reassembly.as_ref() else {
        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            flags = view.flags,
            packetized_sequence = view.packetized_sequence,
            payload_len = view.payload_length,
            "server deflated M continuation quarantined: no active reassembly owner"
        );
        return Ok(Emit::Drop);
    };

    // Sequence zero is the non-data reliable-window ACK/control lane. It does
    // not participate in the source data cursor, so it may pass immediately
    // while a packetized data predecessor is held. The exact empty-shell claim
    // preserves the current unshifted ACK, control kind, and verified CRC.
    if view.sequence == 0 {
        let Some(claim) = transport_identity::claim_server_frame_if_verified(view) else {
            return Ok(Emit::Drop);
        };
        let owner = state
            .deflate
            .server_zlib_stream_owner
            .unwrap_or(ContinuationOwner::UnknownProxyOwned);
        return Ok(
            match transport_identity::verified_server_packet_for_claim(
                bytes,
                view,
                claim,
                owner,
                state.deflate.server_zlib_stream_epoch,
                state.deflate.server_zlib_stream_proxy_owned,
            )? {
                Some(verified) => Emit::VerifiedProofPackets {
                    proof: verified.proof,
                    packets: vec![verified.packet],
                },
                None => Emit::Drop,
            },
        );
    }

    let first_sequence = snapshot.first_sequence;
    let expected_frames = snapshot.expected_frames;
    let distance = view.sequence.wrapping_sub(first_sequence) as usize;
    if distance >= expected_frames {
        // A wrapping distance in the upper half of the sequence space is an
        // older/stale frame, not a packet following the pending window. Never
        // let it enter the ordered transaction under a future-looking alias.
        if distance >= 0x8000 {
            tracing::warn!(
                sequence = view.sequence,
                first_sequence,
                expected_frames,
                "stale server M frame rejected while deflated reassembly is pending"
            );
            return Ok(Emit::Drop);
        }
        let interleaved_packet = VerifiedPacket {
            proof: VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
            packet: consume_interleaved_unclaimed_server_packet(bytes)?,
        };
        let transport_payload_identity = bytes.get(7..).unwrap_or_default().to_vec();
        let Some(reassembly) = state.deflate.server_reassembly.as_mut() else {
            return Ok(Emit::Drop);
        };
        if let Some(index) = reassembly.interleaved_events.iter().position(|event| {
            event.server_origin_generation == server_origin_generation
                && event.sequence == view.sequence
        }) {
            if reassembly.interleaved_events[index].transport_payload_identity
                != transport_payload_identity
            {
                tracing::warn!(
                    sequence = view.sequence,
                    first_sequence = reassembly.first_sequence,
                    server_origin_generation,
                    "conflicting interleaved server M retransmit rejected while retaining the first accepted reliable identity"
                );
                return Ok(Emit::Drop);
            }
            reassembly.interleaved_events[index].packet = bytes.to_vec();
            reassembly.interleaved_events[index].server_peer_ack_sequence =
                server_peer_ack_sequence;
            reassembly.interleaved_packets[index] = interleaved_packet;
            tracing::info!(
                sequence = view.sequence,
                first_sequence = reassembly.first_sequence,
                server_origin_generation,
                "duplicate interleaved server M frame refreshed without duplicating semantic state"
            );
            return Ok(Emit::Consumed);
        }
        if reassembly.interleaved_events.len() >= MAX_INTERLEAVED_PACKETS {
            tracing::warn!(
                sequence = view.sequence,
                first_sequence = reassembly.first_sequence,
                expected_frames = reassembly.expected_frames,
                "server deflated M reassembly abandoned after too many interleaved packets"
            );
            state.deflate.server_reassembly = None;
            return Ok(Emit::Drop);
        }
        let insert_index = reassembly
            .interleaved_events
            .iter()
            .position(|existing| {
                existing.sequence.wrapping_sub(reassembly.first_sequence) > distance as u16
            })
            .unwrap_or(reassembly.interleaved_events.len());
        reassembly.interleaved_events.insert(
            insert_index,
            BufferedInterleavedServerPacket {
                packet: bytes.to_vec(),
                sequence: view.sequence,
                server_peer_ack_sequence,
                server_origin_generation,
                transport_payload_identity,
            },
        );
        reassembly
            .interleaved_packets
            .insert(insert_index, interleaved_packet);
        tracing::info!(
            sequence = view.sequence,
            first_sequence = reassembly.first_sequence,
            expected_frames = reassembly.expected_frames,
            buffered_interleaved = reassembly.interleaved_events.len(),
            server_origin_generation,
            "server M frame buffered behind pending deflated predecessor transaction"
        );
        return Ok(Emit::Consumed);
    }

    let Some(reassembly) = state.deflate.server_reassembly.as_mut() else {
        return Ok(Emit::Drop);
    };

    if let Some(index) = reassembly
        .frames
        .iter()
        .position(|frame| frame.sequence == view.sequence)
    {
        let refreshed = buffered_frame_from_view(
            bytes,
            view,
            server_peer_ack_sequence,
            view.sequence == reassembly.first_sequence,
        )?;
        if buffered_frame_transport_identity(&reassembly.frames[index])
            != buffered_frame_transport_identity(&refreshed)
        {
            tracing::warn!(
                sequence = view.sequence,
                first_sequence = reassembly.first_sequence,
                server_origin_generation,
                "conflicting server deflated M window member rejected while retaining the first accepted reliable identity"
            );
            return Ok(Emit::Drop);
        }
        reassembly.frames[index] = refreshed;
        let buffered_frames = reassembly.frames.len();
        let expected_frames = reassembly.expected_frames;
        let first_sequence = reassembly.first_sequence;
        tracing::info!(
            sequence = view.sequence,
            first_sequence,
            buffered_frames,
            expected_frames,
            ack_sequence = view.ack_sequence,
            server_peer_ack_sequence,
            "duplicate server deflated M frame refreshed without changing immutable payload"
        );
        if buffered_frames + 1 >= expected_frames {
            if let Some(emit) = super::try_emit_salvaged_incomplete_server_deflated_reassembly(
                state,
                "duplicate retransmit while one packetized frame is missing",
            )? {
                return Ok(emit);
            }
        }
        queue_reassembly_progress_ack(
            state,
            "duplicate consumed deflated M frame re-acknowledged",
        )?;
        return Ok(Emit::Consumed);
    }

    let frame = buffered_frame_from_view(bytes, view, server_peer_ack_sequence, false)?;
    let insert_index = reassembly
        .frames
        .iter()
        .position(|existing| {
            existing.sequence.wrapping_sub(reassembly.first_sequence) > distance as u16
        })
        .unwrap_or(reassembly.frames.len());
    reassembly.frames.insert(insert_index, frame);

    if reassembly.frames.len() < reassembly.expected_frames {
        queue_reassembly_progress_ack(state, "server deflated M continuation buffered")?;
        return Ok(Emit::Consumed);
    }

    super::emit_completed_server_deflated_reassembly(state)
}

pub(super) fn queue_reassembly_progress_ack(
    state: &mut SessionState,
    reason: &'static str,
) -> anyhow::Result<()> {
    let Some(ack_sequence) = state
        .deflate
        .server_reassembly
        .as_ref()
        .and_then(highest_contiguous_buffered_sequence)
    else {
        return Ok(());
    };
    super::local_ack::queue_consumed_server_frame_ack(state, ack_sequence, reason)
}

fn highest_contiguous_buffered_sequence(reassembly: &ServerDeflatedReassembly) -> Option<u16> {
    let mut expected_distance = 0usize;
    let mut ack_sequence = None;
    for frame in &reassembly.frames {
        let distance = frame.sequence.wrapping_sub(reassembly.first_sequence) as usize;
        if distance < expected_distance {
            continue;
        }
        if distance != expected_distance {
            break;
        }
        ack_sequence = Some(frame.sequence);
        expected_distance = expected_distance.saturating_add(1);
    }
    ack_sequence
}

fn consume_interleaved_unclaimed_server_packet(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut out_packet = bytes.to_vec();
    out_packet.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if out_packet.len() > 7 {
        // Decompile-backed EE window behavior: byte 7's high nibble selects
        // the M-frame kind. Only kind 0 enters CNetLayerWindow::FrameReceive's
        // reliable-data path, which stores the frame and advances the incoming
        // sequence/ACK cursor. The 0x10 control kind is ACK-only and does not
        // consume a sequence number, so an empty progress carrier must stay a
        // data frame while clearing zlib/packet-length semantics. Preserve only
        // the high-priority queue bit.
        out_packet[7] &= 0x08;
    }
    write_be_u16(&mut out_packet, 8, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear interleaved M packetized sequence"))?;
    write_be_u16(&mut out_packet, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear interleaved M packetized length"))?;
    encode_legacy_m_crc(&mut out_packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair interleaved M CRC"))?;
    Ok(out_packet)
}

fn buffered_frame_from_view(
    bytes: &[u8],
    view: &MFrameView,
    server_peer_ack_sequence: u16,
    first_frame: bool,
) -> anyhow::Result<BufferedFrame> {
    if view.payload_length > bytes.len().saturating_sub(LEGACY_GAMEPLAY_PAYLOAD_OFFSET) {
        anyhow::bail!("M payload length exceeds datagram");
    }

    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let payload_end = payload_start + view.payload_length;
    let compressed_start = if first_frame {
        if view.payload_length < 4 {
            anyhow::bail!("first deflated M frame is too short for inflated length");
        }
        payload_start + 4
    } else {
        payload_start
    };

    Ok(BufferedFrame {
        packet: bytes.to_vec(),
        payload_length: view.payload_length,
        sequence: view.sequence,
        server_peer_ack_sequence,
        ack_sequence: view.ack_sequence,
        compressed_chunk: bytes[compressed_start..payload_end].to_vec(),
    })
}

fn buffered_frame_transport_identity(frame: &BufferedFrame) -> Option<Vec<u8>> {
    frame.packet.get(7..).map(<[u8]>::to_vec)
}

fn completed_stream_frame_transport_identities(
    reassembly: &ServerDeflatedReassembly,
) -> Option<Vec<Vec<u8>>> {
    reassembly
        .frames
        .iter()
        .map(buffered_frame_transport_identity)
        .collect()
}

pub(super) fn build_server_deflated_output_frames(
    reassembly: &ServerDeflatedReassembly,
    combined_payload: &[u8],
    clear_first_frame_flags: u8,
    set_first_packetized_sequence_to_output_count: bool,
) -> anyhow::Result<Vec<Vec<u8>>> {
    if reassembly.frames.is_empty() {
        anyhow::bail!("cannot rebuild deflated output without source frames");
    }

    let output_count = deflated_output_frame_count(reassembly, combined_payload.len())?;
    let mut outputs = Vec::with_capacity(output_count);
    let mut cursor = 0usize;

    for index in 0..output_count {
        if cursor > combined_payload.len() {
            anyhow::bail!("deflated output cursor exceeded combined payload");
        }

        let frame = template_frame_for_output(reassembly, index);
        let final_frame = index + 1 == output_count;
        let remaining = combined_payload.len() - cursor;
        let frames_left = output_count - index;
        let minimum_reserved_for_later = frames_left.saturating_sub(1);
        let max_this_frame = remaining.saturating_sub(minimum_reserved_for_later);
        let frame_capacity = deflated_output_frame_capacity(reassembly, index);
        let chunk_length = if final_frame {
            remaining
        } else if remaining >= frames_left {
            frame_capacity.min(max_this_frame).max(1)
        } else {
            frame_capacity.min(remaining)
        };
        if chunk_length > u16::MAX as usize {
            anyhow::bail!("deflated output chunk too large for legacy packetized length");
        }

        let mut out_packet = frame.packet.clone();
        out_packet.resize(LEGACY_GAMEPLAY_PAYLOAD_OFFSET + chunk_length, 0);
        let output_sequence = reassembly.first_sequence.wrapping_add(index as u16);
        write_be_u16(&mut out_packet, 3, output_sequence)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update rewritten M sequence"))?;

        if chunk_length != 0 {
            out_packet
                [LEGACY_GAMEPLAY_PAYLOAD_OFFSET..LEGACY_GAMEPLAY_PAYLOAD_OFFSET + chunk_length]
                .copy_from_slice(&combined_payload[cursor..cursor + chunk_length]);
            if index > 0 && out_packet.len() > 7 {
                out_packet[7] = continuation_frame_flags(out_packet[7]);
            }
        } else if out_packet.len() > 7 {
            // Empty replacement tails still need to be reliable-data frames so
            // the EE receive window advances. Clear the deflate/stream bits and
            // keep only priority; a 0x10 control frame would be ignored for
            // sequence progress by CNetLayerWindow::FrameReceive.
            out_packet[7] &= 0x08;
        }
        cursor += chunk_length;

        if index == 0 && clear_first_frame_flags != 0 && out_packet.len() > 7 {
            out_packet[7] &= !clear_first_frame_flags;
        }
        if index > 0 {
            write_be_u16(&mut out_packet, 8, 0)
                .then_some(())
                .ok_or_else(|| {
                    anyhow::anyhow!("failed to clear continuation packetized sequence")
                })?;
        }
        write_be_u16(&mut out_packet, 10, chunk_length as u16)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update M packetized length"))?;
        encode_legacy_m_crc(&mut out_packet)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair M CRC"))?;

        outputs.push(out_packet);
    }

    if cursor != combined_payload.len() || outputs.is_empty() {
        anyhow::bail!(
            "deflated output frame capacity mismatch: combined={} emitted={}",
            combined_payload.len(),
            cursor
        );
    }

    if set_first_packetized_sequence_to_output_count {
        let output_count = outputs.len() as u16;
        let first = outputs
            .first_mut()
            .ok_or_else(|| anyhow::anyhow!("missing first deflated output frame"))?;
        write_be_u16(first, 8, output_count)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update first M packetized sequence"))?;
        encode_legacy_m_crc(first)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair first M CRC"))?;
    }

    if outputs.len() > reassembly.frames.len() {
        tracing::info!(
            first_sequence = reassembly.first_sequence,
            original_frames = reassembly.frames.len(),
            output_frames = outputs.len(),
            combined_payload_len = combined_payload.len(),
            safe_payload_bytes = EE_SAFE_M_FRAME_PAYLOAD_BYTES,
            "server deflated rewrite repacketized into extra reliable M frames"
        );
    }

    Ok(outputs)
}

pub(super) fn build_server_raw_gameplay_output_frames(
    reassembly: &ServerDeflatedReassembly,
    raw_gameplay_payload: &[u8],
) -> anyhow::Result<Vec<Vec<u8>>> {
    // Decompile note: EE `CNetLayerInternal::UncompressMessage` forwards the
    // M payload directly to the gameplay dispatcher when flag 0x04 is clear,
    // and only enters zlib handling when flag 0x04 is set.  Verified semantic
    // replacements that already fit in one EE-safe reliable frame can therefore
    // be emitted as raw gameplay bytes by clearing both the deflate bit (0x04)
    // and the persistent-stream bit (0x01), while preserving reliable/priority
    // bits from the source frame.
    build_server_deflated_output_frames(reassembly, raw_gameplay_payload, 0x05, true)
}

fn deflated_output_frame_count(
    reassembly: &ServerDeflatedReassembly,
    combined_payload_len: usize,
) -> anyhow::Result<usize> {
    let mut count = reassembly.frames.len();
    let mut capacity = 0usize;
    for index in 0..reassembly.frames.len() {
        capacity = capacity.saturating_add(deflated_output_frame_capacity(reassembly, index));
    }

    while capacity < combined_payload_len {
        count = count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("deflated output frame count overflow"))?;
        if count > u16::MAX as usize {
            anyhow::bail!("deflated output frame count exceeds packetized sequence range");
        }
        capacity = capacity.saturating_add(EE_SAFE_M_FRAME_PAYLOAD_BYTES);
    }

    Ok(count)
}

fn deflated_output_frame_capacity(reassembly: &ServerDeflatedReassembly, index: usize) -> usize {
    if index < reassembly.frames.len() {
        // The source frame's legacy datagram size is not a reader-owned
        // boundary in EE.  EE `CNetLayerWindow::FrameReceive` accepts the
        // incoming datagram and advances the reliable window by sequence, while
        // the packetized length field bounds the copied gameplay bytes.  Use
        // the proven EE-safe datagram cap for rewritten server->client frames
        // so a small semantic expansion does not manufacture extra reliable
        // frames and sequence shifts.  New spill frames use the same cap.
        EE_SAFE_M_FRAME_PAYLOAD_BYTES
    } else {
        EE_SAFE_M_FRAME_PAYLOAD_BYTES
    }
}

fn template_frame_for_output(
    reassembly: &ServerDeflatedReassembly,
    index: usize,
) -> &BufferedFrame {
    reassembly
        .frames
        .get(index)
        .or_else(|| reassembly.frames.last())
        .expect("deflated output requires at least one source frame")
}

fn continuation_frame_flags(flags: u8) -> u8 {
    (flags & 0x08) | 0x40
}

pub(super) fn build_consumed_server_deflated_frames(
    reassembly: &ServerDeflatedReassembly,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut outputs = Vec::with_capacity(reassembly.frames.len());
    for frame in &reassembly.frames {
        let mut out_packet = frame.packet.clone();
        out_packet.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
        if out_packet.len() > 7 {
            // Keep the reliable-window sequence/ack shell so the client can
            // acknowledge progress. This must remain frame type 0: EE's
            // decompiled FrameReceive only advances incoming reliable sequence
            // numbers on the data-frame branch. Clear zlib/extended semantics
            // and preserve only high-priority queue placement.
            out_packet[7] &= 0x08;
        }
        write_be_u16(&mut out_packet, 8, 0)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to clear consumed M packetized sequence"))?;
        write_be_u16(&mut out_packet, 10, 0)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to clear consumed M packetized length"))?;
        encode_legacy_m_crc(&mut out_packet)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair consumed M CRC"))?;
        outputs.push(out_packet);
    }
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_reassembly(first_sequence: u16, payload_lengths: &[usize]) -> ServerDeflatedReassembly {
        let frames = payload_lengths
            .iter()
            .enumerate()
            .map(|(index, payload_length)| {
                let sequence = first_sequence.wrapping_add(index as u16);
                let mut packet = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + payload_length];
                packet[0] = b'M';
                write_be_u16(&mut packet, 3, sequence);
                write_be_u16(&mut packet, 5, 75);
                packet[7] = if index == 0 { 0x0D } else { 0x48 };
                write_be_u16(&mut packet, 8, payload_lengths.len() as u16);
                write_be_u16(&mut packet, 10, *payload_length as u16);
                encode_legacy_m_crc(&mut packet);
                BufferedFrame {
                    packet,
                    payload_length: *payload_length,
                    sequence,
                    server_peer_ack_sequence: 75,
                    ack_sequence: 75,
                    compressed_chunk: Vec::new(),
                }
            })
            .collect::<Vec<_>>();

        ServerDeflatedReassembly {
            inflated_length: 4096,
            expected_frames: payload_lengths.len(),
            first_sequence,
            server_origin_generation: 0,
            packetized_sequence: payload_lengths.len() as u16,
            zlib_stream: true,
            frames,
            interleaved_packets: Vec::new(),
            interleaved_events: Vec::new(),
        }
    }

    #[test]
    fn buffered_frame_retains_raw_peer_ack_alongside_unshifted_ack() {
        let mut packet = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 4];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, 40));
        assert!(write_be_u16(&mut packet, 5, 80));
        packet[7] = 0x0D;
        assert!(write_be_u16(&mut packet, 8, 1));
        assert!(write_be_u16(&mut packet, 10, 4));
        assert!(encode_legacy_m_crc(&mut packet));
        let client_view = MFrameView::parse(&packet).expect("unshifted frame should parse");

        let frame = buffered_frame_from_view(&packet, &client_view, 82, true)
            .expect("reassembly frame should retain both ACK spaces");

        assert_eq!(frame.server_peer_ack_sequence, 82);
        assert_eq!(frame.ack_sequence, 80);
    }

    #[test]
    fn pending_window_duplicates_refresh_ack_and_retain_first_identity() {
        let full = make_reassembly(120, &[8, 6, 5, 7]);
        let mut pending = full.clone();
        pending.frames = vec![full.frames[0].clone(), full.frames[3].clone()];
        let mut state = SessionState::default();
        state.deflate.server_reassembly = Some(pending);

        let mut retransmit = full.frames[3].packet.clone();
        assert!(write_be_u16(&mut retransmit, 5, 96));
        assert!(encode_legacy_m_crc(&mut retransmit));
        let retransmit_view = MFrameView::parse(&retransmit).expect("valid retransmit frame");
        assert!(matches!(
            continue_server_deflated_reassembly(&retransmit, &retransmit_view, &mut state, 101, 0,)
                .expect("exact pending duplicate should refresh"),
            Emit::Consumed
        ));
        let refreshed = state
            .deflate
            .server_reassembly
            .as_ref()
            .and_then(|reassembly| reassembly.frames.iter().find(|frame| frame.sequence == 123))
            .expect("pending duplicate should remain buffered");
        assert_eq!(refreshed.ack_sequence, 96);
        assert_eq!(refreshed.server_peer_ack_sequence, 101);
        assert_eq!(refreshed.packet, retransmit);

        let mut conflicting = retransmit.clone();
        conflicting[LEGACY_GAMEPLAY_PAYLOAD_OFFSET] ^= 0x80;
        assert!(encode_legacy_m_crc(&mut conflicting));
        let conflicting_view = MFrameView::parse(&conflicting).expect("valid conflicting frame");
        assert!(matches!(
            continue_server_deflated_reassembly(
                &conflicting,
                &conflicting_view,
                &mut state,
                102,
                0,
            )
            .expect("conflicting pending duplicate should fail closed"),
            Emit::Drop
        ));
        let retained = state
            .deflate
            .server_reassembly
            .as_ref()
            .and_then(|reassembly| reassembly.frames.iter().find(|frame| frame.sequence == 123))
            .expect("conflict must retain the first accepted pending member");
        assert_eq!(retained.ack_sequence, 96);
        assert_eq!(retained.server_peer_ack_sequence, 101);
        assert_eq!(retained.packet, retransmit);

        let mut interleaved = full.frames[1].packet.clone();
        assert!(write_be_u16(&mut interleaved, 3, 130));
        assert!(write_be_u16(&mut interleaved, 5, 97));
        assert!(encode_legacy_m_crc(&mut interleaved));
        let interleaved_view = MFrameView::parse(&interleaved).expect("valid interleaved frame");
        assert!(matches!(
            continue_server_deflated_reassembly(
                &interleaved,
                &interleaved_view,
                &mut state,
                103,
                0,
            )
            .expect("first interleaved event should buffer"),
            Emit::Consumed
        ));

        let mut interleaved_retransmit = interleaved.clone();
        assert!(write_be_u16(&mut interleaved_retransmit, 5, 98));
        assert!(encode_legacy_m_crc(&mut interleaved_retransmit));
        let interleaved_retransmit_view =
            MFrameView::parse(&interleaved_retransmit).expect("valid interleaved retransmit");
        assert!(matches!(
            continue_server_deflated_reassembly(
                &interleaved_retransmit,
                &interleaved_retransmit_view,
                &mut state,
                104,
                0,
            )
            .expect("exact interleaved retransmit should refresh"),
            Emit::Consumed
        ));

        let mut interleaved_conflict = interleaved_retransmit.clone();
        interleaved_conflict[LEGACY_GAMEPLAY_PAYLOAD_OFFSET] ^= 0x40;
        assert!(encode_legacy_m_crc(&mut interleaved_conflict));
        let interleaved_conflict_view =
            MFrameView::parse(&interleaved_conflict).expect("valid interleaved conflict");
        assert!(matches!(
            continue_server_deflated_reassembly(
                &interleaved_conflict,
                &interleaved_conflict_view,
                &mut state,
                105,
                0,
            )
            .expect("conflicting interleaved retransmit should fail closed"),
            Emit::Drop
        ));
        let retained_interleaved = state
            .deflate
            .server_reassembly
            .as_ref()
            .and_then(|reassembly| reassembly.interleaved_events.first())
            .expect("interleaved conflict must retain the first accepted event");
        assert_eq!(retained_interleaved.packet, interleaved_retransmit);
        assert_eq!(retained_interleaved.server_peer_ack_sequence, 104);
        assert!(state.deflate.server_zlib_inflater.is_none());
    }

    #[test]
    fn splits_grown_one_frame_rewrite_into_safe_reliable_frames() {
        let reassembly = make_reassembly(5, &[EE_SAFE_M_FRAME_PAYLOAD_BYTES]);
        let combined = vec![0xA5; EE_SAFE_M_FRAME_PAYLOAD_BYTES + 100];

        let outputs =
            build_server_deflated_output_frames(&reassembly, &combined, 0x01, true).unwrap();

        assert_eq!(outputs.len(), 2);
        for output in &outputs {
            assert!(output.len() <= EE_SAFE_M_FRAME_DATAGRAM_BYTES);
            let view = MFrameView::parse(output).unwrap();
            assert!(view.crc_valid);
        }

        let first = MFrameView::parse(&outputs[0]).unwrap();
        let second = MFrameView::parse(&outputs[1]).unwrap();
        assert_eq!(first.sequence, 5);
        assert_eq!(first.packetized_sequence, 2);
        assert_eq!(first.declared_payload_length, EE_SAFE_M_FRAME_PAYLOAD_BYTES);
        assert_eq!(second.sequence, 6);
        assert_eq!(second.packetized_sequence, 0);
        assert_eq!(second.declared_payload_length, 100);
        assert_eq!(second.frame_type, 0);
        assert_eq!(second.flags & 0x40, 0x40);
        assert_eq!(second.flags & 0x04, 0);
    }

    #[test]
    fn keeps_small_growth_in_original_reliable_frame_when_ee_safe() {
        let reassembly = make_reassembly(45, &[251]);
        let combined = vec![0xA5; 294];

        let outputs =
            build_server_deflated_output_frames(&reassembly, &combined, 0x01, true).unwrap();

        assert_eq!(outputs.len(), 1);
        let first = MFrameView::parse(&outputs[0]).unwrap();
        assert!(first.crc_valid);
        assert_eq!(first.sequence, 45);
        assert_eq!(first.packetized_sequence, 1);
        assert_eq!(first.declared_payload_length, 294);
        assert!(outputs[0].len() <= EE_SAFE_M_FRAME_DATAGRAM_BYTES);
    }

    #[test]
    fn keeps_expanded_area_stream_in_original_frame_count_when_ee_safe() {
        let reassembly = make_reassembly(37, &[960, 960, 960, 960, 323]);
        let combined = vec![0x5A; 4167];

        let outputs =
            build_server_deflated_output_frames(&reassembly, &combined, 0x01, true).unwrap();

        assert_eq!(outputs.len(), 5);
        let sequences = outputs
            .iter()
            .map(|packet| MFrameView::parse(packet).unwrap().sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![37, 38, 39, 40, 41]);
        assert_eq!(
            MFrameView::parse(&outputs[0]).unwrap().packetized_sequence,
            5
        );
        assert!(
            outputs
                .iter()
                .all(|packet| packet.len() <= EE_SAFE_M_FRAME_DATAGRAM_BYTES)
        );
        assert!(
            outputs
                .iter()
                .all(|packet| MFrameView::parse(packet).unwrap().crc_valid)
        );
    }

    #[test]
    fn preserves_original_reliable_window_count_when_payload_shrinks() {
        let reassembly = make_reassembly(2, &[500, 500, 500]);
        let combined = vec![0x5A; 700];

        let outputs =
            build_server_deflated_output_frames(&reassembly, &combined, 0x01, true).unwrap();

        assert_eq!(outputs.len(), 3);
        let sequences = outputs
            .iter()
            .map(|packet| MFrameView::parse(packet).unwrap().sequence)
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![2, 3, 4]);
        assert_eq!(
            MFrameView::parse(&outputs[0]).unwrap().packetized_sequence,
            3
        );
        assert!(
            outputs
                .iter()
                .all(|packet| MFrameView::parse(packet).unwrap().crc_valid)
        );
    }

    #[test]
    fn builds_raw_gameplay_replacement_by_clearing_deflate_and_stream_flags() {
        let mut reassembly = make_reassembly(34, &[20]);
        reassembly.frames[0].packet[7] = 0x0F;
        encode_legacy_m_crc(&mut reassembly.frames[0].packet);

        let mut quickbar = vec![b'P', 0x1E, 0x01];
        quickbar.extend_from_slice(&43u32.to_le_bytes());
        quickbar.extend(std::iter::repeat(0).take(36));
        quickbar.push(0);

        let outputs = build_server_raw_gameplay_output_frames(&reassembly, &quickbar).unwrap();

        assert_eq!(outputs.len(), 1);
        let view = MFrameView::parse(&outputs[0]).unwrap();
        assert!(view.crc_valid);
        assert_eq!(view.sequence, 34);
        assert_eq!(view.packetized_sequence, 1);
        assert_eq!(view.declared_payload_length, quickbar.len());
        assert_eq!(view.flags & 0x05, 0);
        assert_eq!(view.flags & 0x0A, 0x0A);
        assert_eq!(
            &outputs[0][LEGACY_GAMEPLAY_PAYLOAD_OFFSET..],
            quickbar.as_slice()
        );
    }

    #[test]
    fn completed_stream_cache_requires_exact_bytes_and_reliable_generation() {
        let mut state = SessionState::default();
        let mut reassembly = make_reassembly(91, &[4, 3]);
        reassembly.server_origin_generation = 7;
        reassembly.inflated_length = 23;
        reassembly.frames[0].compressed_chunk = vec![0x78, 0x9C, 0x10, 0x20];
        reassembly.frames[1].compressed_chunk = vec![0x30, 0x40, 0x50];
        let compressed = [0x78, 0x9C, 0x10, 0x20, 0x30, 0x40, 0x50];
        let replay = CompletedDeflatedReplay::VerifiedPackets {
            family: VerifiedFamily::LoadBar,
            packets: build_consumed_server_deflated_frames(&reassembly)
                .expect("cache replay shells"),
        };

        remember_completed_server_stream_window_with_disposition(
            &mut state,
            &reassembly,
            compressed.len(),
            true,
            replay,
        );

        let CompletedDeflatedStreamWindowMatch::Exact(exact) =
            completed_server_stream_window(&state, &reassembly, &compressed)
        else {
            panic!("exact bytes in the same reliable generation must replay");
        };
        assert!(exact.pre_shifted);
        assert_eq!(exact.server_origin_generation, 7);
        assert_eq!(exact.compressed, compressed);

        let mut conflicting = compressed;
        conflicting[4] ^= 0x80;
        assert!(matches!(
            completed_server_stream_window(&state, &reassembly, &conflicting),
            CompletedDeflatedStreamWindowMatch::Conflict
        ));

        let mut reshaped = make_reassembly(91, &[3, 4]);
        reshaped.server_origin_generation = 7;
        reshaped.inflated_length = 23;
        reshaped.frames[0].compressed_chunk = compressed[..3].to_vec();
        reshaped.frames[1].compressed_chunk = compressed[3..].to_vec();
        assert!(matches!(
            completed_server_stream_window(&state, &reshaped, &compressed),
            CompletedDeflatedStreamWindowMatch::Conflict
        ));

        reassembly.server_origin_generation = 8;
        assert!(matches!(
            completed_server_stream_window(&state, &reassembly, &compressed),
            CompletedDeflatedStreamWindowMatch::Miss
        ));
    }

    #[test]
    fn completed_reliable_stream_slot_pins_identity_and_route_per_generation() {
        let mut state = SessionState::default();
        let packet = make_reassembly(44, &[5]).frames.remove(0).packet;

        remember_completed_server_reliable_stream_slot(
            &mut state,
            44,
            7,
            &packet,
            CompletedServerReliableStreamRoute::StreamWindow,
        );
        assert_eq!(
            completed_server_reliable_stream_slot(&state, 44, 7, &packet),
            CompletedServerReliableStreamSlotMatch::Exact(
                CompletedServerReliableStreamRoute::StreamWindow
            )
        );

        let mut conflicting_tail = packet.clone();
        conflicting_tail.push(0xA5);
        assert_eq!(
            completed_server_reliable_stream_slot(&state, 44, 7, &conflicting_tail),
            CompletedServerReliableStreamSlotMatch::Conflict(
                CompletedServerReliableStreamRoute::StreamWindow
            )
        );
        assert_eq!(
            completed_server_reliable_stream_slot(&state, 44, 8, &conflicting_tail),
            CompletedServerReliableStreamSlotMatch::Miss
        );

        let mut coalesced_first = SessionState::default();
        let mut coalesced_packet = packet.clone();
        let span_offset = coalesced_packet.len();
        coalesced_packet.extend_from_slice(&[0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET]);
        coalesced_packet[span_offset + 7] = 0x0A;
        assert!(write_be_u16(&mut coalesced_packet, span_offset + 8, 1));
        assert!(write_be_u16(&mut coalesced_packet, span_offset + 10, 3));
        coalesced_packet.extend_from_slice(&[b'P', 0x09, 0x05]);
        assert!(encode_legacy_m_crc(&mut coalesced_packet));
        remember_completed_server_reliable_stream_slot(
            &mut coalesced_first,
            44,
            7,
            &coalesced_packet,
            CompletedServerReliableStreamRoute::CoalescedWindow,
        );
        let mut refreshed_embedded_transport = coalesced_packet;
        assert!(write_be_u16(
            &mut refreshed_embedded_transport,
            span_offset + 3,
            45
        ));
        assert!(write_be_u16(
            &mut refreshed_embedded_transport,
            span_offset + 5,
            99
        ));
        assert!(write_be_u16(
            &mut refreshed_embedded_transport,
            span_offset + 8,
            2
        ));
        assert!(encode_legacy_m_crc(&mut refreshed_embedded_transport));
        assert_eq!(
            completed_server_reliable_stream_slot(
                &coalesced_first,
                44,
                7,
                &refreshed_embedded_transport,
            ),
            CompletedServerReliableStreamSlotMatch::Exact(
                CompletedServerReliableStreamRoute::CoalescedWindow
            ),
            "queued-record storage transport fields may refresh on retransmit"
        );
        assert_eq!(
            completed_server_reliable_stream_slot(&coalesced_first, 44, 7, &packet),
            CompletedServerReliableStreamSlotMatch::Conflict(
                CompletedServerReliableStreamRoute::CoalescedWindow
            )
        );
    }

    #[test]
    fn completed_stream_replay_refreshes_each_current_ack_and_crc() {
        let mut reassembly = make_reassembly(120, &[4, 3]);
        reassembly.frames[0].ack_sequence = 81;
        reassembly.frames[1].ack_sequence = 82;
        let mut packets = build_consumed_server_deflated_frames(&reassembly)
            .expect("current replay packet templates");
        let mut spill = packets[1].clone();
        assert!(write_be_u16(&mut spill, 3, 122));
        assert!(write_be_u16(&mut spill, 5, 70));
        assert!(encode_legacy_m_crc(&mut spill));
        packets.push(spill);
        for packet in &mut packets[..2] {
            assert!(write_be_u16(packet, 5, 70));
            assert!(encode_legacy_m_crc(packet));
        }
        let mut replay = CompletedDeflatedReplay::VerifiedProofPackets {
            proof: VerifiedProof::family(VerifiedFamily::LoadBar),
            packets,
        };

        retarget_completed_server_stream_replay(&mut replay, &reassembly)
            .expect("ACK retargeting should succeed");

        let CompletedDeflatedReplay::VerifiedProofPackets { packets, .. } = replay else {
            panic!("replay variant must remain stable");
        };
        let views = packets
            .iter()
            .map(|packet| MFrameView::parse(packet).expect("retargeted packet should parse"))
            .collect::<Vec<_>>();
        assert_eq!(
            views
                .iter()
                .map(|view| view.ack_sequence)
                .collect::<Vec<_>>(),
            vec![81, 82, 82]
        );
        assert!(views.iter().all(|view| view.crc_valid));
    }

    #[test]
    fn stream_bit_prefers_persistent_raw_deflate_contract() {
        fn raw_sync_deflate(bytes: &[u8]) -> Vec<u8> {
            let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
            let mut out = vec![0; bytes.len() + 64];
            compressor
                .compress(bytes, &mut out, flate2::FlushCompress::Sync)
                .expect("raw sync deflate should succeed");
            out.truncate(compressor.total_out() as usize);
            out
        }

        let first = b"P\x03\x03\x07\x00\x00\x00";
        let second = b"P\x04\x01\x17\x00\x00\x00edmonton\x00\x00\x00\x00\x00\x00\x00\x00";
        let mut server_stream = None;

        let first_inflated = inflate_gameplay_payload(
            &raw_sync_deflate(first),
            first.len(),
            true,
            &mut server_stream,
        )
        .expect("first stream-bit raw-deflate record should inflate");
        assert_eq!(first_inflated.bytes, first);
        assert!(first_inflated.used_server_stream);
        assert!(server_stream.is_some());

        let second_inflated = inflate_gameplay_payload(
            &raw_sync_deflate(second),
            second.len(),
            true,
            &mut server_stream,
        )
        .expect("second stream-bit raw-deflate record should use persistent history");
        assert_eq!(second_inflated.bytes, second);
        assert!(second_inflated.used_server_stream);
        assert!(server_stream.is_some());
    }
}

pub(super) fn inflate_gameplay_payload(
    compressed: &[u8],
    inflated_length: usize,
    zlib_stream: bool,
    server_stream: &mut Option<Decompress>,
) -> anyhow::Result<InflatedGameplayPayload> {
    if inflated_length > MAX_REASONABLE_GAMEPLAY_PAYLOAD {
        anyhow::bail!("inflated gameplay length is unreasonable: {inflated_length}");
    }

    if zlib_stream {
        // Diamond's M-frame stream bit maps to a persistent raw-deflate reader
        // in the client transport layer. The first record can carry a zlib
        // header, while later coalesced records continue the same stream
        // without another header. A self-contained inflate can succeed for the
        // first record, but accepting that one-shot candidate discards the
        // inflater history needed by the next semantic record. Prefer the
        // decompile-owned stream contract when the stream bit is present;
        // replay caches above this helper prevent retransmitted records from
        // advancing the inflater twice.
        let zlib_header = looks_like_zlib_wrapped_deflate(compressed);
        match inflate_with_server_stream(compressed, inflated_length, zlib_header, server_stream)? {
            Some(bytes) => {
                return Ok(InflatedGameplayPayload {
                    bytes,
                    used_server_stream: true,
                });
            }
            None => {
                *server_stream = None;
            }
        }
    }
    if let Some(inflated) =
        inflate_with_window(compressed, inflated_length, false, FlushDecompress::Sync)?
    {
        return Ok(InflatedGameplayPayload {
            bytes: inflated,
            used_server_stream: false,
        });
    }
    if let Some(inflated) =
        inflate_with_window(compressed, inflated_length, true, FlushDecompress::Finish)?
    {
        return Ok(InflatedGameplayPayload {
            bytes: inflated,
            used_server_stream: false,
        });
    }
    if let Some(inflated) =
        inflate_with_window(compressed, inflated_length, true, FlushDecompress::Sync)?
    {
        return Ok(InflatedGameplayPayload {
            bytes: inflated,
            used_server_stream: false,
        });
    }

    anyhow::bail!(
        "failed to inflate server gameplay payload: compressed={} inflated={}",
        compressed.len(),
        inflated_length
    )
}

pub(super) fn completed_server_stream_window(
    state: &SessionState,
    reassembly: &ServerDeflatedReassembly,
    compressed: &[u8],
) -> CompletedDeflatedStreamWindowMatch {
    let frame_transport_identities = completed_stream_frame_transport_identities(reassembly);
    let exact = state
        .deflate
        .completed_server_stream_windows
        .iter()
        .find(|window| {
            window.first_sequence == reassembly.first_sequence
                && window.server_origin_generation == reassembly.server_origin_generation
                && window.expected_frames == reassembly.expected_frames
                && window.packetized_sequence == reassembly.packetized_sequence
                && window.inflated_length == reassembly.inflated_length
                && window.compressed.as_slice() == compressed
                && frame_transport_identities
                    .as_ref()
                    .is_some_and(|identities| {
                        window.frame_transport_identities.as_slice() == identities.as_slice()
                    })
        });
    if let Some(window) = exact {
        return CompletedDeflatedStreamWindowMatch::Exact(window.clone());
    }

    if state
        .deflate
        .completed_server_stream_windows
        .iter()
        .any(|window| {
            window.first_sequence == reassembly.first_sequence
                && window.server_origin_generation == reassembly.server_origin_generation
        })
    {
        CompletedDeflatedStreamWindowMatch::Conflict
    } else {
        CompletedDeflatedStreamWindowMatch::Miss
    }
}

pub(super) fn completed_server_reliable_stream_slot(
    state: &SessionState,
    sequence: u16,
    server_origin_generation: u64,
    packet: &[u8],
) -> CompletedServerReliableStreamSlotMatch {
    let Some(transport_identity) = completed_reliable_stream_transport_identity(packet) else {
        return CompletedServerReliableStreamSlotMatch::Miss;
    };
    let Some(slot) = state
        .deflate
        .completed_server_reliable_stream_slots
        .iter()
        .find(|slot| {
            slot.sequence == sequence && slot.server_origin_generation == server_origin_generation
        })
    else {
        return CompletedServerReliableStreamSlotMatch::Miss;
    };
    if slot.transport_identity == transport_identity {
        CompletedServerReliableStreamSlotMatch::Exact(slot.route)
    } else {
        CompletedServerReliableStreamSlotMatch::Conflict(slot.route)
    }
}

pub(super) fn remember_completed_server_reliable_stream_slot(
    state: &mut SessionState,
    sequence: u16,
    server_origin_generation: u64,
    packet: &[u8],
    route: CompletedServerReliableStreamRoute,
) {
    let Some(transport_identity) = completed_reliable_stream_transport_identity(packet) else {
        return;
    };
    // Cache families have independent record limits. Remove ledger-only stale
    // entries before applying the aggregate cap so every disposition that can
    // still replay remains protected from an ordinary/coalesced route switch.
    let protected_stream_slots = state
        .deflate
        .completed_server_stream_windows
        .iter()
        .map(|window| (window.first_sequence, window.server_origin_generation))
        .chain(
            state
                .coalesced_replay
                .completed_deflated_records
                .iter()
                .map(|entry| (entry.sequence, entry.server_origin_generation)),
        )
        .chain(
            state
                .coalesced_replay
                .completed_direct_records
                .iter()
                .map(|entry| (entry.sequence, entry.server_origin_generation)),
        )
        .collect::<std::collections::HashSet<_>>();
    state
        .deflate
        .completed_server_reliable_stream_slots
        .retain(|slot| {
            protected_stream_slots.contains(&(slot.sequence, slot.server_origin_generation))
        });
    if let Some(slot) = state
        .deflate
        .completed_server_reliable_stream_slots
        .iter_mut()
        .find(|slot| {
            slot.sequence == sequence && slot.server_origin_generation == server_origin_generation
        })
    {
        if slot.transport_identity == transport_identity && slot.route == route {
            return;
        }
        tracing::warn!(
            sequence,
            server_origin_generation,
            existing_route = slot.route.as_str(),
            attempted_route = route.as_str(),
            "conflicting completed reliable stream slot was not allowed to replace its first accepted identity"
        );
        return;
    }

    state
        .deflate
        .completed_server_reliable_stream_slots
        .push(CompletedServerReliableStreamSlot {
            sequence,
            server_origin_generation,
            transport_identity,
            route,
        });
    // At most 16 ordinary windows, 64 coalesced deflated records, and 128
    // coalesced direct records can remain cached. One outer slot can own
    // several records, so 208 bounds protected identities; one additional
    // slot retains the current fail-closed route even when it produced no
    // replay-cache entry.
    const MAX_COMPLETED_SERVER_RELIABLE_STREAM_SLOTS: usize = 209;
    if state.deflate.completed_server_reliable_stream_slots.len()
        > MAX_COMPLETED_SERVER_RELIABLE_STREAM_SLOTS
    {
        let overflow = state.deflate.completed_server_reliable_stream_slots.len()
            - MAX_COMPLETED_SERVER_RELIABLE_STREAM_SLOTS;
        state
            .deflate
            .completed_server_reliable_stream_slots
            .drain(0..overflow);
    }
}

fn completed_reliable_stream_transport_identity(packet: &[u8]) -> Option<Vec<u8>> {
    let mut identity = packet.get(7..)?.to_vec();
    let view = MFrameView::parse(packet)?;
    if view.trailing_payload_length == 0 {
        return Some(identity);
    }

    let primary_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET.checked_add(view.payload_length)?;
    let Some(spans) = parse_packetized_spans(packet, primary_end) else {
        // A committed coalesced route is always parseable. Keeping malformed
        // incoming tail bytes exact makes them conflict with that disposition.
        return Some(identity);
    };
    for span in spans {
        // Queued records inherit the primary window when these storage-header
        // fields are zero, and retransmission may refresh them. They are not
        // semantic or route identity. Preserve flags and payload/length bytes.
        for absolute in (span.offset + 3..span.offset + 7).chain(span.offset + 8..span.offset + 10)
        {
            if let Some(byte) = absolute
                .checked_sub(7)
                .and_then(|offset| identity.get_mut(offset))
            {
                *byte = 0;
            }
        }
    }
    Some(identity)
}

pub(super) fn remember_completed_server_stream_window(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    compressed_length: usize,
    replay: CompletedDeflatedReplay,
) {
    remember_completed_server_stream_window_with_disposition(
        state,
        reassembly,
        compressed_length,
        false,
        replay,
    );
}

pub(super) fn remember_completed_server_stream_window_with_disposition(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    compressed_length: usize,
    pre_shifted: bool,
    replay: CompletedDeflatedReplay,
) {
    let compressed = reassembly
        .frames
        .iter()
        .flat_map(|frame| frame.compressed_chunk.iter().copied())
        .collect::<Vec<_>>();
    if compressed.len() != compressed_length {
        tracing::warn!(
            first_sequence = reassembly.first_sequence,
            server_origin_generation = reassembly.server_origin_generation,
            expected_compressed_length = compressed_length,
            actual_compressed_length = compressed.len(),
            "completed server stream replay not cached because source identity length changed"
        );
        return;
    }
    let Some(frame_transport_identities) = completed_stream_frame_transport_identities(reassembly)
    else {
        tracing::warn!(
            first_sequence = reassembly.first_sequence,
            server_origin_generation = reassembly.server_origin_generation,
            "completed server stream replay not cached because a source frame identity was truncated"
        );
        return;
    };

    if let Some(window) = state
        .deflate
        .completed_server_stream_windows
        .iter_mut()
        .find(|window| {
            window.first_sequence == reassembly.first_sequence
                && window.server_origin_generation == reassembly.server_origin_generation
                && window.expected_frames == reassembly.expected_frames
                && window.packetized_sequence == reassembly.packetized_sequence
                && window.inflated_length == reassembly.inflated_length
                && window.compressed == compressed
                && window.frame_transport_identities.as_slice()
                    == frame_transport_identities.as_slice()
        })
    {
        window.replay = replay;
        window.pre_shifted = pre_shifted;
        remember_completed_server_stream_reassembly_slot(state, reassembly);
        return;
    }

    if state
        .deflate
        .completed_server_stream_windows
        .iter()
        .any(|window| {
            window.first_sequence == reassembly.first_sequence
                && window.server_origin_generation == reassembly.server_origin_generation
        })
    {
        tracing::warn!(
            first_sequence = reassembly.first_sequence,
            server_origin_generation = reassembly.server_origin_generation,
            expected_frames = reassembly.expected_frames,
            packetized_sequence = reassembly.packetized_sequence,
            inflated_length = reassembly.inflated_length,
            compressed_length,
            "conflicting completed server stream disposition was not allowed to replace exact cache identity"
        );
        return;
    }

    const MAX_COMPLETED_STREAM_WINDOWS: usize = 16;
    state
        .deflate
        .completed_server_stream_windows
        .push(CompletedDeflatedStreamWindow {
            first_sequence: reassembly.first_sequence,
            server_origin_generation: reassembly.server_origin_generation,
            expected_frames: reassembly.expected_frames,
            packetized_sequence: reassembly.packetized_sequence,
            inflated_length: reassembly.inflated_length,
            compressed,
            frame_transport_identities,
            pre_shifted,
            replay,
        });
    if state.deflate.completed_server_stream_windows.len() > MAX_COMPLETED_STREAM_WINDOWS {
        let overflow =
            state.deflate.completed_server_stream_windows.len() - MAX_COMPLETED_STREAM_WINDOWS;
        state
            .deflate
            .completed_server_stream_windows
            .drain(0..overflow);
    }
    remember_completed_server_stream_reassembly_slot(state, reassembly);
}

fn remember_completed_server_stream_reassembly_slot(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
) {
    let Some(first_frame) = reassembly.frames.first() else {
        return;
    };
    remember_completed_server_reliable_stream_slot(
        state,
        reassembly.first_sequence,
        reassembly.server_origin_generation,
        &first_frame.packet,
        CompletedServerReliableStreamRoute::StreamWindow,
    );
}

pub(super) fn retarget_completed_server_stream_replay(
    replay: &mut CompletedDeflatedReplay,
    reassembly: &ServerDeflatedReassembly,
) -> anyhow::Result<()> {
    let packets = match replay {
        CompletedDeflatedReplay::Packets(packets)
        | CompletedDeflatedReplay::VerifiedPackets { packets, .. }
        | CompletedDeflatedReplay::VerifiedProofPackets { packets, .. } => packets,
    };
    let Some(last_source_frame) = reassembly.frames.last() else {
        anyhow::bail!("completed server stream replay has no current source frame");
    };
    for (index, packet) in packets.iter_mut().enumerate() {
        let source_frame = reassembly.frames.get(index).unwrap_or(last_source_frame);
        write_be_u16(packet, 5, source_frame.ack_sequence)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to retarget completed stream replay ACK"))?;
        encode_legacy_m_crc(packet)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair completed stream replay CRC"))?;
    }
    Ok(())
}
