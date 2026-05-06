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
//! - EE `CNWSArea::PackAreaIntoMessage` writes the area OBJECTID, area resref,
//!   area name mode/data, dimensions, tileset, tiles, post-tile lists, and ends
//!   with two zero `WriteWORD` calls.
//! - EE `CNWMessage::WriteBits` writes fragment bits most-significant-bit
//!   first, and `GetWriteMessage` stores the final fragment bit cursor in the
//!   first byte's high three bits. Inserting an EE-only BOOL must therefore
//!   shift the following fragment bits, not merely OR a mask into byte zero.
//! - EE and Diamond `CNWMessage::SetReadMessage` both treat the first DWORD
//!   after the high-level header as the read-buffer length plus the three-byte
//!   high-level prefix. Moving the fragment stream therefore requires repairing
//!   that DWORD too.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const AREA_MAJOR: u8 = 0x04;
const AREA_CLIENT_AREA_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const MIN_READ_SIZE: usize = 4;

const CRESREF_TEXT_BYTES: usize = 16;
const AREA_NAME_READ_OFFSET: usize = 44;
const AREA_STATIC_WIDTH_BYTES_AFTER_NAME_END: usize = 96;
const MAX_REASONABLE_AREA_DIMENSION: u32 = 512;
const MAX_REASONABLE_AREA_TILE_COUNT: u32 = 65_536;

const TRANSITION_INDEX_PAYLOAD_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const START_X_PAYLOAD_OFFSET: usize = TRANSITION_INDEX_PAYLOAD_OFFSET + 4;
const LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET: usize =
    HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 4 + 4 * 4;
const LEGACY_AREA_OBJECT_ID_BYTES: usize = 4;

// `CNWMessage::GetWriteMessage` stores the final fragment bit count in the
// first byte's high three bits. The actual fragment data starts immediately
// after those header bits.
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const AREA_PRESENT_FRAGMENT_BIT_INDEX: usize = CNW_FRAGMENT_HEADER_BITS;
const AREA_NAME_MODE_FRAGMENT_BIT_INDEX: usize = CNW_FRAGMENT_HEADER_BITS + 1;
const EE_POST_STATIC_ZERO_WORD_BYTES: usize = 4;

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
        < LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET
            + LEGACY_AREA_OBJECT_ID_BYTES
            + CRESREF_TEXT_BYTES
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
    if !legacy_area_object_id_plausible(legacy_area_object_id) || !area_resref_plausible(&area_resref)
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

    let old_fragment_byte = payload[fragment_offset];
    let rewritten_fragment = rewrite_area_fragment_bits(&payload[fragment_offset..])?;
    let new_fragment_byte = *rewritten_fragment.first()?;
    let new_declared = declared.checked_add(EE_POST_STATIC_ZERO_WORD_BYTES as u32)?;
    let new_read_size = read_size + EE_POST_STATIC_ZERO_WORD_BYTES;
    let new_fragment_offset = fragment_offset + EE_POST_STATIC_ZERO_WORD_BYTES;

    let mut replacement =
        Vec::with_capacity(EE_POST_STATIC_ZERO_WORD_BYTES + rewritten_fragment.len());
    replacement.extend_from_slice(&[0u8; EE_POST_STATIC_ZERO_WORD_BYTES]);
    replacement.extend_from_slice(&rewritten_fragment);

    payload.splice(fragment_offset.., replacement);
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared)?;

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
    })
}

fn rewrite_area_fragment_bits(fragment: &[u8]) -> Option<Vec<u8>> {
    let mut bits = decode_cnw_msb_valid_bits(fragment)?;
    if bits.len() <= AREA_PRESENT_FRAGMENT_BIT_INDEX {
        tracing::warn!(
            fragment_size = fragment.len(),
            "Area_ClientArea rewrite skipped: fragment bit stream too short for EE area-present BOOL"
        );
        return None;
    }

    // EE `SendServerToPlayerArea_ClientArea` writes one area-present BOOL
    // immediately before `CNWSArea::PackAreaIntoMessage`. The legacy payloads
    // observed from the 1.69 server begin with the area packer's bits, so the
    // EE-only BOOL must be inserted before them to preserve every following
    // bit's meaning.
    bits.insert(AREA_PRESENT_FRAGMENT_BIT_INDEX, true);

    // EE `CNWSArea::PackAreaIntoMessage` writes BOOL(1) before
    // `WriteCExoString` and BOOL(0) before `WriteCExoLocStringServer`.
    // The verified legacy read-buffer shape at AREA_NAME_READ_OFFSET is a raw
    // CExoString, so emit the EE raw-string selector while keeping the rest of
    // the fragment stream shifted intact.
    if bits.len() <= AREA_NAME_MODE_FRAGMENT_BIT_INDEX {
        tracing::warn!(
            fragment_size = fragment.len(),
            "Area_ClientArea rewrite skipped: fragment bit stream too short for EE area-name BOOL"
        );
        return None;
    }
    bits[AREA_NAME_MODE_FRAGMENT_BIT_INDEX] = true;

    Some(pack_cnw_msb_valid_bits(bits))
}

fn decode_cnw_msb_valid_bits(fragment: &[u8]) -> Option<Vec<bool>> {
    let valid_bits = cnw_fragment_valid_bit_count(fragment)?;
    let mut bits = Vec::with_capacity(valid_bits);
    for bit_index in 0..valid_bits {
        let byte = *fragment.get(bit_index / 8)?;
        let mask = 0x80 >> (bit_index % 8);
        bits.push((byte & mask) != 0);
    }
    Some(bits)
}

fn cnw_fragment_valid_bit_count(fragment: &[u8]) -> Option<usize> {
    let first = *fragment.first()?;
    let final_fragment_bits = ((first & 0xE0) >> 5) as usize;
    let valid_bits = if final_fragment_bits == 0 {
        fragment.len().checked_mul(8)?
    } else {
        fragment
            .len()
            .checked_sub(1)?
            .checked_mul(8)?
            .checked_add(final_fragment_bits)?
    };

    if valid_bits < CNW_FRAGMENT_HEADER_BITS {
        tracing::warn!(
            fragment_size = fragment.len(),
            final_fragment_bits,
            valid_bits,
            "Area_ClientArea rewrite skipped: invalid CNW fragment final-bit header"
        );
        return None;
    }
    Some(valid_bits)
}

fn pack_cnw_msb_valid_bits(mut bits: Vec<bool>) -> Vec<u8> {
    let valid_bits = bits.len();
    let final_fragment_bits = valid_bits % 8;

    bits[0] = (final_fragment_bits & 0x04) != 0;
    bits[1] = (final_fragment_bits & 0x02) != 0;
    bits[2] = (final_fragment_bits & 0x01) != 0;

    let mut packed = vec![0u8; valid_bits.div_ceil(8)];
    for (bit_index, bit) in bits.into_iter().enumerate() {
        if bit {
            packed[bit_index / 8] |= 0x80 >> (bit_index % 8);
        }
    }
    packed
}

fn area_static_layout(payload: &[u8], fragment_offset: usize) -> Option<AreaStaticLayout> {
    let (area_name_length, name_end) =
        read_c_exo_string_shape(payload, fragment_offset, AREA_NAME_READ_OFFSET, 1024)?;
    let width_read_offset = name_end.checked_add(AREA_STATIC_WIDTH_BYTES_AFTER_NAME_END)?;
    let height_read_offset = width_read_offset.checked_add(4)?;
    let tileset_read_offset = height_read_offset.checked_add(4)?;
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

    if tile_count == 0
        || tile_count >= MAX_REASONABLE_AREA_TILE_COUNT
        || tile_count % width != 0
    {
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
    write_u32_le(payload, height_payload_offset, scan.inferred_height).is_some()
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

fn start_fields_plausible(payload: &[u8]) -> bool {
    (0..4).all(|index| {
        read_f32_le(payload, START_X_PAYLOAD_OFFSET + index * 4)
            .is_some_and(|value| {
            value.is_finite() && (index == 3 || value.abs() <= 100_000.0)
            })
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
    bytes.get_mut(offset..offset + 4)?.copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Option<f32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
