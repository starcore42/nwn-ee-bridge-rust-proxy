//! World-status live-object record normalization.
//!
//! `W` records are not object updates, but they can be coalesced into the same
//! live-object byte stream. Keep their tiny transport repair out of the update
//! record translator so `record.rs` stays semantic and typed.

pub(super) fn normalize_record_for_ee(
    bytes: &mut Vec<u8>,
    record_offset: usize,
    record_end: &mut usize,
) -> Option<usize> {
    if *record_end < record_offset + 3
        || bytes.get(record_offset).copied() != Some(b'W')
        || bytes.get(record_offset + 1).copied().unwrap_or(0xFF) > 0x0F
        || bytes.get(record_offset + 2).copied() != Some(0x0E)
    {
        return None;
    }

    let legal_end = record_offset + 3;
    let removed = record_end.saturating_sub(legal_end);
    if removed != 0 {
        bytes.drain(legal_end..*record_end);
        *record_end = legal_end;
    }
    Some(removed)
}
