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
    PLACEABLE_OBJECT_TYPE, locstring, read_f32_le, read_u16_le, read_u32_le,
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
    pub(super) position: Option<VerifiedEeDoorPlaceablePosition>,
    pub(super) scalar_orientation: Option<VerifiedEeDoorPlaceableScalarOrientation>,
    pub(super) vector_orientation: Option<VerifiedEeDoorPlaceableVectorOrientation>,
    pub(super) appearance: Option<VerifiedEeDoorPlaceableAppearance>,
    pub(super) appearance_offset: Option<usize>,
    pub(super) scale_state: Option<VerifiedEeDoorPlaceableScaleState>,
    pub(super) state: Option<VerifiedEeDoorPlaceableState>,
    pub(super) state_bit_cursor: Option<usize>,
    pub(super) placeable_name: Option<VerifiedEePlaceableName>,
    pub(super) next_bit_cursor: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VerifiedEePlaceableName {
    pub(super) read_offset: usize,
    pub(super) read_end: usize,
    pub(super) selector_bit_cursor: usize,
    pub(super) locstring_selector_bit_cursor: Option<usize>,
    pub(super) kind: VerifiedEePlaceableNameKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VerifiedEePlaceableNameKind {
    DirectCExoString { byte_length: usize },
    LocStringInlineCExoString { byte_length: usize },
    LocStringTlkRef { client_tlk: u8, strref: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerifiedLegacyTail9PlaceableName {
    pub(super) kind: VerifiedEePlaceableNameKind,
    pub(super) selector_fragment_bits: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VerifiedEeDoorPlaceablePosition {
    pub(super) read_offset: usize,
    pub(super) bit_cursor: usize,
    pub(super) x_raw: u16,
    pub(super) y_raw: u16,
    pub(super) z_raw: u32,
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) z: f32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VerifiedEeDoorPlaceableScalarOrientation {
    pub(super) read_offset: usize,
    pub(super) bit_cursor: usize,
    pub(super) scalar_tenths_degrees: u16,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VerifiedEeDoorPlaceableVectorOrientation {
    pub(super) read_offset: usize,
    pub(super) bit_cursor: usize,
    pub(super) x_raw: u16,
    pub(super) y_raw: u16,
    pub(super) z_raw: u16,
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerifiedEeDoorPlaceableAppearance {
    pub(super) read_offset: usize,
    pub(super) appearance: u16,
    pub(super) resref: Option<[u8; EE_UPDATE_APPEARANCE_RESREF_READ_BYTES]>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VerifiedEeDoorPlaceableScaleState {
    pub(super) read_offset: usize,
    pub(super) scale_raw: u32,
    pub(super) generic_state_word: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerifiedEeDoorPlaceableState {
    pub(super) bit_cursor: usize,
    pub(super) visual_selector: bool,
    pub(super) visual_state_active: bool,
    pub(super) locked: bool,
    pub(super) lockable: bool,
    pub(super) visual_payload: bool,
    pub(super) neutral_ee_state_suffix: bool,
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
    let debug_live_claim = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();
    if bit_cursor > fragment_bits.len() {
        if debug_live_claim {
            eprintln!(
                "door/placeable update reject fragment cursor offset={offset} record_end={record_end} bit_cursor={bit_cursor} fragment_bits={}",
                fragment_bits.len()
            );
        }
        return None;
    }
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
    // EE dispatch `sub_1407B8380` runs the shared `sub_14079C050` reader and
    // then dispatches object type 0x09 to the placeable-specific
    // `sub_140797780`. That reader consumes six state BOOLs and tests mask bit
    // 19 (`0x0008_0000`) before reading a selector BOOL and either a localized
    // string or `ReadCExoString(32)`. Object type 0x0A instead dispatches to
    // `sub_14076FA20`, which has no corresponding name read. Keep the name bit
    // type-specific so a semantically plausible door row cannot shift the
    // shared fragment cursor.
    let mut allowed_mask = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_APPEARANCE_MASK;
    if object_type == PLACEABLE_OBJECT_TYPE {
        allowed_mask |= LEGACY_UPDATE_NAME_MASK;
    }
    if mask == 0 || (mask & !allowed_mask) != 0 {
        return None;
    }

    let mut read_cursor = offset + LEGACY_UPDATE_HEADER_BYTES;
    let mut fragment_cursor = bit_cursor;
    let mut position = None;
    let mut scalar_orientation = None;
    let mut vector_orientation = None;
    let mut appearance = None;
    let mut appearance_offset = None;
    let mut scale_state = None;
    let mut state = None;
    let mut state_bit_cursor = None;
    let mut placeable_name = None;

    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        // Diamond `sub_467AE0` and EE `sub_14079C050` both read the shared
        // position prefix before orientation: X/Y as full read-buffer
        // `ReadFLOAT(100.0, 16)` words, then Z as `ReadFLOAT(-20.0, 320.0,
        // 18)` with its low two bits in the CNW fragment stream.
        let position_read_offset = read_cursor;
        let position_bit_cursor = fragment_cursor;
        let x_raw = read_u16_le(bytes, read_cursor)?;
        let y_raw = read_u16_le(bytes, read_cursor + 2)?;
        let z_high = u32::from(read_u16_le(bytes, read_cursor + 4)?);
        let z_low = ((fragment_bits.get(fragment_cursor).copied()? as u32) << 1)
            | (fragment_bits.get(fragment_cursor + 1).copied()? as u32);
        let z_raw = (z_high << 2) | z_low;
        position = Some(VerifiedEeDoorPlaceablePosition {
            read_offset: position_read_offset,
            bit_cursor: position_bit_cursor,
            x_raw,
            y_raw,
            z_raw,
            x: f32::from(x_raw) / 100.0,
            y: f32::from(y_raw) / 100.0,
            z: decode_ee_position_z(z_raw),
        });
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
        let orientation_read_offset = read_cursor;
        let orientation_bit_cursor = fragment_cursor;
        let vector_branch = fragment_bits.get(fragment_cursor).copied()?;
        if vector_branch {
            let x_raw = read_u16_le(bytes, read_cursor)?;
            let y_raw = read_u16_le(bytes, read_cursor + 2)?;
            let z_raw = read_u16_le(bytes, read_cursor + 4)?;
            vector_orientation = Some(VerifiedEeDoorPlaceableVectorOrientation {
                read_offset: orientation_read_offset,
                bit_cursor: orientation_bit_cursor,
                x_raw,
                y_raw,
                z_raw,
                x: decode_ee_vector_orientation_component(x_raw),
                y: decode_ee_vector_orientation_component(y_raw),
                z: decode_ee_vector_orientation_component(z_raw),
            });
            read_cursor = read_cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES)?;
            fragment_cursor = advance_bits(
                fragment_bits,
                fragment_cursor,
                EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS,
            )?;
        } else {
            let high = u16::from(*bytes.get(read_cursor)?);
            let mut low = 0_u16;
            for bit_index in 0..4 {
                low <<= 1;
                if fragment_bits
                    .get(fragment_cursor + 1 + bit_index)
                    .copied()
                    .unwrap_or(false)
                {
                    low |= 1;
                }
            }
            scalar_orientation = Some(VerifiedEeDoorPlaceableScalarOrientation {
                read_offset: orientation_read_offset,
                bit_cursor: orientation_bit_cursor,
                scalar_tenths_degrees: (high << 4) | low,
            });
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
        let appearance_read_offset = read_cursor;
        appearance_offset = Some(appearance_read_offset);
        let appearance_word = read_u16_le(bytes, read_cursor)?;
        read_cursor = read_cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
        let resref = if appearance_word >= 0xFFFE {
            let resref_start = read_cursor;
            let resref_end = resref_start.checked_add(EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
            if resref_end > record_end {
                return None;
            }
            let mut resref = [0_u8; EE_UPDATE_APPEARANCE_RESREF_READ_BYTES];
            resref.copy_from_slice(bytes.get(resref_start..resref_end)?);
            read_cursor = resref_end;
            Some(resref)
        } else {
            None
        };
        appearance = Some(VerifiedEeDoorPlaceableAppearance {
            read_offset: appearance_read_offset,
            appearance: appearance_word,
            resref,
        });
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
        let scale_state_read_offset = read_cursor;
        let scale_raw = read_u32_le(bytes, read_cursor)?;
        let scale = read_f32_le(bytes, read_cursor)?;
        let generic_state_word = read_u16_le(bytes, read_cursor + 4)?;
        if !is_plausible_legacy_object_scale(scale) {
            if debug_live_claim {
                eprintln!(
                    "door/placeable update reject scale value offset={offset} record_end={record_end} read_cursor={read_cursor} scale={scale:?} mask=0x{mask:08X}"
                );
            }
            return None;
        }
        scale_state = Some(VerifiedEeDoorPlaceableScaleState {
            read_offset: scale_state_read_offset,
            scale_raw,
            generic_state_word,
        });
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
        let state_cursor = fragment_cursor;
        state_bit_cursor = Some(state_cursor);
        let visual_selector = fragment_bits.get(state_cursor).copied()?;
        let visual_state_active = fragment_bits.get(state_cursor + 1).copied()?;
        let locked = fragment_bits.get(state_cursor + 2).copied()?;
        let lockable = fragment_bits.get(state_cursor + 3).copied()?;
        let visual_payload = fragment_bits.get(state_cursor + 4).copied()?;
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
            let neutral_ee_state_suffix = fragment_bits.get(fragment_cursor).copied()?;
            if neutral_ee_state_suffix {
                return None;
            }
            state = Some(VerifiedEeDoorPlaceableState {
                bit_cursor: state_cursor,
                visual_selector,
                visual_state_active,
                locked,
                lockable,
                visual_payload,
                neutral_ee_state_suffix,
            });
            fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
        }
    }

    if (mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        if object_type != PLACEABLE_OBJECT_TYPE {
            return None;
        }
        let selector_bit_cursor = fragment_cursor;
        let localized = fragment_bits.get(fragment_cursor).copied()?;
        fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
        let name_read_offset = read_cursor;
        let (name_read_end, locstring_selector_bit_cursor, kind) = if localized {
            // EE `sub_140797780` calls `sub_1409735F0` after the outer selector.
            // The helper reads exactly one inner BOOL. Its false branch calls
            // `ReadCExoString(32)`; its true branch calls `ReadBYTE(1, 1)` and
            // then `ReadDWORD(32)`. Diamond `sub_44EB40` -> `sub_53E700` has the
            // same outer/inner BOOL order and the same byte-side branches. Keep
            // this cursor explicit: treating the inner selector as a string bit
            // would shift every following live-object record.
            let inner_selector_bit_cursor = fragment_cursor;
            let client_tlk_ref = fragment_bits.get(fragment_cursor).copied()?;
            fragment_cursor = advance_bits(fragment_bits, fragment_cursor, 1)?;
            if client_tlk_ref {
                let name_read_end = locstring::tlk_locstring_ref_end(bytes, name_read_offset)?;
                let client_tlk = *bytes.get(name_read_offset)?;
                let strref = read_u32_le(bytes, name_read_offset + 1)?;
                (
                    name_read_end,
                    Some(inner_selector_bit_cursor),
                    VerifiedEePlaceableNameKind::LocStringTlkRef { client_tlk, strref },
                )
            } else {
                let name_read_end = inline_cexo_string_end(bytes, name_read_offset)?;
                let byte_length = name_read_end.checked_sub(name_read_offset + 4)?;
                (
                    name_read_end,
                    Some(inner_selector_bit_cursor),
                    VerifiedEePlaceableNameKind::LocStringInlineCExoString { byte_length },
                )
            }
        } else {
            let name_read_end = inline_cexo_string_end(bytes, name_read_offset)?;
            let byte_length = name_read_end.checked_sub(name_read_offset + 4)?;
            (
                name_read_end,
                None,
                VerifiedEePlaceableNameKind::DirectCExoString { byte_length },
            )
        };
        if name_read_end > record_end {
            return None;
        }
        read_cursor = name_read_end;
        placeable_name = Some(VerifiedEePlaceableName {
            read_offset: name_read_offset,
            read_end: name_read_end,
            selector_bit_cursor,
            locstring_selector_bit_cursor,
            kind,
        });
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
        position,
        scalar_orientation,
        vector_orientation,
        appearance,
        appearance_offset,
        scale_state,
        state,
        state_bit_cursor,
        placeable_name,
        next_bit_cursor: fragment_cursor,
    })
}

fn decode_ee_position_z(raw: u32) -> f32 {
    const Z_MIN: f32 = -20.0;
    const Z_MAX: f32 = 320.0;
    const Z_RAW_MAX: f32 = ((1_u32 << 18) - 1) as f32;
    Z_MIN + (raw as f32 / Z_RAW_MAX) * (Z_MAX - Z_MIN)
}

fn decode_ee_vector_orientation_component(raw: u16) -> f32 {
    // EE `sub_14079C050` and Diamond `sub_467AE0` both use
    // `ReadFLOAT(-2.0, 2.0, 16)` for vector-orientation components.
    -2.0 + (f32::from(raw) * 4.0 / f32::from(u16::MAX))
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

pub(super) fn parse_legacy_tail9_placeable_name_for_ee(
    bytes: &[u8],
    name_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    selector_bit_cursor: usize,
) -> Option<VerifiedLegacyTail9PlaceableName> {
    if name_offset >= record_end || record_end > bytes.len() {
        return None;
    }

    // Diamond `sub_44EB40` and EE `sub_140797780` use the same outer name
    // selector. The localized branch then enters Diamond `sub_53E700` / EE
    // `sub_1409735F0`, whose inner selector chooses inline CExoString versus
    // the one-byte client-TLK selector plus DWORD strref. Require the byte-side
    // branch to end at this exact tail9 record boundary before it can be
    // preserved in the EE mask.
    let localized = fragment_bits.get(selector_bit_cursor).copied()?;
    if !localized {
        let name_end = inline_cexo_string_end(bytes, name_offset)?;
        if name_end != record_end {
            return None;
        }
        return Some(VerifiedLegacyTail9PlaceableName {
            kind: VerifiedEePlaceableNameKind::DirectCExoString {
                byte_length: name_end.checked_sub(name_offset + 4)?,
            },
            selector_fragment_bits: 1,
        });
    }

    let client_tlk_ref = fragment_bits
        .get(selector_bit_cursor.checked_add(1)?)
        .copied()?;
    if client_tlk_ref {
        let name_end = locstring::tlk_locstring_ref_end(bytes, name_offset)?;
        if name_end != record_end {
            return None;
        }
        Some(VerifiedLegacyTail9PlaceableName {
            kind: VerifiedEePlaceableNameKind::LocStringTlkRef {
                client_tlk: *bytes.get(name_offset)?,
                strref: read_u32_le(bytes, name_offset + 1)?,
            },
            selector_fragment_bits: 2,
        })
    } else {
        let name_end = inline_cexo_string_end(bytes, name_offset)?;
        if name_end != record_end {
            return None;
        }
        Some(VerifiedLegacyTail9PlaceableName {
            kind: VerifiedEePlaceableNameKind::LocStringInlineCExoString {
                byte_length: name_end.checked_sub(name_offset + 4)?,
            },
            selector_fragment_bits: 2,
        })
    }
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
    fn ee_placeable_update_parser_claims_direct_name_branch() {
        let object_id = 0x1234_ABCD_u32;
        let name = b"Storage Drum";
        let mut live = Vec::new();
        live.extend_from_slice(&[b'U', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&LEGACY_UPDATE_NAME_MASK.to_le_bytes());
        live.extend_from_slice(&(name.len() as u32).to_le_bytes());
        live.extend_from_slice(name);
        let bits = [false]; // direct CExoString selector in `sub_140797780`.

        let claim = parse_verified_ee_door_placeable_update_record(&live, 0, live.len(), &bits, 0)
            .expect("EE placeable direct-name update should exact-claim");
        let placeable_name = claim.placeable_name.expect("typed placeable name");

        assert_eq!(placeable_name.read_offset, LEGACY_UPDATE_HEADER_BYTES);
        assert_eq!(placeable_name.read_end, live.len());
        assert_eq!(placeable_name.selector_bit_cursor, 0);
        assert_eq!(placeable_name.locstring_selector_bit_cursor, None);
        assert_eq!(
            placeable_name.kind,
            VerifiedEePlaceableNameKind::DirectCExoString {
                byte_length: name.len()
            }
        );
        assert_eq!(claim.next_bit_cursor, 1);
    }

    #[test]
    fn ee_placeable_update_parser_claims_locstring_inline_name_branch() {
        let object_id = 0x1234_ABCD_u32;
        let name = b"Storage Drum";
        let mut live = Vec::new();
        live.extend_from_slice(&[b'U', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&LEGACY_UPDATE_NAME_MASK.to_le_bytes());
        live.extend_from_slice(&(name.len() as u32).to_le_bytes());
        live.extend_from_slice(name);
        let bits = [true, false]; // outer locstring, inner inline CExoString.

        let claim = parse_verified_ee_door_placeable_update_record(&live, 0, live.len(), &bits, 0)
            .expect("EE placeable locstring-inline name should exact-claim");
        let placeable_name = claim.placeable_name.expect("typed placeable name");

        assert_eq!(placeable_name.read_offset, LEGACY_UPDATE_HEADER_BYTES);
        assert_eq!(placeable_name.read_end, live.len());
        assert_eq!(placeable_name.selector_bit_cursor, 0);
        assert_eq!(placeable_name.locstring_selector_bit_cursor, Some(1));
        assert_eq!(
            placeable_name.kind,
            VerifiedEePlaceableNameKind::LocStringInlineCExoString {
                byte_length: name.len()
            }
        );
        assert_eq!(claim.next_bit_cursor, 2);
    }

    #[test]
    fn ee_placeable_update_parser_claims_locstring_tlk_ref_name_branch() {
        let object_id = 0x1234_ABCD_u32;
        let strref = 0x0100_75D6_u32;
        let mut live = Vec::new();
        live.extend_from_slice(&[b'U', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&LEGACY_UPDATE_NAME_MASK.to_le_bytes());
        live.push(1); // ReadBYTE(1, 1) client TLK selector.
        live.extend_from_slice(&strref.to_le_bytes());
        let bits = [true, true]; // outer locstring, inner client-TLK ref.

        let claim = parse_verified_ee_door_placeable_update_record(&live, 0, live.len(), &bits, 0)
            .expect("EE placeable locstring TLK ref should exact-claim");
        let placeable_name = claim.placeable_name.expect("typed placeable name");

        assert_eq!(placeable_name.read_offset, LEGACY_UPDATE_HEADER_BYTES);
        assert_eq!(placeable_name.read_end, live.len());
        assert_eq!(placeable_name.selector_bit_cursor, 0);
        assert_eq!(placeable_name.locstring_selector_bit_cursor, Some(1));
        assert_eq!(
            placeable_name.kind,
            VerifiedEePlaceableNameKind::LocStringTlkRef {
                client_tlk: 1,
                strref
            }
        );
        assert_eq!(claim.next_bit_cursor, 2);
    }

    #[test]
    fn ee_door_update_parser_rejects_placeable_only_name_branch() {
        let object_id = 0x1234_ABCD_u32;
        let mut live = Vec::new();
        live.extend_from_slice(&[b'U', DOOR_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&LEGACY_UPDATE_NAME_MASK.to_le_bytes());
        live.extend_from_slice(&0_u32.to_le_bytes());

        assert!(
            parse_verified_ee_door_placeable_update_record(&live, 0, live.len(), &[false], 0,)
                .is_none(),
            "EE object type 0x0A dispatches to sub_14076FA20, which does not consume mask 0x80000"
        );
    }

    #[test]
    fn ee_placeable_update_parser_rejects_invalid_locstring_tlk_selector() {
        let object_id = 0x1234_ABCD_u32;
        let mut live = Vec::new();
        live.extend_from_slice(&[b'U', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&LEGACY_UPDATE_NAME_MASK.to_le_bytes());
        live.push(2); // ReadBYTE(1, 1) can only produce 0 or 1.
        live.extend_from_slice(&0x0100_75D6_u32.to_le_bytes());

        assert!(
            parse_verified_ee_door_placeable_update_record(&live, 0, live.len(), &[true, true], 0,)
                .is_none(),
            "the nested TLK branch must retain the decompiled one-bit client selector"
        );
    }

    #[test]
    fn door_placeable_update_parser_retains_custom_appearance_resref() {
        let object_id = 0x1234_ABCD_u32;
        let mask = LEGACY_UPDATE_APPEARANCE_MASK | LEGACY_UPDATE_SCALE_STATE_MASK;
        let mut live = Vec::new();
        live.extend_from_slice(&[b'U', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&mask.to_le_bytes());
        live.extend_from_slice(&0xFFFE_u16.to_le_bytes());
        let resref = *b"custom_statue001";
        live.extend_from_slice(&resref);
        live.extend_from_slice(&1.0_f32.to_le_bytes());
        live.extend_from_slice(&0x0016_u16.to_le_bytes());

        let claim = parse_verified_ee_door_placeable_update_record(&live, 0, live.len(), &[], 0)
            .expect("exact custom appearance update");
        let appearance = claim.appearance.expect("appearance branch");

        assert_eq!(claim.read_end, live.len());
        assert_eq!(claim.appearance_offset, Some(LEGACY_UPDATE_HEADER_BYTES));
        assert_eq!(appearance.read_offset, LEGACY_UPDATE_HEADER_BYTES);
        assert_eq!(appearance.appearance, 0xFFFE);
        assert_eq!(appearance.resref, Some(resref));
        let scale_state = claim.scale_state.expect("scale/state branch");
        assert_eq!(
            scale_state.read_offset,
            LEGACY_UPDATE_HEADER_BYTES
                + EE_UPDATE_APPEARANCE_WORD_READ_BYTES
                + EE_UPDATE_APPEARANCE_RESREF_READ_BYTES
        );
        assert_eq!(scale_state.scale_raw, 1.0_f32.to_bits());
        assert_eq!(scale_state.generic_state_word, 0x0016);
        assert_eq!(claim.state, None);
        assert_eq!(claim.next_bit_cursor, 0);
    }

    #[test]
    fn door_placeable_update_parser_retains_verified_state_branch() {
        let object_id = 0x1234_ABCD_u32;
        let mask = LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_STATE_MASK;
        let mut live = Vec::new();
        live.extend_from_slice(&[b'U', PLACEABLE_OBJECT_TYPE]);
        live.extend_from_slice(&object_id.to_le_bytes());
        live.extend_from_slice(&mask.to_le_bytes());
        live.extend_from_slice(&[0x10, 0x00, 0x20, 0x00, 0x30, 0x00]);
        let bits = vec![
            true, false, // position low Z bits.
            true, false, true, false, true,  // five legacy state bits.
            false, // EE-only neutral state suffix.
            true,  // following stream bit, not owned by this record.
        ];

        let claim = parse_verified_ee_door_placeable_update_record(&live, 0, live.len(), &bits, 0)
            .expect("exact position+state update");
        let state = claim.state.expect("verified state branch");

        assert_eq!(
            claim.state_bit_cursor,
            Some(LEGACY_UPDATE_POSITION_FRAGMENT_BITS)
        );
        assert_eq!(state.bit_cursor, LEGACY_UPDATE_POSITION_FRAGMENT_BITS);
        assert!(state.visual_selector);
        assert!(!state.visual_state_active);
        assert!(state.locked);
        assert!(!state.lockable);
        assert!(state.visual_payload);
        assert!(!state.neutral_ee_state_suffix);
        assert_eq!(
            claim.next_bit_cursor,
            LEGACY_UPDATE_POSITION_FRAGMENT_BITS + LEGACY_UPDATE_STATE_FRAGMENT_BITS + 1
        );
    }

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
