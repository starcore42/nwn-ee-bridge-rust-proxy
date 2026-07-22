//! Inventory-family live-object update policy.
//!
//! Inventory and GUI item-create submessages can own fragment BOOLs. The
//! decompile-backed rule is intentionally stricter than "the next byte looks
//! like a live-object opcode": legacy `I` records contain opcode-like row bytes,
//! so a boundary inside an inventory record is safe only after the inventory
//! family validates the exact record shape and fragment-bit count.

use super::{read_u16_le, read_u32_le};

const LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK: u16 = 0x0010;
const LEGACY_INVENTORY_RICH_CATEGORY_MASK: u16 = 0x0020;
const LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK: u16 = 0x0004;
const LEGACY_INVENTORY_CATEGORY_COUNT: usize = 3;
const MAX_REASONABLE_CATEGORY_ENTRIES: u16 = 4096;
const MAX_REASONABLE_VALUE_GROUPS: u8 = 64;
const MAX_REASONABLE_FEATURE25_OBJECTS: u32 = 128;
const LEGACY_INVENTORY_GUI_HAND_TRAP_OWNER: u32 = 0xFFFF_FFEC;
const LEGACY_INVENTORY_CURRENT_PLAYER_OWNER: u32 = 0xFFFF_FFFE;
const MAX_GENERIC_INVENTORY_REQUIRED_FRAGMENT_BITS: usize = 8;
const GENERIC_INVENTORY_PARSE_MASK: u16 = 0x0001
    | 0x0002
    | LEGACY_INVENTORY_LEGACY_ICON_LIST_MASK
    | 0x0008
    | LEGACY_INVENTORY_SIMPLE_CATEGORY_MASK
    | LEGACY_INVENTORY_RICH_CATEGORY_MASK
    | 0x0040
    | 0x0080
    | 0x0100
    | 0x0200
    | 0x0400
    | 0x0800
    | 0x1000
    | 0x2000
    | 0x4000
    | 0x8000;

#[derive(Debug, Clone)]
pub(super) struct InventoryRecordClaim {
    pub owner_id: u32,
    pub mask: u16,
    pub fragment_bits: usize,
    pub feature25: Option<InventoryFeature25RecordClaim>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct InventoryRecordPrefixClaim {
    pub read_end: usize,
    pub fragment_bits: usize,
    pub interleaved_fragment_tail_allowed: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct InventoryRecordRewrite {
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct InventoryFragmentBitRepair {
    pub bits_materialized: usize,
}

#[derive(Debug, Clone)]
pub(super) struct InventoryFeature25RecordClaim {
    pub branch_offset: usize,
    pub block_end: usize,
    pub first_count: u32,
    pub first_object_ids: Vec<u32>,
    pub second_count: u32,
    pub second_object_ids: Vec<u32>,
    pub second_fragment_bit_start: usize,
    pub second_fragment_bit_end: usize,
    pub legacy_tail_object_ids: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct InventoryFeature25Candidate {
    branch_offset: usize,
    block_end: usize,
    first_count: u32,
    first_objects_offset: usize,
    first_objects_end: usize,
    second_count: u32,
    second_objects_offset: usize,
    second_objects_end: usize,
    second_fragment_bit_start: usize,
    second_fragment_bit_end: usize,
    legacy_tail_offset: Option<usize>,
    legacy_tail_end: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct GenericInventoryCandidate {
    cursor: usize,
    bits: usize,
    required_fragment_bits: [(usize, bool); MAX_GENERIC_INVENTORY_REQUIRED_FRAGMENT_BITS],
    required_fragment_bit_count: usize,
    feature25: Option<InventoryFeature25Candidate>,
}

impl GenericInventoryCandidate {
    fn new(cursor: usize, bits: usize) -> Self {
        Self {
            cursor,
            bits,
            required_fragment_bits: [(usize::MAX, false);
                MAX_GENERIC_INVENTORY_REQUIRED_FRAGMENT_BITS],
            required_fragment_bit_count: 0,
            feature25: None,
        }
    }

    fn advanced(self, cursor: usize, bits: usize) -> Self {
        Self {
            cursor,
            bits,
            ..self
        }
    }

    fn with_feature25(self, feature25: InventoryFeature25Candidate) -> Self {
        Self {
            feature25: Some(feature25),
            ..self
        }
    }

    fn require_fragment_bit(mut self, bit_index: usize, value: bool) -> Option<Self> {
        for index in 0..self.required_fragment_bit_count {
            let (existing_index, existing_value) = self.required_fragment_bits[index];
            if existing_index == bit_index {
                return (existing_value == value).then_some(self);
            }
        }

        if self.required_fragment_bit_count >= MAX_GENERIC_INVENTORY_REQUIRED_FRAGMENT_BITS {
            return None;
        }
        self.required_fragment_bits[self.required_fragment_bit_count] = (bit_index, value);
        self.required_fragment_bit_count += 1;
        Some(self)
    }

    fn fragment_requirements_match(self, fragment_bits: &[bool], bit_cursor: usize) -> bool {
        for index in 0..self.required_fragment_bit_count {
            let (bit_index, expected) = self.required_fragment_bits[index];
            let Some(target) = bit_cursor.checked_add(bit_index) else {
                return false;
            };
            if fragment_bits.get(target) != Some(&expected) {
                return false;
            }
        }
        true
    }

    fn materialize_missing_true_fragment_requirements(
        self,
        fragment_bits: &mut [bool],
        bit_cursor: usize,
    ) -> bool {
        for index in 0..self.required_fragment_bit_count {
            let (bit_index, expected) = self.required_fragment_bits[index];
            let Some(target) = bit_cursor.checked_add(bit_index) else {
                return false;
            };
            if !expected {
                if fragment_bits.get(target) != Some(&false) {
                    return false;
                }
            } else {
                let Some(slot) = fragment_bits.get_mut(target) else {
                    return false;
                };
                *slot = true;
            }
        }
        true
    }

    fn normalized_fragment_requirements(
        self,
    ) -> [(usize, bool); MAX_GENERIC_INVENTORY_REQUIRED_FRAGMENT_BITS] {
        let mut requirements = self.required_fragment_bits;
        requirements[..self.required_fragment_bit_count].sort_unstable();
        requirements
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Feature25Shape {
    branch_offset: usize,
    first_count: u32,
    first_objects_offset: usize,
    first_objects_end: usize,
    second_count: u32,
    second_objects_offset: usize,
    second_objects_end: usize,
    block_end: usize,
    missing_second_count: bool,
    legacy_tail_offset: Option<usize>,
    legacy_tail_end: Option<usize>,
}

pub(super) fn owns_fragment_tail(opcode: u8) -> bool {
    matches!(opcode, b'I' | b'G')
}

pub(super) fn terminal_fragment_storage_trim_allowed(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    start_bit_cursor: usize,
    end_bit_cursor: usize,
) -> bool {
    d5ff_terminal_fragment_storage_trim_allowed(
        bytes,
        record_offset,
        record_end,
        fragment_bits,
        start_bit_cursor,
        end_bit_cursor,
    )
}

pub(super) fn advance_verified_inventory_record(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> Option<InventoryRecordClaim> {
    let mut matched_gui_quickbar_link_shape =
        try_parse_inventory_2e00_or_2e01_gui_quickbar_link_shape(bytes, offset, record_end).filter(
            |shape| {
                inventory_2e00_or_2e01_gui_quickbar_link_fragment_bits_match(
                    shape,
                    fragment_bits,
                    *bit_cursor,
                )
            },
        );
    if matched_gui_quickbar_link_shape.is_none() {
        matched_gui_quickbar_link_shape =
            try_parse_inventory_2e00_gui_hand_trap_interleaved_tail_exact_shape(
                bytes, offset, record_end,
            )
            .filter(|shape| {
                inventory_2e00_or_2e01_gui_quickbar_link_fragment_bits_match(
                    shape,
                    fragment_bits,
                    *bit_cursor,
                )
            });
    }
    if matched_gui_quickbar_link_shape.is_none() {
        matched_gui_quickbar_link_shape =
            try_parse_inventory_2e00_gui_hand_trap_promoted_false_0800_shape(
                bytes, offset, record_end,
            )
            .filter(|shape| {
                inventory_2e00_or_2e01_gui_quickbar_link_fragment_bits_match(
                    shape,
                    fragment_bits,
                    *bit_cursor,
                )
            });
    }
    if let Some(shape) = matched_gui_quickbar_link_shape {
        *bit_cursor = bit_cursor.saturating_add(shape.fragment_bits);
        return inventory_record_claim(bytes, offset, shape.fragment_bits);
    }

    if let Some(claim) = try_advance_inventory_2000_gui_hand_trap_feature25_object_list(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return Some(claim);
    }

    if let Some(claim) = advance_verified_inventory_d5ff_hg_creature_equipment_state_shape(
        bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return Some(claim);
    }

    let candidate = try_get_legacy_live_inventory_claim_candidate(bytes, offset, record_end)?;
    let candidate = if candidate.bits <= fragment_bits.len().saturating_sub(*bit_cursor)
        && candidate.fragment_requirements_match(fragment_bits, *bit_cursor)
    {
        candidate
    } else {
        // Ambiguous generic inventory masks can have multiple byte-valid
        // candidates with different fragment-BOOL requirements.  The exact
        // verifier must choose by the decompiled BOOL stream, not by the first
        // semantically plausible byte cursor.
        try_get_generic_live_inventory_claim_candidate_matching_fragment_bits(
            bytes,
            offset,
            record_end,
            fragment_bits,
            *bit_cursor,
        )?
    };
    *bit_cursor = bit_cursor.saturating_add(candidate.bits);
    inventory_record_claim_with_feature25(bytes, offset, candidate.bits, candidate.feature25)
}

pub(super) fn verified_inventory_owner_claim_for_ee(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    expected_next_bit_cursor: usize,
) -> Option<super::LiveObjectInventoryOwnerClaim> {
    let mut proof_cursor = bit_cursor;
    let claim = advance_verified_inventory_record(
        bytes,
        offset,
        record_end,
        fragment_bits,
        &mut proof_cursor,
    )?;
    if proof_cursor != expected_next_bit_cursor {
        return None;
    }

    // Diamond `sub_455940` and EE `sub_1407B4F70` both read the inventory
    // record as `I + OBJECTID owner + WORD mask`, then walk the enabled mask
    // branches in the fixed order modelled by `mask.rs`.  Expose this claim
    // only after the same exact cursor validator succeeds so owner/mask
    // diagnostics cannot outpace the fragment-bit proof.
    Some(super::LiveObjectInventoryOwnerClaim {
        owner_id: claim.owner_id,
        mask: claim.mask,
        mask_branches: super::LiveObjectInventoryMaskBranches::from_mask(claim.mask),
        feature25: claim
            .feature25
            .map(|feature25| super::LiveObjectInventoryFeature25Claim {
                branch_offset: feature25.branch_offset,
                block_end: feature25.block_end,
                first_count: feature25.first_count,
                first_object_ids: feature25.first_object_ids,
                second_count: feature25.second_count,
                second_object_ids: feature25.second_object_ids,
                second_fragment_bit_start: bit_cursor
                    .saturating_add(feature25.second_fragment_bit_start),
                second_fragment_bit_end: bit_cursor
                    .saturating_add(feature25.second_fragment_bit_end),
                legacy_tail_object_ids: feature25.legacy_tail_object_ids,
            }),
        fragment_bits: claim.fragment_bits,
        bit_cursor,
        next_bit_cursor: proof_cursor,
    })
}

fn inventory_record_claim(
    bytes: &[u8],
    record_offset: usize,
    fragment_bits: usize,
) -> Option<InventoryRecordClaim> {
    inventory_record_claim_with_feature25(bytes, record_offset, fragment_bits, None)
}

fn inventory_record_claim_with_feature25(
    bytes: &[u8],
    record_offset: usize,
    fragment_bits: usize,
    feature25: Option<InventoryFeature25Candidate>,
) -> Option<InventoryRecordClaim> {
    Some(InventoryRecordClaim {
        owner_id: read_u32_le(bytes, record_offset.checked_add(1)?)?,
        mask: read_u16_le(bytes, record_offset.checked_add(5)?)?,
        fragment_bits,
        feature25: match feature25 {
            Some(claim) => Some(inventory_feature25_record_claim(bytes, claim)?),
            None => None,
        },
    })
}

fn inventory_feature25_record_claim(
    bytes: &[u8],
    claim: InventoryFeature25Candidate,
) -> Option<InventoryFeature25RecordClaim> {
    Some(InventoryFeature25RecordClaim {
        branch_offset: claim.branch_offset,
        block_end: claim.block_end,
        first_count: claim.first_count,
        first_object_ids: collect_feature25_object_ids(
            bytes,
            claim.first_objects_offset,
            claim.first_objects_end,
        )?,
        second_count: claim.second_count,
        second_object_ids: collect_feature25_object_ids(
            bytes,
            claim.second_objects_offset,
            claim.second_objects_end,
        )?,
        second_fragment_bit_start: claim.second_fragment_bit_start,
        second_fragment_bit_end: claim.second_fragment_bit_end,
        legacy_tail_object_ids: match (claim.legacy_tail_offset, claim.legacy_tail_end) {
            (Some(offset), Some(end)) => collect_feature25_object_ids(bytes, offset, end)?,
            _ => Vec::new(),
        },
    })
}

fn collect_feature25_object_ids(bytes: &[u8], offset: usize, end: usize) -> Option<Vec<u32>> {
    if offset > end || end > bytes.len() || (end - offset) % 4 != 0 {
        return None;
    }
    let mut object_ids = Vec::with_capacity((end - offset) / 4);
    let mut cursor = offset;
    while cursor < end {
        object_ids.push(read_u32_le(bytes, cursor)?);
        cursor = cursor.checked_add(4)?;
    }
    Some(object_ids)
}

pub(super) fn try_get_legacy_live_inventory_fragment_bit_count(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    try_get_legacy_live_inventory_claim_candidate(bytes, record_offset, record_end)
        .map(|candidate| candidate.bits)
}

fn try_get_legacy_live_inventory_claim_candidate(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    let object_id_is_legacy_shaped = matches!(object_id, 0xFFFF_FFFD | 0xFFFF_FFFE)
        || looks_like_legacy_live_object_id_value(object_id)
        || (mask == 0x0100 && inventory_0100_compact_session_owner_id_is_allowed(object_id))
        || (mask == 0x2000 && inventory_2000_current_player_owner_id_is_allowed(object_id))
        || (matches!(mask, 0x2E00 | 0x2E01)
            && inventory_gui_quickbar_link_owner_id_is_allowed(object_id))
        || (mask == 0x2A00 && inventory_2a00_object_id_is_allowed(object_id));
    if !object_id_is_legacy_shaped
        && !(mask == 0xD5FF && d5ff_live_stream_object_id_is_allowed(object_id))
    {
        return None;
    }

    if mask == 0x2A00 {
        return try_parse_inventory_2a00_shape(bytes, record_offset, record_end);
    }

    if mask == 0x2000 {
        let feature25 = try_parse_inventory_2000_record(bytes, record_offset, record_end)?;
        let feature25_bits = usize::try_from(feature25.second_count)
            .ok()?
            .saturating_mul(3);
        return Some(
            GenericInventoryCandidate::new(record_end, feature25_bits)
                .with_feature25(feature25.to_candidate(0, feature25_bits)?),
        );
    }

    if mask == 0x0400 {
        // Decompile-backed inventory/equipment delta. Diamond and EE both read
        // a clear-count byte plus slot bytes, then a set-count byte plus slot
        // bytes; the set side owns one CNW BOOL fragment bit per slot. This is
        // therefore an identity translation only after this exact cursor
        // consumption succeeds. It must never be accepted as raw passthrough.
        let set_count = try_parse_inventory_0400(bytes, record_offset.checked_add(7)?, record_end)?;
        return Some(GenericInventoryCandidate::new(
            record_end,
            usize::from(set_count),
        ));
    }

    if mask == 0x2400 {
        return try_parse_inventory_2400_slot_update_shape(bytes, record_offset, record_end)
            .map(|bits| GenericInventoryCandidate::new(record_end, bits));
    }

    if mask == 0x2700 {
        return try_parse_inventory_2700_zero_count_feature25_shape(
            bytes,
            record_offset,
            record_end,
        );
    }

    if matches!(mask, 0x2E00 | 0x2E01) {
        let shape = try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(
            bytes,
            record_offset,
            record_end,
        )?;
        if shape.read_end == record_end || shape.interleaved_fragment_tail_allowed {
            return Some(GenericInventoryCandidate::new(
                record_end,
                shape.fragment_bits,
            ));
        }
        return None;
    }

    if mask == 0xD5FF {
        if let Some(candidate) = try_parse_inventory_d5ff_hg_creature_equipment_state_shape(
            bytes,
            record_offset,
            record_end,
        ) {
            return Some(candidate);
        }
    }

    if (mask & !GENERIC_INVENTORY_PARSE_MASK) == 0 {
        // Diamond and EE both walk the inventory mask in the fixed reader order
        // modelled by the generic parser. Masks with 0x0200/0x0800 need the
        // parser's branch search, but deterministic masks such as the HG self
        // inventory D5FF packet are the same decompile-backed family and must
        // not be excluded just because they do not contain an ambiguous branch.
        return try_parse_generic_inventory_claim_with_branching(
            bytes,
            record_offset,
            record_end,
            mask,
        );
    }

    None
}

fn try_get_generic_live_inventory_claim_candidate_matching_fragment_bits(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<GenericInventoryCandidate> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    let object_id_is_legacy_shaped = matches!(object_id, 0xFFFF_FFFD | 0xFFFF_FFFE)
        || looks_like_legacy_live_object_id_value(object_id)
        || (mask == 0x0100 && inventory_0100_compact_session_owner_id_is_allowed(object_id))
        || (mask == 0x2000 && inventory_2000_current_player_owner_id_is_allowed(object_id));
    if !object_id_is_legacy_shaped || (mask & !GENERIC_INVENTORY_PARSE_MASK) != 0 {
        return None;
    }
    if matches!(
        mask,
        0x0400 | 0x2000 | 0x2400 | 0x2700 | 0x2A00 | 0x2E00 | 0x2E01 | 0xD5FF
    ) {
        return None;
    }

    try_parse_generic_inventory_claim_matching_fragment_bits(
        bytes,
        record_offset,
        record_end,
        mask,
        fragment_bits,
        bit_cursor,
    )
}

pub(super) fn try_get_legacy_live_inventory_prefix_claim(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
) -> Option<InventoryRecordPrefixClaim> {
    if record_offset > bytes.len()
        || search_end > bytes.len()
        || search_end <= record_offset
        || search_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if mask == 0x2A00 {
        return try_parse_inventory_2a00_prefix_shape(bytes, record_offset, search_end);
    }
    if mask == 0x2000 {
        return try_get_inventory_2000_current_player_prefix_claim(
            bytes,
            record_offset,
            search_end,
        );
    }
    if !matches!(object_id, 0xFFFF_FFFD | 0xFFFF_FFFE)
        && !looks_like_legacy_live_object_id_value(object_id)
        && !(mask == 0x0100 && inventory_0100_compact_session_owner_id_is_allowed(object_id))
        && !(matches!(mask, 0x2E00 | 0x2E01)
            && inventory_gui_quickbar_link_owner_id_is_allowed(object_id))
    {
        return None;
    }

    if (mask & !GENERIC_INVENTORY_PARSE_MASK) != 0 {
        return None;
    }

    if mask == 0x2B00 {
        return try_get_inventory_2b00_compact_self_state_prefix_claim(
            bytes,
            record_offset,
            search_end,
        );
    }

    if matches!(mask, 0x2E00 | 0x2E01) {
        if let Some(shape) = try_parse_inventory_2e00_gui_hand_trap_interleaved_tail_exact_shape(
            bytes,
            record_offset,
            search_end,
        ) {
            if shape.read_end > record_offset + 7 && shape.read_end < search_end {
                return Some(InventoryRecordPrefixClaim {
                    read_end: shape.read_end,
                    fragment_bits: shape.fragment_bits,
                    interleaved_fragment_tail_allowed: true,
                });
            }
        }

        if let Some(shape) = try_parse_inventory_2e00_gui_hand_trap_promoted_false_0800_prefix_shape(
            bytes,
            record_offset,
            search_end,
        ) {
            if shape.read_end > record_offset + 7 && shape.read_end < search_end {
                return Some(InventoryRecordPrefixClaim {
                    read_end: shape.read_end,
                    fragment_bits: shape.fragment_bits,
                    interleaved_fragment_tail_allowed: false,
                });
            }
        }

        let shape = try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(
            bytes,
            record_offset,
            search_end,
        )?;
        if shape.read_end <= record_offset + 7 || shape.read_end >= search_end {
            return None;
        }
        return Some(InventoryRecordPrefixClaim {
            read_end: shape.read_end,
            fragment_bits: shape.fragment_bits,
            interleaved_fragment_tail_allowed: shape.interleaved_fragment_tail_allowed,
        });
    }

    // This prefix proof is intentionally limited to deterministic inventory
    // masks. Ambiguous 0x0200/0x0800 records need an exact candidate boundary
    // to choose the byte shape, but large HG inventory state packets such as
    // D5FF have a single decompile-owned read order. When a legacy zlib stream
    // interleaves CNW fragment storage after that deterministic read body, the
    // parser can prove the read-buffer end before the scanner reaches the next
    // live-object opcode.
    if (mask & (0x0200 | 0x0800)) != 0 {
        return None;
    }

    let candidate =
        try_parse_generic_inventory_prefix_with_branching(bytes, record_offset, search_end, mask)?;
    if candidate.cursor <= record_offset + 7 || candidate.cursor >= search_end {
        return None;
    }

    Some(InventoryRecordPrefixClaim {
        read_end: candidate.cursor,
        fragment_bits: candidate.bits,
        interleaved_fragment_tail_allowed:
            generic_inventory_prefix_allows_interleaved_fragment_tail(mask, candidate),
    })
}

pub(super) fn try_get_missing_current_player_2a00_record_end(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
) -> Option<usize> {
    try_get_missing_current_player_2a00_candidate(bytes, record_offset, search_end)
        .map(|(record_end, _)| record_end)
}

fn try_get_missing_current_player_2a00_candidate(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
) -> Option<(usize, GenericInventoryCandidate)> {
    if record_offset > bytes.len()
        || search_end > bytes.len()
        || search_end <= record_offset
        || search_end - record_offset < 8
        || bytes.get(record_offset).copied()? != 0
        || read_u32_le(bytes, record_offset + 1)? != 0x0000_00FE
        || read_u16_le(bytes, record_offset + 5)? != 0x2A00
    {
        return None;
    }

    let mut candidate = bytes.get(..search_end)?.to_vec();
    candidate[record_offset] = b'I';
    for record_end in record_offset + 8..search_end.saturating_sub(1) {
        if !matches!(
            (
                bytes.get(record_end).copied(),
                bytes.get(record_end + 1).copied()
            ),
            (Some(b'G'), Some(b'I' | b'R' | b'Q'))
        ) {
            continue;
        }
        if let Some(claim) =
            try_get_legacy_live_inventory_claim_candidate(&candidate, record_offset, record_end)
        {
            return Some((record_end, claim));
        }
    }
    None
}

fn inventory_0100_compact_session_owner_id_is_allowed(object_id: u32) -> bool {
    // Local Diamond XP2 Chapter 1 emits the current player creature through
    // PlayerList as session id 0xffff_fffe, then uses compact owner id 0xfe for
    // a bounded `I/0x0100` opcode-stream inventory row. Keep the low-id
    // exception on this exact deterministic mask; broader low compact
    // inventory owners still need their own decompile/capture proof.
    object_id == 0x0000_00FE
}

fn inventory_2000_current_player_owner_id_is_allowed(object_id: u32) -> bool {
    // Local Diamond XP2 Chapter 2 emits a standalone Feature-25 inventory row
    // for the current player with compact owner id 0xfe. Diamond `sub_455940`
    // and EE `sub_1407B4F70` both read the owner OBJECTID before walking the
    // 0x2000 branch; object materialization is a later state lookup, not part
    // of the byte cursor. Keep the compact allowance scoped to this exact mask.
    object_id == 0x0000_00FE
}

fn try_get_inventory_2000_current_player_prefix_claim(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
) -> Option<InventoryRecordPrefixClaim> {
    if record_offset > bytes.len()
        || search_end > bytes.len()
        || search_end <= record_offset
        || search_end - record_offset < 24
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u32_le(bytes, record_offset + 1)? != 0x0000_00FE
        || read_u16_le(bytes, record_offset + 5)? != 0x2000
    {
        return None;
    }

    // The local XP2 Chapter 2 stream carries the already-modelled 0x2000
    // zero/zero legacy tail shape: DWORD first_count=0, DWORD second_count=0,
    // then two legacy OBJECTIDs before the next creature add. The exact
    // inventory normalizer removes those two tail OBJECTIDs for EE; this prefix
    // proof only stops the boundary scanner from swallowing the following A/5
    // record as part of one huge inventory row.
    let record_end = record_offset.checked_add(23)?;
    if record_end >= search_end
        || bytes.get(record_end).copied() != Some(b'A')
        || bytes.get(record_end + 1).copied() != Some(super::CREATURE_OBJECT_TYPE)
    {
        return None;
    }
    let shape = try_parse_inventory_2000_record(bytes, record_offset, record_end)?;
    if shape.second_count != 0 || shape.block_end != record_end {
        return None;
    }

    Some(InventoryRecordPrefixClaim {
        read_end: record_end,
        fragment_bits: 0,
        interleaved_fragment_tail_allowed: false,
    })
}

#[cfg(test)]
mod inventory_2000_current_player_tests {
    use super::*;

    #[test]
    fn compact_current_player_feature25_tail_prefix_claim_is_bounded() {
        let mut stream = vec![
            b'I',
            0xFE,
            0x00,
            0x00,
            0x00,
            0x00,
            0x20, // I/0x2000 owner 0xfe
            0x00,
            0x00,
            0x00,
            0x00, // first_count = 0
            0x00,
            0x00,
            0x00,
            0x00, // second_count = 0
            0x3F,
            0x00,
            0x00,
            0x80, // legacy object tail
            0xBE,
            0x2C,
            0x00,
            0x80, // legacy object tail
            b'A',
            super::super::CREATURE_OBJECT_TYPE,
        ];

        let prefix = try_get_legacy_live_inventory_prefix_claim(&stream, 0, stream.len())
            .expect("compact current-player I/0x2000 tail should prove a prefix boundary");
        assert_eq!(prefix.read_end, 23);
        assert_eq!(prefix.fragment_bits, 0);
        assert!(!prefix.interleaved_fragment_tail_allowed);

        let claim = try_get_legacy_live_inventory_claim_candidate(&stream, 0, prefix.read_end)
            .expect("exact legacy I/0x2000 tail record should claim");
        assert_eq!(claim.bits, 0);

        let mut record_end = prefix.read_end;
        let rewrite =
            rewrite_legacy_inventory_record_for_ee(&mut stream, 0, &mut record_end, &[], 0)
                .expect("legacy Feature-25 object tail should normalize away for EE");
        assert_eq!(rewrite.bytes_removed, 8);
        assert_eq!(record_end, 15);
        assert_eq!(
            stream.get(record_end..record_end + 2),
            Some(&[b'A', 0x05][..])
        );
    }
}

pub(super) fn repair_missing_current_player_2a00_opcode_after_4408_for_ee(
    bytes: &mut Vec<u8>,
    record_offset: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<usize> {
    let (record_end, claim) = match try_get_missing_current_player_2a00_candidate(
        bytes,
        record_offset,
        bytes.len(),
    ) {
        Some(candidate) => candidate,
        None => {
            if crate::translate::live_object_update::live_object_debug_env_enabled(
                "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
            ) {
                eprintln!(
                    "live-object missing current-player inventory repair rejected: reason=byte-shape offset={record_offset} preview={:02X?}",
                    bytes
                        .get(record_offset..record_offset.saturating_add(48).min(bytes.len()))
                        .unwrap_or(&[])
                );
            }
            return None;
        }
    };
    let original = *bytes.get(record_offset)?;

    // Local Diamond auto-inventory captures can place the current-player
    // `0x2A00` inventory body after a compact `U/5 0x4408` status record with
    // a zero byte in the live-object opcode slot. The EE/Diamond dispatchers
    // both require top-level inventory rows to enter through opcode `I`; only
    // rewrite this exact compact owner/mask/body shape after the byte parser
    // proves the 0x0200 byte-mask branch, Feature-25 list, and 0x0800 tail.
    // Local auto-inventory can also omit the true selector bits for that body;
    // materialize only those decompile-owned true bits, while any required
    // false bit must already be false on the wire, then re-run the exact EE
    // inventory cursor validator before committing the mutation.
    bytes[record_offset] = b'I';
    let mut proof_bits = fragment_bits.clone();
    if claim.bits > proof_bits.len().saturating_sub(bit_cursor)
        || !claim.materialize_missing_true_fragment_requirements(&mut proof_bits, bit_cursor)
    {
        bytes[record_offset] = original;
        return None;
    }
    let mut proof_cursor = bit_cursor;
    if advance_verified_inventory_record(
        bytes,
        record_offset,
        record_end,
        &proof_bits,
        &mut proof_cursor,
    )
    .is_none()
    {
        if crate::translate::live_object_update::live_object_debug_env_enabled(
            "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
        ) {
            eprintln!(
                "live-object missing current-player inventory repair rejected: reason=fragment-proof offset={record_offset} record_end={record_end} bit_cursor={bit_cursor} next_bits={:?}",
                fragment_bits
                    .get(bit_cursor..bit_cursor.saturating_add(16).min(fragment_bits.len()))
                    .unwrap_or(&[])
            );
        }
        bytes[record_offset] = original;
        return None;
    }
    *fragment_bits = proof_bits;
    if crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
    ) {
        eprintln!(
            "live-object missing current-player inventory repair accepted: offset={record_offset} record_end={record_end} bit_cursor={bit_cursor} proof_cursor={proof_cursor}"
        );
    }
    Some(record_end)
}

pub(super) fn repair_current_player_2a00_selector_bits_after_compact_effect_for_ee(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<InventoryFragmentBitRepair> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u16_le(bytes, record_offset + 5)? != 0x2A00
    {
        return None;
    }
    let object_id = read_u32_le(bytes, record_offset + 1)?;
    if !matches!(object_id, 0x0000_00FE | 0xFFFF_FFFE) {
        return None;
    }

    let claim = try_get_legacy_live_inventory_claim_candidate(bytes, record_offset, record_end)?;
    if claim.bits > fragment_bits.len().saturating_sub(bit_cursor) {
        return None;
    }

    // Local Dark Ranger's compact effect stream carries an already-present
    // `I/0x2A00` current-player inventory row after the `U/5 0x0008`
    // LowLightVision record. Local XP1 Chapter 1 adds the same selector-bit
    // omission after a compact `U/5 0x4408` status update, using the legacy
    // current-player sentinel owner `0xFFFFFFFE` and the shortest observed
    // WORD-list 0x0200 body. The read-buffer body proves the EE/Diamond
    // decompile-owned branch shape: 0x0200, 0x2000 Feature-25 object lists,
    // then a true 0x0800 twelve-byte tail. The legacy fragment span has the
    // same cursor length but leaves true selector BOOLs as zeroes, so
    // materialize only true requirements from that exact parser claim.
    // Required false bits still have to be false on the wire.
    let mut proof_bits = fragment_bits.clone();
    if !claim.materialize_missing_true_fragment_requirements(&mut proof_bits, bit_cursor) {
        return None;
    }
    let bits_materialized = proof_bits
        .iter()
        .zip(fragment_bits.iter())
        .filter(|(after, before)| **after && !**before)
        .count();
    if bits_materialized == 0 {
        return None;
    }

    let mut proof_cursor = bit_cursor;
    advance_verified_inventory_record(
        bytes,
        record_offset,
        record_end,
        &proof_bits,
        &mut proof_cursor,
    )?;

    *fragment_bits = proof_bits;
    Some(InventoryFragmentBitRepair { bits_materialized })
}

fn generic_inventory_prefix_allows_interleaved_fragment_tail(
    mask: u16,
    candidate: GenericInventoryCandidate,
) -> bool {
    // Diamond `sub_455940` and EE `sub_1407B4F70` both consume these
    // deterministic mask branches in the fixed generic order modelled by
    // `mask.rs`. The Starcore5/HG seq31 self-inventory stream proves that the
    // legacy server can then place chunk-local CNW fragment storage immediately
    // after the read-buffer cursor before the next live-object submessage.
    //
    // Keep this deliberately family-owned: ambiguous 0x0200/0x0800 masks are
    // excluded before the generic prefix parser reaches this helper, and the
    // caller still has to promote the adjacent bytes and re-run the exact
    // inventory reader against the resulting fragment-bit stream before the
    // packet can be emitted.
    matches!(mask, 0x0401 | 0xD5FF) && candidate.bits > 0
}

fn try_get_inventory_2b00_compact_self_state_prefix_claim(
    bytes: &[u8],
    record_offset: usize,
    search_end: usize,
) -> Option<InventoryRecordPrefixClaim> {
    if record_offset > bytes.len()
        || search_end > bytes.len()
        || search_end <= record_offset
        || search_end - record_offset < 12
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u32_le(bytes, record_offset + 1)? != 0xFFFF_FFFE
        || read_u16_le(bytes, record_offset + 5)? != 0x2B00
    {
        return None;
    }

    // Same exact Starcore5 self-inventory shape as the rewrite helper below.
    // Prefix proof is deliberately narrow: it only splits a zero-entry 0x0100
    // stream plus Feature-25 zero first-count. Pre-rewrite legacy bytes stop
    // after the first count; post-rewrite EE bytes include the inserted zero
    // second count. In both cases the final exact claim must still validate
    // fragment bits for 0x0200=false and 0x0800=false.
    if bytes.get(record_offset + 7).copied() != Some(0)
        || read_u32_le(bytes, record_offset + 8)? != 0
    {
        return None;
    }

    let ee_rewritten_end = record_offset.checked_add(16)?;
    if ee_rewritten_end < search_end && read_u32_le(bytes, record_offset + 12) == Some(0) {
        return Some(InventoryRecordPrefixClaim {
            read_end: ee_rewritten_end,
            fragment_bits: 3,
            interleaved_fragment_tail_allowed: false,
        });
    }

    let legacy_missing_second_end = record_offset.checked_add(12)?;
    if legacy_missing_second_end < search_end {
        return Some(InventoryRecordPrefixClaim {
            read_end: legacy_missing_second_end,
            fragment_bits: 3,
            interleaved_fragment_tail_allowed: false,
        });
    }

    None
}

pub(super) fn rewrite_legacy_inventory_record_for_ee(
    bytes: &mut Vec<u8>,
    record_offset: usize,
    record_end: &mut usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<InventoryRecordRewrite> {
    if record_offset > bytes.len()
        || *record_end > bytes.len()
        || *record_end <= record_offset
        || *record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let mask = read_u16_le(bytes, record_offset + 5)?;
    if mask == 0xD500 {
        repair_d500_missing_low_d5ff_mask_for_ee(
            bytes,
            record_offset,
            *record_end,
            fragment_bits,
            bit_cursor,
        )?;
        return Some(InventoryRecordRewrite {
            bytes_inserted: 0,
            bytes_removed: 0,
        });
    }

    let feature25_cursor = match mask {
        0x2000 => record_offset.checked_add(7)?,
        0x2008 => {
            // Diamond and EE both consume the low 0x0008 inventory branch before
            // the feature-25 0x2000 branch (`sub_455940` / `sub_1407B4F70`).
            // This exact HG transition capture has that 0x0008 DWORD set to
            // zero, then a legacy 0x2000 zero-first/sentinel object tail. EE has
            // no reader for that tail, so normalize only this proved branch
            // cursor to EE's exact zero/zero feature-25 list shape.
            record_offset.checked_add(7)?.checked_add(4)?
        }
        0x2B00 => {
            // Starcore5/Sooty Crow capture, verified against the EE inventory
            // reader order in `sub_1407B4F70`: 0x0200 reads two CNW BOOLs
            // first, a false first BOOL consumes no read-buffer bytes, 0x0100
            // then reads an opcode-stream count byte, and 0x2000 then reads
            // Feature-25 OBJECTID lists. The legacy server emitted the
            // removal-list first count (`0`) but omitted EE's required second
            // DWORD count before the following `A/5` live-object record. This
            // exact repair inserts that zero second count; the final strict
            // claim still has to prove the 0x0200 false branch and 0x0800 false
            // branch from fragment bits.
            try_get_inventory_2b00_missing_feature25_second_count_cursor(
                bytes,
                record_offset,
                *record_end,
            )?
        }
        0x2A00 => {
            return insert_missing_inventory_2a00_feature25_second_count_zero_for_ee(
                bytes,
                record_offset,
                record_end,
            )
            .map(|bytes_inserted| InventoryRecordRewrite {
                bytes_inserted,
                bytes_removed: 0,
            });
        }
        _ => return None,
    };

    if mask == 0x2B00 {
        return insert_missing_feature25_second_count_zero_for_ee(
            bytes,
            feature25_cursor,
            record_end,
        )
        .map(|bytes_inserted| InventoryRecordRewrite {
            bytes_inserted,
            bytes_removed: 0,
        });
    }

    normalize_legacy_feature25_tail_for_ee(bytes, feature25_cursor, record_end).map(
        |bytes_removed| InventoryRecordRewrite {
            bytes_inserted: 0,
            bytes_removed,
        },
    )
}

fn try_get_inventory_2b00_missing_feature25_second_count_cursor(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset != 12
        || bytes.get(record_offset).copied() != Some(b'I')
        || read_u32_le(bytes, record_offset + 1)? != 0xFFFF_FFFE
        || read_u16_le(bytes, record_offset + 5)? != 0x2B00
    {
        return None;
    }

    // After the zero-byte 0x0200 false branch, the decompiled 0x0100 reader
    // consumes a single opcode-stream count byte. This captured self-inventory
    // row has zero entries, so the following DWORD is the 0x2000 first-count.
    let opcode_stream_count = *bytes.get(record_offset + 7)?;
    if opcode_stream_count != 0 {
        return None;
    }
    let feature25_cursor = record_offset.checked_add(8)?;
    if feature25_cursor.checked_add(4)? != record_end || read_u32_le(bytes, feature25_cursor)? != 0
    {
        return None;
    }
    Some(feature25_cursor)
}

fn try_parse_inventory_2400_slot_update_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<usize> {
    // Decompile/capture evidence:
    //
    // * EE's CNWSMessage::WriteGameObjUpdate_WriteInventorySlotUpdate initializes
    //   an internal object-id sentinel of 0xFFFF_FFFE before emitting an inventory
    //   slot/appearance update path.
    // * The HG/Diamond capture that exposed this packet uses the legacy live-object
    //   inventory read-buffer opcode ('I') with object id 0xFFFF_FFFE and combined
    //   mask 0x2400, immediately followed by a D/5 delete record. The fragment tail
    //   has exactly two semantic bits: one for this inventory slot update and one
    //   for the D/5 record.
    //
    // This is intentionally not a broad plausibility parser. It only claims the
    // exact legacy read-buffer shape we can account for byte-for-byte, allowing the
    // higher live-object dispatcher to keep strict record boundaries instead of
    // quarantining a known semantic packet as an unknown raw zlib blob.
    if record_end.checked_sub(record_offset)? != 23 {
        return None;
    }

    if read_u32_le(bytes, record_offset + 1)? != 0xFFFF_FFFE {
        return None;
    }
    if read_u16_le(bytes, record_offset + 5)? != 0x2400 {
        return None;
    }

    let slot_count = *bytes.get(record_offset + 7)?;
    if slot_count != 1 {
        return None;
    }

    // The observed legacy shape repeats the slot/resource discriminator twice.
    // Requiring equality prevents the parser from swallowing unrelated bytes when
    // a future inventory mask needs a different decompile-backed model.
    let first_slot_or_resref = read_u16_le(bytes, record_offset + 8)?;
    let second_slot_or_resref = read_u16_le(bytes, record_offset + 10)?;
    if first_slot_or_resref != second_slot_or_resref {
        return None;
    }

    if read_u32_le(bytes, record_offset + 12)? != 0 {
        return None;
    }

    let compact_object_id = read_u32_le(bytes, record_offset + 16)?;
    let expanded_object_id = compact_object_id | 0x8000_0000;
    if !looks_like_legacy_live_object_id_value(compact_object_id)
        && !looks_like_legacy_live_object_id_value(expanded_object_id)
    {
        return None;
    }

    if read_u32_le(bytes, record_offset + 20)? != 0 {
        return None;
    }

    Some(1)
}

fn try_parse_inventory_2700_zero_count_feature25_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<GenericInventoryCandidate> {
    // Focused exact shape for captured inventory mask 0x2700:
    //
    //   0x0400 equipment delta,
    //   0x0200 two CNW BOOLs with the second BOOL false selecting the DWORD
    //          zero-count branch,
    //   0x0100 opcode stream,
    //   0x2000 Feature-25 object lists.
    //
    // The large deterministic inventory masks still use the generic reader
    // order; keep this 0x2700 path local so a capture-specific mixed branch
    // does not change unrelated masks such as D5FF.
    let trace = crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
    );
    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;

    let set_count = try_parse_inventory_0400_prefix(bytes, &mut cursor, record_end)?;
    fragment_bits = fragment_bits.checked_add(usize::from(set_count))?;
    if trace {
        eprintln!(
            "inventory 2700 stage: after_0400 offset={record_offset} record_end={record_end} cursor={cursor} bits={fragment_bits} set_count={set_count}"
        );
    }

    if cursor > record_end || record_end - cursor < 4 || read_u32_le(bytes, cursor)? != 0 {
        if trace {
            eprintln!(
                "inventory 2700 rejected: 0200-zero-dword offset={record_offset} cursor={cursor} record_end={record_end}"
            );
        }
        return None;
    }
    let second_0200_bit = fragment_bits.checked_add(1)?;
    let after_0200 =
        GenericInventoryCandidate::new(cursor.checked_add(4)?, fragment_bits.checked_add(2)?)
            .require_fragment_bit(second_0200_bit, false)?;
    let candidates = [after_0200];

    let candidates = apply_0100(bytes, &candidates, record_end);
    if trace {
        eprintln!(
            "inventory 2700 stage: after_0100 candidates={:?}",
            candidates
                .iter()
                .map(|candidate| (candidate.cursor, candidate.bits))
                .collect::<Vec<_>>()
        );
    }
    let candidates = apply_2000(bytes, &candidates, record_end, true);
    if trace {
        eprintln!(
            "inventory 2700 stage: after_2000 candidates={:?}",
            candidates
                .iter()
                .map(|candidate| (candidate.cursor, candidate.bits))
                .collect::<Vec<_>>()
        );
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.cursor == record_end)
}

#[derive(Debug, Clone, Copy)]
struct Inventory2e00Or2e01GuiQuickbarLinkShape {
    read_end: usize,
    fragment_bits: usize,
    branch_0001_extended_bit: Option<usize>,
    branch_0200_second_bit: usize,
    branch_0800_present_bit: usize,
    branch_0800_present_value: bool,
    interleaved_fragment_tail_allowed: bool,
    interleaved_fragment_tail_bytes: usize,
}

fn try_parse_inventory_2e00_or_2e01_gui_quickbar_link_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    let shape = try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(
        bytes,
        record_offset,
        record_end,
    )?;
    if shape.read_end == record_end {
        return Some(shape);
    }
    if shape.interleaved_fragment_tail_allowed
        && shape.interleaved_fragment_tail_bytes > 0
        && shape
            .read_end
            .checked_add(shape.interleaved_fragment_tail_bytes)?
            == record_end
    {
        return Some(shape);
    }
    None
}

fn try_parse_inventory_2e00_gui_hand_trap_interleaved_tail_exact_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if object_id != LEGACY_INVENTORY_GUI_HAND_TRAP_OWNER || mask != 0x2E00 {
        return None;
    }

    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;
    let set_count = try_parse_inventory_0400_prefix(bytes, &mut cursor, record_end)?;
    fragment_bits = fragment_bits.saturating_add(usize::from(set_count));

    let shape = try_parse_inventory_2e00_gui_hand_trap_interleaved_tail(
        bytes,
        object_id,
        mask,
        cursor,
        record_end,
        fragment_bits,
    )?;
    if shape
        .read_end
        .checked_add(shape.interleaved_fragment_tail_bytes)?
        == record_end
    {
        Some(shape)
    } else {
        None
    }
}

fn try_parse_inventory_2e00_gui_hand_trap_promoted_false_0800_shape(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    if record_offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= record_offset
        || record_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if object_id != LEGACY_INVENTORY_GUI_HAND_TRAP_OWNER || mask != 0x2E00 {
        return None;
    }

    // Same decompile-owned `0x2E00` GUI hand-trap reader shape as the
    // interleaved-tail form above, after the adjacent twelve read-buffer bytes
    // have been promoted into the CNW fragment bitstream.  EE and Diamond both
    // gate the fixed 12-byte `0x0800` read-buffer branch on one BOOL; this
    // post-promotion shape is valid only when that BOOL proves false.
    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;
    let set_count = try_parse_inventory_0400_prefix(bytes, &mut cursor, record_end)?;
    fragment_bits = fragment_bits.saturating_add(usize::from(set_count));

    let branch_0200_second_bit = fragment_bits.checked_add(1)?;
    if cursor > record_end || record_end - cursor < 4 || read_u32_le(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    fragment_bits = fragment_bits.checked_add(2)?;

    let feature25 = try_parse_inventory_2000_prefix_at(bytes, cursor, record_end)?;
    cursor = feature25.block_end;
    fragment_bits = fragment_bits.checked_add(
        usize::try_from(feature25.second_count)
            .ok()?
            .checked_mul(3)?,
    )?;

    let branch_0800_present_bit = fragment_bits;
    fragment_bits = fragment_bits.checked_add(1)?;
    if cursor != record_end {
        return None;
    }

    Some(Inventory2e00Or2e01GuiQuickbarLinkShape {
        read_end: cursor,
        fragment_bits,
        branch_0001_extended_bit: None,
        branch_0200_second_bit,
        branch_0800_present_bit,
        branch_0800_present_value: false,
        interleaved_fragment_tail_allowed: false,
        interleaved_fragment_tail_bytes: 0,
    })
}

fn try_parse_inventory_2e00_gui_hand_trap_promoted_false_0800_prefix_shape(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    if record_offset > bytes.len()
        || scan_end > bytes.len()
        || scan_end <= record_offset
        || scan_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if object_id != LEGACY_INVENTORY_GUI_HAND_TRAP_OWNER || mask != 0x2E00 {
        return None;
    }

    // Prefix sibling of the post-promotion exact validator above.  The raw
    // legacy stream carries twelve adjacent bytes of chunk-local CNW fragment
    // storage before the next `GQ` row; once those bytes have been promoted into
    // the fragment bitstream, the decompile-owned inventory read cursor ends
    // immediately before that following `GQ` submessage.  This is intentionally
    // scoped to the `0xFFFFFFEC/0x2E00` GUI hand-trap family so it cannot become
    // a general "split before GQ" heuristic.
    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;
    let set_count = try_parse_inventory_0400_prefix(bytes, &mut cursor, scan_end)?;
    fragment_bits = fragment_bits.saturating_add(usize::from(set_count));

    let branch_0200_second_bit = fragment_bits.checked_add(1)?;
    if cursor > scan_end || scan_end - cursor < 4 || read_u32_le(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    fragment_bits = fragment_bits.checked_add(2)?;

    let feature25 = try_parse_inventory_2000_prefix_at(bytes, cursor, scan_end)?;
    cursor = feature25.block_end;
    fragment_bits = fragment_bits.checked_add(
        usize::try_from(feature25.second_count)
            .ok()?
            .checked_mul(3)?,
    )?;

    let branch_0800_present_bit = fragment_bits;
    fragment_bits = fragment_bits.checked_add(1)?;

    if cursor >= scan_end
        || bytes.get(cursor).copied() != Some(b'G')
        || bytes.get(cursor + 1).copied() != Some(b'Q')
    {
        return None;
    }

    Some(Inventory2e00Or2e01GuiQuickbarLinkShape {
        read_end: cursor,
        fragment_bits,
        branch_0001_extended_bit: None,
        branch_0200_second_bit,
        branch_0800_present_bit,
        branch_0800_present_value: false,
        interleaved_fragment_tail_allowed: false,
        interleaved_fragment_tail_bytes: 0,
    })
}

fn inventory_2e00_or_2e01_gui_quickbar_link_fragment_bits_match(
    shape: &Inventory2e00Or2e01GuiQuickbarLinkShape,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> bool {
    if shape.fragment_bits > fragment_bits.len().saturating_sub(bit_cursor) {
        return false;
    }
    // The 0x2E01 sibling prepends the same 0x0001 compact inventory delta
    // branch that Diamond `sub_455940` and EE `sub_1407B4F70` both read as
    // WORD, DWORD, INT, BOOL. This shape is exact only for the compact branch;
    // if the BOOL is true, the extended row/string branch must be modelled
    // separately instead of accepting this cursor.
    if let Some(bit_index) = shape.branch_0001_extended_bit {
        if fragment_bits.get(bit_cursor + bit_index) != Some(&false) {
            return false;
        }
    }
    // Diamond `sub_455940` and EE `sub_1407B4F70` both drive the 0x0200
    // inventory branch from two CNW BOOLs. The first BOOL can choose either
    // clear path, but the second BOOL must be false for the captured zero-count
    // DWORD shape; otherwise the reader switches to the byte-mask path and the
    // following Feature-25 cursor is not valid.
    if fragment_bits.get(bit_cursor + shape.branch_0200_second_bit) != Some(&false) {
        return false;
    }
    // Mask 0x0800 is the discriminant for the sibling ambiguity this helper
    // resolves: true means the 12 bytes are owned by the reader, false means
    // this exact HG GUI sentinel uses those bytes as an interleaved fragment
    // tail before the next live-object record.
    fragment_bits.get(bit_cursor + shape.branch_0800_present_bit)
        == Some(&shape.branch_0800_present_value)
}

fn try_advance_inventory_2000_gui_hand_trap_feature25_object_list(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> Option<InventoryRecordClaim> {
    // Focused HG GUI hand-trap inventory record seen after area load:
    //
    //   I <0xFFFFFFEC> <mask 0x2000>
    //     0x2000: DWORD first_count, first-list OBJECTIDs,
    //             DWORD second_count, second-list OBJECTIDs,
    //             three CNW BOOLs per second-list object.
    //
    // Diamond `sub_455940` and EE `sub_1407B4F70` share this Feature-25
    // inventory branch shape. The `0xFFFFFFEC` owner remains scoped to the GUI
    // hand-trap sentinel proved from Diamond `IR_HAND_TRAP` (`sub_587890`) and
    // EE GUI quick-add handling (`sub_140860760`); generic negative sentinels
    // still do not become object-shaped inventory records.
    if offset > bytes.len()
        || record_end > bytes.len()
        || record_end <= offset
        || record_end - offset < 7
        || bytes.get(offset).copied() != Some(b'I')
    {
        return None;
    }
    let object_id = read_u32_le(bytes, offset + 1)?;
    let mask = read_u16_le(bytes, offset + 5)?;
    if object_id != LEGACY_INVENTORY_GUI_HAND_TRAP_OWNER || mask != 0x2000 {
        return None;
    }

    let feature25 = try_parse_inventory_2000_prefix_at(bytes, offset.checked_add(7)?, record_end)?;
    if feature25.block_end != record_end {
        return None;
    }
    let consumed_bits = usize::try_from(feature25.second_count)
        .ok()?
        .checked_mul(3)?;
    if consumed_bits > fragment_bits.len().saturating_sub(*bit_cursor) {
        return None;
    }

    *bit_cursor = bit_cursor.saturating_add(consumed_bits);
    inventory_record_claim_with_feature25(
        bytes,
        offset,
        consumed_bits,
        Some(feature25.to_candidate(0, consumed_bits)?),
    )
}

#[cfg(test)]
mod gui_hand_trap_feature25_tests {
    use super::*;

    #[test]
    fn hg_inventory_2000_gui_hand_trap_feature25_object_list_claims_exactly() {
        let record = [
            b'I', 0xEC, 0xFF, 0xFF, 0xFF, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
            0x00, 0x21, 0x70, 0x01, 0x80, 0x35, 0x70, 0x01, 0x80,
        ];
        let fragment_bits = [true, false, true, true, false, true];
        let mut bit_cursor = 0usize;

        let claim = try_advance_inventory_2000_gui_hand_trap_feature25_object_list(
            &record,
            0,
            record.len(),
            &fragment_bits,
            &mut bit_cursor,
        )
        .expect("HG I/0x2000 GUI hand-trap Feature-25 list should claim exactly");

        assert_eq!(claim.fragment_bits, 6);
        let feature25 = claim
            .feature25
            .as_ref()
            .expect("exact I/0x2000 claim should expose the Feature-25 lists");
        assert_eq!(feature25.branch_offset, 7);
        assert_eq!(feature25.block_end, record.len());
        assert_eq!(feature25.first_count, 0);
        assert!(feature25.first_object_ids.is_empty());
        assert_eq!(feature25.second_count, 2);
        assert_eq!(feature25.second_object_ids, [0x8001_7021, 0x8001_7035]);
        assert_eq!(feature25.second_fragment_bit_start, 0);
        assert_eq!(feature25.second_fragment_bit_end, 6);
        assert!(feature25.legacy_tail_object_ids.is_empty());
        assert_eq!(bit_cursor, 6);
    }
}

fn try_parse_inventory_2e00_or_2e01_gui_quickbar_link_prefix(
    bytes: &[u8],
    record_offset: usize,
    scan_end: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    // Exact HG/Diamond inventory read-buffer shapes exposed by the seq40/seq42
    // quarantines:
    //
    //   I <object id> <mask 0x2E00>
    //     0x0400: BYTE clear_count, clear slots, BYTE set_count, set slots,
    //             set_count CNW BOOLs
    //     0x0200: BOOL, BOOL, DWORD zero-count branch
    //     0x2000: DWORD first_count, OBJECTIDs, DWORD second_count, OBJECTIDs,
    //             three CNW BOOLs per second-list object
    //     0x0800: BOOL-present, 12 read-buffer bytes
    //
    //   I <object id> <mask 0x2E01>
    //     0x0001: WORD, DWORD, INT, compact-branch BOOL=false
    //     then the same 0x0400/0x0200/0x2000/0x0800 sequence above
    //
    // Diamond client `sub_455940` and EE client `sub_1407B4F70` use the same
    // reader order and branch gates here. This is therefore an identity
    // translation only after the typed cursor proof succeeds; it is not a raw
    // passthrough escape hatch. The following read-buffer byte must be `G Q`
    // because the server immediately emits the quickbar-link GUI row block as
    // the next live-object submessage.
    if record_offset > bytes.len()
        || scan_end > bytes.len()
        || scan_end <= record_offset
        || scan_end - record_offset < 7
        || bytes.get(record_offset).copied() != Some(b'I')
    {
        return None;
    }
    let mask = read_u16_le(bytes, record_offset + 5)?;
    if !matches!(mask, 0x2E00 | 0x2E01) {
        return None;
    }

    let object_id = read_u32_le(bytes, record_offset + 1)?;
    if !inventory_gui_quickbar_link_owner_id_is_allowed(object_id) {
        return None;
    }

    let mut cursor = record_offset.checked_add(7)?;
    let mut fragment_bits = 0usize;
    let mut branch_0001_extended_bit = None;

    if (mask & 0x0001) != 0 {
        if cursor > scan_end || scan_end - cursor < 10 {
            return None;
        }
        branch_0001_extended_bit = Some(fragment_bits);
        cursor = cursor.checked_add(10)?;
        fragment_bits = fragment_bits.saturating_add(1);
    }

    let set_count = try_parse_inventory_0400_prefix(bytes, &mut cursor, scan_end)?;
    fragment_bits = fragment_bits.saturating_add(usize::from(set_count));

    if let Some(shape) = try_parse_inventory_2e01_large_equipment_quickbar_link_tail(
        bytes,
        mask,
        cursor,
        scan_end,
        fragment_bits,
        branch_0001_extended_bit,
        set_count,
    ) {
        return Some(shape);
    }

    let branch_0200_first_bit = fragment_bits;
    let branch_0200_second_bit = branch_0200_first_bit.checked_add(1)?;
    if cursor > scan_end || scan_end - cursor < 4 || read_u32_le(bytes, cursor)? != 0 {
        return None;
    }
    cursor = cursor.checked_add(4)?;
    fragment_bits = fragment_bits.saturating_add(2);

    let feature25 = try_parse_inventory_2000_prefix_at(bytes, cursor, scan_end)?;
    cursor = feature25.block_end;
    fragment_bits = fragment_bits.saturating_add(
        usize::try_from(feature25.second_count)
            .ok()?
            .checked_mul(3)?,
    );

    let branch_0800_present_bit = fragment_bits;
    if cursor > scan_end || scan_end - cursor < 12 {
        return None;
    }
    cursor = cursor.checked_add(12)?;
    fragment_bits = fragment_bits.saturating_add(1);

    if cursor < scan_end
        && (bytes.get(cursor).copied() != Some(b'G')
            || bytes.get(cursor + 1).copied() != Some(b'Q'))
    {
        return None;
    }

    Some(Inventory2e00Or2e01GuiQuickbarLinkShape {
        read_end: cursor,
        fragment_bits,
        branch_0001_extended_bit,
        branch_0200_second_bit,
        branch_0800_present_bit,
        branch_0800_present_value: true,
        interleaved_fragment_tail_allowed: false,
        interleaved_fragment_tail_bytes: 0,
    })
}

fn try_parse_inventory_2e01_large_equipment_quickbar_link_tail(
    bytes: &[u8],
    mask: u16,
    cursor: usize,
    scan_end: usize,
    fragment_bits: usize,
    branch_0001_extended_bit: Option<usize>,
    set_count: u8,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    // Focused sibling of the existing 0x2E01 quickbar-link inventory family.
    // The decompile-backed common prefix is unchanged: compact 0x0001, then
    // 0x0400 equipment-delta bytes. The source stream carries a large 0x0400
    // set-list followed by an eleven-byte bounded tail before the next `GQ`
    // row stream. Keep this intentionally narrow so it cannot become a
    // general inventory plausibility parser.
    if mask != 0x2E01 || set_count < 16 {
        return None;
    }

    const QUICKBAR_LINK_INTERLEAVED_FRAGMENT_TAIL_BYTES: usize = 11;
    let span_end = cursor.checked_add(QUICKBAR_LINK_INTERLEAVED_FRAGMENT_TAIL_BYTES)?;
    if cursor > scan_end {
        return None;
    }
    if cursor < scan_end
        && (span_end > scan_end
            || (span_end < scan_end
                && (bytes.get(span_end).copied() != Some(b'G')
                    || bytes.get(span_end + 1).copied() != Some(b'Q'))))
    {
        return None;
    }

    let branch_0200_first_bit = fragment_bits;
    let branch_0200_second_bit = branch_0200_first_bit.checked_add(1)?;
    let branch_0800_present_bit = branch_0200_second_bit.checked_add(1)?;
    Some(Inventory2e00Or2e01GuiQuickbarLinkShape {
        read_end: cursor,
        fragment_bits: fragment_bits.checked_add(3)?,
        branch_0001_extended_bit,
        branch_0200_second_bit,
        branch_0800_present_bit,
        branch_0800_present_value: false,
        interleaved_fragment_tail_allowed: true,
        interleaved_fragment_tail_bytes: QUICKBAR_LINK_INTERLEAVED_FRAGMENT_TAIL_BYTES,
    })
}

fn try_parse_inventory_2e00_gui_hand_trap_interleaved_tail(
    bytes: &[u8],
    object_id: u32,
    mask: u16,
    cursor: usize,
    scan_end: usize,
    fragment_bits: usize,
) -> Option<Inventory2e00Or2e01GuiQuickbarLinkShape> {
    // Focused HG area-entry sibling exposed by the live Starcore5 stream:
    //
    //   I <0xFFFFFFEC> <mask 0x2E00>
    //     0x0400 equipment-delta bytes,
    //     0x0200 two BOOLs + DWORD zero-count branch,
    //     0x2000 Feature-25 object lists,
    //     0x0800 BOOL=false,
    //     12 bytes of chunk-local CNW fragment storage before the next P/5 add.
    //
    // EE `sub_1407B4F70` proves the key branch behavior: after the 0x2000
    // object-list reader, mask 0x0800 reads one BOOL and consumes the 12-byte
    // appearance/status read-buffer block only when that BOOL is true. Diamond
    // `sub_455940` follows the same mask order. Therefore this shape is not an
    // alternate raw pass-through; it is the false 0x0800 branch plus a bounded
    // interleaved fragment-storage span owned by this exact GUI sentinel family.
    const GUI_HAND_TRAP_FRAGMENT_TAIL_BYTES: usize = 12;

    if mask != 0x2E00 || object_id != LEGACY_INVENTORY_GUI_HAND_TRAP_OWNER {
        return None;
    }

    let branch_0200_second_bit = fragment_bits.checked_add(1)?;
    if cursor > scan_end || scan_end - cursor < 4 || read_u32_le(bytes, cursor)? != 0 {
        return None;
    }
    let cursor = cursor.checked_add(4)?;
    let fragment_bits = fragment_bits.checked_add(2)?;

    let feature25 = try_parse_inventory_2000_prefix_at(bytes, cursor, scan_end)?;
    let cursor = feature25.block_end;
    let fragment_bits = fragment_bits.checked_add(
        usize::try_from(feature25.second_count)
            .ok()?
            .checked_mul(3)?,
    )?;

    let branch_0800_present_bit = fragment_bits;
    let fragment_bits = fragment_bits.checked_add(1)?;
    if cursor
        .checked_add(GUI_HAND_TRAP_FRAGMENT_TAIL_BYTES)
        .is_none_or(|tail_end| tail_end > scan_end)
    {
        return None;
    }

    Some(Inventory2e00Or2e01GuiQuickbarLinkShape {
        read_end: cursor,
        fragment_bits,
        branch_0001_extended_bit: None,
        branch_0200_second_bit,
        branch_0800_present_bit,
        branch_0800_present_value: false,
        interleaved_fragment_tail_allowed: true,
        interleaved_fragment_tail_bytes: GUI_HAND_TRAP_FRAGMENT_TAIL_BYTES,
    })
}

fn inventory_gui_quickbar_link_owner_id_is_allowed(object_id: u32) -> bool {
    // Diamond also uses 0xFFFFFFFE as the current-player/inventory traversal
    // sentinel. In the Diamond decompile this appears in the inventory row walk
    // state (`sub_4567C0`, e.g. 00456850/00456A92/00456AAC), and the EE/Diamond
    // live-inventory readers still consume the same 0x2E00/0x2E01 mask branches
    // before the following `GQ` row stream. Keep the allowance scoped to this
    // exact quickbar-link family so other negative sentinels still need their
    // own parser proof.
    // Diamond registers the `0xFFFFFFEC` inventory/GUI sentinel in the
    // `IR_HAND_TRAP` action table (`sub_587890` around `00587979`), and EE's
    // GUI quick-add handler branches on the same signed high-word value
    // (`sub_140860760` around `140860845`).  The live HG stream uses that
    // sentinel as the owner id for the already-modelled `I/0x2E00` GUI
    // quickbar-link row packet.  Keep the exception scoped to this exact
    // packet family rather than teaching generic inventory that every negative
    // sentinel is object-shaped.
    looks_like_legacy_live_object_id_value(object_id)
        || object_id == LEGACY_INVENTORY_CURRENT_PLAYER_OWNER
        || object_id == LEGACY_INVENTORY_GUI_HAND_TRAP_OWNER
}

fn try_parse_inventory_0400_prefix(
    bytes: &[u8],
    cursor: &mut usize,
    record_end: usize,
) -> Option<u8> {
    if *cursor >= record_end {
        return None;
    }
    let clear_count = usize::from(bytes[*cursor]);
    *cursor = cursor.checked_add(1)?;
    if clear_count > record_end.saturating_sub(*cursor) {
        return None;
    }
    *cursor = cursor.checked_add(clear_count)?;
    if *cursor >= record_end {
        return None;
    }
    let set_count = bytes[*cursor];
    *cursor = cursor.checked_add(1)?;
    if usize::from(set_count) > record_end.saturating_sub(*cursor) {
        return None;
    }
    *cursor = cursor.checked_add(usize::from(set_count))?;
    Some(set_count)
}

mod bit_count;
mod categories;
mod d5ff;
mod equipment_delta;
mod feature25;
mod icon_list;
mod mask;
mod opcode_stream;

use bit_count::*;
use categories::*;
use d5ff::*;
use equipment_delta::*;
use feature25::*;
use icon_list::*;
use mask::*;
use opcode_stream::*;

#[cfg(test)]
mod compact_0001_handoff_tests {
    use super::*;

    #[test]
    fn inventory_0401_compact_handoff_requires_false_0001_bit() {
        // Diamond sub_455940 (00455AAD..00455D80) and EE sub_1407B4F70
        // (1407B51ED..1407B559F) both read the 0x0001 branch as SHORT, DWORD,
        // INT, BOOL. The false BOOL is the compact handoff to 0x0400; the true
        // BOOL owns an extended tail before later mask branches.
        let record = [
            b'I', 0xEC, 0xFF, 0xFF, 0xFF, // owner id
            0x01, 0x04, // mask 0x0401: 0x0001 state + 0x0400 equipment delta
            0x95, 0x00, // 0x0001 state SHORT
            0xD4, 0xD9, 0xE0, 0x05, // 0x0001 state DWORD
            0xEB, 0x0A, 0x00, 0x00, // 0x0001 state INT
            0x02, 0x1E, 0x6B, // 0x0400 clear-count and clear slots
            0x01, 0x6B, // 0x0400 set-count and set slot
        ];

        let mut compact_cursor = 0usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &[false, true],
            &mut compact_cursor,
        )
        .expect("false 0x0001 BOOL should compact-hand off to 0x0400");
        assert_eq!(claim.fragment_bits, 2);
        assert_eq!(compact_cursor, 2);

        let mut extended_cursor = 0usize;
        assert!(
            advance_verified_inventory_record(
                &record,
                0,
                record.len(),
                &[true, true],
                &mut extended_cursor,
            )
            .is_none(),
            "true 0x0001 BOOL must not compact-hand off directly to 0x0400"
        );
        assert_eq!(extended_cursor, 0);
    }
}

#[cfg(test)]
mod current_player_2a00_selector_repair_tests {
    use super::*;

    #[test]
    fn current_player_2a00_byte_mask_tail_materializes_only_selector_bits() {
        let record = [
            b'I', 0xFE, 0x00, 0x00, 0x00, 0x00, 0x2A, 0x02, 0xDE, 0x7B, 0x00, 0x00, 0x00, 0x00,
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x80, 0x0E, 0x0D,
            0x0D, 0x0A, 0x13, 0x0A, 0x0C, 0x0D, 0x0F, 0x0A, 0x13, 0x0A,
        ];
        let mut fragment_bits = vec![false; 32];
        fragment_bits[3] = true;
        let repair = repair_current_player_2a00_selector_bits_after_compact_effect_for_ee(
            &record,
            0,
            record.len(),
            &mut fragment_bits,
            3,
        )
        .expect("current-player 2A00 selector repair should be exact");

        assert_eq!(repair.bits_materialized, 2);
        assert_eq!(fragment_bits[4], true, "0x0200 byte-mask selector");
        assert_eq!(fragment_bits[11], true, "0x0800 true-tail selector");

        let mut bit_cursor = 3usize;
        let claim = advance_verified_inventory_record(
            &record,
            0,
            record.len(),
            &fragment_bits,
            &mut bit_cursor,
        )
        .expect("repaired inventory record should exact-claim");
        assert_eq!(claim.fragment_bits, 9);
        assert_eq!(bit_cursor, 12);
    }
}

#[cfg(test)]
mod quickbar_link_tail_tests {
    use super::*;

    fn large_equipment_2e01_prefix(set_count: u8) -> Vec<u8> {
        let mut record = vec![b'I'];
        record.extend_from_slice(&LEGACY_INVENTORY_CURRENT_PLAYER_OWNER.to_le_bytes());
        record.extend_from_slice(&0x2E01u16.to_le_bytes());
        record.extend_from_slice(&[0; 10]); // 0x0001 compact branch bytes.
        record.push(0); // 0x0400 clear_count.
        record.push(set_count);
        record.extend(std::iter::repeat(0x2A).take(usize::from(set_count)));
        record
    }

    #[test]
    fn large_equipment_2e01_prefix_requires_bounded_fragment_tail() {
        let record = large_equipment_2e01_prefix(16);

        assert!(
            try_get_legacy_live_inventory_prefix_claim(&record, 0, record.len()).is_none(),
            "0x2E01 large-equipment prefix must not invent an absent interleaved tail span"
        );
    }

    #[test]
    fn large_equipment_2e01_tail_prefix_stops_before_following_gq() {
        let mut stream = large_equipment_2e01_prefix(16);
        let read_end = stream.len();
        stream.extend_from_slice(&[
            0x0E, 0x16, 0x14, 0x12, 0x40, 0x0E, 0x1E, 0x26, 0x24, 0x22, 0x50,
        ]);
        stream.extend_from_slice(&[b'G', b'Q', 0]);

        let prefix = try_get_legacy_live_inventory_prefix_claim(&stream, 0, stream.len())
            .expect("bounded 0x2E01 interleaved-tail prefix should parse");

        assert_eq!(prefix.read_end, read_end);
        assert!(prefix.interleaved_fragment_tail_allowed);
    }
}
