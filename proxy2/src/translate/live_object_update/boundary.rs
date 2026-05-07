//! Legacy live-object record boundary detection.
//!
//! This module owns only stream-shape classification. It does not translate
//! records and it does not mutate read bytes or fragment bits.

use super::{
    gui, inventory, item, locstring, read_u32_le, DOOR_OBJECT_TYPE,
    MAX_COMPACT_LEGACY_LIVE_OBJECT_ID, MIN_COMPACT_LEGACY_LIVE_OBJECT_ID, PLACEABLE_OBJECT_TYPE,
};

pub(super) fn find_next_legacy_live_object_sub_message_boundary_after(
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
            gui::try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, scan_end)
        {
            return record_end;
        }
    }

    let start = scan_end.min(offset + minimum_legacy_live_object_record_length_at(bytes, offset));
    let inventory_record = bytes.get(offset).copied() == Some(b'I');
    let mut suppress_inline_string_boundaries = bytes.get(offset).copied() != Some(b'I');
    if bytes.len().saturating_sub(offset) >= 10
        && bytes[offset] == b'U'
        && bytes[offset + 1] == 0x05
        && looks_like_legacy_live_object_id_at(bytes, offset + 2)
    {
        if let Some(raw_mask) = read_u32_le(bytes, offset + 6) {
            if matches!(raw_mask, 0x0000_0008 | 0x0000_0047 | 0x0000_8000) {
                // Decompile/capture-backed creature `U/5` numeric update shapes
                // contain compact status/movement fields, not CExoString names.
                // Mirroring the mature bridge, do not hide candidate live-object
                // boundaries merely because bytes inside these numeric fields
                // look like a one-byte inline string length plus an opcode.
                suppress_inline_string_boundaries = false;
            }
        }
    }
    let string_scan_start = (offset + 2).min(scan_end);
    for candidate in start..scan_end.saturating_sub(1) {
        if suppress_inline_string_boundaries
            && locstring::candidate_inside_inline_string(bytes, string_scan_start, candidate)
        {
            continue;
        }
        if looks_like_legacy_live_object_sub_message_boundary(bytes, candidate) {
            if inventory_record
                && inventory::try_get_legacy_live_inventory_fragment_bit_count(
                    bytes, offset, candidate,
                )
                .is_none()
            {
                continue;
            }
            return candidate;
        }
    }
    if inventory_record {
        // Decompile/capture-backed inventory discipline: `I` records contain
        // opcode-like row bytes. If no candidate boundary validates as a full
        // inventory record, keep the remaining read buffer together so the
        // semantic inventory translator can either claim the whole record or
        // quarantine it. This mirrors the mature C++ bridge rule documented in
        // `packet-alignment-reference.md`: never split inventory merely because
        // an interior byte looks like `U`, `D`, or another live-object opcode.
        return scan_end;
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
            if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(5).saturating_sub(offset);
            }
            11
        }
        (b'A', DOOR_OBJECT_TYPE) => {
            let Some(first_dword) = read_u32_le(bytes, offset + 6) else {
                return 16;
            };
            let name_offset = offset + 2 + if first_dword == 0 { 12 } else { 8 };
            if let Some(inline_end) = locstring::inline_cexo_string_end(bytes, name_offset) {
                return inline_end.saturating_add(2).saturating_sub(offset);
            }
            16
        }
        (b'U' | b'P', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 10,
        (b'D', 0x05 | 0x06 | 0x07 | 0x09 | 0x0A) => 6,
        (b'G', b'Q') => 3,
        (b'I', _) => 7,
        (b'W', marker) if marker <= 0x0F && bytes.get(offset + 2) == Some(&0x0E) => 3,
        _ => 2,
    }
}

pub(super) fn looks_like_legacy_live_object_sub_message_boundary(
    bytes: &[u8],
    offset: usize,
) -> bool {
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

    let legacy_item_sentinel = item::is_legacy_item_sentinel(bytes, offset);
    if opcode == b'I'
        && (item::is_known_legacy_item_marker(marker)
            || legacy_item_sentinel
            || looks_like_legacy_live_object_id_at(bytes, offset + 1))
    {
        return true;
    }

    if opcode == b'G' && gui::looks_like_legacy_live_gui_sub_message_boundary(bytes, offset) {
        return true;
    }

    opcode == b'W' && bytes.len() - offset >= 3 && marker <= 0x0F && bytes[offset + 2] == 0x0E
}

pub(super) fn looks_like_legacy_live_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(looks_like_legacy_live_object_id_value)
        .unwrap_or(false)
}

pub(super) fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }

    let high_byte = object_id & 0xFF00_0000;
    matches!(
        high_byte,
        // EE's decompile treats object ids as opaque DWORDs. These high-byte
        // filters are scanner guards, not engine rules. HG live-object door
        // and placeable captures use 0x08xxxxxx and 0x35xxxxxx ids, so accept
        // those namespaces explicitly while still rejecting arbitrary shifted
        // ASCII bytes.
        0x8000_0000
            | 0x8800_0000
            | 0xFF00_0000
            | 0x0100_0000
            | 0x0500_0000
            | 0x0800_0000
            | 0x3500_0000
    ) || (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID)
        .contains(&object_id)
}
