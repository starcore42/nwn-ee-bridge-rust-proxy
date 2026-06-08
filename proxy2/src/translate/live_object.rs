//! `GameObjUpdate_LiveObject` transport-shape and narrow semantic rewrites.
//!
//! Legacy 1.69 live-object update bursts can arrive as:
//!
//! `P 05 01 [four fragment bytes] [live-object read bytes...]`
//!
//! EE's `CNWMessage::SetReadMessage` path expects:
//!
//! `P 05 01 [u32 declared] [read bytes...] [fragment bytes...]`
//!
//! This module moves only verified/salvaged legacy fragment prefixes to the
//! CNW tail and performs small, decompile-backed EE shape inserts that belong
//! specifically to live-object payloads.
//!
//! EE decompile anchors used by the transforms below:
//!
//! - `sub_140781E80` gates creature visual-transform reads on update mask bit
//!   0x14, then calls `sub_140973160`.
//! - `sub_140973160` takes the EE-player build branch through
//!   `sub_1407C4AB0(..., 0x2001, 0x23)` and reads an object-level transform
//!   map as two keyed lists. The identity map is two zero DWORD counts.
//! - Door add `sub_140796DD0` reads the live-object id with `sub_1409737C0`,
//!   then one or two DWORDs, then the same visual-transform map before the
//!   door name payload.
//! - Placeable add `sub_1407A7800` reads the live-object id/name/tail fields,
//!   then the same visual-transform map after the legacy appearance tail.
//!
//! The identity-map insertion therefore emits the EE object-level empty map,
//! and only for complete creature add records whose fixed transform prefix ends
//! exactly where EE will begin reading the map, or for verified door/placeable
//! add records at the EE decompile-backed cursor.

use crate::translate::area::{AreaPlaceableContext, AreaPlaceableContextState};
use std::collections::HashSet;

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const LIVE_OBJECT_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
const LEGACY_LIVE_BYTES_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES: usize = 2048;
const MAX_LIVE_OBJECT_NAME_BYTES: usize = 128;
// Zlib-stream chunks can start in the middle of a legacy live-object record.
// The safe resync rule is not "small dropped lead-in"; it is "drop only until
// a decompile-legal live-object submessage is proven by the focused boundary
// classifier below." Keep the byte cap tied to the scan cap so a late `W`
// world-status record is still claimable without reintroducing envelope-only
// pass-through.
const MAX_SALVAGED_LEGACY_LIVE_LEADIN_BYTES: usize = MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES;
const CREATURE_OBJECT_TYPE: u8 = 0x05;
const ITEM_OBJECT_TYPE: u8 = 0x06;
const TRIGGER_OBJECT_TYPE: u8 = 0x07;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
const DOOR_OBJECT_TYPE: u8 = 0x0A;
const LEGACY_UPDATE_POSITION_MASK: u32 = 0x0000_0001;
const LEGACY_UPDATE_ORIENTATION_MASK: u32 = 0x0000_0002;
const LEGACY_UPDATE_SCALE_STATE_MASK: u32 = 0x0000_0004;
const LEGACY_UPDATE_STATE_MASK: u32 = 0x0000_0010;
const LEGACY_UPDATE_NAME_MASK: u32 = 0x0008_0000;
const LEGACY_UPDATE_HEADER_BYTES: usize = 10;
const LEGACY_CREATURE_APPEARANCE_HEADER_BYTES: usize = 8;
const LEGACY_CREATURE_UPDATE_3967_MASK: u32 = 0x0000_3967;
const MAX_APPEARANCE_FOLLOWING_CREATURE_FRAGMENT_SPAN_BYTES: usize = 256;
const LEGACY_UPDATE_POSITION_READ_BYTES: usize = 6;
const LEGACY_UPDATE_POSITION_FRAGMENT_BITS: usize = 2;
const LEGACY_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS: usize = 5;
const LEGACY_UPDATE_STATE_FRAGMENT_BITS: usize = 5;
const LEGACY_DOOR_PLACEABLE_GENERIC_UPDATE_TAIL_BYTES: usize = 9;
const MIN_AMBIGUOUS_TAIL_READ_BYTES: usize = 16;
const CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET: usize = 32;
const EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES: [u8;
    crate::translate::live_object_update::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN] =
    crate::translate::live_object_update::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES;

#[derive(Debug, Clone)]
pub struct LiveObjectNormalizeSummary {
    pub old_wire_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES],
    pub live_bytes_offset: usize,
    pub live_bytes_length: usize,
    pub dropped_leadin_bytes: usize,
    pub salvaged_partial_leadin: bool,
    pub first_record_end: usize,
}

#[derive(Debug, Clone)]
pub struct LiveObjectVisualTransformSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub old_live_bytes_length: usize,
    pub new_live_bytes_length: usize,
    pub old_fragment_bytes: usize,
    pub new_fragment_bytes: usize,
    pub records_examined: u32,
    pub maps_inserted: u32,
    pub bytes_inserted: u32,
    pub bytes_removed: u32,
    pub fragment_bits_trimmed: u32,
    pub legacy_door_model_tokens_removed: u32,
}

#[derive(Debug, Clone)]
pub struct LiveObjectContinuationWrapSummary {
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub dropped_leadin_bytes: usize,
    pub read_bytes_length: usize,
    pub fragment_bytes_length: usize,
    pub new_declared: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct RawPrefixedLiveObjectSplit {
    pub live_bytes_offset: usize,
    pub read_bytes_length: usize,
    pub fragment_bytes_length: usize,
}

#[derive(Debug, Clone)]
pub struct LiveObjectDeclaredLengthRepairCandidate {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub read_bytes_length: usize,
    pub fragment_bytes_length: usize,
}

pub fn raw_prefixed_live_object_split(payload: &[u8]) -> Option<RawPrefixedLiveObjectSplit> {
    if payload.len() < 3 || payload.first().copied() == Some(HIGH_LEVEL_ENVELOPE) {
        return None;
    }

    let live_bytes_offset = legacy_live_object_continuation_boundary_offset(payload)?;
    if live_bytes_offset == 0 || live_bytes_offset > LEGACY_PREFIXED_FRAGMENT_BYTES {
        return None;
    }
    if !payload[..live_bytes_offset]
        .iter()
        .any(|byte| *byte != 0 && *byte != 0xFF)
    {
        return None;
    }
    if !looks_like_legacy_live_object_sub_message_boundary(payload, live_bytes_offset) {
        return None;
    }

    Some(RawPrefixedLiveObjectSplit {
        live_bytes_offset,
        read_bytes_length: payload.len().checked_sub(live_bytes_offset)?,
        fragment_bytes_length: live_bytes_offset,
    })
}

pub fn looks_like_legacy_prefixed_live_object_high_level(payload: &[u8]) -> bool {
    if payload.len() < LEGACY_LIVE_BYTES_OFFSET + 1
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return false;
    }

    let Some(wire_declared) = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES) else {
        return false;
    };
    let Ok(wire_declared) = usize::try_from(wire_declared) else {
        return false;
    };
    if wire_declared >= HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES && wire_declared <= payload.len()
    {
        // EE's live-object reader reaches this branch through high-level
        // `P 05 01` with a CNW declared read-window, then the fragment tail.
        // Diamond/HG prefixed-fragment repair is only valid when bytes 3..7
        // are not a plausible declared window. Treating a valid declaration
        // as fragment storage makes the M stream buffer steal already-owned
        // live-object bursts and emit placeholders instead of real updates.
        return false;
    }

    if !looks_like_legacy_live_object_sub_message_boundary(payload, LEGACY_LIVE_BYTES_OFFSET) {
        return false;
    }

    let first_record_end = find_next_legacy_live_object_sub_message_boundary_after(
        payload,
        LEGACY_LIVE_BYTES_OFFSET,
        payload.len(),
    )
    .min(payload.len());
    first_record_end > LEGACY_LIVE_BYTES_OFFSET
}

pub fn declared_length_repair_candidates(
    payload: &[u8],
) -> Vec<LiveObjectDeclaredLengthRepairCandidate> {
    if payload.len() < LEGACY_LIVE_BYTES_OFFSET + 1
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return Vec::new();
    }

    let Some(old_declared) = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES) else {
        return Vec::new();
    };
    let Ok(old_declared_usize) = usize::try_from(old_declared) else {
        return Vec::new();
    };
    if old_declared_usize < LEGACY_LIVE_BYTES_OFFSET || old_declared_usize >= payload.len() {
        return Vec::new();
    }

    // Decompile discipline:
    //
    // EE's `CNWMessage::SetReadMessage` wants a declared read-buffer window
    // followed by CNW fragment storage. Some legacy live-object bursts carry a
    // stale/packetized declared value that either lands in the middle of a
    // legal `A/U/W` live-object read stream or overruns the decompile-proven
    // read cursor by a few CNW fragment-storage bytes. Do not trust the raw
    // declaration until the real read-window boundary has been searched.
    //
    // This function only proposes transport boundaries. Callers must still run
    // the focused semantic translators and exact `GameObjUpdate_LiveObject`
    // validator before emitting a repaired packet.
    let mut candidates = Vec::new();
    let mut seen_splits = HashSet::new();

    for split in decompile_owned_live_object_read_split_candidates(
        payload,
        LEGACY_LIVE_BYTES_OFFSET,
        payload.len(),
    ) {
        if let Some(candidate) = declared_length_repair_candidate_for_split(
            payload,
            old_declared,
            split,
            &mut seen_splits,
            true,
        ) {
            candidates.push(candidate);
        }
    }

    // Some stale-declared captures end a decompile-owned record with adjacent
    // CNW fragment storage rather than another live-object opcode. Search only
    // the compact physical tail proven by current HG/local evidence. The first
    // compact candidates also prove the legacy read-prefix walk here; later
    // tail hypotheses are transport-only and must pass dispatcher capacity
    // preflight plus exact semantic validation before any claim.
    const MAX_DECLARED_REPAIR_FRAGMENT_TAIL_SEARCH_BYTES: usize = 96;
    const MAX_DECLARED_REPAIR_FRAGMENT_TAIL_CANDIDATES: usize = 16;
    let max_tail = payload
        .len()
        .saturating_sub(LEGACY_LIVE_BYTES_OFFSET)
        .min(MAX_DECLARED_REPAIR_FRAGMENT_TAIL_SEARCH_BYTES);
    for tail_len in 1..=max_tail {
        let split = payload.len().saturating_sub(tail_len);
        if split <= LEGACY_LIVE_BYTES_OFFSET {
            break;
        }
        let require_prefix_walk = tail_len <= MAX_DECLARED_REPAIR_FRAGMENT_TAIL_CANDIDATES;
        if let Some(candidate) = declared_length_repair_candidate_for_split(
            payload,
            old_declared,
            split,
            &mut seen_splits,
            require_prefix_walk,
        ) {
            candidates.push(candidate);
        }
    }

    candidates
}

pub fn declared_length_window_transport_plausible(payload: &[u8]) -> bool {
    if payload.len() < LEGACY_LIVE_BYTES_OFFSET
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return false;
    }
    let Some(declared) = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)
        .and_then(|declared| usize::try_from(declared).ok())
    else {
        return false;
    };
    if declared <= LEGACY_LIVE_BYTES_OFFSET || declared >= payload.len() {
        return false;
    }
    decode_cnw_msb_valid_bits(&payload[declared..]).is_some()
        && !fragment_tail_contains_legacy_live_object_read_boundary(payload, declared)
        && live_object_read_prefix_walks_to(payload, LEGACY_LIVE_BYTES_OFFSET, declared)
}

fn declared_length_repair_candidate_for_split(
    payload: &[u8],
    old_declared: u32,
    split: usize,
    seen_splits: &mut HashSet<usize>,
    require_prefix_walk: bool,
) -> Option<LiveObjectDeclaredLengthRepairCandidate> {
    if split <= LEGACY_LIVE_BYTES_OFFSET || split >= payload.len() || !seen_splits.insert(split) {
        return None;
    }
    if decode_cnw_msb_valid_bits(&payload[split..]).is_none() {
        return None;
    }
    if require_prefix_walk
        && !live_object_read_prefix_walks_to(payload, LEGACY_LIVE_BYTES_OFFSET, split)
    {
        return None;
    }
    let Ok(new_declared) = u32::try_from(split) else {
        return None;
    };
    Some(LiveObjectDeclaredLengthRepairCandidate {
        old_declared,
        new_declared,
        old_payload_length: payload.len(),
        read_bytes_length: split - LEGACY_LIVE_BYTES_OFFSET,
        fragment_bytes_length: payload.len() - split,
    })
}

fn decompile_owned_live_object_read_split_candidates(
    bytes: &[u8],
    start: usize,
    scan_end: usize,
) -> Vec<usize> {
    let scan_end = scan_end.min(bytes.len());
    let mut offset = start;
    let mut splits = Vec::new();
    let mut records = 0usize;
    while offset < scan_end && records < 256 {
        if !looks_like_legacy_live_object_sub_message_boundary(bytes, offset) {
            break;
        }
        let record_end = declared_repair_read_record_end_for_transport(bytes, offset, scan_end);
        if record_end <= offset || record_end > scan_end {
            break;
        }
        splits.push(record_end);
        offset = record_end;
        records = records.saturating_add(1);
    }
    splits
}

fn zero_declared_live_object_tail_split(payload: &[u8], live_bytes_offset: usize) -> Option<usize> {
    if read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)? != 0
        || live_bytes_offset >= payload.len()
        || live_bytes_offset < LEGACY_LIVE_BYTES_OFFSET
    {
        return None;
    }

    // Decompile-backed zero-declared Diamond compatibility:
    //
    // EE `CNWMessage::SetReadMessage` treats bytes 3..7 as the declared CNW
    // read window. A zero DWORD underflows that reader and cannot be forwarded
    // to the EE client. Local Diamond captures show the zero DWORD followed by
    // a normal live-object read stream and a real CNW fragment tail at the end
    // of the packet. In that shape, the zero DWORD is an absent EE declaration,
    // not the fragment tail itself. Search only for a split whose read side is
    // a complete sequence of typed live-object records and whose suffix decodes
    // as CNW MSB fragment storage; the later semantic translators still must
    // prove the records before emit.
    const MAX_ZERO_DECLARED_FRAGMENT_TAIL_SEARCH_BYTES: usize = 1024;
    let max_tail = payload
        .len()
        .saturating_sub(live_bytes_offset)
        .min(MAX_ZERO_DECLARED_FRAGMENT_TAIL_SEARCH_BYTES);
    for tail_len in 1..=max_tail {
        let split = payload.len().saturating_sub(tail_len);
        if split <= live_bytes_offset {
            break;
        }
        let debug =
            std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() && tail_len <= 32;
        if decode_cnw_msb_valid_bits(&payload[split..]).is_none() {
            if debug {
                eprintln!(
                    "live-object zero-declared tail split rejected: reason=decode split={split} tail_len={tail_len} preview={:02X?}",
                    payload
                        .get(split..payload.len().min(split.saturating_add(16)))
                        .unwrap_or(&[])
                );
            }
            continue;
        }
        if fragment_tail_contains_legacy_live_object_read_boundary(payload, split) {
            if debug {
                eprintln!(
                    "live-object zero-declared tail split rejected: reason=tail-boundary split={split} tail_len={tail_len}"
                );
            }
            continue;
        }
        let Some(fragment_bits) = decode_cnw_msb_valid_bits(&payload[split..]) else {
            continue;
        };
        let walks = live_object_read_prefix_walks_to(payload, live_bytes_offset, split);
        let capacity = walks
            && live_object_read_prefix_has_plausible_fragment_capacity(
                payload,
                live_bytes_offset,
                split,
                &fragment_bits,
            );
        if debug {
            eprintln!(
                "live-object zero-declared tail split candidate: split={split} tail_len={tail_len} bits={} walks={walks} capacity={capacity}",
                fragment_bits.len()
            );
        }
        if walks && capacity {
            return Some(split);
        }
    }
    None
}

fn live_object_read_prefix_has_plausible_fragment_capacity(
    bytes: &[u8],
    start: usize,
    end: usize,
    bits: &[bool],
) -> bool {
    if start >= end || end > bytes.len() || bits.len() < CNW_FRAGMENT_HEADER_BITS {
        return false;
    }

    let mut offset = start;
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut pending_creature_appearance_update_proof: Option<(u32, usize)> = None;
    while offset < end {
        if !looks_like_legacy_live_object_sub_message_boundary(bytes, offset) {
            return false;
        }
        let min_record_len = minimum_legacy_live_object_record_length_at(bytes, offset);
        if end.saturating_sub(offset) < min_record_len {
            return false;
        }
        let mut record_end =
            find_next_legacy_live_object_sub_message_boundary_after(bytes, offset, end).min(end);
        if record_end <= offset || record_end > end {
            return false;
        }
        if bytes.get(offset).copied() == Some(HIGH_LEVEL_ENVELOPE)
            && bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
            && record_end < end
            && !looks_like_legacy_live_object_sub_message_boundary(bytes, record_end)
        {
            if let Some(span_end) =
                appearance_following_creature_update_fragment_span_end_for_transport(
                    bytes, record_end, end,
                )
            {
                record_end = span_end;
            }
        }
        match (bytes[offset], bytes[offset + 1]) {
            (b'A', ITEM_OBJECT_TYPE) => {
                if !crate::translate::live_object_update::advance_legacy_item_create_fragment_cursor_for_transport(
                    bytes,
                    offset.saturating_add(2),
                    record_end,
                    bits,
                    &mut bit_cursor,
                ) {
                    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                        eprintln!(
                            "live-object declared capacity rejected: reason=item-create offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
                        );
                    }
                    return false;
                }
            }
            (b'A', TRIGGER_OBJECT_TYPE | DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE) => {
                if !advance_live_add_record_bit_cursor(
                    bytes,
                    bits,
                    offset,
                    record_end,
                    &mut bit_cursor,
                ) {
                    return false;
                }
            }
            (b'U', DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE) => {
                if !advance_legacy_live_update_record_fragment_cursor_for_add_pass(
                    bytes,
                    bits,
                    offset,
                    record_end,
                    &mut bit_cursor,
                ) {
                    return false;
                }
            }
            (b'U', ITEM_OBJECT_TYPE) => {
                if !crate::translate::live_object_update::advance_legacy_item_update_fragment_cursor_for_transport(
                    bytes,
                    offset,
                    record_end,
                    bits,
                    &mut bit_cursor,
                ) {
                    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                        eprintln!(
                            "live-object declared capacity rejected: reason=item-update offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
                        );
                    }
                    return false;
                }
            }
            (b'U', TRIGGER_OBJECT_TYPE) => {
                if !crate::translate::live_object_update::advance_legacy_trigger_update_fragment_cursor_for_transport(
                    bytes,
                    bits,
                    offset,
                    record_end,
                    &mut bit_cursor,
                ) {
                    return false;
                }
            }
            (b'U', CREATURE_OBJECT_TYPE) => {
                if read_u32_le(bytes, offset.saturating_add(6))
                    == Some(LEGACY_CREATURE_UPDATE_3967_MASK)
                    && pending_creature_appearance_update_proof
                        .is_some_and(|(object_id, _)| {
                            read_u32_le(bytes, offset.saturating_add(2)) == Some(object_id)
                        })
                {
                    // A same-object creature appearance immediately before
                    // `U/5 0x3967` proves the inherited source cursor. Reuse
                    // the focused legacy/EE update simulators here instead of
                    // letting the update borrow unspent appearance bits.
                    let mut legacy_cursor = bit_cursor;
                    if crate::translate::live_object_update::advance_legacy_creature_update_fragment_cursor_for_transport(
                        bytes,
                        offset,
                        record_end,
                        bits,
                        &mut legacy_cursor,
                    ) {
                        bit_cursor = legacy_cursor;
                    } else {
                        let mut ee_cursor = bit_cursor;
                        if !crate::translate::live_object_update::advance_verified_creature_update_fragment_cursor_for_ee(
                            bytes,
                            offset,
                            record_end,
                            bits,
                            &mut ee_cursor,
                        ) {
                            if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                                eprintln!(
                                    "live-object declared capacity rejected: reason=creature-update offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
                                );
                            }
                            return false;
                        }
                        bit_cursor = ee_cursor;
                    }
                    pending_creature_appearance_update_proof = None;
                }
            }
            (b'P', CREATURE_OBJECT_TYPE) => {
                let short_partial_appearance =
                    partial_creature_appearance_claim_for_transport_record(
                        bytes,
                        offset,
                        record_end,
                        bits,
                        bit_cursor,
                    );
                if let Some(claim) = short_partial_appearance {
                    if claim.fragment_bits > bits.len().saturating_sub(bit_cursor) {
                        return false;
                    }
                    bit_cursor = bit_cursor.saturating_add(claim.fragment_bits);
                    pending_creature_appearance_update_proof =
                        read_u32_le(bytes, offset.saturating_add(2)).map(|object_id| {
                            (object_id, bit_cursor)
                        });
                } else {
                    let mut appearance_cursor = bit_cursor;
                    if crate::translate::live_object_update::advance_legacy_creature_appearance_fragment_cursor_for_transport(
                        bytes,
                        offset,
                        record_end,
                        bits,
                        &mut appearance_cursor,
                    ) {
                        bit_cursor = appearance_cursor;
                        pending_creature_appearance_update_proof =
                            read_u32_le(bytes, offset.saturating_add(2)).map(|object_id| {
                                (object_id, bit_cursor)
                            });
                    } else if read_u16_le(bytes, offset.saturating_add(6))
                        .is_some_and(|mask| mask != 0xFFFF)
                    {
                        return false;
                    }
                }
            }
            (b'D', object_type) if matches!(object_type, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => {
                let Some(bit_count) =
                    legacy_live_delete_fragment_bit_count(bytes, offset, record_end)
                else {
                    return false;
                };
                if bits.len().saturating_sub(bit_cursor) < bit_count {
                    return false;
                }
                bit_cursor = bit_cursor.saturating_add(bit_count);
            }
            (b'G', _) => {
                let Some(proven_record_end) =
                    crate::translate::live_object_update::legacy_live_gui_record_end_for_transport(
                        bytes, offset, end, bits, bit_cursor,
                    )
                else {
                    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                        eprintln!(
                            "live-object zero-declared GUI capacity rejected: reason=no-proven-end offset={offset} record_end={record_end} end={end} bit_cursor={bit_cursor}"
                        );
                    }
                    return false;
                };
                let zero_fragment_padding_after_gui =
                    proven_record_end < record_end
                        && crate::translate::live_object_update::
                            legacy_live_gui_zero_fragment_storage_span_for_transport(
                                bytes,
                                proven_record_end,
                                record_end,
                            );
                if proven_record_end != record_end && !zero_fragment_padding_after_gui {
                    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                        eprintln!(
                            "live-object zero-declared GUI capacity rejected: reason=end-mismatch offset={offset} record_end={record_end} proven_record_end={proven_record_end} end={end} bit_cursor={bit_cursor}"
                        );
                    }
                    return false;
                }
                if !crate::translate::live_object_update::advance_legacy_live_gui_fragment_cursor_for_transport(
                    bytes,
                    offset,
                    proven_record_end,
                    bits,
                    &mut bit_cursor,
                ) {
                    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                        eprintln!(
                            "live-object zero-declared GUI capacity rejected: reason=advance offset={offset} record_end={proven_record_end} end={end} bit_cursor={bit_cursor}"
                        );
                    }
                    return false;
                }
            }
            _ => {}
        }

        if !(bytes[offset] == b'P' && bytes[offset + 1] == CREATURE_OBJECT_TYPE) {
            pending_creature_appearance_update_proof = None;
        }
        offset = record_end;
    }

    offset == end
}

pub fn declared_length_repair_tail_contains_live_object_read_boundary(
    bytes: &[u8],
    repair: &LiveObjectDeclaredLengthRepairCandidate,
) -> bool {
    let Ok(tail_start) = usize::try_from(repair.new_declared) else {
        return false;
    };
    fragment_tail_contains_legacy_live_object_read_boundary(bytes, tail_start)
}

pub fn declared_length_repair_read_window_ends_after_creature_appearance_update_pair(
    bytes: &[u8],
    repair: &LiveObjectDeclaredLengthRepairCandidate,
) -> bool {
    let Ok(split) = usize::try_from(repair.new_declared) else {
        return false;
    };
    if split <= LEGACY_LIVE_BYTES_OFFSET || split > bytes.len() {
        return false;
    }

    // This is not a semantic claim. It only allows the dispatcher to spend a
    // bounded exact-rewrite attempt on an otherwise ambiguous stale-declared
    // split whose read side ends exactly after the decompile-owned creature
    // appearance/update pairing. When this transport helper can prove the
    // inherited CNW cursor through the P row, the terminal U row must validate
    // from that exact cursor before the split is eligible. Older full-appearance
    // capture paths can still be unsupported at this layer; those retain the
    // final semantic rewrite plus exact EE validator as the emitter gate.
    let Some(fragment_tail) = bytes.get(split..) else {
        return false;
    };
    let Some(fragment_bits) = decode_cnw_msb_valid_bits(fragment_tail) else {
        return false;
    };
    let live = &bytes[LEGACY_LIVE_BYTES_OFFSET..split];
    let mut pending_creature_appearance_update_proof: Option<u32> = None;
    let mut offset = 0usize;
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut bit_cursor_reliable = true;
    let mut records = 0usize;
    while offset + LEGACY_UPDATE_HEADER_BYTES <= live.len() && records < 64 {
        if !looks_like_legacy_live_object_sub_message_boundary(live, offset) {
            bit_cursor_reliable = false;
            offset = offset.saturating_add(1);
            continue;
        }

        let next =
            find_next_legacy_live_object_sub_message_boundary_after(live, offset, live.len())
                .min(live.len());
        if next <= offset {
            break;
        }
        let opcode = live[offset];
        let object_type = live[offset + 1];
        let object_id = read_u32_le(live, offset + 2).unwrap_or(u32::MAX);
        let adjacent_appearance_object_id = pending_creature_appearance_update_proof.take();
        if object_type == CREATURE_OBJECT_TYPE && object_id != u32::MAX {
            if opcode == b'P' {
                let mut appearance_cursor = bit_cursor;
                if !crate::translate::live_object_update::advance_legacy_creature_appearance_fragment_cursor_for_transport(
                    live,
                    offset,
                    next,
                    &fragment_bits,
                    &mut appearance_cursor,
                ) {
                    bit_cursor_reliable = false;
                } else {
                    bit_cursor = appearance_cursor;
                    pending_creature_appearance_update_proof = Some(object_id);
                }
            } else if opcode == b'U'
                && adjacent_appearance_object_id == Some(object_id)
                && read_u32_le(live, offset.saturating_add(6))
                    == Some(LEGACY_CREATURE_UPDATE_3967_MASK)
                && next == live.len()
            {
                if !bit_cursor_reliable {
                    return true;
                }
                let mut legacy_update_cursor = bit_cursor;
                let mut ee_update_cursor = bit_cursor;
                if crate::translate::live_object_update::advance_legacy_creature_update_fragment_cursor_for_transport(
                    live,
                    offset,
                    next,
                    &fragment_bits,
                    &mut legacy_update_cursor,
                ) || crate::translate::live_object_update::advance_verified_creature_update_fragment_cursor_for_ee(
                    live,
                    offset,
                    next,
                    &fragment_bits,
                    &mut ee_update_cursor,
                ) {
                    return true;
                }
                return false;
            } else if opcode == b'U' {
                let mut legacy_update_cursor = bit_cursor;
                let mut ee_update_cursor = bit_cursor;
                if !crate::translate::live_object_update::advance_legacy_creature_update_fragment_cursor_for_transport(
                    live,
                    offset,
                    next,
                    &fragment_bits,
                    &mut legacy_update_cursor,
                ) && !crate::translate::live_object_update::advance_verified_creature_update_fragment_cursor_for_ee(
                    live,
                    offset,
                    next,
                    &fragment_bits,
                    &mut ee_update_cursor,
                ) {
                    bit_cursor_reliable = false;
                    offset = next;
                    records = records.saturating_add(1);
                    continue;
                }
                bit_cursor = if legacy_update_cursor != bit_cursor {
                    legacy_update_cursor
                } else {
                    ee_update_cursor
                };
            }
        }
        offset = next;
        records = records.saturating_add(1);
    }

    false
}

pub fn declared_length_repair_creature_appearance_update_read_split_candidate(
    payload: &[u8],
) -> Option<LiveObjectDeclaredLengthRepairCandidate> {
    if payload.len() < LEGACY_LIVE_BYTES_OFFSET + 1
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let mut seen_splits = HashSet::new();
    decompile_owned_live_object_read_split_candidates(
        payload,
        LEGACY_LIVE_BYTES_OFFSET,
        payload.len(),
    )
    .into_iter()
    .filter_map(|split| {
        declared_length_repair_candidate_for_split(
            payload,
            old_declared,
            split,
            &mut seen_splits,
            true,
        )
    })
    .filter(|candidate| {
        declared_length_repair_tail_contains_live_object_read_boundary(payload, candidate)
            && declared_length_repair_fragment_capacity_plausible(payload, candidate)
            && declared_length_repair_read_window_ends_after_creature_appearance_update_pair(
                payload, candidate,
            )
    })
    .max_by_key(|candidate| candidate.read_bytes_length)
}

pub fn declared_length_repair_fragment_capacity_plausible(
    bytes: &[u8],
    repair: &LiveObjectDeclaredLengthRepairCandidate,
) -> bool {
    let Ok(tail_start) = usize::try_from(repair.new_declared) else {
        return false;
    };
    if tail_start <= LEGACY_LIVE_BYTES_OFFSET || tail_start >= bytes.len() {
        return false;
    }
    let Some(fragment_bits) = decode_cnw_msb_valid_bits(&bytes[tail_start..]) else {
        return false;
    };

    // EE seeds `GameObjUpdate_LiveObject` with one read-buffer window and one
    // compact fragment stream. A stale-declared repair candidate is impossible
    // if the proposed fragment tail cannot supply the fragment BOOLs consumed
    // by the typed legacy read prefix. This is only a preflight; exact ownership
    // is still decided by the focused semantic rewriters plus EE validator.
    live_object_read_prefix_has_plausible_fragment_capacity(
        bytes,
        LEGACY_LIVE_BYTES_OFFSET,
        tail_start,
        &fragment_bits,
    )
}

fn fragment_tail_contains_legacy_live_object_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    // Decompile-backed strictness guard:
    //
    // EE enters `GameObjUpdate_LiveObject` by seeding `CNWMessage::SetReadMessage`
    // with one byte window and one compact fragment/BOOL tail. The fragment tail
    // is not another live-object read stream. If a proposed stale-declared split
    // would leave bytes that still look like legacy `A/U/P/W/...` live-object
    // submessage boundaries, the packet is ambiguous and must be owned by the
    // typed live-object stream translators instead of this transport repair.
    //
    // This deliberately prefers quarantine over truncating a later real record
    // into "fragment" bytes. The Starcore5 post-area stream exposed exactly that
    // failure mode: a short exact prefix validated, while a later `P/5` creature
    // appearance record was stranded in the alleged fragment tail.
    if tail_start >= bytes.len() {
        return false;
    }

    let mut offset = tail_start;
    while offset + 1 < bytes.len() {
        if crate::translate::live_object_update::legacy_live_gui_character_sheet_read_boundary_without_fragment_proof(
            bytes,
            offset,
            bytes.len(),
        ) {
            // `G S` CharacterSheet rows can be shorter than, equal to, or longer
            // than the generic ambiguous-tail floor while still owning CNW BOOLs
            // after a decompiled read-buffer body. When one is left inside the
            // proposed CNW tail, the split is still a live GUI read-boundary
            // ambiguity until the focused character-sheet reader proves the row.
            return true;
        }

        if fragment_tail_starts_with_aligned_short_live_object_read_boundary(bytes, offset) {
            // Diamond/EE short read-buffer rows such as `W current total`,
            // zero-row `GQ`, and delete records are shorter than the broad
            // ambiguous-tail scanner below. When one is left inside the
            // proposed CNW tail, the split is still a live-object read
            // boundary, not fragment storage.
            return true;
        }

        if bytes.len().saturating_sub(offset) >= MIN_AMBIGUOUS_TAIL_READ_BYTES
            && looks_like_legacy_live_object_sub_message_boundary(bytes, offset)
        {
            return true;
        }
        offset += 1;
    }
    false
}

fn fragment_tail_starts_with_aligned_short_live_object_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    if tail_start >= bytes.len()
        || bytes.len().saturating_sub(tail_start) >= MIN_AMBIGUOUS_TAIL_READ_BYTES
    {
        return false;
    }

    if is_work_remaining_record_at(bytes, tail_start) {
        return true;
    }

    if fragment_tail_starts_with_aligned_short_inventory_read_boundary(bytes, tail_start) {
        return true;
    }

    if fragment_tail_starts_with_aligned_short_add_read_boundary(bytes, tail_start) {
        return true;
    }

    if fragment_tail_starts_with_aligned_short_creature_appearance_read_boundary(bytes, tail_start)
    {
        return true;
    }

    if fragment_tail_starts_with_aligned_short_creature_body_part_delta_read_boundary(
        bytes, tail_start,
    ) {
        return true;
    }

    if fragment_tail_starts_with_aligned_short_creature_equipment_delta_read_boundary(
        bytes, tail_start,
    ) {
        return true;
    }

    if fragment_tail_starts_with_aligned_short_gui_read_boundary(bytes, tail_start) {
        return true;
    }

    if fragment_tail_starts_with_aligned_short_update_read_boundary(bytes, tail_start) {
        return true;
    }

    let Some(delete_end) = tail_start.checked_add(6) else {
        return false;
    };
    // EE `sub_1407B35B0` and Diamond's matching delete dispatch read exactly
    // `D/type/OBJECTID`. Creature, item, and placeable deletes also own one
    // following fragment BOOL; trigger and door deletes own none. Either way,
    // the six read-buffer bytes are not compact fragment storage.
    delete_end <= bytes.len()
        && legacy_live_delete_fragment_bit_count(bytes, tail_start, delete_end).is_some()
}

fn fragment_tail_starts_with_aligned_short_gui_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    // Diamond `sub_4589A0` and EE `sub_1407B3F30` own short `G/Q`,
    // `G/I D|U`, and `G/R D|U|M` rows entirely in the read buffer. A stale
    // declared split at one of those opcodes is therefore still a live GUI
    // boundary, even when the byte sequence also decodes as compact CNW bits.
    crate::translate::live_object_update::legacy_live_gui_record_end_for_transport(
        bytes,
        tail_start,
        bytes.len(),
        &[],
        0,
    )
    .is_some_and(|gui_end| gui_end > tail_start && gui_end <= bytes.len())
}

fn fragment_tail_starts_with_aligned_short_inventory_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    bytes.get(tail_start).copied() == Some(b'I')
        && crate::translate::live_object_update::legacy_inventory_fragment_bit_count_for_transport(
            bytes,
            tail_start,
            bytes.len(),
        )
        .is_some()
}

fn fragment_tail_starts_with_aligned_short_add_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    // Compact Diamond placeable adds can be only 15 read-buffer bytes:
    // `A/09 + OBJECTID + four-byte legacy name/token + BYTE/WORD/WORD tail`.
    // Short Diamond door adds can be only 12 read-buffer bytes:
    // `A/0A + OBJECTID + nonzero door DWORD + WORD state tail`.
    // The focused add translator owns the fragment guards and EE visual-map
    // insertion later; stale-declared repair must not first classify either
    // compact row as CNW fragment storage merely because it is below the broad
    // ambiguous-tail floor.
    crate::translate::live_object_update::try_get_legacy_placeable_short_name_add_record_end_for_transport(
        bytes,
        tail_start,
        bytes.len(),
    ) == Some(bytes.len())
        || crate::translate::live_object_update::try_get_legacy_short_door_add_record_end_for_transport(
            bytes,
            tail_start,
            bytes.len(),
        ) == Some(bytes.len())
}

fn fragment_tail_starts_with_aligned_short_creature_appearance_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    // Diamond `sub_448E30` and EE `sub_14077FE10` both read `P/5 + OBJECTID +
    // WORD mask` before the mask-gated appearance body. Mask zero is therefore
    // an eight-byte read-buffer-only record and must not be stolen as a compact
    // CNW fragment tail.
    crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
        bytes,
        tail_start,
        bytes.len(),
    )
    .is_some_and(|appearance_end| appearance_end > tail_start && appearance_end <= bytes.len())
}

fn fragment_tail_starts_with_aligned_short_creature_body_part_delta_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    short_creature_body_part_delta_read_end_for_transport(bytes, tail_start, bytes.len()).is_some()
}

fn short_creature_body_part_delta_read_end_for_transport(
    bytes: &[u8],
    tail_start: usize,
    scan_end: usize,
) -> Option<usize> {
    if let Some(claim) =
        partial_creature_appearance_claim_for_transport(bytes, tail_start, scan_end, None, 0, true)
    {
        return claim.has_body_delta.then_some(claim.record_end);
    }
    None
}

fn partial_creature_appearance_read_end_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    partial_creature_appearance_claim_for_transport(bytes, offset, scan_end, None, 0, false)
        .map(|claim| claim.record_end)
}

fn fragment_tail_starts_with_aligned_short_creature_equipment_delta_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    short_creature_equipment_delta_read_end_for_transport(bytes, tail_start, bytes.len()).is_some()
}

fn short_creature_equipment_delta_read_end_for_transport(
    bytes: &[u8],
    tail_start: usize,
    scan_end: usize,
) -> Option<usize> {
    if let Some(claim) =
        partial_creature_appearance_claim_for_transport(bytes, tail_start, scan_end, None, 0, true)
    {
        return claim.has_equipment_delta.then_some(claim.record_end);
    }
    legacy_zero_equipment_delta_read_end_for_transport(bytes, tail_start, scan_end)
}

fn legacy_zero_equipment_delta_read_end_for_transport(
    bytes: &[u8],
    tail_start: usize,
    scan_end: usize,
) -> Option<usize> {
    const LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK: u16 = 0x0200;
    const LEGACY_APPEARANCE_LEGACY_SKIPPED_FEATURE_0E_MASK: u16 = 0x4000;
    const LEGACY_APPEARANCE_SCALAR_AND_TAIL_MASKS: u16 = 0x0001
        | 0x0002
        | 0x0004
        | 0x0008
        | 0x0010
        | 0x0020
        | 0x0040
        | 0x0080
        | 0x0800
        | 0x1000
        | 0x2000
        | LEGACY_APPEARANCE_LEGACY_SKIPPED_FEATURE_0E_MASK;
    let scan_end = scan_end.min(bytes.len());

    let Some(mask_offset) = tail_start.checked_add(6) else {
        return None;
    };
    if bytes.get(tail_start).copied() != Some(b'P')
        || bytes.get(tail_start + 1).copied() != Some(CREATURE_OBJECT_TYPE)
        || !looks_like_legacy_live_object_id_at(bytes, tail_start + 2)
    {
        return None;
    }
    let Some(mask) = read_u16_le(bytes, mask_offset) else {
        return None;
    };
    if (mask & LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK) == 0
        || (mask
            & !(LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK | LEGACY_APPEARANCE_SCALAR_AND_TAIL_MASKS))
            != 0
    {
        return None;
    }

    let Some(cursor) = tail_start
        .checked_add(LEGACY_CREATURE_APPEARANCE_HEADER_BYTES)
        .and_then(|cursor| advance_short_creature_appearance_scalar_cursor(cursor, mask))
    else {
        return None;
    };
    let Some(count) = bytes.get(cursor).copied() else {
        return None;
    };
    if count != 0 {
        return None;
    }
    let Some(record_end) = cursor.checked_add(1) else {
        return None;
    };

    // Diamond `sub_448E30` and EE `sub_14077FE10` reach the equipment-delta
    // branch after scalar fields, the post-body 0x2000 WORD+DWORD tail, and the
    // legacy-skipped 0x4000 feature byte: mask bit 0x0200 first owns a BYTE
    // count, then each nonzero entry owns CHAR opcode + OBJECTID + DWORD
    // slot/field before any opcode-specific item body. A zero-count delta is
    // therefore a read-buffer row with no fragment BOOLs. The typed appearance
    // parser/writer owns the bounded equipment-delta list; stale-declared repair
    // must not steal the aligned short zero-count form as CNW fragment storage.
    (record_end <= scan_end
        && record_end > tail_start
        && record_end.saturating_sub(tail_start) < MIN_AMBIGUOUS_TAIL_READ_BYTES)
        .then_some(record_end)
}

#[derive(Debug, Clone, Copy)]
struct PartialCreatureAppearanceTransportClaim {
    record_end: usize,
    fragment_bits: usize,
    has_body_delta: bool,
    has_equipment_delta: bool,
}

#[derive(Debug, Clone, Copy)]
struct PartialCreatureAppearanceNameClaim {
    cursor: usize,
    fragment_bits: usize,
}

fn partial_creature_appearance_claim_for_transport_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<PartialCreatureAppearanceTransportClaim> {
    let claim = partial_creature_appearance_claim_for_transport(
        bytes,
        offset,
        record_end,
        Some(fragment_bits),
        bit_cursor,
        false,
    )?;
    (claim.record_end == record_end).then_some(claim)
}

fn partial_creature_appearance_claim_for_transport(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    fragment_bits: Option<&[bool]>,
    bit_cursor: usize,
    require_short: bool,
) -> Option<PartialCreatureAppearanceTransportClaim> {
    const LEGACY_APPEARANCE_BODY_PART_MASK: u16 = 0x0100;
    const LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK: u16 = 0x0200;
    const LEGACY_APPEARANCE_NAME_MASK: u16 = 0x0400;
    const LEGACY_APPEARANCE_POST_BODY_WORD_DWORD_MASK: u16 = 0x2000;
    const LEGACY_APPEARANCE_LEGACY_SKIPPED_FEATURE_0E_MASK: u16 = 0x4000;
    const LEGACY_APPEARANCE_IGNORED_HIGH_MASK: u16 = 0x8000;
    const LEGACY_APPEARANCE_SCALAR_MASKS: u16 =
        0x0001 | 0x0002 | 0x0004 | 0x0008 | 0x0010 | 0x0020 | 0x0040 | 0x0080 | 0x0800 | 0x1000;
    // Mask bit 0x8000 is an explicit zero-payload branch for transport
    // accounting: Diamond `sub_448E30` and EE `sub_14077FE10` both proceed
    // from the post-body/tail branches to equipment-delta `0x0200` without a
    // `0x8000` read.
    const LEGACY_APPEARANCE_PARTIAL_TRANSPORT_MASKS: u16 = LEGACY_APPEARANCE_BODY_PART_MASK
        | LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK
        | LEGACY_APPEARANCE_NAME_MASK
        | LEGACY_APPEARANCE_POST_BODY_WORD_DWORD_MASK
        | LEGACY_APPEARANCE_LEGACY_SKIPPED_FEATURE_0E_MASK
        | LEGACY_APPEARANCE_IGNORED_HIGH_MASK
        | LEGACY_APPEARANCE_SCALAR_MASKS;
    const LEGACY_APPEARANCE_FULL_BODY_PART_COUNT: usize = 0x13;

    let scan_end = scan_end.min(bytes.len());
    let mask_offset = offset.checked_add(6)?;
    if offset.checked_add(LEGACY_CREATURE_APPEARANCE_HEADER_BYTES)? > scan_end
        || bytes.get(offset).copied()? != b'P'
        || bytes.get(offset + 1).copied()? != CREATURE_OBJECT_TYPE
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    let mask = read_u16_le(bytes, mask_offset)?;
    let has_body_delta = (mask & LEGACY_APPEARANCE_BODY_PART_MASK) != 0;
    let has_equipment_delta = (mask & LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK) != 0;
    if mask == 0xFFFF
        || (!has_body_delta && !has_equipment_delta)
        || (mask & !LEGACY_APPEARANCE_PARTIAL_TRANSPORT_MASKS) != 0
    {
        return None;
    }

    let name_start = offset.checked_add(LEGACY_CREATURE_APPEARANCE_HEADER_BYTES)?;
    let mut accepted: Option<PartialCreatureAppearanceTransportClaim> = None;
    for name in partial_creature_appearance_name_claims_for_transport(
        bytes,
        name_start,
        scan_end,
        mask,
        fragment_bits,
        bit_cursor,
    ) {
        let Some(mut cursor) =
            advance_short_creature_appearance_pre_body_scalar_cursor(name.cursor, mask)
        else {
            continue;
        };
        if cursor > scan_end {
            continue;
        }

        if has_body_delta {
            let Some(selector) = bytes.get(cursor).copied() else {
                continue;
            };
            let body_bytes = if selector == 0 {
                1usize
            } else if selector < 0x0A {
                1usize.saturating_add(usize::from(selector).saturating_mul(2))
            } else {
                1usize.saturating_add(LEGACY_APPEARANCE_FULL_BODY_PART_COUNT)
            };
            let Some(next_cursor) = cursor.checked_add(body_bytes) else {
                continue;
            };
            cursor = next_cursor;
        }

        let Some(next_cursor) = advance_short_creature_appearance_post_body_cursor(cursor, mask)
        else {
            continue;
        };
        cursor = next_cursor;

        if has_equipment_delta {
            let Some(count) = bytes.get(cursor).copied() else {
                continue;
            };
            if count != 0 {
                continue;
            }
            let Some(next_cursor) = cursor.checked_add(1) else {
                continue;
            };
            cursor = next_cursor;
        }

        if cursor > scan_end || cursor <= offset {
            continue;
        }
        if require_short && cursor.saturating_sub(offset) >= MIN_AMBIGUOUS_TAIL_READ_BYTES {
            continue;
        }
        if cursor < scan_end && !looks_like_legacy_live_object_sub_message_boundary(bytes, cursor) {
            continue;
        }

        // Diamond `sub_448E30` and EE `sub_14077FE10` read mask bit 0x0400
        // before the scalar/body/equipment branches. This transport-only claim
        // counts those name BOOLs, then advances the same byte-owned partial
        // body/equipment rows that the semantic appearance parser later rewrites
        // or validates. It is deliberately not itself a semantic rewrite.
        let claim = PartialCreatureAppearanceTransportClaim {
            record_end: cursor,
            fragment_bits: name.fragment_bits,
            has_body_delta,
            has_equipment_delta,
        };
        if accepted
            .as_ref()
            .map(|current| claim.record_end < current.record_end)
            .unwrap_or(true)
        {
            accepted = Some(claim);
        }
    }
    accepted
}

fn partial_creature_appearance_name_claims_for_transport(
    bytes: &[u8],
    cursor: usize,
    scan_end: usize,
    mask: u16,
    fragment_bits: Option<&[bool]>,
    bit_cursor: usize,
) -> Vec<PartialCreatureAppearanceNameClaim> {
    const LEGACY_APPEARANCE_NAME_MASK: u16 = 0x0400;
    if (mask & LEGACY_APPEARANCE_NAME_MASK) == 0 {
        return vec![PartialCreatureAppearanceNameClaim {
            cursor,
            fragment_bits: 0,
        }];
    }

    let mut claims = Vec::new();
    if let Some(fragment_bits) = fragment_bits {
        match fragment_bits.get(bit_cursor).copied() {
            Some(false) => {
                if let Some(end) = bounded_inline_cexo_string_end(cursor, bytes, scan_end) {
                    claims.push(PartialCreatureAppearanceNameClaim {
                        cursor: end,
                        fragment_bits: 1,
                    });
                }
            }
            Some(true) => {
                if let Some(first) = advance_partial_creature_locstring_component_for_transport(
                    bytes,
                    cursor,
                    scan_end,
                    fragment_bits,
                    bit_cursor.saturating_add(1),
                ) {
                    if let Some(second) = advance_partial_creature_locstring_component_for_transport(
                        bytes,
                        first.cursor,
                        scan_end,
                        fragment_bits,
                        bit_cursor
                            .saturating_add(1)
                            .saturating_add(first.fragment_bits),
                    ) {
                        claims.push(PartialCreatureAppearanceNameClaim {
                            cursor: second.cursor,
                            fragment_bits: 1usize
                                .saturating_add(first.fragment_bits)
                                .saturating_add(second.fragment_bits),
                        });
                    }
                }
            }
            None => {}
        }
        return claims;
    }

    // Proofless transport scanning uses only the direct CExoString branch. The
    // locstring-pair branch has a minimum eight-byte read body, so named
    // partial body/equipment rows that are short enough to need this ambiguity
    // guard are necessarily direct names; longer locstring rows are handled by
    // the generic boundary floor and by the proof-backed capacity pass above.
    if let Some(end) = bounded_inline_cexo_string_end(cursor, bytes, scan_end) {
        claims.push(PartialCreatureAppearanceNameClaim {
            cursor: end,
            fragment_bits: 1,
        });
    }
    claims
}

fn bounded_inline_cexo_string_end(cursor: usize, bytes: &[u8], scan_end: usize) -> Option<usize> {
    let end = inline_cexo_string_end(bytes, cursor)?;
    (end <= scan_end).then_some(end)
}

fn advance_partial_creature_locstring_component_for_transport(
    bytes: &[u8],
    cursor: usize,
    scan_end: usize,
    fragment_bits: &[bool],
    component_bit_cursor: usize,
) -> Option<PartialCreatureAppearanceNameClaim> {
    let token_component = *fragment_bits.get(component_bit_cursor)?;
    if token_component {
        fragment_bits.get(component_bit_cursor.checked_add(1)?)?;
        let end = cursor.checked_add(4)?;
        return (end <= scan_end).then_some(PartialCreatureAppearanceNameClaim {
            cursor: end,
            fragment_bits: 2,
        });
    }

    let end = bounded_inline_cexo_string_end(cursor, bytes, scan_end)?;
    Some(PartialCreatureAppearanceNameClaim {
        cursor: end,
        fragment_bits: 1,
    })
}

fn advance_short_creature_appearance_pre_body_scalar_cursor(
    mut cursor: usize,
    mask: u16,
) -> Option<usize> {
    if (mask & 0x0001) != 0 {
        cursor = cursor.checked_add(2)?;
    }
    if (mask & 0x0002) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0004) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0080) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0800) != 0 {
        cursor = cursor.checked_add(4)?;
    }
    if (mask & 0x1000) != 0 {
        cursor = cursor.checked_add(4)?;
    }
    if (mask & 0x0008) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0010) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0020) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0040) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    Some(cursor)
}

fn advance_short_creature_appearance_post_body_cursor(
    mut cursor: usize,
    mask: u16,
) -> Option<usize> {
    if (mask & 0x2000) != 0 {
        cursor = cursor.checked_add(2 + 4)?;
    }
    Some(cursor)
}

fn advance_short_creature_appearance_scalar_cursor(mut cursor: usize, mask: u16) -> Option<usize> {
    cursor = advance_short_creature_appearance_pre_body_scalar_cursor(cursor, mask)?;
    advance_short_creature_appearance_post_body_cursor(cursor, mask)
}

fn fragment_tail_starts_with_aligned_short_update_read_boundary(
    bytes: &[u8],
    tail_start: usize,
) -> bool {
    if bytes.get(tail_start).copied() != Some(b'U') {
        return false;
    }
    let Some(min_end) = tail_start.checked_add(LEGACY_UPDATE_HEADER_BYTES) else {
        return false;
    };
    if min_end > bytes.len() {
        return false;
    }

    // These calls are ambiguity detectors, not claims. A stale-declared split
    // has not yet exposed the real fragment stream, so use false placeholder
    // bits only to ask whether a focused update reader can own the short
    // read-buffer bytes. Exact emit still requires the real final validator.
    let placeholder_bits = vec![false; 64];
    let max_end = tail_start
        .checked_add(MIN_AMBIGUOUS_TAIL_READ_BYTES.saturating_sub(1))
        .unwrap_or(bytes.len())
        .min(bytes.len());
    for record_end in min_end..=max_end {
        let object_type = bytes.get(tail_start + 1).copied();
        if object_type == Some(CREATURE_OBJECT_TYPE) {
            let mut bit_cursor = 0usize;
            if crate::translate::live_object_update::advance_verified_creature_update_fragment_cursor_for_ee(
                bytes,
                tail_start,
                record_end,
                &placeholder_bits,
                &mut bit_cursor,
            ) {
                return true;
            }
        } else if object_type == Some(ITEM_OBJECT_TYPE) {
            let mut bit_cursor = 0usize;
            if crate::translate::live_object_update::advance_verified_item_update_fragment_cursor_for_ee(
                bytes,
                tail_start,
                record_end,
                &placeholder_bits,
                &mut bit_cursor,
            ) {
                return true;
            }
        } else if matches!(object_type, Some(PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)) {
            let mut bit_cursor = 0usize;
            if crate::translate::live_object_update::advance_verified_door_placeable_update_fragment_cursor_for_ee(
                bytes,
                tail_start,
                record_end,
                &placeholder_bits,
                &mut bit_cursor,
            ) {
                return true;
            }
        }
    }
    false
}

fn is_work_remaining_record_at(bytes: &[u8], offset: usize) -> bool {
    crate::translate::live_object_update::looks_like_work_remaining_record_at(bytes, offset)
}

pub fn wrap_legacy_live_object_continuation_payload_if_plausible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectContinuationWrapSummary> {
    if let Some(summary) = wrap_raw_legacy_live_object_prefixed_fragment_payload(payload) {
        return Some(summary);
    }

    // Strict-mode discipline: a zlib-inflated blob without a high-level packet
    // header is not, by itself, a packet. Earlier development builds wrapped
    // any plausible live-object-looking continuation into `P 05 01` with a
    // synthetic one-byte fragment tail, but live driver-only runs still showed
    // EE `Unknown Update sub-message` after those deliveries. That means this
    // was acting as a fallback classifier, not a decompile-proven semantic
    // translator.
    //
    // Keep the old implementation below for focused fixture work, but leave it
    // disabled unless a future exact continuation translator can prove record
    // boundaries and fragment-bit ownership for the entire synthesized payload.
    if std::env::var_os("HGBRIDGE_PROXY2_ENABLE_RAW_LIVE_CONTINUATION_WRAP").is_none() {
        let _ = payload;
        return None;
    }

    if payload.len() < 16 || payload.first().copied() == Some(HIGH_LEVEL_ENVELOPE) {
        return None;
    }

    let boundary_offset = legacy_live_object_continuation_boundary_offset(payload)?;
    let source = &payload[boundary_offset..];
    if source.len() < 8 {
        return None;
    }

    // Decompile discipline:
    // EE enters `CNWSMessage::HandleGameObjUpdate` only through high-level
    // family 0x0501, then seeds `CNWMessage::SetReadMessage` from a declared
    // byte-buffer length followed by fragment bits. Some 1.69/HG zlib-stream
    // windows arrive as raw live-object read bytes with no high-level envelope.
    // Emit the narrowest valid EE shape for that verified continuation instead
    // of passing the raw blob through or silently consuming it.
    //
    // The continuation has no trustworthy standalone fragment cursor, so keep
    // one byte as CNW fragment storage. This is intentionally conservative:
    // the exact typed live-object record parsers should grow this into a
    // decompile-derived fragment-boundary decision per opcode.
    const CONTINUATION_FRAGMENT_BYTES: usize = 1;
    if source.len() <= CONTINUATION_FRAGMENT_BYTES {
        return None;
    }
    let read_bytes_length = source.len() - CONTINUATION_FRAGMENT_BYTES;
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + read_bytes_length;
    let new_declared = u32::try_from(new_declared_usize).ok()?;

    let old_payload_length = payload.len();
    let mut rewritten =
        Vec::with_capacity(HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + source.len());
    rewritten.push(HIGH_LEVEL_ENVELOPE);
    rewritten.push(GAME_OBJECT_UPDATE_MAJOR);
    rewritten.push(LIVE_OBJECT_MINOR);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&source[..read_bytes_length]);
    rewritten.extend_from_slice(&source[read_bytes_length..]);

    let summary = LiveObjectContinuationWrapSummary {
        old_payload_length,
        new_payload_length: rewritten.len(),
        dropped_leadin_bytes: boundary_offset,
        read_bytes_length,
        fragment_bytes_length: CONTINUATION_FRAGMENT_BYTES,
        new_declared,
    };
    *payload = rewritten;
    Some(summary)
}

fn wrap_raw_legacy_live_object_prefixed_fragment_payload(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectContinuationWrapSummary> {
    // Decompile-backed transport normalization:
    // Diamond-era live-object traffic can place the CNW fragment storage before
    // the live-object read buffer, while EE's `CNWMessage::SetReadMessage`
    // path reaches `HandleGameObjUpdate` through high-level `P 05 01` with a
    // declared byte read window followed by fragment bytes. When a server zlib
    // stream chunk begins after the high-level envelope, the inflated bytes can
    // therefore look like:
    //
    //   [legacy fragment prefix bytes] [A/U/P/D/I/G/W live-object records...]
    //
    // This is not a semantic claim by itself. It only rebuilds the EE envelope
    // and moves the verified leading fragment prefix to the tail; the focused
    // live-object translators/validators must still claim the resulting
    // `P 05 01` packet before strict mode emits it.
    if payload.len() < 3 || payload.first().copied() == Some(HIGH_LEVEL_ENVELOPE) {
        return None;
    }

    let split = raw_prefixed_live_object_split(payload)?;
    let live_bytes_offset = split.live_bytes_offset;

    let read_bytes_length = payload.len().checked_sub(live_bytes_offset)?;
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(read_bytes_length)?;
    let new_declared = u32::try_from(new_declared_usize).ok()?;

    let old_payload_length = payload.len();
    let mut rewritten = Vec::with_capacity(new_declared_usize + live_bytes_offset);
    rewritten.push(HIGH_LEVEL_ENVELOPE);
    rewritten.push(GAME_OBJECT_UPDATE_MAJOR);
    rewritten.push(LIVE_OBJECT_MINOR);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&payload[live_bytes_offset..]);
    rewritten.extend_from_slice(&payload[..live_bytes_offset]);

    let summary = LiveObjectContinuationWrapSummary {
        old_payload_length,
        new_payload_length: rewritten.len(),
        dropped_leadin_bytes: 0,
        read_bytes_length,
        fragment_bytes_length: live_bytes_offset,
        new_declared,
    };
    *payload = rewritten;
    Some(summary)
}

pub fn normalize_prefixed_fragments_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectNormalizeSummary> {
    if payload.len() < LEGACY_LIVE_BYTES_OFFSET + 1
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let old_wire_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if old_wire_declared >= (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u32
        && (old_wire_declared as usize) <= payload.len()
    {
        return None;
    }
    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] = payload
        [HIGH_LEVEL_HEADER_BYTES..LEGACY_LIVE_BYTES_OFFSET]
        .try_into()
        .ok()?;
    let mut live_bytes_offset = LEGACY_LIVE_BYTES_OFFSET;
    let first_record_end;
    let mut salvaged_partial_leadin = false;
    if !looks_like_legacy_live_object_sub_message_boundary(payload, live_bytes_offset) {
        let salvaged =
            find_salvageable_legacy_live_object_boundary_after_prefixed_fragments(payload)?;
        if salvaged.0 <= LEGACY_LIVE_BYTES_OFFSET {
            return None;
        }
        let dropped_leadin = salvaged.0 - LEGACY_LIVE_BYTES_OFFSET;
        if dropped_leadin > MAX_SALVAGED_LEGACY_LIVE_LEADIN_BYTES {
            return None;
        }
        live_bytes_offset = salvaged.0;
        first_record_end = salvaged.1;
        salvaged_partial_leadin = true;
    } else {
        // Decompile-backed transport normalization:
        // EE `CNWMessage::SetReadMessage` subtracts three from the first DWORD
        // after the high-level header and rejects values that overflow the
        // read buffer. A zero DWORD is therefore not a valid EE declared
        // length. Local Diamond captures show `P 05 01 00 00 00 00` followed
        // by valid `A/U` live-object records, which means those bytes are
        // legacy-prefixed CNW fragment storage. Do not reject the all-zero
        // prefix here; this pass only moves the prefix to the EE fragment tail,
        // and the focused live-object semantic validator still has to prove
        // every record before the router may emit the packet.
        first_record_end = find_next_legacy_live_object_sub_message_boundary_after(
            payload,
            live_bytes_offset,
            payload.len(),
        )
        .min(payload.len());
    }

    let zero_declared_tail_start = zero_declared_live_object_tail_split(payload, live_bytes_offset);
    let live_bytes_end = zero_declared_tail_start.unwrap_or(payload.len());
    let live_bytes_length = live_bytes_end - live_bytes_offset;
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes_length;
    let new_declared = u32::try_from(new_declared_usize).ok()?;

    let mut rewritten = Vec::with_capacity(payload.len() + CNW_LENGTH_BYTES);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&payload[live_bytes_offset..live_bytes_end]);
    if let Some(fragment_tail_start) = zero_declared_tail_start {
        rewritten.extend_from_slice(&payload[fragment_tail_start..]);
    } else {
        rewritten.extend_from_slice(&prefixed_fragment_bytes);
    }

    let old_payload_length = payload.len();
    let new_payload_length = rewritten.len();
    let dropped_leadin_bytes = live_bytes_offset - LEGACY_LIVE_BYTES_OFFSET;
    *payload = rewritten;

    Some(LiveObjectNormalizeSummary {
        old_wire_declared,
        new_declared,
        old_payload_length,
        new_payload_length,
        prefixed_fragment_bytes,
        live_bytes_offset,
        live_bytes_length,
        dropped_leadin_bytes,
        salvaged_partial_leadin,
        first_record_end,
    })
}

pub fn rewrite_creature_add_visual_transform_maps_if_possible(
    payload: &mut Vec<u8>,
    area_context: Option<&AreaPlaceableContext>,
) -> Option<LiveObjectVisualTransformSummary> {
    rewrite_creature_add_visual_transform_maps_inner(payload, area_context, false)
}

pub fn rewrite_creature_add_visual_transform_maps_after_update_if_possible(
    payload: &mut Vec<u8>,
    area_context: Option<&AreaPlaceableContext>,
) -> Option<LiveObjectVisualTransformSummary> {
    rewrite_creature_add_visual_transform_maps_inner(payload, area_context, true)
}

fn rewrite_creature_add_visual_transform_maps_inner(
    payload: &mut Vec<u8>,
    area_context: Option<&AreaPlaceableContext>,
    prefer_verified_update_cursor: bool,
) -> Option<LiveObjectVisualTransformSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let declared = usize::try_from(old_declared).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES || declared > payload.len() {
        return None;
    }

    let mut live_bytes = payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared].to_vec();
    let mut fragment_bytes = payload[declared..].to_vec();
    if !live_bytes.is_empty() && !looks_like_legacy_live_object_sub_message_boundary(&live_bytes, 0)
    {
        // EE and Diamond both enter the live-object reader with the CNW read
        // cursor positioned at a live-object submessage opcode.  A valid
        // high-level `P 05 01` envelope whose declared read window begins with
        // non-record bytes is therefore a stream/reassembly ownership problem,
        // not permission for this add-family translator to scan forward and
        // patch later door/placeable-looking bytes out of context.
        return None;
    }
    let mut fragment_bits = decode_cnw_msb_valid_bits(&fragment_bytes);
    let mut fragment_bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut fragment_bits_reliable = fragment_bits.is_some();
    let mut fragment_bits_trim_safe = true;
    let mut fragment_bits_changed = false;
    let old_live_bytes_length = live_bytes.len();
    let old_fragment_bytes = fragment_bytes.len();
    let mut records_examined = 0u32;
    let mut maps_inserted = 0u32;
    let mut bytes_inserted = 0u32;
    let mut bytes_removed = 0u32;
    let mut fragment_bits_trimmed = 0u32;
    let mut legacy_door_model_tokens_removed = 0u32;
    let mut offset = 0usize;

    while offset + 10 <= live_bytes.len() {
        if !looks_like_legacy_live_object_sub_message_boundary(&live_bytes, offset) {
            offset += 1;
            continue;
        }

        records_examined = records_examined.saturating_add(1);
        let mut record_end = find_next_legacy_live_object_sub_message_boundary_after(
            &live_bytes,
            offset,
            live_bytes.len(),
        )
        .min(live_bytes.len());
        if let Some(compact_placeable_end) =
            compact_placeable_add_end_before_same_object_update_like_tail(
                &live_bytes,
                offset,
                live_bytes.len(),
            )
            .filter(|compact_end| *compact_end < record_end)
        {
            record_end = compact_placeable_end;
        }
        if fragment_bits_reliable
            && live_bytes.get(offset).copied() == Some(b'P')
            && live_bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
        {
            if let Some(bits) = fragment_bits.as_ref() {
                if let Some(verified_end) =
                    crate::translate::live_object_update::try_get_verified_creature_appearance_record_end_for_ee(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                        bits,
                        fragment_bit_cursor,
                    )
                {
                    record_end = verified_end;
                }
            }
        }
        if record_end <= offset {
            offset += 1;
            continue;
        }

        if fragment_bits_reliable {
            if let Some(bits) = fragment_bits.as_mut() {
                let before_fragment_bits_len = bits.len();
                let before_fragment_bit_cursor = fragment_bit_cursor;
                if let Some(record_rewrite) = rewrite_legacy_door_placeable_add_record_for_ee(
                    &mut live_bytes,
                    &mut record_end,
                    bits,
                    &mut fragment_bit_cursor,
                    offset,
                    area_context,
                ) {
                    maps_inserted = maps_inserted.saturating_add(record_rewrite.maps_inserted);
                    bytes_inserted = bytes_inserted.saturating_add(record_rewrite.bytes_inserted);
                    bytes_removed = bytes_removed.saturating_add(record_rewrite.bytes_removed);
                    legacy_door_model_tokens_removed = legacy_door_model_tokens_removed
                        .saturating_add(record_rewrite.legacy_door_model_tokens_removed);
                    fragment_bits_changed |= record_rewrite.fragment_bits_changed;
                    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some()
                        && record_rewrite.fragment_bits_changed
                    {
                        eprintln!(
                            "live-object visual add fragment rewrite applied: offset={offset} record_end={record_end} bit_cursor_before={before_fragment_bit_cursor} bit_cursor_after={fragment_bit_cursor} bits_len_before={before_fragment_bits_len} bits_len_after={} rewrite={record_rewrite:?}",
                            bits.len(),
                        );
                    }
                    offset = record_end.max(offset + 1);
                    continue;
                }
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some()
                    && live_bytes.get(offset).copied() == Some(b'A')
                    && matches!(
                        live_bytes.get(offset + 1).copied(),
                        Some(DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE)
                    )
                {
                    eprintln!(
                        "live-object visual door/placeable rewrite skipped: offset={offset} record_end={record_end} bit_cursor={fragment_bit_cursor} next_bits={:?} preview={:02X?}",
                        bits.get(
                            fragment_bit_cursor
                                ..fragment_bit_cursor.saturating_add(12).min(bits.len())
                        )
                        .unwrap_or(&[]),
                        live_bytes
                            .get(offset..offset.saturating_add(96).min(live_bytes.len()))
                            .unwrap_or(&[])
                    );
                }
                if live_bytes.get(offset).copied() == Some(b'U')
                    && matches!(
                        live_bytes.get(offset + 1).copied(),
                        Some(DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | ITEM_OBJECT_TYPE)
                    )
                {
                    if prefer_verified_update_cursor {
                        let mut verified_cursor = fragment_bit_cursor;
                        if crate::translate::live_object_update::advance_verified_door_placeable_update_fragment_cursor_for_ee(
                            &live_bytes,
                            offset,
                            record_end,
                            bits,
                            &mut verified_cursor,
                        ) {
                            fragment_bit_cursor = verified_cursor;
                            offset = record_end.max(offset + 1);
                            continue;
                        }
                    }

                    // This pass is still walking the Diamond/HG source stream so
                    // it can insert EE visual-transform maps into following add
                    // records. Door updates are especially sensitive here:
                    // Diamond `sub_44E2C0` reads five state BOOLs for mask
                    // `0x10`, while EE `sub_140797780` reads six. The focused
                    // update translator inserts that EE-only sixth false bit
                    // later. Advancing with the EE exact cursor here steals the
                    // first fragment bit from the next add record and makes the
                    // visual-map pass drift.
                    if advance_legacy_live_update_record_fragment_cursor_for_add_pass(
                        &live_bytes,
                        bits,
                        offset,
                        record_end,
                        &mut fragment_bit_cursor,
                    ) {
                        fragment_bits_trim_safe = false;
                        offset = record_end.max(offset + 1);
                        continue;
                    }
                }
                if live_bytes.get(offset).copied() == Some(b'A')
                    && live_bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
                {
                    if let Some(insert_offset) =
                        legacy_add_visual_transform_insert_offset(&live_bytes, offset, record_end)
                    {
                        if let Some(patch) = patch_creature_add_visual_transform_identity_for_ee(
                            &mut live_bytes,
                            insert_offset,
                            &mut record_end,
                        ) {
                            // Diamond `sub_4489F0` and EE `sub_14077F870`
                            // read the same fixed creature-add prefix:
                            // OBJECTID, six 32-bit FLOAT slots, and a WORD.
                            // EE then immediately calls the object visual
                            // transform reader (`sub_140973160`).  This
                            // branch is intentionally before the generic
                            // fragment-cursor advance below, otherwise a
                            // compact but valid 32-byte Diamond creature add
                            // can be skipped as "known" before the EE empty
                            // transform map is inserted.
                            maps_inserted = maps_inserted.saturating_add(1);
                            bytes_inserted = bytes_inserted.saturating_add(patch.bytes_inserted);
                            bytes_removed = bytes_removed.saturating_add(patch.bytes_removed);
                            offset = record_end;
                            continue;
                        }
                    }
                }
                if let Some(trim_safe) = advance_known_live_record_fragment_cursor_for_ee(
                    &live_bytes,
                    bits,
                    offset,
                    record_end,
                    &mut fragment_bit_cursor,
                ) {
                    fragment_bits_trim_safe &= trim_safe;
                    offset = record_end.max(offset + 1);
                    continue;
                }
            } else {
                fragment_bits_reliable = false;
            }
        }

        let Some(insert_offset) =
            legacy_add_visual_transform_insert_offset(&live_bytes, offset, record_end)
        else {
            if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                eprintln!(
                    "live-object visual add pass cursor lost: offset={offset} record_end={record_end} opcode=0x{:02X} marker=0x{:02X} bit_cursor={} preview={:02X?}",
                    live_bytes.get(offset).copied().unwrap_or_default(),
                    live_bytes.get(offset + 1).copied().unwrap_or_default(),
                    fragment_bit_cursor,
                    live_bytes
                        .get(offset..offset.saturating_add(96).min(live_bytes.len()))
                        .unwrap_or(&[])
                );
            }
            fragment_bits_reliable = false;
            offset = record_end.max(offset + 1);
            continue;
        };

        if has_ee_identity_visual_transform_map_at(&live_bytes, insert_offset, record_end) {
            offset = record_end.max(offset + 1);
            continue;
        }

        let Some(patch) = patch_creature_add_visual_transform_identity_for_ee(
            &mut live_bytes,
            insert_offset,
            &mut record_end,
        ) else {
            fragment_bits_reliable = false;
            offset = record_end.max(offset + 1);
            continue;
        };
        maps_inserted = maps_inserted.saturating_add(1);
        bytes_inserted = bytes_inserted.saturating_add(patch.bytes_inserted);
        bytes_removed = bytes_removed.saturating_add(patch.bytes_removed);
        offset = record_end;
    }

    let add_record_semantic_changed = maps_inserted != 0
        || bytes_inserted != 0
        || bytes_removed != 0
        || legacy_door_model_tokens_removed != 0
        || fragment_bits_changed;
    if add_record_semantic_changed && fragment_bits_reliable && fragment_bits_trim_safe {
        if let Some(bits) = fragment_bits.as_mut() {
            if fragment_bit_cursor >= CNW_FRAGMENT_HEADER_BITS && fragment_bit_cursor < bits.len() {
                fragment_bits_trimmed = (bits.len() - fragment_bit_cursor) as u32;
                bits.truncate(fragment_bit_cursor);
                fragment_bits_changed = true;
            }
        }
    }

    if maps_inserted == 0
        && !fragment_bits_changed
        && live_bytes.len() == old_live_bytes_length
        && bytes_removed == 0
        && legacy_door_model_tokens_removed == 0
    {
        return None;
    }

    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes.len();
    let new_declared = u32::try_from(new_declared_usize).ok()?;
    if fragment_bits_changed {
        fragment_bytes = pack_cnw_msb_valid_bits(fragment_bits?);
    }
    let mut rewritten = Vec::with_capacity(new_declared_usize + fragment_bytes.len());
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&live_bytes);
    rewritten.extend_from_slice(&fragment_bytes);

    let summary = LiveObjectVisualTransformSummary {
        old_declared,
        new_declared,
        old_payload_length: payload.len(),
        new_payload_length: rewritten.len(),
        old_live_bytes_length,
        new_live_bytes_length: live_bytes.len(),
        old_fragment_bytes,
        new_fragment_bytes: fragment_bytes.len(),
        records_examined,
        maps_inserted,
        bytes_inserted,
        bytes_removed,
        fragment_bits_trimmed,
        legacy_door_model_tokens_removed,
    };
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!("live-object visual rewrite summary before emit: {summary:?}");
    }
    *payload = rewritten;
    Some(summary)
}

fn find_salvageable_legacy_live_object_boundary_after_prefixed_fragments(
    payload: &[u8],
) -> Option<(usize, usize)> {
    if payload.len() < 16
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let scan_end = payload
        .len()
        .min(LEGACY_LIVE_BYTES_OFFSET + MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES);
    for candidate in LEGACY_LIVE_BYTES_OFFSET..scan_end.saturating_sub(1) {
        if !looks_like_salvageable_legacy_live_object_record_at(payload, candidate, scan_end) {
            continue;
        }
        let record_end = find_next_legacy_live_object_sub_message_boundary_after(
            payload,
            candidate,
            payload.len(),
        )
        .min(payload.len());
        return Some((candidate, record_end));
    }
    None
}

fn live_object_read_prefix_walks_to(bytes: &[u8], start: usize, end: usize) -> bool {
    if start >= end || end > bytes.len() {
        return false;
    }

    let mut offset = start;
    let mut records = 0usize;
    while offset < end {
        if !looks_like_legacy_live_object_sub_message_boundary(bytes, offset) {
            return false;
        }
        let record_end = declared_repair_read_record_end_for_transport(bytes, offset, end);
        if record_end <= offset || record_end > end {
            return false;
        }
        records = records.saturating_add(1);
        offset = record_end;
    }

    records != 0 && offset == end
}

fn declared_repair_read_record_end_for_transport(bytes: &[u8], offset: usize, end: usize) -> usize {
    if bytes.get(offset).copied() == Some(b'U')
        && bytes.get(offset + 1).copied() == Some(TRIGGER_OBJECT_TYPE)
    {
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_verified_trigger_update_record_end_for_transport(
                bytes,
                offset,
                bytes.len(),
            )
        {
            return record_end;
        }
    }

    let record_end =
        find_next_legacy_live_object_sub_message_boundary_after(bytes, offset, end).min(end);
    if bytes.get(offset).copied() == Some(HIGH_LEVEL_ENVELOPE)
        && bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
        && record_end < end
        && !looks_like_legacy_live_object_sub_message_boundary(bytes, record_end)
    {
        if let Some(span_end) = appearance_following_creature_update_fragment_span_end_for_transport(
            bytes, record_end, end,
        ) {
            return span_end;
        }
    }
    record_end
}

fn appearance_following_creature_update_fragment_span_end_for_transport(
    bytes: &[u8],
    span_start: usize,
    end: usize,
) -> Option<usize> {
    if span_start >= end
        || span_start >= bytes.len()
        || looks_like_legacy_live_object_sub_message_boundary(bytes, span_start)
    {
        return None;
    }

    let scan_end = span_start
        .checked_add(MAX_APPEARANCE_FOLLOWING_CREATURE_FRAGMENT_SPAN_BYTES)?
        .min(end)
        .min(bytes.len());
    for span_end in span_start.checked_add(1)?..scan_end.saturating_sub(1) {
        if bytes.get(span_end).copied() != Some(b'U')
            || bytes.get(span_end + 1).copied() != Some(CREATURE_OBJECT_TYPE)
            || read_u32_le(bytes, span_end + 6) != Some(LEGACY_CREATURE_UPDATE_3967_MASK)
            || !looks_like_legacy_live_object_sub_message_boundary(bytes, span_end)
        {
            continue;
        }
        if decode_cnw_msb_valid_bits(bytes.get(span_start..span_end)?).is_some() {
            return Some(span_end);
        }
    }
    None
}

fn looks_like_salvageable_legacy_live_object_record_at(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> bool {
    if !looks_like_legacy_live_object_sub_message_boundary(bytes, record_offset)
        || record_offset + 6 > bytes.len()
    {
        return false;
    }

    let record_end =
        find_next_legacy_live_object_sub_message_boundary_after(bytes, record_offset, scan_end)
            .min(bytes.len());
    if record_end <= record_offset {
        return false;
    }

    let opcode = bytes[record_offset];
    let object_type = bytes[record_offset + 1];
    match opcode {
        b'A' if object_type == CREATURE_OBJECT_TYPE => {
            looks_like_legacy_creature_add_transform_fields(bytes, record_offset, record_end)
        }
        b'A' if object_type == TRIGGER_OBJECT_TYPE => {
            crate::translate::live_object_update::trigger_add_record_end_for_ee(
                bytes,
                record_offset,
                record_end,
            ) == Some(record_end)
        }
        b'A' if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) => {
            // As with door/placeable `U` updates, a shifted stream can contain
            // printable text followed by `A/9` or `A/10`. The Diamond/EE
            // live-object dispatcher only reaches the next record after the
            // add-record reader consumes a complete typed add body, including
            // the EE visual-transform storage once translated. Do not salvage
            // on a scanner-derived minimum length alone.
            crate::translate::live_object_update::try_get_verified_door_placeable_add_record_end_for_transport(
                bytes,
                record_offset,
                scan_end,
            )
            .is_some()
        }
        b'U' if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) => {
            // Door/placeable updates are especially prone to false positives in
            // shifted HG live-object streams: arbitrary text/read-buffer bytes
            // can form `U/9` or `U/10` plus a nonzero DWORD such as
            // `0xFFFFFFF7`. Diamond and EE do not re-enter the live-object
            // dispatcher at that byte unless the whole typed update read span is
            // valid, so salvage must require the same bounded door/placeable
            // cursor proof used by the exact boundary scanner. A mere nonzero
            // mask is not semantic ownership.
            crate::translate::live_object_update::try_get_verified_door_placeable_update_record_end_for_transport(
                bytes,
                record_offset,
                scan_end,
            )
            .is_some()
        }
        b'U' if object_type == CREATURE_OBJECT_TYPE => {
            // A nonzero creature-update mask is not enough to anchor a shifted
            // stream. Several Diamond creature `U/5` families consume optional
            // identity/action/object subfields under CNW fragment BOOL control;
            // without the exact fragment cursor, the transport scanner can
            // mistake bytes inside an appearance/inventory payload for a top
            // level update. Only compact creature-update families with a
            // decompile-owned byte boundary may act as a salvage anchor here.
            crate::translate::live_object_update::try_get_verified_creature_update_record_end_for_transport(
                bytes,
                record_offset,
                scan_end,
            )
            .is_some()
        }
        b'U' if object_type == TRIGGER_OBJECT_TYPE => {
            // The trigger `U/7` decompile proof owns only the aligned
            // transport row: exact EE `0x00000001`, or legacy HG all-bits
            // `0xFFFF_FFF3` including its bounded three-byte tail. That is
            // enough for declared-length repair and boundary scanning once the
            // live-object cursor is already aligned, but it does not prove that
            // an arbitrary shifted byte run may be discarded before the `U/7`.
            false
        }
        b'D' if matches!(object_type, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => {
            record_end - record_offset <= 16
        }
        b'I' => true,
        b'G' => true,
        b'W' => {
            // `W current total` is a valid three-byte identity record when it
            // appears at an already-aligned live-object cursor, but it is much
            // too small to anchor partial-leadin salvage. Text/noise in shifted
            // HG streams can easily contain `57 xx 0E`; accepting that would
            // drop all earlier bytes and leak a transport fragment as a
            // semantic world-status packet. Keep partial salvage for records
            // with stronger typed boundaries and let orphaned `W` bytes remain
            // quarantined until a stream owner proves them.
            false
        }
        _ => false,
    }
}

fn find_next_legacy_live_object_sub_message_boundary_after(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> usize {
    let scan_end = search_end.min(bytes.len());
    if offset >= scan_end {
        return scan_end;
    }

    if bytes.get(offset).copied() == Some(b'G') {
        if let Some(record_end) =
            crate::translate::live_object_update::legacy_live_gui_record_end_for_transport(
                bytes,
                offset,
                scan_end,
                &[],
                0,
            )
        {
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'A') {
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_legacy_item_add_record_end_for_transport(
                bytes, offset, scan_end,
            )
        {
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'A')
        && bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
    {
        let legacy_scalar_record_end = offset.saturating_add(
            CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET
                + crate::translate::live_object_update::visual_transform::LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN,
        );
        if legacy_scalar_record_end <= scan_end
            && looks_like_legacy_creature_add_transform_fields(
                bytes,
                offset,
                legacy_scalar_record_end,
            )
            && crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
                bytes,
                offset + CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET,
                legacy_scalar_record_end,
            )
        {
            return legacy_scalar_record_end;
        }

        let ee_record_end = offset.saturating_add(
            CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET
                + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len(),
        );
        if ee_record_end <= scan_end
            && looks_like_legacy_creature_add_transform_fields(bytes, offset, ee_record_end)
            && has_ee_identity_visual_transform_map_at(
                bytes,
                offset + CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET,
                ee_record_end,
            )
        {
            return ee_record_end;
        }

        let legacy_record_end = offset.saturating_add(CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET);
        if legacy_record_end <= scan_end
            && looks_like_legacy_creature_add_transform_fields(bytes, offset, legacy_record_end)
        {
            // Diamond `sub_4489F0` consumes exactly OBJECTID, six raw FLOAT
            // fields, and a WORD for creature add records. The add-map rewrite
            // owns inserting EE's following object visual-transform identity
            // map at this fixed cursor; do not let generic scanning merge
            // later bytes into the creature add.
            return legacy_record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'A')
        && bytes.get(offset + 1).copied() == Some(TRIGGER_OBJECT_TYPE)
    {
        if let Some(record_end) =
            crate::translate::live_object_update::trigger_add_record_end_for_ee(
                bytes, offset, scan_end,
            )
        {
            return record_end;
        }
    }

    if matches!(
        (bytes.get(offset).copied(), bytes.get(offset + 1).copied()),
        (
            Some(b'A'),
            Some(PLACEABLE_OBJECT_TYPE) | Some(DOOR_OBJECT_TYPE)
        )
    ) {
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_verified_door_placeable_add_record_end_for_transport(
                bytes, offset, scan_end,
            )
        {
            // Door/placeable add records that already carry EE's
            // ObjectVisualTransformData map have a decompile-owned byte
            // boundary before the following `U` update. The visual-transform
            // pass must not merge that following update into the add and run
            // legacy add repair a second time.
            return record_end;
        }
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_legacy_placeable_short_name_add_record_end_for_transport(
                bytes, offset, scan_end,
            )
        {
            // Diamond compact placeable adds can carry a four-byte legacy
            // name/token slot before the BYTE/WORD/WORD tail. Splitting at the
            // decompiled tail cursor lets the focused add translator repair the
            // bytes and fragment BOOLs before the following `U/9` update.
            return record_end;
        }
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_legacy_short_door_add_record_end_for_transport(
                bytes, offset, scan_end,
            )
        {
            // Diamond short door adds can omit the empty direct string and
            // carry only the first door DWORD plus WORD state tail. Split at
            // that decompiled tail cursor so the focused add translator can
            // insert the EE visual-transform map and empty CExoString before
            // any following `U/10` update.
            return record_end;
        }
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_legacy_placeable_bare_name_add_record_end_for_transport(
                bytes, offset, scan_end,
            )
        {
            // Contest local Diamond captures use the related compact
            // placeable-add form: an empty CExoString slot, a bare printable
            // name, then the BYTE/WORD/WORD placeable tail. Split before the
            // same-object update so the focused add translator can insert the
            // EE visual-transform map and rewrite the fragment guards.
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'P')
        && bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
    {
        if let Some(record_end) =
            crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                bytes, offset, scan_end,
            )
        {
            return record_end;
        }
        if let Some(record_end) =
            partial_creature_appearance_read_end_for_transport(bytes, offset, scan_end)
        {
            // Diamond `sub_448E30` and EE `sub_14077FE10` keep non-full
            // body/equipment appearance deltas inside the `P/5` read-buffer
            // row. In particular, a body selector `>= 0x0A` owns the following
            // fixed nineteen body bytes, which can otherwise resemble top-level
            // `D/A/U` records to the generic scanner.
            return record_end;
        }
        if let Some(record_end) =
            short_creature_body_part_delta_read_end_for_transport(bytes, offset, scan_end)
        {
            return record_end;
        }
        if let Some(record_end) =
            short_creature_equipment_delta_read_end_for_transport(bytes, offset, scan_end)
        {
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'U')
        && bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
    {
        if let Some(record_end) = crate::translate::live_object_update::
            try_get_verified_creature_update_record_end_for_transport(bytes, offset, scan_end)
        {
            // Already-EE-shaped creature status-effect updates own the A/D
            // effect rows and their object visual-transform maps. The add-map
            // scanner must not split those rows as top-level live-object adds.
            return record_end;
        }
    }

    if matches!(
        (bytes.get(offset).copied(), bytes.get(offset + 1).copied()),
        (
            Some(b'U'),
            Some(PLACEABLE_OBJECT_TYPE) | Some(DOOR_OBJECT_TYPE)
        )
    ) {
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_verified_door_placeable_update_record_end_for_transport(
                bytes,
                offset,
                scan_end,
            )
        {
            return record_end;
        }
    }

    if bytes.get(offset).copied() == Some(b'U')
        && bytes.get(offset + 1).copied() == Some(TRIGGER_OBJECT_TYPE)
    {
        if let Some(record_end) =
            crate::translate::live_object_update::try_get_verified_trigger_update_record_end_for_transport(
                bytes,
                offset,
                scan_end,
            )
        {
            // Trigger `U/7` rows are not name-bearing streams. The supported
            // legacy HG all-bits row owns a bounded three-byte trigger tail
            // after the shared position fields; those bytes can decode as a CNW
            // fragment storage header, so the transport scanner must not split
            // inside them before the trigger translator sees the complete row.
            return record_end;
        }
    }

    let start = scan_end.min(offset + minimum_legacy_live_object_record_length_at(bytes, offset));
    let inventory_record = bytes.get(offset).copied() == Some(b'I');
    if inventory_record {
        if let Some(read_end) =
            crate::translate::live_object_update::legacy_inventory_prefix_read_end_for_transport(
                bytes, offset, scan_end,
            )
        {
            if read_end > offset && read_end < scan_end {
                // Diamond `sub_455940` and EE `sub_1407B4F70` consume the
                // inventory mask in a typed read-buffer order. Large HG
                // deterministic inventory packets such as D5FF can contain no
                // early opcode-like boundary for the generic scanner to test,
                // so use the bounded inventory prefix proof directly instead
                // of treating the whole remaining live stream as one record.
                return read_end;
            }
        }
    }
    let mut suppress_inline_string_boundaries = bytes.get(offset).copied() != Some(b'I');
    if bytes.len().saturating_sub(offset) >= 10
        && bytes[offset] == b'U'
        && bytes[offset + 1] == CREATURE_OBJECT_TYPE
        && looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        if let Some(raw_mask) = read_u32_le(bytes, offset + 6) {
            if matches!(
                raw_mask,
                0x0000_0008 | 0x0000_0047 | 0x0000_3967 | 0x0000_8000 | 0x0000_C408
            ) {
                // Mirror the focused live-object update boundary model: these
                // decompile/capture-backed creature `U/5` numeric families own
                // compact movement/status fields, not inline CExoString names.
                // In mixed bursts, suppressing opcode-like boundaries inside an
                // imaginary string can swallow the following `D/A/U` records and
                // prevent their exact translators from claiming them.
                suppress_inline_string_boundaries = false;
            }
        }
    }
    let string_scan_start = (offset + 2).min(scan_end);
    for candidate in start..scan_end.saturating_sub(1) {
        if suppress_inline_string_boundaries
            && candidate_inside_inline_string(bytes, string_scan_start, candidate)
        {
            continue;
        }
        if looks_like_legacy_live_object_sub_message_boundary(bytes, candidate) {
            if inventory_record
                && crate::translate::live_object_update::legacy_inventory_fragment_bit_count_for_transport(
                    bytes,
                    offset,
                    candidate,
                )
                .is_none()
            {
                // Diamond `sub_455940` and EE `sub_1407B4F70` consume the
                // full inventory mask shape before returning to the live-object
                // dispatcher. This transport-level scanner must therefore use
                // the same exact inventory cursor proof as
                // `live_object_update::boundary`; otherwise row text or
                // fragment bytes inside an `I` record can look like `GQ`/`A`/`U`
                // and make declared-length repair split a legal inventory span.
                continue;
            }
            return candidate;
        }
    }
    if inventory_record {
        return scan_end;
    }
    scan_end
}

fn compact_placeable_add_end_before_same_object_update_like_tail(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    let scan_end = scan_end.min(bytes.len());
    if offset + 15 > scan_end
        || bytes.get(offset).copied()? != b'A'
        || bytes.get(offset + 1).copied()? != PLACEABLE_OBJECT_TYPE
        || read_u32_le(bytes, offset + 6).is_none()
        || read_u16_le(bytes, offset + 11).is_none()
        || read_u16_le(bytes, offset + 13).is_none()
    {
        return None;
    }

    let object_id = read_u32_le(bytes, offset + 2)?;
    if !looks_like_legacy_live_object_id_value(object_id) {
        return None;
    }

    let compact_end = offset.checked_add(15)?;
    if compact_end >= scan_end {
        return Some(compact_end);
    }
    if looks_like_legacy_live_object_sub_message_boundary(bytes, compact_end) {
        return Some(compact_end);
    }

    let update_body_start = if bytes.get(compact_end).copied()? == PLACEABLE_OBJECT_TYPE {
        compact_end
    } else if bytes.get(compact_end).copied()? == 0
        && bytes.get(compact_end + 1).copied()? == PLACEABLE_OBJECT_TYPE
    {
        compact_end.checked_add(1)?
    } else {
        return None;
    };

    if update_body_start + 9 > scan_end
        || !read_u32_le(bytes, update_body_start + 1).is_some_and(|update_object_id| {
            crate::translate::live_object_update::object_ids::equivalent_legacy_external_object_ids(
                update_object_id,
                object_id,
            )
        })
        || read_u32_le(bytes, update_body_start + 5).is_none_or(|mask| mask == 0)
    {
        return None;
    }

    Some(compact_end)
}

fn minimum_legacy_live_object_record_length_at(bytes: &[u8], offset: usize) -> usize {
    if !looks_like_legacy_live_object_sub_message_boundary(bytes, offset) {
        return 2;
    }

    let opcode = bytes[offset];
    let marker = bytes[offset + 1];
    match (opcode, marker) {
        (b'A', _)
            if crate::translate::live_object_update::
                looks_like_legacy_item_add_record_boundary_for_transport(bytes, offset) =>
        {
            9
        }
        (b'A', 0x05) => CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET,
        (b'A', TRIGGER_OBJECT_TYPE) => {
            crate::translate::live_object_update::trigger_add_min_record_bytes_for_ee()
        }
        (b'A', PLACEABLE_OBJECT_TYPE) => {
            let name_offset = offset + 6;
            if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(4).saturating_sub(offset);
            }
            11
        }
        (b'A', DOOR_OBJECT_TYPE) => {
            let Some(first_dword) = read_u32_le(bytes, offset + 6) else {
                return 16;
            };
            let name_offset = offset + 2 + if first_dword == 0 { 12 } else { 8 };
            if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(2).saturating_sub(offset);
            }
            16
        }
        (b'U', PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) => {
            minimum_legacy_door_placeable_update_record_length_at(bytes, offset)
        }
        (b'U', 0x05) => minimum_legacy_creature_update_record_length_at(bytes, offset),
        (b'P', CREATURE_OBJECT_TYPE) => {
            // Diamond `sub_448E30` and EE `sub_14077FE10` both read a P/5
            // appearance header as CHAR opcode, BYTE object type, OBJECTID,
            // and WORD mask. A zero-mask row ends at those eight bytes and
            // consumes no CNW BOOLs, so capacity proof must not borrow the
            // ten-byte U-record header floor here.
            LEGACY_CREATURE_APPEARANCE_HEADER_BYTES
        }
        (b'U', ITEM_OBJECT_TYPE | TRIGGER_OBJECT_TYPE)
        | (b'P', ITEM_OBJECT_TYPE | TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) => {
            LEGACY_UPDATE_HEADER_BYTES
        }
        (b'D', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 6,
        (b'I', _) => 7,
        (b'W', _) if is_work_remaining_record_at(bytes, offset) => 3,
        _ => 2,
    }
}

fn minimum_legacy_creature_update_record_length_at(bytes: &[u8], offset: usize) -> usize {
    let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
        return LEGACY_UPDATE_HEADER_BYTES;
    };

    if raw_mask == 0x0000_0047 {
        // Same decompile-backed floor as
        // `live_object_update::boundary`: this transport/add-map scanner is
        // not allowed to split a creature `U/5 0x47` record inside its compact
        // position/action bytes. Exact ownership remains with the focused
        // live-object update validator.
        return 32;
    }

    LEGACY_UPDATE_HEADER_BYTES
}

fn minimum_legacy_door_placeable_update_record_length_at(bytes: &[u8], offset: usize) -> usize {
    let Some(raw_mask) = read_u32_le(bytes, offset + 6) else {
        return LEGACY_UPDATE_HEADER_BYTES;
    };

    // Decompile evidence:
    //
    // Diamond door/placeable update records share the common update header, then
    // conditionally read the compact position bytes. HG's anchored all-bits
    // door/placeable updates then carry a nine-byte generic tail
    // (`facing WORD`, state byte, scale DWORD, state WORD) before the name
    // string. EE later receives the same semantics through the focused
    // `live_object_update::record` translator, but this transport-level scanner
    // must still avoid treating bytes inside that tail as a fresh live-object
    // boundary. The Docks captures have `0x49 00` inside the tail; without this
    // minimum, that byte pair looked like an `I` item boundary and made the
    // add-map pass abandon the remaining placeable `A09` records.
    let mut minimum = LEGACY_UPDATE_HEADER_BYTES;
    if (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        minimum = minimum.saturating_add(LEGACY_UPDATE_POSITION_READ_BYTES);
    }

    if (raw_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK)) != 0 {
        minimum = minimum.saturating_add(LEGACY_DOOR_PLACEABLE_GENERIC_UPDATE_TAIL_BYTES);
    }

    if (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        let name_offset = offset.saturating_add(minimum);
        if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
            minimum = inline_end.saturating_sub(offset);
        } else {
            // EE/Diamond simple-name paths still consume a four-byte CExoString
            // length token even when the string is empty. Use it as a lower
            // bound only; exact name parsing remains in live_object_update.
            minimum = minimum.saturating_add(CNW_LENGTH_BYTES);
        }
    }

    minimum
}

fn legacy_live_object_continuation_boundary_offset(bytes: &[u8]) -> Option<usize> {
    let max_scan = bytes
        .len()
        .saturating_sub(2)
        .min(MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES);
    let salvage_scan_end = bytes.len().min(MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES);
    (0..=max_scan).find(|&offset| {
        looks_like_salvageable_legacy_live_object_record_at(bytes, offset, salvage_scan_end)
            || looks_like_hg_low_compact_placeable_continuation_at(bytes, offset, salvage_scan_end)
    })
}

fn looks_like_hg_low_compact_placeable_continuation_at(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> bool {
    let Some(record_header_end) = record_offset.checked_add(6) else {
        return false;
    };
    if record_header_end >= bytes.len()
        || record_header_end >= scan_end
        || bytes[record_offset] != b'A'
        || bytes[record_offset + 1] != PLACEABLE_OBJECT_TYPE
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, record_offset + 2) else {
        return false;
    };
    if !crate::translate::live_object_update::object_ids::is_compact_legacy_object_id(object_id) {
        return false;
    }

    // Decompile evidence keeps this local rather than global:
    // EE `CNWMessage::SetReadMessage` gives `HandleGameObjUpdate` a raw byte
    // window, and EE live-object handlers read object ids as DWORDs. The
    // high-byte/min-compact checks in this file are scanner guards, not engine
    // validity rules. HG Docks captures include a raw zlib continuation with a
    // one-byte lead-in followed by `A 09 <low DWORD id>` and an immediate item
    // read-buffer span. Accept only that continuation shape, with the adjacent
    // item token repeated in the short local window, so shifted text or
    // appearance bytes cannot lower the global object-id threshold.
    if bytes[record_header_end] != b'I' {
        return false;
    }
    let Some(item_token) = bytes.get(record_header_end + 1..record_header_end + 5) else {
        return false;
    };
    if item_token.iter().all(|byte| *byte == 0) || item_token.iter().all(|byte| *byte == 0xFF) {
        return false;
    }

    let repeat_start = record_header_end + 5;
    let repeat_end = bytes
        .len()
        .min(scan_end)
        .min(record_header_end.saturating_add(40));
    repeat_start < repeat_end
        && bytes[repeat_start..repeat_end]
            .windows(item_token.len())
            .any(|window| window == item_token)
}

fn looks_like_legacy_live_object_sub_message_boundary(bytes: &[u8], offset: usize) -> bool {
    if offset > bytes.len() || bytes.len() - offset < 2 {
        return false;
    }

    let opcode = bytes[offset];
    let marker = bytes[offset + 1];
    let typed_object_boundary = matches!(marker, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A)
        && looks_like_legacy_live_object_id_at(bytes, offset + 2);
    let legacy_type5_sentinel_boundary = marker == 0x05
        && bytes.len() - offset >= 6
        && bytes[offset + 2] == 0xFD
        && bytes[offset + 3] == 0xFF
        && bytes[offset + 4] == 0xFF
        && bytes[offset + 5] == 0xFF;
    if matches!(opcode, b'A' | b'D' | b'U' | b'P')
        && (typed_object_boundary || legacy_type5_sentinel_boundary)
    {
        return true;
    }

    if opcode == b'A'
        && crate::translate::live_object_update::
            looks_like_legacy_item_add_record_boundary_for_transport(bytes, offset)
    {
        return true;
    }

    if opcode == b'G'
        && crate::translate::live_object_update::legacy_live_gui_record_end_for_transport(
            bytes,
            offset,
            bytes.len(),
            &[],
            0,
        )
        .is_some()
    {
        return true;
    }

    let legacy_item_sentinel = marker == 0xFD
        && bytes.len() - offset >= 5
        && bytes[offset + 2] == 0xFF
        && bytes[offset + 3] == 0xFF
        && bytes[offset + 4] == 0xFF;
    if opcode == b'I'
        && (marker == 0x05
            || marker == 0xC5
            || legacy_item_sentinel
            || looks_like_legacy_live_object_id_at(bytes, offset + 1))
    {
        return true;
    }

    let gui_object_boundary = opcode == b'G'
        && is_ee_live_gui_sub_opcode_byte(marker)
        && !matches!(marker, b'I' | b'i' | b'R' | b'r')
        && bytes.len() - offset >= 9
        && looks_like_legacy_live_gui_object_id_at(bytes, offset + 5);
    if gui_object_boundary {
        return true;
    }

    let gui_repository_boundary = opcode == b'G'
        && matches!(marker, b'R' | b'r')
        && bytes.len() - offset >= 3
        && ((matches!(bytes[offset + 2], b'A' | b'M')
            && bytes.len() - offset >= 9
            && looks_like_legacy_live_gui_object_id_at(bytes, offset + 5))
            || (matches!(bytes[offset + 2], b'D' | b'U')
                && bytes.len() - offset >= 7
                && looks_like_legacy_live_gui_object_id_at(bytes, offset + 3)));
    if gui_repository_boundary {
        return true;
    }

    let gui_inventory_boundary = opcode == b'G'
        && matches!(marker, b'I' | b'i')
        && bytes.len() - offset >= 3
        && (matches!(bytes[offset + 2], b'D' | b'U')
            || (bytes[offset + 2] == b'A'
                && bytes.len() - offset >= 15
                && looks_like_legacy_live_gui_object_id_at(bytes, offset + 7))
            // Transport-only compatibility boundary.  The focused GUI
            // translator must still prove and rewrite this local Diamond
            // capture shape from `G I/i 00` to the decompile-required
            // `G I/i A`; the exact EE validator never accepts the zero byte.
            || (bytes[offset + 2] == 0x00
                && bytes.len() - offset >= 15
                && looks_like_legacy_live_gui_object_id_at(bytes, offset + 7)));
    if gui_inventory_boundary {
        return true;
    }

    is_work_remaining_record_at(bytes, offset)
}

fn looks_like_legacy_creature_add_transform_fields(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if offset > bytes.len()
        || record_end > bytes.len()
        || record_end < offset + CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    if object_id == u32::MAX || read_u16_le(bytes, offset + 30).is_none() {
        return false;
    }

    for index in 0..6 {
        // Diamond `sub_4489F0` and EE's creature-add writer consume these as
        // raw FLOAT slots. They do not reject NaN/sentinel bit patterns, so the
        // proxy validator must only prove the six fields are present.
        if read_f32_le(bytes, offset + 6 + index * 4).is_none() {
            return false;
        }
    }
    true
}

fn legacy_add_visual_transform_insert_offset(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<usize> {
    if offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= offset
        || bytes.len() - offset < 10
        || bytes[offset] != b'A'
        || read_u32_le(bytes, offset + 2).is_none()
    {
        return None;
    }

    match bytes[offset + 1] {
        // EE door/placeable add handlers read CAurObjectVisualTransformData at
        // fixed decompile-backed cursors relative to the legacy add record:
        // door add reads id + one/two DWORDs first, and placeable add reads
        // id/name/type/appearance/static fields first. Only synthesize the
        // identity map when those surrounding fields parse cleanly inside this
        // exact record. Creature add is only safe for the decompile-backed
        // fixed 32-byte transform prefix: `sub_14077F870` reads six floats, a
        // WORD, then `sub_140973160`. If additional appearance/body bytes are
        // present in the same record, this pass must not guess a split point.
        CREATURE_OBJECT_TYPE => {
            let insert_offset = offset + CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET;
            if record_end == insert_offset
                && looks_like_legacy_creature_add_transform_fields(bytes, offset, record_end)
            {
                Some(insert_offset)
            } else if record_end
                == insert_offset
                    + crate::translate::live_object_update::visual_transform::LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
                && looks_like_legacy_creature_add_transform_fields(bytes, offset, record_end)
                && crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
                    bytes,
                    insert_offset,
                    record_end,
                )
            {
                Some(insert_offset)
            } else {
                None
            }
        }
        DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE => None,
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct CreatureAddVisualTransformPatch {
    bytes_inserted: u32,
    bytes_removed: u32,
}

fn patch_creature_add_visual_transform_identity_for_ee(
    bytes: &mut Vec<u8>,
    visual_offset: usize,
    record_end: &mut usize,
) -> Option<CreatureAddVisualTransformPatch> {
    if has_ee_identity_visual_transform_map_at(bytes, visual_offset, *record_end) {
        return None;
    }

    if crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
        bytes,
        visual_offset,
        *record_end,
    ) {
        let removed =
            crate::translate::live_object_update::visual_transform::replace_legacy_scalar_identity_with_ee_object_identity(
                bytes,
                visual_offset,
                *record_end,
            )?;
        *record_end = (*record_end).checked_sub(
            removed.saturating_sub(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()),
        )?;
        return Some(CreatureAddVisualTransformPatch {
            bytes_inserted: EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32,
            bytes_removed: removed as u32,
        });
    }

    if visual_offset != *record_end {
        return None;
    }

    bytes.splice(
        visual_offset..visual_offset,
        EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES,
    );
    *record_end = (*record_end).checked_add(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len())?;
    Some(CreatureAddVisualTransformPatch {
        bytes_inserted: EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32,
        bytes_removed: 0,
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct DoorPlaceableAddRewrite {
    maps_inserted: u32,
    bytes_inserted: u32,
    bytes_removed: u32,
    fragment_bits_changed: bool,
    legacy_door_model_tokens_removed: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DoorPlaceableAddUpdatePassRewrite {
    pub maps_inserted: u32,
    pub bytes_inserted: u32,
    pub bytes_removed: u32,
    pub fragment_bits_changed: bool,
    pub legacy_door_model_tokens_removed: u32,
}

pub(crate) fn rewrite_legacy_door_placeable_add_record_for_update_pass(
    bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    record_offset: usize,
) -> Option<DoorPlaceableAddUpdatePassRewrite> {
    let rewrite = rewrite_legacy_door_placeable_add_record_for_ee(
        bytes,
        record_end,
        bits,
        bit_cursor,
        record_offset,
        None,
    )?;
    Some(DoorPlaceableAddUpdatePassRewrite {
        maps_inserted: rewrite.maps_inserted,
        bytes_inserted: rewrite.bytes_inserted,
        bytes_removed: rewrite.bytes_removed,
        fragment_bits_changed: rewrite.fragment_bits_changed,
        legacy_door_model_tokens_removed: rewrite.legacy_door_model_tokens_removed,
    })
}

fn rewrite_legacy_door_placeable_add_record_for_ee(
    bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    record_offset: usize,
    area_context: Option<&AreaPlaceableContext>,
) -> Option<DoorPlaceableAddRewrite> {
    if record_offset > bytes.len()
        || *record_end > bytes.len()
        || *record_end <= record_offset
        || bytes.len() - record_offset < 10
        || bytes[record_offset] != b'A'
        || !looks_like_legacy_live_object_id_at(bytes, record_offset + 2)
    {
        return None;
    }

    match bytes[record_offset + 1] {
        DOOR_OBJECT_TYPE => rewrite_legacy_door_add_record_for_ee(
            bytes,
            record_end,
            bits,
            bit_cursor,
            record_offset,
            area_context,
        ),
        PLACEABLE_OBJECT_TYPE => rewrite_legacy_placeable_add_record_for_ee(
            bytes,
            record_end,
            bits,
            bit_cursor,
            record_offset,
            area_context,
        ),
        _ => None,
    }
}

fn rewrite_legacy_door_add_record_for_ee(
    bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    record_offset: usize,
    _area_context: Option<&AreaPlaceableContext>,
) -> Option<DoorPlaceableAddRewrite> {
    if *bit_cursor >= bits.len() {
        return None;
    }
    let object_id = read_u32_le(bytes, record_offset + 2)?;
    let first_dword = read_u32_le(bytes, record_offset + 6)?;
    let compact_omits_second_dword = first_dword == 0
        && legacy_compact_door_add_omits_second_dword_for_ee(bytes, record_offset, *record_end);
    let visual_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };
    let mut summary = DoorPlaceableAddRewrite::default();

    if first_dword == 0 && !compact_omits_second_dword && record_offset + 14 <= *record_end {
        let generic_door_row = read_u32_le(bytes, record_offset + 10)?;
        if crate::translate::genericdoors::generic_door_model_status(generic_door_row)
            == crate::translate::genericdoors::GenericDoorModelStatus::MissingOrEmpty
        {
            // EE and Diamond both resolve first-DWORD-zero door adds through
            // `GenericDoors.2da[secondDWORD].ModelName`, but the model lookup is
            // visual/resource state, not game truth. The legacy server remains
            // authoritative for the door object and later transition/use
            // updates. Retain the add and emit EE's exact visual-transform map;
            // missing assets must be fixed by the resource/NWSync path instead
            // of deleting a live object from the protocol stream.
            tracing::warn!(
                object_id = format_args!("0x{object_id:08X}"),
                generic_door_row,
                "server->client live-object generic door add has no active GenericDoors.2da ModelName proof; retaining server-authored object"
            );
        }
    }

    if compact_omits_second_dword {
        // Diamond `sub_44DE30` and EE `sub_140796DD0` both read a second
        // DWORD when the first door-type DWORD is zero, then resolve
        // `GenericDoors.2da[secondDWORD].ModelName`. Area static context is
        // placeable-owned and is not proof of a door generic row, so the bridge
        // must not synthesize a selector here. Leave this record unclaimed
        // until a decompile-backed packet family proves a different legacy
        // shape.
        tracing::warn!(
            object_id = format_args!("0x{object_id:08X}"),
            "server->client live-object compact generic door add shape is not decompile-backed; quarantining instead of inventing GenericDoors row"
        );
        return None;
    }

    // EE's door add reader consumes ObjectVisualTransformData immediately
    // before the name branch. Some HG/Diamond captures already carry the older
    // 40-byte CAurObjectVisualTransformData scalar identity here; replace that
    // legacy scalar with EE's empty object-map identity instead of letting the
    // first two zero DWORDs masquerade as a complete EE map.
    let replaced_legacy_scalar =
        crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
            bytes,
            visual_offset,
            *record_end,
        );
    if replaced_legacy_scalar {
        let removed =
            crate::translate::live_object_update::visual_transform::replace_legacy_scalar_identity_with_ee_object_identity(
                bytes,
                visual_offset,
                *record_end,
            )?;
        *record_end = (*record_end).checked_sub(
            removed.saturating_sub(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()),
        )?;
        summary.maps_inserted = 1;
        summary.bytes_inserted = summary
            .bytes_inserted
            .saturating_add(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32);
        summary.bytes_removed = summary.bytes_removed.saturating_add(removed as u32);
    }

    let already_has_visual_map = replaced_legacy_scalar
        || has_ee_identity_visual_transform_map_at(bytes, visual_offset, *record_end);
    let mut name_offset = if already_has_visual_map {
        visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()
    } else {
        visual_offset
    };
    if visual_offset > *record_end || name_offset > *record_end {
        return None;
    }

    if !already_has_visual_map {
        bytes.splice(
            visual_offset..visual_offset,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES,
        );
        *record_end += EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
        name_offset += EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
        summary.maps_inserted = 1;
        summary.bytes_inserted = EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32;
    }

    if name_offset + 2 == *record_end && read_u16_le(bytes, name_offset).is_some() {
        // EE reaches door add data through the shared live-object add reader,
        // then `AddDoorAppearanceToMessage` writes the visual-transform map at
        // this cursor. HG/Diamond captures can omit the empty direct string and
        // carry only the final two-byte door tail here. Insert the exact empty
        // `CExoString` slot that EE's direct-name branch reads, leaving the tail
        // bytes intact.
        bytes.splice(name_offset..name_offset, [0, 0, 0, 0]);
        *record_end += CNW_LENGTH_BYTES;
        summary.bytes_inserted = summary
            .bytes_inserted
            .saturating_add(CNW_LENGTH_BYTES as u32);
    }

    let name_shape = legacy_door_add_name_shape_at(bytes, name_offset, *record_end)?;
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object door rewrite candidate: offset={record_offset} record_end={} bit_cursor={} already_has_visual_map={} name_offset={} shape={:?} next_bits={:?} name_tail={:02X?}",
            *record_end,
            *bit_cursor,
            already_has_visual_map,
            name_offset,
            name_shape.kind,
            bits.get(*bit_cursor..bit_cursor.saturating_add(8).min(bits.len()))
                .unwrap_or(&[]),
            bytes
                .get(name_offset..(*record_end).min(name_offset.saturating_add(16)))
                .unwrap_or(&[])
        );
    }
    if let Some((token_offset, token_length)) = name_shape.legacy_model_token {
        if token_offset < name_offset || token_length > (*record_end).saturating_sub(token_offset) {
            return None;
        }
        bytes.drain(token_offset..token_offset + token_length);
        *record_end -= token_length;
        summary.bytes_removed = summary.bytes_removed.saturating_add(token_length as u32);
        summary.legacy_door_model_tokens_removed =
            summary.legacy_door_model_tokens_removed.saturating_add(1);
    }

    if name_shape.is_tail_before_empty_direct_name() {
        if bits.len().saturating_sub(*bit_cursor) < LEGACY_COMPACT_DOOR_TAIL_BOOL_BITS {
            return None;
        }
        let tail = [bytes[name_offset], bytes[name_offset + 1]];
        bytes[name_offset..name_offset + CNW_LENGTH_BYTES].copy_from_slice(&[0, 0, 0, 0]);
        bytes[name_offset + CNW_LENGTH_BYTES..name_offset + CNW_LENGTH_BYTES + 2]
            .copy_from_slice(&tail);
        insert_cnw_msb_bit(bits, *bit_cursor, false)?;
        insert_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
        // This compact Diamond/HG source shape carries the byte-aligned WORD
        // tail first and a zero-length direct name after it. EE's
        // `sub_140796DD0` reads the decompile-owned direct branch as:
        // name BOOL=false, empty CExoString, post-name BOOL, WORD tail, then
        // four trailing BOOLs. Reorder only this exact six-byte source layout
        // and insert the two omitted false branch bits; all other six-byte
        // tails remain unclaimed and quarantine.
        *bit_cursor += 6;
        summary.fragment_bits_changed = true;
    } else if matches!(name_shape.kind, DoorAddNameKind::ShortStrRef) {
        if bits.len().saturating_sub(*bit_cursor) < 5 {
            return None;
        }
        write_u32_le(bytes, name_offset, 0)?;
        summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor, false)?;
        insert_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
        // Diamond short door-name rows carry the outer name BOOL followed by
        // four shared post-name door state BOOLs. EE's canonical direct
        // empty-name path has no StrRef read-buffer field, so the StrRef DWORD
        // is normalized to a zero-length CExoString and the single missing
        // post-name BOOL is inserted at the decompile-owned cursor.
        *bit_cursor += 6;
        summary.fragment_bits_changed = true;
    } else if bits.get(*bit_cursor).copied().unwrap_or(false) {
        match name_shape.kind {
            DoorAddNameKind::TlkLocString => {
                if !bits.get(*bit_cursor + 1).copied().unwrap_or(false) {
                    return None;
                }
            }
            DoorAddNameKind::DirectInline => {
                // EE door add (`sub_140796DD0`) has a canonical direct-name
                // path: outer BOOL false, then `ReadCExoString(0x20)`, then
                // the fixed post-name door tail bits.  The outer=true path
                // enters `ReadCExoLocStringClient` (`sub_1409735F0`) and only
                // reaches `ReadCExoString` after consuming an extra inner
                // BOOL.  That alternative is valid for true locstring payloads,
                // but a legacy direct CExoString record must not be emitted
                // through the locstring helper; doing so lets the helper's
                // branch bit desynchronise the following door tail.
                if *bit_cursor + 1 >= bits.len() {
                    return None;
                }
                summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor, false)?;
                bits.remove(*bit_cursor + 1);
                summary.fragment_bits_changed = true;
                *bit_cursor += 6;
                return Some(summary);
            }
            DoorAddNameKind::CompactTailBeforeEmptyDirectName | DoorAddNameKind::ShortStrRef => {
                return None;
            }
        }
        *bit_cursor += 7;
    } else {
        if matches!(name_shape.kind, DoorAddNameKind::TlkLocString) {
            return None;
        }
        let changed = set_cnw_msb_bit(bits, *bit_cursor, false)?;
        *bit_cursor += 6;
        summary.fragment_bits_changed = changed;
    }

    Some(summary)
}

fn rewrite_legacy_placeable_add_record_for_ee(
    bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    record_offset: usize,
    area_context: Option<&AreaPlaceableContext>,
) -> Option<DoorPlaceableAddRewrite> {
    if *bit_cursor > bits.len() {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 2)?;
    let name_offset = record_offset + 6;
    let inline_name_end = inline_cexo_string_end(bytes, name_offset);
    let short_name = inline_name_end.is_none();
    let mut tail_offset = inline_name_end.unwrap_or(name_offset + 4);
    let before_bits = bits.clone();
    let legacy_outer_locstring = before_bits.get(*bit_cursor).copied().unwrap_or(false);
    let legacy_inner_client_tlk = !short_name
        && legacy_outer_locstring
        && before_bits.get(*bit_cursor + 1).copied().unwrap_or(false);
    let direct_inline_name_payload = !short_name;
    let direct_name_mode_repair =
        legacy_outer_locstring && legacy_inner_client_tlk && direct_inline_name_payload;
    let inline_locstring_name = !short_name && legacy_outer_locstring && !direct_name_mode_repair;
    let source_name_inner_bits = usize::from(inline_locstring_name);
    let destination_name_inner_bits = usize::from(short_name || inline_locstring_name);
    let required_source_bits = 10 + source_name_inner_bits;
    let remaining_source_bits = before_bits.len().saturating_sub(*bit_cursor);
    let compact_empty_inline_name = try_find_legacy_placeable_empty_inline_fallback_name(
        bytes,
        name_offset,
        *record_end,
        false,
    );
    let compact_short_name_token_tail_end = if short_name && compact_empty_inline_name.is_none() {
        legacy_placeable_add_tail_end(bytes, name_offset + CNW_LENGTH_BYTES, *record_end, false)
    } else {
        None
    };
    if remaining_source_bits < required_source_bits
        && compact_empty_inline_name.is_none()
        && compact_short_name_token_tail_end.is_none()
    {
        return None;
    }
    let compact_source_bits =
        if compact_empty_inline_name.is_some() || compact_short_name_token_tail_end.is_some() {
            remaining_source_bits.min(LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS)
        } else {
            0
        };
    let source_post_name_bit = *bit_cursor + 1 + source_name_inner_bits;
    let source_optional_object_bit = if compact_empty_inline_name.is_some() {
        false
    } else if compact_short_name_token_tail_end.is_some() {
        false
    } else {
        before_bits
            .get(source_post_name_bit + 1)
            .copied()
            .unwrap_or(true)
    };
    let source_state_bits = if compact_empty_inline_name.is_none()
        && compact_short_name_token_tail_end.is_none()
        && remaining_source_bits >= required_source_bits
    {
        Some(legacy_placeable_add_state_bits(
            &before_bits,
            *bit_cursor,
            source_name_inner_bits,
        ))
    } else {
        None
    };
    let mut visual_offset = if let Some(recovered) = compact_empty_inline_name {
        let (recovered_tail_offset, recovered_legacy_tail_end) =
            apply_legacy_placeable_empty_inline_fallback_name(bytes, record_end, recovered)?;
        tail_offset = recovered_tail_offset;
        recovered_legacy_tail_end
    } else if let Some(legacy_tail_end) = compact_short_name_token_tail_end {
        tail_offset = name_offset + CNW_LENGTH_BYTES;
        legacy_tail_end
    } else if let Some(legacy_tail_end) =
        legacy_placeable_add_tail_end(bytes, tail_offset, *record_end, false)
    {
        legacy_tail_end
    } else if source_optional_object_bit {
        if let Some(legacy_tail_end) =
            legacy_placeable_add_tail_end(bytes, tail_offset, *record_end, true)
        {
            legacy_tail_end
        } else {
            let recovered = try_find_legacy_placeable_empty_inline_fallback_name(
                bytes,
                name_offset,
                *record_end,
                false,
            )
            .or_else(|| {
                try_find_legacy_placeable_empty_inline_fallback_name(
                    bytes,
                    name_offset,
                    *record_end,
                    true,
                )
            })?;
            let (recovered_tail_offset, recovered_legacy_tail_end) =
                apply_legacy_placeable_empty_inline_fallback_name(bytes, record_end, recovered)?;
            tail_offset = recovered_tail_offset;
            recovered_legacy_tail_end
        }
    } else {
        let recovered = try_find_legacy_placeable_empty_inline_fallback_name(
            bytes,
            name_offset,
            *record_end,
            false,
        )?;
        let (recovered_tail_offset, recovered_legacy_tail_end) =
            apply_legacy_placeable_empty_inline_fallback_name(bytes, record_end, recovered)?;
        tail_offset = recovered_tail_offset;
        recovered_legacy_tail_end
    };
    let compact_tail_zero_extended = visual_offset == tail_offset + 4;
    let full_tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    let optional_tail_end = full_tail_end.checked_add(4)?;
    let legacy_optional_object_bytes_present = visual_offset == optional_tail_end;
    if !source_optional_object_bit && legacy_optional_object_bytes_present {
        // Diamond `sub_44E4A0` and EE `sub_1407A7800` both put the optional
        // object-id guard after the first post-name BOOL and before the trailing
        // placeable state BOOLs. The guarded bytes are a normal OBJECTID in both
        // dialects, so the bridge can preserve them, but only when the bit and
        // byte cursor agree exactly.
        tracing::warn!(
            record_offset,
            bit_cursor = *bit_cursor,
            source_post_name_bit,
            source_optional_object_bit,
            legacy_optional_object_bytes_present,
            "server->client live-object placeable add optional-object bit/byte mismatch"
        );
        if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_PLACEABLE_ADD").is_some() {
            eprintln!(
                "placeable-add optional mismatch record_offset={record_offset} record_end={} bit_cursor={} source_post_name_bit={source_post_name_bit} source_optional={} bytes_optional={} tail_offset={tail_offset} visual_offset={visual_offset} full_tail_end={full_tail_end} optional_tail_end={optional_tail_end}",
                *record_end,
                *bit_cursor,
                source_optional_object_bit,
                legacy_optional_object_bytes_present
            );
        }
        return None;
    }
    if source_optional_object_bit && !legacy_optional_object_bytes_present {
        // The would-be optional-object bit is set in some HG legacy captures,
        // but the Diamond and EE decompiles both require four OBJECTID bytes
        // immediately after the two WORD tail fields when that branch is really
        // active (`sub_53E690` / `sub_1409737C0`). With no guarded bytes at the
        // cursor, this bit cannot be preserved as the EE optional-object guard;
        // it is treated as a legacy-only stray state bit and deliberately
        // cleared below.
        tracing::info!(
            record_offset,
            bit_cursor = *bit_cursor,
            source_post_name_bit,
            "server->client live-object placeable add clears legacy stray optional-object-shaped bit"
        );
    }

    let appearance = read_u16_le(bytes, tail_offset + 1)?;
    let placeable_model_status =
        crate::translate::placeables::placeable_model_status(u32::from(appearance));
    if placeable_model_status == crate::translate::placeables::PlaceableModelStatus::MissingOrEmpty
    {
        // This function is called while testing bounded live-object split and
        // fragment-cursor candidates, so a missing model row here is not yet a
        // final emitted-object fact. Keep the proof visible on the candidate
        // line below, but do not warn from a speculative probe. Diamond
        // `CNWSMessage::AddPlaceableAppearanceToMessage` writes the appearance
        // WORD that both Diamond and EE clients resolve through
        // `placeables.2da[appearance].ModelName`; model/resource absence is
        // still diagnostic-only and never permission to erase the live object.
        tracing::debug!(
            object_id = format_args!("0x{object_id:08X}"),
            appearance,
            "server->client live-object placeable add candidate lacks active placeables.2da ModelName proof"
        );
    }
    tracing::info!(
        record_offset,
        record_end = *record_end,
        bit_cursor = *bit_cursor,
        name_offset,
        tail_offset,
        visual_offset,
        appearance,
        placeable_model_status = ?placeable_model_status,
        short_name,
        legacy_outer_locstring,
        legacy_inner_client_tlk,
        direct_name_mode_repair,
        inline_locstring_name,
        compact_empty_inline_name = compact_empty_inline_name.is_some(),
        compact_source_bits,
        source_name_inner_bits,
        destination_name_inner_bits,
        source_bits = required_source_bits,
        legacy_optional_gate_consumed = source_state_bits.is_some(),
        legacy_optional_gate_source = source_state_bits.map(|_| source_optional_object_bit),
        ee_optional_target_gate = legacy_optional_object_bytes_present,
        placeable_add_state = ?source_state_bits,
        ee_light_is_on = false,
        tail = %format_hex_slice(bytes, tail_offset, (*record_end).saturating_sub(tail_offset).min(16)),
        bits = %format_bit_slice(&before_bits, *bit_cursor, required_source_bits.min(16)),
        "server->client live-object placeable add candidate"
    );

    // EE `CNWCArea::LoadArea` (`sub_1407D95A0`) and Diamond's equivalent
    // static-placeable loader both key their coalescing on object id, but live
    // `U/9` updates still go through `HandleServerToPlayerGenericObjectUpdate`
    // and require an active object-table entry for that same id. Driver-only HG
    // captures proved that deleting this `A/9` record, even when the latest
    // area stream mentioned the same id, makes EE log "Received update message
    // for object that doesn't exist" on the following update. Keep overlap and
    // legacy UserNN rows diagnostic-only; model/resource compatibility belongs
    // in a typed placeable writer, not in an object-lifecycle suppression rule.
    let area_static_duplicate =
        area_context.is_some_and(|context| context.contains_placeable_id(object_id));
    let legacy_user_defined_static = is_legacy_user_defined_placeable_appearance(appearance);
    if area_static_duplicate || legacy_user_defined_static {
        let mut area_rows = String::new();
        let mut area_module_state_mismatch = false;
        if let Some(context) = area_context {
            for (index, row) in context.rows_with_placeable_id(object_id).enumerate() {
                if index != 0 {
                    area_rows.push(',');
                }
                if let (Some(source_state), Some(module_state)) =
                    (source_state_bits, row.module_state)
                {
                    area_module_state_mismatch |=
                        placeable_add_state_conflicts_with_area_module_state(
                            source_state,
                            module_state,
                        );
                }
                let module_state = row
                    .module_state
                    .map(format_area_placeable_module_state)
                    .unwrap_or_else(|| "unproven".to_string());
                area_rows.push_str(&format!(
                    "app=0x{:04X}@{:.2},{:.2},{:.2};state={module_state}",
                    row.appearance, row.x, row.y, row.z
                ));
            }
        }
        tracing::info!(
            object_id = format_args!("0x{object_id:08X}"),
            appearance,
            area_static_duplicate,
            legacy_user_defined_static,
            area_module_state_mismatch,
            source_bits = required_source_bits,
            area_rows = %area_rows,
            "server->client live-object placeable add overlaps area/static context; retaining add so later updates have an active EE object"
        );
    }

    let already_has_ee_visual_map =
        has_ee_identity_visual_transform_map_at(bytes, visual_offset, *record_end);
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_PLACEABLE_ADD").is_some() {
        let post_name_bit = *bit_cursor + 1 + destination_name_inner_bits;
        eprintln!(
            "placeable-add rewrite decision record_offset={record_offset} record_end={} bit_cursor={} visual_offset={visual_offset} already_has_map={already_has_ee_visual_map} legacy_optional_bytes={legacy_optional_object_bytes_present} post_name_bit={post_name_bit} optional_bit={:?} final_bit={:?} direct_name_repair={direct_name_mode_repair} compact_tail={compact_tail_zero_extended}",
            *record_end,
            *bit_cursor,
            bits.get(post_name_bit + 1).copied(),
            bits.get(post_name_bit + 9).copied()
        );
    }
    if already_has_ee_visual_map
        && visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() == *record_end
    {
        let mut verified_cursor = *bit_cursor;
        if crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
            bytes,
            record_offset,
            *record_end,
            bits,
            &mut verified_cursor,
        ) {
            // The byte side and fragment cursor already match the exact EE
            // placeable-add reader. Let the caller's cursor-only path advance
            // this record; do not re-run legacy insertion logic on an already
            // translated add during a later pass.
            return None;
        }
    }
    if already_has_ee_visual_map
        && visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() == *record_end
        && direct_name_mode_repair
        && !compact_tail_zero_extended
    {
        let post_name_bit = *bit_cursor + 1;
        if bits.len() <= post_name_bit + 9 {
            return None;
        }
        let mut summary = DoorPlaceableAddRewrite::default();
        summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor, false)?;
        summary.fragment_bits_changed |= set_cnw_msb_bit(
            bits,
            post_name_bit + 1,
            legacy_optional_object_bytes_present,
        )?;
        summary.fragment_bits_changed |= set_cnw_msb_bit(bits, post_name_bit + 9, false)?;
        if summary.fragment_bits_changed {
            // The first pass may leave a legacy outer=true/inner=true direct
            // name pattern on bytes that are already EE-shaped. EE direct-name
            // placeable adds use outer=false; the former inner bit is then the
            // first post-name state bit, so no extra optional-object guard is
            // inserted on repeat passes.
            *bit_cursor += 11;
            return Some(summary);
        }
        return None;
    }
    if already_has_ee_visual_map
        && visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() == *record_end
        && !direct_name_mode_repair
        && !compact_tail_zero_extended
    {
        let post_name_bit = *bit_cursor + 1 + destination_name_inner_bits;
        if bits.len() <= post_name_bit + 9 {
            return None;
        }
        let mut summary = DoorPlaceableAddRewrite::default();
        summary.fragment_bits_changed |= set_cnw_msb_bit(
            bits,
            post_name_bit + 1,
            legacy_optional_object_bytes_present,
        )?;
        summary.fragment_bits_changed |= set_cnw_msb_bit(bits, post_name_bit + 9, false)?;
        if summary.fragment_bits_changed {
            // The byte side is already EE-shaped, but old captures/fixtures can
            // still carry legacy fragment bits. The decompiled optional-object
            // branch is keyed by bytes as well as the BOOL, so repair only the
            // two EE-validated guard bits and advance the cursor as an exact EE
            // placeable add.
            *bit_cursor += 11 + destination_name_inner_bits;
            return Some(summary);
        }
        // This add record is already in the EE byte layout. Returning `None`
        // lets the cursor-only exact validator own the no-op path instead of
        // rewriting an already translated record a second time.
        return None;
    }

    let mut summary = DoorPlaceableAddRewrite::default();
    if compact_tail_zero_extended {
        // EE and Diamond both read the placeable add tail as:
        // `BYTE type`, one BOOL, `WORD appearance`, `WORD static/state`, then
        // additional BOOLs before EE's visual-transform map. HG live captures
        // sometimes compact an all-zero high byte from the final WORD at the
        // read-buffer boundary. Restore that byte before inserting the EE map so
        // the decompiled reader consumes two full WORD fields.
        bytes.insert(visual_offset, 0);
        *record_end += 1;
        visual_offset += 1;
        summary.bytes_inserted = summary.bytes_inserted.saturating_add(1);
    }

    if compact_empty_inline_name.is_some() || compact_short_name_token_tail_end.is_some() {
        // Local Diamond captures can carry exact compact placeable adds with a
        // recovered inline name, or a four-byte legacy short-name/token slot
        // immediately before the BYTE/WORD/WORD placeable tail. Both forms own
        // only the four Diamond compact tail BOOLs at the source cursor. Late
        // Winds streams can have all but one of those compact source bits
        // consumed by prior decompile-backed update repairs; drain that bounded
        // residue as part of the same compact add shape. EE's `sub_1407A7800`
        // still needs the full direct-name guard run before the visual-
        // transform map, so emit false guards after the bounded byte parser
        // proves one of these compact shapes.
        if compact_short_name_token_tail_end.is_some() {
            write_u32_le(bytes, name_offset, 0)?;
        }
        if compact_source_bits != 0 {
            let drain_end = bit_cursor.checked_add(compact_source_bits)?;
            bits.drain(*bit_cursor..drain_end);
        }
        let emitted_name_inner_bits = usize::from(compact_short_name_token_tail_end.is_some());
        for delta in 0..11 + emitted_name_inner_bits {
            insert_cnw_msb_bit(bits, *bit_cursor + delta, false)?;
        }
        if compact_short_name_token_tail_end.is_some() {
            // The four-byte legacy short-name/token slot has already been
            // canonicalized to an empty CExoString at the byte cursor. Emit the
            // exact EE helper branch for that shape: `sub_1407A7800` sees
            // outer=true and `sub_1409735F0` then sees inner=false before
            // reading the same zero-length `CExoString(32)`. Keeping this
            // branch length aligned with the decompiled reader prevents the
            // following placeable update from borrowing the inner selector bit.
            summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor, true)?;
        }
        summary.fragment_bits_changed = true;
    } else {
        if short_name {
            write_u32_le(bytes, name_offset, 0)?;
            set_cnw_msb_bit(bits, *bit_cursor, true)?;
            insert_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
            summary.fragment_bits_changed = true;
        } else if direct_name_mode_repair {
            // EE placeable add (`sub_1407A7800`) routes outer=true through the
            // locstring helper. When both outer and inner are true but the read
            // bytes at the name cursor are an inline CExoString, the inner bit is
            // really the first post-name state bit in the legacy stream. Force the
            // direct CExoString branch but do not remove that bit.
            summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor, false)?;
        } else if inline_locstring_name {
            summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
        }

        // Placeable add post-name bits are now kept in the same order as the
        // decompiled readers/writers:
        //
        // Diamond client `sub_44E4A0`:
        //   BOOL after type, optional-object BOOL, seven trailing state BOOLs.
        //
        // EE client `sub_1407A7800` / EE server
        // `CNWSMessage::AddPlaceableAppearanceToMessage`:
        //   BOOL after type, optional-object BOOL, eight trailing state BOOLs,
        //   then `ObjectVisualTransformData::Write`.
        //
        // The optional OBJECTID branch is byte-identical between Diamond and EE, so
        // the supported rewrite preserves it only when the four guarded bytes are
        // present at the decompiled cursor. HG captures may set the bit-like legacy
        // position without those bytes; that cannot be the real optional branch, so
        // the EE guard is forced false while the seven shared trailing BOOLs are
        // copied from the decompile-backed legacy positions.
        let post_name_bit = *bit_cursor + 1 + destination_name_inner_bits;
        if bits.len() < post_name_bit + 9 {
            return None;
        }
        let legacy_state = source_state_bits.unwrap_or_else(|| {
            legacy_placeable_add_state_bits(&before_bits, *bit_cursor, source_name_inner_bits)
        });
        summary.fragment_bits_changed |= write_ee_placeable_add_state_bits(
            bits,
            post_name_bit,
            legacy_state,
            legacy_optional_object_bytes_present,
        )?;
    }

    // EE's placeable add reader reaches `ObjectVisualTransformData::Read`
    // after the name/tail BOOL sequence. HG/Diamond captures can carry the
    // legacy 40-byte scalar identity at that cursor, just like door and
    // creature add records. Replace that scalar with EE's object-map identity;
    // do not allow the leading zero DWORDs inside the legacy scalar to be
    // mistaken for a complete EE map.
    let replaced_legacy_scalar =
        crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
            bytes,
            visual_offset,
            *record_end,
        );
    if replaced_legacy_scalar {
        let removed =
            crate::translate::live_object_update::visual_transform::replace_legacy_scalar_identity_with_ee_object_identity(
                bytes,
                visual_offset,
                *record_end,
            )?;
        *record_end = (*record_end).checked_sub(
            removed.saturating_sub(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()),
        )?;
        summary.maps_inserted = 1;
        summary.bytes_inserted = summary
            .bytes_inserted
            .saturating_add(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32);
        summary.bytes_removed = summary.bytes_removed.saturating_add(removed as u32);
    }

    if !replaced_legacy_scalar
        && !has_ee_identity_visual_transform_map_at(bytes, visual_offset, *record_end)
    {
        bytes.splice(
            visual_offset..visual_offset,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES,
        );
        *record_end += EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
        summary.maps_inserted = 1;
        summary.bytes_inserted = summary
            .bytes_inserted
            .saturating_add(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32);
    }

    *bit_cursor += 11 + destination_name_inner_bits;
    Some(summary)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlaceableAddStateBits {
    reputation_visual: bool,
    static_plot: bool,
    useable: bool,
    trap_disarmable: bool,
    lockable: bool,
    locked: bool,
    unknown_1ac: bool,
    name_valid: bool,
}

fn legacy_placeable_add_state_bits(
    bits: &[bool],
    bit_cursor: usize,
    source_name_inner_bits: usize,
) -> PlaceableAddStateBits {
    let bit = |relative: usize| {
        bits.get(bit_cursor + source_name_inner_bits + relative)
            .copied()
            .unwrap_or(false)
    };
    PlaceableAddStateBits {
        reputation_visual: bit(1),
        static_plot: bit(3),
        useable: bit(4),
        trap_disarmable: bit(5),
        lockable: bit(6),
        locked: bit(7),
        unknown_1ac: bit(8),
        name_valid: bit(9),
    }
}

fn placeable_add_state_conflicts_with_area_module_state(
    state: PlaceableAddStateBits,
    module_state: AreaPlaceableContextState,
) -> bool {
    state.useable != module_state.useable
        || state.trap_disarmable != module_state.trap_disarmable
        || state.lockable != module_state.lockable
        || state.locked != module_state.locked
}

fn format_area_placeable_module_state(module_state: AreaPlaceableContextState) -> String {
    format!(
        "static={} useable={} trap={} disarmable={} lockable={} locked={}",
        module_state.static_object,
        module_state.useable,
        module_state.trap_flag,
        module_state.trap_disarmable,
        module_state.lockable,
        module_state.locked
    )
}

fn write_ee_placeable_add_state_bits(
    bits: &mut Vec<bool>,
    post_name_bit: usize,
    state: PlaceableAddStateBits,
    optional_object_bytes_present: bool,
) -> Option<bool> {
    insert_cnw_msb_bit(bits, post_name_bit + 1, optional_object_bytes_present)?;
    let mut changed = true;
    for (delta, value) in [
        (0usize, state.reputation_visual),
        (1, optional_object_bytes_present),
        (2, state.static_plot),
        (3, state.useable),
        (4, state.trap_disarmable),
        (5, state.lockable),
        (6, state.locked),
        (7, state.unknown_1ac),
        (8, state.name_valid),
        (9, false),
    ] {
        changed |= set_cnw_msb_bit(bits, post_name_bit + delta, value)?;
    }
    Some(changed)
}

fn advance_known_live_record_fragment_cursor_for_ee(
    bytes: &[u8],
    bits: &[bool],
    record_offset: usize,
    record_end: usize,
    bit_cursor: &mut usize,
) -> Option<bool> {
    if record_offset + 2 > record_end || record_end > bytes.len() {
        return None;
    }

    match (bytes[record_offset], bytes[record_offset + 1]) {
        (b'A', CREATURE_OBJECT_TYPE)
            if looks_like_legacy_creature_add_transform_fields(
                bytes,
                record_offset,
                record_end,
            ) =>
        {
            Some(true)
        }
        (b'P', CREATURE_OBJECT_TYPE) => {
            // The add-map pass may need to walk past an already translated
            // creature appearance packet before it reaches a later door or
            // placeable add. Keep the walk on the exact live-object validator
            // path instead of teaching this transport-adjacent module creature
            // appearance semantics.
            crate::translate::live_object_update::advance_verified_creature_appearance_fragment_cursor_for_ee(
                bytes,
                record_offset,
                record_end,
                bits,
                bit_cursor,
            )
            .then_some(true)
        }
        (b'U', CREATURE_OBJECT_TYPE) => {
            // Same rule for creature `U` updates: this function only preserves
            // the fragment cursor for later add-record rewrites, while the
            // bounded creature parsers in `live_object_update` remain the
            // decompile-backed owners of the record shape.
            crate::translate::live_object_update::advance_verified_creature_update_fragment_cursor_for_ee(
                bytes,
                record_offset,
                record_end,
                bits,
                bit_cursor,
            )
            .then_some(true)
        }
        (b'A', TRIGGER_OBJECT_TYPE | DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE) => {
            let original_bit_cursor = *bit_cursor;
            if crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                bytes,
                record_offset,
                record_end,
                bits,
                bit_cursor,
            ) {
                return Some(true);
            }
            *bit_cursor = original_bit_cursor;
            advance_live_add_record_bit_cursor(bytes, bits, record_offset, record_end, bit_cursor)
                .then_some(true)
        }
        (b'A', ITEM_OBJECT_TYPE) => {
            crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                bytes,
                record_offset,
                record_end,
                bits,
                bit_cursor,
            )
            .then_some(true)
        }
        (
            b'U',
            TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE | ITEM_OBJECT_TYPE,
        ) => {
            if crate::translate::live_object_update::advance_verified_door_placeable_update_fragment_cursor_for_ee(
                bytes,
                record_offset,
                record_end,
                bits,
                bit_cursor,
            ) {
                return Some(true);
            }
            advance_legacy_live_update_record_fragment_cursor_for_add_pass(
                bytes,
                bits,
                record_offset,
                record_end,
                bit_cursor,
            )
            .then_some(false)
        }
        (b'I', _) => {
            // Inventory records can own fragment BOOLs and may precede compact
            // door/placeable add records in the same live-object burst. Keep
            // inventory parsing inside `live_object_update::inventory`; this
            // add-map pass only needs the verified cursor advance so the
            // following add record can be rewritten at the correct bit offset.
            crate::translate::live_object_update::advance_verified_inventory_fragment_cursor_for_ee(
                bytes,
                record_offset,
                record_end,
                bits,
                bit_cursor,
            )
            .then_some(true)
        }
        (b'D', object_type) => {
            let bit_count =
                legacy_live_delete_fragment_bit_count(bytes, record_offset, record_end)?;
            if matches!(object_type, 0x05 | 0x06 | 0x09 | 0x07 | 0x0A)
                && bits.len().saturating_sub(*bit_cursor) >= bit_count
            {
                *bit_cursor += bit_count;
                Some(true)
            } else {
                None
            }
        }
        (b'W', _)
            if record_end == record_offset + 3
                && is_work_remaining_record_at(bytes, record_offset) =>
        {
            Some(true)
        }
        _ => None,
    }
}

fn advance_legacy_live_update_record_fragment_cursor_for_add_pass(
    bytes: &[u8],
    bits: &[bool],
    record_offset: usize,
    record_end: usize,
    bit_cursor: &mut usize,
) -> bool {
    if record_offset + 10 > record_end
        || record_end > bytes.len()
        || bytes.get(record_offset).copied() != Some(b'U')
    {
        return false;
    }

    let object_type = bytes[record_offset + 1];
    let Some(raw_mask) = read_u32_le(bytes, record_offset + 6) else {
        return false;
    };

    // Cursor-only bridge between `A` add-map rewrites and later focused `U`
    // update rewrites. Diamond `sub_44F3D0` consumes the mask-0x2 generic
    // orientation as the compact scalar shape; EE `sub_14079C050` consumes one
    // extra orientation-mode BOOL before that same scalar branch. The later
    // update translator inserts that EE-only branch bit, but this add pass must
    // still advance over Diamond's four source-owned scalar low bits so a
    // following placeable/door add repairs the correct fragment span.
    let mut consumed = 0usize;
    if (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        consumed = consumed.saturating_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS);
    }
    if matches!(
        object_type,
        ITEM_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) && (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
    {
        let orientation_cursor = bit_cursor.saturating_add(consumed);
        let orientation_bits = if bits.get(orientation_cursor).copied().unwrap_or(false) {
            1
        } else {
            LEGACY_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
        };
        consumed = consumed.saturating_add(orientation_bits);
    }
    if matches!(
        object_type,
        ITEM_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) && (raw_mask & LEGACY_UPDATE_STATE_MASK) != 0
    {
        consumed = consumed.saturating_add(LEGACY_UPDATE_STATE_FRAGMENT_BITS);
    }
    if object_type == ITEM_OBJECT_TYPE && (raw_mask & 0x0000_0040) != 0 {
        // Diamond's item-specific update branch writes its mask-0x40 byte tail
        // in the read buffer and one BOOL in the CNW fragment stream. EE keeps
        // only the BOOL; the later typed item update rewrite owns the byte
        // removal, while this add-map pass only keeps the cursor aligned.
        consumed = consumed.saturating_add(1);
    }

    if bits.len().saturating_sub(*bit_cursor) < consumed {
        return false;
    }
    *bit_cursor = bit_cursor.saturating_add(consumed);
    true
}

fn advance_live_add_record_bit_cursor(
    bytes: &[u8],
    bits: &[bool],
    record_offset: usize,
    record_end: usize,
    bit_cursor: &mut usize,
) -> bool {
    if record_offset + 6 > record_end || record_end > bytes.len() {
        return false;
    }

    match bytes[record_offset + 1] {
        TRIGGER_OBJECT_TYPE => {
            crate::translate::live_object_update::trigger_add_record_end_for_ee(
                bytes,
                record_offset,
                record_end,
            ) == Some(record_end)
                && *bit_cursor <= bits.len()
        }
        DOOR_OBJECT_TYPE => {
            let Some(first_dword) = read_u32_le(bytes, record_offset + 6) else {
                return false;
            };
            let visual_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };
            let name_offset =
                if has_ee_identity_visual_transform_map_at(bytes, visual_offset, record_end) {
                    visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()
                } else {
                    visual_offset
                };
            let Some(name_shape) = legacy_door_add_name_shape_at(bytes, name_offset, record_end)
            else {
                return false;
            };
            if *bit_cursor >= bits.len() {
                return false;
            }
            let source_inner_bits = match name_shape.kind {
                DoorAddNameKind::TlkLocString => {
                    if !bits[*bit_cursor] || !bits.get(*bit_cursor + 1).copied().unwrap_or(false) {
                        return false;
                    }
                    1
                }
                DoorAddNameKind::DirectInline => {
                    if bits[*bit_cursor] && bits.get(*bit_cursor + 1).copied().unwrap_or(true) {
                        return false;
                    }
                    usize::from(bits[*bit_cursor])
                }
                DoorAddNameKind::CompactTailBeforeEmptyDirectName => {
                    if bits.len().saturating_sub(*bit_cursor) < LEGACY_COMPACT_DOOR_TAIL_BOOL_BITS {
                        return false;
                    }
                    *bit_cursor = bit_cursor.saturating_add(LEGACY_COMPACT_DOOR_TAIL_BOOL_BITS);
                    return *bit_cursor <= bits.len();
                }
                DoorAddNameKind::ShortStrRef => {
                    if bits.len().saturating_sub(*bit_cursor) < 5 {
                        return false;
                    }
                    *bit_cursor = bit_cursor.saturating_add(5);
                    return *bit_cursor <= bits.len();
                }
            };
            *bit_cursor = bit_cursor.saturating_add(6 + source_inner_bits);
            *bit_cursor <= bits.len()
        }
        PLACEABLE_OBJECT_TYPE => {
            let name_offset = record_offset + 6;
            if try_find_legacy_placeable_empty_inline_fallback_name(
                bytes,
                name_offset,
                record_end,
                false,
            )
            .is_some()
            {
                // This is the same bounded compact Diamond shape handled by
                // `rewrite_legacy_placeable_add_record_for_ee`: an empty direct
                // CExoString length token, a padded printable name span, and
                // the decompile-owned placeable tail before the next record
                // boundary. The add-pass boundary walker runs before that byte
                // rewrite, so it must advance over Diamond's four source-owned
                // tail BOOLs here instead of requiring the EE writer's final
                // eleven BOOL run.
                let remaining_source_bits = bits.len().saturating_sub(*bit_cursor);
                if remaining_source_bits == 0 {
                    return true;
                }
                if remaining_source_bits < LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS {
                    return false;
                }
                *bit_cursor = bit_cursor.saturating_add(LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS);
                return *bit_cursor <= bits.len();
            }
            if *bit_cursor >= bits.len() {
                return false;
            }
            if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
                if inline_end > name_offset + CNW_LENGTH_BYTES
                    && bits[*bit_cursor]
                    && bits.get(*bit_cursor + 1).copied().unwrap_or(true)
                {
                    return false;
                }
            }
            let dest_inner_bits = usize::from(bits[*bit_cursor]);
            *bit_cursor = bit_cursor.saturating_add(11 + dest_inner_bits);
            *bit_cursor <= bits.len()
        }
        _ => false,
    }
}

fn legacy_live_delete_fragment_bit_count(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    if record_end != record_offset + 6
        || record_end > bytes.len()
        || bytes.get(record_offset).copied() != Some(b'D')
        || !looks_like_legacy_live_object_id_at(bytes, record_offset + 2)
    {
        return None;
    }

    match bytes[record_offset + 1] {
        0x05 | 0x06 | 0x09 => Some(1),
        0x07 | 0x0A => Some(0),
        _ => None,
    }
}

fn is_legacy_user_defined_placeable_appearance(appearance: u16) -> bool {
    // Diamond 1.72 `placeables.2da` rows 202..229 are User01..User28 and
    // rows 275..278 are User92..User95. These rows resolve to the generic
    // user-defined model token rather than a real model resref in EE.
    matches!(appearance, 202..=229 | 275..=278)
}

fn format_hex_slice(bytes: &[u8], offset: usize, length: usize) -> String {
    let Some(slice) = bytes.get(offset..offset.saturating_add(length).min(bytes.len())) else {
        return String::new();
    };
    let mut text = String::new();
    for (index, byte) in slice.iter().enumerate() {
        if index != 0 {
            text.push(' ');
        }
        text.push_str(&format!("{byte:02X}"));
    }
    text
}

fn format_bit_slice(bits: &[bool], offset: usize, length: usize) -> String {
    let mut text = String::new();
    let end = offset.saturating_add(length).min(bits.len());
    for bit in &bits[offset.min(bits.len())..end] {
        text.push(if *bit { '1' } else { '0' });
    }
    text
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoorAddNameKind {
    CompactTailBeforeEmptyDirectName,
    DirectInline,
    ShortStrRef,
    TlkLocString,
}

#[derive(Debug, Clone, Copy)]
struct DoorAddNameShape {
    kind: DoorAddNameKind,
    legacy_model_token: Option<(usize, usize)>,
}

impl DoorAddNameShape {
    fn is_tail_before_empty_direct_name(self) -> bool {
        matches!(self.kind, DoorAddNameKind::CompactTailBeforeEmptyDirectName)
    }
}

const LEGACY_COMPACT_DOOR_TAIL_BOOL_BITS: usize = 4;
const LEGACY_COMPACT_PLACEABLE_ADD_BOOL_BITS: usize = 4;
const LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES: usize = 8;

fn legacy_door_add_name_shape_at(
    bytes: &[u8],
    name_offset: usize,
    record_end: usize,
) -> Option<DoorAddNameShape> {
    if name_offset > record_end || record_end > bytes.len() {
        return None;
    }

    if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
        if inline_end + 2 == record_end {
            return Some(DoorAddNameShape {
                kind: DoorAddNameKind::DirectInline,
                legacy_model_token: None,
            });
        }
        const LEGACY_DOOR_MODEL_TOKEN_BYTES: usize = 4;
        if inline_end + LEGACY_DOOR_MODEL_TOKEN_BYTES + 2 == record_end
            && looks_like_legacy_door_model_token_at(bytes, inline_end, record_end)
        {
            return Some(DoorAddNameShape {
                kind: DoorAddNameKind::DirectInline,
                legacy_model_token: Some((inline_end, LEGACY_DOOR_MODEL_TOKEN_BYTES)),
            });
        }
        return None;
    }

    if looks_like_legacy_door_tlk_locstring_at(bytes, name_offset, record_end) {
        return Some(DoorAddNameShape {
            kind: DoorAddNameKind::TlkLocString,
            legacy_model_token: None,
        });
    }

    let legacy_tail_end = name_offset.checked_add(2 + CNW_LENGTH_BYTES)?;
    if legacy_tail_end == record_end
        && read_u16_le(bytes, name_offset).is_some()
        && read_u32_le(bytes, name_offset + 2) == Some(0)
    {
        // Local Diamond server captures expose this compact source-only shape
        // for some generic doors: the byte-aligned WORD tail appears before an
        // empty direct CExoString slot, and the fragment tail contains only the
        // four final door BOOLs. EE's `sub_140796DD0` has no equivalent byte
        // ordering, so the rewrite path canonicalizes this exact layout to the
        // EE reader order. Keep this deliberately narrow; arbitrary six-byte
        // tails must quarantine instead of being guessed.
        Some(DoorAddNameShape {
            kind: DoorAddNameKind::CompactTailBeforeEmptyDirectName,
            legacy_model_token: None,
        })
    } else if name_offset.checked_add(4 + 2) == Some(record_end)
        && read_u32_le(bytes, name_offset).is_some()
        && read_u16_le(bytes, name_offset + 4).is_some()
    {
        // Normal legacy HG door rows can carry a four-byte short StrRef/name
        // token before the final WORD state. EE's door add reader has no
        // matching read-buffer slot, so the writer normalizes this exact
        // six-byte suffix to an empty direct CExoString plus the same state
        // WORD.
        Some(DoorAddNameShape {
            kind: DoorAddNameKind::ShortStrRef,
            legacy_model_token: None,
        })
    } else {
        None
    }
}

fn legacy_compact_door_add_omits_second_dword_for_ee(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> bool {
    let compact_name_offset = record_offset + 10;
    let normal_name_offset = record_offset + 14;
    legacy_door_add_name_shape_at(bytes, normal_name_offset, record_end).is_none()
        && matches!(
            legacy_door_add_name_shape_at(bytes, compact_name_offset, record_end)
                .map(|shape| shape.kind),
            Some(DoorAddNameKind::TlkLocString)
        )
}

fn looks_like_legacy_door_tlk_locstring_at(
    bytes: &[u8],
    name_offset: usize,
    record_end: usize,
) -> bool {
    const LEGACY_DOOR_TLK_LOCSTRING_BYTES: usize = 1 + 4;
    if name_offset > record_end
        || record_end > bytes.len()
        || name_offset.checked_add(LEGACY_DOOR_TLK_LOCSTRING_BYTES + 2) != Some(record_end)
    {
        return false;
    }

    // `sub_1409735F0` reads inner/client-tlk BOOL=true, then ReadBYTE(1, 1)
    // and ReadDWORD(32) before returning to the door reader, which consumes the
    // final WORD tail. `ReadBYTE(1, 1)` yields the observed 0/1 client TLK
    // selector in HG captures; keep this exact so arbitrary non-string bytes do
    // not become a generic door-name escape hatch.
    matches!(bytes[name_offset], 0 | 1)
        && read_u32_le(bytes, name_offset + 1).is_some()
        && read_u16_le(bytes, name_offset + LEGACY_DOOR_TLK_LOCSTRING_BYTES).is_some()
}

fn looks_like_legacy_door_model_token_at(
    bytes: &[u8],
    token_offset: usize,
    record_end: usize,
) -> bool {
    const LEGACY_DOOR_MODEL_TOKEN_BYTES: usize = 4;
    if token_offset > record_end
        || record_end > bytes.len()
        || record_end - token_offset != LEGACY_DOOR_MODEL_TOKEN_BYTES + 2
    {
        return false;
    }

    let token = &bytes[token_offset..token_offset + LEGACY_DOOR_MODEL_TOKEN_BYTES];
    token.iter().any(|byte| byte.is_ascii_alphanumeric())
        && token
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
        && read_u16_le(bytes, token_offset + LEGACY_DOOR_MODEL_TOKEN_BYTES).is_some()
}

fn decode_cnw_msb_valid_bits(fragment: &[u8]) -> Option<Vec<bool>> {
    let first = *fragment.first()?;
    let final_fragment_bits = ((first & 0xE0) >> 5) as usize;
    let valid_bits = if final_fragment_bits == 0 {
        fragment.len().checked_mul(8)?
    } else {
        fragment
            .len()
            .checked_sub(1)?
            .checked_mul(8)?
            .checked_add(final_fragment_bits)?
    };
    if valid_bits < CNW_FRAGMENT_HEADER_BITS {
        return None;
    }

    let mut bits = Vec::with_capacity(valid_bits);
    for bit_index in 0..valid_bits {
        let byte = *fragment.get(bit_index / 8)?;
        bits.push((byte & (0x80 >> (bit_index % 8))) != 0);
    }
    Some(bits)
}

fn pack_cnw_msb_valid_bits(mut bits: Vec<bool>) -> Vec<u8> {
    if bits.len() < CNW_FRAGMENT_HEADER_BITS {
        return Vec::new();
    }
    let final_fragment_bits = bits.len() % 8;
    bits[0] = (final_fragment_bits & 0x04) != 0;
    bits[1] = (final_fragment_bits & 0x02) != 0;
    bits[2] = (final_fragment_bits & 0x01) != 0;

    let mut packed = vec![0u8; bits.len().div_ceil(8)];
    for (bit_index, bit) in bits.into_iter().enumerate() {
        if bit {
            packed[bit_index / 8] |= 0x80 >> (bit_index % 8);
        }
    }
    packed
}

fn set_cnw_msb_bit(bits: &mut [bool], bit_index: usize, value: bool) -> Option<bool> {
    let bit = bits.get_mut(bit_index)?;
    let changed = *bit != value;
    *bit = value;
    Some(changed)
}

fn insert_cnw_msb_bit(bits: &mut Vec<bool>, bit_index: usize, value: bool) -> Option<()> {
    if bit_index > bits.len() {
        return None;
    }
    bits.insert(bit_index, value);
    Some(())
}

fn legacy_placeable_add_tail_end(
    bytes: &[u8],
    tail_offset: usize,
    record_end: usize,
    allow_optional_object: bool,
) -> Option<usize> {
    let full_tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if tail_offset > record_end || full_tail_end > record_end || full_tail_end > bytes.len() {
        let compact_tail_end = tail_offset.checked_add(1 + 2 + 1)?;
        if compact_tail_end <= record_end
            && compact_tail_end <= bytes.len()
            && (compact_tail_end == record_end
                || has_ee_identity_visual_transform_map_at(bytes, compact_tail_end, record_end)
                || crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
                    bytes,
                    compact_tail_end,
                    record_end,
                ))
        {
            return Some(compact_tail_end);
        }
        return None;
    }
    if full_tail_end == record_end
        || has_ee_identity_visual_transform_map_at(bytes, full_tail_end, record_end)
        || crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
            bytes,
            full_tail_end,
            record_end,
        )
    {
        Some(full_tail_end)
    } else {
        if allow_optional_object {
            let optional_tail_end = full_tail_end.checked_add(4)?;
            if optional_tail_end <= record_end
                && optional_tail_end <= bytes.len()
                && (optional_tail_end == record_end
                    || has_ee_identity_visual_transform_map_at(
                        bytes,
                        optional_tail_end,
                        record_end,
                    )
                    || crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
                        bytes,
                        optional_tail_end,
                        record_end,
                    ))
            {
                return Some(optional_tail_end);
            }
        }
        let compact_tail_end = tail_offset.checked_add(1 + 2 + 1)?;
        if compact_tail_end <= record_end
            && compact_tail_end <= bytes.len()
            && (compact_tail_end == record_end
                || has_ee_identity_visual_transform_map_at(bytes, compact_tail_end, record_end)
                || crate::translate::live_object_update::visual_transform::has_legacy_scalar_visual_transform_identity_at(
                    bytes,
                    compact_tail_end,
                    record_end,
                ))
        {
            Some(compact_tail_end)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LegacyPlaceableEmptyInlineFallbackName {
    length_offset: usize,
    text_start: usize,
    text_end: usize,
    tail_start: usize,
    legacy_tail_end: usize,
}

fn apply_legacy_placeable_empty_inline_fallback_name(
    bytes: &mut Vec<u8>,
    record_end: &mut usize,
    recovered: LegacyPlaceableEmptyInlineFallbackName,
) -> Option<(usize, usize)> {
    let recovered_len = u32::try_from(recovered.text_end - recovered.text_start).ok()?;
    let text_base = recovered.length_offset.checked_add(CNW_LENGTH_BYTES)?;
    write_u32_le(bytes, recovered.length_offset, recovered_len)?;

    let mut text_end = recovered.text_end;
    let mut tail_start = recovered.tail_start;
    let mut legacy_tail_end = recovered.legacy_tail_end;
    if recovered.text_start > text_base {
        let removed = recovered.text_start - text_base;
        bytes.drain(text_base..recovered.text_start);
        *record_end = record_end.checked_sub(removed)?;
        text_end = text_end.checked_sub(removed)?;
        tail_start = tail_start.checked_sub(removed)?;
        legacy_tail_end = legacy_tail_end.checked_sub(removed)?;
    }
    if tail_start > text_end {
        let removed = tail_start - text_end;
        bytes.drain(text_end..tail_start);
        *record_end = record_end.checked_sub(removed)?;
        legacy_tail_end = legacy_tail_end.checked_sub(removed)?;
    }
    Some((text_end, legacy_tail_end))
}

fn try_find_legacy_placeable_empty_inline_fallback_name(
    bytes: &[u8],
    name_offset: usize,
    record_end: usize,
    allow_optional_object: bool,
) -> Option<LegacyPlaceableEmptyInlineFallbackName> {
    if name_offset > record_end
        || record_end > bytes.len()
        || record_end - name_offset < CNW_LENGTH_BYTES + 1 + 1 + 2 + 2
        || read_u32_le(bytes, name_offset)? != 0
    {
        return None;
    }

    if legacy_placeable_add_tail_end(
        bytes,
        name_offset + CNW_LENGTH_BYTES,
        record_end,
        allow_optional_object,
    )
    .is_some()
    {
        return None;
    }

    let base_text_start = name_offset + CNW_LENGTH_BYTES;
    for prefix_skip in 0usize..=LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES {
        let text_start = base_text_start.checked_add(prefix_skip)?;
        if text_start >= record_end {
            break;
        }
        let text_limit = text_start
            .saturating_add(MAX_LIVE_OBJECT_NAME_BYTES)
            .min(record_end);
        for text_end in text_start + 1..=text_limit {
            if !is_legacy_bare_placeable_name_byte(bytes[text_end - 1]) {
                break;
            }
            let padding_limit = text_end.saturating_add(4).min(record_end);
            for tail_start in text_end..=padding_limit {
                if bytes[text_end..tail_start].iter().any(|byte| *byte != 0) {
                    break;
                }
                if let Some(legacy_tail_end) = legacy_placeable_add_tail_end(
                    bytes,
                    tail_start,
                    record_end,
                    allow_optional_object,
                ) {
                    return Some(LegacyPlaceableEmptyInlineFallbackName {
                        length_offset: name_offset,
                        text_start,
                        text_end,
                        tail_start,
                        legacy_tail_end,
                    });
                }
            }
        }
    }

    for prefix_skip in 0usize..=LEGACY_PLACEABLE_EMPTY_NAME_PREFIX_SCAN_BYTES {
        let text_start = base_text_start.checked_add(prefix_skip)?;
        if text_start >= record_end {
            break;
        }
        let tail_limit = text_start
            .saturating_add(MAX_LIVE_OBJECT_NAME_BYTES)
            .min(record_end);
        for tail_start in text_start + 1..=tail_limit {
            let text = &bytes[text_start..tail_start];
            if text
                .first()
                .is_none_or(|byte| !is_legacy_bare_placeable_name_byte(*byte))
            {
                break;
            }
            if !text
                .iter()
                .all(|byte| *byte == 0 || is_legacy_bare_placeable_name_byte(*byte))
            {
                break;
            }
            if !text
                .iter()
                .rfind(|byte| **byte != 0)
                .is_some_and(|byte| is_legacy_bare_placeable_name_byte(*byte))
            {
                continue;
            }
            if let Some(legacy_tail_end) =
                legacy_placeable_add_tail_end(bytes, tail_start, record_end, allow_optional_object)
            {
                // Diamond/EE direct `CExoString` readers consume a bounded byte
                // span, not a C string. Local Diamond compact placeable adds can
                // leave zero control/padding bytes inside that span before the
                // decompile-owned tail. Preserve the exact raw bytes as the EE
                // direct-name payload and only repair the missing length prefix.
                return Some(LegacyPlaceableEmptyInlineFallbackName {
                    length_offset: name_offset,
                    text_start,
                    text_end: tail_start,
                    tail_start,
                    legacy_tail_end,
                });
            }
        }
    }

    None
}

fn is_legacy_bare_placeable_name_byte(byte: u8) -> bool {
    matches!(byte, 0x20..=0x7E | b'\t')
}

fn has_ee_identity_visual_transform_map_at(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    let end = offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
    end <= record_end
        && end <= bytes.len()
        && bytes[offset..end] == EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES
}

fn candidate_inside_inline_string(bytes: &[u8], search_start: usize, candidate: usize) -> bool {
    let mut string_offset = search_start;
    while string_offset + 4 <= candidate && string_offset < bytes.len() {
        if let Some(string_end) =
            inline_cexo_string_end_for_boundary_suppression(bytes, string_offset)
        {
            if string_offset + 4 <= candidate && candidate < string_end {
                return true;
            }
        }
        string_offset += 1;
    }
    false
}

fn inline_cexo_string_end_for_boundary_suppression(bytes: &[u8], offset: usize) -> Option<usize> {
    let end = inline_cexo_string_end(bytes, offset)?;
    let text = bytes.get(offset + CNW_LENGTH_BYTES..end)?;
    if text
        .iter()
        .all(|byte| matches!(*byte, 0 | b'\t' | 0x20..=0x7e))
    {
        Some(end)
    } else {
        None
    }
}

fn inline_cexo_string_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let length = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    const MAX_LIVE_OBJECT_NAME_BYTES: usize = 128;
    if length > MAX_LIVE_OBJECT_NAME_BYTES || bytes.len().saturating_sub(offset + 4) < length {
        return None;
    }

    // Decompile-backed CExoString rule: Diamond `sub_44E4A0` and EE
    // `sub_1407A7800` both call `ReadCExoString(32)` for direct placeable
    // names. That reader consumes the declared byte count; it does not reject
    // embedded NUL bytes. HG sign/placeable names can contain NUL padding, so
    // printable-only validation shifts the following live-object records.
    Some(offset + 4 + length)
}

fn looks_like_legacy_live_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(looks_like_legacy_live_object_id_value)
        .unwrap_or(false)
}

fn looks_like_legacy_live_gui_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(|object_id| {
            object_id == 0x7F00_0000
                || object_id == u32::MAX
                || looks_like_legacy_live_object_id_value(object_id)
        })
        .unwrap_or(false)
}

fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
    crate::translate::live_object_update::object_ids::looks_like_legacy_live_object_id_value(
        object_id,
    )
}

fn is_ee_live_gui_sub_opcode_byte(value: u8) -> bool {
    matches!(
        value,
        b'A' | b'B' | b'C' | b'I' | b'M' | b'Q' | b'R' | b'S' | b'c' | b'i' | b'r'
    )
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    bytes
        .get_mut(offset..offset + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Option<f32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod live_object_id_tests {
    use super::*;

    #[test]
    fn low_compact_placeable_continuation_uses_compact_object_id_guard() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[b'A', PLACEABLE_OBJECT_TYPE]);
        bytes.extend_from_slice(&0x0000_1234u32.to_le_bytes());
        bytes.extend_from_slice(&[b'I', 0x21, 0x43, 0x65, 0x87]);
        bytes.extend_from_slice(&[0x00, 0x21, 0x43, 0x65, 0x87]);

        assert!(looks_like_hg_low_compact_placeable_continuation_at(
            &bytes,
            0,
            bytes.len(),
        ));

        bytes[2..6].copy_from_slice(&0u32.to_le_bytes());
        assert!(!looks_like_hg_low_compact_placeable_continuation_at(
            &bytes,
            0,
            bytes.len(),
        ));

        bytes[2..6].copy_from_slice(&0x8000_1234u32.to_le_bytes());
        assert!(!looks_like_hg_low_compact_placeable_continuation_at(
            &bytes,
            0,
            bytes.len(),
        ));
    }

    #[test]
    fn compact_placeable_update_like_tail_accepts_compact_external_id_alias() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[
            b'A',
            PLACEABLE_OBJECT_TYPE,
            0x84,
            0x00,
            0x00,
            0x80,
            0x18,
            0x16,
            0x00,
            0x00,
            0x05,
            0x00,
            0x00,
            0x00,
            0x00,
        ]);
        bytes.extend_from_slice(&[
            0x00,
            PLACEABLE_OBJECT_TYPE,
            0x84,
            0x00,
            0x00,
            0x00,
            0xF7,
            0x00,
            0x00,
            0x00,
        ]);

        assert_eq!(
            compact_placeable_add_end_before_same_object_update_like_tail(&bytes, 0, bytes.len()),
            Some(15),
            "compact add/update-like split must use the same external/compact id alias rule as the exact update verifier"
        );
    }
}

#[cfg(test)]
mod placeable_add_semantic_tests {
    use super::*;

    fn inline_placeable_add_record() -> (Vec<u8>, usize) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[b'A', PLACEABLE_OBJECT_TYPE, 0x85, 0x00, 0x00, 0x80]);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"Lamp");
        bytes.push(5);
        bytes.extend_from_slice(&0x000Eu16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let record_end = bytes.len();
        (bytes, record_end)
    }

    fn compact_short_name_placeable_add_record() -> (Vec<u8>, usize) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[b'A', PLACEABLE_OBJECT_TYPE, 0xB6, 0x00, 0x00, 0x80]);
        bytes.extend_from_slice(&[0x18, 0x16, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x05, 0x11, 0x00, 0x00, 0x00]);
        let record_end = bytes.len();
        (bytes, record_end)
    }

    fn assert_ee_placeable_add_state_bits(
        bits: &[bool],
        post_name_bit: usize,
        expected: PlaceableAddStateBits,
        expected_optional_target: bool,
    ) {
        assert_eq!(bits.get(post_name_bit), Some(&expected.reputation_visual));
        assert_eq!(bits.get(post_name_bit + 1), Some(&expected_optional_target));
        assert_eq!(bits.get(post_name_bit + 2), Some(&expected.static_plot));
        assert_eq!(bits.get(post_name_bit + 3), Some(&expected.useable));
        assert_eq!(bits.get(post_name_bit + 4), Some(&expected.trap_disarmable));
        assert_eq!(bits.get(post_name_bit + 5), Some(&expected.lockable));
        assert_eq!(bits.get(post_name_bit + 6), Some(&expected.locked));
        assert_eq!(bits.get(post_name_bit + 7), Some(&expected.unknown_1ac));
        assert_eq!(bits.get(post_name_bit + 8), Some(&expected.name_valid));
        assert_eq!(bits.get(post_name_bit + 9), Some(&false));
    }

    #[test]
    fn placeable_add_rewrite_preserves_shared_state_bits_and_neutralizes_absent_optional_target() {
        let (mut bytes, mut record_end) = inline_placeable_add_record();
        let mut bits = vec![
            false, // direct CExoString name branch.
            true,  // legacy reputation/visual selector.
            true,  // stale optional-target-like bit, but no guarded OBJECTID bytes.
            true,  // static/plot.
            true,  // useable.
            false, // trap disarmable: non-trapped visuals must stay non-trapped.
            true,  // lockable.
            false, // locked.
            true,  // unknown 0x1AC sibling.
            true,  // name-valid.
        ];
        let expected = legacy_placeable_add_state_bits(&bits, 0, 0);
        let mut bit_cursor = 0usize;

        let rewrite = rewrite_legacy_placeable_add_record_for_ee(
            &mut bytes,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            0,
            None,
        )
        .expect("placeable add should rewrite through the typed state-bit mapper");

        assert_eq!(rewrite.maps_inserted, 1);
        assert_eq!(bit_cursor, 11);
        assert_ee_placeable_add_state_bits(&bits, 1, expected, false);
        assert!(has_ee_identity_visual_transform_map_at(
            &bytes, 19, record_end
        ));

        let mut verified_cursor = 0usize;
        assert!(
            crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                &bytes,
                0,
                record_end,
                &bits,
                &mut verified_cursor,
            )
        );
        assert_eq!(verified_cursor, bit_cursor);
    }

    #[test]
    fn placeable_add_rewrite_preserves_real_optional_target_guard() {
        let (mut bytes, mut record_end) = inline_placeable_add_record();
        bytes.extend_from_slice(&0x8000_1234u32.to_le_bytes());
        record_end += 4;
        let mut bits = vec![
            false, false, true, false, false, false, false, false, false, true,
        ];
        let expected = legacy_placeable_add_state_bits(&bits, 0, 0);
        let mut bit_cursor = 0usize;

        rewrite_legacy_placeable_add_record_for_ee(
            &mut bytes,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            0,
            None,
        )
        .expect("placeable add with guarded OBJECTID should rewrite");

        assert_eq!(bit_cursor, 11);
        assert_ee_placeable_add_state_bits(&bits, 1, expected, true);
        assert_eq!(read_u32_le(&bytes, 19), Some(0x8000_1234));
        assert!(has_ee_identity_visual_transform_map_at(
            &bytes, 23, record_end
        ));
    }

    #[test]
    fn placeable_add_replaces_legacy_scalar_visual_transform_at_tail_cursor() {
        let (mut bytes, mut record_end) = inline_placeable_add_record();
        let visual_offset = record_end;
        bytes.extend_from_slice(
            &crate::translate::live_object_update::visual_transform::LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        record_end = bytes.len();
        let mut bits = vec![
            false, // direct CExoString name branch.
            true, false, true, false, true, false, true, false, true,
        ];
        let expected = legacy_placeable_add_state_bits(&bits, 0, 0);
        let mut verified_cursor = 0usize;
        assert!(
            !crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                &bytes,
                0,
                record_end,
                &bits,
                &mut verified_cursor,
            ),
            "legacy scalar transform bytes must not be accepted as an EE object map"
        );

        let mut bit_cursor = 0usize;
        let rewrite = rewrite_legacy_placeable_add_record_for_ee(
            &mut bytes,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            0,
            None,
        )
        .expect("legacy scalar placeable add transform should rewrite");

        assert_eq!(rewrite.maps_inserted, 1);
        assert_eq!(rewrite.bytes_removed, 40);
        assert_eq!(
            rewrite.bytes_inserted,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32
        );
        assert_eq!(
            record_end,
            visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()
        );
        assert!(has_ee_identity_visual_transform_map_at(
            &bytes,
            visual_offset,
            record_end
        ));
        assert_eq!(bit_cursor, 11);
        assert_ee_placeable_add_state_bits(&bits, 1, expected, false);

        let mut verified_cursor = 0usize;
        assert!(
            crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                &bytes,
                0,
                record_end,
                &bits,
                &mut verified_cursor,
            ),
            "rewritten placeable add must exact-claim at the EE object-map cursor"
        );
        assert_eq!(verified_cursor, bit_cursor);
    }

    #[test]
    fn compact_placeable_add_drains_any_bounded_source_residue_before_ee_guards() {
        let cases: &[(&str, &[bool])] = &[
            ("empty", &[]),
            ("one-bit", &[true]),
            ("two-bit", &[true, false]),
            ("three-bit", &[true, false, true]),
            ("alternating-four", &[true, false, true, false]),
            ("xp2-1101", &[true, true, false, true]),
            ("xp2-0001", &[false, false, false, true]),
            ("xp2-0010", &[false, false, true, false]),
            ("xp2-1110", &[true, true, true, false]),
        ];
        for (label, residue) in cases {
            let (mut bytes, mut record_end) = compact_short_name_placeable_add_record();
            let mut bits = residue.to_vec();
            let mut bit_cursor = 0usize;

            let rewrite = rewrite_legacy_placeable_add_record_for_ee(
                &mut bytes,
                &mut record_end,
                &mut bits,
                &mut bit_cursor,
                0,
                None,
            )
            .expect("compact placeable add residue should rewrite through the same bounded rule");

            assert_eq!(rewrite.maps_inserted, 1);
            assert_eq!(bit_cursor, 12);
            assert_eq!(bits.len(), 12);
            assert_eq!(bits[0], true, "outer locstring branch for residue {label}");
            assert!(
                bits[1..].iter().all(|bit| !*bit),
                "compact source residue {label} should be replaced by neutral EE guards"
            );
            assert!(has_ee_identity_visual_transform_map_at(
                &bytes, 15, record_end
            ));

            let mut verified_cursor = 0usize;
            assert!(
                crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                    &bytes,
                    0,
                    record_end,
                    &bits,
                    &mut verified_cursor,
                ),
                "rewritten compact add with {label} residual bits should exact-claim"
            );
            assert_eq!(verified_cursor, bit_cursor);
        }
    }
}

#[cfg(test)]
mod placeable_name_mode_tests {
    use super::*;

    fn inline_placeable_add_record() -> (Vec<u8>, usize) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[b'A', PLACEABLE_OBJECT_TYPE, 0xDC, 0x34, 0x00, 0x80]);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"Lamp");
        bytes.push(5);
        bytes.extend_from_slice(&2025u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let record_end = bytes.len();
        (bytes, record_end)
    }

    #[test]
    fn outer_true_inner_false_inline_placeable_name_keeps_locstring_inline_branch() {
        let (mut bytes, mut record_end) = inline_placeable_add_record();
        let mut bits = vec![
            true, false, true, false, true, false, true, false, true, false, true,
        ];
        let mut bit_cursor = 0usize;

        let rewrite = rewrite_legacy_placeable_add_record_for_ee(
            &mut bytes,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            0,
            None,
        )
        .expect("inline locstring placeable add should rewrite");

        assert_eq!(rewrite.maps_inserted, 1);
        assert_eq!(
            rewrite.bytes_inserted,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32
        );
        assert_eq!(bit_cursor, 12);
        assert!(bits[0], "outer locstring branch must remain selected");
        assert!(
            !bits[1],
            "inline locstring inner/client-tlk bit must remain false"
        );
        assert!(has_ee_identity_visual_transform_map_at(
            &bytes, 19, record_end
        ));

        let mut verified_cursor = 0usize;
        assert!(
            crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                &bytes,
                0,
                record_end,
                &bits,
                &mut verified_cursor,
            )
        );
        assert_eq!(verified_cursor, bit_cursor);
    }

    #[test]
    fn outer_true_inner_true_inline_placeable_name_uses_direct_name_repair() {
        let (mut bytes, mut record_end) = inline_placeable_add_record();
        let mut bits = vec![
            true, true, true, false, true, false, true, false, true, false,
        ];
        let mut bit_cursor = 0usize;

        let _rewrite = rewrite_legacy_placeable_add_record_for_ee(
            &mut bytes,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            0,
            None,
        )
        .expect("contradictory direct-name placeable add should rewrite");

        assert_eq!(bit_cursor, 11);
        assert!(
            !bits[0],
            "outer=true, inner=true with inline bytes is the decompile-invalid direct-name repair shape"
        );
        assert!(has_ee_identity_visual_transform_map_at(
            &bytes, 19, record_end
        ));

        let mut verified_cursor = 0usize;
        assert!(
            crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                &bytes,
                0,
                record_end,
                &bits,
                &mut verified_cursor,
            )
        );
        assert_eq!(verified_cursor, bit_cursor);
    }
}

#[cfg(test)]
mod door_add_visual_transform_tests {
    use super::*;

    fn inline_door_add_with_legacy_scalar_transform() -> (Vec<u8>, usize, usize) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[b'A', DOOR_OBJECT_TYPE]);
        bytes.extend_from_slice(&0x8000_4400u32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_000Cu32.to_le_bytes());
        let visual_offset = bytes.len();
        bytes.extend_from_slice(
            &crate::translate::live_object_update::visual_transform::LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"Gate");
        bytes.extend_from_slice(&0x0016u16.to_le_bytes());
        let record_end = bytes.len();
        (bytes, record_end, visual_offset)
    }

    #[test]
    fn door_add_replaces_legacy_scalar_visual_transform_at_name_cursor() {
        let (mut bytes, mut record_end, visual_offset) =
            inline_door_add_with_legacy_scalar_transform();
        let mut bits = vec![false, true, false, true, false, true];
        let mut verified_cursor = 0usize;
        assert!(
            !crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                &bytes,
                0,
                record_end,
                &bits,
                &mut verified_cursor,
            ),
            "legacy scalar transform bytes must not be treated as the EE door object map"
        );

        let mut bit_cursor = 0usize;
        let rewrite = rewrite_legacy_door_add_record_for_ee(
            &mut bytes,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            0,
            None,
        )
        .expect("legacy scalar door add transform should rewrite");

        assert_eq!(rewrite.maps_inserted, 1);
        assert_eq!(rewrite.bytes_removed, 40);
        assert_eq!(
            rewrite.bytes_inserted,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32
        );
        assert!(has_ee_identity_visual_transform_map_at(
            &bytes,
            visual_offset,
            record_end
        ));
        assert_eq!(
            read_u32_le(
                &bytes,
                visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()
            ),
            Some(4),
            "door name must begin immediately after the EE object visual-transform map"
        );
        assert_eq!(bit_cursor, 6);

        let mut verified_cursor = 0usize;
        assert!(
            crate::translate::live_object_update::advance_verified_add_fragment_cursor_for_ee(
                &bytes,
                0,
                record_end,
                &bits,
                &mut verified_cursor,
            ),
            "rewritten door add must exact-claim at the EE object-map cursor"
        );
        assert_eq!(verified_cursor, bit_cursor);
    }
}

#[cfg(test)]
mod declared_length_repair_tests {
    use super::*;

    fn append_legacy_door_add(live: &mut Vec<u8>, object_id: u32, second_dword: u32) {
        live.push(b'A');
        live.push(DOOR_OBJECT_TYPE);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(&second_dword.to_le_bytes());
        live.extend_from_slice(&4u32.to_le_bytes());
        live.extend_from_slice(b"Door");
        live.extend_from_slice(&0x0016u16.to_le_bytes());
    }

    fn legacy_trigger_update_with_tail(tail: [u8; 3]) -> Vec<u8> {
        let mut live = vec![b'U', TRIGGER_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_1007u32.to_le_bytes());
        live.extend_from_slice(&0xFFFF_FFF3u32.to_le_bytes());
        live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
        live.extend_from_slice(&tail);
        live
    }

    fn door_placeable_state_update_live_bytes(object_type: u8) -> Vec<u8> {
        let object_id: u32 = match object_type {
            PLACEABLE_OBJECT_TYPE => 0x8000_3409,
            DOOR_OBJECT_TYPE => 0x8000_340A,
            _ => 0x8000_3400,
        };
        let mut live = vec![b'U', object_type];
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&LEGACY_UPDATE_STATE_MASK.to_le_bytes());
        live
    }

    fn item_hidden_update_live_bytes() -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_3406u32.to_le_bytes());
        live.extend_from_slice(&0x0000_0040u32.to_le_bytes());
        live
    }

    fn item_full_scalar_direct_name_update_live_bytes(name: &[u8]) -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
        live.extend_from_slice(&0xFFFF_FFF3u32.to_le_bytes());
        live.extend_from_slice(&[0xB7, 0x05, 0xC1, 0x04, 0x0F, 0x0F]);
        live.push(0); // scalar-orientation read byte.
        live.extend_from_slice(&0xFFFFu16.to_le_bytes());
        live.extend_from_slice(&[0; 16]);
        live.extend_from_slice(&(name.len() as u32).to_le_bytes());
        live.extend_from_slice(name);
        live
    }

    fn item_full_scalar_direct_name_update_bits() -> Vec<bool> {
        vec![
            false, true, // position residual bits.
            false, true, false, true, false, // scalar orientation selector and residual bits.
            true, false, true, false, true,  // item state bits.
            false, // direct CExoString item name.
        ]
    }

    fn creature_zero_update_live_bytes() -> Vec<u8> {
        let mut live = vec![b'U', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live
    }

    fn creature_delete_live_bytes() -> Vec<u8> {
        let mut live = vec![b'D', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live
    }

    fn creature_zero_appearance_live_bytes() -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&0u16.to_le_bytes());
        live
    }

    fn creature_ignored_high_mask_appearance_live_bytes() -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&0x8000u16.to_le_bytes());
        live
    }

    fn creature_name_only_direct_appearance_live_bytes(object_id: u32) -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&0x0400u16.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live
    }

    fn creature_update_3967_action0_scalar_live_bytes(object_id: u32) -> Vec<u8> {
        let mut live = vec![b'U', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&0x0000_3967u32.to_le_bytes());
        live.extend_from_slice(&0x1111u16.to_le_bytes());
        live.extend_from_slice(&0x2222u16.to_le_bytes());
        live.extend_from_slice(&0x3333u16.to_le_bytes());
        live.push(0x44);
        live.extend_from_slice(&0u16.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(&0u16.to_le_bytes());
        live.extend_from_slice(&0u16.to_le_bytes());
        live.push(0);
        live.extend_from_slice(&0x1234u16.to_le_bytes());
        live.push(1);
        live.extend_from_slice(&0x5678u16.to_le_bytes());
        live.push(2);
        live.extend_from_slice(&0x1111_1111u32.to_le_bytes());
        live.extend_from_slice(&0x2222_2222u32.to_le_bytes());
        live.push(0);
        live.extend_from_slice(&0u16.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.push(0);
        live.extend_from_slice(&0u16.to_le_bytes());
        live.extend_from_slice(&0u16.to_le_bytes());
        live.push(0);
        live.extend_from_slice(&0x8000_000Bu32.to_le_bytes());
        live.extend_from_slice(&0u16.to_le_bytes());
        live
    }

    fn creature_update_3967_action0_scalar_bits() -> Vec<bool> {
        vec![
            true, false, // position residual bits.
            false, // scalar orientation branch.
            true, false, true, false, // scalar orientation residual bits.
            false, // explicit orientation-target guard.
            true,  // 0x0040 state BOOL.
            false, true, // identity branch BOOLs.
            true, false, // associate suffix BOOLs.
        ]
    }

    fn legacy_creature_add_live_bytes(object_id: u32) -> Vec<u8> {
        let mut live = vec![b'A', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&object_id.to_le_bytes());
        for value in [1.0f32, 2.0, 3.0, 0.0, 0.0, 0.0] {
            live.extend_from_slice(&value.to_le_bytes());
        }
        live.extend_from_slice(&0u16.to_le_bytes());
        live
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_cexo_string(bytes: &mut Vec<u8>, text: &[u8]) {
        push_u32(
            bytes,
            u32::try_from(text.len()).expect("test string length fits"),
        );
        bytes.extend_from_slice(text);
    }

    fn push_full_creature_appearance_tail_fields(bytes: &mut Vec<u8>) {
        const LEGACY_APPEARANCE_BODY_PART_COUNT: u8 = 0x13;

        push_u16(bytes, 2);
        bytes.extend_from_slice(&[1, 2]);
        bytes.push(3);
        push_u32(bytes, 0x1122_3344);
        push_u32(bytes, 0x5566_7788);
        bytes.extend_from_slice(&[4, 5, 6, 7]);

        bytes.push(LEGACY_APPEARANCE_BODY_PART_COUNT);
        for part in 0..LEGACY_APPEARANCE_BODY_PART_COUNT {
            bytes.push(part);
        }

        push_u16(bytes, 0x99AA);
        push_u32(bytes, 0xBBCC_DDEE);
    }

    fn push_no_name_active_property_tail(bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        bytes.extend_from_slice(&[0x55, 0x66, 0x77, 0x88]);
        bytes.push(0);
        bytes.extend_from_slice(&[0, 0]);
    }

    fn push_visible_equipment_no_name_item(bytes: &mut Vec<u8>) {
        bytes.push(b'A');
        push_u32(bytes, 0x8000_0042);
        push_u32(bytes, 2);
        push_u32(bytes, 0x01);
        bytes.extend_from_slice(&[0x07, 0x08, 0x09, 0x0A]);
        push_no_name_active_property_tail(bytes);
    }

    fn full_creature_appearance_with_direct_name_and_no_name_equipment(object_id: u32) -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&0xFFFFu16.to_le_bytes());
        push_cexo_string(&mut live, b"Hero");
        push_full_creature_appearance_tail_fields(&mut live);
        live.push(1);
        push_visible_equipment_no_name_item(&mut live);
        live
    }

    fn direct_name_with_no_name_equipment_bits() -> Vec<bool> {
        vec![
            false, // creature direct CExoString name.
            true,  // first Diamond active-property BOOL.
            false, // second Diamond active-property BOOL.
            true,  // third Diamond active-property BOOL.
            false, // fourth Diamond active-property BOOL.
        ]
    }

    fn creature_appearance_update_pair_repair(
        live: &[u8],
        payload_len: usize,
    ) -> LiveObjectDeclaredLengthRepairCandidate {
        let old_declared = (LEGACY_LIVE_BYTES_OFFSET + 1) as u32;
        let new_declared = (LEGACY_LIVE_BYTES_OFFSET + live.len()) as u32;
        LiveObjectDeclaredLengthRepairCandidate {
            old_declared,
            new_declared,
            old_payload_length: payload_len,
            read_bytes_length: live.len(),
            fragment_bytes_length: payload_len - usize::try_from(new_declared).unwrap(),
        }
    }

    fn live_object_payload_with_fragment_bits(live: &[u8], fragment_bits: Vec<bool>) -> Vec<u8> {
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        let declared = (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len()) as u32;
        payload.extend_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(live);
        payload.extend_from_slice(&pack_cnw_msb_valid_bits(fragment_bits));
        payload
    }

    fn creature_name_only_empty_appearance_live_bytes() -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&0x0400u16.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live
    }

    fn creature_name_only_token_inline_appearance_live_bytes() -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&0x0400u16.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes()); // First locstring component token ref.
        live.extend_from_slice(&0u32.to_le_bytes()); // Second locstring component empty string.
        live
    }

    fn creature_scalar_only_appearance_live_bytes(mask: u16) -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&mask.to_le_bytes());
        if (mask & 0x0001) != 0 {
            live.extend_from_slice(&0x1234u16.to_le_bytes());
        }
        if (mask & 0x0002) != 0 {
            live.push(0x12);
        }
        if (mask & 0x0004) != 0 {
            live.push(0x34);
        }
        if (mask & 0x0080) != 0 {
            live.push(0x56);
        }
        if (mask & 0x0800) != 0 {
            live.extend_from_slice(&0x0102_0304u32.to_le_bytes());
        }
        if (mask & 0x1000) != 0 {
            live.extend_from_slice(&0x0506_0708u32.to_le_bytes());
        }
        if (mask & 0x0008) != 0 {
            live.push(0x78);
        }
        if (mask & 0x0010) != 0 {
            live.push(0x9A);
        }
        if (mask & 0x0020) != 0 {
            live.push(0xBC);
        }
        if (mask & 0x0040) != 0 {
            live.push(0xDE);
        }
        live
    }

    fn creature_direct_name_body_part_delta_appearance_live_bytes(
        mask: u16,
        selector: u8,
        name: &[u8],
    ) -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&mask.to_le_bytes());
        live.extend_from_slice(&(name.len() as u32).to_le_bytes());
        live.extend_from_slice(name);
        if (mask & 0x0001) != 0 {
            live.extend_from_slice(&0x1234u16.to_le_bytes());
        }
        if (mask & 0x0002) != 0 {
            live.push(0x12);
        }
        if (mask & 0x0004) != 0 {
            live.push(0x34);
        }
        if (mask & 0x0080) != 0 {
            live.push(0x56);
        }
        if (mask & 0x0800) != 0 {
            live.extend_from_slice(&0x0102_0304u32.to_le_bytes());
        }
        if (mask & 0x1000) != 0 {
            live.extend_from_slice(&0x0506_0708u32.to_le_bytes());
        }
        if (mask & 0x0008) != 0 {
            live.push(0x78);
        }
        if (mask & 0x0010) != 0 {
            live.push(0x9A);
        }
        if (mask & 0x0020) != 0 {
            live.push(0xBC);
        }
        if (mask & 0x0040) != 0 {
            live.push(0xDE);
        }
        live.push(selector);
        if selector < 0x0A {
            for index in 0..selector {
                live.push(index);
                live.push(0x20 + index);
            }
        } else {
            for value in 0..0x13 {
                live.push(0x05 + value);
            }
        }
        if (mask & 0x2000) != 0 {
            live.extend_from_slice(&0x1357u16.to_le_bytes());
            live.extend_from_slice(&0x2468_ACEDu32.to_le_bytes());
        }
        if (mask & 0x0200) != 0 {
            live.push(0);
        }
        live
    }

    fn creature_locstring_name_body_part_delta_appearance_live_bytes(
        mask: u16,
        selector: u8,
    ) -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&mask.to_le_bytes());
        live.extend_from_slice(&0x0100_75D6u32.to_le_bytes()); // First locstring component token.
        live.extend_from_slice(&0u32.to_le_bytes()); // Second locstring component empty string.
        live.push(selector);
        live
    }

    fn creature_body_part_delta_appearance_live_bytes(mask: u16, selector: u8) -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&mask.to_le_bytes());
        if (mask & 0x0001) != 0 {
            live.extend_from_slice(&0x1234u16.to_le_bytes());
        }
        if (mask & 0x0002) != 0 {
            live.push(0x12);
        }
        if (mask & 0x0004) != 0 {
            live.push(0x34);
        }
        if (mask & 0x0080) != 0 {
            live.push(0x56);
        }
        if (mask & 0x0800) != 0 {
            live.extend_from_slice(&0x0102_0304u32.to_le_bytes());
        }
        if (mask & 0x1000) != 0 {
            live.extend_from_slice(&0x0506_0708u32.to_le_bytes());
        }
        if (mask & 0x0008) != 0 {
            live.push(0x78);
        }
        if (mask & 0x0010) != 0 {
            live.push(0x9A);
        }
        if (mask & 0x0020) != 0 {
            live.push(0xBC);
        }
        if (mask & 0x0040) != 0 {
            live.push(0xDE);
        }
        live.push(selector);
        if selector < 0x0A {
            for index in 0..selector {
                live.push(index);
                live.push(0x20 + index);
            }
        } else {
            for value in 0..0x13 {
                live.push(0x05 + value);
            }
        }
        if (mask & 0x2000) != 0 {
            live.extend_from_slice(&0x1357u16.to_le_bytes());
            live.extend_from_slice(&0x2468_ACEDu32.to_le_bytes());
        }
        if (mask & 0x0200) != 0 {
            live.push(0);
        }
        live
    }

    fn creature_zero_equipment_delta_appearance_live_bytes(mask: u16) -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&mask.to_le_bytes());
        if (mask & 0x0001) != 0 {
            live.extend_from_slice(&0x1234u16.to_le_bytes());
        }
        if (mask & 0x0002) != 0 {
            live.push(0x12);
        }
        if (mask & 0x0004) != 0 {
            live.push(0x34);
        }
        if (mask & 0x0080) != 0 {
            live.push(0x56);
        }
        if (mask & 0x0800) != 0 {
            live.extend_from_slice(&0x0102_0304u32.to_le_bytes());
        }
        if (mask & 0x1000) != 0 {
            live.extend_from_slice(&0x0506_0708u32.to_le_bytes());
        }
        if (mask & 0x0008) != 0 {
            live.push(0x78);
        }
        if (mask & 0x0010) != 0 {
            live.push(0x9A);
        }
        if (mask & 0x0020) != 0 {
            live.push(0xBC);
        }
        if (mask & 0x0040) != 0 {
            live.push(0xDE);
        }
        if (mask & 0x2000) != 0 {
            live.extend_from_slice(&0x1357u16.to_le_bytes());
            live.extend_from_slice(&0x2468_ACEDu32.to_le_bytes());
        }
        live.push(0); // 0x0200 equipment-delta count.
        live
    }

    fn creature_equipment_delta_add_appearance_live_bytes() -> Vec<u8> {
        let mut live = vec![b'P', CREATURE_OBJECT_TYPE];
        live.extend_from_slice(&0x0000_00FEu32.to_le_bytes());
        live.extend_from_slice(&0x0200u16.to_le_bytes());
        live.push(1); // 0x0200 equipment-delta count.
        live.push(b'A');
        live.extend_from_slice(&0x8000_0042u32.to_le_bytes()); // embedded item OBJECTID.
        live.extend_from_slice(&2u32.to_le_bytes()); // visible-equipment slot.
        live.extend_from_slice(&0x01u32.to_le_bytes()); // stock weapon row, model type 2.
        live.extend_from_slice(&[0x07, 0x08, 0x09, 0x0A]); // model-type-2 appearance bytes.
        live.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // shared active-property DWORD.
        live.extend_from_slice(&[0x55, 0x66, 0x77, 0x88]); // shared active-property DWORD.
        live.push(0); // active-property count.
        live.extend_from_slice(&[0, 0]); // value-mask trailer.
        live
    }

    fn compact_inventory_scalar_live_bytes() -> Vec<u8> {
        let mut live = vec![b'I'];
        live.extend_from_slice(&0x8000_342Au32.to_le_bytes());
        live.extend_from_slice(&0x0002u16.to_le_bytes());
        live.extend_from_slice(&0x0102_0304u32.to_le_bytes());
        live
    }

    fn gui_inventory_update_live_bytes() -> Vec<u8> {
        let mut live = vec![b'G', b'I', b'U'];
        live.extend_from_slice(&0x8000_3451u32.to_le_bytes());
        live.extend_from_slice(&0x0003u16.to_le_bytes());
        live.push(0x05);
        live
    }

    fn gui_repository_move_live_bytes() -> Vec<u8> {
        let mut live = vec![b'G', b'R', b'M', 0x02, 0x04];
        live.extend_from_slice(&0x8000_3452u32.to_le_bytes());
        live
    }

    fn compact_placeable_short_name_add_live_bytes() -> Vec<u8> {
        let mut live = vec![b'A', PLACEABLE_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_347Eu32.to_le_bytes());
        live.extend_from_slice(&0x0000_38F2u32.to_le_bytes());
        live.push(0x05);
        live.extend_from_slice(&0x0003u16.to_le_bytes());
        live.extend_from_slice(&0x0000u16.to_le_bytes());
        live
    }

    fn compact_door_tail_only_add_live_bytes() -> Vec<u8> {
        let mut live = vec![b'A', DOOR_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_34CDu32.to_le_bytes());
        live.extend_from_slice(&4u32.to_le_bytes());
        live.extend_from_slice(&0x14E5u16.to_le_bytes());
        live
    }

    fn top_level_model_type2_token_name_item_add_live_bytes() -> Vec<u8> {
        let mut live = vec![b'A'];
        live.extend_from_slice(&0x8001_69DCu32.to_le_bytes());
        live.extend_from_slice(&0x10u32.to_le_bytes());
        live.extend_from_slice(&0x01u32.to_le_bytes());
        for part in [0x17u16, 0x3Fu16, 0x17u16] {
            live.extend_from_slice(&part.to_le_bytes());
        }
        live.push(0);
        live.extend_from_slice(&EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES);
        live.extend_from_slice(&0x0000_380Au32.to_le_bytes());
        live.extend_from_slice(&0x0000_0670u32.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&[0, 0, 0xFF]);
        live.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        live
    }

    fn typed_item_create_live_bytes() -> Vec<u8> {
        let mut live = vec![b'A', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
        live.extend_from_slice(&0x01u32.to_le_bytes());
        for part in [0x0Cu16, 0x0Bu16, 0x0Bu16] {
            live.extend_from_slice(&part.to_le_bytes());
        }
        live.push(0);
        live.extend_from_slice(&EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES);
        live.extend_from_slice(&5u32.to_le_bytes());
        live.extend_from_slice(b"Lance");
        live.extend_from_slice(&2u32.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&[0, 0, 0xFF]);
        live.extend_from_slice(&[0; 8]);
        live
    }

    #[test]
    fn declared_length_window_rejects_w_current_total_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&[b'W', 0x10, 0x20]);

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "the three W bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            live_object_read_prefix_walks_to(&payload, LEGACY_LIVE_BYTES_OFFSET, split),
            "the read side is a complete decompile-owned live-object prefix"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned W current-total bytes remain a read-buffer record"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned W read record"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_gui_read_record_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&[b'G', b'Q', 0x00]);

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "zero-row GQ can masquerade as compact CNW fragment storage"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned GQ remains a read-buffer GUI record, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned short GUI record"
        );
    }

    #[test]
    fn declared_length_window_rejects_interior_short_read_boundary_in_fragment_tail() {
        let short_rows = [
            ("W current-total", vec![b'W', 0x10, 0x20]),
            ("G/Q zero-row", vec![b'G', b'Q', 0x00]),
            {
                let mut delete = vec![b'D', TRIGGER_OBJECT_TYPE];
                delete.extend_from_slice(&0x8000_1007u32.to_le_bytes());
                ("D/7 delete", delete)
            },
        ];

        for (label, row) in short_rows {
            let mut live = Vec::new();
            append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

            let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
            let mut payload = vec![
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR,
            ];
            payload.extend_from_slice(&(split as u32).to_le_bytes());
            payload.extend_from_slice(&live);
            payload.push(0x60);
            payload.extend_from_slice(&row);

            assert!(
                decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
                "{label} after one fragment-looking byte can still decode as compact CNW storage"
            );
            assert!(
                !fragment_tail_starts_with_aligned_short_live_object_read_boundary(&payload, split),
                "{label} is not at the proposed tail start in this regression"
            );

            let repair = LiveObjectDeclaredLengthRepairCandidate {
                old_declared: split as u32,
                new_declared: split as u32,
                old_payload_length: payload.len(),
                read_bytes_length: live.len(),
                fragment_bytes_length: payload.len() - split,
            };
            assert!(
                declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
                "{label} remains a live-object read boundary even when one byte follows the proposed split"
            );
            assert!(
                !declared_length_window_transport_plausible(&payload),
                "a plausible CNW bit shape must not hide an interior short {label} row"
            );
        }
    }

    #[test]
    fn declared_length_window_rejects_short_gui_inventory_repository_rows_as_fragment_tail() {
        let gui_cases = [
            ("G/I update", gui_inventory_update_live_bytes()),
            ("G/R move", gui_repository_move_live_bytes()),
        ];
        for (label, gui_tail) in gui_cases {
            let mut live = Vec::new();
            append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

            let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
            let mut payload = vec![
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR,
            ];
            payload.extend_from_slice(&(split as u32).to_le_bytes());
            payload.extend_from_slice(&live);
            payload.extend_from_slice(&gui_tail);

            assert!(
                decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
                "{label} bytes can masquerade as compact CNW fragment storage"
            );
            assert!(
                fragment_tail_starts_with_aligned_short_gui_read_boundary(&payload, split),
                "{label} remains a decompile-owned GUI read-buffer row"
            );

            let repair = LiveObjectDeclaredLengthRepairCandidate {
                old_declared: split as u32,
                new_declared: split as u32,
                old_payload_length: payload.len(),
                read_bytes_length: live.len(),
                fragment_bytes_length: payload.len() - split,
            };
            assert!(
                declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
                "{label} remains a live GUI read row, not a CNW tail"
            );
            assert!(
                !declared_length_window_transport_plausible(&payload),
                "a plausible CNW bit shape must not steal an aligned {label} row"
            );
        }
    }

    #[test]
    fn declared_length_window_rejects_short_inventory_read_record_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&compact_inventory_scalar_live_bytes());

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "short I/0x0002 bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_inventory_read_boundary(&payload, split),
            "aligned I/0x0002 remains a decompile-owned inventory read record"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned short inventory remains a read-buffer row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned short inventory record"
        );
    }

    #[test]
    fn declared_length_window_rejects_compact_placeable_add_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&compact_placeable_short_name_add_live_bytes());

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "15-byte compact A/9 bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_add_read_boundary(&payload, split),
            "aligned compact A/9 remains a decompile-owned placeable add record"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned compact A/9 remains a live-object add row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned compact A/9 add"
        );
    }

    #[test]
    fn declared_length_window_rejects_compact_door_add_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&compact_door_tail_only_add_live_bytes());

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "12-byte compact A/10 bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_add_read_boundary(&payload, split),
            "aligned compact A/10 remains a decompile-owned door add record"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned compact A/10 remains a live-object add row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned compact A/10 add"
        );
    }

    #[test]
    fn declared_length_window_rejects_top_level_item_add_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let item_add = top_level_model_type2_token_name_item_add_live_bytes();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&item_add);

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "top-level item-add bytes can masquerade as compact CNW fragment storage"
        );
        assert_eq!(
            crate::translate::live_object_update::try_get_legacy_item_add_record_end_for_transport(
                &payload,
                split,
                payload.len(),
            ),
            Some(payload.len()),
            "top-level item adds own the A + object-id + slot + item-body read cursor"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned top-level item add remains a live-object read row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned top-level item add"
        );
    }

    #[test]
    fn declared_length_read_prefix_walks_through_top_level_item_add_record() {
        let mut live = top_level_model_type2_token_name_item_add_live_bytes();
        let item_end = live.len();
        live.extend_from_slice(&[b'W', 0x10, 0x20]);

        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            item_end,
            "the transport scanner must not split inside the top-level item body"
        );
        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "top-level item add plus W current-total is a complete live-object read prefix"
        );
    }

    #[test]
    fn declared_length_read_prefix_walks_through_compact_door_add_record() {
        let mut live = compact_door_tail_only_add_live_bytes();
        let door_add_end = live.len();
        live.extend_from_slice(&[b'W', 0x10, 0x20]);

        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            door_add_end,
            "the transport scanner must split after the compact short door add"
        );
        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "compact short door add plus W current-total is a complete live-object read prefix"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_door_placeable_state_update_as_fragment_tail() {
        for object_type in [PLACEABLE_OBJECT_TYPE, DOOR_OBJECT_TYPE] {
            let mut live = Vec::new();
            append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

            let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
            let mut payload = vec![
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR,
            ];
            payload.extend_from_slice(&(split as u32).to_le_bytes());
            payload.extend_from_slice(&live);
            payload.extend_from_slice(&door_placeable_state_update_live_bytes(object_type));

            assert!(
                decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
                "short U/{object_type:#04X} state update bytes can masquerade as compact CNW fragment storage"
            );
            assert_eq!(
                crate::translate::live_object_update::try_get_verified_door_placeable_update_record_end_for_transport(
                    &payload,
                    split,
                    payload.len(),
                ),
                Some(payload.len()),
                "aligned U/{object_type:#04X} state-only update is a decompile-owned read-buffer row"
            );

            let repair = LiveObjectDeclaredLengthRepairCandidate {
                old_declared: split as u32,
                new_declared: split as u32,
                old_payload_length: payload.len(),
                read_bytes_length: live.len(),
                fragment_bytes_length: payload.len() - split,
            };
            assert!(
                declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
                "aligned U/{object_type:#04X} state-only update remains a read-buffer row, not a CNW tail"
            );
            assert!(
                !declared_length_window_transport_plausible(&payload),
                "a plausible CNW bit shape must not steal an aligned short U/{object_type:#04X} state update"
            );
        }
    }

    #[test]
    fn declared_length_window_rejects_short_item_hidden_update_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&item_hidden_update_live_bytes());

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "short U/6 hidden-state update bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_update_read_boundary(&payload, split),
            "aligned U/6 hidden-state update is a decompile-owned short read-buffer row"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned U/6 hidden-state update remains a read-buffer row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned short U/6 hidden-state update"
        );
    }

    #[test]
    fn declared_length_window_rejects_full_item_update_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let item_update = item_full_scalar_direct_name_update_live_bytes(b"Lance");
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&item_update);

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "full U/6 update bytes can masquerade as compact CNW fragment storage"
        );
        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&payload, split, payload.len()),
            payload.len(),
            "the full item update byte window must stay together until fragment bits prove its cursor"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned full U/6 remains a read-buffer row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned full U/6 item update"
        );
    }

    #[test]
    fn declared_length_capacity_counts_item_update_fragment_bits() {
        // Item `U/6` rows consume their own CNW BOOLs even during transport
        // split preflight. A stale-declared repair candidate must not walk a
        // hidden-state item update as read bytes while leaving its required
        // fragment bit to be reinterpreted by a later row.
        let live = item_hidden_update_live_bytes();
        let header_only = vec![false; CNW_FRAGMENT_HEADER_BITS];
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &header_only,
            ),
            "U/6 hidden-state update owns one item fragment bit"
        );

        let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        enough_bits.push(true);
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &enough_bits,
            ),
            "the same U/6 read prefix is plausible once its hidden-state BOOL is present"
        );
    }

    #[test]
    fn declared_length_capacity_counts_typed_item_create_fragment_bits() {
        // Typed `A/6` item-create rows use the shared item body reader. The
        // transport preflight consumes only Diamond-owned item-name and
        // active-property source bits; EE's inserted active-property BOOL is
        // left for the exact live-object rewrite.
        let live = typed_item_create_live_bytes();
        let mut too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        too_few_bits.extend_from_slice(&[false, false, true, false]);
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &too_few_bits,
            ),
            "A/6 item-create capacity must reject a missing Diamond source BOOL"
        );

        let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        enough_bits.extend_from_slice(&[false, false, true, false, false]);
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &enough_bits,
            ),
            "A/6 item-create capacity must accept the exact Diamond source cursor"
        );
    }

    #[test]
    fn declared_length_capacity_rejects_item_create_borrowing_following_update_bits() {
        // CEP-style `A/6 -> U/6` handoff risk: the typed item-create row owns
        // five Diamond source bits before the following item update starts.
        // Even when the following full U/6 has enough bits to look plausible,
        // the transport preflight must reject a stream that can only make the
        // A/6 proof work by consuming the first U/6 bit.
        let mut live = typed_item_create_live_bytes();
        live.extend_from_slice(&item_full_scalar_direct_name_update_live_bytes(b"Lance"));

        let item_create_bits = [false, false, true, false, false];
        let item_update_bits = item_full_scalar_direct_name_update_bits();

        let mut shifted_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        shifted_bits.extend_from_slice(&item_create_bits[..4]);
        shifted_bits.extend_from_slice(&item_update_bits);
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &shifted_bits,
            ),
            "A/6 capacity must not borrow the following U/6 bit and shift the item update cursor"
        );

        let mut exact_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        exact_bits.extend_from_slice(&item_create_bits);
        exact_bits.extend_from_slice(&item_update_bits);
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &exact_bits,
            ),
            "A/6 plus full U/6 capacity should pass when both records own their exact source bits"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_creature_zero_update_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&creature_zero_update_live_bytes());

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "short U/5 zero-mask update bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_update_read_boundary(&payload, split),
            "aligned U/5 zero-mask update is a decompile-owned short read-buffer row"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned U/5 zero-mask update remains a read-buffer row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned short U/5 zero-mask update"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_creature_zero_appearance_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&creature_zero_appearance_live_bytes());

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "short P/5 zero-mask appearance bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_creature_appearance_read_boundary(
                &payload, split
            ),
            "aligned P/5 zero-mask appearance is a decompile-owned short read-buffer row"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned P/5 zero-mask appearance remains a read-buffer row, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned short P/5 zero-mask appearance"
        );
    }

    #[test]
    fn declared_length_creature_appearance_update_pair_requires_u5_position_bits() {
        let object_id = 0x8000_0042;
        let appearance = creature_name_only_direct_appearance_live_bytes(object_id);
        let update = creature_update_3967_action0_scalar_live_bytes(object_id);
        let update_bits = creature_update_3967_action0_scalar_bits();
        let mut live = appearance;
        live.extend_from_slice(&update);

        let mut exact_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        exact_bits.push(false); // direct CExoString creature name selector.
        exact_bits.extend_from_slice(&update_bits);
        let mut exact_payload = live_object_payload_with_fragment_bits(&live, exact_bits);
        let repair = creature_appearance_update_pair_repair(&live, exact_payload.len());
        exact_payload[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
            .copy_from_slice(&repair.old_declared.to_le_bytes());
        assert!(
            declared_length_repair_read_window_ends_after_creature_appearance_update_pair(
                &exact_payload,
                &repair
            ),
            "stale-declared appearance/update split should be eligible only when the direct-name bit and following U/5 bits prove the inherited cursor"
        );

        let mut missing_position_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        missing_position_bits.push(false);
        missing_position_bits
            .extend_from_slice(&update_bits[LEGACY_UPDATE_POSITION_FRAGMENT_BITS..]);
        let mut missing_payload =
            live_object_payload_with_fragment_bits(&live, missing_position_bits);
        let missing_repair = creature_appearance_update_pair_repair(&live, missing_payload.len());
        missing_payload[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
            .copy_from_slice(&missing_repair.old_declared.to_le_bytes());
        assert!(
            !declared_length_repair_read_window_ends_after_creature_appearance_update_pair(
                &missing_payload,
                &missing_repair
            ),
            "byte-shaped P/5 -> U/5 is not enough: the following 0x3967 row owns its two position bits"
        );
    }

    #[test]
    fn declared_length_creature_appearance_update_pair_requires_adjacent_u5_3967() {
        // This stale-declared helper is a transport gate for the adjacent
        // `P/5 -> U/5 0x3967` handoff only. An intervening top-level record may
        // be fragment-neutral, such as Diamond's 32-byte `A/5` creature add, but
        // it still breaks the decompile-owned adjacent pair proof and must fall
        // through to the normal semantic rewrite plus exact validator.
        let object_id = 0x8000_0042;
        let appearance = creature_name_only_direct_appearance_live_bytes(object_id);
        let update = creature_update_3967_action0_scalar_live_bytes(object_id);
        let update_bits = creature_update_3967_action0_scalar_bits();
        let mut live = appearance;
        live.extend_from_slice(&legacy_creature_add_live_bytes(0x8000_0043));
        live.extend_from_slice(&update);

        let mut exact_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        exact_bits.push(false); // direct CExoString creature name selector.
        exact_bits.extend_from_slice(&update_bits);
        let mut payload = live_object_payload_with_fragment_bits(&live, exact_bits);
        let repair = creature_appearance_update_pair_repair(&live, payload.len());
        payload[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
            .copy_from_slice(&repair.old_declared.to_le_bytes());

        assert!(
            !declared_length_repair_read_window_ends_after_creature_appearance_update_pair(
                &payload, &repair
            ),
            "a non-adjacent same-object U/5 must not reuse stale P/5 cursor proof"
        );
    }

    #[test]
    fn declared_length_capacity_counts_full_creature_appearance_bits_before_u5_3967() {
        // Full `P/5` appearances can carry nested visible-equipment item BOOLs.
        // Stale-declared transport capacity must spend those Diamond-owned
        // source bits before proving the following `U/5 0x3967`; otherwise the
        // nested active-property bits can masquerade as the update's missing
        // position residual bits.
        let object_id = 0x8000_0042;
        let appearance = full_creature_appearance_with_direct_name_and_no_name_equipment(object_id);
        let update = creature_update_3967_action0_scalar_live_bytes(object_id);
        let update_bits = creature_update_3967_action0_scalar_bits();
        let mut live = appearance;
        live.extend_from_slice(&update);

        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "full appearance plus following U/5 should be a complete transport read prefix"
        );

        let mut shifted_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        shifted_bits.extend_from_slice(&direct_name_with_no_name_equipment_bits());
        shifted_bits.extend_from_slice(&update_bits[LEGACY_UPDATE_POSITION_FRAGMENT_BITS..]);
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &shifted_bits,
            ),
            "full P/5 source bits must not be skipped to make a shifted 0x3967 cursor pass preflight"
        );

        let mut exact_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        exact_bits.extend_from_slice(&direct_name_with_no_name_equipment_bits());
        exact_bits.extend_from_slice(&update_bits);
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &exact_bits,
            ),
            "the same full P/5 -> U/5 prefix should pass when both records own their exact source bits"
        );
    }

    #[test]
    fn declared_length_capacity_accepts_zero_mask_creature_appearance_without_bool_bits() {
        let live = creature_zero_appearance_live_bytes();
        let empty_fragment_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];

        assert_eq!(
            live.len(),
            LEGACY_CREATURE_APPEARANCE_HEADER_BYTES,
            "Diamond/EE P/5 zero-mask appearance owns only the eight-byte read header"
        );
        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "P/5 zero-mask appearance is a complete live-object read row"
        );
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &empty_fragment_bits,
            ),
            "zero-mask P/5 owns no CNW BOOLs and must not require the ten-byte U-record floor"
        );
    }

    #[test]
    fn declared_length_capacity_accepts_ignored_high_mask_creature_appearance_without_bool_bits() {
        let live = creature_ignored_high_mask_appearance_live_bytes();
        let empty_fragment_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];

        assert_eq!(
            live.len(),
            LEGACY_CREATURE_APPEARANCE_HEADER_BYTES,
            "Diamond/EE P/5 0x8000 owns only the eight-byte read header"
        );
        assert_eq!(
            crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                &live,
                0,
                live.len(),
            ),
            Some(live.len()),
            "0x8000 is an ignored appearance mask bit, not an unsupported payload branch"
        );
        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "P/5 0x8000 is a complete live-object read row"
        );
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &empty_fragment_bits,
            ),
            "ignored-mask P/5 owns no CNW BOOLs and must not require the U-record floor"
        );
    }

    #[test]
    fn declared_length_capacity_counts_name_only_creature_appearance_bits() {
        let live = creature_name_only_empty_appearance_live_bytes();
        let too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        let mut enough_bits = too_few_bits.clone();
        enough_bits.push(false);

        assert_eq!(
            crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                &live,
                0,
                live.len(),
            ),
            Some(live.len()),
            "Diamond/EE P/5 name-only appearance owns the empty direct CExoString read body"
        );
        assert!(
            !short_creature_body_part_delta_read_end_for_transport(&live, 0, live.len()).is_some()
        );
        assert!(
            !short_creature_equipment_delta_read_end_for_transport(&live, 0, live.len()).is_some()
        );
        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "P/5 name-only appearance is a complete live-object read row"
        );
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &too_few_bits,
            ),
            "mask 0x0400 owns the outer name-mode BOOL and cannot pass with only the CNW header"
        );
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &enough_bits,
            ),
            "a direct empty name consumes exactly one P/5 name-mode BOOL after the CNW header"
        );
    }

    #[test]
    fn declared_length_capacity_counts_name_only_creature_appearance_locstring_token_bits() {
        let live = creature_name_only_token_inline_appearance_live_bytes();
        let mut too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        too_few_bits.extend([true, true, false]);
        let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        enough_bits.extend([true, true, false, false]);

        assert_eq!(
            crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                &live,
                0,
                live.len(),
            ),
            Some(live.len()),
            "fragment-shape ambiguous P/5 name-only rows must prefer the branch that lands on the live-object boundary"
        );
        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "transport walk must keep the complete locstring-pair name row together"
        );
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &too_few_bits,
            ),
            "locstring token component owns the language selector before the second component selector"
        );
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &enough_bits,
            ),
            "token first component plus inline second component consumes four P/5 name BOOLs after the CNW header"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_creature_scalar_only_appearance_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&creature_scalar_only_appearance_live_bytes(0x0080));

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "short scalar-only P/5 bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                &payload,
                split,
                payload.len(),
            )
            == Some(payload.len()),
            "the semantic appearance translator owns scalar-only P/5 rows"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_creature_appearance_read_boundary(
                &payload, split
            ),
            "aligned scalar-only P/5 remains a decompile-owned read-buffer row"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned scalar-only P/5 remains a read boundary, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned scalar-only P/5 row"
        );
    }

    #[test]
    fn declared_length_capacity_counts_scalar_only_creature_appearance_without_bool_bits() {
        let mask =
            0x0001 | 0x0002 | 0x0004 | 0x0080 | 0x0800 | 0x1000 | 0x0008 | 0x0010 | 0x0020 | 0x0040;
        let mut live = creature_scalar_only_appearance_live_bytes(mask);
        let appearance_end = live.len();
        live.extend_from_slice(&creature_delete_live_bytes());

        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            appearance_end,
            "transport scan must split after the scalar-only P/5 row before counting following delete bits"
        );

        let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        enough_bits.push(true);
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &enough_bits,
            ),
            "scalar-only P/5 owns no BOOLs; the following D/5 owns exactly one"
        );
        let too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &too_few_bits,
            ),
            "capacity proof must still reserve the delete BOOL after scalar-only P/5"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_creature_named_body_delta_appearance_as_fragment_tail()
    {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&creature_direct_name_body_part_delta_appearance_live_bytes(
            0x0500, 0, b"",
        ));

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "short direct-name P/5 body-delta bytes can masquerade as compact CNW fragment storage"
        );
        assert!(
            crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                &payload,
                split,
                payload.len(),
            )
            == Some(payload.len()),
            "the semantic appearance translator now owns named partial body deltas exactly"
        );
        assert!(
            fragment_tail_starts_with_aligned_short_creature_body_part_delta_read_boundary(
                &payload, split
            ),
            "aligned direct-name P/5 body-delta is still a decompile-owned read-buffer row"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned named P/5 body-delta remains a read boundary, not a CNW tail"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned named P/5 body-delta"
        );
    }

    #[test]
    fn declared_length_capacity_counts_named_partial_creature_appearance_bits() {
        for (label, live, name_bits, byte_only_claims) in [
            (
                "direct empty name",
                creature_direct_name_body_part_delta_appearance_live_bytes(0x0500, 0, b""),
                vec![false],
                true,
            ),
            (
                "locstring token plus inline name",
                creature_locstring_name_body_part_delta_appearance_live_bytes(0x0500, 0),
                vec![true, true, false, false],
                false,
            ),
        ] {
            let mut too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
            too_few_bits.extend(name_bits.iter().copied().take(name_bits.len() - 1));
            let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
            enough_bits.extend(name_bits);
            let byte_only_end =
                crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                    &live,
                    0,
                    live.len(),
                );
            if byte_only_claims {
                assert_eq!(
                    byte_only_end,
                    Some(live.len()),
                    "the semantic appearance translator now owns {label} partial body deltas exactly"
                );
            } else {
                assert!(
                    byte_only_end.is_none(),
                    "locstring partial appearance boundaries still require fragment proof"
                );
            }
            let mut proven_cursor = CNW_FRAGMENT_HEADER_BITS;
            assert!(
                crate::translate::live_object_update::advance_legacy_creature_appearance_fragment_cursor_for_transport(
                    &live,
                    0,
                    live.len(),
                    &enough_bits,
                    &mut proven_cursor,
                ),
                "the proof-backed semantic appearance parser owns {label} partial body deltas exactly"
            );
            assert_eq!(proven_cursor, enough_bits.len());
            assert!(
                live_object_read_prefix_walks_to(&live, 0, live.len()),
                "P/5 {label} partial body-delta is a complete live-object read row"
            );

            assert!(
                !live_object_read_prefix_has_plausible_fragment_capacity(
                    &live,
                    0,
                    live.len(),
                    &too_few_bits,
                ),
                "P/5 {label} body-delta must not pass capacity proof with a missing name branch bit"
            );
            assert!(
                live_object_read_prefix_has_plausible_fragment_capacity(
                    &live,
                    0,
                    live.len(),
                    &enough_bits,
                ),
                "P/5 {label} body-delta consumes exactly its decompiled name BOOLs before the byte-only partial body branch"
            );
        }
    }

    #[test]
    fn declared_length_window_rejects_short_creature_body_delta_appearance_as_fragment_tail() {
        for (label, mask, selector) in [
            ("body selector zero", 0x0100u16, 0u8),
            ("body selector one", 0x0100u16, 1u8),
            ("body selector three", 0x0100u16, 3u8),
            ("appearance type plus body selector zero", 0x0101u16, 0u8),
            ("ignored high bit plus body selector zero", 0x8100u16, 0u8),
            ("0x2000 tail plus body selector zero", 0x2100u16, 0u8),
            ("zero equipment plus body selector zero", 0x0300u16, 0u8),
            (
                "legacy-skipped 0x4000 plus zero equipment and body selector zero",
                0x4300u16,
                0u8,
            ),
        ] {
            let mut live = Vec::new();
            append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

            let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
            let mut payload = vec![
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR,
            ];
            payload.extend_from_slice(&(split as u32).to_le_bytes());
            payload.extend_from_slice(&live);
            payload.extend_from_slice(&creature_body_part_delta_appearance_live_bytes(
                mask, selector,
            ));

            assert!(
                decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
                "short P/5 {label} bytes can masquerade as compact CNW fragment storage"
            );
            assert!(
                crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                    &payload,
                    split,
                    payload.len(),
                )
                == Some(payload.len()),
                "the semantic appearance translator now owns partial body deltas exactly"
            );
            assert!(
                fragment_tail_starts_with_aligned_short_creature_body_part_delta_read_boundary(
                    &payload, split
                ),
                "aligned P/5 {label} is a decompile-owned read-buffer row"
            );

            let repair = LiveObjectDeclaredLengthRepairCandidate {
                old_declared: split as u32,
                new_declared: split as u32,
                old_payload_length: payload.len(),
                read_bytes_length: live.len(),
                fragment_bytes_length: payload.len() - split,
            };
            assert!(
                declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
                "aligned P/5 {label} remains a read boundary, not a CNW tail"
            );
            assert!(
                !declared_length_window_transport_plausible(&payload),
                "a plausible CNW bit shape must not steal an aligned short P/5 body-part delta"
            );
        }
    }

    #[test]
    fn declared_length_window_rejects_short_creature_zero_equipment_delta_appearance_as_fragment_tail()
     {
        for (label, mask) in [
            ("zero-count equipment only", 0x0200u16),
            ("appearance type plus zero-count equipment", 0x0201u16),
            ("ignored high bit plus zero-count equipment", 0x8200u16),
            ("0x2000 tail plus zero-count equipment", 0x2200u16),
            ("legacy-skipped 0x4000 plus zero-count equipment", 0x4200u16),
        ] {
            let mut live = Vec::new();
            append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

            let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
            let mut payload = vec![
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR,
            ];
            payload.extend_from_slice(&(split as u32).to_le_bytes());
            payload.extend_from_slice(&live);
            payload.extend_from_slice(&creature_zero_equipment_delta_appearance_live_bytes(mask));

            assert!(
                decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
                "short P/5 {label} bytes can masquerade as compact CNW fragment storage"
            );
            assert!(
                crate::translate::live_object_update::legacy_creature_appearance_record_end_for_transport(
                    &payload,
                    split,
                    payload.len(),
                )
                == Some(payload.len()),
                "the semantic appearance translator now owns zero-count partial equipment deltas exactly"
            );
            assert!(
                fragment_tail_starts_with_aligned_short_creature_equipment_delta_read_boundary(
                    &payload, split
                ),
                "aligned P/5 {label} is a decompile-owned read-buffer row"
            );

            let repair = LiveObjectDeclaredLengthRepairCandidate {
                old_declared: split as u32,
                new_declared: split as u32,
                old_payload_length: payload.len(),
                read_bytes_length: live.len(),
                fragment_bytes_length: payload.len() - split,
            };
            assert!(
                declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
                "aligned P/5 {label} remains a read boundary, not a CNW tail"
            );
            assert!(
                !declared_length_window_transport_plausible(&payload),
                "a plausible CNW bit shape must not steal an aligned short P/5 equipment delta"
            );
        }
    }

    #[test]
    fn declared_length_capacity_splits_short_partial_creature_appearance_before_delete_bits() {
        for (label, appearance) in [
            (
                "body selector zero",
                creature_body_part_delta_appearance_live_bytes(0x0100, 0),
            ),
            (
                "zero-count equipment",
                creature_zero_equipment_delta_appearance_live_bytes(0x0200),
            ),
        ] {
            let mut live = appearance;
            let appearance_end = live.len();
            live.extend_from_slice(&creature_delete_live_bytes());

            assert_eq!(
                find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
                appearance_end,
                "transport scan must split after the short P/5 {label} row before counting following delete bits"
            );

            let declared = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
            let mut payload = vec![
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR,
            ];
            payload.extend_from_slice(&(declared as u32).to_le_bytes());
            payload.extend_from_slice(&live);
            payload.push(0x60);

            assert!(
                decode_cnw_msb_valid_bits(&payload[declared..]).is_some(),
                "the terminal empty CNW fragment header is syntactically plausible"
            );
            assert!(
                live_object_read_prefix_walks_to(&payload, LEGACY_LIVE_BYTES_OFFSET, declared),
                "the read side is still a complete sequence of live-object rows"
            );
            let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
            enough_bits.push(true);
            assert!(
                live_object_read_prefix_has_plausible_fragment_capacity(
                    &live,
                    0,
                    live.len(),
                    &enough_bits,
                ),
                "D/5 after short P/5 {label} owns exactly one following BOOL"
            );
            let too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
            assert!(
                !live_object_read_prefix_has_plausible_fragment_capacity(
                    &live,
                    0,
                    live.len(),
                    &too_few_bits,
                ),
                "D/5 after short P/5 {label} owns one BOOL; an empty fragment tail must not pass capacity proof"
            );
        }
    }

    #[test]
    fn declared_length_capacity_counts_nonzero_equipment_add_item_bits_before_delete_bits() {
        let mut live = creature_equipment_delta_add_appearance_live_bytes();
        let appearance_end = live.len();
        live.extend_from_slice(&creature_delete_live_bytes());

        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            appearance_end,
            "transport scan must keep the counted equipment A row inside the P/5 appearance row"
        );

        let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        enough_bits.extend_from_slice(&[true, false, true, false]); // equipment item active-property BOOLs.
        enough_bits.push(true); // following D/5 delete BOOL.
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &enough_bits,
            ),
            "P/5 equipment A consumes its item active-property BOOLs before the following D/5 row"
        );

        let mut missing_item_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        missing_item_bits.push(true); // enough only for the delete if the item bits are skipped.
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &missing_item_bits,
            ),
            "capacity proof must not skip the nested equipment item BOOLs"
        );
    }

    #[test]
    fn declared_length_capacity_keeps_full_selector_partial_creature_appearance_together() {
        let mut live = creature_body_part_delta_appearance_live_bytes(0x0100, b'D');
        let appearance_end = live.len();
        live.extend_from_slice(&creature_delete_live_bytes());

        assert_eq!(
            live[LEGACY_CREATURE_APPEARANCE_HEADER_BYTES], b'D',
            "this fixture intentionally starts the body selector with a delete opcode byte"
        );
        assert_eq!(
            live[LEGACY_CREATURE_APPEARANCE_HEADER_BYTES + 1],
            CREATURE_OBJECT_TYPE,
            "the first fixed body byte makes the selector/body pair look like D/5"
        );
        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            appearance_end,
            "transport scan must not split inside the selector >= 0x0A fixed body table"
        );

        let mut enough_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        enough_bits.push(true);
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &enough_bits,
            ),
            "the following D/5 owns one BOOL only after the full selector body row is kept together"
        );
        let too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(
                &live,
                0,
                live.len(),
                &too_few_bits,
            ),
            "the fixed body table bytes must not be reinterpreted as the delete BOOL owner"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_character_sheet_read_record_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&[b'G', b'S']);
        payload.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        payload.extend_from_slice(&0x0000_0020u32.to_le_bytes());
        payload.push(0x7A);

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "short G/S bytes can masquerade as compact CNW fragment storage"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned G/S remains a live GUI read record even when it owns a fragment BOOL"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned short character-sheet record"
        );
    }

    #[test]
    fn declared_length_window_rejects_character_sheet_effect_icon_row_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&[b'G', b'S']);
        payload.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        payload.extend_from_slice(&0x0000_0100u32.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&0x1234u16.to_le_bytes());

        assert_eq!(
            payload.len() - split,
            MIN_AMBIGUOUS_TAIL_READ_BYTES,
            "this G/S row sits exactly on the generic tail scanner floor"
        );
        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "the G/S effect-icon bytes can masquerade as compact CNW fragment storage"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned G/S effect-icon rows remain live GUI read-boundary ambiguity"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned character-sheet effect row"
        );
    }

    #[test]
    fn declared_length_window_rejects_large_character_sheet_combat_row_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&[b'G', b'S']);
        payload.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        payload.extend_from_slice(&0x0000_0040u32.to_le_bytes());

        let first_combat_rows = 40u8;
        payload.extend_from_slice(&[0x11, 0x22, 0x33]);
        payload.extend_from_slice(&[0x40, 0x41, 0x42]);
        payload.push(first_combat_rows);
        for row in 0..first_combat_rows {
            payload.push(row);
        }
        payload.push(0); // second combat-info list count.

        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "large G/S combat bytes can masquerade as compact CNW fragment storage"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned G/S combat rows remain live GUI read-boundary ambiguity even when they need more than 256 placeholder bits"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned large character-sheet combat row"
        );
    }

    #[test]
    fn declared_length_window_rejects_combined_character_sheet_combat_feat_row_as_fragment_tail() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        payload.extend_from_slice(&(split as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&[b'G', b'S']);
        payload.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        payload.extend_from_slice(&0x0000_0260u32.to_le_bytes());
        payload.push(0x7A); // Mask 0x20 read-buffer byte before its BOOL.

        let first_combat_rows = 255u8;
        let second_combat_rows = 255u8;
        payload.extend_from_slice(&[0x11, 0x22, 0x33]);
        payload.extend_from_slice(&[0x40, 0x41, 0x42]);
        payload.push(first_combat_rows);
        for row in 0..first_combat_rows {
            payload.push(row);
        }
        payload.push(second_combat_rows);
        for row in 0..second_combat_rows {
            payload.push(row);
        }

        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&4096u16.to_le_bytes());
        for feat in 0..4096u16 {
            payload.extend_from_slice(&feat.to_le_bytes());
        }

        let minimum_fragment_bits = 1 + 4398 + 4096;
        assert!(
            minimum_fragment_bits > 8192,
            "max combat plus feat rows exceed the previous fixed proofless cap"
        );
        assert!(
            decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
            "large G/S combat+feat bytes can masquerade as compact CNW fragment storage"
        );

        let repair = LiveObjectDeclaredLengthRepairCandidate {
            old_declared: split as u32,
            new_declared: split as u32,
            old_payload_length: payload.len(),
            read_bytes_length: live.len(),
            fragment_bytes_length: payload.len() - split,
        };
        assert!(
            declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
            "aligned G/S rows remain live GUI read-boundary ambiguity when combined branch bits exceed 8192"
        );
        assert!(
            !declared_length_window_transport_plausible(&payload),
            "a plausible CNW bit shape must not steal an aligned combined character-sheet row"
        );
    }

    #[test]
    fn declared_length_read_prefix_walks_through_short_gui_record() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);
        live.extend_from_slice(&[b'G', b'Q', 0x00]);

        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "zero-row GQ is a complete read-buffer GUI row inside the live-object prefix"
        );
        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(
                &live,
                live.len() - 3,
                live.len()
            ),
            live.len(),
            "the transport walker must advance over the whole short GUI row"
        );
    }

    #[test]
    fn declared_length_window_rejects_short_delete_read_record_as_fragment_tail() {
        let delete_cases = [
            ("D/5 creature", CREATURE_OBJECT_TYPE, 1usize),
            ("D/6 item", ITEM_OBJECT_TYPE, 1),
            ("D/9 placeable", PLACEABLE_OBJECT_TYPE, 1),
            ("D/7 trigger", TRIGGER_OBJECT_TYPE, 0),
            ("D/10 door", DOOR_OBJECT_TYPE, 0),
        ];
        for (label, object_type, bit_count) in delete_cases {
            let mut live = Vec::new();
            append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

            let split = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len();
            let mut payload = vec![
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR,
            ];
            payload.extend_from_slice(&(split as u32).to_le_bytes());
            payload.extend_from_slice(&live);
            payload.extend_from_slice(&[b'D', object_type]);
            payload.extend_from_slice(&0x8000_1007u32.to_le_bytes());

            assert!(
                decode_cnw_msb_valid_bits(&payload[split..]).is_some(),
                "{label} read-buffer bytes can masquerade as compact CNW fragment storage"
            );
            assert_eq!(
                legacy_live_delete_fragment_bit_count(&payload, split, payload.len()),
                Some(bit_count),
                "{label} must keep its decompile-owned delete BOOL count"
            );

            let repair = LiveObjectDeclaredLengthRepairCandidate {
                old_declared: split as u32,
                new_declared: split as u32,
                old_payload_length: payload.len(),
                read_bytes_length: live.len(),
                fragment_bytes_length: payload.len() - split,
            };
            assert!(
                declared_length_repair_tail_contains_live_object_read_boundary(&payload, &repair),
                "aligned {label} remains a live-object delete record, not a CNW tail"
            );
            assert!(
                !declared_length_window_transport_plausible(&payload),
                "a plausible CNW bit shape must not steal aligned {label} bytes"
            );
        }
    }

    #[test]
    fn declared_length_read_prefix_capacity_counts_delete_fragment_bits() {
        let delete_cases = [
            ("D/5 creature", CREATURE_OBJECT_TYPE, vec![true]),
            ("D/6 item", ITEM_OBJECT_TYPE, vec![false]),
            ("D/9 placeable", PLACEABLE_OBJECT_TYPE, vec![true]),
            ("D/7 trigger", TRIGGER_OBJECT_TYPE, Vec::new()),
            ("D/10 door", DOOR_OBJECT_TYPE, Vec::new()),
        ];
        for (label, object_type, owned_bits) in delete_cases {
            let mut live = vec![b'D', object_type];
            live.extend_from_slice(&0x8000_1007u32.to_le_bytes());
            live.extend_from_slice(&[b'W', 0x10, 0x20]);

            let mut fragment_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
            fragment_bits.extend_from_slice(&owned_bits);
            assert!(
                live_object_read_prefix_has_plausible_fragment_capacity(
                    &live,
                    0,
                    live.len(),
                    &fragment_bits,
                ),
                "{label} must advance by its exact delete bit count before W"
            );

            if !owned_bits.is_empty() {
                let too_few_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
                assert!(
                    !live_object_read_prefix_has_plausible_fragment_capacity(
                        &live,
                        0,
                        live.len(),
                        &too_few_bits,
                    ),
                    "{label} must not use W or padding as delete fragment-bit storage"
                );
            }
        }
    }

    #[test]
    fn trigger_update_tail_bytes_stay_inside_transport_record() {
        // Diamond/HG trigger update tail proof:
        // `U/7 0xFFFF_FFF3` owns the generic position read fields, two
        // position fragment bits, then a bounded three-byte legacy trigger tail
        // that is removed before EE emission. Those tail bytes can also decode
        // as a compact CNW fragment-storage stream; they are not a stale
        // declared-length split point.
        let live = legacy_trigger_update_with_tail([0xA0, 0x00, 0x00]);
        let tail_start = LEGACY_UPDATE_HEADER_BYTES + LEGACY_UPDATE_POSITION_READ_BYTES;
        assert!(
            decode_cnw_msb_valid_bits(&live[tail_start..]).is_some(),
            "the trigger tail is deliberately shaped like compact fragment storage"
        );

        assert_eq!(
            find_next_legacy_live_object_sub_message_boundary_after(&live, 0, live.len()),
            live.len(),
            "the transport scanner must not split inside the legacy trigger tail"
        );
        assert!(
            !live_object_read_prefix_walks_to(&live, 0, tail_start),
            "a stale declared read window ending before the trigger tail is incomplete"
        );
        assert!(
            live_object_read_prefix_walks_to(&live, 0, live.len()),
            "the complete trigger update row is a valid transport prefix"
        );

        let mut bits = vec![false, false, false, true, false];
        assert!(
            live_object_read_prefix_has_plausible_fragment_capacity(&live, 0, live.len(), &bits),
            "the transport capacity proof must account for the two trigger position bits"
        );
        bits.pop();
        assert!(
            !live_object_read_prefix_has_plausible_fragment_capacity(&live, 0, live.len(), &bits),
            "one trigger position bit is not enough to own the update cursor"
        );

        let declared = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + tail_start;
        let mut stale_payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        stale_payload.extend_from_slice(&(declared as u32).to_le_bytes());
        stale_payload.extend_from_slice(&live);
        assert!(
            !declared_length_window_transport_plausible(&stale_payload),
            "the declared-length classifier must not move trigger tail bytes into the CNW tail"
        );
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    fn rewrite_payload_to_exact_claim_for_test(
        payload: &mut Vec<u8>,
        latest_area_placeables: Option<&crate::translate::area::AreaPlaceableContext>,
    ) -> crate::translate::live_object_update::LiveObjectUpdateClaimSummary {
        assert!(
            crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
                payload,
                latest_area_placeables,
            ),
            "live-object payload should rewrite through the bounded exact adapter"
        );
        crate::translate::live_object_update::claim_payload_if_verified(payload)
            .expect("rewritten live-object payload should exact-claim")
    }

    #[test]
    fn add_map_rewrite_advances_across_legacy_door_update_records() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);
        append_legacy_door_update(&mut live, 0x8000_34D1);
        append_legacy_door_add(&mut live, 0x8000_34D0, 0x0000_03AB);

        let fragment_bits = vec![
            false, false, false, // CNW fragment length header, rewritten by pack.
            false, false, false, false, false, false, // first door add.
            false, true, false, false, false, false, false, // legacy door update.
            false, false, false, false, false, false, // second door add.
        ];
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        let declared = (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len()) as u32;
        payload.extend_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&pack_cnw_msb_valid_bits(fragment_bits));

        let summary = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("door add map rewrite");
        assert_eq!(summary.maps_inserted, 2);
    }

    #[test]
    fn direct_inline_door_add_uses_ee_direct_name_branch() {
        let mut live = Vec::new();
        append_legacy_door_add(&mut live, 0x8000_34D1, 0x0000_000C);

        let fragment_bits = vec![
            false, false, false, // CNW fragment length header, rewritten by pack.
            true, false, false, false, false, false,
            false,
            // Legacy/HG can present the direct CExoString bytes with the
            // outer locstring branch set and the helper's inner bit false.
            // EE can read that alternate helper path, but the decompile-owned
            // canonical shape for direct bytes is outer=false:
            // `sub_140796DD0` -> `ReadCExoString(0x20)`.
        ];
        let mut payload = live_object_payload(live, fragment_bits);

        let summary = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("door add map/name-branch rewrite");
        assert_eq!(summary.maps_inserted, 1);

        let declared = read_u32_le(&payload, HIGH_LEVEL_HEADER_BYTES)
            .expect("rewritten live-object declared length") as usize;
        let fragment_bits = decode_cnw_msb_valid_bits(&payload[declared..])
            .expect("rewritten live-object fragment bits");
        assert_eq!(fragment_bits.len(), CNW_FRAGMENT_HEADER_BITS + 6);
        assert!(
            !fragment_bits[CNW_FRAGMENT_HEADER_BITS],
            "direct CExoString door names must use the EE outer=false branch"
        );

        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("canonical direct-name door add exact claim");
        assert_eq!(claim.add_records, 1);
    }

    #[test]
    fn valid_declared_live_object_burst_is_not_legacy_fragment_prefix_stream() {
        let payload =
            include_bytes!("../../fixtures/live_object/hg_seq29_valid_declared_a07_burst.bin");
        assert_eq!(
            &payload[..HIGH_LEVEL_HEADER_BYTES],
            &[
                HIGH_LEVEL_ENVELOPE,
                GAME_OBJECT_UPDATE_MAJOR,
                LIVE_OBJECT_MINOR
            ]
        );
        let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)
            .expect("live-object fixture declared length") as usize;
        assert!(declared >= HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES);
        assert!(declared <= payload.len());
        assert!(!looks_like_legacy_prefixed_live_object_high_level(payload));
    }

    #[test]
    fn hg_seq29_trigger_door_mixed_burst_rewrites_to_exact_ee_claim() {
        let mut payload =
            include_bytes!("../../fixtures/live_object/hg_seq29_trigger_door_mixed_add_update.bin")
                .to_vec();

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload, None);
        assert!(claim.add_records > 0);
        assert!(claim.update_records > 0);
    }

    #[test]
    fn local_diamond_zero_prefixed_door_burst_stays_unclaimed_after_37_order_audit() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/local_bw167demo_zero_prefixed_door_burst.bin"
        )
        .to_vec();

        let normalize_summary = normalize_prefixed_fragments_payload_if_needed(&mut payload)
            .expect("zero legacy fragment prefix should normalize");
        assert_eq!(normalize_summary.old_wire_declared, 0);
        assert_eq!(normalize_summary.prefixed_fragment_bytes, [0, 0, 0, 0]);

        assert!(
            !crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
                &mut payload,
                None,
            ),
            "zero-prefixed local door burst must reject the shifted 0x37 evidence"
        );
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "failed exact rewrite must leave the normalized door burst quarantined"
        );
    }

    #[test]
    fn hg_seq31_creature_trigger_door_mixed_burst_rewrites_to_exact_ee_claim() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_seq31_creature_trigger_door_mixed_add_update.bin"
        )
        .to_vec();

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload, None);
        assert!(claim.add_records > 0);
        assert!(claim.update_records > 0);
        assert!(claim.creature_update_records > 0);
    }

    #[test]
    fn hg_seq40_creature_otis_mixed_burst_rewrites_to_exact_ee_claim() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_seq40_creature_otis_mixed_add_update.bin"
        )
        .to_vec();

        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        let _ =
            crate::translate::live_object_update::rewrite_add_name_fragment_bits_payload_if_possible(
                &mut payload,
            );
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );

        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("Otis mixed exact claim");
        assert!(claim.add_records > 0);
        assert!(claim.creature_appearance_records > 0);
        assert!(claim.creature_update_records > 0);
    }

    #[test]
    fn hg_seq41_creature_captain_mixed_burst_rewrites_to_exact_ee_claim() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_seq41_creature_captain_mixed_add_update.bin"
        )
        .to_vec();

        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        let _ =
            crate::translate::live_object_update::rewrite_add_name_fragment_bits_payload_if_possible(
                &mut payload,
            );
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );

        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("Captain mixed exact claim");
        assert!(claim.add_records > 0);
        assert!(claim.creature_appearance_records > 0);
        assert!(claim.creature_update_records > 0);
    }

    #[test]
    fn short_door_add_inserts_ee_empty_name_before_tail() {
        let mut live = Vec::new();
        live.push(b'A');
        live.push(DOOR_OBJECT_TYPE);
        live.extend_from_slice(&0x8000_34CDu32.to_le_bytes());
        live.extend_from_slice(&4u32.to_le_bytes());
        live.extend_from_slice(&0x14E5u16.to_le_bytes());

        let fragment_bits = vec![
            false, false, false, // CNW fragment length header, rewritten by pack.
            false, false, false, false, false, false, // door add.
        ];
        let mut payload = live_object_payload(live, fragment_bits);

        let summary = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("short door add rewrite");
        assert_eq!(summary.maps_inserted, 1);
        assert_eq!(summary.bytes_inserted, 12);
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn generic_door_second_dword_is_not_compact_short_name() {
        let mut live = Vec::new();
        live.push(b'A');
        live.push(DOOR_OBJECT_TYPE);
        live.extend_from_slice(&0x8000_34CDu32.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(&0x14E5u32.to_le_bytes());
        live.extend_from_slice(&0u16.to_le_bytes());

        assert!(!legacy_compact_door_add_omits_second_dword_for_ee(
            &live,
            0,
            live.len()
        ));
    }

    #[test]
    fn compact_placeable_add_tail_is_expanded_before_ee_visual_map() {
        let mut live = Vec::new();
        live.push(b'A');
        live.push(PLACEABLE_OBJECT_TYPE);
        live.extend_from_slice(&0x8001_4CFBu32.to_le_bytes());
        let name = b"Class and Equipment Information - Examine Me!";
        live.extend_from_slice(&(name.len() as u32).to_le_bytes());
        live.extend_from_slice(name);
        live.extend_from_slice(&[0x05, 0x91, 0x00, 0x00]);

        let fragment_bits = vec![
            false, false, false, // CNW fragment length header, rewritten by pack.
            false, false, false, false, false, false, false, false, false, false, false,
        ];
        let mut payload = live_object_payload(live, fragment_bits);

        let summary = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("compact placeable add rewrite");
        assert_eq!(summary.maps_inserted, 1);
        assert_eq!(summary.bytes_inserted, 9);
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn compact_placeable_add_with_one_residue_bit_rewrites_to_exact_ee_shape() {
        let mut live = Vec::new();
        live.push(b'A');
        live.push(PLACEABLE_OBJECT_TYPE);
        live.extend_from_slice(&0x8000_00B6u32.to_le_bytes());
        live.extend_from_slice(&[0x18, 0x16, 0x00, 0x00]);
        live.extend_from_slice(&[0x05, 0x11, 0x00, 0x00, 0x00]);

        let fragment_bits = vec![
            false, false, false, // CNW fragment length header, rewritten by pack.
            false, // stale compact add source residue drained before EE guards.
        ];
        let mut payload = live_object_payload(live, fragment_bits);

        let summary = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("one-residue compact placeable add rewrite");
        assert_eq!(summary.maps_inserted, 1);
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn empty_inline_placeable_name_length_is_recovered_before_visual_map() {
        let mut live = Vec::new();
        live.push(b'A');
        live.push(PLACEABLE_OBJECT_TYPE);
        live.extend_from_slice(&0x8000_3566u32.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(b"The Sooty Crow");
        live.extend_from_slice(&[0x05, 0x6D, 0x09, 0x00, 0x00]);

        let fragment_bits = vec![
            false, false, false, // CNW fragment length header, rewritten by pack.
            false, false, false, false, false, false, false, false, false, false,
        ];
        let mut payload = live_object_payload(live, fragment_bits);

        let summary = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("empty inline placeable name rewrite");
        assert_eq!(summary.maps_inserted, 1);
        assert!(summary.bytes_inserted >= EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32);
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn empty_inline_placeable_name_with_embedded_nuls_is_recovered_before_visual_map() {
        let mut live = Vec::new();
        live.push(b'A');
        live.push(PLACEABLE_OBJECT_TYPE);
        live.extend_from_slice(&0x8000_0007u32.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(b"Lute (");
        live.extend_from_slice(&[0, 0, 0, 0]);
        live.extend_from_slice(b"ding)");
        live.extend_from_slice(&[0x05, 0xB9, 0x00, 0x00, 0x00]);

        let fragment_bits = vec![
            false, false, false, // CNW fragment length header, rewritten by pack.
            false, false, false, false,
        ];
        let mut payload = live_object_payload(live, fragment_bits);

        let summary = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("embedded-NUL compact placeable add should rewrite");
        assert_eq!(summary.maps_inserted, 1);
        assert!(summary.bytes_inserted >= EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32);
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn pending_seq31_stream_rewrites_to_exact_live_object_claim() {
        let mut payload =
            include_bytes!("../../fixtures/live_object/pending_live_object_seq31_chunks9.bin")
                .to_vec();

        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        )
        .expect("pending stream update pre-pass rewrite");
        dump_pending_seq31_step("step1-update-prepass", &payload);
        rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("pending stream add-record rewrite");
        dump_pending_seq31_step("step2-add-visual-transform", &payload);
        let update_summary =
            crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
                &mut payload,
            );
        dump_pending_seq31_step("step3-update-finalization", &payload);
        if let Some(update_summary) = update_summary {
            assert!(
                update_summary.update_records_examined > 0
                    && (update_summary.update_records_rewritten > 0
                        || update_summary.bytes_inserted > 0
                        || update_summary.bytes_removed > 0
                        || update_summary.interleaved_fragment_spans_promoted > 0),
                "pending stream update finalization must still make typed progress: {update_summary:?}"
            );
        }
        let _ = crate::translate::live_object_update::rewrite_add_name_fragment_bits_payload_if_possible(
            &mut payload,
        );
        dump_pending_seq31_step("step4-add-name", &payload);
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        dump_pending_seq31_step("step5-add-visual-transform-repeat", &payload);
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        dump_pending_seq31_step("step6-update-repeat", &payload);
        if std::env::var_os("HGBRIDGE_PROXY2_DUMP_PENDING_LIVE").is_some() {
            let _ = std::fs::write("target/pending-seq31-after.bin", &payload);
        }
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("pending stream exact claim");
        assert!(claim.add_records > 0);
        assert!(claim.creature_appearance_records > 0);
    }

    fn dump_pending_seq31_step(name: &str, payload: &[u8]) {
        if std::env::var_os("HGBRIDGE_PROXY2_DUMP_PENDING_LIVE_STEPS").is_none() {
            return;
        }
        let _ = std::fs::create_dir_all("target/pending-seq31-steps");
        let _ = std::fs::write(format!("target/pending-seq31-steps/{name}.bin"), payload);
    }

    #[test]
    fn starcore_current_player_appearance_rewrites_to_exact_live_object_claim() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/starcore_current_player_appearance_unclaimed.bin"
        )
        .to_vec();

        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        let _ = crate::translate::live_object_update::rewrite_add_name_fragment_bits_payload_if_possible(
            &mut payload,
        );

        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("starcore current-player appearance exact claim");
        assert!(claim.add_records > 0);
        assert!(claim.creature_appearance_records > 0);
        assert!(claim.creature_update_records > 0);
    }

    #[test]
    fn docks_placeable_board_burst_repairs_stale_declared_length() {
        let raw =
            include_bytes!("../../fixtures/live_object/docks_placeable_boards_stale_declared.bin");
        let candidates = declared_length_repair_candidates(raw);
        assert!(candidates.iter().any(|candidate| {
            candidate.new_declared == 410 && candidate.fragment_bytes_length == 13
        }));

        let mut payload = raw.to_vec();
        payload[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
            .copy_from_slice(&410u32.to_le_bytes());
        let area_context = crate::translate::area::AreaPlaceableContext {
            area_resref: "zdl_docks".to_string(),
            static_rows: vec![crate::translate::area::AreaPlaceableContextRow {
                object_id: 0x8000_35C8,
                appearance: 0x0E60,
                x: 89.0,
                y: 9.0,
                z: 0.8,
                dir_x: 0.0,
                dir_y: 0.0,
                dir_z: 0.0,
                has_direction: false,
                module_state: None,
            }],
            ..crate::translate::area::AreaPlaceableContext::default()
        };

        let pre_visual_summary = rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            Some(&area_context),
        );
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        if crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some() {
            assert!(
                pre_visual_summary
                    .as_ref()
                    .map(|summary| summary.maps_inserted >= 4)
                    .unwrap_or(false),
                "expected board and Portal placeable maps to be retained and rewritten, got pre={pre_visual_summary:?}"
            );
            return;
        }
        let visual_summary = rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            Some(&area_context),
        );
        let maps_inserted = pre_visual_summary
            .as_ref()
            .map(|summary| summary.maps_inserted)
            .unwrap_or_default()
            + visual_summary
                .as_ref()
                .map(|summary| summary.maps_inserted)
                .unwrap_or_default();
        assert!(
            maps_inserted >= 4,
            "expected board and Portal placeable maps to be retained and rewritten, got pre={pre_visual_summary:?} post={visual_summary:?}"
        );

        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            Some(&area_context),
        );
        let _ = crate::translate::live_object_update::rewrite_update_records_payload_if_possible(
            &mut payload,
        );
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some(),
            "repaired Docks placeable burst must be exact-claimable"
        );
    }

    #[test]
    fn declared_length_repair_keeps_hg_creature_fragment_tail_split_near_front() {
        // The live HG Docks creature update shape has a stale declared value
        // near the physical packet end. The actual EE read window ends at a
        // decompile-owned creature/update cursor, with a large CNW fragment
        // tail after it. Candidate ordering must let that exact split reach
        // the semantic translator quickly instead of starving it behind every
        // false-positive fragment-capacity tail.
        let raw = include_bytes!(
            "../../fixtures/live_object/hg_starc5_docks_seq37_creature_update_slow_20260518.bin"
        );
        let candidates = declared_length_repair_candidates(raw);
        let position = candidates
            .iter()
            .position(|candidate| candidate.new_declared == 256)
            .expect("HG creature update stale-declared split should be proposed");
        assert!(
            position < 80,
            "HG creature update split should be near the front of the strict repair queue, got index {position} out of {} candidates",
            candidates.len()
        );
    }

    fn live_object_payload(live: Vec<u8>, fragment_bits: Vec<bool>) -> Vec<u8> {
        let mut payload = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        let declared = (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live.len()) as u32;
        payload.extend_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&pack_cnw_msb_valid_bits(fragment_bits));
        payload
    }

    fn append_legacy_door_add(live: &mut Vec<u8>, object_id: u32, second_dword: u32) {
        live.push(b'A');
        live.push(DOOR_OBJECT_TYPE);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(&second_dword.to_le_bytes());
        live.extend_from_slice(&4u32.to_le_bytes());
        live.extend_from_slice(b"Door");
        live.extend_from_slice(&0x0016u16.to_le_bytes());
    }

    fn append_legacy_door_update(live: &mut Vec<u8>, object_id: u32) {
        live.push(b'U');
        live.push(DOOR_OBJECT_TYPE);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&0xFFFF_FFF7u32.to_le_bytes());
        live.extend_from_slice(&[
            0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E, 0x00, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x16,
            0x00,
        ]);
        live.extend_from_slice(&4u32.to_le_bytes());
        live.extend_from_slice(b"Door");
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod hg_mixed_door_placeable_translation_tests {
    use super::*;
    use crate::translate::live_object_update as live_update;

    fn rewrite_payload_to_exact_claim_for_test(
        payload: &mut Vec<u8>,
    ) -> live_update::LiveObjectUpdateClaimSummary {
        assert!(
            crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
                payload, None,
            ),
            "mixed add/update stream should rewrite through the bounded exact adapter"
        );
        live_update::claim_payload_if_verified(payload)
            .expect("rewritten mixed add/update stream should exact-claim")
    }

    #[test]
    fn hg_door_mixed_add_update_fixture_rewrites_to_exact_ee_claim() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_door_mixed_add_update_claimed_records.bin"
        )
        .to_vec();
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let _ = live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        assert!(
            live_update::claim_payload_if_verified(&payload).is_some(),
            "rewritten door mixed add/update stream must be exact-claimable"
        );
    }

    #[test]
    fn hg_placeable_mixed_add_update_fixture_rewrites_to_exact_ee_claim() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_placeable_mixed_add_update_claimed_records.bin"
        )
        .to_vec();
        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert!(claim.add_records > 0);
        assert!(claim.update_records > 0);
    }

    #[test]
    fn hg_post_door_placeable_transition_compact_payload_leaves_terminal_name_residual_unclaimed() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_post_door_placeable_transition_compact_after_update.bin"
        )
        .to_vec();

        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let visual_summary =
            rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None).expect(
                "compact transition placeable add should receive an EE visual-transform map",
            );
        assert_eq!(visual_summary.maps_inserted, 1);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let _ = live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        assert!(
            live_update::claim_payload_if_verified(&payload).is_none(),
            "post-door compact placeable add/update payload still carries unowned terminal U/9 inline-name fragment bits"
        );
    }

    #[test]
    fn hg_post_door_door_transition_compact_payload_leaves_terminal_name_residual_unclaimed() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_post_door_door_transition_compact_after_update.bin"
        )
        .to_vec();

        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let visual_summary =
            rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
                .expect("compact transition door add should receive an EE visual-transform map");
        assert_eq!(visual_summary.maps_inserted, 1);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let _ = live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        assert!(
            live_update::claim_payload_if_verified(&payload).is_none(),
            "post-door compact door add/update payload still carries unowned terminal U/10 inline-name fragment bits"
        );
    }

    #[test]
    fn hg_door_transition_ascension_west_mixed_payload_leaves_terminal_tail9_residual_unclaimed() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_door_transition_ascension_west_mixed_liveobject.bin"
        )
        .to_vec();

        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let visual_summary =
            rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None).expect(
                "compact transition placeable add should receive an EE visual-transform map",
            );
        assert_eq!(visual_summary.maps_inserted, 1);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        let _ = live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
        let _ = rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None);
        let _ = live_update::rewrite_update_records_payload_if_possible(&mut payload);
        assert!(
            live_update::claim_payload_if_verified(&payload).is_none(),
            "door-transition Ascension West mixed live-object burst still carries unowned terminal U/9 tail9 fragment bits"
        );
    }

    #[test]
    fn hg_seq38_prefixed_shifted_placeable_stream_stays_quarantined() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_starc5_seq38_placeable_prefixed_unclaimed_20260513.bin"
        )
        .to_vec();

        assert!(
            normalize_prefixed_fragments_payload_if_needed(&mut payload).is_none(),
            "shifted seq38 stream must not be salvaged from false U/A/W byte patterns"
        );
        assert!(live_update::rewrite_update_records_payload_if_possible(&mut payload).is_none());
        assert!(
            rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None).is_none()
        );
        assert!(
            live_update::claim_payload_if_verified(&payload).is_none(),
            "without a decompile-owned first record, seq38 stays quarantined"
        );
    }

    #[test]
    fn hg_seq37_pending_shifted_placeable_chunks_stay_quarantined() {
        for fixture in [
            include_bytes!(
                "../../fixtures/live_object/hg_starc5_seq37_pending_live_object_chunk1_unclaimed_20260513.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../fixtures/live_object/hg_starc5_seq37_pending_live_object_chunk2_unclaimed_20260513.bin"
            )
            .as_slice(),
        ] {
            let mut payload = fixture.to_vec();
            let original = payload.clone();

            assert!(
                normalize_prefixed_fragments_payload_if_needed(&mut payload).is_none(),
                "seq37 chunks already carry an in-range declared length and must not enter prefixed salvage"
            );
            assert!(live_update::rewrite_update_records_payload_if_possible(&mut payload).is_none());
            assert!(
                rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
                    .is_none()
            );
            assert!(
                live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload)
                    .is_none()
            );
            assert_eq!(
                payload, original,
                "shifted seq37 chunks must not be mutated without a typed first record"
            );
            assert!(
                live_update::claim_payload_if_verified(&payload).is_none(),
                "shifted seq37 chunks remain quarantined until a stream owner proves the lead-in"
            );
        }
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod embedded_nul_placeable_name_tests {
    use super::*;
    use crate::translate::live_object_update as live_update;

    #[test]
    fn placeable_add_update_with_embedded_nul_name_rewrites_and_claims() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/hg_placeable_embedded_nul_name_add_update.bin"
        )
        .to_vec();

        rewrite_creature_add_visual_transform_maps_if_possible(&mut payload, None)
            .expect("placeable add visual-transform rewrite");
        let _ = live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
        live_update::rewrite_update_records_payload_if_possible(&mut payload)
            .expect("placeable update rewrite");

        assert!(
            live_update::claim_payload_if_verified(&payload).is_some(),
            "embedded-NUL placeable names are valid CExoString payloads and must remain exact-claimable"
        );
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod local_diamond_live_object_tests {
    use crate::translate::live_object_update as live_update;

    #[test]
    fn local_diamond_bw167demo_initial_live_object_stays_unclaimed_after_37_order_audit() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/local_diamond_bw167demo_initial_live_object_seq12_unclaimed.bin"
        )
        .to_vec();

        assert!(
            !crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
                &mut payload,
                None,
            ),
            "BW167 demo initial live-object stream must reject shifted 0x37 door/placeable rows"
        );

        assert!(
            live_update::claim_payload_if_verified(&payload).is_none(),
            "seq12 initial stream remains active shifted-cursor evidence, not an EE-positive fixture"
        );
    }

    #[test]
    fn local_chapter1e_initial_live_object_rewrites_to_exact_claim() {
        let mut payload = include_bytes!(
            "../../fixtures/live_object/local_chapter1e_seq12_liveobject_20260523.bin"
        )
        .to_vec();

        assert!(
            crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
                &mut payload,
                None,
            ),
            "Chapter1E initial live-object stream must rewrite through the bounded exact adapter"
        );
        let claim = live_update::claim_payload_if_verified(&payload)
            .expect("Chapter1E initial live-object stream must be exact-claimable");
        assert!(claim.add_records > 0);
        assert!(claim.creature_update_records > 0);
    }
}
