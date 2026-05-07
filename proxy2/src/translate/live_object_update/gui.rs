//! Live-object `G` GUI submessage classifiers.
//!
//! The live-object dispatcher is strict: even byte-identical Diamond/EE shapes
//! must be claimed by a semantic module before they are allowed onward. This
//! module handles the `G Q` quickbar/item-link list shape as an identity
//! translation because the EE and Diamond live dispatchers both route top-level
//! `G` into GUI handling, and the decompile-backed C++ bridge verified that
//! `G Q` is a read-buffer-only record:
//!
//! ```text
//! G Q <count:u8> <count * 9 byte rows>
//! ```
//!
//! Each row carries small mode bytes plus an object id at row offset `+2`.
//! There are no object-update fragment BOOLs to consume here, so a verified
//! record is safe to preserve unchanged. Unrecognized `G` submessages are not
//! accepted by this module; they remain quarantined until a focused translator
//! is added.

use super::{boundary, read_u32_le};

const LIVE_GUI_OPCODE: u8 = b'G';
const QUICKBAR_ITEM_LINK_SUBOPCODE: u8 = b'Q';
const QUICKBAR_ITEM_LINK_HEADER_BYTES: usize = 3;
const QUICKBAR_ITEM_LINK_ROW_BYTES: usize = 9;
const QUICKBAR_ITEM_LINK_OBJECT_ID_OFFSET_IN_ROW: usize = 2;

pub(super) fn looks_like_legacy_live_gui_sub_message_boundary(
    bytes: &[u8],
    offset: usize,
) -> bool {
    try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, bytes.len()).is_some()
}

pub(super) fn is_verified_live_gui_read_buffer_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, record_end)
        .map(|verified_end| verified_end == record_end)
        .unwrap_or(false)
}

pub(super) fn try_get_legacy_live_gui_read_buffer_record_end(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> Option<usize> {
    let scan_end = search_end.min(bytes.len());
    if offset > scan_end || scan_end - offset < 2 || bytes[offset] != LIVE_GUI_OPCODE {
        return None;
    }

    let gui_opcode = bytes[offset + 1];
    if !is_known_live_gui_subopcode(gui_opcode) {
        return None;
    }

    match gui_opcode {
        QUICKBAR_ITEM_LINK_SUBOPCODE => {
            if scan_end - offset < QUICKBAR_ITEM_LINK_HEADER_BYTES {
                return None;
            }

            let count = usize::from(bytes[offset + 2]);
            let rows_bytes = count.checked_mul(QUICKBAR_ITEM_LINK_ROW_BYTES)?;
            let record_end = offset
                .checked_add(QUICKBAR_ITEM_LINK_HEADER_BYTES)?
                .checked_add(rows_bytes)?;
            if record_end > scan_end {
                return None;
            }

            for index in 0..count {
                let row_offset =
                    offset + QUICKBAR_ITEM_LINK_HEADER_BYTES + index * QUICKBAR_ITEM_LINK_ROW_BYTES;
                if !looks_like_legacy_live_gui_object_id_at(
                    bytes,
                    row_offset + QUICKBAR_ITEM_LINK_OBJECT_ID_OFFSET_IN_ROW,
                ) {
                    return None;
                }
            }

            Some(record_end)
        }
        _ => None,
    }
}

fn is_known_live_gui_subopcode(value: u8) -> bool {
    matches!(
        value,
        b'A' | b'B' | b'C' | b'I' | b'M' | b'Q' | b'R' | b'S' | b'c' | b'i' | b'r'
    )
}

fn looks_like_legacy_live_gui_object_id_at(bytes: &[u8], offset: usize) -> bool {
    read_u32_le(bytes, offset)
        .map(|object_id| {
            object_id == 0x7F00_0000
                || object_id == u32::MAX
                || boundary::looks_like_legacy_live_object_id_value(object_id)
        })
        .unwrap_or(false)
}
