//! Chat packet semantic claims.
//!
//! This module intentionally claims even byte-identical chat packets instead
//! of relying on strict's high-level opcode classifier as an allow decision.
//!
//! Decompile evidence:
//! - EE `CNWSMessage::SendServerToPlayerChatMessage` case 5 calls
//!   `CNWMessage::CreateWriteMessage(strlen + 4, ..., 1)`, then
//!   `WriteCExoString(..., 0x20)`, then sends high-level family `0x09`,
//!   minor `0x05`.
//! - EE `CNWSMessage::SendServerToPlayerChat_Tell` creates a write message,
//!   writes a raw object id, a `CExoString` chat body, three floats, then one
//!   `BOOL` selecting the speaker-name branch before sending high-level family
//!   `0x09`, minor `0x04`.
//! - The EE chat client handler dispatches cases `4` and `20` through the Tell
//!   display path, consuming the same object/name/sound-position fields before
//!   formatting the local chat line.
//! - The HG/1.69 capture for `Chat_ServerTell` has the same read-buffer shape:
//!   a DWORD byte length followed by that many message bytes, with only the
//!   normal CNW fragment tail after the declared boundary.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const CHAT_MAJOR: u8 = 0x09;
const CHAT_TELL_MINOR: u8 = 0x04;
const SERVER_TELL_MINOR: u8 = 0x05;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const MAX_CHAT_TEXT_BYTES: usize = 8192;
const MAX_CHAT_SPEAKER_BYTES: usize = 512;
const MAX_FRAGMENT_BYTES: usize = 16;
const OBJECT_ID_BYTES: usize = 4;
const FLOAT_BYTES: usize = 4;
const CHAT_TELL_POSITION_FLOATS: usize = 3;
const CHAT_TELL_BOOL_FRAGMENT_BYTES: usize = 1;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ChatClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub text_len: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ChatClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (CHAT_MAJOR, CHAT_TELL_MINOR) => claim_chat_tell(payload, high.minor),
        (CHAT_MAJOR, SERVER_TELL_MINOR) => claim_server_tell(payload, high.minor),
        _ => None,
    }
}

fn claim_chat_tell(payload: &[u8], minor: u8) -> Option<ChatClaimSummary> {
    if payload.len() < READ_START + OBJECT_ID_BYTES + CNW_LENGTH_BYTES {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let fragment_bytes = payload.len().checked_sub(declared)?;
    if declared < READ_START + OBJECT_ID_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || fragment_bytes != CHAT_TELL_BOOL_FRAGMENT_BYTES
        || !cnw_fragment_tail_can_hold_one_bool(&payload[declared..])
    {
        return None;
    }

    let mut cursor = READ_START;
    let object_id = read_le_u32(payload, cursor)?;
    if !looks_like_chat_object_id(object_id) {
        return None;
    }
    cursor = cursor.checked_add(OBJECT_ID_BYTES)?;

    let (message_end, text_len) =
        read_bounded_cexo_string_end(payload, cursor, declared, MAX_CHAT_TEXT_BYTES)?;
    cursor = message_end;

    for _ in 0..CHAT_TELL_POSITION_FLOATS {
        let value = read_le_f32(payload, cursor)?;
        if !value.is_finite() || value.abs() > 1_000_000.0 {
            return None;
        }
        cursor = cursor.checked_add(FLOAT_BYTES)?;
    }

    // The decompiled writer emits one fragment BOOL followed by one of the
    // speaker-name branches. CNW bit writes live in the fragment tail, so the
    // byte-buffer proof here is deliberately about declared-byte cursor
    // exhaustion: current HG captures use two `CExoString` name fields
    // (`"Captain"` and an empty secondary string), while the other branch can
    // end after one direct speaker string. Neither branch requires mutation.
    let (speaker_end, _) =
        read_bounded_cexo_string_end(payload, cursor, declared, MAX_CHAT_SPEAKER_BYTES)?;
    let exact_speaker_shape = speaker_end == declared
        || read_bounded_cexo_string_end(payload, speaker_end, declared, MAX_CHAT_SPEAKER_BYTES)
            .is_some_and(|(second_end, _)| second_end == declared);
    if !exact_speaker_shape {
        return None;
    }

    Some(ChatClaimSummary {
        minor,
        declared,
        text_len,
        fragment_bytes,
    })
}

fn claim_server_tell(payload: &[u8], minor: u8) -> Option<ChatClaimSummary> {
    if payload.len() < READ_START + CNW_LENGTH_BYTES {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START + CNW_LENGTH_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
    {
        return None;
    }

    let text_len = usize::try_from(read_le_u32(payload, READ_START)?).ok()?;
    if text_len > MAX_CHAT_TEXT_BYTES {
        return None;
    }

    let text_end = READ_START
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(text_len)?;
    if text_end != declared {
        return None;
    }

    Some(ChatClaimSummary {
        minor,
        declared,
        text_len,
        fragment_bytes: payload.len() - declared,
    })
}

fn read_bounded_cexo_string_end(
    payload: &[u8],
    offset: usize,
    declared: usize,
    max_bytes: usize,
) -> Option<(usize, usize)> {
    if offset.checked_add(CNW_LENGTH_BYTES)? > declared {
        return None;
    }
    let length = usize::try_from(read_le_u32(payload, offset)?).ok()?;
    if length > max_bytes {
        return None;
    }
    let end = offset.checked_add(CNW_LENGTH_BYTES)?.checked_add(length)?;
    if end > declared {
        return None;
    }
    Some((end, length))
}

fn read_le_f32(payload: &[u8], offset: usize) -> Option<f32> {
    let bytes = payload.get(offset..offset + FLOAT_BYTES)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn looks_like_chat_object_id(object_id: u32) -> bool {
    object_id != 0 && object_id != u32::MAX
}

fn cnw_fragment_tail_can_hold_one_bool(fragment: &[u8]) -> bool {
    let Some(first) = fragment.first().copied() else {
        return false;
    };
    let final_bits = usize::from((first & 0xE0) >> 5);
    let valid_bits = if final_bits == 0 {
        fragment.len().saturating_mul(8)
    } else {
        fragment
            .len()
            .saturating_sub(1)
            .saturating_mul(8)
            .saturating_add(final_bits)
    };
    valid_bits > CNW_FRAGMENT_HEADER_BITS
}
