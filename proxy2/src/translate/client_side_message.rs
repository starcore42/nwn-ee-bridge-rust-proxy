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
const LEGACY_BOUNDED_CC_READ_WINDOW_BYTES: u32 = 0x0000_0079;

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
    if rewrite_legacy_feedback_94_preamble_cc_string(payload).is_some() {
        return claim_feedback(payload);
    }
    if rewrite_legacy_bulk_feedback_declared_text_window(payload).is_some() {
        return claim_feedback(payload);
    }
    if rewrite_legacy_bulk_feedback_stale_declared_full_text_len(payload).is_some() {
        return claim_feedback(payload);
    }
    if rewrite_legacy_bulk_feedback_stale_declared_continued_text(payload).is_some() {
        return claim_feedback(payload);
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

    // HG's area-entry welcome text can arrive as a Diamond-era feedback body
    // that is already inside a deflated M stream but is not in EE's
    // CNW-declared shape.  The decompile-backed target is still precise:
    // EE's CC-message case 11 writes slot-9 as a WORD, and feedback id 0xCC
    // writes only string slot 0 as a DWORD-length CExoString.  The observed
    // legacy shape has the first four fragment bytes where EE would put the
    // declared length, the WORD id at the legacy read start, and another
    // four-byte legacy fragment word before the bulk text window.  Convert
    // that semantic text window into the exact EE form rather than allowing the
    // malformed legacy bytes to leak through.
    if rewrite_legacy_bulk_feedback_fragmented_text_window(payload).is_some() {
        return claim_feedback(payload);
    }

    None
}

fn rewrite_legacy_feedback_94_preamble_cc_string(payload: &mut Vec<u8>) -> Option<()> {
    let high = HighLevel::parse(payload)?;
    if (high.major, high.minor) != (CLIENT_SIDE_MAJOR, FEEDBACK_MINOR) {
        return None;
    }

    // EE `SendServerToPlayerCCMessage` case 11 writes WORD slot-9 first.
    // When slot-9 is 0x00CC, the EE writer jumps to the decompile branch that
    // emits only string slot 0 as a DWORD-length `CExoString`.
    //
    // HG can send its login/welcome bulk text as:
    //
    //   P 12 0B <stale prelude> CC 00 <DWORD text length> <text> <fragments>
    //
    // The leading DWORD is not the EE case-11 declared length, and it is not
    // part of the EE 0x00CC string branch. Treat it as a legacy raw feedback
    // preamble only when the following exact 0x00CC string window validates;
    // otherwise leave the packet unclaimed for quarantine.
    let legacy_preamble = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    // Diamond/HG feedback id 0x00CC is the decompile-backed "direct string"
    // path used by CNWSMessage::SendServerToPlayerCCMessage case 0x0B:
    // WORD feedback id, DWORD CExoString length, bytes, then fragment bits.
    //
    // Captures show this legacy/stale preamble value can be 0x93 or 0x94
    // before the actual 0x00CC text window. A later HG area-entry capture uses
    // 0x79, which matches the bounded CC read body window left behind by the
    // legacy writer while the real bulk text is carried by the following
    // CExoString. We only accept those exact preambles and only when the
    // following id/length/text/trailing-fragment shape validates, so this does
    // not create a raw pass-through path.
    if !matches!(
        legacy_preamble,
        LEGACY_BOUNDED_CC_READ_WINDOW_BYTES | 0x0000_0093 | 0x0000_0094
    ) {
        return None;
    }

    let feedback_id_start = HIGH_LEVEL_HEADER_BYTES.checked_add(CNW_LENGTH_BYTES)?;
    if read_u16_le(payload, feedback_id_start)? != BULK_FEEDBACK_STRING_ID {
        return None;
    }

    let text_length_start = feedback_id_start.checked_add(FEEDBACK_ID_BYTES)?;
    let text_len = usize::try_from(read_le_u32(payload, text_length_start)?).ok()?;
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&text_len) {
        return None;
    }

    let text_start = text_length_start.checked_add(CNW_LENGTH_BYTES)?;
    let text_end = text_start.checked_add(text_len)?;
    if text_end > payload.len() {
        return None;
    }
    let trailing_fragment_bytes = payload.len().checked_sub(text_end)?;
    if trailing_fragment_bytes > MAX_FRAGMENT_BYTES {
        return None;
    }
    if !feedback_text_window_plausible(&payload[text_start..text_end]) {
        return None;
    }

    let text = payload[text_start..text_end].to_vec();
    let trailing_fragments = payload[text_end..].to_vec();
    let declared = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(FEEDBACK_ID_BYTES)?
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(text.len())?;
    let declared_u32 = u32::try_from(declared).ok()?;
    let text_len_u32 = u32::try_from(text.len()).ok()?;

    let mut rewritten = Vec::with_capacity(declared + trailing_fragments.len());
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&declared_u32.to_le_bytes());
    rewritten.extend_from_slice(&BULK_FEEDBACK_STRING_ID.to_le_bytes());
    rewritten.extend_from_slice(&text_len_u32.to_le_bytes());
    rewritten.extend_from_slice(&text);
    rewritten.extend_from_slice(&trailing_fragments);

    *payload = rewritten;
    Some(())
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

    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] = payload
        [HIGH_LEVEL_HEADER_BYTES..LEGACY_READ_START]
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

    let mut rewritten =
        Vec::with_capacity(declared + trailing_fragment_bytes + LEGACY_PREFIXED_FRAGMENT_BYTES);
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

    let legacy_text_len = usize::try_from(read_le_u32(payload, LEGACY_READ_START + 1)?).ok()?;
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

    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] = payload
        [HIGH_LEVEL_HEADER_BYTES..LEGACY_READ_START]
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

fn rewrite_legacy_bulk_feedback_fragmented_text_window(payload: &mut Vec<u8>) -> Option<()> {
    let id_start = LEGACY_READ_START;
    if read_u16_le(payload, id_start)? != BULK_FEEDBACK_STRING_ID {
        return None;
    }

    let text_start = id_start
        .checked_add(FEEDBACK_ID_BYTES)?
        .checked_add(LEGACY_PREFIXED_FRAGMENT_BYTES)?;
    if text_start >= payload.len() {
        return None;
    }

    let text = extract_legacy_feedback_text(&payload[text_start..])?;
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&text.len()) {
        return None;
    }

    let declared = HIGH_LEVEL_HEADER_BYTES
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(FEEDBACK_ID_BYTES)?
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(text.len())?;
    let declared_u32 = u32::try_from(declared).ok()?;
    let text_len_u32 = u32::try_from(text.len()).ok()?;

    let mut rewritten = Vec::with_capacity(declared);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&declared_u32.to_le_bytes());
    rewritten.extend_from_slice(&BULK_FEEDBACK_STRING_ID.to_le_bytes());
    rewritten.extend_from_slice(&text_len_u32.to_le_bytes());
    rewritten.extend_from_slice(&text);

    *payload = rewritten;
    Some(())
}

/// HG's 1.69 bulk feedback path can arrive as a `ClientSideMessage_Feedback`
/// record whose declared high-level span contains the whole CExoString text, while
/// the DWORD immediately after feedback id 0x00CC is a stale legacy/argument count
/// rather than the EE CExoString byte count. The EE client decompile's feedback
/// case for id 0x00CC consumes WORD id + DWORD CExoString length + text, so the
/// strict bridge rewrites only this proven declared-window shape by deriving the
/// EE string length from the validated packet span. The packet is then re-claimed
/// by the normal exact feedback validator before it can be emitted.
fn rewrite_legacy_bulk_feedback_declared_text_window(payload: &mut Vec<u8>) -> Option<()> {
    let high = HighLevel::parse(payload)?;
    if (high.major, high.minor) != (CLIENT_SIDE_MAJOR, FEEDBACK_MINOR) {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let text_start = READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES;
    if declared <= text_start || declared > payload.len() {
        return None;
    }
    if payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES {
        return None;
    }
    if read_u16_le(payload, READ_START)? != BULK_FEEDBACK_STRING_ID {
        return None;
    }

    let legacy_dword =
        usize::try_from(read_le_u32(payload, READ_START + FEEDBACK_ID_BYTES)?).ok()?;
    let text_len = declared.checked_sub(text_start)?;
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&text_len) {
        return None;
    }
    if legacy_dword == text_len || legacy_dword > text_len {
        return None;
    }
    if !feedback_text_window_plausible(&payload[text_start..declared]) {
        return None;
    }

    let text_len_u32 = u32::try_from(text_len).ok()?;
    payload[READ_START + FEEDBACK_ID_BYTES..READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES]
        .copy_from_slice(&text_len_u32.to_le_bytes());
    Some(())
}

/// HG can emit the area-entry bulk feedback with the EE/1.69-shared semantic
/// body already present, but with the CNW declared field left at a stale
/// Diamond-era bounded read window. The decompile-backed EE target remains the
/// same case-11/id-0x00CC shape: WORD feedback id, DWORD CExoString byte count,
/// then the whole text. Accept only that exact self-contained text-length proof
/// and one trailing compact CNW fragment byte, then repair only the declared
/// field so strict validation can prove the emitted packet.
fn rewrite_legacy_bulk_feedback_stale_declared_full_text_len(payload: &mut Vec<u8>) -> Option<()> {
    let high = HighLevel::parse(payload)?;
    if (high.major, high.minor) != (CLIENT_SIDE_MAJOR, FEEDBACK_MINOR) {
        return None;
    }

    let stale_declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let text_start = READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES;
    if stale_declared <= text_start || stale_declared >= payload.len() {
        return None;
    }
    if payload.len().saturating_sub(stale_declared) <= MAX_FRAGMENT_BYTES {
        return None;
    }
    if read_u16_le(payload, READ_START)? != BULK_FEEDBACK_STRING_ID {
        return None;
    }

    let text_len = usize::try_from(read_le_u32(payload, READ_START + FEEDBACK_ID_BYTES)?).ok()?;
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&text_len) {
        return None;
    }

    let text_end = text_start.checked_add(text_len)?;
    if text_end <= stale_declared || text_end > payload.len() {
        return None;
    }

    // This capture family carries exactly one final compact CNW fragment byte
    // after the visible text. Keep that byte outside the declared EE read span;
    // if another fragment layout appears, it should get its own named rewrite.
    if payload.len().checked_sub(text_end)? != 1 {
        return None;
    }
    let text = &payload[text_start..text_end];
    if !feedback_text_window_plausible(text) || !bulk_feedback_text_terminal_marker_valid(text) {
        return None;
    }

    let declared_u32 = u32::try_from(text_end).ok()?;
    payload[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
        .copy_from_slice(&declared_u32.to_le_bytes());
    Some(())
}

/// HG can emit feedback id 0x00CC with a self-consistent first Diamond-era text
/// window, then continue appending the rest of the bulk feedback text before the
/// final compact CNW fragment byte. The EE client-side-message reader for case
/// 0x0B/id 0x00CC does not consume that stale first-window boundary: it expects
/// the decompile-backed `WORD feedback id` followed by one DWORD-length
/// `CExoString`. Rewrite only this bounded legacy shape by deriving the EE
/// string length from the full validated text window and preserving the final
/// fragment byte outside the declared read span.
fn rewrite_legacy_bulk_feedback_stale_declared_continued_text(payload: &mut Vec<u8>) -> Option<()> {
    let high = HighLevel::parse(payload)?;
    if (high.major, high.minor) != (CLIENT_SIDE_MAJOR, FEEDBACK_MINOR) {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let text_start = READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES;
    if declared <= text_start || declared >= payload.len() {
        return None;
    }
    if payload.len().saturating_sub(declared) <= MAX_FRAGMENT_BYTES {
        return None;
    }
    if read_u16_le(payload, READ_START)? != BULK_FEEDBACK_STRING_ID {
        return None;
    }

    let stale_text_len =
        usize::try_from(read_le_u32(payload, READ_START + FEEDBACK_ID_BYTES)?).ok()?;
    if stale_text_len != declared.checked_sub(text_start)? {
        return None;
    }
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&stale_text_len) {
        return None;
    }

    // This source shape has one final CNW compact-fragment byte after the
    // visible feedback string. Do not generalize that byte count: if another
    // capture proves a different fragment layout, it should get its own named
    // rewrite instead of broadening this one.
    let fragment_bytes = 1usize;
    let text_end = payload.len().checked_sub(fragment_bytes)?;
    if text_end <= declared || text_end <= text_start {
        return None;
    }
    let text = &payload[text_start..text_end];
    if !feedback_text_window_plausible(text) || !bulk_feedback_text_terminal_marker_valid(text) {
        return None;
    }

    let text_len = text.len();
    if !(16..=MAX_FEEDBACK_TEXT_BYTES).contains(&text_len) {
        return None;
    }
    let declared = text_start.checked_add(text_len)?;
    let declared_u32 = u32::try_from(declared).ok()?;
    let text_len_u32 = u32::try_from(text_len).ok()?;
    payload[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
        .copy_from_slice(&declared_u32.to_le_bytes());
    payload[READ_START + FEEDBACK_ID_BYTES..READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES]
        .copy_from_slice(&text_len_u32.to_le_bytes());
    Some(())
}

fn bulk_feedback_text_terminal_marker_valid(bytes: &[u8]) -> bool {
    bytes.ends_with(b"</c>") || bytes.ends_with(b"\n") || bytes.ends_with(b"\r\n")
}

fn feedback_text_window_plausible(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }

    let mut useful = 0usize;
    let mut control = 0usize;
    for &byte in bytes {
        match byte {
            b'\r' | b'\n' | b'\t' => useful += 1,
            0x20..=0x7e => useful += 1,
            // HG feedback text can contain legacy color/control bytes embedded in
            // CExoString markup. They are accepted as text evidence, but only
            // after the surrounding declared span and 0x00CC feedback id match.
            0x80..=0xff => useful += 1,
            _ => control += 1,
        }
    }

    useful >= 16 && control <= (bytes.len() / 16).saturating_add(4)
}
fn extract_legacy_feedback_text(raw: &[u8]) -> Option<Vec<u8>> {
    let mut text = Vec::with_capacity(raw.len());
    let mut last_was_space = false;
    let mut kept_printable = 0usize;

    for &byte in raw {
        let mapped = match byte {
            b'\r' | b'\n' | b'\t' => byte,
            b' '..=b'~' => byte,
            0 => b' ',
            _ => b' ',
        };

        if mapped == b' ' {
            if last_was_space {
                continue;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
            if mapped.is_ascii_graphic() {
                kept_printable += 1;
            }
        }

        text.push(mapped);
    }

    while text.last().copied() == Some(b' ') {
        text.pop();
    }

    (kept_printable >= 16).then_some(text)
}

fn feedback_string_tail_valid(payload: &[u8], tail_start: usize, declared: usize) -> bool {
    let Some(length) =
        read_le_u32(payload, tail_start).and_then(|value| usize::try_from(value).ok())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_feedback_94_preamble_cc_string_rewrites_to_exact_ee_text_shape() {
        let legacy = include_bytes!(
            "../../fixtures/client_side_message/hg_feedback_94_preamble_cc_string_legacy.bin"
        );
        assert_eq!(&legacy[..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(
            read_le_u32(legacy, HIGH_LEVEL_HEADER_BYTES),
            Some(0x0000_0094)
        );
        assert_eq!(
            read_u16_le(legacy, HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES),
            Some(BULK_FEEDBACK_STRING_ID)
        );
        assert!(claim_payload_if_verified(legacy).is_none());

        let mut payload = legacy.to_vec();
        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("legacy 0x94 preamble + 0xCC string feedback should rewrite exactly");

        let declared =
            usize::try_from(read_le_u32(&payload, HIGH_LEVEL_HEADER_BYTES).unwrap()).unwrap();
        let text_len =
            usize::try_from(read_le_u32(&payload, READ_START + FEEDBACK_ID_BYTES).unwrap())
                .unwrap();

        assert_eq!(summary.feedback_id, BULK_FEEDBACK_STRING_ID);
        assert_eq!(summary.declared, declared);
        assert_eq!(summary.fragment_bytes, 1);
        assert_eq!(declared, payload.len() - summary.fragment_bytes);
        assert_eq!(
            text_len,
            declared - (READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES)
        );
        assert_eq!(
            read_u16_le(&payload, READ_START),
            Some(BULK_FEEDBACK_STRING_ID)
        );
        assert_eq!(payload.last(), legacy.last());
        assert!(claim_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn legacy_feedback_79_preamble_cc_string_rewrites_to_exact_ee_text_shape() {
        let legacy = include_bytes!(
            "../../fixtures/client_side_message/hg_feedback_79_preamble_cc_string_legacy.bin"
        );
        assert_eq!(&legacy[..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(
            read_le_u32(legacy, HIGH_LEVEL_HEADER_BYTES),
            Some(LEGACY_BOUNDED_CC_READ_WINDOW_BYTES)
        );
        assert_eq!(
            read_u16_le(legacy, HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES),
            Some(BULK_FEEDBACK_STRING_ID)
        );
        assert!(claim_payload_if_verified(legacy).is_none());

        let mut payload = legacy.to_vec();
        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("legacy 0x79 preamble + 0xCC string feedback should rewrite exactly");

        let declared =
            usize::try_from(read_le_u32(&payload, HIGH_LEVEL_HEADER_BYTES).unwrap()).unwrap();
        let text_len =
            usize::try_from(read_le_u32(&payload, READ_START + FEEDBACK_ID_BYTES).unwrap())
                .unwrap();

        assert_eq!(summary.feedback_id, BULK_FEEDBACK_STRING_ID);
        assert_eq!(summary.declared, declared);
        assert_eq!(summary.fragment_bytes, 1);
        assert_eq!(declared, payload.len() - summary.fragment_bytes);
        assert_eq!(
            text_len,
            declared - (READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES)
        );
        assert_eq!(
            read_u16_le(&payload, READ_START),
            Some(BULK_FEEDBACK_STRING_ID)
        );
        assert_eq!(payload.last(), legacy.last());
        assert!(claim_payload_if_verified(&payload).is_some());
    }
}

#[cfg(test)]
mod captured_hg_feedback_93_tests {
    use super::*;

    #[test]
    fn legacy_feedback_93_preamble_cc_string_rewrites_to_exact_ee_text_shape() {
        let legacy = include_bytes!(
            "../../fixtures/client_side_message/hg_feedback_93_preamble_cc_string_legacy.bin"
        );
        assert_eq!(&legacy[0..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(
            read_le_u32(legacy, HIGH_LEVEL_HEADER_BYTES),
            Some(0x0000_0093)
        );
        assert_eq!(
            u16::from_le_bytes([legacy[LEGACY_READ_START], legacy[LEGACY_READ_START + 1]]),
            BULK_FEEDBACK_STRING_ID
        );
        assert!(claim_payload_if_verified(legacy).is_none());

        let mut payload = legacy.to_vec();
        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("captured HG feedback 0x93 legacy text shape should rewrite and claim");

        assert_eq!(summary.feedback_id, BULK_FEEDBACK_STRING_ID);
        assert_eq!(summary.fragment_bytes, 1);
        assert_eq!(&payload[0..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(payload.last(), legacy.last());

        let declared =
            read_le_u32(&payload, HIGH_LEVEL_HEADER_BYTES).expect("EE declared length") as usize;
        let text_len_offset = READ_START + FEEDBACK_ID_BYTES;
        let text_len =
            read_le_u32(&payload, text_len_offset).expect("EE CExoString length") as usize;
        assert_eq!(declared, payload.len() - summary.fragment_bytes);
        assert_eq!(
            text_len,
            declared - (READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES)
        );
        assert!(claim_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn stale_declared_continued_bulk_feedback_rewrites_to_full_ee_text_window() {
        let legacy =
            include_bytes!("../../fixtures/client_side_message/hg_welcome_feedback_204.bin");
        assert_eq!(&legacy[0..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(
            read_u16_le(legacy, READ_START),
            Some(BULK_FEEDBACK_STRING_ID)
        );
        let stale_declared =
            usize::try_from(read_le_u32(legacy, HIGH_LEVEL_HEADER_BYTES).unwrap()).unwrap();
        let stale_text_len =
            usize::try_from(read_le_u32(legacy, READ_START + FEEDBACK_ID_BYTES).unwrap()).unwrap();
        assert_eq!(
            stale_text_len,
            stale_declared - (READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES)
        );
        assert!(legacy.len() - stale_declared > MAX_FRAGMENT_BYTES);
        assert!(claim_payload_if_verified(legacy).is_none());

        let mut payload = legacy.to_vec();
        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("captured HG stale-declared 0xCC feedback should rewrite and claim");

        assert_eq!(summary.feedback_id, BULK_FEEDBACK_STRING_ID);
        assert_eq!(summary.fragment_bytes, 1);
        assert_eq!(&payload[0..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(payload.last(), legacy.last());

        let declared = read_le_u32(&payload, HIGH_LEVEL_HEADER_BYTES).unwrap() as usize;
        let text_len = read_le_u32(&payload, READ_START + FEEDBACK_ID_BYTES).unwrap() as usize;
        assert_eq!(declared, payload.len() - summary.fragment_bytes);
        assert_eq!(
            text_len,
            declared - (READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES)
        );
        assert_eq!(&payload[declared - 4..declared], b"</c>");
        assert!(claim_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn stale_declared_full_len_bulk_feedback_rewrites_declared_to_exact_ee_text_window() {
        let legacy = include_bytes!(
            "../../fixtures/client_side_message/hg_welcome_feedback_594_stale_declared_full_len_legacy.bin"
        );
        assert_eq!(&legacy[0..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(
            read_u16_le(legacy, READ_START),
            Some(BULK_FEEDBACK_STRING_ID)
        );

        let stale_declared =
            usize::try_from(read_le_u32(legacy, HIGH_LEVEL_HEADER_BYTES).unwrap()).unwrap();
        let text_len =
            usize::try_from(read_le_u32(legacy, READ_START + FEEDBACK_ID_BYTES).unwrap()).unwrap();
        let text_start = READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES;
        assert_eq!(stale_declared, 95);
        assert_eq!(text_len, 594);
        assert_eq!(legacy.len() - (text_start + text_len), 1);
        assert!(legacy.len() - stale_declared > MAX_FRAGMENT_BYTES);
        assert!(claim_payload_if_verified(legacy).is_none());

        let mut payload = legacy.to_vec();
        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("captured HG stale-declared full-length 0xCC feedback should claim");

        assert_eq!(summary.feedback_id, BULK_FEEDBACK_STRING_ID);
        assert_eq!(summary.fragment_bytes, 1);
        assert_eq!(&payload[0..3], &[b'P', CLIENT_SIDE_MAJOR, FEEDBACK_MINOR]);
        assert_eq!(payload.last(), legacy.last());

        let declared = read_le_u32(&payload, HIGH_LEVEL_HEADER_BYTES).unwrap() as usize;
        let rewritten_text_len =
            read_le_u32(&payload, READ_START + FEEDBACK_ID_BYTES).unwrap() as usize;
        assert_eq!(declared, payload.len() - summary.fragment_bytes);
        assert_eq!(rewritten_text_len, text_len);
        assert_eq!(
            rewritten_text_len,
            declared - (READ_START + FEEDBACK_ID_BYTES + CNW_LENGTH_BYTES)
        );
        assert_eq!(&payload[declared - 4..declared], b"</c>");
        assert!(claim_payload_if_verified(&payload).is_some());
    }
}
