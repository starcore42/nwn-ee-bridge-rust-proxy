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
const LEGACY_APPEARANCE_ACTIVE_PROPERTY_BYTES: usize = 7;
const LEGACY_APPEARANCE_ACTIVE_PROPERTY_TRAILER_BYTES: usize = 10;
const LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS: usize = 4;
const EE_APPEARANCE_ACTIVE_PROPERTY_EXTRA_BOOL_BITS: usize = 1;
const LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS: usize = 1;
const LEGACY_APPEARANCE_ITEM_NAME_STRREF_LOCSTRING_BITS: usize = 3;
const LEGACY_ARMOR_BASE_ITEM: u32 = 0x10;
const LEGACY_WEAPON_BASE_ITEM: u32 = 0x01;
const LEGACY_MAGIC_STAFF_BASE_ITEM: u32 = 0x2D;
const LEGACY_SHIELD_BASE_ITEM: u32 = 0x38;
const LEGACY_CLOAK_BASE_ITEM: u32 = 0x50;
const LEGACY_ARMOR_APPEARANCE_BYTES: usize = 4 + 19 + 6;
const LEGACY_WEAPON_APPEARANCE_BYTES: usize = 4 + 4;
const LEGACY_SIMPLE_MODEL_APPEARANCE_BYTES: usize = 4 + 1;
const LEGACY_MODEL_TYPE_ONE_APPEARANCE_BYTES: usize = 4 + 1 + 6;
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
}

#[derive(Debug, Clone)]
enum CreatureAppearanceByteInsert {
    MissingSecondInlineNameLength { offset: usize, length: u32 },
    LegacyVisualTransformIdentity { offset: usize },
}

impl CreatureAppearanceByteInsert {
    fn offset(&self) -> usize {
        match self {
            Self::MissingSecondInlineNameLength { offset, .. }
            | Self::LegacyVisualTransformIdentity { offset } => *offset,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::MissingSecondInlineNameLength { length, .. } => length.to_le_bytes().to_vec(),
            Self::LegacyVisualTransformIdentity { .. } => {
                EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.to_vec()
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
    ee_extra_byte_insert_offsets: Vec<usize>,
}

struct LegacyAppearanceItemAddRecord {
    fragment_bits_consumed: usize,
    ee_extra_fragment_bits: usize,
    ee_extra_insert_offsets: Vec<usize>,
    ee_extra_byte_insert_offsets: Vec<usize>,
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
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CreatureAppearanceExtraRewrite {
    pub bits_inserted: usize,
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
    let Some(name_shape) = read_appearance_name_shape(fragment_bits, *bit_cursor) else {
        return false;
    };
    let Some(record) = parse_legacy_creature_appearance_record(
        bytes,
        offset,
        record_end,
        name_shape,
        Some(AppearanceBitProof {
            bit_cursor: *bit_cursor,
            fragment_bits,
            translated_ee: false,
            allow_cross_record_fence: true,
        }),
    ) else {
        return false;
    };
    if record.record_end != record_end
        || record.fragment_bits_consumed > fragment_bits.len().saturating_sub(*bit_cursor)
    {
        return false;
    }
    *bit_cursor = bit_cursor.saturating_add(record.fragment_bits_consumed);
    true
}

pub(super) fn advance_verified_ee_creature_appearance_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let Some(name_shape) = read_appearance_name_shape(fragment_bits, *bit_cursor) else {
        return false;
    };
    let Some(record) = parse_legacy_creature_appearance_record(
        bytes,
        offset,
        record_end,
        name_shape,
        Some(AppearanceBitProof {
            bit_cursor: *bit_cursor,
            fragment_bits,
            translated_ee: true,
            allow_cross_record_fence: false,
        }),
    ) else {
        return false;
    };
    if record.ee_fragment_bits_consumed > fragment_bits.len().saturating_sub(*bit_cursor) {
        return false;
    }
    if !record.ee_extra_byte_inserts.is_empty() {
        return false;
    }

    // EE's visible-equipment active-item-property reader consumes one extra
    // BOOL compared with Diamond's 1.69 stream. The bridge emits legacy
    // expanded visual-transform bytes here, so `sub_140973160`/`sub_140972C70`
    // remain on the legacy no-selector path rather than the current-build map
    // count/identity-selector path. Source-side cursor walking must still use
    // `fragment_bits_consumed`, while translated strict validation advances
    // across only the EE-visible active-property delta.
    *bit_cursor = bit_cursor.saturating_add(record.ee_fragment_bits_consumed);
    true
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
    if !record.ee_extra_byte_insert_offsets.is_empty()
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
    if record.fragment_bits_consumed > fragment_bits.len().saturating_sub(bit_cursor) {
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
    for insert_offset in record.ee_extra_byte_insert_offsets.iter().copied() {
        if insert_offset < offset || insert_offset > *record_end {
            return None;
        }
        let actual_insert_offset = insert_offset.checked_add(bytes_inserted)?;
        bytes.splice(
            actual_insert_offset..actual_insert_offset,
            EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        bytes_inserted =
            bytes_inserted.checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;
        *record_end = (*record_end).checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;
    }

    Some(CreatureAppearanceExtraRewrite {
        bits_inserted: record.ee_extra_insert_offsets.len(),
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

pub(super) fn advance_verified_ee_item_create_record(
    bytes: &[u8],
    item_object_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let Some(record) = parse_legacy_item_create_record(bytes, item_object_offset, record_end)
    else {
        return false;
    };
    let Some(ee_bits) = record
        .fragment_bits_consumed
        .checked_add(record.ee_extra_fragment_bits)
    else {
        return false;
    };
    if !record.ee_extra_byte_insert_offsets.is_empty()
        || ee_bits > fragment_bits.len().saturating_sub(*bit_cursor)
    {
        return false;
    }
    *bit_cursor = bit_cursor.saturating_add(ee_bits);
    true
}

pub(super) fn insert_ee_item_create_extras_for_ee(
    bytes: &mut Vec<u8>,
    item_object_offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureAppearanceExtraRewrite> {
    let record = parse_legacy_item_create_record(bytes, item_object_offset, *record_end)?;
    if record.fragment_bits_consumed > fragment_bits.len().saturating_sub(bit_cursor) {
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
    for insert_offset in record.ee_extra_byte_insert_offsets.iter().copied() {
        if insert_offset < item_object_offset || insert_offset > *record_end {
            return None;
        }
        let actual_insert_offset = insert_offset.checked_add(bytes_inserted)?;
        bytes.splice(
            actual_insert_offset..actual_insert_offset,
            EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES,
        );
        bytes_inserted =
            bytes_inserted.checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;
        *record_end = (*record_end).checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;
    }

    Some(CreatureAppearanceExtraRewrite {
        bits_inserted: record.ee_extra_insert_offsets.len(),
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
    let name_shape = read_appearance_name_shape(fragment_bits, bit_cursor)?;
    let mut repaired_name_shape = None;
    let proof = AppearanceBitProof {
        bit_cursor,
        fragment_bits,
        translated_ee: false,
        allow_cross_record_fence: false,
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
    let record = parse_exact_record(name_shape).or_else(|| {
        let alternate = name_shape.alternate();
        let record = parse_exact_record(alternate)?;
        repaired_name_shape = Some(alternate);
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
        bytes_inserted,
    })
}

fn read_appearance_name_shape(bits: &[bool], bit_cursor: usize) -> Option<AppearanceNameShape> {
    let bit = *bits.get(bit_cursor)?;
    Some(if bit {
        AppearanceNameShape::LocStringPair
    } else {
        AppearanceNameShape::CExoString
    })
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
    if (mask & LEGACY_APPEARANCE_NAME_MASK) != 0 {
        if let Some(proof) = bit_proof {
            if proof.bit_cursor >= proof.fragment_bits.len() {
                return None;
            }
        }
        fragment_bits_consumed = fragment_bits_consumed.checked_add(1)?;
        match name_shape {
            AppearanceNameShape::LocStringPair => {
                if let Some(proof) = bit_proof {
                    let required_bits = 3usize;
                    if required_bits > proof.fragment_bits.len().saturating_sub(proof.bit_cursor) {
                        return None;
                    }
                }
                fragment_bits_consumed = fragment_bits_consumed.checked_add(2)?;
                cursor = advance_message_string(bytes, cursor, limit, MAX_LIVE_OBJECT_NAME_BYTES)?;
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
            AppearanceNameShape::CExoString => {
                cursor = advance_message_string(bytes, cursor, limit, MAX_LIVE_OBJECT_NAME_BYTES)?;
            }
        }
    }

    cursor = advance_legacy_scalar_appearance_fields(bytes, cursor, limit, mask)?;

    if mask == LEGACY_APPEARANCE_ALL_FIELDS_MASK {
        if bytes.get(cursor).copied() != Some(LEGACY_APPEARANCE_BODY_PART_COUNT)
            && bytes.get(cursor.checked_add(4)?).copied() == Some(LEGACY_APPEARANCE_BODY_PART_COUNT)
        {
            // HG full-state creature appearances can carry four legacy compact
            // scalar bytes immediately before the full 19-part body table. The
            // mature C++ bridge only accepts this when the following part table
            // and equipment block validate; mirror that discipline here rather
            // than letting the boundary walker split on item-like bytes inside
            // the appearance record.
            cursor = cursor.checked_add(4)?;
        }
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
            if count == 0 || count > LEGACY_APPEARANCE_MAX_EQUIPMENT_RECORDS {
                return None;
            }
            cursor = cursor.checked_add(1)?;
            let equipment = parse_legacy_visible_equipment_records(bytes, cursor, limit, count)?;
            cursor = equipment.end;
            ee_extra_insert_offsets.extend(
                equipment
                    .ee_extra_insert_offsets
                    .iter()
                    .map(|relative| fragment_bits_consumed.saturating_add(*relative)),
            );
            ee_extra_byte_inserts.extend(equipment.ee_extra_byte_insert_offsets.into_iter().map(
                |offset| CreatureAppearanceByteInsert::LegacyVisualTransformIdentity { offset },
            ));
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
                .map(|proof| proof.allow_cross_record_fence)
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
            )?
        } else {
            CNW_FRAGMENT_HEADER_BITS
        };
        fragment_bits_consumed = fragment_bits_consumed.checked_add(fence_bits)?;
    }

    Some(LegacyAppearanceRecord {
        record_end: cursor,
        fragment_bits_consumed,
        ee_fragment_bits_consumed: fragment_bits_consumed.checked_add(ee_extra_fragment_bits)?,
        ee_extra_insert_offsets,
        ee_extra_byte_inserts,
        equipment_records,
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
    if bytes.get(cursor).copied() != Some(LEGACY_APPEARANCE_BODY_PART_COUNT)
        && bytes.get(cursor.checked_add(4)?).copied() == Some(LEGACY_APPEARANCE_BODY_PART_COUNT)
    {
        cursor = cursor.checked_add(4)?;
    }
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
    if equipment_records == 0 || equipment_records > LEGACY_APPEARANCE_MAX_EQUIPMENT_RECORDS {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let equipment =
        parse_legacy_visible_equipment_records(bytes, cursor, limit, equipment_records)?;
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
            return Some(fence_bits);
        }
    }

    None
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
) -> Option<LegacyVisibleEquipmentParse> {
    if remaining == 0 {
        return Some(LegacyVisibleEquipmentParse {
            end: cursor,
            fragment_bits_consumed: 0,
            ee_extra_fragment_bits: 0,
            ee_extra_insert_offsets: Vec::new(),
            ee_extra_byte_insert_offsets: Vec::new(),
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
            parse_legacy_visible_equipment_records(bytes, next, limit, remaining - 1)
        }
        b'A' => {
            if remaining == 1 {
                let min_next =
                    cursor.checked_add(1 + 4 + 4 + LEGACY_APPEARANCE_MIN_ITEM_NAME_TAIL_BYTES)?;
                for next in min_next..=limit {
                    if let Some(item) = parse_legacy_item_add_record(bytes, cursor, next) {
                        return Some(LegacyVisibleEquipmentParse {
                            end: next,
                            fragment_bits_consumed: item.fragment_bits_consumed,
                            ee_extra_fragment_bits: item.ee_extra_fragment_bits,
                            ee_extra_insert_offsets: item.ee_extra_insert_offsets,
                            ee_extra_byte_insert_offsets: item.ee_extra_byte_insert_offsets,
                        });
                    }
                }
                return None;
            }

            let min_next =
                cursor.checked_add(1 + 4 + 4 + LEGACY_APPEARANCE_MIN_ITEM_NAME_TAIL_BYTES)?;
            for next in min_next..limit {
                if !matches!(bytes.get(next).copied(), Some(b'A' | b'D')) {
                    continue;
                }
                let Some(item) = parse_legacy_item_add_record(bytes, cursor, next) else {
                    continue;
                };
                if let Some(rest) =
                    parse_legacy_visible_equipment_records(bytes, next, limit, remaining - 1)
                {
                    let mut ee_extra_insert_offsets = item.ee_extra_insert_offsets;
                    ee_extra_insert_offsets.extend(
                        rest.ee_extra_insert_offsets
                            .iter()
                            .map(|relative| item.fragment_bits_consumed.saturating_add(*relative)),
                    );
                    let mut ee_extra_byte_insert_offsets = item.ee_extra_byte_insert_offsets;
                    ee_extra_byte_insert_offsets.extend(rest.ee_extra_byte_insert_offsets);
                    return Some(LegacyVisibleEquipmentParse {
                        end: rest.end,
                        fragment_bits_consumed: item
                            .fragment_bits_consumed
                            .checked_add(rest.fragment_bits_consumed)?,
                        ee_extra_fragment_bits: item
                            .ee_extra_fragment_bits
                            .checked_add(rest.ee_extra_fragment_bits)?,
                        ee_extra_insert_offsets,
                        ee_extra_byte_insert_offsets,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_legacy_item_add_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<LegacyAppearanceItemAddRecord> {
    if offset.checked_add(1 + 4 + 4).unwrap_or(usize::MAX) >= record_end
        || record_end > bytes.len()
        || record_end.saturating_sub(offset) > LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES
        || bytes.get(offset).copied() != Some(b'A')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, offset + 1)?;
    if !looks_like_creature_or_legacy_sentinel_id(object_id) {
        return None;
    }

    let slot = read_u32_le(bytes, offset + 5)?;
    if !is_legacy_visible_equipment_slot(slot) {
        return None;
    }

    let body_start = offset + 1 + 4 + 4;
    parse_legacy_item_object_body(bytes, body_start, record_end, slot)
}

fn parse_legacy_item_create_record(
    bytes: &[u8],
    item_object_offset: usize,
    record_end: usize,
) -> Option<LegacyAppearanceItemAddRecord> {
    if item_object_offset.checked_add(4)? >= record_end
        || record_end > bytes.len()
        || record_end.saturating_sub(item_object_offset) > LEGACY_APPEARANCE_MAX_ITEM_ADD_BYTES
    {
        return None;
    }

    let object_id = read_u32_le(bytes, item_object_offset)?;
    if !looks_like_creature_or_legacy_sentinel_id(object_id) {
        return None;
    }

    let body_start = item_object_offset.checked_add(4)?;
    parse_legacy_item_object_body(bytes, body_start, record_end, 0)
}

fn parse_legacy_item_object_body(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    slot: u32,
) -> Option<LegacyAppearanceItemAddRecord> {
    if let Some(item) =
        parse_legacy_visible_equipment_item_add_by_appearance(bytes, body_start, record_end, slot)
    {
        return Some(item);
    }

    for string_offset in body_start..record_end.saturating_sub(4) {
        let Some(length) = read_u32_le(bytes, string_offset) else {
            continue;
        };
        let Ok(length) = usize::try_from(length) else {
            continue;
        };
        if length == 0 || length > MAX_LIVE_OBJECT_NAME_BYTES {
            continue;
        }
        let Some(string_start) = string_offset.checked_add(4) else {
            continue;
        };
        let Some(string_end) = string_start.checked_add(length) else {
            continue;
        };
        if string_end > record_end {
            continue;
        }
        let tail_len = record_end - string_end;
        if !(LEGACY_APPEARANCE_MIN_ITEM_NAME_TAIL_BYTES
            ..=LEGACY_APPEARANCE_MAX_ITEM_NAME_TAIL_BYTES)
            .contains(&tail_len)
        {
            continue;
        }
        if !mostly_printable_message_string(&bytes[string_start..string_end]) {
            continue;
        }
        let tail = &bytes[string_end..record_end];
        if !parse_legacy_active_item_properties_tail(tail) {
            continue;
        }
        // This fallback found a concrete inline CExoString in the item-add
        // bytes. Diamond's item reader consumes one selector BOOL for that
        // branch, then the four active-property BOOLs. EE adds the newer
        // `CanUseItem` BOOL after the value/cost DWORDs.
        let fragment_bits_consumed = LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS
            + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS;
        return Some(LegacyAppearanceItemAddRecord {
            fragment_bits_consumed,
            ee_extra_fragment_bits: EE_APPEARANCE_ACTIVE_PROPERTY_EXTRA_BOOL_BITS,
            ee_extra_insert_offsets: vec![
                LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS.saturating_add(1),
            ],
            ee_extra_byte_insert_offsets: Vec::new(),
        });
    }

    None
}

fn parse_legacy_visible_equipment_item_add_by_appearance(
    bytes: &[u8],
    body_start: usize,
    record_end: usize,
    _slot: u32,
) -> Option<LegacyAppearanceItemAddRecord> {
    let base_item = read_u32_le(bytes, body_start)?;
    let appearance_bytes = legacy_visible_equipment_appearance_bytes(base_item)?;
    let legacy_active_offset = body_start.checked_add(appearance_bytes)?;
    let mut active_offset = legacy_active_offset;
    let mut ee_extra_byte_insert_offsets = Vec::new();
    if has_ee_legacy_visual_transform_identity_at(bytes, active_offset, record_end) {
        active_offset =
            active_offset.checked_add(EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES.len())?;
    } else if has_partial_ee_legacy_visual_transform_identity_at(bytes, active_offset, record_end) {
        return None;
    } else {
        ee_extra_byte_insert_offsets.push(active_offset);
    }
    if active_offset > record_end {
        return None;
    }
    let active_tail = parse_legacy_active_item_properties_tail_for_visible_equipment(
        base_item,
        &bytes[active_offset..record_end],
    )?;
    let mut ee_extra_insert_offsets = Vec::with_capacity(active_tail.ee_extra_insert_offsets.len());
    // EE `sub_14079FAC0` calls `sub_140973160` before `sub_14076BD30`.
    // The current-build visual-map path reads INT map counts, but the bridge
    // deliberately emits the legacy expanded identity map bytes here. On that
    // legacy-build path, `sub_140973160` falls through to `sub_140972C70`,
    // whose matching build gate skips the current-build identity-selector
    // BOOL. The only fragment-bit delta for this captured visible-equipment
    // armor record is therefore the active-property BOOL added by EE's
    // `sub_14076BD30`.
    ee_extra_insert_offsets.extend(active_tail.ee_extra_insert_offsets);
    Some(LegacyAppearanceItemAddRecord {
        fragment_bits_consumed: active_tail.fragment_bits_consumed,
        ee_extra_fragment_bits: EE_APPEARANCE_ACTIVE_PROPERTY_EXTRA_BOOL_BITS,
        ee_extra_insert_offsets,
        ee_extra_byte_insert_offsets,
    })
}

#[derive(Debug, Clone)]
struct LegacyVisibleEquipmentActiveTail {
    fragment_bits_consumed: usize,
    ee_extra_insert_offsets: Vec<usize>,
}

fn legacy_visible_equipment_appearance_bytes(base_item: u32) -> Option<usize> {
    // EE/Diamond item appearance readers (`sub_14079FAC0` / `sub_451020`) select
    // the body width from baseitems.2da `ModelType` after the base item DWORD.
    // Armor row 0x10 uses the extended 19-part+palette layout. HG weapon row
    // 0x01 and magic-staff row 0x2D are `ModelType=2`; EE `sub_14079FAC0` and
    // Diamond `sub_4514C0` read three legacy model bytes plus one final
    // model/color byte before the visual-transform map. Shield row 0x38 is
    // `ModelType=0` and follows the simple one-byte model branch. Cloak row
    // 0x50 is `ModelType=1`, which reads one model byte plus six color bytes
    // before the item name/active-property tail.
    match base_item {
        LEGACY_ARMOR_BASE_ITEM => Some(LEGACY_ARMOR_APPEARANCE_BYTES),
        LEGACY_WEAPON_BASE_ITEM | LEGACY_MAGIC_STAFF_BASE_ITEM => {
            Some(LEGACY_WEAPON_APPEARANCE_BYTES)
        }
        LEGACY_SHIELD_BASE_ITEM => Some(LEGACY_SIMPLE_MODEL_APPEARANCE_BYTES),
        LEGACY_CLOAK_BASE_ITEM => Some(LEGACY_MODEL_TYPE_ONE_APPEARANCE_BYTES),
        _ => None,
    }
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
    let mut cursor = 0usize;
    if base_item == LEGACY_ARMOR_BASE_ITEM {
        if tail.len() < 2 {
            return None;
        }
        cursor += 2;
    }

    // Diamond `sub_451020` first reads the item name selector BOOL. When that
    // selector takes the localized-name path, `sub_53E700` reads one more BOOL
    // before its BYTE/DWORD locstring token. EE then adds the later
    // active-property `CanUseItem` BOOL that Diamond does not write.
    if parse_legacy_active_item_properties_tail_after_name(tail, cursor + 4) {
        // EE `sub_14076BD30` calls the locstring helper when the outer item
        // name BOOL is true. For the strref-shaped HG visible-equipment names
        // (`D6 75 00 01`, etc.), the helper consumes the inner locstring BOOL
        // and the one-bit language selector before the 32-bit TLK token. The
        // previous two-bit count modeled only the two BOOLs and left the next
        // creature `U/5 0x3967` update nine bits early after rewritten full
        // appearance records.
        let name_bits = LEGACY_APPEARANCE_ITEM_NAME_STRREF_LOCSTRING_BITS;
        return Some(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: name_bits + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![name_bits.saturating_add(1)],
        });
    }

    if parse_legacy_active_item_properties_tail_after_inline_string(tail, cursor) {
        let name_bits = LEGACY_APPEARANCE_ITEM_NAME_INLINE_CEXO_BITS;
        return Some(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: name_bits + LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![name_bits.saturating_add(1)],
        });
    }

    if parse_legacy_active_item_properties_tail(&tail[cursor..]) {
        return Some(LegacyVisibleEquipmentActiveTail {
            fragment_bits_consumed: LEGACY_APPEARANCE_DIAMOND_ACTIVE_PROPERTY_BOOL_BITS,
            ee_extra_insert_offsets: vec![1],
        });
    }

    None
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
}
