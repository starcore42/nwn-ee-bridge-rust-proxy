//! Journal packet semantic claims.
//!
//! The strict bridge does not treat a known opcode as safe by itself. Even
//! packet families that are byte-identical between Diamond and EE need a
//! focused translator module to claim the exact shape. The decompile reference
//! names high-level major `0x1C` as Journal. The simple world/quest metadata
//! writers are byte-identical between Diamond and EE, so this module's
//! translation is identity after exact cursor validation.
//!
//! Decompile anchors:
//! - `SendServerToPlayerJournalAddWorld`: INT id, CExoString tag,
//!   CExoString text, DWORD calendar day, DWORD time-of-day.
//! - `SendServerToPlayerJournalAddWorldStrref`: four DWORDs.
//! - `SendServerToPlayerJournalDeleteWorld`: one INT.
//! - `SendServerToPlayerJournalDeleteWorldStrref`: one DWORD.
//! - `SendServerToPlayerJournalDeleteWorldAll`: one BYTE value `1`.
//! - `SendServerToPlayerJournalRemoveQuest`: one CExoString.
//! - `SendServerToPlayerJournalSetQuestPicture`: CExoString tag, INT picture.
//! - `SendServerToPlayerJournalFullUpdate`/`FullUpdateNotNeeded`: the
//!   no-update path writes a zero-length CExoString read-window DWORD followed
//!   by the two final BOOL bits read by EE `sub_1407A2890`
//!   (`nwn ee decompile.txt:1850259`, `:1850816`, `:2900575`).
//!
//! `Journal_AddQuest` and populated `Journal_FullUpdate` quest rows should
//! each get a typed parser before being claimed. `Journal_Updated` is currently
//! claimed for the decompile-backed TLK/string-ref
//! `WriteCExoLocStringServer(..., 0)` branch followed by the two update BOOLs.
//!
//! Client-originated quest-screen status is a separate exact shape:
//! - EE client senders `sub_1407C13D0` and `sub_1407C13B0`
//!   (`nwn ee decompile.txt:2943724`, `nwn ee decompile.txt:2943702`) send
//!   family `0x1C`, minors `0x0A`/`0x0B`, with null data pointer and zero
//!   payload length, so the on-wire payload is only the three-byte high-level
//!   header.
//! - EE server reader `CNWSMessage::HandlePlayerToServerJournalMessage`
//!   (`nwn ee decompile.txt:1661679`, branches at `0x1404570B6` and
//!   `0x140457061`) reads no CNW fields for those minors before toggling the
//!   player's journal-open state.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const JOURNAL_MAJOR: u8 = 0x1C;
const JOURNAL_ADD_WORLD_MINOR: u8 = 0x01;
const JOURNAL_ADD_WORLD_STRREF_MINOR: u8 = 0x02;
const JOURNAL_DELETE_WORLD_MINOR: u8 = 0x03;
const JOURNAL_DELETE_WORLD_STRREF_MINOR: u8 = 0x04;
const JOURNAL_DELETE_WORLD_ALL_MINOR: u8 = 0x05;
const JOURNAL_REMOVE_QUEST_MINOR: u8 = 0x07;
const JOURNAL_SET_QUEST_PICTURE_MINOR: u8 = 0x08;
const JOURNAL_FULL_UPDATE_MINOR: u8 = 0x09;
const JOURNAL_QUEST_SCREEN_OPEN_MINOR: u8 = 0x0A;
const JOURNAL_QUEST_SCREEN_CLOSED_MINOR: u8 = 0x0B;
const JOURNAL_UPDATED_MINOR: u8 = 0x0C;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const BYTE_BYTES: usize = 1;
const DWORD_BYTES: usize = 4;
const MAX_JOURNAL_TITLE_BYTES: usize = 512;
const MAX_JOURNAL_STRING_BYTES: usize = 4096;
const MAX_JOURNAL_FRAGMENT_BYTES: usize = 8;
const FINAL_EMPTY_FRAGMENT_BYTE: u8 = 0x60;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const JOURNAL_FULL_UPDATE_NOT_NEEDED_FRAGMENT_BITS: usize = 2;
const JOURNAL_FULL_UPDATE_NOT_NEEDED_FRAGMENT_BYTE: u8 = 0xBB;
const JOURNAL_UPDATED_TLK_FRAGMENT_BITS: usize = 4;
const JOURNAL_UPDATED_INLINE_FRAGMENT_BITS: usize = 3;
const JOURNAL_UPDATED_LOCSTRING_HAS_TLK_REF_BIT: usize = CNW_FRAGMENT_HEADER_BITS;
const JOURNAL_UPDATED_LOCSTRING_CLIENT_TLK_BIT: usize = CNW_FRAGMENT_HEADER_BITS + 1;

#[derive(Debug, Clone, Copy)]
pub struct JournalClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub title_len: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<JournalClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (JOURNAL_MAJOR, JOURNAL_ADD_WORLD_MINOR) => claim_journal_add_world(payload, high.minor),
        (JOURNAL_MAJOR, JOURNAL_ADD_WORLD_STRREF_MINOR) => {
            claim_fixed_primitive_payload(payload, high.minor, 4 * DWORD_BYTES)
        }
        (JOURNAL_MAJOR, JOURNAL_DELETE_WORLD_MINOR | JOURNAL_DELETE_WORLD_STRREF_MINOR) => {
            claim_fixed_primitive_payload(payload, high.minor, DWORD_BYTES)
        }
        (JOURNAL_MAJOR, JOURNAL_DELETE_WORLD_ALL_MINOR) => {
            claim_journal_delete_world_all(payload, high.minor)
        }
        (JOURNAL_MAJOR, JOURNAL_REMOVE_QUEST_MINOR) => {
            claim_single_string_payload(payload, high.minor)
        }
        (JOURNAL_MAJOR, JOURNAL_SET_QUEST_PICTURE_MINOR) => {
            claim_journal_set_quest_picture(payload, high.minor)
        }
        (JOURNAL_MAJOR, JOURNAL_FULL_UPDATE_MINOR) => {
            claim_journal_full_update(payload, high.minor)
        }
        (JOURNAL_MAJOR, JOURNAL_UPDATED_MINOR) => claim_journal_updated(payload, high.minor),
        _ => None,
    }
}

pub fn claim_client_payload_if_verified(payload: &[u8]) -> Option<JournalClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (JOURNAL_MAJOR, JOURNAL_QUEST_SCREEN_OPEN_MINOR | JOURNAL_QUEST_SCREEN_CLOSED_MINOR) => {
            if payload.len() != HIGH_LEVEL_HEADER_BYTES {
                return None;
            }
            Some(JournalClaimSummary {
                minor: high.minor,
                declared: HIGH_LEVEL_HEADER_BYTES,
                title_len: 0,
                fragment_bytes: 0,
            })
        }
        _ => None,
    }
}

fn claim_journal_add_world(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    let declared = exact_declared_with_empty_fragment(payload)?;
    let mut cursor = READ_START;
    cursor = cursor.checked_add(DWORD_BYTES)?;
    let (next, first_len) = read_c_exo_string(payload, cursor, declared, MAX_JOURNAL_STRING_BYTES)?;
    cursor = next;
    let (next, second_len) =
        read_c_exo_string(payload, cursor, declared, MAX_JOURNAL_STRING_BYTES)?;
    cursor = next.checked_add(2 * DWORD_BYTES)?;
    if cursor != declared {
        return None;
    }
    Some(JournalClaimSummary {
        minor,
        declared,
        title_len: first_len.saturating_add(second_len),
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_fixed_primitive_payload(
    payload: &[u8],
    minor: u8,
    read_bytes: usize,
) -> Option<JournalClaimSummary> {
    let declared = exact_declared_with_empty_fragment(payload)?;
    if declared != READ_START.checked_add(read_bytes)? {
        return None;
    }
    Some(JournalClaimSummary {
        minor,
        declared,
        title_len: 0,
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_journal_delete_world_all(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    let declared = exact_declared_with_empty_fragment(payload)?;
    if declared != READ_START + BYTE_BYTES || payload.get(READ_START) != Some(&1) {
        return None;
    }
    Some(JournalClaimSummary {
        minor,
        declared,
        title_len: 0,
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_single_string_payload(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    let declared = exact_declared_with_empty_fragment(payload)?;
    let (cursor, len) = read_c_exo_string(payload, READ_START, declared, MAX_JOURNAL_STRING_BYTES)?;
    if cursor != declared {
        return None;
    }
    Some(JournalClaimSummary {
        minor,
        declared,
        title_len: len,
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_journal_set_quest_picture(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    let declared = exact_declared_with_empty_fragment(payload)?;
    let (cursor, len) = read_c_exo_string(payload, READ_START, declared, MAX_JOURNAL_STRING_BYTES)?;
    if cursor.checked_add(DWORD_BYTES)? != declared {
        return None;
    }
    Some(JournalClaimSummary {
        minor,
        declared,
        title_len: len,
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_journal_full_update(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    claim_journal_full_update_not_needed(payload, minor)
}

fn claim_journal_full_update_not_needed(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    let declared =
        exact_declared_with_fragment_bits(payload, JOURNAL_FULL_UPDATE_NOT_NEEDED_FRAGMENT_BITS)?;
    if declared != READ_START.checked_add(DWORD_BYTES)?
        || read_le_u32(payload, READ_START)? != 0
        || payload.get(declared) != Some(&JOURNAL_FULL_UPDATE_NOT_NEEDED_FRAGMENT_BYTE)
    {
        return None;
    }

    Some(JournalClaimSummary {
        minor,
        declared,
        title_len: 0,
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_journal_updated(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    claim_journal_updated_tlk_ref(payload, minor)
        .or_else(|| claim_journal_updated_inline_locstring(payload, minor))
}

fn claim_journal_updated_tlk_ref(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    let declared = exact_declared_with_fragment_bits(payload, JOURNAL_UPDATED_TLK_FRAGMENT_BITS)?;
    if declared != READ_START.checked_add(DWORD_BYTES)? {
        return None;
    }

    let fragment = payload.get(declared..)?;
    if fragment_bit(fragment, JOURNAL_UPDATED_LOCSTRING_HAS_TLK_REF_BIT)? != true
        || fragment_bit(fragment, JOURNAL_UPDATED_LOCSTRING_CLIENT_TLK_BIT)? != false
    {
        return None;
    }

    Some(JournalClaimSummary {
        minor,
        declared,
        title_len: 0,
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_journal_updated_inline_locstring(
    payload: &[u8],
    minor: u8,
) -> Option<JournalClaimSummary> {
    let declared =
        exact_declared_with_fragment_bits(payload, JOURNAL_UPDATED_INLINE_FRAGMENT_BITS)?;
    let fragment = payload.get(declared..)?;
    if fragment_bit(fragment, JOURNAL_UPDATED_LOCSTRING_HAS_TLK_REF_BIT)? {
        return None;
    }

    let title_len = usize::try_from(read_le_u32(payload, READ_START)?).ok()?;
    if title_len > MAX_JOURNAL_TITLE_BYTES {
        return None;
    }
    let title_start = READ_START.checked_add(CNW_LENGTH_BYTES)?;
    let title_end = title_start.checked_add(title_len)?;
    if title_end != declared {
        return None;
    }

    Some(JournalClaimSummary {
        minor,
        declared,
        title_len,
        fragment_bytes: payload.len() - declared,
    })
}

fn exact_declared_with_fragment_bits(payload: &[u8], data_bits: usize) -> Option<usize> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let required_bits = CNW_FRAGMENT_HEADER_BITS.checked_add(data_bits)?;
    let required_fragment_bytes = required_bits.checked_add(7)?.checked_div(8)?;
    if declared < READ_START
        || payload.len() != declared.checked_add(required_fragment_bytes)?
        || payload.len().saturating_sub(declared) > MAX_JOURNAL_FRAGMENT_BYTES
        || cnw_fragment_consumable_bits(payload.get(declared..)?)? != required_bits
    {
        return None;
    }
    Some(declared)
}

fn exact_declared_with_empty_fragment(payload: &[u8]) -> Option<usize> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START
        || payload.len() != declared + 1
        || payload[declared] != FINAL_EMPTY_FRAGMENT_BYTE
    {
        return None;
    }
    Some(declared)
}

fn cnw_fragment_consumable_bits(fragment: &[u8]) -> Option<usize> {
    let first = *fragment.first()?;
    let final_fragment_bits = ((first & 0xE0) >> 5) as usize;
    if final_fragment_bits == 0 {
        fragment.len().checked_mul(8)
    } else {
        fragment
            .len()
            .checked_sub(1)?
            .checked_mul(8)?
            .checked_add(final_fragment_bits)
    }
}

fn fragment_bit(fragment: &[u8], bit_index: usize) -> Option<bool> {
    if bit_index >= cnw_fragment_consumable_bits(fragment)? {
        return None;
    }
    let byte = *fragment.get(bit_index / 8)?;
    Some((byte & (0x80 >> (bit_index % 8))) != 0)
}

fn read_c_exo_string(
    payload: &[u8],
    cursor: usize,
    declared: usize,
    max_len: usize,
) -> Option<(usize, usize)> {
    if cursor > declared
        || declared > payload.len()
        || declared.saturating_sub(cursor) < DWORD_BYTES
    {
        return None;
    }
    let len = usize::try_from(read_le_u32(payload, cursor)?).ok()?;
    if len > max_len || len > declared.saturating_sub(cursor + DWORD_BYTES) {
        return None;
    }
    Some((cursor + DWORD_BYTES + len, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_decompile_backed_world_journal_shapes() {
        assert_claimed(
            &journal_add_world_payload(7, "tag", "text", 12, 34),
            JOURNAL_ADD_WORLD_MINOR,
        );
        assert_claimed(
            &journal_dwords_payload(JOURNAL_ADD_WORLD_STRREF_MINOR, &[1, 2, 3, 4]),
            JOURNAL_ADD_WORLD_STRREF_MINOR,
        );
        assert_claimed(
            &journal_dwords_payload(JOURNAL_DELETE_WORLD_MINOR, &[7]),
            JOURNAL_DELETE_WORLD_MINOR,
        );
        assert_claimed(
            &journal_dwords_payload(JOURNAL_DELETE_WORLD_STRREF_MINOR, &[99]),
            JOURNAL_DELETE_WORLD_STRREF_MINOR,
        );
        assert_claimed(
            &journal_delete_world_all_payload(),
            JOURNAL_DELETE_WORLD_ALL_MINOR,
        );
    }

    #[test]
    fn claims_decompile_backed_simple_quest_journal_shapes() {
        assert_claimed(
            &journal_string_payload(JOURNAL_REMOVE_QUEST_MINOR, "quest_tag"),
            JOURNAL_REMOVE_QUEST_MINOR,
        );
        assert_claimed(
            &journal_set_quest_picture_payload("quest_tag", 3),
            JOURNAL_SET_QUEST_PICTURE_MINOR,
        );
    }

    #[test]
    fn rejects_simple_journal_shape_without_exact_empty_fragment() {
        let mut payload = journal_dwords_payload(JOURNAL_DELETE_WORLD_MINOR, &[7]);
        *payload.last_mut().unwrap() = 0;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn claims_decompile_backed_journal_updated_tlk_ref_shape() {
        let payload = [
            b'P',
            JOURNAL_MAJOR,
            JOURNAL_UPDATED_MINOR,
            0x0B,
            0,
            0,
            0,
            0x16,
            0xE2,
            0,
            0,
            0xF4,
        ];

        let claim = claim_payload_if_verified(&payload).expect("claim");
        assert_eq!(claim.minor, JOURNAL_UPDATED_MINOR);
        assert_eq!(claim.declared, READ_START + DWORD_BYTES);
        assert_eq!(claim.title_len, 0);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn claims_decompile_backed_client_quest_screen_status_shapes() {
        for minor in [
            JOURNAL_QUEST_SCREEN_OPEN_MINOR,
            JOURNAL_QUEST_SCREEN_CLOSED_MINOR,
        ] {
            let payload = [b'P', JOURNAL_MAJOR, minor];
            let claim = claim_client_payload_if_verified(&payload).expect("claim");
            assert_eq!(claim.minor, minor);
            assert_eq!(claim.declared, HIGH_LEVEL_HEADER_BYTES);
            assert_eq!(claim.fragment_bytes, 0);

            let mut trailing = payload.to_vec();
            trailing.push(0);
            assert!(claim_client_payload_if_verified(&trailing).is_none());
            assert!(claim_payload_if_verified(&payload).is_none());
        }
    }

    #[test]
    fn claims_decompile_backed_journal_full_update_not_needed_shape() {
        let payload = [
            b'P',
            JOURNAL_MAJOR,
            JOURNAL_FULL_UPDATE_MINOR,
            0x0B,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            JOURNAL_FULL_UPDATE_NOT_NEEDED_FRAGMENT_BYTE,
        ];

        let claim = claim_payload_if_verified(&payload).expect("claim");
        assert_eq!(claim.minor, JOURNAL_FULL_UPDATE_MINOR);
        assert_eq!(claim.declared, READ_START + DWORD_BYTES);
        assert_eq!(claim.title_len, 0);
        assert_eq!(claim.fragment_bytes, 1);

        let mut wrong_fragment = payload;
        wrong_fragment[11] ^= 1;
        assert!(claim_payload_if_verified(&wrong_fragment).is_none());
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_contest_journal_full_update_not_needed_fixture_claims_exactly() {
        let payload = include_bytes!(
            "../../fixtures/journal/local_contest_journal_full_update_not_needed_20260523.bin"
        );
        let claim = claim_payload_if_verified(payload).expect("claim");
        assert_eq!(claim.minor, JOURNAL_FULL_UPDATE_MINOR);
        assert_eq!(claim.declared + claim.fragment_bytes, payload.len());
    }

    #[test]
    fn claims_decompile_backed_journal_updated_inline_locstring_shape() {
        let mut payload = journal_prefix(JOURNAL_UPDATED_MINOR);
        write_string(&mut payload, "Quest advanced");
        let declared = payload.len() as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload.push(0xC8);

        let claim = claim_payload_if_verified(&payload).expect("claim");
        assert_eq!(claim.minor, JOURNAL_UPDATED_MINOR);
        assert_eq!(
            claim.declared,
            READ_START + DWORD_BYTES + "Quest advanced".len()
        );
        assert_eq!(claim.title_len, "Quest advanced".len());
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_journal_updated_tlk_ref_without_exact_fragment_shape() {
        let mut payload = [
            b'P',
            JOURNAL_MAJOR,
            JOURNAL_UPDATED_MINOR,
            0x0B,
            0,
            0,
            0,
            0x16,
            0xE2,
            0,
            0,
            0xF4,
        ];

        payload[11] = 0xE4;
        assert!(claim_payload_if_verified(&payload).is_none());

        let mut trailing = payload.to_vec();
        trailing[11] = 0xF4;
        trailing.push(0);
        assert!(claim_payload_if_verified(&trailing).is_none());
    }

    fn assert_claimed(payload: &[u8], minor: u8) {
        assert!(
            claim_payload_if_verified(payload).is_some_and(|claim| {
                claim.minor == minor && claim.declared + claim.fragment_bytes == payload.len()
            }),
            "journal minor {minor:#04x} should be claimed"
        );
    }

    fn journal_add_world_payload(
        entry_id: i32,
        tag: &str,
        text: &str,
        calendar_day: u32,
        time_of_day: u32,
    ) -> Vec<u8> {
        let mut payload = journal_prefix(JOURNAL_ADD_WORLD_MINOR);
        payload.extend_from_slice(&entry_id.to_le_bytes());
        write_string(&mut payload, tag);
        write_string(&mut payload, text);
        payload.extend_from_slice(&calendar_day.to_le_bytes());
        payload.extend_from_slice(&time_of_day.to_le_bytes());
        finish(payload)
    }

    fn journal_dwords_payload(minor: u8, values: &[u32]) -> Vec<u8> {
        let mut payload = journal_prefix(minor);
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        finish(payload)
    }

    fn journal_delete_world_all_payload() -> Vec<u8> {
        let mut payload = journal_prefix(JOURNAL_DELETE_WORLD_ALL_MINOR);
        payload.push(1);
        finish(payload)
    }

    fn journal_string_payload(minor: u8, value: &str) -> Vec<u8> {
        let mut payload = journal_prefix(minor);
        write_string(&mut payload, value);
        finish(payload)
    }

    fn journal_set_quest_picture_payload(tag: &str, picture: i32) -> Vec<u8> {
        let mut payload = journal_prefix(JOURNAL_SET_QUEST_PICTURE_MINOR);
        write_string(&mut payload, tag);
        payload.extend_from_slice(&picture.to_le_bytes());
        finish(payload)
    }

    fn journal_prefix(minor: u8) -> Vec<u8> {
        vec![b'P', JOURNAL_MAJOR, minor, 0, 0, 0, 0]
    }

    fn write_string(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn finish(mut payload: Vec<u8>) -> Vec<u8> {
        let declared = payload.len() as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload.push(FINAL_EMPTY_FRAGMENT_BYTE);
        payload
    }
}
