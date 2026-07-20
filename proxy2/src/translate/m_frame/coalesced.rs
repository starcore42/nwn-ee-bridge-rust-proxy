//! Coalesced reliable-window span handling.
//!
//! This module owns packetized trailing-span mechanics for bundled server
//! `M` records. It may unwrap/repack a coalesced deflated span, but gameplay
//! meaning must remain delegated to the focused semantic translators.

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    crc::{encode_legacy_m_crc, read_be_u16, write_be_u16},
    packet::m::{
        DeflatedEnvelope, HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView,
        parse_packetized_spans,
    },
    translate::{ContinuationOwner, VerifiedFamily, VerifiedProof},
};

use super::{
    CNW_LENGTH_BYTES, SessionState, deferred_module_resources,
    deflate::deflate_zlib,
    hex_prefix, inflated_cnw_fragment_offset_valid, login_waypoint,
    queue_area_client_area_side_effects_for_window,
    reassembly::{
        self, BufferedFrame, EE_SAFE_M_FRAME_DATAGRAM_BYTES, InflatedGameplayPayload,
        ServerDeflatedReassembly,
    },
    sequence::{
        CoalescedSplitSequenceShift, SequenceShift, sequence_at_or_after, shift_sequence_for_peer,
        trim_coalesced_split_sequence_shifts, trim_sequence_shifts,
    },
    server_dispatch,
    state::{CompletedCoalescedDeflatedRecord, CompletedCoalescedDirectRecord},
};

pub(super) enum CoalescedRewrite {
    Single {
        proof: VerifiedProof,
        packet: Vec<u8>,
    },
    Split {
        packets: Vec<(VerifiedProof, Vec<u8>)>,
    },
    SplitPreShifted {
        packets: Vec<(VerifiedProof, Vec<u8>)>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CoalescedRecordTransportContext {
    sequence: u16,
    server_peer_ack_sequence: u16,
    client_unshifted_ack_sequence: u16,
}

fn coalesced_record_transport_context(
    record: &[u8],
    fallback_sequence: u16,
    server_peer_ack_sequence: u16,
    fallback_client_unshifted_ack_sequence: u16,
) -> CoalescedRecordTransportContext {
    // EE `CNetLayerWindow::FrameReceive` owns the primary reliable frame;
    // `UnpacketizeFullMessages` then walks queued records with the same 12-byte
    // storage header. The checked-in Diamond coalesced fixtures leave the
    // queued record sequence/ACK fields zero, so those fields inherit the
    // primary window. Keep a nonzero stored value when present, but never let
    // the zero storage sentinel erase the frame's transport provenance.
    let sequence = match read_be_u16(record, 3) {
        Some(0) | None => fallback_sequence,
        Some(sequence) => sequence,
    };
    let client_unshifted_ack_sequence = match read_be_u16(record, 5) {
        Some(0) | None => fallback_client_unshifted_ack_sequence,
        Some(ack_sequence) => ack_sequence,
    };
    CoalescedRecordTransportContext {
        sequence,
        server_peer_ack_sequence,
        client_unshifted_ack_sequence,
    }
}

pub(super) fn rewrite_server_window_spans_if_needed(
    bytes: &[u8],
    view: &MFrameView,
    state: &mut SessionState,
    server_peer_ack_sequence: u16,
) -> anyhow::Result<Option<CoalescedRewrite>> {
    if view.trailing_payload_length == 0 {
        return Ok(None);
    }

    let primary_len = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + view.payload_length;
    let Some(spans) = parse_packetized_spans(bytes, primary_len) else {
        return Ok(None);
    };
    if spans.is_empty() {
        return Ok(None);
    }

    let mut rewritten = Vec::new();
    let mut changed = false;
    let mut dropped_spans = 0u32;
    let mut rewritten_deflated_spans = 0u32;
    let mut record_proofs = Vec::new();
    let mut record_rewrites = Vec::new();

    let primary_record = &bytes[..primary_len];
    let primary = rewrite_coalesced_record_for_ee(
        primary_record,
        view.flags,
        view.high,
        view.deflated.as_ref(),
        view.payload_length,
        state,
        view.sequence,
        view.ack_sequence,
        server_peer_ack_sequence,
        0,
    )?;
    changed |= primary.changed;
    if primary.dropped {
        dropped_spans = dropped_spans.saturating_add(1);
    }
    if primary.rewritten_deflated {
        rewritten_deflated_spans = rewritten_deflated_spans.saturating_add(1);
    }
    record_proofs.push(primary.proof.clone());
    if primary.dropped && primary.abort_window_if_primary_consumed {
        let mut consumed = primary.record;
        encode_legacy_m_crc(&mut consumed)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to repair consumed coalesced primary CRC"))?;
        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            old_len = bytes.len(),
            new_len = consumed.len(),
            dropped_spans,
            "server coalesced M window consumed because primary semantic record was quarantined"
        );
        return Ok(Some(CoalescedRewrite::Single {
            proof: VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
            packet: consumed,
        }));
    }
    rewritten.extend_from_slice(&primary.record);
    record_rewrites.push(primary);

    for span in spans {
        let record_end = span.offset + span.record_length;
        let record = &bytes[span.offset..record_end];
        let outcome = rewrite_coalesced_record_for_ee(
            record,
            span.flags,
            span.high,
            span.deflated.as_ref(),
            span.payload_length,
            state,
            view.sequence,
            view.ack_sequence,
            server_peer_ack_sequence,
            span.offset,
        )?;
        changed |= outcome.changed;
        if outcome.dropped {
            dropped_spans = dropped_spans.saturating_add(1);
        }
        if outcome.rewritten_deflated {
            rewritten_deflated_spans = rewritten_deflated_spans.saturating_add(1);
        }
        record_proofs.push(outcome.proof.clone());
        rewritten.extend_from_slice(&outcome.record);
        record_rewrites.push(outcome);
    }

    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair coalesced M CRC"))?;
    tracing::info!(
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        changed,
        rewritten_deflated_spans,
        dropped_spans,
        "server coalesced M window spans rewritten for strict EE delivery"
    );
    if should_split_module_info_resource_window_records(&record_proofs) {
        let (split_packets, pre_shifted) =
            split_rewritten_coalesced_records(record_rewrites, state)?;
        tracing::info!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            output_packets = split_packets.len(),
            "server coalesced module-resource window split into typed standalone records so proxy-owned ServerStatus_ModuleResources sequence insertion stays coherent"
        );
        return if pre_shifted {
            Ok(Some(CoalescedRewrite::SplitPreShifted {
                packets: split_packets,
            }))
        } else {
            Ok(Some(CoalescedRewrite::Split {
                packets: split_packets,
            }))
        };
    }

    if should_split_mixed_area_load_gate_records(
        &record_proofs,
        state.synthetic_area.server_hold_gate.is_some(),
    ) {
        let (split_packets, pre_shifted) =
            split_rewritten_coalesced_records(record_rewrites, state)?;
        tracing::info!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            output_packets = split_packets.len(),
            "server coalesced area-load window split into typed standalone records so non-area spans stay behind the area ACK gate"
        );
        return if pre_shifted {
            Ok(Some(CoalescedRewrite::SplitPreShifted {
                packets: split_packets,
            }))
        } else {
            Ok(Some(CoalescedRewrite::Split {
                packets: split_packets,
            }))
        };
    }

    if rewritten.len() <= EE_SAFE_M_FRAME_DATAGRAM_BYTES {
        return Ok(Some(CoalescedRewrite::Single {
            proof: VerifiedProof::CoalescedWindow(record_proofs),
            packet: rewritten,
        }));
    }

    let (split_packets, pre_shifted) = split_rewritten_coalesced_records(record_rewrites, state)?;
    tracing::info!(
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        coalesced_len = rewritten.len(),
        safe_datagram_bytes = EE_SAFE_M_FRAME_DATAGRAM_BYTES,
        output_packets = split_packets.len(),
        "server coalesced M window exceeded EE-safe datagram budget; split into standalone typed reliable frames"
    );
    if pre_shifted {
        Ok(Some(CoalescedRewrite::SplitPreShifted {
            packets: split_packets,
        }))
    } else {
        Ok(Some(CoalescedRewrite::Split {
            packets: split_packets,
        }))
    }
}

fn should_split_mixed_area_load_gate_records(
    record_proofs: &[VerifiedProof],
    area_gate_active: bool,
) -> bool {
    area_gate_active
        && record_proofs.len() > 1
        && record_proofs
            .iter()
            .any(proof_contains_area_client_area_family)
        && record_proofs
            .iter()
            .any(|proof| !proof_is_area_load_gate_owned(proof))
}

fn should_split_module_info_resource_window_records(record_proofs: &[VerifiedProof]) -> bool {
    record_proofs.len() > 1 && record_proofs.iter().any(proof_contains_module_info_family)
}

fn proof_contains_area_client_area_family(proof: &VerifiedProof) -> bool {
    proof_contains_family(proof, VerifiedFamily::AreaClientArea)
}

fn proof_contains_module_info_family(proof: &VerifiedProof) -> bool {
    proof_contains_family(proof, VerifiedFamily::ModuleInfo)
}

fn proof_contains_family(proof: &VerifiedProof, wanted: VerifiedFamily) -> bool {
    match proof {
        VerifiedProof::Family(family) => *family == wanted,
        VerifiedProof::GameplayStream(families) => families.contains(&wanted),
        VerifiedProof::CoalescedWindow(records) => records
            .iter()
            .any(|record| proof_contains_family(record, wanted)),
    }
}

fn proof_is_area_load_gate_owned(proof: &VerifiedProof) -> bool {
    match proof {
        VerifiedProof::Family(family) => family_is_area_load_gate_owned(*family),
        VerifiedProof::GameplayStream(families) => {
            !families.is_empty() && families.iter().copied().all(family_is_area_load_gate_owned)
        }
        VerifiedProof::CoalescedWindow(records) => {
            !records.is_empty() && records.iter().all(proof_is_area_load_gate_owned)
        }
    }
}

fn family_is_area_load_gate_owned(family: VerifiedFamily) -> bool {
    matches!(
        family,
        VerifiedFamily::AreaClientArea
            | VerifiedFamily::LoadBar
            | VerifiedFamily::ConsumedEmptyMFrame
    )
}

fn split_rewritten_coalesced_records(
    records: Vec<CoalescedRecordRewrite>,
    state: &mut SessionState,
) -> anyhow::Result<(Vec<(VerifiedProof, Vec<u8>)>, bool)> {
    let Some(first_record) = records.first() else {
        return Ok((Vec::new(), false));
    };
    let base_sequence = read_be_u16(&first_record.record, 3)
        .ok_or_else(|| anyhow::anyhow!("coalesced split missing primary sequence"))?;
    let base_ack_sequence = read_be_u16(&first_record.record, 5)
        .ok_or_else(|| anyhow::anyhow!("coalesced split missing primary ACK sequence"))?;

    let mut packets = Vec::with_capacity(records.len());
    let mut assigned_output_frames = 0usize;
    let future_shift_base = base_sequence.wrapping_add(1);
    for (index, outcome) in records.into_iter().enumerate() {
        if outcome.record.len() < LEGACY_GAMEPLAY_PAYLOAD_OFFSET {
            anyhow::bail!(
                "rewritten coalesced record {} too short for standalone M frame",
                index
            );
        }

        let mut packet = outcome.record;
        // Decompile-backed transport rule: `CNetLayerWindow::FrameReceive`
        // accepts only normal reliable frames whose byte 0 is `M`, while
        // `UnpacketizeFullMessages` walks queued coalesced records by the same
        // 12-byte header layout after they have already been stored. If a
        // semantic rewrite needs those queued records to cross a strict ACK
        // gate separately, promote each verified queued record back into an
        // ordinary `M` frame. Packetized trailing records do not necessarily
        // carry standalone reliable sequence/ACK fields; when promoted, the
        // proxy owns a contiguous reliable sequence range beginning at the
        // original coalesced window sequence, then records a future shift for
        // the extra frames it inserted.
        packet[0] = b'M';
        let assigned_sequence = base_sequence.wrapping_add(assigned_output_frames as u16);
        write_be_u16(&mut packet, 3, assigned_sequence)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to assign split coalesced sequence"))?;
        let record_ack_sequence = read_be_u16(&packet, 5).unwrap_or(base_ack_sequence);
        if record_ack_sequence == 0 && base_ack_sequence != 0 {
            write_be_u16(&mut packet, 5, base_ack_sequence)
                .then_some(())
                .ok_or_else(|| anyhow::anyhow!("failed to assign split coalesced ACK"))?;
        }
        let mut output_records = if packet.len() > EE_SAFE_M_FRAME_DATAGRAM_BYTES {
            split_oversized_deflated_coalesced_record(index, packet)?
        } else {
            encode_legacy_m_crc(&mut packet)
                .then_some(())
                .ok_or_else(|| anyhow::anyhow!("failed to repair split coalesced M CRC"))?;
            vec![packet]
        };
        for packet in &mut output_records {
            shift_packet_sequence_for_existing_server_shifts(
                packet,
                &state.sequence.server_sequence_shifts,
                &state.sequence.coalesced_split_sequence_shifts,
                base_sequence,
            )?;
        }
        assigned_output_frames = assigned_output_frames
            .checked_add(output_records.len())
            .ok_or_else(|| anyhow::anyhow!("coalesced split output frame count overflow"))?;
        if assigned_output_frames > u16::MAX as usize {
            anyhow::bail!("coalesced split inserted too many reliable frames");
        }
        packets.extend(
            output_records
                .drain(..)
                .map(|packet| (outcome.proof.clone(), packet)),
        );
    }

    let inserted_extra_packets = assigned_output_frames.saturating_sub(1);
    if inserted_extra_packets == 0 {
        return Ok((packets, false));
    }

    let base = future_shift_base;
    // Apply pre-existing server sequence shifts before recording the new
    // coalesced split shift. This mirrors deflated-window expansion: current
    // replacement packets are emitted pre-shifted, then future server-origin
    // packets at or after the original post-window sequence are shifted by
    // the number of extra reliable frames we inserted.
    let delta = inserted_extra_packets as u16;
    if state
        .sequence
        .coalesced_split_sequence_shifts
        .iter()
        .any(|shift| {
            shift.source_sequence == base_sequence && shift.base == base && shift.delta == delta
        })
    {
        tracing::info!(
            source_sequence = base_sequence,
            shift_base = base,
            inserted_extra_packets,
            shifts = state.sequence.server_sequence_shifts.len(),
            output_frames = assigned_output_frames,
            "server coalesced split replay reused existing future sequence shift"
        );
        return Ok((packets, true));
    }

    state
        .sequence
        .server_sequence_shifts
        .push(SequenceShift { base, delta });
    state
        .sequence
        .coalesced_split_sequence_shifts
        .push(CoalescedSplitSequenceShift {
            source_sequence: base_sequence,
            base,
            delta,
        });
    trim_sequence_shifts(&mut state.sequence.server_sequence_shifts);
    trim_coalesced_split_sequence_shifts(&mut state.sequence.coalesced_split_sequence_shifts);
    tracing::info!(
        source_sequence = base_sequence,
        shift_base = base,
        inserted_extra_packets,
        shifts = state.sequence.server_sequence_shifts.len(),
        output_frames = assigned_output_frames,
        "server coalesced rewrite promoted packetized records into reliable M frames; future server sequences shifted"
    );
    Ok((packets, true))
}

fn split_oversized_deflated_coalesced_record(
    index: usize,
    packet: Vec<u8>,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let view = MFrameView::parse(&packet).ok_or_else(|| {
        anyhow::anyhow!(
            "rewritten coalesced record {} did not parse as standalone M frame",
            index
        )
    })?;
    let deflated = view.deflated.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "rewritten coalesced record {} is {} bytes, exceeding EE-safe datagram budget {}, and is not a deflated payload",
            index,
            packet.len(),
            EE_SAFE_M_FRAME_DATAGRAM_BYTES
        )
    })?;
    if view.payload_length < CNW_LENGTH_BYTES {
        anyhow::bail!(
            "rewritten coalesced deflated record {} is too short for inflated-length prefix",
            index
        );
    }
    let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + view.payload_length;
    let Some(combined_payload) = packet.get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end) else {
        anyhow::bail!(
            "rewritten coalesced deflated record {} payload overflow",
            index
        );
    };
    let combined_payload = combined_payload.to_vec();
    let compressed_chunk = combined_payload[CNW_LENGTH_BYTES..].to_vec();
    let reassembly = ServerDeflatedReassembly {
        inflated_length: deflated.inflated_length,
        expected_frames: 1,
        first_sequence: view.sequence,
        packetized_sequence: view.packetized_sequence,
        zlib_stream: (view.flags & 0x01) != 0,
        frames: vec![BufferedFrame {
            packet,
            payload_length: view.payload_length,
            sequence: view.sequence,
            server_peer_ack_sequence: view.ack_sequence,
            ack_sequence: view.ack_sequence,
            compressed_chunk,
        }],
        interleaved_packets: Vec::new(),
        interleaved_events: Vec::new(),
    };
    let outputs = reassembly::build_server_deflated_output_frames(
        &reassembly,
        &combined_payload,
        0x01,
        true,
    )?;
    if outputs
        .iter()
        .any(|packet| packet.len() > EE_SAFE_M_FRAME_DATAGRAM_BYTES)
    {
        anyhow::bail!(
            "rewritten coalesced deflated record {} could not be split into EE-safe M frames",
            index
        );
    }
    tracing::info!(
        index,
        original_len = reassembly.frames[0].packet.len(),
        output_frames = outputs.len(),
        safe_datagram_bytes = EE_SAFE_M_FRAME_DATAGRAM_BYTES,
        "oversized coalesced deflated record split into standalone reliable M frames"
    );
    Ok(outputs)
}

fn shift_packet_sequence_for_existing_server_shifts(
    packet: &mut [u8],
    shifts: &[SequenceShift],
    coalesced_split_shifts: &[CoalescedSplitSequenceShift],
    current_source_sequence: u16,
) -> anyhow::Result<()> {
    if shifts.is_empty() {
        return Ok(());
    }
    let view = MFrameView::parse(packet)
        .ok_or_else(|| anyhow::anyhow!("pre-shifted split coalesced packet is not an M frame"))?;
    if view.sequence == 0 {
        return Ok(());
    }
    let mut shifted = shift_sequence_for_peer(shifts, view.sequence);
    for split_shift in coalesced_split_shifts {
        if split_shift.source_sequence == current_source_sequence
            && split_shift.delta != 0
            && sequence_at_or_after(view.sequence, split_shift.base)
        {
            shifted = shifted.wrapping_sub(split_shift.delta);
        }
    }
    if shifted == view.sequence {
        return Ok(());
    }
    write_be_u16(packet, 3, shifted)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to pre-shift split coalesced sequence"))?;
    encode_legacy_m_crc(packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair pre-shifted split coalesced CRC"))?;
    Ok(())
}

struct CoalescedRecordRewrite {
    record: Vec<u8>,
    proof: VerifiedProof,
    changed: bool,
    dropped: bool,
    rewritten_deflated: bool,
    abort_window_if_primary_consumed: bool,
}

fn replay_completed_coalesced_deflated_record(
    state: &SessionState,
    record: &[u8],
    sequence: u16,
    offset: usize,
    payload_length: usize,
    inflated_length: usize,
    compressed: &[u8],
) -> Option<CoalescedRecordRewrite> {
    let entry = state
        .coalesced_replay
        .completed_deflated_records
        .iter()
        .find(|entry| {
            entry.sequence == sequence
                && entry.offset == offset
                && entry.payload_length == payload_length
                && entry.inflated_length == inflated_length
                && entry.compressed.as_slice() == compressed
        })?;

    tracing::info!(
        sequence,
        offset,
        payload_length,
        inflated_length,
        proof = entry.proof.as_str(),
        dropped = entry.dropped,
        rewritten_deflated = entry.rewritten_deflated,
        "server coalesced deflated record replayed from typed cache without re-inflating duplicate"
    );
    let mut replay_record = entry.record.clone();
    if replay_record.len() >= LEGACY_GAMEPLAY_PAYLOAD_OFFSET
        && record.len() >= LEGACY_GAMEPLAY_PAYLOAD_OFFSET
    {
        // Retransmitted coalesced deflated records can carry newer reliable-window
        // sequence/ACK fields even when the compressed gameplay bytes are a
        // duplicate of an already-classified semantic span. The replay cache owns
        // only the proven payload rewrite, not those transport fields.
        replay_record[3..7].copy_from_slice(&record[3..7]);
        replay_record[8..10].copy_from_slice(&record[8..10]);
        replay_record[7] = if entry.rewritten_deflated {
            record[7] & !0x01
        } else {
            record[7]
        };
    }
    Some(CoalescedRecordRewrite {
        record: replay_record,
        proof: entry.proof.clone(),
        changed: true,
        dropped: entry.dropped,
        rewritten_deflated: entry.rewritten_deflated,
        abort_window_if_primary_consumed: entry.abort_window_if_primary_consumed,
    })
}

fn remember_completed_coalesced_deflated_record(
    state: &mut SessionState,
    sequence: u16,
    offset: usize,
    payload_length: usize,
    inflated_length: usize,
    compressed: &[u8],
    outcome: &CoalescedRecordRewrite,
) {
    let entry = CompletedCoalescedDeflatedRecord {
        sequence,
        offset,
        payload_length,
        inflated_length,
        compressed: compressed.to_vec(),
        proof: outcome.proof.clone(),
        record: outcome.record.clone(),
        dropped: outcome.dropped,
        rewritten_deflated: outcome.rewritten_deflated,
        abort_window_if_primary_consumed: outcome.abort_window_if_primary_consumed,
    };

    if let Some(existing) = state
        .coalesced_replay
        .completed_deflated_records
        .iter_mut()
        .find(|existing| {
            existing.sequence == sequence
                && existing.offset == offset
                && existing.payload_length == payload_length
                && existing.inflated_length == inflated_length
                && existing.compressed.as_slice() == compressed
        })
    {
        *existing = entry;
        return;
    }

    const MAX_COMPLETED_COALESCED_DEFLATED_RECORDS: usize = 64;
    state
        .coalesced_replay
        .completed_deflated_records
        .push(entry);
    if state.coalesced_replay.completed_deflated_records.len()
        > MAX_COMPLETED_COALESCED_DEFLATED_RECORDS
    {
        let overflow = state.coalesced_replay.completed_deflated_records.len()
            - MAX_COMPLETED_COALESCED_DEFLATED_RECORDS;
        state
            .coalesced_replay
            .completed_deflated_records
            .drain(0..overflow);
    }
}

fn replay_completed_coalesced_direct_record(
    state: &SessionState,
    record: &[u8],
    sequence: u16,
    offset: usize,
    payload: &[u8],
) -> Option<CoalescedRecordRewrite> {
    let entry = state
        .coalesced_replay
        .completed_direct_records
        .iter()
        .find(|entry| {
            entry.sequence == sequence
                && entry.offset == offset
                && entry.payload.as_slice() == payload
        })?;

    let mut replay_record = entry.record.clone();
    if replay_record.len() >= LEGACY_GAMEPLAY_PAYLOAD_OFFSET
        && record.len() >= LEGACY_GAMEPLAY_PAYLOAD_OFFSET
    {
        // EE's reliable-window receive path identifies a retransmission by its
        // sequence slot. ACK and packetized transport fields may advance while
        // the gameplay payload remains the same, so replay the proven rewrite
        // with the current transport header without re-running game semantics.
        replay_record[3..10].copy_from_slice(&record[3..10]);
    }
    tracing::info!(
        sequence,
        offset,
        payload_length = payload.len(),
        proof = entry.proof.as_str(),
        dropped = entry.dropped,
        "server coalesced direct record replayed from typed cache without reapplying semantic effects"
    );
    Some(CoalescedRecordRewrite {
        changed: replay_record.as_slice() != record,
        record: replay_record,
        proof: entry.proof.clone(),
        dropped: entry.dropped,
        rewritten_deflated: false,
        abort_window_if_primary_consumed: entry.abort_window_if_primary_consumed,
    })
}

fn remember_completed_coalesced_direct_record(
    state: &mut SessionState,
    sequence: u16,
    offset: usize,
    payload: &[u8],
    outcome: &CoalescedRecordRewrite,
) {
    let entry = CompletedCoalescedDirectRecord {
        sequence,
        offset,
        payload: payload.to_vec(),
        proof: outcome.proof.clone(),
        record: outcome.record.clone(),
        dropped: outcome.dropped,
        abort_window_if_primary_consumed: outcome.abort_window_if_primary_consumed,
    };
    if let Some(existing) = state
        .coalesced_replay
        .completed_direct_records
        .iter_mut()
        .find(|existing| {
            existing.sequence == sequence
                && existing.offset == offset
                && existing.payload.as_slice() == payload
        })
    {
        *existing = entry;
        return;
    }

    const MAX_COMPLETED_COALESCED_DIRECT_RECORDS: usize = 128;
    state.coalesced_replay.completed_direct_records.push(entry);
    if state.coalesced_replay.completed_direct_records.len()
        > MAX_COMPLETED_COALESCED_DIRECT_RECORDS
    {
        let overflow = state.coalesced_replay.completed_direct_records.len()
            - MAX_COMPLETED_COALESCED_DIRECT_RECORDS;
        state
            .coalesced_replay
            .completed_direct_records
            .drain(0..overflow);
    }
}

fn rewrite_coalesced_record_for_ee(
    record: &[u8],
    flags: u8,
    high: Option<HighLevel>,
    deflated: Option<&DeflatedEnvelope>,
    payload_length: usize,
    state: &mut SessionState,
    sequence: u16,
    ack_sequence: u16,
    server_peer_ack_sequence: u16,
    offset: usize,
) -> anyhow::Result<CoalescedRecordRewrite> {
    if payload_length == 0 {
        return Ok(CoalescedRecordRewrite {
            record: record.to_vec(),
            proof: VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
            changed: false,
            dropped: false,
            rewritten_deflated: false,
            abort_window_if_primary_consumed: false,
        });
    }

    let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + payload_length;
    let Some(payload) = record.get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end) else {
        return consume_coalesced_record(record, offset, "payload-overflow");
    };

    let prefer_deflated = deflated
        .map(|deflated| deflated.plausible && payload_length >= CNW_LENGTH_BYTES)
        .unwrap_or(false);
    if !prefer_deflated && let Some(high) = high {
        if let Some(replay) =
            replay_completed_coalesced_direct_record(state, record, sequence, offset, payload)
        {
            return Ok(replay);
        }
        let source_payload = payload.to_vec();
        let mut payload = source_payload.clone();
        if high.major == 0x01 && high.minor == 0x03 {
            if let Some(shape) = deferred_module_resources::capture_early_status_payload_if_needed(
                &payload,
                sequence,
                ack_sequence,
                &state.module_resources,
                &mut state.deferred_module_resources.pending,
            ) {
                tracing::info!(
                    offset,
                    sequence,
                    ack_sequence,
                    declared = shape.declared,
                    status_string_len = shape.status_string_len,
                    fragment_tail_len = shape.fragment_tail_len,
                    "server coalesced early ServerStatus_ModuleRunning consumed as verified deferred module-resource status"
                );
                let outcome = consume_coalesced_record_with_proof(
                    record,
                    offset,
                    "deferred-module-resources-pending",
                    VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
                    false,
                    false,
                )?;
                remember_completed_coalesced_direct_record(
                    state,
                    sequence,
                    offset,
                    &source_payload,
                    &outcome,
                );
                return Ok(outcome);
            }
        }
        let semantic_rewrite_summary = server_dispatch::rewrite_inflated_payload_for_ee(
            &mut payload,
            Some(&state.area_context.latest_area_placeables),
            server_dispatch::SemanticScope::CoalescedSpan,
            Some(&state.module_resources),
            Some(&state.semantic.objects),
            None,
        );
        super::observe_quickbar_stream_probe_from_rewrite(state, &semantic_rewrite_summary);
        if semantic_rewrite_summary.should_quarantine()
            || !semantic_rewrite_summary.any_rewrite()
            || payload.len() > u16::MAX as usize
        {
            tracing::warn!(
                offset,
                payload_length,
                major = high.major,
                minor = high.minor,
                name = high.name(),
                known = high.is_known(),
                prefix = %hex_prefix(record, 32),
                "server coalesced M record quarantined: semantic translator did not claim high-level payload"
            );
            let outcome = consume_coalesced_record(record, offset, "unclaimed-high-level")?;
            remember_completed_coalesced_direct_record(
                state,
                sequence,
                offset,
                &source_payload,
                &outcome,
            );
            return Ok(outcome);
        }
        if let Some(summary) = semantic_rewrite_summary.area_rewrite.as_ref() {
            state.area_context.latest_area_placeables = summary.placeable_context.clone();
            queue_area_client_area_side_effects_for_window(
                state,
                sequence,
                sequence,
                ack_sequence,
                summary,
            )?;
        }
        let verified_proof = semantic_rewrite_summary.verified_proof();
        // `Login_GetWaypoint` can be carried as a later packetized record
        // inside a coalesced server window. Those records use the same
        // reliable-window header fields, but their first byte is not required
        // to be a standalone `M`, so `MFrameView::parse(record)` is too narrow
        // here. Keep the semantic side effect in `login_waypoint.rs`; this
        // layer supplies only the already-verified payload and the resolved
        // primary/queued-record transport context.
        let transport = coalesced_record_transport_context(
            record,
            sequence,
            server_peer_ack_sequence,
            ack_sequence,
        );
        login_waypoint::maybe_queue_empty_waypoint_response_payload(
            state,
            &payload,
            transport.sequence,
            transport.client_unshifted_ack_sequence,
        )?;
        let live_object_inventory_materialization =
            super::observe_verified_server_payload_semantics(state, &verified_proof, &payload);
        super::apply_verified_server_semantic_side_effects(
            state,
            &verified_proof,
            super::ServerSemanticFrameContext {
                sequence: transport.sequence,
                server_peer_ack_sequence: transport.server_peer_ack_sequence,
                client_unshifted_ack_sequence: transport.client_unshifted_ack_sequence,
                live_object_inventory_materialization,
            },
        );
        queue_module_resources_after_coalesced_module_info_if_ready(
            state,
            &verified_proof,
            transport.sequence,
            transport.client_unshifted_ack_sequence,
        )?;

        let mut out_record = record[..LEGACY_GAMEPLAY_PAYLOAD_OFFSET].to_vec();
        write_be_u16(&mut out_record, 10, payload.len() as u16)
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("failed to update coalesced direct record length"))?;
        out_record.extend_from_slice(&payload);
        let changed = out_record.as_slice() != record;
        tracing::info!(
            offset,
            name = high.name(),
            major = high.major,
            minor = high.minor,
            old_payload_length = payload_length,
            new_payload_length = payload.len(),
            changed,
            "server coalesced direct high-level record semantically claimed for EE"
        );
        let outcome = CoalescedRecordRewrite {
            record: out_record,
            proof: verified_proof,
            changed,
            dropped: false,
            rewritten_deflated: false,
            abort_window_if_primary_consumed: false,
        };
        remember_completed_coalesced_direct_record(
            state,
            sequence,
            offset,
            &source_payload,
            &outcome,
        );
        return Ok(outcome);
    }

    let Some(deflated) = deflated else {
        tracing::warn!(
            offset,
            payload_length,
            prefix = %hex_prefix(record, 32),
            "server coalesced M record quarantined: unknown non-deflated payload"
        );
        return consume_coalesced_record(record, offset, "unknown-non-deflated");
    };

    if !deflated.plausible || payload_length < CNW_LENGTH_BYTES {
        tracing::warn!(
            offset,
            payload_length,
            inflated_length = deflated.inflated_length,
            prefix = %hex_prefix(record, 32),
            "server coalesced M deflated record quarantined: implausible envelope"
        );
        return consume_coalesced_record(record, offset, "implausible-deflated-envelope");
    }

    let compressed = &payload[CNW_LENGTH_BYTES..];
    if let Some(replay) = replay_completed_coalesced_deflated_record(
        state,
        record,
        sequence,
        offset,
        payload_length,
        deflated.inflated_length,
        compressed,
    ) {
        return Ok(replay);
    }
    let InflatedGameplayPayload {
        bytes: mut inflated,
        used_server_stream,
    } = reassembly::inflate_gameplay_payload(
        compressed,
        deflated.inflated_length,
        (flags & 0x01) != 0,
        &mut state.deflate.server_zlib_inflater,
    )?;

    server_dispatch::wrap_legacy_live_object_continuation_if_needed(&mut inflated);
    let single_incomplete_stream_unit =
        server_dispatch::inflated_payload_is_single_incomplete_stream_unit(&inflated);
    // A stale Diamond CNW read-window can make a complete Area_ClientArea
    // object look like one pending gameplay-stream fragment. Give only that
    // typed family an exact semantic proof attempt before the persistent-zlib
    // continuation owner is allowed to consume it. Random P-like zlib tails
    // still stay on the continuation path because the area translator must
    // uniquely infer the fragment boundary and satisfy the EE LoadArea cursor.
    let recovered_incomplete_area_rewrite = if single_incomplete_stream_unit
        && inflated.starts_with(&[b'P', 0x04, 0x01])
    {
        let mut candidate = inflated.clone();
        let rewrite = server_dispatch::rewrite_incomplete_area_client_area_for_ee(
            &mut candidate,
            Some(&state.module_resources),
            Some(&state.semantic.objects),
        );
        if rewrite.as_ref().is_some_and(|rewrite| {
            !rewrite.should_quarantine()
                && rewrite.verified_family() == VerifiedFamily::AreaClientArea
                && rewrite.area_rewrite.is_some()
        }) {
            tracing::info!(
                offset,
                inflated = inflated.len(),
                used_server_stream,
                "server coalesced incomplete stream unit recovered by exact Area_ClientArea proof"
            );
            inflated = candidate;
            rewrite
        } else {
            None
        }
    } else {
        None
    };
    if recovered_incomplete_area_rewrite.is_none()
        && (single_incomplete_stream_unit || HighLevel::parse(&inflated).is_none())
    {
        if let Some(outcome) = rewrite_coalesced_stream_continuation_for_ee(
            record,
            offset,
            &inflated,
            used_server_stream,
            state,
            sequence,
        )? {
            remember_completed_coalesced_deflated_record(
                state,
                sequence,
                offset,
                payload_length,
                deflated.inflated_length,
                compressed,
                &outcome,
            );
            return Ok(outcome);
        }
        let reason = if single_incomplete_stream_unit {
            "coalesced-incomplete-stream-unit"
        } else {
            "coalesced-no-high-level"
        };
        dump_invalid_inflated_payload_for_span(&inflated, sequence, reason);
        tracing::warn!(
            offset,
            inflated = inflated.len(),
            prefix = %hex_prefix(&inflated, 32),
            used_server_stream,
            single_incomplete_stream_unit,
            "server coalesced M deflated record quarantined: no complete high-level payload"
        );
        let outcome = consume_coalesced_record(record, offset, reason)?;
        remember_completed_coalesced_deflated_record(
            state,
            sequence,
            offset,
            payload_length,
            deflated.inflated_length,
            compressed,
            &outcome,
        );
        return Ok(outcome);
    }

    let semantic_rewrite_summary = recovered_incomplete_area_rewrite.unwrap_or_else(|| {
        server_dispatch::rewrite_inflated_payload_for_ee(
            &mut inflated,
            Some(&state.area_context.latest_area_placeables),
            server_dispatch::SemanticScope::CoalescedSpan,
            Some(&state.module_resources),
            Some(&state.semantic.objects),
            None,
        )
    });
    super::observe_quickbar_stream_probe_from_rewrite(state, &semantic_rewrite_summary);
    if semantic_rewrite_summary.should_quarantine() || !semantic_rewrite_summary.any_rewrite() {
        let reason = semantic_rewrite_summary
            .quarantine_reason
            .unwrap_or("coalesced-untranslated-required-semantic-family");
        dump_invalid_inflated_payload_for_span(&inflated, sequence, reason);
        tracing::warn!(
            offset,
            inflated = inflated.len(),
            reason,
            prefix = %hex_prefix(&inflated, 32),
            "server coalesced M deflated record quarantined: required semantic translation is missing"
        );
        let outcome = consume_coalesced_record(record, offset, reason)?;
        remember_completed_coalesced_deflated_record(
            state,
            sequence,
            offset,
            payload_length,
            deflated.inflated_length,
            compressed,
            &outcome,
        );
        return Ok(outcome);
    }
    if !inflated_cnw_fragment_offset_valid(&inflated) {
        dump_invalid_inflated_payload_for_span(
            &inflated,
            sequence,
            "coalesced-invalid-cnw-fragment-offset",
        );
        tracing::warn!(
            offset,
            inflated = inflated.len(),
            prefix = %hex_prefix(&inflated, 32),
            "server coalesced M deflated record quarantined: invalid CNW fragment offset"
        );
        let outcome = consume_coalesced_record(record, offset, "invalid-cnw-fragment-offset")?;
        remember_completed_coalesced_deflated_record(
            state,
            sequence,
            offset,
            payload_length,
            deflated.inflated_length,
            compressed,
            &outcome,
        );
        return Ok(outcome);
    }
    if let Some(summary) = semantic_rewrite_summary.area_rewrite.as_ref() {
        state.area_context.latest_area_placeables = summary.placeable_context.clone();
        queue_area_client_area_side_effects_for_window(
            state,
            sequence,
            sequence,
            ack_sequence,
            summary,
        )?;
    }

    let verified_family = semantic_rewrite_summary.verified_family();
    let verified_proof = semantic_rewrite_summary.verified_proof();
    let live_object_inventory_materialization =
        super::observe_verified_server_payload_semantics(state, &verified_proof, &inflated);
    let transport = coalesced_record_transport_context(
        record,
        sequence,
        server_peer_ack_sequence,
        ack_sequence,
    );
    super::apply_verified_server_semantic_side_effects(
        state,
        &verified_proof,
        super::ServerSemanticFrameContext {
            sequence: transport.sequence,
            server_peer_ack_sequence: transport.server_peer_ack_sequence,
            client_unshifted_ack_sequence: transport.client_unshifted_ack_sequence,
            live_object_inventory_materialization,
        },
    );
    queue_module_resources_after_coalesced_module_info_if_ready(
        state,
        &verified_proof,
        transport.sequence,
        transport.client_unshifted_ack_sequence,
    )?;
    let must_convert_stream = used_server_stream || state.deflate.server_zlib_stream_proxy_owned;
    if used_server_stream {
        state.deflate.server_zlib_stream_proxy_owned = true;
        let owner = verified_proof
            .primary_family()
            .map(ContinuationOwner::from_verified_family)
            .unwrap_or_else(|| ContinuationOwner::from_verified_family(verified_family));
        claim_server_zlib_stream_owner(state, owner);
    }

    let rewritten_compressed = deflate_zlib(&inflated)?;
    let new_payload_length = CNW_LENGTH_BYTES + rewritten_compressed.len();
    if new_payload_length > u16::MAX as usize {
        tracing::warn!(
            offset,
            new_payload_length,
            "server coalesced M deflated record quarantined: rewritten payload too large"
        );
        let outcome = consume_coalesced_record(record, offset, "rewritten-payload-too-large")?;
        remember_completed_coalesced_deflated_record(
            state,
            sequence,
            offset,
            payload_length,
            deflated.inflated_length,
            compressed,
            &outcome,
        );
        return Ok(outcome);
    }

    let mut out_record = record[..LEGACY_GAMEPLAY_PAYLOAD_OFFSET].to_vec();
    if !out_record.is_empty() {
        out_record[7] &= !0x01;
    }
    write_be_u16(&mut out_record, 10, new_payload_length as u16)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to update coalesced deflated record length"))?;
    out_record.extend_from_slice(&(inflated.len() as u32).to_le_bytes());
    out_record.extend_from_slice(&rewritten_compressed);
    let changed = must_convert_stream || out_record.as_slice() != record;
    tracing::info!(
        offset,
        families = ?semantic_rewrite_summary,
        used_server_stream,
        changed,
        "server coalesced deflated record semantically claimed and emitted as EE zlib"
    );

    let outcome = CoalescedRecordRewrite {
        record: out_record,
        proof: verified_proof,
        changed,
        dropped: false,
        rewritten_deflated: true,
        abort_window_if_primary_consumed: false,
    };
    remember_completed_coalesced_deflated_record(
        state,
        sequence,
        offset,
        payload_length,
        deflated.inflated_length,
        compressed,
        &outcome,
    );
    Ok(outcome)
}

fn queue_module_resources_after_coalesced_module_info_if_ready(
    state: &mut SessionState,
    proof: &VerifiedProof,
    record_sequence: u16,
    record_ack_sequence: u16,
) -> anyhow::Result<()> {
    if !proof_contains_module_info_family(proof) {
        return Ok(());
    }

    deferred_module_resources::queue_after_module_info_if_ready(
        &mut state.deferred_module_resources.pending,
        &mut state.synthetic_area.pending_server_to_client_packets,
        &mut state.sequence.server_sequence_shifts,
        record_sequence,
        record_sequence,
        record_ack_sequence,
        &state.module_resources,
    )?;
    tracing::info!(
        record_sequence,
        record_ack_sequence,
        proof = proof.as_str(),
        "server coalesced Module_Info observed; module-resource state transition evaluated"
    );
    Ok(())
}

fn claim_server_zlib_stream_owner(state: &mut SessionState, owner: ContinuationOwner) {
    if state.deflate.server_zlib_stream_owner != Some(owner) {
        state.deflate.server_zlib_stream_epoch =
            state.deflate.server_zlib_stream_epoch.saturating_add(1);
    }
    state.deflate.server_zlib_stream_owner = Some(owner);
}

fn rewrite_coalesced_stream_continuation_for_ee(
    record: &[u8],
    offset: usize,
    inflated: &[u8],
    used_server_stream: bool,
    state: &mut SessionState,
    sequence: u16,
) -> anyhow::Result<Option<CoalescedRecordRewrite>> {
    // A no-header inflated chunk is valid only as continuation bytes from an
    // already-classified Diamond server zlib stream. This is deliberately not a
    // raw passthrough: the source stream is consumed by the proxy inflater, and
    // the EE-facing coalesced record is reduced to an empty reliable progress
    // shell with a typed owner/epoch proof.
    if !used_server_stream || !state.deflate.server_zlib_stream_proxy_owned {
        return Ok(None);
    }

    let owner = state
        .deflate
        .server_zlib_stream_owner
        .unwrap_or(ContinuationOwner::UnknownProxyOwned);
    let stream_epoch = state.deflate.server_zlib_stream_epoch;
    if owner == ContinuationOwner::UnknownProxyOwned || stream_epoch == 0 || inflated.is_empty() {
        tracing::warn!(
            offset,
            owner = owner.as_str(),
            stream_epoch,
            continuation_len = inflated.len(),
            "server coalesced zlib-stream continuation rejected: missing known semantic owner"
        );
        return Ok(None);
    }

    dump_invalid_inflated_payload_for_span(
        inflated,
        sequence,
        "claimed-coalesced-zlib-stream-continuation",
    );
    tracing::info!(
        offset,
        sequence,
        owner = owner.as_str(),
        stream_epoch,
        inflated = inflated.len(),
        prefix = %hex_prefix(inflated, 32),
        "server coalesced zlib-stream continuation claimed as proxy-owned semantic stream tail"
    );

    consume_coalesced_record_with_proof(
        record,
        offset,
        "claimed-zlib-stream-continuation",
        VerifiedProof::family(VerifiedFamily::ServerZlibStreamContinuation {
            owner,
            stream_epoch,
            first_sequence: sequence,
        }),
        false,
        false,
    )
    .map(Some)
}

fn consume_coalesced_record(
    record: &[u8],
    offset: usize,
    reason: &'static str,
) -> anyhow::Result<CoalescedRecordRewrite> {
    consume_coalesced_record_with_proof(
        record,
        offset,
        reason,
        VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
        true,
        true,
    )
}

fn consume_coalesced_record_with_proof(
    record: &[u8],
    offset: usize,
    reason: &'static str,
    proof: VerifiedProof,
    warn: bool,
    abort_window_if_primary_consumed: bool,
) -> anyhow::Result<CoalescedRecordRewrite> {
    let mut out_record = record[..LEGACY_GAMEPLAY_PAYLOAD_OFFSET.min(record.len())].to_vec();
    if out_record.len() < LEGACY_GAMEPLAY_PAYLOAD_OFFSET {
        out_record.resize(LEGACY_GAMEPLAY_PAYLOAD_OFFSET, 0);
    }
    if out_record.len() > 7 {
        out_record[7] &= !0x07;
    }
    write_be_u16(&mut out_record, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear quarantined coalesced record length"))?;
    if warn {
        tracing::warn!(
            offset,
            reason,
            old_len = record.len(),
            "server coalesced M record consumed because strict semantic translation is unavailable"
        );
    } else {
        tracing::info!(
            offset,
            reason,
            old_len = record.len(),
            "server coalesced M record consumed as verified empty progress shell"
        );
    }
    Ok(CoalescedRecordRewrite {
        record: out_record,
        proof,
        changed: true,
        dropped: true,
        rewritten_deflated: false,
        abort_window_if_primary_consumed,
    })
}

fn dump_invalid_inflated_payload_for_span(inflated: &[u8], sequence: u16, reason: &str) {
    let Some(dir) = crate::translate::diagnostics::diagnostic_dump_dir() else {
        return;
    };

    if fs::create_dir_all(&dir).is_err() {
        return;
    }

    let high_name = HighLevel::parse(inflated)
        .map(|high| {
            high.name()
                .replace(['<', '>', '/', '\\', ':', '*', '?', '"', '|'], "_")
        })
        .unwrap_or_else(|| "no-high-level".to_string());
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!(
        "{}-{}-coalesced-seq{}-{}.bin",
        reason, high_name, sequence, millis
    ));

    if fs::write(&path, inflated).is_ok() {
        tracing::info!(
            path = %path.display(),
            inflated_length = inflated.len(),
            sequence,
            reason,
            "dumped invalid coalesced inflated payload for offline fixture analysis"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesced_stream_continuation_requires_known_proxy_owned_owner() {
        let record = [0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        let inflated = [0xEC, 0x00, 0x3C, 0x56, 0xFE, 0x3E];
        let mut state = SessionState::default();
        state.deflate.server_zlib_stream_proxy_owned = true;

        let rejected = rewrite_coalesced_stream_continuation_for_ee(
            &record, 0, &inflated, true, &mut state, 42,
        )
        .expect("continuation helper should not fail");

        assert!(rejected.is_none());
    }

    #[test]
    fn coalesced_stream_continuation_consumes_known_owner_without_raw_emit() {
        let mut record = [0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        record[7] = 0x01;
        let inflated = [0xEC, 0x00, 0x3C, 0x56, 0xFE, 0x3E];
        let mut state = SessionState::default();
        state.deflate.server_zlib_stream_proxy_owned = true;
        state.deflate.server_zlib_stream_owner = Some(ContinuationOwner::GameObjUpdateLiveObject);
        state.deflate.server_zlib_stream_epoch = 9;

        let rewritten = rewrite_coalesced_stream_continuation_for_ee(
            &record, 0, &inflated, true, &mut state, 42,
        )
        .expect("continuation helper should not fail")
        .expect("known proxy-owned stream should be consumed as a typed continuation shell");

        assert!(rewritten.dropped);
        assert!(!rewritten.rewritten_deflated);
        assert_eq!(rewritten.record.len(), LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
        assert_eq!(rewritten.record[10], 0);
        assert_eq!(rewritten.record[11], 0);
        assert_eq!(
            rewritten.proof,
            VerifiedProof::family(VerifiedFamily::ServerZlibStreamContinuation {
                owner: ContinuationOwner::GameObjUpdateLiveObject,
                stream_epoch: 9,
                first_sequence: 42,
            })
        );
    }

    #[test]
    fn p_like_zlib_stream_tail_is_not_reclassified_as_unknown_high_level() {
        let inflated = [
            0x70, 0x5E, 0x51, 0xF2, 0x6E, 0x9E, 0xF9, 0xCF, 0x07, 0x56, 0x82, 0xF5,
        ];

        assert!(HighLevel::parse(&inflated).is_some());
        assert!(
            server_dispatch::inflated_payload_is_single_incomplete_stream_unit(&inflated),
            "P-like zlib stream tails with impossible declared lengths must stay on the continuation path"
        );
    }

    #[test]
    fn coalesced_deflated_replay_cache_returns_typed_shell_without_reinflate() {
        let compressed = [0x10, 0x20, 0x30, 0x40];
        let proof = VerifiedProof::family(VerifiedFamily::ServerZlibStreamContinuation {
            owner: ContinuationOwner::GuiQuickbar,
            stream_epoch: 21,
            first_sequence: 34,
        });
        let mut cached_record = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        cached_record[3..5].copy_from_slice(&34u16.to_be_bytes());
        cached_record[5..7].copy_from_slice(&0x0010u16.to_be_bytes());
        cached_record[7] = 0x00;
        cached_record[8..10].copy_from_slice(&0x1111u16.to_be_bytes());
        let outcome = CoalescedRecordRewrite {
            record: cached_record.clone(),
            proof: proof.clone(),
            changed: true,
            dropped: true,
            rewritten_deflated: true,
            abort_window_if_primary_consumed: false,
        };
        let mut state = SessionState::default();

        remember_completed_coalesced_deflated_record(
            &mut state,
            34,
            67,
            381,
            604,
            &compressed,
            &outcome,
        );

        let mut current_record = cached_record.clone();
        current_record[5..7].copy_from_slice(&0x0020u16.to_be_bytes());
        current_record[7] = 0x01;
        current_record[8..10].copy_from_slice(&0x2222u16.to_be_bytes());

        let replay = replay_completed_coalesced_deflated_record(
            &state,
            &current_record,
            34,
            67,
            381,
            604,
            &compressed,
        )
        .expect("matching coalesced stream duplicate should replay cached proof");

        assert!(replay.changed);
        assert!(replay.dropped);
        assert!(replay.rewritten_deflated);
        assert_eq!(&replay.record[3..7], &current_record[3..7]);
        assert_eq!(&replay.record[8..10], &current_record[8..10]);
        assert_eq!(replay.record[7] & 0x01, 0);
        assert_eq!(&replay.record[10..12], &outcome.record[10..12]);
        assert_eq!(replay.proof, proof);
    }

    #[test]
    fn coalesced_direct_replay_cache_refreshes_transport_without_semantic_reapply() {
        let payload =
            crate::translate::inventory::build_ee_inventory_payload(0x01, 0x8000_1234, true, 4)
                .expect("exact Inventory payload");
        let mut cached_record = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        cached_record[0] = b'M';
        cached_record[3..5].copy_from_slice(&53u16.to_be_bytes());
        cached_record[5..7].copy_from_slice(&80u16.to_be_bytes());
        cached_record[7] = 0x0A;
        cached_record[8..10].copy_from_slice(&1u16.to_be_bytes());
        cached_record[10..12].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        cached_record.extend_from_slice(&payload);
        let outcome = CoalescedRecordRewrite {
            record: cached_record.clone(),
            proof: VerifiedProof::family(VerifiedFamily::Inventory),
            changed: false,
            dropped: false,
            rewritten_deflated: false,
            abort_window_if_primary_consumed: false,
        };
        let mut state = SessionState::default();
        remember_completed_coalesced_direct_record(&mut state, 53, 132, &payload, &outcome);

        let mut current_record = cached_record;
        current_record[5..7].copy_from_slice(&83u16.to_be_bytes());
        current_record[8..10].copy_from_slice(&2u16.to_be_bytes());
        let replay =
            replay_completed_coalesced_direct_record(&state, &current_record, 53, 132, &payload)
                .expect("matching direct reliable retransmit should replay typed output");

        assert_eq!(&replay.record[3..10], &current_record[3..10]);
        assert_eq!(
            replay.proof,
            VerifiedProof::family(VerifiedFamily::Inventory)
        );
        assert!(!replay.dropped);
        assert!(!replay.rewritten_deflated);
    }

    #[test]
    fn coalesced_direct_chat_talk_canonicalizes_diamond_padding_before_proof() {
        let source_payload = [
            0x50, 0x09, 0x01, 0x16, 0, 0, 0, 0xC3, 0xFF, 0xFF, 0xFF, 7, 0, 0, 0, b'c', b'h', b'e',
            b'e', b's', b'e', b'7', 0x64,
        ];
        let mut record = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        record[0] = b'M';
        record[3..5].copy_from_slice(&32u16.to_be_bytes());
        record[5..7].copy_from_slice(&75u16.to_be_bytes());
        record[7] = 0x0A;
        record[8..10].copy_from_slice(&1u16.to_be_bytes());
        record[10..12].copy_from_slice(&(source_payload.len() as u16).to_be_bytes());
        record.extend_from_slice(&source_payload);

        let mut state = SessionState::default();
        let outcome = rewrite_coalesced_record_for_ee(
            &record,
            0x0A,
            HighLevel::parse(&source_payload),
            None,
            source_payload.len(),
            &mut state,
            32,
            75,
            75,
            203,
        )
        .expect("exact live Chat_Talk record should translate");

        assert_eq!(outcome.proof, VerifiedProof::family(VerifiedFamily::Chat));
        assert!(outcome.changed);
        assert!(!outcome.dropped);
        assert_eq!(
            outcome.record[LEGACY_GAMEPLAY_PAYLOAD_OFFSET..],
            [
                0x50, 0x09, 0x01, 0x16, 0, 0, 0, 0xC3, 0xFF, 0xFF, 0xFF, 7, 0, 0, 0, b'c', b'h',
                b'e', b'e', b's', b'e', b'7', 0x60,
            ]
        );
    }

    #[test]
    fn coalesced_direct_inventory_retransmit_observes_semantics_once() {
        let inventory =
            crate::translate::inventory::build_ee_inventory_payload(0x01, 0x8000_1234, true, 4)
                .expect("exact Inventory payload");
        let chat = [b'P', 0x09, 0x05];
        let mut packet = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        packet[0] = b'M';
        packet[3..5].copy_from_slice(&53u16.to_be_bytes());
        packet[5..7].copy_from_slice(&80u16.to_be_bytes());
        packet[7] = 0x0A;
        packet[8..10].copy_from_slice(&1u16.to_be_bytes());
        packet[10..12].copy_from_slice(&(inventory.len() as u16).to_be_bytes());
        packet.extend_from_slice(&inventory);

        let mut trailing = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        trailing[3..5].copy_from_slice(&54u16.to_be_bytes());
        trailing[5..7].copy_from_slice(&80u16.to_be_bytes());
        trailing[7] = 0x0A;
        trailing[8..10].copy_from_slice(&1u16.to_be_bytes());
        trailing[10..12].copy_from_slice(&(chat.len() as u16).to_be_bytes());
        trailing.extend_from_slice(&chat);
        packet.extend_from_slice(&trailing);
        assert!(encode_legacy_m_crc(&mut packet));

        let mut state = SessionState::default();
        state
            .semantic
            .objects
            .observe_materialized_item_object_ids(&[0x8000_1234]);
        super::super::translate_server_to_client(&packet, &mut state)
            .expect("first coalesced Inventory window should translate");
        assert_eq!(
            state
                .semantic
                .ui
                .inventory_equipment_bridge_handoff_emissions,
            1
        );
        assert_eq!(state.coalesced_replay.completed_direct_records.len(), 2);

        let mut retransmit = packet;
        retransmit[5..7].copy_from_slice(&81u16.to_be_bytes());
        assert!(encode_legacy_m_crc(&mut retransmit));
        super::super::translate_server_to_client(&retransmit, &mut state)
            .expect("coalesced Inventory retransmit should replay typed output");

        assert_eq!(
            state
                .semantic
                .ui
                .inventory_equipment_bridge_handoff_emissions,
            1,
            "the same reliable source record must not re-enter the semantic reducer"
        );
        assert_eq!(state.coalesced_replay.completed_direct_records.len(), 2);
    }

    #[test]
    fn coalesced_login_get_waypoint_queues_empty_response() {
        let mut record = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        record[0] = b'M';
        record[3..5].copy_from_slice(&21u16.to_be_bytes());
        record[5..7].copy_from_slice(&72u16.to_be_bytes());
        record[7] = 0x0A;
        record[8..10].copy_from_slice(&1u16.to_be_bytes());
        record[10..12].copy_from_slice(&3u16.to_be_bytes());
        record[LEGACY_GAMEPLAY_PAYLOAD_OFFSET..].copy_from_slice(&[0x70, 0x02, 0x0C]);
        assert!(encode_legacy_m_crc(&mut record));

        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(74);

        let outcome = rewrite_coalesced_record_for_ee(
            &record,
            0x0A,
            Some(HighLevel {
                envelope: 0x70,
                major: 0x02,
                minor: 0x0C,
            }),
            None,
            3,
            &mut state,
            21,
            72,
            72,
            0,
        )
        .expect("coalesced login waypoint should translate");

        assert_eq!(outcome.proof.primary_family(), Some(VerifiedFamily::Login));
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        let pending = state.sequence.pending_client_to_server_packets.remove(0);
        let pending_view = MFrameView::parse(&pending).expect("queued response should parse");
        assert_eq!(pending_view.sequence, 75);
        assert_eq!(pending_view.ack_sequence, 21);
        assert_eq!(
            pending_view.high.map(|high| (high.major, high.minor)),
            Some((0x02, 0x0D))
        );
        assert_eq!(state.sequence.latest_client_sequence_from_client, Some(74));
    }

    #[test]
    fn coalesced_login_get_waypoint_non_primary_span_queues_empty_response() {
        let mut record = vec![0xCCu8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        record[3..5].copy_from_slice(&22u16.to_be_bytes());
        record[5..7].copy_from_slice(&72u16.to_be_bytes());
        record[7] = 0x0A;
        record[8..10].copy_from_slice(&1u16.to_be_bytes());
        record[10..12].copy_from_slice(&3u16.to_be_bytes());
        record[LEGACY_GAMEPLAY_PAYLOAD_OFFSET..].copy_from_slice(&[0x70, 0x02, 0x0C]);

        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(74);

        let outcome = rewrite_coalesced_record_for_ee(
            &record,
            0x0A,
            Some(HighLevel {
                envelope: 0x70,
                major: 0x02,
                minor: 0x0C,
            }),
            None,
            3,
            &mut state,
            21,
            72,
            72,
            202,
        )
        .expect("non-primary coalesced login waypoint span should translate");

        assert_eq!(outcome.proof.primary_family(), Some(VerifiedFamily::Login));
        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        let pending = state.sequence.pending_client_to_server_packets.remove(0);
        let pending_view = MFrameView::parse(&pending).expect("queued response should parse");
        assert_eq!(pending_view.sequence, 75);
        assert_eq!(pending_view.ack_sequence, 22);
        assert_eq!(
            pending_view.high.map(|high| (high.major, high.minor)),
            Some((0x02, 0x0D))
        );
        assert_eq!(
            state.login_waypoint.last_server_get_waypoint_sequence,
            Some(22)
        );
    }

    #[test]
    fn split_rewritten_coalesced_records_promotes_spans_to_crc_valid_m_frames() {
        let mut primary = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        primary[0] = b'M';
        primary[3..5].copy_from_slice(&61u16.to_be_bytes());
        primary[5..7].copy_from_slice(&81u16.to_be_bytes());
        primary[7] = 0x0A;
        primary[8..10].copy_from_slice(&1u16.to_be_bytes());
        primary[10..12].copy_from_slice(&3u16.to_be_bytes());
        primary[12..15].copy_from_slice(&[b'P', 0x09, 0x05]);

        let mut span = vec![0xCCu8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        span[3..5].copy_from_slice(&62u16.to_be_bytes());
        span[5..7].copy_from_slice(&81u16.to_be_bytes());
        span[7] = 0x0A;
        span[8..10].copy_from_slice(&1u16.to_be_bytes());
        span[10..12].copy_from_slice(&3u16.to_be_bytes());
        span[12..15].copy_from_slice(&[b'P', 0x05, 0x02]);

        let mut state = SessionState::default();
        let (packets, pre_shifted) = split_rewritten_coalesced_records(
            vec![
                CoalescedRecordRewrite {
                    record: primary,
                    proof: VerifiedProof::family(VerifiedFamily::Chat),
                    changed: false,
                    dropped: false,
                    rewritten_deflated: false,
                    abort_window_if_primary_consumed: false,
                },
                CoalescedRecordRewrite {
                    record: span,
                    proof: VerifiedProof::family(VerifiedFamily::GameObjUpdateObjectControl),
                    changed: true,
                    dropped: false,
                    rewritten_deflated: false,
                    abort_window_if_primary_consumed: false,
                },
            ],
            &mut state,
        )
        .expect("split coalesced records should promote to standalone M frames");

        assert!(pre_shifted);
        assert_eq!(packets.len(), 2);
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.server_sequence_shifts[0].base, 62);
        assert_eq!(state.sequence.server_sequence_shifts[0].delta, 1);
        assert_eq!(packets[0].0, VerifiedProof::family(VerifiedFamily::Chat));
        assert_eq!(
            packets[1].0,
            VerifiedProof::family(VerifiedFamily::GameObjUpdateObjectControl)
        );
        for (expected_sequence, (_, packet)) in [61u16, 62u16].into_iter().zip(packets.iter()) {
            assert_eq!(packet[0], b'M');
            let view = MFrameView::parse(packet).expect("promoted packet should parse as M");
            assert!(view.crc_valid);
            assert_eq!(view.sequence, expected_sequence);
            assert_eq!(view.ack_sequence, 81);
            assert_eq!(view.payload_length, 3);
        }
    }

    #[test]
    fn split_rewritten_coalesced_records_replay_reuses_future_shift() {
        let mut primary = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        primary[0] = b'M';
        primary[3..5].copy_from_slice(&61u16.to_be_bytes());
        primary[5..7].copy_from_slice(&81u16.to_be_bytes());
        primary[7] = 0x0A;
        primary[8..10].copy_from_slice(&1u16.to_be_bytes());
        primary[10..12].copy_from_slice(&3u16.to_be_bytes());
        primary[12..15].copy_from_slice(&[b'P', 0x09, 0x05]);

        let mut span = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        span[0] = b'M';
        span[3..5].copy_from_slice(&62u16.to_be_bytes());
        span[5..7].copy_from_slice(&81u16.to_be_bytes());
        span[7] = 0x0A;
        span[8..10].copy_from_slice(&1u16.to_be_bytes());
        span[10..12].copy_from_slice(&3u16.to_be_bytes());
        span[12..15].copy_from_slice(&[b'P', 0x05, 0x02]);

        let mut state = SessionState::default();
        let make_records = |primary: Vec<u8>, span: Vec<u8>| {
            vec![
                CoalescedRecordRewrite {
                    record: primary,
                    proof: VerifiedProof::family(VerifiedFamily::Chat),
                    changed: false,
                    dropped: false,
                    rewritten_deflated: false,
                    abort_window_if_primary_consumed: false,
                },
                CoalescedRecordRewrite {
                    record: span,
                    proof: VerifiedProof::family(VerifiedFamily::GameObjUpdateObjectControl),
                    changed: true,
                    dropped: false,
                    rewritten_deflated: false,
                    abort_window_if_primary_consumed: false,
                },
            ]
        };

        let (first_packets, first_pre_shifted) = split_rewritten_coalesced_records(
            make_records(primary.clone(), span.clone()),
            &mut state,
        )
        .expect("first split should promote records");

        assert!(first_pre_shifted);
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.coalesced_split_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.server_sequence_shifts[0].base, 62);
        assert_eq!(state.sequence.server_sequence_shifts[0].delta, 1);
        assert_eq!(
            state.sequence.coalesced_split_sequence_shifts[0].source_sequence,
            61
        );
        assert_eq!(state.sequence.coalesced_split_sequence_shifts[0].base, 62);
        assert_eq!(state.sequence.coalesced_split_sequence_shifts[0].delta, 1);

        let first_sequences: Vec<u16> = first_packets
            .iter()
            .map(|(_, packet)| {
                MFrameView::parse(packet)
                    .expect("first replay packet should parse")
                    .sequence
            })
            .collect();
        assert_eq!(first_sequences, vec![61, 62]);

        let (second_packets, second_pre_shifted) =
            split_rewritten_coalesced_records(make_records(primary, span), &mut state)
                .expect("replayed split should reuse existing future shift");

        assert!(second_pre_shifted);
        assert_eq!(
            state.sequence.server_sequence_shifts.len(),
            1,
            "replaying the same coalesced source window must not append another future shift"
        );
        assert_eq!(
            state.sequence.coalesced_split_sequence_shifts.len(),
            1,
            "the companion replay guard should stay one-to-one with the recorded future shift"
        );
        let second_sequences: Vec<u16> = second_packets
            .iter()
            .map(|(_, packet)| {
                MFrameView::parse(packet)
                    .expect("second replay packet should parse")
                    .sequence
            })
            .collect();
        assert_eq!(
            second_sequences,
            vec![61, 62],
            "replaying the same source window must emit the same reliable sequence numbers"
        );
    }

    #[test]
    fn split_rewritten_coalesced_records_assigns_missing_span_sequences() {
        let mut primary = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        primary[0] = b'M';
        primary[3..5].copy_from_slice(&61u16.to_be_bytes());
        primary[5..7].copy_from_slice(&81u16.to_be_bytes());
        primary[7] = 0x0A;
        primary[8..10].copy_from_slice(&1u16.to_be_bytes());
        primary[10..12].copy_from_slice(&3u16.to_be_bytes());
        primary[12..15].copy_from_slice(&[b'P', 0x09, 0x05]);

        let mut span = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        span[7] = 0x0A;
        span[8..10].copy_from_slice(&1u16.to_be_bytes());
        span[10..12].copy_from_slice(&3u16.to_be_bytes());
        span[12..15].copy_from_slice(&[b'P', 0x05, 0x02]);

        let mut state = SessionState::default();
        let (packets, pre_shifted) = split_rewritten_coalesced_records(
            vec![
                CoalescedRecordRewrite {
                    record: primary,
                    proof: VerifiedProof::family(VerifiedFamily::Chat),
                    changed: false,
                    dropped: false,
                    rewritten_deflated: false,
                    abort_window_if_primary_consumed: false,
                },
                CoalescedRecordRewrite {
                    record: span,
                    proof: VerifiedProof::family(VerifiedFamily::GameObjUpdateObjectControl),
                    changed: true,
                    dropped: false,
                    rewritten_deflated: false,
                    abort_window_if_primary_consumed: false,
                },
            ],
            &mut state,
        )
        .expect("split coalesced records should assign standalone reliable sequences");

        assert!(pre_shifted);
        assert_eq!(packets.len(), 2);
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.server_sequence_shifts[0].base, 62);
        assert_eq!(state.sequence.server_sequence_shifts[0].delta, 1);
        for (expected_sequence, (_, packet)) in [61u16, 62u16].into_iter().zip(packets.iter()) {
            let view = MFrameView::parse(packet).expect("promoted packet should parse as M");
            assert!(view.crc_valid);
            assert_eq!(view.sequence, expected_sequence);
            assert_eq!(view.ack_sequence, 81);
        }
    }

    #[test]
    fn mixed_area_load_gate_window_splits_non_area_span_proofs() {
        let proofs = vec![
            VerifiedProof::family(VerifiedFamily::AreaClientArea),
            VerifiedProof::family(VerifiedFamily::PlayerList),
        ];

        assert!(should_split_mixed_area_load_gate_records(&proofs, true));
        assert!(!should_split_mixed_area_load_gate_records(&proofs, false));
    }

    #[test]
    fn mixed_module_info_resource_window_splits_for_sequence_insertion() {
        let proofs = vec![
            VerifiedProof::family(VerifiedFamily::ClientSideMessage),
            VerifiedProof::family(VerifiedFamily::ModuleInfo),
            VerifiedProof::family(VerifiedFamily::Login),
        ];

        assert!(should_split_module_info_resource_window_records(&proofs));
        assert!(!should_split_module_info_resource_window_records(&[
            VerifiedProof::family(VerifiedFamily::ModuleInfo)
        ]));
        assert!(!should_split_module_info_resource_window_records(&[
            VerifiedProof::family(VerifiedFamily::ClientSideMessage),
            VerifiedProof::family(VerifiedFamily::Login),
        ]));
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn chapter4_module_info_stream_span_prefers_deflate_over_false_module_zero() {
        // Local Diamond Chapter4 startup sends a packetized coalesced record
        // whose deflated payload length prefix starts `50 03 00 00`. That is
        // `P 03/00` only if the transport deflate flag is ignored; after the
        // preceding zlib stream seed it inflates to the real `Module_Info`
        // payload.
        let seed = include_bytes!(
            "../../../fixtures/module_info/local_chapter4_charlist_zlib_stream_seed_20260523.bin"
        );
        let mut state = SessionState::default();
        let seed_inflated = reassembly::inflate_gameplay_payload(
            seed,
            20531,
            true,
            &mut state.deflate.server_zlib_inflater,
        )
        .expect("Chapter4 charlist zlib stream seed should inflate");
        assert!(seed_inflated.used_server_stream);
        assert!(state.deflate.server_zlib_inflater.is_some());

        let packet = include_bytes!(
            "../../../fixtures/module_info/local_chapter4_module_info_coalesced_20260523.bin"
        );
        let view = MFrameView::parse(packet).expect("Chapter4 coalesced packet should parse");
        let primary_len = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + view.payload_length;
        let spans = parse_packetized_spans(packet, primary_len)
            .expect("Chapter4 coalesced packet should have packetized spans");
        let span = spans
            .iter()
            .find(|span| {
                span.high
                    .map(|high| high.major == 0x03 && high.minor == 0x00)
                    .unwrap_or(false)
                    && span
                        .deflated
                        .as_ref()
                        .map(|deflated| deflated.plausible)
                        .unwrap_or(false)
            })
            .expect("Chapter4 fixture should contain the false Module 3/0 deflated span");
        let record = &packet[span.offset..span.offset + span.record_length];

        let rewritten = rewrite_coalesced_record_for_ee(
            record,
            span.flags,
            span.high,
            span.deflated.as_ref(),
            span.payload_length,
            &mut state,
            view.sequence,
            view.ack_sequence,
            view.ack_sequence,
            span.offset,
        )
        .expect("Chapter4 deflated Module_Info span should rewrite");

        assert!(!rewritten.dropped);
        assert!(rewritten.rewritten_deflated);
        assert!(
            proof_contains_module_info_family(&rewritten.proof),
            "the false 3/0 span must be owned by Module_Info after inflation"
        );
        let context = state
            .module_resources
            .observed_module_context()
            .expect("Chapter4 Module_Info should record runtime module context");
        assert_eq!(context.localized_name, "Chapter Four");
        assert_eq!(context.module_resref, "chap3_chap4");
        let observed_rows = context
            .areas
            .iter()
            .map(|area| format!("0x{:08X}:{}", area.object_id, area.name))
            .collect::<Vec<_>>();
        assert!(
            context.areas.iter().any(|area| {
                area.object_id == 0x8000_0368 && area.name.eq_ignore_ascii_case("Castle Never")
            }),
            "runtime context should preserve the Chapter4 area-table row for object 0x80000368; observed {observed_rows:?}"
        );
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn xp2_chapter3_module_info_split_record_remains_strict_valid_when_released() {
        // Local Diamond XP2 Chapter 3 startup uses the same server zlib stream
        // seed as the Chapter4 fixture, then emits Module_Info inside a mixed
        // coalesced window. The module-resource ACK gate splits and delays that
        // deflated Module_Info record, so the released standalone record still
        // needs an exact strict proof.
        let seed = include_bytes!(
            "../../../fixtures/module_info/local_chapter4_charlist_zlib_stream_seed_20260523.bin"
        );
        let mut state = SessionState::default();
        let seed_inflated = reassembly::inflate_gameplay_payload(
            seed,
            20531,
            true,
            &mut state.deflate.server_zlib_inflater,
        )
        .expect("shared local charlist zlib stream seed should inflate");
        assert!(seed_inflated.used_server_stream);

        let packet = include_bytes!(
            "../../../fixtures/module_info/local_xp2_chapter3_module_info_coalesced_20260523.bin"
        );
        let view = MFrameView::parse(packet).expect("XP2 Chapter3 coalesced packet should parse");
        let rewrite =
            rewrite_server_window_spans_if_needed(packet, &view, &mut state, view.ack_sequence)
                .expect("XP2 Chapter3 coalesced packet should rewrite")
                .expect("XP2 Chapter3 coalesced packet should split for module resources");

        let CoalescedRewrite::SplitPreShifted { packets } = rewrite else {
            panic!("XP2 Chapter3 module-resource window should split into verified records");
        };
        let (proof, module_packet) = packets
            .iter()
            .find(|(proof, _)| proof_contains_module_info_family(proof))
            .expect("split window should contain a Module_Info proof");

        let decision = crate::strict::decide_verified_proof_translated(
            crate::packet::Direction::ServerToClientSynthetic,
            proof,
            module_packet,
        );
        assert_eq!(
            decision.verdict,
            crate::strict::Verdict::Allow,
            "released split Module_Info must remain strict-valid: {decision:?}"
        );
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn shadowguard_module_info_window_splits_for_resource_gate() {
        // Local ShadowGuard premium-module startup from 2026-05-24 emits
        // Module_Info inside a coalesced Diamond server window. Keep the split
        // and strict proof pinned so module-resource insertion does not regress
        // into raw coalesced passthrough.
        let packet = include_bytes!(
            "../../../fixtures/m_frame/local_shadowguard_seq7_module_info_coalesced_20260524.bin"
        );
        let mut state = SessionState::default();
        let view = MFrameView::parse(packet).expect("ShadowGuard coalesced packet should parse");
        assert!(view.crc_valid);
        assert_eq!(view.sequence, 7);
        assert_eq!(view.ack_sequence, 72);

        let rewrite =
            rewrite_server_window_spans_if_needed(packet, &view, &mut state, view.ack_sequence)
                .expect("ShadowGuard Module_Info coalesced packet should rewrite")
                .expect("ShadowGuard Module_Info window should split for resource insertion");

        let CoalescedRewrite::SplitPreShifted { packets } = rewrite else {
            panic!("ShadowGuard module-resource window should split into pre-shifted packets");
        };

        let (proof, module_packet) = packets
            .iter()
            .find(|(proof, _)| proof_contains_module_info_family(proof))
            .expect("split window should contain a Module_Info proof");
        let decision = crate::strict::decide_verified_proof_translated(
            crate::packet::Direction::ServerToClientSynthetic,
            proof,
            module_packet,
        );
        assert_eq!(
            decision.verdict,
            crate::strict::Verdict::Allow,
            "released ShadowGuard Module_Info must remain strict-valid: {decision:?}"
        );
        assert!(
            state
                .synthetic_area
                .pending_server_to_client_packets
                .iter()
                .any(|pending| pending.family == VerifiedFamily::ServerStatusModuleResources),
            "coalesced ShadowGuard Module_Info should queue exact module resources"
        );
    }

    #[test]
    fn coalesced_module_info_queues_exact_module_resources_with_sequence_shift() {
        let mut state = SessionState::default();
        assert!(
            state
                .module_resources
                .observe_legacy_module_info_resources(&[], Some("cep23_v1"))
        );

        let mut record = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        record[0] = b'M';
        record[3..5].copy_from_slice(&7u16.to_be_bytes());
        record[5..7].copy_from_slice(&72u16.to_be_bytes());
        record[7] = 0x0A;
        record[8..10].copy_from_slice(&1u16.to_be_bytes());
        record[10..12].copy_from_slice(&3u16.to_be_bytes());
        record[12..15].copy_from_slice(&[b'P', 0x03, 0x01]);

        let transport = coalesced_record_transport_context(&record, 61, 72, 70);
        assert_eq!(transport.sequence, 7);
        assert_eq!(transport.server_peer_ack_sequence, 72);
        assert_eq!(transport.client_unshifted_ack_sequence, 72);

        queue_module_resources_after_coalesced_module_info_if_ready(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::ModuleInfo),
            transport.sequence,
            transport.client_unshifted_ack_sequence,
        )
        .expect("coalesced Module_Info should queue module resources");

        assert_eq!(
            state.synthetic_area.pending_server_to_client_packets.len(),
            1
        );
        let pending = &state.synthetic_area.pending_server_to_client_packets[0];
        assert_eq!(pending.family, VerifiedFamily::ServerStatusModuleResources);
        let view = MFrameView::parse(&pending.packet).expect("pending packet should parse");
        assert!(view.crc_valid);
        assert_eq!(view.sequence, 7);
        assert_eq!(view.ack_sequence, 72);
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.server_sequence_shifts[0].base, 7);
        assert_eq!(state.sequence.server_sequence_shifts[0].delta, 1);
    }

    #[test]
    fn coalesced_zero_transport_fields_inherit_primary_window_for_resource_insertion() {
        let mut state = SessionState::default();
        assert!(
            state
                .module_resources
                .observe_legacy_module_info_resources(&[], Some("cep23_v1"))
        );

        let mut record = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        record[0] = b'M';
        record[7] = 0x0A;
        record[8..10].copy_from_slice(&1u16.to_be_bytes());
        record[10..12].copy_from_slice(&3u16.to_be_bytes());
        record[12..15].copy_from_slice(&[b'P', 0x03, 0x01]);

        let transport = coalesced_record_transport_context(&record, 7, 82, 80);
        assert_eq!(
            transport,
            CoalescedRecordTransportContext {
                sequence: 7,
                server_peer_ack_sequence: 82,
                client_unshifted_ack_sequence: 80,
            }
        );

        queue_module_resources_after_coalesced_module_info_if_ready(
            &mut state,
            &VerifiedProof::family(VerifiedFamily::ModuleInfo),
            transport.sequence,
            transport.client_unshifted_ack_sequence,
        )
        .expect("coalesced Module_Info should queue module resources with window sequence");

        assert_eq!(
            state.synthetic_area.pending_server_to_client_packets.len(),
            1
        );
        let pending = &state.synthetic_area.pending_server_to_client_packets[0];
        assert_eq!(pending.family, VerifiedFamily::ServerStatusModuleResources);
        let view = MFrameView::parse(&pending.packet).expect("pending packet should parse");
        assert!(view.crc_valid);
        assert_eq!(view.sequence, 7);
        assert_eq!(view.ack_sequence, 80);
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        assert_eq!(state.sequence.server_sequence_shifts[0].base, 7);
        assert_eq!(state.sequence.server_sequence_shifts[0].delta, 1);
    }

    #[test]
    fn area_load_gate_window_stays_coalesced_when_all_records_are_gate_owned() {
        let proofs = vec![
            VerifiedProof::family(VerifiedFamily::AreaClientArea),
            VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
        ];

        assert!(!should_split_mixed_area_load_gate_records(&proofs, true));
    }

    #[test]
    fn oversized_deflated_coalesced_record_is_repacketized_and_shifts_later_spans() {
        let combined_payload_len = EE_SAFE_M_FRAME_DATAGRAM_BYTES * 2;
        let inflated_len = 0x0888u32;
        let mut oversized = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + combined_payload_len];
        oversized[0] = b'M';
        oversized[3..5].copy_from_slice(&100u16.to_be_bytes());
        oversized[5..7].copy_from_slice(&80u16.to_be_bytes());
        oversized[7] = 0x0E;
        oversized[8..10].copy_from_slice(&1u16.to_be_bytes());
        oversized[10..12].copy_from_slice(&(combined_payload_len as u16).to_be_bytes());
        oversized[12..16].copy_from_slice(&inflated_len.to_le_bytes());
        for (offset, byte) in oversized[16..].iter_mut().enumerate() {
            *byte = (offset as u8).wrapping_mul(13).wrapping_add(7);
        }
        assert!(encode_legacy_m_crc(&mut oversized));

        let mut following = vec![0u8; LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 3];
        following[0] = b'M';
        following[3..5].copy_from_slice(&101u16.to_be_bytes());
        following[5..7].copy_from_slice(&80u16.to_be_bytes());
        following[7] = 0x0A;
        following[8..10].copy_from_slice(&1u16.to_be_bytes());
        following[10..12].copy_from_slice(&3u16.to_be_bytes());
        following[12..15].copy_from_slice(&[b'P', 0x09, 0x05]);
        assert!(encode_legacy_m_crc(&mut following));

        let mut state = SessionState::default();
        let (packets, pre_shifted) = split_rewritten_coalesced_records(
            vec![
                CoalescedRecordRewrite {
                    record: oversized,
                    proof: VerifiedProof::family(VerifiedFamily::AreaClientArea),
                    changed: true,
                    dropped: false,
                    rewritten_deflated: true,
                    abort_window_if_primary_consumed: false,
                },
                CoalescedRecordRewrite {
                    record: following,
                    proof: VerifiedProof::family(VerifiedFamily::Chat),
                    changed: false,
                    dropped: false,
                    rewritten_deflated: false,
                    abort_window_if_primary_consumed: false,
                },
            ],
            &mut state,
        )
        .expect("oversized deflated coalesced record should be split into safe M frames");

        assert!(pre_shifted);
        assert!(packets.len() >= 4);
        assert_eq!(state.sequence.server_sequence_shifts.len(), 1);
        let shift = &state.sequence.server_sequence_shifts[0];
        assert_eq!(shift.base, 101);
        assert_eq!(shift.delta as usize, packets.len() - 1);

        for (index, (_, packet)) in packets.iter().enumerate() {
            assert!(
                packet.len() <= EE_SAFE_M_FRAME_DATAGRAM_BYTES,
                "packet {index} exceeded EE-safe datagram cap"
            );
            let view = MFrameView::parse(packet).expect("split output should parse as M");
            assert!(view.crc_valid, "packet {index} should have repaired CRC");
            assert_eq!(view.sequence, 100u16.wrapping_add(index as u16));
        }
        assert_eq!(
            packets
                .last()
                .expect("following packet should be present")
                .0,
            VerifiedProof::family(VerifiedFamily::Chat)
        );
    }
}
