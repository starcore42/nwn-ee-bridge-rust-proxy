//! Deflated reliable-window reassembly data types.
//!
//! This module owns deflated-window collection, duplicate replay, safe
//! consumed-frame emission, and reconstruction of repaired deflated output
//! frames. Semantic packet translation remains outside this module.

use flate2::{Decompress, FlushDecompress};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MAX_REASONABLE_GAMEPLAY_PAYLOAD, MFrameView},
    translate::{Emit, VerifiedFamily, VerifiedProof},
};

use super::{
    MAX_INTERLEAVED_PACKETS, MAX_REASSEMBLY_FRAMES, SessionState,
    deflate::{inflate_with_server_stream, inflate_with_window, looks_like_zlib_wrapped_deflate},
    server_dispatch, transport_identity,
};

#[derive(Debug, Clone)]
pub(super) struct ServerDeflatedReassembly {
    pub(super) inflated_length: usize,
    pub(super) expected_frames: usize,
    pub(super) first_sequence: u16,
    pub(super) packetized_sequence: u16,
    pub(super) zlib_stream: bool,
    pub(super) frames: Vec<BufferedFrame>,
    pub(super) interleaved_packets: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(super) struct BufferedFrame {
    pub(super) packet: Vec<u8>,
    pub(super) payload_length: usize,
    pub(super) sequence: u16,
    pub(super) ack_sequence: u16,
    pub(super) compressed_chunk: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct CompletedDeflatedStreamWindow {
    pub(super) first_sequence: u16,
    pub(super) expected_frames: usize,
    pub(super) packetized_sequence: u16,
    pub(super) inflated_length: usize,
    pub(super) compressed_length: usize,
    pub(super) replay: CompletedDeflatedReplay,
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

#[derive(Debug, Clone)]
pub(super) struct InflatedGameplayPayload {
    pub(super) bytes: Vec<u8>,
    pub(super) used_server_stream: bool,
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

    let frame = buffered_frame_from_view(bytes, view, true)?;
    let mut reassembly = ServerDeflatedReassembly {
        inflated_length: deflated.inflated_length,
        expected_frames,
        first_sequence: view.sequence,
        packetized_sequence: view.packetized_sequence,
        zlib_stream: (view.flags & 0x01) != 0,
        frames: Vec::with_capacity(expected_frames),
        interleaved_packets: Vec::new(),
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

    let first_sequence = snapshot.first_sequence;
    let expected_frames = snapshot.expected_frames;
    let distance = view.sequence.wrapping_sub(first_sequence) as usize;
    if distance >= expected_frames {
        let interleaved_packet = claim_or_consume_interleaved_server_packet(bytes, view, state)?;
        let Some(reassembly) = state.deflate.server_reassembly.as_mut() else {
            return Ok(Emit::Drop);
        };
        if reassembly.interleaved_packets.len() >= MAX_INTERLEAVED_PACKETS {
            tracing::warn!(
                sequence = view.sequence,
                first_sequence = reassembly.first_sequence,
                expected_frames = reassembly.expected_frames,
                "server deflated M reassembly abandoned after too many interleaved packets"
            );
            state.deflate.server_reassembly = None;
            return Ok(Emit::Drop);
        }
        reassembly.interleaved_packets.push(interleaved_packet);
        return Ok(Emit::Consumed);
    }

    let Some(reassembly) = state.deflate.server_reassembly.as_mut() else {
        return Ok(Emit::Drop);
    };

    if reassembly
        .frames
        .iter()
        .any(|frame| frame.sequence == view.sequence)
    {
        let buffered_frames = reassembly.frames.len();
        let expected_frames = reassembly.expected_frames;
        let first_sequence = reassembly.first_sequence;
        tracing::warn!(
            sequence = view.sequence,
            first_sequence,
            buffered_frames,
            expected_frames,
            "duplicate server deflated M frame dropped"
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

    let frame = buffered_frame_from_view(bytes, view, false)?;
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

fn claim_or_consume_interleaved_server_packet(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
) -> anyhow::Result<Vec<u8>> {
    if let Some(rewritten) =
        server_dispatch::rewrite_direct_frame_if_needed(bytes, view, &state.module_resources)?
    {
        tracing::info!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            flags = view.flags,
            packetized_sequence = view.packetized_sequence,
            payload_len = view.payload_length,
            "interleaved server M packet semantically claimed while deflated reassembly is pending"
        );
        return Ok(rewritten.packet);
    }

    if let Some(summary) = transport_identity::claim_server_frame_if_verified(view) {
        tracing::info!(
            packet = summary.packet_name,
            reason = summary.reason,
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            flags = view.flags,
            packetized_sequence = view.packetized_sequence,
            payload_len = view.payload_length,
            "interleaved server M transport-only packet claimed as verified no-op"
        );
        return Ok(bytes.to_vec());
    }

    tracing::warn!(
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        flags = view.flags,
        packetized_sequence = view.packetized_sequence,
        payload_len = view.payload_length,
        "interleaved server M packet consumed: no semantic translator or transport identity owner"
    );
    consume_interleaved_unclaimed_server_packet(bytes)
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
        ack_sequence: view.ack_sequence,
        compressed_chunk: bytes[compressed_start..payload_end].to_vec(),
    })
}

pub(super) fn build_server_deflated_output_frames(
    reassembly: &ServerDeflatedReassembly,
    combined_payload: &[u8],
    clear_first_frame_flags: u8,
    set_first_packetized_sequence_to_output_count: bool,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut outputs = Vec::with_capacity(reassembly.frames.len());
    let mut cursor = 0;

    for (index, frame) in reassembly.frames.iter().enumerate() {
        if cursor > combined_payload.len() {
            anyhow::bail!("deflated output cursor exceeded combined payload");
        }

        let final_frame = index + 1 == reassembly.frames.len();
        let remaining = combined_payload.len() - cursor;
        let frames_left = reassembly.frames.len() - index;
        let minimum_reserved_for_later = frames_left.saturating_sub(1);
        let max_this_frame = remaining.saturating_sub(minimum_reserved_for_later);
        let chunk_length = if final_frame {
            remaining
        } else if remaining >= frames_left {
            frame.payload_length.min(max_this_frame).max(1)
        } else {
            frame.payload_length.min(remaining)
        };
        if chunk_length > u16::MAX as usize {
            anyhow::bail!("deflated output chunk too large for legacy packetized length");
        }

        let mut out_packet = frame.packet.clone();
        out_packet.resize(LEGACY_GAMEPLAY_PAYLOAD_OFFSET + chunk_length, 0);
        if chunk_length != 0 {
            out_packet
                [LEGACY_GAMEPLAY_PAYLOAD_OFFSET..LEGACY_GAMEPLAY_PAYLOAD_OFFSET + chunk_length]
                .copy_from_slice(&combined_payload[cursor..cursor + chunk_length]);
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

    Ok(outputs)
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

pub(super) fn inflate_gameplay_payload(
    compressed: &[u8],
    inflated_length: usize,
    zlib_stream: bool,
    server_stream: &mut Option<Decompress>,
) -> anyhow::Result<InflatedGameplayPayload> {
    if inflated_length > MAX_REASONABLE_GAMEPLAY_PAYLOAD {
        anyhow::bail!("inflated gameplay length is unreasonable: {inflated_length}");
    }

    if zlib_stream && !looks_like_zlib_wrapped_deflate(compressed) {
        match inflate_with_server_stream(compressed, inflated_length, server_stream)? {
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

pub(super) fn completed_server_stream_window<'a>(
    state: &'a SessionState,
    reassembly: &ServerDeflatedReassembly,
    compressed_length: usize,
) -> Option<&'a CompletedDeflatedStreamWindow> {
    state
        .deflate
        .completed_server_stream_windows
        .iter()
        .find(|window| {
            window.first_sequence == reassembly.first_sequence
                && window.expected_frames == reassembly.expected_frames
                && window.packetized_sequence == reassembly.packetized_sequence
                && window.inflated_length == reassembly.inflated_length
                && window.compressed_length == compressed_length
        })
}

pub(super) fn remember_completed_server_stream_window(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    compressed_length: usize,
    replay: CompletedDeflatedReplay,
) {
    if let Some(window) = state
        .deflate
        .completed_server_stream_windows
        .iter_mut()
        .find(|window| {
            window.first_sequence == reassembly.first_sequence
                && window.expected_frames == reassembly.expected_frames
                && window.packetized_sequence == reassembly.packetized_sequence
                && window.inflated_length == reassembly.inflated_length
                && window.compressed_length == compressed_length
        })
    {
        window.replay = replay;
        return;
    }

    const MAX_COMPLETED_STREAM_WINDOWS: usize = 16;
    state
        .deflate
        .completed_server_stream_windows
        .push(CompletedDeflatedStreamWindow {
            first_sequence: reassembly.first_sequence,
            expected_frames: reassembly.expected_frames,
            packetized_sequence: reassembly.packetized_sequence,
            inflated_length: reassembly.inflated_length,
            compressed_length,
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
}
