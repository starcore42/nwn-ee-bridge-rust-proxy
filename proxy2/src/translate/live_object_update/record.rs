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
    TRIGGER_OBJECT_TYPE, bits, door, locstring, placeable, read_u32_le, reader, trigger,
    write_u32_le, writer,
};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RecordRewrite {
    pub(super) rewritten: bool,
    pub(super) mask_changed: bool,
    pub(super) bits_changed: bool,
    pub(super) bytes_inserted: u32,
    pub(super) bytes_removed: u32,
    pub(super) bits_inserted: u32,
    pub(super) bits_removed: u32,
}

#[derive(Debug, Clone, Copy)]
enum OrientationFragmentRewrite {
    PreserveExisting,
    InsertScalarBranchBeforeLegacyLowBits,
    InsertScalar(u16),
}

#[derive(Debug, Clone, Copy, Default)]
struct FragmentRewrite {
    bits_changed: bool,
    bits_inserted: u32,
    bits_removed: u32,
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
    let mut tail_ready = false;
    let mut tail_needs_empty_name = false;
    let mut inline_name_drop_begin = None;
    let mut fragment_source_mask = raw_mask;
    let mut orientation_fragment_rewrite = if matches!(
        object_type,
        PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) && (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
    {
        // Diamond generic door/placeable updates (`sub_44F3D0`) read the
        // compact orientation scalar directly. EE's matching reader
        // (`sub_14079C050`) first consumes an orientation-mode BOOL, then reads
        // the scalar path when that branch is false. If the exact EE verifier
        // above did not already accept the record, the legacy scalar low bits
        // need this one neutral branch bit inserted before them.
        OrientationFragmentRewrite::InsertScalarBranchBeforeLegacyLowBits
    } else {
        OrientationFragmentRewrite::PreserveExisting
    };

    if object_type == TRIGGER_OBJECT_TYPE && translated_mask != raw_mask {
        let trigger_update = trigger::parse_legacy_trigger_update_for_ee(
            live_bytes,
            record_offset,
            *record_end,
            bits,
            *bit_cursor,
        )?;
        let removed = (*record_end).saturating_sub(trigger_update.position_read_end);
        live_bytes.drain(trigger_update.position_read_end..*record_end);
        *record_end = trigger_update.position_read_end;
        write_u32_le(
            live_bytes,
            record_offset + 6,
            trigger_update.translated_mask,
        )?;
        *bit_cursor = trigger_update.next_bit_cursor;
        rewrite.mask_changed = true;
        rewrite.bytes_removed = rewrite.bytes_removed.saturating_add(removed as u32);
        rewrite.rewritten = true;
        tracing::info!(
            object_type,
            object_id = format_args!("0x{object_id:08X}"),
            raw_mask = format_args!("0x{:08X}", trigger_update.raw_mask),
            translated_mask = format_args!("0x{:08X}", trigger_update.translated_mask),
            record_offset,
            record_end = *record_end,
            bytes_removed = rewrite.bytes_removed,
            "server->client live-object trigger update translated for EE"
        );
        return Some(rewrite);
    }

    if exact_empty_object_update
        && matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && is_bridge_empty_state_update_mask(raw_mask)
    {
        // See boundary::try_get_ee_door_placeable_update_record_end for the
        // matching stream split. This is a bridge-created intermediate shape:
        // state is the only field that can be represented by an empty read
        // buffer, while position/orientation/name all require read bytes in the
        // EE and Diamond readers. Collapse the mask to the exact state-only
        // update. If the bit cursor is already unreliable, leave fragment bits
        // untouched and let the mandatory final exact claim prove the result.
        translated_mask = LEGACY_UPDATE_STATE_MASK;
        can_translate_read_buffer = true;
        if !*bit_cursor_reliable {
            write_u32_le(live_bytes, record_offset + 6, translated_mask)?;
            rewrite.mask_changed = true;
            rewrite.rewritten = true;
            tracing::info!(
                object_type,
                object_id = format_args!("0x{object_id:08X}"),
                raw_mask = format_args!("0x{raw_mask:08X}"),
                translated_mask = format_args!("0x{translated_mask:08X}"),
                record_offset,
                record_end = *record_end,
                "server->client live-object empty bridge update collapsed to state-only mask"
            );
            return Some(rewrite);
        }
        fragment_source_mask = translated_mask;
    } else if exact_empty_object_update && (raw_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        // EE/Diamond update masks are sparse decompile-owned fields. A genuine
        // empty read-buffer state update can only carry the state bit whose
        // payload lives entirely in CNW fragment BOOLs. Captures like
        // `raw_mask=0xFFFFFFF7` at a ten-byte `U/9` or `U/10` scanner candidate
        // are shifted-stream evidence, not safe state-only updates. This
        // rewrite pass skips them and leaves the final exact live-object claim
        // to prove the repaired stream; do not warn here because legacy HG
        // all-bits masks are intentionally accepted at coarse boundary scan
        // time and normalized only after a bounded record parser owns them.
        if raw_mask != LEGACY_UPDATE_STATE_MASK {
            tracing::trace!(
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
        fragment_source_mask = translated_mask;
    } else if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
    {
        if let Some(inline_name) =
            reader::parse_legacy_inline_named_door_placeable_update_record_for_ee(
                live_bytes,
                record_offset,
                *record_end,
            )
        {
            debug_assert_eq!(inline_name.name_end, *record_end);
            inline_name_drop_begin = Some(inline_name.read_without_name_end);
            can_translate_read_buffer = true;
        } else {
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
                            let orientation_scalar12 =
                                writer::encode_ee_scalar_orientation_from_legacy_facing(
                                    tail.facing,
                                );
                            orientation_fragment_rewrite =
                                OrientationFragmentRewrite::InsertScalar(orientation_scalar12);
                            fragment_source_mask &= !LEGACY_UPDATE_ORIENTATION_MASK;
                        }
                        if (raw_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
                            translated_mask |= LEGACY_UPDATE_SCALE_STATE_MASK;
                        }
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
    let bit_rewrite_candidate = if update_bits_present {
        if !*bit_cursor_reliable {
            *bit_cursor_reliable = false;
            return None;
        }
        let mut rewritten_bits = bits.clone();
        let mut rewritten_bit_cursor = *bit_cursor;
        let Some(bit_rewrite) = rewrite_legacy_live_object_update_bits(
            object_type,
            fragment_source_mask,
            translated_mask,
            orientation_fragment_rewrite,
            &mut rewritten_bits,
            &mut rewritten_bit_cursor,
        ) else {
            *bit_cursor_reliable = false;
            return None;
        };
        Some((rewritten_bits, rewritten_bit_cursor, bit_rewrite))
    } else {
        None
    };

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
            if (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
                && (translated_mask & LEGACY_UPDATE_NAME_MASK) == 0
            {
                // Diamond's bit-13 branch has already been consumed as legacy
                // input. EE generic update readers do not consume that bit, so
                // any remaining legacy inline-name bytes must be removed rather
                // than left for the strict record walker to misidentify as a
                // second live-object submessage.
                let drop_begin =
                    door_placeable_ee_update_name_cursor(record_offset, translated_mask);
                if drop_begin < *record_end {
                    let removed = *record_end - drop_begin;
                    live_bytes.drain(drop_begin..*record_end);
                    *record_end = drop_begin;
                    rewrite.bytes_removed = rewrite.bytes_removed.saturating_add(removed as u32);
                }
            }
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

    if let Some(drop_begin) = inline_name_drop_begin {
        if drop_begin < *record_end {
            // Diamond's bit-13 name branch is an input-only legacy field for
            // generic door/placeable updates. The read-buffer fields before it
            // are already in the EE scalar orientation/scale order confirmed
            // above; remove only the inline name bytes and let the fragment-bit
            // rewrite remove the matching legacy name BOOL.
            let removed = *record_end - drop_begin;
            live_bytes.drain(drop_begin..*record_end);
            *record_end = drop_begin;
            rewrite.bytes_removed = rewrite.bytes_removed.saturating_add(removed as u32);
        }
    }

    if !can_translate_read_buffer && translated_mask != raw_mask {
        return None;
    }

    if let Some((rewritten_bits, rewritten_bit_cursor, bit_rewrite)) = bit_rewrite_candidate {
        *bits = rewritten_bits;
        *bit_cursor = rewritten_bit_cursor;
        rewrite.bits_inserted = rewrite
            .bits_inserted
            .saturating_add(bit_rewrite.bits_inserted);
        rewrite.bits_removed = rewrite
            .bits_removed
            .saturating_add(bit_rewrite.bits_removed);
        rewrite.bits_changed |= bit_rewrite.bits_changed;
    }

    if translated_mask != raw_mask {
        write_u32_le(live_bytes, record_offset + 6, translated_mask)?;
        rewrite.mask_changed = true;
    }

    rewrite.rewritten = rewrite.mask_changed
        || rewrite.bytes_inserted != 0
        || rewrite.bytes_removed != 0
        || rewrite.bits_inserted != 0
        || rewrite.bits_removed != 0
        || rewrite.bits_changed;

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
    if let Some(next_bit_cursor) = trigger::advance_verified_ee_trigger_update_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        *bit_cursor,
    ) {
        *bit_cursor = next_bit_cursor;
        return true;
    }

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

fn is_bridge_empty_state_update_mask(mask: u32) -> bool {
    let ee_supported_all = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK;
    mask == ee_supported_all || mask == (ee_supported_all | LEGACY_UPDATE_NAME_MASK)
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

pub(super) fn door_placeable_legacy_inline_name_cursor(record_start: usize, mask: u32) -> usize {
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

fn door_placeable_ee_update_name_cursor(record_start: usize, mask: u32) -> usize {
    door_placeable_legacy_inline_name_cursor(record_start, mask)
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

fn rewrite_legacy_live_object_update_bits(
    object_type: u8,
    source_mask: u32,
    translated_mask: u32,
    orientation_rewrite: OrientationFragmentRewrite,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
) -> Option<FragmentRewrite> {
    if !matches!(
        object_type,
        TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    ) {
        return Some(FragmentRewrite::default());
    }

    let mut cursor = *bit_cursor;
    let mut rewrite = FragmentRewrite::default();

    let source_has_position = (source_mask & LEGACY_UPDATE_POSITION_MASK) != 0;
    let translated_has_position = (translated_mask & LEGACY_UPDATE_POSITION_MASK) != 0;
    match (source_has_position, translated_has_position) {
        (true, true) => {
            if bits.len().saturating_sub(cursor) < LEGACY_UPDATE_POSITION_FRAGMENT_BITS {
                return None;
            }
            cursor += LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
        }
        (true, false) => {
            bits::erase_msb_bits(bits, cursor, LEGACY_UPDATE_POSITION_FRAGMENT_BITS)?;
            rewrite.bits_removed = rewrite
                .bits_removed
                .saturating_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS as u32);
        }
        (false, true) => return None,
        (false, false) => {}
    }

    let source_has_orientation = (source_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0;
    let translated_has_orientation = (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0;
    if translated_has_orientation {
        match orientation_rewrite {
            OrientationFragmentRewrite::PreserveExisting => {
                if !source_has_orientation
                    || bits.len().saturating_sub(cursor)
                        < EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                {
                    return None;
                }
                // EE `sub_14079C050` reads one orientation-mode BOOL before
                // the compact 12-bit scalar facing value. The byte model we
                // preserve here is the scalar one-byte shape, so the emitted
                // EE branch bit must be `false`. Diamond's writer at
                // `0x4452EF/0x445306` uses that same scalar branch for this
                // shape; if a mixed legacy stream leaves the branch bit dirty,
                // canonicalize the typed scalar model instead of forwarding an
                // invalid vector-branch claim with scalar bytes.
                if bits[cursor] {
                    bits[cursor] = false;
                    rewrite.bits_changed = true;
                }
                cursor += EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS;
            }
            OrientationFragmentRewrite::InsertScalarBranchBeforeLegacyLowBits => {
                if !source_has_orientation
                    || bits.len().saturating_sub(cursor)
                        < EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS.saturating_sub(1)
                {
                    return None;
                }
                bits::insert_msb_bit(bits, cursor, false)?;
                cursor += EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS;
                rewrite.bits_inserted = rewrite.bits_inserted.saturating_add(1);
            }
            OrientationFragmentRewrite::InsertScalar(orientation_scalar12) => {
                let inserted_orientation_bits = [
                    false,
                    ((orientation_scalar12 >> 3) & 1) != 0,
                    ((orientation_scalar12 >> 2) & 1) != 0,
                    ((orientation_scalar12 >> 1) & 1) != 0,
                    (orientation_scalar12 & 1) != 0,
                ];
                bits::insert_msb_bits(bits, cursor, &inserted_orientation_bits)?;
                cursor += inserted_orientation_bits.len();
                rewrite.bits_inserted = rewrite
                    .bits_inserted
                    .saturating_add(inserted_orientation_bits.len() as u32);
            }
        }
    } else if source_has_orientation {
        bits::erase_msb_bits(bits, cursor, EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS)?;
        rewrite.bits_removed = rewrite
            .bits_removed
            .saturating_add(EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS as u32);
    }

    let source_has_state = (source_mask & LEGACY_UPDATE_STATE_MASK) != 0;
    let translated_has_state = (translated_mask & LEGACY_UPDATE_STATE_MASK) != 0;
    match (source_has_state, translated_has_state) {
        (true, true) => {
            if bits.len().saturating_sub(cursor) < LEGACY_UPDATE_STATE_FRAGMENT_BITS {
                return None;
            }
            cursor += LEGACY_UPDATE_STATE_FRAGMENT_BITS;
            if object_type == DOOR_OBJECT_TYPE {
                bits::insert_msb_bit(bits, cursor, false)?;
                cursor += 1;
                rewrite.bits_inserted = rewrite.bits_inserted.saturating_add(1);
            }
        }
        (true, false) => {
            bits::erase_msb_bits(bits, cursor, LEGACY_UPDATE_STATE_FRAGMENT_BITS)?;
            rewrite.bits_removed = rewrite
                .bits_removed
                .saturating_add(LEGACY_UPDATE_STATE_FRAGMENT_BITS as u32);
        }
        (false, true) => return None,
        (false, false) => {}
    }

    let source_has_name = (source_mask & LEGACY_UPDATE_NAME_MASK) != 0;
    let translated_has_name = (translated_mask & LEGACY_UPDATE_NAME_MASK) != 0;
    match (source_has_name, translated_has_name) {
        (true, true) => {
            if bits.len().saturating_sub(cursor) < 1 {
                return None;
            }
            cursor += 1;
        }
        (true, false) => {
            bits::erase_msb_bits(bits, cursor, 1)?;
            rewrite.bits_removed = rewrite.bits_removed.saturating_add(1);
        }
        (false, true) => {
            if object_type == TRIGGER_OBJECT_TYPE {
                return None;
            }
            bits::insert_msb_bit(bits, cursor, false)?;
            cursor += 1;
            rewrite.bits_inserted = rewrite.bits_inserted.saturating_add(1);
        }
        (false, false) => {}
    }

    // Scale/state uses read-buffer bytes only; there are no fragment bits to
    // advance for the `0x4` mask in either Diamond or EE generic updates.
    if (source_mask & LEGACY_UPDATE_SCALE_STATE_MASK) == 0
        && (translated_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0
    {
        return None;
    }

    *bit_cursor = cursor;
    Some(rewrite)
}
