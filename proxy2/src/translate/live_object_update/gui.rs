//! Live-object `G` GUI submessage classifiers.
//!
//! The live-object dispatcher is strict: even byte-identical Diamond/EE shapes
//! must be claimed by a semantic module before they are allowed onward. This
//! module handles read-buffer-only GUI rows such as `G Q` as identity
//! translations because the EE and Diamond live dispatchers both route
//! top-level `G` into GUI handling, and the decompile-backed C++ bridge
//! verified that `G Q` is a read-buffer-only record:
//!
//! ```text
//! G Q <count:u8> <count * 9 byte rows>
//! ```
//!
//! Each row carries small mode bytes plus an object id at row offset `+2`.
//! There are no object-update fragment BOOLs to consume here, so a verified
//! record is safe to preserve unchanged.
//!
//! GUI inventory/repository item-add rows (`G I/i A` and `G R/r A`) are not
//! read-buffer-only. EE's GUI handler reads the GUI prefix and then calls the
//! normal item-create helper, so these rows delegate their item body to the
//! focused appearance/item-create parser. Unrecognized `G` submessages are not
//! accepted by this module; they remain quarantined until a focused translator
//! is added.

use super::{appearance, boundary, read_u32_le};

const LIVE_GUI_OPCODE: u8 = b'G';
const GUI_INVENTORY_SUBOPCODE: u8 = b'I';
const GUI_INVENTORY_LOWER_SUBOPCODE: u8 = b'i';
const GUI_REPOSITORY_SUBOPCODE: u8 = b'R';
const GUI_REPOSITORY_LOWER_SUBOPCODE: u8 = b'r';
const GUI_ADD_INNER_OPCODE: u8 = b'A';
const GUI_DELETE_INNER_OPCODE: u8 = b'D';
const GUI_UPDATE_INNER_OPCODE: u8 = b'U';
const GUI_MOVE_INNER_OPCODE: u8 = b'M';
const QUICKBAR_ITEM_LINK_SUBOPCODE: u8 = b'Q';
const QUICKBAR_ITEM_LINK_HEADER_BYTES: usize = 3;
const QUICKBAR_ITEM_LINK_ROW_BYTES: usize = 9;
const QUICKBAR_ITEM_LINK_OBJECT_ID_OFFSET_IN_ROW: usize = 2;

#[derive(Debug, Clone, Copy)]
pub(super) struct LiveGuiRecordClaim {
    pub item_create: bool,
    pub fragment_bits: usize,
}

pub(super) fn looks_like_legacy_live_gui_sub_message_boundary(bytes: &[u8], offset: usize) -> bool {
    try_get_legacy_live_gui_record_end(bytes, offset, bytes.len()).is_some()
}

pub(super) fn try_get_legacy_live_gui_record_end(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> Option<usize> {
    try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, search_end)
        .or_else(|| try_get_legacy_live_gui_item_create_record_end(bytes, offset, search_end))
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

pub(super) fn advance_verified_live_gui_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> Option<LiveGuiRecordClaim> {
    if is_verified_live_gui_read_buffer_record(bytes, offset, record_end) {
        return Some(LiveGuiRecordClaim {
            item_create: false,
            fragment_bits: 0,
        });
    }

    let item_object_offset = legacy_live_gui_item_object_offset(bytes, offset, record_end)?;
    let before = *bit_cursor;
    if !appearance::advance_verified_ee_item_create_record(
        bytes,
        item_object_offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return None;
    }
    Some(LiveGuiRecordClaim {
        item_create: true,
        fragment_bits: bit_cursor.saturating_sub(before),
    })
}

pub(super) fn insert_ee_live_gui_item_extras_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<appearance::CreatureAppearanceExtraRewrite> {
    let item_object_offset = legacy_live_gui_item_object_offset(bytes, offset, *record_end)?;
    appearance::insert_ee_item_create_extras_for_ee(
        bytes,
        item_object_offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

fn try_get_legacy_live_gui_read_buffer_record_end(
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
        GUI_INVENTORY_SUBOPCODE | GUI_INVENTORY_LOWER_SUBOPCODE => {
            try_get_inventory_read_buffer_record_end(bytes, offset, scan_end)
        }
        GUI_REPOSITORY_SUBOPCODE | GUI_REPOSITORY_LOWER_SUBOPCODE => {
            try_get_repository_read_buffer_record_end(bytes, offset, scan_end)
        }
        _ => None,
    }
}

fn try_get_legacy_live_gui_item_create_record_end(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> Option<usize> {
    let scan_end = search_end.min(bytes.len());
    let item_object_offset = legacy_live_gui_item_object_offset(bytes, offset, scan_end)?;
    appearance::try_get_legacy_item_create_record_end(bytes, item_object_offset, scan_end)
}

fn legacy_live_gui_item_object_offset(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<usize> {
    if record_end > bytes.len()
        || offset.checked_add(3)? > record_end
        || bytes.get(offset).copied() != Some(LIVE_GUI_OPCODE)
    {
        return None;
    }

    let gui_opcode = bytes[offset + 1];
    let inner_opcode = bytes[offset + 2];
    match (gui_opcode, inner_opcode) {
        (GUI_INVENTORY_SUBOPCODE | GUI_INVENTORY_LOWER_SUBOPCODE, GUI_ADD_INNER_OPCODE) => {
            let item_object_offset = offset.checked_add(7)?;
            (item_object_offset < record_end).then_some(item_object_offset)
        }
        (GUI_REPOSITORY_SUBOPCODE | GUI_REPOSITORY_LOWER_SUBOPCODE, GUI_ADD_INNER_OPCODE) => {
            let item_object_offset = offset.checked_add(5)?;
            (item_object_offset < record_end).then_some(item_object_offset)
        }
        _ => None,
    }
}

fn try_get_inventory_read_buffer_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset.checked_add(3)? > scan_end {
        return None;
    }

    match bytes[offset + 2] {
        GUI_DELETE_INNER_OPCODE => fixed_gui_object_row_end(bytes, offset, scan_end, 3, 7),
        GUI_UPDATE_INNER_OPCODE => fixed_gui_object_row_end(bytes, offset, scan_end, 3, 15),
        _ => None,
    }
}

fn try_get_repository_read_buffer_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    if offset.checked_add(3)? > scan_end {
        return None;
    }

    match bytes[offset + 2] {
        GUI_DELETE_INNER_OPCODE => fixed_gui_object_row_end(bytes, offset, scan_end, 3, 7),
        GUI_UPDATE_INNER_OPCODE => fixed_gui_object_row_end(bytes, offset, scan_end, 3, 15),
        GUI_MOVE_INNER_OPCODE => fixed_gui_object_row_end(bytes, offset, scan_end, 5, 9),
        _ => None,
    }
}

fn fixed_gui_object_row_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    object_id_relative_offset: usize,
    row_len: usize,
) -> Option<usize> {
    let record_end = offset.checked_add(row_len)?;
    if record_end > scan_end
        || !looks_like_legacy_live_gui_object_id_at(bytes, offset + object_id_relative_offset)
    {
        return None;
    }
    Some(record_end)
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
