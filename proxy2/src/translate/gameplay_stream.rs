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

use super::{VerifiedFamily, chat, client_side_message, journal, loadbar, party, player_list};

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
    match focused_high_level_unit_end(bytes, offset, high) {
        FocusedUnitEnd::Exact(end) => return Some(end),
        FocusedUnitEnd::Invalid => return None,
        FocusedUnitEnd::NotFocused => {}
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusedUnitEnd {
    Exact(usize),
    Invalid,
    NotFocused,
}

fn focused_high_level_unit_end(bytes: &[u8], offset: usize, high: HighLevel) -> FocusedUnitEnd {
    match (high.major, high.minor) {
        (0x09, _) => focused_chat_unit_end(bytes, offset),
        (0x0A, 0x01..=0x03) => focused_player_list_unit_end(bytes, offset),
        (0x0E, _) => focused_party_unit_end(bytes, offset),
        (0x12, 0x0B) => focused_client_side_message_unit_end(bytes, offset),
        (0x1C, _) => focused_journal_unit_end(bytes, offset),
        (0x2C, 0x01..=0x03) => {
            let Some(declared) = declared_cnw_length(bytes, offset, high) else {
                return FocusedUnitEnd::Invalid;
            };
            let Some(end) = offset
                .checked_add(declared)
                .and_then(|end| end.checked_add(1))
            else {
                return FocusedUnitEnd::Invalid;
            };
            let Some(payload) = bytes.get(offset..end) else {
                return FocusedUnitEnd::Invalid;
            };
            if loadbar::claim_payload_if_verified(payload).is_some() {
                FocusedUnitEnd::Exact(end)
            } else {
                FocusedUnitEnd::Invalid
            }
        }
        _ => FocusedUnitEnd::NotFocused,
    }
}

fn focused_player_list_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_PLAYER_LIST_FRAGMENT_BYTES: usize = 128;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_PLAYER_LIST_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            let mut probe = payload.to_vec();
            if player_list::rewrite_player_list_payload_if_possible(&mut probe).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_party_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_PARTY_FRAGMENT_BYTES: usize = 32;

    if let Some(end) = offset.checked_add(3) {
        if let Some(payload) = bytes.get(offset..end) {
            if party::claim_client_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_PARTY_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if party::claim_server_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_journal_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_JOURNAL_FRAGMENT_BYTES: usize = 8;

    if let Some(end) = offset.checked_add(3) {
        if let Some(payload) = bytes.get(offset..end) {
            if journal::claim_client_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_JOURNAL_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if journal::claim_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_client_side_message_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_CLIENT_SIDE_MESSAGE_FRAGMENT_BYTES: usize = 64;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_CLIENT_SIDE_MESSAGE_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            let mut probe = payload.to_vec();
            if client_side_message::claim_or_rewrite_payload_if_verified(&mut probe).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_chat_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_CHAT_FRAGMENT_BYTES: usize = 16;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_CHAT_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if chat::claim_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn fixed_high_level_length(high: HighLevel) -> Option<usize> {
    match (high.major, high.minor) {
        (0x01, 0x00 | 0x01)
        | (0x02, 0x05 | 0x0C)
        | (0x03, 0x02)
        | (0x04, 0x03)
        | (0x0E, 0x02)
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
    match focused_high_level_unit_end(bytes, offset, high) {
        FocusedUnitEnd::Exact(_) => return true,
        FocusedUnitEnd::Invalid => return false,
        FocusedUnitEnd::NotFocused => {}
    }
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

    #[test]
    fn splits_client_party_get_list_no_body_signal() {
        let bytes = [b'P', 0x0E, 0x02, b'P', 0x04, 0x03];
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x0E);
                assert_eq!(message.minor, 0x02);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected client Party_GetList unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 3);
                assert_eq!(message.major, 0x04);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected client Area_AreaLoaded unit"),
        }
    }

    #[test]
    fn splits_party_list_with_focused_fragment_tail_owner() {
        let mut bytes = party_list_payload(&[0x8000_0001, 0x8000_0002]);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x0E);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected Party_List unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, status_offset);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected ServerStatus_Status unit"),
        }
    }

    #[test]
    fn rejects_shifted_party_list_before_following_status() {
        let mut bytes = party_list_payload(&[0x8000_0001]);
        *bytes.last_mut().expect("fragment tail") = 0x80;
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted Party_List row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_chat_strref_with_focused_fragment_tail_owner() {
        let mut bytes = chat_talk_ref_payload();
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x09);
                assert_eq!(message.minor, 0x08);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected Chat_TalkRef unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, status_offset);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected ServerStatus_Status unit"),
        }
    }

    #[test]
    fn rejects_shifted_chat_strref_tail_before_following_status() {
        let mut bytes = chat_talk_ref_payload();
        *bytes.last_mut().expect("fragment tail") = 0x80;
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted Chat row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_client_side_feedback_with_focused_fragment_tail_owner() {
        let mut bytes = client_side_feedback_payload(b"abcdefghijklmnop");
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x12);
                assert_eq!(message.minor, 0x0B);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected ClientSideMessage_Feedback unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, status_offset);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected ServerStatus_Status unit"),
        }
    }

    #[test]
    fn rejects_oversized_client_side_feedback_tail_before_following_status() {
        let mut bytes = client_side_feedback_payload(b"abcdefghijklmnop");
        bytes.extend_from_slice(&[0; 65]);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected oversized ClientSideMessage tail to remain unclaimed"),
        }
    }

    #[test]
    fn splits_player_list_delete_with_focused_fragment_tail_owner() {
        let mut bytes = player_list_delete_payload(0x8000_0001, 0x80);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x0A);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected PlayerList_Delete unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, status_offset);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected ServerStatus_Status unit"),
        }
    }

    #[test]
    fn rejects_shifted_player_list_delete_before_following_status() {
        let mut bytes = player_list_delete_payload(0x8000_0001, 0xA0);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted PlayerList_Delete row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_loadbar_with_focused_fragment_tail_owner() {
        let mut bytes = loadbar::start_payload(2);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x2C);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected LoadBar_Start unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, status_offset);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected ServerStatus_Status unit"),
        }
    }

    #[test]
    fn rejects_shifted_loadbar_end_before_following_status() {
        let mut bytes = loadbar::end_success_payload(2);
        *bytes.last_mut().expect("fragment tail") = 0x60;
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted LoadBar_End to remain unclaimed"),
        }
    }

    #[test]
    fn splits_journal_with_focused_fragment_tail_owner() {
        let mut bytes = journal_delete_world_payload(7);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x1C);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected Journal_DeleteWorld unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, status_offset);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected ServerStatus_Status unit"),
        }
    }

    #[test]
    fn rejects_shifted_journal_tail_before_following_status() {
        let mut bytes = journal_delete_world_payload(7);
        *bytes.last_mut().expect("fragment tail") = 0x80;
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted Journal row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_client_journal_no_body_controls() {
        let bytes = [b'P', 0x1C, 0x0A, b'P', 0x1C, 0x0B];
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x1C);
                assert_eq!(message.minor, 0x0A);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected Journal quest-screen-open unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 3);
                assert_eq!(message.major, 0x1C);
                assert_eq!(message.minor, 0x0B);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected Journal quest-screen-closed unit"),
        }
    }

    fn journal_delete_world_payload(entry_id: u32) -> Vec<u8> {
        let declared = 3 + 4 + 4;
        let mut payload = vec![b'P', 0x1C, 0x03];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&entry_id.to_le_bytes());
        payload.push(0x60);
        payload
    }

    fn party_list_payload(member_ids: &[u32]) -> Vec<u8> {
        let declared = 3 + 4 + 4 + member_ids.len() * 4;
        let mut payload = vec![b'P', 0x0E, 0x01];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&(member_ids.len() as u32).to_le_bytes());
        for member_id in member_ids {
            payload.extend_from_slice(&member_id.to_le_bytes());
        }
        payload.push(0x60);
        payload
    }

    fn chat_talk_ref_payload() -> Vec<u8> {
        vec![
            0x50, 0x09, 0x08, 0x0F, 0x00, 0x00, 0x00, 0x31, 0x12, 0x00, 0x80, 0xEC, 0x47, 0x01,
            0x00, 0x60,
        ]
    }

    fn client_side_feedback_payload(text: &[u8]) -> Vec<u8> {
        let declared = 3 + 4 + 2 + 4 + text.len();
        let mut payload = vec![b'P', 0x12, 0x0B];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&0x00CCu16.to_le_bytes());
        payload.extend_from_slice(&(text.len() as u32).to_le_bytes());
        payload.extend_from_slice(text);
        payload.push(0x60);
        payload
    }

    fn player_list_delete_payload(player_id: u32, fragment_tail: u8) -> Vec<u8> {
        let declared = 3 + 4 + 4;
        let mut payload = vec![b'P', 0x0A, 0x03];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&player_id.to_le_bytes());
        payload.push(fragment_tail);
        payload
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
