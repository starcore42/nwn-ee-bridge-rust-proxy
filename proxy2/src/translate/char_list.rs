//! Character-list packet semantic translation.
//!
//! The strict bridge rule is that a known opcode is only a classifier; a
//! focused semantic module must still claim and validate the payload before the
//! M transport layer can emit it.
//!
//! Decompile evidence for `CharList_ListResponse`:
//!
//! - EE `CNWSMessage::SendServerToPlayerCharList` calls
//!   `CreateWriteMessage`, writes a WORD character count, then for each
//!   `NWPlayerCharacterList_st` writes first-name and last-name server
//!   locstrings, a fixed 16-byte portrait resref, a BYTE, a WORD, a second
//!   fixed 16-byte resref, a BYTE class count, and then class INT/BYTE pairs.
//!   It then calls `GetWriteMessage` and sends family `0x11`, minor `0x02`.
//! - The 1.69/HG capture uses the same declared CNW window for this packet:
//!   after `P 11 02`, the first DWORD is the declared read-message length, the
//!   data stream starts with the WORD character count, and trailing packetized
//!   fragment bytes remain outside the declared window.
//!
//! No byte mutation is done here. If the stream cannot be walked exactly, the
//! caller must quarantine it and inspect the dump before adding another
//! decompile-backed shape.
//!
//! Decompile evidence for `CharList_UpdateCharResponse`:
//!
//! - EE `CNWSMessage::SendServerToPlayerUpdateCharResponse` builds family
//!   `0x11`, minor `0x04` with `CreateWriteMessage`, then writes a BYTE
//!   response value, a fixed 16-byte `CResRef`, a DWORD BIC byte count, and
//!   the raw BIC bytes via `WriteVOIDPtr`.
//! - Diamond/HG captures use the same outer stream shape but the embedded BIC
//!   GFF can arrive in a legacy non-canonical section layout. EE consumes the
//!   same semantic BIC fields, so this translator canonicalizes only the GFF
//!   section table/body layout while preserving the packet's decompile-backed
//!   envelope shape.

use crate::{crc::read_le_u32, packet::m::HighLevel};

mod gff;

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const CHAR_LIST_MAJOR: u8 = 0x11;
const LIST_RESPONSE_MINOR: u8 = 0x02;
const UPDATE_CHAR_RESPONSE_MINOR: u8 = 0x04;
const MAX_CHAR_LIST_FRAGMENT_BYTES: usize = 64;
const MAX_CHAR_LIST_CHARACTERS: u16 = 256;
const MAX_CHAR_LIST_STRING_BYTES: usize = 4096;
const C_RESREF_TEXT_BYTES: usize = 16;
const CLASS_RECORD_BYTES: usize = 5;
const UPDATE_RESPONSE_STATUS_BYTES: usize = 1;
const UPDATE_RESPONSE_BIC_SIZE_BYTES: usize = 4;
const UPDATE_RESPONSE_FIXED_PREFIX_BYTES: usize =
    UPDATE_RESPONSE_STATUS_BYTES + C_RESREF_TEXT_BYTES + UPDATE_RESPONSE_BIC_SIZE_BYTES;
const MAX_REASONABLE_REASSEMBLED_GAMEPLAY_PAYLOAD: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CharListClaimSummary {
    pub kind: CharListClaimKind,
    pub character_count: u16,
    pub declared: usize,
    pub fragment_bytes: usize,
    pub bic_rewritten: bool,
    pub old_bic_size: usize,
    pub new_bic_size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharListClaimKind {
    ListResponse,
    UpdateCharResponse,
}

pub fn claim_payload_if_verified(payload: &mut Vec<u8>) -> Option<CharListClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (CHAR_LIST_MAJOR, LIST_RESPONSE_MINOR) => claim_list_response(payload),
        (CHAR_LIST_MAJOR, UPDATE_CHAR_RESPONSE_MINOR) => translate_update_char_response(payload),
        _ => None,
    }
}

fn claim_list_response(payload: &[u8]) -> Option<CharListClaimSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + 2 {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_CHAR_LIST_FRAGMENT_BYTES
    {
        return None;
    }

    let mut cursor = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    let character_count = read_u16_le(payload, cursor)?;
    if character_count > MAX_CHAR_LIST_CHARACTERS {
        return None;
    }
    cursor += 2;

    for _ in 0..character_count {
        cursor = advance_c_exo_string(payload, cursor, declared)?;
        cursor = advance_c_exo_string(payload, cursor, declared)?;
        cursor = advance_fixed(payload, cursor, declared, C_RESREF_TEXT_BYTES)?;
        cursor = advance_fixed(payload, cursor, declared, 1)?;
        cursor = advance_fixed(payload, cursor, declared, 2)?;
        cursor = advance_fixed(payload, cursor, declared, C_RESREF_TEXT_BYTES)?;
        let class_count = *payload.get(cursor)?;
        cursor += 1;

        let class_bytes = usize::from(class_count).checked_mul(CLASS_RECORD_BYTES)?;
        cursor = advance_fixed(payload, cursor, declared, class_bytes)?;
    }

    if cursor != declared {
        return None;
    }

    Some(CharListClaimSummary {
        kind: CharListClaimKind::ListResponse,
        character_count,
        declared,
        fragment_bytes: payload.len() - declared,
        bic_rewritten: false,
        old_bic_size: 0,
        new_bic_size: 0,
    })
}

fn translate_update_char_response(payload: &mut Vec<u8>) -> Option<CharListClaimSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + UPDATE_RESPONSE_FIXED_PREFIX_BYTES
    {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + UPDATE_RESPONSE_FIXED_PREFIX_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_CHAR_LIST_FRAGMENT_BYTES
    {
        return None;
    }

    let bic_size_offset =
        HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + UPDATE_RESPONSE_STATUS_BYTES + C_RESREF_TEXT_BYTES;
    let bic_offset = bic_size_offset.checked_add(UPDATE_RESPONSE_BIC_SIZE_BYTES)?;
    let old_bic_size = usize::try_from(read_le_u32(payload, bic_size_offset)?).ok()?;
    if old_bic_size < gff::GFF_HEADER_BYTES
        || old_bic_size > MAX_REASONABLE_REASSEMBLED_GAMEPLAY_PAYLOAD
        || bic_offset.checked_add(old_bic_size)? != declared
    {
        return None;
    }

    let bic = payload.get(bic_offset..declared)?;
    let canonical = gff::canonicalize_bic_gff(bic).ok()?;
    let Some(canonical_bic) = canonical.bytes else {
        return Some(CharListClaimSummary {
            kind: CharListClaimKind::UpdateCharResponse,
            character_count: 0,
            declared,
            fragment_bytes: payload.len() - declared,
            bic_rewritten: false,
            old_bic_size,
            new_bic_size: old_bic_size,
        });
    };

    if canonical_bic.len() > MAX_REASONABLE_REASSEMBLED_GAMEPLAY_PAYLOAD {
        return None;
    }
    let new_declared = declared
        .checked_add(canonical_bic.len())?
        .checked_sub(old_bic_size)?;
    if new_declared > MAX_REASONABLE_REASSEMBLED_GAMEPLAY_PAYLOAD {
        return None;
    }

    let old_payload_len = payload.len();
    let mut rewritten =
        Vec::with_capacity(old_payload_len.checked_add(canonical_bic.len())?.checked_sub(old_bic_size)?);
    rewritten.extend_from_slice(&payload[..bic_offset]);
    rewritten.extend_from_slice(&canonical_bic);
    rewritten.extend_from_slice(&payload[declared..]);
    write_le_u32(&mut rewritten, HIGH_LEVEL_HEADER_BYTES, u32::try_from(new_declared).ok()?)?;
    write_le_u32(&mut rewritten, bic_size_offset, u32::try_from(canonical_bic.len()).ok()?)?;
    *payload = rewritten;

    tracing::info!(
        old_declared = declared,
        new_declared,
        old_bic_size,
        new_bic_size = canonical_bic.len(),
        old_layout = ?canonical.old_layout,
        new_layout = ?canonical.new_layout,
        clamped_struct_field_ranges = canonical.clamped_struct_field_ranges,
        normalized_locstring_fields = canonical.normalized_locstring_fields,
        normalized_variable_fields = canonical.normalized_variable_fields,
        "CharList_UpdateCharResponse BIC GFF canonicalized for EE"
    );

    Some(CharListClaimSummary {
        kind: CharListClaimKind::UpdateCharResponse,
        character_count: 0,
        declared: new_declared,
        fragment_bytes: payload.len() - new_declared,
        bic_rewritten: true,
        old_bic_size,
        new_bic_size: canonical_bic.len(),
    })
}

fn advance_c_exo_string(payload: &[u8], cursor: usize, declared: usize) -> Option<usize> {
    let length = usize::try_from(read_le_u32(payload, cursor)?).ok()?;
    if length > MAX_CHAR_LIST_STRING_BYTES {
        return None;
    }
    advance_fixed(payload, cursor.checked_add(CNW_LENGTH_BYTES)?, declared, length)
}

fn advance_fixed(
    payload: &[u8],
    cursor: usize,
    declared: usize,
    byte_count: usize,
) -> Option<usize> {
    let next = cursor.checked_add(byte_count)?;
    if next > declared || next > payload.len() {
        return None;
    }
    Some(next)
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let chunk = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes(chunk.try_into().ok()?))
}

fn write_le_u32(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let target = bytes.get_mut(offset..offset.checked_add(CNW_LENGTH_BYTES)?)?;
    target.copy_from_slice(&value.to_le_bytes());
    Some(())
}
