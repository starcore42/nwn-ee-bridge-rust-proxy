//! Client-side message semantic claims.
//!
//! EE `CNWSCreature::SendFeedbackMessage` calls
//! `CNWCCMessageData::SetInteger(9, feedback_id)` and then
//! `CNWSMessage::SendServerToPlayerCCMessage` with minor `0x0B`.
//! EE's case-11 decompile writes that slot-9 value as a 16-bit WORD first.
//! For feedback id `0xCC`, it then calls `GetString(0)` and
//! `WriteCExoString(..., 0x20)`. The EE `WriteCExoString` decompile confirms
//! the `0x20` path writes a direct little-endian DWORD length followed by that
//! many bytes.
//!
//! Most observed 1.69/HG feedback packets are already byte-identical after the
//! normal CNW declared-window wrapper, so this module claims them unchanged
//! after exact cursor validation. Some HG bulk-feedback captures use the same
//! semantic `0xCC` message but carry the text in legacy prefixed-fragment
//! layouts. Those shapes are rewritten here, inside the semantic owner, into
//! EE's WORD-id + DWORD-length `CExoString` form. Other 0x12 packets remain
//! unclaimed and quarantine.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const CLIENT_SIDE_MAJOR: u8 = 0x12;
const FEEDBACK_MINOR: u8 = 0x0B;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
const LEGACY_READ_START: usize = HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES;
const FEEDBACK_ID_BYTES: usize = 2;
const MAX_FRAGMENT_BYTES: usize = 64;
const MAX_FEEDBACK_TEXT_BYTES: usize = 4096;
const MAX_FIXED_ARGUMENT_BYTES: usize = 64;
const BULK_FEEDBACK_STRING_ID: u16 = 0x00CC;

#[derive(Debug, Clone, Copy)]
pub struct ClientSideMessageClaimSummary {
    pub feedback_id: u16,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientSideMessageClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (CLIENT_SIDE_MAJOR, FEEDBACK_MINOR) => claim_feedback(payload),
        _ => None,
    }
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut Vec<u8>,
) -> Option<ClientSideMessageClaimSummary> {
    if let Some(summary) = claim_payload_if_verified(payload) {
        return Some(summary);
    }
    rewrite_legacy_bulk_feedback_string(payload)
}

fn claim_feedback(payload: &[u8]) -> Option<ClientSideMessageClaimSummary> {
    if payload.len() < READ_START + FEEDBACK_ID_BYTES {
        return None;
    }
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START + FEEDBACK_ID_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
    {
        return None;
    }

    let feedback_id = read_u16_le(payload, READ_START)?;
    let tail_start = READ_START + FEEDBACK_ID_BYTES;
    let tail_len = declared - tail_start;
    let tail_valid = tail_len == 0
        || feedback_string_tail_valid(payload, tail_start, declared)
        || fixed_dword_tail_valid(tail_len);
    if !tail_valid {
        return None;
    }

    Some(ClientSideMessageClaimSummary {
        feedback_id,
        declared,
        fragment_bytes: payload.len() - declared,
    })
}

fn rewrite_legacy_bulk_feedback_string(
    payload: &mut Vec<u8>,
) -> Option<ClientSideMessageClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if (high.major, high.minor) != (CLIENT_SIDE_MAJOR, FEEDBACK_MINOR) {
        return None;
    }
    if payload.len() < LEGACY_READ_START + FEEDBACK_ID_BYTES + 3 {
        return None;
    }

    // If the normal EE/Diamond CNW declared-window shape validates, the caller
    // already claimed it unchanged. Reaching this path means the DWORD at
    // offset 3 is not an EE fragment offset, so interpret bytes 3..7 as the
    // legacy prefixed fragment bytes for this narrowly observed HG feedback
    // variant.
    let wire_declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if cnw_fragment_offset_valid(payload, wire_declared) || wire_declared == 0 {
        return None;
    }

    let feedback_id = read_u16_le(payload, LEGACY_READ_START)?;
    if feedback_id == BULK_FEEDBACK_STRING_ID {
        let length_start = LEGACY_READ_START + FEEDBACK_ID_BYTES;

        // Current HG captures use a legacy 16-bit text length, two alignment
        // bytes, and then the bulk feedback text. Earlier captures used a
        // 24-bit length with no gap. Keep both complete-message forms exact
        // and local to feedback id 0xCC.
        if rewrite_legacy_bulk_feedback_string_variant(payload, feedback_id, length_start, 2, 2)
            .is_some()
        {
            return claim_feedback(payload);
        }
        if rewrite_legacy_bulk_feedback_string_variant(payload, feedback_id, length_start, 3, 0)
            .is_some()
        {
            return claim_feedback(payload);
        }
    }

    // Diamond's client-side-message case-11 path accepts legacy/raw fields,
    // while EE writes a WORD id and, for id 0xCC, jumps to the branch that emits
    // only `GetString(0)` as a DWORD-length CExoString. HG bulk-feedback captures
    // expose that semantic id as one byte followed by a legacy DWORD marker and
    // then the whole current text window. The marker is not reliable as an EE
    // string length across captures, so preserve every byte in the observed text
    // window as the EE CExoString and keep the original four prefixed fragment
    // bytes as CNW trailing fragments.
    if rewrite_legacy_bulk_feedback_single_byte_id_window(payload).is_some() {
        return claim_feedback(payload);
    }

    None
}

fn rewrite_legacy_bulk_feedback_string_variant(
    payload: &mut Vec<u8>,
    feedback_id: u16,
    length_start: usize,
    length_bytes: usize,
    alignment_gap_bytes: usize,
) -> Option<()> {
    let legacy_text_len = match length_bytes {
        2 => usize::from(read_u16_le(payload, length_start)?),
        3 => read_u24_le(payload, length_start)?,
        _ => return None,
    };
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&legacy_text_len) {
        return None;
    }

    let text_start = length_start
        .checked_add(length_bytes)?
        .checked_add(alignment_gap_bytes)?;
    let text_end = text_start.checked_add(legacy_text_len)?;
    if text_end > payload.len() {
        return None;
    }

    let trailing_fragment_bytes = payload.len().checked_sub(text_end)?;
    if trailing_fragment_bytes + LEGACY_PREFIXED_FRAGMENT_BYTES > MAX_FRAGMENT_BYTES {
        return None;
    }

    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] =
        payload[HIGH_LEVEL_HEADER_BYTES..LEGACY_READ_START]
            .try_into()
            .ok()?;
    let trailing_fragments = payload[text_end..].to_vec();
    let text = payload[text_start..text_end].to_vec();

    let declared = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(FEEDBACK_ID_BYTES)?
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(text.len())?;
    let declared_u32 = u32::try_from(declared).ok()?;
    let text_len_u32 = u32::try_from(text.len()).ok()?;

    let mut rewritten = Vec::with_capacity(
        declared + trailing_fragment_bytes + LEGACY_PREFIXED_FRAGMENT_BYTES,
    );
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&declared_u32.to_le_bytes());
    rewritten.extend_from_slice(&feedback_id.to_le_bytes());
    rewritten.extend_from_slice(&text_len_u32.to_le_bytes());
    rewritten.extend_from_slice(&text);
    rewritten.extend_from_slice(&trailing_fragments);
    rewritten.extend_from_slice(&prefixed_fragment_bytes);

    *payload = rewritten;
    Some(())
}

fn rewrite_legacy_bulk_feedback_single_byte_id_window(payload: &mut Vec<u8>) -> Option<()> {
    if payload.get(LEGACY_READ_START).copied()? != BULK_FEEDBACK_STRING_ID as u8 {
        return None;
    }

    let legacy_text_len =
        usize::try_from(read_le_u32(payload, LEGACY_READ_START + 1)?).ok()?;
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&legacy_text_len) {
        return None;
    }

    let text_start = LEGACY_READ_START
        .checked_add(1)?
        .checked_add(CNW_LENGTH_BYTES)?;
    let available_text_len = payload.len().checked_sub(text_start)?;
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&available_text_len) {
        return None;
    }

    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] =
        payload[HIGH_LEVEL_HEADER_BYTES..LEGACY_READ_START]
            .try_into()
            .ok()?;
    let text = payload[text_start..].to_vec();

    let declared = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(FEEDBACK_ID_BYTES)?
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(text.len())?;
    let declared_u32 = u32::try_from(declared).ok()?;
    let text_len_u32 = u32::try_from(text.len()).ok()?;

    let mut rewritten = Vec::with_capacity(declared + LEGACY_PREFIXED_FRAGMENT_BYTES);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&declared_u32.to_le_bytes());
    rewritten.extend_from_slice(&BULK_FEEDBACK_STRING_ID.to_le_bytes());
    rewritten.extend_from_slice(&text_len_u32.to_le_bytes());
    rewritten.extend_from_slice(&text);
    rewritten.extend_from_slice(&prefixed_fragment_bytes);

    *payload = rewritten;
    Some(())
}

fn feedback_string_tail_valid(payload: &[u8], tail_start: usize, declared: usize) -> bool {
    let Some(length) = read_le_u32(payload, tail_start).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    length <= MAX_FEEDBACK_TEXT_BYTES
        && tail_start
            .checked_add(CNW_LENGTH_BYTES)
            .and_then(|text_start| text_start.checked_add(length))
            == Some(declared)
}

fn fixed_dword_tail_valid(tail_len: usize) -> bool {
    tail_len <= MAX_FIXED_ARGUMENT_BYTES && tail_len % CNW_LENGTH_BYTES == 0
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let chunk = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes(chunk.try_into().ok()?))
}

fn read_u24_le(bytes: &[u8], offset: usize) -> Option<usize> {
    let chunk = bytes.get(offset..offset.checked_add(3)?)?;
    Some(chunk[0] as usize | ((chunk[1] as usize) << 8) | ((chunk[2] as usize) << 16))
}

fn cnw_fragment_offset_valid(payload: &[u8], declared: u32) -> bool {
    let read_message_len = payload.len().saturating_sub(HIGH_LEVEL_HEADER_BYTES);
    if declared < HIGH_LEVEL_HEADER_BYTES as u32 || read_message_len == 0 {
        return false;
    }
    (declared as usize - HIGH_LEVEL_HEADER_BYTES) < read_message_len
}
