//! Decompile-backed `GameObjUpdate_LiveObject` update-record translation.
//!
//! Keep this module narrow. It owns the legacy 1.69 live-object `U` record
//! shape and emits the EE dialect shape for the same semantic update. The M
//! frame layer remains responsible for sockets, deflate, CRC, and sequence
//! repair; this module only rewrites an already-normalized `P 05 01` payload.
//!
//! Anchors used:
//!
//! - EE `CNWCMessage::HandleServerToPlayerGameObjectUpdate` (`sub_14079BCE0`)
//!   dispatches update submessages and logs `Unknown Update sub-message` when
//!   the read stream is shifted.
//! - EE door/placeable update helpers read compact orientation and extra door
//!   state/name-mode BOOLs that are absent from Diamond/HG's 1.69 stream.
//! - The mature C++ bridge's `RewriteLegacyLiveObjectUpdateRecords` carries
//!   the same masks and cursor discipline; this Rust port keeps the transform
//!   focused here instead of folding it into `m_frame`.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const LIVE_OBJECT_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;

const TRIGGER_OBJECT_TYPE: u8 = 0x07;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
const DOOR_OBJECT_TYPE: u8 = 0x0A;

const LEGACY_UPDATE_HEADER_BYTES: usize = 10;
const LEGACY_UPDATE_POSITION_MASK: u32 = 0x0000_0001;
const LEGACY_UPDATE_ORIENTATION_MASK: u32 = 0x0000_0002;
const LEGACY_UPDATE_SCALE_STATE_MASK: u32 = 0x0000_0004;
const LEGACY_UPDATE_STATE_MASK: u32 = 0x0000_0010;
const LEGACY_UPDATE_NAME_MASK: u32 = 0x0008_0000;
const LEGACY_UPDATE_POSITION_READ_BYTES: usize = 6;
const LEGACY_UPDATE_POSITION_FRAGMENT_BITS: usize = 2;
const EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES: usize = 1;
const EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS: usize = 5;
const EE_UPDATE_SCALE_STATE_READ_BYTES: usize = 6;
const LEGACY_UPDATE_STATE_FRAGMENT_BITS: usize = 5;

const MIN_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x0000_1000;
const MAX_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x00FF_FFFF;
const MAX_LIVE_OBJECT_NAME_BYTES: usize = 128;
const MAX_REASONABLE_LIVE_PAYLOAD_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Default)]
pub struct LiveObjectUpdateRewriteSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub old_live_bytes_length: usize,
    pub new_live_bytes_length: usize,
    pub old_fragment_bytes: usize,
    pub new_fragment_bytes: usize,
    pub records_examined: u32,
    pub update_records_examined: u32,
    pub update_records_rewritten: u32,
    pub masks_translated: u32,
    pub bytes_inserted: u32,
    pub bytes_removed: u32,
    pub bits_inserted: u32,
    pub bits_removed: u32,
    pub world_status_records_normalized: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct RecordRewrite {
    rewritten: bool,
    mask_changed: bool,
    bytes_inserted: u32,
    bytes_removed: u32,
    bits_inserted: u32,
    bits_removed: u32,
}

#[derive(Debug, Clone, Copy)]
struct LegacyNamedUpdateTail {
    facing: u16,
    state_byte: u8,
    scale_raw: u32,
    scale: f32,
    generic_state_word: u16,
}

pub fn rewrite_update_records_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectUpdateRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let declared = usize::try_from(old_declared).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }

    let mut live_bytes = payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared].to_vec();
    let mut fragment_bits = decode_cnw_msb_valid_bits(&payload[declared..])?;
    let old_live_bytes_length = live_bytes.len();
    let old_fragment_bytes = payload.len().saturating_sub(declared);

    let mut summary = LiveObjectUpdateRewriteSummary {
        old_declared,
        old_payload_length: payload.len(),
        old_live_bytes_length,
        old_fragment_bytes,
        ..LiveObjectUpdateRewriteSummary::default()
    };

    let mut changed = false;
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut bit_cursor_reliable = true;
    let mut offset = 0usize;
    while offset + 2 <= live_bytes.len() {
        if !looks_like_legacy_live_object_sub_message_boundary(&live_bytes, offset) {
            offset += 1;
            continue;
        }

        summary.records_examined = summary.records_examined.saturating_add(1);
        let opcode = live_bytes[offset];
        let object_type = live_bytes[offset + 1];
        let mut record_end =
            find_next_legacy_live_object_sub_message_boundary_after(&live_bytes, offset, live_bytes.len())
                .min(live_bytes.len());
        if record_end <= offset {
            offset += 1;
            continue;
        }

        if opcode == b'W'
            && record_end >= offset + 3
            && object_type <= 0x0F
            && live_bytes[offset + 2] == 0x0E
        {
            let legal_end = offset + 3;
            if record_end > legal_end {
                let removed = record_end - legal_end;
                live_bytes.drain(legal_end..record_end);
                record_end = legal_end;
                changed = true;
                summary.world_status_records_normalized =
                    summary.world_status_records_normalized.saturating_add(1);
                summary.bytes_removed = summary.bytes_removed.saturating_add(removed as u32);
            }
            offset = record_end;
            continue;
        }

        if opcode == b'A' {
            if bit_cursor_reliable
                && !advance_live_add_record_bit_cursor(
                    &live_bytes,
                    &fragment_bits,
                    offset,
                    record_end,
                    &mut bit_cursor,
                )
            {
                bit_cursor_reliable = false;
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode != b'U'
            || !matches!(object_type, TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
            || record_end < offset + LEGACY_UPDATE_HEADER_BYTES
        {
            offset = record_end.max(offset + 1);
            continue;
        }

        summary.update_records_examined = summary.update_records_examined.saturating_add(1);
        let Some(record_rewrite) = rewrite_update_record_for_ee(
            &mut live_bytes,
            &mut record_end,
            &mut fragment_bits,
            &mut bit_cursor,
            &mut bit_cursor_reliable,
            offset,
        ) else {
            offset = record_end.max(offset + 1);
            continue;
        };

        if record_rewrite.rewritten {
            changed = true;
            summary.update_records_rewritten =
                summary.update_records_rewritten.saturating_add(1);
            if record_rewrite.mask_changed {
                summary.masks_translated = summary.masks_translated.saturating_add(1);
            }
            summary.bytes_inserted =
                summary.bytes_inserted.saturating_add(record_rewrite.bytes_inserted);
            summary.bytes_removed =
                summary.bytes_removed.saturating_add(record_rewrite.bytes_removed);
            summary.bits_inserted =
                summary.bits_inserted.saturating_add(record_rewrite.bits_inserted);
            summary.bits_removed =
                summary.bits_removed.saturating_add(record_rewrite.bits_removed);
        }
        offset = record_end.max(offset + 1);
    }

    if !changed {
        return None;
    }

    let fragment_bytes = pack_cnw_msb_valid_bits(fragment_bits);
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes.len();
    let new_declared = u32::try_from(new_declared_usize).ok()?;
    let new_payload_length = new_declared_usize.checked_add(fragment_bytes.len())?;
    if new_payload_length > MAX_REASONABLE_LIVE_PAYLOAD_BYTES {
        return None;
    }

    let mut rewritten = Vec::with_capacity(new_payload_length);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&live_bytes);
    rewritten.extend_from_slice(&fragment_bytes);

    summary.new_declared = new_declared;
    summary.new_payload_length = rewritten.len();
    summary.new_live_bytes_length = live_bytes.len();
    summary.new_fragment_bytes = fragment_bytes.len();
    *payload = rewritten;
    Some(summary)
}

fn rewrite_update_record_for_ee(
    live_bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    bit_cursor_reliable: &mut bool,
    record_offset: usize,
) -> Option<RecordRewrite> {
    if record_offset + LEGACY_UPDATE_HEADER_BYTES > *record_end || *record_end > live_bytes.len() {
        return None;
    }

    let object_type = live_bytes[record_offset + 1];
    let object_id = read_u32_le(live_bytes, record_offset + 2)?;
    let raw_mask = read_u32_le(live_bytes, record_offset + 6)?;
    let mut translated_mask = translate_legacy_live_object_update_mask(object_type, raw_mask);
    let exact_empty_object_update = *record_end == record_offset + LEGACY_UPDATE_HEADER_BYTES;
    let mut rewrite = RecordRewrite::default();
    let mut can_translate_read_buffer = translated_mask == raw_mask;
    let mut orientation_scalar12 = 0u16;
    let mut tail_ready = false;
    let mut tail_needs_empty_name = false;

    if exact_empty_object_update && (raw_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        translated_mask = LEGACY_UPDATE_STATE_MASK;
        can_translate_read_buffer = true;
    } else if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
    {
        let legacy_tail_offset = door_placeable_update_name_cursor(record_offset, raw_mask);
        let raw_has_legacy_generic_tail =
            (raw_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK)) != 0;
        if legacy_tail_offset <= *record_end && *record_end - legacy_tail_offset >= 9 {
            if let Some(tail) = read_legacy_named_update_tail9(live_bytes, legacy_tail_offset, false)
            {
                let following_payload_ready =
                    legacy_named_update_tail_following_payload_ready(
                        live_bytes,
                        legacy_tail_offset,
                        *record_end,
                    );
                if following_payload_ready || raw_has_legacy_generic_tail {
                    tail_ready = true;
                    tail_needs_empty_name = !following_payload_ready;
                    if (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
                        translated_mask |= LEGACY_UPDATE_ORIENTATION_MASK;
                        orientation_scalar12 = encode_ee_scalar_orientation_from_legacy_facing(
                            tail.facing,
                        );
                    }
                    if (raw_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
                        translated_mask |= LEGACY_UPDATE_SCALE_STATE_MASK;
                    }
                }
            }
        }
    }

    let name_payload_ready = (translated_mask & LEGACY_UPDATE_NAME_MASK) == 0
        || tail_ready
        || legacy_live_update_name_payload_ready(live_bytes, record_offset, *record_end);
    if (translated_mask & LEGACY_UPDATE_NAME_MASK) != 0 && !name_payload_ready {
        if (translated_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
            translated_mask = LEGACY_UPDATE_STATE_MASK;
            let erase_begin = record_offset + LEGACY_UPDATE_HEADER_BYTES;
            if *record_end > erase_begin {
                let removed = *record_end - erase_begin;
                live_bytes.drain(erase_begin..*record_end);
                *record_end = erase_begin;
                rewrite.bytes_removed = rewrite.bytes_removed.saturating_add(removed as u32);
            }
            can_translate_read_buffer = true;
        } else {
            can_translate_read_buffer = false;
        }
    }

    if !can_translate_read_buffer && translated_mask != raw_mask {
        return None;
    }

    let update_bits_present =
        (object_type == TRIGGER_OBJECT_TYPE
            && (translated_mask & LEGACY_UPDATE_POSITION_MASK) != 0)
            || (matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
                && (translated_mask
                    & (LEGACY_UPDATE_POSITION_MASK
                        | LEGACY_UPDATE_ORIENTATION_MASK
                        | LEGACY_UPDATE_SCALE_STATE_MASK
                        | LEGACY_UPDATE_STATE_MASK
                        | LEGACY_UPDATE_NAME_MASK))
                    != 0);
    if update_bits_present
        && (!*bit_cursor_reliable
            || !can_rewrite_legacy_live_object_update_bits(
                object_type,
                translated_mask,
                bits,
                *bit_cursor,
                name_payload_ready || (translated_mask & LEGACY_UPDATE_NAME_MASK) == 0,
            ))
    {
        *bit_cursor_reliable = false;
        return None;
    }

    if tail_ready && (translated_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK)) != 0 {
        let tail_offset = door_placeable_update_name_cursor(record_offset, raw_mask);
        if let Some(tail) = read_legacy_named_update_tail9(live_bytes, tail_offset, false) {
            let ee_tail = build_ee_door_placeable_generic_update_bytes(tail, translated_mask);
            live_bytes.splice(tail_offset..tail_offset + 9, ee_tail.iter().copied());
            if ee_tail.len() >= 9 {
                rewrite.bytes_inserted =
                    rewrite.bytes_inserted.saturating_add((ee_tail.len() - 9) as u32);
            } else {
                rewrite.bytes_removed =
                    rewrite.bytes_removed.saturating_add((9 - ee_tail.len()) as u32);
            }
            *record_end = *record_end - 9 + ee_tail.len();
            can_translate_read_buffer = true;
        }
    }

    if tail_needs_empty_name && (translated_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        let empty_name_offset = door_placeable_ee_update_name_cursor(record_offset, translated_mask);
        if empty_name_offset <= *record_end {
            let removed = (*record_end).saturating_sub(empty_name_offset);
            live_bytes.drain(empty_name_offset..*record_end);
            live_bytes.splice(empty_name_offset..empty_name_offset, [0u8, 0, 0, 0]);
            *record_end = empty_name_offset + 4;
            if removed > 4 {
                rewrite.bytes_removed = rewrite.bytes_removed.saturating_add((removed - 4) as u32);
            } else {
                rewrite.bytes_inserted =
                    rewrite.bytes_inserted.saturating_add((4 - removed) as u32);
            }
            can_translate_read_buffer = true;
        }
    }

    if !can_translate_read_buffer && translated_mask != raw_mask {
        return None;
    }

    if update_bits_present {
        let removed = erase_dropped_legacy_live_object_position_bits(
            object_type,
            raw_mask,
            translated_mask,
            bits,
            *bit_cursor,
        )?;
        rewrite.bits_removed = rewrite.bits_removed.saturating_add(removed);
        let inserted = insert_legacy_live_object_update_bits(
            object_type,
            object_id,
            translated_mask,
            orientation_scalar12,
            bits,
            bit_cursor,
        )?;
        rewrite.bits_inserted = rewrite.bits_inserted.saturating_add(inserted);
    }

    if translated_mask != raw_mask {
        write_u32_le(live_bytes, record_offset + 6, translated_mask)?;
        rewrite.mask_changed = true;
    }

    rewrite.rewritten = rewrite.mask_changed
        || rewrite.bytes_inserted != 0
        || rewrite.bytes_removed != 0
        || rewrite.bits_inserted != 0
        || rewrite.bits_removed != 0;

    tracing::info!(
        object_type,
        object_id = format_args!("0x{object_id:08X}"),
        raw_mask = format_args!("0x{raw_mask:08X}"),
        translated_mask = format_args!("0x{translated_mask:08X}"),
        record_offset,
        record_end = *record_end,
        bits_inserted = rewrite.bits_inserted,
        bits_removed = rewrite.bits_removed,
        bytes_inserted = rewrite.bytes_inserted,
        bytes_removed = rewrite.bytes_removed,
        "server->client live-object update record translated for EE"
    );
    Some(rewrite)
}

fn translate_legacy_live_object_update_mask(object_type: u8, raw_mask: u32) -> u32 {
    match object_type {
        PLACEABLE_OBJECT_TYPE => {
            raw_mask
                & (LEGACY_UPDATE_POSITION_MASK
                    | LEGACY_UPDATE_SCALE_STATE_MASK
                    | LEGACY_UPDATE_STATE_MASK
                    | LEGACY_UPDATE_NAME_MASK)
        }
        DOOR_OBJECT_TYPE => {
            raw_mask
                & (LEGACY_UPDATE_POSITION_MASK
                    | LEGACY_UPDATE_SCALE_STATE_MASK
                    | LEGACY_UPDATE_STATE_MASK
                    | LEGACY_UPDATE_NAME_MASK)
        }
        TRIGGER_OBJECT_TYPE => raw_mask & LEGACY_UPDATE_POSITION_MASK,
        _ => raw_mask,
    }
}

fn door_placeable_update_name_cursor(record_start: usize, mask: u32) -> usize {
    record_start
        + LEGACY_UPDATE_HEADER_BYTES
        + if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
            LEGACY_UPDATE_POSITION_READ_BYTES
        } else {
            0
        }
}

fn door_placeable_ee_update_name_cursor(record_start: usize, mask: u32) -> usize {
    door_placeable_update_name_cursor(record_start, mask)
        + if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
            EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES
        } else {
            0
        }
        + if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
            EE_UPDATE_SCALE_STATE_READ_BYTES
        } else {
            0
        }
}

fn can_rewrite_legacy_live_object_update_bits(
    object_type: u8,
    translated_mask: u32,
    bits: &[bool],
    bit_cursor: usize,
    name_payload_ready: bool,
) -> bool {
    if !matches!(object_type, TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
        return true;
    }

    let mut cursor = bit_cursor;
    let mut available_bits = bits.len();
    if (translated_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        if cursor > available_bits || available_bits - cursor < LEGACY_UPDATE_POSITION_FRAGMENT_BITS
        {
            return false;
        }
        cursor += LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    }

    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
    {
        available_bits += EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS;
        cursor += EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS;
    }

    if (translated_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        if cursor > available_bits || available_bits - cursor < LEGACY_UPDATE_STATE_FRAGMENT_BITS {
            return false;
        }
        cursor += LEGACY_UPDATE_STATE_FRAGMENT_BITS;
        if object_type == DOOR_OBJECT_TYPE {
            available_bits += 1;
            cursor += 1;
        }
    }

    (translated_mask & LEGACY_UPDATE_NAME_MASK) == 0 || (name_payload_ready && cursor <= available_bits)
}

fn erase_dropped_legacy_live_object_position_bits(
    object_type: u8,
    raw_mask: u32,
    translated_mask: u32,
    bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<u32> {
    if !matches!(object_type, TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
        return Some(0);
    }
    let raw_has_position = (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0;
    let translated_has_position = (translated_mask & LEGACY_UPDATE_POSITION_MASK) != 0;
    if !raw_has_position || translated_has_position {
        return Some(0);
    }
    erase_cnw_msb_bits(bits, bit_cursor, LEGACY_UPDATE_POSITION_FRAGMENT_BITS)?;
    Some(LEGACY_UPDATE_POSITION_FRAGMENT_BITS as u32)
}

fn insert_legacy_live_object_update_bits(
    object_type: u8,
    _object_id: u32,
    translated_mask: u32,
    orientation_scalar12: u16,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
) -> Option<u32> {
    let mut cursor = *bit_cursor;
    let mut inserted = 0u32;
    if (translated_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        if bits.len().saturating_sub(cursor) < LEGACY_UPDATE_POSITION_FRAGMENT_BITS {
            return None;
        }
        cursor += LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    }

    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
    {
        let inserted_orientation_bits = [
            false,
            ((orientation_scalar12 >> 3) & 1) != 0,
            ((orientation_scalar12 >> 2) & 1) != 0,
            ((orientation_scalar12 >> 1) & 1) != 0,
            (orientation_scalar12 & 1) != 0,
        ];
        insert_cnw_msb_bits(bits, cursor, &inserted_orientation_bits)?;
        cursor += inserted_orientation_bits.len();
        inserted = inserted.saturating_add(inserted_orientation_bits.len() as u32);
    }

    if (translated_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        if bits.len().saturating_sub(cursor) < LEGACY_UPDATE_STATE_FRAGMENT_BITS {
            return None;
        }
        cursor += LEGACY_UPDATE_STATE_FRAGMENT_BITS;
        if object_type == DOOR_OBJECT_TYPE {
            insert_cnw_msb_bit(bits, cursor, false)?;
            cursor += 1;
            inserted = inserted.saturating_add(1);
        }
    }

    if (translated_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        if object_type == TRIGGER_OBJECT_TYPE {
            return None;
        }
        insert_cnw_msb_bit(bits, cursor, false)?;
        cursor += 1;
        inserted = inserted.saturating_add(1);
    }

    *bit_cursor = cursor;
    Some(inserted)
}

fn build_ee_door_placeable_generic_update_bytes(
    legacy_tail: LegacyNamedUpdateTail,
    translated_mask: u32,
) -> Vec<u8> {
    let mut rewritten = Vec::with_capacity(
        EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES + EE_UPDATE_SCALE_STATE_READ_BYTES,
    );
    if (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let scalar12 = encode_ee_scalar_orientation_from_legacy_facing(legacy_tail.facing);
        rewritten.push(((scalar12 >> 4) & 0xFF) as u8);
    }
    if (translated_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        rewritten.extend_from_slice(&legacy_tail.scale_raw.to_le_bytes());
        rewritten.extend_from_slice(&legacy_tail.generic_state_word.to_le_bytes());
    }
    rewritten
}

fn encode_ee_scalar_orientation_from_legacy_facing(facing: u16) -> u16 {
    let degrees = f64::from(facing) * 360.0 / 65536.0;
    let raw = (degrees * 10.0 + 0.000001).floor() as u32;
    raw.min(0x0FFF) as u16
}

fn read_legacy_named_update_tail9(
    bytes: &[u8],
    offset: usize,
    require_small_state_byte: bool,
) -> Option<LegacyNamedUpdateTail> {
    let facing = read_u16_le(bytes, offset)?;
    let state_byte = *bytes.get(offset + 2)?;
    if require_small_state_byte && state_byte > 10 {
        return None;
    }
    let scale_raw = read_u32_le(bytes, offset + 3)?;
    let scale = read_f32_le(bytes, offset + 3)?;
    let generic_state_word = read_u16_le(bytes, offset + 7)?;
    if !is_plausible_legacy_object_scale(scale) {
        return None;
    }
    Some(LegacyNamedUpdateTail {
        facing,
        state_byte,
        scale_raw,
        scale,
        generic_state_word,
    })
}

fn is_plausible_legacy_object_scale(scale: f32) -> bool {
    scale.is_finite() && (0.01..=100.0).contains(&scale)
}

fn legacy_named_update_tail_following_payload_ready(
    bytes: &[u8],
    tail_offset: usize,
    record_end: usize,
) -> bool {
    if tail_offset > record_end || record_end > bytes.len() || record_end - tail_offset < 13 {
        return false;
    }

    let name_offset = tail_offset + 9;
    if let Some(inline_end) = inline_cexo_string_end(bytes, name_offset) {
        return inline_end <= record_end && record_end - inline_end <= 4;
    }

    if read_u32_le(bytes, name_offset) == Some(0) && name_offset + 4 < record_end {
        let text_start = name_offset + 4;
        let text_length = record_end - text_start;
        if (1..=MAX_LIVE_OBJECT_NAME_BYTES).contains(&text_length)
            && bytes[text_start..record_end]
                .iter()
                .all(|byte| matches!(*byte, 0x20..=0x7E | b'\t'))
        {
            return true;
        }
    }

    record_end.saturating_sub(name_offset + 4) <= 4
}

fn legacy_live_update_name_payload_ready(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> bool {
    if record_offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return false;
    }
    let Some(raw_mask) = read_u32_le(bytes, record_offset + 6) else {
        return false;
    };
    let legacy_name_offset = door_placeable_update_name_cursor(record_offset, raw_mask);
    inline_cexo_string_end(bytes, legacy_name_offset)
        .map(|end| end <= record_end)
        .unwrap_or(false)
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
        DOOR_OBJECT_TYPE => {
            let Some(first_dword) = read_u32_le(bytes, record_offset + 6) else {
                return false;
            };
            let visual_offset = record_offset + 2 + if first_dword == 0 { 12 } else { 8 };
            let name_offset = if has_ee_identity_visual_transform_map_at(bytes, visual_offset, record_end)
            {
                visual_offset + 40
            } else {
                visual_offset
            };
            if name_offset > record_end || *bit_cursor >= bits.len() {
                return false;
            }
            let inline_name = inline_cexo_string_end(bytes, name_offset).is_some();
            *bit_cursor = bit_cursor.saturating_add(if inline_name && bits[*bit_cursor] {
                7
            } else {
                6
            });
            *bit_cursor <= bits.len()
        }
        PLACEABLE_OBJECT_TYPE => {
            if *bit_cursor >= bits.len() {
                return false;
            }
            let dest_inner_bits = usize::from(bits[*bit_cursor]);
            *bit_cursor = bit_cursor.saturating_add(11 + dest_inner_bits);
            *bit_cursor <= bits.len()
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
    match (bytes[offset], bytes[offset + 1]) {
        (b'A', 0x05) => 32,
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
        (b'W', marker) if marker <= 0x0F && bytes.get(offset + 2) == Some(&0x0E) => 3,
        _ => 2,
    }
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
    opcode == b'W' && bytes.len() - offset >= 3 && marker <= 0x0F && bytes[offset + 2] == 0x0E
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

fn has_ee_identity_visual_transform_map_at(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    const IDENTITY_MAP: [u8; 40] = [
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
    let end = offset + IDENTITY_MAP.len();
    end <= record_end && end <= bytes.len() && bytes[offset..end] == IDENTITY_MAP
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

fn insert_cnw_msb_bit(bits: &mut Vec<bool>, bit_index: usize, value: bool) -> Option<()> {
    if bit_index > bits.len() {
        return None;
    }
    bits.insert(bit_index, value);
    Some(())
}

fn insert_cnw_msb_bits(bits: &mut Vec<bool>, bit_index: usize, values: &[bool]) -> Option<()> {
    if bit_index > bits.len() {
        return None;
    }
    for (index, value) in values.iter().copied().enumerate() {
        bits.insert(bit_index + index, value);
    }
    Some(())
}

fn erase_cnw_msb_bits(bits: &mut Vec<bool>, bit_index: usize, count: usize) -> Option<()> {
    if bit_index > bits.len() || bits.len().saturating_sub(bit_index) < count {
        return None;
    }
    bits.drain(bit_index..bit_index + count);
    Some(())
}

fn looks_like_legacy_live_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(looks_like_legacy_live_object_id_value)
        .unwrap_or(false)
}

fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }
    let high_byte = object_id & 0xFF00_0000;
    matches!(
        high_byte,
        0x8000_0000 | 0x8800_0000 | 0xFF00_0000 | 0x0100_0000 | 0x0500_0000
    ) || (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID)
        .contains(&object_id)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Option<f32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    bytes.get_mut(offset..offset + 4)?.copy_from_slice(&value.to_le_bytes());
    Some(())
}
