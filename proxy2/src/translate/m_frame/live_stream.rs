//! Buffered live-object stream handling for deflated M windows.
//!
//! This module only decides whether fragmented `GameObjUpdate_LiveObject` bytes
//! need to be accumulated across reliable windows. Record-family semantics stay
//! in `translate::live_object` and `translate::live_object_update`.

use crate::{
    packet::m::HighLevel,
    translate::{ContinuationOwner, Emit, VerifiedFamily, area, live_object},
};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    CNW_LENGTH_BYTES, SessionState,
    deflate::deflate_zlib,
    hex_prefix, live_update,
    reassembly::{
        CompletedDeflatedReplay, ServerDeflatedReassembly, build_server_deflated_output_frames,
        emit_family_packets_with_interleaved, remember_completed_server_stream_window,
    },
};

#[derive(Debug, Clone)]
pub(super) struct PendingLiveObjectStream {
    kind: PendingLiveObjectStreamKind,
    read_bytes: Vec<u8>,
    fragment_bytes: Vec<u8>,
    first_sequence: u16,
    chunks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingLiveObjectStreamKind {
    LegacyHighLevelFragmentPrefix,
    RawPrefixedFragments,
}

impl PendingLiveObjectStreamKind {
    fn as_str(self) -> &'static str {
        match self {
            PendingLiveObjectStreamKind::LegacyHighLevelFragmentPrefix => {
                "legacy-high-level-fragment-prefix"
            }
            PendingLiveObjectStreamKind::RawPrefixedFragments => "raw-prefixed-fragments",
        }
    }
}

fn build_live_object_placeholder_frames(
    reassembly: &ServerDeflatedReassembly,
) -> anyhow::Result<Vec<Vec<u8>>> {
    // Decompile-backed placeholder discipline:
    //
    // EE only reaches `CNWSMessage::HandleGameObjUpdate` after a high-level
    // `P 05 01` packet seeds `CNWMessage::SetReadMessage` with a declared read
    // buffer and trailing CNW fragment bits. During strict stream buffering we
    // cannot send the real live-object continuation yet, because the record
    // boundary and fragment ownership are only proven after later zlib windows
    // arrive. Sending an empty M control shell here does not advance the EE
    // reliable window, so use the narrowest validator-owned live-object packet
    // instead:
    //
    //   W 00 0E
    //
    // `live_object_update::is_verified_read_buffer_only_record` treats the
    // three-byte `W` world-status shape as fragment-neutral after both EE and
    // Diamond decompile checks showed it is routed inside the live-object read
    // stream, not an object add/update/delete. The single fragment byte 0x60 is
    // an MSB-packed header-only tail: its top bits encode exactly three valid
    // header bits and no semantic BOOL payload.
    const PLACEHOLDER: [u8; 11] = [
        b'P', 0x05, 0x01, // high-level GameObjUpdate_LiveObject
        0x0A, 0x00, 0x00, 0x00, // declared = header + len("W 00 0E")
        b'W', 0x00, 0x0E, // fragment-neutral read-buffer-only record
        0x60, // CNW fragment header bits only
    ];

    let compressed = deflate_zlib(&PLACEHOLDER)?;
    let mut combined = Vec::with_capacity(CNW_LENGTH_BYTES + compressed.len());
    combined.extend_from_slice(&(PLACEHOLDER.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);
    build_server_deflated_output_frames(reassembly, &combined, 0x01, true)
}

pub(super) fn maybe_buffer_or_flush_server_live_object_stream(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    used_server_stream: bool,
    bytes: &mut Vec<u8>,
) -> anyhow::Result<Option<Emit>> {
    claim_server_zlib_stream_owner(state, ContinuationOwner::GameObjUpdateLiveObject);
    if !used_server_stream {
        return Ok(None);
    }
    if !state.deflate.server_zlib_stream_proxy_owned
        && !starts_with_live_object_high_level(bytes)
        && state.live_object.pending_stream.is_none()
    {
        return Ok(None);
    }
    if HighLevel::parse(bytes).is_none() {
        if let Some(kind) = state
            .live_object
            .pending_stream
            .as_ref()
            .map(|pending| pending.kind)
        {
            match kind {
                PendingLiveObjectStreamKind::LegacyHighLevelFragmentPrefix => {
                    append_pending_live_object_continuation(
                        state,
                        reassembly.first_sequence,
                        bytes,
                        kind,
                    );
                }
                PendingLiveObjectStreamKind::RawPrefixedFragments => {
                    let Some(prefix_len) =
                        prefixed_live_object_stream_continuation_prefix_len(bytes)
                    else {
                        return Ok(None);
                    };
                    append_pending_live_object_prefixed_fragment(
                        state,
                        reassembly.first_sequence,
                        bytes,
                        prefix_len,
                    );
                }
            }
            let area_context = state.area_context.latest_area_placeables.clone();
            if flush_pending_live_object_stream_if_verified(state, bytes, Some(&area_context)) {
                tracing::info!(
                    current_sequence = reassembly.first_sequence,
                    rebuilt_inflated = bytes.len(),
                    stream_kind = kind.as_str(),
                    prefix = %hex_prefix(bytes, 32),
                    "server live-object stream continuation flushed as verified GameObjUpdate_LiveObject"
                );
                return Ok(None);
            }

            state.deflate.server_zlib_stream_proxy_owned = true;
            let outputs = build_live_object_placeholder_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::GameObjUpdateLiveObject,
                    packets: outputs.clone(),
                },
            );
            let interleaved_packets = reassembly.interleaved_packets.clone();
            if let Some(pending) = state.live_object.pending_stream.as_ref() {
                tracing::info!(
                    first_sequence = pending.first_sequence,
                    current_sequence = reassembly.first_sequence,
                    stream_kind = pending.kind.as_str(),
                    chunks = pending.chunks,
                    read_bytes = pending.read_bytes.len(),
                    fragment_bytes = pending.fragment_bytes.len(),
                    "server live-object continuation buffered pending semantic claim"
                );
            }
            return Ok(Some(emit_family_packets_with_interleaved(
                VerifiedFamily::GameObjUpdateLiveObject,
                outputs,
                interleaved_packets,
            )));
        }

        if let Some(split) = live_object::raw_prefixed_live_object_split(bytes) {
            append_pending_live_object_prefixed_fragment(
                state,
                reassembly.first_sequence,
                bytes,
                split.live_bytes_offset,
            );
            let area_context = state.area_context.latest_area_placeables.clone();
            if flush_pending_live_object_stream_if_verified(state, bytes, Some(&area_context)) {
                tracing::info!(
                    first_sequence = reassembly.first_sequence,
                    rebuilt_inflated = bytes.len(),
                    prefix = %hex_prefix(bytes, 32),
                    "server live-object raw prefixed stream flushed as verified GameObjUpdate_LiveObject"
                );
                return Ok(None);
            }

            state.deflate.server_zlib_stream_proxy_owned = true;
            let outputs = build_live_object_placeholder_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::GameObjUpdateLiveObject,
                    packets: outputs.clone(),
                },
            );
            let interleaved_packets = reassembly.interleaved_packets.clone();
            if let Some(pending) = state.live_object.pending_stream.as_ref() {
                tracing::info!(
                    first_sequence = pending.first_sequence,
                    current_sequence = reassembly.first_sequence,
                    stream_kind = pending.kind.as_str(),
                    chunks = pending.chunks,
                    read_bytes = pending.read_bytes.len(),
                    fragment_bytes = pending.fragment_bytes.len(),
                    "server live-object raw prefixed stream buffered pending semantic claim"
                );
            }
            return Ok(Some(emit_family_packets_with_interleaved(
                VerifiedFamily::GameObjUpdateLiveObject,
                outputs,
                interleaved_packets,
            )));
        }
    }

    if starts_with_live_object_high_level(bytes) {
        if state.live_object.pending_stream.is_some()
            && complete_live_object_payload_claims_independently(
                bytes,
                Some(&state.area_context.latest_area_placeables),
            )
        {
            if let Some(pending) = state.live_object.pending_stream.take() {
                if let Some(candidate) = build_live_object_stream_payload(&pending) {
                    dump_pending_live_object_candidate(
                        &candidate,
                        pending.first_sequence,
                        pending.chunks,
                        "pending-live-object-stale-before-independent-packet",
                    );
                }
                tracing::info!(
                    stale_first_sequence = pending.first_sequence,
                    current_sequence = reassembly.first_sequence,
                    stream_kind = pending.kind.as_str(),
                    stale_chunks = pending.chunks,
                    read_bytes = pending.read_bytes.len(),
                    fragment_bytes = pending.fragment_bytes.len(),
                    "server live-object pending stream dropped before independent complete GameObjUpdate_LiveObject"
                );
            }
        }
        // Complete high-level live-object packets belong to server_dispatch's
        // focused semantic translators. This stream layer only buffers actual
        // continuations/fragments; running declared-length repair here forces
        // every valid P/05/01 packet through speculative split searches before
        // the bounded typed dispatcher can own it.
        if looks_like_clean_legacy_live_object_fragment(bytes) {
            append_pending_live_object_clean_fragment(state, reassembly.first_sequence, bytes);
            let area_context = state.area_context.latest_area_placeables.clone();
            if flush_pending_live_object_stream_if_verified(state, bytes, Some(&area_context)) {
                tracing::info!(
                    first_sequence = reassembly.first_sequence,
                    rebuilt_inflated = bytes.len(),
                    prefix = %hex_prefix(bytes, 32),
                    "server live-object high-level fragment stream flushed immediately as verified GameObjUpdate_LiveObject"
                );
                return Ok(None);
            }

            state.deflate.server_zlib_stream_proxy_owned = true;
            let outputs = build_live_object_placeholder_frames(reassembly)?;
            remember_completed_server_stream_window(
                state,
                reassembly,
                source_compressed_length,
                CompletedDeflatedReplay::VerifiedPackets {
                    family: VerifiedFamily::GameObjUpdateLiveObject,
                    packets: outputs.clone(),
                },
            );
            let interleaved_packets = reassembly.interleaved_packets.clone();
            if let Some(pending) = state.live_object.pending_stream.as_ref() {
                tracing::info!(
                    first_sequence = pending.first_sequence,
                    current_sequence = reassembly.first_sequence,
                    stream_kind = pending.kind.as_str(),
                    chunks = pending.chunks,
                    read_bytes = pending.read_bytes.len(),
                    fragment_bytes = pending.fragment_bytes.len(),
                    "server live-object stream fragment buffered pending continuation"
                );
            }
            return Ok(Some(emit_family_packets_with_interleaved(
                VerifiedFamily::GameObjUpdateLiveObject,
                outputs,
                interleaved_packets,
            )));
        }

        return Ok(None);
    }

    Ok(None)
}

fn complete_live_object_payload_claims_independently(
    bytes: &[u8],
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> bool {
    if !starts_with_live_object_high_level(bytes) || HighLevel::parse(bytes).is_none() {
        return false;
    }
    if live_update::claim_payload_if_verified(bytes).is_some() {
        return true;
    }
    let mut probe = bytes.to_vec();
    live_update::rewrite_payload_to_exact_ee_if_possible(&mut probe, latest_area_placeables)
        .is_some()
        && live_update::claim_payload_if_verified(&probe).is_some()
}

fn starts_with_live_object_high_level(bytes: &[u8]) -> bool {
    bytes.len() >= 7 && bytes[0] == b'P' && bytes[1] == 0x05 && bytes[2] == 0x01
}

fn looks_like_clean_legacy_live_object_fragment(bytes: &[u8]) -> bool {
    if live_update::claim_payload_if_verified(bytes).is_some() {
        return false;
    }
    let mut probe = bytes.to_vec();
    if live_object::normalize_prefixed_fragments_payload_if_needed(&mut probe)
        .map(|summary| summary.dropped_leadin_bytes == 0 && !summary.salvaged_partial_leadin)
        .unwrap_or(false)
    {
        return true;
    }
    live_object::looks_like_legacy_prefixed_live_object_high_level(bytes)
}

fn append_pending_live_object_clean_fragment(
    state: &mut SessionState,
    first_sequence: u16,
    bytes: &[u8],
) {
    if bytes.len() < 7 {
        return;
    }
    let pending = state
        .live_object
        .pending_stream
        .get_or_insert_with(|| PendingLiveObjectStream {
            kind: PendingLiveObjectStreamKind::LegacyHighLevelFragmentPrefix,
            read_bytes: Vec::new(),
            fragment_bytes: Vec::new(),
            first_sequence,
            chunks: 0,
        });
    let mut normalized = bytes.to_vec();
    if let Some(summary) =
        live_object::normalize_prefixed_fragments_payload_if_needed(&mut normalized)
        && summary.dropped_leadin_bytes == 0
        && !summary.salvaged_partial_leadin
        && append_normalized_live_object_fragment(pending, &normalized)
    {
        pending.chunks = pending.chunks.saturating_add(1);
        return;
    }

    pending.fragment_bytes.extend_from_slice(&bytes[3..7]);
    pending.read_bytes.extend_from_slice(&bytes[7..]);
    pending.chunks = pending.chunks.saturating_add(1);
}

fn append_normalized_live_object_fragment(
    pending: &mut PendingLiveObjectStream,
    normalized: &[u8],
) -> bool {
    let Some(declared) = read_live_object_declared(normalized) else {
        return false;
    };
    if declared < 7 || declared > normalized.len() {
        return false;
    }

    pending
        .read_bytes
        .extend_from_slice(&normalized[7..declared]);
    pending
        .fragment_bytes
        .extend_from_slice(&normalized[declared..]);
    true
}

fn read_live_object_declared(payload: &[u8]) -> Option<usize> {
    // The CNW declared value is an absolute offset in the high-level payload:
    // `P/05/01` + four declared bytes + read-buffer bytes. It is not a length
    // to add after the header.
    let declared = u32::from_le_bytes(payload.get(3..7)?.try_into().ok()?);
    usize::try_from(declared).ok()
}

fn append_pending_live_object_prefixed_fragment(
    state: &mut SessionState,
    first_sequence: u16,
    bytes: &[u8],
    live_bytes_offset: usize,
) {
    if live_bytes_offset == 0 || live_bytes_offset > bytes.len() {
        return;
    }
    let pending = state
        .live_object
        .pending_stream
        .get_or_insert_with(|| PendingLiveObjectStream {
            kind: PendingLiveObjectStreamKind::RawPrefixedFragments,
            read_bytes: Vec::new(),
            fragment_bytes: Vec::new(),
            first_sequence,
            chunks: 0,
        });
    pending
        .fragment_bytes
        .extend_from_slice(&bytes[..live_bytes_offset]);
    pending
        .read_bytes
        .extend_from_slice(&bytes[live_bytes_offset..]);
    pending.chunks = pending.chunks.saturating_add(1);
}

fn append_pending_live_object_continuation(
    state: &mut SessionState,
    first_sequence: u16,
    bytes: &[u8],
    kind: PendingLiveObjectStreamKind,
) {
    let pending = state
        .live_object
        .pending_stream
        .get_or_insert_with(|| PendingLiveObjectStream {
            kind,
            read_bytes: Vec::new(),
            fragment_bytes: Vec::new(),
            first_sequence,
            chunks: 0,
        });
    pending.read_bytes.extend_from_slice(bytes);
    pending.chunks = pending.chunks.saturating_add(1);
}

fn flush_pending_live_object_stream_if_verified(
    state: &mut SessionState,
    bytes: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> bool {
    let Some(mut candidate) = build_pending_live_object_stream_payload(state) else {
        return false;
    };

    let mut claimed = live_update::rewrite_payload_to_exact_ee_if_possible(
        &mut candidate,
        latest_area_placeables,
    )
    .is_some();
    if live_update::claim_payload_if_verified(&candidate).is_none() {
        if let Some(pending) = state.live_object.pending_stream.as_ref() {
            dump_pending_live_object_candidate(
                &candidate,
                pending.first_sequence,
                pending.chunks,
                "pending-live-object-unclaimed",
            );
        }
        return false;
    }
    if let Some(summary) = live_update::canonicalize_player_session_creature_ids_payload_for_ee(
        &mut candidate,
        |compact_id| {
            state
                .semantic
                .objects
                .session_creature_id_for_compact(compact_id)
        },
    ) {
        claimed = true;
        tracing::info!(
            compact_add_ids_observed = summary.compact_add_ids_observed,
            add_ids_rewritten = summary.add_ids_rewritten,
            reference_ids_rewritten = summary.reference_ids_rewritten,
            "server live-object stream canonicalized PlayerList-proven session creature ids for EE"
        );
    }
    if let Some(summary) =
        live_update::canonicalize_compact_external_object_ids_payload_for_ee(&mut candidate)
    {
        claimed = true;
        tracing::info!(
            compact_add_ids_observed = summary.compact_add_ids_observed,
            add_ids_rewritten = summary.add_ids_rewritten,
            reference_ids_rewritten = summary.reference_ids_rewritten,
            "server live-object stream canonicalized compact Diamond external object ids for EE"
        );
    }
    if live_update::claim_payload_if_verified_with_lifecycle(
        &candidate,
        |object_type, object_id| {
            state
                .semantic
                .objects
                .has_active_live_object_for_record(object_type, object_id)
        },
    )
    .is_none()
    {
        if let Some(summary) = live_update::remove_unmaterialized_update_records_payload_if_possible(
            &mut candidate,
            |object_type, object_id| {
                state
                    .semantic
                    .objects
                    .has_active_live_object_for_record(object_type, object_id)
            },
        ) {
            claimed = true;
            tracing::info!(
                old_declared = summary.old_declared,
                new_declared = summary.new_declared,
                removed_update_records = summary.removed_update_records,
                diamond_missing_object_update_records =
                    summary.diamond_missing_object_update_records,
                diamond_missing_object_appearance_records =
                    summary.diamond_missing_object_appearance_records,
                ee_sentinel_inventory_owner_records = summary.ee_sentinel_inventory_owner_records,
                removed_bytes = summary.removed_bytes,
                removed_fragment_bits = summary.removed_fragment_bits,
                "server live-object stream removed Diamond no-op missing-object updates after exact lifecycle proof"
            );
        }
    }
    if live_update::claim_payload_if_verified_with_lifecycle(
        &candidate,
        |object_type, object_id| {
            state
                .semantic
                .objects
                .has_active_live_object_for_record(object_type, object_id)
        },
    )
    .is_none()
    {
        if let Some(pending) = state.live_object.pending_stream.as_ref() {
            dump_pending_live_object_candidate(
                &candidate,
                pending.first_sequence,
                pending.chunks,
                "pending-live-object-lifecycle-unverified",
            );
        }
        tracing::warn!(
            "server live-object stream candidate rejected: exact record shape passed but EE lifecycle proof failed"
        );
        return false;
    }

    if !claimed && HighLevel::parse(&candidate).is_none() {
        return false;
    }

    if let Some(pending) = state.live_object.pending_stream.as_ref() {
        dump_pending_live_object_candidate(
            &candidate,
            pending.first_sequence,
            pending.chunks,
            "pending-live-object-claimed",
        );
    }

    let _ = state.live_object.pending_stream.take();
    *bytes = candidate;
    true
}

fn build_pending_live_object_stream_payload(state: &SessionState) -> Option<Vec<u8>> {
    let pending = state.live_object.pending_stream.as_ref()?;
    build_live_object_stream_payload(pending)
}

fn build_live_object_stream_payload(pending: &PendingLiveObjectStream) -> Option<Vec<u8>> {
    if pending.read_bytes.is_empty() || pending.fragment_bytes.is_empty() {
        return None;
    }

    let declared_usize = 3usize
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(pending.read_bytes.len())?;
    let declared = u32::try_from(declared_usize).ok()?;
    let mut rebuilt = Vec::with_capacity(declared_usize + pending.fragment_bytes.len());
    rebuilt.push(b'P');
    rebuilt.push(0x05);
    rebuilt.push(0x01);
    rebuilt.extend_from_slice(&declared.to_le_bytes());
    rebuilt.extend_from_slice(&pending.read_bytes);
    rebuilt.extend_from_slice(&pending.fragment_bytes);
    Some(rebuilt)
}

fn prefixed_live_object_stream_continuation_prefix_len(bytes: &[u8]) -> Option<usize> {
    let Some(first) = bytes.first().copied() else {
        return None;
    };
    // Once a decompile-valid raw live-object stream is already pending, later
    // zlib windows can begin mid-record. HG captures keep the CNW fragment
    // storage bytes at the front of each such window. The observed Docks stream
    // uses a one-byte prefix (`A7`) for the first continuations and a three-byte
    // prefix (`FF A3 01`) for the later mid-record continuations. This function
    // only decides how much prefix storage to move; the rebuilt `P 05 01` still
    // has to pass the exact live-object semantic validator before emission.
    //
    // Re-audit: a continuation byte that already starts a decompile-recognized
    // live-object read-buffer submessage is not a proved fragment prefix. Moving
    // that opcode into the CNW tail would shift every following record cursor;
    // leave such chunks unclaimed unless another path proves a prefix owner.
    if starts_with_live_object_sub_message_boundary(bytes) {
        return None;
    }
    if first == 0 || bytes.len() <= 1 {
        return None;
    }
    if first == 0xFF {
        let prefix = bytes.get(0..3)?;
        if prefix.iter().any(|byte| *byte != 0xFF) && bytes.len() > 3 {
            return Some(3);
        }
        return None;
    }
    Some(1)
}

fn starts_with_live_object_sub_message_boundary(bytes: &[u8]) -> bool {
    crate::translate::live_object_update::looks_like_live_object_sub_message_boundary(bytes, 0)
}

fn dump_pending_live_object_candidate(
    candidate: &[u8],
    first_sequence: u16,
    chunks: u32,
    reason: &str,
) {
    // Pending live-object streams are speculative candidates assembled while
    // later zlib windows may still arrive. Keep both rejected intermediate
    // shapes and accepted fixture candidates under diagnostics/; complete
    // live-object family refusals are still dumped from server_dispatch.
    let dir = crate::translate::diagnostics::probe_dump_dir();
    let Some(dir) = dir else {
        return;
    };
    let mut path = dir;
    if fs::create_dir_all(&path).is_err() {
        return;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!(
        "{reason}-seq{first_sequence}-chunks{chunks}-{nanos}.bin"
    ));
    if fs::write(&path, candidate).is_ok() {
        tracing::info!(
            path = %path.display(),
            first_sequence,
            chunks,
            candidate_length = candidate.len(),
            reason,
            "server live-object pending stream candidate dumped for offline fixture analysis"
        );
    }
}

fn claim_server_zlib_stream_owner(state: &mut SessionState, owner: ContinuationOwner) {
    if state.deflate.server_zlib_stream_owner != Some(owner) {
        state.deflate.server_zlib_stream_epoch =
            state.deflate.server_zlib_stream_epoch.saturating_add(1);
    }
    state.deflate.server_zlib_stream_owner = Some(owner);
}

#[cfg(test)]
mod fixture_free_tests {
    use super::*;

    fn zero_declared_clean_fragment(live: &[u8], fragment: &[u8]) -> Vec<u8> {
        let mut payload = vec![
            b'P', 0x05, 0x01, // GameObjUpdate_LiveObject
            0x00, 0x00, 0x00, 0x00, // Diamond/HG zero-declared fragment prefix
        ];
        payload.extend_from_slice(live);
        payload.extend_from_slice(fragment);
        payload
    }

    #[test]
    fn normalized_clean_fragment_append_keeps_fragment_tail_out_of_read_bytes() {
        // Generalized from multi-window XP2 live-object evidence: a legacy
        // high-level fragment may carry the zero/invalid EE declared slot at
        // bytes 3..7 and a real CNW fragment-storage tail after the final
        // decompile-owned read-buffer row. Stream assembly must append the
        // normalized read/tail split for each chunk; otherwise per-chunk tail
        // bits become midstream live bytes and later shift add/update cursors.
        let live = vec![
            b'D', 0x05, 0x01, 0x00, 0x00, 0x80, // decompile-owned delete row
            b'W', 0x0C, 0x0E, // fragment-neutral work-remaining row
        ];
        let fragment = vec![0xA0];
        let declared = 7usize + live.len();
        let mut normalized = vec![
            b'P', 0x05, 0x01, // GameObjUpdate_LiveObject
        ];
        normalized.extend_from_slice(&(declared as u32).to_le_bytes());
        normalized.extend_from_slice(&live);
        normalized.extend_from_slice(&fragment);

        let mut pending = PendingLiveObjectStream {
            kind: PendingLiveObjectStreamKind::LegacyHighLevelFragmentPrefix,
            read_bytes: Vec::new(),
            fragment_bytes: Vec::new(),
            first_sequence: 19,
            chunks: 0,
        };
        assert!(append_normalized_live_object_fragment(
            &mut pending,
            &normalized
        ));

        assert_eq!(
            pending.read_bytes,
            vec![b'D', 0x05, 0x01, 0x00, 0x00, 0x80, b'W', 0x0C, 0x0E],
            "only decompile-owned live-object read bytes belong in the pending read stream"
        );
        assert_eq!(
            pending.fragment_bytes,
            vec![0xA0],
            "the per-chunk trailing CNW tail must not be stranded in read bytes"
        );
    }

    #[test]
    fn zero_declared_clean_fragment_chunks_keep_each_tail_out_of_read_bytes() {
        // Chunk-level variant of the XP2 stream-boundary evidence: each
        // high-level fragment starts with Diamond/HG's zero declared slot, then
        // a decompile-owned read-buffer row and a compact CNW tail. The pending
        // stream must normalize every chunk before appending it; otherwise a
        // prior chunk's tail becomes opcode-looking read bytes in the rebuilt
        // `P/05/01` stream.
        let first_live = [b'W', 0x01, 0x0E];
        let second_live = [b'W', 0x02, 0x0E];
        let first_chunk = zero_declared_clean_fragment(&first_live, &[0xA0]);
        let second_chunk = zero_declared_clean_fragment(&second_live, &[0x80]);

        let mut state = SessionState::default();
        append_pending_live_object_clean_fragment(&mut state, 19, &first_chunk);
        append_pending_live_object_clean_fragment(&mut state, 20, &second_chunk);

        let pending = state
            .live_object
            .pending_stream
            .as_ref()
            .expect("clean chunks should leave a pending live-object stream");
        assert_eq!(pending.chunks, 2);
        assert_eq!(
            pending.read_bytes,
            vec![b'W', 0x01, 0x0E, b'W', 0x02, 0x0E],
            "normalized read bytes must contain only decompile-owned W rows"
        );
        assert_eq!(
            pending.fragment_bytes,
            vec![0xA0, 0x80],
            "each chunk's compact CNW tail stays in fragment storage"
        );

        let rebuilt = build_pending_live_object_stream_payload(&state)
            .expect("read bytes and fragment bytes should rebuild a P/05/01 payload");
        let declared = read_live_object_declared(&rebuilt).expect("rebuilt payload has declared");
        assert_eq!(declared, 7 + first_live.len() + second_live.len());
        assert_eq!(&rebuilt[7..declared], &[b'W', 0x01, 0x0E, b'W', 0x02, 0x0E]);
        assert_eq!(&rebuilt[declared..], &[0xA0, 0x80]);
    }

    #[test]
    fn live_object_declared_offset_is_absolute_payload_offset() {
        // CNW `SetReadMessage` receives the declared value as the split point
        // between high-level read bytes and trailing fragment storage. Adding
        // the seven-byte `P/05/01` envelope a second time strands real fragment
        // bytes in the read buffer and shifts every later BOOL cursor.
        let read = [b'W', 0x0C, 0x0E];
        let fragment = [0xA0];
        let declared = 7 + read.len();
        let mut payload = vec![b'P', 0x05, 0x01];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&read);
        payload.extend_from_slice(&fragment);

        assert_eq!(read_live_object_declared(&payload), Some(declared));

        let mut pending = PendingLiveObjectStream {
            kind: PendingLiveObjectStreamKind::LegacyHighLevelFragmentPrefix,
            read_bytes: Vec::new(),
            fragment_bytes: Vec::new(),
            first_sequence: 17,
            chunks: 0,
        };
        assert!(append_normalized_live_object_fragment(
            &mut pending,
            &payload
        ));

        assert_eq!(pending.read_bytes, read);
        assert_eq!(pending.fragment_bytes, fragment);
        assert!(
            declared + 7 > payload.len(),
            "the old add-header interpretation would point past the real fragment split"
        );
    }

    #[test]
    fn raw_prefixed_continuation_does_not_strip_live_object_opcode_boundary() {
        // Raw-prefixed continuation repair is a stream-boundary rule, not a
        // cursor search. If the current chunk begins with a real live-object
        // submessage, the first byte is read-buffer data even though arbitrary
        // CNW fragment bytes can have the same value.
        let continuation = [
            b'U', 0x06, 0xB8, 0x00, 0x00, 0x80, // U/6 + legacy object id
            0x40, 0x00, 0x00, 0x00, // hidden-state update mask
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            None,
            "a decompile-recognized U/6 boundary must not be moved into fragment storage"
        );
    }

    #[test]
    fn raw_prefixed_continuation_does_not_strip_typed_item_create_boundary() {
        // The CEP v2.3 handoff under audit includes a typed item-create row
        // immediately before the disputed full item update. A continuation that
        // begins at `A/6 + OBJECTID` is already read-buffer data; moving the
        // opcode into CNW storage would invent a predecessor for the following
        // item's fragment cursor.
        let continuation = [
            b'A', 0x06, 0xB8, 0x00, 0x00, 0x80, // typed A/6 item create
            0x01, 0x00, 0x00, 0x00, // first item body bytes
            b'U', 0x06, 0xB8, 0x00, 0x00, 0x80,
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            None,
            "a decompile-recognized typed A/6 boundary must remain read-buffer data"
        );
    }

    #[test]
    fn raw_prefixed_continuation_does_not_strip_creature_appearance_boundary() {
        // `P/5 + OBJECTID + mask` is the decompiled creature appearance row
        // header. Raw continuation repair must not treat the leading `P` as a
        // fragment byte just because later exact validation might still reject
        // this partial stream.
        let continuation = [
            b'P', 0x05, 0xB8, 0x00, 0x00, 0x80, // P/5 creature appearance
            0x00, 0x00, // zero mask row
            b'W', 0x0C, 0x0E,
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            None,
            "a decompile-recognized P/5 appearance boundary must remain read-buffer data"
        );
    }

    #[test]
    fn raw_prefixed_continuation_does_not_strip_work_remaining_boundary() {
        // `W current total` is a decompile-owned, read-buffer-only live-object
        // record. A raw continuation that starts with W must not donate the
        // opcode byte to CNW fragment storage.
        let continuation = [
            b'W', 0x0C, 0x0E, // work remaining
            b'U', 0x06, 0xB8, 0x00, 0x00, 0x80,
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            None,
            "a decompile-recognized W boundary must remain read-buffer data"
        );
    }

    #[test]
    fn raw_prefixed_continuation_does_not_strip_gui_quickbar_boundary() {
        // `G Q count` rows are read-buffer-only GUI records in both Diamond and
        // EE. Treating the leading `G` as a one-byte raw prefix would shift the
        // GUI row and every later fragment cursor.
        let continuation = [
            b'G', b'Q', 0x00, // empty quickbar-link row block
            b'W', 0x0C, 0x0E,
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            None,
            "a decompile-recognized GQ boundary must remain read-buffer data"
        );
    }

    #[test]
    fn raw_prefixed_continuation_does_not_strip_inventory_boundary() {
        // Diamond `sub_455940` and EE `sub_1407B4F70` enter inventory rows from
        // the live-object read buffer at `I + OBJECTID + WORD mask`, then spend
        // mask-owned CNW BOOLs later. A continuation that begins at `I` is
        // therefore not a one-byte fragment prefix.
        let continuation = [
            b'I', 0xB8, 0x00, 0x00, 0x80, // inventory owner object id
            0x00, 0x00, // empty inventory mask
            b'W', 0x0C, 0x0E,
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            None,
            "a decompile-recognized inventory boundary must remain read-buffer data"
        );
    }

    #[test]
    fn raw_prefixed_continuation_does_not_strip_delete_boundary() {
        // Diamond `sub_455720` and EE `sub_1407B35B0` read delete rows as
        // `D/type/OBJECTID`; creature, item, and placeable deletes own one
        // following BOOL while trigger and door deletes own none. The leading
        // `D` still belongs to the read buffer, not raw fragment storage.
        let continuation = [
            b'D', 0x05, 0xB8, 0x00, 0x00, 0x80, // creature delete
            b'W', 0x0C, 0x0E,
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            None,
            "a decompile-recognized delete boundary must remain read-buffer data"
        );
    }

    #[test]
    fn raw_prefixed_continuation_keeps_observed_one_byte_prefix_shape() {
        // The Docks raw-prefixed path remains accepted when a non-boundary
        // fragment byte precedes the continuation read bytes. Exact
        // GameObjUpdate validation still owns the rebuilt stream before emit.
        let continuation = [
            0xA7, b'U', 0x06, 0xB8, 0x00, 0x00, 0x80, // prefix, then U/6
            0x40, 0x00, 0x00, 0x00,
        ];

        assert_eq!(
            prefixed_live_object_stream_continuation_prefix_len(&continuation),
            Some(1)
        );
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn complete_hg_town_npc_live_object_is_left_for_dispatcher() {
        let mut state = SessionState::default();
        let reassembly = ServerDeflatedReassembly {
            inflated_length: 579,
            expected_frames: 1,
            first_sequence: 38,
            packetized_sequence: 1,
            zlib_stream: true,
            frames: Vec::new(),
            interleaved_packets: Vec::new(),
        };
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_live_seq38_town_greeter_northern_trader_20260519.bin"
        )
        .to_vec();
        let original = payload.clone();

        let emit = maybe_buffer_or_flush_server_live_object_stream(
            &mut state,
            &reassembly,
            0,
            true,
            &mut payload,
        )
        .expect("stream inspection should not fail for complete high-level payload");

        assert!(
            emit.is_none(),
            "complete P/05/01 payloads should continue to server_dispatch"
        );
        assert_eq!(payload, original);
        assert!(state.live_object.pending_stream.is_none());
    }

    #[test]
    fn independent_complete_live_object_clears_stale_pending_stream() {
        let mut state = SessionState::default();
        state.live_object.pending_stream = Some(PendingLiveObjectStream {
            kind: PendingLiveObjectStreamKind::LegacyHighLevelFragmentPrefix,
            read_bytes: vec![b'A', 0x09, 0x84],
            fragment_bytes: vec![0x60],
            first_sequence: 16,
            chunks: 4,
        });
        let reassembly = ServerDeflatedReassembly {
            inflated_length: 579,
            expected_frames: 1,
            first_sequence: 24,
            packetized_sequence: 1,
            zlib_stream: true,
            frames: Vec::new(),
            interleaved_packets: Vec::new(),
        };
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_live_seq38_town_greeter_northern_trader_20260519.bin"
        )
        .to_vec();
        let original = payload.clone();

        let emit = maybe_buffer_or_flush_server_live_object_stream(
            &mut state,
            &reassembly,
            0,
            true,
            &mut payload,
        )
        .expect("stream inspection should not fail for complete high-level payload");

        assert!(emit.is_none());
        assert_eq!(payload, original);
        assert!(state.live_object.pending_stream.is_none());
    }

    #[test]
    fn local_cepv23_starter_tail_starts_at_declared_offset() {
        // Private CEP v2.3 starter evidence for the generalized declared-offset
        // rule above. The fragment tail starts at the absolute CNW declared
        // offset with `7A 63 23 AC...`; interpreting the declared value as
        // post-header length would skip into the middle of the real tail.
        let payload = include_bytes!(
            "../../../fixtures/live_object/local_cepv23_starter_seq17_lance_lute_patron_liveobject_20260523_unclaimed.bin"
        );
        let declared = read_live_object_declared(payload).expect("fixture has CNW declared offset");

        assert_eq!(declared, 393);
        assert_eq!(payload.len(), 411);
        assert_eq!(&payload[declared..declared + 4], &[0x7A, 0x63, 0x23, 0xAC]);
        assert_eq!(
            &payload[declared + 7..declared + 11],
            &[0x93, 0xA9, 0xC8, 0x39],
            "adding the envelope width skips into the middle of the real fragment tail"
        );
    }

    #[test]
    fn local_cepv23_starter_single_frame_is_left_for_dispatcher() {
        // The local Diamond harness run
        // `C:\nwnbridge\local-diamond-bridge-20260523-190505` logged seq17 as
        // one zlib M window: inflated=411, frames=1, compressed=210. The raw M
        // datagram pins transport provenance; this fixture is the already
        // inflated proxy-side payload handed to live_stream after inflater
        // handling. The disputed tail therefore is not a proxy chunk/
        // continuation boundary.
        let mut state = SessionState::default();
        let reassembly = ServerDeflatedReassembly {
            inflated_length: 411,
            expected_frames: 1,
            first_sequence: 17,
            packetized_sequence: 1,
            zlib_stream: true,
            frames: Vec::new(),
            interleaved_packets: Vec::new(),
        };
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_cepv23_starter_seq17_lance_lute_patron_liveobject_20260523_unclaimed.bin"
        )
        .to_vec();
        let original = payload.clone();

        let emit = maybe_buffer_or_flush_server_live_object_stream(
            &mut state,
            &reassembly,
            210,
            true,
            &mut payload,
        )
        .expect("stream inspection should not fail for complete high-level payload");

        assert!(
            emit.is_none(),
            "complete P/05/01 payloads should continue to server_dispatch"
        );
        assert_eq!(payload, original);
        assert!(state.live_object.pending_stream.is_none());
        assert!(!state.deflate.server_zlib_stream_proxy_owned);
    }
}
