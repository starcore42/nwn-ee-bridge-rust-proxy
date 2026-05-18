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
//! focused appearance/item-create parser. HG short-declared captures can leave
//! CNW fragment-storage bytes after a final GUI item row; those bytes are
//! promoted only after a bounded item-create trial rewrite and exact EE
//! validator both prove the row. Unrecognized `G` submessages are not accepted
//! by this module; they remain quarantined until a focused translator is added.
//!
//! Local Diamond server captures can carry inventory item-add rows as
//! `G I/i 00 <slot/container dword> <item-create body>`. The Diamond client
//! reader at `sub_4589A0` and the EE reader at `sub_1407B3F30` both dispatch
//! only `A`, `D`, and `U` after `G I/i`; the server writer paths around
//! `0x4414E0` likewise emit an explicit `A` before the dword and item body.
//! Treat the zero byte as a legacy missing-inner-opcode capture only when the
//! following item-create object begins at the exact decompile-owned `G I A`
//! cursor (`offset + 7`) and the focused item parser proves the whole row.
//! The translator rewrites that byte to `A`; the exact EE validator below never
//! accepts the zero form.

use super::{appearance, bits, boundary, read_u32_le};

const LIVE_GUI_OPCODE: u8 = b'G';
const GUI_INVENTORY_SUBOPCODE: u8 = b'I';
const GUI_INVENTORY_LOWER_SUBOPCODE: u8 = b'i';
const GUI_REPOSITORY_SUBOPCODE: u8 = b'R';
const GUI_REPOSITORY_LOWER_SUBOPCODE: u8 = b'r';
const GUI_ADD_INNER_OPCODE: u8 = b'A';
const GUI_DELETE_INNER_OPCODE: u8 = b'D';
const GUI_UPDATE_INNER_OPCODE: u8 = b'U';
const GUI_MOVE_INNER_OPCODE: u8 = b'M';
const GUI_LEGACY_MISSING_ADD_INNER_OPCODE: u8 = 0x00;
const QUICKBAR_ITEM_LINK_SUBOPCODE: u8 = b'Q';
const QUICKBAR_ITEM_LINK_HEADER_BYTES: usize = 3;
const QUICKBAR_ITEM_LINK_ROW_BYTES: usize = 9;
const QUICKBAR_ITEM_LINK_OBJECT_ID_OFFSET_IN_ROW: usize = 2;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const MAX_GUI_ITEM_FRAGMENT_SPAN_BYTES: usize = 128;
const MAX_GUI_ZERO_FRAGMENT_STORAGE_BYTES: usize = 8;

/// Extract item-object ids from a GUI live-object record that has already been
/// accepted by `advance_verified_live_gui_record`.
///
/// This is intentionally not a validator. The decompile-backed validators in
/// this module own the exact byte shape for `G Q` quickbar item-link rows and
/// `G I/i A` / `G R/r A` GUI item-create records. This helper only exposes the
/// object ids from that already-proven shape so the session gateway can remember
/// that the client has been told about those item objects before a quickbar
/// packet references them.
pub(super) fn verified_item_materialization_object_ids(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Vec<u32> {
    if record_end <= offset || bytes.get(offset).copied() != Some(LIVE_GUI_OPCODE) {
        return Vec::new();
    }

    match bytes.get(offset + 1).copied() {
        Some(QUICKBAR_ITEM_LINK_SUBOPCODE) => {
            let Some(count) = bytes.get(offset + 2).copied() else {
                return Vec::new();
            };
            let count = usize::from(count);
            let expected_end = offset
                .checked_add(QUICKBAR_ITEM_LINK_HEADER_BYTES)
                .and_then(|start| {
                    start.checked_add(count.checked_mul(QUICKBAR_ITEM_LINK_ROW_BYTES)?)
                });
            if expected_end != Some(record_end) {
                return Vec::new();
            }

            let mut ids = Vec::new();
            let mut cursor = offset + QUICKBAR_ITEM_LINK_HEADER_BYTES;
            for _ in 0..count {
                if let Some(object_id) =
                    read_u32_le(bytes, cursor + QUICKBAR_ITEM_LINK_OBJECT_ID_OFFSET_IN_ROW)
                {
                    if is_materialized_item_object_id(object_id) {
                        ids.push(object_id);
                    }
                }
                cursor += QUICKBAR_ITEM_LINK_ROW_BYTES;
            }
            ids
        }
        Some(GUI_INVENTORY_SUBOPCODE)
        | Some(GUI_INVENTORY_LOWER_SUBOPCODE)
        | Some(GUI_REPOSITORY_SUBOPCODE)
        | Some(GUI_REPOSITORY_LOWER_SUBOPCODE) => {
            let Some(item_object_offset) =
                legacy_live_gui_item_object_offset(bytes, offset, record_end)
            else {
                return Vec::new();
            };
            let Some(object_id) = read_u32_le(bytes, item_object_offset) else {
                return Vec::new();
            };
            if is_materialized_item_object_id(object_id) {
                vec![object_id]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn is_materialized_item_object_id(object_id: u32) -> bool {
    object_id != 0 && object_id != 0x7F00_0000 && object_id != u32::MAX
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LiveGuiRecordClaim {
    pub item_create: bool,
    pub fragment_bits: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LiveGuiItemRewrite {
    pub bits_inserted: usize,
    pub bits_removed: usize,
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
    pub missing_add_inner_opcodes_repaired: usize,
}

#[derive(Debug, Clone, Copy)]
struct LiveGuiItemCreatePrefix {
    item_object_offset: usize,
    missing_add_inner_opcode: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LiveGuiItemFragmentSpanPromotion {
    pub old_record_end: usize,
    pub bytes_promoted: usize,
    pub bits_promoted: usize,
}

pub(super) fn looks_like_legacy_live_gui_sub_message_boundary(bytes: &[u8], offset: usize) -> bool {
    try_get_legacy_live_gui_record_end(bytes, offset, bytes.len()).is_some()
}

pub(super) fn looks_like_legacy_live_gui_rewrite_boundary(bytes: &[u8], offset: usize) -> bool {
    try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, bytes.len()).is_some()
        || legacy_live_gui_item_create_prefix(bytes, offset, bytes.len()).is_some()
}

pub(super) fn is_zero_fragment_storage_span_before_legacy_live_gui_prefix(
    bytes: &[u8],
    span_start: usize,
    span_end: usize,
) -> bool {
    if span_start >= span_end
        || span_end > bytes.len()
        || span_end.saturating_sub(span_start) > MAX_GUI_ZERO_FRAGMENT_STORAGE_BYTES
        || legacy_live_gui_item_create_prefix(bytes, span_end, bytes.len()).is_none()
    {
        return false;
    }
    let Some(decoded_bits) = bits::decode_msb_valid_bits(&bytes[span_start..span_end], 3) else {
        return false;
    };
    decoded_bits.iter().skip(3).all(|bit| !*bit)
}

pub(super) fn try_get_legacy_live_gui_record_end(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> Option<usize> {
    try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, search_end)
        .or_else(|| try_get_legacy_live_gui_item_create_record_end(bytes, offset, search_end))
}

pub(super) fn try_get_legacy_live_gui_item_create_read_end(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> Option<usize> {
    let scan_end = search_end.min(bytes.len());
    let item_object_offset =
        legacy_live_gui_item_create_prefix(bytes, offset, scan_end)?.item_object_offset;
    appearance::try_get_legacy_item_create_record_end(bytes, item_object_offset, scan_end)
}

pub(super) fn try_get_legacy_live_gui_record_end_with_fragment_proof(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, search_end).or_else(|| {
        let scan_end = search_end.min(bytes.len());
        let item_object_offset =
            legacy_live_gui_item_create_prefix(bytes, offset, scan_end)?.item_object_offset;
        appearance::try_get_legacy_gui_item_create_record_end_with_fragment_proof(
            bytes,
            item_object_offset,
            scan_end,
            fragment_bits,
            bit_cursor,
        )
    })
}

pub(super) fn try_get_verified_ee_live_gui_record_end(
    bytes: &[u8],
    offset: usize,
    search_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    try_get_legacy_live_gui_read_buffer_record_end(bytes, offset, search_end).or_else(|| {
        let scan_end = search_end.min(bytes.len());
        let item_object_offset = legacy_live_gui_item_object_offset(bytes, offset, scan_end)?;
        appearance::try_get_verified_ee_gui_item_create_record_end(
            bytes,
            item_object_offset,
            scan_end,
            fragment_bits,
            bit_cursor,
        )
    })
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

pub(super) fn advance_legacy_live_gui_record_for_transport(
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

    let prefix = legacy_live_gui_item_create_prefix(bytes, offset, record_end)?;
    let before = *bit_cursor;
    if !appearance::advance_legacy_gui_item_create_record(
        bytes,
        prefix.item_object_offset,
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
) -> Option<LiveGuiItemRewrite> {
    let prefix = legacy_live_gui_item_create_prefix(bytes, offset, *record_end)?;
    let mut rewrite = LiveGuiItemRewrite::default();
    let appearance_rewrite = appearance::insert_ee_item_create_extras_for_ee(
        bytes,
        prefix.item_object_offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )?;
    rewrite.bits_inserted = appearance_rewrite.bits_inserted;
    rewrite.bits_removed = appearance_rewrite.bits_removed;
    rewrite.bytes_inserted = appearance_rewrite.bytes_inserted;
    rewrite.bytes_removed = appearance_rewrite.bytes_removed;

    if prefix.missing_add_inner_opcode {
        *bytes.get_mut(offset.checked_add(2)?)? = GUI_ADD_INNER_OPCODE;
        rewrite.missing_add_inner_opcodes_repaired = 1;
    }

    (rewrite.bits_inserted != 0
        || rewrite.bits_removed != 0
        || rewrite.bytes_inserted != 0
        || rewrite.bytes_removed != 0
        || rewrite.missing_add_inner_opcodes_repaired != 0)
        .then_some(rewrite)
}

pub(super) fn promote_legacy_live_gui_item_fragment_span_for_ee(
    bytes: &mut Vec<u8>,
    fragment_bits: &mut Vec<bool>,
    offset: usize,
    record_end: &mut usize,
    bit_cursor: usize,
) -> Option<LiveGuiItemFragmentSpanPromotion> {
    let debug = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();
    if offset >= *record_end || *record_end >= bytes.len() {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=range offset={offset} record_end={} len={}",
                *record_end,
                bytes.len()
            );
        }
        return None;
    }
    if legacy_live_gui_item_create_prefix(bytes, offset, *record_end).is_none() {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=prefix offset={offset} record_end={}",
                *record_end
            );
        }
        return None;
    }

    let span_start = *record_end;
    if legacy_live_gui_item_create_prefix(bytes, span_start, bytes.len()).is_some() {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=sibling-gui-prefix span_start={span_start}"
            );
        }
        return None;
    }
    if boundary::looks_like_legacy_live_object_sub_message_boundary(bytes, span_start) {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=following-live-boundary span_start={span_start}"
            );
        }
        return None;
    }
    let span_end = match find_legacy_live_gui_item_fragment_span_end(bytes, span_start) {
        Some(span_end) => span_end,
        None => {
            if debug {
                eprintln!(
                    "live-gui item fragment span rejected: reason=no-next-gui-prefix span_start={span_start}"
                );
            }
            return None;
        }
    };
    let span = bytes.get(span_start..span_end)?;
    if span.is_empty() || span.len() > MAX_GUI_ITEM_FRAGMENT_SPAN_BYTES {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=span-len span_start={span_start} span_end={span_end} len={}",
                span.len()
            );
        }
        return None;
    }

    let mut promoted_bits = match bits::decode_msb_valid_bits(span, CNW_FRAGMENT_HEADER_BITS) {
        Some(bits) => bits,
        None => {
            if debug {
                eprintln!(
                    "live-gui item fragment span rejected: reason=decode span_start={span_start} span_end={span_end} preview={:02X?}",
                    span.get(..span.len().min(16)).unwrap_or(&[])
                );
            }
            return None;
        }
    };
    if promoted_bits.len() < CNW_FRAGMENT_HEADER_BITS {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=short-bits span_start={span_start} span_end={span_end} bits={}",
                promoted_bits.len()
            );
        }
        return None;
    }
    promoted_bits.drain(0..CNW_FRAGMENT_HEADER_BITS);
    if promoted_bits.iter().all(|bit| !*bit) {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=zero-padding span_start={span_start} span_end={span_end}"
            );
        }
        return None;
    }

    let mut proof_bits = fragment_bits.clone();
    if bits::insert_msb_bits(&mut proof_bits, bit_cursor, &promoted_bits).is_none() {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=proof-bit-insert bit_cursor={bit_cursor} promoted_bits={}",
                promoted_bits.len()
            );
        }
        return None;
    }

    let mut trial_bytes = bytes.clone();
    trial_bytes.drain(span_start..span_end);
    let mut trial_record_end = *record_end;
    if insert_ee_live_gui_item_extras_for_ee(
        &mut trial_bytes,
        offset,
        &mut trial_record_end,
        &mut proof_bits,
        bit_cursor,
    )
    .is_none()
    {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=item-rewrite offset={offset} record_end={} span_start={span_start} span_end={span_end}",
                *record_end
            );
        }
        return None;
    }
    let mut proof_cursor = bit_cursor;
    if advance_verified_live_gui_record(
        &trial_bytes,
        offset,
        trial_record_end,
        &proof_bits,
        &mut proof_cursor,
    )
    .is_none()
    {
        if debug {
            eprintln!(
                "live-gui item fragment span rejected: reason=advance offset={offset} record_end={trial_record_end} bit_cursor={bit_cursor} span_start={span_start} span_end={span_end} promoted_bits={} promoted_preview={:?} next_bits={:?} span_preview={:02X?}",
                promoted_bits.len(),
                promoted_bits
                    .get(..promoted_bits.len().min(16))
                    .unwrap_or(&[]),
                proof_bits
                    .get(bit_cursor..bit_cursor.saturating_add(16).min(proof_bits.len()))
                    .unwrap_or(&[]),
                span.get(..span.len().min(16)).unwrap_or(&[])
            );
        }
        return None;
    }

    bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;
    let old_record_end = span_end;
    let bytes_promoted = span.len();
    let bits_promoted = promoted_bits.len();
    bytes.drain(span_start..span_end);
    *record_end = span_start;

    Some(LiveGuiItemFragmentSpanPromotion {
        old_record_end,
        bytes_promoted,
        bits_promoted,
    })
}

pub(super) fn remove_zero_fragment_storage_after_verified_live_gui_item_record_for_ee(
    bytes: &mut Vec<u8>,
    record_end: usize,
) -> Option<usize> {
    if record_end >= bytes.len() {
        return None;
    }
    let span_end = find_legacy_live_gui_item_fragment_span_end(bytes, record_end)?;
    if !is_zero_fragment_storage_span_before_legacy_live_gui_prefix(bytes, record_end, span_end) {
        return None;
    }
    let bytes_removed = span_end.saturating_sub(record_end);
    bytes.drain(record_end..span_end);
    Some(bytes_removed)
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
    let item_object_offset =
        legacy_live_gui_item_create_prefix(bytes, offset, scan_end)?.item_object_offset;
    appearance::try_get_legacy_gui_item_create_record_end(bytes, item_object_offset, scan_end, true)
}

fn find_legacy_live_gui_item_fragment_span_end(bytes: &[u8], span_start: usize) -> Option<usize> {
    let scan_end = span_start
        .checked_add(MAX_GUI_ITEM_FRAGMENT_SPAN_BYTES)?
        .min(bytes.len());
    for candidate in span_start.checked_add(1)?..scan_end {
        if legacy_live_gui_item_create_prefix(bytes, candidate, bytes.len()).is_some() {
            return Some(candidate);
        }
    }
    (scan_end == bytes.len() && span_start < bytes.len()).then_some(bytes.len())
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

fn legacy_live_gui_item_create_prefix(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<LiveGuiItemCreatePrefix> {
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
            (item_object_offset < record_end).then_some(LiveGuiItemCreatePrefix {
                item_object_offset,
                missing_add_inner_opcode: false,
            })
        }
        (
            GUI_INVENTORY_SUBOPCODE | GUI_INVENTORY_LOWER_SUBOPCODE,
            GUI_LEGACY_MISSING_ADD_INNER_OPCODE,
        ) => {
            let item_object_offset = offset.checked_add(7)?;
            (item_object_offset < record_end
                && looks_like_legacy_live_gui_object_id_at(bytes, item_object_offset))
            .then_some(LiveGuiItemCreatePrefix {
                item_object_offset,
                missing_add_inner_opcode: true,
            })
        }
        (GUI_REPOSITORY_SUBOPCODE | GUI_REPOSITORY_LOWER_SUBOPCODE, GUI_ADD_INNER_OPCODE) => {
            let item_object_offset = offset.checked_add(5)?;
            (item_object_offset < record_end).then_some(LiveGuiItemCreatePrefix {
                item_object_offset,
                missing_add_inner_opcode: false,
            })
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
