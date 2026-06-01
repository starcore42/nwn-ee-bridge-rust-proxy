//! EE external-object id canonicalization for verified live-object streams.
//!
//! This module is intentionally narrow: it does not discover record boundaries
//! itself, and it does not make gameplay decisions. It only runs after the
//! parent live-object validator has produced typed record mentions.
//!
//! Decompile anchors:
//!
//! - EE door add `HandleServerToPlayerDoorUpdate_Add` (`sub_140796DD0`) reads
//!   the live-object id, creates/loads the `CNWCDoor`, then calls
//!   `CGameObjectArray::AddExternalObject(&id, object, ...)`.
//! - EE placeable add `HandleServerToPlayerPlaceableUpdate_Add`
//!   (`sub_1407A7800`) follows the same `AddExternalObject(&id, object, ...)`
//!   path.
//! - EE `CGameObjectArray::AddExternalObject` stores the stripped id in the
//!   high-bit external bucket and then ORs the local id with `0x80000000`.
//! - EE creature add `HandleServerToPlayerCreatureUpdate_Add`
//!   (`sub_14077F870`) also calls `AddExternalObject(&id, creature, ...)`.
//! - EE creature appearance `HandleServerToPlayerCreatureUpdate_Appearance`
//!   (`sub_14077FE10`) reads `OBJECTID`, then the appearance mask, then resolves
//!   the creature pointer before consuming the remaining appearance body. If the
//!   stream keeps a compact Diamond id here, EE logs `EXOWARNING: pCreature`.
//! - EE update readers resolve later `U/P/D` object ids through the object
//!   array. A compact Diamond id such as `0x00000003` therefore materializes
//!   through AddExternalObject but is not findable by a later EE update unless
//!   the stream uses the canonical external id `0x80000003`.
//! - Player creatures are a narrower stateful case. PlayerList can prove that
//!   the session-local creature id is `0xffff_ffNN`, while the matching local
//!   Diamond live-object add may encode only compact `0x000000NN`. When that
//!   alias is already proven by PlayerList, the creature add must use the full
//!   session id so EE's later `ReadOBJECTIDServer`/object-array lookups resolve
//!   the same stripped key.

use std::collections::BTreeMap;

use super::{
    CREATURE_OBJECT_TYPE, DOOR_OBJECT_TYPE, GAME_OBJECT_UPDATE_MAJOR, HIGH_LEVEL_ENVELOPE,
    HIGH_LEVEL_HEADER_BYTES, LIVE_OBJECT_MINOR, MAX_COMPACT_LEGACY_LIVE_OBJECT_ID,
    MIN_COMPACT_LEGACY_LIVE_OBJECT_ID, PLACEABLE_OBJECT_TYPE, claim_payload_if_verified,
};

const CNW_LENGTH_BYTES: usize = 4;
const EXTERNAL_OBJECT_ID_BIT: u32 = 0x8000_0000;

pub(crate) fn looks_like_legacy_live_object_id_value(object_id: u32) -> bool {
    object_id != 0
        && object_id != u32::MAX
        && (has_known_legacy_live_object_id_namespace(object_id)
            || is_compact_legacy_object_id(object_id))
}

pub(crate) fn looks_like_legacy_live_object_id_value_with_compact_min(
    object_id: u32,
    min_compact: u32,
) -> bool {
    object_id != 0
        && object_id != u32::MAX
        && (has_known_legacy_live_object_id_namespace(object_id)
            || (min_compact..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID).contains(&object_id))
}

pub(crate) fn has_known_legacy_live_object_id_namespace(object_id: u32) -> bool {
    matches!(
        object_id & 0xFF00_0000,
        // EE's decompiled readers treat OBJECTID as an opaque DWORD. These
        // namespaces are scanner/validator false-positive guards gathered from
        // verified Diamond/HG traffic, not module-specific gameplay rules.
        0x8000_0000
            | 0x8800_0000
            | 0xFF00_0000
            | 0x0100_0000
            | 0x0500_0000
            | 0x0800_0000
            | 0x3500_0000
            | 0xAC00_0000
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveObjectExternalObjectIdCanonicalizeSummary {
    pub compact_add_ids_observed: u32,
    pub add_ids_rewritten: u32,
    pub reference_ids_rewritten: u32,
}

impl LiveObjectExternalObjectIdCanonicalizeSummary {
    pub fn changed(self) -> bool {
        self.add_ids_rewritten != 0 || self.reference_ids_rewritten != 0
    }
}

pub fn canonicalize_compact_external_object_ids_payload_for_ee(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectExternalObjectIdCanonicalizeSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let declared = usize::try_from(read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES || declared > payload.len() {
        return None;
    }

    let claim = claim_payload_if_verified(payload)?;
    let mut compact_to_external = BTreeMap::<(u8, u32), u32>::new();
    let mut summary = LiveObjectExternalObjectIdCanonicalizeSummary::default();

    for mention in &claim.mentions {
        if mention.opcode != b'A' || !is_supported_external_object_type(mention.object_type) {
            continue;
        }
        if !is_compact_legacy_object_id(mention.object_id) {
            continue;
        }
        let external_id = mention.object_id | EXTERNAL_OBJECT_ID_BIT;
        compact_to_external.insert((mention.object_type, mention.object_id), external_id);
        summary.compact_add_ids_observed = summary.compact_add_ids_observed.saturating_add(1);
    }

    if compact_to_external.is_empty() {
        return None;
    }

    let live_bytes_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    for mention in &claim.mentions {
        if !matches!(mention.opcode, b'A' | b'U' | b'P' | b'D') {
            continue;
        }
        let Some(external_id) = compact_to_external
            .get(&(mention.object_type, mention.object_id))
            .copied()
        else {
            continue;
        };
        let object_id_offset = live_bytes_start
            .checked_add(mention.record_offset)?
            .checked_add(2)?;
        if object_id_offset + 4 > declared || object_id_offset + 4 > payload.len() {
            return None;
        }
        if read_u32_le(payload, object_id_offset)? == external_id {
            continue;
        }
        write_u32_le(payload, object_id_offset, external_id)?;
        if mention.opcode == b'A' {
            summary.add_ids_rewritten = summary.add_ids_rewritten.saturating_add(1);
        } else {
            summary.reference_ids_rewritten = summary.reference_ids_rewritten.saturating_add(1);
        }
    }

    if !summary.changed() {
        return None;
    }

    claim_payload_if_verified(payload)?;
    Some(summary)
}

pub fn canonicalize_player_session_creature_ids_payload_for_ee<F>(
    payload: &mut Vec<u8>,
    mut session_creature_id_for_compact: F,
) -> Option<LiveObjectExternalObjectIdCanonicalizeSummary>
where
    F: FnMut(u32) -> Option<u32>,
{
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let declared = usize::try_from(read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES || declared > payload.len() {
        return None;
    }

    let claim = claim_payload_if_verified(payload)?;
    let mut compact_to_session = BTreeMap::<u32, u32>::new();
    let mut summary = LiveObjectExternalObjectIdCanonicalizeSummary::default();

    for mention in &claim.mentions {
        let Some(compact_id) = player_session_compact_id_from_mention(mention) else {
            continue;
        };
        let Some(session_id) = session_creature_id_for_compact(compact_id) else {
            continue;
        };
        if compact_to_session.insert(compact_id, session_id).is_none() {
            summary.compact_add_ids_observed = summary.compact_add_ids_observed.saturating_add(1);
        }
    }

    if compact_to_session.is_empty() {
        return None;
    }

    let live_bytes_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    for mention in &claim.mentions {
        let Some(compact_id) = player_session_compact_id_from_mention(mention) else {
            continue;
        };
        let Some(session_id) = compact_to_session.get(&compact_id).copied() else {
            continue;
        };
        let object_id_offset = live_bytes_start
            .checked_add(mention.record_offset)?
            .checked_add(match mention.opcode {
                b'I' => 1,
                b'A' | b'U' | b'P' | b'D' => 2,
                _ => continue,
            })?;
        if object_id_offset + 4 > declared || object_id_offset + 4 > payload.len() {
            return None;
        }
        if read_u32_le(payload, object_id_offset)? == session_id {
            continue;
        }
        write_u32_le(payload, object_id_offset, session_id)?;
        if mention.opcode == b'A' {
            summary.add_ids_rewritten = summary.add_ids_rewritten.saturating_add(1);
        } else {
            summary.reference_ids_rewritten = summary.reference_ids_rewritten.saturating_add(1);
        }
    }

    if !summary.changed() {
        return None;
    }

    claim_payload_if_verified(payload)?;
    Some(summary)
}

fn player_session_compact_id_from_mention(mention: &super::LiveObjectRecordMention) -> Option<u32> {
    match mention.opcode {
        b'I' => compact_creature_id_from_live_object_wire(mention.object_id),
        b'A' | b'U' | b'P' | b'D' if mention.object_type == CREATURE_OBJECT_TYPE => {
            compact_creature_id_from_live_object_wire(mention.object_id)
        }
        _ => None,
    }
}

fn is_supported_external_object_type(object_type: u8) -> bool {
    matches!(
        object_type,
        CREATURE_OBJECT_TYPE | DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE
    )
}

pub(crate) fn is_compact_legacy_object_id(object_id: u32) -> bool {
    (MIN_COMPACT_LEGACY_LIVE_OBJECT_ID..=MAX_COMPACT_LEGACY_LIVE_OBJECT_ID).contains(&object_id)
}

pub(crate) fn equivalent_legacy_external_object_ids(left: u32, right: u32) -> bool {
    if left == right {
        return true;
    }

    match (
        compact_external_object_id_from_live_object_wire(left),
        compact_external_object_id_from_live_object_wire(right),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn compact_creature_id_from_live_object_wire(object_id: u32) -> Option<u32> {
    if is_compact_legacy_object_id(object_id) {
        return Some(object_id);
    }
    if (object_id & EXTERNAL_OBJECT_ID_BIT) == 0 {
        return None;
    }
    let compact_id = object_id & !EXTERNAL_OBJECT_ID_BIT;
    is_compact_legacy_object_id(compact_id).then_some(compact_id)
}

fn compact_external_object_id_from_live_object_wire(object_id: u32) -> Option<u32> {
    if is_compact_legacy_object_id(object_id) {
        return Some(object_id);
    }
    if (object_id & EXTERNAL_OBJECT_ID_BIT) == 0 {
        return None;
    }
    let compact_id = object_id & !EXTERNAL_OBJECT_ID_BIT;
    is_compact_legacy_object_id(compact_id).then_some(compact_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_live_object_id_filter_accepts_known_external_namespaces() {
        for object_id in [
            0x8000_0001,
            0x8800_0001,
            0xFF00_0001,
            0x0100_0001,
            0x0500_0001,
            0x0800_0001,
            0x3500_0001,
            0xAC00_0001,
        ] {
            assert!(
                looks_like_legacy_live_object_id_value(object_id),
                "namespace for 0x{object_id:08X} should be accepted before focused parser proof"
            );
        }
    }

    #[test]
    fn shared_live_object_id_filter_keeps_null_and_sentinel_rejected() {
        assert!(!looks_like_legacy_live_object_id_value(0));
        assert!(!looks_like_legacy_live_object_id_value(u32::MAX));
    }

    #[test]
    fn compact_floor_variant_preserves_inventory_false_positive_guard() {
        assert!(looks_like_legacy_live_object_id_value(0x0000_00FE));
        assert!(!looks_like_legacy_live_object_id_value_with_compact_min(
            0x0000_00FE,
            0x0000_1000,
        ));
        assert!(looks_like_legacy_live_object_id_value_with_compact_min(
            0x0000_1000,
            0x0000_1000,
        ));
        assert!(looks_like_legacy_live_object_id_value_with_compact_min(
            0xAC00_0001,
            0x0000_1000,
        ));
    }

    #[test]
    fn compact_and_external_forms_compare_as_same_live_object() {
        assert!(equivalent_legacy_external_object_ids(
            0x0000_11FE,
            0x8000_11FE,
        ));
        assert!(!equivalent_legacy_external_object_ids(
            0x0000_11FE,
            0x8000_11FF,
        ));
    }
}
