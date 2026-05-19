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
    pending.fragment_bytes.extend_from_slice(&bytes[3..7]);
    pending.read_bytes.extend_from_slice(&bytes[7..]);
    pending.chunks = pending.chunks.saturating_add(1);
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
            tracing::warn!(
                old_declared = summary.old_declared,
                new_declared = summary.new_declared,
                removed_update_records = summary.removed_update_records,
                diamond_missing_object_update_records =
                    summary.diamond_missing_object_update_records,
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

fn take_pending_live_object_stream_payload(state: &mut SessionState) -> Option<Vec<u8>> {
    let pending = state.live_object.pending_stream.take()?;
    build_live_object_stream_payload(&pending)
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
}
