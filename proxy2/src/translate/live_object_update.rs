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
mod bits;
mod boundary;
mod creature;
mod cursor;
mod door;
mod gui;
mod inventory;
mod item;
mod locstring;
mod placeable;
mod reader;
mod record;
mod writer;
mod world_status;
#[cfg(test)]
mod tests;

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
    pub fragment_bits_trimmed: u32,
    pub world_status_records_normalized: u32,
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
    pub read_buffer_only_records: u32,
    pub add_records: u32,
    pub creature_update_records: u32,
    pub delete_records: u32,
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
    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            return None;
        }

        let record_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        )
        .min(live_bytes.len());
        if record_end <= offset {
            return None;
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
            summary.inventory_fragment_bits = summary.inventory_fragment_bits.saturating_add(
                u32::try_from(inventory_claim.fragment_bits).unwrap_or(u32::MAX),
            );
            offset = record_end;
            continue;
        }
        if gui::is_verified_live_gui_read_buffer_record(live_bytes, offset, record_end) {
            summary.live_gui_read_buffer_records =
                summary.live_gui_read_buffer_records.saturating_add(1);
            offset = record_end;
            continue;
        }
        if is_verified_read_buffer_only_record(live_bytes, offset, record_end) {
            summary.read_buffer_only_records =
                summary.read_buffer_only_records.saturating_add(1);
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
            summary.creature_update_records =
                summary.creature_update_records.saturating_add(1);
            offset = record_end;
            continue;
        }
        if creature::advance_verified_noop_creature_update_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.creature_update_records =
                summary.creature_update_records.saturating_add(1);
            offset = record_end;
            continue;
        }
        if let Some(delete_bits) =
            cursor::legacy_live_delete_fragment_bit_count(live_bytes, offset, record_end)
        {
            if delete_bits > fragment_bits.len().saturating_sub(bit_cursor) {
                return None;
            }
            bit_cursor = bit_cursor.saturating_add(delete_bits);
            summary.delete_records = summary.delete_records.saturating_add(1);
            offset = record_end;
            continue;
        }

        return None;
    }

    if offset != live_bytes.len() || summary.records_examined == 0 {
        return None;
    }

    Some(summary)
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
    let mut offset = 0usize;
    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(&live_bytes, offset) {
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
            if bit_cursor_reliable
                && !cursor::advance_live_add_record_bit_cursor(
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

        if opcode == b'D' {
            if bit_cursor_reliable {
                if let Some(delete_bits) =
                    cursor::legacy_live_delete_fragment_bit_count(&live_bytes, offset, record_end)
                {
                    if fragment_bits.len().saturating_sub(bit_cursor) >= delete_bits {
                        bit_cursor += delete_bits;
                    } else {
                        bit_cursor_reliable = false;
                    }
                } else {
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if inventory::owns_fragment_tail(opcode) {
            // Inventory and GUI item-create live submessages can own CNW
            // fragment BOOLs. Until this focused update translator has exact
            // parsers for those families, do not trim the shared fragment tail
            // after seeing them; preserving bytes is safer than pretending the
            // cursor proof is complete.
            bit_cursor_reliable = false;
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
        return cursor::legacy_live_delete_fragment_bit_count(bytes, offset, record_end)
            == Some(0);
    }

    false
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
