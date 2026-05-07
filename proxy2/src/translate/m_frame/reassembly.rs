//! Deflated reliable-window reassembly data types.
//!
//! This module owns deflated-window collection, duplicate replay, safe
//! consumed-frame emission, and reconstruction of repaired deflated output
//! frames. Semantic packet translation remains outside this module.

use flate2::{Decompress, FlushDecompress};

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{MFrameView, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MAX_REASONABLE_GAMEPLAY_PAYLOAD},
    translate::Emit,
};

use super::{
    deflate::{inflate_with_server_stream, inflate_with_window, looks_like_zlib_wrapped_deflate},
    server_dispatch, transport_identity, SessionState, MAX_INTERLEAVED_PACKETS,
    MAX_REASSEMBLY_FRAMES,
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
    VerifiedPackets(Vec<Vec<u8>>),
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
    state.server_deflated = Some(reassembly);

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
        Ok(Emit::Consumed)
    }
}

pub(super) fn continue_server_deflated_reassembly(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
) -> anyhow::Result<Emit> {
    let Some(snapshot) = state.server_deflated.as_ref() else {
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
        let Some(reassembly) = state.server_deflated.as_mut() else {
            return Ok(Emit::Drop);
        };
        if reassembly.interleaved_packets.len() >= MAX_INTERLEAVED_PACKETS {
            tracing::warn!(
                sequence = view.sequence,
                first_sequence = reassembly.first_sequence,
                expected_frames = reassembly.expected_frames,
                "server deflated M reassembly abandoned after too many interleaved packets"
            );
            state.server_deflated = None;
            return Ok(Emit::Drop);
        }
        reassembly.interleaved_packets.push(interleaved_packet);
        return Ok(Emit::Consumed);
    }

    let Some(reassembly) = state.server_deflated.as_mut() else {
        return Ok(Emit::Drop);
    };

    if reassembly
        .frames
        .iter()
        .any(|frame| frame.sequence == view.sequence)
    {
        tracing::warn!(
            sequence = view.sequence,
            first_sequence = reassembly.first_sequence,
            "duplicate server deflated M frame dropped"
        );
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
        return Ok(Emit::Consumed);
    }

    super::emit_completed_server_deflated_reassembly(state)
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
        return Ok(rewritten);
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
        out_packet[7] &= !0x07;
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
        let chunk_length = if final_frame {
            remaining
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
            // acknowledge progress, but clear stream, packetized, and deflate
            // delivery bits before zeroing packetized count/length. Leaving
            // bit 0x02 set on an empty shell advertises a packetized payload
            // that no longer exists and can stall reliable-window progress.
            out_packet[7] &= !0x07;
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
    state.completed_server_stream_windows.iter().find(|window| {
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
    if completed_server_stream_window(state, reassembly, compressed_length).is_some() {
        return;
    }

    const MAX_COMPLETED_STREAM_WINDOWS: usize = 16;
    state.completed_server_stream_windows.push(CompletedDeflatedStreamWindow {
        first_sequence: reassembly.first_sequence,
        expected_frames: reassembly.expected_frames,
        packetized_sequence: reassembly.packetized_sequence,
        inflated_length: reassembly.inflated_length,
        compressed_length,
        replay,
    });
    if state.completed_server_stream_windows.len() > MAX_COMPLETED_STREAM_WINDOWS {
        let overflow = state.completed_server_stream_windows.len() - MAX_COMPLETED_STREAM_WINDOWS;
        state.completed_server_stream_windows.drain(0..overflow);
    }
}


