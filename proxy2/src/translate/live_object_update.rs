//! Decompile-backed `GameObjUpdate_LiveObject` update-record translation.
//!
//! Keep this module narrow. It owns the legacy 1.69 live-object `U` record
//! shape and emits the EE dialect shape for the same semantic update. The M
//! frame layer remains responsible for sockets, deflate, CRC, and sequence
//! repair; this module only rewrites an already-normalized `P 05 01` payload.
//!
//! Anchors used:
//!
//! - EE `CNWCMessage::HandleServerToPlayerGameObjectUpdate` (`sub_14079BCE0`)
//!   dispatches update submessages and logs `Unknown Update sub-message` when
//!   the read stream is shifted.
//! - EE door/placeable update helpers read compact orientation and extra door
//!   state/name-mode BOOLs that are absent from Diamond/HG's 1.69 stream.
//! - The mature C++ bridge's `RewriteLegacyLiveObjectUpdateRecords` carries
//!   the same masks and cursor discipline; this Rust port keeps the transform
//!   focused here instead of folding it into `m_frame`.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::{Mutex, OnceLock},
};

mod add;
mod add_guard;
mod appearance;
mod bits;
mod boundary;
mod class_rows;
mod creature;
mod creature_add;
mod cursor;
mod door;
mod effects;
#[cfg(test)]
mod fixture_free_tests;
mod fragment_spans;
mod gui;
mod inventory;
mod item;
mod locstring;
pub(crate) mod object_ids;
mod placeable;
mod reader;
mod record;
mod tail_repair;
#[cfg(all(test, hgbridge_private_fixtures))]
mod tests;
mod trigger;
mod visual_effect_rows;
pub(crate) mod visual_transform;
mod world_status;
mod writer;

pub(crate) fn observe_visual_effect_hak_order_top_first(hak_order_top_first: &[String]) {
    visual_effect_rows::observe_hak_order_top_first(hak_order_top_first);
}

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const GAME_OBJECT_UPDATE_MAJOR: u8 = 0x05;
const LIVE_OBJECT_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;

const CREATURE_OBJECT_TYPE: u8 = 0x05;
const ITEM_OBJECT_TYPE: u8 = 0x06;
const TRIGGER_OBJECT_TYPE: u8 = 0x07;
const PLACEABLE_OBJECT_TYPE: u8 = 0x09;
const DOOR_OBJECT_TYPE: u8 = 0x0A;

const LEGACY_UPDATE_HEADER_BYTES: usize = 10;
const LEGACY_UPDATE_POSITION_MASK: u32 = 0x0000_0001;
const LEGACY_UPDATE_ORIENTATION_MASK: u32 = 0x0000_0002;
const LEGACY_UPDATE_SCALE_STATE_MASK: u32 = 0x0000_0004;
const LEGACY_UPDATE_STATE_MASK: u32 = 0x0000_0010;
const LEGACY_UPDATE_APPEARANCE_MASK: u32 = 0x0000_0020;
const LEGACY_DOOR_PLACEABLE_LOW_TAIL_MASK: u32 = 0x0000_00C0;
const LEGACY_UPDATE_NAME_MASK: u32 = 0x0008_0000;
const LEGACY_UPDATE_POSITION_READ_BYTES: usize = 6;
const LEGACY_UPDATE_POSITION_FRAGMENT_BITS: usize = 2;
const EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES: usize = 1;
const EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS: usize = 5;
const EE_UPDATE_ORIENTATION_VECTOR_READ_BYTES: usize = 6;
const EE_UPDATE_ORIENTATION_VECTOR_FRAGMENT_BITS: usize = 1;
const EE_UPDATE_APPEARANCE_WORD_READ_BYTES: usize = 2;
const EE_UPDATE_APPEARANCE_RESREF_READ_BYTES: usize = 16;
const EE_UPDATE_SCALE_STATE_READ_BYTES: usize = 6;
const LEGACY_UPDATE_STATE_FRAGMENT_BITS: usize = 5;

const MIN_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x0000_0001;
const MAX_COMPACT_LEGACY_LIVE_OBJECT_ID: u32 = 0x00FF_FFFF;
const MAX_LIVE_OBJECT_NAME_BYTES: usize = 128;
const MAX_REASONABLE_LIVE_PAYLOAD_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LiveObjectFixtureDumpKey {
    reason: String,
    signature: Vec<u8>,
}

static LIVE_OBJECT_FIXTURE_DUMP_KEYS: OnceLock<Mutex<HashSet<LiveObjectFixtureDumpKey>>> =
    OnceLock::new();

fn live_object_fixture_dump_signature(payload: &[u8], sanitized_reason: &str) -> Vec<u8> {
    let mut signature = payload.to_vec();
    if sanitized_reason.starts_with("live-object-declared-length-repair-len")
        && signature.len() >= HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        && signature.first().copied() == Some(HIGH_LEVEL_ENVELOPE)
        && signature.get(1).copied() == Some(GAME_OBJECT_UPDATE_MAJOR)
        && signature.get(2).copied() == Some(LIVE_OBJECT_MINOR)
    {
        signature[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES].fill(0);
    }
    signature
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn declared_length_repair_dump_signature_ignores_only_declared_slot() {
        let mut first = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        first.extend_from_slice(&0x0000_00e8u32.to_le_bytes());
        first.extend_from_slice(&[0x49, 0xfe, 0x01, 0x02]);

        let mut second = first.clone();
        second[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
            .copy_from_slice(&0x0000_00e3u32.to_le_bytes());

        assert_eq!(
            live_object_fixture_dump_signature(&first, "live-object-declared-length-repair-len233"),
            live_object_fixture_dump_signature(
                &second,
                "live-object-declared-length-repair-len233"
            )
        );

        second[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES] ^= 0x01;
        assert_ne!(
            live_object_fixture_dump_signature(&first, "live-object-declared-length-repair-len233"),
            live_object_fixture_dump_signature(
                &second,
                "live-object-declared-length-repair-len233"
            )
        );
    }

    #[test]
    fn non_declared_repair_dump_signature_keeps_declared_slot() {
        let mut first = vec![
            HIGH_LEVEL_ENVELOPE,
            GAME_OBJECT_UPDATE_MAJOR,
            LIVE_OBJECT_MINOR,
        ];
        first.extend_from_slice(&0x0000_00e8u32.to_le_bytes());
        first.extend_from_slice(&[0x49, 0xfe, 0x01, 0x02]);

        let mut second = first.clone();
        second[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
            .copy_from_slice(&0x0000_00e3u32.to_le_bytes());

        assert_ne!(
            live_object_fixture_dump_signature(&first, "live-object-combined-records-len233"),
            live_object_fixture_dump_signature(&second, "live-object-combined-records-len233")
        );
    }
}

#[derive(Debug, Clone, Default)]
pub struct LiveObjectUpdateRewriteSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub old_live_bytes_length: usize,
    pub new_live_bytes_length: usize,
    pub old_fragment_bytes: usize,
    pub new_fragment_bytes: usize,
    pub records_examined: u32,
    pub update_records_examined: u32,
    pub update_records_rewritten: u32,
    pub masks_translated: u32,
    pub bytes_inserted: u32,
    pub bytes_removed: u32,
    pub bits_inserted: u32,
    pub bits_removed: u32,
    pub fragment_bits_trimmed: u32,
    pub creature_visual_transform_update_records: u32,
    pub live_gui_missing_add_opcodes_repaired: u32,
    pub live_object_missing_appearance_opcodes_repaired: u32,
    pub live_object_missing_update_opcodes_repaired: u32,
    pub interleaved_fragment_spans_promoted: u32,
    pub interleaved_fragment_bytes_promoted: u32,
    pub interleaved_fragment_bits_promoted: u32,
}

#[derive(Debug, Clone, Default)]
pub struct LiveObjectUpdateClaimSummary {
    pub declared: usize,
    pub live_bytes_length: usize,
    pub fragment_bytes: usize,
    pub records_examined: u32,
    pub inventory_records: u32,
    pub inventory_fragment_bits: u32,
    pub live_gui_read_buffer_records: u32,
    pub live_gui_item_create_records: u32,
    pub live_gui_fragment_bits: u32,
    pub materialized_item_object_ids: Vec<u32>,
    pub world_status_records: u32,
    pub read_buffer_only_records: u32,
    pub add_records: u32,
    pub update_records: u32,
    pub creature_appearance_records: u32,
    pub creature_visual_transform_update_records: u32,
    pub creature_update_records: u32,
    pub delete_records: u32,
    pub mentions: Vec<LiveObjectRecordMention>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveObjectRecordPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveObjectRecordOrientation {
    pub scalar_tenths_degrees: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveObjectRecordBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub max_z: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveObjectRecordMention {
    pub opcode: u8,
    pub object_type: u8,
    pub object_id: u32,
    pub requires_materialized_object: bool,
    pub record_offset: usize,
    pub record_end: usize,
    pub fragment_bit_start: usize,
    pub fragment_bit_end: usize,
    pub name: Option<String>,
    pub position: Option<LiveObjectRecordPosition>,
    pub orientation: Option<LiveObjectRecordOrientation>,
    pub bounds: Option<LiveObjectRecordBounds>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveObjectLifecycleViolation {
    pub opcode: u8,
    pub object_type: u8,
    pub object_id: u32,
    pub reason: LiveObjectLifecycleViolationReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveObjectLifecycleViolationReason {
    UpdateBeforeMaterializedAdd,
}

#[derive(Debug, Clone, Default)]
pub struct LiveObjectLifecycleRewriteSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub removed_update_records: u32,
    pub diamond_missing_object_update_records: u32,
    pub diamond_missing_object_appearance_records: u32,
    pub ee_sentinel_inventory_owner_records: u32,
    pub removed_bytes: u32,
    pub removed_fragment_bits: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveObjectLifecycleRewriteKind {
    DiamondMissingObjectUpdateNoop,
    DiamondMissingObjectAppearanceNoop,
    EeSentinelInventoryOwnerAbort,
}

impl LiveObjectLifecycleRewriteKind {
    fn as_str(self) -> &'static str {
        match self {
            LiveObjectLifecycleRewriteKind::DiamondMissingObjectUpdateNoop => {
                "DiamondMissingObjectUpdateNoop"
            }
            LiveObjectLifecycleRewriteKind::DiamondMissingObjectAppearanceNoop => {
                "DiamondMissingObjectAppearanceNoop"
            }
            LiveObjectLifecycleRewriteKind::EeSentinelInventoryOwnerAbort => {
                "EeSentinelInventoryOwnerAbort"
            }
        }
    }
}

#[derive(Debug, Clone)]
struct LiveObjectLifecycleRemoval {
    mention: LiveObjectRecordMention,
    kind: LiveObjectLifecycleRewriteKind,
}

pub use object_ids::{
    LiveObjectExternalObjectIdCanonicalizeSummary,
    canonicalize_compact_external_object_ids_payload_for_ee,
    canonicalize_player_session_creature_ids_payload_for_ee,
};

#[derive(Debug, Clone, Default)]
pub struct LiveObjectAddNameBitRewriteSummary {
    pub records_examined: u32,
    pub add_records_repaired: u32,
    pub bits_inserted: u32,
    pub bits_removed: u32,
    pub old_fragment_bytes: u32,
    pub new_fragment_bytes: u32,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<LiveObjectUpdateClaimSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let declared = usize::try_from(read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }
    let fragment = &payload[declared..];
    let fragment_bits = bits::decode_msb_valid_bits(fragment, CNW_FRAGMENT_HEADER_BITS)?;

    let live_bytes = &payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared];
    if live_bytes.is_empty() {
        // EE's CNWMessage::MessageMoreDataToRead checks both the live-object
        // read buffer and the remaining fragment cursor before dispatching
        // live-object submessages (see sub_14079BCE0 in the EE client
        // decompile).  A P/5/1 frame with no read-buffer bytes and only the
        // three CNW fragment header bits is therefore an exact semantic no-op,
        // not a raw passthrough.  Any extra fragment bits still fail here so a
        // malformed or partially-owned live-object stream remains quarantined.
        if fragment_bits.len() == CNW_FRAGMENT_HEADER_BITS {
            return Some(LiveObjectUpdateClaimSummary {
                declared,
                live_bytes_length: 0,
                fragment_bytes: fragment.len(),
                ..Default::default()
            });
        }
        return None;
    }

    let mut summary = LiveObjectUpdateClaimSummary {
        declared,
        live_bytes_length: live_bytes.len(),
        fragment_bytes: fragment.len(),
        ..LiveObjectUpdateClaimSummary::default()
    };
    let mut offset = 0usize;
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut pending_creature_appearance_update_proof: Option<(u32, usize)> = None;
    while offset + 2 <= live_bytes.len() {
        let proven_gui_record_end = if live_bytes.get(offset).copied() == Some(b'G') {
            gui::try_get_verified_ee_live_gui_record_end(
                live_bytes,
                offset,
                live_bytes.len(),
                &fragment_bits,
                bit_cursor,
            )
        } else {
            None
        };
        if live_bytes.get(offset).copied() == Some(b'G') && proven_gui_record_end.is_none() {
            trace_claim_reject(
                "unverified-live-gui-record",
                live_bytes,
                offset,
                offset.saturating_add(2).min(live_bytes.len()),
                bit_cursor,
            );
            return None;
        }
        if proven_gui_record_end.is_none()
            && !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset)
        {
            trace_claim_reject(
                "missing-live-object-submessage-boundary",
                live_bytes,
                offset,
                offset.saturating_add(2).min(live_bytes.len()),
                bit_cursor,
            );
            return None;
        }

        let mut record_end = proven_gui_record_end.unwrap_or_else(|| {
            boundary::find_next_legacy_live_object_sub_message_boundary_after(
                live_bytes,
                offset,
                live_bytes.len(),
            )
            .min(live_bytes.len())
        });
        let mut verified_creature_appearance_next_bit_cursor = None;
        if live_bytes.get(offset).copied() == Some(b'P')
            && live_bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
        {
            if let Some((verified_end, next_bit_cursor)) =
                appearance::try_get_verified_ee_creature_appearance_record_end_and_cursor(
                    live_bytes,
                    offset,
                    live_bytes.len(),
                    &fragment_bits,
                    bit_cursor,
                )
            {
                record_end = verified_end;
                verified_creature_appearance_next_bit_cursor = Some(next_bit_cursor);
            }
        }
        if live_bytes.get(offset).copied() == Some(b'G') {
            if let Some(verified_end) = gui::try_get_verified_ee_live_gui_record_end(
                live_bytes,
                offset,
                live_bytes.len(),
                &fragment_bits,
                bit_cursor,
            ) {
                record_end = verified_end;
            }
        }
        if live_bytes.get(offset).copied() == Some(b'U') {
            if let Some(verified_end) =
                effects::try_get_verified_ee_looping_visual_effect_update_record_end(
                    live_bytes,
                    offset,
                    live_bytes.len(),
                )
            {
                record_end = verified_end;
            }
        }
        if record_end <= offset {
            trace_claim_reject(
                "non-advancing-live-object-boundary",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            return None;
        }
        if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
            eprintln!(
                "live-object claim boundary: offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} preview={:02X?}",
                live_bytes.get(offset).copied().unwrap_or_default(),
                live_bytes.get(offset + 1).copied().unwrap_or_default(),
                live_bytes
                    .get(
                        offset
                            ..record_end
                                .min(offset.saturating_add(96))
                                .min(live_bytes.len())
                    )
                    .unwrap_or(&[])
            );
        }

        summary.records_examined = summary.records_examined.saturating_add(1);
        let record_bit_cursor = bit_cursor;
        if let Some(next_bit_cursor) = verified_creature_appearance_next_bit_cursor {
            bit_cursor = next_bit_cursor;
            summary.creature_appearance_records =
                summary.creature_appearance_records.saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            pending_creature_appearance_update_proof =
                live_object_record_object_id(live_bytes, offset, record_end)
                    .map(|object_id| (object_id, bit_cursor));
            trace_claim_accept(
                "creature-appearance",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if let Some(inventory_claim) = inventory::advance_verified_inventory_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.inventory_records = summary.inventory_records.saturating_add(1);
            summary.inventory_fragment_bits = summary
                .inventory_fragment_bits
                .saturating_add(u32::try_from(inventory_claim.fragment_bits).unwrap_or(u32::MAX));
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            trace_claim_accept("inventory", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        if let Some(gui_claim) = gui::advance_verified_live_gui_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            if gui_claim.item_create {
                summary.live_gui_item_create_records =
                    summary.live_gui_item_create_records.saturating_add(1);
            } else {
                summary.live_gui_read_buffer_records =
                    summary.live_gui_read_buffer_records.saturating_add(1);
            }
            summary.live_gui_fragment_bits = summary
                .live_gui_fragment_bits
                .saturating_add(u32::try_from(gui_claim.fragment_bits).unwrap_or(u32::MAX));
            summary.materialized_item_object_ids.extend(
                gui::verified_item_materialization_object_ids(live_bytes, offset, record_end),
            );
            trace_claim_accept("live-gui", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        if world_status::is_verified_work_remaining_record(live_bytes, offset, record_end) {
            summary.world_status_records = summary.world_status_records.saturating_add(1);
            trace_claim_accept(
                "world-status-work-remaining",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if add::advance_verified_add_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.add_records = summary.add_records.saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            trace_claim_accept("add", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        let record_probe = record::advance_verified_update_record_for_ee(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        );
        if record_probe {
            summary.update_records = summary.update_records.saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            trace_claim_accept("update", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }
        if appearance::advance_verified_ee_creature_appearance_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.creature_appearance_records =
                summary.creature_appearance_records.saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            pending_creature_appearance_update_proof =
                live_object_record_object_id(live_bytes, offset, record_end)
                    .map(|object_id| (object_id, bit_cursor));
            trace_claim_accept(
                "creature-appearance",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if creature::advance_verified_noop_creature_appearance_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            summary.creature_update_records = summary.creature_update_records.saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            pending_creature_appearance_update_proof =
                live_object_record_object_id(live_bytes, offset, record_end)
                    .map(|object_id| (object_id, bit_cursor));
            trace_claim_accept(
                "noop-creature-appearance",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if appearance::is_verified_ee_creature_visual_transform_update_record(
            live_bytes, offset, record_end,
        ) {
            summary.creature_visual_transform_update_records = summary
                .creature_visual_transform_update_records
                .saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            trace_claim_accept(
                "creature-visual-transform",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        let mut creature_probe_bit_cursor = bit_cursor;
        let creature_probe = creature::advance_verified_noop_creature_update_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut creature_probe_bit_cursor,
        );
        if creature_probe {
            bit_cursor = creature_probe_bit_cursor;
            summary.creature_update_records = summary.creature_update_records.saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            if let (Some((pending_object_id, _)), Some(update_object_id)) = (
                pending_creature_appearance_update_proof,
                live_object_record_object_id(live_bytes, offset, record_end),
            ) {
                if pending_object_id == update_object_id {
                    pending_creature_appearance_update_proof = None;
                }
            }
            trace_claim_accept(
                "creature-update",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
            );
            offset = record_end;
            continue;
        }
        if let (Some((pending_object_id, _pending_bit_cursor)), Some(update_object_id)) = (
            pending_creature_appearance_update_proof,
            live_object_record_object_id(live_bytes, offset, record_end),
        ) {
            if pending_object_id == update_object_id {
                // Strict validation must model what the EE client will read at
                // this submessage boundary. Earlier versions allowed a
                // `U/5 0x3967` record to be treated as "already consumed" by a
                // preceding creature appearance fence proof. That proved an
                // internal proxy cursor relationship, not the standalone
                // client reader for the `U` record that is still present on the
                // wire, and harness captures showed EE logging
                // `Unknown Update sub-message` after such accepts. Keep the
                // pending proof only as context; the focused creature-update
                // simulator above must advance this record exactly.
                pending_creature_appearance_update_proof = None;
            }
        }
        if let Some(delete_bits) =
            cursor::legacy_live_delete_fragment_bit_count(live_bytes, offset, record_end)
        {
            if delete_bits > fragment_bits.len().saturating_sub(bit_cursor) {
                return None;
            }
            bit_cursor = bit_cursor.saturating_add(delete_bits);
            summary.delete_records = summary.delete_records.saturating_add(1);
            push_verified_record_mention(
                &mut summary,
                live_bytes,
                offset,
                record_end,
                &fragment_bits,
                record_bit_cursor,
                bit_cursor,
            );
            trace_claim_accept("delete", live_bytes, offset, record_end, bit_cursor);
            offset = record_end;
            continue;
        }

        trace_claim_reject(
            "no-exact-record-validator-accepted-boundary",
            live_bytes,
            offset,
            record_end,
            bit_cursor,
        );
        return None;
    }

    if offset != live_bytes.len() || summary.records_examined == 0 {
        trace_claim_reject(
            "live-object-final-cursor-mismatch",
            live_bytes,
            offset.min(live_bytes.len()),
            live_bytes.len(),
            bit_cursor,
        );
        return None;
    }

    if bit_cursor != fragment_bits.len() {
        trace_claim_reject(
            "live-object-fragment-cursor-mismatch",
            live_bytes,
            live_bytes.len(),
            live_bytes.len(),
            bit_cursor,
        );
        return None;
    }

    Some(summary)
}

pub fn rewrite_add_name_fragment_bits_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectAddNameBitRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    let declared = usize::try_from(read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }

    let live_bytes = &payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared];
    let mut fragment_bits =
        bits::decode_msb_valid_bits(&payload[declared..], CNW_FRAGMENT_HEADER_BITS)?;
    let old_fragment_bytes = payload.len().saturating_sub(declared);
    let mut summary = LiveObjectAddNameBitRewriteSummary {
        old_fragment_bytes: u32::try_from(old_fragment_bytes).unwrap_or(u32::MAX),
        ..LiveObjectAddNameBitRewriteSummary::default()
    };
    let mut offset = 0usize;
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;

    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            trace_add_name_rewrite_reject(
                "missing-live-object-submessage-boundary",
                live_bytes,
                offset,
                offset.saturating_add(2).min(live_bytes.len()),
                bit_cursor,
                &fragment_bits,
            );
            return None;
        }
        let record_end = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        )
        .min(live_bytes.len());
        let record_end = if live_bytes.get(offset).copied() == Some(b'P')
            && live_bytes.get(offset + 1).copied() == Some(CREATURE_OBJECT_TYPE)
        {
            appearance::try_get_verified_ee_creature_appearance_record_end(
                live_bytes,
                offset,
                live_bytes.len(),
                &fragment_bits,
                bit_cursor,
            )
            .unwrap_or(record_end)
        } else {
            record_end
        };
        if record_end <= offset {
            trace_add_name_rewrite_reject(
                "non-advancing-live-object-boundary",
                live_bytes,
                offset,
                record_end,
                bit_cursor,
                &fragment_bits,
            );
            return None;
        }
        summary.records_examined = summary.records_examined.saturating_add(1);

        if let Some(_inventory_claim) = inventory::advance_verified_inventory_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            offset = record_end;
            continue;
        }
        if let Some(_gui_claim) = gui::advance_verified_live_gui_record(
            live_bytes,
            offset,
            record_end,
            &fragment_bits,
            &mut bit_cursor,
        ) {
            offset = record_end;
            continue;
        }
        if world_status::is_verified_work_remaining_record(live_bytes, offset, record_end) {
            offset = record_end;
            continue;
        }

        match (live_bytes[offset], live_bytes[offset + 1]) {
            (b'A', DOOR_OBJECT_TYPE) => {
                if repair_inline_door_add_name_bit(
                    live_bytes,
                    offset,
                    record_end,
                    &mut fragment_bits,
                    &mut bit_cursor,
                    &mut summary,
                )
                .is_none()
                {
                    trace_add_name_rewrite_reject(
                        "door-add-name-bit-repair-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'A', PLACEABLE_OBJECT_TYPE) => {
                if repair_inline_placeable_add_name_bit(
                    live_bytes,
                    offset,
                    record_end,
                    &mut fragment_bits,
                    &mut bit_cursor,
                    &mut summary,
                )
                .is_none()
                {
                    trace_add_name_rewrite_reject(
                        "placeable-add-name-bit-repair-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'A', _) => {
                if !add::advance_verified_add_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    if let Some(repair) = appearance::repair_verified_ee_item_add_name_fragment_bits(
                        live_bytes,
                        offset,
                        record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    ) {
                        bit_cursor = repair.next_bit_cursor;
                        summary.add_records_repaired =
                            summary.add_records_repaired.saturating_add(1);
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(repair.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bits_removed = summary
                            .bits_removed
                            .saturating_add(u32::try_from(repair.bits_removed).unwrap_or(u32::MAX));
                        offset = record_end;
                        continue;
                    }
                    trace_add_name_rewrite_reject(
                        "add-record-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'U', PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) => {
                if !record::advance_verified_update_record_for_ee(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    if summary.add_records_repaired != 0 {
                        // This helper owns add-record name-mode bit repair, not
                        // door/placeable update-mask translation. Once a proven
                        // add repair has been made, stop before a still-legacy
                        // update record and let the following update-family pass
                        // consume/translate it with its own exact validator.
                        break;
                    }
                    trace_add_name_rewrite_reject(
                        "update-record-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'P', CREATURE_OBJECT_TYPE) => {
                if !appearance::advance_verified_ee_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) && !creature::advance_verified_noop_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    if summary.add_records_repaired != 0 {
                        // This helper owns only add-record name-mode bit repair. Once a
                        // repair has been proven, do not discard it because a later
                        // unrelated family needs the full live-object validator. The
                        // caller still immediately runs the exact final claim; if this
                        // P/5 record is not translated/validated, the packet remains
                        // quarantined rather than leaking.
                        break;
                    }
                    trace_add_name_rewrite_reject(
                        "creature-appearance-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'U', CREATURE_OBJECT_TYPE) => {
                if appearance::advance_verified_ee_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) || creature::advance_verified_noop_creature_appearance_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) || appearance::is_verified_ee_creature_visual_transform_update_record(
                    live_bytes, offset, record_end,
                ) || creature::advance_verified_noop_creature_update_record(
                    live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    // This pass only owns add-record name-mode bit repair. Creature records
                    // are accepted here only after the same exact validators used by the
                    // final live-object claim prove their cursor shape.
                } else {
                    if summary.add_records_repaired != 0 {
                        // See the P/5 case above: once this add-name-only helper has
                        // made a proven repair, later unrelated families are validated
                        // by the mandatory final live-object claim instead of causing
                        // the repair to be discarded.
                        break;
                    }
                    trace_add_name_rewrite_reject(
                        "creature-update-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
            }
            (b'W', _)
                if world_status::is_verified_work_remaining_record(
                    live_bytes, offset, record_end,
                ) => {}
            (b'D', object_type) => {
                let delete_bits =
                    cursor::legacy_live_delete_fragment_bit_count(live_bytes, offset, record_end)?;
                if !matches!(object_type, 0x05 | 0x06 | 0x07 | 0x09 | 0x0A)
                    || fragment_bits.len().saturating_sub(bit_cursor) < delete_bits
                {
                    trace_add_name_rewrite_reject(
                        "delete-record-cursor-rejected",
                        live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                        &fragment_bits,
                    );
                    return None;
                }
                bit_cursor = bit_cursor.saturating_add(delete_bits);
            }
            _ => {
                trace_add_name_rewrite_reject(
                    "unsupported-live-object-record",
                    live_bytes,
                    offset,
                    record_end,
                    bit_cursor,
                    &fragment_bits,
                );
                return None;
            }
        }
        offset = record_end;
    }

    if summary.add_records_repaired == 0 {
        trace_add_name_rewrite_reject(
            "no-add-name-bits-needed-repair",
            live_bytes,
            live_bytes.len(),
            live_bytes.len(),
            bit_cursor,
            &fragment_bits,
        );
        return None;
    }

    let fragment_bytes = bits::pack_msb_valid_bits(fragment_bits, CNW_FRAGMENT_HEADER_BITS);
    summary.new_fragment_bytes = u32::try_from(fragment_bytes.len()).unwrap_or(u32::MAX);
    payload.truncate(declared);
    payload.extend_from_slice(&fragment_bytes);
    Some(summary)
}

fn trace_add_name_rewrite_reject(
    reason: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
    fragment_bits: &[bool],
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    let preview_limit = if reason == "missing-live-object-submessage-boundary" {
        offset.saturating_add(40)
    } else {
        record_end
    };
    let preview_end = preview_limit
        .min(live_bytes.len())
        .min(offset.saturating_add(40));
    let preview = live_bytes.get(offset..preview_end).unwrap_or(&[]);
    eprintln!(
        "live-object add-name repair rejected: reason={reason} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} next_bits={:?} preview={:02X?}",
        live_bytes.get(offset).copied().unwrap_or_default(),
        live_bytes.get(offset + 1).copied().unwrap_or_default(),
        fragment_bits
            .get(bit_cursor..bit_cursor.saturating_add(12).min(fragment_bits.len()))
            .unwrap_or(&[]),
        preview
    );
}

fn repair_inline_door_add_name_bit(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    summary: &mut LiveObjectAddNameBitRewriteSummary,
) -> Option<()> {
    let first_dword = read_u32_le(live_bytes, offset + 6)?;
    let visual_offset = offset + 2 + if first_dword == 0 { 12 } else { 8 };
    if !creature::has_ee_identity_visual_transform_map_at(live_bytes, visual_offset, record_end) {
        return None;
    }
    let name_offset = visual_offset + 40;
    let inline_end = locstring::inline_cexo_string_end(live_bytes, name_offset)?;
    if inline_end > name_offset + CNW_LENGTH_BYTES
        && fragment_bits.get(*bit_cursor).copied().unwrap_or(false)
    {
        if fragment_bits.len().saturating_sub(*bit_cursor + 1) < 1 {
            return None;
        }
        // EE door add (`sub_140796DD0`) has a decompile-owned direct-name
        // reader path: outer BOOL false, then `ReadCExoString(0x20)`, then the
        // fixed post-name door tail bits.  The outer=true branch enters
        // `ReadCExoLocStringClient` and consumes an extra inner BOOL before it
        // can reach an inline string.  Legacy/HG captures sometimes present a
        // direct CExoString with that legacy helper bit still present; normalize
        // it to the exact EE direct branch instead of letting a final exact
        // claim fail or, worse, consume one extra bit and shift the following
        // door/placeable records.
        fragment_bits[*bit_cursor] = false;
        fragment_bits.remove(*bit_cursor + 1);
        summary.add_records_repaired = summary.add_records_repaired.saturating_add(1);
        summary.bits_removed = summary.bits_removed.saturating_add(1);
        *bit_cursor = bit_cursor.saturating_add(6);
    } else {
        if fragment_bits.get(*bit_cursor).copied().unwrap_or(true) {
            return None;
        }
        *bit_cursor = bit_cursor.saturating_add(6);
    }
    Some(())
}

fn repair_inline_placeable_add_name_bit(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: &mut usize,
    summary: &mut LiveObjectAddNameBitRewriteSummary,
) -> Option<()> {
    let name_offset = offset + 6;
    let inline_end = locstring::inline_cexo_string_end(live_bytes, name_offset)?;
    let tail_end = inline_end.checked_add(1 + 2 + 2)?;
    if tail_end > record_end {
        return None;
    }
    let optional_object_bytes_present =
        if creature::has_ee_identity_visual_transform_map_at(live_bytes, tail_end, record_end) {
            false
        } else {
            let optional_end = tail_end.checked_add(4)?;
            if optional_end <= record_end
                && creature::has_ee_identity_visual_transform_map_at(
                    live_bytes,
                    optional_end,
                    record_end,
                )
            {
                true
            } else {
                return None;
            }
        };
    if fragment_bits.len().saturating_sub(*bit_cursor) < 11 {
        return None;
    }
    if inline_end > name_offset + CNW_LENGTH_BYTES
        && fragment_bits.get(*bit_cursor).copied().unwrap_or(false)
    {
        if optional_object_bytes_present {
            // EE `sub_1407A7800` selects direct inline CExoString placeable
            // names with the outer locstring BOOL set false. The optional-
            // object branch gives this repair an exact byte-side proof:
            // without it, preserve the older locstring-inline cursor path
            // already covered by existing HG fixtures.
            fragment_bits[*bit_cursor] = false;
            fragment_bits[*bit_cursor + 2] = true;
            fragment_bits[*bit_cursor + 10] = false;
            summary.add_records_repaired = summary.add_records_repaired.saturating_add(1);
            *bit_cursor = bit_cursor.saturating_add(11);
        } else if fragment_bits[*bit_cursor + 1] {
            fragment_bits[*bit_cursor] = false;
            summary.add_records_repaired = summary.add_records_repaired.saturating_add(1);
            *bit_cursor = bit_cursor.saturating_add(11);
        } else {
            *bit_cursor = bit_cursor.saturating_add(12);
        }
    } else {
        if inline_end > name_offset + CNW_LENGTH_BYTES
            && fragment_bits.get(*bit_cursor).copied().unwrap_or(true)
        {
            return None;
        }
        if optional_object_bytes_present {
            fragment_bits[*bit_cursor + 2] = true;
            fragment_bits[*bit_cursor + 10] = false;
        }
        *bit_cursor = bit_cursor.saturating_add(11);
    }
    Some(())
}

pub(crate) fn advance_verified_door_placeable_update_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    record::advance_verified_update_record_for_ee(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn advance_verified_inventory_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    inventory::advance_verified_inventory_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
    .is_some()
}

pub(crate) fn advance_verified_add_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    add::advance_verified_add_record(live_bytes, offset, record_end, fragment_bits, bit_cursor)
}

fn repair_missing_creature_appearance_opcode_after_add_for_ee(
    live_bytes: &mut [u8],
    offset: usize,
    scan_end: usize,
    expected_object_id: u32,
) -> Option<usize> {
    let scan_end = scan_end.min(live_bytes.len());
    if offset + 8 > scan_end
        || live_bytes.get(offset).copied()? != 0
        || live_bytes.get(offset + 1).copied()? != CREATURE_OBJECT_TYPE
        || read_u32_le(live_bytes, offset + 2)? != expected_object_id
        || read_u16_le(live_bytes, offset + 6)? != 0xFFFF
    {
        return None;
    }

    let mut trial = live_bytes.to_vec();
    trial[offset] = b'P';
    let record_end =
        appearance::try_get_legacy_creature_appearance_record_end(&trial, offset, scan_end)?;
    if record_end <= offset || record_end > scan_end {
        return None;
    }

    // Diamond can emit this compact all-fields appearance body immediately
    // after the matching `A/5` record with the opcode byte zeroed. The
    // preceding verified add proves object identity; the legacy appearance
    // parser owns the body, and the later appearance writer/final validator
    // still have to prove the EE-facing packet before emission.
    live_bytes[offset] = b'P';
    Some(record_end)
}

pub(crate) fn try_get_verified_door_placeable_add_record_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    boundary::try_get_ee_door_placeable_add_record_end_for_transport(live_bytes, offset, scan_end)
}

pub(crate) fn try_get_legacy_placeable_short_name_add_record_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    boundary::try_get_legacy_placeable_short_name_add_record_end_for_transport(
        live_bytes, offset, scan_end,
    )
}

pub(crate) fn try_get_legacy_placeable_bare_name_add_record_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    boundary::try_get_legacy_placeable_bare_name_add_record_end_for_transport(
        live_bytes, offset, scan_end,
    )
}

pub(crate) fn try_get_verified_creature_update_record_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    boundary::try_get_ee_creature_update_record_end_for_transport(live_bytes, offset, scan_end)
}

pub(crate) fn try_get_verified_door_placeable_update_record_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    boundary::try_get_ee_door_placeable_update_record_end_for_transport(
        live_bytes, offset, scan_end,
    )
}

pub(crate) fn legacy_creature_appearance_record_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    // Transport declared-length repair needs the same decompile-owned `P/5`
    // creature appearance byte boundary as the semantic live-object pass.
    // Diamond `sub_448E30` consumes the full appearance body and embedded
    // visible-equipment subobjects before the live-object dispatcher resumes;
    // without this proof, the transport repair scanner can mistake bytes inside
    // the appearance/equipment body for top-level `A/U/P` records.
    appearance::try_get_legacy_creature_appearance_record_end(live_bytes, offset, scan_end)
}

pub(crate) fn legacy_inventory_fragment_bit_count_for_transport(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<usize> {
    inventory::try_get_legacy_live_inventory_fragment_bit_count(live_bytes, offset, record_end)
        .or_else(|| {
            let claim = inventory::try_get_legacy_live_inventory_prefix_claim(
                live_bytes, offset, record_end,
            )?;
            (claim.interleaved_fragment_tail_allowed
                && claim.read_end > offset
                && claim.read_end < record_end)
                .then_some(claim.fragment_bits)
        })
}

pub(crate) fn legacy_inventory_prefix_read_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    search_end: usize,
) -> Option<usize> {
    inventory::try_get_legacy_live_inventory_prefix_claim(live_bytes, offset, search_end)
        .and_then(|claim| (!claim.interleaved_fragment_tail_allowed).then_some(claim.read_end))
}

pub(crate) fn legacy_live_gui_record_end_for_transport(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    gui::try_get_legacy_live_gui_record_end_with_fragment_proof(
        live_bytes,
        offset,
        scan_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn advance_legacy_live_gui_fragment_cursor_for_transport(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    gui::advance_legacy_live_gui_record_for_transport(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
    .is_some()
}

pub(crate) fn legacy_live_gui_zero_fragment_storage_span_for_transport(
    live_bytes: &[u8],
    span_start: usize,
    span_end: usize,
) -> bool {
    gui::is_zero_fragment_storage_span_before_legacy_live_gui_prefix(
        live_bytes, span_start, span_end,
    )
}

pub(crate) fn advance_verified_creature_appearance_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    let original_bit_cursor = *bit_cursor;
    if appearance::advance_verified_ee_creature_appearance_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    ) {
        return true;
    }

    *bit_cursor = original_bit_cursor;
    creature::advance_verified_noop_creature_appearance_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn try_get_verified_creature_appearance_record_end_for_ee(
    live_bytes: &[u8],
    offset: usize,
    scan_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
) -> Option<usize> {
    appearance::try_get_verified_ee_creature_appearance_record_end(
        live_bytes,
        offset,
        scan_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn advance_verified_creature_update_fragment_cursor_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: &mut usize,
) -> bool {
    if appearance::is_verified_ee_creature_visual_transform_update_record(
        live_bytes, offset, record_end,
    ) {
        return true;
    }
    if effects::is_verified_ee_looping_visual_effect_update_record(live_bytes, offset, record_end) {
        return true;
    }

    creature::advance_verified_noop_creature_update_record(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
    )
}

pub(crate) fn trigger_add_record_end_for_ee(
    bytes: &[u8],
    offset: usize,
    scan_end: usize,
) -> Option<usize> {
    trigger::try_get_trigger_add_record_end(bytes, offset, scan_end)
}

pub(crate) fn trigger_add_min_record_bytes_for_ee() -> usize {
    trigger::TRIGGER_ADD_MIN_RECORD_BYTES
}

fn live_object_record_object_id(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<u32> {
    if offset >= record_end || record_end > live_bytes.len() {
        return None;
    }
    if live_bytes.get(offset).copied() == Some(b'I') {
        if offset + 5 > record_end {
            return None;
        }
        return Some(u32::from_le_bytes(
            live_bytes.get(offset + 1..offset + 5)?.try_into().ok()?,
        ));
    }
    if offset + 6 > record_end {
        return None;
    }
    Some(u32::from_le_bytes(
        live_bytes.get(offset + 2..offset + 6)?.try_into().ok()?,
    ))
}

fn verified_record_mention(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    next_bit_cursor: usize,
) -> Option<LiveObjectRecordMention> {
    if offset + 6 > record_end || record_end > live_bytes.len() {
        return None;
    }
    let opcode = *live_bytes.get(offset)?;
    let object_id = live_object_record_object_id(live_bytes, offset, record_end)?;
    let object_type = if opcode == b'I' {
        0
    } else {
        *live_bytes.get(offset + 1)?
    };
    let requires_materialized_object = if opcode == b'I' {
        inventory_record_requires_materialized_owner_for_ee(
            live_bytes, offset, record_end, object_id,
        )
    } else if opcode == b'P' && object_type == CREATURE_OBJECT_TYPE {
        // EE `P/5` routes through `sub_1407B25C0` -> `sub_14077FE10`.
        // That reader resolves the creature, logs `pCreature` if it is absent,
        // but still consumes the appearance body with all writes guarded by the
        // resolved pointer. This differs from generic `U` records, whose EE
        // object lookup can abort before consuming the mask body. Treat exact
        // appearance records as cursor-safe without requiring a prior add.
        false
    } else {
        update_requires_materialized_object(object_type)
    };
    Some(LiveObjectRecordMention {
        opcode,
        object_type,
        object_id,
        requires_materialized_object,
        record_offset: offset,
        record_end,
        fragment_bit_start: bit_cursor,
        fragment_bit_end: next_bit_cursor,
        name: verified_record_name(live_bytes, offset, record_end, opcode, object_type),
        position: verified_record_position(
            live_bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
            opcode,
        ),
        orientation: verified_record_orientation(
            live_bytes,
            offset,
            record_end,
            fragment_bits,
            bit_cursor,
            opcode,
            object_type,
        ),
        bounds: verified_record_bounds(live_bytes, offset, record_end, opcode, object_type),
    })
}

fn push_verified_record_mention(
    summary: &mut LiveObjectUpdateClaimSummary,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    next_bit_cursor: usize,
) {
    if let Some(mention) = verified_record_mention(
        live_bytes,
        offset,
        record_end,
        fragment_bits,
        bit_cursor,
        next_bit_cursor,
    ) {
        summary.mentions.push(mention);
    }
}

pub fn claim_payload_if_verified_with_lifecycle<F>(
    payload: &[u8],
    mut is_already_materialized: F,
) -> Option<LiveObjectUpdateClaimSummary>
where
    F: FnMut(u8, u32) -> bool,
{
    let claim = claim_payload_if_verified(payload)?;
    if let Some(violation) = first_lifecycle_violation(&claim, |object_type, object_id| {
        is_already_materialized(object_type, object_id)
    }) {
        tracing::info!(
            opcode = %char::from(violation.opcode),
            object_type = violation.object_type,
            object_id = violation.object_id,
            reason = ?violation.reason,
            "live-object exact claim rejected: EE reader would update an object that is not materialized"
        );
        return None;
    }
    Some(claim)
}

pub fn first_lifecycle_violation<F>(
    claim: &LiveObjectUpdateClaimSummary,
    mut is_already_materialized: F,
) -> Option<LiveObjectLifecycleViolation>
where
    F: FnMut(u8, u32) -> bool,
{
    let mut materialized_in_payload = BTreeSet::<(u8, u32)>::new();

    for mention in &claim.mentions {
        let key = (mention.object_type, mention.object_id);
        match mention.opcode {
            b'A' => {
                materialized_in_payload.insert(key);
            }
            b'D' => {
                materialized_in_payload.remove(&key);
            }
            b'U' | b'P' | b'I' | b'G' | b'W' => {
                if mention.requires_materialized_object
                    && !materialized_in_payload.contains(&key)
                    && !is_already_materialized(mention.object_type, mention.object_id)
                {
                    return Some(LiveObjectLifecycleViolation {
                        opcode: mention.opcode,
                        object_type: mention.object_type,
                        object_id: mention.object_id,
                        reason: LiveObjectLifecycleViolationReason::UpdateBeforeMaterializedAdd,
                    });
                }
            }
            _ => {}
        }
    }

    None
}

fn update_requires_materialized_object(object_type: u8) -> bool {
    matches!(
        object_type,
        CREATURE_OBJECT_TYPE | TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE
    )
}

fn inventory_record_requires_materialized_owner_for_ee(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    object_id: u32,
) -> bool {
    if offset + 7 > record_end || record_end > live_bytes.len() {
        return false;
    }
    let Some(mask) = read_u16_le(live_bytes, offset + 5) else {
        return false;
    };

    // EE `CNWSMessage::HandleServerToPlayerInventory_Update` (`sub_1407B4F70`)
    // checks the resolved object pointer before the `0x2000` Feature-25 branch:
    // `test rsi, rsi; jz loc_1407B82D3`, then only reads the DWORD/list body
    // when the object exists. Diamond `sub_455940` has the same pointer guard
    // for the corresponding branch. A legacy sentinel owner such as
    // `0xFFFF_FFFE` therefore cannot be forwarded to EE: the EE reader exits
    // before consuming the read-buffer bytes, and the next byte is interpreted
    // as a bogus live-object submessage. Drop only this decompile-proven
    // sentinel-owner case; ordinary object ids need the object registry to
    // prove presence before we make broader inventory lifecycle decisions.
    (mask & 0x2000) != 0 && matches!(object_id, 0xFFFF_FFFE | 0xFFFF_FFFD)
}

fn cached_external_materialized<F>(
    external_materialized: &mut BTreeMap<(u8, u32), bool>,
    is_already_materialized: &mut F,
    object_type: u8,
    object_id: u32,
) -> bool
where
    F: FnMut(u8, u32) -> bool,
{
    let key = (object_type, object_id);
    if let Some(materialized) = external_materialized.get(&key) {
        return *materialized;
    }
    let materialized = is_already_materialized(object_type, object_id);
    external_materialized.insert(key, materialized);
    materialized
}

pub fn remove_unmaterialized_update_records_payload_if_possible<F>(
    payload: &mut Vec<u8>,
    mut is_already_materialized: F,
) -> Option<LiveObjectLifecycleRewriteSummary>
where
    F: FnMut(u8, u32) -> bool,
{
    let claim = claim_payload_if_verified(payload)?;
    let mut materialized_in_payload = BTreeSet::<(u8, u32)>::new();
    let mut external_materialized = BTreeMap::<(u8, u32), bool>::new();
    let mut missing_removable_required_record_offsets = BTreeSet::<(u8, u32, usize)>::new();
    let mut removals = Vec::<LiveObjectLifecycleRemoval>::new();

    for mention in &claim.mentions {
        let key = (mention.object_type, mention.object_id);
        match mention.opcode {
            b'A' => {
                materialized_in_payload.insert(key);
            }
            b'D' => {
                materialized_in_payload.remove(&key);
            }
            b'U' | b'P' | b'I' | b'G' | b'W' => {
                if mention.requires_materialized_object
                    && !materialized_in_payload.contains(&key)
                    && !cached_external_materialized(
                        &mut external_materialized,
                        &mut is_already_materialized,
                        mention.object_type,
                        mention.object_id,
                    )
                    && removable_unmaterialized_lifecycle_kind(mention).is_some()
                {
                    missing_removable_required_record_offsets.insert((
                        mention.object_type,
                        mention.object_id,
                        mention.record_offset,
                    ));
                }
            }
            _ => {}
        }
    }

    materialized_in_payload.clear();
    for mention in &claim.mentions {
        let key = (mention.object_type, mention.object_id);
        match mention.opcode {
            b'A' => {
                materialized_in_payload.insert(key);
            }
            b'D' => {
                materialized_in_payload.remove(&key);
            }
            b'P' if mention.object_type == CREATURE_OBJECT_TYPE
                && !materialized_in_payload.contains(&key)
                && !cached_external_materialized(
                    &mut external_materialized,
                    &mut is_already_materialized,
                    mention.object_type,
                    mention.object_id,
                )
                && missing_removable_required_record_offsets.contains(&(
                    mention.object_type,
                    mention.object_id,
                    mention.record_end,
                )) =>
            {
                // EE's creature appearance reader consumes `P/5` bodies even
                // when the target creature is absent, with all writes guarded
                // by the resolved pointer. If the immediately following
                // same-object `U/5` is a Diamond-only missing-object no-op that
                // EE would abort before consuming, removing only the `U/5`
                // record can invalidate the exact appearance tail proof. Drop
                // the adjacent absent-creature `P/5` no-op as part of the same
                // bounded lifecycle rewrite.
                removals.push(LiveObjectLifecycleRemoval {
                    mention: mention.clone(),
                    kind: LiveObjectLifecycleRewriteKind::DiamondMissingObjectAppearanceNoop,
                });
            }
            b'U' | b'P' | b'I' | b'G' | b'W' => {
                if mention.requires_materialized_object
                    && !materialized_in_payload.contains(&key)
                    && !cached_external_materialized(
                        &mut external_materialized,
                        &mut is_already_materialized,
                        mention.object_type,
                        mention.object_id,
                    )
                {
                    let Some(kind) = removable_unmaterialized_lifecycle_kind(mention) else {
                        tracing::warn!(
                            opcode = %char::from(mention.opcode),
                            object_type = mention.object_type,
                            object_id = mention.object_id,
                            "live-object lifecycle violation is not removable without a decompile-backed compatibility rule"
                        );
                        continue;
                    };
                    removals.push(LiveObjectLifecycleRemoval {
                        mention: mention.clone(),
                        kind,
                    });
                }
            }
            _ => {}
        }
    }

    if removals.is_empty() {
        return None;
    }

    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let declared = usize::try_from(old_declared).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }

    let mut live_bytes = payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared].to_vec();
    let mut fragment_bits =
        bits::decode_msb_valid_bits(&payload[declared..], CNW_FRAGMENT_HEADER_BITS)?;
    let mut summary = LiveObjectLifecycleRewriteSummary {
        old_declared,
        ..LiveObjectLifecycleRewriteSummary::default()
    };
    let mut applied_removals = Vec::new();

    removals.sort_by_key(|removal| removal.mention.record_offset);
    for removal in removals.into_iter().rev() {
        let mention = removal.mention;
        if mention.record_offset >= mention.record_end
            || mention.record_end > live_bytes.len()
            || mention.fragment_bit_start > mention.fragment_bit_end
            || mention.fragment_bit_end > fragment_bits.len()
        {
            return None;
        }
        let removed_bytes = mention.record_end.saturating_sub(mention.record_offset);
        let removed_bits = mention
            .fragment_bit_end
            .saturating_sub(mention.fragment_bit_start);
        live_bytes.drain(mention.record_offset..mention.record_end);
        bits::erase_msb_bits(&mut fragment_bits, mention.fragment_bit_start, removed_bits)?;
        summary.removed_update_records = summary.removed_update_records.saturating_add(1);
        match removal.kind {
            LiveObjectLifecycleRewriteKind::DiamondMissingObjectUpdateNoop => {
                summary.diamond_missing_object_update_records = summary
                    .diamond_missing_object_update_records
                    .saturating_add(1);
            }
            LiveObjectLifecycleRewriteKind::DiamondMissingObjectAppearanceNoop => {
                summary.diamond_missing_object_appearance_records = summary
                    .diamond_missing_object_appearance_records
                    .saturating_add(1);
            }
            LiveObjectLifecycleRewriteKind::EeSentinelInventoryOwnerAbort => {
                summary.ee_sentinel_inventory_owner_records = summary
                    .ee_sentinel_inventory_owner_records
                    .saturating_add(1);
            }
        }
        summary.removed_bytes = summary
            .removed_bytes
            .saturating_add(u32::try_from(removed_bytes).unwrap_or(u32::MAX));
        summary.removed_fragment_bits = summary
            .removed_fragment_bits
            .saturating_add(u32::try_from(removed_bits).unwrap_or(u32::MAX));
        applied_removals.push((
            removal.kind,
            mention.opcode,
            mention.object_type,
            mention.object_id,
            removed_bytes,
            removed_bits,
        ));
    }

    let new_declared = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(live_bytes.len())?;
    let new_declared_u32 = u32::try_from(new_declared).ok()?;
    let mut rewritten =
        Vec::with_capacity(new_declared.checked_add(fragment_bits.len().div_ceil(8))?);
    rewritten.extend_from_slice(&[
        HIGH_LEVEL_ENVELOPE,
        GAME_OBJECT_UPDATE_MAJOR,
        LIVE_OBJECT_MINOR,
    ]);
    rewritten.extend_from_slice(&new_declared_u32.to_le_bytes());
    rewritten.extend_from_slice(&live_bytes);
    rewritten.extend_from_slice(&bits::pack_msb_valid_bits(
        fragment_bits,
        CNW_FRAGMENT_HEADER_BITS,
    ));
    summary.new_declared = new_declared_u32;

    claim_payload_if_verified(&rewritten)?;
    *payload = rewritten;
    for (kind, opcode, object_type, object_id, removed_bytes, removed_bits) in applied_removals {
        tracing::info!(
            rewrite_kind = kind.as_str(),
            opcode = %char::from(opcode),
            object_type,
            object_id,
            removed_bytes,
            removed_bits,
            "live-object lifecycle record removed after exact boundary proof"
        );
    }
    Some(summary)
}

fn removable_unmaterialized_lifecycle_kind(
    mention: &LiveObjectRecordMention,
) -> Option<LiveObjectLifecycleRewriteKind> {
    match mention.opcode {
        b'U' if update_requires_materialized_object(mention.object_type) => {
            // Decompile-backed compatibility rule:
            //
            // EE `GameObjUpdate_LiveObject` routes `U` through
            // `sub_1407B8380`, which reads the object type, object id, and mask,
            // then calls the shared generic update reader `sub_14079C050`.
            // `sub_14079C050` resolves the object pointer before the mask-body
            // reads; if the object is absent it logs
            // "Received update message for object that does not exist" and
            // returns before consuming position/orientation/state/name fields.
            //
            // Diamond's matching path (`sub_459700` -> `sub_467AE0`) resolves the
            // object early too, but `sub_467AE0` consumes the mask-driven update
            // fields before `test ebx, ebx` at `loc_467CB8`; a missing object
            // therefore becomes a no-op after the read cursor is already aligned.
            //
            // The proxy may remove only an exactly-bounded missing-object `U`
            // record. This preserves the Diamond cursor semantics without
            // forwarding an EE-aborting record.
            Some(LiveObjectLifecycleRewriteKind::DiamondMissingObjectUpdateNoop)
        }
        b'I' if mention.requires_materialized_object => {
            // Sentinel-owner inventory records are separate from generic object
            // updates. The exact inventory parser marks only the decompile-proven
            // feature-25 sentinel-owner case as requiring materialization, so the
            // lifecycle cleanup may remove that exact record but must not treat
            // arbitrary inventory rows as generic no-op object updates.
            Some(LiveObjectLifecycleRewriteKind::EeSentinelInventoryOwnerAbort)
        }
        _ => None,
    }
}

fn verified_record_name(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    opcode: u8,
    object_type: u8,
) -> Option<String> {
    if opcode != b'A' || record_end > live_bytes.len() {
        return None;
    }

    let name_offset = match object_type {
        PLACEABLE_OBJECT_TYPE => offset.checked_add(6)?,
        DOOR_OBJECT_TYPE => {
            let first_dword = read_u32_le(live_bytes, offset.checked_add(6)?)?;
            let visual_offset = offset.checked_add(2 + if first_dword == 0 { 12 } else { 8 })?;
            if creature::has_ee_identity_visual_transform_map_at(
                live_bytes,
                visual_offset,
                record_end,
            ) {
                visual_offset.checked_add(40)?
            } else {
                visual_offset
            }
        }
        _ => return None,
    };

    read_inline_live_string(live_bytes, name_offset, record_end)
        .or_else(|| read_short_locstring_name(live_bytes, name_offset, record_end))
}

fn read_inline_live_string(bytes: &[u8], offset: usize, record_end: usize) -> Option<String> {
    let length = usize::try_from(read_u32_le(bytes, offset)?).ok()?;
    if length > MAX_LIVE_OBJECT_NAME_BYTES {
        return None;
    }
    let start = offset.checked_add(4)?;
    let end = start.checked_add(length)?;
    if end > record_end || end > bytes.len() {
        return None;
    }
    let text = bytes.get(start..end)?;
    if !text
        .iter()
        .all(|byte| matches!(*byte, b'\t' | b'\n' | b'\r' | 0x20..=0x7E))
    {
        return None;
    }
    Some(String::from_utf8_lossy(text).to_string())
}

fn read_short_locstring_name(bytes: &[u8], offset: usize, record_end: usize) -> Option<String> {
    if offset.checked_add(4)? > record_end || offset.checked_add(4)? > bytes.len() {
        return None;
    }
    let strref = read_u32_le(bytes, offset)?;
    Some(format!("strref:{strref}"))
}

fn verified_record_position(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    opcode: u8,
) -> Option<LiveObjectRecordPosition> {
    if opcode != b'U'
        || offset + LEGACY_UPDATE_HEADER_BYTES + LEGACY_UPDATE_POSITION_READ_BYTES > record_end
        || record_end > live_bytes.len()
        || (read_u32_le(live_bytes, offset + 6)? & LEGACY_UPDATE_POSITION_MASK) == 0
        || fragment_bits.len().saturating_sub(bit_cursor) < LEGACY_UPDATE_POSITION_FRAGMENT_BITS
    {
        return None;
    }

    let fixed_x = read_u16_le(live_bytes, offset + LEGACY_UPDATE_HEADER_BYTES)? as u32;
    let fixed_y = read_u16_le(live_bytes, offset + LEGACY_UPDATE_HEADER_BYTES + 2)? as u32;
    let fixed_z = read_u16_le(live_bytes, offset + LEGACY_UPDATE_HEADER_BYTES + 4)? as u32;
    let z_low_bits: u32 = ((if fragment_bits.get(bit_cursor).copied().unwrap_or(false) {
        1
    } else {
        0
    }) << 1)
        | if fragment_bits.get(bit_cursor + 1).copied().unwrap_or(false) {
            1
        } else {
            0
        };
    let z_raw = (fixed_z << 2) | z_low_bits;
    let z_max = (1_u32 << 18) - 1;
    Some(LiveObjectRecordPosition {
        x: fixed_x as f32 / 100.0,
        y: fixed_y as f32 / 100.0,
        z: -20.0 + (z_raw as f32 / z_max as f32) * 340.0,
    })
}

fn verified_record_orientation(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &[bool],
    bit_cursor: usize,
    opcode: u8,
    object_type: u8,
) -> Option<LiveObjectRecordOrientation> {
    if opcode != b'U'
        || !matches!(object_type, DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE)
        || offset + LEGACY_UPDATE_HEADER_BYTES > record_end
        || record_end > live_bytes.len()
    {
        return None;
    }

    let mask = read_u32_le(live_bytes, offset + 6)?;
    if (mask & LEGACY_UPDATE_ORIENTATION_MASK) == 0 {
        return None;
    }

    let mut read_cursor = offset + LEGACY_UPDATE_HEADER_BYTES;
    let mut fragment_cursor = bit_cursor;
    if (mask & LEGACY_UPDATE_POSITION_MASK) != 0 {
        read_cursor = read_cursor.checked_add(LEGACY_UPDATE_POSITION_READ_BYTES)?;
        fragment_cursor = fragment_cursor.checked_add(LEGACY_UPDATE_POSITION_FRAGMENT_BITS)?;
    }
    if read_cursor + EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES > record_end
        || fragment_bits.len().saturating_sub(fragment_cursor)
            < EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
    {
        return None;
    }

    // EE `sub_14079C050` and Diamond `sub_467AE0` both read the generic
    // door/placeable orientation as a BOOL branch followed by the compact
    // scalar `ReadFLOAT(10.0,12)` path when the branch is false. The bridge
    // writer emits only that scalar branch for translated legacy records, so a
    // registry orientation is recorded only after the exact EE-shaped branch
    // bit is present and false.
    if fragment_bits.get(fragment_cursor).copied()? {
        return None;
    }
    let high = u16::from(*live_bytes.get(read_cursor)?);
    let mut low = 0_u16;
    for bit_index in 0..4 {
        low <<= 1;
        if fragment_bits
            .get(fragment_cursor + 1 + bit_index)
            .copied()
            .unwrap_or(false)
        {
            low |= 1;
        }
    }

    Some(LiveObjectRecordOrientation {
        scalar_tenths_degrees: (high << 4) | low,
    })
}

fn verified_record_bounds(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    opcode: u8,
    object_type: u8,
) -> Option<LiveObjectRecordBounds> {
    const TRIGGER_GEOMETRY_COUNT_OFFSET: usize = 15;
    const TRIGGER_VERTEX_START_OFFSET: usize = 16;
    const TRIGGER_VERTEX_FLOATS: usize = 3;
    const FLOAT_BYTES: usize = 4;
    const TRIGGER_VERTEX_BYTES: usize = TRIGGER_VERTEX_FLOATS * FLOAT_BYTES;

    if opcode != b'A'
        || object_type != TRIGGER_OBJECT_TYPE
        || record_end > live_bytes.len()
        || offset + TRIGGER_VERTEX_START_OFFSET > record_end
    {
        return None;
    }

    // EE `AddTriggerGeometryToMessage` writes a BYTE vertex count followed by
    // XYZ float triples. The exact trigger-add validator has already bounded
    // `record_end`; this parser only derives the clicked geometry center from
    // that decompile-owned shape for protocol-state decisions.
    let vertex_count = *live_bytes.get(offset + TRIGGER_GEOMETRY_COUNT_OFFSET)? as usize;
    if vertex_count == 0 {
        return None;
    }
    let geometry_bytes = vertex_count.checked_mul(TRIGGER_VERTEX_BYTES)?;
    if offset
        .checked_add(TRIGGER_VERTEX_START_OFFSET)?
        .checked_add(geometry_bytes)?
        != record_end
    {
        return None;
    }

    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut min_z = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut max_z = f32::NEG_INFINITY;
    let mut cursor = offset + TRIGGER_VERTEX_START_OFFSET;
    for _ in 0..vertex_count {
        let x = read_f32_le(live_bytes, cursor)?;
        cursor += FLOAT_BYTES;
        let y = read_f32_le(live_bytes, cursor)?;
        cursor += FLOAT_BYTES;
        let z = read_f32_le(live_bytes, cursor)?;
        cursor += FLOAT_BYTES;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return None;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        min_z = min_z.min(z);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        max_z = max_z.max(z);
    }

    Some(LiveObjectRecordBounds {
        min_x,
        min_y,
        min_z,
        max_x,
        max_y,
        max_z,
    })
}

fn trace_claim_reject(
    reason: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    let preview_end = record_end
        .min(live_bytes.len())
        .min(offset.saturating_add(96));
    let preview = live_bytes.get(offset..preview_end).unwrap_or(&[]);
    let following_end = live_bytes.len().min(record_end.saturating_add(48));
    let following = live_bytes.get(record_end..following_end).unwrap_or(&[]);
    tracing::trace!(
        reason,
        offset,
        record_end,
        bit_cursor,
        opcode = live_bytes.get(offset).copied().unwrap_or_default(),
        marker = live_bytes.get(offset + 1).copied().unwrap_or_default(),
        preview = ?preview,
        "live-object exact claim rejected"
    );
    eprintln!(
        "live-object exact claim rejected: reason={reason} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} preview={:02X?} following={:02X?}",
        live_bytes.get(offset).copied().unwrap_or_default(),
        live_bytes.get(offset + 1).copied().unwrap_or_default(),
        preview,
        following
    );
}

pub fn promote_work_remaining_trailing_fragment_span_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectUpdateRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
        || claim_payload_if_verified(payload).is_some()
    {
        return None;
    }

    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let declared = usize::try_from(old_declared).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }

    let mut live_bytes = payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared].to_vec();
    if !live_bytes.is_empty()
        && !boundary::looks_like_legacy_live_object_sub_message_boundary(&live_bytes, 0)
    {
        return None;
    }
    let mut fragment_bytes = payload[declared..].to_vec();
    let mut fragment_bits = bits::decode_msb_valid_bits(&fragment_bytes, CNW_FRAGMENT_HEADER_BITS)?;
    let old_live_bytes_length = live_bytes.len();
    let old_fragment_bytes = fragment_bytes.len();
    let promotion = fragment_spans::promote_work_remaining_trailing_fragment_span_for_ee(
        &mut live_bytes,
        &mut fragment_bytes,
        &mut fragment_bits,
    )?;

    let fragment_bytes = bits::pack_msb_valid_bits(fragment_bits, CNW_FRAGMENT_HEADER_BITS);
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes.len();
    let new_declared = u32::try_from(new_declared_usize).ok()?;
    let new_payload_length = new_declared_usize.checked_add(fragment_bytes.len())?;
    if new_payload_length > MAX_REASONABLE_LIVE_PAYLOAD_BYTES {
        return None;
    }

    let mut rewritten = Vec::with_capacity(new_payload_length);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&live_bytes);
    rewritten.extend_from_slice(&fragment_bytes);

    let mut summary = LiveObjectUpdateRewriteSummary {
        old_declared,
        new_declared,
        old_payload_length: payload.len(),
        new_payload_length: rewritten.len(),
        old_live_bytes_length,
        new_live_bytes_length: live_bytes.len(),
        old_fragment_bytes,
        new_fragment_bytes: fragment_bytes.len(),
        ..LiveObjectUpdateRewriteSummary::default()
    };
    summary.interleaved_fragment_spans_promoted = 1;
    summary.interleaved_fragment_bytes_promoted =
        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX);
    summary.interleaved_fragment_bits_promoted =
        u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX);
    summary.bytes_removed = u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX);

    *payload = rewritten;
    Some(summary)
}

pub fn rewrite_update_records_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<LiveObjectUpdateRewriteSummary> {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object update rewrite entered: payload_len={} prefix={:02X?}",
            payload.len(),
            payload.get(..payload.len().min(12)).unwrap_or(&[])
        );
    }
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return None;
    }

    // Translation must be idempotent at the packet-family boundary. The
    // M-frame dispatch path may run focused live-object passes in sequence so
    // mixed add/update records can expose one another, but once a payload is
    // already accepted by the exact EE-shape validator, running the legacy
    // update rewriter again would be byte surgery against a proven packet.
    // Diamond and EE both route GUI inventory item-create rows through the
    // same item-create helper after the `G I/i A` prefix; the first successful
    // legacy rewrite therefore owns the semantic transform, and later passes
    // must treat the packet as already translated.
    if claim_payload_if_verified(payload).is_some() {
        return None;
    }

    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let declared = usize::try_from(old_declared).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || declared > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
    {
        return None;
    }

    let mut live_bytes = payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared].to_vec();
    let mut fragment_bytes = payload[declared..].to_vec();
    if !live_bytes.is_empty()
        && !gui::looks_like_legacy_live_gui_rewrite_boundary(&live_bytes, 0)
        && !boundary::looks_like_legacy_live_object_sub_message_boundary(&live_bytes, 0)
    {
        // The update-family translator owns bounded live-object records, not
        // arbitrary resync inside a declared read window.  If the first byte is
        // not a decompile-backed live-object submessage boundary, a transport
        // or gameplay-stream layer must claim and split the leading bytes
        // before semantic mutation is safe.
        return None;
    }
    let mut fragment_bits = bits::decode_msb_valid_bits(&fragment_bytes, CNW_FRAGMENT_HEADER_BITS)?;
    let old_live_bytes_length = live_bytes.len();
    let old_fragment_bytes = payload.len().saturating_sub(declared);

    let mut summary = LiveObjectUpdateRewriteSummary {
        old_declared,
        old_payload_length: payload.len(),
        old_live_bytes_length,
        old_fragment_bytes,
        ..LiveObjectUpdateRewriteSummary::default()
    };

    let mut changed = false;
    if let Some(promotion) = fragment_spans::promote_work_remaining_trailing_fragment_span_for_ee(
        &mut live_bytes,
        &mut fragment_bytes,
        &mut fragment_bits,
    ) {
        changed = true;
        summary.interleaved_fragment_spans_promoted = summary
            .interleaved_fragment_spans_promoted
            .saturating_add(1);
        summary.interleaved_fragment_bytes_promoted = summary
            .interleaved_fragment_bytes_promoted
            .saturating_add(u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX));
        summary.interleaved_fragment_bits_promoted = summary
            .interleaved_fragment_bits_promoted
            .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
        summary.bytes_removed = summary
            .bytes_removed
            .saturating_add(u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX));
    }
    let mut bit_cursor = CNW_FRAGMENT_HEADER_BITS;
    let mut bit_cursor_reliable = true;
    let mut pending_creature_p_tail_repair: Option<
        tail_repair::PendingCreatureAppearanceTailRepair,
    > = None;
    let mut offset = 0usize;
    let mut last_verified_record_end = 0usize;
    let mut last_verified_creature_4408_record_end = None;
    let mut last_verified_creature_effect_only_record_end = None;
    let mut last_verified_creature_0047_fragment_prefix_record_end = None;
    let mut last_verified_creature_add_record: Option<(usize, u32)> = None;
    let mut last_verified_add_record: Option<(usize, u8, u32)> = None;
    let mut last_verified_door_add_fragment_span_record: Option<(usize, u32)> = None;
    let mut last_verified_record_allows_trailing_fragment_promotion = false;
    let mut terminal_work_remaining_fragment_storage_record: Option<(usize, usize)> = None;
    let mut loop_iterations = 0usize;
    let max_loop_iterations = old_live_bytes_length.saturating_mul(4).saturating_add(64);
    while offset + 2 <= live_bytes.len() {
        loop_iterations = loop_iterations.saturating_add(1);
        if loop_iterations > max_loop_iterations {
            trace_update_rewrite_cursor_unreliable(
                "live-object-update-rewrite-iteration-bound",
                &live_bytes,
                offset,
                offset.saturating_add(2).min(live_bytes.len()),
                bit_cursor,
            );
            return None;
        }
        if !bit_cursor_reliable {
            // A failed exact cursor advance means every later CNW fragment-bit
            // position is unproven. Stop before a later repair mutates bits at
            // a stale cursor and shifts an otherwise exact intervening record.
            break;
        }
        let proven_gui_record_end =
            if bit_cursor_reliable && live_bytes.get(offset).copied() == Some(b'G') {
                gui::try_get_legacy_live_gui_record_end_with_fragment_proof(
                    &live_bytes,
                    offset,
                    live_bytes.len(),
                    &fragment_bits,
                    bit_cursor,
                )
            } else {
                None
            };
        let legacy_gui_rewrite_boundary = live_bytes.get(offset).copied() == Some(b'G')
            && gui::looks_like_legacy_live_gui_rewrite_boundary(&live_bytes, offset);
        if live_bytes.get(offset).copied() == Some(b'G')
            && std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some()
        {
            eprintln!(
                "live-object update rewrite GUI boundary probe: offset={offset} legacy={legacy_gui_rewrite_boundary} proven={:?} preview={:02X?}",
                proven_gui_record_end,
                live_bytes
                    .get(offset..offset.saturating_add(16).min(live_bytes.len()))
                    .unwrap_or(&[])
            );
        }
        if bit_cursor_reliable
            && last_verified_creature_4408_record_end == Some(offset)
            && inventory::repair_missing_current_player_2a00_opcode_after_4408_for_ee(
                &mut live_bytes,
                offset,
                &mut fragment_bits,
                bit_cursor,
            )
            .is_some()
        {
            changed = true;
            last_verified_creature_4408_record_end = None;
            continue;
        }
        if bit_cursor_reliable && last_verified_creature_4408_record_end == Some(offset) {
            if let Some(selector_repair) =
                inventory::repair_current_player_2a00_selector_bits_after_compact_effect_for_ee(
                    &live_bytes,
                    offset,
                    boundary::find_next_legacy_live_object_sub_message_boundary_after(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                    )
                    .min(live_bytes.len()),
                    &mut fragment_bits,
                    bit_cursor,
                )
            {
                let _bits_materialized = selector_repair.bits_materialized;
                changed = true;
                last_verified_creature_4408_record_end = None;
                continue;
            }
        }
        if bit_cursor_reliable && last_verified_creature_effect_only_record_end == Some(offset) {
            if let Some(selector_repair) =
                inventory::repair_current_player_2a00_selector_bits_after_compact_effect_for_ee(
                    &live_bytes,
                    offset,
                    boundary::find_next_legacy_live_object_sub_message_boundary_after(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                    )
                    .min(live_bytes.len()),
                    &mut fragment_bits,
                    bit_cursor,
                )
            {
                let _bits_materialized = selector_repair.bits_materialized;
                changed = true;
                last_verified_creature_effect_only_record_end = None;
                continue;
            }
        }
        if bit_cursor_reliable {
            if let Some((add_record_end, add_object_id)) = last_verified_creature_add_record {
                if add_record_end == offset {
                    let scan_end = live_bytes.len();
                    if let Some(record_end) =
                        repair_missing_creature_appearance_opcode_after_add_for_ee(
                            &mut live_bytes,
                            offset,
                            scan_end,
                            add_object_id,
                        )
                    {
                        tracing::info!(
                            object_id = format_args!("0x{add_object_id:08X}"),
                            record_end,
                            "live-object creature appearance missing opcode repaired after verified add"
                        );
                        changed = true;
                        summary.live_object_missing_appearance_opcodes_repaired = summary
                            .live_object_missing_appearance_opcodes_repaired
                            .saturating_add(1);
                        last_verified_creature_add_record = None;
                        continue;
                    }
                }
            }
            if let Some((add_record_end, add_object_type, add_object_id)) = last_verified_add_record
            {
                if add_record_end == offset {
                    if let Some(span) = boundary::
                        try_get_legacy_missing_opcode_door_placeable_low_tail_fragment_span_after_add(
                            &live_bytes,
                            offset,
                            live_bytes.len(),
                            add_object_type,
                            add_object_id,
                        )
                    {
                        let Some(span_bytes) = live_bytes.get(span.span_start..span.span_end)
                        else {
                            return None;
                        };
                        let Some(mut decoded_bits) = bits::decode_msb_valid_bits(
                            span_bytes,
                            CNW_FRAGMENT_HEADER_BITS.saturating_add(span.low_tail_bits),
                        )
                        else {
                            return None;
                        };
                        if decoded_bits.len()
                            == CNW_FRAGMENT_HEADER_BITS.saturating_add(span.low_tail_bits)
                        {
                            decoded_bits.drain(0..CNW_FRAGMENT_HEADER_BITS);
                            if decoded_bits.len() == span.low_tail_bits {
                                // Local Chapter1 Diamond can place the legacy
                                // door/placeable low 0x40/0x80 BOOLs in a
                                // one-byte CNW fragment-storage span between a
                                // compact `A` record and that same object's
                                // missing-opcode update body. The preceding
                                // verified add pins the object id/type, the
                                // boundary helper proves the decompiled shared
                                // update prefix, and these low bits have no EE
                                // reader. Drop only the proven storage byte;
                                // the next loop repairs the missing `U` and the
                                // typed update writer emits the EE mask.
                                let bytes_removed = span.span_end.saturating_sub(span.span_start);
                                live_bytes.drain(span.span_start..span.span_end);
                                changed = true;
                                summary.interleaved_fragment_spans_promoted = summary
                                    .interleaved_fragment_spans_promoted
                                    .saturating_add(1);
                                summary.interleaved_fragment_bytes_promoted = summary
                                    .interleaved_fragment_bytes_promoted
                                    .saturating_add(
                                        u32::try_from(bytes_removed).unwrap_or(u32::MAX),
                                    );
                                summary.bytes_removed = summary.bytes_removed.saturating_add(
                                    u32::try_from(bytes_removed).unwrap_or(u32::MAX),
                                );
                                continue;
                            }
                        }
                    }
                }
                if add_record_end == offset
                    && boundary::try_get_legacy_missing_opcode_door_placeable_update_body_end_after_add(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                        add_object_type,
                        add_object_id,
                    )
                    .is_some()
                {
                    // Local Winds of Eremor Diamond streams can emit a compact
                    // `A/9` placeable add followed immediately by the same
                    // object's update body starting at the type byte, or at a
                    // leading zero byte before the type (`00 09 id mask...`),
                    // instead of the top-level `U` opcode. The
                    // boundary helper above proves the body against the same
                    // door/placeable generic update reader shape and the same
                    // object id before this transactional insertion. The
                    // focused update-record rewrite and final exact validator
                    // still own the emitted EE bytes and fragment cursor.
                    if live_bytes.get(offset).copied() == Some(0)
                        && live_bytes.get(offset + 1).copied() == Some(add_object_type)
                    {
                        live_bytes[offset] = b'U';
                    } else {
                        live_bytes.insert(offset, b'U');
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(1);
                    }
                    changed = true;
                    summary.live_object_missing_update_opcodes_repaired = summary
                        .live_object_missing_update_opcodes_repaired
                        .saturating_add(1);
                    last_verified_add_record = None;
                    continue;
                }
                if add_record_end == offset
                    && boundary::try_get_legacy_missing_type_door_placeable_update_end_after_add(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                        add_object_type,
                        add_object_id,
                    )
                    .is_some()
                {
                    // Local XP2 Chapter 2 emits the same decompile-shaped
                    // door/placeable update body as the missing-opcode path,
                    // but keeps the top-level `U` and leaves the object-type
                    // byte as zero. The preceding verified `A` record pins the
                    // object type and id before this one-byte repair.
                    live_bytes[offset + 1] = add_object_type;
                    changed = true;
                    summary.live_object_missing_update_opcodes_repaired = summary
                        .live_object_missing_update_opcodes_repaired
                        .saturating_add(1);
                    last_verified_add_record = None;
                    continue;
                }
            }
            if let Some((door_record_end, door_object_id)) =
                last_verified_door_add_fragment_span_record
            {
                if door_record_end == offset {
                    // Diamond door adds can be followed by a chunk-local
                    // CNW fragment span that begins with `U 00 <same id>`.
                    // That prefix is boundary-looking, so probe the
                    // decompile-owned door-add span before the generic live
                    // boundary scanner treats it as a malformed update.
                    if let Some(promotion) = fragment_spans::
                        promote_door_add_following_missing_type_update_fragment_span_for_ee(
                            &mut live_bytes,
                            &mut fragment_bits,
                            offset,
                            bit_cursor,
                            door_object_id,
                        )
                    {
                        changed = true;
                        summary.interleaved_fragment_spans_promoted =
                            summary.interleaved_fragment_spans_promoted.saturating_add(1);
                        summary.interleaved_fragment_bytes_promoted =
                            summary.interleaved_fragment_bytes_promoted.saturating_add(
                                u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                            );
                        summary.interleaved_fragment_bits_promoted =
                            summary.interleaved_fragment_bits_promoted.saturating_add(
                                u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                            );
                        summary.bytes_removed = summary.bytes_removed.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                        last_verified_door_add_fragment_span_record = None;
                        continue;
                    }
                }
            }
        }
        if proven_gui_record_end.is_none()
            && !legacy_gui_rewrite_boundary
            && !boundary::looks_like_legacy_live_object_sub_message_boundary(&live_bytes, offset)
        {
            if summary.records_examined > 0 {
                if bit_cursor_reliable {
                    if let Some((door_record_end, door_object_id)) =
                        last_verified_door_add_fragment_span_record
                    {
                        if door_record_end == offset {
                            if let Some(promotion) = fragment_spans::
                                promote_door_add_following_missing_type_update_fragment_span_for_ee(
                                    &mut live_bytes,
                                    &mut fragment_bits,
                                    offset,
                                    bit_cursor,
                                    door_object_id,
                                )
                            {
                                changed = true;
                                summary.interleaved_fragment_spans_promoted = summary
                                    .interleaved_fragment_spans_promoted
                                    .saturating_add(1);
                                summary.interleaved_fragment_bytes_promoted =
                                    summary.interleaved_fragment_bytes_promoted.saturating_add(
                                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                                    );
                                summary.interleaved_fragment_bits_promoted =
                                    summary.interleaved_fragment_bits_promoted.saturating_add(
                                        u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                                    );
                                summary.bytes_removed = summary.bytes_removed.saturating_add(
                                    u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                                );
                                last_verified_door_add_fragment_span_record = None;
                                continue;
                            }
                        }
                    }
                }
                if let Some(removal) =
                    fragment_spans::remove_trailing_zero_fragment_storage_after_verified_record_for_ee(
                        &mut live_bytes,
                        offset,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted =
                        summary.interleaved_fragment_spans_promoted.saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                        );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                    );
                    continue;
                }
            }
            last_verified_record_allows_trailing_fragment_promotion = false;
            last_verified_creature_4408_record_end = None;
            last_verified_creature_0047_fragment_prefix_record_end = None;
            last_verified_creature_add_record = None;
            last_verified_add_record = None;
            last_verified_door_add_fragment_span_record = None;
            offset += 1;
            continue;
        }

        summary.records_examined = summary.records_examined.saturating_add(1);
        let opcode = live_bytes[offset];
        let object_type = live_bytes[offset + 1];
        let mut record_end = if opcode == b'G' {
            // GUI inventory/repository item-create rows contain item-object
            // bodies that can look like unrelated live-object records. When
            // the row is not already exact-EE-verifiable, derive the legacy
            // row end from the decompiled GUI row shape and sibling GUI
            // boundaries, then let the rewrite and final exact validator prove
            // the emitted EE packet.
            proven_gui_record_end
                .or_else(|| {
                    gui::try_get_legacy_live_gui_item_create_read_end(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                    )
                })
                .or_else(|| {
                    gui::try_get_legacy_live_gui_record_end(&live_bytes, offset, live_bytes.len())
                })
                .unwrap_or_else(|| {
                    boundary::find_next_legacy_live_object_sub_message_boundary_after(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                    )
                    .min(live_bytes.len())
                })
        } else {
            boundary::find_next_legacy_live_object_sub_message_boundary_after(
                &live_bytes,
                offset,
                live_bytes.len(),
            )
            .min(live_bytes.len())
        };
        let mut creature_appearance_already_ee_shaped = false;
        let mut creature_appearance_verified_ee_shaped = false;
        let mut creature_appearance_legacy_end: Option<usize> = None;
        if opcode == b'P' && object_type == CREATURE_OBJECT_TYPE {
            if bit_cursor_reliable {
                if let Some(verified_end) =
                    appearance::try_get_verified_ee_creature_appearance_record_end(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    record_end = verified_end;
                    creature_appearance_already_ee_shaped = true;
                    creature_appearance_verified_ee_shaped = true;
                }
            }
            if !creature_appearance_already_ee_shaped {
                if let Some(legacy_end) = appearance::try_get_legacy_creature_appearance_record_end(
                    &live_bytes,
                    offset,
                    live_bytes.len(),
                ) {
                    // Full creature appearance records own a counted
                    // visible-equipment stream. Those embedded equipment rows
                    // can begin with live-object-looking `A`/`D` bytes, so the
                    // semantic appearance parser must claim the decompile-owned
                    // record end before the generic live-object boundary
                    // scanner splits the nested item add into a false top-level
                    // packet. The transactional appearance rewrite below still
                    // has to prove the fragment cursor before anything is sent.
                    record_end = legacy_end;
                    creature_appearance_legacy_end = Some(legacy_end);
                }
            }
            if !creature_appearance_already_ee_shaped {
                if let Some(byte_shape_end) =
                    appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
                        &live_bytes,
                        offset,
                        live_bytes.len(),
                    )
                {
                    if creature_appearance_legacy_end
                        .map(|legacy_end| byte_shape_end >= legacy_end)
                        .unwrap_or(true)
                    {
                        record_end = byte_shape_end;
                        creature_appearance_already_ee_shaped = true;
                    }
                }
            }
        }
        if opcode == b'G' && bit_cursor_reliable {
            if let Some(verified_end) = gui::try_get_legacy_live_gui_record_end_with_fragment_proof(
                &live_bytes,
                offset,
                live_bytes.len(),
                &fragment_bits,
                bit_cursor,
            ) {
                record_end = verified_end;
            }
        }
        if record_end <= offset {
            offset += 1;
            continue;
        }

        if let Some(bytes_removed) =
            remove_midstream_work_remaining_fragment_storage_after_top_level_record_for_ee(
                &mut live_bytes,
                offset,
                &mut record_end,
            )
        {
            changed = true;
            summary.bytes_removed = summary
                .bytes_removed
                .saturating_add(u32::try_from(bytes_removed).unwrap_or(u32::MAX));
            terminal_work_remaining_fragment_storage_record = None;
        }

        if let Some(legal_end) = verified_work_remaining_record_legal_end(&live_bytes, offset) {
            if record_end == live_bytes.len() && legal_end < record_end {
                terminal_work_remaining_fragment_storage_record = Some((offset, legal_end));
                offset = record_end.max(offset + 1);
                continue;
            }
        } else {
            terminal_work_remaining_fragment_storage_record = None;
        }

        if world_status::claim_identity_record_for_ee(&live_bytes, offset, record_end) {
            terminal_work_remaining_fragment_storage_record =
                (record_end == live_bytes.len()).then_some((offset, record_end));
            last_verified_record_end = record_end;
            last_verified_record_allows_trailing_fragment_promotion = false;
            offset = record_end;
            continue;
        }

        if opcode == b'A' {
            if let Some(add_rewrite) =
                creature_add::insert_ee_visual_transform_for_legacy_creature_add(
                    &mut live_bytes,
                    offset,
                    &mut record_end,
                )
            {
                changed = true;
                summary.bytes_inserted = summary
                    .bytes_inserted
                    .saturating_add(u32::try_from(add_rewrite.bytes_inserted).unwrap_or(u32::MAX));
                summary.bytes_removed = summary
                    .bytes_removed
                    .saturating_add(u32::try_from(add_rewrite.bytes_removed).unwrap_or(u32::MAX));
            }

            if bit_cursor_reliable {
                let exact_add_start_bit_cursor = bit_cursor;
                if add::advance_verified_add_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = false;
                    last_verified_creature_add_record =
                        if bit_cursor_reliable && object_type == CREATURE_OBJECT_TYPE {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_id))
                        } else {
                            None
                        };
                    last_verified_add_record =
                        if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_type, object_id))
                        } else {
                            None
                        };
                    last_verified_door_add_fragment_span_record = if object_type == DOOR_OBJECT_TYPE
                    {
                        read_u32_le(&live_bytes, offset + 2)
                            .map(|object_id| (record_end, object_id))
                    } else {
                        None
                    };
                    offset = record_end.max(offset + 1);
                    continue;
                }
                bit_cursor = exact_add_start_bit_cursor;

                if let Some(repair) = appearance::repair_verified_ee_item_add_name_fragment_bits(
                    &live_bytes,
                    offset,
                    record_end,
                    &mut fragment_bits,
                    bit_cursor,
                ) {
                    changed = true;
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(repair.bits_inserted).unwrap_or(u32::MAX));
                    summary.bits_removed = summary
                        .bits_removed
                        .saturating_add(u32::try_from(repair.bits_removed).unwrap_or(u32::MAX));
                    bit_cursor = repair.next_bit_cursor;
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = false;
                    last_verified_creature_add_record =
                        if bit_cursor_reliable && object_type == CREATURE_OBJECT_TYPE {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_id))
                        } else {
                            None
                        };
                    last_verified_add_record =
                        if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_type, object_id))
                        } else {
                            None
                        };
                    last_verified_door_add_fragment_span_record = if object_type == DOOR_OBJECT_TYPE
                    {
                        read_u32_le(&live_bytes, offset + 2)
                            .map(|object_id| (record_end, object_id))
                    } else {
                        None
                    };
                    offset = record_end.max(offset + 1);
                    continue;
                }

                if object_type == ITEM_OBJECT_TYPE {
                    if let Some(item_rewrite) = appearance::insert_ee_item_create_extras_for_ee(
                        &mut live_bytes,
                        offset + 2,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    ) {
                        if item_rewrite.bits_inserted != 0
                            || item_rewrite.bits_removed != 0
                            || item_rewrite.bytes_inserted != 0
                            || item_rewrite.bytes_removed != 0
                        {
                            changed = true;
                            summary.bits_inserted = summary.bits_inserted.saturating_add(
                                u32::try_from(item_rewrite.bits_inserted).unwrap_or(u32::MAX),
                            );
                            summary.bits_removed = summary.bits_removed.saturating_add(
                                u32::try_from(item_rewrite.bits_removed).unwrap_or(u32::MAX),
                            );
                            summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                                u32::try_from(item_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                            );
                            summary.bytes_removed = summary.bytes_removed.saturating_add(
                                u32::try_from(item_rewrite.bytes_removed).unwrap_or(u32::MAX),
                            );
                        }
                    }
                }

                if let Some(item_rewrite) = appearance::insert_ee_item_add_extras_for_ee(
                    &mut live_bytes,
                    offset,
                    &mut record_end,
                    &mut fragment_bits,
                    bit_cursor,
                ) {
                    if item_rewrite.bits_inserted != 0
                        || item_rewrite.bits_removed != 0
                        || item_rewrite.bytes_inserted != 0
                        || item_rewrite.bytes_removed != 0
                    {
                        changed = true;
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(item_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bits_removed = summary.bits_removed.saturating_add(
                            u32::try_from(item_rewrite.bits_removed).unwrap_or(u32::MAX),
                        );
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(item_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bytes_removed = summary.bytes_removed.saturating_add(
                            u32::try_from(item_rewrite.bytes_removed).unwrap_or(u32::MAX),
                        );
                    }
                }

                let exact_add_start_bit_cursor = bit_cursor;
                if add::advance_verified_add_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                ) {
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = false;
                    last_verified_creature_add_record =
                        if bit_cursor_reliable && object_type == CREATURE_OBJECT_TYPE {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_id))
                        } else {
                            None
                        };
                    last_verified_add_record =
                        if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_type, object_id))
                        } else {
                            None
                        };
                    last_verified_door_add_fragment_span_record = if object_type == DOOR_OBJECT_TYPE
                    {
                        read_u32_le(&live_bytes, offset + 2)
                            .map(|object_id| (record_end, object_id))
                    } else {
                        None
                    };
                    offset = record_end.max(offset + 1);
                    continue;
                }
                bit_cursor = exact_add_start_bit_cursor;

                let add_guard_bits_before = fragment_bits.len();
                if let Some(placeable_guard_changed) =
                    add_guard::repair_verified_ee_placeable_add_guard_bits(
                        &live_bytes,
                        offset,
                        record_end,
                        &mut fragment_bits,
                        &mut bit_cursor,
                    )
                {
                    changed |= placeable_guard_changed;
                    if fragment_bits.len() > add_guard_bits_before {
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(fragment_bits.len() - add_guard_bits_before)
                                .unwrap_or(u32::MAX),
                        );
                    }
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = false;
                    last_verified_creature_add_record =
                        if bit_cursor_reliable && object_type == CREATURE_OBJECT_TYPE {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_id))
                        } else {
                            None
                        };
                    last_verified_add_record =
                        if matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE) {
                            read_u32_le(&live_bytes, offset + 2)
                                .map(|object_id| (record_end, object_type, object_id))
                        } else {
                            None
                        };
                    last_verified_door_add_fragment_span_record = if object_type == DOOR_OBJECT_TYPE
                    {
                        read_u32_le(&live_bytes, offset + 2)
                            .map(|object_id| (record_end, object_id))
                    } else {
                        None
                    };
                    offset = record_end.max(offset + 1);
                    continue;
                }

                if object_type == DOOR_OBJECT_TYPE {
                    if let Some(promotion) = fragment_spans::
                        promote_door_add_embedded_missing_type_update_fragment_span_for_ee(
                            &mut live_bytes,
                            &mut fragment_bits,
                            offset,
                            record_end,
                            bit_cursor,
                        )
                    {
                        changed = true;
                        summary.interleaved_fragment_spans_promoted =
                            summary.interleaved_fragment_spans_promoted.saturating_add(1);
                        summary.interleaved_fragment_bytes_promoted =
                            summary.interleaved_fragment_bytes_promoted.saturating_add(
                                u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                            );
                        summary.interleaved_fragment_bits_promoted =
                            summary.interleaved_fragment_bits_promoted.saturating_add(
                                u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                            );
                        summary.bytes_removed = summary.bytes_removed.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                        last_verified_door_add_fragment_span_record = None;
                        continue;
                    }
                }

                let add_record_start_bit_cursor = bit_cursor;
                if !(object_type == ITEM_OBJECT_TYPE
                    && appearance::advance_verified_ee_item_create_record(
                        &live_bytes,
                        offset + 2,
                        record_end,
                        &fragment_bits,
                        &mut bit_cursor,
                    ))
                    && !appearance::advance_verified_ee_item_add_record(
                        &live_bytes,
                        offset,
                        record_end,
                        &fragment_bits,
                        &mut bit_cursor,
                    )
                    && !cursor::advance_live_add_record_bit_cursor(
                        &live_bytes,
                        &fragment_bits,
                        offset,
                        record_end,
                        &mut bit_cursor,
                    )
                    && !cursor::advance_legacy_add_record_bit_cursor_for_update_pass(
                        &live_bytes,
                        &fragment_bits,
                        offset,
                        record_end,
                        &mut bit_cursor,
                    )
                {
                    bit_cursor = add_record_start_bit_cursor;
                    if last_verified_record_allows_trailing_fragment_promotion
                        && offset == last_verified_record_end
                        && record_end == live_bytes.len()
                    {
                        if let Some(promotion) = fragment_spans::
                            promote_boundary_collision_trailing_fragment_prefix_after_verified_record_for_ee(
                                &mut live_bytes,
                                &mut fragment_bits,
                                offset,
                                bit_cursor,
                            )
                        {
                            changed = true;
                            summary.interleaved_fragment_spans_promoted = summary
                                .interleaved_fragment_spans_promoted
                                .saturating_add(1);
                            summary.interleaved_fragment_bytes_promoted =
                                summary.interleaved_fragment_bytes_promoted.saturating_add(
                                    u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                                );
                            summary.interleaved_fragment_bits_promoted =
                                summary.interleaved_fragment_bits_promoted.saturating_add(
                                    u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                                );
                            summary.bytes_removed = summary.bytes_removed.saturating_add(
                                u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                            );
                            last_verified_record_allows_trailing_fragment_promotion = false;
                            continue;
                        }
                    }
                    trace_update_rewrite_cursor_unreliable(
                        "add-record-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            last_verified_record_end = record_end;
            let ee_creature_add_record = object_type == CREATURE_OBJECT_TYPE
                && creature::looks_like_ee_creature_add_record(&live_bytes, offset, record_end);
            last_verified_creature_add_record = if bit_cursor_reliable
                && object_type == CREATURE_OBJECT_TYPE
                && ee_creature_add_record
            {
                read_u32_le(&live_bytes, offset + 2).map(|object_id| (record_end, object_id))
            } else {
                None
            };
            last_verified_add_record = if bit_cursor_reliable
                && matches!(object_type, PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE)
            {
                read_u32_le(&live_bytes, offset + 2)
                    .map(|object_id| (record_end, object_type, object_id))
            } else {
                None
            };
            last_verified_door_add_fragment_span_record =
                if bit_cursor_reliable && object_type == DOOR_OBJECT_TYPE {
                    read_u32_le(&live_bytes, offset + 2).map(|object_id| (record_end, object_id))
                } else {
                    None
                };
            last_verified_record_allows_trailing_fragment_promotion = ee_creature_add_record;
            if ee_creature_add_record && record_end < live_bytes.len() {
                if let Some(promotion) =
                    fragment_spans::promote_trailing_fragment_prefix_after_verified_record_for_ee(
                        &mut live_bytes,
                        &mut fragment_bytes,
                        &mut fragment_bits,
                        record_end,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    last_verified_record_allows_trailing_fragment_promotion = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'D' {
            if bit_cursor_reliable {
                if let Some(delete_bits) =
                    cursor::legacy_live_delete_fragment_bit_count(&live_bytes, offset, record_end)
                {
                    if fragment_bits.len().saturating_sub(bit_cursor) >= delete_bits {
                        bit_cursor += delete_bits;
                    } else {
                        trace_update_rewrite_cursor_unreliable(
                            "delete-record-fragment-bits-insufficient",
                            &live_bytes,
                            offset,
                            record_end,
                            bit_cursor,
                        );
                        bit_cursor_reliable = false;
                    }
                } else {
                    trace_update_rewrite_cursor_unreliable(
                        "delete-record-bit-count-unknown",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'P' && object_type == 0x05 {
            if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                eprintln!(
                    "live-object creature-P rewrite state: offset={offset} record_end={record_end} bit_cursor={bit_cursor} bit_cursor_reliable={bit_cursor_reliable} already_ee={creature_appearance_already_ee_shaped} verified_ee={creature_appearance_verified_ee_shaped} legacy_end={creature_appearance_legacy_end:?}"
                );
            }
            let original_fragment_bits_for_tail_repair = fragment_bits.clone();
            let mut appearance_bits_inserted_for_tail_repair = 0usize;
            let mut appearance_tail_fragment_bits_adjusted = false;
            let mut appearance_rewrite_changed_stream = false;
            let legacy_appearance_rewrite_candidate = bit_cursor_reliable
                && !creature_appearance_verified_ee_shaped
                && creature_appearance_legacy_end
                    .map(|legacy_end| legacy_end <= record_end)
                    .unwrap_or(false);
            let may_attempt_appearance_rewrite = (bit_cursor_reliable
                && (!creature_appearance_already_ee_shaped || legacy_appearance_rewrite_candidate))
                || (!bit_cursor_reliable && creature_appearance_already_ee_shaped)
                // The outer creature body can already be EE build-0x23 shaped
                // while nested visible-equipment item subobjects still need
                // the same transactional EE active-property inserts.
                || (bit_cursor_reliable
                    && creature_appearance_already_ee_shaped
                    && !creature_appearance_verified_ee_shaped);
            if may_attempt_appearance_rewrite {
                if let Some(appearance_rewrite) =
                    appearance::insert_ee_creature_appearance_extras_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    )
                {
                    appearance_bits_inserted_for_tail_repair = appearance_rewrite.bits_inserted;
                    if appearance_rewrite.bits_inserted != 0
                        || appearance_rewrite.bits_removed != 0
                        || appearance_rewrite.bytes_inserted != 0
                        || appearance_rewrite.bytes_removed != 0
                    {
                        appearance_rewrite_changed_stream = true;
                        changed = true;
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(appearance_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bits_removed = summary.bits_removed.saturating_add(
                            u32::try_from(appearance_rewrite.bits_removed).unwrap_or(u32::MAX),
                        );
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(appearance_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bytes_removed = summary.bytes_removed.saturating_add(
                            u32::try_from(appearance_rewrite.bytes_removed).unwrap_or(u32::MAX),
                        );
                    }
                }
            }
            if bit_cursor_reliable
                && creature_appearance_already_ee_shaped
                && !creature_appearance_verified_ee_shaped
            {
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                    eprintln!(
                        "live-object creature-P already-EE repair considered: offset={offset} record_end={record_end} bit_cursor={bit_cursor}"
                    );
                }
                if let Some(appearance_rewrite) =
                    appearance::repair_ee_creature_appearance_name_bits_if_possible(
                        &live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    )
                {
                    appearance_bits_inserted_for_tail_repair =
                        appearance_bits_inserted_for_tail_repair
                            .saturating_add(appearance_rewrite.bits_inserted);
                    changed = true;
                    appearance_rewrite_changed_stream = true;
                    summary.bits_inserted = summary.bits_inserted.saturating_add(
                        u32::try_from(appearance_rewrite.bits_inserted).unwrap_or(u32::MAX),
                    );
                    summary.bits_removed = summary.bits_removed.saturating_add(
                        u32::try_from(appearance_rewrite.bits_removed).unwrap_or(u32::MAX),
                    );
                }
                if let Some(appearance_rewrite) =
                    appearance::remove_ee_creature_appearance_zero_fragment_padding_if_possible(
                        &live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    )
                {
                    if appearance_rewrite.bits_inserted != 0 || appearance_rewrite.bits_removed != 0
                    {
                        changed = true;
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(appearance_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bits_removed = summary.bits_removed.saturating_add(
                            u32::try_from(appearance_rewrite.bits_removed).unwrap_or(u32::MAX),
                        );
                    }
                }
            }
            let mut advanced_appearance_cursor = bit_cursor;
            if appearance::advance_verified_ee_creature_appearance_record(
                &live_bytes,
                offset,
                record_end,
                &fragment_bits,
                &mut advanced_appearance_cursor,
            ) {
                bit_cursor = advanced_appearance_cursor;
                bit_cursor_reliable = true;
                if let Some(removal) = appearance::
                    remove_ee_appearance_trailing_legacy_tail_before_verified_creature_update_for_ee(
                        &mut live_bytes,
                        record_end,
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                    );
                    appearance_tail_fragment_bits_adjusted = true;
                }
            } else if creature::advance_verified_noop_creature_appearance_record(
                &live_bytes,
                offset,
                record_end,
                &fragment_bits,
                &mut advanced_appearance_cursor,
            ) {
                bit_cursor = advanced_appearance_cursor;
                bit_cursor_reliable = true;
            } else if bit_cursor_reliable {
                let mut repaired_tail = false;
                if let Some(ee_shape_end) =
                    appearance::try_get_ee_creature_appearance_record_end_before_verified_creature_update_tail_for_ee(
                        &live_bytes,
                        offset,
                        record_end,
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    let mut ee_shape_cursor = bit_cursor;
                    if appearance::advance_verified_ee_creature_appearance_record(
                        &live_bytes,
                        offset,
                        ee_shape_end,
                        &fragment_bits,
                        &mut ee_shape_cursor,
                    ) {
                        if let Some(removal) = appearance::
                            remove_ee_appearance_trailing_legacy_tail_before_verified_creature_update_for_ee(
                                &mut live_bytes,
                                ee_shape_end,
                                &fragment_bits,
                                ee_shape_cursor,
                            )
                        {
                            changed = true;
                            summary.bytes_removed = summary.bytes_removed.saturating_add(
                                u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                            );
                            record_end = ee_shape_end;
                            bit_cursor = ee_shape_cursor;
                            bit_cursor_reliable = true;
                            appearance_tail_fragment_bits_adjusted = true;
                            repaired_tail = true;
                        } else if fragment_spans::
                            verified_appearance_following_creature_update_span_offset_for_ee(
                                &live_bytes,
                                ee_shape_end,
                                ee_shape_cursor,
                                &fragment_bits,
                            )
                            .is_some()
                        {
                            record_end = ee_shape_end;
                            bit_cursor = ee_shape_cursor;
                            bit_cursor_reliable = true;
                            repaired_tail = true;
                        }
                    }
                }
                if !repaired_tail {
                    trace_update_rewrite_cursor_unreliable(
                        "creature-appearance-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            if bit_cursor_reliable {
                if let Some(promotion) =
                    fragment_spans::promote_appearance_following_creature_update_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    appearance_tail_fragment_bits_adjusted = true;
                }
                if !appearance_tail_fragment_bits_adjusted {
                    if let Some(promotion) =
                        fragment_spans::promote_appearance_following_item_add_span_for_ee(
                            &mut live_bytes,
                            &mut fragment_bits,
                            record_end,
                            bit_cursor,
                        )
                    {
                        changed = true;
                        summary.interleaved_fragment_spans_promoted = summary
                            .interleaved_fragment_spans_promoted
                            .saturating_add(1);
                        summary.interleaved_fragment_bytes_promoted =
                            summary.interleaved_fragment_bytes_promoted.saturating_add(
                                u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                            );
                        summary.interleaved_fragment_bits_promoted =
                            summary.interleaved_fragment_bits_promoted.saturating_add(
                                u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                            );
                        summary.bytes_removed = summary.bytes_removed.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                        );
                        appearance_tail_fragment_bits_adjusted = true;
                    }
                }
                if let Some(removal) =
                    fragment_spans::remove_trailing_zero_fragment_storage_after_verified_record_for_ee(
                        &mut live_bytes,
                        record_end,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted =
                        summary.interleaved_fragment_spans_promoted.saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                        );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(removal.bytes_removed).unwrap_or(u32::MAX),
                    );
                }
            }
            if bit_cursor_reliable
                && (appearance_bits_inserted_for_tail_repair != 0
                    || appearance_rewrite_changed_stream)
                && !appearance_tail_fragment_bits_adjusted
                && bit_cursor >= appearance_bits_inserted_for_tail_repair
            {
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                    eprintln!(
                        "live-object creature-P tail repair armed: offset={offset} bit_cursor={bit_cursor} inserted_bits={appearance_bits_inserted_for_tail_repair} reason=appearance-stream-changed"
                    );
                }
                pending_creature_p_tail_repair =
                    live_object_record_object_id(&live_bytes, offset, record_end).and_then(
                        |object_id| {
                            tail_repair::PendingCreatureAppearanceTailRepair::new(
                                original_fragment_bits_for_tail_repair,
                                bit_cursor - appearance_bits_inserted_for_tail_repair,
                                bit_cursor,
                                appearance_bits_inserted_for_tail_repair,
                                offset,
                                object_id,
                            )
                        },
                    );
            } else if appearance_tail_fragment_bits_adjusted {
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                    eprintln!(
                        "live-object creature-P tail repair cleared: offset={offset} reason=fragment-bits-adjusted bit_cursor={bit_cursor} inserted_bits={appearance_bits_inserted_for_tail_repair}"
                    );
                }
                pending_creature_p_tail_repair = None;
            } else if bit_cursor_reliable && creature_appearance_already_ee_shaped {
                if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                    eprintln!(
                        "live-object creature-P tail repair armed: offset={offset} bit_cursor={bit_cursor} inserted_bits=0 reason=already-ee-shaped"
                    );
                }
                pending_creature_p_tail_repair =
                    live_object_record_object_id(&live_bytes, offset, record_end).and_then(
                        |object_id| {
                            tail_repair::PendingCreatureAppearanceTailRepair::new(
                                fragment_bits.clone(),
                                bit_cursor,
                                bit_cursor,
                                0,
                                offset,
                                object_id,
                            )
                        },
                    );
            } else if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some()
                && opcode == b'P'
                && object_type == 0x05
            {
                eprintln!(
                    "live-object creature-P tail repair not armed: offset={offset} already_ee={creature_appearance_already_ee_shaped} bit_cursor_reliable={bit_cursor_reliable} bit_cursor={bit_cursor} inserted_bits={appearance_bits_inserted_for_tail_repair}"
                );
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'U' && object_type == CREATURE_OBJECT_TYPE {
            if bit_cursor_reliable {
                if let Some(effect_rewrite) =
                    effects::rewrite_legacy_looping_visual_effect_update_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                    )
                {
                    if effect_rewrite.bytes_inserted != 0 {
                        changed = true;
                        summary.update_records_rewritten =
                            summary.update_records_rewritten.saturating_add(1);
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(effect_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                        );
                    }
                    let mut advanced_effect_cursor = bit_cursor;
                    if record::advance_verified_update_record_for_ee(
                        &live_bytes,
                        offset,
                        record_end,
                        &fragment_bits,
                        &mut advanced_effect_cursor,
                    ) {
                        bit_cursor = advanced_effect_cursor;
                        last_verified_creature_4408_record_end = None;
                        offset = record_end.max(offset + 1);
                        continue;
                    }
                }

                if let Some(visual_rewrite) =
                    appearance::rewrite_creature_visual_transform_update_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.creature_visual_transform_update_records = summary
                        .creature_visual_transform_update_records
                        .saturating_add(1);
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(visual_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                    );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(visual_rewrite.bytes_removed).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary.bits_inserted.saturating_add(
                        u32::try_from(visual_rewrite.bits_inserted).unwrap_or(u32::MAX),
                    );
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add((visual_rewrite.bytes_removed != 0) as u32);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(visual_rewrite.bytes_removed).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted =
                        summary.interleaved_fragment_bits_promoted.saturating_add(
                            u32::try_from(visual_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                }
                if appearance::is_verified_ee_creature_visual_transform_update_record(
                    &live_bytes,
                    offset,
                    record_end,
                ) {
                    last_verified_creature_4408_record_end = None;
                    offset = record_end.max(offset + 1);
                    continue;
                }
                if creature::rewrite_3967_bare_second_identity_string_for_ee(
                    &mut live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    bit_cursor,
                )
                .is_some()
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                }
                if let Some(omitted_action_code_rewrite) =
                    creature::insert_3967_hg_action_ffff_omitted_code_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(omitted_action_code_rewrite.bytes_inserted)
                            .unwrap_or(u32::MAX),
                    );
                }
                if let Some(action_ffff_rewrite) =
                    creature::remove_3967_action_ffff_legacy_bridge_followup_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(action_ffff_rewrite.bytes_removed).unwrap_or(u32::MAX),
                    );
                }
                if let Some(action0_rewrite) =
                    creature::remove_3967_action0_legacy_bridge_followup_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(action0_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                    );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(action0_rewrite.bytes_removed).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary.bits_inserted.saturating_add(
                        u32::try_from(action0_rewrite.bits_inserted).unwrap_or(u32::MAX),
                    );
                }
                if let Some(action0_damage_rewrite) =
                    creature::insert_3967_action0_missing_damage_byte_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(action0_damage_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                    );
                }
                if let Some(action0_associate_rewrite) =
                    creature::insert_3967_action0_short_associate_suffix_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(action0_associate_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                    );
                }
                if creature::repair_3967_action2_optional_float_bool_for_ee(
                    &live_bytes,
                    offset,
                    record_end,
                    &mut fragment_bits,
                    bit_cursor,
                ) {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                }
                if creature::repair_legacy_c408_visual_effect_count_for_ee(
                    &mut live_bytes,
                    offset,
                    record_end,
                )
                .or_else(|| {
                    creature::repair_legacy_4408_visual_effect_count_for_ee(
                        &mut live_bytes,
                        offset,
                        record_end,
                    )
                })
                .or_else(|| {
                    creature::repair_legacy_effect_only_visual_effect_count_for_ee(
                        &mut live_bytes,
                        offset,
                        record_end,
                    )
                })
                .is_some()
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                }
                if let Some(status_effect_rewrite) =
                    creature::insert_creature_update_status_effect_identity_maps_for_ee(
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &fragment_bits,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.update_records_rewritten =
                        summary.update_records_rewritten.saturating_add(1);
                    summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                        u32::try_from(status_effect_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                    );
                }
                if let Some(promotion) =
                    fragment_spans::promote_effect_only_creature_update_following_gui_fragment_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        offset,
                        &mut record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted =
                        summary.interleaved_fragment_bits_promoted.saturating_add(
                            u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                        );
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    bit_cursor = promotion.end_bit_cursor;
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = true;
                    last_verified_creature_4408_record_end = None;
                    last_verified_creature_effect_only_record_end = Some(record_end);
                    last_verified_creature_0047_fragment_prefix_record_end = None;
                    offset = record_end.max(offset + 1);
                    continue;
                }
                if let Some(promotion) =
                    fragment_spans::promote_legacy_creature_update_large_interleaved_fragment_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        offset,
                        &mut record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    let promoted_creature_mask = read_u32_le(&live_bytes, offset + 6);
                    bit_cursor = promotion.end_bit_cursor;
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = true;
                    last_verified_creature_4408_record_end =
                        (promoted_creature_mask == Some(0x0000_4408)).then_some(record_end);
                    last_verified_creature_effect_only_record_end =
                        (promoted_creature_mask == Some(0x0000_0008)).then_some(record_end);
                    last_verified_creature_0047_fragment_prefix_record_end =
                        (promoted_creature_mask == Some(0x0000_0047)).then_some(record_end);
                    offset = record_end.max(offset + 1);
                    continue;
                }
                if let Some(promotion) =
                    fragment_spans::promote_creature_update_interleaved_fragment_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        offset,
                        &mut record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    let promoted_creature_mask = read_u32_le(&live_bytes, offset + 6);
                    bit_cursor = promotion.end_bit_cursor;
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = true;
                    last_verified_creature_4408_record_end =
                        (promoted_creature_mask == Some(0x0000_4408)).then_some(record_end);
                    last_verified_creature_effect_only_record_end =
                        (promoted_creature_mask == Some(0x0000_0008)).then_some(record_end);
                    last_verified_creature_0047_fragment_prefix_record_end =
                        (promoted_creature_mask == Some(0x0000_0047)).then_some(record_end);
                    offset = record_end.max(offset + 1);
                    continue;
                }
                let mut advanced_cursor = bit_cursor;
                if creature::advance_verified_noop_creature_update_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut advanced_cursor,
                ) {
                    bit_cursor = advanced_cursor;
                    pending_creature_p_tail_repair = None;
                    last_verified_record_end = record_end;
                    last_verified_record_allows_trailing_fragment_promotion = true;
                    let verified_creature_mask = read_u32_le(&live_bytes, offset + 6);
                    last_verified_creature_4408_record_end =
                        (verified_creature_mask == Some(0x0000_4408)).then_some(record_end);
                    last_verified_creature_effect_only_record_end =
                        (verified_creature_mask == Some(0x0000_0008)).then_some(record_end);
                    last_verified_creature_0047_fragment_prefix_record_end =
                        (verified_creature_mask == Some(0x0000_0047)).then_some(record_end);
                } else if let Some(pending) = pending_creature_p_tail_repair.as_ref() {
                    if let Some(repair) = tail_repair::try_repair_for_creature_update(
                        pending,
                        &mut live_bytes,
                        offset,
                        &mut record_end,
                        &mut fragment_bits,
                        bit_cursor,
                    ) {
                        changed = true;
                        summary.interleaved_fragment_spans_promoted = summary
                            .interleaved_fragment_spans_promoted
                            .saturating_add(1);
                        if repair.new_bits_len >= repair.old_bits_len {
                            summary.bits_inserted = summary.bits_inserted.saturating_add(
                                u32::try_from(repair.new_bits_len - repair.old_bits_len)
                                    .unwrap_or(u32::MAX),
                            );
                        } else {
                            summary.bits_removed = summary.bits_removed.saturating_add(
                                u32::try_from(repair.old_bits_len - repair.new_bits_len)
                                    .unwrap_or(u32::MAX),
                            );
                        }
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(repair.bytes_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bytes_removed = summary.bytes_removed.saturating_add(
                            u32::try_from(repair.bytes_removed).unwrap_or(u32::MAX),
                        );
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(repair.bits_inserted).unwrap_or(u32::MAX),
                        );
                        bit_cursor = repair.bit_cursor;
                        pending_creature_p_tail_repair = None;
                        last_verified_creature_4408_record_end = None;
                        last_verified_creature_effect_only_record_end = None;
                        last_verified_creature_0047_fragment_prefix_record_end = None;
                    } else {
                        trace_update_rewrite_cursor_unreliable(
                            "creature-update-cursor-advance-failed",
                            &live_bytes,
                            offset,
                            record_end,
                            bit_cursor,
                        );
                        bit_cursor_reliable = false;
                    }
                } else {
                    trace_update_rewrite_cursor_unreliable(
                        "creature-update-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode == b'G' {
            if bit_cursor_reliable {
                if let Some(promotion) = gui::promote_legacy_live_gui_item_fragment_span_for_ee(
                    &mut live_bytes,
                    &mut fragment_bits,
                    offset,
                    &mut record_end,
                    bit_cursor,
                ) {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                }

                if let Some(gui_rewrite) = gui::insert_ee_live_gui_item_extras_for_ee(
                    &mut live_bytes,
                    offset,
                    &mut record_end,
                    &mut fragment_bits,
                    bit_cursor,
                ) {
                    if gui_rewrite.bits_inserted != 0
                        || gui_rewrite.bits_removed != 0
                        || gui_rewrite.bytes_inserted != 0
                        || gui_rewrite.bytes_removed != 0
                        || gui_rewrite.missing_add_inner_opcodes_repaired != 0
                    {
                        changed = true;
                        summary.bits_inserted = summary.bits_inserted.saturating_add(
                            u32::try_from(gui_rewrite.bits_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bits_removed = summary.bits_removed.saturating_add(
                            u32::try_from(gui_rewrite.bits_removed).unwrap_or(u32::MAX),
                        );
                        summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                            u32::try_from(gui_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                        );
                        summary.bytes_removed = summary.bytes_removed.saturating_add(
                            u32::try_from(gui_rewrite.bytes_removed).unwrap_or(u32::MAX),
                        );
                        summary.live_gui_missing_add_opcodes_repaired = summary
                            .live_gui_missing_add_opcodes_repaired
                            .saturating_add(
                                u32::try_from(gui_rewrite.missing_add_inner_opcodes_repaired)
                                    .unwrap_or(u32::MAX),
                            );
                    }
                }

                if gui::advance_verified_live_gui_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                )
                .is_none()
                {
                    trace_update_rewrite_cursor_unreliable(
                        "live-gui-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                } else if let Some(bytes_removed) =
                    gui::remove_zero_fragment_storage_after_verified_live_gui_item_record_for_ee(
                        &mut live_bytes,
                        record_end,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted = summary
                        .interleaved_fragment_bytes_promoted
                        .saturating_add(u32::try_from(bytes_removed).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary
                        .bytes_removed
                        .saturating_add(u32::try_from(bytes_removed).unwrap_or(u32::MAX));
                }
            }
            last_verified_record_end = record_end;
            last_verified_record_allows_trailing_fragment_promotion = false;
            offset = record_end.max(offset + 1);
            continue;
        }

        if inventory::owns_fragment_tail(opcode) {
            if let Some(inventory_rewrite) = inventory::rewrite_legacy_inventory_record_for_ee(
                &mut live_bytes,
                offset,
                &mut record_end,
            ) {
                changed = true;
                summary.bytes_inserted = summary.bytes_inserted.saturating_add(
                    u32::try_from(inventory_rewrite.bytes_inserted).unwrap_or(u32::MAX),
                );
                summary.bytes_removed = summary.bytes_removed.saturating_add(
                    u32::try_from(inventory_rewrite.bytes_removed).unwrap_or(u32::MAX),
                );
            }

            if bit_cursor_reliable {
                if let Some(promotion) =
                    fragment_spans::promote_inventory_interleaved_fragment_span_for_ee(
                        &mut live_bytes,
                        &mut fragment_bits,
                        offset,
                        &mut record_end,
                        bit_cursor,
                    )
                {
                    changed = true;
                    summary.interleaved_fragment_spans_promoted = summary
                        .interleaved_fragment_spans_promoted
                        .saturating_add(1);
                    summary.interleaved_fragment_bytes_promoted =
                        summary.interleaved_fragment_bytes_promoted.saturating_add(
                            u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                        );
                    summary.interleaved_fragment_bits_promoted = summary
                        .interleaved_fragment_bits_promoted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                    summary.bytes_removed = summary.bytes_removed.saturating_add(
                        u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                    );
                    summary.bits_inserted = summary
                        .bits_inserted
                        .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
                }

                if inventory::advance_verified_inventory_record(
                    &live_bytes,
                    offset,
                    record_end,
                    &fragment_bits,
                    &mut bit_cursor,
                )
                .is_none()
                {
                    if last_verified_creature_0047_fragment_prefix_record_end == Some(offset) {
                        if let Some(promotion) = fragment_spans::
                            promote_creature_0047_following_add_boundary_collision_fragment_span_for_ee(
                                &mut live_bytes,
                                &mut fragment_bits,
                                offset,
                                record_end,
                                bit_cursor,
                            )
                        {
                            changed = true;
                            summary.interleaved_fragment_spans_promoted = summary
                                .interleaved_fragment_spans_promoted
                                .saturating_add(1);
                            summary.interleaved_fragment_bytes_promoted =
                                summary.interleaved_fragment_bytes_promoted.saturating_add(
                                    u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                                );
                            summary.interleaved_fragment_bits_promoted =
                                summary.interleaved_fragment_bits_promoted.saturating_add(
                                    u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                                );
                            summary.bytes_removed = summary.bytes_removed.saturating_add(
                                u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX),
                            );
                            summary.bits_inserted = summary.bits_inserted.saturating_add(
                                u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX),
                            );
                            last_verified_record_allows_trailing_fragment_promotion = false;
                            last_verified_creature_0047_fragment_prefix_record_end = None;
                            continue;
                        }
                    }
                    trace_update_rewrite_cursor_unreliable(
                        "inventory-cursor-advance-failed",
                        &live_bytes,
                        offset,
                        record_end,
                        bit_cursor,
                    );
                    bit_cursor_reliable = false;
                }
            }
            last_verified_record_end = record_end;
            last_verified_creature_4408_record_end = None;
            last_verified_creature_effect_only_record_end = None;
            last_verified_creature_0047_fragment_prefix_record_end = None;
            offset = record_end.max(offset + 1);
            continue;
        }

        if opcode != b'U'
            || !matches!(
                object_type,
                TRIGGER_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE | DOOR_OBJECT_TYPE | ITEM_OBJECT_TYPE
            )
            || record_end < offset + LEGACY_UPDATE_HEADER_BYTES
        {
            offset = record_end.max(offset + 1);
            continue;
        }

        summary.update_records_examined = summary.update_records_examined.saturating_add(1);
        let Some(record_rewrite) = record::rewrite_update_record_for_ee(
            &mut live_bytes,
            &mut record_end,
            &mut fragment_bits,
            &mut bit_cursor,
            &mut bit_cursor_reliable,
            offset,
        ) else {
            if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                eprintln!(
                    "live-object update rewrite skipped: offset={offset} record_end={record_end} bit_cursor={bit_cursor} bit_cursor_reliable={bit_cursor_reliable} opcode=0x{:02X} marker=0x{:02X} mask={:?}",
                    live_bytes.get(offset).copied().unwrap_or_default(),
                    live_bytes.get(offset + 1).copied().unwrap_or_default(),
                    read_u32_le(&live_bytes, offset + 6).map(|mask| format!("0x{mask:08X}"))
                );
            }
            offset = record_end.max(offset + 1);
            continue;
        };

        if record_rewrite.rewritten {
            if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
                eprintln!(
                    "live-object update record rewrite applied: offset={offset} record_end={record_end} bit_cursor={bit_cursor} rewrite={record_rewrite:?}"
                );
            }
            changed = true;
            summary.update_records_rewritten = summary.update_records_rewritten.saturating_add(1);
            if record_rewrite.mask_changed {
                summary.masks_translated = summary.masks_translated.saturating_add(1);
            }
            summary.bytes_inserted = summary
                .bytes_inserted
                .saturating_add(record_rewrite.bytes_inserted);
            summary.bytes_removed = summary
                .bytes_removed
                .saturating_add(record_rewrite.bytes_removed);
            summary.bits_inserted = summary
                .bits_inserted
                .saturating_add(record_rewrite.bits_inserted);
            summary.bits_removed = summary
                .bits_removed
                .saturating_add(record_rewrite.bits_removed);
        }
        last_verified_record_end = record_end;
        last_verified_record_allows_trailing_fragment_promotion = false;
        offset = record_end.max(offset + 1);
    }

    if summary.records_examined > 0
        && last_verified_record_allows_trailing_fragment_promotion
        && last_verified_record_end < live_bytes.len()
    {
        if let Some(promotion) =
            fragment_spans::promote_trailing_fragment_prefix_after_verified_record_for_ee(
                &mut live_bytes,
                &mut fragment_bytes,
                &mut fragment_bits,
                last_verified_record_end,
            )
        {
            changed = true;
            summary.interleaved_fragment_spans_promoted = summary
                .interleaved_fragment_spans_promoted
                .saturating_add(1);
            summary.interleaved_fragment_bytes_promoted = summary
                .interleaved_fragment_bytes_promoted
                .saturating_add(u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX));
            summary.interleaved_fragment_bits_promoted = summary
                .interleaved_fragment_bits_promoted
                .saturating_add(u32::try_from(promotion.bits_promoted).unwrap_or(u32::MAX));
            summary.bytes_removed = summary
                .bytes_removed
                .saturating_add(u32::try_from(promotion.bytes_promoted).unwrap_or(u32::MAX));
        }
    }

    if bit_cursor_reliable
        && bit_cursor >= CNW_FRAGMENT_HEADER_BITS
        && bit_cursor < fragment_bits.len()
    {
        summary.fragment_bits_trimmed = (fragment_bits.len() - bit_cursor) as u32;
        fragment_bits.truncate(bit_cursor);
    }

    if let Some((offset, legal_end)) = terminal_work_remaining_fragment_storage_record {
        if let Some(bytes_removed) =
            remove_terminal_work_remaining_fragment_storage_with_final_claim(
                &mut live_bytes,
                &fragment_bits,
                offset,
                legal_end,
            )
        {
            changed = true;
            summary.bytes_removed = summary
                .bytes_removed
                .saturating_add(u32::try_from(bytes_removed).unwrap_or(u32::MAX));
        }
    }

    if !changed {
        return None;
    }

    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_some() {
        eprintln!(
            "live-object update rewrite summary before emit: bit_cursor_reliable={bit_cursor_reliable} bit_cursor={bit_cursor} fragment_bits={} live_bytes={} summary={summary:?}",
            fragment_bits.len(),
            live_bytes.len(),
        );
    }

    let fragment_bytes = bits::pack_msb_valid_bits(fragment_bits, CNW_FRAGMENT_HEADER_BITS);
    let new_declared_usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + live_bytes.len();
    let new_declared = u32::try_from(new_declared_usize).ok()?;
    let new_payload_length = new_declared_usize.checked_add(fragment_bytes.len())?;
    if new_payload_length > MAX_REASONABLE_LIVE_PAYLOAD_BYTES {
        return None;
    }

    let mut rewritten = Vec::with_capacity(new_payload_length);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&live_bytes);
    rewritten.extend_from_slice(&fragment_bytes);

    summary.new_declared = new_declared;
    summary.new_payload_length = rewritten.len();
    summary.new_live_bytes_length = live_bytes.len();
    summary.new_fragment_bytes = fragment_bytes.len();
    *payload = rewritten;
    Some(summary)
}

fn verified_work_remaining_record_legal_end(live_bytes: &[u8], offset: usize) -> Option<usize> {
    let legal_end = offset.checked_add(3)?;
    (legal_end <= live_bytes.len()
        && world_status::is_verified_work_remaining_record(live_bytes, offset, legal_end))
    .then_some(legal_end)
}

fn remove_midstream_work_remaining_fragment_storage_after_top_level_record_for_ee(
    live_bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
) -> Option<usize> {
    // Only the top-level live-object boundary loop may call this. `W`-shaped
    // bytes inside appearance, GUI, inventory, or item bodies remain owned by
    // those nested record parsers.
    let legal_end = verified_work_remaining_record_legal_end(live_bytes, offset)?;
    if *record_end <= legal_end || *record_end >= live_bytes.len() {
        return None;
    }
    if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, *record_end)
        || !looks_like_bounded_cnw_fragment_storage_span(&live_bytes[legal_end..*record_end])
    {
        return None;
    }

    let removed = *record_end - legal_end;
    trace_work_remaining_fragment_storage_removed(
        "midstream-top-level",
        offset,
        legal_end,
        *record_end,
        removed,
    );
    live_bytes.drain(legal_end..*record_end);
    *record_end = legal_end;
    Some(removed)
}

fn remove_terminal_work_remaining_fragment_storage_with_final_claim(
    live_bytes: &mut Vec<u8>,
    fragment_bits: &[bool],
    offset: usize,
    legal_end: usize,
) -> Option<usize> {
    if verified_work_remaining_record_legal_end(live_bytes, offset)? != legal_end {
        return None;
    }
    if legal_end >= live_bytes.len()
        || !looks_like_bounded_cnw_fragment_storage_span(&live_bytes[legal_end..])
    {
        return None;
    }

    let mut candidate = live_bytes.clone();
    let removed = candidate.len().saturating_sub(legal_end);
    candidate.truncate(legal_end);
    let payload = live_object_payload_from_parts(&candidate, fragment_bits)?;
    claim_payload_if_verified(&payload)?;
    trace_work_remaining_fragment_storage_removed(
        "terminal",
        offset,
        legal_end,
        live_bytes.len(),
        removed,
    );
    *live_bytes = candidate;
    Some(removed)
}

fn trace_work_remaining_fragment_storage_removed(
    kind: &'static str,
    offset: usize,
    legal_end: usize,
    record_end: usize,
    removed: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object work-remaining fragment-storage removed: kind={kind} offset={offset} legal_end={legal_end} record_end={record_end} bytes_removed={removed}",
    );
}

fn looks_like_bounded_cnw_fragment_storage_span(span: &[u8]) -> bool {
    const MAX_WORK_REMAINING_DUPLICATE_FRAGMENT_STORAGE_BYTES: usize = 64;

    if span.is_empty() || span.len() > MAX_WORK_REMAINING_DUPLICATE_FRAGMENT_STORAGE_BYTES {
        return false;
    }
    bits::decode_msb_valid_bits(span, CNW_FRAGMENT_HEADER_BITS)
        .is_some_and(|decoded| decoded.len() > CNW_FRAGMENT_HEADER_BITS)
}

fn live_object_payload_from_parts(live_bytes: &[u8], fragment_bits: &[bool]) -> Option<Vec<u8>> {
    let fragment_bytes =
        bits::pack_msb_valid_bits(fragment_bits.to_vec(), CNW_FRAGMENT_HEADER_BITS);
    let declared_usize = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(live_bytes.len())?;
    let declared = u32::try_from(declared_usize).ok()?;
    let payload_length = declared_usize.checked_add(fragment_bytes.len())?;
    if payload_length > MAX_REASONABLE_LIVE_PAYLOAD_BYTES {
        return None;
    }

    let mut payload = Vec::with_capacity(payload_length);
    payload.extend_from_slice(&[
        HIGH_LEVEL_ENVELOPE,
        GAME_OBJECT_UPDATE_MAJOR,
        LIVE_OBJECT_MINOR,
    ]);
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(live_bytes);
    payload.extend_from_slice(&fragment_bytes);
    Some(payload)
}

fn trace_claim_accept(
    family: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object claim accepted: family={family} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X}",
        live_bytes.get(offset).copied().unwrap_or_default(),
        live_bytes.get(offset + 1).copied().unwrap_or_default()
    );
}

fn trace_update_rewrite_cursor_unreliable(
    reason: &'static str,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    let preview_end = record_end
        .min(live_bytes.len())
        .min(offset.saturating_add(40));
    let preview = live_bytes.get(offset..preview_end).unwrap_or(&[]);
    eprintln!(
        "live-object update rewrite cursor unreliable: reason={reason} offset={offset} record_end={record_end} bit_cursor={bit_cursor} opcode=0x{:02X} marker=0x{:02X} preview={:02X?}",
        live_bytes.get(offset).copied().unwrap_or_default(),
        live_bytes.get(offset + 1).copied().unwrap_or_default(),
        preview
    );
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Option<f32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    bytes
        .get_mut(offset..offset + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

/// Dump a live-object payload that was accepted or rejected by a semantic
/// ownership probe.
///
/// This is intentionally environment-gated and lives beside the live-object
/// translator rather than as ad-hoc test output.  The strict bridge treats a
/// no-op live-object claim as provisional evidence: captured payloads should be
/// promoted into fixtures and then either assigned to an exact translator or
/// rejected. These probe dumps are written below the diagnostics subdirectory
/// instead of the quarantine root so rejected split candidates do not look like
/// packets the proxy actually refused to emit. Set `NWN_BRIDGE_QUARANTINE_DIR`
/// to enable capture. The older `HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR` name is
/// still accepted as a fallback.
pub fn dump_live_object_fixture_candidate(payload: &[u8], reason: &str) {
    let Some(dir) = crate::translate::diagnostics::probe_dump_dir() else {
        return;
    };

    let sanitized_reason: String = reason
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let key = LiveObjectFixtureDumpKey {
        reason: sanitized_reason.clone(),
        signature: live_object_fixture_dump_signature(payload, &sanitized_reason),
    };
    let keys = LIVE_OBJECT_FIXTURE_DUMP_KEYS.get_or_init(|| Mutex::new(HashSet::new()));
    match keys.lock() {
        Ok(mut keys) => {
            if !keys.insert(key) {
                tracing::debug!(
                    reason = sanitized_reason,
                    payload_length = payload.len(),
                    "duplicate live-object fixture candidate dump suppressed"
                );
                return;
            }
        }
        Err(error) => {
            tracing::warn!(
                %error,
                reason = sanitized_reason,
                payload_length = payload.len(),
                "live-object fixture candidate dump dedupe state poisoned; skipping diagnostic dump"
            );
            return;
        }
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut path = dir;
    if std::fs::create_dir_all(&path).is_err() {
        return;
    }
    path.push(format!(
        "{}-len{}-{}.bin",
        sanitized_reason,
        payload.len(),
        nanos
    ));
    let _ = std::fs::write(path, payload);
}

/// Return true when a live-object payload contains door/placeable add or update
/// records that require a family-specific semantic translator.
///
/// Decompile evidence: EE routes these through dedicated readers
/// (`sub_140796DD0` / `sub_140797780` for doors and `sub_1407A7800` /
/// `sub_1407A8460` for placeables). Even when a payload is otherwise
/// structurally walkable, the generic no-op claimed-record path must not own
/// these records because Diamond/HG streams can omit or compact EE-only visual,
/// orientation, and name-mode fields.
pub fn payload_contains_door_or_placeable_add_update_record(payload: &[u8]) -> bool {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != GAME_OBJECT_UPDATE_MAJOR
        || payload[2] != LIVE_OBJECT_MINOR
    {
        return false;
    }

    let Some(declared) =
        read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES || declared > payload.len() {
        return false;
    }

    let live_bytes = &payload[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..declared];
    let mut offset = 0usize;
    while offset + 2 <= live_bytes.len() {
        if !boundary::looks_like_legacy_live_object_sub_message_boundary(live_bytes, offset) {
            offset += 1;
            continue;
        }

        let opcode = live_bytes[offset];
        let object_type = live_bytes[offset + 1];
        if matches!(opcode, b'A' | b'U')
            && matches!(object_type, DOOR_OBJECT_TYPE | PLACEABLE_OBJECT_TYPE)
        {
            return true;
        }

        let next = boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live_bytes,
            offset,
            live_bytes.len(),
        );
        offset = next.max(offset + 1);
    }

    false
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod claimed_records_guard_tests {
    use super::*;

    #[test]
    fn door_mixed_add_update_payload_must_not_use_generic_claimed_records() {
        let payload = include_bytes!(
            "../../fixtures/live_object/hg_door_mixed_add_update_claimed_records.bin"
        );
        assert!(payload_contains_door_or_placeable_add_update_record(
            payload
        ));
    }

    #[test]
    fn placeable_mixed_add_update_payload_must_not_use_generic_claimed_records() {
        let payload = include_bytes!(
            "../../fixtures/live_object/hg_placeable_mixed_add_update_claimed_records.bin"
        );
        assert!(payload_contains_door_or_placeable_add_update_record(
            payload
        ));
    }
}
