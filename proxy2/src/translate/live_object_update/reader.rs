//! Bounded legacy live-object update readers.
//!
//! The update path deliberately keeps byte parsing separate from mutation:
//! callers first parse a bounded legacy or EE shape into one of these tiny
//! structs, then the writer/validator decides what to emit or claim.

use super::{
    DOOR_OBJECT_TYPE, EE_UPDATE_APPEARANCE_RESREF_READ_BYTES, EE_UPDATE_APPEARANCE_WORD_READ_BYTES,
    EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS, EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES,
    EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS, EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES,
    EE_UPDATE_SCALE_STATE_READ_BYTES, LEGACY_UPDATE_APPEARANCE_MASK, LEGACY_UPDATE_HEADER_BYTES,
    LEGACY_UPDATE_NAME_MASK, LEGACY_UPDATE_ORIENTATION_MASK, LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
    LEGACY_UPDATE_POSITION_MASK, LEGACY_UPDATE_POSITION_READ_BYTES, LEGACY_UPDATE_SCALE_STATE_MASK,
    LEGACY_UPDATE_STATE_FRAGMENT_BITS, LEGACY_UPDATE_STATE_MASK, MAX_LIVE_OBJECT_NAME_BYTES,
    PLACEABLE_OBJECT_TYPE, read_f32_le, read_u16_le, read_u32_le,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyNamedUpdateTail {
    pub(super) facing: u16,
    pub(super) scale_raw: u32,
    pub(super) generic_state_word: u16,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VerifiedEeDoorPlaceableUpdateRecord {
    pub(super) read_end: usize,
    pub(super) next_bit_cursor: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LegacyInlineNamedDoorPlaceableUpdateRecord {
    pub(super) read_without_name_end: usize,
    pub(super) name_end: usize,
}

pub(super) fn read_legacy_named_update_tail9(
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
        scale_raw,
        generic_state_word,
    })
}

pub(super) fn legacy_named_update_tail_following_payload_ready(
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

    read_u32_le(bytes, name_offset) == Some(0) && record_end.saturating_sub(name_offset + 4) <= 4
}

pub(super) fn parse_legacy_inline_named_door_placeable_update_record_for_ee(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<LegacyInlineNamedDoorPlaceableUpdateRecord> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U' {
        return None;
    }
    let object_type = bytes.get(offset + 1).copied()?;
    if !matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
        return None;
    }

    let mask = read_u32_le(bytes, offset + 6)?;
    let debug_live_claim = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();
    if (mask & LEGACY_UPDATE_NAME_MASK) == 0 {
        return None;
    }
    let shared_generic_mask = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_APPEARANCE_MASK
        | LEGACY_UPDATE_NAME_MASK;
    if (mask & !shared_generic_mask) != 0 {
        // High sparse legacy masks often use the older Diamond tail shape that
        // stores facing/scale/state at the post-position cursor. Defer to that
        // focused converter only when its bounded reader proves a plausible
        // tail9 record. Local Diamond captures can also carry sparse all-bits
        // masks with a compact padded inline-name payload instead; rejecting
        // those solely because the mask is sparse would quarantine a valid
        // decompile-owned read shape.
        let legacy_tail_offset = offset
            .checked_add(LEGACY_UPDATE_HEADER_BYTES)?
            .checked_add(if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
                LEGACY_UPDATE_POSITION_READ_BYTES
            } else {
                0
            })?;
        let raw_has_legacy_generic_tail =
            (mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK)) != 0;
        if legacy_tail_offset <= record_end
            && record_end - legacy_tail_offset >= 9
            && read_legacy_named_update_tail9(bytes, legacy_tail_offset, false).is_some()
            && (raw_has_legacy_generic_tail
                || legacy_named_update_tail_following_payload_ready(
                    bytes,
                    legacy_tail_offset,
                    record_end,
                ))
        {
            return None;
        }
    }
    let byte_proven_inline_name_cursor =
        legacy_inline_name_cursor_from_record_end(bytes, offset, record_end, mask);
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let mut compact_legacy_bits = 1usize; // Diamond-only legacy name branch.
        if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
            compact_legacy_bits =
                compact_legacy_bits.saturating_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS);
        }
        if (mask & LEGACY_UPDATE_STATE_MASK) != 0 {
            compact_legacy_bits =
                compact_legacy_bits.saturating_add(LEGACY_UPDATE_STATE_FRAGMENT_BITS);
        }
        if let Some(read_without_name_end) = byte_proven_inline_name_cursor {
            if fragment_bits.len().saturating_sub(bit_cursor) == compact_legacy_bits {
                if debug_live_claim {
                    eprintln!(
                        "legacy inline door/placeable compact scalar name accepted offset={offset} record_end={record_end} read_without_name_end={read_without_name_end} bit_cursor={bit_cursor} mask=0x{mask:08X}"
                    );
                }
                return Some(LegacyInlineNamedDoorPlaceableUpdateRecord {
                    read_without_name_end,
                    name_end: record_end,
                });
            }
        }
    }

    // Diamond client `sub_467AE0` and EE client `sub_14079C050` consume the
    // shared generic door/placeable fields in decompile order:
    //
    // 1. position (`0x1`) as the shared packed XYZ field,
    // 2. orientation (`0x2`) as BOOL scalar/vector branch plus the selected
    //    scalar or three-component vector payload,
    // 3. appearance/resref (`0x20`) as WORD plus optional CResRef,
    // 4. scale/state (`0x4`) as FLOAT scale plus WORD generic state,
    // 5. state (`0x10`) in fragment BOOLs only,
    // 6. legacy name (`0x80000`) as a fragment presence BOOL plus a CExoString
    //    or locstring payload.
    //
    // Diamond ignores many sparse high mask bits in this family, so unknown
    // legacy input bits are not a reason to reject the input parser. They are
    // still dropped from the emitted EE mask and the exact EE validator below
    // rejects them on output.
    let mut read_cursor = offset + LEGACY_UPDATE_HEADER_BYTES;
    let mut fragment_cursor = bit_cursor;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
        )?;
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let vector_branch = fragment_bits.get(fragment_cursor).copied()?;
        if vector_branch {
            read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES)?;
            fragment_cursor = advance_bits(
                fragment_bits,
                fragment_cursor,
                EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS,
            )?;
        } else {
            read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?;
            fragment_cursor = advance_bits(
                fragment_bits,
                fragment_cursor,
                EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS,
            )?;
        }
    }
    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let appearance_word = read_u16_le(bytes, read_cursor)?;
        read_cursor = read_cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
        if appearance_word >= 0xFFFE {
            read_cursor = read_cursor.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
        }
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        read_cursor = read_cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_STATE_FRAGMENT_BITS,
        )?;
    }
    fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
    let _legacy_name_bit_cursor = fragment_cursor;
    if read_cursor > record_end {
        if let Some(byte_proven_cursor) = byte_proven_inline_name_cursor {
            if debug_live_claim {
                eprintln!(
                    "legacy inline door/placeable name cursor recovered after stale branch offset={offset} record_end={record_end} old_read_cursor={read_cursor} new_read_cursor={byte_proven_cursor} bit_cursor={bit_cursor} mask=0x{mask:08X}"
                );
            }
            read_cursor = byte_proven_cursor;
        } else {
            return None;
        }
    }

    if inline_cexo_string_end(bytes, read_cursor) != Some(record_end) {
        if let Some(byte_proven_cursor) = byte_proven_inline_name_cursor {
            if debug_live_claim && byte_proven_cursor != read_cursor {
                eprintln!(
                    "legacy inline door/placeable name cursor repaired offset={offset} record_end={record_end} old_read_cursor={read_cursor} new_read_cursor={byte_proven_cursor} bit_cursor={bit_cursor} mask=0x{mask:08X}"
                );
            }
            read_cursor = byte_proven_cursor;
        }
    }

    let inline_name_end = inline_cexo_string_end(bytes, read_cursor);
    let name_end = if inline_name_end == Some(record_end) {
        record_end
    } else if legacy_packed_name_tail_ready(bytes, read_cursor, record_end) {
        record_end
    } else {
        if debug_live_claim {
            eprintln!(
                "legacy inline door/placeable name reject offset={offset} record_end={record_end} read_cursor={read_cursor} inline_name_end={inline_name_end:?} bit_cursor={bit_cursor} mask=0x{mask:08X} tail={:?}",
                bytes
                    .get(read_cursor..record_end.min(read_cursor.saturating_add(32)))
                    .unwrap_or(&[])
            );
        }
        return None;
    };
    if name_end != record_end {
        return None;
    }

    Some(LegacyInlineNamedDoorPlaceableUpdateRecord {
        read_without_name_end: read_cursor,
        name_end,
    })
}

fn legacy_inline_name_cursor_from_record_end(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    mask: u32,
) -> Option<usize> {
    let mut cursors = vec![offset.checked_add(LEGACY_UPDATE_HEADER_BYTES)?];
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        for cursor in &mut cursors {
            *cursor = cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
        }
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let mut branched = Vec::with_capacity(cursors.len().saturating_mul(2));
        for cursor in cursors {
            branched.push(cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?);
            branched.push(cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES)?);
        }
        cursors = branched;
    }
    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let mut after_appearance = Vec::with_capacity(cursors.len());
        for cursor in cursors {
            let appearance_word = read_u16_le(bytes, cursor)?;
            let mut next = cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
            if appearance_word >= 0xFFFE {
                next = next.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
            }
            after_appearance.push(next);
        }
        cursors = after_appearance;
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        for cursor in &mut cursors {
            *cursor = cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
        }
    }

    let mut proven_cursor = None;
    for cursor in cursors {
        if cursor > record_end {
            continue;
        }
        if inline_cexo_string_end(bytes, cursor) == Some(record_end) {
            proven_cursor = match proven_cursor {
                Some(existing) if existing != cursor => return None,
                Some(existing) => Some(existing),
                None => Some(cursor),
            };
        }
    }
    proven_cursor
}

pub(super) fn parse_verified_ee_door_placeable_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<VerifiedEeDoorPlaceableUpdateRecord> {
    if offset + LEGACY_UPDATE_HEADER_BYTES > record_end || record_end > bytes.len() {
        return None;
    }
    if bytes.get(offset).copied()? != b'U' {
        return None;
    }
    let object_type = bytes.get(offset + 1).copied()?;
    if !matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
        return None;
    }

    let mask = read_u32_le(bytes, offset + 6)?;
    // EE generic door/placeable updates are verified against the actual EE
    // reader/writer shape. `sub_14079C050` consumes position, orientation,
    // scale/state, and state here; unlike Diamond's generic update path, it has
    // no bit-13 / 0x0008_0000 name branch. A translated EE packet carrying that
    // legacy bit is therefore invalid even if its byte layout looks plausible.
    let allowed_mask = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_APPEARANCE_MASK;
    if mask == 0 || (mask & !allowed_mask) != 0 {
        return None;
    }

    let mut read_cursor = offset + LEGACY_UPDATE_HEADER_BYTES;
    let mut fragment_cursor = bit_cursor;
    let debug_live_claim = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();

    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
        if read_cursor > record_end {
            return None;
        }
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
        )?;
    }

    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        // EE `sub_14079C050` first reads a BOOL branch for generic
        // orientation. `false` selects the compact scalar branch; `true`
        // selects the three-component vector branch. Diamond's generic reader
        // uses the same branch contract, so exact validation preserves either
        // shape instead of canonicalizing everything to scalar.
        let vector_branch = fragment_bits.get(fragment_cursor).copied()?;
        if vector_branch {
            read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES)?;
            fragment_cursor = advance_bits(
                fragment_bits,
                fragment_cursor,
                EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS,
            )?;
        } else {
            read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES)?;
            fragment_cursor = advance_bits(
                fragment_bits,
                fragment_cursor,
                EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS,
            )?;
        }
        if read_cursor > record_end {
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let appearance_word = read_u16_le(bytes, read_cursor)?;
        read_cursor = read_cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
        if appearance_word >= 0xFFFE {
            read_cursor = read_cursor.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
        }
        if read_cursor > record_end {
            if debug_live_claim {
                eprintln!(
                    "door/placeable update reject appearance cursor offset={offset} record_end={record_end} read_cursor={read_cursor}"
                );
            }
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        // Diamond `sub_467AE0` reads mask 0x20 at loc_467C29 before mask 0x4
        // at loc_467C6B; EE `sub_14079C050` preserves that order at
        // loc_14079C690 before loc_14079CB44. A swapped same-length row can
        // otherwise land byte-exact while decoding an impossible scale here.
        let scale = read_f32_le(bytes, read_cursor)?;
        if !is_plausible_legacy_object_scale(scale) {
            if debug_live_claim {
                eprintln!(
                    "door/placeable update reject scale value offset={offset} record_end={record_end} read_cursor={read_cursor} scale={scale:?} mask=0x{mask:08X}"
                );
            }
            return None;
        }
        read_cursor = read_cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
        if read_cursor > record_end {
            if debug_live_claim {
                eprintln!(
                    "door/placeable update reject scale cursor offset={offset} record_end={record_end} read_cursor={read_cursor}"
                );
            }
            return None;
        }
    }

    if (mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        fragment_cursor = advance_bits(
            fragment_bits,
            fragment_cursor,
            LEGACY_UPDATE_STATE_FRAGMENT_BITS,
        )?;
        if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
            // EE's object-specific placeable/door update readers consume one
            // extra BOOL beyond Diamond's five legacy state bits. The
            // translator inserts `false`, so an exact bridge packet must carry
            // that neutral branch.
            if fragment_bits.get(fragment_cursor).copied()? {
                return None;
            }
            fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
        }
    }

    if read_cursor != record_end {
        if debug_live_claim {
            eprintln!(
                "door/placeable update reject read cursor offset={offset} record_end={record_end} read_cursor={read_cursor} mask=0x{mask:08X}"
            );
        }
        return None;
    }

    Some(VerifiedEeDoorPlaceableUpdateRecord {
        read_end: read_cursor,
        next_bit_cursor: fragment_cursor,
    })
}

fn inline_cexo_string_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let length = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    if length > MAX_LIVE_OBJECT_NAME_BYTES || bytes.len().saturating_sub(offset + 4) < length {
        return None;
    }
    // Diamond and EE direct-name branches both read a bounded CExoString. The
    // decompiled reader consumes raw bytes, so embedded NUL padding is legal
    // and must not force a shifted short-name interpretation.
    Some(offset + 4 + length)
}

pub(super) fn legacy_packed_name_tail_ready(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if offset >= record_end || record_end > bytes.len() {
        return false;
    }

    let tail = &bytes[offset..record_end];
    if tail.len() > MAX_LIVE_OBJECT_NAME_BYTES + 8 {
        return false;
    }

    // Diamond's bit-packed `ReadCExoString(32)` branch may leave the printable
    // name bytes byte-aligned while the length/control bits live in the CNW
    // fragment tail. The translator never emits this Diamond-only name branch
    // to EE; this check only proves the bounded payload we are about to drop
    // does not overlap the next live-object record.
    let text_start = tail
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(tail.len());
    if text_start == tail.len() {
        return true;
    }

    // Some local Diamond records interleave zero control/padding bytes after
    // the first printable byte. The decompile-owned payload is still bounded
    // by the live-object record end, and the translator drops this legacy-only
    // name branch rather than forwarding it to EE, so accept the tail when
    // every non-zero byte is printable name material.
    tail[text_start..]
        .iter()
        .all(|byte| *byte == 0 || matches!(*byte, 0x20..=0x7E | b'\t'))
}

pub(super) fn legacy_name_tail_ready(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    inline_cexo_string_end(bytes, offset) == Some(record_end)
        || legacy_packed_name_tail_ready(bytes, offset, record_end)
}

pub(super) fn legacy_low_bit_control_tail_ready(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if offset > record_end || record_end > bytes.len() {
        return false;
    }

    // Winds of Eremor and XP2 local Diamond captures show door/placeable
    // `U/0x09|0x0A 0xF7` records whose shared generic prefix is exact, followed
    // by a legacy-only low 0x40/0x80 control WORD. Older local captures append a
    // zero WORD; XP2 door/placeable rows carry a second bounded mode WORD
    // (`0x0000` or `0x0001`). EE `sub_14079C050` plus the door/placeable
    // specific readers have no consumers for those low bits. The bridge may
    // drop this suffix only after the caller has proven the typed generic
    // prefix and the next record boundary.
    match record_end - offset {
        2 => read_u16_le(bytes, offset).is_some(),
        4 => {
            read_u16_le(bytes, offset).is_some()
                && read_u16_le(bytes, offset + 2).is_some_and(|mode| mode <= 1)
        }
        6 => {
            // The same local family can carry a leading zero WORD before the
            // control/mode pair when a compact add is followed by a same-object
            // update body with the top-level `U` byte omitted. That leading
            // WORD has no EE/Diamond low-bit consumer either, so it is accepted
            // only as part of this bounded suffix shape.
            read_u16_le(bytes, offset) == Some(0)
                && read_u16_le(bytes, offset + 2).is_some()
                && read_u16_le(bytes, offset + 4).is_some_and(|mode| mode <= 1)
        }
        _ => false,
    }
}

fn advance_bits(bits: &[bool], cursor: usize, count: usize) -> Option<usize> {
    if bits.len().saturating_sub(cursor) < count {
        return None;
    }
    cursor.checked_add(count)
}

pub(super) fn is_plausible_legacy_object_scale(scale: f32) -> bool {
    scale.is_finite() && (0.01..=100.0).contains(&scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_low_bit_control_tail_accepts_bounded_word_suffixes() {
        assert!(legacy_low_bit_control_tail_ready(&[0x18, 0x16], 0, 2));
        assert!(legacy_low_bit_control_tail_ready(
            &[0x18, 0x16, 0x00, 0x00],
            0,
            4
        ));
        assert!(legacy_low_bit_control_tail_ready(
            &[0x00, 0x00, 0x18, 0x16, 0x00, 0x00],
            0,
            6
        ));
        assert!(legacy_low_bit_control_tail_ready(
            &[0x00, 0x00, 0x18, 0x16, 0x01, 0x00],
            0,
            6
        ));
        assert!(!legacy_low_bit_control_tail_ready(
            &[0x00, 0x00, 0x18, 0x16, 0x02, 0x00],
            0,
            6
        ));
        assert!(legacy_low_bit_control_tail_ready(
            &[0x18, 0x16, 0x01, 0x00],
            0,
            4
        ));
        assert!(!legacy_low_bit_control_tail_ready(
            &[0x18, 0x16, 0x02, 0x00],
            0,
            4
        ));
    }
}
