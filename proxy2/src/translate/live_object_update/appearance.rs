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
// Reassembled packetized live-object streams can promote chunk-local zero
// storage into the shared CNW fragment tail. The padding is still removed only
// when the fully rewritten EE appearance validator accepts the exact record, so
// this bound controls search cost rather than semantic acceptance.
const LEGACY_APPEARANCE_MAX_ZERO_FRAGMENT_PADDING_BITS: usize = 64;

fn legacy_full_appearance_body_table_padding(
    bytes: &[u8],
    cursor: usize,
    limit: usize,
) -> Option<usize> {
    // Diamond's appearance reader consumes a count byte before the full
    // 19-byte body table.  Captured HG full-state creature appearances expose
    // two legacy compact-scalar layouts before that exact count byte:
    //
    //   * +4: previously verified HG full-appearance compact scalar block.
    //   * +5: empty-name/current-player capture where the scalar block has an
    //     extra zero byte before the same decompiled 0x13 body-table count.
    //
    // This is deliberately not a scan.  Only the decompile-owned count byte
    // positions are accepted, and the following table must fit inside the
    // bounded record window before the visible-equipment parser can claim it.
    for padding in [0usize, 4, 5] {
        let part_cursor = cursor.checked_add(padding)?;
        if bytes.get(part_cursor).copied() != Some(LEGACY_APPEARANCE_BODY_PART_COUNT) {
            continue;
        }
        let after_table =
            part_cursor.checked_add(1 + usize::from(LEGACY_APPEARANCE_BODY_PART_COUNT))?;
        if after_table <= limit {
            return Some(padding);
        }
    }
    None
}
const LEGACY_APPEARANCE_ACTIVE_PROPERTY_BYTES: usize = 7;
const LEGACY_APPEARANCE_ACTIVE_PROPERTY_TRAILER_BYTES: usize = 10;
const LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS: usize = 4;
const EE_APPEARANCE_ACTIVE_PROPERTY_EXTRA_BOOL_BITS: usize = 1;
const EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES: usize = 0x72;
const LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS: usize = 1;
const LEGACY_APPEARANCE_ITEM_NAME_BARE_INLINE_LOCSTRING_BITS: usize = 2;
const LEGACY_APPEARANCE_ITEM_NAME_STRREF_LOCSTRING_BITS: usize = 3;
const LEGACY_ARMOR_BASE_ITEM: u32 = 0x10;

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
const LEGACY_FULL_APPEARANCE_PRECEDING_FRAGMENT_FENCE_CANDIDATES: [usize; 2] = [
    0,
    CNW_FRAGMENT_HEADER_BITS + 1,
];
const EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES: [u8; 40] = [
    0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F,
];

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
    MissingSecondInlineNameLength { offset: usize, length: u32 },
    EeModelType3ArmorAccessoryTable { offset: usize },
    LegacyVisualTransformIdentity { offset: usize },
    LegacyVisualTransformIdentitySuffix { offset: usize, start: usize },
}

impl CreatureAppearanceByteInsert {
    fn offset(&self) -> usize {
        match self {
            Self::MissingSecondInlineNameLength { offset, .. }
            | Self::EeModelType3ArmorAccessoryTable { offset }
            | Self::LegacyVisualTransformIdentity { offset }
            | Self::LegacyVisualTransformIdentitySuffix { offset, .. } => *offset,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::MissingSecondInlineNameLength { length, .. } => length.to_le_bytes().to_vec(),
            Self::EeModelType3ArmorAccessoryTable { .. } => {
                vec![0; EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES]
            }
            Self::LegacyVisualTransformIdentity { .. } => {
                EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.to_vec()
            }
            Self::LegacyVisualTransformIdentitySuffix { start, .. } => {
                EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES
                    .get(*start..)
                    .unwrap_or(&[])
                    .to_vec()
            }
        }
    }
}

#[derive(Debug, Clone)]
struct LegacyVisibleEquipmentParse {
    end: usize,
    fragment_bits_consumed: usize,
    ee_extra_fragment_bits: usize,
    ee_extra_insert_offsets: Vec<usize>,
    ee_extra_byte_inserts: Vec<CreatureAppearanceByteInsert>,
    first_positive_name_selector_relative_start: Option<usize>,
    token_selector_padding_repair_relative_start: Option<usize>,
    inline_active_name_fence_repair_relative_start: Option<usize>,
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
        let Some(record) =
            parse_legacy_creature_appearance_record(bytes, offset, scan_end, name_shape, None)
        else {
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
        let Some(record) =
            parse_legacy_creature_appearance_record(bytes, offset, scan_end, name_shape, None)
        else {
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
        true,
        false,
    ) else {
        return false;
    };
    if verified.record.record_end != record_end {
        return false;
    }
    if verified.record.ee_fragment_bits_consumed
        > fragment_bits.len().saturating_sub(verified.proof_cursor)
    {
        return false;
    }
    if !verified.record.ee_extra_byte_inserts.is_empty() {
        return false;
    }

    // EE's visible-equipment active-item-property reader consumes one extra
    // BOOL compared with Diamond's 1.69 stream. The bridge emits legacy
    // expanded visual-transform bytes here, so `sub_140973160`/`sub_140972C70`
    // remain on the legacy no-selector path rather than the current-build map
    // count/identity-selector path. Source-side cursor walking must still use
    // `fragment_bits_consumed`, while translated strict validation advances
    // across only the EE-visible active-property delta.
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
    let verified = parse_verified_creature_appearance_with_optional_preceding_fence(
        bytes,
        offset,
        scan_end,
        fragment_bits,
        bit_cursor,
        true,
        false,
    )?;
    if verified.record.ee_fragment_bits_consumed
        > fragment_bits.len().saturating_sub(verified.proof_cursor)
    {
        return None;
    }
    if !verified.record.ee_extra_byte_inserts.is_empty() {
        return None;
    }
    Some(verified.record.record_end)
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
        || visual_offset.checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())
            != Some(record_end)
    {
        return false;
    }

    let Some(object_id) = read_u32_le(bytes, offset + 2) else {
        return false;
    };
    looks_like_legacy_creature_object_id(object_id)
        && has_ee_legacy_visual_transform_identity_at(bytes, visual_offset, record_end)
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
    // server has no transform bytes for this branch, the bridge emits a
    // neutral legacy-build identity map. Any bytes the boundary walker grouped
    // after the selector are chunk-local CNW fragment storage, not part of this
    // semantic record, so promote them back into the fragment stream before
    // inserting the EE-only identity map.
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
        EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES,
    );
    *record_end = (*record_end).checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;

    Some(CreatureVisualTransformUpdateRewrite {
        bytes_inserted: EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len(),
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
    let Some(record) = parse_legacy_item_add_record(bytes, offset, record_end) else {
        return false;
    };
    let Some(ee_bits) = record
        .fragment_bits_consumed
        .checked_add(record.ee_extra_fragment_bits)
    else {
        return false;
    };
    if !record.name_fragment_proof.matches(fragment_bits, *bit_cursor) {
        return false;
    }
    for relative_offset in record.ee_extra_insert_offsets.iter().copied() {
        let Some(bit) = bit_cursor
            .checked_add(relative_offset)
            .and_then(|index| fragment_bits.get(index))
        else {
            return false;
        };
        if *bit {
            return false;
        }
    }
    if !record.ee_extra_byte_inserts.is_empty()
        || ee_bits > fragment_bits.len().saturating_sub(*bit_cursor)
    {
        return false;
    }
    *bit_cursor = bit_cursor.saturating_add(ee_bits);
    true
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
        || !record.name_fragment_proof.matches(fragment_bits, bit_cursor)
    {
        return None;
    }

    // Top-level item adds use the same Diamond writer as visible equipment:
    // `WriteGameObjUpdate_WriteInventorySlotAdd` writes `A`, object id, slot,
    // then the item object. EE's item reader reaches `sub_14079FAC0` and
    // `sub_14076BD30` for armor-shaped item payloads, so the exact rewrite is
    // identical to the nested visible-equipment item-add case: insert the
    // legacy-build identity visual map in the read buffer and insert the
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

    let mut bytes_inserted = 0usize;
    let mut byte_inserts = record.ee_extra_byte_inserts;
    byte_inserts.sort_by_key(CreatureAppearanceByteInsert::offset);
    for insert in byte_inserts {
        let insert_offset = insert.offset();
        if insert_offset < offset || insert_offset > *record_end {
            return None;
        }
        let insert_bytes = insert.bytes();
        let actual_insert_offset = insert_offset.checked_add(bytes_inserted)?;
        bytes.splice(
            actual_insert_offset..actual_insert_offset,
            insert_bytes.iter().copied(),
        );
        bytes_inserted = bytes_inserted.checked_add(insert_bytes.len())?;
        *record_end = (*record_end).checked_add(insert_bytes.len())?;
    }

    Some(CreatureAppearanceExtraRewrite {
        bits_inserted: record.ee_extra_insert_offsets.len(),
        bits_removed: 0,
        bytes_inserted,
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
        let mut matching_records = parse_legacy_item_create_record_candidates(
            bytes,
            item_object_offset,
            record_end,
        )
        .into_iter()
        .filter(|record| {
            record.fragment_bits_consumed <= fragment_bits.len().saturating_sub(bit_cursor)
                && record.name_fragment_proof.matches(fragment_bits, bit_cursor)
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

pub(super) fn advance_verified_ee_item_create_record(
    bytes: &[u8],
    item_object_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    for record in parse_legacy_item_create_record_candidates(bytes, item_object_offset, record_end) {
        let Some(ee_bits) = record
            .fragment_bits_consumed
            .checked_add(record.ee_extra_fragment_bits)
        else {
            continue;
        };
        if !record.name_fragment_proof.matches(fragment_bits, *bit_cursor) {
            continue;
        }
        if !record.ee_extra_byte_inserts.is_empty()
            || ee_bits > fragment_bits.len().saturating_sub(*bit_cursor)
        {
            continue;
        }
        if record.ee_extra_insert_offsets.iter().copied().any(|relative_offset| {
            bit_cursor
                .checked_add(relative_offset)
                .and_then(|index| fragment_bits.get(index))
                .copied()
                .unwrap_or(true)
        }) {
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

fn looks_like_gui_item_create_prefix_at(bytes: &[u8], offset: usize) -> bool {
    if offset.checked_add(3).unwrap_or(usize::MAX) > bytes.len()
        || bytes.get(offset).copied() != Some(b'G')
        || bytes.get(offset + 2).copied() != Some(b'A')
    {
        return false;
    }

    let item_object_offset = match bytes[offset + 1] {
        b'I' | b'i' => offset.checked_add(7),
        b'R' | b'r' => offset.checked_add(5),
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
    let mut matching_records = parse_legacy_item_create_record_candidates(
        bytes,
        item_object_offset,
        *record_end,
    )
    .into_iter()
    .filter(|record| {
        record.fragment_bits_consumed <= fragment_bits.len().saturating_sub(bit_cursor)
            && record.name_fragment_proof.matches(fragment_bits, bit_cursor)
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

    let mut bytes_inserted = 0usize;
    let mut byte_inserts = record.ee_extra_byte_inserts;
    byte_inserts.sort_by_key(CreatureAppearanceByteInsert::offset);
    for insert in byte_inserts {
        let insert_offset = insert.offset();
        if insert_offset < item_object_offset || insert_offset > *record_end {
            return None;
        }
        let insert_bytes = insert.bytes();
        let actual_insert_offset = insert_offset.checked_add(bytes_inserted)?;
        bytes.splice(
            actual_insert_offset..actual_insert_offset,
            insert_bytes.iter().copied(),
        );
        bytes_inserted = bytes_inserted.checked_add(insert_bytes.len())?;
        *record_end = (*record_end).checked_add(insert_bytes.len())?;
    }

    Some(CreatureAppearanceExtraRewrite {
        bits_inserted: record.ee_extra_insert_offsets.len(),
        bits_removed: 0,
        bytes_inserted,
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
    let leading_fence_name_shape = if legacy_full_appearance_preceding_fence_bits_are_proven(
        fragment_bits,
        bit_cursor,
        CNW_FRAGMENT_HEADER_BITS + 1,
    ) {
        bit_cursor
            .checked_add(CNW_FRAGMENT_HEADER_BITS + 1)
            .and_then(|cursor| read_appearance_name_shape(fragment_bits, cursor))
    } else {
        None
    };
    let mut repaired_name_shape = None;
    let mut record_name_shape = name_shape;
    let mut record_from_fragment_proof = false;
    let proof = AppearanceBitProof {
        bit_cursor,
        fragment_bits,
        translated_ee: false,
        allow_cross_record_fence: false,
        owner_offset: offset,
    };
    let parse_exact_record = |shape| {
        let record = parse_legacy_creature_appearance_record(
            bytes,
            offset,
            *record_end,
            shape,
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
        let record =
            parse_legacy_creature_appearance_record(bytes, offset, *record_end, shape, None)?;
        if record.record_end != *record_end
            || record.fragment_bits_consumed > fragment_bits.len().saturating_sub(bit_cursor)
        {
            return None;
        }
        Some(record)
    };
    let mut record = parse_exact_record(name_shape)
        .inspect(|_| {
            record_from_fragment_proof = true;
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
            let fenced = leading_fence_name_shape?;
            let record = parse_exact_byte_record_without_fragment_proof(fenced)?;
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
        *fragment_bits.get_mut(bit_cursor)? = repaired.fragment_bit();
    }
    if !record_from_fragment_proof {
        if let Some(delta) = proven_name_fragment_delta_for_byte_only_appearance_parse(
            record_name_shape,
            fragment_bits,
            bit_cursor,
        ) {
            apply_name_fragment_delta_to_appearance_record(&mut record, delta)?;
        }
    }
    let mut bits_removed = 0usize;
    if !record_from_fragment_proof {
        let effective_name_shape = record_name_shape;
        let leading_fence_bits = if legacy_full_appearance_preceding_fence_bits_are_proven(
            fragment_bits,
            bit_cursor,
            CNW_FRAGMENT_HEADER_BITS + 1,
        ) && bit_cursor
            .checked_add(CNW_FRAGMENT_HEADER_BITS + 1)
            .and_then(|cursor| read_appearance_name_shape(fragment_bits, cursor))
            .is_some()
        {
            CNW_FRAGMENT_HEADER_BITS + 1
        } else {
            0
        };
        let mut minimum_padding_start = (match effective_name_shape {
            AppearanceNameShape::LocStringPair => 3,
            AppearanceNameShape::CExoString => 1,
        } as usize)
        .saturating_add(leading_fence_bits);
        if let Some(preferred_start) = record.preferred_zero_padding_relative_start {
            minimum_padding_start =
                minimum_padding_start.max(preferred_start.saturating_add(leading_fence_bits));
        }
        let token_selector_padding_repair_relative_start = record
            .token_selector_padding_repair_relative_start
            .map(|relative_start| relative_start.saturating_add(leading_fence_bits));
        let inline_active_name_fence_repair_relative_start = record
            .inline_active_name_fence_repair_relative_start
            .map(|relative_start| relative_start.saturating_add(leading_fence_bits));
        let removal = find_zero_fragment_padding_removal_for_ee_appearance(
            bytes,
            offset,
            *record_end,
            fragment_bits,
            bit_cursor,
            &record,
            minimum_padding_start,
            token_selector_padding_repair_relative_start,
            inline_active_name_fence_repair_relative_start,
        )?;
        for range in removal.ranges.iter().rev() {
            let absolute_start = bit_cursor.checked_add(range.relative_start)?;
            fragment_bits.drain(absolute_start..absolute_start.checked_add(range.count)?);
            bits_removed = bits_removed.checked_add(range.count)?;
        }
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object appearance zero fragment padding removed: offset={offset} ranges={:?} record_end={}",
                removal.ranges, *record_end
            );
        }
    }

    for (inserted, relative_offset) in record.ee_extra_insert_offsets.iter().copied().enumerate() {
        super::bits::insert_msb_bit(
            fragment_bits,
            bit_cursor
                .checked_add(relative_offset)?
                .checked_add(inserted)?,
            false,
        )?;
    }

    let mut byte_inserts = record.ee_extra_byte_inserts;
    byte_inserts.sort_by_key(CreatureAppearanceByteInsert::offset);
    let mut bytes_inserted = 0usize;
    for insert in byte_inserts.iter() {
        let insert_offset = insert.offset();
        if insert_offset < offset || insert_offset > *record_end {
            *bytes = original_bytes;
            *fragment_bits = original_fragment_bits;
            *record_end = original_record_end;
            return None;
        }
        let insert_bytes = insert.bytes();
        let actual_insert_offset = insert_offset.checked_add(bytes_inserted)?;
        bytes.splice(
            actual_insert_offset..actual_insert_offset,
            insert_bytes.iter().copied(),
        );
        bytes_inserted = bytes_inserted.checked_add(insert_bytes.len())?;
        *record_end = (*record_end).checked_add(insert_bytes.len())?;
    }

    let mut proof_cursor = bit_cursor;
    if !advance_verified_ee_creature_appearance_record(
        bytes,
        offset,
        *record_end,
        fragment_bits,
        &mut proof_cursor,
    ) {
        if debug_live_claim_enabled_for_offset(offset) {
            eprintln!(
                "live-object appearance transactional rewrite rejected: offset={offset} record_end={} bits_inserted={} bytes_inserted={bytes_inserted}",
                *record_end,
                record.ee_extra_insert_offsets.len(),
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
        bytes_inserted,
    })
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
    if record.record_end != record_end || record.ee_extra_insert_offsets.is_empty() {
        return None;
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
        return Some(ZeroFragmentPaddingRemoval {
            ranges: Vec::new(),
        });
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
        let candidate_proven =
            legacy_visible_equipment_inline_name_fence_bits_are_proven(
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
            if accepted.is_none() {
                for (first_index, first_secondary) in
                    secondary_ranges.iter().copied().enumerate()
                {
                    let Some(first_secondary_end) =
                        first_secondary.relative_start.checked_add(first_secondary.count)
                    else {
                        continue;
                    };
                    for second_secondary in
                        secondary_ranges.iter().copied().skip(first_index + 1)
                    {
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
    let Some(inline_selector_cursor) = first_fence_cursor
        .checked_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS)
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
        post_removal_relative_offset.checked_add(base_range.count).filter(|relative| {
            *relative >= base_end
        })
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
    let Some(bytes_inserted) = apply_creature_appearance_byte_inserts(
        &mut trial_bytes,
        offset,
        &mut trial_record_end,
        &record.ee_extra_byte_inserts,
    )
    else {
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
            "live-object appearance zero fragment padding trial rejected: offset={offset} record_end={record_end} trial_record_end={trial_record_end} bit_cursor={bit_cursor} ranges={ranges:?} bit_inserts={:?} byte_insert_offsets={byte_insert_offsets:?} byte_insert_kinds={byte_insert_kinds:?} bytes_inserted={bytes_inserted} trial_bits={trial_bit_window:?}",
            record.ee_extra_insert_offsets
        );
    }
    exact
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
) -> Option<usize> {
    let mut byte_inserts = inserts.to_vec();
    byte_inserts.sort_by_key(CreatureAppearanceByteInsert::offset);
    let mut bytes_inserted = 0usize;
    for insert in byte_inserts.iter() {
        let insert_offset = insert.offset();
        if insert_offset < offset || insert_offset > *record_end {
            return None;
        }
        let insert_bytes = insert.bytes();
        let actual_insert_offset = insert_offset.checked_add(bytes_inserted)?;
        bytes.splice(
            actual_insert_offset..actual_insert_offset,
            insert_bytes.iter().copied(),
        );
        bytes_inserted = bytes_inserted.checked_add(insert_bytes.len())?;
        *record_end = (*record_end).checked_add(insert_bytes.len())?;
    }
    Some(bytes_inserted)
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
        let record = parse_legacy_creature_appearance_record(
            bytes,
            offset,
            scan_end,
            name_shape,
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

    // The decompiled appearance reader starts directly at the current bit
    // cursor. Treat that exact shape as authoritative. Only if it cannot prove
    // the record do we try the separately named transport-fence compatibility
    // path below.
    if let Some(exact) = parse_candidate(0) {
        return Some(exact);
    }

    let mut accepted: Option<VerifiedAppearanceParse> = None;
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
                legacy_appearance_boundary_candidate_is_better(
                    mask,
                    &candidate.record,
                    &current.record,
                ) || (candidate.record.record_end == current.record.record_end
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
    // This is deliberately not a generic "skip N bits" rule. The only
    // currently verified leading-fence capture has the CNW final-valid-bit
    // header set to seven (`111`) followed by one set data bit before the real
    // appearance name selector. Other leading shapes must quarantine until a
    // capture/decompile trace gives them a precise owner.
    if preceding_fence_bits != CNW_FRAGMENT_HEADER_BITS + 1 {
        return false;
    }
    let Some(fence) = fragment_bits.get(bit_cursor..bit_cursor.saturating_add(preceding_fence_bits))
    else {
        return false;
    };
    fence.iter().all(|bit| *bit)
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
    record.token_selector_padding_repair_relative_start =
        checked_shift_optional_relative(record.token_selector_padding_repair_relative_start, delta)?;
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

fn parse_legacy_creature_appearance_record(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    name_shape: AppearanceNameShape,
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
    let mut ee_extra_byte_inserts = Vec::new();
    let mut preferred_zero_padding_relative_start = None;
    let mut token_selector_padding_repair_relative_start = None;
    let mut inline_active_name_fence_repair_relative_start = None;
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

    cursor = advance_legacy_scalar_appearance_fields(bytes, cursor, limit, mask)?;

    if mask == LEGACY_APPEARANCE_ALL_FIELDS_MASK {
        cursor = cursor.checked_add(legacy_full_appearance_body_table_padding(
            bytes, cursor, limit,
        )?)?;
        let part_count = *bytes.get(cursor)?;
        if part_count != LEGACY_APPEARANCE_BODY_PART_COUNT {
            return None;
        }
        cursor = cursor.checked_add(1 + usize::from(part_count))?;
        if cursor > limit {
            return None;
        }
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
        // EE's writer only emits this byte for sufficiently new EE clients.
        // A Diamond/1.69 server cannot satisfy that build gate, so the legacy
        // appearance stream carries no bytes for this mask bit.
    }

    let equipment_records =
        if mask == LEGACY_APPEARANCE_ALL_FIELDS_MASK {
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
            let require_translated_byte_shape =
                bit_proof.map(|proof| proof.translated_ee).unwrap_or(false);
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

    let translated_ee_bit_proof = bit_proof
        .map(|proof| proof.translated_ee)
        .unwrap_or(false);
    let may_probe_following_creature_update_fence = !translated_ee_bit_proof
        && (cursor < limit
            || (cursor == limit
                && bit_proof
                    .map(|proof| proof.allow_cross_record_fence)
                    .unwrap_or(false)));
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
        // decompiled following `U/5` reader still proves how many fence bits
        // the legacy source appearance side must account for before that
        // reader starts. Translated EE validation leaves that cross-record
        // fence with the following update parser; otherwise a repaired
        // multi-record stream can drift three bits before the next appearance.
        // This is not a boundary guess: the fence is accepted only when the
        // focused creature-update parser consumes the following record from
        // that exact post-fence bit cursor.
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
    cursor = cursor.checked_add(legacy_full_appearance_body_table_padding(
        bytes, cursor, limit,
    )?)?;
    let part_count = *bytes.get(cursor)?;
    if part_count != LEGACY_APPEARANCE_BODY_PART_COUNT {
        return None;
    }
    cursor = cursor.checked_add(1 + usize::from(part_count))?;
    if cursor > limit {
        return None;
    }
    cursor = cursor.checked_add(2 + 4)?;
    if cursor > limit {
        return None;
    }
    let equipment_records = *bytes.get(cursor)?;
    if equipment_records > LEGACY_APPEARANCE_MAX_EQUIPMENT_RECORDS {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let equipment =
        parse_legacy_visible_equipment_records(bytes, cursor, limit, equipment_records, false, None, 0, 0)?;
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

    for fence_bits in LEGACY_FULL_APPEARANCE_FOLLOWING_CREATURE_UPDATE_FRAGMENT_FENCE_CANDIDATES {
        let Some(mut probe_cursor) = base_cursor.checked_add(fence_bits) else {
            continue;
        };
        if probe_cursor > proof.fragment_bits.len() {
            continue;
        }
        if super::creature::advance_verified_noop_creature_update_record_exact_cursor(
            bytes,
            following_offset,
            following_end,
            proof.fragment_bits,
            &mut probe_cursor,
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
        let language_bit_cursor = component_bit_cursor.checked_add(1)?;
        proof.fragment_bits.get(language_bit_cursor)?;
        let end = cursor.checked_add(4)?;
        if end > limit || end > bytes.len() {
            return None;
        }
        Some(LegacyLocStringComponentAdvance {
            end,
            fragment_bits_consumed: 2,
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
    mut cursor: usize,
    limit: usize,
    mask: u16,
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
        // Diamond/legacy build path: BYTE portrait id. EE's writer can use a
        // WORD for newer clients, but the 1.69 server stream uses the compact
        // byte shape observed in the legacy decompile/capture path.
        cursor = cursor.checked_add(1)?;
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
                    if let Some(rest) =
                        parse_legacy_visible_equipment_records(
                            bytes,
                            next,
                            limit,
                            remaining - 1,
                            require_translated_byte_shape,
                            bit_proof,
                            legacy_bits_before.checked_add(item.fragment_bits_consumed)?,
                            ee_extra_bits_before.checked_add(item.ee_extra_fragment_bits)?,
                        )
                    {
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
                        ee_extra_insert_offsets.extend(rest.ee_extra_insert_offsets.iter().map(
                            |relative| item.fragment_bits_consumed.saturating_add(*relative),
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
        .get(start_cursor..start_cursor.saturating_add(16).min(proof.fragment_bits.len()))
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
        .get(start_cursor..start_cursor.saturating_add(16).min(proof.fragment_bits.len()))
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
    (scan_start..candidate_end.saturating_sub(1))
        .any(|candidate| boundary::looks_like_legacy_live_object_sub_message_boundary(bytes, candidate))
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
    let appearance_bytes = appearance_layout.legacy_bytes;
    let Some(legacy_active_offset) = body_start.checked_add(appearance_bytes) else {
        return Vec::new();
    };
    let mut active_offset = legacy_active_offset;
    let mut ee_extra_byte_inserts = Vec::new();
    if appearance_layout.needs_ee_model_type_3_table {
        if has_ee_model_type_3_armor_accessory_table_at(bytes, active_offset, record_end) {
            let Some(next_active_offset) =
                active_offset.checked_add(EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES)
            else {
                return Vec::new();
            };
            active_offset = next_active_offset;
        } else {
            // EE `sub_14079FAC0` model-type 3 branch consumes a 0x72 byte
            // armor/accessory table after the legacy body-part/color bytes
            // and before `sub_140973160`. Diamond `sub_451020` returns from
            // the item appearance reader before that table, so legacy visible
            // equipment needs an explicit zero table before the visual map.
            ee_extra_byte_inserts.push(
                CreatureAppearanceByteInsert::EeModelType3ArmorAccessoryTable {
                    offset: active_offset,
                },
            );
        }
    }
    if has_ee_legacy_visual_transform_identity_at(bytes, active_offset, record_end) {
        let Some(next_active_offset) =
            active_offset.checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())
        else {
            return Vec::new();
        };
        active_offset = next_active_offset;
    } else if has_partial_ee_legacy_visual_transform_identity_at(bytes, active_offset, record_end) {
        return Vec::new();
    } else {
        ee_extra_byte_inserts.push(CreatureAppearanceByteInsert::LegacyVisualTransformIdentity {
            offset: active_offset,
        });
    }
    if active_offset > record_end {
        return Vec::new();
    }
    let active_tails = legacy_active_item_properties_tail_candidates_for_visible_equipment(
        base_item,
        appearance_layout.model_type,
        &bytes[active_offset..record_end],
    );
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_ACTIVE_TAIL").is_some()
        && (4600..=5200).contains(&body_start)
    {
        eprintln!(
            "live-object visible-equipment active-tail candidates: body_start={body_start} record_end={record_end} base_item=0x{base_item:08X} legacy_active_offset={legacy_active_offset} active_offset={active_offset} tail_len={} candidates={active_tails:?}",
            record_end.saturating_sub(active_offset)
        );
    }
    let mut candidates = Vec::with_capacity(active_tails.len());
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
                    if prefix >= EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len() {
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
            active_byte_inserts.push(CreatureAppearanceByteInsert::MissingSecondInlineNameLength {
                offset,
                length,
            });
        }
        // EE `sub_14079FAC0` calls `sub_140973160` before `sub_14076BD30`.
        // The current-build visual-map path reads INT map counts, but the bridge
        // deliberately emits the legacy expanded identity map bytes here. On that
        // legacy-build path, `sub_140973160` falls through to `sub_140972C70`,
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
    candidates
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

fn has_ee_model_type_3_armor_accessory_table_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let end = offset.saturating_add(EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES);
    end <= record_end && end <= bytes.len() && bytes[offset..end].iter().all(|byte| *byte == 0)
}

fn has_ee_legacy_visual_transform_identity_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let end = offset.saturating_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len());
    end <= record_end
        && end <= bytes.len()
        && bytes[offset..end] == EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES
}

fn has_partial_ee_legacy_visual_transform_identity_at(
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
        .min(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len());
    available > 0
        && available < EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len()
        && bytes[offset..offset + available]
            == EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES[..available]
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
            // byte of the legacy-build visual-transform identity block; the
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

    let text_limit = tail.len().min(text_start.saturating_add(MAX_LIVE_OBJECT_NAME_BYTES));
    let mut text_end = text_start;
    while text_end < text_limit && is_legacy_bare_active_item_name_byte(tail[text_end]) {
        text_end += 1;
    }
    text_end > text_start && parse_legacy_active_item_properties_tail_after_name(tail, text_end)
}

fn legacy_direct_bare_inline_active_item_name_length(
    tail: &[u8],
    cursor: usize,
) -> Option<usize> {
    if cursor >= tail.len() || !is_legacy_bare_active_item_name_byte(tail[cursor]) {
        return None;
    }

    let text_limit = tail.len().min(cursor.saturating_add(MAX_LIVE_OBJECT_NAME_BYTES));
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
    if tail.get(cursor).copied() != EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.first().copied() {
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
    if tail.len() < LEGACY_APPEARANCE_MIN_ITEM_NAME_TAIL_BYTES {
        return false;
    }
    let Some(active_count) = tail.get(8).copied() else {
        return false;
    };
    if active_count > 32 {
        return false;
    }
    let active_bytes =
        usize::from(active_count).saturating_mul(LEGACY_APPEARANCE_ACTIVE_PROPERTY_BYTES);
    let Some(expected_len) = 4usize
        .checked_add(4)
        .and_then(|value| value.checked_add(1))
        .and_then(|value| value.checked_add(active_bytes))
        .and_then(|value| value.checked_add(LEGACY_APPEARANCE_ACTIVE_PROPERTY_TRAILER_BYTES))
    else {
        return false;
    };
    if tail.len() != expected_len {
        return false;
    }
    let trailer_offset = 9 + active_bytes;
    tail.get(trailer_offset + 1).copied() == Some(0xFF)
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

#[cfg(test)]
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
            usize::try_from(rewrite.bytes_inserted).expect("rewrite size fits usize")
                >= EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES
        );

        let name = b"Militia Armor";
        let name_pos = payload
            .windows(name.len())
            .position(|window| window == name)
            .expect("armor name should remain present");
        let identity_end = name_pos
            .checked_sub(6)
            .expect("armor name must follow armor short and length");
        let identity_start = identity_end
            .checked_sub(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())
            .expect("identity bytes precede armor active tail");
        let table_start = identity_start
            .checked_sub(EE_MODEL_TYPE_3_ARMOR_ACCESSORY_TABLE_BYTES)
            .expect("model-type 3 table precedes identity map");

        assert!(
            payload[table_start..identity_start]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(
            &payload[identity_start..identity_end],
            EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES
        );
        assert!(super::super::claim_payload_if_verified(&payload).is_some());
    }
}
