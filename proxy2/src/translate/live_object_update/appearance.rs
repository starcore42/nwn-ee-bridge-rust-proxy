//! Legacy creature appearance (`P/5`) live-object update parsing.
//!
//! This module owns one semantic family: the server-to-client appearance update
//! written by `CNWSMessage::WriteGameObjUpdate_UpdateAppearance`. The generic
//! live-object boundary walker must not split this record on interior bytes:
//! the decompiled writer can embed visible-equipment item records whose payloads
//! contain bytes that look like top-level `U/9` or `A` live-object opcodes.
//!
//! Decompile anchors:
//!
//! - EE server `WriteGameObjUpdate_UpdateAppearance` writes `CHAR 'P'`, the
//!   creature object type byte, object id, and a WORD appearance mask before the
//!   mask-selected fields.
//! - The legacy/Diamond path for old clients writes the portrait/body-part
//!   fields using compact BYTE encodings, then for mask `0xFFFF` writes a
//!   visible-equipment count followed by fixed dummy `D` records and variable
//!   `A` item-add records.
//! - EE client `HandleServerToPlayerGameObjectUpdate` routes `P` to the
//!   appearance reader and reports `Unknown Update sub-message` only after the
//!   reader returns and unread bytes remain, so the proxy must keep this whole
//!   semantic record together.

use super::{
    CNW_FRAGMENT_HEADER_BITS, LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
    MAX_COMPACT_LEGACY_LIVE_OBJECT_ID, MAX_LIVE_OBJECT_NAME_BYTES,
    MAX_REASONABLE_LIVE_PAYLOAD_BYTES, MIN_COMPACT_LEGACY_LIVE_OBJECT_ID, bits, boundary,
    fragment_spans, read_u16_le, read_u32_le,
};

const LEGACY_CREATURE_TYPE: u8 = 0x05;
const LEGACY_APPEARANCE_HEADER_BYTES: usize = 8;
const LEGACY_CREATURE_VISUAL_TRANSFORM_UPDATE_HEADER_BYTES: usize = 7;
const LEGACY_APPEARANCE_NAME_MASK: u16 = 0x0400;
const LEGACY_APPEARANCE_ALL_FIELDS_MASK: u16 = 0xFFFF;
const LEGACY_APPEARANCE_BODY_PART_MASK: u16 = 0x0100;
const LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK: u16 = 0x0200;
const LEGACY_APPEARANCE_BODY_PART_COUNT: u8 = 0x13;
const LEGACY_APPEARANCE_MAX_EQUIPMENT_RECORDS: u8 = 32;
const LEGACY_APPEARANCE_DUMMY_ITEM_OBJECT_ID: u32 = 0x7F00_0000;
const LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES: usize = 4096;
const LEGACY_APPEARANCE_MIN_ITEM_NAME_TAIL_BYTES: usize = 19;
const LEGACY_APPEARANCE_MAX_ITEM_NAME_TAIL_BYTES: usize = 96;
const MAX_EE_APPEARANCE_TRAILING_LEGACY_TAIL_BYTES: usize = 128;
const MAX_EE_APPEARANCE_FOLLOWING_CREATURE_UPDATE_EMPTY_FRAGMENT_SPAN_BYTES: usize = 2;
// Reassembled packetized live-object streams can promote chunk-local zero
// storage into the shared CNW fragment tail. The padding is still removed only
// when the fully rewritten EE appearance validator accepts the exact record, so
// this bound controls search cost rather than semantic acceptance.
const LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS: usize = 64;

const LEGACY_APPEARANCE_ACTIVE_PROPERTY_BYTES: usize = 7;
const LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS: usize = 4;
const LEGACY_APPEARANCE_MIN_ACTIVE_PROPERTY_TAIL_BYTES: usize = 11;
const EE_APPEARANCE_ACTIVE_PROPERTY_EXTRA_BOOL_BITS: usize = 1;
const EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES: usize = 0x72;
const LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS: usize = 1;
const LEGACY_APPEARANCE_ITEM_NAME_BARE_INLINE_LOCSTRING_BITS: usize = 2;
const LEGACY_APPEARANCE_ITEM_NAME_STRREF_LOCSTRING_BITS: usize = 3;
const LEGACY_ARMOR_BASE_ITEM: u32 = 0x10;
const WORK_REMAINING_OPCODE: u8 = b'W';
const WORK_REMAINING_RECORD_BYTES: usize = 3;
const GUI_ZERO_FRAGMENT_STORAGE_MAX_BYTES: usize = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CreatureAppearanceWireDialect {
    LegacyDiamond,
    EeBuild8193,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ItemAppearanceWireDialect {
    LegacyDiamond,
    EeBuild8193,
}

fn ee_active_property_extra_bool_insert_offset(name_bits: usize) -> usize {
    // After the item-name selector/name branch, Diamond `sub_451020` reads one
    // BOOL, two DWORDs, then three more BOOLs before the property-count BYTE.
    // EE `sub_14076BD30` reads one BOOL, the same two DWORDs, then four BOOLs.
    // The EE-only BOOL is therefore the first post-DWORD BOOL, immediately
    // after the shared pre-DWORD BOOL in fragment-stream order.
    name_bits.saturating_add(1)
}

// Diamond full-appearance records can be followed immediately by a creature U
// update whose CNW fragment fence is still owned by the update parser.  The
// observed fence widths are deliberately tiny and are only accepted when the
// following U record exact parser, including interleaved-span proof, consumes a
// complete record boundary.  This keeps the appearance rewrite from claiming a
// transport split heuristically.
const LEGACY_FULL_APPEARANCE_FOLLOWING_CREATURE_UPDATE_FRAGMENT_FENCE_CANDIDATES: [usize; 3] = [
    CNW_FRAGMENT_HEADER_BITS,
    CNW_FRAGMENT_HEADER_BITS + 1,
    CNW_FRAGMENT_HEADER_BITS + LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
];
// A full creature appearance may be preceded by packetized CNW fragment fence
// bits in captured HG streams. This is transport framing, not an appearance
// reader field, so it is only tried after a zero-fence appearance proof fails.
// Keeping zero as the first-class exact path prevents the fence from shifting
// otherwise-valid records such as Town Greeter.
//
// The appearance reader itself is still the decompile-owned proof boundary:
// Diamond `sub_448E30` and EE's corresponding appearance path consume the name
// selector as the first semantic fragment bit. The optional candidates below are
// only accepted when those leading bits prove a packetized CNW fence and the
// focused appearance parser consumes the full record at the post-fence cursor.
const LEGACY_FULL_APPEARANCE_PRECEDING_FRAGMENT_FENCE_CANDIDATES: [usize; 4] = [
    0,
    CNW_FRAGMENT_HEADER_BITS,
    CNW_FRAGMENT_HEADER_BITS + 1,
    CNW_FRAGMENT_HEADER_BITS * 2,
];
const EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES: [u8;
    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN] =
    super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES;
const LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES: [u8;
    super::visual_transform::LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN] =
    super::visual_transform::LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES;
const LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN: usize =
    super::visual_transform::LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum AppearanceNameShape {
    LocStringPair,
    CExoString,
}

impl AppearanceNameShape {
    fn fragment_bit(self) -> bool {
        matches!(self, AppearanceNameShape::LocStringPair)
    }

    fn alternate(self) -> Self {
        match self {
            AppearanceNameShape::LocStringPair => AppearanceNameShape::CExoString,
            AppearanceNameShape::CExoString => AppearanceNameShape::LocStringPair,
        }
    }
}

#[derive(Debug, Clone)]
struct LegacyAppearanceRecord {
    record_end: usize,
    fragment_bits_consumed: usize,
    ee_fragment_bits_consumed: usize,
    ee_extra_insert_offsets: Vec<usize>,
    ee_name_bit_rewrites: Vec<FragmentNameBitRewrite>,
    ee_extra_byte_inserts: Vec<CreatureAppearanceByteInsert>,
    equipment_records: u8,
    preferred_zero_padding_relative_start: Option<usize>,
    token_selector_padding_repair_relative_start: Option<usize>,
    inline_active_name_fence_repair_relative_start: Option<usize>,
}

#[derive(Debug, Clone)]
struct VerifiedAppearanceParse {
    record: LegacyAppearanceRecord,
    proof_cursor: usize,
    preceding_fence_bits: usize,
}

#[derive(Debug, Clone)]
enum CreatureAppearanceByteInsert {
    MissingSecondInlineNameLength {
        offset: usize,
        length: u32,
    },
    EeFeature23CreatureScalarHighByte {
        offset: usize,
    },
    EeFeature23CreatureBodyPartHighByte {
        offset: usize,
    },
    EeFeature0eCreatureTailByte {
        offset: usize,
    },
    EeFeature23ItemAppearanceHighByte {
        offset: usize,
    },
    EeModelType3ArmorAccessoryTable {
        offset: usize,
        legacy_palette: [u8; 6],
    },
    LegacyVisualTransformIdentity {
        offset: usize,
    },
    LegacyVisualTransformIdentitySuffix {
        offset: usize,
        start: usize,
    },
    LegacyScalarVisualTransformIdentityReplacement {
        offset: usize,
    },
    LegacyFullPartTablePrefixRemoval {
        offset: usize,
        bytes: usize,
    },
}

impl CreatureAppearanceByteInsert {
    fn offset(&self) -> usize {
        match self {
            Self::MissingSecondInlineNameLength { offset, .. }
            | Self::EeFeature23CreatureScalarHighByte { offset }
            | Self::EeFeature23CreatureBodyPartHighByte { offset }
            | Self::EeFeature0eCreatureTailByte { offset }
            | Self::EeFeature23ItemAppearanceHighByte { offset }
            | Self::EeModelType3ArmorAccessoryTable { offset, .. }
            | Self::LegacyVisualTransformIdentity { offset }
            | Self::LegacyVisualTransformIdentitySuffix { offset, .. }
            | Self::LegacyScalarVisualTransformIdentityReplacement { offset }
            | Self::LegacyFullPartTablePrefixRemoval { offset, .. } => *offset,
        }
    }

    fn order(&self) -> u8 {
        match self {
            Self::EeFeature23CreatureScalarHighByte { .. }
            | Self::EeFeature23CreatureBodyPartHighByte { .. }
            | Self::EeFeature0eCreatureTailByte { .. }
            | Self::EeFeature23ItemAppearanceHighByte { .. } => 0,
            Self::EeModelType3ArmorAccessoryTable { .. } => 1,
            Self::LegacyVisualTransformIdentity { .. }
            | Self::LegacyVisualTransformIdentitySuffix { .. }
            | Self::LegacyScalarVisualTransformIdentityReplacement { .. } => 2,
            Self::MissingSecondInlineNameLength { .. } => 3,
            Self::LegacyFullPartTablePrefixRemoval { .. } => 0,
        }
    }

    fn bytes_removed(&self) -> usize {
        match self {
            Self::LegacyScalarVisualTransformIdentityReplacement { .. } => {
                LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
            }
            Self::LegacyFullPartTablePrefixRemoval { bytes, .. } => *bytes,
            _ => 0,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::MissingSecondInlineNameLength { length, .. } => length.to_le_bytes().to_vec(),
            Self::EeFeature23CreatureScalarHighByte { .. }
            | Self::EeFeature23CreatureBodyPartHighByte { .. }
            | Self::EeFeature0eCreatureTailByte { .. }
            | Self::EeFeature23ItemAppearanceHighByte { .. } => vec![0],
            Self::EeModelType3ArmorAccessoryTable { legacy_palette, .. } => {
                ee_model_type_3_armor_accessory_table_from_legacy_palette(*legacy_palette)
            }
            Self::LegacyVisualTransformIdentity { .. } => {
                EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.to_vec()
            }
            Self::LegacyVisualTransformIdentitySuffix { start, .. } => {
                EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES
                    .get(*start..)
                    .unwrap_or(&[])
                    .to_vec()
            }
            Self::LegacyScalarVisualTransformIdentityReplacement { .. } => {
                EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.to_vec()
            }
            Self::LegacyFullPartTablePrefixRemoval { .. } => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CreatureAppearanceByteApplySummary {
    bytes_inserted: usize,
    bytes_removed: usize,
}

#[derive(Debug, Clone)]
struct LegacyVisibleEquipmentParse {
    end: usize,
    fragment_bits_consumed: usize,
    ee_extra_fragment_bits: usize,
    ee_extra_insert_offsets: Vec<usize>,
    ee_name_bit_rewrites: Vec<FragmentNameBitRewrite>,
    ee_extra_byte_inserts: Vec<CreatureAppearanceByteInsert>,
    first_positive_name_selector_relative_start: Option<usize>,
    token_selector_padding_repair_relative_start: Option<usize>,
    inline_active_name_fence_repair_relative_start: Option<usize>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct FragmentNameBitRewrite {
    relative_offset: usize,
    proof: LegacyItemNameFragmentProof,
}

#[derive(Debug, Clone)]
struct LegacyAppearanceItemAddRecord {
    fragment_bits_consumed: usize,
    ee_extra_fragment_bits: usize,
    ee_extra_insert_offsets: Vec<usize>,
    ee_extra_byte_inserts: Vec<CreatureAppearanceByteInsert>,
    name_fragment_proof: LegacyItemNameFragmentProof,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum LegacyItemNameFragmentProof {
    None,
    InlineCExoString,
    LocStringToken,
    LocStringInlineCExoString,
    BareInlineLocString,
}

impl LegacyItemNameFragmentProof {
    fn matches(self, fragment_bits: &[bool], bit_cursor: usize) -> bool {
        match self {
            Self::None => true,
            Self::InlineCExoString => fragment_bits
                .get(bit_cursor)
                .map(|bit| !*bit)
                .unwrap_or(false),
            Self::LocStringToken => fragment_bits
                .get(bit_cursor)
                .zip(fragment_bits.get(bit_cursor.saturating_add(1)))
                .map(|(outer, inner)| *outer && *inner)
                .unwrap_or(false),
            Self::LocStringInlineCExoString | Self::BareInlineLocString => fragment_bits
                .get(bit_cursor)
                .zip(fragment_bits.get(bit_cursor.saturating_add(1)))
                .map(|(outer, inner)| *outer && !*inner)
                .unwrap_or(false),
        }
    }

    fn starts_with_positive_selector(self) -> bool {
        matches!(
            self,
            Self::LocStringToken | Self::LocStringInlineCExoString | Self::BareInlineLocString
        )
    }
}

fn item_name_bit_rewrites(
    item: &LegacyAppearanceItemAddRecord,
    relative_offset: usize,
) -> Vec<FragmentNameBitRewrite> {
    if item.name_fragment_proof == LegacyItemNameFragmentProof::None {
        Vec::new()
    } else {
        vec![FragmentNameBitRewrite {
            relative_offset,
            proof: item.name_fragment_proof,
        }]
    }
}

#[derive(Debug, Clone, Copy)]
struct MissingSecondInlineNameCandidate {
    name_end: usize,
    name_len: usize,
    record_end: usize,
    equipment_records: u8,
}

#[derive(Clone, Copy)]
struct AppearanceBitProof<'a> {
    bit_cursor: usize,
    fragment_bits: &'a [bool],
    translated_ee: bool,
    allow_cross_record_fence: bool,
    owner_offset: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CreatureAppearanceExtraRewrite {
    pub bits_inserted: usize,
    pub bits_removed: usize,
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureVisualTransformUpdateRewrite {
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
    pub bits_inserted: usize,
}

pub(super) fn try_get_legacy_creature_appearance_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    let scan_end = scan_end.min(bytes.len());
    let mut accepted: Option<LegacyAppearanceRecord> = None;
    let mask = read_u16_le(bytes, offset.checked_add(6)?).unwrap_or(0);
    for name_shape in [
        AppearanceNameShape::LocStringPair,
        AppearanceNameShape::CExoString,
    ] {
        let Some(record) = parse_creature_appearance_record(
            bytes,
            offset,
            scan_end,
            name_shape,
            CreatureAppearanceWireDialect::LegacyDiamond,
            None,
        ) else {
            continue;
        };
        if accepted
            .as_ref()
            .map(|current| legacy_appearance_boundary_candidate_is_better(mask, &record, current))
            .unwrap_or(true)
        {
            accepted = Some(record);
        }
    }
    accepted.map(|record| record.record_end)
}

pub(super) fn try_get_ee_creature_appearance_record_end_by_byte_shape(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    let scan_end = scan_end.min(bytes.len());
    let mut accepted: Option<LegacyAppearanceRecord> = None;
    let mask = read_u16_le(bytes, offset.checked_add(6)?).unwrap_or(0);
    for name_shape in [
        AppearanceNameShape::LocStringPair,
        AppearanceNameShape::CExoString,
    ] {
        let Some(record) = parse_creature_appearance_record(
            bytes,
            offset,
            scan_end,
            name_shape,
            CreatureAppearanceWireDialect::EeBuild8193,
            None,
        ) else {
            continue;
        };
        // This is a byte-shape guard for later rewrite passes whose fragment
        // cursor has already become unreliable. EE `sub_14079FAC0` consumes
        // the model-type 3 armor/accessory table and the legacy visual
        // transform identity bytes before `sub_14076BD30`; Diamond omits those
        // bytes. If the parser no longer requests any EE byte inserts, the
        // visible-equipment subobjects are already in the EE read-buffer shape.
        // Do not use this as a full validator: fragment-bit proof still belongs
        // to `advance_verified_ee_creature_appearance_record`.
        if !record.ee_extra_byte_inserts.is_empty() {
            continue;
        }
        if legacy_full_appearance_extends_past_ee_candidate(
            bytes,
            offset,
            scan_end,
            record.record_end,
        ) {
            continue;
        }
        if accepted
            .as_ref()
            .map(|current| legacy_appearance_boundary_candidate_is_better(mask, &record, current))
            .unwrap_or(true)
        {
            accepted = Some(record);
        }
    }
    accepted.map(|record| record.record_end)
}

fn legacy_appearance_boundary_candidate_is_better(
    mask: u16,
    candidate: &LegacyAppearanceRecord,
    current: &LegacyAppearanceRecord,
) -> bool {
    if mask == LEGACY_APPEARANCE_ALL_FIELDS_MASK {
        // Diamond `WriteGameObjUpdate_UpdateAppearance` writes full-state
        // creature appearances as a 19-byte body table, then a visible-
        // equipment count and exactly that many `D`/`A` records.  EE's
        // `HandleServerToPlayerGameObjectUpdate` consumes the same semantic
        // block before returning to the live-object dispatcher.  A shorter
        // branch can be byte-plausible when the name selector bit is stale and
        // scalar/equipment bytes accidentally line up; prefer the branch that
        // proves the larger equipment block, then the later exact boundary.
        return candidate.equipment_records > current.equipment_records
            || (candidate.equipment_records == current.equipment_records
                && candidate.record_end > current.record_end);
    }

    // Non-full updates are still modeled conservatively.  Until each partial
    // mask family has its own exact typed parser, keep the historical shortest
    // accepted boundary to avoid swallowing a following live-object record.
    candidate.record_end < current.record_end
}

fn legacy_full_appearance_extends_past_ee_candidate(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    candidate_end: usize,
) -> bool {
    if read_u16_le(bytes, offset.saturating_add(6)) != Some(LEGACY_APPEARANCE_ALL_FIELDS_MASK) {
        return false;
    }

    // Decompile-backed false-positive guard.
    //
    // EE build 8193 widens the 0x0080 appearance scalar and full-body part
    // values when `ServerSatisfiesBuild(0x2001, 0x23, 0)` is true. A raw
    // Diamond full-state `P/5` can accidentally look like an already-EE record
    // when the byte after the compact legacy scalar is itself >= 0x0A; EE then
    // treats that legacy prefix byte as the full-body selector, consumes WORD
    // body parts across the real equipment list, and reaches a short
    // zero-equipment boundary. If Diamond's `sub_448E30` model proves a longer
    // full appearance record at the same offset, that shorter EE claim is only
    // shifted legacy bytes and must not suppress the typed translation.
    try_get_legacy_creature_appearance_record_end(bytes, offset, scan_end)
        .is_some_and(|legacy_end| legacy_end > candidate_end)
}

pub(super) fn advance_verified_legacy_creature_appearance_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let Some(verified) = parse_verified_creature_appearance_with_optional_preceding_fence(
        bytes,
        offset,
        record_end,
        fragment_bits,
        *bit_cursor,
        Some(record_end),
        false,
        true,
    ) else {
        return false;
    };
    if verified.record.record_end != record_end
        || verified.record.fragment_bits_consumed
            > fragment_bits.len().saturating_sub(verified.proof_cursor)
    {
        return false;
    }
    if verified.preceding_fence_bits != 0 {
        trace_preceding_appearance_fence(offset, *bit_cursor, &verified);
    }
    *bit_cursor = verified
        .proof_cursor
        .saturating_add(verified.record.fragment_bits_consumed);
    true
}

pub(super) fn advance_verified_ee_creature_appearance_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let Some(verified) = parse_verified_creature_appearance_with_optional_preceding_fence(
        bytes,
        offset,
        record_end,
        fragment_bits,
        *bit_cursor,
        Some(record_end),
        true,
        false,
    ) else {
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object EE appearance verify rejected: offset={offset} reason=parse-none record_end={record_end} bit_cursor={}",
                *bit_cursor
            );
        }
        return false;
    };
    if verified.record.record_end != record_end {
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object EE appearance verify rejected: offset={offset} reason=record-end expected={record_end} parsed={} bit_cursor={} proof_cursor={} preceding_fence_bits={} ee_bits={} legacy_bits={} byte_inserts={:?}",
                verified.record.record_end,
                *bit_cursor,
                verified.proof_cursor,
                verified.preceding_fence_bits,
                verified.record.ee_fragment_bits_consumed,
                verified.record.fragment_bits_consumed,
                verified.record.ee_extra_byte_inserts
            );
        }
        return false;
    }
    if legacy_full_appearance_extends_past_ee_candidate(bytes, offset, bytes.len(), record_end) {
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object EE appearance verify rejected: offset={offset} reason=legacy-full-record-extends-past-ee-candidate record_end={record_end} bit_cursor={} proof_cursor={}",
                *bit_cursor, verified.proof_cursor
            );
        }
        return false;
    }
    if verified.record.ee_fragment_bits_consumed
        > fragment_bits.len().saturating_sub(verified.proof_cursor)
    {
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object EE appearance verify rejected: offset={offset} reason=fragment-overflow proof_cursor={} ee_bits={} available={}",
                verified.proof_cursor,
                verified.record.ee_fragment_bits_consumed,
                fragment_bits.len().saturating_sub(verified.proof_cursor)
            );
        }
        return false;
    }
    if !verified.record.ee_extra_byte_inserts.is_empty() {
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object EE appearance verify rejected: offset={offset} reason=remaining-byte-inserts record_end={record_end} parsed_end={} bit_cursor={} proof_cursor={} preceding_fence_bits={} byte_inserts={:?} ee_bit_inserts={:?}",
                verified.record.record_end,
                *bit_cursor,
                verified.proof_cursor,
                verified.preceding_fence_bits,
                verified.record.ee_extra_byte_inserts,
                verified.record.ee_extra_insert_offsets
            );
        }
        return false;
    }

    // EE's visible-equipment active-item-property reader consumes one extra
    // BOOL compared with Diamond's 1.69 stream. Source-side cursor walking must
    // still use `fragment_bits_consumed`, while translated strict validation
    // advances across only the EE-visible active-property delta.
    if verified.preceding_fence_bits != 0 {
        trace_preceding_appearance_fence(offset, *bit_cursor, &verified);
    }
    *bit_cursor = verified
        .proof_cursor
        .saturating_add(verified.record.ee_fragment_bits_consumed);
    true
}

pub(super) fn try_get_verified_ee_creature_appearance_record_end(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let scan_end = scan_end.min(bytes.len());
    if offset >= scan_end {
        return None;
    }
    let exact_candidate = |candidate_end: usize| -> Option<usize> {
        if candidate_end <= offset || candidate_end > scan_end {
            return None;
        }
        let verified = parse_verified_creature_appearance_with_optional_preceding_fence(
            bytes,
            offset,
            candidate_end,
            fragment_bits,
            bit_cursor,
            Some(candidate_end),
            true,
            false,
        )?;
        if verified.record.record_end != candidate_end
            || verified.record.ee_fragment_bits_consumed
                > fragment_bits.len().saturating_sub(verified.proof_cursor)
            || !verified.record.ee_extra_byte_inserts.is_empty()
            || legacy_full_appearance_extends_past_ee_candidate(
                bytes,
                offset,
                scan_end,
                candidate_end,
            )
        {
            return None;
        }

        let mut exact_cursor = bit_cursor;
        if !advance_verified_ee_creature_appearance_record(
            bytes,
            offset,
            candidate_end,
            fragment_bits,
            &mut exact_cursor,
        ) {
            return None;
        }

        let candidate_is_boundary = candidate_end >= scan_end
            || boundary::looks_like_legacy_live_object_sub_message_boundary(bytes, candidate_end);
        let candidate_is_verified_following_creature_update = !candidate_is_boundary
            && candidate_end < scan_end
            && find_verified_following_creature_update_offset_after_appearance(
                bytes,
                candidate_end,
                fragment_bits,
                exact_cursor,
            ) == Some(candidate_end);
        if !candidate_is_boundary && !candidate_is_verified_following_creature_update {
            // Full creature appearances own their visible-equipment substream.
            // A shorter EE-byte-plausible parse can stop on bytes that are
            // still inside that subobject list; do not report it as an
            // already-translated top-level `P/5` boundary unless the following
            // byte is itself a proven live-object boundary.  `U/5 0x3967`
            // cannot be proven from bytes alone, so the only non-generic
            // exception is the focused creature-update reader above, using the
            // exact fragment cursor left by this appearance record.
            return None;
        }
        Some(candidate_end)
    };

    if let Some(byte_shape_end) =
        try_get_ee_creature_appearance_record_end_by_byte_shape(bytes, offset, scan_end)
    {
        if let Some(record_end) = exact_candidate(byte_shape_end) {
            return Some(record_end);
        }
    }

    let mut search_from = offset.saturating_add(2);
    while search_from < scan_end {
        let candidate_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            bytes,
            search_from,
            scan_end,
        )
        .min(scan_end);
        if candidate_end <= offset || candidate_end > scan_end {
            break;
        }
        if let Some(record_end) = exact_candidate(candidate_end) {
            return Some(record_end);
        }
        if candidate_end == scan_end {
            return None;
        }
        search_from = candidate_end.saturating_add(1);
    }

    exact_candidate(scan_end)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureAppearanceTrailingTailRemoval {
    pub bytes_removed: usize,
}

pub(super) fn try_get_ee_creature_appearance_record_end_before_verified_creature_update_tail_for_ee(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let scan_end = scan_end.min(bytes.len());
    if offset >= scan_end {
        return None;
    }

    let mut candidate_ends = Vec::new();
    if let Some(byte_shape_end) =
        try_get_ee_creature_appearance_record_end_by_byte_shape(bytes, offset, scan_end)
    {
        push_verified_tail_candidate_end(&mut candidate_ends, offset, scan_end, byte_shape_end);
    }
    collect_verified_tail_candidate_ends_from_following_creature_updates(
        bytes,
        offset,
        scan_end,
        &mut candidate_ends,
    );

    candidate_ends.sort_unstable();
    candidate_ends.dedup();

    let mut accepted_end = None;
    for candidate_end in candidate_ends {
        let Some(verified) = parse_verified_creature_appearance_with_optional_preceding_fence(
            bytes,
            offset,
            candidate_end,
            fragment_bits,
            bit_cursor,
            Some(candidate_end),
            true,
            false,
        ) else {
            continue;
        };
        if verified.record.record_end == candidate_end
            && verified.record.ee_fragment_bits_consumed
                <= fragment_bits.len().saturating_sub(verified.proof_cursor)
            && verified.record.ee_extra_byte_inserts.is_empty()
        {
            let mut exact_cursor = bit_cursor;
            if advance_verified_ee_creature_appearance_record(
                bytes,
                offset,
                candidate_end,
                fragment_bits,
                &mut exact_cursor,
            ) && find_verified_following_creature_update_offset_after_appearance(
                bytes,
                candidate_end,
                fragment_bits,
                exact_cursor,
            )
            .is_some()
            {
                accepted_end = Some(candidate_end);
            }
        }
    }

    accepted_end
}

fn collect_verified_tail_candidate_ends_from_following_creature_updates(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    candidate_ends: &mut Vec<usize>,
) {
    let scan_end = scan_end.min(bytes.len());
    let mut following_offset = offset.saturating_add(LEGACY_APPEARANCE_HEADER_BYTES);
    while following_offset + 1 < scan_end {
        if bytes.get(following_offset).copied() == Some(b'U')
            && bytes.get(following_offset + 1).copied() == Some(LEGACY_CREATURE_TYPE)
        {
            push_verified_tail_candidate_end(candidate_ends, offset, scan_end, following_offset);
            for empty_span_bytes in
                1..=MAX_EE_APPEARANCE_FOLLOWING_CREATURE_UPDATE_EMPTY_FRAGMENT_SPAN_BYTES
            {
                if let Some(candidate_end) = following_offset.checked_sub(empty_span_bytes) {
                    push_verified_tail_candidate_end(
                        candidate_ends,
                        offset,
                        scan_end,
                        candidate_end,
                    );
                }
            }
        }
        following_offset = following_offset.saturating_add(1);
    }
}

fn push_verified_tail_candidate_end(
    candidate_ends: &mut Vec<usize>,
    offset: usize,
    scan_end: usize,
    candidate_end: usize,
) {
    if candidate_end > offset.saturating_add(LEGACY_APPEARANCE_HEADER_BYTES)
        && candidate_end <= scan_end
    {
        candidate_ends.push(candidate_end);
    }
}

fn find_verified_following_creature_update_offset_after_appearance(
    bytes: &[u8],
    appearance_end: usize,
    fragment_bits: &[bool],
    bit_cursor_after_appearance: usize,
) -> Option<usize> {
    if appearance_end >= bytes.len() {
        return None;
    }
    let search_limit = appearance_end
        .saturating_add(MAX_EE_APPEARANCE_TRAILING_LEGACY_TAIL_BYTES)
        .min(bytes.len());
    for following_offset in appearance_end..search_limit.saturating_sub(1) {
        if bytes.get(following_offset).copied()? != b'U'
            || bytes.get(following_offset + 1).copied()? != LEGACY_CREATURE_TYPE
        {
            continue;
        }
        // `U/5 0x3967` is deliberately not a transport/salvage boundary: its
        // decompiled reader consumes identity/action/object subfields under the
        // CNW fragment cursor.  Here we are no longer guessing from bytes alone:
        // the preceding appearance parser has already left an exact cursor, so
        // each candidate below is accepted only if the focused creature-update
        // simulator can consume the record from that cursor.
        let following_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            bytes,
            following_offset,
            bytes.len(),
        )
        .min(bytes.len());
        if following_end <= following_offset {
            continue;
        }

        let mut following_cursor = bit_cursor_after_appearance;
        let exact_ee_update =
            super::creature::advance_verified_noop_creature_update_record_exact_cursor(
                bytes,
                following_offset,
                following_end,
                fragment_bits,
                &mut following_cursor,
            );
        let legacy_rewriteable_update = if exact_ee_update {
            false
        } else {
            following_cursor = bit_cursor_after_appearance;
            super::creature::advance_verified_legacy_creature_update_record_for_span_owner(
                bytes,
                following_offset,
                following_end,
                fragment_bits,
                &mut following_cursor,
            )
        };
        let legacy_update_before_empty_fragment_span =
            if exact_ee_update || legacy_rewriteable_update {
                false
            } else if following_offset <= appearance_end {
                false
            } else {
                super::creature::legacy_creature_update_3967_read_end_before_fragment_span(
                    bytes,
                    following_offset,
                    following_end,
                    fragment_bits,
                    bit_cursor_after_appearance,
                    MAX_EE_APPEARANCE_FOLLOWING_CREATURE_UPDATE_EMPTY_FRAGMENT_SPAN_BYTES,
                )
                .is_some_and(|(read_end, _)| {
                    appearance_following_creature_update_trailing_span_is_empty_fragment_storage(
                        bytes.get(read_end..following_end).unwrap_or(&[]),
                    )
                })
            };
        if exact_ee_update || legacy_rewriteable_update || legacy_update_before_empty_fragment_span
        {
            return Some(following_offset);
        }
    }

    None
}

fn appearance_following_creature_update_trailing_span_is_empty_fragment_storage(
    span: &[u8],
) -> bool {
    if span.is_empty()
        || span.len() > MAX_EE_APPEARANCE_FOLLOWING_CREATURE_UPDATE_EMPTY_FRAGMENT_SPAN_BYTES
    {
        return false;
    }
    bits::decode_msb_valid_bits(span, CNW_FRAGMENT_HEADER_BITS).is_some_and(|decoded| {
        decoded
            .iter()
            .skip(CNW_FRAGMENT_HEADER_BITS)
            .all(|bit| !*bit)
    })
}

pub(super) fn remove_ee_appearance_trailing_legacy_tail_before_verified_creature_update_for_ee(
    bytes: &mut Vec<u8>,
    appearance_end: usize,
    fragment_bits: &[bool],
    bit_cursor_after_appearance: usize,
) -> Option<CreatureAppearanceTrailingTailRemoval> {
    let following_offset = find_verified_following_creature_update_offset_after_appearance(
        bytes,
        appearance_end,
        fragment_bits,
        bit_cursor_after_appearance,
    )?;
    let bytes_removed = following_offset.saturating_sub(appearance_end);
    bytes.drain(appearance_end..following_offset);
    Some(CreatureAppearanceTrailingTailRemoval { bytes_removed })
}

pub(super) fn is_verified_ee_creature_visual_transform_update_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let Some(visual_offset) =
        offset.checked_add(LEGACY_CREATURE_VISUAL_TRANSFORM_UPDATE_HEADER_BYTES)
    else {
        return false;
    };
    if record_end > bytes.len()
        || bytes.get(offset).copied() != Some(b'U')
        || bytes.get(offset + 1).copied() != Some(LEGACY_CREATURE_TYPE)
        || visual_offset.checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len())
            != Some(record_end)
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    looks_like_legacy_creature_object_id(object_id)
        && has_ee_object_visual_transform_identity_at(bytes, visual_offset, record_end)
}

pub(super) fn rewrite_creature_visual_transform_update_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureVisualTransformUpdateRewrite> {
    let selector_end = offset.checked_add(LEGACY_CREATURE_VISUAL_TRANSFORM_UPDATE_HEADER_BYTES)?;
    if *record_end > bytes.len()
        || bytes.get(offset).copied()? != b'U'
        || bytes.get(offset + 1).copied()? != LEGACY_CREATURE_TYPE
        || selector_end > *record_end
    {
        return None;
    }

    let object_id = read_u32_le(bytes, offset + 2)?;
    if !looks_like_legacy_creature_object_id(object_id) {
        return None;
    }

    if let Some(raw_mask) = read_u32_le(bytes, offset + 6) {
        if super::creature::is_supported_legacy_creature_update_cursor_mask(raw_mask) {
            // EE/Diamond creature update records (`sub_140781E80` /
            // `sub_44ADD0`) read a four-byte update mask immediately after
            // the object id. The legacy visual-transform selector branch reads
            // only one byte there. Do not reinterpret a decompile-supported
            // creature update mask such as `0x00000047`, `0x00003967`,
            // `0x0000C408`, or `0x00008000` as selector-plus-fragment data;
            // doing so leaves the EE live-object dispatcher with shifted
            // read/fragment cursors and produces `Unknown Update sub-message`.
            return None;
        }
    }

    if is_verified_ee_creature_visual_transform_update_record(bytes, offset, *record_end) {
        return None;
    }

    // Diamond's `sub_448E30` `U/5` branch reads only the object id and one
    // `ReadBYTE(8)` selector, then clears/applies the local visual effect.
    // EE's corresponding `sub_14077FE10` branch reads the same selector and
    // then calls `sub_140973160` for a visual-transform map. Because the 1.69
    // server has no transform bytes for this branch, the bridge emits the EE
    // object-level identity map: two zero DWORD counts. Any bytes the boundary
    // walker grouped after the selector are chunk-local CNW fragment storage,
    // not part of this semantic record, so promote them back into the fragment
    // stream before inserting the EE-only identity map.
    let old_record_end = *record_end;
    let mut bytes_removed = 0usize;
    let mut bits_inserted = 0usize;
    if old_record_end > selector_end {
        let span = bytes.get(selector_end..old_record_end)?;
        let mut promoted_bits = bits::decode_msb_valid_bits(span, CNW_FRAGMENT_HEADER_BITS)?;
        if promoted_bits.len() < CNW_FRAGMENT_HEADER_BITS {
            return None;
        }
        promoted_bits.drain(0..CNW_FRAGMENT_HEADER_BITS);
        bits::insert_msb_bits(fragment_bits, bit_cursor, &promoted_bits)?;
        bits_inserted = promoted_bits.len();
        bytes_removed = old_record_end.saturating_sub(selector_end);
        bytes.drain(selector_end..old_record_end);
        *record_end = selector_end;
    }

    bytes.splice(
        selector_end..selector_end,
        EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
    );
    *record_end = (*record_end).checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;

    Some(CreatureVisualTransformUpdateRewrite {
        bytes_inserted: EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len(),
        bytes_removed,
        bits_inserted,
    })
}

pub(super) fn looks_like_legacy_item_add_record_boundary(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset).copied() == Some(b'A')
        && read_u32_le(bytes, offset.saturating_add(1))
            .map(looks_like_creature_or_legacy_sentinel_id)
            .unwrap_or(false)
        && read_u32_le(bytes, offset.saturating_add(5))
            .map(is_legacy_visible_equipment_slot)
            .unwrap_or(false)
}

pub(super) fn advance_verified_ee_item_add_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    for record in parse_legacy_item_add_record_candidates(bytes, offset, record_end) {
        let Some(ee_bits) = record
            .fragment_bits_consumed
            .checked_add(record.ee_extra_fragment_bits)
        else {
            continue;
        };
        if !record
            .name_fragment_proof
            .matches(fragment_bits, *bit_cursor)
        {
            continue;
        }
        if record
            .ee_extra_insert_offsets
            .iter()
            .copied()
            .any(|relative_offset| {
                bit_cursor
                    .checked_add(relative_offset)
                    .and_then(|index| fragment_bits.get(index))
                    .copied()
                    .unwrap_or(true)
            })
        {
            continue;
        }
        if !record.ee_extra_byte_inserts.is_empty()
            || ee_bits > fragment_bits.len().saturating_sub(*bit_cursor)
        {
            continue;
        }
        *bit_cursor = bit_cursor.saturating_add(ee_bits);
        return true;
    }
    false
}

pub(super) fn insert_ee_item_add_extras_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureAppearanceExtraRewrite> {
    let record = parse_legacy_item_add_record(bytes, offset, *record_end)?;
    if record.fragment_bits_consumed > fragment_bits.len().saturating_sub(bit_cursor)
        || !record
            .name_fragment_proof
            .matches(fragment_bits, bit_cursor)
    {
        return None;
    }

    // Top-level item adds use the same Diamond writer as visible equipment:
    // `WriteGameObjUpdate_WriteInventorySlotAdd` writes `A`, object id, slot,
    // then the item object. EE's item reader reaches `sub_14079FAC0` and
    // `sub_14076BD30` for armor-shaped item payloads, so the exact rewrite is
    // identical to the nested visible-equipment item-add case: insert the
    // EE object identity visual map in the read buffer and insert the
    // EE-only active-property BOOL in the CNW fragment stream.
    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        bits::insert_msb_bit(
            fragment_bits,
            bit_cursor
                .checked_add(relative_offset)?
                .checked_add(inserted)?,
            false,
        )?;
    }

    let byte_apply = apply_creature_appearance_byte_inserts(
        bytes,
        offset,
        record_end,
        &record.ee_extra_byte_inserts,
    )?;

    Some(CreatureAppearanceExtraRewrite {
        bits_inserted: record.ee_extra_insert_offsets.len(),
        bits_removed: 0,
        bytes_inserted: byte_apply.bytes_inserted,
        bytes_removed: byte_apply.bytes_removed,
    })
}

pub(super) fn try_get_legacy_item_create_record_end(
    bytes: &[u8],
    item_object_offset: usize,
    search_end: usize,
) -> Option<usize> {
    let scan_end = search_end
        .min(bytes.len())
        .min(item_object_offset.checked_add(4 + LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)?);
    let min_end = item_object_offset.checked_add(4)?.checked_add(1)?;
    if min_end > scan_end {
        return None;
    }

    for record_end in min_end..=scan_end {
        if parse_legacy_item_create_record(bytes, item_object_offset, record_end).is_some() {
            return Some(record_end);
        }
    }
    None
}

pub(super) fn try_get_legacy_item_create_record_end_with_fragment_proof(
    bytes: &[u8],
    item_object_offset: usize,
    search_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    // GUI inventory/repository item-create rows are lengthless in the read
    // buffer: the Diamond and EE handlers both consume the GUI prefix, then call
    // the same item object helper used by top-level item adds. A byte-only scan
    // can therefore stop at a plausible active-property tail inside a later
    // CExoString. When the live-object pass has the current CNW fragment cursor,
    // prefer an endpoint that also proves the decompiled item-name branch and
    // lands on the next verified live-object boundary.
    let scan_end = search_end
        .min(bytes.len())
        .min(item_object_offset.checked_add(4 + LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)?);
    let min_end = item_object_offset.checked_add(4)?.checked_add(1)?;
    if min_end > scan_end {
        return None;
    }

    for record_end in min_end..=scan_end {
        if !item_create_record_end_lands_on_stream_boundary(bytes, record_end, search_end) {
            continue;
        }
        let mut matching_records =
            parse_legacy_item_create_record_candidates(bytes, item_object_offset, record_end)
                .into_iter()
                .filter(|record| {
                    record.fragment_bits_consumed <= fragment_bits.len().saturating_sub(bit_cursor)
                        && record
                            .name_fragment_proof
                            .matches(fragment_bits, bit_cursor)
                });
        let Some(_record) = matching_records.next() else {
            continue;
        };
        if matching_records.next().is_some() {
            continue;
        }
        return Some(record_end);
    }
    None
}

pub(super) fn try_get_verified_ee_item_create_record_end(
    bytes: &[u8],
    item_object_offset: usize,
    search_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let scan_end = search_end
        .min(bytes.len())
        .min(item_object_offset.checked_add(4 + LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)?);
    let min_end = item_object_offset.checked_add(4)?.checked_add(1)?;
    if min_end > scan_end {
        return None;
    }

    for record_end in min_end..=scan_end {
        if !item_create_record_end_lands_on_stream_boundary(bytes, record_end, search_end) {
            continue;
        }
        let mut probe_cursor = bit_cursor;
        if advance_verified_ee_item_create_record(
            bytes,
            item_object_offset,
            record_end,
            fragment_bits,
            &mut probe_cursor,
        ) {
            return Some(record_end);
        }
    }
    None
}

pub(super) fn try_get_legacy_gui_item_create_record_end(
    bytes: &[u8],
    item_object_offset: usize,
    search_end: usize,
    allow_missing_inventory_add_opcode: bool,
) -> Option<usize> {
    let scan_end = search_end
        .min(bytes.len())
        .min(item_object_offset.checked_add(4 + LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)?);
    let min_end = item_object_offset.checked_add(4)?.checked_add(1)?;
    if min_end > scan_end {
        return None;
    }

    for record_end in min_end..=scan_end {
        if !gui_item_create_record_end_lands_on_stream_boundary(
            bytes,
            record_end,
            search_end,
            allow_missing_inventory_add_opcode,
        ) {
            continue;
        }
        if parse_legacy_item_create_record(bytes, item_object_offset, record_end).is_some() {
            return Some(record_end);
        }
    }
    None
}

pub(super) fn try_get_legacy_gui_item_create_record_end_with_fragment_proof(
    bytes: &[u8],
    item_object_offset: usize,
    search_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    // GUI inventory/repository item-create rows are embedded in a stream of
    // sibling GUI rows. A generic live-object boundary is too broad here: the
    // item body can contain byte patterns that resemble unrelated live-object
    // records. For this decompile-backed GUI path, only a sibling GUI item row,
    // a proven work-remaining `W` row, or the declared stream end proves the row
    // boundary.
    let scan_end = search_end
        .min(bytes.len())
        .min(item_object_offset.checked_add(4 + LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)?);
    let min_end = item_object_offset.checked_add(4)?.checked_add(1)?;
    if min_end > scan_end {
        return None;
    }

    for record_end in min_end..=scan_end {
        if !gui_item_create_record_end_lands_on_stream_boundary(bytes, record_end, search_end, true)
        {
            continue;
        }
        let debug = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();
        let mut matching_records =
            parse_legacy_item_create_record_candidates(bytes, item_object_offset, record_end)
                .into_iter()
                .filter(|record| {
                    let matches = record.fragment_bits_consumed
                        <= fragment_bits.len().saturating_sub(bit_cursor)
                        && record
                            .name_fragment_proof
                            .matches(fragment_bits, bit_cursor);
                    if debug {
                        eprintln!(
                            "live-object legacy GUI item-create endpoint candidate: item_object_offset={item_object_offset} record_end={record_end} bit_cursor={bit_cursor} record_bits={} ee_extra_bits={} proof={:?} matches={matches}",
                            record.fragment_bits_consumed,
                            record.ee_extra_fragment_bits,
                            record.name_fragment_proof
                        );
                    }
                    matches
                });
        let Some(_record) = matching_records.next() else {
            continue;
        };
        if matching_records.next().is_some() {
            continue;
        }
        return Some(record_end);
    }
    None
}

pub(super) fn advance_legacy_gui_item_create_record(
    bytes: &[u8],
    item_object_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    // Transport declared-window repair uses this before the bytes have been
    // rewritten to EE shape. Diamond `sub_451680` and EE `sub_14079FAC0` both
    // read the item-create selector and active-property BOOLs from the CNW
    // fragment stream; the source cursor must therefore be proven from the
    // legacy record model, not guessed from how many bytes remain before the
    // next GUI sibling row.
    let debug = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();
    let mut matching_records =
        parse_legacy_item_create_record_candidates(bytes, item_object_offset, record_end)
            .into_iter()
            .filter(|record| {
                let matches = record.fragment_bits_consumed
                    <= fragment_bits.len().saturating_sub(*bit_cursor)
                    && record
                        .name_fragment_proof
                        .matches(fragment_bits, *bit_cursor);
                if debug {
                    eprintln!(
                        "live-object legacy GUI item-create candidate: item_object_offset={item_object_offset} record_end={record_end} bit_cursor={} record_bits={} ee_extra_bits={} proof={:?} matches={matches}",
                        *bit_cursor,
                        record.fragment_bits_consumed,
                        record.ee_extra_fragment_bits,
                        record.name_fragment_proof
                    );
                }
                matches
            });
    let Some(record) = matching_records.next() else {
        return false;
    };
    if matching_records.next().is_some() {
        return false;
    }
    let Some(next_cursor) = bit_cursor.checked_add(record.fragment_bits_consumed) else {
        return false;
    };
    *bit_cursor = next_cursor;
    true
}

pub(super) fn try_get_verified_ee_gui_item_create_record_end(
    bytes: &[u8],
    item_object_offset: usize,
    search_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let scan_end = search_end
        .min(bytes.len())
        .min(item_object_offset.checked_add(4 + LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)?);
    let min_end = item_object_offset.checked_add(4)?.checked_add(1)?;
    if min_end > scan_end {
        return None;
    }

    for record_end in min_end..=scan_end {
        if !gui_item_create_record_end_lands_on_stream_boundary(
            bytes, record_end, search_end, false,
        ) {
            continue;
        }
        let mut probe_cursor = bit_cursor;
        if advance_verified_ee_item_create_record(
            bytes,
            item_object_offset,
            record_end,
            fragment_bits,
            &mut probe_cursor,
        ) {
            return Some(record_end);
        }
    }
    None
}

pub(super) fn advance_verified_ee_item_create_record(
    bytes: &[u8],
    item_object_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let debug = std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some();
    for record in parse_legacy_item_create_record_candidates(bytes, item_object_offset, record_end)
    {
        let Some(ee_bits) = record
            .fragment_bits_consumed
            .checked_add(record.ee_extra_fragment_bits)
        else {
            if debug {
                eprintln!(
                    "live-object item-create exact reject: reason=bits-overflow item_object_offset={item_object_offset} record_end={record_end}"
                );
            }
            continue;
        };
        if !record
            .name_fragment_proof
            .matches(fragment_bits, *bit_cursor)
        {
            if debug {
                eprintln!(
                    "live-object item-create exact reject: reason=name-proof item_object_offset={item_object_offset} record_end={record_end} bit_cursor={} record_bits={} ee_extra_bits={} proof={:?}",
                    *bit_cursor,
                    record.fragment_bits_consumed,
                    record.ee_extra_fragment_bits,
                    record.name_fragment_proof
                );
            }
            continue;
        }
        if !record.ee_extra_byte_inserts.is_empty()
            || ee_bits > fragment_bits.len().saturating_sub(*bit_cursor)
        {
            if debug {
                eprintln!(
                    "live-object item-create exact reject: reason=shape-or-bits item_object_offset={item_object_offset} record_end={record_end} bit_cursor={} ee_bits={ee_bits} remaining_bits={} byte_inserts={}",
                    *bit_cursor,
                    fragment_bits.len().saturating_sub(*bit_cursor),
                    record.ee_extra_byte_inserts.len()
                );
            }
            continue;
        }
        if record
            .ee_extra_insert_offsets
            .iter()
            .copied()
            .any(|relative_offset| {
                bit_cursor
                    .checked_add(relative_offset)
                    .and_then(|index| fragment_bits.get(index))
                    .copied()
                    .unwrap_or(true)
            })
        {
            if debug {
                eprintln!(
                    "live-object item-create exact reject: reason=missing-inserted-bits item_object_offset={item_object_offset} record_end={record_end} bit_cursor={} insert_offsets={:?}",
                    *bit_cursor, record.ee_extra_insert_offsets
                );
            }
            continue;
        }
        *bit_cursor = bit_cursor.saturating_add(ee_bits);
        return true;
    }
    false
}

fn item_create_record_end_lands_on_stream_boundary(
    bytes: &[u8],
    record_end: usize,
    search_end: usize,
) -> bool {
    let scan_end = search_end.min(bytes.len());
    record_end == scan_end
        || (record_end < scan_end
            && (boundary::looks_like_legacy_live_object_sub_message_boundary(bytes, record_end)
                || looks_like_gui_item_create_prefix_at(bytes, record_end)))
}

fn gui_item_create_record_end_lands_on_stream_boundary(
    bytes: &[u8],
    record_end: usize,
    search_end: usize,
    allow_missing_inventory_add_opcode: bool,
) -> bool {
    let scan_end = search_end.min(bytes.len());
    record_end == scan_end
        || (record_end < scan_end
            && looks_like_gui_item_create_prefix_at_with_policy(
                bytes,
                record_end,
                allow_missing_inventory_add_opcode,
            ))
        || looks_like_work_remaining_boundary_at(bytes, record_end, scan_end)
        || (allow_missing_inventory_add_opcode
            && looks_like_zero_fragment_storage_before_gui_item_boundary_at(
                bytes,
                record_end,
                scan_end,
                allow_missing_inventory_add_opcode,
            ))
}

fn looks_like_work_remaining_boundary_at(bytes: &[u8], offset: usize, scan_end: usize) -> bool {
    offset
        .checked_add(WORK_REMAINING_RECORD_BYTES)
        .is_some_and(|end| end <= scan_end)
        && bytes.get(offset).copied() == Some(WORK_REMAINING_OPCODE)
}

fn looks_like_zero_fragment_storage_before_gui_item_boundary_at(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    allow_missing_inventory_add_opcode: bool,
) -> bool {
    if offset >= scan_end || offset >= bytes.len() {
        return false;
    }
    let max_end = offset
        .saturating_add(GUI_ZERO_FRAGMENT_STORAGE_MAX_BYTES)
        .min(scan_end)
        .min(bytes.len());
    for span_end in offset + 1..=max_end {
        if !looks_like_gui_item_create_prefix_at_with_policy(
            bytes,
            span_end,
            allow_missing_inventory_add_opcode,
        ) {
            continue;
        }
        let Some(decoded_bits) =
            bits::decode_msb_valid_bits(bytes.get(offset..span_end).unwrap_or(&[]), 3)
        else {
            continue;
        };
        if decoded_bits.iter().skip(3).all(|bit| !*bit) {
            return true;
        }
    }
    false
}

fn looks_like_gui_item_create_prefix_at(bytes: &[u8], offset: usize) -> bool {
    looks_like_gui_item_create_prefix_at_with_policy(bytes, offset, false)
}

fn looks_like_gui_item_create_prefix_at_with_policy(
    bytes: &[u8],
    offset: usize,
    allow_missing_inventory_add_opcode: bool,
) -> bool {
    if offset.checked_add(3).unwrap_or(usize::MAX) > bytes.len()
        || bytes.get(offset).copied() != Some(b'G')
    {
        return false;
    }

    let inner_opcode = bytes[offset + 2];
    let item_object_offset = match bytes[offset + 1] {
        b'I' | b'i'
            if inner_opcode == b'A'
                || (allow_missing_inventory_add_opcode && inner_opcode == 0x00) =>
        {
            offset.checked_add(7)
        }
        b'R' | b'r' if inner_opcode == b'A' => offset.checked_add(5),
        _ => None,
    };
    let Some(item_object_offset) = item_object_offset else {
        return false;
    };
    read_u32_le(bytes, item_object_offset)
        .map(|object_id| {
            object_id == 0x7F00_0000
                || object_id == u32::MAX
                || boundary::looks_like_legacy_live_object_id_value(object_id)
        })
        .unwrap_or(false)
}

pub(super) fn insert_ee_item_create_extras_for_ee(
    bytes: &mut Vec<u8>,
    item_object_offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureAppearanceExtraRewrite> {
    let mut matching_records =
        parse_legacy_item_create_record_candidates(bytes, item_object_offset, *record_end)
            .into_iter()
            .filter(|record| {
                record.fragment_bits_consumed <= fragment_bits.len().saturating_sub(bit_cursor)
                    && record
                        .name_fragment_proof
                        .matches(fragment_bits, bit_cursor)
            });
    let record = matching_records.next()?;
    if matching_records.next().is_some() {
        return None;
    }

    // GUI inventory/repository item-create rows call the same EE helper as
    // top-level item adds after their GUI prefix:
    //
    //   item object id -> item appearance -> active item properties
    //
    // Keep the transformation here so the GUI module only owns row framing,
    // while the decompile-owned item appearance/active-property deltas stay
    // with the other item-create translators.
    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        bits::insert_msb_bit(
            fragment_bits,
            bit_cursor
                .checked_add(relative_offset)?
                .checked_add(inserted)?,
            false,
        )?;
    }

    let byte_apply = apply_creature_appearance_byte_inserts(
        bytes,
        item_object_offset,
        record_end,
        &record.ee_extra_byte_inserts,
    )?;

    Some(CreatureAppearanceExtraRewrite {
        bits_inserted: record.ee_extra_insert_offsets.len(),
        bits_removed: 0,
        bytes_inserted: byte_apply.bytes_inserted,
        bytes_removed: byte_apply.bytes_removed,
    })
}

pub(super) fn insert_ee_creature_appearance_extras_for_ee(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureAppearanceExtraRewrite> {
    let original_bytes = bytes.clone();
    let original_fragment_bits = fragment_bits.clone();
    let original_record_end = *record_end;
    if let Some(existing_ee_end) =
        try_get_ee_creature_appearance_record_end_by_byte_shape(bytes, offset, bytes.len())
    {
        if existing_ee_end > *record_end {
            // A previous structured pass may already have inserted EE's
            // visible-equipment byte-only subobjects, while the generic legacy
            // boundary finder still stops at the old Diamond active-tail
            // offset. Extend the record to the proven EE byte shape instead of
            // inserting a second armor table / visual-transform block. Fragment
            // validation still happens immediately after this function returns.
            *record_end = existing_ee_end;
        }
    }
    let name_shape = read_appearance_name_shape(fragment_bits, bit_cursor)?;
    let leading_fence = LEGACY_FULL_APPEARANCE_PRECEDING_FRAGMENT_FENCE_CANDIDATES
        .iter()
        .copied()
        .filter(|fence_bits| *fence_bits != 0)
        .find_map(|fence_bits| {
            if !legacy_full_appearance_preceding_fence_bits_are_proven(
                fragment_bits,
                bit_cursor,
                fence_bits,
            ) {
                return None;
            }
            let cursor = bit_cursor.checked_add(fence_bits)?;
            let shape = read_appearance_name_shape(fragment_bits, cursor)?;
            Some((fence_bits, cursor, shape))
        });
    let mut repaired_name_shape = None;
    let mut record_name_shape = name_shape;
    let mut record_from_fragment_proof = false;
    let mut appearance_bit_cursor = bit_cursor;
    let proof = AppearanceBitProof {
        bit_cursor,
        fragment_bits,
        translated_ee: false,
        allow_cross_record_fence: false,
        owner_offset: offset,
    };
    let parse_exact_record = |shape| {
        let record = parse_creature_appearance_record(
            bytes,
            offset,
            *record_end,
            shape,
            CreatureAppearanceWireDialect::LegacyDiamond,
            Some(proof),
        )?;
        if record.record_end != *record_end
            || record.fragment_bits_consumed > fragment_bits.len().saturating_sub(bit_cursor)
        {
            return None;
        }
        Some(record)
    };
    let parse_exact_byte_record_without_fragment_proof = |shape| {
        // Full `P/5` appearances are byte-delimited at `*record_end`, but the
        // decompiled reader can still prove a packetized fragment fence owned
        // by the immediately following `U/5` record.  Keep the surrounding
        // live-object stream visible to the byte-only fallback parser, then
        // require the read-buffer cursor to land exactly on `*record_end`.
        // Clipping `limit` to `*record_end` makes the same byte record look
        // valid while under-counting its fragment bits, which later places EE's
        // active-property BOOL inside the following record's fragment proof.
        let record = parse_creature_appearance_record(
            bytes,
            offset,
            bytes.len(),
            shape,
            CreatureAppearanceWireDialect::LegacyDiamond,
            None,
        )?;
        if record.record_end != *record_end
            || record.fragment_bits_consumed > fragment_bits.len().saturating_sub(bit_cursor)
        {
            return None;
        }
        Some(record)
    };
    let verified_preceding_fence_record =
        parse_verified_creature_appearance_with_optional_preceding_fence(
            bytes,
            offset,
            *record_end,
            fragment_bits,
            bit_cursor,
            Some(*record_end),
            false,
            false,
        )
        .filter(|verified| {
            verified.record.record_end == *record_end
                && verified.record.fragment_bits_consumed
                    <= fragment_bits.len().saturating_sub(verified.proof_cursor)
        });
    let mut record = verified_preceding_fence_record
        .map(|verified| {
            appearance_bit_cursor = verified.proof_cursor;
            record_from_fragment_proof = true;
            record_name_shape = read_appearance_name_shape(fragment_bits, verified.proof_cursor)
                .unwrap_or(name_shape);
            verified.record
        })
        .or_else(|| {
            parse_exact_record(name_shape).inspect(|_| {
                record_from_fragment_proof = true;
            })
        })
        .or_else(|| {
            let alternate = name_shape.alternate();
            let record = parse_exact_record(alternate)?;
            record_from_fragment_proof = true;
            repaired_name_shape = Some(alternate);
            record_name_shape = alternate;
            Some(record)
        })
        .or_else(|| {
            let (_, cursor, fenced) = leading_fence?;
            let record = parse_exact_byte_record_without_fragment_proof(fenced)?;
            appearance_bit_cursor = cursor;
            record_name_shape = fenced;
            Some(record)
        })
        .or_else(|| {
            let record = parse_exact_byte_record_without_fragment_proof(name_shape)?;
            record_name_shape = name_shape;
            Some(record)
        })
        .or_else(|| {
            let alternate = name_shape.alternate();
            let record = parse_exact_byte_record_without_fragment_proof(alternate)?;
            repaired_name_shape = Some(alternate);
            record_name_shape = alternate;
            Some(record)
        })?;
    if let Some(repaired) = repaired_name_shape {
        // Diamond and EE both branch on this BOOL: false reads one direct
        // CExoString, true reads two locstring helpers. If the current cursor's
        // bit selects an impossible branch but the alternate branch consumes
        // the full decompiled appearance record and proves the following
        // creature update, translate the bit to match the byte shape instead of
        // forwarding a raw overflowing `P` record.
        *fragment_bits.get_mut(appearance_bit_cursor)? = repaired.fragment_bit();
    }
    if !record_from_fragment_proof && record_name_shape == AppearanceNameShape::LocStringPair {
        // Diamond/HG creature appearances can byte-prove the two-name branch
        // even when the incoming fragment cursor advertises the single-string
        // branch. EE then routes each component through the locstring helper,
        // whose inner BOOL must select the inline CExoString path for this
        // decompile-backed HG wire shape.
        *fragment_bits.get_mut(appearance_bit_cursor.checked_add(1)?)? = false;
        *fragment_bits.get_mut(appearance_bit_cursor.checked_add(2)?)? = false;
    }
    let byte_only_stream_padding_probe = record_from_fragment_proof
        && record.ee_extra_insert_offsets.is_empty()
        && !record.ee_extra_byte_inserts.is_empty()
        && bytes.get(*record_end).copied() == Some(b'U')
        && bytes.get((*record_end).saturating_add(1)).copied() == Some(LEGACY_CREATURE_TYPE);
    if !record_from_fragment_proof || byte_only_stream_padding_probe {
        if let Some(delta) = proven_name_fragment_delta_for_byte_only_appearance_parse(
            record_name_shape,
            fragment_bits,
            appearance_bit_cursor,
        ) {
            apply_name_fragment_delta_to_appearance_record(&mut record, delta)?;
        }
    }
    let mut bits_removed = 0usize;
    if debug_live_claim_enabled_for_offset(offset) {
        let bit_window = fragment_bits
            .get(bit_cursor..bit_cursor.saturating_add(24).min(fragment_bits.len()))
            .unwrap_or(&[]);
        eprintln!(
            "live-object appearance rewrite model: offset={offset} source_bit_cursor={bit_cursor} appearance_bit_cursor={appearance_bit_cursor} from_fragment_proof={record_from_fragment_proof} record_end={} legacy_bits={} ee_bits={} ee_bit_inserts={:?} ee_byte_inserts={:?} bit_window={:?}",
            record.record_end,
            record.fragment_bits_consumed,
            record.ee_fragment_bits_consumed,
            record.ee_extra_insert_offsets,
            record.ee_extra_byte_inserts,
            bit_window,
        );
    }
    // Byte-only EE dialect repairs, such as build-0x23 creature/body-part WORD
    // widening, can still expose chunk-local zero fragment storage before the
    // next live-object record. The padding search decides whether anything must
    // be removed; for byte-only records it requires the following record to
    // validate from the same trial cursor before accepting a non-empty removal.
    let byte_only_stream_padding_probe = record_from_fragment_proof
        && record.ee_extra_insert_offsets.is_empty()
        && !record.ee_extra_byte_inserts.is_empty();
    if !record_from_fragment_proof || byte_only_stream_padding_probe {
        let effective_name_shape = record_name_shape;
        let mut minimum_padding_start = (match effective_name_shape {
            AppearanceNameShape::LocStringPair => 3,
            AppearanceNameShape::CExoString => 1,
        } as usize)
            .saturating_add(0);
        if let Some(preferred_start) = record.preferred_zero_padding_relative_start {
            minimum_padding_start = minimum_padding_start.max(preferred_start);
        }
        let token_selector_padding_repair_relative_start =
            record.token_selector_padding_repair_relative_start;
        let inline_active_name_fence_repair_relative_start =
            record.inline_active_name_fence_repair_relative_start;
        let removal = find_zero_fragment_padding_removal_for_ee_appearance(
            bytes,
            offset,
            *record_end,
            fragment_bits,
            appearance_bit_cursor,
            &record,
            minimum_padding_start,
            token_selector_padding_repair_relative_start,
            inline_active_name_fence_repair_relative_start,
        );
        if let Some(removal) = removal {
            for range in removal.ranges.iter().rev() {
                let absolute_start = appearance_bit_cursor.checked_add(range.relative_start)?;
                fragment_bits.drain(absolute_start..absolute_start.checked_add(range.count)?);
                bits_removed = bits_removed.checked_add(range.count)?;
            }
            if debug_live_claim_enabled_for_offset(offset) {
                eprintln!(
                    "live-object appearance zero fragment padding removed: offset={offset} ranges={:?} record_end={}",
                    removal.ranges, *record_end
                );
            }
        } else if !record_from_fragment_proof
            // EE build 8193's creature/body-part widening is read-buffer-only:
            // the decompiled reader consumes the same CNW fragment bits before
            // and after the byte expansion. Let that transactional writer reach
            // the exact EE validator below instead of requiring unrelated
            // fragment-padding removal as proof.
            && !(record.ee_extra_insert_offsets.is_empty()
                && !record.ee_extra_byte_inserts.is_empty()
                && record.fragment_bits_consumed == record.ee_fragment_bits_consumed)
        {
            return None;
        }
    }
    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        super::bits::insert_msb_bit(
            fragment_bits,
            appearance_bit_cursor
                .checked_add(relative_offset)?
                .checked_add(inserted)?,
            false,
        )?;
    }

    let Some(byte_apply) = apply_creature_appearance_byte_inserts(
        bytes,
        offset,
        record_end,
        &record.ee_extra_byte_inserts,
    ) else {
        *bytes = original_bytes;
        *fragment_bits = original_fragment_bits;
        *record_end = original_record_end;
        return None;
    };

    let mut bytes_removed = byte_apply.bytes_removed;
    let mut proof_cursor = bit_cursor;
    if !advance_verified_ee_creature_appearance_record(
        bytes,
        offset,
        *record_end,
        fragment_bits,
        &mut proof_cursor,
    ) {
        if let Some(translated_end) =
            try_get_ee_creature_appearance_record_end_before_verified_creature_update_tail_for_ee(
                bytes,
                offset,
                *record_end,
                fragment_bits,
                bit_cursor,
            )
        {
            let mut translated_cursor = bit_cursor;
            if advance_verified_ee_creature_appearance_record(
                bytes,
                offset,
                translated_end,
                fragment_bits,
                &mut translated_cursor,
            ) {
                // Diamond's full appearance writer can leave legacy-only
                // read-buffer tail bytes immediately before the following
                // creature update after the EE body/item widening is applied.
                // This is not a generic truncation: the tail is removed only
                // when the EE appearance validates at the shorter end and the
                // following `U/5` record is verified from the same fragment
                // cursor.
                if let Some(removal) =
                    remove_ee_appearance_trailing_legacy_tail_before_verified_creature_update_for_ee(
                        bytes,
                        translated_end,
                        fragment_bits,
                        translated_cursor,
                    )
                {
                    *record_end = translated_end;
                    bytes_removed = bytes_removed.checked_add(removal.bytes_removed)?;
                    proof_cursor = bit_cursor;
                    if advance_verified_ee_creature_appearance_record(
                        bytes,
                        offset,
                        *record_end,
                        fragment_bits,
                        &mut proof_cursor,
                    ) {
                        return Some(CreatureAppearanceExtraRewrite {
                            bits_inserted: record.ee_extra_insert_offsets.len(),
                            bits_removed,
                            bytes_inserted: byte_apply.bytes_inserted,
                            bytes_removed,
                        });
                    }
                }
            }
        }
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object appearance transactional rewrite rejected: offset={offset} record_end={} bits_inserted={} bytes_inserted={} bytes_removed={}",
                *record_end,
                record.ee_extra_insert_offsets.len(),
                byte_apply.bytes_inserted,
                bytes_removed,
            );
        }
        *bytes = original_bytes;
        *fragment_bits = original_fragment_bits;
        *record_end = original_record_end;
        return None;
    }

    Some(CreatureAppearanceExtraRewrite {
        bits_inserted: record.ee_extra_insert_offsets.len(),
        bits_removed,
        bytes_inserted: byte_apply.bytes_inserted,
        bytes_removed,
    })
}

pub(super) fn remove_ee_creature_appearance_zero_fragment_padding_if_possible(
    bytes: &[u8],
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureAppearanceExtraRewrite> {
    let original_fragment_bits = fragment_bits.clone();
    let original_record_end = *record_end;
    let existing_ee_end =
        try_get_ee_creature_appearance_record_end_by_byte_shape(bytes, offset, bytes.len())?;
    if debug_live_claim_enabled_for_offset(offset) {
        eprintln!(
            "live-object EE appearance name bit repair probe: offset={offset} original_record_end={original_record_end} existing_ee_end={existing_ee_end} bit_cursor={bit_cursor}"
        );
    }
    if existing_ee_end > *record_end {
        *record_end = existing_ee_end;
    }

    let mut exact_cursor = bit_cursor;
    if advance_verified_ee_creature_appearance_record(
        bytes,
        offset,
        *record_end,
        fragment_bits,
        &mut exact_cursor,
    ) {
        return Some(CreatureAppearanceExtraRewrite::default());
    }

    let name_shape = read_appearance_name_shape(fragment_bits, bit_cursor)?;
    let record = parse_creature_appearance_record(
        bytes,
        offset,
        *record_end,
        name_shape,
        CreatureAppearanceWireDialect::EeBuild8193,
        None,
    )?;
    if record.record_end != *record_end || !record.ee_extra_byte_inserts.is_empty() {
        *record_end = original_record_end;
        return None;
    }

    // This is the already-EE counterpart to the Diamond-to-EE appearance
    // rewrite above. EE `sub_14077FE10` still consumes only the creature-name
    // selector and visible-equipment active-property BOOLs from the CNW
    // fragment stream; all full appearance body/equipment bytes are read-buffer
    // data. When a prior structured pass has already emitted the EE byte shape,
    // a later pass may still see promoted chunk-local zero storage before the
    // first visible-equipment item-name selector. Remove those zeros only when
    // the exact EE appearance validator, and for byte-only records the following
    // creature-update stream proof, accepts the rewritten fragment cursor.
    //
    let mut minimum_padding_start = match name_shape {
        AppearanceNameShape::LocStringPair => 3,
        AppearanceNameShape::CExoString => 1,
    };
    if let Some(preferred_start) = record.preferred_zero_padding_relative_start {
        minimum_padding_start = minimum_padding_start.max(preferred_start);
    }

    let mut padding_only_record = record.clone();
    // The parser reports EE-only active-property BOOL positions as
    // `ee_extra_insert_offsets` because the same typed model is also used for
    // Diamond input. Some already-EE byte-shaped records already have those BOOL
    // bits in the fragment stream; try that padding-only normalization first.
    padding_only_record.ee_extra_insert_offsets.clear();
    let mut candidates = vec![padding_only_record];
    if !record.ee_extra_insert_offsets.is_empty() {
        // Other already-EE byte-shaped records got their read-buffer promotion in
        // an earlier pass, but still carry Diamond's fragment shape. In that
        // case the decompiled EE active-property reader still needs the
        // EE-only BOOL inserted even though no byte insertion remains.
        candidates.push(record);
    }

    for candidate_record in candidates {
        *fragment_bits = original_fragment_bits.clone();
        *record_end = existing_ee_end;
        let Some(removal) = find_zero_fragment_padding_removal_for_ee_appearance(
            bytes,
            offset,
            *record_end,
            fragment_bits,
            bit_cursor,
            &candidate_record,
            minimum_padding_start,
            candidate_record.token_selector_padding_repair_relative_start,
            candidate_record.inline_active_name_fence_repair_relative_start,
        ) else {
            continue;
        };

        let mut bits_removed = 0usize;
        for range in removal.ranges.iter().rev() {
            let absolute_start = bit_cursor.checked_add(range.relative_start)?;
            fragment_bits.drain(absolute_start..absolute_start.checked_add(range.count)?);
            bits_removed = bits_removed.checked_add(range.count)?;
        }

        for (inserted, relative_offset) in candidate_record
            .ee_extra_insert_offsets
            .iter()
            .copied()
            .enumerate()
        {
            super::bits::insert_msb_bit(
                fragment_bits,
                bit_cursor
                    .checked_add(relative_offset)?
                    .checked_add(inserted)?,
                false,
            )?;
        }

        let mut proof_cursor = bit_cursor;
        if advance_verified_ee_creature_appearance_record(
            bytes,
            offset,
            *record_end,
            fragment_bits,
            &mut proof_cursor,
        ) {
            return Some(CreatureAppearanceExtraRewrite {
                bits_inserted: candidate_record.ee_extra_insert_offsets.len(),
                bits_removed,
                bytes_inserted: 0,
                bytes_removed: 0,
            });
        }
    }

    *fragment_bits = original_fragment_bits;
    *record_end = original_record_end;
    None
}

pub(super) fn repair_ee_creature_appearance_name_bits_if_possible(
    bytes: &[u8],
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureAppearanceExtraRewrite> {
    let original_fragment_bits = fragment_bits.clone();
    let original_record_end = *record_end;
    let existing_ee_end =
        try_get_ee_creature_appearance_record_end_by_byte_shape(bytes, offset, bytes.len())?;
    if existing_ee_end > *record_end {
        *record_end = existing_ee_end;
    }

    let mut exact_cursor = bit_cursor;
    if advance_verified_ee_creature_appearance_record(
        bytes,
        offset,
        *record_end,
        fragment_bits,
        &mut exact_cursor,
    ) {
        return None;
    }

    let candidate_fences = LEGACY_FULL_APPEARANCE_PRECEDING_FRAGMENT_FENCE_CANDIDATES;
    for preceding_fence_bits in candidate_fences {
        if preceding_fence_bits != 0
            && !legacy_full_appearance_preceding_fence_bits_are_proven(
                fragment_bits,
                bit_cursor,
                preceding_fence_bits,
            )
        {
            continue;
        }
        let Some(appearance_bit_cursor) = bit_cursor.checked_add(preceding_fence_bits) else {
            continue;
        };
        let Some(byte_record) = parse_creature_appearance_record(
            bytes,
            offset,
            *record_end,
            AppearanceNameShape::LocStringPair,
            CreatureAppearanceWireDialect::EeBuild8193,
            None,
        ) else {
            continue;
        };
        if byte_record.record_end != *record_end || !byte_record.ee_extra_byte_inserts.is_empty() {
            if debug_live_claim_enabled_for_offset(offset) {
                eprintln!(
                    "live-object EE appearance name bit repair candidate rejected: offset={offset} reason=byte-record-shape fence_bits={preceding_fence_bits} parsed_end={} expected_end={} byte_inserts={:?}",
                    byte_record.record_end, *record_end, byte_record.ee_extra_byte_inserts
                );
            }
            continue;
        }
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object EE appearance name bit repair candidate accepted: offset={offset} fence_bits={preceding_fence_bits} appearance_bit_cursor={appearance_bit_cursor} record_end={} ee_bits={} legacy_bits={} equipment_records={}",
                byte_record.record_end,
                byte_record.ee_fragment_bits_consumed,
                byte_record.fragment_bits_consumed,
                byte_record.equipment_records
            );
        }
        if parse_creature_appearance_record(
            bytes,
            offset,
            *record_end,
            AppearanceNameShape::CExoString,
            CreatureAppearanceWireDialect::EeBuild8193,
            None,
        )
        .is_some_and(|record| {
            record.record_end == *record_end && record.ee_extra_byte_inserts.is_empty()
        }) {
            // Both branches byte-parse to the same boundary. Without a
            // decompile-owned discriminator in the fragment stream, changing
            // the selector would be a heuristic rewrite rather than a repair.
            continue;
        }

        // EE and Diamond share the outer creature-name selector: false reads a
        // direct CExoString, true reads two locstring helpers. Captured HG full
        // creature appearances can already be in the EE byte dialect while the
        // promoted fragment stream still advertises the single-string branch.
        // When the byte parser proves the explicit two-inline-name shape, force
        // the selector and both locstring helper bits to the inline CExoString
        // path, then require the normal exact EE validator to consume the full
        // record before the rewrite is accepted.
        let mut trial_fragment_bits = original_fragment_bits.clone();
        let source_name_shape =
            read_appearance_name_shape(&trial_fragment_bits, appearance_bit_cursor)?;
        *trial_fragment_bits.get_mut(appearance_bit_cursor)? = true;
        let name_bits_inserted = match source_name_shape {
            AppearanceNameShape::CExoString => {
                // The source fragment stream selected the single CExoString
                // branch, so the two EE locstring-helper selector bits are not
                // present yet. Insert them instead of overwriting the next
                // semantic fragment bits.
                super::bits::insert_msb_bit(
                    &mut trial_fragment_bits,
                    appearance_bit_cursor.checked_add(1)?,
                    false,
                )?;
                super::bits::insert_msb_bit(
                    &mut trial_fragment_bits,
                    appearance_bit_cursor.checked_add(2)?,
                    false,
                )?;
                2usize
            }
            AppearanceNameShape::LocStringPair => {
                *trial_fragment_bits.get_mut(appearance_bit_cursor.checked_add(1)?)? = false;
                *trial_fragment_bits.get_mut(appearance_bit_cursor.checked_add(2)?)? = false;
                0usize
            }
        };
        let mut nested_name_bits_inserted = 0usize;
        for rewrite in byte_record.ee_name_bit_rewrites.iter().copied() {
            nested_name_bits_inserted =
                nested_name_bits_inserted.checked_add(apply_item_name_fragment_proof_rewrite(
                    &mut trial_fragment_bits,
                    appearance_bit_cursor.checked_add(rewrite.relative_offset)?,
                    rewrite.proof,
                )?)?;
        }
        for (inserted, relative_offset) in byte_record
            .ee_extra_insert_offsets
            .iter()
            .copied()
            .enumerate()
        {
            super::bits::insert_msb_bit(
                &mut trial_fragment_bits,
                appearance_bit_cursor
                    .checked_add(relative_offset)?
                    .checked_add(inserted)?,
                false,
            )?;
        }
        if debug_live_claim_enabled_for_offset(offset) {
            let window = trial_fragment_bits
                .get(appearance_bit_cursor..appearance_bit_cursor.saturating_add(6))
                .unwrap_or(&[]);
            eprintln!(
                "live-object EE appearance name bit repair bits staged: offset={offset} appearance_bit_cursor={appearance_bit_cursor} bits={window:?}"
            );
        }

        let mut proof_cursor = bit_cursor;
        if advance_verified_ee_creature_appearance_record(
            bytes,
            offset,
            *record_end,
            &trial_fragment_bits,
            &mut proof_cursor,
        ) {
            *fragment_bits = trial_fragment_bits;
            if debug_live_claim_enabled_for_offset(offset) {
                eprintln!(
                    "live-object EE appearance name bits repaired: offset={offset} record_end={} bit_cursor={bit_cursor} appearance_bit_cursor={appearance_bit_cursor} preceding_fence_bits={preceding_fence_bits}",
                    *record_end
                );
            }
            return Some(CreatureAppearanceExtraRewrite {
                bits_inserted: name_bits_inserted
                    .saturating_add(nested_name_bits_inserted)
                    .saturating_add(byte_record.ee_extra_insert_offsets.len()),
                bits_removed: 0,
                bytes_inserted: 0,
                bytes_removed: 0,
            });
        }
    }

    *fragment_bits = original_fragment_bits;
    *record_end = original_record_end;
    None
}

fn apply_item_name_fragment_proof_rewrite(
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
    proof: LegacyItemNameFragmentProof,
) -> Option<usize> {
    match proof {
        LegacyItemNameFragmentProof::None => Some(0),
        LegacyItemNameFragmentProof::InlineCExoString => {
            *fragment_bits.get_mut(bit_cursor)? = false;
            Some(0)
        }
        LegacyItemNameFragmentProof::LocStringToken => {
            let source_outer = *fragment_bits.get(bit_cursor)?;
            *fragment_bits.get_mut(bit_cursor)? = true;
            if source_outer {
                *fragment_bits.get_mut(bit_cursor.checked_add(1)?)? = true;
                Some(0)
            } else {
                super::bits::insert_msb_bit(fragment_bits, bit_cursor.checked_add(1)?, true)?;
                Some(1)
            }
        }
        LegacyItemNameFragmentProof::LocStringInlineCExoString
        | LegacyItemNameFragmentProof::BareInlineLocString => {
            let source_outer = *fragment_bits.get(bit_cursor)?;
            *fragment_bits.get_mut(bit_cursor)? = true;
            if source_outer {
                *fragment_bits.get_mut(bit_cursor.checked_add(1)?)? = false;
                Some(0)
            } else {
                super::bits::insert_msb_bit(fragment_bits, bit_cursor.checked_add(1)?, false)?;
                Some(1)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZeroFragmentPaddingRemoval {
    ranges: Vec<ZeroFragmentPaddingRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ZeroFragmentPaddingRange {
    relative_start: usize,
    count: usize,
}

fn find_zero_fragment_padding_removal_for_ee_appearance(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    minimum_relative_start: usize,
    token_selector_padding_repair_relative_start: Option<usize>,
    inline_active_name_fence_repair_relative_start: Option<usize>,
) -> Option<ZeroFragmentPaddingRemoval> {
    // EE `sub_14077FE10` and Diamond `sub_43E9C0` consume the creature `P/5`
    // scalar fields, full 19-part table, 0x2000 tail, and visible-equipment
    // `A`/`D` opcodes from the read buffer. The only fragment bits in this
    // family are the name/locstring selectors and active-property BOOLs.
    // Reassembled HG streams can still carry chunk-local zero fragment padding
    // before those selector bits after prior read-buffer promotion. Treat that
    // as transport storage, not as a semantic field, and remove it only when
    // the exact EE appearance validator accepts the fully rewritten record.
    if record.record_end != record_end {
        return None;
    }

    if record.ee_extra_insert_offsets.is_empty() {
        // Some full-state Diamond appearance records need only EE read-buffer
        // widening: build-0x23 WORD body parts/scalars and the build-0x0E tail
        // byte. Those records have no EE-only fragment bits to insert, but HG
        // reassembled streams can still carry chunk-local zero padding between
        // the appearance selector bit and the following `U/5 0x3967` update.
        // Appearance-only validation cannot distinguish those zeros from
        // harmless trailing storage, so this byte-only path accepts a removal
        // only when the rewritten appearance and the immediately following
        // creature update both validate from the same trial cursor.
        if zero_fragment_padding_removal_candidate_is_stream_exact(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
            record,
            &[],
        ) {
            trace_zero_fragment_padding_repair("byte-only-stream-candidate", offset, &[]);
            return Some(ZeroFragmentPaddingRemoval { ranges: Vec::new() });
        }

        let candidate_ranges = collect_zero_fragment_padding_ranges(
            fragment_bits,
            bit_cursor,
            minimum_relative_start,
            record
                .fragment_bits_consumed
                .saturating_add(LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS),
        );
        let mut accepted: Option<ZeroFragmentPaddingRemoval> = None;
        for range in candidate_ranges.iter().copied() {
            if !zero_fragment_padding_removal_candidate_is_stream_exact(
                bytes,
                offset,
                record_end,
                fragment_bits,
                bit_cursor,
                record,
                &[range],
            ) {
                continue;
            }
            let candidate = ZeroFragmentPaddingRemoval {
                ranges: vec![range],
            };
            if let Some(current) = accepted.as_ref() {
                if zero_fragment_padding_removal_is_strict_subset(&candidate, current) {
                    trace_zero_fragment_padding_repair(
                        "byte-only-stream-candidate",
                        offset,
                        &[range],
                    );
                    accepted = Some(candidate);
                    continue;
                }
                if zero_fragment_padding_removal_is_strict_subset(current, &candidate) {
                    continue;
                }
                if zero_fragment_padding_removal_removes_fewer_bits(&candidate, current) {
                    trace_zero_fragment_padding_repair(
                        "byte-only-stream-candidate",
                        offset,
                        &[range],
                    );
                    accepted = Some(candidate);
                    continue;
                }
                if zero_fragment_padding_removal_removes_fewer_bits(current, &candidate) {
                    continue;
                }
                if zero_fragment_padding_removals_produce_same_fragment_bits(
                    fragment_bits,
                    bit_cursor,
                    record,
                    &candidate,
                    current,
                ) {
                    continue;
                }
                trace_zero_fragment_padding_repair("byte-only-stream-ambiguous", offset, &[range]);
                return None;
            }
            accepted = Some(candidate);
            trace_zero_fragment_padding_repair("byte-only-stream-candidate", offset, &[range]);
        }

        for (index, first) in candidate_ranges.iter().copied().enumerate() {
            for second in candidate_ranges.iter().copied().skip(index + 1) {
                let Some(first_end) = first.relative_start.checked_add(first.count) else {
                    continue;
                };
                if first_end >= second.relative_start {
                    continue;
                }
                let Some(total_removed) = first.count.checked_add(second.count) else {
                    continue;
                };
                if total_removed > LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
                    continue;
                }
                let ranges = [first, second];
                if !zero_fragment_padding_removal_candidate_is_stream_exact(
                    bytes,
                    offset,
                    record_end,
                    fragment_bits,
                    bit_cursor,
                    record,
                    &ranges,
                ) {
                    continue;
                }
                let candidate = ZeroFragmentPaddingRemoval {
                    ranges: ranges.to_vec(),
                };
                if let Some(current) = accepted.as_ref() {
                    if zero_fragment_padding_removal_is_strict_subset(&candidate, current) {
                        trace_zero_fragment_padding_repair(
                            "byte-only-stream-candidate",
                            offset,
                            &ranges,
                        );
                        accepted = Some(candidate);
                        continue;
                    }
                    if zero_fragment_padding_removal_is_strict_subset(current, &candidate) {
                        continue;
                    }
                    if zero_fragment_padding_removal_removes_fewer_bits(&candidate, current) {
                        trace_zero_fragment_padding_repair(
                            "byte-only-stream-candidate",
                            offset,
                            &ranges,
                        );
                        accepted = Some(candidate);
                        continue;
                    }
                    if zero_fragment_padding_removal_removes_fewer_bits(current, &candidate) {
                        continue;
                    }
                    if zero_fragment_padding_removals_produce_same_fragment_bits(
                        fragment_bits,
                        bit_cursor,
                        record,
                        &candidate,
                        current,
                    ) {
                        continue;
                    }
                    trace_zero_fragment_padding_repair(
                        "byte-only-stream-ambiguous",
                        offset,
                        &ranges,
                    );
                    return None;
                }
                accepted = Some(candidate);
                trace_zero_fragment_padding_repair("byte-only-stream-candidate", offset, &ranges);
            }
        }

        if accepted.is_none() {
            trace_zero_fragment_padding_repair("byte-only-stream-none", offset, &[]);
        }
        return accepted;
    }

    if zero_fragment_padding_removal_candidate_is_exact(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        record,
        &[],
    ) {
        trace_zero_fragment_padding_repair("candidate", offset, &[]);
        return Some(ZeroFragmentPaddingRemoval { ranges: Vec::new() });
    }
    if zero_fragment_padding_removal_candidate_has_verified_trailing_tail(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        record,
        &[],
    ) {
        trace_zero_fragment_padding_repair("candidate-trailing-tail", offset, &[]);
        return Some(ZeroFragmentPaddingRemoval { ranges: Vec::new() });
    }

    if let Some(removal) = leading_visible_equipment_selector_padding_removal_if_exact(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        record,
        minimum_relative_start,
    ) {
        trace_zero_fragment_padding_repair(
            "visible-equipment-leading-selector-padding",
            offset,
            removal.ranges.as_slice(),
        );
        return Some(removal);
    }

    if let Some(relative_start) = token_selector_padding_repair_relative_start {
        let range = ZeroFragmentPaddingRange {
            relative_start,
            count: 1,
        };
        if relative_start >= minimum_relative_start
            && legacy_locstring_token_selector_padding_bits_are_proven(
                fragment_bits,
                bit_cursor,
                relative_start,
            )
            && zero_fragment_padding_removal_candidate_is_exact(
                bytes,
                offset,
                record_end,
                fragment_bits,
                bit_cursor,
                record,
                &[range],
            )
        {
            trace_zero_fragment_padding_repair("token-selector-padding", offset, &[range]);
            return Some(ZeroFragmentPaddingRemoval {
                ranges: vec![range],
            });
        }
    }

    if let Some(relative_start) = inline_active_name_fence_repair_relative_start {
        let range = ZeroFragmentPaddingRange {
            relative_start,
            count: LEGACY_UPDATE_POSITION_FRAGMENT_BITS,
        };
        let candidate_proven = legacy_visible_equipment_inline_name_fence_bits_are_proven(
            fragment_bits,
            bit_cursor,
            relative_start,
        );
        let candidate_exact = candidate_proven
            && zero_fragment_padding_removal_candidate_is_exact(
                bytes,
                offset,
                record_end,
                fragment_bits,
                bit_cursor,
                record,
                &[range],
            );
        if candidate_exact {
            // Diamond `sub_448E30` reads the visible-equipment item appearance
            // bytes with `sub_4514C0`, optionally routes object creation through
            // `sub_441EC0`/`sub_42E490`, then immediately calls active-item
            // `sub_451020`; none of those intervening helpers consume CNW
            // fragment bits. When promoted fragment storage leaves two legacy
            // position/fence bits immediately before a byte-proven inline item
            // name, those bits are transport residue, not item-name semantics.
            // Keep this repair narrow and require the full EE appearance
            // validator to accept after the removal and all EE insertions.
            trace_zero_fragment_padding_repair(
                "visible-equipment-inline-name-fence",
                offset,
                &[range],
            );
            return Some(ZeroFragmentPaddingRemoval {
                ranges: vec![range],
            });
        } else if candidate_proven {
            // The two-bit inline equipment fence is decompile-backed, but
            // composing it with later zero-padding deletions is not safe to
            // decide from the appearance record alone. Multiple locally exact
            // combinations can consume the same EE appearance shape while
            // shifting the next live-object record's fragment cursor
            // differently. Only accept a composed repair when the rewritten
            // EE appearance and the immediately following creature update are
            // both exact from the same trial cursor.
            let mut accepted: Option<ZeroFragmentPaddingRemoval> = None;
            let secondary_minimum = relative_start.saturating_add(range.count);
            let secondary_ranges = collect_zero_fragment_padding_ranges_after_base_removal(
                fragment_bits,
                bit_cursor,
                range,
                secondary_minimum,
                record
                    .fragment_bits_consumed
                    .saturating_add(LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS),
            );
            if debug_live_claim_enabled_for_offset(offset) {
                eprintln!(
                    "live-object appearance inline-name fence stream-proof search: offset={offset} base_range={range:?} post_minimum={secondary_minimum} secondary_count={} first={:?} last={:?}",
                    secondary_ranges.len(),
                    secondary_ranges.first(),
                    secondary_ranges.last()
                );
            }
            for secondary in secondary_ranges.iter().copied() {
                let Some(total_removed) = range.count.checked_add(secondary.count) else {
                    continue;
                };
                if total_removed > LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
                    continue;
                }
                if accepted
                    .as_ref()
                    .map(|current| {
                        total_removed
                            >= zero_fragment_padding_ranges_total(current.ranges.as_slice())
                    })
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(range_end) = range.relative_start.checked_add(range.count) else {
                    continue;
                };
                if secondary.relative_start < range_end {
                    continue;
                }
                let ranges = [range, secondary];
                if !zero_fragment_padding_removal_candidate_is_stream_exact(
                    bytes,
                    offset,
                    record_end,
                    fragment_bits,
                    bit_cursor,
                    record,
                    &ranges,
                ) {
                    continue;
                }
                let candidate = ZeroFragmentPaddingRemoval {
                    ranges: ranges.to_vec(),
                };
                if let Some(current) = accepted.as_ref() {
                    if inline_fence_zero_fragment_padding_removal_is_preferred(&candidate, current)
                    {
                        accepted = Some(candidate);
                        continue;
                    }
                    if inline_fence_zero_fragment_padding_removal_is_preferred(current, &candidate)
                    {
                        continue;
                    }
                    trace_zero_fragment_padding_repair(
                        "visible-equipment-inline-name-fence-stream-ambiguous",
                        offset,
                        &ranges,
                    );
                    return None;
                }
                accepted = Some(candidate);
            }
            if accepted.is_none() {
                for (first_index, first_secondary) in secondary_ranges.iter().copied().enumerate() {
                    let Some(first_secondary_end) = first_secondary
                        .relative_start
                        .checked_add(first_secondary.count)
                    else {
                        continue;
                    };
                    for second_secondary in secondary_ranges.iter().copied().skip(first_index + 1) {
                        if first_secondary_end >= second_secondary.relative_start {
                            continue;
                        }
                        let Some(total_removed) = range
                            .count
                            .checked_add(first_secondary.count)
                            .and_then(|value| value.checked_add(second_secondary.count))
                        else {
                            continue;
                        };
                        if total_removed > LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
                            continue;
                        }
                        if accepted
                            .as_ref()
                            .map(|current| {
                                total_removed
                                    >= zero_fragment_padding_ranges_total(current.ranges.as_slice())
                            })
                            .unwrap_or(false)
                        {
                            continue;
                        }
                        let Some(range_end) = range.relative_start.checked_add(range.count) else {
                            continue;
                        };
                        if first_secondary.relative_start < range_end {
                            continue;
                        }
                        let ranges = [range, first_secondary, second_secondary];
                        if !zero_fragment_padding_removal_candidate_is_stream_exact(
                            bytes,
                            offset,
                            record_end,
                            fragment_bits,
                            bit_cursor,
                            record,
                            &ranges,
                        ) {
                            continue;
                        }
                        let candidate = ZeroFragmentPaddingRemoval {
                            ranges: ranges.to_vec(),
                        };
                        if let Some(current) = accepted.as_ref() {
                            if inline_fence_zero_fragment_padding_removal_is_preferred(
                                &candidate, current,
                            ) {
                                accepted = Some(candidate);
                                continue;
                            }
                            if inline_fence_zero_fragment_padding_removal_is_preferred(
                                current, &candidate,
                            ) {
                                continue;
                            }
                            trace_zero_fragment_padding_repair(
                                "visible-equipment-inline-name-fence-stream-ambiguous",
                                offset,
                                &ranges,
                            );
                            return None;
                        }
                        accepted = Some(candidate);
                    }
                }
            }
            if let Some(removal) = accepted {
                trace_zero_fragment_padding_repair(
                    "visible-equipment-inline-name-fence-stream-proven",
                    offset,
                    removal.ranges.as_slice(),
                );
                return Some(removal);
            }
            trace_zero_fragment_padding_repair(
                "visible-equipment-inline-name-fence-rejected",
                offset,
                &[range],
            );
            return None;
        } else {
            trace_zero_fragment_padding_repair(
                "visible-equipment-inline-name-fence-not-proven",
                offset,
                &[range],
            );
        }
    }

    let candidate_ranges = collect_zero_fragment_padding_ranges(
        fragment_bits,
        bit_cursor,
        minimum_relative_start,
        record
            .fragment_bits_consumed
            .saturating_add(LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS),
    );

    let mut accepted: Option<ZeroFragmentPaddingRemoval> = None;
    for range in candidate_ranges.iter().copied() {
        if !zero_fragment_padding_removal_candidate_is_exact(
            bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
            record,
            &[range],
        ) {
            continue;
        }
        let candidate = ZeroFragmentPaddingRemoval {
            ranges: vec![range],
        };
        if let Some(current) = accepted.as_ref() {
            if zero_fragment_padding_removal_is_strict_subset(&candidate, current) {
                trace_zero_fragment_padding_repair("candidate", offset, &[range]);
                accepted = Some(candidate);
                continue;
            }
            if zero_fragment_padding_removal_is_strict_subset(current, &candidate) {
                continue;
            }
            if zero_fragment_padding_removal_prefers_ee_insert_boundary(&candidate, current, record)
            {
                trace_zero_fragment_padding_repair("candidate", offset, &[range]);
                accepted = Some(candidate);
                continue;
            }
            if zero_fragment_padding_removal_prefers_ee_insert_boundary(current, &candidate, record)
            {
                continue;
            }
            trace_zero_fragment_padding_repair("ambiguous", offset, &[range]);
            return None;
        }
        accepted = Some(candidate);
        trace_zero_fragment_padding_repair("candidate", offset, &[range]);
    }

    for (index, first) in candidate_ranges.iter().copied().enumerate() {
        for second in candidate_ranges.iter().copied().skip(index + 1) {
            let Some(first_end) = first.relative_start.checked_add(first.count) else {
                continue;
            };
            if first_end >= second.relative_start {
                continue;
            }
            let Some(total_removed) = first.count.checked_add(second.count) else {
                continue;
            };
            if total_removed > LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
                continue;
            }
            let ranges = [first, second];
            if !zero_fragment_padding_removal_candidate_is_exact(
                bytes,
                offset,
                record_end,
                fragment_bits,
                bit_cursor,
                record,
                &ranges,
            ) {
                continue;
            }
            let candidate = ZeroFragmentPaddingRemoval {
                ranges: ranges.to_vec(),
            };
            if let Some(current) = accepted.as_ref() {
                if zero_fragment_padding_removal_is_strict_subset(&candidate, current) {
                    trace_zero_fragment_padding_repair("candidate", offset, &ranges);
                    accepted = Some(candidate);
                    continue;
                }
                if zero_fragment_padding_removal_is_strict_subset(current, &candidate) {
                    continue;
                }
                if zero_fragment_padding_removal_prefers_ee_insert_boundary(
                    &candidate, current, record,
                ) {
                    trace_zero_fragment_padding_repair("candidate", offset, &ranges);
                    accepted = Some(candidate);
                    continue;
                }
                if zero_fragment_padding_removal_prefers_ee_insert_boundary(
                    current, &candidate, record,
                ) {
                    continue;
                }
                trace_zero_fragment_padding_repair("ambiguous", offset, &ranges);
                return None;
            }
            accepted = Some(candidate);
            trace_zero_fragment_padding_repair("candidate", offset, &ranges);
        }
    }
    if accepted.is_none() {
        trace_zero_fragment_padding_repair("none", offset, &[]);
    }
    accepted
}

fn leading_visible_equipment_selector_padding_removal_if_exact(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    minimum_relative_start: usize,
) -> Option<ZeroFragmentPaddingRemoval> {
    // Reassembled HG full-appearance streams can promote a run of zero
    // transport bits between the creature-name selector and the first
    // visible-equipment active-item name selector.  The decompiled appearance
    // reader has no semantic fields in that gap: after the full body/equipment
    // read-buffer path, the next fragment bit belongs to the embedded item-name
    // branch.  This fast candidate avoids the quadratic generic search for the
    // common "many zeros then first positive selector" capture shape, but it is
    // still accepted only when the fully emitted EE appearance validates.
    let absolute_start = bit_cursor.checked_add(minimum_relative_start)?;
    if absolute_start >= fragment_bits.len() || fragment_bits.get(absolute_start).copied()? {
        return None;
    }

    let mut leading_count = 0usize;
    while leading_count < LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
        let index = absolute_start.checked_add(leading_count)?;
        let Some(bit) = fragment_bits.get(index).copied() else {
            break;
        };
        if bit {
            break;
        }
        leading_count = leading_count.checked_add(1)?;
    }
    if leading_count == 0 {
        return None;
    }

    let leading = ZeroFragmentPaddingRange {
        relative_start: minimum_relative_start,
        count: leading_count,
    };
    if zero_fragment_padding_removal_candidate_is_exact(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        record,
        &[leading],
    ) {
        return Some(ZeroFragmentPaddingRemoval {
            ranges: vec![leading],
        });
    }

    // Some token-name captures carry a second, much smaller zero run
    // immediately after the first positive selector. Do not interpret it here;
    // simply try the bounded removal and let the exact EE validator decide
    // whether those bits are transport residue or semantic locstring state.
    let selector_relative = minimum_relative_start.checked_add(leading_count)?;
    let secondary_relative = selector_relative.checked_add(1)?;
    let secondary_absolute = bit_cursor.checked_add(secondary_relative)?;
    let mut secondary_count = 0usize;
    while secondary_count < LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
        let index = secondary_absolute.checked_add(secondary_count)?;
        let Some(bit) = fragment_bits.get(index).copied() else {
            break;
        };
        if bit {
            break;
        }
        secondary_count = secondary_count.checked_add(1)?;
    }
    if secondary_count == 0 {
        return None;
    }
    let secondary = ZeroFragmentPaddingRange {
        relative_start: secondary_relative,
        count: secondary_count,
    };
    if zero_fragment_padding_removal_candidate_is_exact(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        record,
        &[leading, secondary],
    ) {
        return Some(ZeroFragmentPaddingRemoval {
            ranges: vec![leading, secondary],
        });
    }

    None
}

fn legacy_locstring_token_selector_padding_bits_are_proven(
    fragment_bits: &[bool],
    bit_cursor: usize,
    relative_start: usize,
) -> bool {
    let Some(padding_cursor) = bit_cursor.checked_add(relative_start) else {
        return false;
    };
    let Some(outer_cursor) = padding_cursor.checked_sub(1) else {
        return false;
    };
    let Some(inner_cursor) = padding_cursor.checked_add(1) else {
        return false;
    };
    fragment_bits
        .get(outer_cursor)
        .zip(fragment_bits.get(padding_cursor))
        .zip(fragment_bits.get(inner_cursor))
        .map(|((outer, padding), inner)| *outer && !*padding && *inner)
        .unwrap_or(false)
}

fn legacy_visible_equipment_inline_name_fence_bits_are_proven(
    fragment_bits: &[bool],
    bit_cursor: usize,
    relative_start: usize,
) -> bool {
    let Some(first_fence_cursor) = bit_cursor.checked_add(relative_start) else {
        return false;
    };
    let Some(second_fence_cursor) = first_fence_cursor.checked_add(1) else {
        return false;
    };
    let Some(inline_selector_cursor) =
        first_fence_cursor.checked_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS)
    else {
        return false;
    };
    fragment_bits
        .get(first_fence_cursor)
        .zip(fragment_bits.get(second_fence_cursor))
        .zip(fragment_bits.get(inline_selector_cursor))
        .map(|((first, second), inline_selector)| *first && *second && !*inline_selector)
        .unwrap_or(false)
}

fn zero_fragment_padding_removal_is_strict_subset(
    candidate: &ZeroFragmentPaddingRemoval,
    other: &ZeroFragmentPaddingRemoval,
) -> bool {
    zero_fragment_padding_ranges_total(candidate.ranges.as_slice())
        < zero_fragment_padding_ranges_total(other.ranges.as_slice())
        && zero_fragment_padding_ranges_are_subset(
            candidate.ranges.as_slice(),
            other.ranges.as_slice(),
        )
}

fn zero_fragment_padding_removal_prefers_ee_insert_boundary(
    candidate: &ZeroFragmentPaddingRemoval,
    other: &ZeroFragmentPaddingRemoval,
    record: &LegacyAppearanceRecord,
) -> bool {
    let candidate_total = zero_fragment_padding_ranges_total(candidate.ranges.as_slice());
    if candidate_total != zero_fragment_padding_ranges_total(other.ranges.as_slice()) {
        return false;
    }

    // This is intentionally narrower than a score. EE `sub_14077FE10` adds
    // three BOOL fields that Diamond `sub_43E9C0` does not consume; those exact
    // insertion offsets are carried by the parsed appearance record. When two
    // locally exact padding-removal candidates remove the same number of bits,
    // prefer the one that deletes a zero run starting exactly at one of those
    // EE-only insertion boundaries. That preserves legacy semantic bits while
    // discarding promoted transport padding at a decompile-confirmed EE field.
    let candidate_boundary_hits =
        zero_fragment_padding_removal_ee_insert_boundary_hits(candidate, record);
    let other_boundary_hits = zero_fragment_padding_removal_ee_insert_boundary_hits(other, record);
    candidate_boundary_hits > other_boundary_hits
}

fn zero_fragment_padding_removal_removes_fewer_bits(
    candidate: &ZeroFragmentPaddingRemoval,
    other: &ZeroFragmentPaddingRemoval,
) -> bool {
    // Byte-only appearance rewrites have no EE-only fragment insertion boundary
    // to anchor a padding repair. Once the rewritten appearance and following
    // creature update both validate exactly, choose the smallest zero-padding
    // deletion that preserves stream alignment. Equal-size alternatives still
    // quarantine unless they produce an identical fragment stream.
    zero_fragment_padding_ranges_total(candidate.ranges.as_slice())
        < zero_fragment_padding_ranges_total(other.ranges.as_slice())
}

fn zero_fragment_padding_removals_produce_same_fragment_bits(
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    candidate: &ZeroFragmentPaddingRemoval,
    other: &ZeroFragmentPaddingRemoval,
) -> bool {
    let Some(candidate_bits) = zero_fragment_padding_removal_resulting_fragment_bits(
        fragment_bits,
        bit_cursor,
        record,
        candidate.ranges.as_slice(),
    ) else {
        return false;
    };
    let Some(other_bits) = zero_fragment_padding_removal_resulting_fragment_bits(
        fragment_bits,
        bit_cursor,
        record,
        other.ranges.as_slice(),
    ) else {
        return false;
    };
    candidate_bits == other_bits
}

fn zero_fragment_padding_removal_resulting_fragment_bits(
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    ranges: &[ZeroFragmentPaddingRange],
) -> Option<Vec<bool>> {
    let mut trial_bits = fragment_bits.to_vec();
    for range in ranges.iter().rev() {
        let absolute_start = bit_cursor.checked_add(range.relative_start)?;
        let absolute_end = absolute_start.checked_add(range.count)?;
        if absolute_end > trial_bits.len() {
            return None;
        }
        trial_bits.drain(absolute_start..absolute_end);
    }
    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        let insert_at = bit_cursor
            .checked_add(relative_offset)?
            .checked_add(inserted)?;
        super::bits::insert_msb_bit(&mut trial_bits, insert_at, false)?;
    }
    Some(trial_bits)
}

fn zero_fragment_padding_removal_ee_insert_boundary_hits(
    removal: &ZeroFragmentPaddingRemoval,
    record: &LegacyAppearanceRecord,
) -> usize {
    removal
        .ranges
        .iter()
        .filter(|range| {
            record
                .ee_extra_insert_offsets
                .iter()
                .any(|offset| *offset == range.relative_start)
        })
        .count()
}

fn inline_fence_zero_fragment_padding_removal_is_preferred(
    candidate: &ZeroFragmentPaddingRemoval,
    other: &ZeroFragmentPaddingRemoval,
) -> bool {
    if zero_fragment_padding_removal_is_strict_subset(candidate, other) {
        return true;
    }

    // This branch is reached only after Diamond's visible-equipment item
    // reader proves the inline-name selector and the fully translated EE
    // appearance validator accepts both candidates. When multiple exact
    // repairs remain, choose the minimal removal: the compatibility transform's
    // contract is to delete promoted transport/padding residue, so preserving
    // the greatest number of original source BOOLs is the safest decompile-
    // aligned tie-break. Equal-sized non-subset repairs still quarantine as
    // ambiguous below.
    let candidate_total = zero_fragment_padding_ranges_total(candidate.ranges.as_slice());
    let other_total = zero_fragment_padding_ranges_total(other.ranges.as_slice());
    if candidate_total != other_total {
        return candidate_total < other_total;
    }

    inline_fence_zero_fragment_padding_removal_uses_smaller_earliest_padding_run(candidate, other)
}

fn inline_fence_zero_fragment_padding_removal_uses_smaller_earliest_padding_run(
    candidate: &ZeroFragmentPaddingRemoval,
    other: &ZeroFragmentPaddingRemoval,
) -> bool {
    candidate
        .ranges
        .iter()
        .skip(1)
        .zip(other.ranges.iter().skip(1))
        .find_map(|(candidate_range, other_range)| {
            (candidate_range.relative_start != other_range.relative_start)
                .then_some(candidate_range.relative_start < other_range.relative_start)
                .or_else(|| {
                    (candidate_range.count != other_range.count)
                        .then_some(candidate_range.count < other_range.count)
                })
        })
        .unwrap_or(false)
}

fn zero_fragment_padding_ranges_are_subset(
    candidate: &[ZeroFragmentPaddingRange],
    other: &[ZeroFragmentPaddingRange],
) -> bool {
    candidate.iter().all(|range| {
        let Some(end) = range.relative_start.checked_add(range.count) else {
            return false;
        };
        (range.relative_start..end).all(|relative| {
            other.iter().any(|other_range| {
                let Some(other_end) = other_range.relative_start.checked_add(other_range.count)
                else {
                    return false;
                };
                relative >= other_range.relative_start && relative < other_end
            })
        })
    })
}

fn zero_fragment_padding_ranges_total(ranges: &[ZeroFragmentPaddingRange]) -> usize {
    ranges.iter().map(|range| range.count).sum()
}

fn collect_zero_fragment_padding_ranges(
    fragment_bits: &[bool],
    bit_cursor: usize,
    minimum_relative_start: usize,
    maximum_relative_start: usize,
) -> Vec<ZeroFragmentPaddingRange> {
    let mut ranges = Vec::new();
    for relative_start in minimum_relative_start..=maximum_relative_start {
        let Some(absolute_start) = bit_cursor.checked_add(relative_start) else {
            continue;
        };
        if absolute_start >= fragment_bits.len() {
            break;
        }
        for count in 1..=LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
            let Some(absolute_end) = absolute_start.checked_add(count) else {
                continue;
            };
            let Some(candidate_bits) = fragment_bits.get(absolute_start..absolute_end) else {
                break;
            };
            if candidate_bits.iter().any(|bit| *bit) {
                break;
            }
            ranges.push(ZeroFragmentPaddingRange {
                relative_start,
                count,
            });
        }
    }
    ranges
}

fn collect_zero_fragment_padding_ranges_after_base_removal(
    fragment_bits: &[bool],
    bit_cursor: usize,
    base_range: ZeroFragmentPaddingRange,
    minimum_post_removal_relative_start: usize,
    maximum_post_removal_relative_start: usize,
) -> Vec<ZeroFragmentPaddingRange> {
    let mut ranges = Vec::new();
    for post_relative_start in
        minimum_post_removal_relative_start..=maximum_post_removal_relative_start
    {
        let Some(original_relative_start) =
            relative_offset_before_base_removal(post_relative_start, base_range)
        else {
            continue;
        };
        let Some(absolute_start) = bit_cursor.checked_add(original_relative_start) else {
            continue;
        };
        if absolute_start >= fragment_bits.len() {
            break;
        }
        for count in 1..=LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS {
            let Some(original_relative_end) = original_relative_start.checked_add(count) else {
                continue;
            };
            if zero_fragment_padding_range_overlaps_base_removal(
                original_relative_start,
                original_relative_end,
                base_range,
            ) {
                break;
            }
            let Some(absolute_end) = absolute_start.checked_add(count) else {
                continue;
            };
            let Some(candidate_bits) = fragment_bits.get(absolute_start..absolute_end) else {
                break;
            };
            if candidate_bits.iter().any(|bit| *bit) {
                break;
            }
            ranges.push(ZeroFragmentPaddingRange {
                relative_start: original_relative_start,
                count,
            });
        }
    }
    ranges
}

fn relative_offset_before_base_removal(
    post_removal_relative_offset: usize,
    base_range: ZeroFragmentPaddingRange,
) -> Option<usize> {
    let base_end = base_range.relative_start.checked_add(base_range.count)?;
    if post_removal_relative_offset < base_range.relative_start {
        Some(post_removal_relative_offset)
    } else {
        post_removal_relative_offset
            .checked_add(base_range.count)
            .filter(|relative| *relative >= base_end)
    }
}

fn zero_fragment_padding_range_overlaps_base_removal(
    original_relative_start: usize,
    original_relative_end: usize,
    base_range: ZeroFragmentPaddingRange,
) -> bool {
    let Some(base_end) = base_range.relative_start.checked_add(base_range.count) else {
        return true;
    };
    original_relative_start < base_end && original_relative_end > base_range.relative_start
}

fn zero_fragment_padding_removal_candidate_is_exact(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    ranges: &[ZeroFragmentPaddingRange],
) -> bool {
    let mut trial_bits = fragment_bits.to_vec();
    for range in ranges.iter().rev() {
        let Some(absolute_start) = bit_cursor.checked_add(range.relative_start) else {
            return false;
        };
        let Some(absolute_end) = absolute_start.checked_add(range.count) else {
            return false;
        };
        if absolute_end > trial_bits.len() {
            return false;
        }
        trial_bits.drain(absolute_start..absolute_end);
    }
    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        let Some(insert_at) = bit_cursor
            .checked_add(relative_offset)
            .and_then(|cursor| cursor.checked_add(inserted))
        else {
            return false;
        };
        if super::bits::insert_msb_bit(&mut trial_bits, insert_at, false).is_none() {
            return false;
        }
    }

    let mut trial_bytes = bytes.to_vec();
    let mut trial_record_end = record_end;
    let Some(byte_apply) = apply_creature_appearance_byte_inserts(
        &mut trial_bytes,
        offset,
        &mut trial_record_end,
        &record.ee_extra_byte_inserts,
    ) else {
        return false;
    };

    let mut proof_cursor = bit_cursor;
    let exact = advance_verified_ee_creature_appearance_record(
        &trial_bytes,
        offset,
        trial_record_end,
        &trial_bits,
        &mut proof_cursor,
    );
    if !exact && debug_live_claim_verbose_trials_enabled_for_offset(offset) {
        let byte_insert_offsets = record
            .ee_extra_byte_inserts
            .iter()
            .map(CreatureAppearanceByteInsert::offset)
            .collect::<Vec<_>>();
        let byte_insert_kinds = record
            .ee_extra_byte_inserts
            .iter()
            .map(|insert| format!("{insert:?}"))
            .collect::<Vec<_>>();
        let trial_bit_window = trial_bits
            .get(bit_cursor..bit_cursor.saturating_add(32).min(trial_bits.len()))
            .unwrap_or(&[])
            .to_vec();
        eprintln!(
            "live-object appearance zero fragment padding trial rejected: offset={offset} record_end={record_end} trial_record_end={trial_record_end} bit_cursor={bit_cursor} ranges={ranges:?} bit_inserts={:?} byte_insert_offsets={byte_insert_offsets:?} byte_insert_kinds={byte_insert_kinds:?} bytes_inserted={} bytes_removed={} trial_bits={trial_bit_window:?}",
            record.ee_extra_insert_offsets, byte_apply.bytes_inserted, byte_apply.bytes_removed,
        );
    }
    exact
}

fn zero_fragment_padding_removal_candidate_has_verified_trailing_tail(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    ranges: &[ZeroFragmentPaddingRange],
) -> bool {
    let mut trial_bits = fragment_bits.to_vec();
    for range in ranges.iter().rev() {
        let Some(absolute_start) = bit_cursor.checked_add(range.relative_start) else {
            return false;
        };
        let Some(absolute_end) = absolute_start.checked_add(range.count) else {
            return false;
        };
        if absolute_end > trial_bits.len() {
            return false;
        }
        trial_bits.drain(absolute_start..absolute_end);
    }
    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        let Some(insert_at) = bit_cursor
            .checked_add(relative_offset)
            .and_then(|cursor| cursor.checked_add(inserted))
        else {
            return false;
        };
        if super::bits::insert_msb_bit(&mut trial_bits, insert_at, false).is_none() {
            return false;
        }
    }

    let mut trial_bytes = bytes.to_vec();
    let mut trial_record_end = record_end;
    if apply_creature_appearance_byte_inserts(
        &mut trial_bytes,
        offset,
        &mut trial_record_end,
        &record.ee_extra_byte_inserts,
    )
    .is_none()
    {
        return false;
    }

    try_get_ee_creature_appearance_record_end_before_verified_creature_update_tail_for_ee(
        &trial_bytes,
        offset,
        trial_record_end,
        &trial_bits,
        bit_cursor,
    )
    .is_some()
}

struct ZeroFragmentPaddingTrial {
    bytes: Vec<u8>,
    fragment_bits: Vec<bool>,
    record_end: usize,
    proof_cursor: usize,
}

fn zero_fragment_padding_removal_candidate_is_stream_exact(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    ranges: &[ZeroFragmentPaddingRange],
) -> bool {
    let Some(trial) = build_zero_fragment_padding_trial(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        record,
        ranges,
    ) else {
        return false;
    };
    let following_offset = trial.record_end;
    if following_offset + 2 > trial.bytes.len()
        || trial.bytes.get(following_offset).copied() != Some(b'U')
        || trial.bytes.get(following_offset + 1).copied() != Some(LEGACY_CREATURE_TYPE)
    {
        return false;
    }

    let following_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
        &trial.bytes,
        following_offset,
        trial.bytes.len(),
    )
    .min(trial.bytes.len());
    if following_end <= following_offset {
        return false;
    }

    let mut following_cursor = trial.proof_cursor;
    if super::creature::advance_verified_noop_creature_update_record_exact_cursor(
        &trial.bytes,
        following_offset,
        following_end,
        &trial.fragment_bits,
        &mut following_cursor,
    ) {
        return true;
    }

    let mut action0_trial_bytes = trial.bytes.clone();
    let mut action0_trial_fragment_bits = trial.fragment_bits.clone();
    let mut action0_following_end = following_end;
    if super::creature::remove_3967_action0_legacy_bridge_followup_for_ee(
        &mut action0_trial_bytes,
        following_offset,
        &mut action0_following_end,
        &mut action0_trial_fragment_bits,
        trial.proof_cursor,
    )
    .is_some()
    {
        let mut following_cursor = trial.proof_cursor;
        if super::creature::advance_verified_noop_creature_update_record_exact_cursor(
            &action0_trial_bytes,
            following_offset,
            action0_following_end,
            &action0_trial_fragment_bits,
            &mut following_cursor,
        ) {
            return true;
        }
    }

    fragment_spans::verified_creature_update_3967_read_end_before_interleaved_fragment_span(
        &trial.bytes,
        following_offset,
        following_end,
        &trial.fragment_bits,
        trial.proof_cursor,
    )
    .is_some()
}

fn build_zero_fragment_padding_trial(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    record: &LegacyAppearanceRecord,
    ranges: &[ZeroFragmentPaddingRange],
) -> Option<ZeroFragmentPaddingTrial> {
    let mut trial_bits = fragment_bits.to_vec();
    for range in ranges.iter().rev() {
        let absolute_start = bit_cursor.checked_add(range.relative_start)?;
        let absolute_end = absolute_start.checked_add(range.count)?;
        if absolute_end > trial_bits.len() {
            return None;
        }
        trial_bits.drain(absolute_start..absolute_end);
    }
    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        let insert_at = bit_cursor
            .checked_add(relative_offset)
            .and_then(|cursor| cursor.checked_add(inserted))?;
        super::bits::insert_msb_bit(&mut trial_bits, insert_at, false)?;
    }

    let mut trial_bytes = bytes.to_vec();
    let mut trial_record_end = record_end;
    apply_creature_appearance_byte_inserts(
        &mut trial_bytes,
        offset,
        &mut trial_record_end,
        &record.ee_extra_byte_inserts,
    )?;

    let mut proof_cursor = bit_cursor;
    if !advance_verified_ee_creature_appearance_record(
        &trial_bytes,
        offset,
        trial_record_end,
        &trial_bits,
        &mut proof_cursor,
    ) {
        return None;
    }

    Some(ZeroFragmentPaddingTrial {
        bytes: trial_bytes,
        fragment_bits: trial_bits,
        record_end: trial_record_end,
        proof_cursor,
    })
}

fn debug_live_claim_enabled_for_offset(offset: usize) -> bool {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return false;
    }
    let Ok(filter) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM_OWNER_OFFSET") else {
        return true;
    };
    filter.split(',').any(|part| {
        part.trim()
            .parse::<usize>()
            .map(|wanted| wanted == offset)
            .unwrap_or(false)
    })
}

fn debug_live_claim_enabled_for_nearby_offset(offset: usize) -> bool {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return false;
    }
    let Ok(filter) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM_OWNER_OFFSET") else {
        return true;
    };
    filter.split(',').any(|part| {
        let Ok(wanted) = part.trim().parse::<usize>() else {
            return false;
        };
        offset == wanted
    })
}

fn debug_live_claim_verbose_trials_enabled_for_offset(offset: usize) -> bool {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM_VERBOSE_TRIALS").is_none() {
        return false;
    }
    debug_live_claim_enabled_for_offset(offset)
}

fn trace_zero_fragment_padding_repair(
    state: &'static str,
    offset: usize,
    ranges: &[ZeroFragmentPaddingRange],
) {
    if !debug_live_claim_enabled_for_offset(offset)
        && std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_APPEARANCE_REPAIR").is_none()
    {
        return;
    }
    eprintln!(
        "live-object appearance zero fragment padding candidate: state={state} offset={offset} ranges={ranges:?}"
    );
}

fn apply_creature_appearance_byte_inserts(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    inserts: &[CreatureAppearanceByteInsert],
) -> Option<CreatureAppearanceByteApplySummary> {
    let mut byte_inserts = inserts.to_vec();
    byte_inserts.sort_by_key(|insert| (insert.offset(), insert.order()));
    let mut byte_delta = 0isize;
    let mut summary = CreatureAppearanceByteApplySummary::default();
    for insert in byte_inserts.iter() {
        let insert_offset = insert.offset();
        if insert_offset < offset || insert_offset > *record_end {
            return None;
        }
        let actual_insert_offset = if byte_delta.is_negative() {
            insert_offset.checked_sub(byte_delta.unsigned_abs())?
        } else {
            insert_offset.checked_add(usize::try_from(byte_delta).ok()?)?
        };
        let bytes_removed = insert.bytes_removed();
        let removal_end = actual_insert_offset.checked_add(bytes_removed)?;
        if removal_end > bytes.len() {
            return None;
        }
        if matches!(
            insert,
            CreatureAppearanceByteInsert::LegacyScalarVisualTransformIdentityReplacement { .. }
        ) && bytes.get(actual_insert_offset..removal_end)
            != Some(&LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES[..])
        {
            return None;
        }
        let insert_bytes = insert.bytes();
        bytes.splice(
            actual_insert_offset..removal_end,
            insert_bytes.iter().copied(),
        );
        summary.bytes_inserted = summary.bytes_inserted.checked_add(insert_bytes.len())?;
        summary.bytes_removed = summary.bytes_removed.checked_add(bytes_removed)?;
        if insert_bytes.len() >= bytes_removed {
            *record_end = (*record_end).checked_add(insert_bytes.len() - bytes_removed)?;
        } else {
            *record_end = (*record_end).checked_sub(bytes_removed - insert_bytes.len())?;
        }
        byte_delta = byte_delta
            .checked_add(isize::try_from(insert_bytes.len()).ok()?)?
            .checked_sub(isize::try_from(bytes_removed).ok()?)?;
    }
    Some(summary)
}

fn read_appearance_name_shape(bits: &[bool], bit_cursor: usize) -> Option<AppearanceNameShape> {
    let bit = *bits.get(bit_cursor)?;
    Some(if bit {
        AppearanceNameShape::LocStringPair
    } else {
        AppearanceNameShape::CExoString
    })
}

fn parse_verified_creature_appearance_with_optional_preceding_fence(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    preferred_record_end: Option<usize>,
    translated_ee: bool,
    allow_cross_record_fence: bool,
) -> Option<VerifiedAppearanceParse> {
    let scan_end = scan_end.min(bytes.len());
    let mask = read_u16_le(bytes, offset.checked_add(6)?)?;
    let parse_candidate = |preceding_fence_bits: usize| -> Option<VerifiedAppearanceParse> {
        if preceding_fence_bits != 0 && mask != LEGACY_APPEARANCE_ALL_FIELDS_MASK {
            return None;
        }
        if preceding_fence_bits != 0
            && !legacy_full_appearance_preceding_fence_bits_are_proven(
                fragment_bits,
                bit_cursor,
                preceding_fence_bits,
            )
        {
            return None;
        }
        let proof_cursor = bit_cursor.checked_add(preceding_fence_bits)?;
        let name_shape = read_appearance_name_shape(fragment_bits, proof_cursor)?;
        let record = parse_creature_appearance_record(
            bytes,
            offset,
            scan_end,
            name_shape,
            if translated_ee {
                CreatureAppearanceWireDialect::EeBuild8193
            } else {
                CreatureAppearanceWireDialect::LegacyDiamond
            },
            Some(AppearanceBitProof {
                bit_cursor: proof_cursor,
                fragment_bits,
                translated_ee,
                allow_cross_record_fence,
                owner_offset: offset,
            }),
        )?;
        Some(VerifiedAppearanceParse {
            record,
            proof_cursor,
            preceding_fence_bits,
        })
    };

    // The decompiled appearance reader normally starts directly at the current
    // bit cursor, so exact/no-fence is preferred for all partial masks. Full
    // `0xFFFF` appearances are the one exception with fixture-backed ambiguity:
    // a stale name selector can make bytes inside the visible-equipment block
    // look like a shorter CExoString appearance. For that full-state branch,
    // compare every proven candidate with the decompile-owned full-appearance
    // boundary policy instead of returning the first byte-plausible parse.
    let mut accepted: Option<VerifiedAppearanceParse> = parse_candidate(0);
    if accepted.is_some() && mask != LEGACY_APPEARANCE_ALL_FIELDS_MASK {
        return accepted;
    }

    for preceding_fence_bits in LEGACY_FULL_APPEARANCE_PRECEDING_FRAGMENT_FENCE_CANDIDATES {
        if preceding_fence_bits == 0 {
            continue;
        }
        let Some(candidate) = parse_candidate(preceding_fence_bits) else {
            continue;
        };
        if accepted
            .as_ref()
            .map(|current| {
                let candidate_is_preferred_end = preferred_record_end
                    .is_some_and(|preferred| candidate.record.record_end == preferred);
                let current_is_preferred_end = preferred_record_end
                    .is_some_and(|preferred| current.record.record_end == preferred);
                (candidate_is_preferred_end && !current_is_preferred_end)
                    || (!current_is_preferred_end
                        && legacy_appearance_boundary_candidate_is_better(
                            mask,
                            &candidate.record,
                            &current.record,
                        ))
                    || (candidate.record.record_end == current.record.record_end
                        && candidate.record.equipment_records == current.record.equipment_records
                        && candidate.preceding_fence_bits < current.preceding_fence_bits)
            })
            .unwrap_or(true)
        {
            accepted = Some(candidate);
        }
    }
    accepted
}

fn legacy_full_appearance_preceding_fence_bits_are_proven(
    fragment_bits: &[bool],
    bit_cursor: usize,
    preceding_fence_bits: usize,
) -> bool {
    // This is deliberately not a generic "skip N bits" rule. CNW fragment
    // storage owns the first three MSB bits of a packetized fragment byte as
    // the final-byte valid-bit count; the bridge's `decode_msb_valid_bits`
    // keeps those bits in the decoded stream so the packet can be repacked
    // losslessly. When such a packetized span is promoted between live-object
    // records, a following full creature appearance can therefore be preceded
    // by a non-semantic three-bit CNW header. A later HG stream can stack two
    // promoted packetized headers before the same semantic appearance record;
    // that shape is accepted only as `110 110`, and only when the focused
    // appearance parser consumes the full record at the post-fence cursor.
    // Verified captures currently prove only a `110` header-only fence, the
    // stacked `110 110` header pair, and the older `1111` header-plus-data fence.
    // Other leading shapes must quarantine until a capture/decompile trace gives
    // them a precise owner; notably `100` is a valid semantic appearance prefix
    // for existing repeated creature records and must not be treated as a fence.
    let Some(fence) =
        fragment_bits.get(bit_cursor..bit_cursor.saturating_add(preceding_fence_bits))
    else {
        return false;
    };
    let header_value = fence
        .iter()
        .take(CNW_FRAGMENT_HEADER_BITS)
        .fold(0usize, |value, bit| (value << 1) | usize::from(*bit));
    match preceding_fence_bits {
        CNW_FRAGMENT_HEADER_BITS => header_value == 0b110,
        n if n == CNW_FRAGMENT_HEADER_BITS * 2 => {
            let second_header_value = fence
                .iter()
                .skip(CNW_FRAGMENT_HEADER_BITS)
                .take(CNW_FRAGMENT_HEADER_BITS)
                .fold(0usize, |value, bit| (value << 1) | usize::from(*bit));
            header_value == 0b110 && second_header_value == 0b110
        }
        n if n == CNW_FRAGMENT_HEADER_BITS + 1 => {
            // Captured HG full-appearance streams can start after a promoted
            // packetized fragment byte whose three-bit valid-count header is
            // `101`, followed by one previous-record data bit. The next bit is
            // then the decompile-owned appearance name selector. Keep this as
            // a named fence shape instead of a generic skip: callers accept it
            // only when the full Diamond/EE appearance reader consumes the
            // exact record from the post-fence cursor.
            fence
                .get(CNW_FRAGMENT_HEADER_BITS)
                .copied()
                .unwrap_or(false)
                && matches!(header_value, 0b101 | 0b111)
        }
        _ => false,
    }
}

fn proven_name_fragment_delta_for_byte_only_appearance_parse(
    name_shape: AppearanceNameShape,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    let actual = proven_appearance_name_fragment_bits(name_shape, fragment_bits, bit_cursor)?;
    let assumed = byte_only_appearance_name_fragment_bits(name_shape);
    actual.checked_sub(assumed)
}

fn byte_only_appearance_name_fragment_bits(name_shape: AppearanceNameShape) -> usize {
    match name_shape {
        AppearanceNameShape::CExoString => 1,
        AppearanceNameShape::LocStringPair => 3,
    }
}

fn proven_appearance_name_fragment_bits(
    name_shape: AppearanceNameShape,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    match name_shape {
        AppearanceNameShape::CExoString => fragment_bits
            .get(bit_cursor)
            .copied()
            .filter(|selector| !*selector)
            .map(|_| 1),
        AppearanceNameShape::LocStringPair => {
            fragment_bits
                .get(bit_cursor)
                .copied()
                .filter(|selector| *selector)?;
            let mut cursor = bit_cursor.checked_add(1)?;
            cursor = advance_proven_locstring_component_bits(fragment_bits, cursor)?;
            cursor = advance_proven_locstring_component_bits(fragment_bits, cursor)?;
            cursor.checked_sub(bit_cursor)
        }
    }
}

fn advance_proven_locstring_component_bits(
    fragment_bits: &[bool],
    component_bit_cursor: usize,
) -> Option<usize> {
    let inner_is_tlk_token = *fragment_bits.get(component_bit_cursor)?;
    if inner_is_tlk_token {
        fragment_bits.get(component_bit_cursor.checked_add(1)?)?;
        component_bit_cursor.checked_add(2)
    } else {
        component_bit_cursor.checked_add(1)
    }
}

fn apply_name_fragment_delta_to_appearance_record(
    record: &mut LegacyAppearanceRecord,
    delta: usize,
) -> Option<()> {
    if delta == 0 {
        return Some(());
    }

    // The byte-only fallback below mirrors Diamond's read-buffer walk when the
    // full fragment proof has already drifted. For a localized creature name it
    // historically assumed the two `CExoLocString` components each consumed the
    // inline-string branch's one BOOL. Diamond `sub_53E700` and EE's matching
    // locstring reader consume two BOOLs for a TLK-token component, though: the
    // inner token selector plus the language selector. When the current
    // fragment stream proves that wider locstring prefix, shift every later
    // visible-equipment active-property insert by the same amount instead of
    // placing EE's extra `sub_14076BD30` BOOL inside a legacy field.
    record.fragment_bits_consumed = record.fragment_bits_consumed.checked_add(delta)?;
    record.ee_fragment_bits_consumed = record.ee_fragment_bits_consumed.checked_add(delta)?;
    for offset in record.ee_extra_insert_offsets.iter_mut() {
        *offset = offset.checked_add(delta)?;
    }
    record.preferred_zero_padding_relative_start =
        checked_shift_optional_relative(record.preferred_zero_padding_relative_start, delta)?;
    record.token_selector_padding_repair_relative_start = checked_shift_optional_relative(
        record.token_selector_padding_repair_relative_start,
        delta,
    )?;
    record.inline_active_name_fence_repair_relative_start = checked_shift_optional_relative(
        record.inline_active_name_fence_repair_relative_start,
        delta,
    )?;
    Some(())
}

fn checked_shift_optional_relative(value: Option<usize>, delta: usize) -> Option<Option<usize>> {
    match value {
        Some(value) => Some(Some(value.checked_add(delta)?)),
        None => Some(None),
    }
}

fn looks_like_legacy_creature_object_id(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }
    matches!(
        object_id & 0xFF00_0000,
        0x8000_0000 | 0x8800_0000 | 0xFF00_0000 | 0x0100_0000 | 0x0500_0000
    ) || (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID)
        .contains(&object_id)
}

fn parse_creature_appearance_record(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    name_shape: AppearanceNameShape,
    dialect: CreatureAppearanceWireDialect,
    bit_proof: Option<AppearanceBitProof<'_>>,
) -> Option<LegacyAppearanceRecord> {
    if offset.checked_add(LEGACY_APPEARANCE_HEADER_BYTES)? > limit
        || limit > bytes.len()
        || limit > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
        || bytes.get(offset).copied()? != b'P'
        || bytes.get(offset + 1).copied()? != LEGACY_CREATURE_TYPE
    {
        return None;
    }

    let object_id = read_u32_le(bytes, offset + 2)?;
    if !looks_like_creature_or_legacy_sentinel_id(object_id) {
        return None;
    }

    let mask = read_u16_le(bytes, offset + 6)?;
    if mask == 0 {
        return None;
    }

    let mut cursor = offset + LEGACY_APPEARANCE_HEADER_BYTES;
    let mut fragment_bits_consumed = 0usize;
    let mut ee_extra_fragment_bits = 0usize;
    let mut ee_extra_insert_offsets = Vec::new();
    let mut ee_name_bit_rewrites = Vec::new();
    let mut ee_extra_byte_inserts = Vec::new();
    let mut preferred_zero_padding_relative_start = None;
    let mut token_selector_padding_repair_relative_start = None;
    let mut inline_active_name_fence_repair_relative_start = None;
    let translated_ee_bit_proof = dialect == CreatureAppearanceWireDialect::EeBuild8193;
    if (mask & LEGACY_APPEARANCE_NAME_MASK) != 0 {
        if let Some(proof) = bit_proof {
            let Some(selector) = proof.fragment_bits.get(proof.bit_cursor).copied() else {
                return None;
            };
            if selector != name_shape.fragment_bit() {
                return None;
            }
        }
        fragment_bits_consumed = fragment_bits_consumed.checked_add(1)?;
        match name_shape {
            AppearanceNameShape::LocStringPair => {
                if let Some(proof) = bit_proof {
                    let mut component_bit_cursor = proof.bit_cursor.checked_add(1)?;
                    let first = advance_legacy_locstring_component_with_proof(
                        bytes,
                        cursor,
                        limit,
                        MAX_LIVE_OBJECT_NAME_BYTES,
                        proof,
                        component_bit_cursor,
                    )?;
                    cursor = first.end;
                    component_bit_cursor =
                        component_bit_cursor.checked_add(first.fragment_bits_consumed)?;

                    let second = advance_legacy_locstring_component_with_proof(
                        bytes,
                        cursor,
                        limit,
                        MAX_LIVE_OBJECT_NAME_BYTES,
                        proof,
                        component_bit_cursor,
                    )?;
                    cursor = second.end;
                    fragment_bits_consumed = fragment_bits_consumed
                        .checked_add(first.fragment_bits_consumed)?
                        .checked_add(second.fragment_bits_consumed)?;
                } else {
                    fragment_bits_consumed = fragment_bits_consumed.checked_add(2)?;
                    cursor =
                        advance_message_string(bytes, cursor, limit, MAX_LIVE_OBJECT_NAME_BYTES)?;
                    if let Some(standard_second_end) =
                        advance_message_string(bytes, cursor, limit, MAX_LIVE_OBJECT_NAME_BYTES)
                    {
                        cursor = standard_second_end;
                    } else {
                        let candidate = select_missing_second_inline_name_candidate(
                            bytes, cursor, limit, mask, bit_proof,
                        )?;
                        ee_extra_byte_inserts.push(
                            CreatureAppearanceByteInsert::MissingSecondInlineNameLength {
                                offset: cursor,
                                length: u32::try_from(candidate.name_len).ok()?,
                            },
                        );
                        cursor = candidate.name_end;
                    }
                }
            }
            AppearanceNameShape::CExoString => {
                cursor = advance_message_string(bytes, cursor, limit, MAX_LIVE_OBJECT_NAME_BYTES)?;
            }
        }
    }

    cursor = advance_creature_appearance_scalar_fields(
        bytes,
        cursor,
        limit,
        mask,
        dialect,
        &mut ee_extra_byte_inserts,
    )?;

    if mask == LEGACY_APPEARANCE_ALL_FIELDS_MASK {
        cursor = advance_creature_appearance_body_fields(
            bytes,
            cursor,
            limit,
            dialect,
            &mut ee_extra_byte_inserts,
        )?;
    } else if (mask & LEGACY_APPEARANCE_BODY_PART_MASK) != 0 {
        // The partial body-part delta path is a different decompiled branch
        // with count/delta semantics. Do not claim it until we have a capture
        // fixture and exact reader model for that branch.
        return None;
    }

    if (mask & 0x2000) != 0 {
        cursor = cursor.checked_add(2 + 4)?;
        if cursor > limit {
            return None;
        }
    }

    if (mask & 0x4000) != 0 {
        match dialect {
            CreatureAppearanceWireDialect::LegacyDiamond => {
                // EE `sub_14077FE10` gates this byte on
                // `ServerSatisfiesBuild(0x2001, 0x0E, 0)`. BNVR advertises the
                // proxy-owned `0x2001/0x23` EE-facing dialect, so a Diamond
                // source packet with mask bit 0x4000 needs a neutral byte before
                // the visible-equipment count.
                ee_extra_byte_inserts.push(
                    CreatureAppearanceByteInsert::EeFeature0eCreatureTailByte { offset: cursor },
                );
            }
            CreatureAppearanceWireDialect::EeBuild8193 => {
                cursor = cursor.checked_add(1)?;
                if cursor > limit {
                    return None;
                }
            }
        }
    }

    let equipment_records = if mask == LEGACY_APPEARANCE_ALL_FIELDS_MASK {
        let count = *bytes.get(cursor)?;
        // The decompiled all-fields writer emits a bounded visible-
        // equipment count followed by that many `A`/`D` item records. Zero
        // is a valid exact shape for creatures with no visible equipment;
        // the recursive item parser below already treats `remaining == 0`
        // as consuming no bytes or fragment bits.
        if count > LEGACY_APPEARANCE_MAX_EQUIPMENT_RECORDS {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        let require_translated_byte_shape = translated_ee_bit_proof;
        let equipment = parse_legacy_visible_equipment_records(
            bytes,
            cursor,
            limit,
            count,
            require_translated_byte_shape,
            bit_proof,
            fragment_bits_consumed,
            ee_extra_fragment_bits,
        )?;
        cursor = equipment.end;
        preferred_zero_padding_relative_start =
            equipment.first_positive_name_selector_relative_start;
        token_selector_padding_repair_relative_start =
            equipment.token_selector_padding_repair_relative_start;
        inline_active_name_fence_repair_relative_start =
            equipment.inline_active_name_fence_repair_relative_start;
        ee_extra_insert_offsets.extend(
            equipment
                .ee_extra_insert_offsets
                .iter()
                .map(|relative| fragment_bits_consumed.saturating_add(*relative)),
        );
        ee_name_bit_rewrites.extend(equipment.ee_name_bit_rewrites.iter().map(|rewrite| {
            FragmentNameBitRewrite {
                relative_offset: fragment_bits_consumed.saturating_add(rewrite.relative_offset),
                proof: rewrite.proof,
            }
        }));
        ee_extra_byte_inserts.extend(equipment.ee_extra_byte_inserts);
        fragment_bits_consumed =
            fragment_bits_consumed.checked_add(equipment.fragment_bits_consumed)?;
        ee_extra_fragment_bits =
            ee_extra_fragment_bits.checked_add(equipment.ee_extra_fragment_bits)?;
        count
    } else if (mask & LEGACY_APPEARANCE_EQUIPMENT_DELTA_MASK) != 0 {
        // The non-`0xFFFF` equipment-delta branch compares individual slots
        // and writes a compact change list. Keep it quarantined until modeled.
        return None;
    } else {
        0
    };

    let may_probe_following_creature_update_fence = cursor < limit
        || (cursor == limit
            && bit_proof
                .map(|proof| proof.allow_cross_record_fence || proof.translated_ee)
                .unwrap_or(false));
    if mask == LEGACY_APPEARANCE_ALL_FIELDS_MASK
        && equipment_records != 0
        && may_probe_following_creature_update_fence
        && cursor < bytes.len()
        && bytes.get(cursor).copied() == Some(b'U')
        && bytes.get(cursor + 1).copied() == Some(LEGACY_CREATURE_TYPE)
    {
        // `limit` is the logical end of the appearance record, but the CNW
        // fragment cursor is shared by the immediately following live-object
        // record. When the next record starts exactly at `limit`, the
        // decompiled following `U/5` reader still proves how many packetized
        // fence bits the appearance-side cursor must account for before that
        // reader starts. This is transport cursor accounting, not an invented
        // appearance field: the fence is accepted only when the focused
        // creature-update parser consumes the following record from that exact
        // post-fence cursor.
        // Diamond's `sub_448E30` proves the semantic full-appearance record by
        // reading the name selectors, scalar fields, body table, and visible
        // equipment item records. Captured HG streams can then place the
        // following creature `U/5` update behind a packetized CNW fragment
        // fence. The minimal fence is the three-bit MSB valid-bit header; some
        // verified streams carry the two low bits that the following
        // decompiled `ReadUnsigned(..., 18)` field will otherwise see as a
        // false vector/target selector. Do not guess: accept a fence width only
        // when the focused creature-update validator consumes the following
        // `U/5` record exactly at that cursor.
        let fence_bits = if let Some(proof) = bit_proof {
            select_following_creature_update_fragment_fence_bits(
                bytes,
                cursor,
                proof,
                fragment_bits_consumed,
                ee_extra_fragment_bits,
            )
        } else {
            Some(CNW_FRAGMENT_HEADER_BITS)
        };
        if let Some(fence_bits) = fence_bits {
            fragment_bits_consumed = fragment_bits_consumed.checked_add(fence_bits)?;
        } else if cursor < limit {
            return None;
        }
    }

    trace_appearance_record(
        offset,
        object_id,
        mask,
        name_shape,
        cursor,
        fragment_bits_consumed,
        ee_extra_fragment_bits,
        equipment_records,
        bit_proof,
    );

    Some(LegacyAppearanceRecord {
        record_end: cursor,
        fragment_bits_consumed,
        ee_fragment_bits_consumed: fragment_bits_consumed.checked_add(ee_extra_fragment_bits)?,
        ee_extra_insert_offsets,
        ee_name_bit_rewrites,
        ee_extra_byte_inserts,
        equipment_records,
        preferred_zero_padding_relative_start,
        token_selector_padding_repair_relative_start,
        inline_active_name_fence_repair_relative_start,
    })
}

fn select_missing_second_inline_name_candidate(
    bytes: &[u8],
    second_name_offset: usize,
    limit: usize,
    mask: u16,
    bit_proof: Option<AppearanceBitProof<'_>>,
) -> Option<MissingSecondInlineNameCandidate> {
    if mask != LEGACY_APPEARANCE_ALL_FIELDS_MASK {
        return None;
    }
    if let Some(proof) = bit_proof {
        let first_inner = proof.bit_cursor.checked_add(1)?;
        let second_inner = proof.bit_cursor.checked_add(2)?;
        if proof
            .fragment_bits
            .get(first_inner)
            .copied()
            .unwrap_or(true)
            || proof
                .fragment_bits
                .get(second_inner)
                .copied()
                .unwrap_or(true)
        {
            return None;
        }
    }

    let scan_limit = limit
        .min(bytes.len())
        .min(second_name_offset.checked_add(MAX_LIVE_OBJECT_NAME_BYTES)?);
    let mut accepted: Option<MissingSecondInlineNameCandidate> = None;
    for name_end in second_name_offset.checked_add(1)?..=scan_limit {
        let name = bytes.get(second_name_offset..name_end)?;
        if !legacy_missing_second_name_bytes_are_inline_printable(name) {
            continue;
        }
        let Some(candidate) =
            score_missing_second_inline_name_tail(bytes, name_end, limit, name.len())
        else {
            continue;
        };
        let better = accepted
            .as_ref()
            .map(|current| {
                candidate.equipment_records > current.equipment_records
                    || (candidate.equipment_records == current.equipment_records
                        && candidate.record_end > current.record_end)
            })
            .unwrap_or(true);
        if better {
            accepted = Some(candidate);
        }
    }
    accepted
}

fn advance_creature_appearance_body_fields(
    bytes: &[u8],
    mut cursor: usize,
    limit: usize,
    dialect: CreatureAppearanceWireDialect,
    ee_extra_byte_inserts: &mut Vec<CreatureAppearanceByteInsert>,
) -> Option<usize> {
    if matches!(dialect, CreatureAppearanceWireDialect::LegacyDiamond) {
        let prefixed_full_count_offset = cursor.checked_add(4)?;
        let prefixed_full_values_offset = cursor.checked_add(5)?;
        let prefixed_full_end = cursor.checked_add(24)?;
        if prefixed_full_end <= limit
            && prefixed_full_end <= bytes.len()
            && bytes.get(prefixed_full_count_offset).copied()
                == Some(LEGACY_APPEARANCE_BODY_PART_COUNT)
        {
            // Verified HG full-state creature appearances can carry four
            // legacy bytes immediately before Diamond's 0x13 full body table.
            // EE's reader starts at the count byte, so the bridge drops only
            // that proven prefix and widens the 19 BYTE part values for the
            // proxy-owned 0x2001/0x23 EE dialect.
            ee_extra_byte_inserts.push(
                CreatureAppearanceByteInsert::LegacyFullPartTablePrefixRemoval {
                    offset: cursor,
                    bytes: 4,
                },
            );
            for index in 0..usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT) {
                ee_extra_byte_inserts.push(
                    CreatureAppearanceByteInsert::EeFeature23CreatureBodyPartHighByte {
                        offset: prefixed_full_values_offset
                            .checked_add(index)?
                            .checked_add(1)?,
                    },
                );
            }
            return Some(prefixed_full_end);
        }

        let zero_prefixed_full_count_offset = cursor.checked_add(8)?;
        let zero_prefixed_full_values_offset = cursor.checked_add(9)?;
        let zero_prefixed_full_end = cursor.checked_add(28)?;
        if zero_prefixed_full_end <= limit
            && zero_prefixed_full_end <= bytes.len()
            && bytes.get(cursor..cursor.checked_add(4)?) == Some(&[0, 0, 0, 0][..])
            && bytes.get(zero_prefixed_full_count_offset).copied()
                == Some(LEGACY_APPEARANCE_BODY_PART_COUNT)
        {
            // Same decompile-backed prefix shape with an additional four-byte
            // zero pad observed in short/promoted HG creature streams. Treat it
            // as prefix removal only when the rest of the typed appearance
            // record validates.
            ee_extra_byte_inserts.push(
                CreatureAppearanceByteInsert::LegacyFullPartTablePrefixRemoval {
                    offset: cursor,
                    bytes: 8,
                },
            );
            for index in 0..usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT) {
                ee_extra_byte_inserts.push(
                    CreatureAppearanceByteInsert::EeFeature23CreatureBodyPartHighByte {
                        offset: zero_prefixed_full_values_offset
                            .checked_add(index)?
                            .checked_add(1)?,
                    },
                );
            }
            return Some(zero_prefixed_full_end);
        }
    }

    let direct_selector = *bytes.get(cursor)?;
    if direct_selector == 0 {
        // Diamond `sub_448E30` still enters the body-appearance branch when
        // mask bit `0x0100` is set, but a zero selector consumes only that
        // byte and leaves the existing body table unchanged.  Treating
        // `0xFFFF` as "must carry a full 19-part table" made compact local
        // Diamond current-creature appearances unclaimable even though the
        // decompiled reader accepts them exactly.
        return cursor.checked_add(1).filter(|end| *end <= limit);
    }
    if direct_selector < 0x0A {
        // The same reader accepts a compact delta table: selector count, then
        // count pairs of body-part index/value bytes.  This branch is byte-
        // identical in the Diamond reader path; EE high-byte expansion is only
        // needed for the full 19-part table below.
        return cursor
            .checked_add(1)?
            .checked_add(usize::from(direct_selector).checked_mul(2)?)
            .filter(|end| *end <= limit);
    }

    // Diamond `sub_448E30` compares the selector against `0x0A`; selectors
    // `1..=9` are compact index/value deltas, while any selector `>= 0x0A`
    // enters the fixed full-body branch and reads exactly 19 body-part bytes.
    // The selector is not itself required to equal `0x13`.
    cursor = cursor.checked_add(1)?;
    match dialect {
        CreatureAppearanceWireDialect::LegacyDiamond => {
            for _ in 0..LEGACY_APPEARANCE_BODY_PART_COUNT {
                cursor = cursor.checked_add(1)?;
                ee_extra_byte_inserts.push(
                    CreatureAppearanceByteInsert::EeFeature23CreatureBodyPartHighByte {
                        offset: cursor,
                    },
                );
            }
        }
        CreatureAppearanceWireDialect::EeBuild8193 => {
            cursor = cursor
                .checked_add(usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT).checked_mul(2)?)?;
        }
    }
    (cursor <= limit).then_some(cursor)
}

fn legacy_missing_second_name_bytes_are_inline_printable(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| matches!(byte, 0x09 | 0x0A | 0x0D | 0x20..=0x7E | 0x80..=0xFF))
}

fn score_missing_second_inline_name_tail(
    bytes: &[u8],
    name_end: usize,
    limit: usize,
    name_len: usize,
) -> Option<MissingSecondInlineNameCandidate> {
    let mut cursor = advance_legacy_scalar_appearance_fields(
        bytes,
        name_end,
        limit,
        LEGACY_APPEARANCE_ALL_FIELDS_MASK,
    )?;
    let mut ignored_inserts = Vec::new();
    cursor = advance_creature_appearance_body_fields(
        bytes,
        cursor,
        limit,
        CreatureAppearanceWireDialect::LegacyDiamond,
        &mut ignored_inserts,
    )?;
    cursor = cursor.checked_add(2 + 4)?;
    if cursor > limit {
        return None;
    }
    let equipment_records = *bytes.get(cursor)?;
    if equipment_records > LEGACY_APPEARANCE_MAX_EQUIPMENT_RECORDS {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let equipment = parse_legacy_visible_equipment_records(
        bytes,
        cursor,
        limit,
        equipment_records,
        false,
        None,
        0,
        0,
    )?;
    Some(MissingSecondInlineNameCandidate {
        name_end,
        name_len,
        record_end: equipment.end,
        equipment_records,
    })
}

fn select_following_creature_update_fragment_fence_bits(
    bytes: &[u8],
    following_offset: usize,
    proof: AppearanceBitProof<'_>,
    legacy_bits_before_fence: usize,
    ee_extra_bits_before_fence: usize,
) -> Option<usize> {
    let translated_delta = if proof.translated_ee {
        ee_extra_bits_before_fence
    } else {
        0
    };
    let base_cursor = proof
        .bit_cursor
        .checked_add(legacy_bits_before_fence)?
        .checked_add(translated_delta)?;
    if base_cursor > proof.fragment_bits.len() {
        return None;
    }

    let following_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
        bytes,
        following_offset,
        bytes.len(),
    )
    .min(bytes.len());
    if following_end <= following_offset {
        return None;
    }

    if following_creature_update_validates_after_optional_action0_bridge_rewrite(
        bytes,
        following_offset,
        following_end,
        proof.fragment_bits,
        base_cursor,
    ) {
        trace_appearance_fence_candidate(
            following_offset,
            base_cursor,
            0,
            proof.translated_ee,
            true,
            "no-fence-creature-update",
        );
        return Some(0);
    }

    if proof.translated_ee {
        // EE's live-object dispatcher reaches the following `U/5` record as a
        // separate submessage. A translated appearance record may therefore
        // prove its own byte/fragment shape, but it must not consume or imply a
        // fragment fence on behalf of the next update. If the next update does
        // not validate at the exact post-appearance cursor, this appearance
        // proof is not a safe stream boundary and another candidate must win.
        trace_appearance_fence_candidate(
            following_offset,
            base_cursor,
            0,
            proof.translated_ee,
            false,
            "ee-cross-record-fence-rejected-unverified-following-update",
        );
        return None;
    }

    for fence_bits in LEGACY_FULL_APPEARANCE_FOLLOWING_CREATURE_UPDATE_FRAGMENT_FENCE_CANDIDATES {
        let Some(probe_cursor) = base_cursor.checked_add(fence_bits) else {
            continue;
        };
        if probe_cursor > proof.fragment_bits.len() {
            continue;
        }
        if following_creature_update_validates_after_optional_action0_bridge_rewrite(
            bytes,
            following_offset,
            following_end,
            proof.fragment_bits,
            probe_cursor,
        ) {
            trace_appearance_fence_candidate(
                following_offset,
                base_cursor,
                fence_bits,
                proof.translated_ee,
                true,
                "direct-creature-update",
            );
            return Some(fence_bits);
        }
        if fragment_spans::verified_creature_update_3967_read_end_before_interleaved_fragment_span(
            bytes,
            following_offset,
            following_end,
            proof.fragment_bits,
            probe_cursor,
        )
        .is_some()
        {
            trace_appearance_fence_candidate(
                following_offset,
                base_cursor,
                fence_bits,
                proof.translated_ee,
                true,
                "interleaved-creature-update",
            );
            return Some(fence_bits);
        }
        trace_appearance_fence_candidate(
            following_offset,
            base_cursor,
            fence_bits,
            proof.translated_ee,
            false,
            "rejected",
        );
    }

    None
}

fn following_creature_update_validates_after_optional_action0_bridge_rewrite(
    bytes: &[u8],
    following_offset: usize,
    following_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> bool {
    let mut proof_cursor = bit_cursor;
    if super::creature::advance_verified_noop_creature_update_record_exact_cursor(
        bytes,
        following_offset,
        following_end,
        fragment_bits,
        &mut proof_cursor,
    ) {
        return true;
    }

    let mut action0_trial_bytes = bytes.to_vec();
    let mut action0_trial_fragment_bits = fragment_bits.to_vec();
    let mut action0_following_end = following_end;
    if super::creature::remove_3967_action0_legacy_bridge_followup_for_ee(
        &mut action0_trial_bytes,
        following_offset,
        &mut action0_following_end,
        &mut action0_trial_fragment_bits,
        bit_cursor,
    )
    .is_none()
    {
        return false;
    }

    let mut action0_probe_cursor = bit_cursor;
    super::creature::advance_verified_noop_creature_update_record_exact_cursor(
        &action0_trial_bytes,
        following_offset,
        action0_following_end,
        &action0_trial_fragment_bits,
        &mut action0_probe_cursor,
    )
}

fn trace_appearance_record(
    offset: usize,
    object_id: u32,
    mask: u16,
    name_shape: AppearanceNameShape,
    record_end: usize,
    fragment_bits_consumed: usize,
    ee_extra_fragment_bits: usize,
    equipment_records: u8,
    bit_proof: Option<AppearanceBitProof<'_>>,
) {
    if !debug_live_claim_enabled_for_offset(offset) {
        return;
    }
    let proof_cursor = bit_proof.map(|proof| proof.bit_cursor);
    let translated_ee = bit_proof.map(|proof| proof.translated_ee).unwrap_or(false);
    let ee_bits = fragment_bits_consumed.saturating_add(ee_extra_fragment_bits);
    eprintln!(
        "live-object appearance accepted: offset={offset} object_id=0x{object_id:08X} mask=0x{mask:04X} name_shape={name_shape:?} record_end={record_end} proof_cursor={proof_cursor:?} translated_ee={translated_ee} fragment_bits={fragment_bits_consumed} ee_extra_bits={ee_extra_fragment_bits} ee_bits={ee_bits} equipment_records={equipment_records}"
    );
}

fn trace_appearance_fence_candidate(
    following_offset: usize,
    base_cursor: usize,
    fence_bits: usize,
    translated_ee: bool,
    accepted: bool,
    reason: &'static str,
) {
    if !debug_live_claim_enabled_for_offset(following_offset) {
        return;
    }
    let probe_cursor = base_cursor.saturating_add(fence_bits);
    eprintln!(
        "live-object appearance fence candidate: following_offset={following_offset} base_cursor={base_cursor} fence_bits={fence_bits} probe_cursor={probe_cursor} translated_ee={translated_ee} accepted={accepted} reason={reason}"
    );
}

fn trace_preceding_appearance_fence(
    offset: usize,
    original_cursor: usize,
    verified: &VerifiedAppearanceParse,
) {
    if !debug_live_claim_enabled_for_offset(offset) {
        return;
    }
    eprintln!(
        "live-object appearance preceding fence accepted: offset={offset} original_cursor={original_cursor} fence_bits={} proof_cursor={} record_end={} fragment_bits={} ee_bits={}",
        verified.preceding_fence_bits,
        verified.proof_cursor,
        verified.record.record_end,
        verified.record.fragment_bits_consumed,
        verified.record.ee_fragment_bits_consumed,
    );
}

#[derive(Debug, Clone, Copy)]
struct LegacyLocStringComponentAdvance {
    end: usize,
    fragment_bits_consumed: usize,
}

fn advance_legacy_locstring_component_with_proof(
    bytes: &[u8],
    cursor: usize,
    limit: usize,
    max_len: usize,
    proof: AppearanceBitProof<'_>,
    component_bit_cursor: usize,
) -> Option<LegacyLocStringComponentAdvance> {
    let inner_is_tlk_token = *proof.fragment_bits.get(component_bit_cursor)?;
    if inner_is_tlk_token {
        let end = cursor.checked_add(1)?.checked_add(4)?;
        if end > limit || end > bytes.len() {
            return None;
        }
        Some(LegacyLocStringComponentAdvance {
            end,
            fragment_bits_consumed: 1,
        })
    } else {
        Some(LegacyLocStringComponentAdvance {
            end: advance_message_string(bytes, cursor, limit, max_len)?,
            fragment_bits_consumed: 1,
        })
    }
}

fn advance_legacy_scalar_appearance_fields(
    bytes: &[u8],
    cursor: usize,
    limit: usize,
    mask: u16,
) -> Option<usize> {
    let mut ignored_inserts = Vec::new();
    advance_creature_appearance_scalar_fields(
        bytes,
        cursor,
        limit,
        mask,
        CreatureAppearanceWireDialect::LegacyDiamond,
        &mut ignored_inserts,
    )
}

fn advance_creature_appearance_scalar_fields(
    bytes: &[u8],
    mut cursor: usize,
    limit: usize,
    mask: u16,
    dialect: CreatureAppearanceWireDialect,
    ee_extra_byte_inserts: &mut Vec<CreatureAppearanceByteInsert>,
) -> Option<usize> {
    if (mask & 0x0001) != 0 {
        cursor = cursor.checked_add(2)?;
    }
    if (mask & 0x0002) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0004) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0080) != 0 {
        cursor = cursor.checked_add(1)?;
        match dialect {
            CreatureAppearanceWireDialect::LegacyDiamond => {
                // BNVR advertises the proxy-owned `0x2001/0x23` EE-facing
                // dialect, and EE `sub_14077FE10` reads this appearance scalar
                // as a WORD in that branch. Diamond writes the compact BYTE, so
                // the bridge inserts a neutral high byte instead of letting the
                // following DWORD field become the high half of this value.
                ee_extra_byte_inserts.push(
                    CreatureAppearanceByteInsert::EeFeature23CreatureScalarHighByte {
                        offset: cursor,
                    },
                );
            }
            CreatureAppearanceWireDialect::EeBuild8193 => {
                cursor = cursor.checked_add(1)?;
            }
        }
    }
    if (mask & 0x0800) != 0 {
        cursor = cursor.checked_add(4)?;
    }
    if (mask & 0x1000) != 0 {
        cursor = cursor.checked_add(4)?;
    }
    if (mask & 0x0008) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0010) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0020) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x0040) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if cursor > limit || cursor > bytes.len() {
        return None;
    }
    Some(cursor)
}

fn parse_legacy_visible_equipment_records(
    bytes: &[u8],
    cursor: usize,
    limit: usize,
    remaining: u8,
    require_translated_byte_shape: bool,
    bit_proof: Option<AppearanceBitProof<'_>>,
    legacy_bits_before: usize,
    ee_extra_bits_before: usize,
) -> Option<LegacyVisibleEquipmentParse> {
    if remaining == 0 {
        return Some(LegacyVisibleEquipmentParse {
            end: cursor,
            fragment_bits_consumed: 0,
            ee_extra_fragment_bits: 0,
            ee_extra_insert_offsets: Vec::new(),
            ee_name_bit_rewrites: Vec::new(),
            ee_extra_byte_inserts: Vec::new(),
            first_positive_name_selector_relative_start: None,
            token_selector_padding_repair_relative_start: None,
            inline_active_name_fence_repair_relative_start: None,
        });
    }
    if cursor >= limit {
        return None;
    }

    match *bytes.get(cursor)? {
        b'D' => {
            let next = cursor.checked_add(1 + 4 + 4)?;
            if next > limit
                || read_u32_le(bytes, cursor + 1)? != LEGACY_APPEARANCE_DUMMY_ITEM_OBJECT_ID
            {
                return None;
            }
            let slot = read_u32_le(bytes, cursor + 5)?;
            if !is_legacy_visible_equipment_slot(slot) {
                return None;
            }
            parse_legacy_visible_equipment_records(
                bytes,
                next,
                limit,
                remaining - 1,
                require_translated_byte_shape,
                bit_proof,
                legacy_bits_before,
                ee_extra_bits_before,
            )
        }
        b'A' => {
            if remaining == 1 {
                let min_next =
                    cursor.checked_add(1 + 4 + 4 + LEGACY_APPEARANCE_MIN_ITEM_NAME_TAIL_BYTES)?;
                let max_next = cursor
                    .checked_add(LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)
                    .map(|end| end.min(limit))
                    .unwrap_or(limit);
                let mut accepted: Option<LegacyVisibleEquipmentParse> = None;
                for next in min_next..=max_next {
                    for item in parse_legacy_item_add_record_candidates(bytes, cursor, next) {
                        let spans_record_boundary =
                            visible_equipment_item_candidate_spans_live_object_boundary(
                                bytes, cursor, next,
                            );
                        let has_pending_byte_inserts = !item.ee_extra_byte_inserts.is_empty();
                        if require_translated_byte_shape && has_pending_byte_inserts {
                            trace_visible_equipment_parse_skip(
                                cursor,
                                next,
                                remaining,
                                item.ee_extra_byte_inserts.len(),
                                "translated-shape-still-needs-byte-inserts",
                            );
                            if spans_record_boundary {
                                return None;
                            }
                            continue;
                        }
                        if spans_record_boundary {
                            continue;
                        }
                        if !item_add_record_matches_bit_proof(
                            &item,
                            bit_proof,
                            legacy_bits_before,
                            ee_extra_bits_before,
                        ) {
                            continue;
                        }
                        let ee_name_bit_rewrites = item_name_bit_rewrites(&item, 0);
                        let first_positive_name_selector_relative_start = item
                            .name_fragment_proof
                            .starts_with_positive_selector()
                            .then_some(legacy_bits_before);
                        let token_selector_padding_repair_relative_start =
                            legacy_item_token_selector_padding_repair_relative_start(
                                &item,
                                legacy_bits_before,
                            );
                        let inline_active_name_fence_repair_relative_start =
                            legacy_item_inline_active_name_fence_repair_relative_start(
                                &item,
                                legacy_bits_before,
                            );
                        let candidate = LegacyVisibleEquipmentParse {
                            end: next,
                            fragment_bits_consumed: item.fragment_bits_consumed,
                            ee_extra_fragment_bits: item.ee_extra_fragment_bits,
                            ee_extra_insert_offsets: item.ee_extra_insert_offsets,
                            ee_name_bit_rewrites,
                            ee_extra_byte_inserts: item.ee_extra_byte_inserts,
                            first_positive_name_selector_relative_start,
                            token_selector_padding_repair_relative_start,
                            inline_active_name_fence_repair_relative_start,
                        };
                        if accepted
                            .as_ref()
                            .map(|current| {
                                candidate.end > current.end
                                    || (!has_pending_byte_inserts
                                        && !current.ee_extra_byte_inserts.is_empty()
                                        && candidate.end == current.end)
                            })
                            .unwrap_or(true)
                        {
                            accepted = Some(candidate);
                        }
                    }
                }
                return accepted;
            }

            let min_next =
                cursor.checked_add(1 + 4 + 4 + LEGACY_APPEARANCE_MIN_ITEM_NAME_TAIL_BYTES)?;
            let max_next = cursor
                .checked_add(LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES)
                .map(|end| end.min(limit))
                .unwrap_or(limit);
            let mut accepted: Option<LegacyVisibleEquipmentParse> = None;
            for next in min_next..max_next {
                if !matches!(bytes.get(next).copied(), Some(b'A' | b'D')) {
                    continue;
                }
                for item in parse_legacy_item_add_record_candidates(bytes, cursor, next) {
                    if require_translated_byte_shape && !item.ee_extra_byte_inserts.is_empty() {
                        trace_visible_equipment_parse_skip(
                            cursor,
                            next,
                            remaining,
                            item.ee_extra_byte_inserts.len(),
                            "translated-shape-still-needs-byte-inserts",
                        );
                        continue;
                    }
                    if !item_add_record_matches_bit_proof(
                        &item,
                        bit_proof,
                        legacy_bits_before,
                        ee_extra_bits_before,
                    ) {
                        continue;
                    }
                    if let Some(rest) = parse_legacy_visible_equipment_records(
                        bytes,
                        next,
                        limit,
                        remaining - 1,
                        require_translated_byte_shape,
                        bit_proof,
                        legacy_bits_before.checked_add(item.fragment_bits_consumed)?,
                        ee_extra_bits_before.checked_add(item.ee_extra_fragment_bits)?,
                    ) {
                        let mut ee_name_bit_rewrites = item_name_bit_rewrites(&item, 0);
                        let first_positive_name_selector_relative_start = item
                            .name_fragment_proof
                            .starts_with_positive_selector()
                            .then_some(legacy_bits_before)
                            .or(rest.first_positive_name_selector_relative_start);
                        let token_selector_padding_repair_relative_start =
                            legacy_item_token_selector_padding_repair_relative_start(
                                &item,
                                legacy_bits_before,
                            )
                            .or(rest.token_selector_padding_repair_relative_start);
                        let inline_active_name_fence_repair_relative_start =
                            legacy_item_inline_active_name_fence_repair_relative_start(
                                &item,
                                legacy_bits_before,
                            )
                            .or(rest.inline_active_name_fence_repair_relative_start);
                        let mut ee_extra_insert_offsets = item.ee_extra_insert_offsets;
                        ee_extra_insert_offsets.extend(
                            rest.ee_extra_insert_offsets.iter().map(|relative| {
                                item.fragment_bits_consumed.saturating_add(*relative)
                            }),
                        );
                        ee_name_bit_rewrites.extend(rest.ee_name_bit_rewrites.iter().map(
                            |rewrite| {
                                FragmentNameBitRewrite {
                                    relative_offset: item
                                        .fragment_bits_consumed
                                        .saturating_add(rewrite.relative_offset),
                                    proof: rewrite.proof,
                                }
                            },
                        ));
                        let mut ee_extra_byte_inserts = item.ee_extra_byte_inserts;
                        ee_extra_byte_inserts.extend(rest.ee_extra_byte_inserts);
                        let candidate = LegacyVisibleEquipmentParse {
                            end: rest.end,
                            fragment_bits_consumed: item
                                .fragment_bits_consumed
                                .checked_add(rest.fragment_bits_consumed)?,
                            ee_extra_fragment_bits: item
                                .ee_extra_fragment_bits
                                .checked_add(rest.ee_extra_fragment_bits)?,
                            ee_extra_insert_offsets,
                            ee_name_bit_rewrites,
                            ee_extra_byte_inserts,
                            first_positive_name_selector_relative_start,
                            token_selector_padding_repair_relative_start,
                            inline_active_name_fence_repair_relative_start,
                        };
                        // The decompiled all-fields appearance record carries
                        // an explicit visible-equipment count. When more than
                        // one byte-plausible split satisfies the remaining
                        // count, prefer the split that proves the furthest
                        // equipment-list boundary. This prevents an `A` or `D`
                        // byte inside an item name/properties blob from
                        // becoming the next embedded equipment record while a
                        // later exact split consumes the full counted list.
                        if accepted
                            .as_ref()
                            .map(|current| candidate.end > current.end)
                            .unwrap_or(true)
                        {
                            accepted = Some(candidate);
                        }
                    }
                }
            }
            accepted
        }
        _ => None,
    }
}

fn item_add_record_matches_bit_proof(
    item: &LegacyAppearanceItemAddRecord,
    bit_proof: Option<AppearanceBitProof<'_>>,
    legacy_bits_before: usize,
    ee_extra_bits_before: usize,
) -> bool {
    let Some(proof) = bit_proof else {
        return true;
    };
    let Some(start_cursor) = proof
        .bit_cursor
        .checked_add(legacy_bits_before)
        .and_then(|cursor| {
            if proof.translated_ee {
                cursor.checked_add(ee_extra_bits_before)
            } else {
                Some(cursor)
            }
        })
    else {
        return false;
    };
    let translated_delta = if proof.translated_ee {
        item.ee_extra_fragment_bits
    } else {
        0
    };
    let Some(consumed_bits) = item.fragment_bits_consumed.checked_add(translated_delta) else {
        return false;
    };
    if consumed_bits > proof.fragment_bits.len().saturating_sub(start_cursor) {
        trace_visible_equipment_bit_proof_reject(
            "insufficient-fragment-bits",
            start_cursor,
            consumed_bits,
            item,
            proof,
        );
        return false;
    }
    if !item
        .name_fragment_proof
        .matches(proof.fragment_bits, start_cursor)
    {
        trace_visible_equipment_bit_proof_reject(
            "name-proof-mismatch",
            start_cursor,
            consumed_bits,
            item,
            proof,
        );
        return false;
    }
    if proof.translated_ee {
        for relative_offset in item.ee_extra_insert_offsets.iter().copied() {
            let Some(bit) = start_cursor
                .checked_add(relative_offset)
                .and_then(|index| proof.fragment_bits.get(index))
            else {
                trace_visible_equipment_bit_proof_reject(
                    "missing-ee-extra-bit",
                    start_cursor,
                    consumed_bits,
                    item,
                    proof,
                );
                return false;
            };
            if *bit {
                trace_visible_equipment_bit_proof_reject(
                    "ee-extra-bit-not-false",
                    start_cursor,
                    consumed_bits,
                    item,
                    proof,
                );
                return false;
            }
        }
    }
    trace_visible_equipment_bit_proof_accept(start_cursor, consumed_bits, item, proof);
    true
}

fn legacy_item_token_selector_padding_repair_relative_start(
    item: &LegacyAppearanceItemAddRecord,
    legacy_bits_before: usize,
) -> Option<usize> {
    // Diamond `sub_451020` reads the item-name outer BOOL from the fragment
    // stream and, when true, immediately calls the locstring helper
    // `sub_53E700`. The token branch of that helper reads the inner BOOL as
    // true, then a language bit, then the DWORD TLK token from the read buffer.
    //
    // If the byte parser has already proven the item-name byte shape is that
    // DWORD-token branch, the only permitted promoted-padding repair inside
    // the selector pair is the single zero bit between the outer and inner
    // selectors. This keeps the compatibility transform owned by the typed
    // item-name subobject instead of letting the creature-appearance wrapper
    // delete arbitrary zero bits from active-property or later fields.
    if item.name_fragment_proof != LegacyItemNameFragmentProof::LocStringToken {
        return None;
    }
    legacy_bits_before.checked_add(1)
}

fn legacy_item_inline_active_name_fence_repair_relative_start(
    item: &LegacyAppearanceItemAddRecord,
    legacy_bits_before: usize,
) -> Option<usize> {
    // Diamond `sub_451020` consumes the item-name selector as the next fragment
    // bit after the visible-equipment item appearance path. A byte-proven direct
    // `CExoString` item name therefore gives us a narrow anchor for detecting
    // promoted non-semantic fence bits immediately before that selector.
    (item.name_fragment_proof == LegacyItemNameFragmentProof::InlineCExoString)
        .then_some(legacy_bits_before)
}

fn trace_visible_equipment_bit_proof_reject(
    reason: &'static str,
    start_cursor: usize,
    consumed_bits: usize,
    item: &LegacyAppearanceItemAddRecord,
    proof: AppearanceBitProof<'_>,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF").is_none() {
        return;
    }
    if let Ok(min_cursor) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF_MIN_CURSOR") {
        if min_cursor
            .parse::<usize>()
            .map(|min_cursor| start_cursor < min_cursor)
            .unwrap_or(false)
        {
            return;
        }
    }
    if let Ok(max_cursor) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF_MAX_CURSOR") {
        if max_cursor
            .parse::<usize>()
            .map(|max_cursor| start_cursor > max_cursor)
            .unwrap_or(false)
        {
            return;
        }
    }
    if let Ok(owner_offset) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF_OWNER_OFFSET") {
        if owner_offset
            .parse::<usize>()
            .map(|owner_offset| proof.owner_offset != owner_offset)
            .unwrap_or(false)
        {
            return;
        }
    }
    let preview = proof
        .fragment_bits
        .get(
            start_cursor
                ..start_cursor
                    .saturating_add(16)
                    .min(proof.fragment_bits.len()),
        )
        .unwrap_or(&[]);
    eprintln!(
        "live-object visible-equipment bit proof rejected: reason={reason} owner_offset={} start_cursor={start_cursor} consumed_bits={consumed_bits} translated_ee={} name_proof={:?} item_bits={} item_ee_extra_bits={} item_ee_insert_offsets={:?} bits={:?}",
        proof.owner_offset,
        proof.translated_ee,
        item.name_fragment_proof,
        item.fragment_bits_consumed,
        item.ee_extra_fragment_bits,
        item.ee_extra_insert_offsets,
        preview,
    );
}

fn trace_visible_equipment_bit_proof_accept(
    start_cursor: usize,
    consumed_bits: usize,
    item: &LegacyAppearanceItemAddRecord,
    proof: AppearanceBitProof<'_>,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF_ACCEPT").is_none() {
        return;
    }
    if let Ok(min_cursor) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF_MIN_CURSOR") {
        if min_cursor
            .parse::<usize>()
            .map(|min_cursor| start_cursor < min_cursor)
            .unwrap_or(false)
        {
            return;
        }
    }
    if let Ok(max_cursor) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF_MAX_CURSOR") {
        if max_cursor
            .parse::<usize>()
            .map(|max_cursor| start_cursor > max_cursor)
            .unwrap_or(false)
        {
            return;
        }
    }
    if let Ok(owner_offset) = std::env::var("HGBRIDGE_PROXY2_DEBUG_LIVE_BIT_PROOF_OWNER_OFFSET") {
        if owner_offset
            .parse::<usize>()
            .map(|owner_offset| proof.owner_offset != owner_offset)
            .unwrap_or(false)
        {
            return;
        }
    }
    let preview = proof
        .fragment_bits
        .get(
            start_cursor
                ..start_cursor
                    .saturating_add(16)
                    .min(proof.fragment_bits.len()),
        )
        .unwrap_or(&[]);
    eprintln!(
        "live-object visible-equipment bit proof accepted: owner_offset={} start_cursor={start_cursor} consumed_bits={consumed_bits} translated_ee={} name_proof={:?} item_bits={} item_ee_extra_bits={} item_ee_insert_offsets={:?} bits={:?}",
        proof.owner_offset,
        proof.translated_ee,
        item.name_fragment_proof,
        item.fragment_bits_consumed,
        item.ee_extra_fragment_bits,
        item.ee_extra_insert_offsets,
        preview,
    );
}

fn visible_equipment_item_candidate_spans_live_object_boundary(
    bytes: &[u8],
    item_offset: usize,
    candidate_end: usize,
) -> bool {
    // Visible-equipment `A` records are embedded inside the decompiled P/5
    // equipment count. While searching the un-length-prefixed final item, a
    // byte-plausible active-property tail can accidentally absorb the first
    // bytes of the next live-object record. Do not accept any item candidate
    // that crosses a boundary the focused live-object boundary classifier can
    // prove from the EE/Diamond dispatch shape.
    let scan_start = item_offset
        .checked_add(1 + 4 + 4)
        .unwrap_or(candidate_end)
        .min(candidate_end);
    (scan_start..candidate_end.saturating_sub(1)).any(|candidate| {
        boundary::looks_like_legacy_live_object_sub_message_boundary(bytes, candidate)
    })
}

fn trace_visible_equipment_parse_skip(
    cursor: usize,
    candidate_end: usize,
    remaining: u8,
    pending_byte_inserts: usize,
    reason: &'static str,
) {
    if !debug_live_claim_enabled_for_nearby_offset(cursor) {
        return;
    }
    eprintln!(
        "live-object visible-equipment parse skip: cursor={cursor} candidate_end={candidate_end} remaining={remaining} pending_byte_inserts={pending_byte_inserts} reason={reason}"
    );
}

fn parse_legacy_item_add_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<LegacyAppearanceItemAddRecord> {
    parse_legacy_item_add_record_candidates(bytes, offset, record_end)
        .into_iter()
        .next()
}

fn parse_legacy_item_add_record_candidates(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Vec<LegacyAppearanceItemAddRecord> {
    if offset.checked_add(1 + 4 + 4).unwrap_or(usize::MAX) >= record_end
        || record_end > bytes.len()
        || record_end.saturating_sub(offset) > LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES
        || bytes.get(offset).copied() != Some(b'A')
    {
        return Vec::new();
    }

    let Some(object_id) = read_u32_le(bytes, offset + 1) else {
        return Vec::new();
    };
    if !looks_like_creature_or_legacy_sentinel_id(object_id) {
        return Vec::new();
    }

    let Some(slot) = read_u32_le(bytes, offset + 5) else {
        return Vec::new();
    };
    if !is_legacy_visible_equipment_slot(slot) {
        return Vec::new();
    }

    let body_start = offset + 1 + 4 + 4;
    parse_legacy_item_object_body_candidates(bytes, body_start, record_end, slot)
}

fn parse_legacy_item_create_record(
    bytes: &[u8],
    item_object_offset: usize,
    record_end: usize,
) -> Option<LegacyAppearanceItemAddRecord> {
    parse_legacy_item_create_record_candidates(bytes, item_object_offset, record_end)
        .into_iter()
        .next()
}

fn parse_legacy_item_create_record_candidates(
    bytes: &[u8],
    item_object_offset: usize,
    record_end: usize,
) -> Vec<LegacyAppearanceItemAddRecord> {
    let Some(min_object_end) = item_object_offset.checked_add(4) else {
        return Vec::new();
    };
    if min_object_end >= record_end
        || record_end > bytes.len()
        || record_end.saturating_sub(item_object_offset) > LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES
    {
        return Vec::new();
    }

    let Some(object_id) = read_u32_le(bytes, item_object_offset) else {
        return Vec::new();
    };
    if !looks_like_creature_or_legacy_sentinel_id(object_id) {
        return Vec::new();
    }

    let Some(body_start) = item_object_offset.checked_add(4) else {
        return Vec::new();
    };
    // Diamond `sub_451680` reads the item OBJECTID, checks overflow, then calls
    // the shared item body reader. EE `sub_14079FE30` has the same shape: object
    // id via `sub_1409737C0`, then `sub_14079FAC0`, then active properties via
    // `sub_14076BD30`. There is no item-create-only fragment selector between
    // the object id and the body; only the body/name and active-property helpers
    // own CNW fragment bits.
    parse_legacy_item_object_body_candidates(bytes, body_start, record_end, 0)
}

fn parse_legacy_item_object_body(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    slot: u32,
) -> Option<LegacyAppearanceItemAddRecord> {
    parse_legacy_item_object_body_candidates(bytes, body_start, record_end, slot)
        .into_iter()
        .next()
}

fn parse_legacy_item_object_body_candidates(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    slot: u32,
) -> Vec<LegacyAppearanceItemAddRecord> {
    // Visible-equipment item bodies are not searched by "any printable string
    // followed by a plausible active-property tail" here. Diamond's
    // `sub_451020` reaches the name reader only after the baseitems.2da
    // model-type appearance body, and EE's `sub_14079FAC0` follows the same
    // semantic order with extra model-type-3/visual-transform bytes. Keeping
    // parsing anchored to that decompile-owned active offset avoids accepting
    // an `A`/`D` byte inside an item name or property blob as the next visible
    // equipment record.
    parse_legacy_visible_equipment_item_add_by_appearance_candidates(
        bytes, body_start, record_end, slot,
    )
}

fn parse_legacy_visible_equipment_item_add_by_appearance(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    _slot: u32,
) -> Option<LegacyAppearanceItemAddRecord> {
    parse_legacy_visible_equipment_item_add_by_appearance_candidates(
        bytes, body_start, record_end, _slot,
    )
    .into_iter()
    .next()
}

fn parse_legacy_visible_equipment_item_add_by_appearance_candidates(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    _slot: u32,
) -> Vec<LegacyAppearanceItemAddRecord> {
    let Some(base_item) = read_u32_le(bytes, body_start) else {
        return Vec::new();
    };
    let Some(appearance_layout) = legacy_visible_equipment_appearance_layout(base_item) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    collect_visible_equipment_item_add_candidates_for_dialect(
        bytes,
        body_start,
        record_end,
        base_item,
        appearance_layout,
        ItemAppearanceWireDialect::LegacyDiamond,
        &mut candidates,
    );
    collect_visible_equipment_item_add_candidates_for_dialect(
        bytes,
        body_start,
        record_end,
        base_item,
        appearance_layout,
        ItemAppearanceWireDialect::EeBuild8193,
        &mut candidates,
    );
    candidates
}

fn collect_visible_equipment_item_add_candidates_for_dialect(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    base_item: u32,
    appearance_layout: LegacyVisibleEquipmentAppearanceLayout,
    dialect: ItemAppearanceWireDialect,
    candidates: &mut Vec<LegacyAppearanceItemAddRecord>,
) {
    let appearance_bytes = appearance_layout.legacy_bytes;
    let Some(legacy_active_offset) = body_start.checked_add(appearance_bytes) else {
        return;
    };
    let Some(mut active_offset) = (match dialect {
        ItemAppearanceWireDialect::LegacyDiamond => Some(legacy_active_offset),
        ItemAppearanceWireDialect::EeBuild8193 => ee_feature23_item_appearance_end_if_valid(
            bytes,
            body_start,
            record_end,
            appearance_layout.model_type,
        ),
    }) else {
        return;
    };
    let mut ee_extra_byte_inserts = Vec::new();
    if dialect == ItemAppearanceWireDialect::LegacyDiamond
        && !push_ee_feature23_item_appearance_widening_inserts(
            appearance_layout.model_type,
            body_start,
            &mut ee_extra_byte_inserts,
        )
    {
        return;
    };
    if appearance_layout.needs_ee_model_type_3_table {
        if has_ee_model_type_3_armor_accessory_table_at(bytes, active_offset, record_end) {
            let Some(next_active_offset) =
                active_offset.checked_add(EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES)
            else {
                return;
            };
            active_offset = next_active_offset;
        } else if dialect == ItemAppearanceWireDialect::LegacyDiamond {
            // EE `sub_14079FAC0` model-type 3 branch consumes nineteen model
            // part WORDs, six legacy/global palette bytes, then a 0x72-byte
            // armor/accessory table: nineteen rows times the same six color
            // layers. Diamond `sub_451020` stops after the six palette bytes.
            // Preserve Diamond's palette semantics by seeding every EE table
            // row from those six bytes; a zero table is cursor-correct but
            // visually erases body armor coloration.
            let Some(legacy_palette) =
                legacy_model_type_3_palette_bytes(bytes, body_start, record_end)
            else {
                return;
            };
            ee_extra_byte_inserts.push(
                CreatureAppearanceByteInsert::EeModelType3ArmorAccessoryTable {
                    offset: active_offset,
                    legacy_palette,
                },
            );
        } else {
            return;
        }
    }
    if has_ee_object_visual_transform_identity_at(bytes, active_offset, record_end) {
        let Some(next_active_offset) =
            active_offset.checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len())
        else {
            return;
        };
        active_offset = next_active_offset;
    } else if has_partial_ee_object_visual_transform_identity_at(bytes, active_offset, record_end) {
        return;
    } else if dialect == ItemAppearanceWireDialect::LegacyDiamond
        && super::visual_transform::has_legacy_scalar_visual_transform_identity_at(
            bytes,
            active_offset,
            record_end,
        )
    {
        // Diamond/HG visible-equipment item bodies can carry the legacy
        // scalar ObjectVisualTransform identity immediately after the
        // baseitems.2da-driven appearance body. EE's `sub_14079FAC0` reaches
        // the object-map reader (`sub_140973160`) at this same semantic point,
        // so the bridge replaces the 40-byte scalar identity with the EE
        // empty object-map identity instead of inserting an EE map before it.
        ee_extra_byte_inserts.push(
            CreatureAppearanceByteInsert::LegacyScalarVisualTransformIdentityReplacement {
                offset: active_offset,
            },
        );
        let Some(next_active_offset) =
            active_offset.checked_add(LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)
        else {
            return;
        };
        active_offset = next_active_offset;
    } else if dialect == ItemAppearanceWireDialect::LegacyDiamond {
        ee_extra_byte_inserts.push(
            CreatureAppearanceByteInsert::LegacyVisualTransformIdentity {
                offset: active_offset,
            },
        );
    } else {
        return;
    }
    if active_offset > record_end {
        return;
    }
    let active_tails = legacy_active_item_properties_tail_candidates_for_visible_equipment(
        base_item,
        appearance_layout.model_type,
        &bytes[active_offset..record_end],
    );
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_ACTIVE_TAIL").is_some() {
        eprintln!(
            "live-object visible-equipment active-tail candidates: body_start={body_start} record_end={record_end} base_item=0x{base_item:08X} legacy_active_offset={legacy_active_offset} active_offset={active_offset} tail_len={} tail_preview={:02X?} candidates={active_tails:?}",
            record_end.saturating_sub(active_offset),
            bytes
                .get(active_offset..record_end.min(active_offset.saturating_add(96)))
                .unwrap_or(&[])
        );
    }
    for active_tail in active_tails {
        let mut ee_extra_insert_offsets =
            Vec::with_capacity(active_tail.ee_extra_insert_offsets.len());
        let mut active_byte_inserts =
            Vec::with_capacity(ee_extra_byte_inserts.len().saturating_add(1));
        for insert in ee_extra_byte_inserts.iter().cloned() {
            match insert {
                CreatureAppearanceByteInsert::LegacyVisualTransformIdentity { offset }
                    if offset == active_offset
                        && active_tail.visual_transform_identity_prefix_bytes > 0 =>
                {
                    let prefix = active_tail.visual_transform_identity_prefix_bytes;
                    if prefix >= EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len() {
                        continue;
                    }
                    let Some(suffix_offset) = active_offset.checked_add(prefix) else {
                        continue;
                    };
                    active_byte_inserts.push(
                        CreatureAppearanceByteInsert::LegacyVisualTransformIdentitySuffix {
                            offset: suffix_offset,
                            start: prefix,
                        },
                    );
                }
                other => active_byte_inserts.push(other),
            }
        }
        if let Some(length) = active_tail.missing_inline_name_length {
            let Ok(length) = u32::try_from(length) else {
                continue;
            };
            let Some(offset) =
                active_offset.checked_add(active_tail.missing_inline_name_relative_offset)
            else {
                continue;
            };
            active_byte_inserts.push(
                CreatureAppearanceByteInsert::MissingSecondInlineNameLength { offset, length },
            );
        }
        // EE `sub_14079FAC0` calls `sub_140973160` before `sub_14076BD30`.
        // The current-build visual-map path reads INT map counts, but the bridge
        // deliberately emits the legacy expanded identity map bytes here. On that
        // EE object-map path, `sub_140973160` reads the two zero DWORD counts,
        // whose matching build gate skips the current-build identity-selector
        // BOOL. The only fragment-bit delta for this captured visible-equipment
        // armor record is therefore the active-property BOOL added by EE's
        // `sub_14076BD30`.
        ee_extra_insert_offsets.extend(active_tail.ee_extra_insert_offsets);
        candidates.push(LegacyAppearanceItemAddRecord {
            fragment_bits_consumed: active_tail.fragment_bits_consumed,
            ee_extra_fragment_bits: EE_APPEARANCE_ACTIVE_PROPERTY_EXTRA_BOOL_BITS,
            ee_extra_insert_offsets,
            ee_extra_byte_inserts: active_byte_inserts,
            name_fragment_proof: active_tail.name_fragment_proof,
        });
    }
}

#[derive(Debug, Clone)]
struct LegacyVisibleEquipmentActiveTail {
    fragment_bits_consumed: usize,
    ee_extra_insert_offsets: Vec<usize>,
    missing_inline_name_length: Option<usize>,
    missing_inline_name_relative_offset: usize,
    visual_transform_identity_prefix_bytes: usize,
    name_fragment_proof: LegacyItemNameFragmentProof,
}

#[derive(Debug, Clone, Copy)]
struct LegacyVisibleEquipmentAppearanceLayout {
    model_type: i8,
    legacy_bytes: usize,
    needs_ee_model_type_3_table: bool,
}

fn legacy_visible_equipment_appearance_layout(
    base_item: u32,
) -> Option<LegacyVisibleEquipmentAppearanceLayout> {
    // EE/Diamond item appearance readers (`sub_14079FAC0` / `sub_451020`) select
    // the body width from baseitems.2da `ModelType` after the base item DWORD.
    // Use the same shared module-resource table as quickbar item translation so
    // HG/CEP custom rows such as 0x13A are handled by their verified 2DA model
    // type instead of by live-object-specific hard-coding.
    let model_type = crate::translate::baseitems::base_item_model_type(base_item)?;
    let legacy_bytes =
        crate::translate::baseitems::legacy_item_appearance_read_size_for_model_type(model_type)?;
    Some(LegacyVisibleEquipmentAppearanceLayout {
        model_type,
        legacy_bytes,
        needs_ee_model_type_3_table: model_type == 3,
    })
}

fn push_ee_feature23_item_appearance_widening_inserts(
    model_type: i8,
    body_start: usize,
    inserts: &mut Vec<CreatureAppearanceByteInsert>,
) -> bool {
    // EE `sub_14079FAC0`, like quickbar's item reader, checks
    // `ServerSatisfiesBuild(0x2001, 0x23, 0)` and reads model-part values as
    // WORDs in the proxy-owned EE-facing dialect. Diamond `sub_451020` writes
    // the same semantic part values as BYTEs. These inserts are the live-object
    // counterpart of quickbar/writer.rs' widened item appearance writer.
    let Some(parts_start) = body_start.checked_add(4) else {
        return false;
    };
    let mut push_after_byte = |relative: usize| -> bool {
        let Some(offset) = parts_start
            .checked_add(relative)
            .and_then(|offset| offset.checked_add(1))
        else {
            return false;
        };
        inserts.push(CreatureAppearanceByteInsert::EeFeature23ItemAppearanceHighByte { offset });
        true
    };
    match model_type {
        0 | 1 => push_after_byte(0),
        2 => (0..3).all(&mut push_after_byte),
        3 => (0..usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT)).all(push_after_byte),
        _ => false,
    }
}

fn ee_feature23_item_appearance_end_if_valid(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    model_type: i8,
) -> Option<usize> {
    let mut cursor = body_start.checked_add(4)?;
    match model_type {
        0 => {
            require_zero_high_byte_word(bytes, cursor, record_end)?;
            cursor = cursor.checked_add(2)?;
        }
        1 => {
            require_zero_high_byte_word(bytes, cursor, record_end)?;
            cursor = cursor.checked_add(2 + 6)?;
        }
        2 => {
            for _ in 0..3 {
                require_zero_high_byte_word(bytes, cursor, record_end)?;
                cursor = cursor.checked_add(2)?;
            }
            cursor = cursor.checked_add(1)?;
        }
        3 => {
            for _ in 0..usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT) {
                require_zero_high_byte_word(bytes, cursor, record_end)?;
                cursor = cursor.checked_add(2)?;
            }
            cursor = cursor.checked_add(6)?;
        }
        _ => return None,
    }
    (cursor <= record_end && cursor <= bytes.len()).then_some(cursor)
}

fn require_zero_high_byte_word(bytes: &[u8], offset: usize, record_end: usize) -> Option<()> {
    let high = offset.checked_add(1)?;
    if high >= record_end || high >= bytes.len() || bytes.get(high).copied()? != 0 {
        return None;
    }
    Some(())
}

fn legacy_model_type_3_palette_bytes(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
) -> Option<[u8; 6]> {
    // Legacy model type 3 appearance is:
    //   DWORD base item, 19 compact part bytes, 6 global palette bytes.
    // EE build 8193 widens the 19 parts to WORDs, but the six palette bytes
    // remain byte-sized and are immediately followed by the EE-only 0x72 table.
    let palette_start = body_start
        .checked_add(4)?
        .checked_add(usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT))?;
    let palette_end = palette_start.checked_add(6)?;
    if palette_end > record_end || palette_end > bytes.len() {
        return None;
    }
    let mut palette = [0u8; 6];
    palette.copy_from_slice(&bytes[palette_start..palette_end]);
    Some(palette)
}

fn ee_model_type_3_armor_accessory_table_from_legacy_palette(legacy_palette: [u8; 6]) -> Vec<u8> {
    let mut table = Vec::with_capacity(EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES);
    for _ in 0..usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT) {
        table.extend_from_slice(&legacy_palette);
    }
    table
}

fn has_ee_model_type_3_armor_accessory_table_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let end = offset.saturating_add(EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES);
    if end > record_end || end > bytes.len() {
        return false;
    }
    let table = &bytes[offset..end];
    table.iter().all(|byte| *byte == 0) || table.chunks_exact(6).all(|chunk| chunk == &table[..6])
}

fn has_ee_object_visual_transform_identity_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let end = offset.saturating_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len());
    end <= record_end
        && end <= bytes.len()
        && bytes[offset..end] == EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES
}

fn has_partial_ee_object_visual_transform_identity_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    if offset >= record_end || offset >= bytes.len() {
        return false;
    }
    let available = record_end
        .saturating_sub(offset)
        .min(bytes.len().saturating_sub(offset))
        .min(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len());
    available > 0
        && available < EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len()
        && bytes[offset..offset + available]
            == EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES[..available]
}

fn parse_legacy_active_item_properties_tail_for_visible_equipment(
    base_item: u32,
    tail: &[u8],
) -> Option<LegacyVisibleEquipmentActiveTail> {
    let model_type = crate::translate::baseitems::base_item_model_type(base_item)?;
    legacy_active_item_properties_tail_candidates_for_visible_equipment(base_item, model_type, tail)
        .into_iter()
        .next()
}

fn legacy_active_item_properties_tail_candidates_for_visible_equipment(
    base_item: u32,
    model_type: i8,
    tail: &[u8],
) -> Vec<LegacyVisibleEquipmentActiveTail> {
    let mut cursor = 0usize;
    if base_item == LEGACY_ARMOR_BASE_ITEM {
        if tail.len() < 2 {
            return Vec::new();
        }
        cursor += 2;
    }
    let mut candidates = Vec::new();

    // Diamond `sub_451020` first reads the item name selector BOOL. When that
    // selector takes the localized-name path, `sub_53E700` reads one more BOOL
    // before its BYTE/DWORD locstring token. EE then adds the later
    // active-property `CanUseItem` BOOL that Diamond does not write.
    if base_item == LEGACY_ARMOR_BASE_ITEM
        && parse_legacy_active_item_properties_tail_after_name(tail, cursor + 1)
    {
        // The armor branch consumes its two-byte armor-only field before item
        // names. HG seq40 captures show Diamond's localized-name branch can
        // then advance one read byte while the selector, inner selector, and
        // language selector live in the fragment stream. This still consumes
        // the same three locstring fragment bits as the wider TLK-token shape
        // below, but the read cursor lands at the active-property block after
        // one byte instead of four.
        let name_bits = LEGACY_APPEARANCE_ITEM_NAME_STRREF_LOCSTRING_BITS;
        candidates.push(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: name_bits + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(name_bits)],
            missing_inline_name_length: None,
            missing_inline_name_relative_offset: 0,
            visual_transform_identity_prefix_bytes: 0,
            name_fragment_proof: LegacyItemNameFragmentProof::LocStringToken,
        });
    }
    if parse_legacy_active_item_properties_tail_after_name(tail, cursor + 4) {
        // EE `sub_14076BD30` calls the locstring helper when the outer item
        // name BOOL is true. For the strref-shaped HG visible-equipment names
        // (`D6 75 00 01`, etc.), the helper consumes the inner locstring BOOL
        // and the one-bit language selector before the 32-bit TLK token. The
        // previous two-bit count modeled only the two BOOLs and left the next
        // creature `U/5 0x3967` update nine bits early after rewritten full
        // appearance records.
        let name_bits = LEGACY_APPEARANCE_ITEM_NAME_STRREF_LOCSTRING_BITS;
        candidates.push(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: name_bits + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(name_bits)],
            missing_inline_name_length: None,
            missing_inline_name_relative_offset: 0,
            visual_transform_identity_prefix_bytes: 0,
            name_fragment_proof: LegacyItemNameFragmentProof::LocStringToken,
        });
    }

    if parse_legacy_active_item_properties_tail_after_inline_string(tail, cursor) {
        let name_bits = LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS;
        candidates.push(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: name_bits + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(name_bits)],
            missing_inline_name_length: None,
            missing_inline_name_relative_offset: 0,
            visual_transform_identity_prefix_bytes: 0,
            name_fragment_proof: LegacyItemNameFragmentProof::InlineCExoString,
        });

        // Diamond's active-item name reader first consumes the outer item-name
        // selector. When that selector enters the locstring helper,
        // `sub_53E700` consumes an inner BOOL; the inner false branch then reads
        // a normal length-prefixed CExoString. That is byte-identical to the
        // outer-false CExoString branch above, but it consumes two fragment bits
        // (`true, false`) instead of one (`false`). Keep both candidates so the
        // fragment proof, not the read-buffer bytes alone, chooses the exact
        // decompiled branch.
        let name_bits = LEGACY_APPEARANCE_ITEM_NAME_BARE_INLINE_LOCSTRING_BITS;
        candidates.push(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: name_bits + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(name_bits)],
            missing_inline_name_length: None,
            missing_inline_name_relative_offset: 0,
            visual_transform_identity_prefix_bytes: 0,
            name_fragment_proof: LegacyItemNameFragmentProof::LocStringInlineCExoString,
        });
    }

    if (0..=3).contains(&model_type) {
        if let Some(name_len) = legacy_direct_bare_inline_active_item_name_length(tail, cursor) {
            // HG repository `G R A` rows can use the model-type-defined legacy
            // item appearance followed immediately by a printable active item
            // name. Diamond and EE both select that appearance width from
            // `baseitems.2da`; the following active-property reader is the same
            // helper regardless of model type. The EE writer must still feed
            // `sub_14076BD30` a normal CExoString body, so the rewrite inserts
            // the omitted length DWORD. The fragment stream, not this byte
            // branch, decides whether the decompiled selector path is direct
            // inline or locstring-inline.
            let name_bits = LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS;
            candidates.push(LegacyVisibleEquipmentActiveTail {
                fragment_bits_consumed: name_bits
                    + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
                ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(
                    name_bits,
                )],
                missing_inline_name_length: Some(name_len),
                missing_inline_name_relative_offset: 0,
                visual_transform_identity_prefix_bytes: 0,
                name_fragment_proof: LegacyItemNameFragmentProof::InlineCExoString,
            });

            let name_bits = LEGACY_APPEARANCE_ITEM_NAME_BARE_INLINE_LOCSTRING_BITS;
            candidates.push(LegacyVisibleEquipmentActiveTail {
                fragment_bits_consumed: name_bits
                    + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
                ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(
                    name_bits,
                )],
                missing_inline_name_length: Some(name_len),
                missing_inline_name_relative_offset: 0,
                visual_transform_identity_prefix_bytes: 0,
                name_fragment_proof: LegacyItemNameFragmentProof::LocStringInlineCExoString,
            });
        }
        if let Some(name_len) =
            legacy_single_zero_prefixed_bare_inline_active_item_name_length(tail, cursor)
        {
            // HG repository `G R A` rows can serialize the model-type-defined
            // legacy appearance followed by a single zero byte and then the
            // printable active-property name. The EE reader reaches
            // `sub_140973160` before `sub_14076BD30`, so that zero is the first
            // byte of the EE object visual-transform identity map; the
            // bridge completes the remaining identity bytes and inserts the
            // omitted CExoString length immediately before the printable text.
            // The exact active-property tail and fragment proof still choose
            // whether the decompiled name selector is direct-inline or
            // locstring-inline.
            let name_bits = LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS;
            candidates.push(LegacyVisibleEquipmentActiveTail {
                fragment_bits_consumed: name_bits
                    + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
                ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(
                    name_bits,
                )],
                missing_inline_name_length: Some(name_len),
                missing_inline_name_relative_offset: 1,
                visual_transform_identity_prefix_bytes: 1,
                name_fragment_proof: LegacyItemNameFragmentProof::InlineCExoString,
            });

            let name_bits = LEGACY_APPEARANCE_ITEM_NAME_BARE_INLINE_LOCSTRING_BITS;
            candidates.push(LegacyVisibleEquipmentActiveTail {
                fragment_bits_consumed: name_bits
                    + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
                ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(
                    name_bits,
                )],
                missing_inline_name_length: Some(name_len),
                missing_inline_name_relative_offset: 1,
                visual_transform_identity_prefix_bytes: 1,
                name_fragment_proof: LegacyItemNameFragmentProof::LocStringInlineCExoString,
            });
        }
        if parse_legacy_active_item_properties_tail_after_bare_inline_string(tail, cursor) {
            // The same decompiled item-name path used by armor rows also shows
            // up in HG model-type-2 GUI repository rows: the fragment stream
            // selects the locstring helper's inline-string branch, while the
            // read buffer stores a zero-length legacy CExoString sentinel
            // followed by the printable name and exact active-property tail.
            // Keep this model-type-gated rather than base-item-gated so custom
            // `baseitems.2da` rows follow the verified reader shape without
            // hard-coding HG item ids.
            let name_bits = LEGACY_APPEARANCE_ITEM_NAME_BARE_INLINE_LOCSTRING_BITS;
            candidates.push(LegacyVisibleEquipmentActiveTail {
                fragment_bits_consumed: name_bits
                    + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
                ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(
                    name_bits,
                )],
                missing_inline_name_length: None,
                missing_inline_name_relative_offset: 0,
                visual_transform_identity_prefix_bytes: 0,
                name_fragment_proof: LegacyItemNameFragmentProof::BareInlineLocString,
            });
        }
    }

    if base_item == LEGACY_ARMOR_BASE_ITEM
        && parse_legacy_active_item_properties_tail_after_bare_inline_string(tail, cursor)
    {
        // Diamond `sub_451020` uses the same active-item name reader reached by
        // quickbar item buttons: the first BOOL selects the locstring helper,
        // and the helper's second BOOL can select an inline string body. HG
        // visible body-armor captures serialize that inline text behind a
        // zero-length legacy CExoString sentinel rather than a normal length
        // DWORD. This is not an item-boundary guess: the printable name must be
        // immediately followed by an exact active-property tail, and the branch
        // is only accepted for armor-shaped visible-equipment records.
        let name_bits = LEGACY_APPEARANCE_ITEM_NAME_BARE_INLINE_LOCSTRING_BITS;
        candidates.push(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: name_bits + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(name_bits)],
            missing_inline_name_length: None,
            missing_inline_name_relative_offset: 0,
            visual_transform_identity_prefix_bytes: 0,
            name_fragment_proof: LegacyItemNameFragmentProof::BareInlineLocString,
        });
    }

    if parse_legacy_active_item_properties_tail(&tail[cursor..]) {
        candidates.push(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![ee_active_property_extra_bool_insert_offset(0)],
            missing_inline_name_length: None,
            missing_inline_name_relative_offset: 0,
            visual_transform_identity_prefix_bytes: 0,
            name_fragment_proof: LegacyItemNameFragmentProof::None,
        });
    }

    candidates
}

fn parse_legacy_active_item_properties_tail_after_inline_string(
    tail: &[u8],
    cursor: usize,
) -> bool {
    let Some(length) = read_u32_le(tail, cursor) else {
        return false;
    };
    let Ok(length) = usize::try_from(length) else {
        return false;
    };
    if length > MAX_LIVE_OBJECT_NAME_BYTES {
        return false;
    }
    let Some(name_start) = cursor.checked_add(4) else {
        return false;
    };
    let Some(name_end) = name_start.checked_add(length) else {
        return false;
    };
    if name_end > tail.len() {
        return false;
    }
    if length != 0 && !mostly_printable_message_string(&tail[name_start..name_end]) {
        return false;
    }
    parse_legacy_active_item_properties_tail_after_name(tail, name_end)
}

fn parse_legacy_active_item_properties_tail_after_bare_inline_string(
    tail: &[u8],
    cursor: usize,
) -> bool {
    let Some(length) = read_u32_le(tail, cursor) else {
        return false;
    };
    if length != 0 {
        return false;
    }
    let Some(text_start) = cursor.checked_add(4) else {
        return false;
    };
    if text_start >= tail.len() || !is_legacy_bare_active_item_name_byte(tail[text_start]) {
        return false;
    }

    let text_limit = tail
        .len()
        .min(text_start.saturating_add(MAX_LIVE_OBJECT_NAME_BYTES));
    let mut text_end = text_start;
    while text_end < text_limit && is_legacy_bare_active_item_name_byte(tail[text_end]) {
        text_end += 1;
    }
    text_end > text_start && parse_legacy_active_item_properties_tail_after_name(tail, text_end)
}

fn legacy_direct_bare_inline_active_item_name_length(tail: &[u8], cursor: usize) -> Option<usize> {
    if cursor >= tail.len() || !is_legacy_bare_active_item_name_byte(tail[cursor]) {
        return None;
    }

    let text_limit = tail
        .len()
        .min(cursor.saturating_add(MAX_LIVE_OBJECT_NAME_BYTES));
    let mut text_end = cursor;
    while text_end < text_limit && is_legacy_bare_active_item_name_byte(tail[text_end]) {
        text_end += 1;
        if parse_legacy_active_item_properties_tail_after_name(tail, text_end) {
            return Some(text_end.saturating_sub(cursor));
        }
    }
    None
}

fn legacy_single_zero_prefixed_bare_inline_active_item_name_length(
    tail: &[u8],
    cursor: usize,
) -> Option<usize> {
    if tail.get(cursor).copied() != EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.first().copied() {
        return None;
    }
    let text_start = cursor.checked_add(1)?;
    if text_start >= tail.len() || !is_legacy_bare_active_item_name_byte(tail[text_start]) {
        return None;
    }

    let text_limit = tail
        .len()
        .min(text_start.saturating_add(MAX_LIVE_OBJECT_NAME_BYTES));
    let mut text_end = text_start;
    while text_end < text_limit && is_legacy_bare_active_item_name_byte(tail[text_end]) {
        text_end += 1;
        if parse_legacy_active_item_properties_tail_after_name(tail, text_end) {
            return Some(text_end.saturating_sub(text_start));
        }
    }
    None
}

fn is_legacy_bare_active_item_name_byte(ch: u8) -> bool {
    (0x20..=0x7E).contains(&ch)
}

fn parse_legacy_active_item_properties_tail_after_name(tail: &[u8], cursor: usize) -> bool {
    if cursor > tail.len() || tail.len() - cursor < 11 {
        return false;
    }
    let fixed_end = cursor + 8;
    let property_count = tail[fixed_end];
    if property_count > 32 {
        return false;
    }
    let property_bytes =
        usize::from(property_count).saturating_mul(LEGACY_APPEARANCE_ACTIVE_PROPERTY_BYTES);
    let masks_offset = fixed_end + 1 + property_bytes;
    if masks_offset + 2 > tail.len() {
        return false;
    }
    let value_mask = tail[masks_offset + 1];
    let expected =
        masks_offset + 2 + usize::try_from(value_mask.count_ones()).unwrap_or(usize::MAX);
    expected == tail.len()
}

fn parse_legacy_active_item_properties_tail(tail: &[u8]) -> bool {
    if tail.len() < LEGACY_APPEARANCE_MIN_ACTIVE_PROPERTY_TAIL_BYTES {
        return false;
    }

    // Diamond `sub_451020` reaches this shape when the item-name BOOL selects
    // the no-name branch. It then reads the same active-property body as the
    // named branches: eight fixed bytes, an active-property count, `count`
    // seven-byte property rows, two trailer mask bytes, and one extra byte for
    // each set bit in the second mask. Keep this variable-width trailer here
    // instead of a fixed ten-byte suffix so zero-declared local Diamond GUI
    // rows do not get split at a shorter locstring-looking prefix.
    parse_legacy_active_item_properties_tail_after_name(tail, 0)
}

fn advance_message_string(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    max_len: usize,
) -> Option<usize> {
    let length = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    if length > max_len {
        return None;
    }
    let end = offset.checked_add(4)?.checked_add(length)?;
    if end > limit || end > bytes.len() {
        return None;
    }
    Some(end)
}

fn mostly_printable_message_string(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let printable = bytes
        .iter()
        .filter(|byte| matches!(byte, 0x09 | 0x0A | 0x0D | 0x20..=0x7E | 0x80..=0xFF))
        .count();
    printable + 2 >= bytes.len()
}

fn is_legacy_visible_equipment_slot(slot: u32) -> bool {
    // `WriteGameObjUpdate_WriteInventorySlotAdd` writes the caller-provided
    // slot DWORD verbatim immediately after the item object id. EE's full
    // appearance writer normally calls that helper for slots 2, 1, 0x20, 0x10,
    // and 0x40, but HG/Diamond captures show the legacy armor/body visual item
    // can be serialized as slot 0 while keeping the same bounded item-add
    // shape. Treat that as a verified visible-equipment slot, not a live-object
    // boundary candidate inside the appearance block.
    matches!(slot, 0 | 1 | 2 | 0x10 | 0x20 | 0x40)
}

fn looks_like_creature_or_legacy_sentinel_id(object_id: u32) -> bool {
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }
    if object_id == 0xFFFF_FFF7 || object_id == 0xFFFF_FFFD {
        return true;
    }
    matches!(
        object_id & 0xFF00_0000,
        0x8000_0000 | 0x8800_0000 | 0xFF00_0000 | 0x0100_0000 | 0x0500_0000
    ) || (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID)
        .contains(&object_id)
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn legacy_all_fields_appearance_claims_embedded_equipment_block() {
        let payload =
            include_bytes!("../../../fixtures/live_object/player_appearance_false_u09.bin");
        let declared = usize::try_from(read_u32_le(payload, 3).expect("declared")).unwrap();
        let live = &payload[7..declared];
        let record_end = try_get_legacy_creature_appearance_record_end(live, 32, live.len())
            .expect("record end");
        assert_eq!(record_end, 495);
    }

    #[test]
    fn legacy_all_fields_appearance_advances_embedded_equipment_bits() {
        let payload =
            include_bytes!("../../../fixtures/live_object/player_appearance_false_u09.bin");
        let declared = usize::try_from(read_u32_le(payload, 3).expect("declared")).unwrap();
        let live = &payload[7..declared];
        let fragment_bits =
            super::super::bits::decode_msb_valid_bits(&payload[declared..], 3).expect("bits");
        let mut bit_cursor = 3usize;
        assert!(advance_verified_legacy_creature_appearance_record(
            live,
            32,
            495,
            &fragment_bits,
            &mut bit_cursor,
        ));
        assert_eq!(bit_cursor, 29);
    }

    #[test]
    fn hg_current_player_empty_name_full_appearance_rewrites_and_claims_exactly() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_current_player_empty_name_full_appearance.bin"
        )
        .to_vec();

        assert!(super::super::claim_payload_if_verified(&payload).is_none());
        let rewrite = super::super::rewrite_update_records_payload_if_possible(&mut payload)
            .expect("current-player appearance rewrite");
        assert!(
            rewrite.bytes_inserted > 0,
            "expected EE visual-transform/body appearance bytes to be inserted"
        );

        let claim = super::super::claim_payload_if_verified(&payload)
            .expect("translated current-player appearance should claim");
        assert_eq!(claim.add_records, 1);
        assert_eq!(claim.creature_appearance_records, 1);
        assert_eq!(claim.creature_update_records, 1);
    }

    #[test]
    fn town_watch_visible_armor_inserts_model_type_3_table_before_visual_map() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/town_watch_visible_equipment_missing_armor_table.bin"
        )
        .to_vec();

        let rewrite = super::super::rewrite_update_records_payload_if_possible(&mut payload)
            .expect("town watch appearance rewrite");
        assert!(
            rewrite.bytes_inserted > 0,
            "expected appearance rewrite to insert EE-only bytes before exact validation"
        );

        let name = b"Militia Armor";
        let name_pos = payload
            .windows(name.len())
            .position(|window| window == name)
            .expect("armor name should remain present");
        let inserted_shape_len = EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES
            + EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len();
        payload[..name_pos]
            .windows(inserted_shape_len)
            .position(|window| {
                let (model_type_3_table, visual_identity) =
                    window.split_at(EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES);
                model_type_3_table
                    .chunks_exact(6)
                    .all(|chunk| chunk == &model_type_3_table[..6])
                    && visual_identity == &EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES[..]
            })
            .expect("model-type 3 armor table should be immediately followed by EE identity map");
        assert!(super::super::claim_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn model_type_3_armor_table_repeats_legacy_palette_per_part() {
        let table = ee_model_type_3_armor_accessory_table_from_legacy_palette([1, 2, 3, 4, 5, 6]);
        assert_eq!(table.len(), EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES);
        for chunk in table.chunks_exact(6) {
            assert_eq!(chunk, &[1, 2, 3, 4, 5, 6]);
        }
    }
}
