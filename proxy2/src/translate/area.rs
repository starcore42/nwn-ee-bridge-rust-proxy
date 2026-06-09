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
    collections::{HashMap, HashSet},
    fmt::Write,
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
const DIAMOND_LONG_AREA_NAME_BYTES: usize = 21;
const DIAMOND_SHORT_AREA_NAME_BYTES: usize = 16;
const DIAMOND_LEGACY_AREA_NAME_BYTES: usize = 20;
const DIAMOND_NO_AREA_NAME_BYTES: usize = 4;
const DIAMOND_COMPACT_AREA_NAME_BYTES: usize = 14;
const DIAMOND_LONG_AREA_NAME_TEXT_BYTES: usize =
    DIAMOND_LONG_AREA_NAME_BYTES - EE_CEXO_STRING_LENGTH_BYTES;
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
const MAX_DECLARED_ZERO_AREA_FRAGMENT_BYTES: usize = 16;
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
const MIN_MODULE_NAME_PREFIX_MATCH_CHARS: usize = 8;
const MIN_MODULE_FILE_PREFIX_MATCH_CHARS: usize = 6;
const MAX_GFF_FIELD_COUNT: u32 = 65_536;
const MAX_GFF_STRUCT_COUNT: u32 = 65_536;
const GFF_TYPE_BYTE: u32 = 0;
const GFF_TYPE_WORD: u32 = 2;
const GFF_TYPE_SHORT: u32 = 3;
const GFF_TYPE_DWORD: u32 = 4;
const GFF_TYPE_INT: u32 = 5;
const GFF_TYPE_FLOAT: u32 = 8;
const GFF_TYPE_CEXO_STRING: u32 = 10;
const GFF_TYPE_RESREF: u32 = 11;
const GFF_TYPE_CEXO_LOCSTRING: u32 = 12;
const GFF_TYPE_LIST: u32 = 15;
const AREA_SOUND_X_OFFSET: usize = 40;
const AREA_SOUND_Y_OFFSET: usize = 44;
const MAX_STATIC_PLACEABLE_COMPONENT_ABS: f32 = 100_000.0;
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
    LegacyDiamondLongFixedAreaName,
    LegacyDiamondFixedAreaName,
    LegacyDiamondShortFixedAreaName,
    LegacyDiamondNoAreaName,
    LegacyDiamondFragmentedAreaResrefRepair,
    LegacyDiamondDeclaredZeroReadWindow,
    LegacyDiamondCompactAreaName,
    LegacyDiamondFragmentedCExoAreaName,
    LegacyDiamondModuleResourceAreaRepair,
    LegacyDiamondCompactPostTileTailRepair,
    LegacyDiamondMissingSquareDimensionsRepair,
    LegacyHgMissingHeightRepair,
    LegacyHgMissingWidthRepair,
    LegacyDiamondSoundCountZeroMeansOneRepair,
    LegacyDiamondStaticPlaceableCountZeroRepair,
    LegacyDiamondStaticPlaceableDirectionNormalize,
    LegacyDiamondModuleResourceStaticPlaceableRepair,
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
    pub module_resource_static_placeable_repairs: u32,
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
        self.contains_light_placeable_id(object_id) || self.contains_static_placeable_id(object_id)
    }

    pub fn contains_light_placeable_id(&self, object_id: u32) -> bool {
        self.light_rows.iter().any(|row| row.object_id == object_id)
    }

    pub fn contains_static_placeable_id(&self, object_id: u32) -> bool {
        self.static_rows
            .iter()
            .any(|row| row.object_id == object_id)
    }

    pub fn matching_placeable_rows(
        &self,
        object_id: u32,
    ) -> impl Iterator<Item = AreaPlaceableContextRowMatch<'_>> {
        self.light_rows
            .iter()
            .filter(move |row| row.object_id == object_id)
            .map(|row| AreaPlaceableContextRowMatch {
                kind: AreaPlaceableContextRowKind::Light,
                row,
            })
            .chain(
                self.static_rows
                    .iter()
                    .filter(move |row| row.object_id == object_id)
                    .map(|row| AreaPlaceableContextRowMatch {
                        kind: AreaPlaceableContextRowKind::Static,
                        row,
                    }),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaPlaceableContextRowKind {
    Light,
    Static,
}

impl AreaPlaceableContextRowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AreaPlaceableContextRowKind::Light => "light",
            AreaPlaceableContextRowKind::Static => "static",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AreaPlaceableContextRowMatch<'a> {
    pub kind: AreaPlaceableContextRowKind,
    pub row: &'a AreaPlaceableContextRow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AreaPlaceableContextObjectIdConfidence {
    #[default]
    Unique,
    AreaObjectAlias,
    DuplicateObjectId,
    AreaObjectAliasDuplicate,
}

impl AreaPlaceableContextObjectIdConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            AreaPlaceableContextObjectIdConfidence::Unique => "unique",
            AreaPlaceableContextObjectIdConfidence::AreaObjectAlias => "area-alias",
            AreaPlaceableContextObjectIdConfidence::DuplicateObjectId => "duplicate",
            AreaPlaceableContextObjectIdConfidence::AreaObjectAliasDuplicate => {
                "area-alias+duplicate"
            }
        }
    }

    fn is_unique(self) -> bool {
        matches!(self, AreaPlaceableContextObjectIdConfidence::Unique)
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
    pub object_id_confidence: AreaPlaceableContextObjectIdConfidence,
    pub module_state: Option<AreaPlaceableContextState>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AreaPlaceableContextState {
    pub static_object: bool,
    pub useable: bool,
    pub trap_flag: bool,
    pub trap_disarmable: bool,
    pub lockable: bool,
    pub locked: bool,
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
    DiamondFixed21,
    DiamondFixed20,
    DiamondFixed16,
    DiamondCompactFragmented,
    DiamondNoAreaName,
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
    sound_count: u16,
    static_count_read_offset: usize,
    static_rows_read_offset: usize,
    static_rows_count: u16,
    zero_static_placeable_rows: u16,
}

pub fn rewrite_area_client_area_payload(payload: &mut Vec<u8>) -> Option<AreaRewriteSummary> {
    rewrite_area_client_area_payload_with_module_context(payload, None)
}

pub(crate) fn rewrite_area_client_area_payload_with_module_context(
    payload: &mut Vec<u8>,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
) -> Option<AreaRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != AREA_MAJOR
        || payload[2] != AREA_CLIENT_AREA_MINOR
    {
        return None;
    }

    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if declared == 0 {
        if let Some(summary) =
            rewrite_declared_zero_area_client_area_payload(payload, module_context)
        {
            return Some(summary);
        }
    }

    rewrite_declared_area_client_area_payload(payload, module_context)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AreaRewriteDiagnosticMode {
    Normal,
    DeclaredZeroProbe,
}

fn rewrite_declared_area_client_area_payload(
    payload: &mut Vec<u8>,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
) -> Option<AreaRewriteSummary> {
    rewrite_declared_area_client_area_payload_with_mode(
        payload,
        module_context,
        AreaRewriteDiagnosticMode::Normal,
    )
}

fn rewrite_declared_area_client_area_payload_with_mode(
    payload: &mut Vec<u8>,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
    diagnostic_mode: AreaRewriteDiagnosticMode,
) -> Option<AreaRewriteSummary> {
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
    )
    .unwrap_or_default();
    if !legacy_area_object_id_plausible(legacy_area_object_id) {
        tracing::warn!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            area_resref = %area_resref,
            "Area_ClientArea rewrite skipped: implausible area OBJECTID"
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
    let static_layout = select_area_static_layout_for_rewrite(
        payload,
        fragment_offset,
        legacy_area_object_id,
        &static_layout,
        module_context,
    );

    let area_resref_was_plausible = area_resref_plausible(&area_resref);
    let mut working_payload = payload.clone();
    let fragmented_cexo_resource_repair_required = compact_cexo_area_needs_module_resource_repair(
        &working_payload,
        fragment_offset,
        &static_layout,
        area_resref_was_plausible,
    );
    let fragmented_area_resref_resource_repair_required = static_layout.area_name_encoding
        == AreaNameEncoding::DiamondNoAreaName
        && compact_packet_area_resref_fragments(&working_payload, fragment_offset)
            .is_some_and(|fragments| fragments.len() >= 2);
    let module_resource_area_repair = repair_compact_area_from_module_resource(
        &mut working_payload,
        fragment_offset,
        legacy_area_object_id,
        &static_layout,
        !area_resref_was_plausible || fragmented_cexo_resource_repair_required,
        diagnostic_mode == AreaRewriteDiagnosticMode::Normal,
        module_context,
    );
    if let Some(resource_info) = module_resource_area_repair.as_ref() {
        area_resref = resource_info.resref.clone();
    } else if !area_resref_was_plausible {
        tracing::warn!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            area_resref = %area_resref,
            "Area_ClientArea rewrite skipped: implausible area resref without a proven local module resource repair"
        );
        return None;
    }
    let (_, _, resource_fragment_offset, _) = area_client_area_read_window(&working_payload)?;
    let mut tile_scan = scan_area_tile_stream(&working_payload, resource_fragment_offset);
    let square_dimensions_repaired = repair_missing_square_area_dimensions(
        &mut working_payload,
        resource_fragment_offset,
        &mut tile_scan,
    );
    let width_repaired = square_dimensions_repaired
        || repair_missing_area_width(
            &mut working_payload,
            resource_fragment_offset,
            &mut tile_scan,
        );
    let height_repaired = square_dimensions_repaired
        || repair_missing_area_height(
            &mut working_payload,
            resource_fragment_offset,
            &mut tile_scan,
        );
    let diamond_fixed_name_rewritten = matches!(
        static_layout.area_name_encoding,
        AreaNameEncoding::DiamondFixed20 | AreaNameEncoding::DiamondFixed16
    ) && module_resource_area_repair.is_none();
    if static_layout.area_name_encoding == AreaNameEncoding::DiamondFixed21
        && module_resource_area_repair.is_none()
    {
        tracing::warn!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            area_resref = %area_resref,
            "Area_ClientArea rewrite skipped: 21-byte Diamond fixed-name window requires proven local module resource repair"
        );
        return None;
    }
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
        let (_, _, fixed_name_fragment_offset, _) = area_client_area_read_window(&working_payload)?;
        tile_scan = scan_area_tile_stream(&working_payload, fixed_name_fragment_offset);
    }
    let compact_post_tile_tail_repaired =
        if let Some(resource_info) = module_resource_area_repair.as_ref() {
            let (_, _, tail_fragment_offset, _) = area_client_area_read_window(&working_payload)?;
            repair_compact_post_tile_tail_for_ee(
                &mut working_payload,
                tail_fragment_offset,
                &tile_scan,
                resource_info,
            )
        } else {
            false
        };
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
    let module_resource_static_placeable_repair_info =
        module_resource_area_repair.as_ref().cloned().or_else(|| {
            module_area_resource_info_for_named_static_placeables(
                &working_payload,
                working_fragment_offset,
                &tile_scan,
                &area_resref,
                module_context,
            )
        });
    let module_resource_static_placeable_repairs = module_resource_static_placeable_repair_info
        .as_ref()
        .and_then(|resource_info| {
            repair_module_resource_static_placeable_rows(
                &mut working_payload,
                working_fragment_offset,
                &tile_scan,
                resource_info,
            )
        })
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
        AreaNameEncoding::DiamondFixed21 => {
            rewrite_kinds.push(AreaRewriteKind::LegacyDiamondLongFixedAreaName);
        }
        AreaNameEncoding::DiamondFixed20 => {
            rewrite_kinds.push(AreaRewriteKind::LegacyDiamondFixedAreaName);
        }
        AreaNameEncoding::DiamondFixed16 => {
            rewrite_kinds.push(AreaRewriteKind::LegacyDiamondShortFixedAreaName);
        }
        AreaNameEncoding::DiamondNoAreaName => {
            rewrite_kinds.push(AreaRewriteKind::LegacyDiamondNoAreaName);
        }
        _ => {}
    }
    if module_resource_area_repair.is_some() && fragmented_area_resref_resource_repair_required {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondFragmentedAreaResrefRepair);
    }
    if module_resource_area_repair.is_some() {
        match static_layout.area_name_encoding {
            AreaNameEncoding::CExoString => {
                rewrite_kinds.push(AreaRewriteKind::LegacyDiamondFragmentedCExoAreaName);
            }
            _ => rewrite_kinds.push(AreaRewriteKind::LegacyDiamondCompactAreaName),
        }
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
    if module_resource_static_placeable_repairs != 0 {
        rewrite_kinds.push(AreaRewriteKind::LegacyDiamondModuleResourceStaticPlaceableRepair);
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
        module_resource_static_placeable_repair_info.as_ref(),
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
        match diagnostic_mode {
            AreaRewriteDiagnosticMode::Normal => {
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
            }
            AreaRewriteDiagnosticMode::DeclaredZeroProbe => {
                tracing::debug!(
                    area_resref = %area_resref,
                    tileset_resref = %tileset_resref,
                    old_declared = declared,
                    new_declared,
                    old_read_size = read_size,
                    new_read_size,
                    old_fragment_offset = fragment_offset,
                    new_fragment_offset,
                    "Area_ClientArea declared-zero read-window candidate rejected by exact EE LoadArea cursor proof"
                );
            }
        }
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
            module_resource_static_placeable_repairs,
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
        module_resource_static_placeable_repairs,
        static_placeable_trailing_rows_dropped,
        rewrite_kinds,
        placeable_context_valid,
        placeable_light_count,
        placeable_static_count,
        placeable_context,
    })
}

fn rewrite_declared_zero_area_client_area_payload(
    payload: &mut Vec<u8>,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
) -> Option<AreaRewriteSummary> {
    if payload.len() <= HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES {
        return None;
    }

    let mut candidates = Vec::new();
    let max_fragment_bytes =
        MAX_DECLARED_ZERO_AREA_FRAGMENT_BYTES.min(payload.len() - HIGH_LEVEL_HEADER_BYTES);
    for fragment_size in 1..=max_fragment_bytes {
        let fragment_offset = payload.len().checked_sub(fragment_size)?;
        if fragment_offset < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES {
            continue;
        }
        let fragment = payload.get(fragment_offset..)?;
        let Some(fragment_bits) = cnw_fragment_consumable_bits(fragment) else {
            continue;
        };
        // Declared-zero Diamond packets are observed both with the minimal
        // pre-tile tail and with additional decompile-owned post-tile list
        // selector bits. Keep this as a bounded tail-size probe; the staged
        // legacy source proof and exact EE LoadArea proof below remain the
        // acceptance criteria.
        if fragment_bits < LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS {
            continue;
        }

        let mut staged = payload.clone();
        write_u32_le(&mut staged, HIGH_LEVEL_HEADER_BYTES, fragment_offset as u32)?;
        let Some(mut summary) = rewrite_declared_area_client_area_payload_with_mode(
            &mut staged,
            module_context,
            AreaRewriteDiagnosticMode::DeclaredZeroProbe,
        ) else {
            continue;
        };
        summary.old_declared = 0;
        summary
            .rewrite_kinds
            .push(AreaRewriteKind::LegacyDiamondDeclaredZeroReadWindow);
        candidates.push((fragment_offset, staged, summary));
    }

    if candidates.len() != 1 {
        tracing::warn!(
            candidates = candidates.len(),
            payload_len = payload.len(),
            "Area_ClientArea rewrite skipped: declared-zero read window did not resolve uniquely"
        );
        return None;
    }

    let (inferred_declared, staged, summary) = candidates.remove(0);
    tracing::info!(
        rewrite_kind = ?AreaRewriteKind::LegacyDiamondDeclaredZeroReadWindow,
        inferred_declared,
        new_declared = summary.new_declared,
        old_fragment_offset = summary.old_fragment_offset,
        new_fragment_offset = summary.new_fragment_offset,
        area_resref = summary.area_resref.as_str(),
        tileset_resref = summary.tileset_resref.as_str(),
        width = summary.width,
        packet_height = summary.packet_height,
        inferred_height = summary.inferred_height,
        tile_count = summary.tile_count,
        "Area_ClientArea declared-zero read window repaired from exact validated legacy fragment tail"
    );
    *payload = staged;
    Some(summary)
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

    if let Some((area_name_length, name_end)) =
        read_diamond_fixed_area_name_shape(payload, fragment_offset)
    {
        if let Some(layout) = area_static_layout_for_dialect(
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
        }) {
            return Some(layout);
        }
    }

    if let Some((area_name_length, name_end)) =
        read_diamond_long_fixed_area_name_shape(payload, fragment_offset)
    {
        if let Some(layout) = area_static_layout_for_dialect(
            payload,
            fragment_offset,
            AreaNameEncoding::DiamondFixed21,
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

    diamond_no_area_name_static_layout(payload, fragment_offset)
}

fn diamond_no_area_name_static_layout(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<AreaStaticLayout> {
    read_diamond_no_area_name_shape(payload, fragment_offset).and_then(
        |(area_name_length, name_end)| {
            area_static_layout_for_dialect(
                payload,
                fragment_offset,
                AreaNameEncoding::DiamondNoAreaName,
                area_name_length,
                name_end,
                AreaStaticDialect::Legacy169,
                LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
                LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
                LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
            )
        },
    )
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
        AreaNameEncoding::DiamondFixed21 => (
            DIAMOND_LONG_AREA_NAME_BYTES,
            DIAMOND_LONG_AREA_NAME_TEXT_BYTES,
        ),
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
    placeables: Vec<ModuleAreaPlaceable>,
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

#[derive(Debug, Clone)]
struct ModuleAreaPlaceable {
    tag: String,
    appearance: u16,
    x: f32,
    y: f32,
    z: f32,
    bearing: f32,
    static_object: bool,
    useable: bool,
    trap_flag: bool,
    trap_disarmable: bool,
    lockable: bool,
    locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleStaticPlaceableRowMatchKind {
    ExactAppearanceAtLeastTwoCoordinates,
    ZeroAppearanceAllCoordinates,
}

#[derive(Debug, Clone, Copy)]
struct ModuleStaticPlaceableRowClaim {
    cursor: usize,
    placeable_index: usize,
    object_id: u32,
    appearance: u16,
    x: f32,
    y: f32,
    z: f32,
    dir_x: f32,
    dir_y: f32,
    dir_z: f32,
    match_kind: ModuleStaticPlaceableRowMatchKind,
}

#[derive(Debug, Clone, Copy)]
struct ModuleStaticPlaceableContextClaim {
    row: ModuleStaticPlaceableRowClaim,
    state: AreaPlaceableContextState,
}

fn repair_compact_area_from_module_resource(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
    legacy_area_object_id: u32,
    layout: &AreaStaticLayout,
    allow_fragmented_cexo_name_repair: bool,
    allow_valid_scan_empty_tail_rewrite: bool,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
) -> Option<ModuleAreaResourceInfo> {
    if layout.dialect != AreaStaticDialect::Legacy169 {
        return None;
    }
    match layout.area_name_encoding {
        AreaNameEncoding::DiamondCompactFragmented
        | AreaNameEncoding::DiamondFixed21
        | AreaNameEncoding::DiamondFixed20
        | AreaNameEncoding::DiamondFixed16
        | AreaNameEncoding::DiamondNoAreaName => {}
        AreaNameEncoding::CExoString => {
            if !allow_fragmented_cexo_name_repair {
                return None;
            }
            if usize::try_from(layout.area_name_length).ok()? == 0
                || diamond_cexo_string_area_name_fragments(payload, fragment_offset).is_none()
            {
                return None;
            }
        }
    }

    let packet_width = read_area_u32(payload, fragment_offset, layout.width_read_offset)?;
    let packet_height = read_area_u32(payload, fragment_offset, layout.height_read_offset)?;
    let info = module_area_resource_info_for_compact_packet(
        payload,
        fragment_offset,
        legacy_area_object_id,
        layout,
        module_context,
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

    let mut effective_fragment_offset = fragment_offset;
    let mut effective_layout = layout.clone();
    write_fixed_resref_payload(
        payload,
        LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES,
        &info.resref,
    )?;
    match layout.area_name_encoding {
        AreaNameEncoding::DiamondCompactFragmented => {
            write_compact_area_name_as_cexo_string(payload, effective_fragment_offset, &info.name)?;
        }
        AreaNameEncoding::DiamondFixed21 => {
            write_long_fixed_area_name_as_cexo_string(
                payload,
                effective_fragment_offset,
                &info.name,
            )?;
        }
        AreaNameEncoding::DiamondFixed20 => {
            write_fixed_area_name_as_cexo_string(payload, effective_fragment_offset, &info.name)?;
        }
        AreaNameEncoding::DiamondFixed16 => {
            write_short_fixed_area_name_as_cexo_string(
                payload,
                effective_fragment_offset,
                &info.name,
            )?;
        }
        AreaNameEncoding::DiamondNoAreaName => {
            let inserted =
                replace_no_area_name_as_cexo_string(payload, fragment_offset, &info.name)?;
            effective_fragment_offset = effective_fragment_offset.checked_add(inserted)?;
            remove_no_area_name_fragment_selector(payload, effective_fragment_offset)?;
            effective_layout.area_name_encoding = AreaNameEncoding::CExoString;
            effective_layout.area_name_length = u32::try_from(info.name.len()).ok()?;
            effective_layout.area_name_end_read_offset = effective_layout
                .area_name_end_read_offset
                .checked_add(inserted)?;
            effective_layout.width_read_offset =
                effective_layout.width_read_offset.checked_add(inserted)?;
            effective_layout.height_read_offset =
                effective_layout.height_read_offset.checked_add(inserted)?;
            effective_layout.tileset_read_offset =
                effective_layout.tileset_read_offset.checked_add(inserted)?;
            effective_layout.first_tile_read_offset = effective_layout
                .first_tile_read_offset
                .checked_add(inserted)?;
        }
        AreaNameEncoding::CExoString => {
            if usize::try_from(layout.area_name_length).ok()? != info.name.len() {
                return None;
            }
            write_area_cexo_string_exact(
                payload,
                effective_fragment_offset,
                AREA_NAME_READ_OFFSET,
                &info.name,
            )?;
        }
    }
    write_area_u32(
        payload,
        effective_fragment_offset,
        effective_layout.width_read_offset,
        info.width,
    )?;
    write_area_u32(
        payload,
        effective_fragment_offset,
        effective_layout.height_read_offset,
        info.height,
    )?;
    write_area_fixed_resref(
        payload,
        effective_fragment_offset,
        effective_layout.tileset_read_offset,
        &info.tileset,
    )?;

    let mut repaired_scan = scan_area_tile_stream(payload, effective_fragment_offset);
    if !module_resource_tile_scan_matches(&repaired_scan, &info) {
        if let Some(new_fragment_offset) = rewrite_module_resource_tiles_with_tail_compaction(
            payload,
            effective_fragment_offset,
            &effective_layout,
            &info,
        ) {
            effective_fragment_offset = new_fragment_offset;
        } else if let Some(new_fragment_offset) = rewrite_module_resource_tiles_with_empty_tail(
            payload,
            effective_fragment_offset,
            &effective_layout,
            &info,
        ) {
            effective_fragment_offset = new_fragment_offset;
        } else {
            write_module_resource_tiles(
                payload,
                effective_fragment_offset,
                effective_layout.first_tile_read_offset,
                &info,
            )?;
        }
        repaired_scan = scan_area_tile_stream(payload, effective_fragment_offset);
    }
    if !module_resource_tile_scan_matches(&repaired_scan, &info) {
        return None;
    }
    if allow_valid_scan_empty_tail_rewrite
        && info.map_notes.is_empty()
        && info.sounds.is_empty()
        && legacy_area_source_tail_exact_read_proof(
            payload,
            effective_fragment_offset,
            &repaired_scan,
        )
        .is_none()
    {
        let new_fragment_offset = rewrite_module_resource_tiles_with_empty_tail(
            payload,
            effective_fragment_offset,
            &effective_layout,
            &info,
        )?;
        effective_fragment_offset = new_fragment_offset;
        repaired_scan = scan_area_tile_stream(payload, effective_fragment_offset);
        if !module_resource_tile_scan_matches(&repaired_scan, &info)
            || legacy_area_source_tail_exact_read_proof(
                payload,
                effective_fragment_offset,
                &repaired_scan,
            )
            .is_none()
        {
            return None;
        }
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

fn compact_cexo_area_needs_module_resource_repair(
    payload: &[u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    area_resref_was_plausible: bool,
) -> bool {
    if !area_resref_was_plausible
        || layout.dialect != AreaStaticDialect::Legacy169
        || layout.area_name_encoding != AreaNameEncoding::CExoString
    {
        return false;
    }

    let Some(name_len) = usize::try_from(layout.area_name_length).ok() else {
        return false;
    };
    if name_len == 0 || diamond_cexo_string_area_name_fragments(payload, fragment_offset).is_none()
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

    packet_width == 0
        || packet_height == 0
        || !scan_area_tile_stream(payload, fragment_offset).valid
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

fn unique_no_name_area_resource_for_truncated_packet_resref(
    payload: &[u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    table: &ModuleAreaResourceTable,
    packet_area_resref: &str,
) -> Option<ModuleAreaResourceInfo> {
    let packet_resref = packet_area_resref.to_ascii_lowercase();
    if packet_resref.len() < 6 {
        return None;
    }
    if layout.dialect != AreaStaticDialect::Legacy169
        || layout.area_name_encoding != AreaNameEncoding::DiamondNoAreaName
    {
        return None;
    }
    let scan = scan_area_tile_stream_from_layout(payload, fragment_offset, layout.clone(), false);
    if !scan.valid || scan.width == 0 || scan.inferred_height == 0 {
        return None;
    }
    let packet_height = if scan.packet_height == 0 {
        scan.inferred_height
    } else {
        scan.packet_height
    };
    let packet_tileset = fixed_resref_preview(
        payload,
        HIGH_LEVEL_HEADER_BYTES.checked_add(layout.tileset_read_offset)?,
    )?;

    let mut matches = table
        .areas
        .iter()
        .filter(|info| {
            let info_resref = info.resref.to_ascii_lowercase();
            (info_resref == packet_resref
                || (info_resref.len() > packet_resref.len()
                    && info_resref.starts_with(&packet_resref)))
                && info.width == scan.width
                && info.height == packet_height
                && scan.tile_count == info.width.saturating_mul(info.height)
                && info.tileset.eq_ignore_ascii_case(&packet_tileset)
        })
        .cloned()
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn unique_no_name_area_resource_for_fragmented_packet_resref(
    payload: &[u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    table: &ModuleAreaResourceTable,
    packet_area_resref_fragments: Option<&[String]>,
) -> Option<ModuleAreaResourceInfo> {
    if layout.dialect != AreaStaticDialect::Legacy169
        || layout.area_name_encoding != AreaNameEncoding::DiamondNoAreaName
    {
        return None;
    }
    let fragments = packet_area_resref_fragments?;
    if fragments.len() < 2 {
        return None;
    }

    let packet_width = read_area_u32(payload, fragment_offset, layout.width_read_offset)?;
    let packet_height = read_area_u32(payload, fragment_offset, layout.height_read_offset)?;
    let packet_tileset = fixed_resref_preview(
        payload,
        HIGH_LEVEL_HEADER_BYTES.checked_add(layout.tileset_read_offset)?,
    )?;
    if !area_resref_plausible(&packet_tileset) {
        return None;
    }

    let mut matches = table
        .areas
        .iter()
        .filter(|info| {
            (packet_width == 0 || packet_width == info.width)
                && (packet_height == 0 || packet_height == info.height)
                && info.tileset.eq_ignore_ascii_case(&packet_tileset)
                && compact_fragments_match_allowing_singletons(&info.resref, fragments)
        })
        .cloned()
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn compact_packet_area_resref_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    let start = LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET.checked_add(LEGACY_AREA_OBJECT_ID_BYTES)?;
    let end = start.checked_add(CRESREF_TEXT_BYTES)?;
    if end > fragment_offset || end > payload.len() {
        return None;
    }
    let fragments = compact_ascii_runs_allowing_singletons(payload.get(start..end)?, |byte| {
        byte.is_ascii_alphanumeric() || byte == b'_'
    })?;
    (fragments.len() >= 2).then_some(fragments)
}

fn write_module_resource_tiles(
    payload: &mut [u8],
    fragment_offset: usize,
    first_tile_read_offset: usize,
    info: &ModuleAreaResourceInfo,
) -> Option<()> {
    let tile_bytes = module_resource_tile_bytes(info)?;
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

fn rewrite_module_resource_tiles_with_tail_compaction(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    info: &ModuleAreaResourceInfo,
) -> Option<usize> {
    let tile_bytes = module_resource_tile_bytes(info)?;
    let old_tail_start = find_unique_legacy_tail_start_for_module_resource_tiles(
        payload,
        fragment_offset,
        layout,
        info,
    )?;
    let tile_start_payload = HIGH_LEVEL_HEADER_BYTES.checked_add(layout.first_tile_read_offset)?;
    let old_tail_payload = HIGH_LEVEL_HEADER_BYTES.checked_add(old_tail_start)?;
    if tile_start_payload > old_tail_payload || old_tail_payload > fragment_offset {
        return None;
    }

    let old_tile_bytes = old_tail_payload.checked_sub(tile_start_payload)?;
    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let new_tile_bytes = tile_bytes.len();
    let new_declared = adjust_declared_by_len_delta(declared, old_tile_bytes, new_tile_bytes)?;
    payload.splice(tile_start_payload..old_tail_payload, tile_bytes);
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared)?;

    if new_tile_bytes >= old_tile_bytes {
        fragment_offset.checked_add(new_tile_bytes.checked_sub(old_tile_bytes)?)
    } else {
        fragment_offset.checked_sub(old_tile_bytes.checked_sub(new_tile_bytes)?)
    }
}

fn rewrite_module_resource_tiles_with_empty_tail(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    info: &ModuleAreaResourceInfo,
) -> Option<usize> {
    // Resource-backed compact packets can carry a tile/body section that is not
    // the EE/Diamond tile-record reader shape. When local ARE/GIT evidence
    // proves there are no map pins or sound objects, emit the exact empty
    // post-tile list branch instead of preserving unclaimed compact bytes.
    if !info.map_notes.is_empty() || !info.sounds.is_empty() {
        return None;
    }

    let tile_bytes = module_resource_tile_bytes(info)?;
    let tile_start_payload = HIGH_LEVEL_HEADER_BYTES.checked_add(layout.first_tile_read_offset)?;
    if tile_start_payload > fragment_offset || fragment_offset > payload.len() {
        return None;
    }

    let mut replacement = Vec::with_capacity(tile_bytes.len().checked_add(14)?);
    replacement.extend_from_slice(&tile_bytes);
    replacement.extend_from_slice(&0u32.to_le_bytes());
    replacement.extend_from_slice(&0u32.to_le_bytes());
    replacement.extend_from_slice(&0u16.to_le_bytes());
    replacement.extend_from_slice(&0u16.to_le_bytes());
    replacement.extend_from_slice(&0u16.to_le_bytes());

    let old_len = fragment_offset.checked_sub(tile_start_payload)?;
    let new_len = replacement.len();
    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let new_declared = adjust_declared_by_len_delta(declared, old_len, new_len)?;
    payload.splice(tile_start_payload..fragment_offset, replacement);
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared)?;

    let new_fragment_offset = if new_len >= old_len {
        fragment_offset.checked_add(new_len.checked_sub(old_len)?)?
    } else {
        fragment_offset.checked_sub(old_len.checked_sub(new_len)?)?
    };
    truncate_area_fragment_to_pre_tile_bits(payload, new_fragment_offset)?;
    Some(new_fragment_offset)
}

fn module_resource_tile_bytes(info: &ModuleAreaResourceInfo) -> Option<Vec<u8>> {
    if info.tiles.is_empty()
        || info.tiles.len() != usize::try_from(info.width.checked_mul(info.height)?).ok()?
    {
        return None;
    }
    let mut tile_bytes = Vec::new();
    for tile in &info.tiles {
        write_module_resource_tile_record(&mut tile_bytes, tile)?;
    }
    Some(tile_bytes)
}

fn find_unique_legacy_tail_start_for_module_resource_tiles(
    payload: &[u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    info: &ModuleAreaResourceInfo,
) -> Option<usize> {
    let (_, read_size, _, fragment_size) = area_client_area_read_window(payload)?;
    if fragment_size == 0 {
        return None;
    }
    let expected_tile_count = info.width.checked_mul(info.height)?;
    if expected_tile_count == 0 || expected_tile_count > MAX_REASONABLE_AREA_TILE_COUNT {
        return None;
    }
    if layout.first_tile_read_offset > read_size {
        return None;
    }

    let mut found = None;
    for candidate in layout.first_tile_read_offset..=read_size {
        let scan = AreaTileStreamScan {
            valid: true,
            layout: layout.clone(),
            width: info.width,
            packet_height: info.height,
            inferred_height: info.height,
            tile_count: expected_tile_count,
            tile_end_read_offset: candidate,
        };
        if legacy_area_source_tail_exact_read_proof(payload, fragment_offset, &scan).is_some() {
            if found.is_some() {
                return None;
            }
            found = Some(candidate);
        }
    }
    found
}

fn truncate_area_fragment_to_pre_tile_bits(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
) -> Option<()> {
    let fragment = payload.get(fragment_offset..)?;
    let bits = decode_cnw_msb_valid_bits(fragment, LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)?;
    let payload_bits = bits
        .get(CNW_FRAGMENT_HEADER_BITS..LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)?
        .to_vec();
    let rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)?;
    payload.splice(fragment_offset.., rewritten_fragment);
    Some(())
}

fn adjust_declared_by_len_delta(declared: u32, old_len: usize, new_len: usize) -> Option<u32> {
    if new_len >= old_len {
        declared.checked_add(u32::try_from(new_len.checked_sub(old_len)?).ok()?)
    } else {
        declared.checked_sub(u32::try_from(old_len.checked_sub(new_len)?).ok()?)
    }
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

fn replace_no_area_name_as_cexo_string(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
    name: &str,
) -> Option<usize> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty()
        || name_bytes.len() > 32
        || !name_bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }

    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    if payload_offset > fragment_offset || payload_offset > payload.len() {
        return None;
    }
    let replacement_len = EE_CEXO_STRING_LENGTH_BYTES.checked_add(name_bytes.len())?;
    if replacement_len < DIAMOND_NO_AREA_NAME_BYTES {
        return None;
    }
    let replaced_end = payload_offset.checked_add(DIAMOND_NO_AREA_NAME_BYTES)?;
    if replaced_end > fragment_offset || replaced_end > payload.len() {
        return None;
    }
    let inserted = replacement_len.checked_sub(DIAMOND_NO_AREA_NAME_BYTES)?;
    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let new_declared = declared.checked_add(u32::try_from(inserted).ok()?)?;
    let mut bytes = Vec::with_capacity(replacement_len);
    bytes.extend_from_slice(&(u32::try_from(name_bytes.len()).ok()?).to_le_bytes());
    bytes.extend_from_slice(name_bytes);
    payload.splice(payload_offset..replaced_end, bytes);
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared)?;
    Some(inserted)
}

fn remove_no_area_name_fragment_selector(
    payload: &mut Vec<u8>,
    fragment_offset: usize,
) -> Option<()> {
    let fragment = payload.get(fragment_offset..)?;
    let bits = decode_cnw_msb_valid_bits(
        fragment,
        CNW_FRAGMENT_HEADER_BITS + AREA_PRESENT_USER_BOOL_COUNT + 2,
    )?;
    let mut payload_bits = bits.get(CNW_FRAGMENT_HEADER_BITS..)?.to_vec();
    let legacy_no_name_locstring_payload_bit = AREA_PRESENT_USER_BOOL_COUNT.checked_add(1)?;
    if legacy_no_name_locstring_payload_bit >= payload_bits.len() {
        return None;
    }

    // Local Prelude follows the 1.69 no-inline-name path: a legacy locstring
    // selector bit is paired with the four-byte field at the area-name read
    // site. Replacing that field with EE's raw CExoString branch must remove
    // the selector before source-tail proof and row trimming run.
    payload_bits.remove(legacy_no_name_locstring_payload_bit);
    let rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)?;
    payload.splice(fragment_offset.., rewritten_fragment);
    Some(())
}

fn write_long_fixed_area_name_as_cexo_string(
    payload: &mut [u8],
    fragment_offset: usize,
    name: &str,
) -> Option<()> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty()
        || name_bytes.len() > DIAMOND_LONG_AREA_NAME_TEXT_BYTES
        || !name_bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return None;
    }

    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_LONG_AREA_NAME_BYTES)?;
    let text_start = payload_offset.checked_add(EE_CEXO_STRING_LENGTH_BYTES)?;
    let text_end = text_start.checked_add(DIAMOND_LONG_AREA_NAME_TEXT_BYTES)?;
    if end > fragment_offset || end > payload.len() || text_end > end {
        return None;
    }
    payload[payload_offset..payload_offset + EE_CEXO_STRING_LENGTH_BYTES]
        .copy_from_slice(&(DIAMOND_LONG_AREA_NAME_TEXT_BYTES as u32).to_le_bytes());
    payload[text_start..text_end].fill(0);
    payload[text_start..text_start + name_bytes.len()].copy_from_slice(name_bytes);
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
    if let Some((sound_count_offset, _map_note_count_consumed)) =
        repair_compact_map_note_tail_for_ee(&mut candidate, fragment_offset, scan, info)
    {
        if repair_compact_sound_tail_for_ee(
            &mut candidate,
            fragment_offset,
            sound_count_offset,
            info,
        )
        .is_some()
        {
            let candidate_scan = scan_area_tile_stream(&candidate, fragment_offset);
            // Compact post-tile repair rewrites list counts/strings only.  The
            // inferred module rows must not be allowed to change which tile
            // bytes the decompiled area reader owned before the tail handoff.
            if repaired_compact_post_tile_scan_matches(scan, &candidate_scan)
                && legacy_area_source_tail_exact_read_proof(
                    &candidate,
                    fragment_offset,
                    &candidate_scan,
                )
                .is_some()
            {
                *payload = candidate;
                return true;
            }
        }
    }
    false
}

fn repaired_compact_post_tile_scan_matches(
    source_scan: &AreaTileStreamScan,
    repaired_scan: &AreaTileStreamScan,
) -> bool {
    repaired_scan.valid
        && repaired_scan.layout.width_read_offset == source_scan.layout.width_read_offset
        && repaired_scan.layout.height_read_offset == source_scan.layout.height_read_offset
        && repaired_scan.layout.tileset_read_offset == source_scan.layout.tileset_read_offset
        && repaired_scan.layout.first_tile_read_offset == source_scan.layout.first_tile_read_offset
        && repaired_scan.width == source_scan.width
        && repaired_scan.packet_height == source_scan.packet_height
        && repaired_scan.inferred_height == source_scan.inferred_height
        && repaired_scan.tile_count == source_scan.tile_count
        && repaired_scan.tile_end_read_offset == source_scan.tile_end_read_offset
}

fn repair_compact_map_note_tail_for_ee(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
    info: &ModuleAreaResourceInfo,
) -> Option<(usize, usize)> {
    let note_count = u32::try_from(info.map_notes.len()).ok()?;
    if note_count == 0 {
        let tail_start = scan.tile_end_read_offset;
        if read_area_u32(payload, fragment_offset, tail_start) != Some(0)
            || read_area_u32(payload, fragment_offset, tail_start.checked_add(4)?) != Some(0)
        {
            return None;
        }
        return Some((tail_start.checked_add(8)?, 0));
    }
    if note_count > MAX_AREA_POST_TILE_LIST_COUNT {
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
    compact_ascii_runs_allowing_singletons(bytes, |byte| {
        byte.is_ascii_alphanumeric() || byte == b'_' || byte == b' '
    })
}

fn compact_printable_ascii_runs_allowing_singletons(bytes: &[u8]) -> Option<Vec<String>> {
    compact_ascii_runs_allowing_singletons(bytes, |byte| byte.is_ascii_graphic() || byte == b' ')
}

fn compact_ascii_runs_allowing_singletons(
    bytes: &[u8],
    valid_byte: impl Fn(u8) -> bool,
) -> Option<Vec<String>> {
    let mut fragments = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor] == 0 {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] != 0 {
            let byte = bytes[cursor];
            if !valid_byte(byte) {
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

fn area_resource_matches_single_area_fragments(
    info: &ModuleAreaResourceInfo,
    fragments: &[String],
) -> bool {
    compact_fragments_match_single_area(&info.name, fragments)
        || compact_fragments_match_single_area(&info.resref, fragments)
}

fn compact_fragments_match_single_area(target: &str, fragments: &[String]) -> bool {
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
    matched != 0
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

    scan_area_tile_stream_from_layout(payload, fragment_offset, layout, allow_legacy_missing_width)
}

fn scan_area_tile_stream_from_layout(
    payload: &[u8],
    fragment_offset: usize,
    layout: AreaStaticLayout,
    allow_legacy_missing_width: bool,
) -> AreaTileStreamScan {
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

fn select_area_static_layout_for_rewrite(
    payload: &[u8],
    fragment_offset: usize,
    legacy_area_object_id: u32,
    primary: &AreaStaticLayout,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
) -> AreaStaticLayout {
    if primary.area_name_encoding != AreaNameEncoding::CExoString {
        return primary.clone();
    }

    let primary_scan =
        scan_area_tile_stream_from_layout(payload, fragment_offset, primary.clone(), false);
    if primary_scan.valid {
        return primary.clone();
    }

    let Some(no_name_layout) = diamond_no_area_name_static_layout(payload, fragment_offset) else {
        return primary.clone();
    };
    let Some(resource_info) = module_area_resource_info_for_compact_packet(
        payload,
        fragment_offset,
        legacy_area_object_id,
        &no_name_layout,
        module_context,
    ) else {
        return primary.clone();
    };

    tracing::info!(
        legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
        area_resref = resource_info.resref.as_str(),
        area_name = resource_info.name.as_str(),
        primary_area_name_length = primary.area_name_length,
        "Area_ClientArea source layout switched from false CExoString to Diamond no-name after exact local resource proof"
    );
    no_name_layout
}

fn collect_area_post_tile_placeable_context(
    payload: &[u8],
    fragment_offset: usize,
    area_resref: &str,
    legacy_area_object_id: u32,
    filter_ambiguous_context_rows: bool,
    module_resource_info: Option<&ModuleAreaResourceInfo>,
) -> Option<AreaPlaceableContext> {
    let scan = scan_area_tile_stream(payload, fragment_offset);
    if !scan.valid {
        return None;
    }
    let source_tail_proof =
        legacy_area_source_tail_exact_read_proof(payload, fragment_offset, &scan)?;

    let mut cursor = scan.tile_end_read_offset;

    let fragment_bits_available = area_payload_fragment_bits_available(payload, fragment_offset)?;
    let (_, next_cursor, next_bit_cursor) = advance_area_transition_rows(
        payload,
        fragment_offset,
        cursor,
        LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS,
        fragment_bits_available,
    )?;
    cursor = next_cursor;

    let (_, next_cursor) = advance_area_map_pin_rows(payload, fragment_offset, cursor)?;
    cursor = next_cursor;

    let (_, next_cursor, _) = advance_area_sound_rows(
        payload,
        fragment_offset,
        cursor,
        next_bit_cursor,
        fragment_bits_available,
    )?;
    cursor = next_cursor;

    let mut light_rows = Vec::new();
    let (_, next_cursor) = walk_area_light_placeable_rows(
        payload,
        fragment_offset,
        cursor,
        |row| {
            if area_placeable_context_id_is_ambiguous(
                row.object_id,
                legacy_area_object_id,
                filter_ambiguous_context_rows,
                &light_rows,
                &[],
            ) {
                tracing::debug!(
                    area_resref,
                    object_id = row.object_id,
                    legacy_area_object_id,
                    "Area_ClientArea placeable context retained area-alias light-placeable row identity"
                );
            }
            light_rows.push(row);
            Some(())
        },
    )?;
    cursor = next_cursor;

    let static_count = read_area_u16(payload, fragment_offset, cursor)?;
    if cursor != source_tail_proof.static_count_read_offset {
        return None;
    }
    if source_tail_proof.zero_static_placeable_rows == 0 {
        if static_count != source_tail_proof.static_rows_count {
            return None;
        }
    } else if static_count != 0 {
        return None;
    }
    cursor = cursor.checked_add(2)?;
    if cursor != source_tail_proof.static_rows_read_offset {
        return None;
    }
    let module_context_claims = module_static_placeable_context_claims(
        payload,
        fragment_offset,
        &source_tail_proof,
        module_resource_info,
    )
    .unwrap_or_default();
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
        let module_state = module_context_claims.iter().find_map(|claim| {
            (claim.row.cursor == cursor
                && module_static_placeable_row_claim_matches_source(
                    payload,
                    fragment_offset,
                    &claim.row,
                )?)
            .then_some(claim.state)
        });
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
            object_id_confidence: AreaPlaceableContextObjectIdConfidence::Unique,
            module_state,
        });
        cursor = cursor.checked_add(4 + 2 + 6 * 4)?;
    }
    mark_area_placeable_context_object_id_confidence(
        legacy_area_object_id,
        &mut light_rows,
        &mut static_rows,
    );

    Some(AreaPlaceableContext {
        area_resref: area_resref.to_string(),
        light_rows,
        static_rows,
    })
}

fn module_static_placeable_context_claims(
    payload: &[u8],
    fragment_offset: usize,
    proof: &LegacyAreaSourceTailProof,
    module_resource_info: Option<&ModuleAreaResourceInfo>,
) -> Option<Vec<ModuleStaticPlaceableContextClaim>> {
    let info = module_resource_info?;
    let static_placeables = info
        .placeables
        .iter()
        .filter(|placeable| {
            placeable.static_object && module_static_placeable_resource_row_safe(placeable)
        })
        .collect::<Vec<_>>();
    let row_claims = unique_module_static_placeable_row_matches(
        payload,
        fragment_offset,
        proof,
        &static_placeables,
    )?;
    let mut context_claims = Vec::with_capacity(row_claims.len());
    for claim in row_claims {
        let placeable = static_placeables[claim.placeable_index];
        if !module_static_placeable_row_claim_direction_matches_resource(&claim, placeable)? {
            return None;
        }
        context_claims.push(ModuleStaticPlaceableContextClaim {
            row: claim,
            state: area_placeable_context_state_from_module_placeable(placeable),
        });
    }

    Some(context_claims)
}

#[cfg(test)]
fn module_static_placeable_context_state(
    module_resource_info: Option<&ModuleAreaResourceInfo>,
    appearance: u16,
    x: f32,
    y: f32,
    z: f32,
    dir_x: f32,
    dir_y: f32,
    dir_z: f32,
) -> Option<AreaPlaceableContextState> {
    // The static-row state handoff to later live-object diagnostics is only
    // sound when the row proves the decompiled static mesh orientation too.
    // EE `CNWCArea::LoadArea` passes the first triplet as placement and the
    // second triplet into the static helper's yaw path; Diamond follows the
    // same two-triplet row shape. Appearance plus nearby coordinates are not
    // enough to identify the original GIT row's lock/trap/useable state.
    let static_placeables = module_resource_info?
        .placeables
        .iter()
        .filter(|placeable| {
            if !placeable.static_object
                || !module_static_placeable_resource_row_safe(placeable)
                || !module_static_placeable_row_matches_resource(appearance, x, y, z, placeable)
            {
                return false;
            }
            let Some((expected_x, expected_y, expected_z)) =
                static_placeable_direction_from_bearing(placeable.bearing)
            else {
                return false;
            };
            area_float_close(dir_x, expected_x, 0.01)
                && area_float_close(dir_y, expected_y, 0.01)
                && area_float_close(dir_z, expected_z, 0.01)
        })
        .collect::<Vec<_>>();
    if static_placeables.len() != 1 {
        return None;
    }

    Some(area_placeable_context_state_from_module_placeable(
        static_placeables[0],
    ))
}

fn module_static_placeable_row_claim_direction_matches_resource(
    claim: &ModuleStaticPlaceableRowClaim,
    placeable: &ModuleAreaPlaceable,
) -> Option<bool> {
    let (expected_x, expected_y, expected_z) =
        static_placeable_direction_from_bearing(placeable.bearing)?;
    Some(
        area_float_close(claim.dir_x, expected_x, 0.01)
            && area_float_close(claim.dir_y, expected_y, 0.01)
            && area_float_close(claim.dir_z, expected_z, 0.01),
    )
}

fn area_placeable_context_state_from_module_placeable(
    placeable: &ModuleAreaPlaceable,
) -> AreaPlaceableContextState {
    AreaPlaceableContextState {
        static_object: placeable.static_object,
        useable: placeable.useable,
        trap_flag: placeable.trap_flag,
        trap_disarmable: placeable.trap_disarmable,
        lockable: placeable.lockable,
        locked: placeable.locked,
    }
}

fn mark_area_placeable_context_object_id_confidence(
    legacy_area_object_id: u32,
    light_rows: &mut [AreaPlaceableContextRow],
    static_rows: &mut [AreaPlaceableContextRow],
) {
    let mut object_id_counts = HashMap::<u32, usize>::new();
    for object_id in light_rows
        .iter()
        .chain(static_rows.iter())
        .map(|row| row.object_id)
    {
        *object_id_counts.entry(object_id).or_default() += 1;
    }

    for row in light_rows.iter_mut().chain(static_rows.iter_mut()) {
        let area_alias = row.object_id == legacy_area_object_id;
        let duplicate = object_id_counts
            .get(&row.object_id)
            .copied()
            .is_some_and(|count| count > 1);
        row.object_id_confidence = match (area_alias, duplicate) {
            (false, false) => AreaPlaceableContextObjectIdConfidence::Unique,
            (true, false) => AreaPlaceableContextObjectIdConfidence::AreaObjectAlias,
            (false, true) => AreaPlaceableContextObjectIdConfidence::DuplicateObjectId,
            (true, true) => AreaPlaceableContextObjectIdConfidence::AreaObjectAliasDuplicate,
        };
        if !row.object_id_confidence.is_unique() {
            row.module_state = None;
        }
    }
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

fn advance_area_transition_rows(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
    bit_cursor: usize,
    fragment_bits_available: usize,
) -> Option<(u32, usize, usize)> {
    let transition_count = read_area_u32(payload, fragment_offset, cursor)?;
    if transition_count > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    let mut cursor = cursor.checked_add(4)?;
    let mut bit_cursor = bit_cursor;
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

        // EE and Diamond both write a byte-buffer body followed by fragment
        // bits for the transition label: one visibility BOOL, the
        // CExoLocString TLK/direct selector, and one extra BOOL before the
        // DWORD TLK branch. Keep every area tail walker on this same cursor
        // contract so later map/sound/static rows cannot be shifted by a
        // plausible-looking transition name.
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
        if bit_cursor > fragment_bits_available
            || HIGH_LEVEL_HEADER_BYTES + cursor > fragment_offset
        {
            return None;
        }
    }

    Some((transition_count, cursor, bit_cursor))
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

    let (_, next_cursor, next_bit_cursor) = advance_area_transition_rows(
        payload,
        fragment_offset,
        cursor,
        bit_cursor,
        fragment_bits_available,
    )?;
    cursor = next_cursor;
    bit_cursor = next_bit_cursor;

    let (_map_pin_count, after_map_pins) =
        advance_area_map_pin_rows(payload, fragment_offset, cursor)?;
    cursor = after_map_pins;

    let (sound_count, next_cursor, next_bit_cursor) = advance_area_sound_rows(
        payload,
        fragment_offset,
        cursor,
        bit_cursor,
        fragment_bits_available,
    )?;
    cursor = next_cursor;
    bit_cursor = next_bit_cursor;

    let (_, next_cursor) =
        walk_area_light_placeable_rows(payload, fragment_offset, cursor, |_| Some(()))?;
    cursor = next_cursor;

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
        sound_count,
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
        if !value.is_finite() || value.abs() > MAX_STATIC_PLACEABLE_COMPONENT_ABS {
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
    if proof.zero_static_placeable_rows != 0 {
        // A zero static-placeable WORD means the decompiled area reader does
        // not consume following row-shaped bytes.  The drop helper owns that
        // legacy tail; do not normalize it into claimed semantic rows.
        return Some(0);
    }
    if proof.static_rows_count == 0 {
        return Some(0);
    }

    let mut candidate = payload.to_vec();
    let mut normalized = 0u32;
    let mut cursor = proof.static_rows_read_offset;
    for _ in 0..proof.static_rows_count {
        let dir_x = read_area_f32(&candidate, fragment_offset, cursor + 18)?;
        let dir_y = read_area_f32(&candidate, fragment_offset, cursor + 22)?;
        let dir_z = read_area_f32(&candidate, fragment_offset, cursor + 26)?;
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
            write_area_f32(&mut candidate, fragment_offset, cursor + 18, new_x)?;
            write_area_f32(&mut candidate, fragment_offset, cursor + 22, new_y)?;
            write_area_f32(&mut candidate, fragment_offset, cursor + 26, new_z)?;
            normalized = normalized.checked_add(1)?;
        }
        cursor = cursor.checked_add(AREA_STATIC_PLACEABLE_ROW_BYTES)?;
    }

    let repaired_proof =
        legacy_area_source_tail_exact_read_proof(&candidate, fragment_offset, scan)?;
    if repaired_proof.static_rows_read_offset != proof.static_rows_read_offset
        || repaired_proof.static_rows_count != proof.static_rows_count
        || repaired_proof.zero_static_placeable_rows != proof.zero_static_placeable_rows
    {
        return None;
    }
    payload.copy_from_slice(&candidate);

    Some(normalized)
}

fn repair_module_resource_static_placeable_rows(
    payload: &mut [u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
    info: &ModuleAreaResourceInfo,
) -> Option<u32> {
    if !scan.valid || info.placeables.is_empty() {
        return Some(0);
    }

    let static_placeables = info
        .placeables
        .iter()
        .filter(|placeable| {
            placeable.static_object && module_static_placeable_resource_row_safe(placeable)
        })
        .collect::<Vec<_>>();
    let proof = legacy_area_source_tail_exact_read_proof(payload, fragment_offset, scan)?;
    if proof.zero_static_placeable_rows != 0 {
        // Row-shaped bytes after a zero count are outside the Diamond/EE static
        // list contract.  They may be dropped, but module GIT evidence must not
        // promote or rewrite them as claimed static rows.
        return Some(0);
    }
    if proof.static_rows_count == 0
        || static_placeables.len() != usize::from(proof.static_rows_count)
    {
        return Some(0);
    }

    let Some(matches) = unique_module_static_placeable_row_matches(
        payload,
        fragment_offset,
        &proof,
        &static_placeables,
    ) else {
        return Some(0);
    };

    let mut candidate = payload.to_vec();
    let mut repaired = 0u32;
    for claim in matches {
        let placeable = static_placeables[claim.placeable_index];
        if !module_static_placeable_row_claim_matches_payload(
            &candidate,
            fragment_offset,
            &claim,
            placeable,
        )? {
            return None;
        }
        let (dir_x, dir_y, dir_z) = static_placeable_direction_from_bearing(placeable.bearing)?;
        let changed = claim.appearance != placeable.appearance
            || !same_f32_bits(claim.x, placeable.x)
            || !same_f32_bits(claim.y, placeable.y)
            || !same_f32_bits(claim.z, placeable.z)
            || !same_f32_bits(claim.dir_x, dir_x)
            || !same_f32_bits(claim.dir_y, dir_y)
            || !same_f32_bits(claim.dir_z, dir_z);
        write_area_u16(
            &mut candidate,
            fragment_offset,
            claim.cursor.checked_add(4)?,
            placeable.appearance,
        )?;
        write_area_f32(
            &mut candidate,
            fragment_offset,
            claim.cursor.checked_add(6)?,
            placeable.x,
        )?;
        write_area_f32(
            &mut candidate,
            fragment_offset,
            claim.cursor.checked_add(10)?,
            placeable.y,
        )?;
        write_area_f32(
            &mut candidate,
            fragment_offset,
            claim.cursor.checked_add(14)?,
            placeable.z,
        )?;
        write_area_f32(
            &mut candidate,
            fragment_offset,
            claim.cursor.checked_add(18)?,
            dir_x,
        )?;
        write_area_f32(
            &mut candidate,
            fragment_offset,
            claim.cursor.checked_add(22)?,
            dir_y,
        )?;
        write_area_f32(
            &mut candidate,
            fragment_offset,
            claim.cursor.checked_add(26)?,
            dir_z,
        )?;
        if changed {
            repaired = repaired.checked_add(1)?;
        }
    }

    let repaired_proof =
        legacy_area_source_tail_exact_read_proof(&candidate, fragment_offset, scan)?;
    if repaired_proof.static_rows_read_offset != proof.static_rows_read_offset
        || repaired_proof.static_rows_count != proof.static_rows_count
        || repaired_proof.zero_static_placeable_rows != proof.zero_static_placeable_rows
    {
        return None;
    }
    payload.copy_from_slice(&candidate);

    Some(repaired)
}

fn unique_module_static_placeable_row_matches(
    payload: &[u8],
    fragment_offset: usize,
    proof: &LegacyAreaSourceTailProof,
    static_placeables: &[&ModuleAreaPlaceable],
) -> Option<Vec<ModuleStaticPlaceableRowClaim>> {
    if proof.zero_static_placeable_rows != 0
        || static_placeables.len() != usize::from(proof.static_rows_count)
    {
        return None;
    }

    let mut remaining = (0..static_placeables.len()).collect::<Vec<_>>();
    let mut matches = Vec::with_capacity(static_placeables.len());
    let mut cursor = proof.static_rows_read_offset;
    for _ in 0..proof.static_rows_count {
        let candidates = remaining
            .iter()
            .copied()
            .filter_map(|index| {
                module_static_placeable_row_claim(
                    payload,
                    fragment_offset,
                    cursor,
                    index,
                    static_placeables[index],
                )
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return None;
        }
        let matched = candidates[0];
        matches.push(matched);
        remaining.retain(|index| *index != matched.placeable_index);
        cursor = cursor.checked_add(AREA_STATIC_PLACEABLE_ROW_BYTES)?;
    }

    Some(matches)
}

fn module_static_placeable_row_claim(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
    placeable_index: usize,
    placeable: &ModuleAreaPlaceable,
) -> Option<ModuleStaticPlaceableRowClaim> {
    let object_id = read_area_u32(payload, fragment_offset, cursor)?;
    let appearance = read_area_u16(payload, fragment_offset, cursor.checked_add(4)?)?;
    let x = read_area_f32(payload, fragment_offset, cursor.checked_add(6)?)?;
    let y = read_area_f32(payload, fragment_offset, cursor.checked_add(10)?)?;
    let z = read_area_f32(payload, fragment_offset, cursor.checked_add(14)?)?;
    let dir_x = read_area_f32(payload, fragment_offset, cursor.checked_add(18)?)?;
    let dir_y = read_area_f32(payload, fragment_offset, cursor.checked_add(22)?)?;
    let dir_z = read_area_f32(payload, fragment_offset, cursor.checked_add(26)?)?;
    let match_kind = module_static_placeable_row_match_kind(appearance, x, y, z, placeable)?;
    Some(ModuleStaticPlaceableRowClaim {
        cursor,
        placeable_index,
        object_id,
        appearance,
        x,
        y,
        z,
        dir_x,
        dir_y,
        dir_z,
        match_kind,
    })
}

fn module_static_placeable_row_claim_matches_source(
    payload: &[u8],
    fragment_offset: usize,
    claim: &ModuleStaticPlaceableRowClaim,
) -> Option<bool> {
    let object_id = read_area_u32(payload, fragment_offset, claim.cursor)?;
    let appearance = read_area_u16(payload, fragment_offset, claim.cursor.checked_add(4)?)?;
    let x = read_area_f32(payload, fragment_offset, claim.cursor.checked_add(6)?)?;
    let y = read_area_f32(payload, fragment_offset, claim.cursor.checked_add(10)?)?;
    let z = read_area_f32(payload, fragment_offset, claim.cursor.checked_add(14)?)?;
    let dir_x = read_area_f32(payload, fragment_offset, claim.cursor.checked_add(18)?)?;
    let dir_y = read_area_f32(payload, fragment_offset, claim.cursor.checked_add(22)?)?;
    let dir_z = read_area_f32(payload, fragment_offset, claim.cursor.checked_add(26)?)?;
    Some(
        object_id == claim.object_id
            && appearance == claim.appearance
            && same_f32_bits(x, claim.x)
            && same_f32_bits(y, claim.y)
            && same_f32_bits(z, claim.z)
            && same_f32_bits(dir_x, claim.dir_x)
            && same_f32_bits(dir_y, claim.dir_y)
            && same_f32_bits(dir_z, claim.dir_z),
    )
}

fn module_static_placeable_row_claim_matches_payload(
    payload: &[u8],
    fragment_offset: usize,
    claim: &ModuleStaticPlaceableRowClaim,
    placeable: &ModuleAreaPlaceable,
) -> Option<bool> {
    let current = module_static_placeable_row_claim(
        payload,
        fragment_offset,
        claim.cursor,
        claim.placeable_index,
        placeable,
    )?;
    Some(
        module_static_placeable_row_claim_matches_source(payload, fragment_offset, claim)?
            && current.match_kind == claim.match_kind
            && current.object_id == claim.object_id
            && current.appearance == claim.appearance
            && same_f32_bits(current.x, claim.x)
            && same_f32_bits(current.y, claim.y)
            && same_f32_bits(current.z, claim.z)
            && same_f32_bits(current.dir_x, claim.dir_x)
            && same_f32_bits(current.dir_y, claim.dir_y)
            && same_f32_bits(current.dir_z, claim.dir_z),
    )
}

#[cfg(test)]
fn module_static_placeable_row_matches_resource(
    appearance: u16,
    x: f32,
    y: f32,
    z: f32,
    placeable: &ModuleAreaPlaceable,
) -> bool {
    module_static_placeable_row_match_kind(appearance, x, y, z, placeable).is_some()
}

fn module_static_placeable_row_match_kind(
    appearance: u16,
    x: f32,
    y: f32,
    z: f32,
    placeable: &ModuleAreaPlaceable,
) -> Option<ModuleStaticPlaceableRowMatchKind> {
    // The static row itself contains no tag/resref.  Module-backed repair may
    // only use the GIT row as the missing semantic authority after the packet
    // proves the decompiled static row shape and the row has enough placement
    // coordinates to identify one static GIT row. A legacy zero appearance is
    // treated as missing only when all placement coordinates match; nonzero
    // appearances must agree with the resource appearance.
    const TOLERANCE: f32 = 0.01;
    let component_matches = [
        area_float_close(x, placeable.x, TOLERANCE),
        area_float_close(y, placeable.y, TOLERANCE),
        area_float_close(z, placeable.z, TOLERANCE),
    ];
    let matching_components = component_matches
        .into_iter()
        .filter(|matches| *matches)
        .count();
    if appearance == placeable.appearance {
        return (matching_components >= 2)
            .then_some(ModuleStaticPlaceableRowMatchKind::ExactAppearanceAtLeastTwoCoordinates);
    }
    (appearance == 0 && component_matches.into_iter().all(|matches| matches))
        .then_some(ModuleStaticPlaceableRowMatchKind::ZeroAppearanceAllCoordinates)
}

fn module_static_placeable_resource_row_safe(placeable: &ModuleAreaPlaceable) -> bool {
    // Local GIT rows are evidence for repairing a packet row only when their
    // replacement values still live inside the same decompiled static-row value
    // domain that the packet proof accepts. Otherwise malformed module data can
    // turn an appearance/two-coordinate match into a failed cursor rewrite or a
    // false trap/use/lock state handoff.
    [placeable.x, placeable.y, placeable.z, placeable.bearing]
        .into_iter()
        .all(f32::is_finite)
        && [placeable.x, placeable.y, placeable.z]
            .into_iter()
            .all(|value| value.abs() <= MAX_STATIC_PLACEABLE_COMPONENT_ABS)
        && static_placeable_direction_from_bearing(placeable.bearing).is_some()
}

fn area_float_close(actual: f32, expected: f32, tolerance: f32) -> bool {
    (actual - expected).abs() <= tolerance
}

fn static_placeable_direction_from_bearing(bearing: f32) -> Option<(f32, f32, f32)> {
    bearing
        .is_finite()
        .then_some((-bearing.sin(), bearing.cos(), 0.0))
}

fn normalize_static_placeable_direction_components(
    dir_x: f32,
    dir_y: f32,
    dir_z: f32,
) -> Option<(f32, f32, f32)> {
    if !dir_x.is_finite()
        || !dir_y.is_finite()
        || !dir_z.is_finite()
        || dir_x.abs() > MAX_STATIC_PLACEABLE_COMPONENT_ABS
        || dir_y.abs() > MAX_STATIC_PLACEABLE_COMPONENT_ABS
        || dir_z.abs() > MAX_STATIC_PLACEABLE_COMPONENT_ABS
    {
        return None;
    }

    let horizontal_len_sq = dir_x.mul_add(dir_x, dir_y * dir_y);
    if !horizontal_len_sq.is_finite() {
        return None;
    }
    if horizontal_len_sq <= 1.0e-12 {
        // A zero-length direction vector has no decompile-preserving yaw. Do
        // not invent north here; a module-resource repair may still supply the
        // original GIT bearing before the final EE reader proof runs.
        return None;
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

    let (transition_count, next_cursor, next_bit_cursor) = advance_area_transition_rows(
        payload,
        fragment_offset,
        cursor,
        bit_cursor,
        fragment_bits_available,
    )?;
    cursor = next_cursor;
    bit_cursor = next_bit_cursor;

    let (map_pin_count, after_map_pins) =
        advance_area_map_pin_rows(payload, fragment_offset, cursor)?;
    cursor = after_map_pins;

    let (sound_count, next_cursor, next_bit_cursor) = advance_area_sound_rows(
        payload,
        fragment_offset,
        cursor,
        bit_cursor,
        fragment_bits_available,
    )?;
    cursor = next_cursor;
    bit_cursor = next_bit_cursor;

    let (light_count, next_cursor) =
        walk_area_light_placeable_rows(payload, fragment_offset, cursor, |_| Some(()))?;
    cursor = next_cursor;
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
    if first_post_static_count != 0 {
        // The bridge-owned EE dialect for legacy Area_ClientArea packets adds
        // two post-static zero WORDs.  No non-empty first-list row shape has
        // been proven from Diamond/EE readers yet, so accepting one here would
        // let a shifted static-placeable cursor masquerade as an exact EE
        // packet by skipping arbitrary WORDs.
        return None;
    }
    cursor = cursor.checked_add(2)?;

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

fn advance_area_map_pin_rows(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
) -> Option<(u32, usize)> {
    let map_pin_count = read_area_u32(payload, fragment_offset, cursor)?;
    if map_pin_count > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    let mut cursor = cursor.checked_add(4)?;
    for _ in 0..map_pin_count {
        // EE/Diamond post-tile map-pin rows are byte-buffer only in the
        // decompiled Area_ClientArea reader/writer path: DWORD pin id, bounded
        // CExoString label, then three FLOAT coordinates. They do not consume
        // CNW fragment bits, unlike transition locstrings and sound rows.
        let pin_id = read_area_u32(payload, fragment_offset, cursor)?;
        if pin_id > MAX_AREA_POST_TILE_LIST_COUNT {
            return None;
        }
        let Some((_, after_label)) =
            read_c_exo_string_shape(payload, fragment_offset, cursor.checked_add(4)?, 4096)
        else {
            return None;
        };
        for component in 0..3 {
            let Some(value) = read_area_f32(
                payload,
                fragment_offset,
                after_label.checked_add(component * 4)?,
            ) else {
                return None;
            };
            if !value.is_finite() || value.abs() > 100_000.0 {
                return None;
            }
        }
        cursor = after_label.checked_add(3 * 4)?;
        if HIGH_LEVEL_HEADER_BYTES.checked_add(cursor)? > fragment_offset {
            return None;
        }
    }

    Some((map_pin_count, cursor))
}

fn advance_area_sound_rows(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
    bit_cursor: usize,
    fragment_bits_available: usize,
) -> Option<(u16, usize, usize)> {
    let sound_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(sound_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    let mut cursor = cursor.checked_add(2)?;
    let mut bit_cursor = bit_cursor;

    for _ in 0..sound_count {
        let resref_count = read_area_u16(
            payload,
            fragment_offset,
            cursor.checked_add(AREA_SOUND_RESREF_COUNT_OFFSET)?,
        )?;
        if resref_count > MAX_AREA_SOUND_RESREFS {
            return None;
        }

        // EE `CNWCArea::LoadArea` calls the same sound-object reader shape
        // Diamond uses here: a fixed 54-byte byte-buffer body, `WORD`
        // CResRef count, repeated 16-byte CResRefs, and six fragment BOOLs.
        // Keep every post-tile walker on that byte/bit cursor contract before
        // exposing later light/static placeable rows.
        bit_cursor = bit_cursor.checked_add(6)?;
        if bit_cursor > fragment_bits_available {
            return None;
        }
        cursor = cursor.checked_add(
            AREA_SOUND_BASE_BYTES.checked_add(resref_count as usize * CRESREF_TEXT_BYTES)?,
        )?;
        if HIGH_LEVEL_HEADER_BYTES.checked_add(cursor)? > fragment_offset {
            return None;
        }
    }

    Some((sound_count, cursor, bit_cursor))
}

fn walk_area_light_placeable_rows<F>(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
    mut on_row: F,
) -> Option<(u16, usize)>
where
    F: FnMut(AreaPlaceableContextRow) -> Option<()>,
{
    let light_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(light_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    let mut cursor = cursor.checked_add(2)?;
    for _ in 0..light_count {
        // EE `CNWSArea::PackAreaIntoMessage` emits the light-placeable list as
        // a byte-only WORD count followed by `OBJECTID`, `WORD appearance`, and
        // one position triplet. EE `CNWCArea::LoadArea` feeds exactly that row
        // into the light helper, and Diamond's reader has the same one-triplet
        // branch before static placeables. No CNW fragment BOOLs are consumed
        // here; sharing this walker keeps later static rows from being exposed
        // after an unproven light-row cursor.
        let row = read_area_light_placeable_row(payload, fragment_offset, cursor)?;
        on_row(row)?;
        cursor = cursor.checked_add(AREA_LIGHT_PLACEABLE_ROW_BYTES)?;
        if HIGH_LEVEL_HEADER_BYTES.checked_add(cursor)? > fragment_offset {
            return None;
        }
    }

    Some((light_count, cursor))
}

fn read_area_light_placeable_row(
    payload: &[u8],
    fragment_offset: usize,
    cursor: usize,
) -> Option<AreaPlaceableContextRow> {
    let object_id = read_area_u32(payload, fragment_offset, cursor)?;
    if !legacy_area_object_id_plausible(object_id) {
        return None;
    }
    let appearance = read_area_u16(payload, fragment_offset, cursor + 4)?;
    let x = read_area_f32(payload, fragment_offset, cursor + 6)?;
    let y = read_area_f32(payload, fragment_offset, cursor + 10)?;
    let z = read_area_f32(payload, fragment_offset, cursor + 14)?;
    for value in [x, y, z] {
        if !value.is_finite() || value.abs() > 100_000.0 {
            return None;
        }
    }

    Some(AreaPlaceableContextRow {
        object_id,
        appearance,
        x,
        y,
        z,
        has_direction: false,
        ..AreaPlaceableContextRow::default()
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
    // Missing dimension repairs are safe only when the inferred width/height
    // still hand off to the exact decompile-owned post-tile cursor.
    let mut candidate = payload.to_vec();
    if write_u32_le(&mut candidate, height_payload_offset, scan.inferred_height).is_none() {
        false
    } else {
        let repaired_scan = scan_area_tile_stream(&candidate, fragment_offset);
        if !repaired_missing_dimension_scan_matches(scan, &repaired_scan)
            || !legacy_area_source_tail_consumes_read_buffer(
                &candidate,
                fragment_offset,
                &repaired_scan,
            )
        {
            return false;
        }
        payload.copy_from_slice(&candidate);
        *scan = repaired_scan;
        true
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
    let mut candidate = payload.to_vec();
    if write_u32_le(&mut candidate, width_payload_offset, legacy_scan.width).is_none() {
        return false;
    }

    let repaired_scan = scan_area_tile_stream(&candidate, fragment_offset);
    if !repaired_missing_dimension_scan_matches(&legacy_scan, &repaired_scan)
        || !legacy_area_source_tail_consumes_read_buffer(
            &candidate,
            fragment_offset,
            &repaired_scan,
        )
    {
        return false;
    }
    payload.copy_from_slice(&candidate);
    *scan = repaired_scan;
    true
}

fn repaired_missing_dimension_scan_matches(
    source_scan: &AreaTileStreamScan,
    repaired_scan: &AreaTileStreamScan,
) -> bool {
    repaired_scan.valid
        && repaired_scan.layout.first_tile_read_offset == source_scan.layout.first_tile_read_offset
        && repaired_scan.layout.tileset_read_offset == source_scan.layout.tileset_read_offset
        && repaired_scan.width == source_scan.width
        && repaired_scan.packet_height == source_scan.inferred_height
        && repaired_scan.inferred_height == source_scan.inferred_height
        && repaired_scan.tile_count == source_scan.tile_count
        && repaired_scan.tile_end_read_offset == source_scan.tile_end_read_offset
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

        let mut candidate = payload.to_vec();
        if write_area_u32(
            &mut candidate,
            fragment_offset,
            layout.width_read_offset,
            side,
        )
        .is_none()
            || write_area_u32(
                &mut candidate,
                fragment_offset,
                layout.height_read_offset,
                side,
            )
            .is_none()
        {
            return false;
        }

        let repaired_scan = scan_area_tile_stream(&candidate, fragment_offset);
        if repaired_missing_square_dimension_scan_matches(
            &layout,
            tile_count,
            cursor,
            side,
            &repaired_scan,
        ) && legacy_area_source_tail_consumes_read_buffer(
            &candidate,
            fragment_offset,
            &repaired_scan,
        ) {
            payload.copy_from_slice(&candidate);
            *scan = repaired_scan;
            return true;
        }
    }

    false
}

fn repaired_missing_square_dimension_scan_matches(
    source_layout: &AreaStaticLayout,
    source_tile_count: u32,
    source_tile_end_read_offset: usize,
    side: u32,
    repaired_scan: &AreaTileStreamScan,
) -> bool {
    repaired_scan.valid
        && repaired_scan.layout.width_read_offset == source_layout.width_read_offset
        && repaired_scan.layout.height_read_offset == source_layout.height_read_offset
        && repaired_scan.layout.tileset_read_offset == source_layout.tileset_read_offset
        && repaired_scan.layout.first_tile_read_offset == source_layout.first_tile_read_offset
        && repaired_scan.width == side
        && repaired_scan.packet_height == side
        && repaired_scan.inferred_height == side
        && repaired_scan.tile_count == source_tile_count
        && repaired_scan.tile_end_read_offset == source_tile_end_read_offset
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
    // that shape, so convert only this proven row-local legacy encoding after
    // the shared transition/map-pin walkers prove the sound-list cursor.
    // Multi-sound rows already carry their true count and are left untouched.
    let fragment = payload.get(fragment_offset..)?;
    let fragment_bits_available = cnw_fragment_consumable_bits(fragment)?;
    let bit_cursor = LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS;
    if bit_cursor > fragment_bits_available {
        return None;
    }

    let mut cursor = scan.tile_end_read_offset;
    let (_, next_cursor, _) = advance_area_transition_rows(
        payload,
        fragment_offset,
        cursor,
        bit_cursor,
        fragment_bits_available,
    )?;
    cursor = next_cursor;

    let (_, next_cursor) = advance_area_map_pin_rows(payload, fragment_offset, cursor)?;
    cursor = next_cursor;

    let sound_count = read_area_u16(payload, fragment_offset, cursor)?;
    if u32::from(sound_count) > MAX_AREA_POST_TILE_LIST_COUNT {
        return None;
    }
    cursor = cursor.checked_add(2)?;

    let mut repair_count_offsets = Vec::new();
    for _ in 0..sound_count {
        let count_offset = cursor.checked_add(AREA_SOUND_RESREF_COUNT_OFFSET)?;
        let resref_count = read_area_u16(payload, fragment_offset, count_offset)?;
        let effective_count = if resref_count == 0
            && fixed_cresref_at_read_offset_plausible(
                payload,
                fragment_offset,
                cursor.checked_add(AREA_SOUND_BASE_BYTES)?,
            ) {
            repair_count_offsets.push(count_offset);
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

    let repairs = u32::try_from(repair_count_offsets.len()).ok()?;
    if repair_count_offsets.is_empty() {
        return Some(0);
    }

    // Stage row-local count repairs and then re-run the full post-tile proof.
    // A zero-count compact sound row is owned only when the repaired row also
    // lands on the decompiled six-BOOL sound cursor and exact following lists.
    let mut candidate = payload.to_vec();
    for count_offset in repair_count_offsets {
        write_area_u16(&mut candidate, fragment_offset, count_offset, 1)?;
    }
    let candidate_scan = scan_area_tile_stream(&candidate, fragment_offset);
    if !candidate_scan.valid
        || candidate_scan.tile_end_read_offset != scan.tile_end_read_offset
        || candidate_scan.width != scan.width
        || candidate_scan.packet_height != scan.packet_height
        || candidate_scan.tile_count != scan.tile_count
        || legacy_area_source_tail_exact_read_proof(&candidate, fragment_offset, &candidate_scan)
            .is_none()
    {
        return None;
    }
    payload.copy_from_slice(&candidate);

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
    let mut candidate = payload.clone();
    let fragment = candidate.get(fragment_offset..)?.to_vec();
    candidate.truncate(HIGH_LEVEL_HEADER_BYTES.checked_add(proof.static_rows_read_offset)?);
    candidate.extend_from_slice(&fragment);
    let new_declared = (HIGH_LEVEL_HEADER_BYTES + proof.static_rows_read_offset) as u32;
    write_u32_le(&mut candidate, HIGH_LEVEL_HEADER_BYTES, new_declared)?;

    let new_fragment_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(proof.static_rows_read_offset)?;
    let trimmed_scan = scan_area_tile_stream(&candidate, new_fragment_offset);
    if !trimmed_scan.valid
        || trimmed_scan.tile_end_read_offset != scan.tile_end_read_offset
        || trimmed_scan.width != scan.width
        || trimmed_scan.packet_height != scan.packet_height
        || trimmed_scan.inferred_height != scan.inferred_height
        || trimmed_scan.tile_count != scan.tile_count
    {
        return None;
    }
    let trimmed_proof =
        legacy_area_source_tail_exact_read_proof(&candidate, new_fragment_offset, &trimmed_scan)?;
    if trimmed_proof.zero_static_placeable_rows != 0 || trimmed_proof.static_rows_count != 0 {
        return None;
    }
    *payload = candidate;

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
    layout: &AreaStaticLayout,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
) -> Option<ModuleAreaResourceInfo> {
    let owned_context = if module_context.is_none() {
        crate::translate::module::observed_module_context()
    } else {
        None
    };
    let observed_context = module_context.or(owned_context.as_ref());
    let fallback_context = if observed_context.is_none() {
        if layout.area_name_encoding != AreaNameEncoding::DiamondNoAreaName {
            tracing::debug!(
                legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
                "Area_ClientArea compact resource repair skipped: no observed Module_Info context"
            );
            return None;
        }
        tracing::debug!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            "Area_ClientArea compact resource repair using packet-local no-name CResRef without observed Module_Info context"
        );
        Some(crate::translate::module::ObservedModuleContext {
            localized_name: String::new(),
            module_resref: String::new(),
            areas: Vec::new(),
        })
    } else {
        None
    };
    let context = observed_context
        .or(fallback_context.as_ref())
        .expect("observed or fallback Module_Info context should be available");
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
    }
    let compact_fragments = match layout.area_name_encoding {
        AreaNameEncoding::DiamondCompactFragmented => {
            diamond_compact_area_name_fragments(payload, fragment_offset)
        }
        AreaNameEncoding::DiamondFixed21 => {
            diamond_long_fixed_area_name_fragments(payload, fragment_offset)
        }
        AreaNameEncoding::DiamondFixed20 => {
            diamond_fixed_area_name_fragments(payload, fragment_offset)
        }
        AreaNameEncoding::DiamondFixed16 => {
            diamond_short_fixed_area_name_fragments(payload, fragment_offset)
        }
        AreaNameEncoding::CExoString => {
            diamond_cexo_string_area_name_fragments(payload, fragment_offset)
        }
        AreaNameEncoding::DiamondNoAreaName => None,
    };
    let packet_area_resref_fragments =
        if layout.area_name_encoding == AreaNameEncoding::DiamondNoAreaName {
            compact_packet_area_resref_fragments(payload, fragment_offset)
        } else {
            None
        };
    let packet_area_resref = fixed_resref_preview(
        payload,
        LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES,
    )
    .filter(|resref| area_resref_plausible(resref));
    if compact_fragments.is_none()
        && packet_area_resref.is_none()
        && packet_area_resref_fragments.is_none()
    {
        tracing::debug!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            area_indices = ?area_indices,
            "Area_ClientArea compact resource repair skipped: compact area-name fragments and packet area resref were unavailable"
        );
        return None;
    }
    let direct_resref_module_path = packet_area_resref.as_deref().and_then(|resref| {
        if layout.area_name_encoding == AreaNameEncoding::DiamondNoAreaName {
            observed_module_file_path_for_area_resref(context, resref).or_else(|| {
                observed_module_file_path_for_no_name_area_resref(
                    context,
                    payload,
                    fragment_offset,
                    layout,
                    resref,
                )
            })
        } else {
            observed_module_file_path_for_area_resref(context, resref)
        }
    });
    let module_path_proven_by_direct_area_resref = direct_resref_module_path.is_some();
    let fragmented_resref_module_path =
        packet_area_resref_fragments
            .as_deref()
            .and_then(|fragments| {
                observed_module_file_path_for_fragmented_area_resref(
                    context,
                    payload,
                    fragment_offset,
                    layout,
                    fragments,
                )
            });
    let module_path_proven_by_fragmented_area_resref = fragmented_resref_module_path.is_some();
    let primary_module_path = observed_module_file_path(context);
    let Some(module_path) = direct_resref_module_path
        .or(fragmented_resref_module_path)
        .or(primary_module_path)
    else {
        tracing::debug!(
            legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
            module_name = context.localized_name.as_str(),
            module_resref = context.module_resref.as_str(),
            packet_area_resref = packet_area_resref.as_deref().unwrap_or(""),
            observed_areas = ?context
                .areas
                .iter()
                .map(|area| format!("0x{:08X}:{}", area.object_id, area.name))
                .collect::<Vec<_>>(),
            "Area_ClientArea compact resource repair skipped: observed Module_Info context did not resolve to one local module file"
        );
        return None;
    };
    let table = read_module_area_resource_table(&module_path)?;
    let table_matches_observed_context = module_table_matches_observed_context(&table, context);
    if !table_matches_observed_context
        && !module_path_proven_by_direct_area_resref
        && !module_path_proven_by_fragmented_area_resref
    {
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

    if compact_fragments.is_none() {
        if !module_path_proven_by_direct_area_resref
            && !module_path_proven_by_fragmented_area_resref
            && !table_matches_observed_context
        {
            return None;
        }

        if let Some(packet_area_resref) = packet_area_resref.as_ref() {
            if let Some(info) = table
                .areas
                .iter()
                .find(|area| area.resref.eq_ignore_ascii_case(packet_area_resref))
                .cloned()
            {
                tracing::info!(
                    legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
                    packet_area_resref = packet_area_resref.as_str(),
                    area_resref = info.resref.as_str(),
                    area_name = info.name.as_str(),
                    module_path = %module_path.display(),
                    "Area_ClientArea compact resource repair resolved no-name packet by exact local area resref"
                );
                return Some(info);
            }
            if let Some(info) = unique_no_name_area_resource_for_truncated_packet_resref(
                payload,
                fragment_offset,
                layout,
                &table,
                packet_area_resref,
            ) {
                tracing::info!(
                    legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
                    packet_area_resref = packet_area_resref.as_str(),
                    area_resref = info.resref.as_str(),
                    area_name = info.name.as_str(),
                    module_path = %module_path.display(),
                    "Area_ClientArea compact resource repair resolved no-name packet by unique local area resref prefix and exact dimensions/tileset"
                );
                return Some(info);
            }
        }

        if let Some(info) = unique_no_name_area_resource_for_fragmented_packet_resref(
            payload,
            fragment_offset,
            layout,
            &table,
            packet_area_resref_fragments.as_deref(),
        ) {
            tracing::info!(
                legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
                packet_area_resref_fragments = ?packet_area_resref_fragments,
                area_resref = info.resref.as_str(),
                area_name = info.name.as_str(),
                module_path = %module_path.display(),
                "Area_ClientArea compact resource repair resolved no-name packet by fragmented local area resref and exact tileset proof"
            );
            return Some(info);
        }
        return None;
    }
    let compact_fragments = compact_fragments?;

    if area_indices.is_empty() {
        if context.areas.len() == 1 && table.areas.len() == 1 {
            let info = table.areas.first()?.clone();
            let observed_area = context.areas.first()?;
            let observed_fragments = compact_observed_text_fragments(&observed_area.name);
            if module_table_identity_matches_observed_context(&table, &context)
                && area_resource_matches_single_area_fragments(&info, &observed_fragments)
                && area_resource_matches_single_area_fragments(&info, &compact_fragments)
            {
                tracing::info!(
                    legacy_area_object_id = format_args!("0x{legacy_area_object_id:08X}"),
                    observed_area_object_id = format_args!("0x{:08X}", observed_area.object_id),
                    area_resref = info.resref.as_str(),
                    area_name = info.name.as_str(),
                    "Area_ClientArea compact resource repair resolved single-area Module_Info object-id alias"
                );
                return Some(info);
            }
        }
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

fn module_area_resource_info_for_named_static_placeables(
    payload: &[u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
    area_resref: &str,
    module_context: Option<&crate::translate::module::ObservedModuleContext>,
) -> Option<ModuleAreaResourceInfo> {
    if !area_resref_plausible(area_resref) || !scan.valid {
        return None;
    }
    let packet_tileset = fixed_resref_preview(
        payload,
        HIGH_LEVEL_HEADER_BYTES.checked_add(scan.layout.tileset_read_offset)?,
    )?;
    let owned_context = if module_context.is_none() {
        crate::translate::module::observed_module_context()
    } else {
        None
    };
    let observed_context = module_context.or(owned_context.as_ref());
    let explicit_candidate = explicit_observed_module_file_candidate();
    let explicit_key = explicit_candidate.as_ref().map(|path| path_key(path));

    let mut candidates = Vec::new();
    if let Some(candidate) = explicit_candidate {
        candidates.push(candidate);
    }
    if let Some(context) = observed_context {
        if let Some(candidate) = observed_module_file_path_for_area_resref(context, area_resref) {
            candidates.push(candidate);
        }
        candidates.extend(observed_module_file_candidates(context));
    }

    let mut seen_paths = HashSet::new();
    let mut matched_tables = HashSet::new();
    let mut matched_info = None;
    for candidate in candidates {
        let candidate_key = path_key(&candidate);
        if !seen_paths.insert(candidate_key.clone()) || !candidate.is_file() {
            continue;
        }
        let Some(table) = read_module_area_resource_table(&candidate) else {
            continue;
        };
        let explicit_match = explicit_key
            .as_ref()
            .is_some_and(|key| key == &candidate_key);
        if !explicit_match {
            let Some(context) = observed_context else {
                continue;
            };
            let identity_matches = module_table_identity_matches_observed_context(&table, context)
                || module_file_path_matches_observed_context(&candidate, context);
            let area_matches = module_table_area_matches_observed_context(&table, context);
            if !identity_matches && !area_matches {
                continue;
            }
        }

        let area_matches = table
            .areas
            .iter()
            .filter(|info| {
                info.resref.eq_ignore_ascii_case(area_resref)
                    && module_named_static_placeable_packet_matches_resource(
                        payload,
                        fragment_offset,
                        scan,
                        &packet_tileset,
                        info,
                    )
            })
            .cloned()
            .collect::<Vec<_>>();
        if area_matches.len() != 1 {
            continue;
        }

        let table_key = module_area_resource_table_match_key(&table);
        if matched_tables.insert(table_key) {
            if matched_info.is_some() {
                return None;
            }
            matched_info = area_matches.into_iter().next();
        }
    }

    matched_info
}

fn module_named_static_placeable_packet_matches_resource(
    payload: &[u8],
    fragment_offset: usize,
    scan: &AreaTileStreamScan,
    packet_tileset: &str,
    info: &ModuleAreaResourceInfo,
) -> bool {
    if !module_resource_tile_scan_matches(scan, info)
        || !info.tileset.eq_ignore_ascii_case(packet_tileset)
    {
        return false;
    }
    let Some(proof) = legacy_area_source_tail_exact_read_proof(payload, fragment_offset, scan)
    else {
        return false;
    };
    let static_placeables = info
        .placeables
        .iter()
        .filter(|placeable| {
            placeable.static_object && module_static_placeable_resource_row_safe(placeable)
        })
        .collect::<Vec<_>>();
    unique_module_static_placeable_row_matches(payload, fragment_offset, &proof, &static_placeables)
        .is_some()
}

fn module_table_matches_observed_context(
    table: &ModuleAreaResourceTable,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    module_table_identity_matches_observed_context(table, context)
        || module_table_area_matches_observed_context(table, context)
}

fn module_table_area_matches_observed_context(
    table: &ModuleAreaResourceTable,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    module_table_area_order_matches_observed_context(table, context)
        || module_table_unordered_area_matches_observed_context(table, context)
}

fn module_table_identity_matches_observed_context(
    table: &ModuleAreaResourceTable,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    table.module_name.as_deref().is_some_and(|name| {
        same_resource_text(name, &context.localized_name)
            || resource_text_prefix_matches(name, &context.localized_name)
    }) || table
        .module_resref
        .as_deref()
        .is_some_and(|resref| same_resource_text(resref, &context.module_resref))
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

fn resource_text_prefix_matches(left: &str, right: &str) -> bool {
    let left = normalized_resource_text(left);
    let right = normalized_resource_text(right);
    if left.is_empty() || right.is_empty() || left == right {
        return false;
    }
    let shorter = left.len().min(right.len());
    let longer = left.len().max(right.len());
    shorter >= MIN_MODULE_NAME_PREFIX_MATCH_CHARS
        && shorter.saturating_mul(100) >= longer.saturating_mul(60)
        && (left.starts_with(&right) || right.starts_with(&left))
}

fn module_file_path_matches_observed_context(
    path: &Path,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let stem = normalized_resource_text(stem);
    [
        context.localized_name.as_str(),
        context.module_resref.as_str(),
    ]
    .into_iter()
    .map(normalized_resource_text)
    .any(|observed| {
        !observed.is_empty()
            && (observed == stem
                || (observed.len().min(stem.len()) >= MIN_MODULE_FILE_PREFIX_MATCH_CHARS
                    && (stem.starts_with(&observed) || observed.starts_with(&stem))))
    })
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
    observed_module_file_path_from_candidates(context, observed_module_file_candidates(context))
}

fn observed_module_file_path_for_area_resref(
    context: &crate::translate::module::ObservedModuleContext,
    area_resref: &str,
) -> Option<PathBuf> {
    if !area_resref_plausible(area_resref) {
        return None;
    }

    let mut seen = HashSet::new();
    let mut fallback = None;
    let mut fallback_count = 0usize;
    let mut fallback_keys = HashSet::new();
    for candidate in observed_module_file_candidates(context) {
        if !seen.insert(path_key(&candidate)) || !candidate.is_file() {
            continue;
        }
        let Some(table) = read_module_area_resource_table(&candidate) else {
            continue;
        };
        let identity_matches = module_table_identity_matches_observed_context(&table, context)
            || module_file_path_matches_observed_context(&candidate, context);
        if !identity_matches
            || !table
                .areas
                .iter()
                .any(|area| area.resref.eq_ignore_ascii_case(area_resref))
        {
            continue;
        }
        let table_key = module_area_resource_table_match_key(&table);
        if fallback_keys.insert(table_key) {
            fallback_count = fallback_count.saturating_add(1);
            if fallback.is_none() {
                fallback = Some(candidate);
            }
        }
    }

    if fallback_count == 1 { fallback } else { None }
}

fn observed_module_file_path_for_no_name_area_resref(
    context: &crate::translate::module::ObservedModuleContext,
    payload: &[u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    area_resref: &str,
) -> Option<PathBuf> {
    // Diamond no-inline-name packets still carry the static area CResRef in
    // the legacy object-id slot. When compact Module_Info alignment is stale,
    // the packet-local CResRef plus exact static layout/tile proof can identify
    // one local ARE without trusting the broken module name/table context.
    if !area_resref_plausible(area_resref) {
        return None;
    }

    if let Some(candidate) = explicit_observed_module_file_candidate() {
        if read_module_area_resource_table(&candidate).is_some_and(|table| {
            unique_no_name_area_resource_for_truncated_packet_resref(
                payload,
                fragment_offset,
                layout,
                &table,
                area_resref,
            )
            .is_some()
                || exact_no_name_area_resource_for_packet_resref(&table, area_resref).is_some()
        }) {
            return Some(candidate);
        }
    }

    let mut seen = HashSet::new();
    let mut fallback = None;
    let mut fallback_count = 0usize;
    let mut fallback_keys = HashSet::new();
    for candidate in observed_module_file_candidates(context) {
        if !seen.insert(path_key(&candidate)) || !candidate.is_file() {
            continue;
        }
        let Some(table) = read_module_area_resource_table(&candidate) else {
            continue;
        };
        if unique_no_name_area_resource_for_truncated_packet_resref(
            payload,
            fragment_offset,
            layout,
            &table,
            area_resref,
        )
        .is_none()
        {
            continue;
        }
        let table_key = module_area_resource_table_match_key(&table);
        if fallback_keys.insert(table_key) {
            fallback_count = fallback_count.saturating_add(1);
            if fallback.is_none() {
                fallback = Some(candidate);
            }
        }
    }

    if fallback_count == 1 { fallback } else { None }
}

fn exact_no_name_area_resource_for_packet_resref(
    table: &ModuleAreaResourceTable,
    packet_area_resref: &str,
) -> Option<ModuleAreaResourceInfo> {
    if !area_resref_plausible(packet_area_resref) {
        return None;
    }
    let mut matches = table
        .areas
        .iter()
        .filter(|info| info.resref.eq_ignore_ascii_case(packet_area_resref))
        .cloned()
        .collect::<Vec<_>>();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn observed_module_file_path_for_fragmented_area_resref(
    context: &crate::translate::module::ObservedModuleContext,
    payload: &[u8],
    fragment_offset: usize,
    layout: &AreaStaticLayout,
    fragments: &[String],
) -> Option<PathBuf> {
    // Compact 1.69 Module_Info can lose enough area-table alignment that the
    // observed module context no longer proves the local module file by name.
    // For no-name Area_ClientArea packets, the decompiled CResRef slot still
    // gives bounded module-local evidence: fragmented area-resref ASCII plus
    // the exact tileset field. Accept only a single matching local module table.
    if fragments.len() < 2 {
        return None;
    }

    if let Some(candidate) = explicit_observed_module_file_candidate() {
        if read_module_area_resource_table(&candidate).is_some_and(|table| {
            unique_no_name_area_resource_for_fragmented_packet_resref(
                payload,
                fragment_offset,
                layout,
                &table,
                Some(fragments),
            )
            .is_some()
        }) {
            return Some(candidate);
        }
    }

    let mut seen = HashSet::new();
    let mut fallback = None;
    let mut fallback_count = 0usize;
    let mut fallback_keys = HashSet::new();
    for candidate in observed_module_file_candidates(context) {
        if !seen.insert(path_key(&candidate)) || !candidate.is_file() {
            continue;
        }
        let Some(table) = read_module_area_resource_table(&candidate) else {
            continue;
        };
        if unique_no_name_area_resource_for_fragmented_packet_resref(
            payload,
            fragment_offset,
            layout,
            &table,
            Some(fragments),
        )
        .is_none()
        {
            continue;
        }
        let table_key = module_area_resource_table_match_key(&table);
        if fallback_keys.insert(table_key) {
            fallback_count = fallback_count.saturating_add(1);
            if fallback.is_none() {
                fallback = Some(candidate);
            }
        }
    }

    if fallback_count == 1 { fallback } else { None }
}

fn observed_module_file_path_from_candidates<I>(
    context: &crate::translate::module::ObservedModuleContext,
    candidates: I,
) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut seen = HashSet::new();
    let mut fallback = None;
    let mut fallback_count = 0usize;
    let mut fallback_keys = HashSet::new();
    for candidate in candidates {
        if !seen.insert(path_key(&candidate)) || !candidate.is_file() {
            continue;
        }
        let Some(table) = read_module_area_resource_table(&candidate) else {
            continue;
        };
        let area_matches = module_table_area_matches_observed_context(&table, context);
        let identity_matches = module_table_identity_matches_observed_context(&table, context)
            || module_file_path_matches_observed_context(&candidate, context);
        let single_area_identity_matches =
            identity_matches && module_table_single_area_matches_observed_context(&table, context);
        if identity_matches && (area_matches || single_area_identity_matches) {
            return Some(candidate);
        }
        if area_matches {
            let table_key = module_area_resource_table_match_key(&table);
            if fallback_keys.insert(table_key) {
                fallback_count = fallback_count.saturating_add(1);
                if fallback.is_none() {
                    fallback = Some(candidate);
                }
            }
        }
    }
    if fallback_count == 1 { fallback } else { None }
}

fn explicit_observed_module_file_candidate() -> Option<PathBuf> {
    std::env::var("NWN_BRIDGE_MODULE_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn module_area_resource_table_match_key(table: &ModuleAreaResourceTable) -> String {
    let mut key = String::new();
    let _ = write!(
        key,
        "module:{:?}:{:?}|order:",
        table.module_name.as_deref().unwrap_or_default(),
        table.module_resref.as_deref().unwrap_or_default()
    );
    for area_resref in &table.area_order {
        let _ = write!(key, "{area_resref:?};");
    }
    key.push_str("|areas:");
    for area in &table.areas {
        let _ = write!(
            key,
            "{:?}:{:?}:{}:{}:{:?}|tiles:",
            area.resref, area.name, area.width, area.height, area.tileset
        );
        for tile in &area.tiles {
            let _ = write!(
                key,
                "{}:{:08X}:{}:{}:{}:{}:{}:{}:{}:{};",
                tile.tile_id,
                tile.orientation,
                tile.height_raw,
                tile.main_light1,
                tile.main_light2,
                tile.source_light1,
                tile.source_light2,
                tile.anim_loop1,
                tile.anim_loop2,
                tile.anim_loop3
            );
        }
        key.push_str("|notes:");
        for note in &area.map_notes {
            let _ = write!(
                key,
                "{:?}:{:08X}:{:08X}:{:08X};",
                note.text,
                note.x.to_bits(),
                note.y.to_bits(),
                note.z.to_bits()
            );
        }
        key.push_str("|sounds:");
        for sound in &area.sounds {
            let _ = write!(
                key,
                "{:?}:{:08X}:{:08X}:{:08X}:",
                sound.tag,
                sound.x.to_bits(),
                sound.y.to_bits(),
                sound.z.to_bits()
            );
            for resref in &sound.resrefs {
                let _ = write!(key, "{resref:?};");
            }
        }
        key.push('|');
    }
    key
}

fn module_table_single_area_matches_observed_context(
    table: &ModuleAreaResourceTable,
    context: &crate::translate::module::ObservedModuleContext,
) -> bool {
    if context.areas.len() != 1 || table.areas.len() != 1 {
        return false;
    }
    let Some(observed_area) = context.areas.first() else {
        return false;
    };
    let observed_fragments = compact_observed_text_fragments(&observed_area.name);
    table
        .areas
        .first()
        .is_some_and(|info| area_resource_matches_single_area_fragments(info, &observed_fragments))
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
                if let Some(runtime) = parse_git_runtime_info(git_bytes) {
                    area.map_notes = runtime.map_notes;
                    area.sounds = runtime.sounds;
                    area.placeables = runtime.placeables;
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
        placeables: Vec::new(),
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

#[derive(Debug, Clone)]
struct ModuleGitRuntimeInfo {
    map_notes: Vec<ModuleAreaMapNote>,
    sounds: Vec<ModuleAreaSound>,
    placeables: Vec<ModuleAreaPlaceable>,
}

fn parse_git_runtime_info(bytes: &[u8]) -> Option<ModuleGitRuntimeInfo> {
    let fields = gff_root_fields(bytes)?;
    let (map_notes, sounds) = match (
        parse_git_map_notes(bytes, &fields),
        parse_git_sounds(bytes, &fields),
    ) {
        (Some(map_notes), Some(sounds)) => (map_notes, sounds),
        _ => (Vec::new(), Vec::new()),
    };
    // Static placeable semantics are independent of the older map-note/sound
    // tail repair path and must not be lost because an unrelated list failed.
    Some(ModuleGitRuntimeInfo {
        map_notes,
        sounds,
        placeables: parse_git_placeables(bytes, &fields).unwrap_or_default(),
    })
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

fn parse_git_placeables(
    bytes: &[u8],
    root_fields: &[GffField],
) -> Option<Vec<ModuleAreaPlaceable>> {
    let Some(placeable_list) = gff_field_by_label(root_fields, "Placeable List") else {
        return Some(Vec::new());
    };
    if placeable_list.field_type != GFF_TYPE_LIST {
        return None;
    }

    let mut placeables = Vec::new();
    for struct_index in gff_list_structs(bytes, placeable_list.data)? {
        let fields = gff_struct_fields(bytes, struct_index)?;
        let placeable = ModuleAreaPlaceable {
            tag: gff_field_by_label(&fields, "Tag")
                .and_then(|field| gff_string_value(bytes, field))
                .unwrap_or_default(),
            appearance: required_gff_u16(&fields, "Appearance")?,
            x: required_gff_float(&fields, "X")?,
            y: required_gff_float(&fields, "Y")?,
            z: required_gff_float(&fields, "Z")?,
            bearing: required_gff_float(&fields, "Bearing")?,
            static_object: optional_gff_bool(&fields, "Static"),
            useable: optional_gff_bool(&fields, "Useable"),
            trap_flag: optional_gff_bool(&fields, "TrapFlag"),
            trap_disarmable: optional_gff_bool(&fields, "TrapDisarmable"),
            lockable: optional_gff_bool(&fields, "Lockable"),
            locked: optional_gff_bool(&fields, "Locked"),
        };
        if ![placeable.x, placeable.y, placeable.z, placeable.bearing]
            .into_iter()
            .all(f32::is_finite)
        {
            return None;
        }
        placeables.push(placeable);
    }

    Some(placeables)
}

fn required_gff_float(fields: &[GffField], label: &str) -> Option<f32> {
    gff_field_by_label(fields, label).and_then(gff_float_value)
}

fn required_gff_u32(fields: &[GffField], label: &str) -> Option<u32> {
    gff_field_by_label(fields, label).and_then(gff_dword_value)
}

fn required_gff_u16(fields: &[GffField], label: &str) -> Option<u16> {
    gff_field_by_label(fields, label).and_then(gff_u16_value)
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

fn optional_gff_bool(fields: &[GffField], label: &str) -> bool {
    optional_gff_byte(fields, label) != 0
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

fn gff_u16_value(field: &GffField) -> Option<u16> {
    match field.field_type {
        GFF_TYPE_WORD => u16::try_from(field.data).ok(),
        GFF_TYPE_SHORT if field.data <= i16::MAX as u32 => u16::try_from(field.data).ok(),
        GFF_TYPE_DWORD | GFF_TYPE_INT => u16::try_from(field.data).ok(),
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

fn read_diamond_long_fixed_area_name_shape(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<(u32, usize)> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    if payload_offset > fragment_offset
        || fragment_offset - payload_offset < DIAMOND_LONG_AREA_NAME_BYTES
    {
        return None;
    }
    let bytes = payload.get(payload_offset..payload_offset + DIAMOND_LONG_AREA_NAME_BYTES)?;
    if !bytes
        .iter()
        .all(|byte| *byte == 0 || (0x20u8..=0x7Eu8).contains(byte))
    {
        return None;
    }
    if !bytes.iter().any(|byte| *byte != 0) {
        return None;
    }

    // Local Contest of Champions evidence shows a 21-byte Diamond fixed
    // area-name read window. The bridge only accepts it after the static
    // width/height/tileset offsets prove the layout and the resource-backed
    // repair resolves the compact text to one local ARE.
    Some((
        DIAMOND_LONG_AREA_NAME_BYTES as u32,
        AREA_NAME_READ_OFFSET + DIAMOND_LONG_AREA_NAME_BYTES,
    ))
}

fn read_diamond_no_area_name_shape(payload: &[u8], fragment_offset: usize) -> Option<(u32, usize)> {
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    if payload_offset > fragment_offset || payload_offset > payload.len() {
        return None;
    }

    // Official 1.69 Prelude evidence takes the no-inline-name branch: the
    // Diamond reader consumes the four-byte legacy field at the area-name site,
    // then reads width/height/tileset at the normal legacy offsets. Accept this
    // source layout only after the legacy tileset CResRef position proves the
    // following static area block.
    Some((
        DIAMOND_NO_AREA_NAME_BYTES as u32,
        AREA_NAME_READ_OFFSET + DIAMOND_NO_AREA_NAME_BYTES,
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

fn diamond_cexo_string_area_name_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    let (length, _) =
        read_c_exo_string_shape(payload, fragment_offset, AREA_NAME_READ_OFFSET, 1024)?;
    if length == 0 {
        return None;
    }
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let text_start = payload_offset.checked_add(EE_CEXO_STRING_LENGTH_BYTES)?;
    let text_end = text_start.checked_add(usize::try_from(length).ok()?)?;
    if text_end > fragment_offset || text_end > payload.len() {
        return None;
    }
    compact_printable_ascii_runs_allowing_singletons(payload.get(text_start..text_end)?)
}

fn diamond_fixed_area_name_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    read_diamond_fixed_area_name_shape(payload, fragment_offset)?;
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_LEGACY_AREA_NAME_BYTES)?;
    let bytes = payload.get(payload_offset..end)?;
    compact_printable_ascii_runs_allowing_singletons(bytes)
}

fn diamond_long_fixed_area_name_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    read_diamond_long_fixed_area_name_shape(payload, fragment_offset)?;
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_LONG_AREA_NAME_BYTES)?;
    let bytes = payload.get(payload_offset..end)?;
    compact_printable_ascii_runs_allowing_singletons(bytes)
}

fn diamond_short_fixed_area_name_fragments(
    payload: &[u8],
    fragment_offset: usize,
) -> Option<Vec<String>> {
    read_diamond_short_fixed_area_name_shape(payload, fragment_offset)?;
    let payload_offset = HIGH_LEVEL_HEADER_BYTES.checked_add(AREA_NAME_READ_OFFSET)?;
    let end = payload_offset.checked_add(DIAMOND_SHORT_AREA_NAME_BYTES)?;
    let bytes = payload.get(payload_offset..end)?;
    compact_printable_ascii_runs_allowing_singletons(bytes)
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

#[cfg(test)]
fn legacy_area_payload_with_extra_fragment_bits(
    payload: &[u8],
    extra_fragment_bits: usize,
) -> (Vec<u8>, usize) {
    let (_, _, fragment_offset, _) =
        area_client_area_read_window(payload).expect("test area read window");
    let mut bits = decode_cnw_msb_valid_bits(
        payload
            .get(fragment_offset..)
            .expect("test payload should contain a fragment stream"),
        CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("test fragment should decode");
    let mut payload_bits = bits.split_off(CNW_FRAGMENT_HEADER_BITS);
    payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));

    let mut shifted = payload[..fragment_offset].to_vec();
    shifted.extend_from_slice(
        &encode_cnw_msb_payload_bits(&payload_bits)
            .expect("shifted legacy area fragment bits should encode"),
    );
    (shifted, fragment_offset)
}

#[cfg(test)]
mod public_static_direction_tests {
    use super::*;

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_f32(bytes: &mut Vec<u8>, value: f32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_fixed_resref(bytes: &mut Vec<u8>, value: &str) {
        assert!(value.len() <= CRESREF_TEXT_BYTES);
        let mut resref = [0u8; CRESREF_TEXT_BYTES];
        resref[..value.len()].copy_from_slice(value.as_bytes());
        bytes.extend_from_slice(&resref);
    }

    fn pad_to_read_offset(bytes: &mut Vec<u8>, read_offset: usize) {
        bytes.resize(HIGH_LEVEL_HEADER_BYTES + read_offset, 0);
    }

    fn fixed_name_square_dimension_payload() -> (Vec<u8>, usize) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        let mut name = [0u8; DIAMOND_LEGACY_AREA_NAME_BYTES];
        name[..9].copy_from_slice(b"BW167Demo");
        payload.extend_from_slice(&name);
        let name_end = AREA_NAME_READ_OFFSET + DIAMOND_LEGACY_AREA_NAME_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 0);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 0);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1); // tile id
        push_u32(&mut payload, 0); // orientation
        push_u32(&mut payload, 0); // height
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]); // source light bytes

        push_u32(&mut payload, 0); // transition rows
        push_u32(&mut payload, 0); // map-pin rows
        push_u16(&mut payload, 0); // sound rows
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, 0); // static-placeable rows

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic area payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let fragment = encode_cnw_msb_payload_bits(&vec![
            false;
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                - CNW_FRAGMENT_HEADER_BITS
        ])
        .expect("legacy Area_ClientArea pre-tile bits should encode");
        payload.extend_from_slice(&fragment);

        (payload, fragment_offset)
    }

    fn angle_delta(actual: f32, expected: f32) -> f32 {
        let two_pi = std::f32::consts::PI * 2.0;
        (actual - expected + std::f32::consts::PI).rem_euclid(two_pi) - std::f32::consts::PI
    }

    fn module_info_with_placeables(placeables: Vec<ModuleAreaPlaceable>) -> ModuleAreaResourceInfo {
        ModuleAreaResourceInfo {
            resref: "testarea".to_string(),
            name: "Test Area".to_string(),
            width: 1,
            height: 1,
            tileset: "ttr01".to_string(),
            tiles: Vec::new(),
            map_notes: Vec::new(),
            sounds: Vec::new(),
            placeables,
        }
    }

    fn module_area_info(
        resref: &str,
        width: u32,
        height: u32,
        tileset: &str,
    ) -> ModuleAreaResourceInfo {
        ModuleAreaResourceInfo {
            resref: resref.to_string(),
            name: resref.to_string(),
            width,
            height,
            tileset: tileset.to_string(),
            tiles: Vec::new(),
            map_notes: Vec::new(),
            sounds: Vec::new(),
            placeables: Vec::new(),
        }
    }

    fn no_name_area_payload_with_fragmented_resref(
        fragments: &[&str],
        tileset: &str,
    ) -> (Vec<u8>, usize) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);
        payload.resize(LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET, 0);
        push_u32(&mut payload, 0x8000_0042);

        let mut area_resref = [0u8; CRESREF_TEXT_BYTES];
        let mut cursor = 0usize;
        for fragment in fragments {
            let bytes = fragment.as_bytes();
            assert!(cursor + bytes.len() <= CRESREF_TEXT_BYTES);
            area_resref[cursor..cursor + bytes.len()].copy_from_slice(bytes);
            cursor += bytes.len().saturating_add(1);
        }
        payload.extend_from_slice(&area_resref);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0xFFFF_FFFF);
        let name_end = AREA_NAME_READ_OFFSET + DIAMOND_NO_AREA_NAME_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 0);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 0);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, tileset);

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic no-name area payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let fragment = encode_cnw_msb_payload_bits(&vec![
            false;
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                - CNW_FRAGMENT_HEADER_BITS
        ])
        .expect("legacy Area_ClientArea pre-tile bits should encode");
        payload.extend_from_slice(&fragment);

        (payload, fragment_offset)
    }

    #[test]
    fn fragmented_no_name_area_resource_requires_unique_resref_tileset_match() {
        let (payload, fragment_offset) =
            no_name_area_payload_with_fragmented_resref(&["a08_bar", "ks"], "tin01");
        let layout = area_static_layout(&payload, fragment_offset)
            .expect("synthetic no-name area should expose a static layout");
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            compact_packet_area_resref_fragments(&payload, fragment_offset)
                .expect("packet area resref should expose compact fragments"),
            vec!["a08_bar".to_string(), "ks".to_string()]
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.width_read_offset),
            Some(0)
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.height_read_offset),
            Some(0)
        );

        let mut one_match = ModuleAreaResourceTable {
            module_name: None,
            module_resref: None,
            area_order: Vec::new(),
            areas: vec![
                module_area_info("a08_barracks", 5, 3, "tin01"),
                module_area_info("a08_caves", 4, 4, "tin01"),
            ],
        };
        let info = unique_no_name_area_resource_for_fragmented_packet_resref(
            &payload,
            fragment_offset,
            &layout,
            &one_match,
            Some(&["a08_bar".to_string(), "ks".to_string()]),
        )
        .expect("one fragmented resref plus tileset match may identify the module ARE");
        assert_eq!(info.resref, "a08_barracks");

        one_match
            .areas
            .push(module_area_info("a08_barracks2", 6, 3, "tin01"));
        assert!(
            unique_no_name_area_resource_for_fragmented_packet_resref(
                &payload,
                fragment_offset,
                &layout,
                &one_match,
                Some(&["a08_bar".to_string(), "ks".to_string()]),
            )
            .is_none(),
            "zero-dimension fragmented packets must not select a module ARE when more than one local resource has the same resref fragments and tileset"
        );

        let wrong_tileset = ModuleAreaResourceTable {
            module_name: None,
            module_resref: None,
            area_order: Vec::new(),
            areas: vec![module_area_info("a08_barracks", 5, 3, "tcn01")],
        };
        assert!(
            unique_no_name_area_resource_for_fragmented_packet_resref(
                &payload,
                fragment_offset,
                &layout,
                &wrong_tileset,
                Some(&["a08_bar".to_string(), "ks".to_string()]),
            )
            .is_none(),
            "packet-local tileset is part of the decompile-owned area resource proof"
        );
    }

    fn static_placeable_source_row_payload(
        appearance: u16,
        x: f32,
        y: f32,
        z: f32,
        dir_x: f32,
        dir_y: f32,
        dir_z: f32,
    ) -> (Vec<u8>, usize, AreaTileStreamScan) {
        static_placeable_source_rows_payload(&[(appearance, x, y, z, dir_x, dir_y, dir_z)])
    }

    fn static_placeable_source_rows_payload(
        rows: &[(u16, f32, f32, f32, f32, f32, f32)],
    ) -> (Vec<u8>, usize, AreaTileStreamScan) {
        static_placeable_source_rows_payload_with_count(
            u16::try_from(rows.len()).expect("test row count should fit in WORD"),
            rows,
        )
    }

    fn static_placeable_source_rows_payload_with_count(
        static_count: u16,
        rows: &[(u16, f32, f32, f32, f32, f32, f32)],
    ) -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        push_u32(&mut payload, 0); // transition rows
        push_u32(&mut payload, 0); // map-pin rows
        push_u16(&mut payload, 0); // sound rows
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, static_count);
        for (index, (appearance, x, y, z, dir_x, dir_y, dir_z)) in rows.iter().enumerate() {
            push_u32(
                &mut payload,
                0x8000_0042u32
                    .checked_add(u32::try_from(index).expect("test row index should fit in DWORD"))
                    .expect("test object id should stay in the legacy object namespace"),
            );
            push_u16(&mut payload, *appearance);
            push_f32(&mut payload, *x);
            push_f32(&mut payload, *y);
            push_f32(&mut payload, *z);
            push_f32(&mut payload, *dir_x);
            push_f32(&mut payload, *dir_y);
            push_f32(&mut payload, *dir_z);
        }

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let fragment = encode_cnw_msb_payload_bits(&vec![
            false;
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                - CNW_FRAGMENT_HEADER_BITS
        ])
        .expect("legacy Area_ClientArea pre-tile bits should encode");
        payload.extend_from_slice(&fragment);

        let scan = AreaTileStreamScan {
            valid: true,
            tile_end_read_offset: CNW_LENGTH_BYTES,
            ..AreaTileStreamScan::default()
        };
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)
        );
        (payload, fragment_offset, scan)
    }

    fn real_area_static_placeable_source_rows_payload_with_count(
        static_count: u16,
        rows: &[(u16, f32, f32, f32, f32, f32, f32)],
    ) -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        // One minimal Diamond/EE tile row: DWORD tile, DWORD orientation, DWORD
        // height, WORD flags, and the always-present two source-light bytes.
        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 0); // transition rows
        push_u32(&mut payload, 0); // map-pin rows
        push_u16(&mut payload, 0); // sound rows
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, static_count);
        for (index, (appearance, x, y, z, dir_x, dir_y, dir_z)) in rows.iter().enumerate() {
            push_u32(
                &mut payload,
                0x8000_0042u32
                    .checked_add(u32::try_from(index).expect("test row index should fit in DWORD"))
                    .expect("test object id should stay in the legacy object namespace"),
            );
            push_u16(&mut payload, *appearance);
            push_f32(&mut payload, *x);
            push_f32(&mut payload, *y);
            push_f32(&mut payload, *z);
            push_f32(&mut payload, *dir_x);
            push_f32(&mut payload, *dir_y);
            push_f32(&mut payload, *dir_z);
        }

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let fragment = encode_cnw_msb_payload_bits(&vec![
            false;
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                - CNW_FRAGMENT_HEADER_BITS
        ])
        .expect("legacy Area_ClientArea pre-tile bits should encode");
        payload.extend_from_slice(&fragment);

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(scan.width, 1);
        assert_eq!(scan.packet_height, 1);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)
        );
        (payload, fragment_offset, scan)
    }

    fn real_area_tlk_transition_static_placeable_payload() -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 1); // transition rows
        push_u32(&mut payload, 0x8000_0099); // transition object id
        push_f32(&mut payload, 1.25);
        push_f32(&mut payload, 2.5);
        push_f32(&mut payload, 0.0);
        push_u32(&mut payload, 0x0000_1234); // TLK string ref branch
        push_u32(&mut payload, 0); // map-pin rows
        push_u16(&mut payload, 0); // sound rows
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, 1); // static-placeable rows
        push_u32(&mut payload, 0x8000_0042);
        push_u16(&mut payload, 82);
        push_f32(&mut payload, 10.0);
        push_f32(&mut payload, 20.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 1.0);
        push_f32(&mut payload, 0.0);

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        payload_bits.extend_from_slice(&[false, true, false]);
        let fragment =
            encode_cnw_msb_payload_bits(&payload_bits).expect("test fragment should encode");
        payload.extend_from_slice(&fragment);

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS + 3)
        );
        (payload, fragment_offset, scan)
    }

    fn real_area_direct_transition_static_placeable_payload(
        extra_direct_label_bit: bool,
    ) -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 1); // transition rows
        push_u32(&mut payload, 0x8000_0099); // transition object id
        push_f32(&mut payload, 1.25);
        push_f32(&mut payload, 2.5);
        push_f32(&mut payload, 0.0);
        let label = b"direct-transition";
        push_u32(&mut payload, label.len() as u32);
        payload.extend_from_slice(label);
        push_u32(&mut payload, 0); // map-pin rows
        push_u16(&mut payload, 0); // sound rows
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, 1); // static-placeable rows
        push_u32(&mut payload, 0x8000_0042);
        push_u16(&mut payload, 82);
        push_f32(&mut payload, 10.0);
        push_f32(&mut payload, 20.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 1.0);
        push_f32(&mut payload, 0.0);

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        payload_bits.extend_from_slice(&[false, false]);
        if extra_direct_label_bit {
            payload_bits.push(false);
        }
        let fragment =
            encode_cnw_msb_payload_bits(&payload_bits).expect("test fragment should encode");
        payload.extend_from_slice(&fragment);

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS + 2 + usize::from(extra_direct_label_bit))
        );
        (payload, fragment_offset, scan)
    }

    fn real_area_long_map_pin_static_placeable_payload() -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 0); // transition rows
        push_u32(&mut payload, 1); // map-pin rows
        push_u32(&mut payload, 1); // map-pin id
        let label = vec![b'M'; 1500];
        push_u32(
            &mut payload,
            u32::try_from(label.len()).expect("test map-pin label length should fit in DWORD"),
        );
        payload.extend_from_slice(&label);
        push_f32(&mut payload, 3.0);
        push_f32(&mut payload, 4.0);
        push_f32(&mut payload, 0.0);
        push_u16(&mut payload, 0); // sound rows
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, 1); // static-placeable rows
        push_u32(&mut payload, 0x8000_0042);
        push_u16(&mut payload, 82);
        push_f32(&mut payload, 10.0);
        push_f32(&mut payload, 20.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 1.0);
        push_f32(&mut payload, 0.0);

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let fragment = encode_cnw_msb_payload_bits(&vec![
            false;
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                - CNW_FRAGMENT_HEADER_BITS
        ])
        .expect("legacy Area_ClientArea pre-tile bits should encode");
        payload.extend_from_slice(&fragment);

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)
        );
        (payload, fragment_offset, scan)
    }

    fn push_test_sound_row_with_count(bytes: &mut Vec<u8>, resref: &str, resref_count: u16) {
        let before = bytes.len();
        push_u32(bytes, 0x8000_0088);
        bytes.extend_from_slice(&[0x01, 0x00, 0x00]);
        push_f32(bytes, 1.25);
        push_u32(bytes, 0);
        bytes.push(0);
        push_u32(bytes, 0);
        push_u32(bytes, 0);
        for value in [0.25, 0.5, 0.75, 1.0, 3.0, 4.0, 0.0] {
            push_f32(bytes, value);
        }
        assert_eq!(bytes.len() - before, AREA_SOUND_RESREF_COUNT_OFFSET);
        push_u16(bytes, resref_count);
        assert_eq!(bytes.len() - before, AREA_SOUND_BASE_BYTES);
        push_fixed_resref(bytes, resref);
    }

    fn push_test_sound_row(bytes: &mut Vec<u8>, resref: &str) {
        push_test_sound_row_with_count(bytes, resref, 1);
    }

    fn real_area_sound_static_placeable_payload(
        include_sound_bits: bool,
    ) -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 0); // transition rows
        push_u32(&mut payload, 0); // map-pin rows
        push_u16(&mut payload, 1); // sound rows
        push_test_sound_row(&mut payload, "al_mg_portal1");
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, 1); // static-placeable rows
        push_u32(&mut payload, 0x8000_0042);
        push_u16(&mut payload, 82);
        push_f32(&mut payload, 10.0);
        push_f32(&mut payload, 20.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 1.0);
        push_f32(&mut payload, 0.0);

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        if include_sound_bits {
            payload_bits.extend_from_slice(&[false; 6]);
        }
        let fragment =
            encode_cnw_msb_payload_bits(&payload_bits).expect("test fragment should encode");
        payload.extend_from_slice(&fragment);

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(scan.width, 1);
        assert_eq!(scan.packet_height, 1);
        (payload, fragment_offset, scan)
    }

    fn real_area_map_pin_zero_count_sound_static_placeable_payload()
    -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 0); // transition rows
        push_u32(&mut payload, 1); // map-pin rows
        push_u32(&mut payload, 2); // map-pin id
        let label = b"sound-pin";
        push_u32(&mut payload, label.len() as u32);
        payload.extend_from_slice(label);
        push_f32(&mut payload, 3.0);
        push_f32(&mut payload, 4.0);
        push_f32(&mut payload, 0.0);
        push_u16(&mut payload, 1); // sound rows
        push_test_sound_row_with_count(&mut payload, "al_mg_portal1", 0);
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, 1); // static-placeable rows
        push_u32(&mut payload, 0x8000_0042);
        push_u16(&mut payload, 82);
        push_f32(&mut payload, 10.0);
        push_f32(&mut payload, 20.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 1.0);
        push_f32(&mut payload, 0.0);

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        payload_bits.extend_from_slice(&[false; 6]);
        let fragment =
            encode_cnw_msb_payload_bits(&payload_bits).expect("test fragment should encode");
        payload.extend_from_slice(&fragment);

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(scan.width, 1);
        assert_eq!(scan.packet_height, 1);
        (payload, fragment_offset, scan)
    }

    fn compact_map_note_sound_tail_payload(
        extra_fragment_bits: usize,
    ) -> (Vec<u8>, usize, AreaTileStreamScan, ModuleAreaResourceInfo) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 0); // compact transition/map-note count placeholder
        push_u32(&mut payload, 0x8000_0099);
        push_f32(&mut payload, 3.0);
        push_f32(&mut payload, 4.0);
        push_f32(&mut payload, 0.0);
        let label = b"soundpin";
        push_u32(&mut payload, 0); // compact zero-length string header
        payload.extend_from_slice(label);
        push_u32(&mut payload, 0); // map-pin rows after repaired transition rows
        push_u16(&mut payload, 1); // sound rows
        push_test_sound_row(&mut payload, "al_mg_portal1");
        push_u16(&mut payload, 0); // light-placeable rows
        push_u16(&mut payload, 0); // static-placeable rows

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        payload_bits.extend_from_slice(&[true, false]); // visible direct transition label
        payload_bits.extend_from_slice(&[false; 6]); // sound-object BOOLs
        payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));
        let fragment =
            encode_cnw_msb_payload_bits(&payload_bits).expect("test fragment should encode");
        payload.extend_from_slice(&fragment);

        let info = ModuleAreaResourceInfo {
            resref: "testarea".to_string(),
            name: "Test Area".to_string(),
            width: 1,
            height: 1,
            tileset: "ttr01".to_string(),
            tiles: Vec::new(),
            map_notes: vec![ModuleAreaMapNote {
                text: "soundpin".to_string(),
                x: 3.0,
                y: 4.0,
                z: 0.0,
            }],
            sounds: vec![ModuleAreaSound {
                tag: "test_sound".to_string(),
                x: 3.0,
                y: 4.0,
                z: 0.0,
                resrefs: vec!["al_mg_portal1".to_string()],
            }],
            placeables: Vec::new(),
        };

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(scan.width, 1);
        assert_eq!(scan.packet_height, 1);
        (payload, fragment_offset, scan, info)
    }

    fn real_area_light_static_placeable_payload(
        light_object_id: u32,
    ) -> (Vec<u8>, usize, AreaTileStreamScan) {
        let mut payload = vec![HIGH_LEVEL_ENVELOPE, AREA_MAJOR, AREA_CLIENT_AREA_MINOR];
        payload.extend_from_slice(&[0, 0, 0, 0]);

        pad_to_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        push_u32(&mut payload, 0); // area CExoString length
        let name_end = AREA_NAME_READ_OFFSET + EE_CEXO_STRING_LENGTH_BYTES;

        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        push_u32(&mut payload, 1);
        pad_to_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_fixed_resref(&mut payload, "ttr01");

        push_u32(&mut payload, 1);
        push_u32(&mut payload, 0);
        push_u32(&mut payload, 0);
        push_u16(&mut payload, 0x000C);
        payload.extend_from_slice(&[0, 0]);

        push_u32(&mut payload, 0); // transition rows
        push_u32(&mut payload, 0); // map-pin rows
        push_u16(&mut payload, 0); // sound rows
        push_u16(&mut payload, 1); // light-placeable rows
        push_u32(&mut payload, light_object_id);
        push_u16(&mut payload, 77);
        push_f32(&mut payload, 5.0);
        push_f32(&mut payload, 6.0);
        push_f32(&mut payload, 0.0);
        push_u16(&mut payload, 1); // static-placeable rows
        push_u32(&mut payload, 0x8000_0042);
        push_u16(&mut payload, 82);
        push_f32(&mut payload, 10.0);
        push_f32(&mut payload, 20.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 0.0);
        push_f32(&mut payload, 1.0);
        push_f32(&mut payload, 0.0);

        let read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic payload");
        let fragment_offset = HIGH_LEVEL_HEADER_BYTES + read_size;
        let fragment = encode_cnw_msb_payload_bits(&vec![
            false;
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                - CNW_FRAGMENT_HEADER_BITS
        ])
        .expect("legacy Area_ClientArea pre-tile bits should encode");
        payload.extend_from_slice(&fragment);

        let scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(scan.width, 1);
        assert_eq!(scan.packet_height, 1);
        (payload, fragment_offset, scan)
    }

    fn ee_area_static_placeable_payload_with_post_static_tail(
        first_post_static_rows: &[u16],
        second_post_static_count: u16,
    ) -> Vec<u8> {
        let (legacy_payload, legacy_fragment_offset, legacy_scan) =
            real_area_static_placeable_source_rows_payload_with_count(0, &[]);
        legacy_area_source_tail_exact_read_proof(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_scan,
        )
        .expect("minimal legacy source should have exact post-tile cursor proof");
        let legacy_layout = area_static_layout(&legacy_payload, legacy_fragment_offset)
            .expect("minimal legacy source should expose an area layout");
        assert_eq!(legacy_layout.dialect, AreaStaticDialect::Legacy169);

        let mut payload = expand_legacy_area_static_header_for_ee(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_layout,
        )
        .expect("test payload should expand to the EE static-header dialect");
        let first_count =
            u16::try_from(first_post_static_rows.len()).expect("test row count should fit in WORD");
        push_u16(&mut payload, first_count);
        for row in first_post_static_rows {
            push_u16(&mut payload, *row);
        }
        push_u16(&mut payload, second_post_static_count);

        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        let rewritten_fragment =
            rewrite_area_fragment_bits(&legacy_payload[legacy_fragment_offset..])
                .expect("test fragment should rewrite to the EE Area_ClientArea bit dialect");
        payload.extend_from_slice(&rewritten_fragment);
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic EE payload");
        payload
    }

    fn ee_area_static_placeable_payload_with_static_rows(
        rows: &[(u16, f32, f32, f32, f32, f32, f32)],
        extra_fragment_bits: usize,
    ) -> Vec<u8> {
        let (legacy_payload, legacy_fragment_offset, legacy_scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                u16::try_from(rows.len()).expect("test row count should fit in WORD"),
                rows,
            );
        legacy_area_source_tail_exact_read_proof(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_scan,
        )
        .expect("static-row source should have exact post-tile cursor proof");
        let legacy_layout = area_static_layout(&legacy_payload, legacy_fragment_offset)
            .expect("static-row source should expose an area layout");
        assert_eq!(legacy_layout.dialect, AreaStaticDialect::Legacy169);

        let mut payload = expand_legacy_area_static_header_for_ee(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_layout,
        )
        .expect("test payload should expand to the EE static-header dialect");
        push_u16(&mut payload, 0);
        push_u16(&mut payload, 0);

        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        let mut rewritten_fragment =
            rewrite_area_fragment_bits(&legacy_payload[legacy_fragment_offset..])
                .expect("test fragment should rewrite to the EE Area_ClientArea bit dialect");
        if extra_fragment_bits != 0 {
            let mut bits = decode_cnw_msb_valid_bits(&rewritten_fragment, CNW_FRAGMENT_HEADER_BITS)
                .expect("rewritten test fragment should decode");
            let mut payload_bits = bits.split_off(CNW_FRAGMENT_HEADER_BITS);
            payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));
            rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted EE area fragment bits should encode");
        }
        payload.extend_from_slice(&rewritten_fragment);
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic EE payload");
        payload
    }

    fn ee_area_light_placeable_payload_with_extra_fragment_bits(
        extra_fragment_bits: usize,
    ) -> Vec<u8> {
        let (legacy_payload, legacy_fragment_offset, legacy_scan) =
            real_area_light_static_placeable_payload(0x8000_0077);
        legacy_area_source_tail_exact_read_proof(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_scan,
        )
        .expect("light-row source should have exact post-tile cursor proof");
        let legacy_layout = area_static_layout(&legacy_payload, legacy_fragment_offset)
            .expect("light-row source should expose an area layout");
        assert_eq!(legacy_layout.dialect, AreaStaticDialect::Legacy169);

        let mut payload = expand_legacy_area_static_header_for_ee(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_layout,
        )
        .expect("test payload should expand to the EE static-header dialect");
        push_u16(&mut payload, 0);
        push_u16(&mut payload, 0);

        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        let mut rewritten_fragment =
            rewrite_area_fragment_bits(&legacy_payload[legacy_fragment_offset..])
                .expect("test fragment should rewrite to the EE Area_ClientArea bit dialect");
        if extra_fragment_bits != 0 {
            let mut bits = decode_cnw_msb_valid_bits(&rewritten_fragment, CNW_FRAGMENT_HEADER_BITS)
                .expect("rewritten test fragment should decode");
            let mut payload_bits = bits.split_off(CNW_FRAGMENT_HEADER_BITS);
            payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));
            rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted EE area fragment bits should encode");
        }
        payload.extend_from_slice(&rewritten_fragment);
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic EE payload");
        payload
    }

    fn ee_area_map_pin_payload_with_extra_fragment_bits(extra_fragment_bits: usize) -> Vec<u8> {
        let (legacy_payload, legacy_fragment_offset, legacy_scan) =
            real_area_long_map_pin_static_placeable_payload();
        legacy_area_source_tail_exact_read_proof(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_scan,
        )
        .expect("map-pin source should have exact post-tile cursor proof");
        let legacy_layout = area_static_layout(&legacy_payload, legacy_fragment_offset)
            .expect("map-pin source should expose an area layout");
        assert_eq!(legacy_layout.dialect, AreaStaticDialect::Legacy169);

        let mut payload = expand_legacy_area_static_header_for_ee(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_layout,
        )
        .expect("test payload should expand to the EE static-header dialect");
        push_u16(&mut payload, 0);
        push_u16(&mut payload, 0);

        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        let mut rewritten_fragment =
            rewrite_area_fragment_bits(&legacy_payload[legacy_fragment_offset..])
                .expect("test fragment should rewrite to the EE Area_ClientArea bit dialect");
        if extra_fragment_bits != 0 {
            let mut bits = decode_cnw_msb_valid_bits(&rewritten_fragment, CNW_FRAGMENT_HEADER_BITS)
                .expect("rewritten test fragment should decode");
            let mut payload_bits = bits.split_off(CNW_FRAGMENT_HEADER_BITS);
            payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));
            rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted EE area fragment bits should encode");
        }
        payload.extend_from_slice(&rewritten_fragment);
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic EE payload");
        payload
    }

    fn ee_area_tlk_transition_payload_with_extra_fragment_bits(
        extra_fragment_bits: usize,
    ) -> Vec<u8> {
        let (legacy_payload, legacy_fragment_offset, legacy_scan) =
            real_area_tlk_transition_static_placeable_payload();
        legacy_area_source_tail_exact_read_proof(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_scan,
        )
        .expect("TLK transition source should have exact post-tile cursor proof");
        let legacy_layout = area_static_layout(&legacy_payload, legacy_fragment_offset)
            .expect("TLK transition source should expose an area layout");
        assert_eq!(legacy_layout.dialect, AreaStaticDialect::Legacy169);

        let mut payload = expand_legacy_area_static_header_for_ee(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_layout,
        )
        .expect("test payload should expand to the EE static-header dialect");
        push_u16(&mut payload, 0);
        push_u16(&mut payload, 0);

        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        let mut rewritten_fragment =
            rewrite_area_fragment_bits(&legacy_payload[legacy_fragment_offset..])
                .expect("test fragment should rewrite to the EE Area_ClientArea bit dialect");
        if extra_fragment_bits != 0 {
            let mut bits = decode_cnw_msb_valid_bits(&rewritten_fragment, CNW_FRAGMENT_HEADER_BITS)
                .expect("rewritten test fragment should decode");
            let mut payload_bits = bits.split_off(CNW_FRAGMENT_HEADER_BITS);
            payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));
            rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted EE area fragment bits should encode");
        }
        payload.extend_from_slice(&rewritten_fragment);
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic EE payload");
        payload
    }

    fn ee_area_direct_transition_payload_with_extra_fragment_bits(
        extra_fragment_bits: usize,
    ) -> Vec<u8> {
        let (legacy_payload, legacy_fragment_offset, legacy_scan) =
            real_area_direct_transition_static_placeable_payload(false);
        legacy_area_source_tail_exact_read_proof(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_scan,
        )
        .expect("direct transition source should have exact post-tile cursor proof");
        let legacy_layout = area_static_layout(&legacy_payload, legacy_fragment_offset)
            .expect("direct transition source should expose an area layout");
        assert_eq!(legacy_layout.dialect, AreaStaticDialect::Legacy169);

        let mut payload = expand_legacy_area_static_header_for_ee(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_layout,
        )
        .expect("test payload should expand to the EE static-header dialect");
        push_u16(&mut payload, 0);
        push_u16(&mut payload, 0);

        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        let mut rewritten_fragment =
            rewrite_area_fragment_bits(&legacy_payload[legacy_fragment_offset..])
                .expect("test fragment should rewrite to the EE Area_ClientArea bit dialect");
        if extra_fragment_bits != 0 {
            let mut bits = decode_cnw_msb_valid_bits(&rewritten_fragment, CNW_FRAGMENT_HEADER_BITS)
                .expect("rewritten test fragment should decode");
            let mut payload_bits = bits.split_off(CNW_FRAGMENT_HEADER_BITS);
            payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));
            rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted EE area fragment bits should encode");
        }
        payload.extend_from_slice(&rewritten_fragment);
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic EE payload");
        payload
    }

    fn ee_area_sound_payload_with_extra_fragment_bits(extra_fragment_bits: usize) -> Vec<u8> {
        let (legacy_payload, legacy_fragment_offset, legacy_scan) =
            real_area_sound_static_placeable_payload(true);
        legacy_area_source_tail_exact_read_proof(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_scan,
        )
        .expect("sound-row source should have exact post-tile cursor proof");
        let legacy_layout = area_static_layout(&legacy_payload, legacy_fragment_offset)
            .expect("sound-row source should expose an area layout");
        assert_eq!(legacy_layout.dialect, AreaStaticDialect::Legacy169);

        let mut payload = expand_legacy_area_static_header_for_ee(
            &legacy_payload,
            legacy_fragment_offset,
            &legacy_layout,
        )
        .expect("test payload should expand to the EE static-header dialect");
        push_u16(&mut payload, 0);
        push_u16(&mut payload, 0);

        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        let mut rewritten_fragment =
            rewrite_area_fragment_bits(&legacy_payload[legacy_fragment_offset..])
                .expect("test fragment should rewrite to the EE Area_ClientArea bit dialect");
        if extra_fragment_bits != 0 {
            let mut bits = decode_cnw_msb_valid_bits(&rewritten_fragment, CNW_FRAGMENT_HEADER_BITS)
                .expect("rewritten test fragment should decode");
            let mut payload_bits = bits.split_off(CNW_FRAGMENT_HEADER_BITS);
            payload_bits.extend(std::iter::repeat(false).take(extra_fragment_bits));
            rewritten_fragment = encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted EE area fragment bits should encode");
        }
        payload.extend_from_slice(&rewritten_fragment);
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit in the synthetic EE payload");
        payload
    }

    #[test]
    fn transition_tlk_source_rows_consume_exactly_three_fragment_bits() {
        let (payload, fragment_offset, scan) = real_area_tlk_transition_static_placeable_payload();
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("TLK transition branch should have exact source cursor proof");
        assert_eq!(proof.static_rows_count, 1);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS + 3)
        );

        let mut extra_bit_payload = payload.clone();
        extra_bit_payload.truncate(fragment_offset);
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        payload_bits.extend_from_slice(&[false, true, false, false]);
        extra_bit_payload.extend_from_slice(
            &encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted TLK transition bits should encode"),
        );
        let shifted_scan = scan_area_tile_stream(&extra_bit_payload, fragment_offset);
        assert!(shifted_scan.valid);
        assert!(
            legacy_area_source_tail_exact_read_proof(
                &extra_bit_payload,
                fragment_offset,
                &shifted_scan,
            )
            .is_none(),
            "Diamond/EE TLK transition labels own exactly visibility, selector, and TLK guard bits"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &extra_bit_payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose later rows after an unowned transition-list fragment bit"
        );
    }

    #[test]
    fn exact_ee_area_proof_requires_zero_post_static_counts() {
        let zero_tail = ee_area_static_placeable_payload_with_post_static_tail(&[], 0);
        let proof = ee_area_client_area_exact_read_proof(&zero_tail)
            .expect("two zero post-static WORDs should satisfy the EE reader proof");
        assert_eq!(proof.static_count, 0);
        assert_eq!(proof.first_post_static_count, 0);
        assert_eq!(proof.second_post_static_count, 0);
        assert_eq!(proof.read_end, proof.read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let nonzero_first = ee_area_static_placeable_payload_with_post_static_tail(&[0x1234], 0);
        assert!(
            ee_area_client_area_exact_read_proof(&nonzero_first).is_none(),
            "the legacy bridge dialect owns this tail as two zero WORDs; a nonzero first list has no proven row contract"
        );

        let nonzero_second = ee_area_static_placeable_payload_with_post_static_tail(&[], 1);
        assert!(
            ee_area_client_area_exact_read_proof(&nonzero_second).is_none(),
            "the second post-static count is also bridge-owned zero for legacy Area_ClientArea rewrites"
        );
    }

    #[test]
    fn exact_ee_area_proof_rejects_pre_tile_inserted_branch_drift() {
        let exact = ee_area_static_placeable_payload_with_static_rows(&[], 0);
        let proof = ee_area_client_area_exact_read_proof(&exact)
            .expect("minimal EE area should satisfy the exact pre-tile cursor proof");
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let mut tileset_options_bit = exact.clone();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&tileset_options_bit).expect("test area read window");
        set_cnw_msb_bit(
            tileset_options_bit
                .get_mut(fragment_offset..)
                .expect("test fragment slice"),
            EE_AREA_BUILD36_3_TILESET_OPTIONS_BOOL_BIT_INDEX,
        )
        .expect("test should set the EE tileset-options BOOL");
        assert!(
            ee_area_client_area_exact_read_proof(&tileset_options_bit).is_none(),
            "EE build-36.3 tileset-options BOOL is an inserted false branch for legacy rewrites"
        );

        let mut tileset_options_count = exact.clone();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&tileset_options_count).expect("test area read window");
        let layout = area_static_layout(&tileset_options_count, fragment_offset)
            .expect("test EE area should expose a static layout");
        write_area_u32(
            &mut tileset_options_count,
            fragment_offset,
            layout.tileset_read_offset + CRESREF_TEXT_BYTES,
            1,
        )
        .expect("test should set a non-empty tileset-options count");
        assert!(
            ee_area_client_area_exact_read_proof(&tileset_options_count).is_none(),
            "EE build-36.3 tileset-options count is bridge-owned zero until a row shape is proven"
        );

        let mut tile_loop_bit = exact.clone();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&tile_loop_bit).expect("test area read window");
        set_cnw_msb_bit(
            tile_loop_bit
                .get_mut(fragment_offset..)
                .expect("test fragment slice"),
            EE_AREA_BUILD36_5_TILE_LOOP_BOOL_BIT_INDEX,
        )
        .expect("test should set the EE tile-loop BOOL");
        assert!(
            ee_area_client_area_exact_read_proof(&tile_loop_bit).is_none(),
            "EE build-36.5 tile-loop BOOL is an inserted false branch before tile rows"
        );
    }

    #[test]
    fn static_placeable_source_rows_do_not_consume_fragment_bits() {
        let (payload, fragment_offset, scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                1,
                &[(82, 10.0, 20.0, 0.0, 0.0, 1.0, 0.0)],
            );
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("one static row should have exact source cursor proof");
        let (_, read_size, _, _) =
            area_client_area_read_window(&payload).expect("test area read window");
        assert_eq!(proof.static_rows_count, 1);
        assert_eq!(
            proof.static_rows_read_offset + AREA_STATIC_PLACEABLE_ROW_BYTES,
            read_size
        );
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)
        );

        let mut extra_bit_payload = payload.clone();
        extra_bit_payload.truncate(fragment_offset);
        extra_bit_payload.extend_from_slice(
            &encode_cnw_msb_payload_bits(&vec![
                false;
                LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                    - CNW_FRAGMENT_HEADER_BITS
                    + 1
            ])
            .expect("shifted legacy area bits should encode"),
        );
        let shifted_scan = scan_area_tile_stream(&extra_bit_payload, fragment_offset);
        assert!(shifted_scan.valid);
        assert!(
            legacy_area_source_tail_exact_read_proof(
                &extra_bit_payload,
                fragment_offset,
                &shifted_scan,
            )
            .is_none(),
            "Diamond static-placeable rows are read-buffer only; an extra CNW bit after the row is unowned"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &extra_bit_payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose static rows after an unowned static-list fragment bit"
        );
    }

    #[test]
    fn light_placeable_source_rows_do_not_consume_fragment_bits() {
        let (payload, fragment_offset, scan) =
            real_area_light_static_placeable_payload(0x8000_0077);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("light row should have exact source cursor proof");
        assert_eq!(proof.static_rows_count, 1);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)
        );

        let mut extra_bit_payload = payload.clone();
        extra_bit_payload.truncate(fragment_offset);
        extra_bit_payload.extend_from_slice(
            &encode_cnw_msb_payload_bits(&vec![
                false;
                LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                    - CNW_FRAGMENT_HEADER_BITS
                    + 1
            ])
            .expect("shifted legacy area bits should encode"),
        );
        let shifted_scan = scan_area_tile_stream(&extra_bit_payload, fragment_offset);
        assert!(shifted_scan.valid);
        assert!(
            legacy_area_source_tail_exact_read_proof(
                &extra_bit_payload,
                fragment_offset,
                &shifted_scan,
            )
            .is_none(),
            "Diamond light-placeable rows are read-buffer only; an extra CNW bit before static rows is unowned"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &extra_bit_payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose light/static rows after an unowned light-list fragment bit"
        );
    }

    #[test]
    fn map_pin_source_rows_do_not_consume_fragment_bits() {
        let (payload, fragment_offset, scan) = real_area_long_map_pin_static_placeable_payload();
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("map-pin row should have exact source cursor proof");
        assert_eq!(proof.static_rows_count, 1);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)
        );

        let mut extra_bit_payload = payload.clone();
        extra_bit_payload.truncate(fragment_offset);
        extra_bit_payload.extend_from_slice(
            &encode_cnw_msb_payload_bits(&vec![
                false;
                LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                    - CNW_FRAGMENT_HEADER_BITS
                    + 1
            ])
            .expect("shifted legacy area bits should encode"),
        );
        let shifted_scan = scan_area_tile_stream(&extra_bit_payload, fragment_offset);
        assert!(shifted_scan.valid);
        assert!(
            legacy_area_source_tail_exact_read_proof(
                &extra_bit_payload,
                fragment_offset,
                &shifted_scan,
            )
            .is_none(),
            "Diamond map-pin rows are read-buffer only; an extra CNW bit before sound/static lists is unowned"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &extra_bit_payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose later rows after an unowned map-pin fragment bit"
        );
    }

    #[test]
    fn sound_source_rows_consume_exactly_six_fragment_bits() {
        let (payload, fragment_offset, scan) = real_area_sound_static_placeable_payload(true);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("sound row with six BOOLs should have exact source cursor proof");
        assert_eq!(proof.sound_count, 1);
        assert_eq!(proof.static_rows_count, 1);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS + 6)
        );

        let mut extra_bit_payload = payload.clone();
        extra_bit_payload.truncate(fragment_offset);
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        payload_bits.extend_from_slice(&[false; 7]);
        extra_bit_payload.extend_from_slice(
            &encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted legacy area sound bits should encode"),
        );
        let shifted_scan = scan_area_tile_stream(&extra_bit_payload, fragment_offset);
        assert!(shifted_scan.valid);
        assert!(
            legacy_area_source_tail_exact_read_proof(
                &extra_bit_payload,
                fragment_offset,
                &shifted_scan,
            )
            .is_none(),
            "Diamond/EE sound rows own exactly six CNW BOOLs; a seventh bit before light/static rows is unowned"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &extra_bit_payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose later rows after an unowned sound-list fragment bit"
        );
    }

    #[test]
    fn exact_ee_area_proof_rejects_static_placeable_fragment_tail() {
        let exact = ee_area_static_placeable_payload_with_static_rows(
            &[(82, 10.0, 20.0, 0.0, 0.0, 1.0, 0.0)],
            0,
        );
        let proof = ee_area_client_area_exact_read_proof(&exact)
            .expect("one EE static row plus zero post-static WORDs should be exact");
        assert_eq!(proof.static_count, 1);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let shifted = ee_area_static_placeable_payload_with_static_rows(
            &[(82, 10.0, 20.0, 0.0, 0.0, 1.0, 0.0)],
            1,
        );
        assert!(
            ee_area_client_area_exact_read_proof(&shifted).is_none(),
            "EE static-placeable rows also own no CNW fragment bits; a byte-exact row with one extra bit must stay unclaimed"
        );
    }

    #[test]
    fn exact_ee_area_proof_rejects_transition_fragment_tail() {
        let exact = ee_area_tlk_transition_payload_with_extra_fragment_bits(0);
        let proof = ee_area_client_area_exact_read_proof(&exact)
            .expect("one EE TLK transition row and one static row should be exact");
        assert_eq!(proof.transition_count, 1);
        assert_eq!(proof.static_count, 1);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let shifted = ee_area_tlk_transition_payload_with_extra_fragment_bits(1);
        assert!(
            ee_area_client_area_exact_read_proof(&shifted).is_none(),
            "EE transition labels own exactly their decompiled locstring bits before map/sound/static lists"
        );

        let exact_direct = ee_area_direct_transition_payload_with_extra_fragment_bits(0);
        let direct_proof = ee_area_client_area_exact_read_proof(&exact_direct)
            .expect("one EE direct transition row and one static row should be exact");
        assert_eq!(direct_proof.transition_count, 1);
        assert_eq!(direct_proof.static_count, 1);
        assert_eq!(
            direct_proof.fragment_bits_consumed,
            direct_proof.fragment_bits_available
        );

        let shifted_direct = ee_area_direct_transition_payload_with_extra_fragment_bits(1);
        assert!(
            ee_area_client_area_exact_read_proof(&shifted_direct).is_none(),
            "EE direct transition labels own only visibility and selector bits before later lists"
        );
    }

    #[test]
    fn exact_ee_area_proof_rejects_map_pin_fragment_tail() {
        let exact = ee_area_map_pin_payload_with_extra_fragment_bits(0);
        let proof = ee_area_client_area_exact_read_proof(&exact)
            .expect("one EE map-pin row and one static row should be exact");
        assert_eq!(proof.map_pin_count, 1);
        assert_eq!(proof.static_count, 1);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let shifted = ee_area_map_pin_payload_with_extra_fragment_bits(1);
        assert!(
            ee_area_client_area_exact_read_proof(&shifted).is_none(),
            "EE map-pin rows also own no CNW fragment bits before sound/static lists"
        );
    }

    #[test]
    fn exact_ee_area_proof_rejects_sound_fragment_tail() {
        let exact = ee_area_sound_payload_with_extra_fragment_bits(0);
        let proof = ee_area_client_area_exact_read_proof(&exact)
            .expect("one EE sound row and one static row should be exact");
        assert_eq!(proof.sound_count, 1);
        assert_eq!(proof.static_count, 1);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let shifted = ee_area_sound_payload_with_extra_fragment_bits(1);
        assert!(
            ee_area_client_area_exact_read_proof(&shifted).is_none(),
            "EE sound rows own exactly six CNW BOOLs before light/static lists"
        );
    }

    #[test]
    fn exact_ee_area_proof_rejects_light_placeable_fragment_tail() {
        let exact = ee_area_light_placeable_payload_with_extra_fragment_bits(0);
        let proof = ee_area_client_area_exact_read_proof(&exact)
            .expect("one EE light row and one static row should be exact");
        assert_eq!(proof.light_count, 1);
        assert_eq!(proof.static_count, 1);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let shifted = ee_area_light_placeable_payload_with_extra_fragment_bits(1);
        assert!(
            ee_area_client_area_exact_read_proof(&shifted).is_none(),
            "EE light-placeable rows also own no CNW fragment bits before static rows"
        );
    }

    #[test]
    fn tile_rows_do_not_consume_fragment_bits_before_post_tile_lists() {
        let (payload, fragment_offset, scan) =
            real_area_static_placeable_source_rows_payload_with_count(0, &[]);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("tile-only source area should have exact cursor proof");
        assert_eq!(proof.static_rows_count, 0);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS)
        );

        let mut shifted_payload = payload.clone();
        shifted_payload.truncate(fragment_offset);
        shifted_payload.extend_from_slice(
            &encode_cnw_msb_payload_bits(&vec![
                false;
                LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS
                    - CNW_FRAGMENT_HEADER_BITS
                    + 1
            ])
            .expect("shifted legacy area bits should encode"),
        );
        let shifted_scan = scan_area_tile_stream(&shifted_payload, fragment_offset);
        assert!(shifted_scan.valid);
        assert!(
            legacy_area_source_tail_exact_read_proof(
                &shifted_payload,
                fragment_offset,
                &shifted_scan,
            )
            .is_none(),
            "Diamond area tile rows are dimension-driven read-buffer records; an extra CNW fragment bit before post-tile lists is unowned"
        );
    }

    #[test]
    fn exact_ee_area_proof_rejects_tile_loop_fragment_tail() {
        let exact = ee_area_static_placeable_payload_with_static_rows(&[], 0);
        let proof = ee_area_client_area_exact_read_proof(&exact)
            .expect("tile-only EE area with empty post-static tail should be exact");
        assert_eq!(proof.static_count, 0);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);

        let shifted = ee_area_static_placeable_payload_with_static_rows(&[], 1);
        assert!(
            ee_area_client_area_exact_read_proof(&shifted).is_none(),
            "EE area tile rows also own no CNW fragment bits; shifted tile-loop fragments must stay unclaimed"
        );
    }

    #[test]
    fn placeable_context_uses_transition_locstring_bits_before_static_rows() {
        let (payload, fragment_offset, scan) = real_area_tlk_transition_static_placeable_payload();
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("TLK transition branch should keep exact source cursor proof");
        assert_eq!(proof.static_rows_count, 1);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("placeable context walker must share the transition label bit cursor");
        assert_eq!(context.static_rows.len(), 1);
        assert_eq!(context.light_rows.len(), 0);
        assert_eq!(context.static_rows[0].appearance, 82);
        assert_eq!(context.static_rows[0].x, 10.0);
        assert_eq!(context.static_rows[0].y, 20.0);
        assert!(context.static_rows[0].has_direction);
    }

    #[test]
    fn placeable_context_uses_direct_transition_cexostring_bits_before_static_rows() {
        let (payload, fragment_offset, scan) =
            real_area_direct_transition_static_placeable_payload(false);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("direct transition label should keep exact source cursor proof");
        assert_eq!(proof.static_rows_count, 1);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("placeable context walker must share the direct-label transition bit cursor");
        assert_eq!(context.static_rows.len(), 1);
        assert_eq!(context.light_rows.len(), 0);
        assert_eq!(context.static_rows[0].appearance, 82);
        assert_eq!(context.static_rows[0].x, 10.0);
        assert_eq!(context.static_rows[0].y, 20.0);
        assert!(context.static_rows[0].has_direction);
    }

    #[test]
    fn direct_transition_cexostring_does_not_consume_tlk_guard_bit() {
        let (payload, fragment_offset, scan) =
            real_area_direct_transition_static_placeable_payload(true);
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan).is_none(),
            "a direct transition label owns only visibility and selector bits; the TLK-only guard bit must remain unclaimed"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose static rows after an unowned direct-label bit"
        );
    }

    #[test]
    fn placeable_context_uses_map_pin_cursor_before_static_rows() {
        let (payload, fragment_offset, scan) = real_area_long_map_pin_static_placeable_payload();
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("long CExoString map-pin label should keep exact source cursor proof");
        assert_eq!(proof.static_rows_count, 1);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("placeable context walker must share the exact map-pin cursor");
        assert_eq!(context.light_rows.len(), 0);
        assert_eq!(context.static_rows.len(), 1);
        assert_eq!(context.static_rows[0].appearance, 82);
        assert_eq!(context.static_rows[0].x, 10.0);
        assert_eq!(context.static_rows[0].y, 20.0);
        assert!(context.static_rows[0].has_direction);
    }

    #[test]
    fn placeable_context_uses_sound_bits_before_static_rows() {
        let (payload, fragment_offset, scan) = real_area_sound_static_placeable_payload(true);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("sound rows with six BOOLs should keep exact source cursor proof");
        assert_eq!(proof.sound_count, 1);
        assert_eq!(proof.static_rows_count, 1);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("placeable context walker must share the exact sound bit cursor");
        assert_eq!(context.light_rows.len(), 0);
        assert_eq!(context.static_rows.len(), 1);
        assert_eq!(context.static_rows[0].appearance, 82);
        assert_eq!(context.static_rows[0].x, 10.0);
        assert_eq!(context.static_rows[0].y, 20.0);
        assert!(context.static_rows[0].has_direction);
    }

    #[test]
    fn placeable_context_rejects_sound_rows_without_bits() {
        let (payload, fragment_offset, scan) = real_area_sound_static_placeable_payload(false);
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan).is_none(),
            "a sound byte row without its six fragment BOOLs is not an exact Diamond area tail"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose later static rows when the sound-row bit cursor is unproven"
        );
    }

    #[test]
    fn zero_sound_count_repair_uses_shared_map_pin_cursor() {
        let (mut payload, fragment_offset, scan) =
            real_area_map_pin_zero_count_sound_static_placeable_payload();
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan).is_none(),
            "a zero resref count plus a following CResRef is a compact legacy sound row until repaired"
        );

        let repairs = repair_legacy_zero_sound_counts(&mut payload, fragment_offset, &scan)
            .expect("sound repair should use the decompiled map-pin cursor before sound rows");
        assert_eq!(repairs, 1);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("repaired sound row should keep exact source cursor proof");
        assert_eq!(proof.sound_count, 1);
        assert_eq!(proof.static_rows_count, 1);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("context walker must share map-pin and repaired sound-row cursor proof");
        assert_eq!(context.light_rows.len(), 0);
        assert_eq!(context.static_rows.len(), 1);
        assert_eq!(context.static_rows[0].appearance, 82);
    }

    #[test]
    fn zero_sound_count_repair_requires_exact_sound_fragment_cursor() {
        let (payload, fragment_offset, _) =
            real_area_map_pin_zero_count_sound_static_placeable_payload();
        let mut shifted_payload = payload.clone();
        shifted_payload.truncate(fragment_offset);
        let mut payload_bits =
            vec![false; LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS];
        payload_bits.extend_from_slice(&[false; 7]);
        shifted_payload.extend_from_slice(
            &encode_cnw_msb_payload_bits(&payload_bits)
                .expect("shifted legacy sound bits should encode"),
        );
        let shifted_scan = scan_area_tile_stream(&shifted_payload, fragment_offset);
        assert!(shifted_scan.valid);

        let fragment_bits_available =
            area_payload_fragment_bits_available(&shifted_payload, fragment_offset)
                .expect("shifted fragment should expose valid bits");
        let (_, cursor, _) = advance_area_transition_rows(
            &shifted_payload,
            fragment_offset,
            shifted_scan.tile_end_read_offset,
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS,
            fragment_bits_available,
        )
        .expect("test transition count should advance");
        let (_, sound_count_offset) =
            advance_area_map_pin_rows(&shifted_payload, fragment_offset, cursor)
                .expect("test map pin should advance");
        let sound_row_count_offset = sound_count_offset + 2 + AREA_SOUND_RESREF_COUNT_OFFSET;
        assert_eq!(
            read_area_u16(&shifted_payload, fragment_offset, sound_count_offset),
            Some(1)
        );
        assert_eq!(
            read_area_u16(&shifted_payload, fragment_offset, sound_row_count_offset),
            Some(0)
        );

        let original_shifted_payload = shifted_payload.clone();
        assert!(
            repair_legacy_zero_sound_counts(&mut shifted_payload, fragment_offset, &shifted_scan)
                .is_none(),
            "zero-count sound repair must not claim a row unless the six sound BOOLs consume the exact fragment cursor"
        );
        assert_eq!(
            shifted_payload, original_shifted_payload,
            "failed sound-count repair must leave the byte row untouched"
        );
    }

    #[test]
    fn compact_post_tile_tail_repair_requires_exact_fragment_cursor() {
        let (mut payload, fragment_offset, scan, info) = compact_map_note_sound_tail_payload(0);
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan).is_none(),
            "compact map-note tails are unclaimed until the transition count and CExoString header are repaired"
        );
        assert!(repair_compact_post_tile_tail_for_ee(
            &mut payload,
            fragment_offset,
            &scan,
            &info,
        ));
        let repaired_scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(repaired_compact_post_tile_scan_matches(
            &scan,
            &repaired_scan
        ));
        let proof =
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &repaired_scan)
                .expect("repaired compact post-tile tail should consume the exact source cursor");
        assert_eq!(proof.sound_count, 1);
        assert_eq!(proof.static_rows_count, 0);
        assert_eq!(
            area_payload_fragment_bits_available(&payload, fragment_offset),
            Some(LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS + 2 + 6)
        );

        let (mut shifted_payload, shifted_fragment_offset, shifted_scan, shifted_info) =
            compact_map_note_sound_tail_payload(1);
        let original_shifted_payload = shifted_payload.clone();
        assert!(
            !repair_compact_post_tile_tail_for_ee(
                &mut shifted_payload,
                shifted_fragment_offset,
                &shifted_scan,
                &shifted_info,
            ),
            "compact post-tile repair must reject a tail with one unowned transition/sound fragment bit"
        );
        assert_eq!(
            shifted_payload, original_shifted_payload,
            "failed compact post-tile repair must leave the source payload untouched"
        );
    }

    #[test]
    fn placeable_context_uses_light_rows_before_static_rows() {
        let (payload, fragment_offset, scan) =
            real_area_light_static_placeable_payload(0x8000_0077);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("light row should keep exact source cursor proof");
        assert_eq!(proof.static_rows_count, 1);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("placeable context walker must share the exact light-row cursor");
        assert_eq!(context.light_rows.len(), 1);
        assert_eq!(context.static_rows.len(), 1);
        assert_eq!(context.light_rows[0].object_id, 0x8000_0077);
        assert_eq!(context.light_rows[0].appearance, 77);
        assert_eq!(context.light_rows[0].x, 5.0);
        assert_eq!(context.light_rows[0].y, 6.0);
        assert!(!context.light_rows[0].has_direction);
        assert_eq!(context.static_rows[0].appearance, 82);
        assert!(context.static_rows[0].has_direction);

        assert!(context.contains_light_placeable_id(0x8000_0077));
        assert!(!context.contains_static_placeable_id(0x8000_0077));
        let light_matches = context
            .matching_placeable_rows(0x8000_0077)
            .collect::<Vec<_>>();
        assert_eq!(light_matches.len(), 1);
        assert_eq!(light_matches[0].kind, AreaPlaceableContextRowKind::Light);
        assert_eq!(light_matches[0].row.object_id, 0x8000_0077);

        assert!(!context.contains_light_placeable_id(0x8000_0042));
        assert!(context.contains_static_placeable_id(0x8000_0042));
        let static_matches = context
            .matching_placeable_rows(0x8000_0042)
            .collect::<Vec<_>>();
        assert_eq!(static_matches.len(), 1);
        assert_eq!(static_matches[0].kind, AreaPlaceableContextRowKind::Static);
        assert_eq!(static_matches[0].row.object_id, 0x8000_0042);
    }

    #[test]
    fn placeable_context_marks_duplicate_light_static_object_ids() {
        let duplicate_object_id = 0x8000_0042;
        let (payload, fragment_offset, _scan) =
            real_area_light_static_placeable_payload(duplicate_object_id);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("duplicate light/static ids should still expose exact wire rows");

        assert_eq!(context.light_rows.len(), 1);
        assert_eq!(context.static_rows.len(), 1);
        assert_eq!(
            context.light_rows[0].object_id_confidence,
            AreaPlaceableContextObjectIdConfidence::DuplicateObjectId
        );
        assert_eq!(
            context.static_rows[0].object_id_confidence,
            AreaPlaceableContextObjectIdConfidence::DuplicateObjectId
        );

        let matches = context
            .matching_placeable_rows(duplicate_object_id)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].kind, AreaPlaceableContextRowKind::Light);
        assert_eq!(matches[1].kind, AreaPlaceableContextRowKind::Static);
    }

    #[test]
    fn placeable_context_rejects_unproven_light_row_shape() {
        let (payload, fragment_offset, scan) =
            real_area_light_static_placeable_payload(0x0000_0077);
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan).is_none(),
            "light rows outside the legacy object namespace are not an exact Diamond area tail"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &payload,
                fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose later static rows after an unproven light row"
        );
    }

    #[test]
    fn placeable_context_does_not_claim_zero_count_static_tail_rows() {
        let (payload, fragment_offset, scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                0,
                &[(82, 10.0, 20.0, 0.0, 0.0, 1.0, 0.0)],
            );
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("row-shaped bytes after a zero static count remain a bounded legacy tail");
        assert_eq!(proof.static_rows_count, 1);
        assert_eq!(proof.zero_static_placeable_rows, 1);

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            None,
        )
        .expect("zero-count static tail still has exact source cursor proof");
        assert_eq!(context.light_rows.len(), 0);
        assert_eq!(
            context.static_rows.len(),
            0,
            "static context must follow the decompiled WORD count, not row-shaped tail bytes"
        );
    }

    #[test]
    fn placeable_context_requires_exact_tail_after_static_rows() {
        let (mut payload, fragment_offset, _scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                1,
                &[(82, 10.0, 20.0, 0.0, 0.0, 1.0, 0.0)],
            );
        let fragment = payload[fragment_offset..].to_vec();
        payload.truncate(fragment_offset);
        push_u16(&mut payload, 0xBEEF);
        let new_read_size = payload.len() - HIGH_LEVEL_HEADER_BYTES;
        write_u32_le(
            &mut payload,
            HIGH_LEVEL_HEADER_BYTES,
            (HIGH_LEVEL_HEADER_BYTES + new_read_size) as u32,
        )
        .expect("declared read size should fit after adding trailing bytes");
        let new_fragment_offset = payload.len();
        payload.extend_from_slice(&fragment);

        let shifted_scan = scan_area_tile_stream(&payload, new_fragment_offset);
        assert!(shifted_scan.valid);
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, new_fragment_offset, &shifted_scan)
                .is_none(),
            "trailing bytes after the static-row loop are outside the Diamond area-tail proof"
        );
        assert!(
            collect_area_post_tile_placeable_context(
                &payload,
                new_fragment_offset,
                "testarea",
                0x8000_0001,
                false,
                None,
            )
            .is_none(),
            "context collection must not expose static rows from a non-exact post-tile tail"
        );
    }

    #[test]
    fn zero_count_static_tail_is_not_normalized_or_module_repaired() {
        let placeable = ModuleAreaPlaceable {
            tag: "unclaimed_tail_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_2,
            static_object: true,
            useable: true,
            trap_flag: true,
            trap_disarmable: true,
            lockable: true,
            locked: true,
        };
        let info = module_info_with_placeables(vec![placeable]);
        let (mut payload, fragment_offset, scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                0,
                &[(82, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0)],
            );
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("zero-count row-shaped tail should still have a bounded source proof");
        assert_eq!(proof.static_rows_count, 1);
        assert_eq!(proof.zero_static_placeable_rows, 1);
        let original = payload.clone();

        assert_eq!(
            normalize_legacy_static_placeable_directions(&mut payload, fragment_offset, &scan),
            Some(0),
            "normalization must not claim row-shaped bytes after a zero static count"
        );
        assert_eq!(
            repair_module_resource_static_placeable_rows(
                &mut payload,
                fragment_offset,
                &scan,
                &info
            ),
            Some(0),
            "module GIT proof must not promote row-shaped bytes after a zero static count"
        );
        assert_eq!(
            payload, original,
            "unclaimed zero-count static tail bytes must stay untouched until the drop path owns them"
        );

        let dropped =
            drop_legacy_zero_static_placeable_trailing_rows(&mut payload, fragment_offset, &scan)
                .expect("zero-count static tail should be droppable");
        assert_eq!(dropped, 1);
        let new_fragment_offset = HIGH_LEVEL_HEADER_BYTES + proof.static_rows_read_offset;
        let trimmed_scan = scan_area_tile_stream(&payload, new_fragment_offset);
        let trimmed_proof =
            legacy_area_source_tail_exact_read_proof(&payload, new_fragment_offset, &trimmed_scan)
                .expect("trimmed zero-count source should keep exact cursor proof");
        assert_eq!(trimmed_proof.static_rows_count, 0);
        assert_eq!(trimmed_proof.zero_static_placeable_rows, 0);
    }

    #[test]
    fn zero_count_static_tail_drop_rejects_later_bad_row_without_partial_shorten() {
        let (mut payload, fragment_offset, scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                0,
                &[
                    (82, 10.0, 20.0, 0.0, 0.0, 1.0, 0.0),
                    (83, 30.0, 40.0, 0.0, 1.0, 0.0, 0.0),
                ],
            );
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("two zero-count tail rows should be a bounded legacy tail before corruption");
        assert_eq!(proof.zero_static_placeable_rows, 2);
        let second_tail_row = proof.static_rows_read_offset + AREA_STATIC_PLACEABLE_ROW_BYTES;
        write_area_u32(&mut payload, fragment_offset, second_tail_row, 0)
            .expect("test should be able to corrupt the second tail object id");
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan).is_none(),
            "a bad later row means the zero-count tail is not a bounded row sequence"
        );
        let original = payload.clone();

        assert_eq!(
            drop_legacy_zero_static_placeable_trailing_rows(&mut payload, fragment_offset, &scan),
            None,
            "the drop path must not trim a valid prefix of an unproven zero-count tail"
        );
        assert_eq!(
            payload, original,
            "rejected zero-count tail drop must leave the read buffer and fragment offset unchanged"
        );
    }

    #[test]
    fn static_direction_normalization_rejects_zero_vector_without_inventing_yaw() {
        assert_eq!(
            normalize_static_placeable_direction_components(0.0, 0.0, 0.0),
            None
        );
        assert_eq!(
            normalize_static_placeable_direction_components(1.0e-8, 0.0, 0.0),
            None
        );
        assert!(
            !static_placeable_direction_is_ee_safe(0.0, 0.0, 0.0),
            "zero-length static-placeable rows cannot satisfy the EE direction proof"
        );
    }

    #[test]
    fn static_direction_normalization_preserves_nonzero_yaw() {
        let (dir_x, dir_y, dir_z) = normalize_static_placeable_direction_components(2.0, -3.0, 4.0)
            .expect("nonzero horizontal vector should normalize");
        let source_yaw = (-2.0f32).atan2(-3.0);
        let normalized_yaw = (-dir_x).atan2(dir_y);

        assert!(static_placeable_direction_is_ee_safe(dir_x, dir_y, dir_z));
        assert!(
            angle_delta(normalized_yaw, source_yaw).abs() <= 1.0e-6,
            "normalization must preserve the decompiled atan2(-x, y) yaw"
        );
    }

    #[test]
    fn static_direction_normalization_rejects_later_zero_vector_without_partial_write() {
        let (mut payload, fragment_offset, scan) = static_placeable_source_rows_payload(&[
            (82, 10.0, 20.0, 0.0, 2.0, 0.0, 3.0),
            (83, 30.0, 40.0, 0.0, 0.0, 0.0, 0.0),
        ]);
        legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("synthetic multi-row source should have an exact legacy cursor proof");
        let original = payload.clone();

        assert_eq!(
            normalize_legacy_static_placeable_directions(&mut payload, fragment_offset, &scan),
            None,
            "one unrepairable static direction row must reject the whole staged normalization"
        );
        assert_eq!(
            payload, original,
            "rejected static-direction normalization must leave earlier rows untouched"
        );
    }

    #[test]
    fn square_dimension_repair_commits_only_after_exact_tail_proof() {
        let (mut exact, fragment_offset) = fixed_name_square_dimension_payload();
        let layout = area_static_layout(&exact, fragment_offset).expect("synthetic area layout");
        assert_eq!(layout.area_name_encoding, AreaNameEncoding::DiamondFixed20);
        let mut scan = scan_area_tile_stream(&exact, fragment_offset);
        assert!(!scan.valid);

        assert!(
            repair_missing_square_area_dimensions(&mut exact, fragment_offset, &mut scan),
            "exact one-tile square source should accept the inferred 1x1 dimensions"
        );
        assert!(scan.valid);
        assert_eq!(
            read_area_u32(&exact, fragment_offset, layout.width_read_offset),
            Some(1)
        );
        assert_eq!(
            read_area_u32(&exact, fragment_offset, layout.height_read_offset),
            Some(1)
        );

        let (source, _) = fixed_name_square_dimension_payload();
        let (mut shifted, shifted_fragment_offset) =
            legacy_area_payload_with_extra_fragment_bits(&source, 1);
        let shifted_layout =
            area_static_layout(&shifted, shifted_fragment_offset).expect("shifted area layout");
        let mut shifted_scan = scan_area_tile_stream(&shifted, shifted_fragment_offset);
        let original_shifted = shifted.clone();
        assert!(
            !repair_missing_square_area_dimensions(
                &mut shifted,
                shifted_fragment_offset,
                &mut shifted_scan,
            ),
            "one unowned post-tile fragment bit must block the inferred square dimensions"
        );
        assert_eq!(
            shifted, original_shifted,
            "rejected square-dimension candidates must not rewrite the source bytes"
        );
        assert_eq!(
            read_area_u32(
                &shifted,
                shifted_fragment_offset,
                shifted_layout.width_read_offset,
            ),
            Some(0)
        );
        assert_eq!(
            read_area_u32(
                &shifted,
                shifted_fragment_offset,
                shifted_layout.height_read_offset,
            ),
            Some(0)
        );
    }

    #[test]
    fn module_static_row_repair_uses_resource_bearing_for_unsafe_direction() {
        let placeable = ModuleAreaPlaceable {
            tag: "bearing_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_2,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: false,
            lockable: true,
            locked: true,
        };
        let info = module_info_with_placeables(vec![placeable.clone()]);
        let (mut payload, fragment_offset, scan) =
            static_placeable_source_row_payload(82, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("synthetic source row should have an exact legacy cursor proof");
        assert!(
            !area_static_placeable_ee_row_shape_valid(
                &payload,
                fragment_offset,
                proof.static_rows_read_offset,
            ),
            "zero direction is a valid Diamond-shaped row but not an EE-safe static yaw"
        );

        let repairs = repair_module_resource_static_placeable_rows(
            &mut payload,
            fragment_offset,
            &scan,
            &info,
        )
        .expect("unique module-backed row should be repairable");
        assert_eq!(repairs, 1);

        let repaired_proof =
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
                .expect("module repair must preserve the exact source cursor proof");
        assert_eq!(
            repaired_proof.static_rows_read_offset,
            proof.static_rows_read_offset
        );
        assert_eq!(repaired_proof.static_rows_count, proof.static_rows_count);
        assert_eq!(
            repaired_proof.zero_static_placeable_rows,
            proof.zero_static_placeable_rows
        );
        assert!(
            area_static_placeable_ee_row_shape_valid(
                &payload,
                fragment_offset,
                proof.static_rows_read_offset,
            ),
            "resource-backed bearing should make the row acceptable to the EE static reader"
        );

        let (expected_x, expected_y, expected_z) =
            static_placeable_direction_from_bearing(placeable.bearing)
                .expect("finite module bearing should produce a static direction");
        assert_eq!(
            read_area_f32(
                &payload,
                fragment_offset,
                proof.static_rows_read_offset + 18
            ),
            Some(expected_x)
        );
        assert_eq!(
            read_area_f32(
                &payload,
                fragment_offset,
                proof.static_rows_read_offset + 22
            ),
            Some(expected_y)
        );
        assert_eq!(
            read_area_f32(
                &payload,
                fragment_offset,
                proof.static_rows_read_offset + 26
            ),
            Some(expected_z)
        );
    }

    #[test]
    fn module_static_row_repair_ignores_nonfinite_bearing_without_partial_write() {
        let placeables = vec![
            ModuleAreaPlaceable {
                tag: "valid_bearing_chest".to_string(),
                appearance: 82,
                x: 10.0,
                y: 20.0,
                z: 0.0,
                bearing: std::f32::consts::FRAC_PI_2,
                static_object: true,
                useable: true,
                trap_flag: false,
                trap_disarmable: false,
                lockable: true,
                locked: true,
            },
            ModuleAreaPlaceable {
                tag: "invalid_bearing_chest".to_string(),
                appearance: 83,
                x: 30.0,
                y: 40.0,
                z: 0.0,
                bearing: f32::NAN,
                static_object: true,
                useable: true,
                trap_flag: false,
                trap_disarmable: false,
                lockable: true,
                locked: true,
            },
        ];
        let info = module_info_with_placeables(placeables);
        let (mut payload, fragment_offset, scan) = static_placeable_source_rows_payload(&[
            (82, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0),
            (83, 30.0, 40.0, 0.0, 0.0, 0.0, 0.0),
        ]);
        legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("synthetic multi-row source should have an exact legacy cursor proof");
        let original = payload.clone();

        assert_eq!(
            repair_module_resource_static_placeable_rows(
                &mut payload,
                fragment_offset,
                &scan,
                &info,
            ),
            Some(0),
            "a module row without finite bearing is not resource proof, but it must not poison the packet cursor proof"
        );
        assert_eq!(
            payload, original,
            "unsafe module static-row repair candidates must not partially rewrite earlier rows"
        );
    }

    #[test]
    fn module_static_row_repair_rejects_ambiguous_resource_match() {
        let placeable = ModuleAreaPlaceable {
            tag: "ambiguous_chest_a".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_2,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: false,
            lockable: true,
            locked: false,
        };
        let mut duplicate = placeable.clone();
        duplicate.tag = "ambiguous_chest_b".to_string();
        duplicate.locked = true;
        let info = module_info_with_placeables(vec![placeable, duplicate]);
        let (mut payload, fragment_offset, scan) = static_placeable_source_rows_payload(&[
            (82, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0),
            (82, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0),
        ]);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("two-row source packet should have an exact legacy cursor proof");
        assert_eq!(proof.static_rows_count, 2);
        let original = payload.clone();

        let repairs = repair_module_resource_static_placeable_rows(
            &mut payload,
            fragment_offset,
            &scan,
            &info,
        )
        .expect("ambiguous rows are rejected without invalidating the packet proof");
        assert_eq!(repairs, 0);
        assert_eq!(
            payload, original,
            "ambiguous module matches must not rewrite appearance, position, or direction"
        );
    }

    #[test]
    fn named_static_resource_candidate_requires_unique_row_identity_not_count() {
        let (payload, fragment_offset, mut scan) =
            static_placeable_source_row_payload(82, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0);
        scan.width = 1;
        scan.packet_height = 1;
        scan.inferred_height = 1;
        scan.tile_count = 1;
        legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("synthetic named static row should have exact source cursor proof");

        let weak_count_only_info = module_info_with_placeables(vec![ModuleAreaPlaceable {
            tag: "wrong_count_only_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 99.0,
            z: 88.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: true,
            trap_disarmable: true,
            lockable: true,
            locked: true,
        }]);
        assert!(
            !module_named_static_placeable_packet_matches_resource(
                &payload,
                fragment_offset,
                &scan,
                "ttr01",
                &weak_count_only_info,
            ),
            "same area resref/tileset/tile grid/static count must not select a module resource without the static-row identity proof"
        );

        let two_coordinate_info = module_info_with_placeables(vec![ModuleAreaPlaceable {
            tag: "two_coordinate_named_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 88.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: false,
            lockable: true,
            locked: false,
        }]);
        assert!(
            module_named_static_placeable_packet_matches_resource(
                &payload,
                fragment_offset,
                &scan,
                "ttr01",
                &two_coordinate_info,
            ),
            "appearance plus two placement coordinates may still select the unique module row for staged repair"
        );
    }

    #[test]
    fn named_static_resource_candidate_rejects_unsafe_static_rows() {
        let (payload, fragment_offset, mut scan) =
            static_placeable_source_row_payload(82, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0);
        scan.width = 1;
        scan.packet_height = 1;
        scan.inferred_height = 1;
        scan.tile_count = 1;
        legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("synthetic named static row should have exact source cursor proof");

        let unsafe_info = module_info_with_placeables(vec![ModuleAreaPlaceable {
            tag: "unsafe_two_coordinate_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: MAX_STATIC_PLACEABLE_COMPONENT_ABS + 1.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: true,
            trap_disarmable: true,
            lockable: true,
            locked: true,
        }]);
        assert!(
            !module_named_static_placeable_packet_matches_resource(
                &payload,
                fragment_offset,
                &scan,
                "ttr01",
                &unsafe_info,
            ),
            "appearance plus two coordinates must not select a named module resource when the GIT row is outside the static-row value domain"
        );
    }

    #[test]
    fn module_static_row_repair_claim_records_source_identity_before_mutation() {
        let placeable = ModuleAreaPlaceable {
            tag: "claim_backed_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: false,
            lockable: true,
            locked: false,
        };
        let info = module_info_with_placeables(vec![placeable.clone()]);
        let (payload, fragment_offset, scan) =
            static_placeable_source_row_payload(82, 10.0, 20.0, 88.0, 0.0, 0.0, 0.0);
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("synthetic static row should have exact source cursor proof");
        let static_placeables = info
            .placeables
            .iter()
            .filter(|placeable| {
                placeable.static_object && module_static_placeable_resource_row_safe(placeable)
            })
            .collect::<Vec<_>>();
        let claims = unique_module_static_placeable_row_matches(
            &payload,
            fragment_offset,
            &proof,
            &static_placeables,
        )
        .expect("two-coordinate module row should produce one immutable claim");
        assert_eq!(claims.len(), 1);
        let claim = claims[0];
        assert_eq!(
            claim.match_kind,
            ModuleStaticPlaceableRowMatchKind::ExactAppearanceAtLeastTwoCoordinates
        );
        assert_eq!(claim.placeable_index, 0);
        assert_eq!(claim.object_id, 0x8000_0042);
        assert_eq!(claim.appearance, 82);
        assert_eq!(claim.x, 10.0);
        assert_eq!(claim.y, 20.0);
        assert_eq!(claim.z, 88.0);
        assert_eq!(
            module_static_placeable_row_claim_matches_payload(
                &payload,
                fragment_offset,
                &claim,
                static_placeables[0],
            ),
            Some(true)
        );

        let mut shifted_payload = payload.clone();
        write_area_f32(
            &mut shifted_payload,
            fragment_offset,
            claim.cursor + 14,
            77.0,
        )
        .expect("test should be able to alter the source z coordinate");
        assert_eq!(
            module_static_placeable_row_claim_matches_payload(
                &shifted_payload,
                fragment_offset,
                &claim,
                static_placeables[0],
            ),
            Some(false),
            "a pre-mutation row claim must not authorize a later shifted source row"
        );

        let mut reassigned_payload = payload.clone();
        write_area_u32(
            &mut reassigned_payload,
            fragment_offset,
            claim.cursor,
            0x8000_0043,
        )
        .expect("test should be able to alter the source object id");
        assert_eq!(
            module_static_placeable_row_claim_matches_payload(
                &reassigned_payload,
                fragment_offset,
                &claim,
                static_placeables[0],
            ),
            Some(false),
            "a row claim must cover the packet object id even though module matching ignores it"
        );
    }

    #[test]
    fn module_static_row_repair_requires_appearance_plus_two_coordinates() {
        let placeable = ModuleAreaPlaceable {
            tag: "two_coordinate_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: false,
            lockable: true,
            locked: false,
        };
        let info = module_info_with_placeables(vec![placeable.clone()]);

        let (mut one_coordinate_payload, one_fragment_offset, one_scan) =
            static_placeable_source_row_payload(82, 10.0, 99.0, 88.0, 0.0, 0.0, 0.0);
        legacy_area_source_tail_exact_read_proof(
            &one_coordinate_payload,
            one_fragment_offset,
            &one_scan,
        )
        .expect("one-coordinate row still has a decompiled source cursor proof");
        let one_original = one_coordinate_payload.clone();
        let one_repairs = repair_module_resource_static_placeable_rows(
            &mut one_coordinate_payload,
            one_fragment_offset,
            &one_scan,
            &info,
        )
        .expect("non-matching row should leave the packet proof intact");
        assert_eq!(
            one_repairs, 0,
            "appearance plus only one coordinate must not authorize a module-backed rewrite"
        );
        assert_eq!(
            one_coordinate_payload, one_original,
            "weak module matches must not rewrite position or direction bytes"
        );

        let (mut two_coordinate_payload, two_fragment_offset, two_scan) =
            static_placeable_source_row_payload(82, 10.0, 20.0, 88.0, 0.0, 0.0, 0.0);
        let two_proof = legacy_area_source_tail_exact_read_proof(
            &two_coordinate_payload,
            two_fragment_offset,
            &two_scan,
        )
        .expect("two-coordinate row should have a decompiled source cursor proof");
        let two_repairs = repair_module_resource_static_placeable_rows(
            &mut two_coordinate_payload,
            two_fragment_offset,
            &two_scan,
            &info,
        )
        .expect("unique two-coordinate row should be repairable from GIT state");
        assert_eq!(two_repairs, 1);

        let repaired_proof = legacy_area_source_tail_exact_read_proof(
            &two_coordinate_payload,
            two_fragment_offset,
            &two_scan,
        )
        .expect("module repair must preserve the exact source cursor proof");
        assert_eq!(
            repaired_proof.static_rows_read_offset,
            two_proof.static_rows_read_offset
        );
        assert_eq!(
            repaired_proof.static_rows_count,
            two_proof.static_rows_count
        );
        assert_eq!(
            read_area_f32(
                &two_coordinate_payload,
                two_fragment_offset,
                two_proof.static_rows_read_offset + 14
            ),
            Some(placeable.z),
            "the unmatched third coordinate is repaired only after the two-coordinate proof"
        );
        let (expected_x, expected_y, expected_z) =
            static_placeable_direction_from_bearing(placeable.bearing)
                .expect("finite module bearing should produce a static direction");
        assert_eq!(
            read_area_f32(
                &two_coordinate_payload,
                two_fragment_offset,
                two_proof.static_rows_read_offset + 18
            ),
            Some(expected_x)
        );
        assert_eq!(
            read_area_f32(
                &two_coordinate_payload,
                two_fragment_offset,
                two_proof.static_rows_read_offset + 22
            ),
            Some(expected_y)
        );
        assert_eq!(
            read_area_f32(
                &two_coordinate_payload,
                two_fragment_offset,
                two_proof.static_rows_read_offset + 26
            ),
            Some(expected_z)
        );
    }

    #[test]
    fn module_static_row_repair_allows_zero_appearance_only_with_all_coordinates() {
        let placeable = ModuleAreaPlaceable {
            tag: "zero_appearance_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: false,
            lockable: true,
            locked: false,
        };
        let info = module_info_with_placeables(vec![placeable.clone()]);

        let (mut full_match_payload, full_match_fragment_offset, full_match_scan) =
            static_placeable_source_row_payload(0, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0);
        let full_match_proof = legacy_area_source_tail_exact_read_proof(
            &full_match_payload,
            full_match_fragment_offset,
            &full_match_scan,
        )
        .expect("zero-appearance source row should still have exact static cursor proof");
        let repairs = repair_module_resource_static_placeable_rows(
            &mut full_match_payload,
            full_match_fragment_offset,
            &full_match_scan,
            &info,
        )
        .expect("all-coordinate zero-appearance row should be repairable from GIT state");
        assert_eq!(repairs, 1);
        let repaired_proof = legacy_area_source_tail_exact_read_proof(
            &full_match_payload,
            full_match_fragment_offset,
            &full_match_scan,
        )
        .expect("zero-appearance repair must preserve the exact static cursor proof");
        assert_eq!(
            repaired_proof.static_rows_read_offset,
            full_match_proof.static_rows_read_offset
        );
        assert_eq!(
            repaired_proof.static_rows_count,
            full_match_proof.static_rows_count
        );
        assert_eq!(
            read_area_u16(
                &full_match_payload,
                full_match_fragment_offset,
                full_match_proof.static_rows_read_offset + 4,
            ),
            Some(placeable.appearance),
            "a zero appearance WORD may be filled only after all placement coordinates match"
        );
        assert!(
            area_static_placeable_ee_row_shape_valid(
                &full_match_payload,
                full_match_fragment_offset,
                full_match_proof.static_rows_read_offset,
            ),
            "module bearing should make the repaired zero-appearance row EE-safe"
        );

        let (mut partial_match_payload, partial_match_fragment_offset, partial_match_scan) =
            static_placeable_source_row_payload(0, 10.0, 20.0, 99.0, 0.0, 0.0, 0.0);
        legacy_area_source_tail_exact_read_proof(
            &partial_match_payload,
            partial_match_fragment_offset,
            &partial_match_scan,
        )
        .expect("partial zero-appearance row still has exact static cursor proof");
        let partial_original = partial_match_payload.clone();
        let repairs = repair_module_resource_static_placeable_rows(
            &mut partial_match_payload,
            partial_match_fragment_offset,
            &partial_match_scan,
            &info,
        )
        .expect("unmatched zero-appearance row should leave the packet proof intact");
        assert_eq!(
            repairs, 0,
            "zero appearance does not relax the placement proof to two coordinates"
        );
        assert_eq!(
            partial_match_payload, partial_original,
            "weak zero-appearance matches must not rewrite appearance, position, or bearing"
        );

        let (mut wrong_appearance_payload, wrong_appearance_fragment_offset, wrong_appearance_scan) =
            static_placeable_source_row_payload(83, 10.0, 20.0, 0.0, 0.0, 0.0, 0.0);
        legacy_area_source_tail_exact_read_proof(
            &wrong_appearance_payload,
            wrong_appearance_fragment_offset,
            &wrong_appearance_scan,
        )
        .expect("wrong-appearance row still has exact static cursor proof");
        let wrong_original = wrong_appearance_payload.clone();
        let repairs = repair_module_resource_static_placeable_rows(
            &mut wrong_appearance_payload,
            wrong_appearance_fragment_offset,
            &wrong_appearance_scan,
            &info,
        )
        .expect("non-matching appearance should leave the packet proof intact");
        assert_eq!(
            repairs, 0,
            "only a literal zero appearance WORD is treated as a missing value"
        );
        assert_eq!(
            wrong_appearance_payload, wrong_original,
            "nonzero appearance mismatches must not be repaired from coordinates alone"
        );
    }

    #[test]
    fn malformed_module_static_geometry_is_not_resource_proof() {
        let placeable = ModuleAreaPlaceable {
            tag: "bad_static_geometry_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: MAX_STATIC_PLACEABLE_COMPONENT_ABS + 1.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: true,
            trap_disarmable: true,
            lockable: true,
            locked: true,
        };
        let expected_direction = static_placeable_direction_from_bearing(placeable.bearing)
            .expect("finite GIT bearing should produce a row direction");
        let info = module_info_with_placeables(vec![placeable]);

        let (mut payload, fragment_offset, scan) =
            static_placeable_source_row_payload(82, 10.0, 20.0, 0.0, 0.0, 1.0, 0.0);
        legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("packet row should still have exact decompiled static cursor proof");
        let original = payload.clone();

        let repairs = repair_module_resource_static_placeable_rows(
            &mut payload,
            fragment_offset,
            &scan,
            &info,
        )
        .expect("malformed resource row should be ignored without poisoning packet proof");
        assert_eq!(
            repairs, 0,
            "a GIT row outside the static-row value domain must not authorize repair"
        );
        assert_eq!(
            payload, original,
            "bad resource geometry must not rewrite appearance, position, or direction bytes"
        );
        assert_eq!(
            module_static_placeable_context_state(
                Some(&info),
                82,
                10.0,
                20.0,
                0.0,
                expected_direction.0,
                expected_direction.1,
                expected_direction.2,
            ),
            None,
            "unsafe resource geometry must not seed later trap/use/lock state context"
        );
    }

    #[test]
    fn module_context_state_requires_matching_static_direction_triplet() {
        let placeable = ModuleAreaPlaceable {
            tag: "locked_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_2,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: false,
            lockable: true,
            locked: true,
        };
        let expected_direction = static_placeable_direction_from_bearing(placeable.bearing)
            .expect("finite GIT bearing should produce a row direction");
        let info = module_info_with_placeables(vec![placeable]);

        let state = module_static_placeable_context_state(
            Some(&info),
            82,
            10.0,
            99.0,
            0.0,
            expected_direction.0,
            expected_direction.1,
            expected_direction.2,
        )
        .expect("appearance plus two coordinates plus bearing-derived direction proves state");
        assert_eq!(
            state,
            AreaPlaceableContextState {
                static_object: true,
                useable: true,
                trap_flag: false,
                trap_disarmable: false,
                lockable: true,
                locked: true,
            }
        );

        assert_eq!(
            module_static_placeable_context_state(Some(&info), 82, 10.0, 99.0, 0.0, 0.0, 1.0, 0.0),
            None,
            "a plausible appearance/position row must not inherit GIT state when the second static triplet proves a different yaw"
        );
    }

    #[test]
    fn module_context_state_allows_zero_appearance_only_with_full_row_identity() {
        let placeable = ModuleAreaPlaceable {
            tag: "zero_appearance_locked_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: true,
            trap_disarmable: true,
            lockable: true,
            locked: true,
        };
        let expected_direction = static_placeable_direction_from_bearing(placeable.bearing)
            .expect("finite GIT bearing should produce a row direction");
        let info = module_info_with_placeables(vec![placeable]);

        assert_eq!(
            module_static_placeable_context_state(
                Some(&info),
                0,
                10.0,
                20.0,
                0.0,
                expected_direction.0,
                expected_direction.1,
                expected_direction.2,
            ),
            Some(AreaPlaceableContextState {
                static_object: true,
                useable: true,
                trap_flag: true,
                trap_disarmable: true,
                lockable: true,
                locked: true,
            }),
            "a zero appearance WORD is treated as missing only after all placement coordinates and the direction triplet prove the static GIT row"
        );

        assert_eq!(
            module_static_placeable_context_state(
                Some(&info),
                0,
                10.0,
                20.0,
                99.0,
                expected_direction.0,
                expected_direction.1,
                expected_direction.2,
            ),
            None,
            "zero appearance does not relax module-state context to a two-coordinate match"
        );

        assert_eq!(
            module_static_placeable_context_state(
                Some(&info),
                83,
                10.0,
                20.0,
                0.0,
                expected_direction.0,
                expected_direction.1,
                expected_direction.2,
            ),
            None,
            "only literal zero appearance is treated as missing; nonzero mismatches must not inherit GIT state"
        );
    }

    #[test]
    fn module_context_state_requires_unique_static_resource_row() {
        let placeable = ModuleAreaPlaceable {
            tag: "locked_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_4,
            static_object: true,
            useable: true,
            trap_flag: false,
            trap_disarmable: true,
            lockable: true,
            locked: false,
        };
        let expected_direction = static_placeable_direction_from_bearing(placeable.bearing)
            .expect("finite GIT bearing should produce a row direction");

        let mut duplicate = placeable.clone();
        duplicate.tag = "duplicate_locked_chest".to_string();
        duplicate.locked = true;
        let duplicate_info = module_info_with_placeables(vec![placeable.clone(), duplicate]);
        assert_eq!(
            module_static_placeable_context_state(
                Some(&duplicate_info),
                82,
                10.0,
                20.0,
                0.0,
                expected_direction.0,
                expected_direction.1,
                expected_direction.2,
            ),
            None,
            "matching appearance/position/direction is not enough when two static GIT rows can own different state"
        );

        let mut non_static = placeable;
        non_static.static_object = false;
        let non_static_info = module_info_with_placeables(vec![non_static]);
        assert_eq!(
            module_static_placeable_context_state(
                Some(&non_static_info),
                82,
                10.0,
                20.0,
                0.0,
                expected_direction.0,
                expected_direction.1,
                expected_direction.2,
            ),
            None,
            "live/non-static module placeables must not seed static-row trap/use/lock context"
        );
    }

    #[test]
    fn placeable_context_module_state_requires_list_level_static_claims() {
        let placeable = ModuleAreaPlaceable {
            tag: "single_locked_static_chest".to_string(),
            appearance: 82,
            x: 10.0,
            y: 20.0,
            z: 0.0,
            bearing: std::f32::consts::FRAC_PI_2,
            static_object: true,
            useable: true,
            trap_flag: true,
            trap_disarmable: true,
            lockable: true,
            locked: true,
        };
        let expected_direction = static_placeable_direction_from_bearing(placeable.bearing)
            .expect("finite GIT bearing should produce a row direction");
        let info = module_info_with_placeables(vec![placeable]);
        let (valid_payload, valid_fragment_offset, _valid_scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                1,
                &[(
                    82,
                    10.0,
                    20.0,
                    0.0,
                    expected_direction.0,
                    expected_direction.1,
                    expected_direction.2,
                )],
            );
        let valid_context = collect_area_post_tile_placeable_context(
            &valid_payload,
            valid_fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            Some(&info),
        )
        .expect("exact one-row static list should be exposed as area context");
        assert_eq!(valid_context.static_rows.len(), 1);
        assert_eq!(
            valid_context.static_rows[0].module_state,
            Some(AreaPlaceableContextState {
                static_object: true,
                useable: true,
                trap_flag: true,
                trap_disarmable: true,
                lockable: true,
                locked: true,
            }),
            "one-to-one static-list proof should export module trap/use/lock state"
        );

        let alias_context = collect_area_post_tile_placeable_context(
            &valid_payload,
            valid_fragment_offset,
            "testarea",
            0x8000_0042,
            false,
            Some(&info),
        )
        .expect("area-object alias row should still be exposed as exact wire context");
        assert_eq!(alias_context.static_rows.len(), 1);
        assert_eq!(
            alias_context.static_rows[0].object_id_confidence,
            AreaPlaceableContextObjectIdConfidence::AreaObjectAlias
        );
        assert_eq!(
            alias_context.static_rows[0].module_state, None,
            "area-object aliases are diagnostic context and must not seed live-object state mismatches"
        );

        let (payload, fragment_offset, scan) =
            real_area_static_placeable_source_rows_payload_with_count(
                2,
                &[
                    (
                        82,
                        10.0,
                        20.0,
                        0.0,
                        expected_direction.0,
                        expected_direction.1,
                        expected_direction.2,
                    ),
                    (
                        82,
                        10.0,
                        20.0,
                        0.0,
                        expected_direction.0,
                        expected_direction.1,
                        expected_direction.2,
                    ),
                ],
            );
        let proof = legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan)
            .expect("duplicate static rows should still have exact decompiled cursor proof");
        assert_eq!(proof.static_rows_count, 2);
        assert_eq!(
            module_static_placeable_context_state(
                Some(&info),
                82,
                10.0,
                20.0,
                0.0,
                expected_direction.0,
                expected_direction.1,
                expected_direction.2,
            ),
            Some(AreaPlaceableContextState {
                static_object: true,
                useable: true,
                trap_flag: true,
                trap_disarmable: true,
                lockable: true,
                locked: true,
            }),
            "the row-local helper remains narrower than the production list-level handoff"
        );

        let context = collect_area_post_tile_placeable_context(
            &payload,
            fragment_offset,
            "testarea",
            0x8000_0001,
            false,
            Some(&info),
        )
        .expect("exact static rows should still be exposed as area context rows");
        assert_eq!(context.static_rows.len(), 2);
        assert!(
            context
                .static_rows
                .iter()
                .all(|row| row.module_state.is_none()),
            "module trap/use/lock state requires one-to-one claims for the whole static list"
        );
    }
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

    struct EnvVarTestGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarTestGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests that mutate module-resource environment are
            // serialized by MODULE_CONTEXT_TEST_LOCK, and no other test in
            // this crate mutates NWN_BRIDGE_MODULE_PATH.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarTestGuard {
        fn drop(&mut self) {
            // SAFETY: see EnvVarTestGuard::set; this guard restores the same
            // serialized test-only environment variable.
            unsafe {
                if let Some(previous) = &self.previous {
                    std::env::set_var(self.key, previous);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    fn float_close(actual: f32, expected: f32, tolerance: f32) -> bool {
        (actual - expected).abs() <= tolerance
    }

    fn assert_float_close(label: &str, actual: f32, expected: f32, tolerance: f32) {
        assert!(
            float_close(actual, expected, tolerance),
            "{label}: actual={actual:?} expected={expected:?} tolerance={tolerance:?}"
        );
    }

    fn angle_delta(actual: f32, expected: f32) -> f32 {
        let two_pi = std::f32::consts::PI * 2.0;
        (actual - expected + std::f32::consts::PI).rem_euclid(two_pi) - std::f32::consts::PI
    }

    fn assert_angle_close(label: &str, actual: f32, expected: f32, tolerance: f32) {
        let delta = angle_delta(actual, expected);
        assert!(
            delta.abs() <= tolerance,
            "{label}: actual={actual:?} expected={expected:?} delta={delta:?} tolerance={tolerance:?}"
        );
    }

    fn static_row_bearing(row: &AreaPlaceableContextRow) -> f32 {
        (-row.dir_x).atan2(row.dir_y)
    }

    fn assert_static_area_rows_match_module_placeables(
        rows: &[AreaPlaceableContextRow],
        placeables: &[ModuleAreaPlaceable],
    ) {
        let static_placeables = placeables
            .iter()
            .filter(|placeable| placeable.static_object)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), static_placeables.len());
        let mut remaining = (0..static_placeables.len()).collect::<Vec<_>>();
        for (index, row) in rows.iter().enumerate() {
            assert!(
                row.has_direction,
                "static placeable row {index} should carry direction"
            );
            let matching = remaining
                .iter()
                .copied()
                .filter(|candidate_index| {
                    let placeable = static_placeables[*candidate_index];
                    row.appearance == placeable.appearance
                        && float_close(row.x, placeable.x, 0.01)
                        && float_close(row.y, placeable.y, 0.01)
                        && float_close(row.z, placeable.z, 0.01)
                        && float_close(row.dir_z, 0.0, 0.01)
                        && angle_delta(static_row_bearing(row), placeable.bearing).abs() <= 0.01
                })
                .collect::<Vec<_>>();
            assert_eq!(
                matching.len(),
                1,
                "static placeable row {index} should match one GIT static placeable: appearance={} x={:?} y={:?} z={:?} bearing={:?}",
                row.appearance,
                row.x,
                row.y,
                row.z,
                static_row_bearing(row)
            );
            let placeable_index = matching[0];
            let placeable = static_placeables[placeable_index];
            assert_eq!(
                row.appearance, placeable.appearance,
                "static placeable row {index} tag={}",
                placeable.tag
            );
            assert_float_close("static placeable x", row.x, placeable.x, 0.01);
            assert_float_close("static placeable y", row.y, placeable.y, 0.01);
            assert_float_close("static placeable z", row.z, placeable.z, 0.01);
            assert_float_close("static placeable dir_z", row.dir_z, 0.0, 0.01);
            assert_angle_close(
                "static placeable bearing",
                static_row_bearing(row),
                placeable.bearing,
                0.01,
            );
            assert!(
                !placeable.trap_flag,
                "static area placeable row {index} unexpectedly comes from a trapped GIT placeable"
            );
            let state = row
                .module_state
                .expect("module-backed static placeable row should retain its GIT state context");
            assert_eq!(state.static_object, placeable.static_object);
            assert_eq!(state.useable, placeable.useable);
            assert_eq!(state.trap_flag, placeable.trap_flag);
            assert_eq!(state.trap_disarmable, placeable.trap_disarmable);
            assert_eq!(state.lockable, placeable.lockable);
            assert_eq!(state.locked, placeable.locked);
            remaining.retain(|candidate_index| *candidate_index != placeable_index);
        }
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
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_DARK_RANGER_INN_AREA_VARIANT: &[u8] =
        include_bytes!("../../fixtures/area/local_dark_ranger_inn_area_variant_20260521.bin");
    const LOCAL_WINDS_EREMOR_MOUNT_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_winds_eremor_mount_client_area_compact.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CEPV22_WELCOME_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_cepv22_welcome_client_area_compact.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CEPV23_WELCOME_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_cepv23_area_client_area_area_20260520.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CEPV23_SKIES_DECLARED_ZERO: &[u8] =
        include_bytes!("../../fixtures/area/local_cepv23_skies_area_declared_zero_20260520.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CONTEST_CHAMPIONS_ITEMS_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_contest_champions_items_area_compact.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_PRELUDE_M0Q1A_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_prelude_m0q1a_client_area_20260522.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CHAPTER1_MAP_M1Q1_DECLARED_ZERO: &[u8] = include_bytes!(
        "../../fixtures/area/local_chapter1_map_m1q1_declared_zero_area_20260522.bin"
    );
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CHAPTER1E_MAP_M1Q6A_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_chapter1e_map_m1q6a_area_20260523.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CHAPTER2E_M2Q4A_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_chapter2e_m2q4a_client_area_20260522.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CHAPTER2E_M2Q4A_NO_CONTEXT: &[u8] =
        include_bytes!("../../fixtures/area/local_chapter2e_m2q4a_area_no_context_20260523.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CHAPTER2_A08_BARRACKS_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_chapter2_a08_barracks_area_20260523.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CHAPTER3_M3Q1A10_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_chapter3_m3q1a10_area_20260523.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_CHAPTER4_MAP_M1Q6A_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_chapter4_map_m1q6a_area_20260523.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_XP2_CHAPTER3_GATESOFCANIA_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_xp2_chapter3_gatesofcania_area_20260523.bin");
    #[cfg(hgbridge_private_fixtures)]
    const LOCAL_XP1_Q1A2DROGONFLOOR2_COMPACT: &[u8] =
        include_bytes!("../../fixtures/area/local_xp1_q1a2drogonfloor2_area_20260522.bin");

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
    fn missing_height_repair_requires_exact_post_tile_fragment_cursor() {
        let (mut payload, fragment_offset) = legacy_area_payload_with_extra_fragment_bits(
            DOCKS_OF_ASCENSION_LEGACY_MISSING_HEIGHT,
            1,
        );
        let mut scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(scan.valid);
        assert_eq!(scan.width, 11);
        assert_eq!(scan.packet_height, 0);
        assert_eq!(scan.inferred_height, 14);
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &scan).is_none(),
            "extra post-tile fragment bits must block the legacy source cursor proof"
        );
        let layout = area_static_layout(&payload, fragment_offset).expect("test area layout");
        let original = payload.clone();

        assert!(
            !repair_missing_area_height(&mut payload, fragment_offset, &mut scan),
            "height repair must not run unless the inferred dimensions also prove the exact post-tile cursor"
        );
        assert_eq!(
            payload, original,
            "failed height repair must leave the dimension bytes and fragment stream untouched"
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.height_read_offset),
            Some(0)
        );
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
        let _context_guard = module_context_test_guard();
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", "__hgbridge_no_such_module__.mod");
        let empty_context = crate::translate::module::ObservedModuleContext {
            localized_name: String::new(),
            module_resref: String::new(),
            areas: Vec::new(),
        };
        let mut payload = DOCKS_OF_ASCENSION_LEGACY_ZERO_SOUND_COUNTS.to_vec();
        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&empty_context),
        )
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
        assert!(
            summary
                .placeable_context
                .static_rows
                .iter()
                .all(|row| row.module_state.is_none()),
            "HG Docks has no proven local module resource in this fixture; static-row context must not invent GIT trap/use/lock state"
        );
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
    fn missing_width_repair_requires_exact_post_tile_fragment_cursor() {
        let (mut payload, fragment_offset) =
            legacy_area_payload_with_extra_fragment_bits(VOYAGE_LEGACY_MISSING_WIDTH, 1);
        let layout = area_static_layout(&payload, fragment_offset).expect("test area layout");
        let mut scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(!scan.valid);
        let legacy_scan =
            scan_area_tile_stream_allow_legacy_missing_width(&payload, fragment_offset);
        assert!(legacy_scan.valid);
        assert_eq!(legacy_scan.width, 4);
        assert_eq!(legacy_scan.packet_height, 5);
        assert!(
            legacy_area_source_tail_exact_read_proof(&payload, fragment_offset, &legacy_scan)
                .is_none(),
            "extra post-tile fragment bits must block the legacy source cursor proof"
        );
        let original = payload.clone();

        assert!(
            !repair_missing_area_width(&mut payload, fragment_offset, &mut scan),
            "width repair must not run unless the inferred dimensions also prove the exact post-tile cursor"
        );
        assert_eq!(
            payload, original,
            "failed width repair must leave the dimension bytes and fragment stream untouched"
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.width_read_offset),
            Some(0)
        );
    }

    #[test]
    fn missing_square_dimension_repair_requires_exact_post_tile_fragment_cursor() {
        let (mut payload, fragment_offset) =
            legacy_area_payload_with_extra_fragment_bits(LOCAL_DIAMOND_BW167DEMO_FIXED_NAME, 1);
        let layout = area_static_layout(&payload, fragment_offset).expect("test area layout");
        assert_eq!(layout.dialect, AreaStaticDialect::Legacy169);
        assert_eq!(layout.area_name_encoding, AreaNameEncoding::DiamondFixed20);
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.width_read_offset),
            Some(0)
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.height_read_offset),
            Some(0)
        );

        let mut scan = scan_area_tile_stream(&payload, fragment_offset);
        assert!(!scan.valid);
        let original = payload.clone();

        assert!(
            !repair_missing_square_area_dimensions(&mut payload, fragment_offset, &mut scan),
            "square-dimension repair must not commit inferred dimensions when a post-tile fragment bit remains unowned"
        );
        assert_eq!(
            payload, original,
            "failed square-dimension repair must leave the dimension bytes and fragment stream untouched"
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.width_read_offset),
            Some(0)
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.height_read_offset),
            Some(0)
        );
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

        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            None,
        )
        .expect("observed module context maps compact area packet to local ARE resource");
        assert_eq!(compact_info.resref, "cormanthor");
        assert_eq!(compact_info.name, "Cormanthor");
        assert_eq!(compact_info.width, 8);
        assert_eq!(compact_info.height, 9);
        assert_eq!(compact_info.tileset, "ttf01");
        assert_eq!(compact_info.map_notes.len(), 2);
        assert_eq!(compact_info.sounds.len(), 8);
        assert_eq!(compact_info.placeables.len(), 8);
        assert_eq!(
            compact_info
                .placeables
                .iter()
                .filter(|placeable| placeable.static_object)
                .count(),
            6
        );
        assert!(
            compact_info
                .placeables
                .iter()
                .all(|placeable| !placeable.trap_flag)
        );

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
        let staged_named_static_info = module_area_resource_info_for_named_static_placeables(
            &staged_source,
            staged_fragment_offset,
            &staged_scan,
            "cormanthor",
            None,
        )
        .expect("named static placeable rows should resolve to the local GIT resource");
        assert_eq!(staged_named_static_info.resref, "cormanthor");
        assert_eq!(
            staged_named_static_info
                .placeables
                .iter()
                .filter(|placeable| placeable.static_object)
                .count(),
            usize::from(staged_proof.static_rows_count)
        );

        let mut repaired_source = payload.clone();
        repair_compact_area_from_module_resource(
            &mut repaired_source,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            false,
            false,
            None,
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
        assert!(
            summary.module_resource_static_placeable_repairs > 0,
            "module GIT evidence should repair at least one semantically shifted static placeable row"
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondModuleResourceStaticPlaceableRepair)
        );
        assert_eq!(proof.transition_count, 2);
        assert_eq!(proof.sound_count, 8);
        assert_eq!(proof.light_count, 0);
        assert_eq!(proof.static_count, 6);
        assert_static_area_rows_match_module_placeables(
            &summary.placeable_context.static_rows,
            &compact_info.placeables,
        );
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

        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            None,
        )
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_dark_ranger_area_variant_uses_resource_tiles_when_packet_tiles_differ() {
        let _context_guard = module_context_test_guard();
        let observed_context = crate::translate::module::ObservedModuleContext {
            localized_name: "Dark R".to_string(),
            module_resref: "Dark_R".to_string(),
            areas: vec![
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_0000,
                    name: "Inn L".to_string(),
                },
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_0000,
                    name: "Dead 's Marsh".to_string(),
                },
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_0000,
                    name: "T Dark R 's Ruins".to_string(),
                },
            ],
        };

        let mut payload = LOCAL_DARK_RANGER_INN_AREA_VARIANT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let source_layout =
            area_static_layout(&payload, fragment_offset).expect("compact static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondFixed20
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("innof")
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, source_layout.width_read_offset),
            Some(5)
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, source_layout.height_read_offset),
            Some(0)
        );
        assert!(!scan_area_tile_stream(&payload, fragment_offset).valid);

        let module_path = observed_module_file_path(&observed_context)
            .expect("observed Dark Ranger local module file path");
        let duplicate_path = std::env::temp_dir().join(format!(
            "hgbridge-dark-ranger-module-copy-{}.mod",
            std::process::id()
        ));
        std::fs::copy(&module_path, &duplicate_path)
            .expect("copy matching module table for duplicate fallback proof");
        let resolved_duplicate = observed_module_file_path_from_candidates(
            &observed_context,
            vec![duplicate_path.clone(), module_path.clone()],
        );
        std::fs::remove_file(&duplicate_path).expect("remove duplicate module copy");
        assert_eq!(
            resolved_duplicate.as_deref(),
            Some(duplicate_path.as_path())
        );

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("compact local Dark Ranger variant area rewrite");
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

        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            None,
        )
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
            false,
            false,
            None,
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_cepv22_fragmented_cexo_area_uses_module_resource_resref() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "cepv22_".to_string(),
                module_resref: "cepv22".to_string(),
                areas: vec![
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Welcom o CEPv2.2".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "CEP 2.1 Tester".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "TNO cast exterior".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Ho Farm H e B ique".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0225,
                        name: "* * _Crafter".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0228,
                        name: "Beach".to_string(),
                    },
                ],
            },
        );

        let mut payload = LOCAL_CEPV22_WELCOME_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let source_layout =
            area_static_layout(&payload, fragment_offset).expect("fragmented CExo static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::CExoString
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("")
        );
        assert_eq!(
            diamond_cexo_string_area_name_fragments(&payload, fragment_offset)
                .expect("fragmented CExoString area-name fragments"),
            vec!["Welcom".to_string(), "o CEPv2.2".to_string()]
        );
        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            None,
        )
        .expect("observed module context maps fragmented CExo area to local ARE resource");
        assert_eq!(
            usize::try_from(source_layout.area_name_length).ok(),
            Some(compact_info.name.len())
        );

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("fragmented local CEP area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten CEP area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, compact_info.resref);
        assert_eq!(summary.tileset_resref, compact_info.tileset);
        assert_eq!(summary.width, compact_info.width);
        assert_eq!(summary.packet_height, compact_info.height);
        assert_eq!(
            summary.tile_count,
            compact_info.width.saturating_mul(compact_info.height)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondFragmentedCExoAreaName)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_cepv23_fragmented_cexo_area_repairs_plausible_resref_source() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "cep_starter_module".to_string(),
                module_resref: "cepv23_starter".to_string(),
                areas: vec![
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Welcom o CEPv2.3".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "CEP 2.3 Tester".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "TNO cast exterior".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Horse Farm H e B ique".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0225,
                        name: "* * _Crafter".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0228,
                        name: "Beach".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_023B,
                        name: "_Enc ter Area".to_string(),
                    },
                ],
            },
        );

        let mut payload = LOCAL_CEPV23_WELCOME_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let source_layout =
            area_static_layout(&payload, fragment_offset).expect("fragmented CExo static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::CExoString
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("area")
        );
        assert_eq!(
            diamond_cexo_string_area_name_fragments(&payload, fragment_offset)
                .expect("fragmented CExoString area-name fragments"),
            vec!["Welcom".to_string(), "o CEPv2.3".to_string()]
        );
        assert!(compact_cexo_area_needs_module_resource_repair(
            &payload,
            fragment_offset,
            &source_layout,
            true
        ));
        assert!(!scan_area_tile_stream(&payload, fragment_offset).valid);

        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            None,
        )
        .expect("observed module context maps fragmented CExo area to local ARE resource");
        assert_eq!(compact_info.resref, "area");
        assert_eq!(
            usize::try_from(source_layout.area_name_length).ok(),
            Some(compact_info.name.len())
        );

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("fragmented local CEPv23 area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten CEPv23 area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, compact_info.resref);
        assert_eq!(summary.tileset_resref, compact_info.tileset);
        assert_eq!(summary.width, compact_info.width);
        assert_eq!(summary.packet_height, compact_info.height);
        assert_eq!(
            summary.tile_count,
            compact_info.width.saturating_mul(compact_info.height)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondFragmentedCExoAreaName)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_cepv23_skies_declared_zero_area_repairs_from_resource_tail() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "Skyboxes".to_string(),
                module_resref: "cepv23_skies".to_string(),
                areas: vec![crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_0001,
                    name: "A".to_string(),
                }],
            },
        );

        let mut payload = LOCAL_CEPV23_SKIES_DECLARED_ZERO.to_vec();
        assert_eq!(read_u32_le(&payload, HIGH_LEVEL_HEADER_BYTES), Some(0));
        let mut staged = payload.clone();
        let inferred_declared = staged.len() - 2;
        write_u32_le(
            &mut staged,
            HIGH_LEVEL_HEADER_BYTES,
            inferred_declared as u32,
        )
        .expect("stage declared-zero read window");
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&staged).expect("staged read window");
        let source_layout =
            area_static_layout(&staged, fragment_offset).expect("staged static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondFixed16
        );
        assert_eq!(
            read_u32_le(&staged, LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET),
            Some(0x8000_0000)
        );
        assert_eq!(
            diamond_short_fixed_area_name_fragments(&staged, fragment_offset)
                .expect("short fixed area-name fragments"),
            vec![" A".to_string()]
        );
        let compact_info = module_area_resource_info_for_compact_packet(
            &staged,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            None,
        )
        .expect("single-area Module_Info alias maps to local ARE resource");
        assert_eq!(compact_info.resref, "startarea");
        assert_eq!(compact_info.name, "! Start Area");
        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("declared-zero CEPv23 skies area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten skies area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.old_declared, 0);
        assert_eq!(summary.area_resref, "startarea");
        assert_eq!(summary.tileset_resref, "tdc01");
        assert_eq!(summary.width, 3);
        assert_eq!(summary.packet_height, 3);
        assert_eq!(summary.tile_count, 9);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondDeclaredZeroReadWindow)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_contest_items_area_uses_split_resref_and_name_fragments() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "Contest Of Champ".to_string(),
                module_resref: "Contest_Of_Champ".to_string(),
                areas: vec![
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Arena 00".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "s Area".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Blue Tea HQ".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0544,
                        name: "d Tea HQ".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0A1B,
                        name: "Tea".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0xF251_4820,
                        name: "Purple Tea".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0xC951_4820,
                        name: "Arena".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0xBC31_3020,
                        name: "Arena 02".to_string(),
                    },
                ],
            },
        );

        let mut payload = LOCAL_CONTEST_CHAMPIONS_ITEMS_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("fixture read window");
        let source_layout =
            area_static_layout(&payload, fragment_offset).expect("compact static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondFixed21
        );
        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            None,
        )
        .expect("observed module context maps split item area packet to local ARE resource");
        assert_eq!(compact_info.resref, "itemrestrictions");
        assert_eq!(compact_info.name, "Restrictions Area");

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("compact local Contest item area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Contest area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, compact_info.resref);
        assert_eq!(summary.tileset_resref, compact_info.tileset);
        assert_eq!(summary.width, compact_info.width);
        assert_eq!(summary.packet_height, compact_info.height);
        assert_eq!(
            summary.tile_count,
            compact_info.width.saturating_mul(compact_info.height)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_prelude_no_name_area_uses_exact_area_resref_resource() {
        let _context_guard = module_context_test_guard();
        crate::translate::module::remember_observed_module_context_for_tests(
            crate::translate::module::ObservedModuleContext {
                localized_name: "Prelude".to_string(),
                module_resref: "Prelude".to_string(),
                areas: vec![
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Se r Barr".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Train Halls".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Gradu Chamber".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_0000,
                        name: "Academy".to_string(),
                    },
                    crate::translate::module::ObservedModuleArea {
                        object_id: 0x8000_026A,
                        name: "S s".to_string(),
                    },
                ],
            },
        );

        let mut payload = LOCAL_PRELUDE_M0Q1A_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("Prelude area read window");
        let source_layout = area_static_layout(&payload, fragment_offset).expect("static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        let context =
            crate::translate::module::observed_module_context().expect("observed module context");
        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &source_layout,
            Some(&context),
        )
        .expect("Prelude no-name area packet resolves by exact area resref");
        assert_eq!(compact_info.resref, "m0q1a");
        assert_eq!(compact_info.name, "M0Q1A");
        assert_eq!(compact_info.width, 3);
        assert_eq!(compact_info.height, 3);
        assert_eq!(compact_info.tileset, "tic01");

        let summary = rewrite_area_client_area_payload(&mut payload).expect("Prelude area rewrite");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Prelude area should match EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "m0q1a");
        assert_eq!(summary.tileset_resref, "tic01");
        assert_eq!(summary.width, 3);
        assert_eq!(summary.packet_height, 3);
        assert_eq!(summary.tile_count, 9);
        assert_eq!(summary.sound_count_zero_one_repairs, 8);
        assert_eq!(summary.static_placeable_trailing_rows_dropped, 13);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondModuleResourceAreaRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondSoundCountZeroMeansOneRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondIgnoredTrailingStaticPlaceableRowsDropped)
        );
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_chapter1_declared_zero_no_name_area_accepts_post_tile_fragment_tail() {
        let _context_guard = module_context_test_guard();
        let chapter1_module_path = r"C:\NWN\NWN Diamond\nwm\Chapter1.nwm";
        assert!(std::path::Path::new(chapter1_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", chapter1_module_path);
        // The live compact Module_Info table has 130 entries and resolves this
        // module by area-table evidence. Keep the unit context small by using
        // the same local module table identity plus enough exact ARE names to
        // prove the resource table without embedding the full Module_Info dump.
        let observed_context = crate::translate::module::ObservedModuleContext {
            localized_name: "Chapter One".to_string(),
            module_resref: "map_m1q1k".to_string(),
            areas: vec![
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_0000,
                    name: "Map_M1Q2K".to_string(),
                },
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_0467,
                    name: "Map_M1Q4A".to_string(),
                },
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_0001,
                    name: "Map_M1Q2E".to_string(),
                },
            ],
        };
        crate::translate::module::remember_observed_module_context_for_tests(
            observed_context.clone(),
        );

        let mut payload = LOCAL_CHAPTER1_MAP_M1Q1_DECLARED_ZERO.to_vec();
        assert_eq!(read_u32_le(&payload, HIGH_LEVEL_HEADER_BYTES), Some(0));

        let mut staged = payload.clone();
        let inferred_declared = staged.len() - 8;
        write_u32_le(
            &mut staged,
            HIGH_LEVEL_HEADER_BYTES,
            inferred_declared as u32,
        )
        .expect("stage declared-zero read window");
        let (_, _, fragment_offset, fragment_size) =
            area_client_area_read_window(&staged).expect("staged read window");
        assert_eq!(fragment_size, 8);
        assert_eq!(
            cnw_fragment_consumable_bits(&staged[fragment_offset..]),
            Some(57)
        );
        let source_layout =
            area_static_layout(&staged, fragment_offset).expect("staged static layout");
        assert_eq!(
            source_layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            fixed_resref_preview(
                &staged,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("map_m1q1")
        );
        let source_scan = scan_area_tile_stream(&staged, fragment_offset);
        assert!(source_scan.valid);
        assert_eq!(source_scan.width, 3);
        assert_eq!(source_scan.packet_height, 0);
        assert_eq!(source_scan.inferred_height, 5);
        assert_eq!(source_scan.tile_count, 15);

        let compact_info = module_area_resource_info_for_compact_packet(
            &staged,
            fragment_offset,
            0x8000_18B2,
            &source_layout,
            Some(&observed_context),
        )
        .expect(
            "Chapter1 no-name area packet resolves by exact dimensions/tileset resource prefix",
        );
        assert_eq!(compact_info.resref, "map_m1q1k");
        assert_eq!(compact_info.name, "MAP_M1Q1K");
        assert_eq!(compact_info.width, 3);
        assert_eq!(compact_info.height, 5);
        assert_eq!(compact_info.tileset, "tic01");

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("declared-zero Chapter1 map_m1q1 area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Chapter1 area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.old_declared, 0);
        assert_eq!(summary.area_resref, "map_m1q1k");
        assert_eq!(summary.tileset_resref, "tic01");
        assert_eq!(summary.width, 3);
        assert_eq!(summary.packet_height, 5);
        assert_eq!(summary.inferred_height, 5);
        assert_eq!(summary.tile_count, 15);
        assert_eq!(summary.sound_count_zero_one_repairs, 0);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondDeclaredZeroReadWindow)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
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
        assert_eq!(proof.transition_count, 0);
        assert_eq!(proof.map_pin_count, 0);
        assert_eq!(proof.sound_count, 7);
        assert_eq!(proof.light_count, 1);
        assert_eq!(proof.static_count, 31);
        assert_eq!(proof.first_post_static_count, 0);
        assert_eq!(proof.second_post_static_count, 0);
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_chapter1e_no_name_area_rebuilds_map_m1q6a_from_packet_resref() {
        let _context_guard = module_context_test_guard();
        let chapter1e_module_path = r"C:\NWN\NWN Diamond\nwm\Chapter1E.nwm";
        assert!(std::path::Path::new(chapter1e_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", chapter1e_module_path);
        let observed_context = crate::translate::module::ObservedModuleContext {
            localized_name: String::new(),
            module_resref: String::new(),
            areas: Vec::new(),
        };

        let mut payload = LOCAL_CHAPTER1E_MAP_M1Q6A_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("Chapter1E area read window");
        let layout =
            area_static_layout(&payload, fragment_offset).expect("Chapter1E static layout");
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("map_m1q6a")
        );
        let info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_020C,
            &layout,
            Some(&observed_context),
        )
        .expect("Chapter1E no-name area packet resolves by exact local area resref");
        assert_eq!(info.resref, "map_m1q6a");

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("Chapter1E map_m1q6a area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Chapter1E area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "map_m1q6a");
        assert_eq!(summary.tileset_resref, info.tileset);
        assert_eq!(summary.width, info.width);
        assert_eq!(summary.packet_height, info.height);
        assert_eq!(summary.inferred_height, info.height);
        assert_eq!(proof.read_end, summary.new_read_size);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp1_no_name_area_rebuilds_resource_tiles_with_empty_tail() {
        let _context_guard = module_context_test_guard();
        let xp1_module_path = r"C:\NWN\NWN Diamond\nwm\XP1-Chapter 1.nwm";
        assert!(std::path::Path::new(xp1_module_path).is_file());
        let _module_path_guard = EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", xp1_module_path);
        let observed_context = crate::translate::module::ObservedModuleContext {
            localized_name: "XP1-Chapter 1".to_string(),
            module_resref: "XP1-Chapt".to_string(),
            areas: Vec::new(),
        };

        let mut payload = LOCAL_XP1_Q1A2DROGONFLOOR2_COMPACT.to_vec();
        let (_, _, fragment_offset, fragment_size) =
            area_client_area_read_window(&payload).expect("XP1 read window");
        let layout = area_static_layout(&payload, fragment_offset).expect("XP1 static layout");
        assert_eq!(fragment_size, 8);
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        let info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_00A1,
            &layout,
            Some(&observed_context),
        )
        .expect("XP1 resource info");
        assert_eq!(info.resref, "q1a2drogonfloor2");
        assert_eq!(info.width, 6);
        assert_eq!(info.height, 6);
        assert_eq!(info.tileset, "tin01");
        assert!(info.map_notes.is_empty());
        assert!(info.sounds.is_empty());

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("XP1 area should rewrite from local resource evidence");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten XP1 area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "q1a2drogonfloor2");
        assert_eq!(summary.tileset_resref, "tin01");
        assert_eq!(summary.width, 6);
        assert_eq!(summary.packet_height, 6);
        assert_eq!(summary.inferred_height, 6);
        assert_eq!(summary.tile_count, 36);
        assert!(summary.new_read_size < summary.old_read_size);
        assert_eq!(proof.transition_count, 0);
        assert_eq!(proof.map_pin_count, 0);
        assert_eq!(proof.sound_count, 0);
        assert_eq!(proof.light_count, 0);
        assert_eq!(proof.static_count, 0);
        assert_eq!(proof.first_post_static_count, 0);
        assert_eq!(proof.second_post_static_count, 0);
        assert_eq!(proof.fragment_bits_consumed, proof.fragment_bits_available);
        assert!(ee_area_client_area_payload_shape_valid(&payload));
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_chapter2e_no_name_area_uses_layout_gated_resource_lookup() {
        let _context_guard = module_context_test_guard();
        let chapter2e_module_path = r"C:\NWN\NWN Diamond\nwm\Chapter2E.nwm";
        assert!(std::path::Path::new(chapter2e_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", chapter2e_module_path);
        let observed_context = crate::translate::module::ObservedModuleContext {
            localized_name: "Luskan and Host Tower".to_string(),
            module_resref: "m2q4a".to_string(),
            areas: vec![
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_03B3,
                    name: "M2Q4A".to_string(),
                },
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_03B4,
                    name: "M2Q4A01".to_string(),
                },
                crate::translate::module::ObservedModuleArea {
                    object_id: 0x8000_03B5,
                    name: "M2Q4A02".to_string(),
                },
            ],
        };
        crate::translate::module::remember_observed_module_context_for_tests(
            observed_context.clone(),
        );

        let mut payload = LOCAL_CHAPTER2E_M2Q4A_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("Chapter2E area read window");
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("m2q4a")
        );
        let primary_layout =
            area_static_layout(&payload, fragment_offset).expect("primary static layout");
        assert_eq!(
            primary_layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert!(!scan_area_tile_stream(&payload, fragment_offset).valid);

        let no_name_layout =
            diamond_no_area_name_static_layout(&payload, fragment_offset).expect("no-name layout");
        assert_eq!(
            no_name_layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert!(
            diamond_fixed_area_name_fragments(&payload, fragment_offset).is_none(),
            "this capture is already proven as no-name; fragment-name probing must stay gated by the selected layout"
        );
        let selected = select_area_static_layout_for_rewrite(
            &payload,
            fragment_offset,
            0x8000_03B3,
            &primary_layout,
            Some(&observed_context),
        );
        assert_eq!(
            selected.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_03B3,
            &selected,
            Some(&observed_context),
        )
        .expect("Chapter2E no-name area packet resolves by exact local area resref");
        assert_eq!(compact_info.resref, "m2q4a");

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("Chapter2E m2q4a area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Chapter2E area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, compact_info.resref);
        assert_eq!(summary.tileset_resref, compact_info.tileset);
        assert_eq!(summary.width, compact_info.width);
        assert_eq!(summary.packet_height, compact_info.height);
        assert_eq!(
            summary.tile_count,
            compact_info.width.saturating_mul(compact_info.height)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_chapter2e_no_name_area_resolves_from_explicit_module_without_context() {
        let _context_guard = module_context_test_guard();
        let chapter2e_module_path = r"C:\NWN\NWN Diamond\nwm\Chapter2E.nwm";
        assert!(std::path::Path::new(chapter2e_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", chapter2e_module_path);
        let empty_context = crate::translate::module::ObservedModuleContext {
            localized_name: String::new(),
            module_resref: String::new(),
            areas: Vec::new(),
        };

        let mut payload = LOCAL_CHAPTER2E_M2Q4A_NO_CONTEXT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("Chapter2E area read window");
        let layout = area_static_layout(&payload, fragment_offset).expect("Chapter2E layout");
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("m2q4a")
        );

        let compact_info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_03B3,
            &layout,
            Some(&empty_context),
        )
        .expect("explicit local module path should prove packet-local area CResRef");
        assert_eq!(compact_info.resref, "m2q4a");

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&empty_context),
        )
        .expect("Chapter2E m2q4a area should rewrite before Module_Info context");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Chapter2E area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "m2q4a");
        assert_eq!(summary.tileset_resref, compact_info.tileset);
        assert_eq!(summary.width, compact_info.width);
        assert_eq!(summary.packet_height, compact_info.height);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_chapter2_no_name_area_resolves_fragmented_area_resref() {
        let _context_guard = module_context_test_guard();
        let chapter2_module_path = r"C:\NWN\NWN Diamond\nwm\Chapter2.nwm";
        assert!(std::path::Path::new(chapter2_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", chapter2_module_path);
        let observed_context = crate::translate::module::ObservedModuleContext {
            // This mirrors the current local Chapter2 compact Module_Info
            // failure mode: the area packet itself has the better resource
            // proof than the observed module table names.
            localized_name: "chap1_chap".to_string(),
            module_resref: "chap1_chap".to_string(),
            areas: Vec::new(),
        };

        let mut payload = LOCAL_CHAPTER2_A08_BARRACKS_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("Chapter2 area read window");
        let layout = area_static_layout(&payload, fragment_offset).expect("Chapter2 static layout");
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("a08_bar")
        );
        assert_eq!(
            compact_packet_area_resref_fragments(&payload, fragment_offset)
                .expect("fragmented packet area resref"),
            vec!["a08_bar".to_string(), "ks".to_string()]
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                HIGH_LEVEL_HEADER_BYTES + layout.tileset_read_offset
            )
            .as_deref(),
            Some("tin01")
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.width_read_offset),
            Some(0)
        );
        assert_eq!(
            read_area_u32(&payload, fragment_offset, layout.height_read_offset),
            Some(0)
        );

        let info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0000,
            &layout,
            Some(&observed_context),
        )
        .expect("fragmented no-name area packet resolves through the local ARE table");
        assert_eq!(info.resref, "a08_barracks");
        assert_eq!(info.width, 5);
        assert_eq!(info.height, 3);
        assert_eq!(info.tileset, "tin01");
        assert_eq!(info.placeables.len(), 43);
        assert_eq!(
            info.placeables
                .iter()
                .filter(|placeable| placeable.static_object)
                .count(),
            32
        );
        assert_eq!(
            info.placeables
                .iter()
                .filter(|placeable| placeable.useable)
                .count(),
            7
        );
        assert_eq!(
            info.placeables
                .iter()
                .filter(|placeable| placeable.trap_disarmable)
                .count(),
            43
        );
        assert_eq!(
            info.placeables
                .iter()
                .filter(|placeable| placeable.lockable)
                .count(),
            0
        );
        assert_eq!(
            info.placeables
                .iter()
                .filter(|placeable| placeable.locked)
                .count(),
            0
        );
        assert!(
            info.placeables.iter().all(|placeable| !placeable.trap_flag),
            "Chapter2 a08_barracks GIT has no trapped placeables; trap visuals must not be invented by area/live translation"
        );

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("Chapter2 a08_barracks area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Chapter2 area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "a08_barracks");
        assert_eq!(summary.tileset_resref, "tin01");
        assert_eq!(summary.width, 5);
        assert_eq!(summary.packet_height, 3);
        assert_eq!(summary.inferred_height, 3);
        assert_eq!(summary.tile_count, 15);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondFragmentedAreaResrefRepair)
        );
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_chapter3_no_name_area_rebuilds_resource_tiles_with_tail() {
        let _context_guard = module_context_test_guard();
        let chapter3_module_path = r"C:\NWN\NWN Diamond\nwm\Chapter3.nwm";
        assert!(std::path::Path::new(chapter3_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", chapter3_module_path);
        let observed_context = crate::translate::module::ObservedModuleContext {
            // Local Chapter3 Module_Info can expose a stale compact module
            // resref/name window, so the packet's direct area CResRef plus the
            // local module snapshot must prove the ARE identity.
            localized_name: "chap2_chap".to_string(),
            module_resref: "chap2_chap".to_string(),
            areas: Vec::new(),
        };

        let mut payload = LOCAL_CHAPTER3_M3Q1A10_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("Chapter3 area read window");
        let layout = area_static_layout(&payload, fragment_offset).expect("Chapter3 static layout");
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("m3q1a10")
        );

        let info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0488,
            &layout,
            Some(&observed_context),
        )
        .expect("Chapter3 no-name area packet resolves through the local ARE table");
        assert_eq!(info.resref, "m3q1a10");
        assert_eq!(info.tileset, "tin01");

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("Chapter3 m3q1a10 area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Chapter3 area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "m3q1a10");
        assert_eq!(summary.tileset_resref, "tin01");
        assert_eq!(summary.width, info.width);
        assert_eq!(summary.packet_height, info.height);
        assert_eq!(summary.inferred_height, info.height);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_chapter4_no_name_area_rebuilds_resource_tiles_with_tail() {
        let _context_guard = module_context_test_guard();
        let chapter4_module_path = r"C:\NWN\NWN Diamond\nwm\Chapter4.nwm";
        assert!(std::path::Path::new(chapter4_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", chapter4_module_path);
        let observed_context = crate::translate::module::ObservedModuleContext {
            // Local Chapter4 Module_Info has already been proven separately;
            // this area packet carries the direct compact area CResRef and the
            // local ARE table must prove the tile/static tail shape exactly.
            localized_name: "Chapter Four".to_string(),
            module_resref: "chap3_chap4".to_string(),
            areas: Vec::new(),
        };

        let mut payload = LOCAL_CHAPTER4_MAP_M1Q6A_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("Chapter4 area read window");
        let layout = area_static_layout(&payload, fragment_offset).expect("Chapter4 static layout");
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("map_m1q6a")
        );

        let info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_0368,
            &layout,
            Some(&observed_context),
        )
        .expect("Chapter4 no-name area packet resolves through the local ARE table");
        assert_eq!(info.resref, "map_m1q6a");

        let summary = rewrite_area_client_area_payload_with_module_context(
            &mut payload,
            Some(&observed_context),
        )
        .expect("Chapter4 map_m1q6a area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten Chapter4 area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "map_m1q6a");
        assert_eq!(summary.tileset_resref, info.tileset);
        assert_eq!(summary.width, info.width);
        assert_eq!(summary.packet_height, info.height);
        assert_eq!(summary.inferred_height, info.height);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
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

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp2_chapter3_no_name_area_resolves_from_packet_resref_without_module_context() {
        let _context_guard = module_context_test_guard();
        let xp2_chapter3_module_path = r"C:\NWN\NWN Diamond\nwm\XP2_Chapter3.nwm";
        assert!(std::path::Path::new(xp2_chapter3_module_path).is_file());
        let _module_path_guard =
            EnvVarTestGuard::set("NWN_BRIDGE_MODULE_PATH", xp2_chapter3_module_path);

        let mut payload = LOCAL_XP2_CHAPTER3_GATESOFCANIA_COMPACT.to_vec();
        let (_, _, fragment_offset, _) =
            area_client_area_read_window(&payload).expect("XP2 Chapter3 area read window");
        let layout =
            area_static_layout(&payload, fragment_offset).expect("XP2 Chapter3 static layout");
        assert_eq!(
            layout.area_name_encoding,
            AreaNameEncoding::DiamondNoAreaName
        );
        assert_eq!(
            fixed_resref_preview(
                &payload,
                LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET + LEGACY_AREA_OBJECT_ID_BYTES
            )
            .as_deref(),
            Some("gatesofcania")
        );

        let info = module_area_resource_info_for_compact_packet(
            &payload,
            fragment_offset,
            0x8000_08F5,
            &layout,
            None,
        )
        .expect("packet-local no-name area CResRef should resolve through the local XP2 ARE table");
        assert_eq!(info.resref, "gatesofcania");

        let summary = rewrite_area_client_area_payload(&mut payload)
            .expect("XP2 Chapter3 gatesofcania area should rewrite exactly");
        let proof = ee_area_client_area_exact_read_proof(&payload)
            .expect("rewritten XP2 Chapter3 area should satisfy EE LoadArea cursor proof");

        assert_eq!(summary.area_resref, "gatesofcania");
        assert_eq!(summary.tileset_resref, info.tileset);
        assert_eq!(summary.width, info.width);
        assert_eq!(summary.packet_height, info.height);
        assert_eq!(summary.inferred_height, info.height);
        assert!(
            summary
                .rewrite_kinds
                .contains(&AreaRewriteKind::LegacyDiamondNoAreaName)
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
