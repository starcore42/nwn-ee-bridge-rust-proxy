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
//! - EE `CNWCMessage::HandleServerToPlayerCharacterList` dispatches minor
//!   `0x02` through `ReadCExoLocStringClient` for those two name fields, then
//!   reads `CResRef(16)`, `BYTE(8)`, `WORD(16)`, `CResRef(16)`, and up to
//!   eight class `INT(32)`/`BYTE(8)` pairs before asserting that no message
//!   overflow or underflow occurred.
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
const MAX_CHAR_LIST_CLASSES: u8 = 8;
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

    let fragments = payload.get(declared..)?;
    let mut reader = CharListListResponseReader::new(payload, declared, fragments)?;
    let character_count = reader.read_u16()?;
    if character_count > MAX_CHAR_LIST_CHARACTERS {
        return None;
    }

    for _ in 0..character_count {
        reader.read_server_locstring()?;
        reader.read_server_locstring()?;
        reader.skip_bytes(C_RESREF_TEXT_BYTES)?;
        reader.skip_bytes(1)?;
        reader.skip_bytes(2)?;
        reader.skip_bytes(C_RESREF_TEXT_BYTES)?;
        let class_count = reader.read_u8()?;
        if class_count > MAX_CHAR_LIST_CLASSES {
            return None;
        }
        let class_bytes = usize::from(class_count).checked_mul(CLASS_RECORD_BYTES)?;
        reader.skip_bytes(class_bytes)?;
    }

    if !reader.finished_exactly() {
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

/// Exact `CharList_ListResponse` read cursor for the EE client locstring form.
///
/// `CNWSMessage::WriteCExoLocStringServer` packetizes the TLK/string selector
/// bits, and the EE client consumes them through
/// `CNWMessage::ReadCExoLocStringClient`. The previous bridge reader walked two
/// plain `CExoString`s and ignored those bits, which let a structurally
/// different list response through as "known". This reader keeps the byte
/// cursor and fragment cursor coupled: a packet is claimed only when both
/// cursors end exactly on the declared decompile-backed boundary.
struct CharListListResponseReader<'a> {
    read_buffer: &'a [u8],
    declared: usize,
    fragments: &'a [u8],
    meaningful_fragment_bits: usize,
    cursor: usize,
    fragment_cursor: usize,
    fragment_bit: u8,
}

impl<'a> CharListListResponseReader<'a> {
    fn new(read_buffer: &'a [u8], declared: usize, fragments: &'a [u8]) -> Option<Self> {
        if fragments.is_empty() {
            return None;
        }

        let mut reader = Self {
            read_buffer,
            declared,
            fragments,
            meaningful_fragment_bits: 0,
            cursor: HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES,
            fragment_cursor: 0,
            fragment_bit: 0,
        };
        let final_fragment_bits = reader.read_bits(3)? as usize;
        reader.meaningful_fragment_bits = if final_fragment_bits == 0 {
            fragments.len().checked_mul(8)?
        } else {
            fragments
                .len()
                .checked_sub(1)?
                .checked_mul(8)?
                .checked_add(final_fragment_bits)?
        };
        if reader.meaningful_fragment_bits < 3 {
            return None;
        }
        Some(reader)
    }

    fn read_u8(&mut self) -> Option<u8> {
        let next = self.cursor.checked_add(1)?;
        if next > self.declared {
            return None;
        }
        let value = *self.read_buffer.get(self.cursor)?;
        self.cursor = next;
        Some(value)
    }

    fn read_u16(&mut self) -> Option<u16> {
        let next = self.cursor.checked_add(2)?;
        if next > self.declared {
            return None;
        }
        let chunk = self.read_buffer.get(self.cursor..next)?;
        self.cursor = next;
        Some(u16::from_le_bytes(chunk.try_into().ok()?))
    }

    fn read_u32(&mut self) -> Option<u32> {
        let next = self.cursor.checked_add(CNW_LENGTH_BYTES)?;
        if next > self.declared {
            return None;
        }
        let chunk = self.read_buffer.get(self.cursor..next)?;
        self.cursor = next;
        Some(u32::from_le_bytes(chunk.try_into().ok()?))
    }

    fn read_string(&mut self) -> Option<()> {
        let len = usize::try_from(self.read_u32()?).ok()?;
        if len > MAX_CHAR_LIST_STRING_BYTES {
            return None;
        }
        self.skip_bytes(len)
    }

    fn read_server_locstring(&mut self) -> Option<()> {
        let is_client_tlk = self.read_bool()?;
        if is_client_tlk {
            let _language_selector = self.read_bool()?;
            let _str_ref = self.read_u32()?;
        } else {
            self.read_string()?;
        }
        Some(())
    }

    fn skip_bytes(&mut self, byte_count: usize) -> Option<()> {
        let next = self.cursor.checked_add(byte_count)?;
        if next > self.declared || next > self.read_buffer.len() {
            return None;
        }
        self.cursor = next;
        Some(())
    }

    fn read_bool(&mut self) -> Option<bool> {
        Some(self.read_bits(1)? != 0)
    }

    fn read_bits(&mut self, count: u8) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            if self.consumed_fragment_bits() >= self.fragments.len().checked_mul(8)? {
                return None;
            }
            let byte = *self.fragments.get(self.fragment_cursor)?;
            let bit = (byte >> (7 - self.fragment_bit)) & 1;
            value = (value << 1) | u32::from(bit);
            self.fragment_bit += 1;
            if self.fragment_bit == 8 {
                self.fragment_bit = 0;
                self.fragment_cursor += 1;
            }
        }
        Some(value)
    }

    fn consumed_fragment_bits(&self) -> usize {
        self.fragment_cursor
            .checked_mul(8)
            .and_then(|bits| bits.checked_add(usize::from(self.fragment_bit)))
            .unwrap_or(usize::MAX)
    }

    fn finished_exactly(&self) -> bool {
        self.cursor == self.declared
            && self.consumed_fragment_bits() == self.meaningful_fragment_bits
    }
}

fn translate_update_char_response(payload: &mut Vec<u8>) -> Option<CharListClaimSummary> {
    if payload.len()
        < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + UPDATE_RESPONSE_FIXED_PREFIX_BYTES
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

    let bic_size_offset = HIGH_LEVEL_HEADER_BYTES
        + CNW_LENGTH_BYTES
        + UPDATE_RESPONSE_STATUS_BYTES
        + C_RESREF_TEXT_BYTES;
    let bic_offset = bic_size_offset.checked_add(UPDATE_RESPONSE_BIC_SIZE_BYTES)?;
    let old_bic_size = usize::try_from(read_le_u32(payload, bic_size_offset)?).ok()?;
    if old_bic_size < gff::GFF_HEADER_BYTES
        || old_bic_size > MAX_REASONABLE_REASSEMBLED_GAMEPLAY_PAYLOAD
        || bic_offset.checked_add(old_bic_size)? != declared
    {
        return None;
    }

    let bic = payload.get(bic_offset..declared)?;
    let canonical = match gff::canonicalize_bic_gff(bic) {
        Ok(summary) => summary,
        Err(reason) => {
            tracing::warn!(
                declared,
                old_bic_size,
                reason = %reason,
                "CharList_UpdateCharResponse BIC GFF rejected by exact canonicalizer"
            );
            return None;
        }
    };
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
    let mut rewritten = Vec::with_capacity(
        old_payload_len
            .checked_add(canonical_bic.len())?
            .checked_sub(old_bic_size)?,
    );
    rewritten.extend_from_slice(&payload[..bic_offset]);
    rewritten.extend_from_slice(&canonical_bic);
    rewritten.extend_from_slice(&payload[declared..]);
    write_le_u32(
        &mut rewritten,
        HIGH_LEVEL_HEADER_BYTES,
        u32::try_from(new_declared).ok()?,
    )?;
    write_le_u32(
        &mut rewritten,
        bic_size_offset,
        u32::try_from(canonical_bic.len()).ok()?,
    )?;
    *payload = rewritten;

    tracing::info!(
        old_declared = declared,
        new_declared,
        old_bic_size,
        new_bic_size = canonical_bic.len(),
        old_layout = ?canonical.old_layout,
        new_layout = ?canonical.new_layout,
        repaired_legacy_section_offsets = canonical.repaired_legacy_section_offsets,
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

fn write_le_u32(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let target = bytes.get_mut(offset..offset.checked_add(CNW_LENGTH_BYTES)?)?;
    target.copy_from_slice(&value.to_le_bytes());
    Some(())
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    fn push_c_exo_string(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn push_resref(out: &mut Vec<u8>, value: &str) {
        let mut resref = [0u8; C_RESREF_TEXT_BYTES];
        let bytes = value.as_bytes();
        let copy_len = bytes.len().min(C_RESREF_TEXT_BYTES);
        resref[..copy_len].copy_from_slice(&bytes[..copy_len]);
        out.extend_from_slice(&resref);
    }

    fn push_character(read: &mut Vec<u8>, first_name: &str, bic_resref: &str) {
        push_c_exo_string(read, first_name);
        push_c_exo_string(read, "");
        push_resref(read, "po_heurodis_");
        read.push(0);
        read.extend_from_slice(&4u16.to_le_bytes());
        push_resref(read, bic_resref);
        read.push(1);
        read.extend_from_slice(&37u32.to_le_bytes());
        read.push(40);
    }

    fn build_list_response_fixture(character_count: u16, fragment_header: u8) -> Vec<u8> {
        let mut read = Vec::new();
        read.extend_from_slice(&character_count.to_le_bytes());
        push_character(&mut read, "Starcore-Druid 6.0", "starcore-druid60");
        if character_count > 1 {
            push_character(&mut read, "Starcore-Wizard 6.0", "starcore-wiz60");
        }

        let declared = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + read.len();
        let mut payload = vec![b'P', CHAR_LIST_MAJOR, LIST_RESPONSE_MINOR];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&read);
        payload.push(fragment_header);
        payload
    }

    #[test]
    fn claims_list_response_with_server_locstring_fragment_bits() {
        let mut payload = build_list_response_fixture(2, 0b1110_0000);

        let summary = claim_payload_if_verified(&mut payload)
            .expect("server-locstring list response should be claimed exactly");

        assert_eq!(summary.kind, CharListClaimKind::ListResponse);
        assert_eq!(summary.character_count, 2);
        assert_eq!(summary.fragment_bytes, 1);
        assert!(!summary.bic_rewritten);
    }

    #[test]
    fn rejects_list_response_without_fragment_proof() {
        let mut payload = build_list_response_fixture(1, 0b1010_0000);
        payload.pop();

        assert!(
            claim_payload_if_verified(&mut payload).is_none(),
            "CharList_ListResponse must not be claimed without server-locstring fragment bits"
        );
    }

    #[test]
    fn rejects_list_response_with_padding_fragment_bits() {
        let mut payload = build_list_response_fixture(1, 0b1110_0000);

        assert!(
            claim_payload_if_verified(&mut payload).is_none(),
            "CharList_ListResponse must consume the exact declared fragment-bit count"
        );
    }

    #[test]
    fn starcore5_update_response_fixture_canonicalizes_sparse_bic() {
        let mut payload = include_bytes!(
            "../../fixtures/char_list/starcore5_update_char_response_sparse_bic.bin"
        )
        .to_vec();
        let old_len = payload.len();

        let summary = match claim_payload_if_verified(&mut payload) {
            Some(summary) => summary,
            None => {
                let declared =
                    usize::try_from(read_le_u32(&payload, HIGH_LEVEL_HEADER_BYTES).unwrap())
                        .unwrap();
                let bic_size_offset = HIGH_LEVEL_HEADER_BYTES
                    + CNW_LENGTH_BYTES
                    + UPDATE_RESPONSE_STATUS_BYTES
                    + C_RESREF_TEXT_BYTES;
                let bic_offset = bic_size_offset + UPDATE_RESPONSE_BIC_SIZE_BYTES;
                let old_bic_size =
                    usize::try_from(read_le_u32(&payload, bic_size_offset).unwrap()).unwrap();
                let reason = gff::canonicalize_bic_gff(&payload[bic_offset..declared])
                    .err()
                    .unwrap_or_else(|| {
                        "outer CharList envelope rejected after GFF accepted".into()
                    });
                panic!(
                    "captured Starcore5 UpdateCharResponse should be claimed: bic_size={old_bic_size} reason={reason}"
                );
            }
        };

        assert_eq!(summary.kind, CharListClaimKind::UpdateCharResponse);
        assert_eq!(summary.old_bic_size, 45_255);
        assert!(summary.bic_rewritten);
        assert!(summary.new_bic_size < summary.old_bic_size);
        assert!(payload.len() < old_len);

        let mut second_pass = payload.clone();
        let second = claim_payload_if_verified(&mut second_pass)
            .expect("canonicalized UpdateCharResponse should still validate exactly");
        assert_eq!(second.kind, CharListClaimKind::UpdateCharResponse);
        assert!(!second.bic_rewritten);
        assert_eq!(second.old_bic_size, summary.new_bic_size);
    }
}
