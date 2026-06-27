//! Inflated gameplay stream splitting.
//!
//! The reliable `M` layer inflates zlib envelopes, but the resulting bytes are
//! not always "one packet equals one semantic message". A window may contain a
//! complete `P major minor` high-level message, a continuation of a previously
//! classified zlib stream, or bytes that are still waiting for a later fragment.
//!
//! This module is deliberately pure: it classifies inflated byte ranges and can
//! rejoin translated units, but it does not mutate packets or decide semantic
//! ownership. Semantic translators still live in focused packet-family modules.

use crate::packet::m::{HighLevel, MAX_REASONABLE_GAMEPLAY_PAYLOAD};

use super::VerifiedFamily;

#[derive(Debug, Clone, Copy)]
pub enum GameplayUnit<'a> {
    HighLevel(HighLevelMessage<'a>),
    Continuation(&'a [u8]),
    PendingFragment(&'a [u8]),
    Unknown(&'a [u8]),
}

#[derive(Debug, Clone, Copy)]
pub struct HighLevelMessage<'a> {
    pub offset: usize,
    pub envelope: u8,
    pub major: u8,
    pub minor: u8,
    pub payload: &'a [u8],
    pub declared: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum TranslatedGameplayUnit {
    Owned {
        family: VerifiedFamily,
        bytes: Vec<u8>,
    },
    TransportOnly(Vec<u8>),
    Quarantined {
        reason: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct SplitResult<T> {
    pub units: T,
    pub complete: bool,
}

pub fn split_inflated_gameplay(bytes: &[u8]) -> SplitResult<Vec<GameplayUnit<'_>>> {
    if bytes.is_empty() {
        return SplitResult {
            units: Vec::new(),
            complete: true,
        };
    }

    let mut offset = 0usize;
    let mut units = Vec::new();
    let mut complete = true;

    while offset < bytes.len() {
        let Some(high) = HighLevel::parse(&bytes[offset..]) else {
            let tail = &bytes[offset..];
            if offset == 0 {
                units.push(GameplayUnit::Continuation(tail));
            } else {
                units.push(GameplayUnit::Unknown(tail));
            }
            complete = false;
            break;
        };

        let Some(end) = high_level_unit_end(bytes, offset, high) else {
            units.push(GameplayUnit::PendingFragment(&bytes[offset..]));
            complete = false;
            break;
        };

        let declared = declared_cnw_length(bytes, offset, high);
        units.push(GameplayUnit::HighLevel(HighLevelMessage {
            offset,
            envelope: high.envelope,
            major: high.major,
            minor: high.minor,
            payload: &bytes[offset..end],
            declared,
        }));

        if end <= offset {
            units.push(GameplayUnit::Unknown(&bytes[offset..]));
            complete = false;
            break;
        }
        offset = end;
    }

    SplitResult { units, complete }
}

fn high_level_unit_end(bytes: &[u8], offset: usize, high: HighLevel) -> Option<usize> {
    if let Some(length) = fixed_high_level_length(high) {
        return offset.checked_add(length).filter(|end| *end <= bytes.len());
    }

    let declared = declared_cnw_length(bytes, offset, high)?;
    if declared < 3 || declared > MAX_REASONABLE_GAMEPLAY_PAYLOAD {
        return None;
    }

    let candidates = declared_read_end_candidates(bytes, offset, declared);
    if candidates.is_empty() {
        return None;
    }
    for read_end in &candidates {
        // CNW declared lengths bound the read-buffer window, not the trailing
        // compact BOOL fragment storage. Fragment bytes can legitimately begin
        // with `0x50` (`P`) and arbitrary major/minor-looking bytes; do not
        // split there unless the following span proves a complete high-level
        // unit of its own.
        if *read_end == bytes.len() || boundary_has_plausible_unit(bytes, *read_end) {
            return Some(*read_end);
        }
    }

    if is_live_object_high_level(high) {
        // `GameObjUpdate_LiveObject` is the dangerous CNW family for stream
        // splitting: the decompiled EE live-object reader seeds
        // `CNWMessage::SetReadMessage` from this declared read-window, then
        // continues through packed fragment BOOL bytes. Legacy/HG coalesced
        // spans can carry stale short declarations, and the read/fragment body
        // legitimately contains `0x70` bytes inside object ids or item
        // subobjects (for example `A <object id 0x800170C7>`). Those bytes must
        // not be promoted to a top-level EE high-level boundary. Keep the
        // whole remaining live-object blob together so the focused
        // declared-length repair and exact live-object validator can either
        // claim it or quarantine it.
        return Some(bytes.len());
    }

    // CNW read-buffer lengths do not describe the compact BOOL fragment tail.
    // When multiple gameplay messages are concatenated, the next `P major minor`
    // begins after that fragment tail. We only split at a later offset if that
    // offset itself can start a bounded high-level unit; otherwise this message
    // conservatively owns the remaining bytes.
    let scan_start = candidates
        .iter()
        .copied()
        .min()
        .unwrap_or(offset)
        .saturating_add(1);
    find_next_plausible_high_level_boundary(bytes, scan_start).or(Some(bytes.len()))
}

fn fixed_high_level_length(high: HighLevel) -> Option<usize> {
    match (high.major, high.minor) {
        (0x01, 0x00 | 0x01)
        | (0x02, 0x05 | 0x0C)
        | (0x03, 0x02)
        | (0x04, 0x03)
        | (0x11, 0x01)
        | (0x31, 0x01 | 0x02) => Some(3),
        _ => None,
    }
}

fn is_live_object_high_level(high: HighLevel) -> bool {
    high.major == 0x05 && high.minor == 0x01
}

fn declared_cnw_length(bytes: &[u8], offset: usize, high: HighLevel) -> Option<usize> {
    if fixed_high_level_length(high).is_some() {
        return None;
    }
    let start = offset.checked_add(3)?;
    let end = start.checked_add(4)?;
    let slice: [u8; 4] = bytes.get(start..end)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(slice)).ok()
}

fn declared_read_end_candidates(bytes: &[u8], offset: usize, declared: usize) -> Vec<usize> {
    let mut candidates = Vec::with_capacity(2);
    if let Some(end) = offset.checked_add(declared) {
        if end <= bytes.len() {
            candidates.push(end);
        }
    }
    if let Some(end) = offset
        .checked_add(4)
        .and_then(|base| base.checked_add(declared))
    {
        if end <= bytes.len() && !candidates.contains(&end) {
            candidates.push(end);
        }
    }
    candidates
}

fn find_next_plausible_high_level_boundary(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < bytes.len() {
        if HighLevel::parse(&bytes[cursor..]).is_some()
            && boundary_has_plausible_unit(bytes, cursor)
        {
            return Some(cursor);
        }
        cursor = cursor.saturating_add(1);
    }
    None
}

fn boundary_has_plausible_unit(bytes: &[u8], offset: usize) -> bool {
    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return false;
    };
    if let Some(length) = fixed_high_level_length(high) {
        return offset
            .checked_add(length)
            .map(|end| end <= bytes.len())
            .unwrap_or(false);
    }
    declared_cnw_length(bytes, offset, high)
        .filter(|declared| *declared >= 3 && *declared <= MAX_REASONABLE_GAMEPLAY_PAYLOAD)
        .map(|declared| !declared_read_end_candidates(bytes, offset, declared).is_empty())
        .unwrap_or(false)
}

pub fn rejoin_translated_units(units: &[TranslatedGameplayUnit]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for unit in units {
        match unit {
            TranslatedGameplayUnit::Owned { bytes, .. }
            | TranslatedGameplayUnit::TransportOnly(bytes) => out.extend_from_slice(bytes),
            TranslatedGameplayUnit::Quarantined { .. } => return None,
        }
    }
    Some(out)
}

pub fn translate_units<'a, F>(
    units: Vec<GameplayUnit<'a>>,
    mut translate_high_level: F,
) -> Vec<TranslatedGameplayUnit>
where
    F: FnMut(HighLevelMessage<'a>) -> TranslatedGameplayUnit,
{
    units
        .into_iter()
        .map(|unit| match unit {
            GameplayUnit::HighLevel(message) => translate_high_level(message),
            GameplayUnit::Continuation(bytes) | GameplayUnit::PendingFragment(bytes) => {
                TranslatedGameplayUnit::TransportOnly(bytes.to_vec())
            }
            GameplayUnit::Unknown(bytes) => TranslatedGameplayUnit::Quarantined {
                reason: if bytes.is_empty() {
                    "empty-unknown-gameplay-unit"
                } else {
                    "unknown-gameplay-unit"
                },
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_client_and_server_status_no_body_messages() {
        let bytes = [b'P', 0x01, 0x00, b'P', 0x01, 0x01];
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x00);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected client ServerStatus_0 unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 3);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected server ServerStatus_Status unit"),
        }
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod private_fixture_tests {
    use super::*;

    #[test]
    fn splits_fixed_empty_high_level_messages() {
        let bytes = [b'P', 0x03, 0x02, b'P', 0x04, 0x03];
        let split = split_inflated_gameplay(&bytes);
        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x03);
                assert_eq!(message.minor, 0x02);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected first high-level unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 3);
                assert_eq!(message.major, 0x04);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected second high-level unit"),
        }
    }

    #[test]
    fn splits_declared_message_before_next_high_level() {
        let mut bytes = vec![b'P', 0x03, 0x03];
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&[b'P', 0x04, 0x03]);
        let split = split_inflated_gameplay(&bytes);
        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.declared, Some(7));
                assert_eq!(message.payload.len(), 7);
            }
            _ => panic!("expected declared high-level unit"),
        }
    }

    #[test]
    fn live_object_fragment_tail_starting_with_p_is_not_split_as_high_level() {
        let bytes = include_bytes!(
            "../../fixtures/live_object/hg_area_entry_door_signs_mixed_liveobject.bin"
        );
        let split = split_inflated_gameplay(bytes);
        assert!(split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.major, 0x05);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.declared, Some(600));
                assert_eq!(message.payload.len(), bytes.len());
            }
            _ => panic!("expected one live-object high-level unit"),
        }
    }

    #[test]
    fn short_declared_live_object_with_embedded_0x70_object_id_stays_one_unit() {
        let bytes = include_bytes!(
            "../../fixtures/live_object/hg_starc5_seq39_creature_add_coalesced_unclaimed.bin"
        );
        let split = split_inflated_gameplay(bytes);
        assert!(split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.major, 0x05);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.declared, Some(0x56));
                assert_eq!(message.payload.len(), bytes.len());
            }
            _ => panic!("expected one live-object high-level unit"),
        }
    }
}
