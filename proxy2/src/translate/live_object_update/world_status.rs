//! World-status live-object record normalization and exact identity claim.
//!
//! `W` records are not object updates, but they can be coalesced into the same
//! live-object byte stream. Keep their tiny transport repair out of the update
//! record translator so `record.rs` stays semantic and typed.
//!
//! Decompile anchors:
//!
//! - Diamond/EE server writer `CNWSMessage::WriteGameObjUpdate_WorkRemaining`
//!   writes exactly `CHAR 'W'`, then two `BYTE` counters.
//! - EE client reader `sub_1407B85A0`
//!   (`HandleServerToPlayerUpdate_WorkRemaining`) is dispatched after the live
//!   object opcode byte has already been consumed and then performs exactly two
//!   `ReadBYTE(8, 1)` calls.
//!
//! There is therefore no dialect byte rewrite here. The semantic transform is a
//! verified identity translation: parse the exact `W current total` shape, leave
//! it unchanged, and consume no CNW fragment bits.

const WORK_REMAINING_OPCODE: u8 = b'W';
const WORK_REMAINING_RECORD_BYTES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WorkRemainingRecord {
    pub current: u8,
    pub total: u8,
}

pub(super) fn parse_work_remaining_record(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> Option<WorkRemainingRecord> {
    if record_end != record_offset.checked_add(WORK_REMAINING_RECORD_BYTES)?
        || record_end > bytes.len()
        || bytes.get(record_offset).copied()? != WORK_REMAINING_OPCODE
    {
        return None;
    }

    Some(WorkRemainingRecord {
        current: bytes[record_offset + 1],
        total: bytes[record_offset + 2],
    })
}

pub(super) fn is_verified_work_remaining_record(
    bytes: &[u8],
    record_offset: usize,
    record_end: usize,
) -> bool {
    parse_work_remaining_record(bytes, record_offset, record_end).is_some()
}

pub(super) fn normalize_record_for_ee(
    bytes: &mut Vec<u8>,
    record_offset: usize,
    record_end: &mut usize,
) -> Option<usize> {
    if *record_end < record_offset.checked_add(WORK_REMAINING_RECORD_BYTES)?
        || bytes.get(record_offset).copied() != Some(WORK_REMAINING_OPCODE)
    {
        return None;
    }

    let legal_end = record_offset + WORK_REMAINING_RECORD_BYTES;
    let removed = record_end.saturating_sub(legal_end);
    if removed != 0 {
        bytes.drain(legal_end..*record_end);
        *record_end = legal_end;
    }
    Some(removed)
}
