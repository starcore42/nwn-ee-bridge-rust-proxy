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
//! - `sub_140973160` takes the 1.69-server-build branch through
//!   `sub_1407C4AB0(..., 0x2001, 0x23)` and reads ten legacy `LerpFloat`
//!   values through `sub_140972C70`.
//! - Door add `sub_140796DD0` reads the live-object id with `sub_1409737C0`,
//!   then one or two DWORDs, then the same visual-transform map before the
//!   door name payload.
//! - Placeable add `sub_1407A7800` reads the live-object id/name/tail fields,
//!   then the same visual-transform map after the legacy appearance tail.
//!
//! The identity-map insertion therefore emits exactly ten 32-bit legacy
//! `LerpFloat` defaults, and only for complete creature add records whose
//! fixed transform prefix ends exactly where EE will begin reading the map, or
//! for verified door/placeable add records at the EE decompile-backed cursor.

use crate::translate::area::AreaPlaceableContext;

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const LIVE_OBJECT_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
const LEGACY_LIVE_BYTES_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const MAX_LEGACY_LIVE_LEADIN_SCAN_BYTES: usize = 2048;
const MIN_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x0000_1000;
const MAX_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x00FF_FFFF;
const CREATURE_OBJECT_TYPE: u8 = 0x05;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
const DOOR_OBJECT_TYPE: u8 = 0x0A;
const CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET: usize = 32;
const EE_LEGACY_VISUAL_TRANSFORM_LERP_FLOAT_COUNT: usize = 10;
const EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES: [u8; 40] = [
    0x00, 0x00, 0x80, 0x3F,
    0x00, 0x00, 0x80, 0x3F,
    0x00, 0x00, 0x80, 0x3F,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x80, 0x3F,
];

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
    pub records_examined: u32,
    pub maps_inserted: u32,
    pub bytes_inserted: u32,
    pub area_placeable_adds_suppressed: u32,
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

pub fn wrap_legacy_live_object_continuation_payload_if_plausible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectContinuationWrapSummary> {
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
    let mut rewritten = Vec::with_capacity(
        HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + source.len(),
    );
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
    if old_wire_declared == 0 {
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
        let salvaged = find_salvageable_legacy_live_object_boundary_after_prefixed_fragments(payload)?;
        if salvaged.0 <= LEGACY_LIVE_BYTES_OFFSET {
            return None;
        }
        live_bytes_offset = salvaged.0;
        first_record_end = salvaged.1;
        salvaged_partial_leadin = true;
    } else {
        first_record_end = find_next_legacy_live_object_sub_message_boundary_after(
            payload,
            live_bytes_offset,
            payload.len(),
        )
        .min(payload.len());
    }

    let live_bytes_length = payload.len() - live_bytes_offset;
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes_length;
    let new_declared = u32::try_from(new_declared_usize).ok()?;

    let mut rewritten = Vec::with_capacity(payload.len() + CNW_LENGTH_BYTES);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&payload[live_bytes_offset..]);
    rewritten.extend_from_slice(&prefixed_fragment_bytes);

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
    let mut fragment_bits = decode_cnw_msb_valid_bits(&fragment_bytes);
    let mut fragment_bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut fragment_bits_reliable = fragment_bits.is_some();
    let mut fragment_bits_changed = false;
    let old_live_bytes_length = live_bytes.len();
    let mut records_examined = 0u32;
    let mut maps_inserted = 0u32;
    let mut bytes_inserted = 0u32;
    let mut area_placeable_adds_suppressed = 0u32;
    let mut offset = 0usize;

    while offset + 10 <= live_bytes.len() {
        if !looks_like_legacy_live_object_sub_message_boundary(&live_bytes, offset) {
            offset += 1;
            continue;
        }

        records_examined = records_examined.saturating_add(1);
        let mut record_end =
            find_next_legacy_live_object_sub_message_boundary_after(&live_bytes, offset, live_bytes.len())
                .min(live_bytes.len());
        if record_end <= offset {
            offset += 1;
            continue;
        }

        if fragment_bits_reliable {
            if let Some(bits) = fragment_bits.as_mut() {
                if let Some(record_rewrite) = rewrite_legacy_door_placeable_add_record_for_ee(
                    &mut live_bytes,
                    &mut record_end,
                    bits,
                    &mut fragment_bit_cursor,
                    offset,
                    area_context,
                ) {
                    maps_inserted = maps_inserted.saturating_add(record_rewrite.maps_inserted);
                    bytes_inserted =
                        bytes_inserted.saturating_add(record_rewrite.bytes_inserted);
                    area_placeable_adds_suppressed = area_placeable_adds_suppressed
                        .saturating_add(record_rewrite.area_placeable_adds_suppressed);
                    fragment_bits_changed |= record_rewrite.fragment_bits_changed;
                    offset = if record_rewrite.record_removed {
                        offset
                    } else {
                        record_end.max(offset + 1)
                    };
                    continue;
                }
            } else {
                fragment_bits_reliable = false;
            }
        }

        let Some(insert_offset) =
            legacy_add_visual_transform_insert_offset(&live_bytes, offset, record_end)
        else {
            fragment_bits_reliable = false;
            offset = record_end.max(offset + 1);
            continue;
        };

        if has_ee_identity_visual_transform_map_at(&live_bytes, insert_offset, record_end) {
            offset = record_end.max(offset + 1);
            continue;
        }

        live_bytes.splice(
            insert_offset..insert_offset,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES,
        );
        maps_inserted = maps_inserted.saturating_add(1);
        bytes_inserted = bytes_inserted
            .saturating_add(EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32);
        offset = record_end + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
    }

    if maps_inserted == 0
        && !fragment_bits_changed
        && live_bytes.len() == old_live_bytes_length
        && area_placeable_adds_suppressed == 0
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
        records_examined,
        maps_inserted,
        bytes_inserted,
        area_placeable_adds_suppressed,
    };
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
        let record_end =
            find_next_legacy_live_object_sub_message_boundary_after(payload, candidate, payload.len())
                .min(payload.len());
        return Some((candidate, record_end));
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
        b'A' if object_type == 0x09 || object_type == 0x0A => {
            record_end - record_offset >= minimum_legacy_live_object_record_length_at(bytes, record_offset)
        }
        b'U' if matches!(object_type, 0x05 | 0x07 | 0x09 | 0x0A) => {
            let Some(raw_mask) = read_u32_le(bytes, record_offset + 6) else {
                return false;
            };
            raw_mask != 0 && raw_mask != u32::MAX
        }
        b'D' if matches!(object_type, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => {
            record_end - record_offset <= 16
        }
        b'I' => true,
        b'G' | b'W' => true,
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

    let start = scan_end.min(offset + minimum_legacy_live_object_record_length_at(bytes, offset));
    let suppress_inline_string_boundaries = bytes.get(offset).copied() != Some(b'I');
    let string_scan_start = (offset + 2).min(scan_end);
    for candidate in start..scan_end.saturating_sub(1) {
        if suppress_inline_string_boundaries
            && candidate_inside_inline_string(bytes, string_scan_start, candidate)
        {
            continue;
        }
        if looks_like_legacy_live_object_sub_message_boundary(bytes, candidate) {
            return candidate;
        }
    }
    scan_end
}

fn minimum_legacy_live_object_record_length_at(bytes: &[u8], offset: usize) -> usize {
    if !looks_like_legacy_live_object_sub_message_boundary(bytes, offset) {
        return 2;
    }

    let opcode = bytes[offset];
    let marker = bytes[offset + 1];
    match (opcode, marker) {
        (b'A', 0x05) => CREATURE_ADD_VISUAL_TRANSFORM_READ_OFFSET,
        (b'A', PLACEABLE_OBJECT_TYPE) => {
            let name_offset = offset + 6;
            if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(5).saturating_sub(offset);
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
        (b'U' | b'P', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 10,
        (b'D', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 6,
        (b'I', _) => 7,
        (b'W', _) if marker <= 0x0F && bytes.get(offset + 2) == Some(&0x0E) => 3,
        _ => 2,
    }
}

fn legacy_live_object_continuation_boundary_offset(bytes: &[u8]) -> Option<usize> {
    let max_scan = bytes.len().saturating_sub(2).min(96);
    (0..=max_scan).find(|&offset| {
        looks_like_legacy_live_object_sub_message_boundary(bytes, offset)
    })
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
                && looks_like_legacy_live_gui_object_id_at(bytes, offset + 7)));
    if gui_inventory_boundary {
        return true;
    }

    opcode == b'W' && bytes.len() - offset >= 3 && marker <= 0x0F && bytes[offset + 2] == 0x0E
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
    if (object_id & 0x8000_0000) == 0 || read_u16_le(bytes, offset + 30).is_none() {
        return false;
    }

    for index in 0..6 {
        let Some(value) = read_f32_le(bytes, offset + 6 + index * 4) else {
            return false;
        };
        if !value.is_finite() || value.abs() > 1_000_000_000.0 {
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
        || !looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        return None;
    }

    match bytes[offset + 1] {
        // EE door/placeable add handlers read CAurObjectVisualTransformData at
        // fixed decompile-backed cursors relative to the legacy add record:
        // door add reads id + one/two DWORDs first, and placeable add reads
        // id/name/type/appearance/static fields first. Only synthesize the
        // identity map when those surrounding fields parse cleanly inside this
        // exact record. Creature add remains disabled until its body/appearance
        // bitfield parser owns the complete cursor; guessing there crashes EE
        // and hides armor/body appearance problems behind a shifted stream.
        CREATURE_OBJECT_TYPE => None,
        DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE => None,
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DoorPlaceableAddRewrite {
    maps_inserted: u32,
    bytes_inserted: u32,
    fragment_bits_changed: bool,
    record_removed: bool,
    area_placeable_adds_suppressed: u32,
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
    area_context: Option<&AreaPlaceableContext>,
) -> Option<DoorPlaceableAddRewrite> {
    if *bit_cursor >= bits.len() {
        return None;
    }
    let first_dword = read_u32_le(bytes, record_offset + 6)?;
    let visual_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };

    // EE's door add reader consumes CAurObjectVisualTransformData immediately
    // before the name branch. Legacy streams start with the name at this
    // cursor. If a previous translation pass has already inserted the identity
    // map, the name has moved after the map; otherwise we insert the map and
    // must advance the semantic name cursor ourselves before repairing the
    // name-mode fragment bits below.
    let already_has_visual_map =
        has_ee_identity_visual_transform_map_at(bytes, visual_offset, *record_end);
    let mut name_offset = if already_has_visual_map {
        visual_offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()
    } else {
        visual_offset
    };
    if visual_offset > *record_end
        || name_offset > *record_end
        || !looks_like_legacy_door_add_name_at(bytes, name_offset, *record_end)
    {
        return None;
    }

    let mut summary = DoorPlaceableAddRewrite::default();
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

    let short_name = inline_cexo_string_end(bytes, name_offset).is_none();
    if short_name {
        write_u32_le(bytes, name_offset, 0)?;
        set_cnw_msb_bit(bits, *bit_cursor, false)?;
        insert_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
        *bit_cursor += 6;
        summary.fragment_bits_changed = true;
    } else if bits.get(*bit_cursor).copied().unwrap_or(false) {
        set_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
        *bit_cursor += 7;
        summary.fragment_bits_changed = true;
    } else {
        let changed = set_cnw_msb_bit(bits, *bit_cursor, false)?;
        *bit_cursor += 6;
        summary.fragment_bits_changed = changed;
    }

    if summary.maps_inserted != 0 || summary.fragment_bits_changed {
        Some(summary)
    } else {
        None
    }
}

fn rewrite_legacy_placeable_add_record_for_ee(
    bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    record_offset: usize,
    area_context: Option<&AreaPlaceableContext>,
) -> Option<DoorPlaceableAddRewrite> {
    if *bit_cursor >= bits.len() {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 2)?;
    let name_offset = record_offset + 6;
    let inline_name_end = inline_cexo_string_end(bytes, name_offset);
    let short_name = inline_name_end.is_none();
    let tail_offset = inline_name_end.unwrap_or(name_offset + 4);
    let visual_offset = legacy_placeable_add_tail_end(bytes, tail_offset, *record_end)?;
    let before_bits = bits.clone();
    let legacy_outer_locstring = before_bits.get(*bit_cursor).copied().unwrap_or(false);
    let legacy_inner_client_tlk = !short_name
        && legacy_outer_locstring
        && before_bits.get(*bit_cursor + 1).copied().unwrap_or(false);
    let direct_inline_name_payload = !short_name;
    let direct_name_mode_repair =
        legacy_outer_locstring && legacy_inner_client_tlk && direct_inline_name_payload;
    let inline_locstring_name =
        !short_name && legacy_outer_locstring && !direct_name_mode_repair;
    let source_name_inner_bits = usize::from(inline_locstring_name);
    let destination_name_inner_bits = usize::from(short_name || inline_locstring_name);
    let required_source_bits = 10 + source_name_inner_bits;
    if before_bits.len().saturating_sub(*bit_cursor) < required_source_bits {
        return None;
    }

    let appearance = read_u16_le(bytes, tail_offset + 1)?;
    tracing::info!(
        record_offset,
        record_end = *record_end,
        bit_cursor = *bit_cursor,
        name_offset,
        tail_offset,
        visual_offset,
        appearance,
        short_name,
        legacy_outer_locstring,
        legacy_inner_client_tlk,
        direct_name_mode_repair,
        inline_locstring_name,
        source_name_inner_bits,
        destination_name_inner_bits,
        source_bits = required_source_bits,
        tail = %format_hex_slice(bytes, tail_offset, (*record_end).saturating_sub(tail_offset).min(16)),
        bits = %format_bit_slice(&before_bits, *bit_cursor, required_source_bits.min(16)),
        "server->client live-object placeable add candidate"
    );

    // EE `sub_1407A7800` resolves the add-record appearance row through
    // `placeables.2da` and then immediately tries to load the resulting model
    // resref. Diamond/HG UserNN placeable rows intentionally have no standalone
    // model at that row; the usable model comes from the area/static placeable
    // context. Until the Rust bridge carries that context across from
    // `Area_ClientArea`, strict translation must not emit these as ordinary EE
    // live adds or EE will try to load USER.mdl and crash during startup.
    if area_context.is_some_and(|context| context.contains_placeable_id(object_id))
        || is_legacy_user_defined_placeable_appearance(appearance)
    {
        let source_span = required_source_bits;
        if bits.len().saturating_sub(*bit_cursor) < source_span {
            return None;
        }
        bits.drain(*bit_cursor..(*bit_cursor + source_span));
        bytes.drain(record_offset..*record_end);
        *record_end = record_offset;
        let mut area_rows = String::new();
        if let Some(context) = area_context {
            for (index, row) in context.rows_with_placeable_id(object_id).enumerate() {
                if index != 0 {
                    area_rows.push(',');
                }
                area_rows.push_str(&format!(
                    "app=0x{:04X}@{:.2},{:.2},{:.2}",
                    row.appearance, row.x, row.y, row.z
                ));
            }
        }
        tracing::info!(
            object_id = format_args!("0x{object_id:08X}"),
            appearance,
            source_bits = source_span,
            area_rows = %area_rows,
            "server->client live-object placeable add suppressed because Area_ClientArea already owns it"
        );
        return Some(DoorPlaceableAddRewrite {
            maps_inserted: 0,
            bytes_inserted: 0,
            fragment_bits_changed: true,
            record_removed: true,
            area_placeable_adds_suppressed: 1,
        });
    }

    let mut summary = DoorPlaceableAddRewrite::default();
    if short_name {
        write_u32_le(bytes, name_offset, 0)?;
        set_cnw_msb_bit(bits, *bit_cursor, true)?;
        insert_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
        summary.fragment_bits_changed = true;
    } else if direct_name_mode_repair {
        summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor, false)?;
    } else if inline_locstring_name {
        summary.fragment_bits_changed |= set_cnw_msb_bit(bits, *bit_cursor + 1, false)?;
    }

    let optional_bit = *bit_cursor + 2 + destination_name_inner_bits;
    insert_cnw_msb_bit(bits, optional_bit, false)?;
    summary.fragment_bits_changed = true;

    let source_shift = source_name_inner_bits;
    let post_name_bit = *bit_cursor + 1 + destination_name_inner_bits;
    if bits.len() <= post_name_bit + 9 {
        return None;
    }
    for (destination_delta, source_relative) in [
        (0usize, 1usize),
        (2, 3),
        (3, 4),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 8),
        (8, 9),
    ] {
        let value = before_bits
            .get(*bit_cursor + source_relative + source_shift)
            .copied()
            .unwrap_or(false);
        summary.fragment_bits_changed |=
            set_cnw_msb_bit(bits, post_name_bit + destination_delta, value)?;
    }
    summary.fragment_bits_changed |= set_cnw_msb_bit(bits, post_name_bit + 9, false)?;

    if !has_ee_identity_visual_transform_map_at(bytes, visual_offset, *record_end) {
        bytes.splice(
            visual_offset..visual_offset,
            EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES,
        );
        *record_end += EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
        summary.maps_inserted = 1;
        summary.bytes_inserted = EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len() as u32;
    }

    *bit_cursor += 11 + destination_name_inner_bits;
    if summary.maps_inserted != 0 || summary.fragment_bits_changed {
        Some(summary)
    } else {
        None
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

fn looks_like_legacy_door_add_name_at(bytes: &[u8], name_offset: usize, record_end: usize) -> bool {
    if name_offset > record_end || record_end > bytes.len() {
        return false;
    }
    if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
        return inline_end <= record_end && record_end - inline_end >= 2;
    }
    let legacy_tail_end = name_offset + 4 + 2;
    legacy_tail_end <= record_end
        && (legacy_tail_end == record_end
            || has_ee_identity_visual_transform_map_at(bytes, legacy_tail_end, record_end))
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

fn try_find_legacy_door_add_visual_transform_insert_offset(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let first_dword = read_u32_le(bytes, record_offset + 6)?;
    let insert_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };
    if insert_offset > record_end {
        return None;
    }

    // EE `sub_140796DD0` calls `sub_140973160` at this cursor, then consumes a
    // fragment BOOL selecting short-locstring vs inline `CExoString` name data.
    // Accept either verified inline string data or the compact legacy short-name
    // shape where the read-buffer side only has a four-byte name token and a
    // two-byte state tail.
    if let Some(inline_end) = inline_cexo_string_end(bytes, insert_offset) {
        if inline_end <= record_end && record_end - inline_end >= 2 {
            return Some(insert_offset);
        }
    }

    let short_tail_end = insert_offset.checked_add(6)?;
    if short_tail_end <= record_end {
        Some(insert_offset)
    } else {
        None
    }
}

fn try_find_legacy_placeable_add_visual_transform_insert_offset(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let name_offset = record_offset + 6;
    let tail_offset = inline_cexo_string_end(bytes, name_offset).unwrap_or(name_offset + 4);
    let legacy_tail_end = legacy_placeable_add_tail_end(bytes, tail_offset, record_end)?;

    // EE `sub_1407A7800` reads name, type byte, appearance/static WORD fields
    // and paired fragment BOOLs before the add-record visual-transform map.
    Some(legacy_tail_end)
}

fn legacy_placeable_add_tail_end(
    bytes: &[u8],
    tail_offset: usize,
    record_end: usize,
) -> Option<usize> {
    let tail_end = tail_offset.checked_add(1 + 2 + 2)?;
    if tail_offset > record_end || tail_end > record_end || tail_end > bytes.len() {
        return None;
    }
    if tail_end == record_end || has_ee_identity_visual_transform_map_at(bytes, tail_end, record_end)
    {
        Some(tail_end)
    } else {
        None
    }
}

fn has_ee_identity_visual_transform_map_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let end = offset + EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len();
    end <= record_end
        && end <= bytes.len()
        && bytes[offset..end] == EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES
}

fn candidate_inside_inline_string(bytes: &[u8], search_start: usize, candidate: usize) -> bool {
    let mut string_offset = search_start;
    while string_offset + 4 <= candidate && string_offset < bytes.len() {
        if let Some(string_end) = inline_cexo_string_end(bytes, string_offset) {
            if string_offset + 4 <= candidate && candidate < string_end {
                return true;
            }
        }
        string_offset += 1;
    }
    false
}

fn inline_cexo_string_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let length = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    const MAX_LIVE_OBJECT_NAME_BYTES: usize = 128;
    if length > MAX_LIVE_OBJECT_NAME_BYTES || bytes.len().saturating_sub(offset + 4) < length {
        return None;
    }

    let text_start = offset + 4;
    let end = text_start + length;
    if bytes[text_start..end]
        .iter()
        .all(|byte| matches!(*byte, 0x20..=0x7E | b'\t'))
    {
        Some(end)
    } else {
        None
    }
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
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }

    // Match the mature C++ proxy's bounded object-id classifier. Accepting any
    // nonzero high byte makes shifted ASCII/name/appearance bytes look like
    // live-object ids, which in turn causes false record boundaries and shifted
    // door/placeable transforms.
    let high_byte = object_id & 0xFF00_0000;
    matches!(
        high_byte,
        0x8000_0000 | 0x8800_0000 | 0xFF00_0000 | 0x0100_0000 | 0x0500_0000
    ) || (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID)
        .contains(&object_id)
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
    bytes.get_mut(offset..offset + 4)?.copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Option<f32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

const _: () = {
    assert!(
        EE_LIVE_VISUAL_TRANSFORM_IDENTITY_MAP_BYTES.len()
            == EE_LEGACY_VISUAL_TRANSFORM_LERP_FLOAT_COUNT * CNW_LENGTH_BYTES
    );
};
