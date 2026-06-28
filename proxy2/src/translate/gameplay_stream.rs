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

use super::{
    VerifiedFamily, area, char_list, chat, client_char_list, client_side_message, custom_token,
    game_obj_update, inventory, journal, loadbar, module, module_resources, module_time, party,
    player_list, quickbar, server_status, sound,
};

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
        (0x01, 0x01) => focused_server_status_status_unit_end(bytes, offset),
        (0x01, 0x03) => focused_module_resources_unit_end(bytes, offset),
        (0x09, _) => focused_chat_unit_end(bytes, offset),
        (0x0A, 0x01..=0x03) => focused_player_list_unit_end(bytes, offset),
        (0x0C, 0x01 | 0x02) => focused_inventory_unit_end(bytes, offset),
        (0x03, 0x01) => focused_module_info_unit_end(bytes, offset),
        (0x03, 0x03) => focused_module_time_unit_end(bytes, offset),
        (0x04, 0x01) => focused_area_client_area_unit_end(bytes, offset),
        (0x05, 0x02 | 0x03 | 0x07) => focused_game_obj_update_unit_end(bytes, offset),
        (0x0E, _) => focused_party_unit_end(bytes, offset),
        (0x11, 0x01 | 0x03) => focused_client_char_list_unit_end(bytes, offset),
        (0x11, 0x02 | 0x04) => focused_char_list_unit_end(bytes, offset),
        (0x12, 0x0B) => focused_client_side_message_unit_end(bytes, offset),
        (0x17, 0x03) => focused_sound_unit_end(bytes, offset),
        (0x1C, _) => focused_journal_unit_end(bytes, offset),
        (0x1E, 0x01) => focused_quickbar_unit_end(bytes, offset),
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
        (0x32, 0x01 | 0x02) => focused_custom_token_unit_end(bytes, offset),
        _ => FocusedUnitEnd::NotFocused,
    }
}

fn focused_module_resources_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_MODULE_RESOURCES_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_MODULE_RESOURCES_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if module_resources::claim_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_server_status_status_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    let Some(end) = offset.checked_add(3) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(payload) = bytes.get(offset..end) else {
        return FocusedUnitEnd::Invalid;
    };

    if server_status::claim_status_payload_if_verified(payload).is_some()
        && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
    {
        FocusedUnitEnd::Exact(end)
    } else {
        FocusedUnitEnd::Invalid
    }
}

fn focused_module_time_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_MODULE_TIME_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_MODULE_TIME_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if module_time::claim_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_area_client_area_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    let mut candidate_ends = Vec::new();
    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        if read_end <= offset || read_end > bytes.len() {
            continue;
        }

        let mut cursor = read_end.saturating_add(1);
        while cursor < bytes.len() {
            if HighLevel::parse(&bytes[cursor..]).is_some()
                && boundary_has_plausible_unit(bytes, cursor)
            {
                candidate_ends.push(cursor);
            }
            cursor = cursor.saturating_add(1);
        }
    }
    candidate_ends.push(bytes.len());
    candidate_ends.sort_unstable();
    candidate_ends.dedup();

    for end in candidate_ends {
        let Some(payload) = bytes.get(offset..end) else {
            continue;
        };
        if area::claim_or_rewrite_payload_if_verified(payload)
            && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
        {
            return FocusedUnitEnd::Exact(end);
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_module_info_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_MODULE_INFO_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    let mut read_window_verified = false;
    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        if let Some(read_window) = bytes.get(offset..read_end) {
            read_window_verified |= module::module_info_read_window_shape_valid(read_window);
        }

        for fragment_bytes in 0..=MAX_MODULE_INFO_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if module::claim_module_info_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    if read_window_verified {
        FocusedUnitEnd::Invalid
    } else {
        FocusedUnitEnd::NotFocused
    }
}

fn focused_game_obj_update_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_GAME_OBJ_UPDATE_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_GAME_OBJ_UPDATE_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if game_obj_update::claim_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_sound_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_SOUND_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_SOUND_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if sound::claim_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_custom_token_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_CUSTOM_TOKEN_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    let Some(read_end) = offset.checked_add(declared) else {
        return FocusedUnitEnd::Invalid;
    };
    for fragment_bytes in 0..=MAX_CUSTOM_TOKEN_FRAGMENT_BYTES {
        let Some(end) = read_end.checked_add(fragment_bytes) else {
            break;
        };
        let Some(payload) = bytes.get(offset..end) else {
            break;
        };
        let mut probe = payload.to_vec();
        if custom_token::claim_or_rewrite_payload_if_verified(&mut probe).is_some()
            && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
        {
            return FocusedUnitEnd::Exact(end);
        }
    }

    FocusedUnitEnd::Invalid
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

fn focused_inventory_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_INVENTORY_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_INVENTORY_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            let mut probe = payload.to_vec();
            if inventory::claim_or_rewrite_payload_if_verified(&mut probe).is_some()
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

fn focused_client_char_list_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_CLIENT_CHAR_LIST_FRAGMENT_BYTES: usize = 1;

    if let Some(end) = offset.checked_add(3) {
        if let Some(payload) = bytes.get(offset..end) {
            if client_char_list::claim_payload_if_verified(payload).is_some()
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
        for fragment_bytes in 0..=MAX_CLIENT_CHAR_LIST_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            if client_char_list::claim_payload_if_verified(payload).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_char_list_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_CHAR_LIST_FRAGMENT_BYTES: usize = 64;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in 0..=MAX_CHAR_LIST_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            let Some(payload) = bytes.get(offset..end) else {
                break;
            };
            let mut probe = payload.to_vec();
            if char_list::claim_payload_if_verified(&mut probe).is_some()
                && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
            {
                return FocusedUnitEnd::Exact(end);
            }
        }
    }

    FocusedUnitEnd::Invalid
}

fn focused_quickbar_unit_end(bytes: &[u8], offset: usize) -> FocusedUnitEnd {
    const MAX_QUICKBAR_FRAGMENT_BYTES: usize = 512;
    const QUICKBAR_LEGACY_PREFIX_BYTES: usize = 3 + 4;
    const QUICKBAR_SLOT_BYTES: usize = 36;
    const QUICKBAR_MIN_FRAGMENT_BYTES: usize = 1;

    let Some(high) = HighLevel::parse(&bytes[offset..]) else {
        return FocusedUnitEnd::Invalid;
    };
    let Some(declared) = declared_cnw_length(bytes, offset, high) else {
        return FocusedUnitEnd::Invalid;
    };

    let mut candidate_ends = Vec::new();
    for read_end in declared_read_end_candidates(bytes, offset, declared) {
        for fragment_bytes in QUICKBAR_MIN_FRAGMENT_BYTES..=MAX_QUICKBAR_FRAGMENT_BYTES {
            let Some(end) = read_end.checked_add(fragment_bytes) else {
                break;
            };
            if end > bytes.len() {
                break;
            }
            candidate_ends.push(end);
        }
    }

    let min_legacy_end = offset
        .checked_add(QUICKBAR_LEGACY_PREFIX_BYTES)
        .and_then(|end| end.checked_add(QUICKBAR_SLOT_BYTES))
        .and_then(|end| end.checked_add(QUICKBAR_MIN_FRAGMENT_BYTES));
    if let Some(min_legacy_end) = min_legacy_end {
        let max_legacy_end = min_legacy_end
            .checked_add(MAX_QUICKBAR_FRAGMENT_BYTES)
            .map(|end| end.min(bytes.len()))
            .unwrap_or(bytes.len());
        for end in min_legacy_end..=max_legacy_end {
            candidate_ends.push(end);
        }
    }

    candidate_ends.sort_unstable();
    candidate_ends.dedup();
    for end in candidate_ends {
        let Some(payload) = bytes.get(offset..end) else {
            continue;
        };
        if quickbar_payload_claims_complete_unit(payload)
            && (end == bytes.len() || boundary_has_plausible_unit(bytes, end))
        {
            return FocusedUnitEnd::Exact(end);
        }
    }

    FocusedUnitEnd::Invalid
}

fn quickbar_payload_claims_complete_unit(payload: &[u8]) -> bool {
    if quickbar::ee_set_all_buttons_payload_shape_valid(payload) {
        return true;
    }

    let mut probe = payload.to_vec();
    if let Some((_, summary)) =
        quickbar::normalize_and_rewrite_quickbar_payload_if_possible(&mut probe)
    {
        return !quickbar::rewrite_summary_needs_more_quickbar_bytes(&summary)
            && quickbar::ee_set_all_buttons_payload_shape_valid(&probe);
    }

    let mut probe = payload.to_vec();
    if let Some(summary) = quickbar::rewrite_simple_quickbar_payload_if_possible(&mut probe) {
        return !quickbar::rewrite_summary_needs_more_quickbar_bytes(&summary)
            && quickbar::ee_set_all_buttons_payload_shape_valid(&probe);
    }

    false
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
        (0x01, 0x00)
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
    fn rejects_server_status_status_tail_slack_before_following_status() {
        let bytes = [b'P', 0x01, 0x01, 0x00, b'P', 0x01, 0x01];
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected status with unowned slack to remain pending"),
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
    fn splits_module_resources_with_focused_fragment_owner() {
        let mut bytes = module_resources_payload();
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x01);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected ServerStatus_ModuleResources unit"),
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
    fn rejects_shifted_module_resources_tail_before_following_status() {
        let mut bytes = module_resources_payload();
        *bytes.last_mut().expect("fragment tail") = 0x60;
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted ServerStatus_ModuleResources row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_module_info_with_focused_fragment_tail_owner() {
        let mut bytes = module_info_payload(&[0xC0]);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x03);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected Module_Info unit"),
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
    fn rejects_shifted_module_info_tail_before_following_status() {
        let mut bytes = module_info_payload(&[0xA0]);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted Module_Info row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_module_time_with_focused_empty_fragment_owner() {
        let mut bytes = module_time_payload(0x02, &[0x12], &[0x60]);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x03);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected Module_Time unit"),
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
    fn rejects_shifted_module_time_tail_before_following_status() {
        let mut bytes = module_time_payload(0x02, &[0x12], &[0x80]);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted Module_Time row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_legacy_area_client_area_with_focused_rewrite_owner() {
        let mut bytes = area_client_area_legacy_payload();
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x04);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected rewrite-owned Area_ClientArea unit"),
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
    fn rejects_area_client_area_slack_before_following_status() {
        let mut bytes = area_client_area_ee_payload();
        bytes.push(0);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected slack-bearing Area_ClientArea row to remain unclaimed"),
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
    fn splits_client_char_list_request_update_with_focused_empty_cursor_owner() {
        let mut bytes = client_char_list_request_update_payload(&[0x60]);
        let area_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x04, 0x03]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x11);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), area_offset);
            }
            _ => panic!("expected ClientCharList_RequestUpdateChar unit"),
        }
        match split.units[1] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, area_offset);
                assert_eq!(message.major, 0x04);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), 3);
            }
            _ => panic!("expected client Area_AreaLoaded unit"),
        }
    }

    #[test]
    fn rejects_shifted_client_char_list_request_update_before_following_area() {
        let mut bytes = client_char_list_request_update_payload(&[0x80]);
        bytes.extend_from_slice(&[b'P', 0x04, 0x03]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted ClientCharList row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_char_list_response_with_focused_fragment_tail_owner() {
        let mut bytes = char_list_response_payload(&[0xA0]);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x11);
                assert_eq!(message.minor, 0x02);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected CharList_ListResponse unit"),
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
    fn rejects_shifted_char_list_response_before_following_status() {
        let mut bytes = char_list_response_payload(&[0xA8]);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted CharList_ListResponse row to remain unclaimed"),
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
    fn splits_inventory_equip_with_focused_fragment_tail_owner() {
        let mut bytes = inventory_equip_payload(0x8000_1234, 4, 0x90);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x0C);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected Inventory_Equip unit"),
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
    fn splits_legacy_inventory_equip_rewrite_shape_before_following_status() {
        let mut bytes = legacy_inventory_equip_payload(1, 0x8000_1234, 4, 0x90);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x0C);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected rewrite-owned Inventory_Equip unit"),
        }
    }

    #[test]
    fn rejects_shifted_inventory_equip_tail_before_following_status() {
        let mut bytes = inventory_equip_payload(0x8000_1234, 4, 0xA0);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted Inventory_Equip row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_game_obj_update_sibling_minors_with_focused_fragment_tail_owner() {
        let cases = [
            (
                game_obj_update_obj_control_payload(0x0000_0000, 0xFFFF_FFFE, 0x73),
                0x02,
            ),
            (
                game_obj_update_vis_effect_payload(
                    0x8000_1234,
                    0x0025,
                    [15.955528f32, 24.014782f32, 0.0f32],
                    0x61,
                ),
                0x03,
            ),
            (
                game_obj_update_destroy_item_payload(0x8000_2C67, 0x7C),
                0x07,
            ),
        ];

        for (mut bytes, minor) in cases {
            let status_offset = bytes.len();
            bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
            let split = split_inflated_gameplay(&bytes);

            assert!(split.complete, "minor {minor:#04x} should split cleanly");
            assert_eq!(split.units.len(), 2);
            match split.units[0] {
                GameplayUnit::HighLevel(message) => {
                    assert_eq!(message.offset, 0);
                    assert_eq!(message.major, 0x05);
                    assert_eq!(message.minor, minor);
                    assert_eq!(message.payload.len(), status_offset);
                }
                _ => panic!("expected focused GameObjUpdate sibling unit"),
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
    }

    #[test]
    fn rejects_shifted_game_obj_update_tail_before_following_status() {
        let mut bytes = game_obj_update_obj_control_payload(0, 0xFFFF_FFFE, 0x53);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted GameObjUpdate row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_sound_object_stop_with_focused_empty_cursor_owner() {
        let mut bytes = sound_object_stop_payload(0x8000_0247, 0x76);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x17);
                assert_eq!(message.minor, 0x03);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected Sound_Object_Stop unit"),
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
    fn rejects_shifted_sound_object_stop_tail_before_following_status() {
        let mut bytes = sound_object_stop_payload(0x8000_0247, 0x80);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected shifted Sound_Object_Stop row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_quickbar_with_focused_fragment_tail_owner() {
        let mut bytes =
            quickbar::build_blank_set_all_buttons_payload(b'P').expect("blank quickbar payload");
        assert!(quickbar::ee_set_all_buttons_payload_shape_valid(&bytes));
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x1E);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected GuiQuickbar_SetAllButtons unit"),
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
    fn rejects_unowned_quickbar_slack_before_following_status() {
        let mut bytes =
            quickbar::build_blank_set_all_buttons_payload(b'P').expect("blank quickbar payload");
        bytes.push(0);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected slack-bearing GuiQuickbar row to remain unclaimed"),
        }
    }

    #[test]
    fn splits_custom_token_with_focused_fragment_tail_owner() {
        let mut bytes = custom_token_set_payload(0x1234, b"hello");
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x32);
                assert_eq!(message.minor, 0x01);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected SetCustomToken unit"),
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
    fn splits_rewrite_owned_custom_token_list_before_following_status() {
        let mut bytes = custom_token_list_payload_with_count(2, &[(0x1234, &b"a"[..])]);
        let status_offset = bytes.len();
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(split.complete);
        assert_eq!(split.units.len(), 2);
        match split.units[0] {
            GameplayUnit::HighLevel(message) => {
                assert_eq!(message.offset, 0);
                assert_eq!(message.major, 0x32);
                assert_eq!(message.minor, 0x02);
                assert_eq!(message.payload.len(), status_offset);
            }
            _ => panic!("expected rewrite-owned SetCustomTokenList unit"),
        }
    }

    #[test]
    fn rejects_oversized_custom_token_tail_before_following_status() {
        let mut bytes = custom_token_set_payload(0x1234, b"hello");
        bytes.push(0);
        bytes.extend_from_slice(&[b'P', 0x01, 0x01]);
        let split = split_inflated_gameplay(&bytes);

        assert!(!split.complete);
        assert_eq!(split.units.len(), 1);
        match split.units[0] {
            GameplayUnit::PendingFragment(payload) => assert_eq!(payload, bytes.as_slice()),
            _ => panic!("expected oversized SetCustomToken tail to remain unclaimed"),
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

    fn module_info_payload(fragment_tail: &[u8]) -> Vec<u8> {
        let mut payload = vec![b'P', 0x03, 0x01, 0, 0, 0, 0];
        push_string(&mut payload, "Path of Ascension CEP Legends");
        push_string(&mut payload, "Path of Ascension CEP Legends");
        payload.push(0x02);
        push_resref(&mut payload, "poa_mod");
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&0x8000_0001u32.to_le_bytes());
        push_string(&mut payload, "Armor Shop");
        payload.push(0);
        let declared = payload.len() as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(fragment_tail);
        payload
    }

    fn module_resources_payload() -> Vec<u8> {
        let runtime = module_resources::ModuleResourceRuntime::default();
        assert!(runtime.observe_legacy_module_info_resources(
            &["cep2_custom".to_string(), "cep2_top_v23".to_string()],
            Some("cep23_v1"),
        ));
        let (payload, _) = module_resources::build_server_status_module_resources_payload(
            &runtime,
            "Path of Ascension",
        )
        .expect("module-resource payload");
        assert!(module_resources::claim_payload_if_verified(&payload).is_some());
        payload
    }

    fn module_time_payload(mask: u8, body: &[u8], fragment_tail: &[u8]) -> Vec<u8> {
        let declared = 3 + 4 + 1 + body.len();
        let mut payload = vec![b'P', 0x03, 0x03];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.push(mask);
        payload.extend_from_slice(body);
        payload.extend_from_slice(fragment_tail);
        payload
    }

    fn area_client_area_ee_payload() -> Vec<u8> {
        let mut payload = area_client_area_legacy_payload();
        area::rewrite_area_client_area_payload(&mut payload)
            .expect("synthetic legacy Area_ClientArea should rewrite to EE shape");
        assert!(area::ee_area_client_area_payload_shape_valid(&payload));
        payload
    }

    fn area_client_area_legacy_payload() -> Vec<u8> {
        const AREA_NAME_READ_OFFSET: usize = 44;
        const LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET: usize = 27;
        const DIAMOND_LEGACY_AREA_NAME_BYTES: usize = 20;
        const LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END: usize = 96;
        const LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END: usize = 100;
        const LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END: usize = 104;
        const CNW_FRAGMENT_HEADER_BITS: usize = 3;
        const LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS: usize = 14;

        let mut payload = vec![b'P', 0x04, 0x01, 0, 0, 0, 0];
        payload.resize(LEGACY_AREA_OBJECT_ID_PAYLOAD_OFFSET, 0);
        payload.extend_from_slice(&0x8000_0042u32.to_le_bytes());
        push_resref(&mut payload, "testarea");

        pad_to_area_read_offset(&mut payload, AREA_NAME_READ_OFFSET);
        let mut name = [0u8; DIAMOND_LEGACY_AREA_NAME_BYTES];
        name[..8].copy_from_slice(b"TestArea");
        payload.extend_from_slice(&name);
        let name_end = AREA_NAME_READ_OFFSET + DIAMOND_LEGACY_AREA_NAME_BYTES;

        pad_to_area_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_WIDTH_BYTES_AFTER_NAME_END,
        );
        payload.extend_from_slice(&1u32.to_le_bytes());
        pad_to_area_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_HEIGHT_BYTES_AFTER_NAME_END,
        );
        payload.extend_from_slice(&1u32.to_le_bytes());
        pad_to_area_read_offset(
            &mut payload,
            name_end + LEGACY_AREA_TILESET_BYTES_AFTER_NAME_END,
        );
        push_resref(&mut payload, "ttr01");

        payload.extend_from_slice(&1u32.to_le_bytes()); // tile id
        payload.extend_from_slice(&0u32.to_le_bytes()); // orientation
        payload.extend_from_slice(&0u32.to_le_bytes()); // height
        payload.extend_from_slice(&0x000Cu16.to_le_bytes());
        payload.extend_from_slice(&[0, 0]); // source light bytes
        payload.extend_from_slice(&0u32.to_le_bytes()); // transition rows
        payload.extend_from_slice(&0u32.to_le_bytes()); // map-pin rows
        payload.extend_from_slice(&0u16.to_le_bytes()); // sound rows
        payload.extend_from_slice(&0u16.to_le_bytes()); // light-placeable rows
        payload.extend_from_slice(&0u16.to_le_bytes()); // static-placeable rows

        let declared = payload.len() as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(&encode_cnw_msb_payload_bits(&vec![
            false;
            LEGACY_AREA_LOAD_PRE_TILE_FRAGMENT_BITS - CNW_FRAGMENT_HEADER_BITS
        ]));

        assert!(area::claim_or_rewrite_payload_if_verified(&payload));
        payload
    }

    fn pad_to_area_read_offset(bytes: &mut Vec<u8>, read_offset: usize) {
        bytes.resize(3 + read_offset, 0);
    }

    fn encode_cnw_msb_payload_bits(payload_bits: &[bool]) -> Vec<u8> {
        const CNW_FRAGMENT_HEADER_BITS: usize = 3;

        let valid_bits = CNW_FRAGMENT_HEADER_BITS + payload_bits.len();
        let byte_count = valid_bits.div_ceil(8);
        let final_fragment_bits = valid_bits % 8;
        let mut fragment = vec![0u8; byte_count];

        for header_bit in 0..CNW_FRAGMENT_HEADER_BITS {
            if ((final_fragment_bits >> (CNW_FRAGMENT_HEADER_BITS - 1 - header_bit)) & 1) != 0 {
                set_cnw_msb_bit(&mut fragment, header_bit);
            }
        }
        for (payload_bit_index, value) in payload_bits.iter().enumerate() {
            if *value {
                set_cnw_msb_bit(&mut fragment, CNW_FRAGMENT_HEADER_BITS + payload_bit_index);
            }
        }

        fragment
    }

    fn set_cnw_msb_bit(fragment: &mut [u8], bit_index: usize) {
        fragment[bit_index / 8] |= 0x80 >> (bit_index % 8);
    }

    fn client_char_list_request_update_payload(fragment_tail: &[u8]) -> Vec<u8> {
        const DECLARED: usize = 3 + 4 + 1 + 16;

        let mut payload = vec![b'P', 0x11, 0x03];
        payload.extend_from_slice(&(DECLARED as u32).to_le_bytes());
        payload.push(0x05);
        payload.extend_from_slice(b"starcore-druid60");
        payload.extend_from_slice(fragment_tail);
        payload
    }

    fn char_list_response_payload(fragment_tail: &[u8]) -> Vec<u8> {
        let mut read = Vec::new();
        read.extend_from_slice(&1u16.to_le_bytes());
        push_string(&mut read, "Character 0");
        push_string(&mut read, "");
        push_resref(&mut read, "po_heurodis_");
        read.push(0);
        read.extend_from_slice(&4u16.to_le_bytes());
        push_resref(&mut read, "char00");
        read.push(1);
        read.extend_from_slice(&37u32.to_le_bytes());
        read.push(40);

        let declared = 3 + 4 + read.len();
        let mut payload = vec![b'P', 0x11, 0x02];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&read);
        payload.extend_from_slice(fragment_tail);
        payload
    }

    fn push_string(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn push_resref(out: &mut Vec<u8>, value: &str) {
        let mut resref = [0u8; 16];
        let bytes = value.as_bytes();
        let copy_len = bytes.len().min(resref.len());
        resref[..copy_len].copy_from_slice(&bytes[..copy_len]);
        out.extend_from_slice(&resref);
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

    fn inventory_equip_payload(object_id: u32, equip_slot: u32, fragment_tail: u8) -> Vec<u8> {
        let declared = 3 + 4 + 4 + 4;
        let mut payload = vec![b'P', 0x0C, 0x01];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.extend_from_slice(&equip_slot.to_le_bytes());
        payload.push(fragment_tail);
        payload
    }

    fn legacy_inventory_equip_payload(
        legacy_prefix: u32,
        object_id: u32,
        equip_slot: u32,
        fragment_tail: u8,
    ) -> Vec<u8> {
        let declared = 3 + 4 + 4 + 4 + 4;
        let mut payload = vec![b'P', 0x0C, 0x01];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&legacy_prefix.to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.extend_from_slice(&equip_slot.to_le_bytes());
        payload.push(fragment_tail);
        payload
    }

    fn game_obj_update_obj_control_payload(
        player_id: u32,
        object_id: u32,
        fragment_tail: u8,
    ) -> Vec<u8> {
        let declared = 3 + 4 + 4 + 4;
        let mut payload = vec![b'P', 0x05, 0x02];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&player_id.to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.push(fragment_tail);
        payload
    }

    fn game_obj_update_vis_effect_payload(
        object_id: u32,
        effect_id: u16,
        position: [f32; 3],
        fragment_tail: u8,
    ) -> Vec<u8> {
        let declared = 3 + 4 + 4 + 2 + 3 * 4;
        let mut payload = vec![b'P', 0x05, 0x03];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.extend_from_slice(&effect_id.to_le_bytes());
        for value in position {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(fragment_tail);
        payload
    }

    fn game_obj_update_destroy_item_payload(object_id: u32, fragment_tail: u8) -> Vec<u8> {
        let declared = 3 + 4 + 4;
        let mut payload = vec![b'P', 0x05, 0x07];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.push(fragment_tail);
        payload
    }

    fn sound_object_stop_payload(object_id: u32, fragment_tail: u8) -> Vec<u8> {
        let declared = 3 + 4 + 4;
        let mut payload = vec![b'P', 0x17, 0x03];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload.push(fragment_tail);
        payload
    }

    fn custom_token_set_payload(token_id: u32, value: &[u8]) -> Vec<u8> {
        let declared = 3 + 4 + 4 + 4 + value.len();
        let mut payload = vec![b'P', 0x32, 0x01];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&token_id.to_le_bytes());
        payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
        payload.extend_from_slice(value);
        payload.push(0x60);
        payload
    }

    fn custom_token_list_payload_with_count(count: u32, entries: &[(u32, &[u8])]) -> Vec<u8> {
        let body_len = 4 + entries
            .iter()
            .map(|(_, value)| 4 + 4 + value.len())
            .sum::<usize>();
        let declared = 3 + 4 + body_len;
        let mut payload = vec![b'P', 0x32, 0x02];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&count.to_le_bytes());
        for (token_id, value) in entries {
            payload.extend_from_slice(&token_id.to_le_bytes());
            payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
            payload.extend_from_slice(value);
        }
        payload.push(0x60);
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
        let mut bytes = vec![b'P', 0x28, 0x04];
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
