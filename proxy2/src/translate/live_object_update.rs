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

mod add;
mod appearance;
mod bits;
mod boundary;
mod class_rows;
mod creature;
mod creature_add;
mod cursor;
mod door;
mod fragment_spans;
mod gui;
mod inventory;
mod item;
mod locstring;
mod placeable;
mod reader;
mod record;
mod tail_repair;
#[cfg(test)]
mod tests;
mod trigger;
mod world_status;
mod writer;

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const LIVE_OBJECT_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;

const CREATURE_OBJECT_TYPE: u8 = 0x05;
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
    pub fragment_bits_trimmed: u32,
    pub world_status_records_normalized: u32,
    pub creature_visual_transform_update_records: u32,
    pub interleaved_fragment_spans_promoted: u32,
    pub interleaved_fragment_bytes_promoted: u32,
    pub interleaved_fragment_bits_promoted: u32,
}

#[derive(Debug, Clone, Default)]
pub struct LiveObjectUpdateClaimSummary {
    pub declared: usize,
    pub live_bytes_length: usize,
    pub fragment_bytes: usize,
    pub records_examined: u32,
    pub inventory_records: u32,
    pub inventory_fragment_bits: u32,
    pub live_gui_read_buffer_records: u32,
    pub live_gui_item_create_records: u32,
    pub live_gui_fragment_bits: u32,
    pub read_buffer_only_records: u32,
    pub add_records: u32,
    pub update_records: u32,
    pub creature_appearance_records: u32,
    pub creature_visual_transform_update_records: u32,
    pub creature_update_records: u32,
    pub delete_records: u32,
}

#[derive(Debug, Clone, Default)]
pub struct LiveObjectAddNameBitRewriteSummary {
    pub records_examined: u32,
    pub add_records_repaired: u32,
    pub bits_removed: u32,
    pub old_fragment_bytes: u32,
    pub new_fragment_bytes: u32,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<LiveObjectUpdateClaimSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let declared = usize::try_from(read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }
    let fragment = &payload[declared..];
    let fragment_bits = bits::decode_msb_valid_bits(fragment, CNW_FRAGMENT_HEADER_BITS)?;

    let live_bytes = &payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared];
    if live_bytes.is_empty() {
        return None;
    }

    let mut summary = LiveObjectUpdateClaimSummary {
        declared,
        live_bytes_length: live_bytes.len(),
        fragment_bytes: fragment.len(),
        ..LiveObjectUpdateClaimSummary::default()
    };
    let mut offset = 0usize;
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut pending_creature_appearance_update_proof: Option<(u32, usize)> = None;
    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            trace_claim_reject(
                "missing-live-object-submessage-boundary",
                live_bytes,
                offset,
                offset.saturating_add(2).min(live_bytes.len()),
                bit_cursor,
            );
            return None;
        }

        let mut record_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        )
        .min(live_bytes.len());
        if live_bytes.get(offset).copied() == Some(b'P')
            && live_bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
        {
            if let Some(verified_end) = appearance::try_get_verified_ee_creature_appearance_record_end(
                live_bytes,
                offset,
                live_bytes.len(),
                &fragment_bits,
                bit_cursor,
            ) {
                record_end = verified_end;
            }
        }
        if record_end <= offset {
            trace_claim_reject(
                "non-advancing-live-object-boundary",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            return None;
        }
        if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
            eprintln!(
                "live-object claim boundary: offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} preview={:02X?}",
                live_bytes.get(offset).copied().unwrap_or_default(),
                live_bytes.get(offset + 1).copied().unwrap_or_default(),
                live_bytes
                    .get(
                        offset
                            ..record_end
                                .min(offset.saturating_add(24))
                                .min(live_bytes.len())
                    )
                    .unwrap_or(&[])
            );
        }

        summary.records_examined = summary.records_examined.saturating_add(1);
        if let Some(inventory_claim) = inventory::advance_verified_inventory_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.inventory_records = summary.inventory_records.saturating_add(1);
            summary.inventory_fragment_bits = summary
                .inventory_fragment_bits
                .saturating_add(u32::try_from(inventory_claim.fragment_bits).unwrap_or(u32::MAX));
            trace_claim_accept("inventory", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        if let Some(gui_claim) = gui::advance_verified_live_gui_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            if gui_claim.item_create {
                summary.live_gui_item_create_records =
                    summary.live_gui_item_create_records.saturating_add(1);
            } else {
                summary.live_gui_read_buffer_records =
                    summary.live_gui_read_buffer_records.saturating_add(1);
            }
            summary.live_gui_fragment_bits = summary
                .live_gui_fragment_bits
                .saturating_add(u32::try_from(gui_claim.fragment_bits).unwrap_or(u32::MAX));
            trace_claim_accept("live-gui", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        if is_verified_read_buffer_only_record(live_bytes, offset, record_end) {
            summary.read_buffer_only_records = summary.read_buffer_only_records.saturating_add(1);
            trace_claim_accept(
                "read-buffer-only",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if add::advance_verified_add_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.add_records = summary.add_records.saturating_add(1);
            trace_claim_accept("add", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        let record_probe = record::advance_verified_update_record_for_ee(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        );
        if record_probe {
            summary.update_records = summary.update_records.saturating_add(1);
            trace_claim_accept("update", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        if appearance::advance_verified_ee_creature_appearance_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.creature_appearance_records =
                summary.creature_appearance_records.saturating_add(1);
            pending_creature_appearance_update_proof =
                live_object_record_object_id(live_bytes, offset, record_end)
                    .map(|object_id| (object_id, bit_cursor));
            trace_claim_accept(
                "creature-appearance",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if creature::advance_verified_noop_creature_appearance_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.creature_update_records = summary.creature_update_records.saturating_add(1);
            pending_creature_appearance_update_proof =
                live_object_record_object_id(live_bytes, offset, record_end)
                    .map(|object_id| (object_id, bit_cursor));
            trace_claim_accept(
                "noop-creature-appearance",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if appearance::is_verified_ee_creature_visual_transform_update_record(
            live_bytes, offset, record_end,
        ) {
            summary.creature_visual_transform_update_records = summary
                .creature_visual_transform_update_records
                .saturating_add(1);
            trace_claim_accept(
                "creature-visual-transform",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        let creature_probe = creature::advance_verified_noop_creature_update_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        );
        if creature_probe {
            summary.creature_update_records = summary.creature_update_records.saturating_add(1);
            if let (Some((pending_object_id, _)), Some(update_object_id)) = (
                pending_creature_appearance_update_proof,
                live_object_record_object_id(live_bytes, offset, record_end),
            ) {
                if pending_object_id == update_object_id {
                    pending_creature_appearance_update_proof = None;
                }
            }
            trace_claim_accept(
                "creature-update",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if let (Some((pending_object_id, pending_bit_cursor)), Some(update_object_id)) = (
            pending_creature_appearance_update_proof,
            live_object_record_object_id(live_bytes, offset, record_end),
        ) {
            if pending_object_id == update_object_id
                && (creature::legacy_3967_update_was_already_consumed_from_cursor(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    pending_bit_cursor,
                    bit_cursor,
                ) || creature::legacy_3967_update_was_already_consumed_to_cursor(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    bit_cursor,
                ))
            {
                pending_creature_appearance_update_proof = None;
                summary.creature_update_records =
                    summary.creature_update_records.saturating_add(1);
                trace_claim_accept(
                    "creature-update-already-consumed",
                    live_bytes,
                    offset,
                    record_end,
                    bit_cursor,
                );
                offset = record_end;
                continue;
            }
        }
        if let Some(delete_bits) =
            cursor::legacy_live_delete_fragment_bit_count(live_bytes, offset, record_end)
        {
            if delete_bits > fragment_bits.len().saturating_sub(bit_cursor) {
                return None;
            }
            bit_cursor = bit_cursor.saturating_add(delete_bits);
            summary.delete_records = summary.delete_records.saturating_add(1);
            trace_claim_accept("delete", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }

        trace_claim_reject(
            "no-exact-record-validator-accepted-boundary",
            live_bytes,
            offset,
            record_end,
            bit_cursor,
        );
        return None;
    }

    if offset != live_bytes.len() || summary.records_examined == 0 {
        trace_claim_reject(
            "live-object-final-cursor-mismatch",
            live_bytes,
            offset.min(live_bytes.len()),
            live_bytes.len(),
            bit_cursor,
        );
        return None;
    }

    if bit_cursor != fragment_bits.len() {
        trace_claim_reject(
            "live-object-fragment-cursor-mismatch",
            live_bytes,
            live_bytes.len(),
            live_bytes.len(),
            bit_cursor,
        );
        return None;
    }

    Some(summary)
}

pub fn rewrite_add_name_fragment_bits_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectAddNameBitRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let declared = usize::try_from(read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }

    let live_bytes = &payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared];
    let mut fragment_bits =
        bits::decode_msb_valid_bits(&payload[declared..], CNW_FRAGMENT_HEADER_BITS)?;
    let old_fragment_bytes = payload.len().saturating_sub(declared);
    let mut summary = LiveObjectAddNameBitRewriteSummary {
        old_fragment_bytes: u32::try_from(old_fragment_bytes).unwrap_or(u32::MAX),
        ..LiveObjectAddNameBitRewriteSummary::default()
    };
    let mut offset = 0usize;
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;

    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            trace_add_name_rewrite_reject(
                "missing-live-object-submessage-boundary",
                live_bytes,
                offset,
                offset.saturating_add(2).min(live_bytes.len()),
                bit_cursor,
                &fragment_bits,
            );
            return None;
        }
        let record_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        )
        .min(live_bytes.len());
        let record_end = if live_bytes.get(offset).copied() == Some(b'P')
            && live_bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
        {
            appearance::try_get_verified_ee_creature_appearance_record_end(
                live_bytes,
                offset,
                live_bytes.len(),
                &fragment_bits,
                bit_cursor,
            )
            .unwrap_or(record_end)
        } else {
            record_end
        };
        if record_end <= offset {
            trace_add_name_rewrite_reject(
                "non-advancing-live-object-boundary",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
                &fragment_bits,
            );
            return None;
        }
        summary.records_examined = summary.records_examined.saturating_add(1);

        if let Some(_inventory_claim) = inventory::advance_verified_inventory_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            offset = record_end;
            continue;
        }
        if let Some(_gui_claim) = gui::advance_verified_live_gui_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            offset = record_end;
            continue;
        }
        if is_verified_read_buffer_only_record(live_bytes, offset, record_end) {
            offset = record_end;
            continue;
        }

        match (live_bytes[offset], live_bytes[offset + 1]) {
            (b'A', DOOR_OBJECT_TYPE) => {
                if repair_inline_door_add_name_bit(
                    live_bytes,
                    offset,
                    record_end,
                    &mut fragment_bits,
                    &mut bit_cursor,
                    &mut summary,
                )
                .is_none()
                {
                    trace_add_name_rewrite_reject(
                        "door-add-name-bit-repair-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'A', PLACEABLE_OBJECT_TYPE) => {
                if repair_inline_placeable_add_name_bit(
                    live_bytes,
                    offset,
                    record_end,
                    &mut fragment_bits,
                    &mut bit_cursor,
                    &mut summary,
                )
                .is_none()
                {
                    trace_add_name_rewrite_reject(
                        "placeable-add-name-bit-repair-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'A', _) => {
                if !add::advance_verified_add_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    trace_add_name_rewrite_reject(
                        "add-record-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'U', PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) => {
                if !record::advance_verified_update_record_for_ee(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    if summary.add_records_repaired != 0 {
                        // This helper owns add-record name-mode bit repair, not
                        // door/placeable update-mask translation. Once a proven
                        // add repair has been made, stop before a still-legacy
                        // update record and let the following update-family pass
                        // consume/translate it with its own exact validator.
                        break;
                    }
                    trace_add_name_rewrite_reject(
                        "update-record-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'P', CREATURE_OBJECT_TYPE) => {
                if !appearance::advance_verified_ee_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) && !creature::advance_verified_noop_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    if summary.add_records_repaired != 0 {
                        // This helper owns only add-record name-mode bit repair. Once a
                        // repair has been proven, do not discard it because a later
                        // unrelated family needs the full live-object validator. The
                        // caller still immediately runs the exact final claim; if this
                        // P/5 record is not translated/validated, the packet remains
                        // quarantined rather than leaking.
                        break;
                    }
                    trace_add_name_rewrite_reject(
                        "creature-appearance-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'U', CREATURE_OBJECT_TYPE) => {
                if appearance::advance_verified_ee_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) || creature::advance_verified_noop_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) || appearance::is_verified_ee_creature_visual_transform_update_record(
                    live_bytes, offset, record_end,
                ) || creature::advance_verified_noop_creature_update_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    // This pass only owns add-record name-mode bit repair. Creature records
                    // are accepted here only after the same exact validators used by the
                    // final live-object claim prove their cursor shape.
                } else {
                    if summary.add_records_repaired != 0 {
                        // See the P/5 case above: once this add-name-only helper has
                        // made a proven repair, later unrelated families are validated
                        // by the mandatory final live-object claim instead of causing
                        // the repair to be discarded.
                        break;
                    }
                    trace_add_name_rewrite_reject(
                        "creature-update-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'W', marker) if marker <= 0x0F && live_bytes.get(offset + 2) == Some(&0x0E) => {}
            (b'D', object_type) => {
                let delete_bits =
                    cursor::legacy_live_delete_fragment_bit_count(live_bytes, offset, record_end)?;
                if !matches!(object_type, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A)
                    || fragment_bits.len().saturating_sub(bit_cursor) < delete_bits
                {
                    trace_add_name_rewrite_reject(
                        "delete-record-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
                bit_cursor = bit_cursor.saturating_add(delete_bits);
            }
            _ => {
                trace_add_name_rewrite_reject(
                    "unsupported-live-object-record",
                    live_bytes,
                    offset,
                    record_end,
                    bit_cursor,
                    &fragment_bits,
                );
                return None;
            }
        }
        offset = record_end;
    }

    if summary.add_records_repaired == 0 {
        trace_add_name_rewrite_reject(
            "no-add-name-bits-needed-repair",
            live_bytes,
            live_bytes.len(),
            live_bytes.len(),
            bit_cursor,
            &fragment_bits,
        );
        return None;
    }

    let fragment_bytes = bits::pack_msb_valid_bits(fragment_bits, CNW_FRAGMENT_HEADER_BITS);
    summary.new_fragment_bytes = u32::try_from(fragment_bytes.len()).unwrap_or(u32::MAX);
    payload.truncate(declared);
    payload.extend_from_slice(&fragment_bytes);
    Some(summary)
}

fn trace_add_name_rewrite_reject(
    reason: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
    fragment_bits: &[bool],
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    let preview_limit = if reason == "missing-live-object-submessage-boundary" {
        offset.saturating_add(40)
    } else {
        record_end
    };
    let preview_end = preview_limit
        .min(live_bytes.len())
        .min(offset.saturating_add(40));
    let preview = live_bytes.get(offset..preview_end).unwrap_or(&[]);
    eprintln!(
        "live-object add-name repair rejected: reason={reason} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} next_bits={:?} preview={:02X?}",
        live_bytes.get(offset).copied().unwrap_or_default(),
        live_bytes.get(offset + 1).copied().unwrap_or_default(),
        fragment_bits
            .get(bit_cursor..bit_cursor.saturating_add(12).min(fragment_bits.len()))
            .unwrap_or(&[]),
        preview
    );
}

fn repair_inline_door_add_name_bit(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    summary: &mut LiveObjectAddNameBitRewriteSummary,
) -> Option<()> {
    let first_dword = read_u32_le(live_bytes, offset + 6)?;
    let visual_offset = offset + 2 + if first_dword == 0 { 12 } else { 8 };
    if !creature::has_ee_identity_visual_transform_map_at(live_bytes, visual_offset, record_end) {
        return None;
    }
    let name_offset = visual_offset + 40;
    let inline_end = locstring::inline_cexo_string_end(live_bytes, name_offset)?;
    if inline_end > name_offset + CNW_LENGTH_BYTES
        && fragment_bits.get(*bit_cursor).copied().unwrap_or(false)
    {
        if fragment_bits.len().saturating_sub(*bit_cursor + 1) < 1 {
            return None;
        }
        if fragment_bits[*bit_cursor + 1] {
            fragment_bits[*bit_cursor + 1] = false;
            summary.add_records_repaired = summary.add_records_repaired.saturating_add(1);
        }
        *bit_cursor = bit_cursor.saturating_add(7);
    } else {
        if fragment_bits.get(*bit_cursor).copied().unwrap_or(true) {
            return None;
        }
        *bit_cursor = bit_cursor.saturating_add(6);
    }
    Some(())
}

fn repair_inline_placeable_add_name_bit(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    summary: &mut LiveObjectAddNameBitRewriteSummary,
) -> Option<()> {
    let name_offset = offset + 6;
    let inline_end = locstring::inline_cexo_string_end(live_bytes, name_offset)?;
    let tail_end = inline_end.checked_add(1 + 2 + 2)?;
    if tail_end > record_end
        || !creature::has_ee_identity_visual_transform_map_at(live_bytes, tail_end, record_end)
    {
        return None;
    }
    if inline_end > name_offset + CNW_LENGTH_BYTES
        && fragment_bits.get(*bit_cursor).copied().unwrap_or(false)
    {
        if fragment_bits.len().saturating_sub(*bit_cursor + 1) < 1 {
            return None;
        }
        if fragment_bits[*bit_cursor + 1] {
            fragment_bits[*bit_cursor] = false;
            summary.add_records_repaired = summary.add_records_repaired.saturating_add(1);
            *bit_cursor = bit_cursor.saturating_add(11);
        } else {
            *bit_cursor = bit_cursor.saturating_add(12);
        }
    } else {
        if inline_end > name_offset + CNW_LENGTH_BYTES
            && fragment_bits.get(*bit_cursor).copied().unwrap_or(true)
        {
            return None;
        }
        *bit_cursor = bit_cursor.saturating_add(11);
    }
    Some(())
}

pub(crate) fn advance_verified_door_placeable_update_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    record::advance_verified_update_record_for_ee(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn advance_verified_inventory_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    inventory::advance_verified_inventory_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
    .is_some()
}

pub(crate) fn advance_verified_creature_appearance_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let original_bit_cursor = *bit_cursor;
    if appearance::advance_verified_ee_creature_appearance_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return true;
    }

    *bit_cursor = original_bit_cursor;
    creature::advance_verified_noop_creature_appearance_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn try_get_verified_creature_appearance_record_end_for_ee(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    appearance::try_get_verified_ee_creature_appearance_record_end(
        live_bytes,
        offset,
        scan_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn advance_verified_creature_update_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if appearance::is_verified_ee_creature_visual_transform_update_record(
        live_bytes, offset, record_end,
    ) {
        return true;
    }

    creature::advance_verified_noop_creature_update_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn trigger_add_record_end_for_ee(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    trigger::try_get_trigger_add_record_end(bytes, offset, scan_end)
}

pub(crate) fn trigger_add_min_record_bytes_for_ee() -> usize {
    trigger::TRIGGER_ADD_MIN_RECORD_BYTES
}

fn live_object_record_object_id(live_bytes: &[u8], offset: usize, record_end: usize) -> Option<u32> {
    if offset + 6 > record_end || record_end > live_bytes.len() {
        return None;
    }
    Some(u32::from_le_bytes(
        live_bytes.get(offset + 2..offset + 6)?.try_into().ok()?,
    ))
}

fn trace_claim_reject(
    reason: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    let preview_end = record_end
        .min(live_bytes.len())
        .min(offset.saturating_add(40));
    let preview = live_bytes.get(offset..preview_end).unwrap_or(&[]);
    tracing::trace!(
        reason,
        offset,
        record_end,
        bit_cursor,
        opcode = live_bytes.get(offset).copied().unwrap_or_default(),
        marker = live_bytes.get(offset + 1).copied().unwrap_or_default(),
        preview = ?preview,
        "live-object exact claim rejected"
    );
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object exact claim rejected: reason={reason} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} preview={:02X?}",
            live_bytes.get(offset).copied().unwrap_or_default(),
            live_bytes.get(offset + 1).copied().unwrap_or_default(),
            preview
        );
    }
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
    let mut fragment_bits =
        bits::decode_msb_valid_bits(&payload[declared..], CNW_FRAGMENT_HEADER_BITS)?;
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
    let mut pending_creature_p_tail_repair: Option<
        tail_repair::PendingCreatureAppearanceTailRepair,
    > = None;
    let mut offset = 0usize;
    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(&live_bytes, offset) {
            if summary.records_examined > 0 {
                if let Some(removal) =
                    fragment_spans::remove_trailing_zero_fragment_storage_after_verified_record_for_ee(
                        &mut live_bytes,
                        offset,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted =
                        summary.interleaved_fragment_spans_promoted.saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                        );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                    );
                    continue;
                }
            }
            offset += 1;
            continue;
        }

        summary.records_examined = summary.records_examined.saturating_add(1);
        let opcode = live_bytes[offset];
        let object_type = live_bytes[offset + 1];
        let mut record_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            &live_bytes,
            offset,
            live_bytes.len(),
        )
        .min(live_bytes.len());
        let mut creature_appearance_already_ee_shaped = false;
        let mut creature_appearance_verified_ee_shaped = false;
        if opcode == b'P' && object_type == CREATURE_OBJECT_TYPE {
            if bit_cursor_reliable {
                if let Some(verified_end) = appearance::try_get_verified_ee_creature_appearance_record_end(
                    &live_bytes,
                    offset,
                    live_bytes.len(),
                    &fragment_bits,
                    bit_cursor,
                ) {
                    record_end = verified_end;
                    creature_appearance_already_ee_shaped = true;
                    creature_appearance_verified_ee_shaped = true;
                }
            }
            if !creature_appearance_already_ee_shaped {
                if let Some(byte_shape_end) =
                    appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                    )
                {
                    record_end = byte_shape_end;
                    creature_appearance_already_ee_shaped = true;
                }
            }
        }
        if record_end <= offset {
            offset += 1;
            continue;
        }

        if let Some(removed) =
            world_status::normalize_record_for_ee(&mut live_bytes, offset, &mut record_end)
        {
            if removed != 0 {
                changed = true;
                summary.world_status_records_normalized =
                    summary.world_status_records_normalized.saturating_add(1);
                summary.bytes_removed = summary.bytes_removed.saturating_add(removed as u32);
            }
            offset = record_end;
            continue;
        }

        if opcode == b'A' {
            if bit_cursor_reliable {
                if let Some(add_rewrite) =
                    creature_add::insert_ee_visual_transform_for_legacy_creature_add(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                    )
                {
                    changed = true;
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(add_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                    );
                }

                if let Some(item_rewrite) = appearance::insert_ee_item_add_extras_for_ee(
                    &mut live_bytes,
                    offset,
                    &mut record_end,
                    &mut fragment_bits,
                    bit_cursor,
                ) {
                    if item_rewrite.bits_inserted != 0
                        || item_rewrite.bits_removed != 0
                        || item_rewrite.bytes_inserted != 0
                    {
                        changed = true;
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(item_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bits_removed = summary.bits_removed.saturating_add(
                            u32::try_from(item_rewrite.bits_removed).unwrap_or(u32::MAX),
                        );
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(item_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                        );
                    }
                }

                if !appearance::advance_verified_ee_item_add_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) && !cursor::advance_live_add_record_bit_cursor(
                    &live_bytes,
                    &fragment_bits,
                    offset,
                    record_end,
                    &mut bit_cursor,
                ) && !cursor::advance_legacy_add_record_bit_cursor_for_update_pass(
                    &live_bytes,
                    &fragment_bits,
                    offset,
                    record_end,
                    &mut bit_cursor,
                ) {
                    trace_update_rewrite_cursor_unreliable(
                        "add-record-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'D' {
            if bit_cursor_reliable {
                if let Some(delete_bits) =
                    cursor::legacy_live_delete_fragment_bit_count(&live_bytes, offset, record_end)
                {
                    if fragment_bits.len().saturating_sub(bit_cursor) >= delete_bits {
                        bit_cursor += delete_bits;
                    } else {
                        trace_update_rewrite_cursor_unreliable(
                            "delete-record-fragment-bits-insufficient",
                            &live_bytes,
                            offset,
                            record_end,
                            bit_cursor,
                        );
                        bit_cursor_reliable = false;
                    }
                } else {
                    trace_update_rewrite_cursor_unreliable(
                        "delete-record-bit-count-unknown",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'P' && object_type == 0x05 {
            let original_fragment_bits_for_tail_repair = fragment_bits.clone();
            let mut appearance_bits_inserted_for_tail_repair = 0usize;
            let mut appearance_tail_fragment_bits_adjusted = false;
            let may_attempt_appearance_rewrite = (bit_cursor_reliable
                && !creature_appearance_verified_ee_shaped)
                || (!bit_cursor_reliable && creature_appearance_already_ee_shaped);
            if may_attempt_appearance_rewrite {
                if let Some(appearance_rewrite) =
                    appearance::insert_ee_creature_appearance_extras_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    )
                {
                    appearance_bits_inserted_for_tail_repair = appearance_rewrite.bits_inserted;
                    if appearance_rewrite.bits_inserted != 0
                        || appearance_rewrite.bits_removed != 0
                        || appearance_rewrite.bytes_inserted != 0
                    {
                        changed = true;
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(appearance_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bits_removed = summary.bits_removed.saturating_add(
                            u32::try_from(appearance_rewrite.bits_removed).unwrap_or(u32::MAX),
                        );
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(appearance_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                        );
                    }
                }
            }
            let mut advanced_appearance_cursor = bit_cursor;
            if appearance::advance_verified_ee_creature_appearance_record(
                &live_bytes,
                offset,
                record_end,
                &fragment_bits,
                &mut advanced_appearance_cursor,
            ) {
                bit_cursor = advanced_appearance_cursor;
                bit_cursor_reliable = true;
            } else if bit_cursor_reliable {
                trace_update_rewrite_cursor_unreliable(
                    "creature-appearance-cursor-advance-failed",
                    &live_bytes,
                    offset,
                    record_end,
                    bit_cursor,
                );
                bit_cursor_reliable = false;
            }
            if bit_cursor_reliable {
                if let Some(promotion) =
                    fragment_spans::promote_appearance_following_creature_update_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    appearance_tail_fragment_bits_adjusted = true;
                }
                if let Some(removal) =
                    fragment_spans::remove_trailing_zero_fragment_storage_after_verified_record_for_ee(
                        &mut live_bytes,
                        record_end,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted =
                        summary.interleaved_fragment_spans_promoted.saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                        );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                    );
                }
            }
            if bit_cursor_reliable
                && appearance_bits_inserted_for_tail_repair != 0
                && !appearance_tail_fragment_bits_adjusted
                && bit_cursor >= appearance_bits_inserted_for_tail_repair
            {
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                    eprintln!(
                        "live-object creature-P tail repair armed: offset={offset} bit_cursor={bit_cursor} inserted_bits={appearance_bits_inserted_for_tail_repair}"
                    );
                }
                pending_creature_p_tail_repair =
                    live_object_record_object_id(&live_bytes, offset, record_end).and_then(
                        |object_id| {
                            tail_repair::PendingCreatureAppearanceTailRepair::new(
                                original_fragment_bits_for_tail_repair,
                                bit_cursor - appearance_bits_inserted_for_tail_repair,
                                bit_cursor,
                                appearance_bits_inserted_for_tail_repair,
                                offset,
                                object_id,
                            )
                        },
                    );
            } else if appearance_tail_fragment_bits_adjusted {
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                    eprintln!(
                        "live-object creature-P tail repair cleared: offset={offset} reason=fragment-bits-adjusted bit_cursor={bit_cursor} inserted_bits={appearance_bits_inserted_for_tail_repair}"
                    );
                }
                pending_creature_p_tail_repair = None;
            } else if bit_cursor_reliable && creature_appearance_already_ee_shaped {
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                    eprintln!(
                        "live-object creature-P tail repair armed: offset={offset} bit_cursor={bit_cursor} inserted_bits=0 reason=already-ee-shaped"
                    );
                }
                pending_creature_p_tail_repair =
                    live_object_record_object_id(&live_bytes, offset, record_end).and_then(
                        |object_id| {
                            tail_repair::PendingCreatureAppearanceTailRepair::new(
                                fragment_bits.clone(),
                                bit_cursor,
                                bit_cursor,
                                0,
                                offset,
                                object_id,
                            )
                        },
                    );
            } else if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some()
                && opcode == b'P'
                && object_type == 0x05
            {
                eprintln!(
                    "live-object creature-P tail repair not armed: offset={offset} already_ee={creature_appearance_already_ee_shaped} bit_cursor_reliable={bit_cursor_reliable} bit_cursor={bit_cursor} inserted_bits={appearance_bits_inserted_for_tail_repair}"
                );
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'U' && object_type == CREATURE_OBJECT_TYPE {
            if bit_cursor_reliable {
                if let Some(visual_rewrite) =
                    appearance::rewrite_creature_visual_transform_update_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.creature_visual_transform_update_records = summary
                        .creature_visual_transform_update_records
                        .saturating_add(1);
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(visual_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                    );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(visual_rewrite.bytes_removed).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary.bits_inserted.saturating_add(
                        u32::try_from(visual_rewrite.bits_inserted).unwrap_or(u32::MAX),
                    );
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add((visual_rewrite.bytes_removed != 0) as u32);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(visual_rewrite.bytes_removed).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted =
                        summary.interleaved_fragment_bits_promoted.saturating_add(
                            u32::try_from(visual_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                }
                if appearance::is_verified_ee_creature_visual_transform_update_record(
                    &live_bytes,
                    offset,
                    record_end,
                ) {
                    offset = record_end.max(offset + 1);
                    continue;
                }
                if creature::repair_3967_action2_optional_float_bool_for_ee(
                    &live_bytes,
                    offset,
                    record_end,
                    &mut fragment_bits,
                    bit_cursor,
                ) {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                }
                if let Some(promotion) =
                    fragment_spans::promote_creature_update_interleaved_fragment_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        offset,
                        &mut record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    bit_cursor = promotion.end_bit_cursor;
                    offset = record_end.max(offset + 1);
                    continue;
                }
                let mut advanced_cursor = bit_cursor;
                if creature::advance_verified_noop_creature_update_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut advanced_cursor,
                ) {
                    bit_cursor = advanced_cursor;
                    pending_creature_p_tail_repair = None;
                } else if creature::legacy_3967_update_was_already_consumed_to_cursor(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    bit_cursor,
                ) {
                    pending_creature_p_tail_repair = None;
                } else if let Some(pending) = pending_creature_p_tail_repair.as_ref() {
                    if let Some(repair) = tail_repair::try_repair_for_creature_update(
                        pending,
                        &live_bytes,
                        offset,
                        record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    ) {
                        changed = true;
                        summary.interleaved_fragment_spans_promoted =
                            summary.interleaved_fragment_spans_promoted.saturating_add(1);
                        if repair.new_bits_len >= repair.old_bits_len {
                            summary.bits_inserted = summary.bits_inserted.saturating_add(
                                u32::try_from(repair.new_bits_len - repair.old_bits_len)
                                    .unwrap_or(u32::MAX),
                            );
                        } else {
                            summary.bits_removed = summary.bits_removed.saturating_add(
                                u32::try_from(repair.old_bits_len - repair.new_bits_len)
                                    .unwrap_or(u32::MAX),
                            );
                        }
                        bit_cursor = repair.bit_cursor;
                        pending_creature_p_tail_repair = None;
                    } else {
                        trace_update_rewrite_cursor_unreliable(
                            "creature-update-cursor-advance-failed",
                            &live_bytes,
                            offset,
                            record_end,
                            bit_cursor,
                        );
                        bit_cursor_reliable = false;
                    }
                } else {
                    trace_update_rewrite_cursor_unreliable(
                        "creature-update-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'G' {
            if bit_cursor_reliable {
                if let Some(gui_rewrite) = gui::insert_ee_live_gui_item_extras_for_ee(
                    &mut live_bytes,
                    offset,
                    &mut record_end,
                    &mut fragment_bits,
                    bit_cursor,
                ) {
                    if gui_rewrite.bits_inserted != 0 || gui_rewrite.bytes_inserted != 0 {
                        changed = true;
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(gui_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(gui_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                        );
                    }
                }

                if gui::advance_verified_live_gui_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                )
                .is_none()
                {
                    trace_update_rewrite_cursor_unreliable(
                        "live-gui-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if inventory::owns_fragment_tail(opcode) {
            if bit_cursor_reliable {
                if let Some(promotion) =
                    fragment_spans::promote_inventory_interleaved_fragment_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        offset,
                        &mut record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                }

                if inventory::advance_verified_inventory_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                )
                .is_none()
                {
                    trace_update_rewrite_cursor_unreliable(
                        "inventory-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode != b'U'
            || !matches!(
                object_type,
                TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
            )
            || record_end < offset + LEGACY_UPDATE_HEADER_BYTES
        {
            offset = record_end.max(offset + 1);
            continue;
        }

        summary.update_records_examined = summary.update_records_examined.saturating_add(1);
        let Some(record_rewrite) = record::rewrite_update_record_for_ee(
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
            summary.update_records_rewritten = summary.update_records_rewritten.saturating_add(1);
            if record_rewrite.mask_changed {
                summary.masks_translated = summary.masks_translated.saturating_add(1);
            }
            summary.bytes_inserted = summary
                .bytes_inserted
                .saturating_add(record_rewrite.bytes_inserted);
            summary.bytes_removed = summary
                .bytes_removed
                .saturating_add(record_rewrite.bytes_removed);
            summary.bits_inserted = summary
                .bits_inserted
                .saturating_add(record_rewrite.bits_inserted);
            summary.bits_removed = summary
                .bits_removed
                .saturating_add(record_rewrite.bits_removed);
        }
        offset = record_end.max(offset + 1);
    }

    if !changed {
        return None;
    }

    if bit_cursor_reliable
        && bit_cursor >= CNW_FRAGMENT_HEADER_BITS
        && bit_cursor < fragment_bits.len()
    {
        summary.fragment_bits_trimmed = (fragment_bits.len() - bit_cursor) as u32;
        fragment_bits.truncate(bit_cursor);
    }

    let fragment_bytes = bits::pack_msb_valid_bits(fragment_bits, CNW_FRAGMENT_HEADER_BITS);
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

fn is_verified_read_buffer_only_record(bytes: &[u8], offset: usize, record_end: usize) -> bool {
    if offset >= record_end || record_end > bytes.len() {
        return false;
    }

    if gui::is_verified_live_gui_read_buffer_record(bytes, offset, record_end) {
        return true;
    }

    // `W` world-status records are three read-buffer bytes and fragment
    // neutral. `world_status::normalize_record_for_ee` strips legacy tails; if
    // the record is already exactly the legal length, this claim is a verified
    // no-op.
    if bytes[offset] == b'W'
        && record_end == offset + 3
        && bytes.get(offset + 1).copied().unwrap_or(0xFF) <= 0x0F
        && bytes.get(offset + 2).copied() == Some(0x0E)
    {
        return true;
    }

    if bytes[offset] == b'D' {
        return cursor::legacy_live_delete_fragment_bit_count(bytes, offset, record_end) == Some(0);
    }

    false
}

fn trace_claim_accept(
    family: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object claim accepted: family={family} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X}",
        live_bytes.get(offset).copied().unwrap_or_default(),
        live_bytes.get(offset + 1).copied().unwrap_or_default()
    );
}

fn trace_update_rewrite_cursor_unreliable(
    reason: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    let preview_end = record_end
        .min(live_bytes.len())
        .min(offset.saturating_add(40));
    let preview = live_bytes.get(offset..preview_end).unwrap_or(&[]);
    eprintln!(
        "live-object update rewrite cursor unreliable: reason={reason} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} preview={:02X?}",
        live_bytes.get(offset).copied().unwrap_or_default(),
        live_bytes.get(offset + 1).copied().unwrap_or_default(),
        preview
    );
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
    bytes
        .get_mut(offset..offset + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

/// Dump a live-object payload that was accepted by a semantic ownership probe.
///
/// This is intentionally environment-gated and lives beside the live-object
/// translator rather than as ad-hoc test output.  The strict bridge treats a
/// no-op live-object claim as provisional evidence: captured payloads should be
/// promoted into fixtures and then either assigned to an exact translator or
/// rejected.  Set HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR to enable capture.
pub fn dump_live_object_fixture_candidate(payload: &[u8], reason: &str) {
    let Ok(dir) = std::env::var("HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR") else {
        return;
    };
    if dir.trim().is_empty() {
        return;
    }

    let sanitized_reason: String = reason
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut path = std::path::PathBuf::from(dir);
    if std::fs::create_dir_all(&path).is_err() {
        return;
    }
    path.push(format!(
        "{}-len{}-{}.bin",
        sanitized_reason,
        payload.len(),
        nanos
    ));
    let _ = std::fs::write(path, payload);
}

/// Return true when a live-object payload contains door/placeable add or update
/// records that require a family-specific semantic translator.
///
/// Decompile evidence: EE routes these through dedicated readers
/// (`sub_140796DD0` / `sub_140797780` for doors and `sub_1407A7800` /
/// `sub_1407A8460` for placeables). Even when a payload is otherwise
/// structurally walkable, the generic no-op claimed-record path must not own
/// these records because Diamond/HG streams can omit or compact EE-only visual,
/// orientation, and name-mode fields.
pub fn payload_contains_door_or_placeable_add_update_record(payload: &[u8]) -> bool {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return false;
    }

    let Some(declared) =
        read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES || declared > payload.len() {
        return false;
    }

    let live_bytes = &payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared];
    let mut offset = 0usize;
    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            offset += 1;
            continue;
        }

        let opcode = live_bytes[offset];
        let object_type = live_bytes[offset + 1];
        if matches!(opcode, b'A' | b'U')
            && matches!(object_type, DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE)
        {
            return true;
        }

        let next = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        );
        offset = next.max(offset + 1);
    }

    false
}

#[cfg(test)]
mod claimed_records_guard_tests {
    use super::*;

    #[test]
    fn door_mixed_add_update_payload_must_not_use_generic_claimed_records() {
        let payload = include_bytes!(
            "../../fixtures/live_object/hg_door_mixed_add_update_claimed_records.bin"
        );
        assert!(payload_contains_door_or_placeable_add_update_record(
            payload
        ));
    }

    #[test]
    fn placeable_mixed_add_update_payload_must_not_use_generic_claimed_records() {
        let payload = include_bytes!(
            "../../fixtures/live_object/hg_placeable_mixed_add_update_claimed_records.bin"
        );
        assert!(payload_contains_door_or_placeable_add_update_record(
            payload
        ));
    }
}
