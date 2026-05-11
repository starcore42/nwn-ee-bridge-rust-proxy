//! `Area_ClientArea` semantic translation.
//!
//! This module answers one narrow question:
//! "Given the verified Diamond `Area_ClientArea` byte shape emitted by a
//! 1.69 server, what exact EE-facing byte shape should we emit?"
//!
//! Decompile anchors used for this transform:
//!
//! - EE `CNWSMessage::SendServerToPlayerArea_ClientArea` writes the transition
//!   index, start position/facing, area-present BOOL, then delegates to
//!   `CNWSArea::PackAreaIntoMessage` before sending high-level major/minor
//!   `0x04/0x01`.
//! - EE `CNWCArea::LoadArea` reads the area OBJECTID, area resref, an EE-only
//!   area-name mode BOOL, area name data, dimensions, tileset, tiles, and
//!   post-tile lists. Later EE-only grass/tile-render fields are gated through
//!   `CNetLayer::ServerSatisfiesBuild(0x2001, ...)`; against a 1.69 server
//!   those branches are false, so the tile byte stream stays Diamond-shaped.
//! - EE `CNWSMessage::SendServerToPlayerArea_ClientArea` writes the
//!   area-present BOOL immediately before calling `CNWSArea::PackAreaIntoMessage`.
//!   `PackAreaIntoMessage` then writes the area-name BOOL. Diamond `CNWCArea`
//!   reads the area name without that discriminator. The decompiled EE client
//!   consumes the earlier area flag before `CNWCArea::LoadArea`; the legacy
//!   driver shim fixed this by forcing the existing `0x08` first-fragment-byte
//!   bit at the area-name read site, not by inserting and shifting every later
//!   area BOOL. Driver-only mode requires the proxy to force that bit in-band.
//! - EE and Diamond `CNWMessage::SetReadMessage` both treat the first DWORD
//!   after the high-level header as the read-buffer length plus the three-byte
//!   high-level prefix. Moving the fragment stream therefore requires repairing
//!   that DWORD too.
//! - EE `CNWMessage::SetReadMessage` consumes the first three fragment bits as
//!   the "valid bits in final fragment byte" count, and
//!   `MessageReadUnderflow` treats zero as "the final byte has no valid bits",
//!   not "the final byte is full". Fragment repacking must therefore allocate
//!   `valid_bits / 8 + 1` bytes, preserving the trailing padding byte when the
//!   valid-bit count lands exactly on a byte boundary.
//! - EE `CNWCArea::LoadArea` performs two post-static-list WORD reads for
//!   zero-count server-side lists that are not present in the legacy stream.
//!   The old driver shim had to synthesize both counts at the client read site;
//!   driver-only mode requires the proxy to insert both zero WORDs in-band.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const AREA_MAJOR: u8 = 0x04;
const AREA_CLIENT_AREA_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const MIN_READ_SIZE: usize = 4;

const CRESREF_TEXT_BYTES: usize = 16;
const AREA_NAME_READ_OFFSET: usize = 44;
const AREA_WIDTH_BYTES_AFTER_NAME_END: usize = 96;
const AREA_HEIGHT_BYTES_AFTER_NAME_END: usize = 100;
const AREA_TILESET_BYTES_AFTER_NAME_END: usize = 104;
const MAX_REASONABLE_AREA_DIMENSION: u32 = 512;
const MAX_REASONABLE_AREA_TILE_COUNT: u32 = 65_536;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const AREA_PRESENT_USER_BOOL_COUNT: usize = 1;
const AREA_NAME_MODE_FORCE_MASK: u8 = 0x08;

const TRANSITION_INDEX_PAYLOAD_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const START_X_PAYLOAD_OFFSET: usize = TRANSITION_INDEX_PAYLOAD_OFFSET + 4;
const LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET: usize =
    HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 4 + 4 * 4;
const LEGACY_AREA_OBJECT_ID_BYTES: usize = 4;

const EE_POST_STATIC_LIST_ZERO_WORD_BYTES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaRewriteKind {
    ExactEeAreaNameModeBitForce,
    ExactEePostStaticListZeroWords,
    LegacyHgMissingHeightRepair,
}

#[derive(Debug, Clone)]
pub struct AreaRewriteSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_read_size: usize,
    pub new_read_size: usize,
    pub old_fragment_offset: usize,
    pub new_fragment_offset: usize,
    pub fragment_size: usize,
    pub legacy_area_object_id: u32,
    pub area_resref: String,
    pub old_fragment_byte: u8,
    pub new_fragment_byte: u8,
    pub area_name_length: u32,
    pub area_name_end_read_offset: usize,
    pub width_read_offset: usize,
    pub height_read_offset: usize,
    pub tileset_read_offset: usize,
    pub first_tile_read_offset: usize,
    pub width: u32,
    pub packet_height: u32,
    pub inferred_height: u32,
    pub tile_count: u32,
    pub tile_scan_valid: bool,
    pub height_repaired: bool,
    pub rewrite_kinds: Vec<AreaRewriteKind>,
    pub placeable_context_valid: bool,
    pub placeable_light_count: usize,
    pub placeable_static_count: usize,
    pub placeable_context: AreaPlaceableContext,
}

#[derive(Debug, Clone, Default)]
pub struct AreaPlaceableContext {
    pub area_resref: String,
    pub light_rows: Vec<AreaPlaceableContextRow>,
    pub static_rows: Vec<AreaPlaceableContextRow>,
}

impl AreaPlaceableContext {
    pub fn contains_placeable_id(&self, object_id: u32) -> bool {
        self.light_rows
            .iter()
            .chain(self.static_rows.iter())
            .any(|row| row.object_id == object_id)
    }

    pub fn rows_with_placeable_id(
        &self,
        object_id: u32,
    ) -> impl Iterator<Item = &AreaPlaceableContextRow> {
        self.light_rows
            .iter()
            .chain(self.static_rows.iter())
            .filter(move |row| row.object_id == object_id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AreaPlaceableContextRow {
    pub object_id: u32,
    pub appearance: u16,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub dir_x: f32,
    pub dir_y: f32,
    pub dir_z: f32,
    pub has_direction: bool,
}

#[derive(Debug, Clone, Default)]
struct AreaStaticLayout {
    valid: bool,
    area_name_length: u32,
    area_name_end_read_offset: usize,
    width_read_offset: usize,
    height_read_offset: usize,
    tileset_read_offset: usize,
    first_tile_read_offset: usize,
}

#[derive(Debug, Clone, Default)]
struct AreaTileStreamScan {
    valid: bool,
    layout: AreaStaticLayout,
    width: u32,
    packet_height: u32,
    inferred_height: u32,
    tile_count: u32,
    tile_end_read_offset: usize,
}

pub fn rewrite_area_client_area_payload(payload: &mut Vec<u8>) -> Option<AreaRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != AREA_MAJOR
        || payload[2] != AREA_CLIENT_AREA_MINOR
    {
        return None;
    }

    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if declared < (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u32 {
        tracing::warn!(
            declared,
            "Area_ClientArea rewrite skipped: invalid CNW length DWORD"
        );
        return None;
    }

    let payload_size = payload.len().checked_sub(HIGH_LEVEL_HEADER_BYTES)?;
    let read_size = declared as usize - HIGH_LEVEL_HEADER_BYTES;
    if !(MIN_READ_SIZE..=payload_size).contains(&read_size) {
        tracing::warn!(
            declared,
            read_size,
            payload_size,
            "Area_ClientArea rewrite skipped: read size outside payload"
        );
        return None;
    }

    let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
    let fragment_size = payload.len().checked_sub(fragment_offset)?;
    if fragment_size == 0 {
        tracing::warn!(
            declared,
            fragment_offset,
            "Area_ClientArea rewrite skipped: missing fragment bit stream"
        );
        return None;
    }

    if fragment_offset
        < LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES + CRESREF_TEXT_BYTES
    {
        tracing::warn!(
            declared,
            fragment_offset,
            "Area_ClientArea rewrite skipped: too short for area OBJECTID/resref"
        );
        return None;
    }

    let legacy_area_object_id = read_u32_le(payload, LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET)?;
    let area_resref = fixed_resref_preview(
        payload,
        LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES,
    )?;
    if !legacy_area_object_id_plausible(legacy_area_object_id)
        || !area_resref_plausible(&area_resref)
    {
        tracing::warn!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            area_resref = %area_resref,
            "Area_ClientArea rewrite skipped: implausible area OBJECTID/resref"
        );
        return None;
    }

    if !start_fields_plausible(payload) {
        tracing::warn!("Area_ClientArea rewrite skipped: implausible transition/start fields");
        return None;
    }

    let static_layout = area_static_layout(payload, fragment_offset)?;
    if !static_layout.valid {
        tracing::warn!("Area_ClientArea rewrite skipped: static layout unavailable");
        return None;
    }

    let mut tile_scan = scan_area_tile_stream(payload, fragment_offset);
    let height_repaired = repair_missing_area_height(payload, fragment_offset, &mut tile_scan);
    if !tile_scan.valid {
        tracing::warn!(
            area_resref = %area_resref,
            width = tile_scan.width,
            packet_height = tile_scan.packet_height,
            inferred_height = tile_scan.inferred_height,
            tile_count = tile_scan.tile_count,
            first_tile_read_offset = static_layout.first_tile_read_offset,
            "Area_ClientArea rewrite skipped: decompile-shaped tile stream did not validate"
        );
        return None;
    }
    let mut rewrite_kinds = vec![
        AreaRewriteKind::ExactEeAreaNameModeBitForce,
        AreaRewriteKind::ExactEePostStaticListZeroWords,
    ];
    if height_repaired {
        rewrite_kinds.push(AreaRewriteKind::LegacyHgMissingHeightRepair);
    }

    let old_fragment_byte = payload[fragment_offset];
    let rewritten_fragment = rewrite_area_fragment_bits(&payload[fragment_offset..])?;
    let new_fragment_byte = *rewritten_fragment.first()?;
    let new_declared = declared + EE_POST_STATIC_LIST_ZERO_WORD_BYTES as u32;
    let new_read_size = read_size + EE_POST_STATIC_LIST_ZERO_WORD_BYTES;
    let new_fragment_offset = fragment_offset + EE_POST_STATIC_LIST_ZERO_WORD_BYTES;
    let placeable_context =
        collect_area_post_tile_placeable_context(payload, fragment_offset, &area_resref);
    let placeable_context_valid = placeable_context.is_some();
    let placeable_context = placeable_context.unwrap_or_default();
    let placeable_light_count = placeable_context.light_rows.len();
    let placeable_static_count = placeable_context.static_rows.len();

    let mut rewritten_payload = Vec::with_capacity(
        fragment_offset + EE_POST_STATIC_LIST_ZERO_WORD_BYTES + rewritten_fragment.len(),
    );
    rewritten_payload.extend_from_slice(&payload[..fragment_offset]);
    rewritten_payload.extend_from_slice(&[0, 0, 0, 0]);
    rewritten_payload.extend_from_slice(&rewritten_fragment);
    *payload = rewritten_payload;
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared)?;
    for kind in &rewrite_kinds {
        tracing::info!(
            rewrite_kind = ?kind,
            area_resref = %area_resref,
            old_declared = declared,
            new_declared,
            old_fragment_offset = fragment_offset,
            new_fragment_offset,
            width = tile_scan.width,
            packet_height = tile_scan.packet_height,
            inferred_height = tile_scan.inferred_height,
            tile_count = tile_scan.tile_count,
            tile_scan_valid = tile_scan.valid,
            height_repaired,
            first_tile_read_offset = static_layout.first_tile_read_offset,
            old_fragment_byte,
            new_fragment_byte,
            "Area_ClientArea named compatibility rewrite applied"
        );
    }

    Some(AreaRewriteSummary {
        old_declared: declared,
        new_declared,
        old_read_size: read_size,
        new_read_size,
        old_fragment_offset: fragment_offset,
        new_fragment_offset,
        fragment_size,
        legacy_area_object_id,
        area_resref,
        old_fragment_byte,
        new_fragment_byte,
        area_name_length: static_layout.area_name_length,
        area_name_end_read_offset: static_layout.area_name_end_read_offset,
        width_read_offset: static_layout.width_read_offset,
        height_read_offset: static_layout.height_read_offset,
        tileset_read_offset: static_layout.tileset_read_offset,
        first_tile_read_offset: static_layout.first_tile_read_offset,
        width: tile_scan.width,
        packet_height: tile_scan.packet_height,
        inferred_height: tile_scan.inferred_height,
        tile_count: tile_scan.tile_count,
        tile_scan_valid: tile_scan.valid,
        height_repaired,
        rewrite_kinds,
        placeable_context_valid,
        placeable_light_count,
        placeable_static_count,
        placeable_context,
    })
}

pub fn ee_area_client_area_payload_shape_valid(payload: &[u8]) -> bool {
    let Some((_, _, fragment_offset, fragment_size)) = area_client_area_read_window(payload) else {
        return false;
    };
    if fragment_size == 0
        || payload
            .get(fragment_offset)
            .map(|byte| (byte & AREA_NAME_MODE_FORCE_MASK) == 0)
            .unwrap_or(true)
    {
        return false;
    }

    let Some(legacy_area_object_id) = read_u32_le(payload, LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET)
    else {
        return false;
    };
    let Some(area_resref) = fixed_resref_preview(
        payload,
        LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES,
    ) else {
        return false;
    };
    if !legacy_area_object_id_plausible(legacy_area_object_id)
        || !area_resref_plausible(&area_resref)
        || !start_fields_plausible(payload)
    {
        return false;
    }

    area_static_layout(payload, fragment_offset)
        .filter(|layout| layout.valid)
        .is_some()
        && scan_area_tile_stream(payload, fragment_offset).valid
}

fn area_client_area_read_window(payload: &[u8]) -> Option<(u32, usize, usize, usize)> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != AREA_MAJOR
        || payload[2] != AREA_CLIENT_AREA_MINOR
    {
        return None;
    }

    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if declared < (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u32 {
        return None;
    }
    let payload_size = payload.len().checked_sub(HIGH_LEVEL_HEADER_BYTES)?;
    let read_size = declared as usize - HIGH_LEVEL_HEADER_BYTES;
    if !(MIN_READ_SIZE..=payload_size).contains(&read_size) {
        return None;
    }
    let fragment_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(read_size)?;
    let fragment_size = payload.len().checked_sub(fragment_offset)?;
    Some((declared, read_size, fragment_offset, fragment_size))
}

fn rewrite_area_fragment_bits(fragment: &[u8]) -> Option<Vec<u8>> {
    let _bits = decode_cnw_msb_valid_bits(
        fragment,
        CNW_FRAGMENT_HEADER_BITS + AREA_PRESENT_USER_BOOL_COUNT + 1,
    )?;
    let mut rewritten = fragment.to_vec();
    let Some(first) = rewritten.first_mut() else {
        tracing::warn!(
            fragment_size = fragment.len(),
            "Area_ClientArea rewrite skipped: fragment bit stream too short for EE area BOOL force"
        );
        return None;
    };
    *first |= AREA_NAME_MODE_FORCE_MASK;
    Some(rewritten)
}

fn decode_cnw_msb_valid_bits(fragment: &[u8], min_valid_bits: usize) -> Option<Vec<bool>> {
    let first = *fragment.first()?;
    let final_fragment_bits = ((first & 0xE0) >> 5) as usize;
    let valid_bits = fragment
        .len()
        .checked_sub(1)?
        .checked_mul(8)?
        .checked_add(final_fragment_bits)?;
    if valid_bits < min_valid_bits {
        tracing::warn!(
            valid_bits,
            min_valid_bits,
            fragment_size = fragment.len(),
            "Area_ClientArea rewrite skipped: fragment bit stream has too few valid bits"
        );
        return None;
    }

    let mut bits = Vec::with_capacity(valid_bits);
    for bit_index in 0..valid_bits {
        let byte = *fragment.get(bit_index / 8)?;
        bits.push((byte & (0x80 >> (bit_index % 8))) != 0);
    }
    Some(bits)
}

fn area_static_layout(payload: &[u8], fragment_offset: usize) -> Option<AreaStaticLayout> {
    let (area_name_length, name_end) =
        read_c_exo_string_shape(payload, fragment_offset, AREA_NAME_READ_OFFSET, 1024)?;
    // EE `CNWCArea::LoadArea` first consumes three INTs and several
    // environment DWORD/BYTE/BOOL fields after the area-name payload. Those
    // early DWORDs are not the tile-grid dimensions. The decompiled client
    // later reads the actual grid width/height into `[area+0Ch]` and
    // `[area+10h]`, immediately before `ReadCResRef(16)` for the tileset; the
    // EE server writer mirrors this with `[area+0Ch]`, `[area+10h]`, then
    // `WriteCResRef`. The HG `docksofascension` fixture proves this offset:
    // width=11 is at `name_end + 96`, the legacy missing height DWORD is zero
    // at `name_end + 100`, and tileset `ttr01` starts at `name_end + 104`.
    let width_read_offset = name_end.checked_add(AREA_WIDTH_BYTES_AFTER_NAME_END)?;
    let height_read_offset = name_end.checked_add(AREA_HEIGHT_BYTES_AFTER_NAME_END)?;
    let tileset_read_offset = name_end.checked_add(AREA_TILESET_BYTES_AFTER_NAME_END)?;
    let first_tile_read_offset = tileset_read_offset.checked_add(CRESREF_TEXT_BYTES)?;

    Some(AreaStaticLayout {
        valid: true,
        area_name_length,
        area_name_end_read_offset: name_end,
        width_read_offset,
        height_read_offset,
        tileset_read_offset,
        first_tile_read_offset,
    })
}

fn scan_area_tile_stream(payload: &[u8], fragment_offset: usize) -> AreaTileStreamScan {
    let Some(layout) = area_static_layout(payload, fragment_offset) else {
        return AreaTileStreamScan::default();
    };

    let Some(width) = read_area_u32(payload, fragment_offset, layout.width_read_offset) else {
        return AreaTileStreamScan {
            layout,
            ..AreaTileStreamScan::default()
        };
    };
    let Some(packet_height) = read_area_u32(payload, fragment_offset, layout.height_read_offset)
    else {
        return AreaTileStreamScan {
            layout,
            width,
            ..AreaTileStreamScan::default()
        };
    };
    if width == 0 || width > MAX_REASONABLE_AREA_DIMENSION {
        return AreaTileStreamScan {
            layout,
            width,
            packet_height,
            ..AreaTileStreamScan::default()
        };
    }

    let mut cursor = layout.first_tile_read_offset;
    let mut tile_count = 0u32;
    while tile_count < MAX_REASONABLE_AREA_TILE_COUNT {
        let Some(record_length) = area_tile_record_length_at(payload, fragment_offset, cursor)
        else {
            break;
        };
        tile_count += 1;
        cursor += record_length;
    }

    if tile_count == 0 || tile_count >= MAX_REASONABLE_AREA_TILE_COUNT || tile_count % width != 0 {
        return AreaTileStreamScan {
            layout,
            width,
            packet_height,
            tile_count,
            tile_end_read_offset: cursor,
            ..AreaTileStreamScan::default()
        };
    }

    let inferred_height = tile_count / width;
    if inferred_height == 0 || inferred_height > MAX_REASONABLE_AREA_DIMENSION {
        return AreaTileStreamScan {
            layout,
            width,
            packet_height,
            inferred_height,
            tile_count,
            tile_end_read_offset: cursor,
            ..AreaTileStreamScan::default()
        };
    }
    if packet_height != 0 && packet_height != inferred_height {
        return AreaTileStreamScan {
            layout,
            width,
            packet_height,
            inferred_height,
            tile_count,
            tile_end_read_offset: cursor,
            ..AreaTileStreamScan::default()
        };
    }

    AreaTileStreamScan {
        valid: true,
        layout,
        width,
        packet_height,
        inferred_height,
        tile_count,
        tile_end_read_offset: cursor,
    }
}

fn collect_area_post_tile_placeable_context(
    payload: &[u8],
    fragment_offset: usize,
    area_resref: &str,
) -> Option<AreaPlaceableContext> {
    let scan = scan_area_tile_stream(payload, fragment_offset);
    if !scan.valid {
        return None;
    }

    let mut cursor = scan.tile_end_read_offset;

    let transition_count = read_area_u32(payload, fragment_offset, cursor)?;
    if transition_count > 4096 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    for _ in 0..transition_count {
        let name_offset = cursor.checked_add(4 + 3 * 4)?;
        let (_, after_name) = read_c_exo_string_shape(payload, fragment_offset, name_offset, 1024)?;
        cursor = after_name;
    }

    let map_pin_count = read_area_u32(payload, fragment_offset, cursor)?;
    if map_pin_count > 4096 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    for _ in 0..map_pin_count {
        let label_offset = cursor.checked_add(4)?;
        let (_, after_label) =
            read_c_exo_string_shape(payload, fragment_offset, label_offset, 1024)?;
        cursor = after_label.checked_add(3 * 4)?;
        if HIGH_LEVEL_HEADER_BYTES + cursor > fragment_offset {
            return None;
        }
    }

    let sound_count = read_area_u16(payload, fragment_offset, cursor)?;
    if sound_count > 4096 {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    for _ in 0..sound_count {
        const AREA_SOUND_RESREF_COUNT_OFFSET: usize = 52;
        const AREA_SOUND_BASE_BYTES: usize = 54;
        let resref_count = read_area_u16(
            payload,
            fragment_offset,
            cursor + AREA_SOUND_RESREF_COUNT_OFFSET,
        )?;
        if resref_count > 64 {
            return None;
        }
        let bytes =
            AREA_SOUND_BASE_BYTES.checked_add(resref_count as usize * CRESREF_TEXT_BYTES)?;
        cursor = cursor.checked_add(bytes)?;
        if HIGH_LEVEL_HEADER_BYTES + cursor > fragment_offset {
            return None;
        }
    }

    let light_count = read_area_u16(payload, fragment_offset, cursor)?;
    cursor = cursor.checked_add(2)?;
    let mut light_rows = Vec::with_capacity(light_count as usize);
    for _ in 0..light_count {
        let object_id = read_area_u32(payload, fragment_offset, cursor)?;
        let appearance = read_area_u16(payload, fragment_offset, cursor + 4)?;
        let x = read_area_f32(payload, fragment_offset, cursor + 6)?;
        let y = read_area_f32(payload, fragment_offset, cursor + 10)?;
        let z = read_area_f32(payload, fragment_offset, cursor + 14)?;
        light_rows.push(AreaPlaceableContextRow {
            object_id,
            appearance,
            x,
            y,
            z,
            has_direction: false,
            ..AreaPlaceableContextRow::default()
        });
        cursor = cursor.checked_add(4 + 2 + 3 * 4)?;
    }

    let static_count = read_area_u16(payload, fragment_offset, cursor)?;
    cursor = cursor.checked_add(2)?;
    let mut static_rows = Vec::with_capacity(static_count as usize);
    for _ in 0..static_count {
        let object_id = read_area_u32(payload, fragment_offset, cursor)?;
        let appearance = read_area_u16(payload, fragment_offset, cursor + 4)?;
        let x = read_area_f32(payload, fragment_offset, cursor + 6)?;
        let y = read_area_f32(payload, fragment_offset, cursor + 10)?;
        let z = read_area_f32(payload, fragment_offset, cursor + 14)?;
        let dir_x = read_area_f32(payload, fragment_offset, cursor + 18)?;
        let dir_y = read_area_f32(payload, fragment_offset, cursor + 22)?;
        let dir_z = read_area_f32(payload, fragment_offset, cursor + 26)?;
        static_rows.push(AreaPlaceableContextRow {
            object_id,
            appearance,
            x,
            y,
            z,
            dir_x,
            dir_y,
            dir_z,
            has_direction: true,
        });
        cursor = cursor.checked_add(4 + 2 + 6 * 4)?;
    }

    Some(AreaPlaceableContext {
        area_resref: area_resref.to_string(),
        light_rows,
        static_rows,
    })
}

fn repair_missing_area_height(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &mut AreaTileStreamScan,
) -> bool {
    if !scan.valid || scan.packet_height != 0 || scan.inferred_height == 0 {
        return false;
    }
    let height_payload_offset = HIGH_LEVEL_HEADER_BYTES + scan.layout.height_read_offset;
    if height_payload_offset > fragment_offset || fragment_offset - height_payload_offset < 4 {
        return false;
    }
    if write_u32_le(payload, height_payload_offset, scan.inferred_height).is_some() {
        scan.packet_height = scan.inferred_height;
        true
    } else {
        false
    }
}

fn area_tile_record_length_at(
    payload: &[u8],
    fragment_offset: usize,
    read_offset: usize,
) -> Option<usize> {
    let tile_id = read_area_u32(payload, fragment_offset, read_offset)?;
    let orientation = read_area_u32(payload, fragment_offset, read_offset + 4)?;
    let raw_height = read_area_u32(payload, fragment_offset, read_offset + 8)?;
    let flags = read_area_u16(payload, fragment_offset, read_offset + 12)?;

    if tile_id > 65_535 || orientation > 3 {
        return None;
    }
    let signed_height = raw_height as i32 as i64;
    if !(-256..=1024).contains(&signed_height) {
        return None;
    }
    if (flags & 0xFF00) != 0 || (flags & 0x000C) != 0x000C {
        return None;
    }

    let length = area_tile_record_byte_count(flags);
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || length > fragment_offset - payload_offset {
        return None;
    }
    Some(length)
}

fn area_tile_record_byte_count(flags: u16) -> usize {
    let mut length = 4 + 4 + 4 + 2;
    if (flags & 0x0001) != 0 {
        length += 1;
    }
    if (flags & 0x0002) != 0 {
        length += 1;
    }
    // EE's writer ORs the source-light bits and then writes both source-light
    // bytes. Diamond packets reaching this bridge already use the same tile
    // record byte grammar; the missing dialect pieces are elsewhere.
    length += 2;
    if (flags & 0x0010) != 0 {
        length += 1;
    }
    if (flags & 0x0020) != 0 {
        length += 1;
    }
    if (flags & 0x0040) != 0 {
        length += 1;
    }
    if (flags & 0x0080) != 0 {
        length += 1;
    }
    length
}

fn read_c_exo_string_shape(
    payload: &[u8],
    fragment_offset: usize,
    read_offset: usize,
    max_length: u32,
) -> Option<(u32, usize)> {
    let length = read_area_u32(payload, fragment_offset, read_offset)?;
    if length > max_length {
        return None;
    }
    let string_read_offset = read_offset.checked_add(4)?;
    let string_payload_offset = HIGH_LEVEL_HEADER_BYTES + string_read_offset;
    let length_usize = length as usize;
    if string_payload_offset > fragment_offset
        || length_usize > fragment_offset - string_payload_offset
    {
        return None;
    }
    if !payload[string_payload_offset..string_payload_offset + length_usize]
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }
    Some((length, string_read_offset + length_usize))
}

fn read_area_u32(payload: &[u8], fragment_offset: usize, read_offset: usize) -> Option<u32> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < 4 {
        return None;
    }
    read_u32_le(payload, payload_offset)
}

fn read_area_u16(payload: &[u8], fragment_offset: usize, read_offset: usize) -> Option<u16> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < 2 {
        return None;
    }
    let bytes = payload.get(payload_offset..payload_offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_area_f32(payload: &[u8], fragment_offset: usize, read_offset: usize) -> Option<f32> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < 4 {
        return None;
    }
    read_f32_le(payload, payload_offset)
}

fn start_fields_plausible(payload: &[u8]) -> bool {
    (0..4).all(|index| {
        read_f32_le(payload, START_X_PAYLOAD_OFFSET + index * 4)
            .is_some_and(|value| value.is_finite() && (index == 3 || value.abs() <= 100_000.0))
    })
}

fn fixed_resref_preview(payload: &[u8], offset: usize) -> Option<String> {
    let bytes = payload.get(offset..offset + CRESREF_TEXT_BYTES)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(CRESREF_TEXT_BYTES);
    Some(String::from_utf8_lossy(&bytes[..end]).to_string())
}

fn legacy_area_object_id_plausible(object_id: u32) -> bool {
    (0x8000_0000..0xFFFF_0000).contains(&object_id)
}

fn area_resref_plausible(resref: &str) -> bool {
    !resref.is_empty()
        && resref.len() <= CRESREF_TEXT_BYTES
        && resref
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    bytes
        .get_mut(offset..offset + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Option<f32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCKS_OF_ASCENSION_LEGACY_MISSING_HEIGHT: &[u8] = include_bytes!(
        "../../fixtures/area/hg_docksofascension_client_area_legacy_missing_height.bin"
    );

    #[test]
    fn docksofascension_uses_decompile_backed_tile_dimension_offsets() {
        let payload = DOCKS_OF_ASCENSION_LEGACY_MISSING_HEIGHT;
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(payload).expect("fixture read window");
        let layout = area_static_layout(payload, fragment_offset).expect("fixture static layout");

        assert_eq!(
            read_area_u32(payload, fragment_offset, layout.width_read_offset),
            Some(11)
        );
        assert_eq!(
            read_area_u32(payload, fragment_offset, layout.height_read_offset),
            Some(0)
        );
        assert_eq!(
            fixed_resref_preview(payload, HIGH_LEVEL_HEADER_BYTES + layout.tileset_read_offset)
                .as_deref(),
            Some("ttr01")
        );

        let scan = scan_area_tile_stream(payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(scan.width, 11);
        assert_eq!(scan.packet_height, 0);
        assert_eq!(scan.inferred_height, 14);
        assert_eq!(scan.tile_count, 154);
    }

    #[test]
    fn docksofascension_rewrite_repairs_missing_height_and_validates() {
        let mut payload = DOCKS_OF_ASCENSION_LEGACY_MISSING_HEIGHT.to_vec();
        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("legacy missing-height area rewrite");

        assert!(summary.tile_scan_valid);
        assert!(summary.height_repaired);
        assert_eq!(summary.width, 11);
        assert_eq!(summary.packet_height, 14);
        assert_eq!(summary.inferred_height, 14);
        assert_eq!(summary.tile_count, 154);
        assert_eq!(summary.area_resref, "docksofascension");
        assert!(summary
            .rewrite_kinds
            .contains(&AreaRewriteKind::LegacyHgMissingHeightRepair));
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }
}
