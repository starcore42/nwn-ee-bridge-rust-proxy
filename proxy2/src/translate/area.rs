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
//!   post-tile lists. EE `CNWSArea::PackAreaIntoMessage` and
//!   `CNWCArea::LoadArea` both gate two static-header float triplets on
//!   `ServerSatisfiesBuild(0x2001, 0x23, 0)`. The bridge must advertise this
//!   proxy-owned EE-facing server dialect in BNVR so later live-object
//!   visual-transform readers are selected correctly without also enabling newer
//!   unmodeled build gates. Driver-only mode then requires the proxy to
//!   synthesize those two EE writer fields before width/height/tileset.
//! - The same advertised build necessarily satisfies the earlier
//!   `ServerSatisfiesBuild(0x2001, 0x24, 3)` gate. The decompiled EE writer
//!   emits, after the tileset `CResRef`, a fragment BOOL followed by a 32-bit
//!   row count and optional rows (`BYTE`, `CExoString`, eight `FLOAT`s). The
//!   legacy stream has no equivalent; until a non-empty row capture is proven,
//!   this module emits the exact empty EE branch: `false` plus `INT(0)`.
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
//!   the "valid bits in final fragment byte" count. The decompiled
//!   `MessageReadUnderflow` final check and the driver-only Docks trace agree
//!   that a zero final-bit count reaches the clean state as
//!   `fragments=N/N bits=0/0`, so this module treats zero as a full final
//!   fragment byte when proving final cursor exhaustion.
//! - EE `CNWCArea::LoadArea` performs two post-static-list WORD reads for
//!   zero-count server-side lists that are not present in the legacy stream.
//!   The old driver shim had to synthesize both counts at the client read site;
//!   driver-only mode requires the proxy to insert both zero WORDs in-band.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const AREA_MAJOR: u8 = 0x04;
const AREA_CLIENT_AREA_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const MIN_READ_SIZE: usize = 4;

const CRESREF_TEXT_BYTES: usize = 16;
const AREA_NAME_READ_OFFSET: usize = 44;
const EE_CEXO_STRING_LENGTH_BYTES: usize = 4;
const DIAMOND_SHORT_AREA_NAME_BYTES: usize = 16;
const DIAMOND_LEGACY_AREA_NAME_BYTES: usize = 20;
const DIAMOND_COMPACT_AREA_NAME_BYTES: usize = 14;
const DIAMOND_SHORT_AREA_NAME_TEXT_BYTES: usize =
    DIAMOND_SHORT_AREA_NAME_BYTES - EE_CEXO_STRING_LENGTH_BYTES;
const DIAMOND_FIXED_AREA_NAME_TEXT_BYTES: usize =
    DIAMOND_LEGACY_AREA_NAME_BYTES - EE_CEXO_STRING_LENGTH_BYTES;
const DIAMOND_COMPACT_AREA_NAME_TEXT_BYTES: usize =
    DIAMOND_COMPACT_AREA_NAME_BYTES - EE_CEXO_STRING_LENGTH_BYTES;
const LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END: usize = 96;
const LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END: usize = 100;
const LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END: usize = 104;
const EE_AREA_WIDTH_BYTES_AFTER_NAME_END: usize = 120;
const EE_AREA_HEIGHT_BYTES_AFTER_NAME_END: usize = 124;
const EE_AREA_TILESET_BYTES_AFTER_NAME_END: usize = 128;
const EE_AREA_STATIC_BUILD35_FIRST_INSERT_AFTER_NAME_END: usize = 24;
const EE_AREA_STATIC_BUILD35_SECOND_INSERT_AFTER_NAME_END: usize = 37;
const EE_AREA_STATIC_BUILD35_INSERT_BYTES: usize = 12;
const EE_AREA_STATIC_BUILD35_TOTAL_INSERT_BYTES: usize = EE_AREA_STATIC_BUILD35_INSERT_BYTES * 2;
const EE_AREA_BUILD36_3_EMPTY_TILESET_OPTIONS_BYTES: usize = 4;
const EE_AREA_READ_BUFFER_INSERT_BYTES: usize = EE_AREA_STATIC_BUILD35_TOTAL_INSERT_BYTES
    + EE_AREA_BUILD36_3_EMPTY_TILESET_OPTIONS_BYTES
    + EE_POST_STATIC_LIST_ZERO_WORD_BYTES;
const MAX_REASONABLE_AREA_DIMENSION: u32 = 512;
const MAX_REASONABLE_AREA_TILE_COUNT: u32 = 65_536;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const AREA_PRESENT_USER_BOOL_COUNT: usize = 1;
const LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS: usize = 14;
const EE_AREA_BUILD36_3_TILESET_OPTIONS_BOOL_BIT_INDEX: usize =
    LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS;
const EE_AREA_BUILD36_5_TILE_LOOP_BOOL_BIT_INDEX: usize =
    EE_AREA_BUILD36_3_TILESET_OPTIONS_BOOL_BIT_INDEX + 1;
const EE_AREA_LOAD_PRE_TILE_FRAGMENT_BITS: usize = LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS + 2;

const TRANSITION_INDEX_PAYLOAD_OFFSET: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const START_X_PAYLOAD_OFFSET: usize = TRANSITION_INDEX_PAYLOAD_OFFSET + 4;
const LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET: usize =
    HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 4 + 4 * 4;
const LEGACY_AREA_OBJECT_ID_BYTES: usize = 4;

const EE_POST_STATIC_LIST_ZERO_WORD_BYTES: usize = 4;
const MAX_AREA_POST_TILE_LIST_COUNT: u32 = 4096;
const MAX_AREA_SOUND_RESREFS: u16 = 64;
const AREA_LIGHT_PLACEABLE_ROW_BYTES: usize = 4 + 2 + 3 * 4;
const AREA_STATIC_PLACEABLE_ROW_BYTES: usize = 4 + 2 + 6 * 4;
const RESTYPE_ARE: u16 = 2012;
const RESTYPE_IFO: u16 = 2014;
const RESTYPE_GIT: u16 = 2023;
const MAX_MODULE_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ERF_KEY_COUNT: u32 = 65_536;
const MAX_OBSERVED_MODULE_SCAN_FILES: usize = 512;
const MIN_OBSERVED_MODULE_AREA_MATCHES: usize = 3;
const MAX_GFF_FIELD_COUNT: u32 = 65_536;
const MAX_GFF_STRUCT_COUNT: u32 = 65_536;
const GFF_TYPE_BYTE: u32 = 0;
const GFF_TYPE_DWORD: u32 = 4;
const GFF_TYPE_INT: u32 = 5;
const GFF_TYPE_FLOAT: u32 = 8;
const GFF_TYPE_CEXO_STRING: u32 = 10;
const GFF_TYPE_RESREF: u32 = 11;
const GFF_TYPE_CEXO_LOCSTRING: u32 = 12;
const GFF_TYPE_LIST: u32 = 15;
const AREA_SOUND_X_OFFSET: usize = 40;
const AREA_SOUND_Y_OFFSET: usize = 44;
const AREA_SOUND_Z_OFFSET: usize = 48;
const AREA_SOUND_RESREF_COUNT_OFFSET: usize = 52;
const AREA_SOUND_BASE_BYTES: usize = 54;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaRewriteKind {
    ExactEeAreaNameModeCExoStringBit,
    ExactEeAreaStaticBuild35FloatTriplets,
    ExactEeAreaBuild363EmptyTilesetOptions,
    ExactEeAreaBuild365TileLoopBool,
    ExactEePostStaticListZeroWords,
    LegacyDiamondFixedAreaName,
    LegacyDiamondShortFixedAreaName,
    LegacyDiamondCompactAreaName,
    LegacyDiamondModuleResourceAreaRepair,
    LegacyDiamondCompactPostTileTailRepair,
    LegacyDiamondMissingSquareDimensionsRepair,
    LegacyHgMissingHeightRepair,
    LegacyHgMissingWidthRepair,
    LegacyDiamondSoundCountZeroMeansOneRepair,
    LegacyDiamondStaticPlaceableCountZeroRepair,
    LegacyDiamondStaticPlaceableDirectionNormalize,
    LegacyDiamondIgnoredTrailingStaticPlaceableRowsDropped,
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
    pub tileset_resref: String,
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
    pub width_repaired: bool,
    pub sound_count_zero_one_repairs: u32,
    pub static_placeable_count_zero_repairs: u32,
    pub static_placeable_direction_normalizations: u32,
    pub static_placeable_trailing_rows_dropped: u32,
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
    dialect: AreaStaticDialect,
    area_name_encoding: AreaNameEncoding,
    area_name_length: u32,
    area_name_end_read_offset: usize,
    width_read_offset: usize,
    height_read_offset: usize,
    tileset_read_offset: usize,
    first_tile_read_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AreaStaticDialect {
    Legacy169,
    EeBuild8193StaticHeader,
}

impl Default for AreaStaticDialect {
    fn default() -> Self {
        Self::Legacy169
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AreaNameEncoding {
    CExoString,
    DiamondFixed20,
    DiamondFixed16,
    DiamondCompactFragmented,
}

impl Default for AreaNameEncoding {
    fn default() -> Self {
        Self::CExoString
    }
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

#[derive(Debug, Clone)]
struct AreaExactReadProof {
    read_size: usize,
    read_end: usize,
    fragment_bits_available: usize,
    fragment_bits_consumed: usize,
    transition_count: u32,
    map_pin_count: u32,
    sound_count: u16,
    light_count: u16,
    static_count: u16,
    first_post_static_count: u16,
    second_post_static_count: u16,
}

#[derive(Debug, Clone, Copy, Default)]
struct LegacyAreaSourceTailProof {
    static_count_read_offset: usize,
    static_rows_read_offset: usize,
    static_rows_count: u16,
    zero_static_placeable_rows: u16,
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
    let mut area_resref = fixed_resref_preview(
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

    let mut working_payload = payload.clone();
    let module_resource_area_repair = repair_compact_area_from_module_resource(
        &mut working_payload,
        fragment_offset,
        legacy_area_object_id,
        &static_layout,
    );
    if let Some(resource_info) = module_resource_area_repair.as_ref() {
        area_resref = resource_info.resref.clone();
    }
    let mut tile_scan = scan_area_tile_stream(&working_payload, fragment_offset);
    let square_dimensions_repaired = repair_missing_square_area_dimensions(
        &mut working_payload,
        fragment_offset,
        &mut tile_scan,
    );
    let width_repaired = square_dimensions_repaired
        || repair_missing_area_width(&mut working_payload, fragment_offset, &mut tile_scan);
    let height_repaired = square_dimensions_repaired
        || repair_missing_area_height(&mut working_payload, fragment_offset, &mut tile_scan);
    let diamond_fixed_name_rewritten = matches!(
        static_layout.area_name_encoding,
        AreaNameEncoding::DiamondFixed20 | AreaNameEncoding::DiamondFixed16
    ) && module_resource_area_repair.is_none();
    if diamond_fixed_name_rewritten {
        if !rewrite_diamond_fixed_area_name_to_ee_cexo_string(
            &mut working_payload,
            fragment_offset,
            &static_layout,
        ) {
            tracing::warn!(
                area_resref = %area_resref,
                "Area_ClientArea rewrite skipped: Diamond fixed-name block could not be converted to the EE CExoString branch"
            );
            return None;
        }
        tile_scan = scan_area_tile_stream(&working_payload, fragment_offset);
    }
    let compact_post_tile_tail_repaired =
        module_resource_area_repair
            .as_ref()
            .is_some_and(|resource_info| {
                repair_compact_post_tile_tail_for_ee(
                    &mut working_payload,
                    fragment_offset,
                    &tile_scan,
                    resource_info,
                )
            });
    let (_, _, post_tail_fragment_offset, _) = area_client_area_read_window(&working_payload)?;
    if compact_post_tile_tail_repaired {
        tile_scan = scan_area_tile_stream(&working_payload, post_tail_fragment_offset);
    }
    let sound_count_zero_one_repairs = repair_legacy_zero_sound_counts(
        &mut working_payload,
        post_tail_fragment_offset,
        &tile_scan,
    )
    .unwrap_or(0);
    let static_placeable_trailing_rows_dropped = drop_legacy_zero_static_placeable_trailing_rows(
        &mut working_payload,
        post_tail_fragment_offset,
        &tile_scan,
    )
    .unwrap_or(0);
    let (_, working_read_size, working_fragment_offset, _) =
        area_client_area_read_window(&working_payload)?;
    let working_declared = (HIGH_LEVEL_HEADER_BYTES + working_read_size) as u32;
    if static_placeable_trailing_rows_dropped != 0 {
        tile_scan = scan_area_tile_stream(&working_payload, working_fragment_offset);
    }
    let static_placeable_count_zero_repairs = 0;
    let static_placeable_direction_normalizations = normalize_legacy_static_placeable_directions(
        &mut working_payload,
        working_fragment_offset,
        &tile_scan,
    )
    .unwrap_or(0);
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
        AreaRewriteKind::ExactEeAreaNameModeCExoStringBit,
        AreaRewriteKind::ExactEeAreaStaticBuild35FloatTriplets,
        AreaRewriteKind::ExactEeAreaBuild363EmptyTilesetOptions,
        AreaRewriteKind::ExactEeAreaBuild365TileLoopBool,
        AreaRewriteKind::ExactEePostStaticListZeroWords,
    ];
    if height_repaired {
        rewrite_kinds.push(AreaRewriteKind::LegacyHgMissingHeightRepair);
    }
    if width_repaired {
        rewrite_kinds.push(AreaRewriteKind::LegacyHgMissingWidthRepair);
    }
    match static_layout.area_name_encoding {
        AreaNameEncoding::DiamondFixed20 => {
            rewrite_kinds.push(AreaRewriteKind::LegacyDiamondFixedAreaName);
        }
        AreaNameEncoding::DiamondFixed16 => {
            rewrite_kinds.push(AreaRewriteKind::LegacyDiamondShortFixedAreaName);
        }
        _ => {}
    }
    if module_resource_area_repair.is_some() {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondCompactAreaName);
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondModuleResourceAreaRepair);
    }
    if compact_post_tile_tail_repaired {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondCompactPostTileTailRepair);
    }
    if square_dimensions_repaired {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondMissingSquareDimensionsRepair);
    }
    if sound_count_zero_one_repairs != 0 {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondSoundCountZeroMeansOneRepair);
    }
    if static_placeable_count_zero_repairs != 0 {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondStaticPlaceableCountZeroRepair);
    }
    if static_placeable_direction_normalizations != 0 {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondStaticPlaceableDirectionNormalize);
    }
    if static_placeable_trailing_rows_dropped != 0 {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondIgnoredTrailingStaticPlaceableRowsDropped);
    }

    let old_fragment_byte = working_payload[working_fragment_offset];
    let rewritten_fragment =
        rewrite_area_fragment_bits(&working_payload[working_fragment_offset..])?;
    let new_fragment_byte = *rewritten_fragment.first()?;
    let static_expanded_read = expand_legacy_area_static_header_for_ee(
        &working_payload,
        working_fragment_offset,
        &tile_scan.layout,
    )?;
    let ee_insert_bytes = EE_AREA_READ_BUFFER_INSERT_BYTES;
    let new_declared = working_declared + ee_insert_bytes as u32;
    let new_read_size = working_read_size + ee_insert_bytes;
    let new_fragment_offset = working_fragment_offset + ee_insert_bytes;
    let placeable_context = collect_area_post_tile_placeable_context(
        &working_payload,
        working_fragment_offset,
        &area_resref,
        legacy_area_object_id,
        static_placeable_count_zero_repairs != 0,
    );
    let placeable_context_valid = placeable_context.is_some();
    let placeable_context = placeable_context.unwrap_or_default();
    let placeable_light_count = placeable_context.light_rows.len();
    let placeable_static_count = placeable_context.static_rows.len();

    let mut rewritten_payload = Vec::with_capacity(new_fragment_offset + rewritten_fragment.len());
    rewritten_payload.extend_from_slice(&static_expanded_read);
    rewritten_payload.extend_from_slice(&[0, 0, 0, 0]);
    rewritten_payload.extend_from_slice(&rewritten_fragment);
    write_u32_le(
        &mut rewritten_payload,
        HIGH_LEVEL_HEADER_BYTES,
        new_declared,
    )?;
    let Some(rewritten_layout) = area_static_layout(&rewritten_payload, new_fragment_offset) else {
        tracing::warn!(
            area_resref = %area_resref,
            old_declared = declared,
            new_declared,
            old_read_size = read_size,
            new_read_size,
            old_fragment_offset = fragment_offset,
            new_fragment_offset,
            "Area_ClientArea rewrite skipped: rewritten packet does not expose an EE static layout"
        );
        return None;
    };
    let tileset_resref = fixed_resref_preview(
        &rewritten_payload,
        HIGH_LEVEL_HEADER_BYTES + rewritten_layout.tileset_read_offset,
    )
    .unwrap_or_else(|| "<invalid>".to_string());
    if !area_resref_plausible(&tileset_resref) {
        tracing::warn!(
            area_resref = %area_resref,
            tileset_resref = %tileset_resref,
            tileset_read_offset = rewritten_layout.tileset_read_offset,
            old_declared = declared,
            new_declared,
            "Area_ClientArea rewrite skipped: rewritten EE packet has an implausible tileset CResRef"
        );
        return None;
    }
    let Some(exact_proof) = ee_area_client_area_exact_read_proof(&rewritten_payload) else {
        tracing::warn!(
            area_resref = %area_resref,
            tileset_resref = %tileset_resref,
            old_declared = declared,
            new_declared,
            old_read_size = read_size,
            new_read_size,
            old_fragment_offset = fragment_offset,
            new_fragment_offset,
            "Area_ClientArea rewrite skipped: rewritten packet does not satisfy exact EE LoadArea cursor proof"
        );
        return None;
    };
    *payload = rewritten_payload;
    for kind in &rewrite_kinds {
        tracing::info!(
            rewrite_kind = ?kind,
            area_resref = %area_resref,
            tileset_resref = %tileset_resref,
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
            width_repaired,
            sound_count_zero_one_repairs,
            static_placeable_count_zero_repairs,
            static_placeable_direction_normalizations,
            static_placeable_trailing_rows_dropped,
            first_tile_read_offset = rewritten_layout.first_tile_read_offset,
            old_fragment_byte,
            new_fragment_byte,
            post_tile_end = exact_proof.read_end,
            read_limit = exact_proof.read_size,
            fragment_bits_consumed = exact_proof.fragment_bits_consumed,
            fragment_bits_available = exact_proof.fragment_bits_available,
            transition_count = exact_proof.transition_count,
            map_pin_count = exact_proof.map_pin_count,
            sound_count = exact_proof.sound_count,
            light_count = exact_proof.light_count,
            static_count = exact_proof.static_count,
            first_post_static_count = exact_proof.first_post_static_count,
            second_post_static_count = exact_proof.second_post_static_count,
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
        tileset_resref,
        old_fragment_byte,
        new_fragment_byte,
        area_name_length: rewritten_layout.area_name_length,
        area_name_end_read_offset: rewritten_layout.area_name_end_read_offset,
        width_read_offset: rewritten_layout.width_read_offset,
        height_read_offset: rewritten_layout.height_read_offset,
        tileset_read_offset: rewritten_layout.tileset_read_offset,
        first_tile_read_offset: rewritten_layout.first_tile_read_offset,
        width: tile_scan.width,
        packet_height: tile_scan.packet_height,
        inferred_height: tile_scan.inferred_height,
        tile_count: tile_scan.tile_count,
        tile_scan_valid: tile_scan.valid,
        height_repaired,
        width_repaired,
        sound_count_zero_one_repairs,
        static_placeable_count_zero_repairs,
        static_placeable_direction_normalizations,
        static_placeable_trailing_rows_dropped,
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
    if fragment_size == 0 {
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

    let structural_prefix_valid = area_static_layout(payload, fragment_offset)
        .filter(|layout| layout.valid)
        .is_some()
        && scan_area_tile_stream(payload, fragment_offset).valid;
    structural_prefix_valid && ee_area_client_area_exact_read_proof(payload).is_some()
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
    let bits =
        decode_cnw_msb_valid_bits(fragment, EE_AREA_BUILD36_3_TILESET_OPTIONS_BOOL_BIT_INDEX)?;
    let mut payload_bits = bits.get(CNW_FRAGMENT_HEADER_BITS..)?.to_vec();
    let area_name_mode_payload_bit = AREA_PRESENT_USER_BOOL_COUNT;
    let Some(area_name_mode_bit) = payload_bits.get_mut(area_name_mode_payload_bit) else {
        tracing::warn!(
            fragment_size = fragment.len(),
            "Area_ClientArea rewrite skipped: fragment bit stream too short for EE area-name CExoString BOOL"
        );
        return None;
    };
    *area_name_mode_bit = true;

    let tileset_options_payload_bit =
        EE_AREA_BUILD36_3_TILESET_OPTIONS_BOOL_BIT_INDEX.checked_sub(CNW_FRAGMENT_HEADER_BITS)?;
    if tileset_options_payload_bit > payload_bits.len() {
        tracing::warn!(
            tileset_options_payload_bit,
            payload_bits = payload_bits.len(),
            "Area_ClientArea rewrite skipped: fragment bit stream too short for EE build-36.3 tileset-options BOOL insertion"
        );
        return None;
    }
    payload_bits.insert(tileset_options_payload_bit, false);

    let tile_loop_payload_bit =
        EE_AREA_BUILD36_5_TILE_LOOP_BOOL_BIT_INDEX.checked_sub(CNW_FRAGMENT_HEADER_BITS)?;
    if tile_loop_payload_bit > payload_bits.len() {
        tracing::warn!(
            tile_loop_payload_bit,
            payload_bits = payload_bits.len(),
            "Area_ClientArea rewrite skipped: fragment bit stream too short for EE build-36.5 pre-tile BOOL insertion"
        );
        return None;
    }
    payload_bits.insert(tile_loop_payload_bit, false);
    encode_cnw_msb_payload_bits(&payload_bits)
}

fn decode_cnw_msb_valid_bits(fragment: &[u8], min_valid_bits: usize) -> Option<Vec<bool>> {
    let valid_bits = cnw_fragment_consumable_bits(fragment)?;
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

fn cnw_fragment_consumable_bits(fragment: &[u8]) -> Option<usize> {
    let first = *fragment.first()?;
    let final_fragment_bits = ((first & 0xE0) >> 5) as usize;
    if final_fragment_bits == 0 {
        fragment.len().checked_mul(8)
    } else {
        fragment
            .len()
            .checked_sub(1)?
            .checked_mul(8)?
            .checked_add(final_fragment_bits)
    }
}

fn encode_cnw_msb_payload_bits(payload_bits: &[bool]) -> Option<Vec<u8>> {
    let valid_bits = CNW_FRAGMENT_HEADER_BITS.checked_add(payload_bits.len())?;
    let byte_count = valid_bits.checked_add(7)?.checked_div(8)?;
    let mut fragment = vec![0u8; byte_count];
    let final_fragment_bits = valid_bits % 8;

    for header_bit in 0..CNW_FRAGMENT_HEADER_BITS {
        if ((final_fragment_bits >> (CNW_FRAGMENT_HEADER_BITS - 1 - header_bit)) & 1) != 0 {
            set_cnw_msb_bit(&mut fragment, header_bit)?;
        }
    }

    for (payload_bit_index, value) in payload_bits.iter().enumerate() {
        if *value {
            set_cnw_msb_bit(
                &mut fragment,
                CNW_FRAGMENT_HEADER_BITS.checked_add(payload_bit_index)?,
            )?;
        }
    }

    Some(fragment)
}

fn set_cnw_msb_bit(fragment: &mut [u8], bit_index: usize) -> Option<()> {
    let byte = fragment.get_mut(bit_index / 8)?;
    *byte |= 0x80 >> (bit_index % 8);
    Some(())
}

fn fragment_bit(fragment: &[u8], bit_index: usize) -> Option<bool> {
    if bit_index >= cnw_fragment_consumable_bits(fragment)? {
        return None;
    }
    let byte = *fragment.get(bit_index / 8)?;
    Some((byte & (0x80 >> (bit_index % 8))) != 0)
}

fn area_payload_fragment_bits_available(payload: &[u8], fragment_offset: usize) -> Option<usize> {
    cnw_fragment_consumable_bits(payload.get(fragment_offset..)?)
}

fn area_payload_fragment_bit(
    payload: &[u8],
    fragment_offset: usize,
    bit_index: usize,
) -> Option<bool> {
    fragment_bit(payload.get(fragment_offset..)?, bit_index)
}

fn area_static_layout(payload: &[u8], fragment_offset: usize) -> Option<AreaStaticLayout> {
    // The same packet family can appear in two strict shapes inside this
    // bridge:
    //
    // - Legacy 1.69 server packets have width/height/tileset at
    //   name_end+96/+100/+104.
    // - EE-facing packets, after BNVR advertises the proxy-owned build-35
    //   server dialect, must include the two
    //   `ServerSatisfiesBuild(0x2001, 0x23, 0)` float triplets proven in both
    //   EE `PackAreaIntoMessage` and `LoadArea`, moving
    //   width/height/tileset to name_end+120/+124/+128. That dialect also
    //   satisfies build 36.3, so the EE shape additionally carries an empty
    //   tileset-options block between the tileset `CResRef` and the first tile.
    //
    // Prefer the EE-expanded shape when it proves a CResRef at the EE tileset
    // offset; otherwise fall back to the exact legacy shape so the rewrite can
    // parse and repair the source packet before emitting an EE packet.
    if let Some((area_name_length, name_end)) =
        read_c_exo_string_shape(payload, fragment_offset, AREA_NAME_READ_OFFSET, 1024)
    {
        if let Some(layout) = area_static_layout_for_dialect(
            payload,
            fragment_offset,
            AreaNameEncoding::CExoString,
            area_name_length,
            name_end,
            AreaStaticDialect::EeBuild8193StaticHeader,
            EE_AREA_WIDTH_BYTES_AFTER_NAME_END,
            EE_AREA_HEIGHT_BYTES_AFTER_NAME_END,
            EE_AREA_TILESET_BYTES_AFTER_NAME_END,
        )
        .or_else(|| {
            area_static_layout_for_dialect(
                payload,
                fragment_offset,
                AreaNameEncoding::CExoString,
                area_name_length,
                name_end,
                AreaStaticDialect::Legacy169,
                LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
                LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
                LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
            )
        }) {
            return Some(layout);
        }
    }

    if let Some((area_name_length, name_end)) =
        read_diamond_compact_area_name_shape(payload, fragment_offset)
    {
        if let Some(layout) = area_static_layout_for_dialect(
            payload,
            fragment_offset,
            AreaNameEncoding::DiamondCompactFragmented,
            area_name_length,
            name_end,
            AreaStaticDialect::Legacy169,
            LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
            LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
            LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        ) {
            return Some(layout);
        }
    }

    if let Some((area_name_length, name_end)) =
        read_diamond_short_fixed_area_name_shape(payload, fragment_offset)
    {
        if let Some(layout) = area_static_layout_for_dialect(
            payload,
            fragment_offset,
            AreaNameEncoding::DiamondFixed16,
            area_name_length,
            name_end,
            AreaStaticDialect::EeBuild8193StaticHeader,
            EE_AREA_WIDTH_BYTES_AFTER_NAME_END,
            EE_AREA_HEIGHT_BYTES_AFTER_NAME_END,
            EE_AREA_TILESET_BYTES_AFTER_NAME_END,
        )
        .or_else(|| {
            area_static_layout_for_dialect(
                payload,
                fragment_offset,
                AreaNameEncoding::DiamondFixed16,
                area_name_length,
                name_end,
                AreaStaticDialect::Legacy169,
                LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
                LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
                LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
            )
        }) {
            return Some(layout);
        }
    }

    let (area_name_length, name_end) =
        read_diamond_fixed_area_name_shape(payload, fragment_offset)?;
    area_static_layout_for_dialect(
        payload,
        fragment_offset,
        AreaNameEncoding::DiamondFixed20,
        area_name_length,
        name_end,
        AreaStaticDialect::EeBuild8193StaticHeader,
        EE_AREA_WIDTH_BYTES_AFTER_NAME_END,
        EE_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        EE_AREA_TILESET_BYTES_AFTER_NAME_END,
    )
    .or_else(|| {
        area_static_layout_for_dialect(
            payload,
            fragment_offset,
            AreaNameEncoding::DiamondFixed20,
            area_name_length,
            name_end,
            AreaStaticDialect::Legacy169,
            LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
            LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
            LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        )
    })
}

fn area_static_layout_for_dialect(
    payload: &[u8],
    fragment_offset: usize,
    area_name_encoding: AreaNameEncoding,
    area_name_length: u32,
    name_end: usize,
    dialect: AreaStaticDialect,
    width_after_name_end: usize,
    height_after_name_end: usize,
    tileset_after_name_end: usize,
) -> Option<AreaStaticLayout> {
    let width_read_offset = name_end.checked_add(width_after_name_end)?;
    let height_read_offset = name_end.checked_add(height_after_name_end)?;
    let tileset_read_offset = name_end.checked_add(tileset_after_name_end)?;
    let tileset_end_read_offset = tileset_read_offset.checked_add(CRESREF_TEXT_BYTES)?;
    let first_tile_read_offset = match dialect {
        AreaStaticDialect::Legacy169 => tileset_end_read_offset,
        AreaStaticDialect::EeBuild8193StaticHeader => {
            if read_area_u32(payload, fragment_offset, tileset_end_read_offset)? != 0 {
                return None;
            }
            tileset_end_read_offset.checked_add(EE_AREA_BUILD36_3_EMPTY_TILESET_OPTIONS_BYTES)?
        }
    };
    if !fixed_cresref_at_read_offset_plausible(payload, fragment_offset, tileset_read_offset) {
        return None;
    }

    Some(AreaStaticLayout {
        valid: true,
        dialect,
        area_name_encoding,
        area_name_length,
        area_name_end_read_offset: name_end,
        width_read_offset,
        height_read_offset,
        tileset_read_offset,
        first_tile_read_offset,
    })
}

fn expand_legacy_area_static_header_for_ee(
    payload: &[u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
) -> Option<Vec<u8>> {
    if layout.dialect != AreaStaticDialect::Legacy169 {
        return None;
    }

    let first_insert_read_offset = layout
        .area_name_end_read_offset
        .checked_add(EE_AREA_STATIC_BUILD35_FIRST_INSERT_AFTER_NAME_END)?;
    let second_insert_read_offset = layout
        .area_name_end_read_offset
        .checked_add(EE_AREA_STATIC_BUILD35_SECOND_INSERT_AFTER_NAME_END)?;
    let tileset_options_count_insert_read_offset =
        layout.tileset_read_offset.checked_add(CRESREF_TEXT_BYTES)?;
    let first_insert_payload_offset =
        HIGH_LEVEL_HEADER_BYTES.checked_add(first_insert_read_offset)?;
    let second_insert_payload_offset =
        HIGH_LEVEL_HEADER_BYTES.checked_add(second_insert_read_offset)?;
    let tileset_options_count_insert_payload_offset =
        HIGH_LEVEL_HEADER_BYTES.checked_add(tileset_options_count_insert_read_offset)?;
    if first_insert_payload_offset > second_insert_payload_offset
        || second_insert_payload_offset > tileset_options_count_insert_payload_offset
        || tileset_options_count_insert_payload_offset > fragment_offset
    {
        return None;
    }

    // EE writer branch 1 (`CNWSArea` offsets 0xAC/0xB0/0xB4) is emitted after
    // the first three environment DWORDs. Branch 2 (0xCC/0xD0/0xD4) is emitted
    // after the B8/BC DWORD pair. Legacy 1.69 has no corresponding fields, so
    // the safest decompile-backed dialect writer emits zero floats at exactly
    // those two writer positions. The build-36.3 tileset-options branch is
    // immediately after the tileset CResRef; with no legacy source rows, the
    // strict EE writer emits the empty `INT(0)` count and the fragment rewrite
    // above inserts the matching false BOOL.
    let mut expanded =
        Vec::with_capacity(fragment_offset + EE_AREA_STATIC_BUILD35_TOTAL_INSERT_BYTES);
    expanded.extend_from_slice(payload.get(..first_insert_payload_offset)?);
    expanded.extend_from_slice(&[0; EE_AREA_STATIC_BUILD35_INSERT_BYTES]);
    expanded
        .extend_from_slice(payload.get(first_insert_payload_offset..second_insert_payload_offset)?);
    expanded.extend_from_slice(&[0; EE_AREA_STATIC_BUILD35_INSERT_BYTES]);
    expanded.extend_from_slice(
        payload.get(second_insert_payload_offset..tileset_options_count_insert_payload_offset)?,
    );
    expanded.extend_from_slice(&[0; EE_AREA_BUILD36_3_EMPTY_TILESET_OPTIONS_BYTES]);
    expanded.extend_from_slice(
        payload.get(tileset_options_count_insert_payload_offset..fragment_offset)?,
    );
    Some(expanded)
}

fn rewrite_diamond_fixed_area_name_to_ee_cexo_string(
    payload: &mut [u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
) -> bool {
    let (fixed_name_bytes, text_bytes) = match layout.area_name_encoding {
        AreaNameEncoding::DiamondFixed20 => (
            DIAMOND_LEGACY_AREA_NAME_BYTES,
            DIAMOND_FIXED_AREA_NAME_TEXT_BYTES,
        ),
        AreaNameEncoding::DiamondFixed16 => (
            DIAMOND_SHORT_AREA_NAME_BYTES,
            DIAMOND_SHORT_AREA_NAME_TEXT_BYTES,
        ),
        _ => return false,
    };
    if layout.area_name_length as usize != fixed_name_bytes
        || layout.area_name_end_read_offset != AREA_NAME_READ_OFFSET + fixed_name_bytes
    {
        return false;
    }

    let payload_offset = HIGH_LEVEL_HEADER_BYTES + AREA_NAME_READ_OFFSET;
    let Some(end) = payload_offset.checked_add(fixed_name_bytes) else {
        return false;
    };
    if end > fragment_offset || end > payload.len() {
        return false;
    }

    // EE `CNWCArea::LoadArea` reads the area-name mode BOOL and, in the true
    // branch, calls `CNWMessage::ReadCExoString(0x20)`: a 32-bit byte length
    // followed by that many bytes. Local 1.69 demo captures carry fixed-width
    // legacy name windows here. Preserve the exact read cursor advance by
    // turning the leading DWORD into the matching fixed-window text length and
    // keeping the remaining legacy bytes as the CExoString payload.
    payload[payload_offset..payload_offset + EE_CEXO_STRING_LENGTH_BYTES]
        .copy_from_slice(&(text_bytes as u32).to_le_bytes());
    true
}

#[derive(Debug, Clone)]
struct ModuleAreaResourceInfo {
    resref: String,
    name: String,
    width: u32,
    height: u32,
    tileset: String,
    tiles: Vec<ModuleAreaTile>,
    map_notes: Vec<ModuleAreaMapNote>,
    sounds: Vec<ModuleAreaSound>,
}

#[derive(Debug, Clone)]
struct ModuleAreaTile {
    tile_id: u32,
    orientation: u32,
    height_raw: u32,
    main_light1: u8,
    main_light2: u8,
    source_light1: u8,
    source_light2: u8,
    anim_loop1: u8,
    anim_loop2: u8,
    anim_loop3: u8,
}

#[derive(Debug, Clone)]
struct ModuleAreaMapNote {
    text: String,
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Clone)]
struct ModuleAreaSound {
    tag: String,
    x: f32,
    y: f32,
    z: f32,
    resrefs: Vec<String>,
}

fn repair_compact_area_from_module_resource(
    payload: &mut [u8],
    fragment_offset: usize,
    legacy_area_object_id: u32,
    layout: &AreaStaticLayout,
) -> Option<ModuleAreaResourceInfo> {
    if layout.dialect != AreaStaticDialect::Legacy169 {
        return None;
    }
    if !matches!(
        layout.area_name_encoding,
        AreaNameEncoding::DiamondCompactFragmented
            | AreaNameEncoding::DiamondFixed20
            | AreaNameEncoding::DiamondFixed16
    ) {
        return None;
    }

    let packet_width = read_area_u32(payload, fragment_offset, layout.width_read_offset)?;
    let packet_height = read_area_u32(payload, fragment_offset, layout.height_read_offset)?;
    let info = module_area_resource_info_for_compact_packet(
        payload,
        fragment_offset,
        legacy_area_object_id,
    )?;
    if info.width == 0
        || info.height == 0
        || info.width > MAX_REASONABLE_AREA_DIMENSION
        || info.height > MAX_REASONABLE_AREA_DIMENSION
        || match info.width.checked_mul(info.height) {
            Some(count) => count > MAX_REASONABLE_AREA_TILE_COUNT,
            None => true,
        }
    {
        return None;
    }
    if (packet_width != 0 && packet_width != info.width)
        || (packet_height != 0 && packet_height != info.height)
    {
        return None;
    }

    write_fixed_resref_payload(
        payload,
        LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES,
        &info.resref,
    )?;
    match layout.area_name_encoding {
        AreaNameEncoding::DiamondCompactFragmented => {
            write_compact_area_name_as_cexo_string(payload, fragment_offset, &info.name)?;
        }
        AreaNameEncoding::DiamondFixed20 => {
            write_fixed_area_name_as_cexo_string(payload, fragment_offset, &info.name)?;
        }
        AreaNameEncoding::DiamondFixed16 => {
            write_short_fixed_area_name_as_cexo_string(payload, fragment_offset, &info.name)?;
        }
        _ => return None,
    }
    write_area_u32(
        payload,
        fragment_offset,
        layout.width_read_offset,
        info.width,
    )?;
    write_area_u32(
        payload,
        fragment_offset,
        layout.height_read_offset,
        info.height,
    )?;
    write_area_fixed_resref(
        payload,
        fragment_offset,
        layout.tileset_read_offset,
        &info.tileset,
    )?;

    let mut repaired_scan = scan_area_tile_stream(payload, fragment_offset);
    if !module_resource_tile_scan_matches(&repaired_scan, &info) {
        write_module_resource_tiles(
            payload,
            fragment_offset,
            layout.first_tile_read_offset,
            &info,
        )?;
        repaired_scan = scan_area_tile_stream(payload, fragment_offset);
    }
    if !module_resource_tile_scan_matches(&repaired_scan, &info) {
        return None;
    }

    tracing::info!(
        area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
        area_resref = info.resref.as_str(),
        area_name = info.name.as_str(),
        width = info.width,
        height = info.height,
        tileset = info.tileset.as_str(),
        "Area_ClientArea compact Diamond area fields repaired from observed Module_Info and local ARE resource"
    );

    Some(info)
}

fn module_resource_tile_scan_matches(
    scan: &AreaTileStreamScan,
    info: &ModuleAreaResourceInfo,
) -> bool {
    scan.valid
        && scan.width == info.width
        && scan.packet_height == info.height
        && scan.tile_count == info.width.saturating_mul(info.height)
}

fn write_module_resource_tiles(
    payload: &mut [u8],
    fragment_offset: usize,
    first_tile_read_offset: usize,
    info: &ModuleAreaResourceInfo,
) -> Option<()> {
    if info.tiles.is_empty()
        || info.tiles.len() != usize::try_from(info.width.checked_mul(info.height)?).ok()?
    {
        return None;
    }
    let mut tile_bytes = Vec::new();
    for tile in &info.tiles {
        write_module_resource_tile_record(&mut tile_bytes, tile)?;
    }
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(first_tile_read_offset)?;
    let end = payload_offset.checked_add(tile_bytes.len())?;
    if end > fragment_offset || end > payload.len() {
        return None;
    }
    payload
        .get_mut(payload_offset..end)?
        .copy_from_slice(&tile_bytes);
    Some(())
}

fn write_module_resource_tile_record(out: &mut Vec<u8>, tile: &ModuleAreaTile) -> Option<()> {
    if tile.tile_id > 65_535 || tile.orientation > 3 {
        return None;
    }
    let signed_height = tile.height_raw as i32 as i64;
    if !(-256..=1024).contains(&signed_height) {
        return None;
    }

    let mut flags = 0x000C | 0x0020 | 0x0040 | 0x0080;
    if tile.main_light1 != 0 {
        flags |= 0x0001;
    }
    if tile.main_light2 != 0 {
        flags |= 0x0002;
    }
    if (flags & 0xFF00) != 0 {
        return None;
    }

    out.extend_from_slice(&tile.tile_id.to_le_bytes());
    out.extend_from_slice(&tile.orientation.to_le_bytes());
    out.extend_from_slice(&tile.height_raw.to_le_bytes());
    out.extend_from_slice(&(flags as u16).to_le_bytes());
    if (flags & 0x0001) != 0 {
        out.push(tile.main_light1);
    }
    if (flags & 0x0002) != 0 {
        out.push(tile.main_light2);
    }
    out.push(module_area_source_light_wire_value(tile.source_light1));
    out.push(module_area_source_light_wire_value(tile.source_light2));
    out.push(tile.anim_loop1);
    out.push(tile.anim_loop2);
    out.push(tile.anim_loop3);
    Some(())
}

fn module_area_source_light_wire_value(value: u8) -> u8 {
    value.saturating_sub(u8::from(value != 0))
}

fn write_compact_area_name_as_cexo_string(
    payload: &mut [u8],
    fragment_offset: usize,
    name: &str,
) -> Option<()> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty()
        || name_bytes.len() > DIAMOND_COMPACT_AREA_NAME_TEXT_BYTES
        || !name_bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }

    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_COMPACT_AREA_NAME_BYTES)?;
    if end > fragment_offset || end > payload.len() {
        return None;
    }
    payload[payload_offset..payload_offset + 4]
        .copy_from_slice(&(DIAMOND_COMPACT_AREA_NAME_TEXT_BYTES as u32).to_le_bytes());
    payload[payload_offset + 4..end].fill(0);
    payload[payload_offset + 4..payload_offset + 4 + name_bytes.len()].copy_from_slice(name_bytes);
    Some(())
}

fn write_short_fixed_area_name_as_cexo_string(
    payload: &mut [u8],
    fragment_offset: usize,
    name: &str,
) -> Option<()> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty()
        || name_bytes.len() > DIAMOND_SHORT_AREA_NAME_TEXT_BYTES
        || !name_bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }

    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let text_start = payload_offset.checked_add(EE_CEXO_STRING_LENGTH_BYTES)?;
    let text_end = text_start.checked_add(DIAMOND_SHORT_AREA_NAME_TEXT_BYTES)?;
    if text_end > fragment_offset || text_end > payload.len() {
        return None;
    }
    payload
        .get_mut(payload_offset..text_start)?
        .copy_from_slice(&(DIAMOND_SHORT_AREA_NAME_TEXT_BYTES as u32).to_le_bytes());
    payload.get_mut(text_start..text_end)?.fill(0);
    payload
        .get_mut(text_start..text_start.checked_add(name_bytes.len())?)?
        .copy_from_slice(name_bytes);
    Some(())
}

fn write_fixed_area_name_as_cexo_string(
    payload: &mut [u8],
    fragment_offset: usize,
    name: &str,
) -> Option<()> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty()
        || name_bytes.len() > DIAMOND_FIXED_AREA_NAME_TEXT_BYTES
        || !name_bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }

    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let text_start = payload_offset.checked_add(EE_CEXO_STRING_LENGTH_BYTES)?;
    let text_end = text_start.checked_add(DIAMOND_FIXED_AREA_NAME_TEXT_BYTES)?;
    if text_end > fragment_offset || text_end > payload.len() {
        return None;
    }
    payload
        .get_mut(payload_offset..text_start)?
        .copy_from_slice(&(DIAMOND_FIXED_AREA_NAME_TEXT_BYTES as u32).to_le_bytes());
    payload.get_mut(text_start..text_end)?.fill(0);
    payload
        .get_mut(text_start..text_start.checked_add(name_bytes.len())?)?
        .copy_from_slice(name_bytes);
    Some(())
}

fn repair_compact_post_tile_tail_for_ee(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
    info: &ModuleAreaResourceInfo,
) -> bool {
    if !scan.valid {
        return false;
    }
    let tail_start = scan.tile_end_read_offset;
    if read_area_u32(payload, fragment_offset, tail_start) != Some(0) {
        return false;
    }

    let mut candidate = payload.clone();
    let Some((sound_count_offset, _map_note_count_consumed)) =
        repair_compact_map_note_tail_for_ee(&mut candidate, fragment_offset, scan, info)
    else {
        return false;
    };
    if repair_compact_sound_tail_for_ee(&mut candidate, fragment_offset, sound_count_offset, info)
        .is_none()
    {
        return false;
    }
    let candidate_scan = scan_area_tile_stream(&candidate, fragment_offset);
    if !candidate_scan.valid || candidate_scan.tile_end_read_offset != scan.tile_end_read_offset {
        return false;
    }
    if legacy_area_source_tail_exact_read_proof(&candidate, fragment_offset, &candidate_scan)
        .is_none()
    {
        return false;
    }

    *payload = candidate;
    true
}

fn repair_compact_map_note_tail_for_ee(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
    info: &ModuleAreaResourceInfo,
) -> Option<(usize, usize)> {
    let note_count = u32::try_from(info.map_notes.len()).ok()?;
    if note_count == 0 || note_count > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }

    let tail_start = scan.tile_end_read_offset;
    write_area_u32(payload, fragment_offset, tail_start, note_count)?;

    let mut cursor = tail_start.checked_add(4)?;
    let mut remaining = (0..info.map_notes.len()).collect::<Vec<_>>();
    let fragment_bits_available = area_payload_fragment_bits_available(payload, fragment_offset)?;
    let mut bit_cursor = LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS;
    for _ in 0..info.map_notes.len() {
        if area_payload_fragment_bit(payload, fragment_offset, bit_cursor)? != true {
            return None;
        }
        bit_cursor = bit_cursor.checked_add(1)?;
        if area_payload_fragment_bit(payload, fragment_offset, bit_cursor)? != false {
            return None;
        }
        bit_cursor = bit_cursor.checked_add(1)?;
        if bit_cursor > fragment_bits_available {
            return None;
        }

        let object_id = read_area_u32(payload, fragment_offset, cursor)?;
        if !legacy_area_object_id_plausible(object_id) {
            return None;
        }
        let x = read_area_f32(payload, fragment_offset, cursor.checked_add(4)?)?;
        let y = read_area_f32(payload, fragment_offset, cursor.checked_add(8)?)?;
        let z = read_area_f32(payload, fragment_offset, cursor.checked_add(12)?)?;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return None;
        }

        let matched_index = remaining
            .iter()
            .copied()
            .filter(|index| {
                let note = &info.map_notes[*index];
                same_f32_bits(x, note.x) && same_f32_bits(y, note.y) && same_f32_bits(z, note.z)
            })
            .collect::<Vec<_>>();
        if matched_index.len() != 1 {
            return None;
        }
        let note_index = matched_index[0];
        let note = &info.map_notes[note_index];
        let text_read_offset = cursor.checked_add(4 + 3 * 4)?;
        if !compact_cexo_string_span_matches_text(
            payload,
            fragment_offset,
            text_read_offset,
            &note.text,
        ) {
            return None;
        }
        write_area_cexo_string_exact(payload, fragment_offset, text_read_offset, &note.text)?;
        cursor = text_read_offset
            .checked_add(EE_CEXO_STRING_LENGTH_BYTES)?
            .checked_add(note.text.len())?;
        remaining.retain(|index| *index != note_index);
    }

    write_area_u32(payload, fragment_offset, cursor, 0)?;
    Some((cursor.checked_add(4)?, info.map_notes.len()))
}

fn repair_compact_sound_tail_for_ee(
    payload: &mut [u8],
    fragment_offset: usize,
    sound_count_offset: usize,
    info: &ModuleAreaResourceInfo,
) -> Option<usize> {
    let sound_count = usize::from(read_area_u16(payload, fragment_offset, sound_count_offset)?);
    if sound_count == 0
        || sound_count != info.sounds.len()
        || sound_count > MAX_AREA_POST_TILE_LIST_COUNT as usize
    {
        return None;
    }

    let mut cursor = sound_count_offset.checked_add(2)?;
    let mut remaining = (0..info.sounds.len()).collect::<Vec<_>>();
    for _ in 0..sound_count {
        let matches = remaining
            .iter()
            .copied()
            .filter(|index| {
                compact_sound_row_matches_resource(
                    payload,
                    fragment_offset,
                    cursor,
                    &info.sounds[*index],
                )
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        let sound_index = matches[0];
        let sound = &info.sounds[sound_index];
        write_sound_row_from_resource(payload, fragment_offset, cursor, sound)?;
        cursor = cursor.checked_add(sound_row_bytes(sound)?)?;
        remaining.retain(|index| *index != sound_index);
    }

    Some(cursor)
}

fn compact_sound_row_matches_resource(
    payload: &[u8],
    fragment_offset: usize,
    row_read_offset: usize,
    sound: &ModuleAreaSound,
) -> bool {
    let Some(row_bytes) = sound_row_bytes(sound) else {
        return false;
    };
    if sound.tag.len() > 64 || !sound.tag.bytes().all(|byte| byte.is_ascii()) {
        return false;
    }
    if HIGH_LEVEL_HEADER_BYTES
        .checked_add(row_read_offset)
        .and_then(|offset| offset.checked_add(row_bytes))
        .is_none_or(|end| end > fragment_offset)
    {
        return false;
    }
    let Some(object_id) = read_area_u32(payload, fragment_offset, row_read_offset) else {
        return false;
    };
    if !legacy_area_object_id_plausible(object_id) {
        return false;
    }
    if read_area_f32(
        payload,
        fragment_offset,
        row_read_offset + AREA_SOUND_X_OFFSET,
    )
    .is_none_or(|value| !same_f32_bits(value, sound.x))
        || !compact_sound_coord_matches_resource(
            payload,
            fragment_offset,
            row_read_offset + AREA_SOUND_Y_OFFSET,
            sound.y,
        )
    {
        return false;
    }

    let source_count = read_area_u16(
        payload,
        fragment_offset,
        row_read_offset + AREA_SOUND_RESREF_COUNT_OFFSET,
    );
    let expected_count = u16::try_from(sound.resrefs.len()).ok();
    if source_count != expected_count && !(source_count == Some(0) && expected_count == Some(1)) {
        return false;
    }
    if !compact_sound_coord_matches_resource(
        payload,
        fragment_offset,
        row_read_offset + AREA_SOUND_Z_OFFSET,
        sound.z,
    ) {
        return false;
    }

    sound.resrefs.iter().enumerate().all(|(index, resref)| {
        let read_offset = row_read_offset + AREA_SOUND_BASE_BYTES + index * CRESREF_TEXT_BYTES;
        fixed_resref_at_read_offset(payload, fragment_offset, read_offset)
            .is_some_and(|source| source.eq_ignore_ascii_case(resref))
    })
}

fn compact_sound_coord_matches_resource(
    payload: &[u8],
    fragment_offset: usize,
    read_offset: usize,
    expected: f32,
) -> bool {
    let Some(payload_offset) = HIGH_LEVEL_HEADER_BYTES.checked_add(read_offset) else {
        return false;
    };
    if payload_offset > fragment_offset || fragment_offset - payload_offset < 4 {
        return false;
    }
    let Some(source) = payload.get(payload_offset..payload_offset + 4) else {
        return false;
    };
    let expected = expected.to_le_bytes();
    source == expected.as_slice()
        || (source.get(0..3) == Some(&expected[0..3]) && source.get(3) == Some(&0))
}

fn write_sound_row_from_resource(
    payload: &mut [u8],
    fragment_offset: usize,
    row_read_offset: usize,
    sound: &ModuleAreaSound,
) -> Option<()> {
    let object_id = read_area_u32(payload, fragment_offset, row_read_offset)?;
    if !legacy_area_object_id_plausible(object_id) {
        return None;
    }
    // The local Diamond row already carries the decompiled sound scalar body.
    // The compact defects proven in these packets are row-local: missing
    // single-resref counts and truncated coordinate high bytes.
    write_area_f32(
        payload,
        fragment_offset,
        row_read_offset + AREA_SOUND_Y_OFFSET,
        sound.y,
    )?;
    write_area_f32(
        payload,
        fragment_offset,
        row_read_offset + AREA_SOUND_Z_OFFSET,
        sound.z,
    )?;
    write_area_u16(
        payload,
        fragment_offset,
        row_read_offset + AREA_SOUND_RESREF_COUNT_OFFSET,
        u16::try_from(sound.resrefs.len()).ok()?,
    )?;
    for (index, resref) in sound.resrefs.iter().enumerate() {
        write_area_fixed_resref(
            payload,
            fragment_offset,
            row_read_offset + AREA_SOUND_BASE_BYTES + index * CRESREF_TEXT_BYTES,
            resref,
        )?;
    }
    Some(())
}

fn sound_row_bytes(sound: &ModuleAreaSound) -> Option<usize> {
    if sound.resrefs.is_empty() || sound.resrefs.len() > MAX_AREA_SOUND_RESREFS as usize {
        return None;
    }
    AREA_SOUND_BASE_BYTES.checked_add(sound.resrefs.len().checked_mul(CRESREF_TEXT_BYTES)?)
}

fn compact_cexo_string_span_matches_text(
    payload: &[u8],
    fragment_offset: usize,
    read_offset: usize,
    text: &str,
) -> bool {
    if text.is_empty() || text.len() > 4096 {
        return false;
    }
    let Some(declared) = read_area_u32(payload, fragment_offset, read_offset) else {
        return false;
    };
    if declared != 0 && declared as usize != text.len() {
        return false;
    }
    let Some(payload_offset) = HIGH_LEVEL_HEADER_BYTES.checked_add(read_offset) else {
        return false;
    };
    let Some(end) = payload_offset
        .checked_add(EE_CEXO_STRING_LENGTH_BYTES)
        .and_then(|offset| offset.checked_add(text.len()))
    else {
        return false;
    };
    if end > fragment_offset || end > payload.len() {
        return false;
    }
    // Compact local rows appear in two proven storage forms: a zero length
    // field followed by the fixed-width compact text window, or a CExoString
    // length whose payload is still fragmented ASCII. Both consume the same
    // byte window that the exact EE writer overwrites below.
    let Some(text_bytes) = payload.get(payload_offset + EE_CEXO_STRING_LENGTH_BYTES..end) else {
        return false;
    };
    let Some(fragments) = compact_fragmented_ascii_runs_allowing_singletons(text_bytes) else {
        return false;
    };
    compact_fragments_match_allowing_singletons(text, &fragments)
}

fn write_area_cexo_string_exact(
    payload: &mut [u8],
    fragment_offset: usize,
    read_offset: usize,
    text: &str,
) -> Option<()> {
    if text.is_empty() || text.len() > 4096 || !text.bytes().all(|byte| byte.is_ascii()) {
        return None;
    }
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(read_offset)?;
    let text_start = payload_offset.checked_add(EE_CEXO_STRING_LENGTH_BYTES)?;
    let end = text_start.checked_add(text.len())?;
    if end > fragment_offset || end > payload.len() {
        return None;
    }
    payload
        .get_mut(payload_offset..text_start)?
        .copy_from_slice(&(u32::try_from(text.len()).ok()?).to_le_bytes());
    payload
        .get_mut(text_start..end)?
        .copy_from_slice(text.as_bytes());
    Some(())
}

fn compact_fragmented_ascii_runs_allowing_singletons(bytes: &[u8]) -> Option<Vec<String>> {
    let mut fragments = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor] == 0 {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != 0 {
            let byte = bytes[cursor];
            if !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b' ') {
                return None;
            }
            cursor += 1;
        }
        if cursor > start {
            let fragment = String::from_utf8_lossy(bytes.get(start..cursor)?).to_string();
            if fragment.bytes().any(|byte| byte.is_ascii_alphabetic()) {
                fragments.push(fragment);
            }
        }
    }

    (!fragments.is_empty()).then_some(fragments)
}

fn compact_fragments_match_allowing_singletons(target: &str, fragments: &[String]) -> bool {
    let target = normalized_resource_text(target);
    if target.is_empty() || fragments.is_empty() {
        return false;
    }

    let mut cursor = 0usize;
    let mut matched = 0usize;
    for fragment in fragments {
        let fragment = normalized_resource_text(fragment);
        if fragment.is_empty() {
            continue;
        }
        let Some(found) = target[cursor..].find(&fragment) else {
            return false;
        };
        cursor = cursor.saturating_add(found).saturating_add(fragment.len());
        matched = matched.saturating_add(fragment.len());
    }
    matched >= 4.min(target.len())
}

fn scan_area_tile_stream(payload: &[u8], fragment_offset: usize) -> AreaTileStreamScan {
    scan_area_tile_stream_with_policy(payload, fragment_offset, false)
}

fn scan_area_tile_stream_allow_legacy_missing_width(
    payload: &[u8],
    fragment_offset: usize,
) -> AreaTileStreamScan {
    scan_area_tile_stream_with_policy(payload, fragment_offset, true)
}

fn scan_area_tile_stream_with_policy(
    payload: &[u8],
    fragment_offset: usize,
    allow_legacy_missing_width: bool,
) -> AreaTileStreamScan {
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
    if width > MAX_REASONABLE_AREA_DIMENSION
        || (width == 0
            && (!allow_legacy_missing_width
                || packet_height == 0
                || packet_height > MAX_REASONABLE_AREA_DIMENSION))
    {
        return AreaTileStreamScan {
            layout,
            width,
            packet_height,
            ..AreaTileStreamScan::default()
        };
    }

    if width != 0 && packet_height != 0 {
        let Some(expected_tile_count) = width.checked_mul(packet_height) else {
            return AreaTileStreamScan {
                layout,
                width,
                packet_height,
                ..AreaTileStreamScan::default()
            };
        };
        if expected_tile_count == 0 || expected_tile_count > MAX_REASONABLE_AREA_TILE_COUNT {
            return AreaTileStreamScan {
                layout,
                width,
                packet_height,
                ..AreaTileStreamScan::default()
            };
        }

        // Both Diamond and EE `CNWCArea::LoadArea` are dimension driven: once
        // width and height are known, the reader consumes exactly
        // `width * height` tile records before the transition/map/sound/light
        // lists. Do not scan past that boundary looking for "more tiles";
        // post-tile rows can be binary-plausible enough to create false
        // records, which then prevents the exact list parser from seeing the
        // true boundary.
        let mut cursor = layout.first_tile_read_offset;
        for _ in 0..expected_tile_count {
            let Some(record_length) = area_tile_record_length_at(payload, fragment_offset, cursor)
            else {
                return AreaTileStreamScan {
                    layout,
                    width,
                    packet_height,
                    tile_count: expected_tile_count,
                    tile_end_read_offset: cursor,
                    ..AreaTileStreamScan::default()
                };
            };
            if record_length == 0 {
                return AreaTileStreamScan {
                    layout,
                    width,
                    packet_height,
                    tile_count: expected_tile_count,
                    tile_end_read_offset: cursor,
                    ..AreaTileStreamScan::default()
                };
            }
            let Some(next_cursor) = cursor.checked_add(record_length) else {
                return AreaTileStreamScan {
                    layout,
                    width,
                    packet_height,
                    tile_count: expected_tile_count,
                    tile_end_read_offset: cursor,
                    ..AreaTileStreamScan::default()
                };
            };
            cursor = next_cursor;
        }

        return AreaTileStreamScan {
            valid: true,
            layout,
            width,
            packet_height,
            inferred_height: packet_height,
            tile_count: expected_tile_count,
            tile_end_read_offset: cursor,
        };
    }

    let mut cursor = layout.first_tile_read_offset;
    let mut tile_count = 0u32;
    while tile_count < MAX_REASONABLE_AREA_TILE_COUNT {
        let Some(record_length) = area_tile_record_length_at(payload, fragment_offset, cursor)
        else {
            break;
        };
        if record_length == 0 {
            break;
        }
        tile_count += 1;
        cursor += record_length;
    }

    if tile_count == 0 || tile_count >= MAX_REASONABLE_AREA_TILE_COUNT {
        return AreaTileStreamScan {
            layout,
            width,
            packet_height,
            tile_count,
            tile_end_read_offset: cursor,
            ..AreaTileStreamScan::default()
        };
    }

    let (effective_width, inferred_height) = if width == 0 {
        if tile_count % packet_height != 0 {
            return AreaTileStreamScan {
                layout,
                width,
                packet_height,
                tile_count,
                tile_end_read_offset: cursor,
                ..AreaTileStreamScan::default()
            };
        }
        let inferred_width = tile_count / packet_height;
        (inferred_width, packet_height)
    } else {
        if tile_count % width != 0 {
            return AreaTileStreamScan {
                layout,
                width,
                packet_height,
                tile_count,
                tile_end_read_offset: cursor,
                ..AreaTileStreamScan::default()
            };
        }
        (width, tile_count / width)
    };
    if effective_width == 0 || effective_width > MAX_REASONABLE_AREA_DIMENSION {
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
        width: effective_width,
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
    legacy_area_object_id: u32,
    filter_ambiguous_context_rows: bool,
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
        if area_placeable_context_id_is_ambiguous(
            object_id,
            legacy_area_object_id,
            filter_ambiguous_context_rows,
            &light_rows,
            &[],
        ) {
            tracing::debug!(
                area_resref,
                object_id,
                legacy_area_object_id,
                "Area_ClientArea placeable context retained area-alias light-placeable row identity"
            );
        }
        light_rows.push(AreaPlaceableContextRow {
            object_id,
            appearance,
            x,
            y,
            z,
            has_direction: false,
            ..AreaPlaceableContextRow::default()
        });
        cursor = cursor.checked_add(AREA_LIGHT_PLACEABLE_ROW_BYTES)?;
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
        if area_placeable_context_id_is_ambiguous(
            object_id,
            legacy_area_object_id,
            filter_ambiguous_context_rows,
            &light_rows,
            &static_rows,
        ) {
            tracing::debug!(
                area_resref,
                object_id,
                legacy_area_object_id,
                "Area_ClientArea placeable context retained area-alias static-placeable row identity"
            );
        }
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

fn area_placeable_context_id_is_ambiguous(
    object_id: u32,
    legacy_area_object_id: u32,
    filter_ambiguous_context_rows: bool,
    light_rows: &[AreaPlaceableContextRow],
    static_rows: &[AreaPlaceableContextRow],
) -> bool {
    // EE `CNWSArea::PackAreaIntoMessage` first counts runtime placeables with
    // light/static state and then writes every row that passes the same runtime
    // check as `OBJECTID + WORD + FLOAT...`.  Local `bw167demo` captures prove
    // that legacy rows inferred from a zero-count Diamond tail can legitimately
    // reuse the area object's id for more than one static row.  The proxy-side
    // context is only a wire-derived hint for later semantic translators, so it
    // must retain the rows exactly as the decompiled sender/reader shape does.
    // Ambiguous identities stay visible but are logged as diagnostic context
    // when the row came from a compatibility repair.
    if !filter_ambiguous_context_rows {
        return false;
    }

    object_id == legacy_area_object_id
        || light_rows.iter().any(|row| row.object_id == object_id)
        || static_rows.iter().any(|row| row.object_id == object_id)
}

fn legacy_area_source_tail_consumes_read_buffer(
    payload: &[u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
) -> bool {
    legacy_area_source_tail_exact_read_proof(payload, fragment_offset, scan).is_some()
}

fn legacy_area_source_tail_exact_read_proof(
    payload: &[u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
) -> Option<LegacyAreaSourceTailProof> {
    let (_, read_size, _, fragment_size) = area_client_area_read_window(payload)?;
    if fragment_size == 0 || !scan.valid {
        return None;
    }
    let fragment_bits_available = area_payload_fragment_bits_available(payload, fragment_offset)?;

    // This is the legacy-side counterpart to the EE `LoadArea` proof below.
    // The source packet has not yet gained the two EE post-static zero WORDs,
    // but the decompiled reader still reaches the post-tile lists with the
    // same fragment cursor: fixed fragment-header bits, Area_ClientArea's
    // area-present BOOL, the later forced EE area-name BOOL position, and the
    // static/environment BOOL reads before tiles.
    let mut bit_cursor = LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS;
    if bit_cursor > fragment_bits_available {
        return None;
    }

    let mut cursor = scan.tile_end_read_offset;

    let transition_count = read_area_u32(payload, fragment_offset, cursor)?;
    if transition_count > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    for _ in 0..transition_count {
        let object_id = read_area_u32(payload, fragment_offset, cursor)?;
        if !legacy_area_object_id_plausible(object_id) {
            return None;
        }
        for component in 0..3 {
            let value = read_area_f32(payload, fragment_offset, cursor + 4 + component * 4)?;
            if !value.is_finite() || value.abs() > 100_000.0 {
                return None;
            }
        }

        area_payload_fragment_bit(payload, fragment_offset, bit_cursor)?;
        bit_cursor = bit_cursor.checked_add(1)?;
        let client_tlk = area_payload_fragment_bit(payload, fragment_offset, bit_cursor)?;
        bit_cursor = bit_cursor.checked_add(1)?;
        let locstring_offset = cursor.checked_add(4 + 3 * 4)?;
        cursor = if client_tlk {
            area_payload_fragment_bit(payload, fragment_offset, bit_cursor)?;
            bit_cursor = bit_cursor.checked_add(1)?;
            read_area_u32(payload, fragment_offset, locstring_offset)?;
            locstring_offset.checked_add(4)?
        } else {
            read_c_exo_string_shape(payload, fragment_offset, locstring_offset, 4096)?.1
        };
    }

    let map_pin_count = read_area_u32(payload, fragment_offset, cursor)?;
    if map_pin_count != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;

    let sound_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(sound_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    for _ in 0..sound_count {
        const AREA_SOUND_RESREF_COUNT_OFFSET: usize = 52;
        const AREA_SOUND_BASE_BYTES: usize = 54;
        let resref_count = read_area_u16(
            payload,
            fragment_offset,
            cursor.checked_add(AREA_SOUND_RESREF_COUNT_OFFSET)?,
        )?;
        if resref_count > MAX_AREA_SOUND_RESREFS {
            return None;
        }
        bit_cursor = bit_cursor.checked_add(6)?;
        if bit_cursor > fragment_bits_available {
            return None;
        }
        cursor = cursor.checked_add(
            AREA_SOUND_BASE_BYTES.checked_add(resref_count as usize * CRESREF_TEXT_BYTES)?,
        )?;
        if HIGH_LEVEL_HEADER_BYTES + cursor > fragment_offset {
            return None;
        }
    }

    let light_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(light_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    for _ in 0..light_count {
        let object_id = read_area_u32(payload, fragment_offset, cursor)?;
        if !legacy_area_object_id_plausible(object_id) {
            return None;
        }
        read_area_u16(payload, fragment_offset, cursor + 4)?;
        for component in 0..3 {
            let value = read_area_f32(payload, fragment_offset, cursor + 6 + component * 4)?;
            if !value.is_finite() || value.abs() > 100_000.0 {
                return None;
            }
        }
        cursor = cursor.checked_add(AREA_LIGHT_PLACEABLE_ROW_BYTES)?;
    }

    let static_count_read_offset = cursor;
    let static_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(static_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    let static_rows_read_offset = cursor;
    let mut static_rows_count = static_count;
    let mut zero_static_placeable_rows = 0u16;
    if static_count == 0 {
        if let Some(row_count) =
            legacy_zero_static_placeable_rows_count(payload, fragment_offset, cursor, read_size)
        {
            zero_static_placeable_rows = row_count;
            static_rows_count = row_count;
            cursor = cursor.checked_add(
                usize::from(row_count).checked_mul(AREA_STATIC_PLACEABLE_ROW_BYTES)?,
            )?;
        }
    } else {
        for _ in 0..static_count {
            if !area_static_placeable_source_row_shape_valid(payload, fragment_offset, cursor) {
                return None;
            }
            cursor = cursor.checked_add(AREA_STATIC_PLACEABLE_ROW_BYTES)?;
        }
    }

    if cursor != read_size || bit_cursor != fragment_bits_available {
        return None;
    }

    Some(LegacyAreaSourceTailProof {
        static_count_read_offset,
        static_rows_read_offset,
        static_rows_count,
        zero_static_placeable_rows,
    })
}

fn legacy_zero_static_placeable_rows_count(
    payload: &[u8],
    fragment_offset: usize,
    rows_read_offset: usize,
    read_size: usize,
) -> Option<u16> {
    if rows_read_offset >= read_size {
        return None;
    }
    let remaining = read_size.checked_sub(rows_read_offset)?;
    if remaining == 0 || remaining % AREA_STATIC_PLACEABLE_ROW_BYTES != 0 {
        return None;
    }
    let row_count = remaining / AREA_STATIC_PLACEABLE_ROW_BYTES;
    if row_count == 0 || row_count > MAX_AREA_POST_TILE_LIST_COUNT as usize {
        return None;
    }
    let row_count = u16::try_from(row_count).ok()?;
    let mut cursor = rows_read_offset;
    for _ in 0..row_count {
        if !area_static_placeable_source_row_shape_valid(payload, fragment_offset, cursor) {
            return None;
        }
        cursor = cursor.checked_add(AREA_STATIC_PLACEABLE_ROW_BYTES)?;
    }
    (cursor == read_size).then_some(row_count)
}

fn area_static_placeable_source_row_shape_valid(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
) -> bool {
    let Some(object_id) = read_area_u32(payload, fragment_offset, cursor) else {
        return false;
    };
    if !legacy_area_object_id_plausible(object_id) {
        return false;
    }
    if read_area_u16(payload, fragment_offset, cursor + 4).is_none() {
        return false;
    }
    for component in 0..6 {
        let Some(value) = read_area_f32(payload, fragment_offset, cursor + 6 + component * 4)
        else {
            return false;
        };
        if !value.is_finite() || value.abs() > 100_000.0 {
            return false;
        }
    }
    true
}

fn area_static_placeable_ee_row_shape_valid(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
) -> bool {
    if !area_static_placeable_source_row_shape_valid(payload, fragment_offset, cursor) {
        return false;
    }
    let Some(dir_x) = read_area_f32(payload, fragment_offset, cursor + 18) else {
        return false;
    };
    let Some(dir_y) = read_area_f32(payload, fragment_offset, cursor + 22) else {
        return false;
    };
    let Some(dir_z) = read_area_f32(payload, fragment_offset, cursor + 26) else {
        return false;
    };
    static_placeable_direction_is_ee_safe(dir_x, dir_y, dir_z)
}

fn normalize_legacy_static_placeable_directions(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
) -> Option<u32> {
    if !scan.valid {
        return Some(0);
    }

    let proof = legacy_area_source_tail_exact_read_proof(payload, fragment_offset, scan)?;
    if proof.static_rows_count == 0 {
        return Some(0);
    }

    let mut normalized = 0u32;
    let mut cursor = proof.static_rows_read_offset;
    for _ in 0..proof.static_rows_count {
        let dir_x = read_area_f32(payload, fragment_offset, cursor + 18)?;
        let dir_y = read_area_f32(payload, fragment_offset, cursor + 22)?;
        let dir_z = read_area_f32(payload, fragment_offset, cursor + 26)?;
        if static_placeable_direction_is_ee_safe(dir_x, dir_y, dir_z) {
            cursor = cursor.checked_add(AREA_STATIC_PLACEABLE_ROW_BYTES)?;
            continue;
        }
        let (new_x, new_y, new_z) =
            normalize_static_placeable_direction_components(dir_x, dir_y, dir_z)?;
        if !same_f32_bits(dir_x, new_x)
            || !same_f32_bits(dir_y, new_y)
            || !same_f32_bits(dir_z, new_z)
        {
            // EE and Diamond both pass this row's direction vector to the
            // static-placeable client object and then derive yaw from that
            // vector. Some Diamond area packets use denormalized vectors that
            // still imply the correct yaw but are unsafe for the EE-side model
            // transform. Preserve the decompiled yaw semantics by writing a
            // canonical horizontal unit vector with the same atan2(-x, y)
            // result before the EE LoadArea reader sees the row.
            write_area_f32(payload, fragment_offset, cursor + 18, new_x)?;
            write_area_f32(payload, fragment_offset, cursor + 22, new_y)?;
            write_area_f32(payload, fragment_offset, cursor + 26, new_z)?;
            normalized = normalized.checked_add(1)?;
        }
        cursor = cursor.checked_add(AREA_STATIC_PLACEABLE_ROW_BYTES)?;
    }

    let repaired_proof = legacy_area_source_tail_exact_read_proof(payload, fragment_offset, scan)?;
    if repaired_proof.static_rows_read_offset != proof.static_rows_read_offset
        || repaired_proof.static_rows_count != proof.static_rows_count
        || repaired_proof.zero_static_placeable_rows != proof.zero_static_placeable_rows
    {
        return None;
    }

    Some(normalized)
}

fn normalize_static_placeable_direction_components(
    dir_x: f32,
    dir_y: f32,
    dir_z: f32,
) -> Option<(f32, f32, f32)> {
    if !dir_x.is_finite()
        || !dir_y.is_finite()
        || !dir_z.is_finite()
        || dir_x.abs() > 100_000.0
        || dir_y.abs() > 100_000.0
        || dir_z.abs() > 100_000.0
    {
        return None;
    }

    let horizontal_len_sq = dir_x.mul_add(dir_x, dir_y * dir_y);
    if !horizontal_len_sq.is_finite() {
        return None;
    }
    if horizontal_len_sq <= 1.0e-12 {
        return Some((0.0, 1.0, 0.0));
    }
    let horizontal_len = horizontal_len_sq.sqrt();
    if !horizontal_len.is_finite() || horizontal_len <= 0.0 {
        return None;
    }
    Some((dir_x / horizontal_len, dir_y / horizontal_len, 0.0))
}

fn static_placeable_direction_is_ee_safe(dir_x: f32, dir_y: f32, dir_z: f32) -> bool {
    if !dir_x.is_finite() || !dir_y.is_finite() || !dir_z.is_finite() {
        return false;
    }
    if dir_z.abs() > 1.0e-4 {
        return false;
    }
    let horizontal_len_sq = dir_x.mul_add(dir_x, dir_y * dir_y);
    horizontal_len_sq.is_finite() && (0.999..=1.001).contains(&horizontal_len_sq)
}

fn same_f32_bits(left: f32, right: f32) -> bool {
    left.to_bits() == right.to_bits()
}

fn ee_area_client_area_exact_read_proof(payload: &[u8]) -> Option<AreaExactReadProof> {
    let (_, read_size, fragment_offset, fragment_size) = area_client_area_read_window(payload)?;
    if fragment_size == 0 {
        return None;
    }
    let fragment = payload.get(fragment_offset..)?;
    let fragment_bits_available = cnw_fragment_consumable_bits(fragment)?;
    let area_name_mode_bit = CNW_FRAGMENT_HEADER_BITS + AREA_PRESENT_USER_BOOL_COUNT;
    if !fragment_bit(fragment, area_name_mode_bit)? {
        return None;
    }

    let scan = scan_area_tile_stream(payload, fragment_offset);
    if !scan.valid {
        return None;
    }
    if scan.layout.dialect != AreaStaticDialect::EeBuild8193StaticHeader {
        return None;
    }
    if scan.layout.area_name_encoding != AreaNameEncoding::CExoString {
        return None;
    }

    let tileset_options_present =
        fragment_bit(fragment, EE_AREA_BUILD36_3_TILESET_OPTIONS_BOOL_BIT_INDEX)?;
    if tileset_options_present {
        return None;
    }
    let tileset_options_count_read_offset = scan
        .layout
        .tileset_read_offset
        .checked_add(CRESREF_TEXT_BYTES)?;
    if read_area_u32(payload, fragment_offset, tileset_options_count_read_offset)? != 0 {
        return None;
    }
    let tile_loop_extra_bool = fragment_bit(fragment, EE_AREA_BUILD36_5_TILE_LOOP_BOOL_BIT_INDEX)?;
    if tile_loop_extra_bool {
        return None;
    }

    // `CNWCArea::LoadArea` reaches tile byte zero at read-buffer offset
    // `first_tile_read_offset` after consuming the legacy pre-tile bits plus
    // the build-36.3 tileset-options BOOL and build-36.5 pre-tile BOOL proven
    // in EE `LoadArea` / `CNWSArea::PackAreaIntoMessage`.
    let mut bit_cursor = EE_AREA_LOAD_PRE_TILE_FRAGMENT_BITS;
    if bit_cursor > fragment_bits_available {
        return None;
    }

    let mut cursor = scan.tile_end_read_offset;

    let transition_count = read_area_u32(payload, fragment_offset, cursor)?;
    if transition_count > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    for _ in 0..transition_count {
        let object_id = read_area_u32(payload, fragment_offset, cursor)?;
        if !legacy_area_object_id_plausible(object_id) {
            return None;
        }
        for component in 0..3 {
            let value = read_area_f32(payload, fragment_offset, cursor + 4 + component * 4)?;
            if !value.is_finite() || value.abs() > 100_000.0 {
                return None;
            }
        }

        // EE writer/reader pair for this list uses one fragment BOOL for the
        // transition-name visibility flag and then the shared
        // `CExoLocStringServer` selector. HG Docks takes the inline
        // `CExoString` branch for each row; the TLK branch is modeled because
        // the decompiled helper has an exact one-bit-plus-DWORD shape.
        area_payload_fragment_bit(payload, fragment_offset, bit_cursor)?;
        bit_cursor = bit_cursor.checked_add(1)?;
        let client_tlk = area_payload_fragment_bit(payload, fragment_offset, bit_cursor)?;
        bit_cursor = bit_cursor.checked_add(1)?;
        let locstring_offset = cursor.checked_add(4 + 3 * 4)?;
        cursor = if client_tlk {
            area_payload_fragment_bit(payload, fragment_offset, bit_cursor)?;
            bit_cursor = bit_cursor.checked_add(1)?;
            read_area_u32(payload, fragment_offset, locstring_offset)?;
            locstring_offset.checked_add(4)?
        } else {
            read_c_exo_string_shape(payload, fragment_offset, locstring_offset, 4096)?.1
        };
    }

    let map_pin_count = read_area_u32(payload, fragment_offset, cursor)?;
    if map_pin_count != 0 {
        // The Docks-proven EE/Diamond path has no map pins. Keep this strict
        // until the decompiled non-empty map-pin row is traced and modeled.
        return None;
    }
    cursor = cursor.checked_add(4)?;

    let sound_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(sound_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    for _ in 0..sound_count {
        const AREA_SOUND_RESREF_COUNT_OFFSET: usize = 52;
        const AREA_SOUND_BASE_BYTES: usize = 54;
        let resref_count = read_area_u16(
            payload,
            fragment_offset,
            cursor.checked_add(AREA_SOUND_RESREF_COUNT_OFFSET)?,
        )?;
        if resref_count > MAX_AREA_SOUND_RESREFS {
            return None;
        }
        bit_cursor = bit_cursor.checked_add(6)?;
        if bit_cursor > fragment_bits_available {
            return None;
        }
        cursor = cursor.checked_add(
            AREA_SOUND_BASE_BYTES.checked_add(resref_count as usize * CRESREF_TEXT_BYTES)?,
        )?;
        if HIGH_LEVEL_HEADER_BYTES + cursor > fragment_offset {
            return None;
        }
    }

    let light_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(light_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    for _ in 0..light_count {
        let object_id = read_area_u32(payload, fragment_offset, cursor)?;
        if !legacy_area_object_id_plausible(object_id) {
            return None;
        }
        read_area_u16(payload, fragment_offset, cursor + 4)?;
        for component in 0..3 {
            let value = read_area_f32(payload, fragment_offset, cursor + 6 + component * 4)?;
            if !value.is_finite() || value.abs() > 100_000.0 {
                return None;
            }
        }
        cursor = cursor.checked_add(AREA_LIGHT_PLACEABLE_ROW_BYTES)?;
    }

    let static_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(static_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    for _ in 0..static_count {
        if !area_static_placeable_ee_row_shape_valid(payload, fragment_offset, cursor) {
            return None;
        }
        cursor = cursor.checked_add(4 + 2 + 6 * 4)?;
    }

    let first_post_static_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(first_post_static_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    cursor = cursor.checked_add(first_post_static_count as usize * 2)?;
    if HIGH_LEVEL_HEADER_BYTES + cursor > fragment_offset {
        return None;
    }

    let second_post_static_count = read_area_u16(payload, fragment_offset, cursor)?;
    if second_post_static_count != 0 {
        // EE's area writer finishes these legacy-facing packets with a zero
        // creature/server-side tail count. Model the non-empty branch only
        // after a capture and decompile pass prove the row shape.
        return None;
    }
    cursor = cursor.checked_add(2)?;

    if cursor != read_size || bit_cursor != fragment_bits_available {
        return None;
    }

    Some(AreaExactReadProof {
        read_size,
        read_end: cursor,
        fragment_bits_available,
        fragment_bits_consumed: bit_cursor,
        transition_count,
        map_pin_count,
        sound_count,
        light_count,
        static_count,
        first_post_static_count,
        second_post_static_count,
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

fn repair_missing_area_width(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &mut AreaTileStreamScan,
) -> bool {
    if scan.valid {
        return false;
    }

    let legacy_scan = scan_area_tile_stream_allow_legacy_missing_width(payload, fragment_offset);
    if !legacy_scan.valid || legacy_scan.width == 0 || legacy_scan.packet_height == 0 {
        return false;
    }
    let Some(packet_width) = read_area_u32(
        payload,
        fragment_offset,
        legacy_scan.layout.width_read_offset,
    ) else {
        return false;
    };
    if packet_width != 0 {
        return false;
    }

    // EE and Diamond both read/write the area grid width as the DWORD
    // immediately before the height DWORD and tileset CResRef.  HG `voyage`
    // captures have that decompile-backed field zeroed while the height is
    // present and the tile stream plus post-tile lists validate exactly as a
    // 4x5 grid.  Repair only that narrow legacy encoding: one missing width,
    // non-zero height, and a fully bounded tile scan that proves the inferred
    // width before any EE packet is emitted.
    let width_payload_offset = HIGH_LEVEL_HEADER_BYTES + legacy_scan.layout.width_read_offset;
    if width_payload_offset > fragment_offset || fragment_offset - width_payload_offset < 4 {
        return false;
    }
    if write_u32_le(payload, width_payload_offset, legacy_scan.width).is_none() {
        return false;
    }

    let repaired_scan = scan_area_tile_stream(payload, fragment_offset);
    if !repaired_scan.valid {
        return false;
    }
    *scan = repaired_scan;
    true
}

fn repair_missing_square_area_dimensions(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &mut AreaTileStreamScan,
) -> bool {
    if scan.valid {
        return false;
    }

    let Some(layout) = area_static_layout(payload, fragment_offset) else {
        return false;
    };
    if layout.dialect != AreaStaticDialect::Legacy169
        || layout.area_name_encoding != AreaNameEncoding::DiamondFixed20
    {
        return false;
    }
    let Some(packet_width) = read_area_u32(payload, fragment_offset, layout.width_read_offset)
    else {
        return false;
    };
    let Some(packet_height) = read_area_u32(payload, fragment_offset, layout.height_read_offset)
    else {
        return false;
    };
    if packet_width != 0 || packet_height != 0 {
        return false;
    }

    // Diamond `CNWCArea::LoadArea` reads the width DWORD, height DWORD, then a
    // 16-byte tileset CResRef immediately before the tile loop. Local Diamond
    // server captures for the stock demo module show the legacy fixed-name
    // branch above with both dimension DWORDs zeroed even though the following
    // tile stream is complete and bounded.
    //
    // Do not trust "the longest run of tile-looking records" here: the
    // post-tile lists are also compact binary records and can contain bytes
    // that resemble tile flags. Instead, advance through the decompile-owned
    // tile reader, try only perfect-square prefixes, and keep a candidate only
    // when the normal exact area scanner proves the resulting width, height,
    // tile stream, and post-tile lists consume the declared read buffer.
    let mut cursor = layout.first_tile_read_offset;
    let mut tile_count = 0u32;
    while tile_count < MAX_REASONABLE_AREA_TILE_COUNT {
        let Some(record_length) = area_tile_record_length_at(payload, fragment_offset, cursor)
        else {
            break;
        };
        if record_length == 0 {
            break;
        }
        tile_count = tile_count.saturating_add(1);
        cursor = cursor.saturating_add(record_length);

        let Some(side) = perfect_square_root(tile_count) else {
            continue;
        };
        if side == 0 || side > MAX_REASONABLE_AREA_DIMENSION {
            continue;
        }

        if write_area_u32(payload, fragment_offset, layout.width_read_offset, side).is_none()
            || write_area_u32(payload, fragment_offset, layout.height_read_offset, side).is_none()
        {
            let _ = write_area_u32(payload, fragment_offset, layout.width_read_offset, 0);
            let _ = write_area_u32(payload, fragment_offset, layout.height_read_offset, 0);
            return false;
        }

        let repaired_scan = scan_area_tile_stream(payload, fragment_offset);
        if repaired_scan.valid
            && repaired_scan.width == side
            && repaired_scan.packet_height == side
            && legacy_area_source_tail_consumes_read_buffer(
                payload,
                fragment_offset,
                &repaired_scan,
            )
        {
            *scan = repaired_scan;
            return true;
        }

        let _ = write_area_u32(payload, fragment_offset, layout.width_read_offset, 0);
        let _ = write_area_u32(payload, fragment_offset, layout.height_read_offset, 0);
    }

    false
}

fn perfect_square_root(value: u32) -> Option<u32> {
    if value == 0 {
        return None;
    }
    let mut side = 1u32;
    while side.saturating_mul(side) < value {
        side = side.checked_add(1)?;
    }
    (side.saturating_mul(side) == value).then_some(side)
}

fn repair_legacy_zero_sound_counts(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
) -> Option<u32> {
    if !scan.valid {
        return None;
    }

    // EE `CNWSSoundObject::PackIntoMessage` writes a WORD sound-count at the
    // end of the fixed 54-byte sound-object body, then writes exactly that many
    // fixed `CResRef(16)` entries. Live 1.69 HG Docks packets use the legacy
    // single-entry compact form for some sound rows: the count WORD is zero,
    // but one valid CResRef immediately follows. Driver-only EE cannot consume
    // that shape, so convert only this proven row-local legacy encoding to the
    // exact EE writer shape. Multi-sound rows already carry their true count and
    // are left untouched.
    let fragment = payload.get(fragment_offset..)?;
    let fragment_bits_available = cnw_fragment_consumable_bits(fragment)?;
    let mut bit_cursor = LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS;
    if bit_cursor > fragment_bits_available {
        return None;
    }

    let mut cursor = scan.tile_end_read_offset;
    let transition_count = read_area_u32(payload, fragment_offset, cursor)?;
    if transition_count > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    for _ in 0..transition_count {
        cursor.checked_add(4 + 3 * 4)?;
        bit_cursor = bit_cursor.checked_add(1)?;
        let client_tlk = fragment_bit(fragment, bit_cursor)?;
        bit_cursor = bit_cursor.checked_add(1)?;
        let locstring_offset = cursor.checked_add(4 + 3 * 4)?;
        cursor = if client_tlk {
            bit_cursor = bit_cursor.checked_add(1)?;
            read_area_u32(payload, fragment_offset, locstring_offset)?;
            locstring_offset.checked_add(4)?
        } else {
            read_c_exo_string_shape(payload, fragment_offset, locstring_offset, 4096)?.1
        };
    }

    let map_pin_count = read_area_u32(payload, fragment_offset, cursor)?;
    if map_pin_count != 0 {
        // Keep the repair scoped to the Docks-proven no-map-pin branch already
        // modeled by the exact EE area proof.
        return None;
    }
    cursor = cursor.checked_add(4)?;

    let sound_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(sound_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;

    let mut repairs = 0u32;
    for _ in 0..sound_count {
        const AREA_SOUND_RESREF_COUNT_OFFSET: usize = 52;
        const AREA_SOUND_BASE_BYTES: usize = 54;

        let count_offset = cursor.checked_add(AREA_SOUND_RESREF_COUNT_OFFSET)?;
        let resref_count = read_area_u16(payload, fragment_offset, count_offset)?;
        let effective_count = if resref_count == 0
            && fixed_cresref_at_read_offset_plausible(
                payload,
                fragment_offset,
                cursor.checked_add(AREA_SOUND_BASE_BYTES)?,
            ) {
            write_area_u16(payload, fragment_offset, count_offset, 1)?;
            repairs = repairs.checked_add(1)?;
            1usize
        } else {
            usize::from(resref_count)
        };

        if effective_count > usize::from(MAX_AREA_SOUND_RESREFS) {
            return None;
        }
        cursor = cursor.checked_add(
            AREA_SOUND_BASE_BYTES.checked_add(effective_count.checked_mul(CRESREF_TEXT_BYTES)?)?,
        )?;
        if HIGH_LEVEL_HEADER_BYTES + cursor > fragment_offset {
            return None;
        }
    }

    Some(repairs)
}

fn drop_legacy_zero_static_placeable_trailing_rows(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
) -> Option<u32> {
    if !scan.valid {
        return Some(0);
    }

    let proof = legacy_area_source_tail_exact_read_proof(payload, fragment_offset, scan)?;
    if proof.zero_static_placeable_rows == 0 {
        return Some(0);
    }

    let (_, read_size, _, fragment_size) = area_client_area_read_window(payload)?;
    if fragment_size == 0 || proof.static_rows_read_offset > read_size {
        return None;
    }

    // Diamond and EE area writers both emit a WORD static-placeable count and
    // then loop exactly `count` times, writing `OBJECTID + WORD + 6 FLOAT`s for
    // each static row.  The matching clients therefore do not consume rows when
    // the count is zero.  Local 1.69 captures can still carry row-shaped bytes
    // after a zero count; those bytes are not claimed by the decompiled
    // Area_ClientArea static-placeable reader, so the strict bridge must not
    // "repair" them into semantic rows.  Instead, trim the unclaimed legacy tail
    // before emitting the exact EE packet and keep the drop explicit in the
    // rewrite summary/logs.
    let fragment = payload.get(fragment_offset..)?.to_vec();
    payload.truncate(HIGH_LEVEL_HEADER_BYTES.checked_add(proof.static_rows_read_offset)?);
    payload.extend_from_slice(&fragment);
    let new_declared = (HIGH_LEVEL_HEADER_BYTES + proof.static_rows_read_offset) as u32;
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared)?;

    let new_fragment_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(proof.static_rows_read_offset)?;
    let trimmed_scan = scan_area_tile_stream(payload, new_fragment_offset);
    let trimmed_proof =
        legacy_area_source_tail_exact_read_proof(payload, new_fragment_offset, &trimmed_scan)?;
    if trimmed_proof.zero_static_placeable_rows != 0 || trimmed_proof.static_rows_count != 0 {
        return None;
    }

    Some(u32::from(proof.zero_static_placeable_rows))
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

#[derive(Debug, Clone)]
struct ModuleAreaResourceTable {
    module_name: Option<String>,
    module_resref: Option<String>,
    area_order: Vec<String>,
    areas: Vec<ModuleAreaResourceInfo>,
}

#[derive(Debug, Clone)]
struct ErfResourceEntry {
    resref: String,
    restype: u16,
    offset: usize,
    size: usize,
}

#[derive(Debug, Clone)]
struct GffField {
    field_type: u32,
    label: String,
    data: u32,
}

#[derive(Debug, Clone, Copy)]
struct GffLayout {
    struct_offset: usize,
    struct_count: usize,
    field_offset: usize,
    field_count: usize,
    label_offset: usize,
    label_count: usize,
    field_data_offset: usize,
    field_indices_offset: usize,
    field_indices_count: usize,
    list_indices_offset: usize,
    list_indices_count: usize,
}

fn module_area_resource_info_for_compact_packet(
    payload: &[u8],
    fragment_offset: usize,
    legacy_area_object_id: u32,
) -> Option<ModuleAreaResourceInfo> {
    let context = crate::translate::module::observed_module_context()?;
    let area_indices = context
        .areas
        .iter()
        .enumerate()
        .filter_map(|(index, area)| (area.object_id == legacy_area_object_id).then_some(index))
        .collect::<Vec<_>>();
    if area_indices.is_empty() {
        tracing::debug!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            observed_area_ids = ?context
                .areas
                .iter()
                .map(|area| format!("0x{:08X}:{}", area.object_id, area.name))
                .collect::<Vec<_>>(),
            "Area_ClientArea compact resource repair skipped: area object id was not present in observed Module_Info context"
        );
        return None;
    }
    let compact_fragments = diamond_compact_area_name_fragments(payload, fragment_offset)
        .or_else(|| diamond_short_fixed_area_name_fragments(payload, fragment_offset))
        .or_else(|| diamond_fixed_area_name_fragments(payload, fragment_offset));
    let Some(compact_fragments) = compact_fragments else {
        tracing::debug!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            area_indices = ?area_indices,
            "Area_ClientArea compact resource repair skipped: compact area-name fragments were unavailable"
        );
        return None;
    };
    let module_path = observed_module_file_path(&context)?;
    let table = read_module_area_resource_table(&module_path)?;
    if !module_table_matches_observed_context(&table, &context) {
        tracing::debug!(
            module_path = %module_path.display(),
            module_name = table.module_name.as_deref().unwrap_or(""),
            module_resref = table.module_resref.as_deref().unwrap_or(""),
            observed_module_name = context.localized_name.as_str(),
            observed_module_resref = context.module_resref.as_str(),
            observed_areas = ?context
                .areas
                .iter()
                .map(|area| format!("0x{:08X}:{}", area.object_id, area.name))
                .collect::<Vec<_>>(),
            table_area_order = ?table.area_order,
            "Area_ClientArea compact resource repair skipped: local module table did not match observed Module_Info context"
        );
        return None;
    }

    if area_indices.len() == 1 {
        let area_index = area_indices[0];
        let observed_area_name = context.areas.get(area_index)?.name.as_str();
        if let Some(area_resref) = table.area_order.get(area_index) {
            if let Some(info) = table
                .areas
                .iter()
                .find(|area| area.resref.eq_ignore_ascii_case(area_resref))
                .filter(|info| {
                    area_resource_matches_observed_compact_name(
                        info,
                        observed_area_name,
                        &compact_fragments,
                    )
                })
                .cloned()
            {
                return Some(info);
            }
        }
    }

    // Local compact Module_Info streams are still decompile-shaped area tables,
    // but the observed object-id order can differ from the module IFO area
    // order, and Diamond can reuse the same compact area object id for more
    // than one visible area in the compact stream. Resolve those narrow cases
    // by requiring one exact local ARE resource whose name/resref matches both
    // a same-object-id observed Module_Info name and the current
    // Area_ClientArea packet fragments. Ambiguous matches stay unclaimed so the
    // exact EE validator remains the final authority.
    let mut matches = table
        .areas
        .iter()
        .filter(|info| {
            area_indices.iter().any(|index| {
                context.areas.get(*index).is_some_and(|observed_area| {
                    area_resource_matches_observed_compact_name(
                        info,
                        &observed_area.name,
                        &compact_fragments,
                    )
                })
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn module_table_matches_observed_context(
    table: &ModuleAreaResourceTable,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    table
        .module_name
        .as_deref()
        .is_some_and(|name| same_resource_text(name, &context.localized_name))
        || table
            .module_resref
            .as_deref()
            .is_some_and(|resref| same_resource_text(resref, &context.module_resref))
        || module_table_area_order_matches_observed_context(table, context)
        || module_table_unordered_area_matches_observed_context(table, context)
}

fn area_resource_matches_observed_compact_name(
    info: &ModuleAreaResourceInfo,
    observed_area_name: &str,
    compact_fragments: &[String],
) -> bool {
    let name = normalized_resource_text(&info.name);
    let resref = normalized_resource_text(&info.resref);
    let observed = normalized_resource_text(observed_area_name);
    let observed_fragments = compact_observed_text_fragments(observed_area_name);
    let observed_matches = (!observed.is_empty() && (observed == name || observed == resref))
        || compact_fragments_match(&name, &observed_fragments)
        || compact_fragments_match(&resref, &observed_fragments)
        || compact_fragments_match_allowing_singletons(&name, &observed_fragments)
        || compact_fragments_match_allowing_singletons(&resref, &observed_fragments);
    let packet_matches = compact_fragments_match(&name, compact_fragments)
        || compact_fragments_match(&resref, compact_fragments)
        || compact_fragments_match_allowing_singletons(&name, compact_fragments)
        || compact_fragments_match_allowing_singletons(&resref, compact_fragments);
    observed_matches && packet_matches
}

fn module_table_area_order_matches_observed_context(
    table: &ModuleAreaResourceTable,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    if context.areas.is_empty() || table.area_order.len() != context.areas.len() {
        return false;
    }

    let required_matches = MIN_OBSERVED_MODULE_AREA_MATCHES
        .min(context.areas.len())
        .min(table.area_order.len());
    let mut matches = 0usize;
    for (index, observed_area) in context.areas.iter().enumerate() {
        let Some(area_resref) = table.area_order.get(index) else {
            return false;
        };
        let Some(info) = table
            .areas
            .iter()
            .find(|area| area.resref.eq_ignore_ascii_case(area_resref))
        else {
            continue;
        };
        let observed_fragments = compact_observed_text_fragments(&observed_area.name);
        if area_resource_matches_observed_compact_name(
            info,
            &observed_area.name,
            &observed_fragments,
        ) {
            matches = matches.saturating_add(1);
        }
    }

    matches >= required_matches
}

fn module_table_unordered_area_matches_observed_context(
    table: &ModuleAreaResourceTable,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    if context.areas.is_empty() || table.areas.is_empty() {
        return false;
    }

    let required_matches = MIN_OBSERVED_MODULE_AREA_MATCHES
        .min(context.areas.len())
        .min(table.areas.len());
    let mut matched_table_indices = HashSet::new();
    let mut matches = 0usize;
    for observed_area in &context.areas {
        let observed_fragments = compact_observed_text_fragments(&observed_area.name);
        let matched_indices = table
            .areas
            .iter()
            .enumerate()
            .filter(|(_, info)| {
                area_resource_matches_observed_compact_name(
                    info,
                    &observed_area.name,
                    &observed_fragments,
                )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matched_indices.len() != 1 {
            continue;
        }
        if matched_table_indices.insert(matched_indices[0]) {
            matches = matches.saturating_add(1);
        }
    }

    matches >= required_matches
}

fn compact_observed_text_fragments(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|fragment| !fragment.trim().is_empty())
        .map(|fragment| fragment.trim().to_owned())
        .collect()
}

fn compact_fragments_match(target: &str, fragments: &[String]) -> bool {
    if target.is_empty() || fragments.is_empty() {
        return false;
    }

    let mut cursor = 0usize;
    let mut matched = 0usize;
    for fragment in fragments {
        let fragment = normalized_resource_text(fragment);
        if fragment.len() < 2 {
            continue;
        }
        let Some(found) = target[cursor..].find(&fragment) else {
            return false;
        };
        cursor = cursor.saturating_add(found).saturating_add(fragment.len());
        matched = matched.saturating_add(fragment.len());
    }
    matched >= 4
}

fn same_resource_text(left: &str, right: &str) -> bool {
    normalized_resource_text(left) == normalized_resource_text(right)
}

fn normalized_resource_text(value: &str) -> String {
    value
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect()
}

fn observed_module_file_path(
    context: &crate::translate::module::ObservedModuleContext,
) -> Option<PathBuf> {
    let mut seen = HashSet::new();
    for candidate in observed_module_file_candidates(context) {
        if !seen.insert(path_key(&candidate)) || !candidate.is_file() {
            continue;
        }
        if read_module_area_resource_table(&candidate)
            .as_ref()
            .is_some_and(|table| module_table_matches_observed_context(table, context))
        {
            return Some(candidate);
        }
    }
    None
}

fn observed_module_file_candidates(
    context: &crate::translate::module::ObservedModuleContext,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("NWN_BRIDGE_MODULE_PATH") {
        candidates.push(PathBuf::from(path));
    }

    let mut names = Vec::new();
    if !context.localized_name.trim().is_empty() {
        names.push(context.localized_name.trim().to_owned());
    }
    if !context.module_resref.trim().is_empty() {
        names.push(context.module_resref.trim().to_owned());
    }

    let mut dirs = Vec::new();
    if let Ok(value) = std::env::var("NWN_BRIDGE_MODULE_DIRS") {
        dirs.extend(split_env_list(&value).map(PathBuf::from));
    }
    dirs.extend([
        PathBuf::from(r"C:\NWN\NWN Diamond\modules"),
        PathBuf::from(r"C:\NWN\NWN Diamond\nwm"),
        PathBuf::from("NWN Diamond").join("modules"),
        PathBuf::from("NWN Diamond").join("nwm"),
    ]);

    for dir in &dirs {
        for name in &names {
            candidates.push(dir.join(format!("{name}.mod")));
            candidates.push(dir.join(format!("{name}.nwm")));
            candidates.push(dir.join(name));
        }
    }

    for dir in &dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut scanned = 0usize;
        for entry in entries.flatten() {
            if scanned >= MAX_OBSERVED_MODULE_SCAN_FILES {
                break;
            }
            scanned = scanned.saturating_add(1);
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
                continue;
            };
            if extension.eq_ignore_ascii_case("mod") || extension.eq_ignore_ascii_case("nwm") {
                candidates.push(path);
            }
        }
    }

    candidates
}

fn read_module_area_resource_table(path: &Path) -> Option<ModuleAreaResourceTable> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > MAX_MODULE_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let resources = read_erf_resource_entries(&bytes)?;

    let ifo = resources
        .iter()
        .find(|entry| entry.restype == RESTYPE_IFO)?;
    let ifo_bytes = bytes.get(ifo.offset..ifo.offset.checked_add(ifo.size)?)?;
    let (module_name, module_resref, area_order) = parse_ifo_module_area_list(ifo_bytes)?;
    if area_order.is_empty() {
        return None;
    }

    let mut areas = Vec::new();
    for area_resref in &area_order {
        let Some(entry) = resources.iter().find(|entry| {
            entry.restype == RESTYPE_ARE && entry.resref.eq_ignore_ascii_case(area_resref)
        }) else {
            continue;
        };
        let are_bytes = bytes.get(entry.offset..entry.offset.checked_add(entry.size)?)?;
        if let Some(mut area) = parse_are_resource_info(are_bytes, &entry.resref) {
            if let Some(git_entry) = resources.iter().find(|entry| {
                entry.restype == RESTYPE_GIT && entry.resref.eq_ignore_ascii_case(area_resref)
            }) {
                let git_bytes =
                    bytes.get(git_entry.offset..git_entry.offset.checked_add(git_entry.size)?)?;
                if let Some((map_notes, sounds)) = parse_git_runtime_info(git_bytes) {
                    area.map_notes = map_notes;
                    area.sounds = sounds;
                }
            }
            areas.push(area);
        }
    }
    if areas.is_empty() {
        return None;
    }

    Some(ModuleAreaResourceTable {
        module_name,
        module_resref,
        area_order,
        areas,
    })
}

fn read_erf_resource_entries(bytes: &[u8]) -> Option<Vec<ErfResourceEntry>> {
    if bytes.len() < 32 {
        return None;
    }
    let magic = bytes.get(0..4)?;
    if !matches!(magic, b"MOD " | b"NWM " | b"ERF " | b"HAK ") || bytes.get(4..8)? != b"V1.0" {
        return None;
    }

    let entry_count = read_u32_le(bytes, 16)?;
    let key_list_offset = usize::try_from(read_u32_le(bytes, 24)?).ok()?;
    let resource_list_offset = usize::try_from(read_u32_le(bytes, 28)?).ok()?;
    if entry_count > MAX_ERF_KEY_COUNT
        || key_list_offset >= bytes.len()
        || resource_list_offset >= bytes.len()
    {
        return None;
    }

    let mut entries = Vec::with_capacity(usize::try_from(entry_count).ok()?);
    for index in 0..usize::try_from(entry_count).ok()? {
        let key_offset = key_list_offset.checked_add(index.checked_mul(24)?)?;
        let key = bytes.get(key_offset..key_offset.checked_add(24)?)?;
        let resref = fixed_resref_bytes_to_string(key.get(0..16)?)?;
        let resource_id = usize::try_from(read_u32_le(key, 16)?).ok()?;
        let restype = u16::from_le_bytes([*key.get(20)?, *key.get(21)?]);
        let resource_entry_offset =
            resource_list_offset.checked_add(resource_id.checked_mul(8)?)?;
        let resource_offset = usize::try_from(read_u32_le(bytes, resource_entry_offset)?).ok()?;
        let resource_size = usize::try_from(read_u32_le(bytes, resource_entry_offset + 4)?).ok()?;
        if resource_offset.checked_add(resource_size)? > bytes.len() {
            return None;
        }
        entries.push(ErfResourceEntry {
            resref,
            restype,
            offset: resource_offset,
            size: resource_size,
        });
    }

    Some(entries)
}

fn parse_ifo_module_area_list(
    bytes: &[u8],
) -> Option<(Option<String>, Option<String>, Vec<String>)> {
    let fields = gff_root_fields(bytes)?;
    let module_name = fields
        .iter()
        .find(|field| field.label == "Mod_Name")
        .and_then(|field| gff_locstring_value(bytes, field));
    let module_resref = fields
        .iter()
        .find(|field| field.label == "Mod_Entry_Area")
        .and_then(|field| gff_resref_value(bytes, field));
    let area_list = fields
        .iter()
        .find(|field| field.label == "Mod_Area_list" && field.field_type == GFF_TYPE_LIST)?;
    let area_order = gff_list_structs(bytes, area_list.data)?
        .into_iter()
        .filter_map(|struct_index| {
            gff_struct_fields(bytes, struct_index).and_then(|fields| {
                fields
                    .iter()
                    .find(|field| field.label == "Area_Name")
                    .and_then(|field| gff_resref_value(bytes, field))
            })
        })
        .collect::<Vec<_>>();
    Some((module_name, module_resref, area_order))
}

fn parse_are_resource_info(bytes: &[u8], fallback_resref: &str) -> Option<ModuleAreaResourceInfo> {
    let fields = gff_root_fields(bytes)?;
    let resref = fields
        .iter()
        .find(|field| field.label == "ResRef")
        .and_then(|field| gff_resref_value(bytes, field))
        .unwrap_or_else(|| fallback_resref.to_owned());
    let name = fields
        .iter()
        .find(|field| field.label == "Name")
        .and_then(|field| gff_locstring_value(bytes, field))
        .or_else(|| {
            fields
                .iter()
                .find(|field| field.label == "Tag")
                .and_then(|field| gff_string_value(bytes, field))
        })
        .unwrap_or_else(|| resref.clone());
    let width = fields
        .iter()
        .find(|field| field.label == "Width")
        .and_then(|field| gff_dword_value(field))?;
    let height = fields
        .iter()
        .find(|field| field.label == "Height")
        .and_then(|field| gff_dword_value(field))?;
    let tileset = fields
        .iter()
        .find(|field| field.label == "Tileset")
        .and_then(|field| gff_resref_value(bytes, field))?;
    let tiles = parse_are_tiles(bytes, &fields, width, height)?;
    Some(ModuleAreaResourceInfo {
        resref,
        name,
        width,
        height,
        tileset,
        tiles,
        map_notes: Vec::new(),
        sounds: Vec::new(),
    })
}

fn parse_are_tiles(
    bytes: &[u8],
    root_fields: &[GffField],
    width: u32,
    height: u32,
) -> Option<Vec<ModuleAreaTile>> {
    let Some(tile_list) = gff_field_by_label(root_fields, "Tile_List") else {
        return Some(Vec::new());
    };
    if tile_list.field_type != GFF_TYPE_LIST {
        return None;
    }
    let expected_count = usize::try_from(width.checked_mul(height)?).ok()?;
    if expected_count == 0 || expected_count > MAX_REASONABLE_AREA_TILE_COUNT as usize {
        return None;
    }
    let tile_structs = gff_list_structs(bytes, tile_list.data)?;
    if tile_structs.len() != expected_count {
        return None;
    }

    tile_structs
        .into_iter()
        .map(|struct_index| {
            let fields = gff_struct_fields(bytes, struct_index)?;
            let tile = ModuleAreaTile {
                tile_id: required_gff_u32(&fields, "Tile_ID")?,
                orientation: required_gff_u32(&fields, "Tile_Orientation")?,
                height_raw: required_gff_raw_i32_bits(&fields, "Tile_Height")?,
                main_light1: optional_gff_byte(&fields, "Tile_MainLight1"),
                main_light2: optional_gff_byte(&fields, "Tile_MainLight2"),
                source_light1: optional_gff_byte(&fields, "Tile_SrcLight1"),
                source_light2: optional_gff_byte(&fields, "Tile_SrcLight2"),
                anim_loop1: optional_gff_byte(&fields, "Tile_AnimLoop1"),
                anim_loop2: optional_gff_byte(&fields, "Tile_AnimLoop2"),
                anim_loop3: optional_gff_byte(&fields, "Tile_AnimLoop3"),
            };
            (tile.tile_id <= 65_535 && tile.orientation <= 3).then_some(tile)
        })
        .collect()
}

fn parse_git_runtime_info(bytes: &[u8]) -> Option<(Vec<ModuleAreaMapNote>, Vec<ModuleAreaSound>)> {
    let fields = gff_root_fields(bytes)?;
    Some((
        parse_git_map_notes(bytes, &fields)?,
        parse_git_sounds(bytes, &fields)?,
    ))
}

fn parse_git_map_notes(bytes: &[u8], root_fields: &[GffField]) -> Option<Vec<ModuleAreaMapNote>> {
    let Some(waypoint_list) = gff_field_by_label(root_fields, "WaypointList") else {
        return Some(Vec::new());
    };
    if waypoint_list.field_type != GFF_TYPE_LIST {
        return None;
    }

    let mut notes = Vec::new();
    for struct_index in gff_list_structs(bytes, waypoint_list.data)? {
        let fields = gff_struct_fields(bytes, struct_index)?;
        let has_map_note = gff_field_by_label(&fields, "HasMapNote")
            .and_then(gff_byte_value)
            .unwrap_or(0)
            != 0;
        let map_note_enabled = gff_field_by_label(&fields, "MapNoteEnabled")
            .and_then(gff_byte_value)
            .unwrap_or(0)
            != 0;
        if !has_map_note || !map_note_enabled {
            continue;
        }
        let text = gff_field_by_label(&fields, "MapNote")
            .and_then(|field| gff_locstring_value(bytes, field))?;
        if text.trim().is_empty() || text.len() > 4096 || !text.bytes().all(|byte| byte.is_ascii())
        {
            continue;
        }
        let x = gff_field_by_label(&fields, "XPosition").and_then(gff_float_value)?;
        let y = gff_field_by_label(&fields, "YPosition").and_then(gff_float_value)?;
        let z = gff_field_by_label(&fields, "ZPosition").and_then(gff_float_value)?;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return None;
        }
        notes.push(ModuleAreaMapNote { text, x, y, z });
    }

    Some(notes)
}

fn parse_git_sounds(bytes: &[u8], root_fields: &[GffField]) -> Option<Vec<ModuleAreaSound>> {
    let Some(sound_list) = gff_field_by_label(root_fields, "SoundList") else {
        return Some(Vec::new());
    };
    if sound_list.field_type != GFF_TYPE_LIST {
        return None;
    }

    let mut sounds = Vec::new();
    for struct_index in gff_list_structs(bytes, sound_list.data)? {
        let fields = gff_struct_fields(bytes, struct_index)?;
        let resrefs = parse_git_sound_resrefs(bytes, &fields)?;
        if resrefs.is_empty() || resrefs.len() > MAX_AREA_SOUND_RESREFS as usize {
            return None;
        }
        let sound = ModuleAreaSound {
            tag: gff_field_by_label(&fields, "Tag")
                .and_then(|field| gff_string_value(bytes, field))
                .unwrap_or_default(),
            x: required_gff_float(&fields, "XPosition")?,
            y: required_gff_float(&fields, "YPosition")?,
            z: required_gff_float(&fields, "ZPosition")?,
            resrefs,
        };
        if !sound_floats_finite(&sound) {
            return None;
        }
        sounds.push(sound);
    }

    Some(sounds)
}

fn parse_git_sound_resrefs(bytes: &[u8], fields: &[GffField]) -> Option<Vec<String>> {
    let sounds_list = gff_field_by_label(fields, "Sounds")?;
    if sounds_list.field_type != GFF_TYPE_LIST {
        return None;
    }
    gff_list_structs(bytes, sounds_list.data)?
        .into_iter()
        .map(|struct_index| {
            let fields = gff_struct_fields(bytes, struct_index)?;
            let resref = gff_field_by_label(&fields, "Sound")
                .and_then(|field| gff_resref_value(bytes, field))?;
            area_resref_plausible(&resref).then_some(resref)
        })
        .collect()
}

fn required_gff_float(fields: &[GffField], label: &str) -> Option<f32> {
    gff_field_by_label(fields, label).and_then(gff_float_value)
}

fn required_gff_u32(fields: &[GffField], label: &str) -> Option<u32> {
    gff_field_by_label(fields, label).and_then(gff_dword_value)
}

fn required_gff_raw_i32_bits(fields: &[GffField], label: &str) -> Option<u32> {
    let field = gff_field_by_label(fields, label)?;
    matches!(field.field_type, GFF_TYPE_INT | GFF_TYPE_DWORD).then_some(field.data)
}

fn optional_gff_byte(fields: &[GffField], label: &str) -> u8 {
    gff_field_by_label(fields, label)
        .and_then(gff_byte_value)
        .unwrap_or(0)
}

fn sound_floats_finite(sound: &ModuleAreaSound) -> bool {
    [sound.x, sound.y, sound.z].into_iter().all(f32::is_finite)
}

fn gff_root_fields(bytes: &[u8]) -> Option<Vec<GffField>> {
    let layout = gff_layout(bytes)?;
    gff_struct_fields_with_layout(bytes, &layout, 0)
}

fn gff_struct_fields(bytes: &[u8], struct_index: u32) -> Option<Vec<GffField>> {
    let layout = gff_layout(bytes)?;
    gff_struct_fields_with_layout(bytes, &layout, struct_index)
}

fn gff_struct_fields_with_layout(
    bytes: &[u8],
    layout: &GffLayout,
    struct_index: u32,
) -> Option<Vec<GffField>> {
    let struct_index = usize::try_from(struct_index).ok()?;
    if struct_index >= layout.struct_count {
        return None;
    }
    let struct_offset = layout
        .struct_offset
        .checked_add(struct_index.checked_mul(12)?)?;
    let data = read_u32_le(bytes, struct_offset + 4)?;
    let field_count = read_u32_le(bytes, struct_offset + 8)?;
    if field_count > MAX_GFF_FIELD_COUNT {
        return None;
    }
    let indices = if field_count == 1 {
        vec![data]
    } else {
        let start = usize::try_from(data).ok()?;
        let count = usize::try_from(field_count).ok()?;
        if start.checked_add(count.checked_mul(4)?)? > layout.field_indices_count {
            return None;
        }
        (0..count)
            .map(|index| read_u32_le(bytes, layout.field_indices_offset + start + index * 4))
            .collect::<Option<Vec<_>>>()?
    };

    indices
        .into_iter()
        .map(|field_index| gff_field(bytes, layout, field_index))
        .collect()
}

fn gff_field(bytes: &[u8], layout: &GffLayout, field_index: u32) -> Option<GffField> {
    let field_index = usize::try_from(field_index).ok()?;
    if field_index >= layout.field_count {
        return None;
    }
    let offset = layout
        .field_offset
        .checked_add(field_index.checked_mul(12)?)?;
    let field_type = read_u32_le(bytes, offset)?;
    let label_index = usize::try_from(read_u32_le(bytes, offset + 4)?).ok()?;
    let data = read_u32_le(bytes, offset + 8)?;
    if label_index >= layout.label_count {
        return None;
    }
    let label_offset = layout
        .label_offset
        .checked_add(label_index.checked_mul(16)?)?;
    let label = fixed_resref_bytes_to_string(bytes.get(label_offset..label_offset + 16)?)?;
    Some(GffField {
        field_type,
        label,
        data,
    })
}

fn gff_layout(bytes: &[u8]) -> Option<GffLayout> {
    if bytes.len() < 56 || bytes.get(4..8)? != b"V3.2" {
        return None;
    }
    let magic = bytes.get(0..4)?;
    if !matches!(magic, b"IFO " | b"ARE " | b"GIT " | b"GFF ") {
        return None;
    }
    let struct_offset = usize::try_from(read_u32_le(bytes, 8)?).ok()?;
    let struct_count = usize::try_from(read_u32_le(bytes, 12)?).ok()?;
    let field_offset = usize::try_from(read_u32_le(bytes, 16)?).ok()?;
    let field_count = usize::try_from(read_u32_le(bytes, 20)?).ok()?;
    let label_offset = usize::try_from(read_u32_le(bytes, 24)?).ok()?;
    let label_count = usize::try_from(read_u32_le(bytes, 28)?).ok()?;
    let field_data_offset = usize::try_from(read_u32_le(bytes, 32)?).ok()?;
    let _field_data_count = usize::try_from(read_u32_le(bytes, 36)?).ok()?;
    let field_indices_offset = usize::try_from(read_u32_le(bytes, 40)?).ok()?;
    let field_indices_count = usize::try_from(read_u32_le(bytes, 44)?).ok()?;
    let list_indices_offset = usize::try_from(read_u32_le(bytes, 48)?).ok()?;
    let list_indices_count = usize::try_from(read_u32_le(bytes, 52)?).ok()?;
    if struct_count > MAX_GFF_STRUCT_COUNT as usize
        || field_count > MAX_GFF_FIELD_COUNT as usize
        || struct_offset >= bytes.len()
        || field_offset >= bytes.len()
        || label_offset >= bytes.len()
        || field_data_offset >= bytes.len()
        || field_indices_offset > bytes.len()
        || list_indices_offset > bytes.len()
    {
        return None;
    }
    Some(GffLayout {
        struct_offset,
        struct_count,
        field_offset,
        field_count,
        label_offset,
        label_count,
        field_data_offset,
        field_indices_offset,
        field_indices_count,
        list_indices_offset,
        list_indices_count,
    })
}

fn gff_list_structs(bytes: &[u8], data: u32) -> Option<Vec<u32>> {
    let layout = gff_layout(bytes)?;
    let start = usize::try_from(data).ok()?;
    let count_offset = layout.list_indices_offset.checked_add(start)?;
    let count = usize::try_from(read_u32_le(bytes, count_offset)?).ok()?;
    let entries_offset = count_offset.checked_add(4)?;
    let end = entries_offset.checked_add(count.checked_mul(4)?)?;
    if end
        > layout
            .list_indices_offset
            .checked_add(layout.list_indices_count)?
        || end > bytes.len()
    {
        return None;
    }
    (0..count)
        .map(|index| read_u32_le(bytes, entries_offset + index * 4))
        .collect()
}

fn gff_field_by_label<'a>(fields: &'a [GffField], label: &str) -> Option<&'a GffField> {
    fields.iter().find(|field| field.label == label)
}

fn gff_byte_value(field: &GffField) -> Option<u8> {
    match field.field_type {
        GFF_TYPE_BYTE => u8::try_from(field.data).ok(),
        _ => None,
    }
}

fn gff_dword_value(field: &GffField) -> Option<u32> {
    match field.field_type {
        GFF_TYPE_DWORD => Some(field.data),
        GFF_TYPE_INT if field.data <= i32::MAX as u32 => Some(field.data),
        _ => None,
    }
}

fn gff_float_value(field: &GffField) -> Option<f32> {
    (field.field_type == GFF_TYPE_FLOAT).then_some(f32::from_bits(field.data))
}

fn gff_string_value(bytes: &[u8], field: &GffField) -> Option<String> {
    if field.field_type != GFF_TYPE_CEXO_STRING {
        return None;
    }
    let layout = gff_layout(bytes)?;
    let offset = layout
        .field_data_offset
        .checked_add(usize::try_from(field.data).ok()?)?;
    let len = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    let start = offset.checked_add(4)?;
    let end = start.checked_add(len)?;
    if end > bytes.len() {
        return None;
    }
    Some(String::from_utf8_lossy(bytes.get(start..end)?).to_string())
}

fn gff_resref_value(bytes: &[u8], field: &GffField) -> Option<String> {
    if field.field_type != GFF_TYPE_RESREF {
        return None;
    }
    let layout = gff_layout(bytes)?;
    let offset = layout
        .field_data_offset
        .checked_add(usize::try_from(field.data).ok()?)?;
    let len = usize::from(*bytes.get(offset)?);
    if len > CRESREF_TEXT_BYTES {
        return None;
    }
    let start = offset.checked_add(1)?;
    let end = start.checked_add(len)?;
    if end > bytes.len() {
        return None;
    }
    Some(String::from_utf8_lossy(bytes.get(start..end)?).to_string())
}

fn gff_locstring_value(bytes: &[u8], field: &GffField) -> Option<String> {
    if field.field_type != GFF_TYPE_CEXO_LOCSTRING {
        return None;
    }
    let layout = gff_layout(bytes)?;
    let offset = layout
        .field_data_offset
        .checked_add(usize::try_from(field.data).ok()?)?;
    let _total_size = read_u32_le(bytes, offset)?;
    let _string_ref = read_u32_le(bytes, offset + 4)?;
    let count = usize::try_from(read_u32_le(bytes, offset + 8)?).ok()?;
    let mut cursor = offset.checked_add(12)?;
    let mut first = None;
    for _ in 0..count {
        let _language = read_u32_le(bytes, cursor)?;
        let len = usize::try_from(read_u32_le(bytes, cursor + 4)?).ok()?;
        let start = cursor.checked_add(8)?;
        let end = start.checked_add(len)?;
        if end > bytes.len() {
            return None;
        }
        if first.is_none() {
            first = Some(String::from_utf8_lossy(bytes.get(start..end)?).to_string());
        }
        cursor = end;
    }
    first
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
    // `CNWMessage::ReadCExoString(max)` is length-bounded byte storage, not a
    // printable text validator. HG area transition names can contain embedded
    // NUL bytes in the inline branch, and the EE reader accepts them while
    // advancing the cursor exactly by the declared length.
    Some((length, string_read_offset + length_usize))
}

fn read_diamond_fixed_area_name_shape(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<(u32, usize)> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    if payload_offset > fragment_offset
        || fragment_offset - payload_offset < DIAMOND_LEGACY_AREA_NAME_BYTES
    {
        return None;
    }
    let bytes = payload.get(payload_offset..payload_offset + DIAMOND_LEGACY_AREA_NAME_BYTES)?;
    if !bytes
        .iter()
        .all(|byte| *byte == 0 || (0x20u8..=0x7Eu8).contains(byte))
    {
        return None;
    }
    let non_zero = bytes.iter().any(|byte| *byte != 0);
    if !non_zero {
        return None;
    }

    // Local Diamond demo captures use a legacy twenty-byte fixed area-name
    // window here. The EE writer branch we emit later is `ReadCExoString(0x20)`,
    // so the rewrite preserves this exact read-window length by replacing the
    // first DWORD with a sixteen-byte CExoString length and leaving the
    // remaining bytes as the string storage.
    Some((
        DIAMOND_LEGACY_AREA_NAME_BYTES as u32,
        AREA_NAME_READ_OFFSET + DIAMOND_LEGACY_AREA_NAME_BYTES,
    ))
}

fn read_diamond_short_fixed_area_name_shape(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<(u32, usize)> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    if payload_offset > fragment_offset
        || fragment_offset - payload_offset < DIAMOND_SHORT_AREA_NAME_BYTES
    {
        return None;
    }
    let bytes = payload.get(payload_offset..payload_offset + DIAMOND_SHORT_AREA_NAME_BYTES)?;
    if !bytes
        .iter()
        .all(|byte| *byte == 0 || (0x20u8..=0x7Eu8).contains(byte))
    {
        return None;
    }
    if !bytes.iter().any(|byte| *byte != 0) {
        return None;
    }

    Some((
        DIAMOND_SHORT_AREA_NAME_BYTES as u32,
        AREA_NAME_READ_OFFSET + DIAMOND_SHORT_AREA_NAME_BYTES,
    ))
}

fn read_diamond_compact_area_name_shape(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<(u32, usize)> {
    let fragments = diamond_compact_area_name_fragments(payload, fragment_offset)?;
    let fragment_text_bytes = fragments.iter().map(String::len).sum::<usize>();
    if fragment_text_bytes < 4 {
        return None;
    }
    Some((
        DIAMOND_COMPACT_AREA_NAME_BYTES as u32,
        AREA_NAME_READ_OFFSET + DIAMOND_COMPACT_AREA_NAME_BYTES,
    ))
}

fn diamond_compact_area_name_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_COMPACT_AREA_NAME_BYTES)?;
    if end > fragment_offset || end > payload.len() {
        return None;
    }
    let bytes = payload.get(payload_offset..end)?;
    if bytes.get(..EE_CEXO_STRING_LENGTH_BYTES)? != [0, 0, 0, 0] {
        return None;
    }
    let fragments = compact_fragmented_ascii_runs(bytes.get(EE_CEXO_STRING_LENGTH_BYTES..)?)?;
    (fragments.len() >= 2).then_some(fragments)
}

fn diamond_fixed_area_name_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    read_diamond_fixed_area_name_shape(payload, fragment_offset)?;
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_LEGACY_AREA_NAME_BYTES)?;
    let bytes = payload.get(payload_offset..end)?;
    compact_fragmented_ascii_runs_allowing_singletons(bytes)
}

fn diamond_short_fixed_area_name_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    read_diamond_short_fixed_area_name_shape(payload, fragment_offset)?;
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_SHORT_AREA_NAME_BYTES)?;
    let bytes = payload.get(payload_offset..end)?;
    compact_fragmented_ascii_runs_allowing_singletons(bytes)
}

fn compact_fragmented_ascii_runs(bytes: &[u8]) -> Option<Vec<String>> {
    let mut fragments = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor] == 0 {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != 0 {
            let byte = bytes[cursor];
            if !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b' ') {
                return None;
            }
            cursor += 1;
        }
        if cursor > start {
            let fragment = String::from_utf8_lossy(bytes.get(start..cursor)?).to_string();
            if fragment.bytes().any(|byte| byte.is_ascii_alphabetic()) {
                fragments.push(fragment);
            }
        }
    }

    let fragment_text_bytes = fragments.iter().map(String::len).sum::<usize>();
    (fragment_text_bytes >= 2).then_some(fragments)
}

fn write_area_fixed_resref(
    payload: &mut [u8],
    fragment_offset: usize,
    read_offset: usize,
    value: &str,
) -> Option<()> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(read_offset)?;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < CRESREF_TEXT_BYTES {
        return None;
    }
    write_fixed_resref_payload(payload, payload_offset, value)
}

fn write_fixed_resref_payload(payload: &mut [u8], offset: usize, value: &str) -> Option<()> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > CRESREF_TEXT_BYTES
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }
    let end = offset.checked_add(CRESREF_TEXT_BYTES)?;
    if end > payload.len() {
        return None;
    }
    payload.get_mut(offset..end)?.fill(0);
    payload
        .get_mut(offset..offset + bytes.len())?
        .copy_from_slice(bytes);
    Some(())
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

fn write_area_u32(
    payload: &mut [u8],
    fragment_offset: usize,
    read_offset: usize,
    value: u32,
) -> Option<()> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < 4 {
        return None;
    }
    write_u32_le(payload, payload_offset, value)
}

fn write_area_u16(
    payload: &mut [u8],
    fragment_offset: usize,
    read_offset: usize,
    value: u16,
) -> Option<()> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < 2 {
        return None;
    }
    payload
        .get_mut(payload_offset..payload_offset + 2)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn write_area_f32(
    payload: &mut [u8],
    fragment_offset: usize,
    read_offset: usize,
    value: f32,
) -> Option<()> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < 4 {
        return None;
    }
    payload
        .get_mut(payload_offset..payload_offset + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn fixed_cresref_at_read_offset_plausible(
    payload: &[u8],
    fragment_offset: usize,
    read_offset: usize,
) -> bool {
    fixed_resref_at_read_offset(payload, fragment_offset, read_offset).is_some_and(|resref| {
        !resref.is_empty()
            && resref.len() <= CRESREF_TEXT_BYTES
            && resref
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

fn fixed_resref_at_read_offset(
    payload: &[u8],
    fragment_offset: usize,
    read_offset: usize,
) -> Option<String> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES + read_offset;
    if payload_offset > fragment_offset || fragment_offset - payload_offset < CRESREF_TEXT_BYTES {
        return None;
    }
    fixed_resref_preview(payload, payload_offset)
}

fn start_fields_plausible(payload: &[u8]) -> bool {
    (0..4).all(|index| {
        read_f32_le(payload, START_X_PAYLOAD_OFFSET + index * 4)
            .is_some_and(|value| value.is_finite() && (index == 3 || value.abs() <= 100_000.0))
    })
}

fn fixed_resref_preview(payload: &[u8], offset: usize) -> Option<String> {
    let bytes = payload.get(offset..offset + CRESREF_TEXT_BYTES)?;
    fixed_resref_bytes_to_string(bytes)
}

fn fixed_resref_bytes_to_string(bytes: &[u8]) -> Option<String> {
    if bytes.len() != CRESREF_TEXT_BYTES {
        return None;
    }
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(CRESREF_TEXT_BYTES);
    Some(String::from_utf8_lossy(&bytes[..end]).to_string())
}

fn split_env_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([';', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
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

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static MODULE_CONTEXT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn module_context_test_guard() -> MutexGuard<'static, ()> {
        MODULE_CONTEXT_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("module context test lock poisoned")
    }

    const DOCKS_OF_ASCENSION_LEGACY_MISSING_HEIGHT: &[u8] = include_bytes!(
        "../../fixtures/area/hg_docksofascension_client_area_legacy_missing_height.bin"
    );
    const DOCKS_OF_ASCENSION_LEGACY_ZERO_SOUND_COUNTS: &[u8] = include_bytes!(
        "../../fixtures/area/hg_docksofascension_client_area_legacy_zero_sound_counts.bin"
    );
    const VOYAGE_LEGACY_MISSING_WIDTH: &[u8] =
        include_bytes!("../../fixtures/area/hg_voyage_client_area_legacy_missing_width.bin");
    const LOCAL_DIAMOND_BW167DEMO_FIXED_NAME: &[u8] =
        include_bytes!("../../fixtures/area/local_diamond_bw167demo_client_area_fixed_name.bin");
    const LOCAL_TO_HEIR_CORMANTHOR_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_to_heir_cormanthor_client_area_compact.bin");
    const LOCAL_DARK_RANGER_INN_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_dark_ranger_inn_client_area_compact.bin");
    const LOCAL_WINDS_EREMOR_MOUNT_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_winds_eremor_mount_client_area_compact.bin");

    #[test]
    fn docksofascension_uses_decompile_backed_tile_dimension_offsets() {
        let payload = DOCKS_OF_ASCENSION_LEGACY_MISSING_HEIGHT;
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(payload).expect("fixture read window");
        let layout = area_static_layout(payload, fragment_offset).expect("fixture static layout");

        assert_eq!(layout.dialect, AreaStaticDialect::Legacy169);
        assert_eq!(
            read_area_u32(payload, fragment_offset, layout.width_read_offset),
            Some(11)
        );
        assert_eq!(
            read_area_u32(payload, fragment_offset, layout.height_read_offset),
            Some(0)
        );
        assert_eq!(
            fixed_resref_preview(
                payload,
                HIGH_LEVEL_HEADER_BYTES + layout.tileset_read_offset
            )
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
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyHgMissingHeightRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::ExactEeAreaStaticBuild35FloatTriplets)
        );
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[test]
    fn docksofascension_rewrite_consumes_exact_ee_area_reader_window() {
        let mut payload = DOCKS_OF_ASCENSION_LEGACY_MISSING_HEIGHT.to_vec();
        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("legacy missing-height area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten area should match EE LoadArea cursor proof");
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("rewritten read window");
        let layout =
            area_static_layout(&payload, fragment_offset).expect("rewritten static layout");

        assert_eq!(layout.dialect, AreaStaticDialect::EeBuild8193StaticHeader);
        assert_eq!(
            layout.width_read_offset,
            layout.area_name_end_read_offset + EE_AREA_WIDTH_BYTES_AFTER_NAME_END
        );
        assert_eq!(
            layout.height_read_offset,
            layout.area_name_end_read_offset + EE_AREA_HEIGHT_BYTES_AFTER_NAME_END
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                HIGH_LEVEL_HEADER_BYTES + layout.tileset_read_offset
            )
            .as_deref(),
            Some("ttr01")
        );
        assert_eq!(
            layout.first_tile_read_offset,
            layout.tileset_read_offset
                + CRESREF_TEXT_BYTES
                + EE_AREA_BUILD36_3_EMPTY_TILESET_OPTIONS_BYTES
        );
        assert_eq!(
            read_area_u32(
                &payload,
                fragment_offset,
                layout.tileset_read_offset + CRESREF_TEXT_BYTES
            ),
            Some(0)
        );
        assert_eq!(
            fragment_bit(
                &payload[fragment_offset..],
                EE_AREA_BUILD36_3_TILESET_OPTIONS_BOOL_BIT_INDEX
            ),
            Some(false)
        );
        assert_eq!(
            fragment_bit(
                &payload[fragment_offset..],
                EE_AREA_BUILD36_5_TILE_LOOP_BOOL_BIT_INDEX
            ),
            Some(false)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::ExactEeAreaBuild363EmptyTilesetOptions)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::ExactEeAreaBuild365TileLoopBool)
        );
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert_eq!(proof.transition_count, 8);
        assert_eq!(proof.map_pin_count, 0);
        assert_eq!(proof.sound_count, 11);
        assert_eq!(proof.light_count, 16);
        assert_eq!(proof.static_count, 194);
        assert_eq!(proof.first_post_static_count, 0);
        assert_eq!(proof.second_post_static_count, 0);
        assert_eq!(summary.placeable_light_count, 16);
        assert_eq!(summary.placeable_static_count, 194);
    }

    #[test]
    fn docksofascension_rewrite_repairs_legacy_zero_sound_counts() {
        let mut payload = DOCKS_OF_ASCENSION_LEGACY_ZERO_SOUND_COUNTS.to_vec();
        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("legacy zero-sound-count area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "docksofascension");
        assert_eq!(summary.sound_count_zero_one_repairs, 3);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondSoundCountZeroMeansOneRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::ExactEeAreaStaticBuild35FloatTriplets)
        );
        assert_eq!(proof.sound_count, 11);
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[test]
    fn voyage_rewrite_repairs_legacy_missing_width_and_validates() {
        let mut payload = VOYAGE_LEGACY_MISSING_WIDTH.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let layout = area_static_layout(&payload, fragment_offset).expect("fixture static layout");

        assert_eq!(layout.dialect, AreaStaticDialect::Legacy169);
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.width_read_offset),
            Some(0)
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.height_read_offset),
            Some(5)
        );
        assert!(!scan_area_tile_stream(&payload, fragment_offset).valid);

        let legacy_scan =
            scan_area_tile_stream_allow_legacy_missing_width(&payload, fragment_offset);
        assert!(legacy_scan.valid);
        assert_eq!(legacy_scan.width, 4);
        assert_eq!(legacy_scan.packet_height, 5);
        assert_eq!(legacy_scan.inferred_height, 5);
        assert_eq!(legacy_scan.tile_count, 20);

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("legacy missing-width area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "voyage");
        assert!(summary.tile_scan_valid);
        assert!(summary.width_repaired);
        assert!(!summary.height_repaired);
        assert_eq!(summary.width, 4);
        assert_eq!(summary.packet_height, 5);
        assert_eq!(summary.inferred_height, 5);
        assert_eq!(summary.tile_count, 20);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyHgMissingWidthRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::ExactEeAreaStaticBuild35FloatTriplets)
        );
        assert_eq!(proof.sound_count, 1);
        assert_eq!(proof.light_count, 0);
        assert_eq!(proof.static_count, 3);
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[test]
    fn local_diamond_bw167demo_fixed_name_rewrite_validates() {
        let mut payload = LOCAL_DIAMOND_BW167DEMO_FIXED_NAME.to_vec();
        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("local Diamond fixed-name area rewrite");

        assert_eq!(summary.area_resref, "edmo");
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondFixedAreaName)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondMissingSquareDimensionsRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondIgnoredTrailingStaticPlaceableRowsDropped)
        );
        assert_eq!(summary.width, 8);
        assert_eq!(summary.packet_height, 8);
        assert_eq!(summary.tile_count, 64);
        assert_eq!(summary.static_placeable_count_zero_repairs, 0);
        assert_eq!(summary.static_placeable_direction_normalizations, 0);
        assert_eq!(summary.static_placeable_trailing_rows_dropped, 7);
        // The Diamond and EE decompiles both gate static-placeable rows by the
        // preceding WORD count.  When the legacy packet says that count is zero,
        // row-shaped trailing bytes are unclaimed by the Area_ClientArea
        // semantic reader and must be dropped rather than promoted into static
        // rows.
        assert_eq!(summary.placeable_static_count, 0);
        let proof =
            ee_area_client_area_exact_read_proof(&payload).expect("rewritten local area proof");
        assert_eq!(proof.static_count, 0);
        assert_eq!(proof.first_post_static_count, 0);
        assert_eq!(proof.second_post_static_count, 0);
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("rewritten read window");
        let layout = area_static_layout(&payload, fragment_offset)
            .expect("rewritten EE CExoString area layout");
        assert_eq!(layout.area_name_encoding, AreaNameEncoding::CExoString);
        assert_eq!(
            layout.area_name_length,
            DIAMOND_FIXED_AREA_NAME_TEXT_BYTES as u32
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                HIGH_LEVEL_HEADER_BYTES + layout.tileset_read_offset
            )
            .as_deref(),
            Some("ttr01")
        );
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[test]
    fn local_to_heir_compact_area_uses_module_resource_dimensions() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "To H".to_string(),
                module_resref: "To_H".to_string(),
                areas: vec![
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Cor thor".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0001,
                        name: "D F st".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0002,
                        name: "Cavern Ent".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0003,
                        name: "Dr Caverns".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0004,
                        name: "F Cavern".to_string(),
                    },
                ],
            },
        );

        let mut payload = LOCAL_TO_HEIR_CORMANTHOR_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let source_layout =
            area_static_layout(&payload, fragment_offset).expect("compact static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondCompactFragmented
        );
        assert_eq!(source_layout.area_name_end_read_offset, 58);
        assert_eq!(source_layout.width_read_offset, 154);
        assert_eq!(source_layout.height_read_offset, 158);
        assert_eq!(source_layout.tileset_read_offset, 162);
        assert_eq!(source_layout.first_tile_read_offset, 178);
        assert!(!scan_area_tile_stream(&payload, fragment_offset).valid);

        let context =
            crate::translate::module::observed_module_context().expect("observed module context");
        let compact_fragments = diamond_compact_area_name_fragments(&payload, fragment_offset)
            .expect("compact area-name fragments");
        assert_eq!(
            compact_fragments,
            vec!["Cor".to_string(), "thor".to_string()]
        );
        let module_path =
            observed_module_file_path(&context).expect("observed local module file path");
        let module_table =
            read_module_area_resource_table(&module_path).expect("local module ARE table");
        assert!(module_table_matches_observed_context(
            &module_table,
            &context
        ));
        assert_eq!(
            module_table.area_order.first().map(String::as_str),
            Some("cormanthor")
        );

        let compact_info =
            module_area_resource_info_for_compact_packet(&payload, fragment_offset, 0x8000_0000)
                .expect("observed module context maps compact area packet to local ARE resource");
        assert_eq!(compact_info.resref, "cormanthor");
        assert_eq!(compact_info.name, "Cormanthor");
        assert_eq!(compact_info.width, 8);
        assert_eq!(compact_info.height, 9);
        assert_eq!(compact_info.tileset, "ttf01");
        assert_eq!(compact_info.map_notes.len(), 2);
        assert_eq!(compact_info.sounds.len(), 8);

        let mut staged_source = payload.clone();
        write_fixed_resref_payload(
            &mut staged_source,
            LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES,
            &compact_info.resref,
        )
        .expect("write packet area resref");
        write_compact_area_name_as_cexo_string(
            &mut staged_source,
            fragment_offset,
            &compact_info.name,
        )
        .expect("write compact area name as CExoString");
        write_area_u32(
            &mut staged_source,
            fragment_offset,
            source_layout.width_read_offset,
            compact_info.width,
        )
        .expect("write area width");
        write_area_u32(
            &mut staged_source,
            fragment_offset,
            source_layout.height_read_offset,
            compact_info.height,
        )
        .expect("write area height");
        write_area_fixed_resref(
            &mut staged_source,
            fragment_offset,
            source_layout.tileset_read_offset,
            &compact_info.tileset,
        )
        .expect("write area tileset");
        let staged_scan = scan_area_tile_stream(&staged_source, fragment_offset);
        assert!(staged_scan.valid);
        assert!(repair_compact_post_tile_tail_for_ee(
            &mut staged_source,
            fragment_offset,
            &staged_scan,
            &compact_info,
        ));
        let (_, _, staged_fragment_offset, _) =
            area_client_area_read_window(&staged_source).expect("staged read window");
        let staged_scan = scan_area_tile_stream(&staged_source, staged_fragment_offset);
        assert_eq!(
            repair_legacy_zero_sound_counts(
                &mut staged_source,
                staged_fragment_offset,
                &staged_scan
            ),
            Some(0)
        );
        let staged_proof = legacy_area_source_tail_exact_read_proof(
            &staged_source,
            staged_fragment_offset,
            &staged_scan,
        )
        .expect("staged compact tail repair should validate source cursor");
        assert_eq!(staged_proof.static_rows_count, 6);

        let mut repaired_source = payload.clone();
        repair_compact_area_from_module_resource(
            &mut repaired_source,
            fragment_offset,
            0x8000_0000,
            &source_layout,
        )
        .expect("compact area source fields repaired from local ARE resource");
        let repaired_scan = scan_area_tile_stream(&repaired_source, fragment_offset);
        assert!(repaired_scan.valid);
        assert!(repair_compact_post_tile_tail_for_ee(
            &mut repaired_source,
            fragment_offset,
            &repaired_scan,
            &compact_info,
        ));
        let (_, _, repaired_fragment_offset, _) =
            area_client_area_read_window(&repaired_source).expect("repaired read window");
        let repaired_scan = scan_area_tile_stream(&repaired_source, repaired_fragment_offset);
        assert_eq!(
            repair_legacy_zero_sound_counts(
                &mut repaired_source,
                repaired_fragment_offset,
                &repaired_scan
            ),
            Some(0)
        );
        assert!(legacy_area_source_tail_consumes_read_buffer(
            &repaired_source,
            repaired_fragment_offset,
            &repaired_scan
        ));

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("compact local To Heir area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten compact area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "cormanthor");
        assert_eq!(summary.tileset_resref, "ttf01");
        assert_eq!(summary.width, 8);
        assert_eq!(summary.packet_height, 9);
        assert_eq!(summary.tile_count, 72);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondCompactAreaName)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondModuleResourceAreaRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondCompactPostTileTailRepair)
        );
        assert_eq!(proof.transition_count, 2);
        assert_eq!(proof.sound_count, 8);
        assert_eq!(proof.light_count, 0);
        assert_eq!(proof.static_count, 6);
        assert_eq!(proof.first_post_static_count, 0);
        assert_eq!(proof.second_post_static_count, 0);
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[test]
    fn local_dark_ranger_compact_area_uses_resource_height_when_width_is_present() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "Dark R".to_string(),
                module_resref: "Dark_R".to_string(),
                areas: vec![
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Inn L".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Dead s Marsh".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "T Dark R s Ruins".to_string(),
                    },
                ],
            },
        );

        let mut payload = LOCAL_DARK_RANGER_INN_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let source_layout =
            area_static_layout(&payload, fragment_offset).expect("compact static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondFixed20
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, source_layout.width_read_offset),
            Some(5)
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, source_layout.height_read_offset),
            Some(0)
        );

        let compact_info =
            module_area_resource_info_for_compact_packet(&payload, fragment_offset, 0x8000_0000)
                .expect("observed module context maps compact area packet to local ARE resource");
        assert_eq!(compact_info.resref, "innofthelasthope");
        assert_eq!(compact_info.name, "Inn of the Lance");
        assert_eq!(compact_info.width, 5);
        assert_eq!(compact_info.height, 3);
        assert_eq!(compact_info.tileset, "tin01");

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("compact local Dark Ranger area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten compact area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "innofthelasthope");
        assert_eq!(summary.tileset_resref, "tin01");
        assert_eq!(summary.width, 5);
        assert_eq!(summary.packet_height, 3);
        assert_eq!(summary.tile_count, 15);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondModuleResourceAreaRepair)
        );
        assert!(proof.read_end == summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[test]
    fn local_winds_eremor_compact_area_uses_packet_fragments_when_object_id_repeats() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "Ere".to_string(),
                module_resref: "Ere".to_string(),
                areas: vec![
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Keep Ere".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "M t Ere".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0xE200_0000,
                        name: "T Crypts Ere".to_string(),
                    },
                ],
            },
        );

        let mut payload = LOCAL_WINDS_EREMOR_MOUNT_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let source_layout =
            area_static_layout(&payload, fragment_offset).expect("compact static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondFixed16
        );
        assert_eq!(source_layout.area_name_end_read_offset, 60);
        assert_eq!(source_layout.width_read_offset, 156);
        assert_eq!(source_layout.height_read_offset, 160);
        assert_eq!(source_layout.tileset_read_offset, 164);
        assert_eq!(source_layout.first_tile_read_offset, 180);
        assert_eq!(
            diamond_short_fixed_area_name_fragments(&payload, fragment_offset)
                .expect("short fixed-window area fragments"),
            vec!["M".to_string(), "t".to_string(), "Ere".to_string()]
        );

        let context =
            crate::translate::module::observed_module_context().expect("observed module context");
        let module_path =
            observed_module_file_path(&context).expect("observed local module file path");
        let module_table =
            read_module_area_resource_table(&module_path).expect("local module ARE table");
        assert!(module_table_area_order_matches_observed_context(
            &module_table,
            &context
        ));
        assert!(module_table_unordered_area_matches_observed_context(
            &module_table,
            &context
        ));

        let compact_info =
            module_area_resource_info_for_compact_packet(&payload, fragment_offset, 0x8000_0000)
                .expect("compact packet should resolve to the unique local ARE resource");
        assert_eq!(compact_info.resref, "mountiridor");
        assert_eq!(compact_info.name, "Mount Eremor");
        assert_eq!(compact_info.width, 16);
        assert_eq!(compact_info.height, 15);
        assert_eq!(compact_info.tileset, "tcn01");
        assert_eq!(compact_info.map_notes.len(), 3);
        assert_eq!(compact_info.sounds.len(), 123);

        let mut repaired_source = payload.clone();
        repair_compact_area_from_module_resource(
            &mut repaired_source,
            fragment_offset,
            0x8000_0000,
            &source_layout,
        )
        .expect("compact area source fields repaired from local ARE resource");
        let repaired_scan = scan_area_tile_stream(&repaired_source, fragment_offset);
        assert!(repaired_scan.valid);
        assert_eq!(repaired_scan.width, 16);
        assert_eq!(repaired_scan.packet_height, 15);
        assert!(repair_compact_post_tile_tail_for_ee(
            &mut repaired_source,
            fragment_offset,
            &repaired_scan,
            &compact_info,
        ));
        let (_, _, repaired_fragment_offset, _) =
            area_client_area_read_window(&repaired_source).expect("repaired read window");
        let repaired_scan = scan_area_tile_stream(&repaired_source, repaired_fragment_offset);
        assert!(legacy_area_source_tail_consumes_read_buffer(
            &repaired_source,
            repaired_fragment_offset,
            &repaired_scan
        ));

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("compact local Winds of Eremor area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten compact area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "mountiridor");
        assert_eq!(summary.tileset_resref, "tcn01");
        assert_eq!(summary.width, 16);
        assert_eq!(summary.packet_height, 15);
        assert_eq!(summary.tile_count, 240);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondCompactAreaName)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondShortFixedAreaName)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondModuleResourceAreaRepair)
        );
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }
}
