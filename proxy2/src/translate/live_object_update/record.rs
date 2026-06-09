//! Typed live-object `U` update-record translation.
//!
//! This module owns the exact semantic question for update records:
//! given a bounded legacy `U` record and its fragment cursor, what EE-shaped
//! record and bit stream should be emitted?

use super::{
    CNW_FRAGMENT_HEADER_BITS, CREATURE_OBJECT_TYPE, DOOR_OBJECT_TYPE,
    EE_UPDATE_APPEARANCE_WORD_READ_BYTES, EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS,
    EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES, EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS,
    EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES, EE_UPDATE_SCALE_STATE_READ_BYTES, ITEM_OBJECT_TYPE,
    LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK, LEGACY_UPDATE_APPEARANCE_MASK, LEGACY_UPDATE_HEADER_BYTES,
    LEGACY_UPDATE_NAME_MASK, LEGACY_UPDATE_ORIENTATION_MASK, LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
    LEGACY_UPDATE_POSITION_MASK, LEGACY_UPDATE_POSITION_READ_BYTES, LEGACY_UPDATE_SCALE_STATE_MASK,
    LEGACY_UPDATE_STATE_FRAGMENT_BITS, LEGACY_UPDATE_STATE_MASK, PLACEABLE_OBJECT_TYPE,
    TRIGGER_OBJECT_TYPE, bits, boundary, door, effects, item, locstring, object_ids, placeable,
    read_f32_le, read_u16_le, read_u32_le, reader, trigger, world_status, write_u32_le, writer,
};
use crate::translate::area::{AreaPlaceableContext, AreaPlaceableContextState};
#[cfg(test)]
use crate::translate::area::{
    AreaPlaceableContextRow, AreaPlaceableContextRowKind, format_area_placeable_context_row,
};

const MAX_DOOR_PLACEABLE_UPDATE_INTERLEAVED_FRAGMENT_STORAGE_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RecordRewrite {
    pub(super) rewritten: bool,
    pub(super) mask_changed: bool,
    pub(super) bits_changed: bool,
    pub(super) terminal_fragment_trim_allowed: bool,
    pub(super) bytes_inserted: u32,
    pub(super) bytes_removed: u32,
    pub(super) bits_inserted: u32,
    pub(super) bits_removed: u32,
    pub(super) item_update_claim: Option<item::ItemUpdateRewriteClaim>,
    pub(super) bit_claim: Option<RecordRewriteBitClaim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecordRewriteBitClaim {
    pub(super) source_mask: u32,
    pub(super) translated_mask: u32,
    pub(super) source_bits_consumed: u32,
    pub(super) family: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum OrientationFragmentRewrite {
    PreserveExisting,
    ForceScalar,
    ForceVector,
    InsertLegacyByteScalarPad,
    InsertScalar(u16),
}

#[derive(Debug, Clone, Copy, Default)]
struct FragmentRewrite {
    bits_changed: bool,
    bits_inserted: u32,
    bits_removed: u32,
}

#[derive(Debug, Clone, Copy)]
struct CompactDoorPlaceableTail9UpdateClaim {
    tail_offset: usize,
    tail: reader::LegacyNamedUpdateTail,
    following_payload_ready: bool,
    translated_mask: u32,
    fragment_source_mask: u32,
    orientation_rewrite: OrientationFragmentRewrite,
}

impl CompactDoorPlaceableTail9UpdateClaim {
    fn tail_needs_empty_name(self) -> bool {
        !self.following_payload_ready
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlaceableUpdateStateBits {
    visual_selector: bool,
    visual_state_active: bool,
    locked: bool,
    lockable: bool,
    visual_payload: bool,
}

pub(super) fn rewrite_update_record_for_ee(
    live_bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    bit_cursor_reliable: &mut bool,
    record_offset: usize,
) -> Option<RecordRewrite> {
    rewrite_update_record_for_ee_with_area_context(
        live_bytes,
        record_end,
        bits,
        bit_cursor,
        bit_cursor_reliable,
        record_offset,
        None,
    )
}

pub(super) fn rewrite_update_record_for_ee_with_area_context(
    live_bytes: &mut Vec<u8>,
    record_end: &mut usize,
    bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    bit_cursor_reliable: &mut bool,
    record_offset: usize,
    area_context: Option<&AreaPlaceableContext>,
) -> Option<RecordRewrite> {
    if record_offset + LEGACY_UPDATE_HEADER_BYTES > *record_end || *record_end > live_bytes.len() {
        return None;
    }

    let object_type = live_bytes[record_offset + 1];
    let object_id = read_u32_le(live_bytes, record_offset + 2)?;
    let raw_mask = read_u32_le(live_bytes, record_offset + 6)?;
    let original_bit_cursor = *bit_cursor;
    if matches!(
        object_type,
        PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE | CREATURE_OBJECT_TYPE
    ) && raw_mask == effects::LOOPING_VISUAL_EFFECT_UPDATE_MASK
    {
        let effect_rewrite = effects::rewrite_legacy_looping_visual_effect_update_for_ee(
            live_bytes,
            record_offset,
            record_end,
        )?;
        return Some(RecordRewrite {
            rewritten: effect_rewrite.bytes_inserted != 0,
            bytes_inserted: effect_rewrite.bytes_inserted,
            ..RecordRewrite::default()
        });
    }
    if object_type == CREATURE_OBJECT_TYPE
        && raw_mask == 0
        && effects::has_legacy_looping_visual_effect_body_without_mask(
            live_bytes,
            record_offset,
            *record_end,
        )
    {
        // EE and Diamond only read the looping visual-effect delta body when
        // update-mask bit `0x0008` is set. Local XP2 Chapter 2 captures can
        // carry the exact `WORD count` + short effect rows with that bit
        // omitted. Repair only this bounded creature-body shape, then reuse
        // the normal EE visual-transform expansion and exact validator.
        let mut candidate = live_bytes.clone();
        let mut candidate_record_end = *record_end;
        write_u32_le(
            &mut candidate,
            record_offset + 6,
            effects::LOOPING_VISUAL_EFFECT_UPDATE_MASK,
        )?;
        let effect_rewrite = effects::rewrite_legacy_looping_visual_effect_update_for_ee(
            &mut candidate,
            record_offset,
            &mut candidate_record_end,
        )?;
        if !effects::is_verified_ee_looping_visual_effect_update_record(
            &candidate,
            record_offset,
            candidate_record_end,
        ) {
            return None;
        }
        *live_bytes = candidate;
        *record_end = candidate_record_end;
        return Some(RecordRewrite {
            rewritten: true,
            mask_changed: true,
            bytes_inserted: effect_rewrite.bytes_inserted,
            ..RecordRewrite::default()
        });
    }

    if object_type == ITEM_OBJECT_TYPE {
        if !*bit_cursor_reliable {
            return None;
        }
        let Some(item_rewrite) = item::rewrite_update_record_for_ee(
            live_bytes,
            record_offset,
            record_end,
            bits,
            *bit_cursor,
        ) else {
            debug_update_record_reject(
                "item-update-fragment-cursor-unowned",
                live_bytes,
                record_offset,
                *record_end,
                raw_mask,
                item::translate_update_mask(raw_mask),
                *bit_cursor,
            );
            *bit_cursor_reliable = false;
            return None;
        };
        *bit_cursor = item_rewrite.next_bit_cursor;
        let rewrite = RecordRewrite {
            rewritten: item_rewrite.rewritten,
            mask_changed: item_rewrite.mask_changed,
            bytes_removed: item_rewrite.bytes_removed,
            item_update_claim: Some(item_rewrite.claim),
            ..RecordRewrite::default()
        };
        if rewrite.rewritten {
            tracing::info!(
                object_type,
                object_id = format_args!("0x{object_id:08X}"),
                raw_mask = format_args!("0x{raw_mask:08X}"),
                translated_mask = format_args!("0x{:08X}", item::translate_update_mask(raw_mask)),
                record_offset,
                record_end = *record_end,
                bytes_removed = rewrite.bytes_removed,
                "server->client live-object item update translated for EE"
            );
        }
        return Some(rewrite);
    }
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
    let mut compact_tail9_claim = None;
    let mut inline_name_drop_begin = None;
    let mut byte_gap_drop_range: Option<(usize, usize)> = None;
    let mut inline_name_compact_proven = false;
    let mut low_prefix_interleaved_fragment_span_begin = None;
    let mut fragment_source_mask = raw_mask;
    let mut legacy_low_tail_fragment_bits_to_remove = 0usize;
    let mut low_tail_zero_fragment_bits_to_insert = 0usize;
    let mut orientation_fragment_rewrite =
        if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
            && (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
        {
            // Diamond `sub_467AE0` and EE `sub_14079C050` use the same generic
            // orientation branch: BOOL false => scalar `ReadFLOAT(10,12)`;
            // BOOL true => three `ReadFLOAT(-2,2,16)` vector components.
            // Preserve that branch by default. The legacy-tail converter below
            // is kept only for older captures that genuinely carried a compact
            // facing WORD outside the shared generic reader shape.
            OrientationFragmentRewrite::PreserveExisting
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
            if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                tracing::trace!(
                    object_type,
                    object_id = format_args!("0x{object_id:08X}"),
                    raw_mask = format_args!("0x{raw_mask:08X}"),
                    record_offset,
                    record_end = *record_end,
                    "server->client live-object update record rejected: empty record has non-state mask bits"
                );
            }
            return None;
        }
        translated_mask = LEGACY_UPDATE_STATE_MASK;
        can_translate_read_buffer = true;
        fragment_source_mask = translated_mask;
    } else if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
    {
        if let Some(claim) = parse_compact_door_placeable_tail9_update_claim(
            live_bytes,
            record_offset,
            *record_end,
            object_type,
            raw_mask,
            translated_mask,
        ) {
            // Legacy compact door/placeable update captures can carry a bounded
            // nine-byte tail at the post-position cursor: WORD facing, one
            // legacy generic byte, FLOAT scale, WORD generic state. Direct
            // Diamond `nwserver.exe` disassembly of the normal 0x445160 writer
            // uses an orientation BOOL for mask 0x0002, so do not cite this
            // compact tail9 shape as that normal writer path. A 2026-06-08
            // stock server `U` writer census found no other typed
            // `U/type/id/mask` writer for this row. The only executable
            // little-endian `0xFFFFFFF7` hit is inside the 0x4401F0 add/snapshot
            // writer. That path passes the mask into the 0x44AC70 snapshot field
            // copier, whose checked range has no direct CNW write calls, before
            // 0x4401F0 emits an `A/type/id` row. The HGX decompile's five
            // 0xFFFFFFF7 text hits are mask/string cleanup, not CNWMessage
            // emission. Keep the compact tail as
            // capture-backed legacy evidence until a source writer is proven.
            // These bytes can also accidentally form a
            // bounded CExoString candidate, so this typed tail reader must win
            // before the compact inline-name repair is considered.
            tail_ready = true;
            tail_needs_empty_name = claim.tail_needs_empty_name();
            translated_mask = claim.translated_mask;
            fragment_source_mask = claim.fragment_source_mask;
            orientation_fragment_rewrite = claim.orientation_rewrite;
            compact_tail9_claim = Some(claim);
        } else if let Some(inline_name) =
            reader::parse_legacy_inline_named_door_placeable_update_record_for_ee(
                live_bytes,
                record_offset,
                *record_end,
                bits,
                *bit_cursor,
            )
        {
            debug_assert_eq!(inline_name.name_end, *record_end);
            inline_name_drop_begin = Some(inline_name.read_without_name_end);
            inline_name_compact_proven = true;
            can_translate_read_buffer = true;
            if (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
                let orientation_bit_cursor = *bit_cursor
                    + if (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
                        LEGACY_UPDATE_POSITION_FRAGMENT_BITS
                    } else {
                        0
                    };
                let compact_legacy_tail_bits = if (raw_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
                    LEGACY_UPDATE_STATE_FRAGMENT_BITS
                } else {
                    0
                } + 1;
                if bits.len().saturating_sub(orientation_bit_cursor) == compact_legacy_tail_bits
                    && door_placeable_update_read_end_for_orientation_branch(
                        live_bytes,
                        record_offset,
                        inline_name.read_without_name_end,
                        raw_mask,
                        false,
                    ) == Some(inline_name.read_without_name_end)
                    && door_placeable_update_read_end_for_orientation_branch(
                        live_bytes,
                        record_offset,
                        inline_name.read_without_name_end,
                        raw_mask,
                        true,
                    ) != Some(inline_name.read_without_name_end)
                {
                    // Some Diamond captures carry the scalar orientation high
                    // byte in the read buffer but do not carry the scalar
                    // low-nibble fragment bits before the state/name bits.
                    // EE's reader still expects a scalar selector plus four
                    // low bits, so insert zero padding at the decompile-owned
                    // orientation branch instead of consuming state bits as
                    // orientation data.
                    orientation_fragment_rewrite =
                        OrientationFragmentRewrite::InsertLegacyByteScalarPad;
                }
            }
        }
    }

    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && !tail_ready
        && !inline_name_compact_proven
        && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
        && (raw_mask & !translated_mask) != 0
    {
        if let Some(prefix_end) = door_placeable_update_read_end_for_orientation_branch(
            live_bytes,
            record_offset,
            *record_end,
            translated_mask,
            false,
        ) {
            if prefix_end < *record_end {
                // Diamond's low generic door/placeable update fields are
                // byte-compatible with EE for the scalar orientation branch, but
                // HG/Diamond can append the legacy name/fragment storage inside
                // the read-body range before the next live-object boundary. EE
                // has no generic name consumer for this family, so keep the
                // exact shared low-mask prefix, promote only the source fragment
                // bits needed to prove that prefix, then drop the legacy-only
                // tail.
                tail_ready = false;
                tail_needs_empty_name = false;
                can_translate_read_buffer = true;
                inline_name_drop_begin = Some(prefix_end);
                low_prefix_interleaved_fragment_span_begin = Some(prefix_end);
                fragment_source_mask = translated_mask | LEGACY_UPDATE_NAME_MASK;
                orientation_fragment_rewrite = OrientationFragmentRewrite::ForceScalar;
            }
        }
    } else if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && translated_mask != raw_mask
        && (raw_mask & LEGACY_UPDATE_NAME_MASK) == 0
        && (raw_mask & !translated_mask) != 0
        && (raw_mask & !translated_mask & !LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK) == 0
    {
        let mut low_tail_candidate_mask = translated_mask;
        let mut low_tail_prefix_end =
            verified_door_placeable_update_read_end_for_current_orientation_branch(
                live_bytes,
                record_offset,
                *record_end,
                low_tail_candidate_mask,
                bits,
                *bit_cursor,
            )
            .filter(|prefix_end| {
                *prefix_end == *record_end
                    || reader::legacy_name_tail_ready(live_bytes, *prefix_end, *record_end)
                    || reader::legacy_low_bit_control_tail_ready(
                        live_bytes,
                        *prefix_end,
                        *record_end,
                    )
            });
        if low_tail_prefix_end.is_none() && (translated_mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
            // `0x20` is decompile-owned in EE `sub_14079C050` and Diamond
            // `sub_467AE0`: it must read at least a WORD. Some CEP placeable
            // low-bit updates set that bit without carrying the read bytes.
            // In that exact case, the only valid EE shape is the same shared
            // prefix with the absent appearance field removed.
            let without_appearance = translated_mask & !LEGACY_UPDATE_APPEARANCE_MASK;
            if let Some(prefix_end) =
                verified_door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    *record_end,
                    without_appearance,
                    false,
                )
                .filter(|prefix_end| {
                    reader::legacy_low_bit_control_tail_ready(live_bytes, *prefix_end, *record_end)
                        && door_placeable_update_read_end_for_orientation_branch(
                            live_bytes,
                            record_offset,
                            *prefix_end,
                            without_appearance,
                            true,
                        ) != Some(*prefix_end)
                })
            {
                // Prelude's local `U/9 0xF7` shape combines the same absent
                // appearance bit with the bounded low 0x40/0x80 suffix. The
                // source fragment stream resumes at state bits where EE would
                // read the orientation selector; the vector branch can leave a
                // one-byte zero suffix that also resembles a packed-name tail,
                // but the raw mask has no name bit and the scalar byte cursor
                // owns the complete low-bit suffix.
                low_tail_candidate_mask = without_appearance;
                low_tail_prefix_end = Some(prefix_end);
            } else if let Some(prefix_end) =
                verified_door_placeable_update_read_end_for_current_orientation_branch(
                    live_bytes,
                    record_offset,
                    *record_end,
                    without_appearance,
                    bits,
                    *bit_cursor,
                )
                .filter(|prefix_end| {
                    *prefix_end == *record_end
                        || reader::legacy_name_tail_ready(live_bytes, *prefix_end, *record_end)
                })
            {
                low_tail_candidate_mask = without_appearance;
                low_tail_prefix_end = Some(prefix_end);
            } else if let Some((appearance_gap_begin, appearance_gap_end, prefix_end)) =
                stale_absent_appearance_gap_for_scalar_update(
                    live_bytes,
                    record_offset,
                    *record_end,
                    without_appearance,
                )
                .filter(|(_, _, prefix_end)| {
                    reader::legacy_low_bit_control_tail_ready(live_bytes, *prefix_end, *record_end)
                })
            {
                // Some legacy low-tail rows carry two stale bytes at the EE
                // appearance cursor even though the original 1.69/EE reader
                // has no valid mask-0x20 appearance field for this shape. The
                // scale field is only decompile-plausible after that gap is
                // skipped, so remove the gap before removing the low-tail
                // suffix; clearing only the bit would shift scale/state.
                low_tail_candidate_mask = without_appearance;
                low_tail_prefix_end = Some(prefix_end);
                byte_gap_drop_range = Some((appearance_gap_begin, appearance_gap_end));
                if bits.len() == *bit_cursor
                    && (raw_mask & !without_appearance & LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK) != 0
                    && !suffix_is_fragment_neutral_work_remaining_only(live_bytes, *record_end)
                {
                    low_tail_zero_fragment_bits_to_insert =
                        low_prefix_door_placeable_update_source_fragment_bits(without_appearance);
                }
            } else if let Some(prefix_end) =
                verified_door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    *record_end,
                    without_appearance,
                    false,
                )
                .filter(|prefix_end| {
                    (*prefix_end == *record_end
                        || reader::legacy_name_tail_ready(live_bytes, *prefix_end, *record_end)
                        || reader::legacy_low_bit_control_tail_ready(
                            live_bytes,
                            *prefix_end,
                            *record_end,
                        ))
                        && door_placeable_update_read_end_for_orientation_branch(
                            live_bytes,
                            record_offset,
                            *prefix_end,
                            without_appearance,
                            true,
                        ) != Some(*prefix_end)
                })
            {
                // Prelude's local `U/9 0xF7` shape combines the same absent
                // appearance bit with the bounded low 0x40/0x80 suffix, but the
                // source fragment stream resumes at state bits where EE would
                // read the orientation selector. The scalar byte cursor is the
                // only decompile-valid prefix: the vector branch would run into
                // the low-tail suffix, so let the shared low-tail block below
                // insert the missing scalar fragment bits.
                low_tail_candidate_mask = without_appearance;
                low_tail_prefix_end = Some(prefix_end);
            }
        }
        if low_tail_prefix_end.is_none()
            && bits.len() == *bit_cursor
            && (raw_mask & !translated_mask & LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK) != 0
            && !suffix_is_fragment_neutral_work_remaining_only(live_bytes, *record_end)
        {
            if let Some(prefix_end) =
                verified_door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    *record_end,
                    low_tail_candidate_mask,
                    false,
                )
                .filter(|prefix_end| {
                    reader::legacy_low_bit_control_tail_ready(live_bytes, *prefix_end, *record_end)
                })
            {
                if door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    prefix_end,
                    low_tail_candidate_mask,
                    true,
                ) != Some(prefix_end)
                {
                    // Some legacy no-fragment low-tail records arrive after
                    // preceding records have consumed the CNW bit stream. The
                    // read-buffer prefix still proves the scalar branch and the
                    // trailing WORD/zero-WORD suffix, but EE's generic reader
                    // needs the low bits for position, scalar orientation, and
                    // neutral state. Insert only those zero lower bits for this
                    // exact shape; a following fragment-neutral `W` suffix is
                    // not enough to own this insertion.
                    low_tail_prefix_end = Some(prefix_end);
                    low_tail_zero_fragment_bits_to_insert =
                        low_prefix_door_placeable_update_source_fragment_bits(
                            low_tail_candidate_mask,
                        );
                }
            }
        }
        if let Some(prefix_end) = low_tail_prefix_end {
            let unsupported_low_tail_bits = raw_mask & !low_tail_candidate_mask;
            if low_tail_zero_fragment_bits_to_insert == 0
                && bits.len() == *bit_cursor
                && (unsupported_low_tail_bits & LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK) != 0
                && (unsupported_low_tail_bits
                    & !(LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK | LEGACY_UPDATE_APPEARANCE_MASK))
                    == 0
                && prefix_end < *record_end
                && reader::legacy_low_bit_control_tail_ready(live_bytes, prefix_end, *record_end)
                && !suffix_is_fragment_neutral_work_remaining_only(live_bytes, *record_end)
                && verified_door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    prefix_end,
                    low_tail_candidate_mask,
                    false,
                ) == Some(prefix_end)
                && verified_door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    prefix_end,
                    low_tail_candidate_mask,
                    true,
                ) != Some(prefix_end)
            {
                // With no remaining CNW source bits, the already-proven
                // read-buffer prefix and bounded low-bit suffix identify the
                // same decompile-owned scalar shape. Insert neutral source bits
                // only when the record is not followed solely by fragment-
                // neutral `W` rows; `W current total` consumes no bits and
                // cannot serve as a cursor owner for this repair.
                low_tail_zero_fragment_bits_to_insert =
                    low_prefix_door_placeable_update_source_fragment_bits(low_tail_candidate_mask);
            }
            // CEP v2.2 and XP2 local Diamond door/placeable updates can set low
            // 0x40/0x80 mask bits and append a bounded legacy name/control tail
            // after the exact shared generic prefix. EE has no reader for those
            // low bits in either the generic update leg (`sub_14079C050`) or
            // door/placeable-specific legs, so the bridge must prove the prefix
            // and remove only the legacy-only tail before emitting the EE mask.
            translated_mask = low_tail_candidate_mask;
            can_translate_read_buffer = true;
            fragment_source_mask = translated_mask;
            inline_name_drop_begin = (prefix_end < *record_end).then_some(prefix_end);
            if (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
                && (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
            {
                let orientation_bit_cursor = *bit_cursor
                    + if (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
                        LEGACY_UPDATE_POSITION_FRAGMENT_BITS
                    } else {
                        0
                    };
                let source_tail_bits = if (raw_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
                    LEGACY_UPDATE_STATE_FRAGMENT_BITS
                } else {
                    0
                };
                let available_after_orientation = bits.len().saturating_sub(orientation_bit_cursor);
                let legacy_low_tail_bits = (raw_mask
                    & !translated_mask
                    & LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK)
                    .count_ones() as usize;
                let scalar_prefix_proven =
                    verified_door_placeable_update_read_end_for_orientation_branch(
                        live_bytes,
                        record_offset,
                        prefix_end,
                        translated_mask,
                        false,
                    ) == Some(prefix_end)
                        && verified_door_placeable_update_read_end_for_orientation_branch(
                            live_bytes,
                            record_offset,
                            prefix_end,
                            translated_mask,
                            true,
                        ) != Some(prefix_end);
                let low_tail_suffix_proven =
                    reader::legacy_low_bit_control_tail_ready(live_bytes, prefix_end, *record_end);
                let missing_scalar_bits_at_low_tail = low_tail_suffix_proven
                    && bits.get(orientation_bit_cursor).copied() == Some(true)
                    && available_after_orientation
                        >= source_tail_bits.saturating_add(legacy_low_tail_bits);
                if available_after_orientation >= source_tail_bits
                    && (available_after_orientation
                        < EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                            .saturating_add(source_tail_bits)
                        || missing_scalar_bits_at_low_tail)
                    && scalar_prefix_proven
                {
                    // The low 0x40/0x80 placeable-tail captures can have the
                    // same scalar-orientation split as the inline-name legacy
                    // form above: the read buffer carries the high eight yaw
                    // bits, while the legacy fragment stream resumes at the
                    // state bits. EE's scalar branch still needs the selector
                    // plus four low bits, so insert zero padding only after the
                    // typed low-tail prefix proves the scalar byte cursor.
                    orientation_fragment_rewrite =
                        OrientationFragmentRewrite::InsertLegacyByteScalarPad;
                    if available_after_orientation
                        == source_tail_bits.saturating_add(legacy_low_tail_bits)
                        || missing_scalar_bits_at_low_tail
                    {
                        // The two low placeable-specific mask bits are
                        // Diamond-only input for this tail form. Once their
                        // bounded WORD/zero-WORD read-buffer suffix has been
                        // proven and dropped, remove the matching fragment
                        // BOOLs so the following compact add starts at its own
                        // decompiled cursor.
                        legacy_low_tail_fragment_bits_to_remove = legacy_low_tail_bits;
                    }
                }
            }
        }
    } else if object_type == PLACEABLE_OBJECT_TYPE
        && raw_mask == translated_mask
        && raw_mask
            == (LEGACY_UPDATE_POSITION_MASK
                | LEGACY_UPDATE_ORIENTATION_MASK
                | LEGACY_UPDATE_SCALE_STATE_MASK
                | LEGACY_UPDATE_STATE_MASK)
    {
        if let Some(prefix_end) = door_placeable_update_read_end_for_orientation_branch(
            live_bytes,
            record_offset,
            *record_end,
            translated_mask,
            false,
        )
        .filter(|prefix_end| {
            prefix_end
                .checked_add(2)
                .is_some_and(|name_offset| name_offset <= *record_end)
                && read_u16_le(live_bytes, *prefix_end).is_some()
                && reader::legacy_name_tail_ready(live_bytes, *prefix_end + 2, *record_end)
        }) {
            // Local CEP placeable updates can append a legacy-only generic
            // WORD plus direct CExoString after the exact EE shared
            // position/orientation/scale/state prefix even though the name bit
            // is not present. EE `sub_14079C050` and placeable-specific
            // `sub_140797780` have no consumer for that suffix; once the typed
            // prefix is proven, drop only the suffix before exact validation.
            can_translate_read_buffer = true;
            inline_name_drop_begin = Some(prefix_end);
            fragment_source_mask = translated_mask;
            orientation_fragment_rewrite = OrientationFragmentRewrite::ForceScalar;
        }
    }
    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && raw_mask == translated_mask
        && raw_mask
            == (LEGACY_UPDATE_POSITION_MASK
                | LEGACY_UPDATE_ORIENTATION_MASK
                | LEGACY_UPDATE_SCALE_STATE_MASK
                | LEGACY_UPDATE_STATE_MASK)
        && byte_gap_drop_range.is_none()
    {
        if let Some((appearance_gap_begin, appearance_gap_end, prefix_end)) =
            stale_absent_appearance_gap_for_scalar_update(
                live_bytes,
                record_offset,
                *record_end,
                translated_mask,
            )
        {
            // Old accepted fixtures can carry two stale bytes at the same
            // post-scalar cursor even after mask 0x20 has already been cleared.
            // The original generic reader reaches scale immediately after the
            // scalar byte for mask 0x17, so prove that scale only after the gap,
            // force the scalar fragment branch, and remove any bounded suffix.
            can_translate_read_buffer = true;
            byte_gap_drop_range = Some((appearance_gap_begin, appearance_gap_end));
            if prefix_end < *record_end {
                inline_name_drop_begin = Some(prefix_end);
                low_prefix_interleaved_fragment_span_begin = Some(prefix_end);
            }
            fragment_source_mask = translated_mask;
            orientation_fragment_rewrite = OrientationFragmentRewrite::ForceScalar;
        }
    }
    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && raw_mask == translated_mask
        && can_translate_read_buffer
        && inline_name_drop_begin.is_none()
        && byte_gap_drop_range.is_none()
        && update_record_owns_fragment_bits(object_type, translated_mask)
        && *record_end < live_bytes.len()
        && boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, *record_end)
    {
        let prefix_end = match orientation_fragment_rewrite {
            OrientationFragmentRewrite::ForceScalar
            | OrientationFragmentRewrite::InsertLegacyByteScalarPad
            | OrientationFragmentRewrite::InsertScalar(_) => {
                verified_door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    *record_end,
                    translated_mask,
                    false,
                )
            }
            OrientationFragmentRewrite::ForceVector => {
                verified_door_placeable_update_read_end_for_orientation_branch(
                    live_bytes,
                    record_offset,
                    *record_end,
                    translated_mask,
                    true,
                )
            }
            OrientationFragmentRewrite::PreserveExisting => {
                verified_door_placeable_update_read_end_for_current_orientation_branch(
                    live_bytes,
                    record_offset,
                    *record_end,
                    translated_mask,
                    bits,
                    *bit_cursor,
                )
            }
        };
        if let Some(prefix_end) = prefix_end.filter(|prefix_end| {
            door_placeable_update_interleaved_fragment_storage_suffix_ready(
                live_bytes,
                *prefix_end,
                *record_end,
            )
        }) {
            // Diamond `sub_467AE0` and EE `sub_14079C050` both stop this
            // generic door/placeable update at the typed prefix proven above.
            // A bounded CNW fragment-storage span before the next top-level
            // live-object boundary is transport storage, not scale/state data.
            inline_name_drop_begin = Some(prefix_end);
        }
    }

    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
        && (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0
    {
        let orientation_read_target = inline_name_drop_begin.unwrap_or(*record_end);
        let orientation_bit_cursor = *bit_cursor
            + if (raw_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
                LEGACY_UPDATE_POSITION_FRAGMENT_BITS
            } else {
                0
            };
        let scalar_read_end = door_placeable_update_read_end_for_orientation_branch(
            live_bytes,
            record_offset,
            orientation_read_target,
            raw_mask,
            false,
        );
        let vector_read_end = door_placeable_update_read_end_for_orientation_branch(
            live_bytes,
            record_offset,
            orientation_read_target,
            raw_mask,
            true,
        );
        let source_tail_bits = if (raw_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
            LEGACY_UPDATE_STATE_FRAGMENT_BITS
        } else {
            0
        };
        let available_after_orientation = bits.len().saturating_sub(orientation_bit_cursor);
        if scalar_read_end == Some(orientation_read_target)
            && vector_read_end != Some(orientation_read_target)
            && available_after_orientation >= source_tail_bits
            && available_after_orientation
                < EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS.saturating_add(source_tail_bits)
        {
            // Some legacy streams carry the scalar orientation high byte in the
            // read buffer but resume the fragment cursor directly at state
            // bits. EE still needs the scalar selector plus four low bits, so
            // insert neutral scalar bits before consuming the proven state
            // block.
            orientation_fragment_rewrite = OrientationFragmentRewrite::InsertLegacyByteScalarPad;
        }
        match bits.get(orientation_bit_cursor).copied() {
            Some(false)
                if scalar_read_end != Some(orientation_read_target)
                    && vector_read_end == Some(orientation_read_target) =>
            {
                // Diamond `sub_467AE0` and EE `sub_14079C050` both branch on
                // this BOOL before reading orientation. If the byte cursor only
                // matches the six-byte vector branch, force that exact reader
                // path instead of letting a stale false bit shift later fields.
                orientation_fragment_rewrite = OrientationFragmentRewrite::ForceVector;
            }
            Some(true)
                if scalar_read_end == Some(orientation_read_target)
                    && vector_read_end != Some(orientation_read_target)
                    && !matches!(
                        orientation_fragment_rewrite,
                        OrientationFragmentRewrite::InsertLegacyByteScalarPad
                    ) =>
            {
                // The inverse stale selector occurs in older HG fixtures:
                // scalar read bytes land exactly on the record end, while the
                // vector branch overflows into the next submessage.
                orientation_fragment_rewrite = OrientationFragmentRewrite::ForceScalar;
            }
            _ => {}
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
        debug_update_record_reject(
            "read-buffer-unclaimed-before-bit-rewrite",
            live_bytes,
            record_offset,
            *record_end,
            raw_mask,
            translated_mask,
            *bit_cursor,
        );
        return None;
    }

    if let Some(span_begin) = low_prefix_interleaved_fragment_span_begin {
        if *record_end == live_bytes.len()
            && inline_name_drop_begin == Some(span_begin)
            && !inline_name_compact_proven
            && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
        {
            // The compact inline-name path above owns the terminal Diamond
            // name-drop shape when the source fragment bits are exact. If an
            // isolated terminal record instead needs to borrow bits from the
            // inline-name read body, there is no following record to prove that
            // the remaining source bitstream is aligned.
            debug_update_record_reject(
                "terminal-door-placeable-inline-name-interleaved-fragment-span",
                live_bytes,
                record_offset,
                *record_end,
                raw_mask,
                translated_mask,
                *bit_cursor,
            );
            return None;
        }
        let required_source_bits =
            low_prefix_door_placeable_update_source_fragment_bits(fragment_source_mask);
        let available_bits = bits.len().saturating_sub(*bit_cursor);
        if available_bits < required_source_bits {
            let missing_bits = required_source_bits - available_bits;
            let promote_bytes = missing_bits.saturating_add(7) / 8;
            if span_begin >= *record_end
                || span_begin.saturating_add(promote_bytes) > *record_end
                || promote_bytes == 0
            {
                debug_update_record_reject(
                    "low-prefix-fragment-span-too-short",
                    live_bytes,
                    record_offset,
                    *record_end,
                    raw_mask,
                    translated_mask,
                    *bit_cursor,
                );
                return None;
            }
            let promoted_bits = first_msb_bits(
                &live_bytes[span_begin..span_begin + promote_bytes],
                missing_bits,
            )?;
            bits::insert_msb_bits(
                bits,
                bit_cursor.saturating_add(available_bits),
                &promoted_bits,
            )?;
            live_bytes.drain(span_begin..span_begin + promote_bytes);
            *record_end = record_end.saturating_sub(promote_bytes);
            rewrite.bits_inserted = rewrite
                .bits_inserted
                .saturating_add(u32::try_from(missing_bits).unwrap_or(u32::MAX));
            rewrite.bytes_removed = rewrite
                .bytes_removed
                .saturating_add(u32::try_from(promote_bytes).unwrap_or(u32::MAX));
        }
    }

    let update_bits_present = update_record_owns_fragment_bits(object_type, translated_mask);
    if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && raw_mask == translated_mask
        && can_translate_read_buffer
        && update_bits_present
    {
        let read_target = inline_name_drop_begin.unwrap_or(*record_end);
        let verified_read_end = if byte_gap_drop_range.is_some() {
            stale_absent_appearance_gap_for_scalar_update(
                live_bytes,
                record_offset,
                read_target,
                translated_mask,
            )
            .map(|(_, _, prefix_end)| prefix_end)
        } else {
            match orientation_fragment_rewrite {
                OrientationFragmentRewrite::ForceScalar
                | OrientationFragmentRewrite::InsertLegacyByteScalarPad
                | OrientationFragmentRewrite::InsertScalar(_) => {
                    verified_door_placeable_update_read_end_for_orientation_branch(
                        live_bytes,
                        record_offset,
                        read_target,
                        translated_mask,
                        false,
                    )
                }
                OrientationFragmentRewrite::ForceVector => {
                    verified_door_placeable_update_read_end_for_orientation_branch(
                        live_bytes,
                        record_offset,
                        read_target,
                        translated_mask,
                        true,
                    )
                }
                OrientationFragmentRewrite::PreserveExisting => {
                    verified_door_placeable_update_read_end_for_current_orientation_branch(
                        live_bytes,
                        record_offset,
                        read_target,
                        translated_mask,
                        bits,
                        *bit_cursor,
                    )
                }
            }
        };
        if verified_read_end != Some(read_target) {
            debug_update_record_reject(
                "door-placeable-read-buffer-unproven-before-bit-rewrite",
                live_bytes,
                record_offset,
                *record_end,
                raw_mask,
                translated_mask,
                *bit_cursor,
            );
            return None;
        }
    }
    let source_placeable_state_bits = if object_type == PLACEABLE_OBJECT_TYPE && update_bits_present
    {
        let mut source_bits = bits.clone();
        let zero_bits_inserted = if low_tail_zero_fragment_bits_to_insert == 0 {
            true
        } else {
            let inserted = vec![false; low_tail_zero_fragment_bits_to_insert];
            bits::insert_msb_bits(&mut source_bits, *bit_cursor, &inserted).is_some()
        };
        if zero_bits_inserted {
            placeable_update_source_state_bits_at(
                &source_bits,
                *bit_cursor,
                fragment_source_mask,
                orientation_fragment_rewrite,
            )
        } else {
            None
        }
    } else if object_type == PLACEABLE_OBJECT_TYPE {
        placeable_update_state_bits_at(bits, original_bit_cursor, raw_mask)
    } else {
        None
    };
    let bit_rewrite_candidate = if update_bits_present {
        if !*bit_cursor_reliable {
            *bit_cursor_reliable = false;
            return None;
        }
        let mut rewritten_bits = bits.clone();
        let mut rewritten_bit_cursor = *bit_cursor;
        let mut preinserted_zero_bits = 0u32;
        if low_tail_zero_fragment_bits_to_insert != 0 {
            let inserted = vec![false; low_tail_zero_fragment_bits_to_insert];
            bits::insert_msb_bits(&mut rewritten_bits, rewritten_bit_cursor, &inserted)?;
            preinserted_zero_bits =
                u32::try_from(low_tail_zero_fragment_bits_to_insert).unwrap_or(u32::MAX);
        }
        let Some(bit_rewrite) = rewrite_legacy_live_object_update_bits(
            object_type,
            fragment_source_mask,
            translated_mask,
            orientation_fragment_rewrite,
            &mut rewritten_bits,
            &mut rewritten_bit_cursor,
        ) else {
            debug_update_record_reject(
                "fragment-bit-rewrite-failed",
                live_bytes,
                record_offset,
                *record_end,
                raw_mask,
                translated_mask,
                *bit_cursor,
            );
            *bit_cursor_reliable = false;
            return None;
        };
        let mut bit_rewrite = bit_rewrite;
        bit_rewrite.bits_inserted = bit_rewrite
            .bits_inserted
            .saturating_add(preinserted_zero_bits);
        Some((rewritten_bits, rewritten_bit_cursor, bit_rewrite))
    } else {
        None
    };

    // The tail9 converter owns the legacy name BOOL and EE's inserted scalar/
    // state bits only; a terminal extra bit has no following record to claim it.
    if let Some((rewritten_bits, rewritten_bit_cursor, _)) = bit_rewrite_candidate.as_ref() {
        if *record_end == live_bytes.len()
            && tail_ready
            && matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
            && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
            && (raw_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK)) != 0
            && *rewritten_bit_cursor != rewritten_bits.len()
        {
            debug_update_record_reject(
                "terminal-door-placeable-tail9-residual-fragment-bits",
                live_bytes,
                record_offset,
                *record_end,
                raw_mask,
                translated_mask,
                *bit_cursor,
            );
            return None;
        }
        if *record_end == live_bytes.len()
            && inline_name_drop_begin.is_some()
            && matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
            && (raw_mask & LEGACY_UPDATE_NAME_MASK) != 0
            && (translated_mask & LEGACY_UPDATE_NAME_MASK) == 0
            && *rewritten_bit_cursor != rewritten_bits.len()
        {
            debug_update_record_reject(
                "terminal-door-placeable-inline-name-residual-fragment-bits",
                live_bytes,
                record_offset,
                *record_end,
                raw_mask,
                translated_mask,
                *bit_cursor,
            );
            return None;
        }
    }

    if tail_ready
        && (translated_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK))
            != 0
    {
        if let Some(claim) = compact_tail9_claim {
            let ee_tail =
                writer::build_ee_door_placeable_generic_update_bytes(claim.tail, translated_mask);
            live_bytes.splice(
                claim.tail_offset..claim.tail_offset + 9,
                ee_tail.iter().copied(),
            );
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

    let repaired_stale_absent_appearance_gap = byte_gap_drop_range.is_some();
    if let Some((drop_begin, drop_end)) = byte_gap_drop_range {
        if drop_begin < drop_end && drop_end <= *record_end {
            let removed = drop_end - drop_begin;
            live_bytes.drain(drop_begin..drop_end);
            *record_end = record_end.saturating_sub(removed);
            if let Some(drop_begin_suffix) = inline_name_drop_begin.as_mut() {
                if *drop_begin_suffix >= drop_end {
                    *drop_begin_suffix -= removed;
                } else if *drop_begin_suffix > drop_begin {
                    *drop_begin_suffix = drop_begin;
                }
            }
            rewrite.bytes_removed = rewrite.bytes_removed.saturating_add(removed as u32);
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
        debug_update_record_reject(
            "read-buffer-unclaimed-after-byte-rewrite",
            live_bytes,
            record_offset,
            *record_end,
            raw_mask,
            translated_mask,
            *bit_cursor,
        );
        return None;
    }

    if let Some((mut rewritten_bits, rewritten_bit_cursor, mut bit_rewrite)) = bit_rewrite_candidate
    {
        if legacy_low_tail_fragment_bits_to_remove != 0 {
            if bits::erase_msb_bits(
                &mut rewritten_bits,
                rewritten_bit_cursor,
                legacy_low_tail_fragment_bits_to_remove,
            )
            .is_none()
            {
                debug_update_record_reject(
                    "low-tail-fragment-bit-remove-failed",
                    live_bytes,
                    record_offset,
                    *record_end,
                    raw_mask,
                    translated_mask,
                    *bit_cursor,
                );
                return None;
            }
            bit_rewrite.bits_removed = bit_rewrite.bits_removed.saturating_add(
                u32::try_from(legacy_low_tail_fragment_bits_to_remove).unwrap_or(u32::MAX),
            );
        }
        // The older all-bits tail9 converter has its own named-tail cursor;
        // this terminal residual check is only for direct low-tail suffix repairs.
        if *record_end == live_bytes.len()
            && matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
            && !tail_ready
            && (raw_mask & !translated_mask & LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK) != 0
            && rewritten_bit_cursor != rewritten_bits.len()
        {
            debug_update_record_reject(
                "terminal-door-placeable-low-tail-residual-fragment-bits",
                live_bytes,
                record_offset,
                *record_end,
                raw_mask,
                translated_mask,
                *bit_cursor,
            );
            return None;
        }
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

    if tail_ready
        && let Some(claim) = compact_tail9_claim
        && let Ok(source_bits_consumed) = u32::try_from(
            low_prefix_door_placeable_update_source_fragment_bits(claim.fragment_source_mask),
        )
    {
        rewrite.bit_claim = Some(RecordRewriteBitClaim {
            source_mask: claim.fragment_source_mask,
            translated_mask,
            source_bits_consumed,
            family: "update-compact-tail9-rewrite",
        });
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
    // A state-only door/placeable update owns exactly the five Diamond state
    // BOOLs and EE's inserted neutral sixth BOOL; no terminal reader owns a
    // seventh bit. Broader legacy door/placeable repairs keep the existing
    // top-level trim gate only after their typed byte/bit paths above have
    // proven the record-specific cursor.
    rewrite.terminal_fragment_trim_allowed = rewrite.rewritten
        && matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        && update_bits_present
        // The mask-0x17 stale absent-appearance repair removes bytes that no
        // Diamond/EE reader owns. It may produce an exact stream when the
        // source bits are complete, but that byte-gap proof cannot also own a
        // later terminal fragment bit.
        && !repaired_stale_absent_appearance_gap
        && translated_mask != LEGACY_UPDATE_STATE_MASK;
    let ee_placeable_state_bits = (object_type == PLACEABLE_OBJECT_TYPE)
        .then(|| placeable_update_state_bits_at(bits, original_bit_cursor, translated_mask))
        .flatten();
    trace_placeable_update_area_context(
        area_context,
        object_id,
        raw_mask,
        translated_mask,
        record_offset,
        *record_end,
        source_placeable_state_bits,
        ee_placeable_state_bits,
    );

    tracing::info!(
        object_type,
        object_id = format_args!("0x{object_id:08X}"),
        raw_mask = format_args!("0x{raw_mask:08X}"),
        translated_mask = format_args!("0x{translated_mask:08X}"),
        record_offset,
        record_end = *record_end,
        source_placeable_state = ?source_placeable_state_bits,
        ee_placeable_state = ?ee_placeable_state_bits,
        bits_inserted = rewrite.bits_inserted,
        bits_removed = rewrite.bits_removed,
        bytes_inserted = rewrite.bytes_inserted,
        bytes_removed = rewrite.bytes_removed,
        "server->client live-object update record translated for EE"
    );
    Some(rewrite)
}

fn trace_placeable_update_area_context(
    area_context: Option<&AreaPlaceableContext>,
    object_id: u32,
    raw_mask: u32,
    translated_mask: u32,
    record_offset: usize,
    record_end: usize,
    source_state_bits: Option<PlaceableUpdateStateBits>,
    ee_state_bits: Option<PlaceableUpdateStateBits>,
) {
    let Some(context) = area_context else {
        return;
    };
    let overlap = context.placeable_overlap_by(|row_object_id| {
        object_ids::equivalent_legacy_external_object_ids(row_object_id, object_id)
    });
    if overlap.is_empty() {
        return;
    }

    let area_rows = overlap.formatted_rows();
    let source_area_module_lock_mismatch = source_state_bits.is_some_and(|source_state| {
        overlap.any_static_module_state_conflict(|module_state| {
            placeable_update_lock_state_conflicts_with_area_module_state(source_state, module_state)
        })
    });
    let ee_area_module_lock_mismatch = ee_state_bits.is_some_and(|ee_state| {
        overlap.any_static_module_state_conflict(|module_state| {
            placeable_update_lock_state_conflicts_with_area_module_state(ee_state, module_state)
        })
    });
    let area_light_duplicate = overlap.has_light_row();
    let area_static_duplicate = overlap.has_static_row();

    tracing::info!(
        object_id = format_args!("0x{object_id:08X}"),
        area_resref = context.area_resref.as_str(),
        raw_mask = format_args!("0x{raw_mask:08X}"),
        translated_mask = format_args!("0x{translated_mask:08X}"),
        record_offset,
        record_end,
        area_placeable_overlap = true,
        area_light_duplicate,
        area_static_duplicate,
        source_placeable_state = ?source_state_bits,
        ee_placeable_state = ?ee_state_bits,
        source_area_module_lock_mismatch,
        ee_area_module_lock_mismatch,
        area_rows = %area_rows,
        "server->client live-object placeable update overlaps area/static context"
    );
}

fn placeable_update_lock_state_conflicts_with_area_module_state(
    state: PlaceableUpdateStateBits,
    module_state: AreaPlaceableContextState,
) -> bool {
    state.lockable != module_state.lockable || state.locked != module_state.locked
}

fn parse_compact_door_placeable_tail9_update_claim(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    object_type: u8,
    raw_mask: u32,
    translated_mask: u32,
) -> Option<CompactDoorPlaceableTail9UpdateClaim> {
    if !matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
        || (raw_mask & LEGACY_UPDATE_NAME_MASK) == 0
    {
        return None;
    }

    let tail_offset = door_placeable_update_name_cursor(record_offset, raw_mask);
    if tail_offset > record_end || record_end.saturating_sub(tail_offset) < 9 {
        return None;
    }

    let tail = reader::read_legacy_named_update_tail9(bytes, tail_offset, false)?;
    let following_payload_ready =
        reader::legacy_named_update_tail_following_payload_ready(bytes, tail_offset, record_end);
    let raw_has_legacy_generic_tail =
        (raw_mask & (LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_SCALE_STATE_MASK)) != 0;
    if !following_payload_ready && !raw_has_legacy_generic_tail {
        return None;
    }

    // The tail9 cursor does not contain EE's mask-0x20 appearance WORD. Diamond
    // and EE both read appearance before scale/state in the shared generic
    // reader; this legacy tail starts with facing, then scale/state, so carrying
    // 0x20 into the EE mask would shift the scale cursor while still landing on
    // the same byte length.
    let mut claim_translated_mask = translated_mask & !LEGACY_UPDATE_APPEARANCE_MASK;
    let mut fragment_source_mask = raw_mask;
    let mut orientation_rewrite = OrientationFragmentRewrite::PreserveExisting;
    if (raw_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        claim_translated_mask |= LEGACY_UPDATE_ORIENTATION_MASK;
        let orientation_scalar12 =
            writer::encode_ee_scalar_orientation_from_legacy_facing(tail.facing);
        orientation_rewrite = OrientationFragmentRewrite::InsertScalar(orientation_scalar12);
        fragment_source_mask &= !LEGACY_UPDATE_ORIENTATION_MASK;
    }
    if (raw_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        claim_translated_mask |= LEGACY_UPDATE_SCALE_STATE_MASK;
    }

    Some(CompactDoorPlaceableTail9UpdateClaim {
        tail_offset,
        tail,
        following_payload_ready,
        translated_mask: claim_translated_mask,
        fragment_source_mask,
        orientation_rewrite,
    })
}

fn placeable_update_state_bits_at(
    bits: &[bool],
    bit_cursor: usize,
    mask: u32,
) -> Option<PlaceableUpdateStateBits> {
    if (mask & LEGACY_UPDATE_STATE_MASK) == 0 {
        return None;
    }

    let mut cursor = bit_cursor;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        cursor = cursor.checked_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS)?;
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let vector_orientation = bits.get(cursor).copied()?;
        cursor = cursor.checked_add(if vector_orientation {
            EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS
        } else {
            EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
        })?;
    }

    Some(PlaceableUpdateStateBits {
        visual_selector: bits.get(cursor).copied()?,
        visual_state_active: bits.get(cursor + 1).copied()?,
        locked: bits.get(cursor + 2).copied()?,
        lockable: bits.get(cursor + 3).copied()?,
        visual_payload: bits.get(cursor + 4).copied()?,
    })
}

fn placeable_update_source_state_bits_at(
    bits: &[bool],
    bit_cursor: usize,
    source_mask: u32,
    orientation_rewrite: OrientationFragmentRewrite,
) -> Option<PlaceableUpdateStateBits> {
    if (source_mask & LEGACY_UPDATE_STATE_MASK) == 0 {
        return None;
    }

    let mut cursor = bit_cursor;
    if (source_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        cursor = cursor.checked_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS)?;
    }
    if (source_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        match orientation_rewrite {
            OrientationFragmentRewrite::InsertLegacyByteScalarPad
            | OrientationFragmentRewrite::InsertScalar(_) => {
                // The byte reader already proved the scalar orientation branch
                // and the bridge inserts EE scalar bits. The source fragment
                // cursor is already at the following state block.
            }
            OrientationFragmentRewrite::PreserveExisting => {
                let vector_orientation = bits.get(cursor).copied()?;
                cursor = cursor.checked_add(if vector_orientation {
                    EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS
                } else {
                    EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                })?;
            }
            OrientationFragmentRewrite::ForceScalar => {
                let _stale_selector = bits.get(cursor).copied()?;
                cursor = cursor.checked_add(EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS)?;
            }
            OrientationFragmentRewrite::ForceVector => {
                let _stale_selector = bits.get(cursor).copied()?;
                cursor = cursor.checked_add(EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS)?;
            }
        }
    }

    Some(PlaceableUpdateStateBits {
        visual_selector: bits.get(cursor).copied()?,
        visual_state_active: bits.get(cursor + 1).copied()?,
        locked: bits.get(cursor + 2).copied()?,
        lockable: bits.get(cursor + 3).copied()?,
        visual_payload: bits.get(cursor + 4).copied()?,
    })
}

fn debug_update_record_reject(
    reason: &'static str,
    live_bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    raw_mask: u32,
    translated_mask: u32,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object update record rewrite rejected: reason={reason} offset={record_offset} record_end={record_end} bit_cursor={bit_cursor} raw_mask=0x{raw_mask:08X} translated_mask=0x{translated_mask:08X} preview={:02X?}",
        live_bytes
            .get(record_offset..record_end.min(record_offset.saturating_add(64)))
            .unwrap_or(&[])
    );
}

pub(super) fn advance_verified_update_record_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if effects::is_verified_ee_looping_visual_effect_update_record(live_bytes, offset, record_end) {
        return true;
    }

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

    if let Some(next_bit_cursor) = item::advance_verified_ee_item_update_record(
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
        ITEM_OBJECT_TYPE => item::translate_update_mask(raw_mask),
        TRIGGER_OBJECT_TYPE => raw_mask & LEGACY_UPDATE_POSITION_MASK,
        _ => raw_mask,
    }
}

fn is_bridge_empty_state_update_mask(mask: u32) -> bool {
    let ee_supported_all = LEGACY_UPDATE_POSITION_MASK
        | LEGACY_UPDATE_ORIENTATION_MASK
        | LEGACY_UPDATE_SCALE_STATE_MASK
        | LEGACY_UPDATE_STATE_MASK
        | LEGACY_UPDATE_APPEARANCE_MASK;
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

fn door_placeable_update_read_end_for_orientation_branch(
    bytes: &[u8],
    record_start: usize,
    record_end: usize,
    mask: u32,
    vector_orientation: bool,
) -> Option<usize> {
    let mut cursor = record_start.checked_add(LEGACY_UPDATE_HEADER_BYTES)?;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        cursor = cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        cursor = cursor.checked_add(if vector_orientation {
            EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES
        } else {
            EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES
        })?;
    }
    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let appearance_word = read_u16_le(bytes, cursor)?;
        cursor = cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
        if appearance_word >= 0xFFFE {
            cursor = cursor.checked_add(super::EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
        }
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        cursor = cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    }
    (cursor <= record_end).then_some(cursor)
}

fn verified_door_placeable_update_read_end_for_current_orientation_branch(
    bytes: &[u8],
    record_start: usize,
    record_end: usize,
    mask: u32,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let orientation_bit_cursor =
        bit_cursor.checked_add(if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
            LEGACY_UPDATE_POSITION_FRAGMENT_BITS
        } else {
            0
        })?;
    let vector_orientation = if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        fragment_bits.get(orientation_bit_cursor).copied()?
    } else {
        false
    };
    verified_door_placeable_update_read_end_for_orientation_branch(
        bytes,
        record_start,
        record_end,
        mask,
        vector_orientation,
    )
}

fn verified_door_placeable_update_read_end_for_orientation_branch(
    bytes: &[u8],
    record_start: usize,
    record_end: usize,
    mask: u32,
    vector_orientation: bool,
) -> Option<usize> {
    let mut cursor = record_start.checked_add(LEGACY_UPDATE_HEADER_BYTES)?;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        cursor = cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
    }
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        cursor = cursor.checked_add(if vector_orientation {
            EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES
        } else {
            EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES
        })?;
    }
    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        let appearance_word = read_u16_le(bytes, cursor)?;
        cursor = cursor.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
        if appearance_word >= 0xFFFE {
            cursor = cursor.checked_add(super::EE_UPDATE_APPEARANCE_RESREF_READ_BYTES)?;
        }
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        let scale = read_f32_le(bytes, cursor)?;
        if !reader::is_plausible_legacy_object_scale(scale) {
            return None;
        }
        cursor = cursor.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    }
    (cursor <= record_end).then_some(cursor)
}

fn stale_absent_appearance_gap_for_scalar_update(
    bytes: &[u8],
    record_start: usize,
    record_end: usize,
    mask_without_appearance: u32,
) -> Option<(usize, usize, usize)> {
    if (mask_without_appearance & LEGACY_UPDATE_SCALE_STATE_MASK) == 0 {
        return None;
    }
    if verified_door_placeable_update_read_end_for_orientation_branch(
        bytes,
        record_start,
        record_end,
        mask_without_appearance,
        false,
    )
    .is_some()
    {
        return None;
    }

    let scalar_prefix_mask =
        mask_without_appearance & (LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_ORIENTATION_MASK);
    let gap_begin = door_placeable_update_read_end_for_orientation_branch(
        bytes,
        record_start,
        record_end,
        scalar_prefix_mask,
        false,
    )?;
    let gap_end = gap_begin.checked_add(EE_UPDATE_APPEARANCE_WORD_READ_BYTES)?;
    if gap_end > record_end {
        return None;
    }

    let scale = read_f32_le(bytes, gap_end)?;
    if !reader::is_plausible_legacy_object_scale(scale) {
        return None;
    }
    let cursor = gap_end.checked_add(EE_UPDATE_SCALE_STATE_READ_BYTES)?;
    if cursor > record_end {
        return None;
    }
    if (mask_without_appearance & LEGACY_UPDATE_STATE_MASK) != 0 {
        // Generic state lives in fragment BOOLs; no read-buffer movement.
    }
    Some((gap_begin, gap_end, cursor))
}

fn door_placeable_update_read_end_for_current_orientation_branch(
    bytes: &[u8],
    record_start: usize,
    record_end: usize,
    mask: u32,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let orientation_bit_cursor =
        bit_cursor.checked_add(if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
            LEGACY_UPDATE_POSITION_FRAGMENT_BITS
        } else {
            0
        })?;
    let vector_orientation = if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        fragment_bits.get(orientation_bit_cursor).copied()?
    } else {
        false
    };
    door_placeable_update_read_end_for_orientation_branch(
        bytes,
        record_start,
        record_end,
        mask,
        vector_orientation,
    )
}

pub(super) fn door_placeable_legacy_inline_name_cursor(record_start: usize, mask: u32) -> usize {
    let mut cursor = door_placeable_update_name_cursor(record_start, mask)
        + if (mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
            EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES
        } else {
            0
        };
    if (mask & LEGACY_UPDATE_APPEARANCE_MASK) != 0 {
        cursor += EE_UPDATE_APPEARANCE_WORD_READ_BYTES;
    }
    if (mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        cursor += EE_UPDATE_SCALE_STATE_READ_BYTES;
    }
    cursor
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
                    | LEGACY_UPDATE_APPEARANCE_MASK
                    | LEGACY_UPDATE_STATE_MASK
                    | LEGACY_UPDATE_NAME_MASK))
                != 0)
}

fn low_prefix_door_placeable_update_source_fragment_bits(source_mask: u32) -> usize {
    let mut bits = 0usize;
    if (source_mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        bits = bits.saturating_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS);
    }
    if (source_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        bits = bits.saturating_add(EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS);
    }
    if (source_mask & LEGACY_UPDATE_STATE_MASK) != 0 {
        bits = bits.saturating_add(LEGACY_UPDATE_STATE_FRAGMENT_BITS);
    }
    if (source_mask & LEGACY_UPDATE_NAME_MASK) != 0 {
        bits = bits.saturating_add(1);
    }
    bits
}

fn suffix_is_fragment_neutral_work_remaining_only(bytes: &[u8], mut offset: usize) -> bool {
    if offset >= bytes.len() {
        return false;
    }

    while offset < bytes.len() {
        if !world_status::is_work_remaining_record_at(bytes, offset) {
            return false;
        }
        offset = offset.saturating_add(3);
    }
    true
}

fn door_placeable_update_interleaved_fragment_storage_suffix_ready(
    bytes: &[u8],
    prefix_end: usize,
    record_end: usize,
) -> bool {
    if prefix_end >= record_end || record_end > bytes.len() {
        return false;
    }
    let span = &bytes[prefix_end..record_end];
    if span.len() > MAX_DOOR_PLACEABLE_UPDATE_INTERLEAVED_FRAGMENT_STORAGE_BYTES
        || reader::legacy_name_tail_ready(bytes, prefix_end, record_end)
        || reader::legacy_low_bit_control_tail_ready(bytes, prefix_end, record_end)
    {
        return false;
    }
    bits::decode_msb_valid_bits(span, CNW_FRAGMENT_HEADER_BITS)
        .is_some_and(|decoded| decoded.len() > CNW_FRAGMENT_HEADER_BITS)
}

fn first_msb_bits(bytes: &[u8], bit_count: usize) -> Option<Vec<bool>> {
    if bit_count == 0 || bytes.len().saturating_mul(8) < bit_count {
        return None;
    }
    let mut out = Vec::with_capacity(bit_count);
    for bit_index in 0..bit_count {
        let byte = *bytes.get(bit_index / 8)?;
        out.push((byte & (0x80 >> (bit_index % 8))) != 0);
    }
    Some(out)
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
    let source_placeable_state_before = if object_type == PLACEABLE_OBJECT_TYPE
        && (source_mask & LEGACY_UPDATE_STATE_MASK) != 0
        && (translated_mask & LEGACY_UPDATE_STATE_MASK) != 0
    {
        Some(placeable_update_source_state_bits_at(
            bits,
            *bit_cursor,
            source_mask,
            orientation_rewrite,
        )?)
    } else {
        None
    };

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
                if !source_has_orientation || bits.len().saturating_sub(cursor) < 1 {
                    return None;
                }
                cursor += if bits[cursor] {
                    EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS
                } else {
                    EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                };
            }
            OrientationFragmentRewrite::ForceScalar => {
                if !source_has_orientation
                    || bits.len().saturating_sub(cursor)
                        < EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                {
                    return None;
                }
                let selector = bits.get_mut(cursor)?;
                rewrite.bits_changed |= *selector;
                *selector = false;
                cursor += EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS;
            }
            OrientationFragmentRewrite::ForceVector => {
                if !source_has_orientation
                    || bits.len().saturating_sub(cursor)
                        < EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS
                {
                    return None;
                }
                let selector = bits.get_mut(cursor)?;
                rewrite.bits_changed |= !*selector;
                *selector = true;
                cursor += EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS;
            }
            OrientationFragmentRewrite::InsertLegacyByteScalarPad => {
                if !source_has_orientation {
                    return None;
                }
                let inserted_orientation_bits = [false, false, false, false, false];
                bits::insert_msb_bits(bits, cursor, &inserted_orientation_bits)?;
                cursor += inserted_orientation_bits.len();
                rewrite.bits_inserted = rewrite
                    .bits_inserted
                    .saturating_add(inserted_orientation_bits.len() as u32);
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
        if bits.len().saturating_sub(cursor) < 1 {
            return None;
        }
        let orientation_bits = if bits[cursor] {
            EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS
        } else {
            EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
        };
        bits::erase_msb_bits(bits, cursor, orientation_bits)?;
        rewrite.bits_removed = rewrite.bits_removed.saturating_add(orientation_bits as u32);
    }

    let source_has_state = (source_mask & LEGACY_UPDATE_STATE_MASK) != 0;
    let translated_has_state = (translated_mask & LEGACY_UPDATE_STATE_MASK) != 0;
    match (source_has_state, translated_has_state) {
        (true, true) => {
            if bits.len().saturating_sub(cursor) < LEGACY_UPDATE_STATE_FRAGMENT_BITS {
                return None;
            }
            cursor += LEGACY_UPDATE_STATE_FRAGMENT_BITS;
            if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
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

    if let Some(source_state) = source_placeable_state_before {
        let emitted_state = placeable_update_state_bits_at(bits, *bit_cursor, translated_mask)?;
        if source_state != emitted_state {
            return None;
        }
    }

    *bit_cursor = cursor;
    Some(rewrite)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(
        visual_selector: bool,
        visual_state_active: bool,
        locked: bool,
        lockable: bool,
        visual_payload: bool,
    ) -> PlaceableUpdateStateBits {
        PlaceableUpdateStateBits {
            visual_selector,
            visual_state_active,
            locked,
            lockable,
            visual_payload,
        }
    }

    #[test]
    fn placeable_update_area_context_helpers_keep_identity_and_lock_proof() {
        let row = AreaPlaceableContextRow {
            object_id: 0x8000_0042,
            appearance: 0x0052,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            dir_x: 0.0,
            dir_y: 1.0,
            dir_z: 0.0,
            has_direction: true,
            object_id_confidence:
                crate::translate::area::AreaPlaceableContextObjectIdConfidence::AreaObjectAlias,
            module_state: Some(AreaPlaceableContextState {
                static_object: true,
                useable: true,
                trap_flag: false,
                trap_disarmable: false,
                lockable: true,
                locked: false,
            }),
        };

        assert_eq!(
            format_area_placeable_context_row(AreaPlaceableContextRowKind::Static, &row),
            "static:id=area-alias;app=0x0052@10.00,20.00,0.00;dir=0.00,1.00,0.00;state=static=true useable=true trap=false disarmable=false lockable=true locked=false"
        );
        assert!(
            placeable_update_lock_state_conflicts_with_area_module_state(
                state(false, true, true, true, true),
                row.module_state.expect("test row has module state"),
            ),
            "locked=true conflicts with the proven module static row"
        );
        assert!(
            !placeable_update_lock_state_conflicts_with_area_module_state(
                state(false, true, false, true, true),
                row.module_state.expect("test row has module state"),
            ),
            "matching lockable/locked bits should not report a module mismatch"
        );
    }

    fn compact_tail9_update_bytes(raw_mask: u32) -> Vec<u8> {
        let mut live = vec![b'U', DOOR_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_0004u32.to_le_bytes());
        live.extend_from_slice(&raw_mask.to_le_bytes());
        live.extend_from_slice(&[0xD0, 0x07, 0xF4, 0x01, 0x0F, 0x0F]);
        live.extend_from_slice(&0x2EA8u16.to_le_bytes());
        live.push(0x02);
        live.extend_from_slice(&1.0f32.to_le_bytes());
        live.extend_from_slice(&0x0016u16.to_le_bytes());
        live.extend_from_slice(&0x0000_14E5u32.to_le_bytes());
        live
    }

    #[test]
    fn compact_tail9_claim_separates_capture_tail_from_stock_orientation_cursor() {
        let raw_mask = 0xFFFF_FFF7;
        let live = compact_tail9_update_bytes(raw_mask);
        let translated_mask = translate_legacy_live_object_update_mask(DOOR_OBJECT_TYPE, raw_mask);

        let claim = parse_compact_door_placeable_tail9_update_claim(
            &live,
            0,
            live.len(),
            DOOR_OBJECT_TYPE,
            raw_mask,
            translated_mask,
        )
        .expect("bounded compact tail9 update should claim");

        assert_eq!(
            claim.tail_offset,
            LEGACY_UPDATE_HEADER_BYTES + LEGACY_UPDATE_POSITION_READ_BYTES
        );
        assert_eq!(claim.tail.facing, 0x2EA8);
        assert_eq!(claim.tail.generic_state_word, 0x0016);
        assert!(!claim.following_payload_ready);
        assert!(claim.tail_needs_empty_name());
        assert_eq!(
            claim.translated_mask,
            LEGACY_UPDATE_POSITION_MASK
                | LEGACY_UPDATE_ORIENTATION_MASK
                | LEGACY_UPDATE_SCALE_STATE_MASK
                | LEGACY_UPDATE_STATE_MASK,
            "tail9 must drop EE's appearance/name bits before emission"
        );
        assert_eq!(
            claim.fragment_source_mask & LEGACY_UPDATE_ORIENTATION_MASK,
            0,
            "tail9 source bits start at position/state/name; stock orientation BOOLs are inserted"
        );
        assert_ne!(
            claim.fragment_source_mask & LEGACY_UPDATE_NAME_MASK,
            0,
            "the legacy name BOOL is input-only and removed by the bit rewrite"
        );
        match claim.orientation_rewrite {
            OrientationFragmentRewrite::InsertScalar(scalar12) => assert_eq!(
                scalar12,
                writer::encode_ee_scalar_orientation_from_legacy_facing(0x2EA8)
            ),
            other => panic!("expected inserted scalar orientation, got {other:?}"),
        }
    }

    #[test]
    fn compact_tail9_claim_rejects_name_only_suffix_without_generic_tail_owner() {
        let raw_mask = LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_NAME_MASK;
        let live = compact_tail9_update_bytes(raw_mask);
        let translated_mask = translate_legacy_live_object_update_mask(DOOR_OBJECT_TYPE, raw_mask);

        assert!(
            parse_compact_door_placeable_tail9_update_claim(
                &live,
                0,
                live.len(),
                DOOR_OBJECT_TYPE,
                raw_mask,
                translated_mask,
            )
            .is_none(),
            "invalid legacy suffix bytes are not enough without orientation/scale tail ownership"
        );
    }

    #[test]
    fn compact_tail9_rewrite_reports_source_bit_claim() {
        let raw_mask = 0xFFFF_FFF7;
        let mut live = compact_tail9_update_bytes(raw_mask);
        let mut record_end = live.len();
        let mut bits = vec![
            true, false, // position fragment bits.
            false, true, false, false, true, // compact source state block.
            true, // legacy input-only name bit removed by the tail9 rewrite.
        ];
        let mut bit_cursor = 0usize;
        let mut bit_cursor_reliable = true;

        let rewrite = rewrite_update_record_for_ee(
            &mut live,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            &mut bit_cursor_reliable,
            0,
        )
        .expect("compact tail9 row should rewrite through its typed claim");
        let claim = rewrite
            .bit_claim
            .expect("compact tail9 rewrite must report source ownership");

        assert!(bit_cursor_reliable);
        assert!(rewrite.rewritten);
        assert_eq!(claim.family, "update-compact-tail9-rewrite");
        assert_eq!(
            claim.source_mask & LEGACY_UPDATE_ORIENTATION_MASK,
            0,
            "tail9 source bits exclude stock orientation selector/residuals"
        );
        assert_eq!(
            claim.source_bits_consumed,
            (LEGACY_UPDATE_POSITION_FRAGMENT_BITS + LEGACY_UPDATE_STATE_FRAGMENT_BITS + 1) as u32
        );
        assert_eq!(claim.translated_mask, read_u32_le(&live, 6).unwrap());
        assert_eq!(
            bit_cursor, 13,
            "EE emission owns position, inserted scalar orientation, widened state, and no name bit"
        );
        assert_eq!(
            usize::try_from(rewrite.bits_inserted).unwrap()
                - usize::try_from(rewrite.bits_removed).unwrap()
                + usize::try_from(claim.source_bits_consumed).unwrap(),
            bit_cursor,
            "post-mutation bit counts must match the parser's source claim"
        );
    }

    #[test]
    fn source_state_diagnostic_keeps_compact_state_cursor_when_scalar_bits_are_inserted() {
        let source_mask =
            LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_STATE_MASK;
        let bits = vec![
            true, false, // position fragment
            false, true, false, false, false, // compact legacy state block
            true, false, false, false, false, // following low-tail/control bits
        ];

        assert_eq!(
            placeable_update_source_state_bits_at(
                &bits,
                0,
                source_mask,
                OrientationFragmentRewrite::InsertLegacyByteScalarPad,
            ),
            Some(state(false, true, false, false, false))
        );
        assert_eq!(
            placeable_update_state_bits_at(&bits, 0, source_mask),
            Some(state(true, false, false, false, false))
        );
    }

    #[test]
    fn source_state_diagnostic_consumes_preserved_scalar_orientation() {
        let source_mask =
            LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_STATE_MASK;
        let bits = vec![
            false, true, // position fragment
            false, true, false, true, false, // scalar orientation fragment
            true, false, true, false, true, // state block
        ];

        assert_eq!(
            placeable_update_source_state_bits_at(
                &bits,
                0,
                source_mask,
                OrientationFragmentRewrite::PreserveExisting,
            ),
            Some(state(true, false, true, false, true))
        );
    }

    #[test]
    fn source_state_diagnostic_uses_forced_scalar_width_when_selector_is_stale() {
        let source_mask =
            LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_STATE_MASK;
        let bits = vec![
            true, false, // position fragment
            true, false, true, false, true, // stale vector selector plus scalar-width payload
            false, true, true, false, true, // state block
        ];

        assert_eq!(
            placeable_update_source_state_bits_at(
                &bits,
                0,
                source_mask,
                OrientationFragmentRewrite::ForceScalar,
            ),
            Some(state(false, true, true, false, true))
        );
        assert_eq!(
            placeable_update_source_state_bits_at(
                &bits,
                0,
                source_mask,
                OrientationFragmentRewrite::PreserveExisting,
            ),
            Some(state(false, true, false, true, false)),
            "a stale vector selector must not drive source-state diagnostics"
        );
    }

    #[test]
    fn source_state_diagnostic_uses_forced_vector_width_when_selector_is_stale() {
        let source_mask =
            LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_STATE_MASK;
        let bits = vec![
            false, true,  // position fragment
            false, // stale scalar selector
            true, false, true, false, true, // state block
            false, false, true, true, false, // following bits that stale scalar would misread
        ];

        assert_eq!(
            placeable_update_source_state_bits_at(
                &bits,
                0,
                source_mask,
                OrientationFragmentRewrite::ForceVector,
            ),
            Some(state(true, false, true, false, true))
        );
        assert_eq!(
            placeable_update_source_state_bits_at(
                &bits,
                0,
                source_mask,
                OrientationFragmentRewrite::PreserveExisting,
            ),
            Some(state(true, false, false, true, true)),
            "a stale scalar selector would read vector payload bits as state"
        );
    }

    #[test]
    fn placeable_update_bit_rewrite_preserves_state_across_orientation_repairs() {
        let mask =
            LEGACY_UPDATE_POSITION_MASK | LEGACY_UPDATE_ORIENTATION_MASK | LEGACY_UPDATE_STATE_MASK;
        for (label, orientation_rewrite, bits, expected_cursor, expected_inserted) in [
            (
                "insert scalar pad",
                OrientationFragmentRewrite::InsertLegacyByteScalarPad,
                vec![
                    true, false, // position fragment
                    false, true, true, false, true, // state block
                ],
                LEGACY_UPDATE_POSITION_FRAGMENT_BITS
                    + EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                    + LEGACY_UPDATE_STATE_FRAGMENT_BITS
                    + 1,
                EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS as u32 + 1,
            ),
            (
                "force scalar",
                OrientationFragmentRewrite::ForceScalar,
                vec![
                    true, false, // position fragment
                    true, false, true, false, true, // stale selector plus scalar payload
                    false, true, true, false, true, // state block
                ],
                LEGACY_UPDATE_POSITION_FRAGMENT_BITS
                    + EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
                    + LEGACY_UPDATE_STATE_FRAGMENT_BITS
                    + 1,
                1,
            ),
            (
                "force vector",
                OrientationFragmentRewrite::ForceVector,
                vec![
                    false, true,  // position fragment
                    false, // stale scalar selector
                    true, false, true, false, true, // state block
                ],
                LEGACY_UPDATE_POSITION_FRAGMENT_BITS
                    + EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS
                    + LEGACY_UPDATE_STATE_FRAGMENT_BITS
                    + 1,
                1,
            ),
        ] {
            let expected_state =
                placeable_update_source_state_bits_at(&bits, 0, mask, orientation_rewrite)
                    .expect("source state should decode before rewrite");
            let mut rewritten_bits = bits;
            let mut cursor = 0usize;

            let rewrite = rewrite_legacy_live_object_update_bits(
                PLACEABLE_OBJECT_TYPE,
                mask,
                mask,
                orientation_rewrite,
                &mut rewritten_bits,
                &mut cursor,
            )
            .expect("placeable state-preserving bit rewrite should succeed");

            assert_eq!(cursor, expected_cursor, "{label}");
            assert_eq!(rewrite.bits_inserted, expected_inserted, "{label}");
            assert_eq!(
                placeable_update_state_bits_at(&rewritten_bits, 0, mask),
                Some(expected_state),
                "{label}: emitted EE state bits must match the source cursor"
            );
        }
    }

    fn scalar_item_update_with_full_legacy_mask() -> Vec<u8> {
        let mut live = vec![b'U', ITEM_OBJECT_TYPE];
        live.extend_from_slice(&0x8000_00B8u32.to_le_bytes());
        live.extend_from_slice(&0xFFFF_FFF3u32.to_le_bytes());
        live.extend_from_slice(&[0xB7, 0x05, 0xC1, 0x04, 0x0F, 0x0F]);
        live.push(0); // scalar-orientation read byte.
        live.extend_from_slice(&0xFFFFu16.to_le_bytes());
        live.extend_from_slice(&[0; 16]);
        live.extend_from_slice(&5u32.to_le_bytes());
        live.extend_from_slice(b"Lance");
        live
    }

    #[test]
    fn failed_item_update_marks_following_fragment_cursor_unreliable() {
        // The CEP v2.3 handoff risk is a shared cursor rule, not an item-only
        // special case. Diamond `sub_467AE0` and EE `sub_14079C050` branch on
        // the item orientation BOOL before reading orientation bytes. If that
        // selector says vector but the bounded `U/6` bytes are scalar-shaped,
        // no later live-object row may reuse the old cursor as if the item
        // update had consumed zero bits.
        let mut live = scalar_item_update_with_full_legacy_mask();
        let original_live = live.clone();
        let mut record_end = live.len();
        let mut bits = vec![
            false, true, // position residual bits.
            true, // vector orientation selector, despite scalar-shaped bytes.
            true, false, true, false, true,  // state bits at the shifted cursor.
            false, // direct CExoString item name if the scalar cursor were valid.
            true,  // hidden BOOL if the scalar cursor were valid.
        ];
        let original_bits = bits.clone();
        let mut bit_cursor = 0usize;
        let mut bit_cursor_reliable = true;

        assert!(
            rewrite_update_record_for_ee(
                &mut live,
                &mut record_end,
                &mut bits,
                &mut bit_cursor,
                &mut bit_cursor_reliable,
                0,
            )
            .is_none(),
            "a vector-selected scalar item update has no decompile-owned cursor"
        );
        assert!(
            !bit_cursor_reliable,
            "later records must not be rewritten against the stale pre-item cursor"
        );
        assert_eq!(bit_cursor, 0);
        assert_eq!(record_end, original_live.len());
        assert_eq!(live, original_live);
        assert_eq!(bits, original_bits);
    }

    #[test]
    fn successful_item_update_rewrite_reports_accepted_claim() {
        let mut live = scalar_item_update_with_full_legacy_mask();
        let mut record_end = live.len();
        let mut bits = vec![
            true, true, // position residual bits.
            false, true, false, true, true, // scalar orientation selector/residuals.
            false, false, false, false, false, // state bits.
            false, // direct CExoString item name.
            true,  // following stream bit, not owned by the raw Diamond full mask.
        ];
        let mut bit_cursor = 0usize;
        let mut bit_cursor_reliable = true;

        let rewrite = rewrite_update_record_for_ee(
            &mut live,
            &mut record_end,
            &mut bits,
            &mut bit_cursor,
            &mut bit_cursor_reliable,
            0,
        )
        .expect("decompile-shaped full item update should rewrite through the item claim");
        let claim = rewrite
            .item_update_claim
            .expect("item update rewrites must report the accepted cursor claim");

        assert!(bit_cursor_reliable);
        assert!(rewrite.rewritten);
        assert!(rewrite.mask_changed);
        assert_eq!(claim.raw_mask, 0xFFFF_FFF3);
        assert_eq!(
            claim.translated_mask,
            item::translate_update_mask(0xFFFF_FFF3)
        );
        assert_eq!(claim.cursor.read_end, record_end);
        assert_eq!(claim.cursor.next_bit_cursor, bit_cursor);
        assert_eq!(claim.cursor.orientation_vector, Some(false));
        assert_eq!(read_u32_le(&live, 6), Some(claim.translated_mask));
        assert_eq!(
            bit_cursor,
            bits.len() - 1,
            "the following stream bit remains outside the item U/6 claim"
        );
    }
}
