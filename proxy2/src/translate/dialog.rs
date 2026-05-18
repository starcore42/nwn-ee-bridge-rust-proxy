//! Dialog packet semantic claims.
//!
//! Dialog traffic is intentionally claimed here even when the Diamond and EE
//! byte shapes are identical.  The strict bridge rule is that a known opcode is
//! not an allow proof by itself; a focused semantic owner must prove the exact
//! reader cursor shape before bytes are emitted to the opposite dialect.
//!
//! Decompile evidence used for the currently-owned shapes:
//! - Diamond's server-to-client high-level dispatcher routes major `0x14` to
//!   the Dialog handler (`sub_443C70` in the local Diamond decompile).
//! - The `0x14/0x01` branch reads three server OBJECTIDs with `sub_53E690`
//!   (a bounded four-byte read), then a bounded direct text/locstring payload,
//!   then two fragment BOOLs.  The observed local Diamond server payload uses a
//!   direct `CExoString`: DWORD byte length followed by UTF-8/ANSI bytes.
//! - The `0x14/0x02` branch reads a bounded DWORD window, a server OBJECTID,
//!   two DWORD reply-list counts, then one direct string plus DWORD reply id per
//!   entry, followed by the same compact fragment BOOL tail.  The local Diamond
//!   capture for the door/placeable interaction matches that direct-string
//!   reply-list shape exactly.
//!
//! This module is deliberately narrow.  Other dialog minors and locstring/bit
//! variants should remain quarantined until captures plus decompile traces add
//! another typed parser branch.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const DIALOG_MAJOR: u8 = 0x14;
const DIALOG_ENTRY_MINOR: u8 = 0x01;
const DIALOG_REPLIES_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const DWORD_BYTES: usize = 4;
const OBJECT_ID_BYTES: usize = 4;
const MAX_DIALOG_TEXT_BYTES: usize = 8192;
const MAX_DIALOG_REPLY_TEXT_BYTES: usize = 512;
const MAX_DIALOG_REPLIES: usize = 64;
const DIALOG_BOOL_FRAGMENT_BYTES: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub text_bytes: usize,
    pub reply_count: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<DialogClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (DIALOG_MAJOR, DIALOG_ENTRY_MINOR) => claim_dialog_entry(payload, high.minor),
        (DIALOG_MAJOR, DIALOG_REPLIES_MINOR) => claim_dialog_replies(payload, high.minor),
        _ => None,
    }
}

fn claim_dialog_entry(payload: &[u8], minor: u8) -> Option<DialogClaimSummary> {
    let declared = declared_with_dialog_fragment_tail(payload)?;
    let mut cursor = READ_START;

    for _ in 0..3 {
        let _object_id = read_le_u32(payload, cursor)?;
        cursor = cursor.checked_add(OBJECT_ID_BYTES)?;
    }

    let (cursor, text_bytes) = read_c_exo_string(payload, cursor, declared, MAX_DIALOG_TEXT_BYTES)?;
    if cursor != declared {
        return None;
    }

    Some(DialogClaimSummary {
        minor,
        declared,
        text_bytes,
        reply_count: 0,
        fragment_bytes: payload.len() - declared,
    })
}

fn claim_dialog_replies(payload: &[u8], minor: u8) -> Option<DialogClaimSummary> {
    let declared = declared_with_dialog_fragment_tail(payload)?;
    let mut cursor = READ_START;

    // Diamond's branch starts with a bounded 32-bit value before the target /
    // conversation object reference.  The observed local server emits zero for
    // this value; it is still modeled as an owned DWORD rather than ignored
    // bytes so cursor proof remains exact.
    let _dialog_node = read_le_u32(payload, cursor)?;
    cursor = cursor.checked_add(DWORD_BYTES)?;

    let _target_object_id = read_le_u32(payload, cursor)?;
    cursor = cursor.checked_add(OBJECT_ID_BYTES)?;

    let primary_count = read_count(payload, cursor)?;
    cursor = cursor.checked_add(DWORD_BYTES)?;
    let secondary_count = read_count(payload, cursor)?;
    cursor = cursor.checked_add(DWORD_BYTES)?;
    let total_replies = primary_count.checked_add(secondary_count)?;
    if total_replies > MAX_DIALOG_REPLIES {
        return None;
    }

    let mut text_bytes = 0usize;
    for _ in 0..total_replies {
        let (next, len) =
            read_c_exo_string(payload, cursor, declared, MAX_DIALOG_REPLY_TEXT_BYTES)?;
        cursor = next;
        text_bytes = text_bytes.checked_add(len)?;
        let _reply_id = read_le_u32(payload, cursor)?;
        cursor = cursor.checked_add(DWORD_BYTES)?;
    }

    if cursor != declared {
        return None;
    }

    Some(DialogClaimSummary {
        minor,
        declared,
        text_bytes,
        reply_count: total_replies,
        fragment_bytes: payload.len() - declared,
    })
}

fn declared_with_dialog_fragment_tail(payload: &[u8]) -> Option<usize> {
    if payload.len() < READ_START + DIALOG_BOOL_FRAGMENT_BYTES {
        return None;
    }
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START
        || declared > payload.len()
        || payload.len() != declared.checked_add(DIALOG_BOOL_FRAGMENT_BYTES)?
    {
        return None;
    }
    Some(declared)
}

fn read_count(payload: &[u8], offset: usize) -> Option<usize> {
    let value = usize::try_from(read_le_u32(payload, offset)?).ok()?;
    (value <= MAX_DIALOG_REPLIES).then_some(value)
}

fn read_c_exo_string(
    payload: &[u8],
    cursor: usize,
    declared: usize,
    max_len: usize,
) -> Option<(usize, usize)> {
    if cursor > declared || declared > payload.len() {
        return None;
    }
    let len = usize::try_from(read_le_u32(payload, cursor)?).ok()?;
    if len > max_len {
        return None;
    }
    let text_start = cursor.checked_add(CNW_LENGTH_BYTES)?;
    let text_end = text_start.checked_add(len)?;
    if text_end > declared {
        return None;
    }
    Some((text_end, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_local_diamond_dialog_entry_capture_shape() {
        let payload = local_diamond_dialog_entry_payload();
        let claim = claim_payload_if_verified(&payload).expect("dialog entry should claim");
        assert_eq!(claim.minor, DIALOG_ENTRY_MINOR);
        assert_eq!(claim.declared, 0x5D);
        assert_eq!(claim.text_bytes, 70);
        assert_eq!(claim.reply_count, 0);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn claims_local_diamond_dialog_replies_capture_shape() {
        let payload = local_diamond_dialog_replies_payload();
        let claim = claim_payload_if_verified(&payload).expect("dialog replies should claim");
        assert_eq!(claim.minor, DIALOG_REPLIES_MINOR);
        assert_eq!(claim.declared, 0x2C);
        assert_eq!(claim.text_bytes, 5);
        assert_eq!(claim.reply_count, 2);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_dialog_when_declared_cursor_does_not_match() {
        let mut payload = local_diamond_dialog_entry_payload();
        payload[3..7].copy_from_slice(&0x5Eu32.to_le_bytes());
        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_dialog_without_fragment_bool_tail() {
        let mut payload = local_diamond_dialog_replies_payload();
        payload.pop();
        assert!(claim_payload_if_verified(&payload).is_none());
    }

    fn local_diamond_dialog_entry_payload() -> Vec<u8> {
        let text = b"This home seems to be abandoned. Do you want to take possession of it?";
        let declared = READ_START + (3 * OBJECT_ID_BYTES) + CNW_LENGTH_BYTES + text.len();
        let mut payload = vec![0x50, DIALOG_MAJOR, DIALOG_ENTRY_MINOR];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        payload.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        payload.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        payload.extend_from_slice(&(text.len() as u32).to_le_bytes());
        payload.extend_from_slice(text);
        payload.push(0x87);
        payload
    }

    fn local_diamond_dialog_replies_payload() -> Vec<u8> {
        let yes = b"Yes";
        let no = b"No";
        let declared = READ_START
            + DWORD_BYTES
            + OBJECT_ID_BYTES
            + DWORD_BYTES
            + DWORD_BYTES
            + CNW_LENGTH_BYTES
            + yes.len()
            + DWORD_BYTES
            + CNW_LENGTH_BYTES
            + no.len()
            + DWORD_BYTES;
        let mut payload = vec![0x50, DIALOG_MAJOR, DIALOG_REPLIES_MINOR];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&(yes.len() as u32).to_le_bytes());
        payload.extend_from_slice(yes);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&(no.len() as u32).to_le_bytes());
        payload.extend_from_slice(no);
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.push(0xE1);
        payload
    }
}
