//! Typed live-object `U` update-record translation.
//!
//! This module owns the exact semantic question for update records:
//! given a bounded legacy `U` record and its fragment cursor, what EE-shaped
//! record and bit stream should be emitted?

use super::{
    DOOR_OBJECT_TYPE, EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS,
    EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES, EE_UPDATE_SCALE_STATE_READ_BYTES,
    LEGACY_UPDATE_HEADER_BYTES, LEGACY_UPDATE_NAME_MASK, LEGACY_UPDATE_ORIENTATION_MASK,
    LEGACY_UPDATE_POSITION_FRAGMENT_BITS, LEGACY_UPDATE_POSITION_MASK,
    LEGACY_UPDATE_POSITION_READ_BYTES, LEGACY_UPDATE_SCALE_STATE_MASK,
    LEGACY_UPDATE_STATE_FRAGMENT_BITS, LEGACY_UPDATE_STATE_MASK, PLACEABLE_OBJECT_TYPE,
    TRIGGER_OBJECT_TYPE, bits, door, locstring, placeable, read_u32_le, reader, write_u32_le,
    writer,
};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RecordRewrite {
    pub(super) rewritten: bool,
    pub(super) mask_changed: bool,
    pub(super) bytes_inserted: u32,
    pub(super) bytes_removed: u32,
    pub(super) bits_inserted: u32,
    pub(super) bits_removed: u32,
}

pub(super) fn rewrite_update_record_for_ee(
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
    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
        if let Some(claim) = reader::parse_verified_ee_door_placeable_update_record(
            live_bytes,
            record_offset,
            *record_end,
            bits,
            *bit_cursor,
        ) {
            *bit_cursor = claim.next_bit_cursor;
            return Some(RecordRewrite::default());
        }
    }

    let mut translated_mask = translate_legacy_live_object_update_mask(object_type, raw_mask);
    let exact_empty_object_update = *record_end == record_offset + LEGACY_UPDATE_HEADER_BYTES;
    let mut rewrite = RecordRewrite::default();
    let mut can_translate_read_buffer = translated_mask == raw_mask;
    let mut orientation_scalar12 = 0u16;
    let mut tail_ready = false;
    let mut tail_needs_empty_name = false;

    if exact_empty_object_update && (raw_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        // EE/Diamond update masks are sparse decompile-owned fields. A genuine
        // empty read-buffer state update can only carry the state bit whose
        // payload lives entirely in CNW fragment BOOLs. Captures like
        // `raw_mask=0xFFFFFFF7` at a ten-byte `U/9` boundary are shifted-stream
        // evidence, not a safe state-only update; quarantining them is better
        // than translating away the proof of misalignment.
        if raw_mask != LEGACY_UPDATE_STATE_MASK {
            tracing::warn!(
                object_type,
                object_id = format_args!("0x{object_id:08X}"),
                raw_mask = format_args!("0x{raw_mask:08X}"),
                record_offset,
                record_end = *record_end,
                "server->client live-object update record rejected: empty record has non-state mask bits"
            );
            return None;
        }
        translated_mask = LEGACY_UPDATE_STATE_MASK;
        can_translate_read_buffer = true;
    } else if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
    {
        let legacy_tail_offset = door_placeable_update_name_cursor(record_offset, raw_mask);
        let raw_has_legacy_generic_tail =
            (raw_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK)) != 0;
        if legacy_tail_offset <= *record_end && *record_end - legacy_tail_offset >= 9 {
            if let Some(tail) =
                reader::read_legacy_named_update_tail9(live_bytes, legacy_tail_offset, false)
            {
                let following_payload_ready =
                    reader::legacy_named_update_tail_following_payload_ready(
                        live_bytes,
                        legacy_tail_offset,
                        *record_end,
                    );
                if following_payload_ready || raw_has_legacy_generic_tail {
                    tail_ready = true;
                    tail_needs_empty_name = !following_payload_ready;
                    if (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
                        translated_mask |= LEGACY_UPDATE_ORIENTATION_MASK;
                        orientation_scalar12 =
                            writer::encode_ee_scalar_orientation_from_legacy_facing(tail.facing);
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
        || locstring::legacy_live_update_name_payload_ready(live_bytes, record_offset, *record_end);
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

    if !can_translate_read_buffer && translated_mask != raw_mask && !tail_ready {
        return None;
    }

    let update_bits_present = update_record_owns_fragment_bits(object_type, translated_mask);
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

    if tail_ready
        && (translated_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK))
            != 0
    {
        let tail_offset = door_placeable_update_name_cursor(record_offset, raw_mask);
        if let Some(tail) = reader::read_legacy_named_update_tail9(live_bytes, tail_offset, false) {
            let ee_tail =
                writer::build_ee_door_placeable_generic_update_bytes(tail, translated_mask);
            live_bytes.splice(tail_offset..tail_offset + 9, ee_tail.iter().copied());
            if ee_tail.len() >= 9 {
                rewrite.bytes_inserted = rewrite
                    .bytes_inserted
                    .saturating_add((ee_tail.len() - 9) as u32);
            } else {
                rewrite.bytes_removed = rewrite
                    .bytes_removed
                    .saturating_add((9 - ee_tail.len()) as u32);
            }
            *record_end = *record_end - 9 + ee_tail.len();
            can_translate_read_buffer = true;
        }
    }

    if tail_needs_empty_name && (translated_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        let empty_name_offset =
            door_placeable_ee_update_name_cursor(record_offset, translated_mask);
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

pub(super) fn advance_verified_update_record_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let Some(claim) = reader::parse_verified_ee_door_placeable_update_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        *bit_cursor,
    ) else {
        if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some()
            && offset + 2 <= live_bytes.len()
            && matches!(live_bytes[offset], b'U')
            && matches!(
                live_bytes[offset + 1],
                PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
            )
        {
            eprintln!(
                "live-object update claim rejected: offset={offset} record_end={record_end} marker=0x{:02X} bit_cursor={} next_bits={:?}",
                live_bytes[offset + 1],
                *bit_cursor,
                fragment_bits
                    .get(*bit_cursor..bit_cursor.saturating_add(20).min(fragment_bits.len()))
                    .unwrap_or(&[])
            );
        }
        return false;
    };
    if claim.read_end != record_end {
        return false;
    }
    *bit_cursor = claim.next_bit_cursor;
    true
}

fn translate_legacy_live_object_update_mask(object_type: u8, raw_mask: u32) -> u32 {
    match object_type {
        PLACEABLE_OBJECT_TYPE => placeable::translate_update_mask(raw_mask),
        DOOR_OBJECT_TYPE => door::translate_update_mask(raw_mask),
        TRIGGER_OBJECT_TYPE => raw_mask & LEGACY_UPDATE_POSITION_MASK,
        _ => raw_mask,
    }
}

pub(super) fn door_placeable_update_name_cursor(record_start: usize, mask: u32) -> usize {
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

fn update_record_owns_fragment_bits(object_type: u8, translated_mask: u32) -> bool {
    (object_type == TRIGGER_OBJECT_TYPE && (translated_mask & LEGACY_UPDATE_POSITION_MASK) != 0)
        || (matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
            && (translated_mask
                & (LEGACY_UPDATE_POSITION_MASK
                    | LEGACY_UPDATE_ORIENTATION_MASK
                    | LEGACY_UPDATE_SCALE_STATE_MASK
                    | LEGACY_UPDATE_STATE_MASK
                    | LEGACY_UPDATE_NAME_MASK))
                != 0)
}

fn can_rewrite_legacy_live_object_update_bits(
    object_type: u8,
    translated_mask: u32,
    bits: &[bool],
    bit_cursor: usize,
    name_payload_ready: bool,
) -> bool {
    if !matches!(
        object_type,
        TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) {
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

    (translated_mask & LEGACY_UPDATE_NAME_MASK) == 0
        || (name_payload_ready && cursor <= available_bits)
}

fn erase_dropped_legacy_live_object_position_bits(
    object_type: u8,
    raw_mask: u32,
    translated_mask: u32,
    bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<u32> {
    if !matches!(
        object_type,
        TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) {
        return Some(0);
    }
    let raw_has_position = (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0;
    let translated_has_position = (translated_mask & LEGACY_UPDATE_POSITION_MASK) != 0;
    if !raw_has_position || translated_has_position {
        return Some(0);
    }
    bits::erase_msb_bits(bits, bit_cursor, LEGACY_UPDATE_POSITION_FRAGMENT_BITS)?;
    Some(LEGACY_UPDATE_POSITION_FRAGMENT_BITS as u32)
}

fn insert_legacy_live_object_update_bits(
    object_type: u8,
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
        bits::insert_msb_bits(bits, cursor, &inserted_orientation_bits)?;
        cursor += inserted_orientation_bits.len();
        inserted = inserted.saturating_add(inserted_orientation_bits.len() as u32);
    }

    if (translated_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        if bits.len().saturating_sub(cursor) < LEGACY_UPDATE_STATE_FRAGMENT_BITS {
            return None;
        }
        cursor += LEGACY_UPDATE_STATE_FRAGMENT_BITS;
        if object_type == DOOR_OBJECT_TYPE {
            bits::insert_msb_bit(bits, cursor, false)?;
            cursor += 1;
            inserted = inserted.saturating_add(1);
        }
    }

    if (translated_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        if object_type == TRIGGER_OBJECT_TYPE {
            return None;
        }
        bits::insert_msb_bit(bits, cursor, false)?;
        cursor += 1;
        inserted = inserted.saturating_add(1);
    }

    *bit_cursor = cursor;
    Some(inserted)
}
