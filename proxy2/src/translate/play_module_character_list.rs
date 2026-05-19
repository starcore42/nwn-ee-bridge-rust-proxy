//! `PlayModuleCharacterList` semantic claims.
//!
//! Decompile anchors:
//!
//! - EE `CNWSMessage::HandlePlayerToServerPlayModuleCharacterList` dispatches
//!   minor `0x01` to `_Start` and minor `0x02` to `_Stop`.
//! - The handler checks `MessageReadOverflow` before dispatching and does not
//!   read a CNW body for either startup packet.
//! - The EE packet-name table maps `0x3101` and `0x3102` to the same
//!   Start/Stop names. The EE and Diamond shapes are therefore the same
//!   three-byte high-level envelope; this module claims that no-op transform.
//! - EE `CNWSMessage::SendServerToPlayerPlayModuleCharacterListResponse`
//!   sends family `0x31`, minor `0x03` after writing a BOOL result bit, a
//!   DWORD creature/object id, two `WriteCExoLocStringServer` fields, a WORD
//!   portrait id, an optional fixed 16-byte `CResRef` for custom portraits,
//!   then a BYTE class count followed by class/level BYTE pairs.
//!
//! The server response is currently an identity translation: the captured
//! 1.69/HG shape matches the EE reader/writer layout above. It is still routed
//! through this semantic module so strict mode can prove the declared CNW read
//! cursor and fragment-bit cursor exactly before emitting it.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const PLAY_MODULE_CHARACTER_LIST_MAJOR: u8 = 0x31;
const START_MINOR: u8 = 0x01;
const STOP_MINOR: u8 = 0x02;
const RESPONSE_MINOR: u8 = 0x03;
const EMPTY_HIGH_LEVEL_BYTES: usize = 3;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_CURSOR_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const OBJECT_ID_BYTES: usize = 4;
const C_RESREF_TEXT_BYTES: usize = 16;
const CLASS_RECORD_BYTES: usize = 2;
const MAX_FRAGMENT_BYTES: usize = 8;
const MAX_STRING_BYTES: usize = 4096;
const MAX_CLASSES: u8 = 3;

#[derive(Debug, Clone, Copy)]
pub struct PlayModuleCharacterListClaimSummary {
    pub packet_name: &'static str,
    pub kind: PlayModuleCharacterListKind,
    pub declared: usize,
    pub fragment_bytes: usize,
    pub object_id: Option<u32>,
    pub success: Option<bool>,
    pub class_count: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayModuleCharacterListKind {
    Start,
    Stop,
    Response,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<PlayModuleCharacterListClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != PLAY_MODULE_CHARACTER_LIST_MAJOR {
        return None;
    }

    match high.minor {
        START_MINOR if payload.len() == EMPTY_HIGH_LEVEL_BYTES => {
            Some(PlayModuleCharacterListClaimSummary {
                packet_name: "PlayModuleCharacterList_Start",
                kind: PlayModuleCharacterListKind::Start,
                declared: EMPTY_HIGH_LEVEL_BYTES,
                fragment_bytes: 0,
                object_id: None,
                success: None,
                class_count: None,
            })
        }
        STOP_MINOR if payload.len() == EMPTY_HIGH_LEVEL_BYTES => {
            Some(PlayModuleCharacterListClaimSummary {
                packet_name: "PlayModuleCharacterList_Stop",
                kind: PlayModuleCharacterListKind::Stop,
                declared: EMPTY_HIGH_LEVEL_BYTES,
                fragment_bytes: 0,
                object_id: None,
                success: None,
                class_count: None,
            })
        }
        RESPONSE_MINOR => claim_response(payload),
        _ => None,
    }
}

fn claim_response(payload: &[u8]) -> Option<PlayModuleCharacterListClaimSummary> {
    if payload.len() < READ_CURSOR_START + OBJECT_ID_BYTES + 1 {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_CURSOR_START + OBJECT_ID_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
    {
        return None;
    }

    let fragments = payload.get(declared..)?;
    if fragments.is_empty() {
        return None;
    }

    let mut reader = ResponseReader::new(payload, declared, fragments)?;
    let final_fragment_bits = reader.read_bits(3)? as u8;
    if final_fragment_bits == 0 && fragments.len() == 1 {
        return None;
    }

    let success = reader.read_bool()?;
    let object_id = reader.read_u32()?;

    let class_count = if success {
        reader.read_server_locstring()?;
        reader.read_server_locstring()?;
        let portrait_id = reader.read_u16()?;
        if portrait_id >= 0xFFFE {
            reader.skip_bytes(C_RESREF_TEXT_BYTES)?;
        }

        let class_count = reader.read_u8()?;
        if class_count > MAX_CLASSES {
            return None;
        }
        reader.skip_bytes(usize::from(class_count).checked_mul(CLASS_RECORD_BYTES)?)?;
        Some(class_count)
    } else {
        None
    };

    if reader.cursor != declared
        || reader.consumed_fragment_bits() != reader.meaningful_fragment_bits
    {
        return None;
    }

    Some(PlayModuleCharacterListClaimSummary {
        packet_name: "PlayModuleCharacterList_Response",
        kind: PlayModuleCharacterListKind::Response,
        declared,
        fragment_bytes: payload.len() - declared,
        object_id: Some(object_id),
        success: Some(success),
        class_count,
    })
}

#[derive(Debug, Clone)]
struct ResponseReader<'a> {
    read_buffer: &'a [u8],
    declared: usize,
    fragments: &'a [u8],
    meaningful_fragment_bits: usize,
    cursor: usize,
    fragment_cursor: usize,
    fragment_bit: u8,
}

impl<'a> ResponseReader<'a> {
    fn new(read_buffer: &'a [u8], declared: usize, fragments: &'a [u8]) -> Option<Self> {
        Some(Self {
            read_buffer,
            declared,
            fragments,
            meaningful_fragment_bits: meaningful_fragment_bits(fragments)?,
            cursor: READ_CURSOR_START,
            fragment_cursor: 0,
            fragment_bit: 0,
        })
    }

    fn read_bit(&mut self) -> Option<u32> {
        if self.consumed_fragment_bits() >= self.meaningful_fragment_bits {
            return None;
        }
        let byte = *self.fragments.get(self.fragment_cursor)?;
        let bit = (byte >> (7 - self.fragment_bit)) & 1;
        self.fragment_bit += 1;
        if self.fragment_bit == 8 {
            self.fragment_bit = 0;
            self.fragment_cursor += 1;
        }
        Some(u32::from(bit))
    }

    fn read_bits(&mut self, count: u8) -> Option<u32> {
        if count > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | self.read_bit()?;
        }
        Some(value)
    }

    fn read_bool(&mut self) -> Option<bool> {
        Some(self.read_bit()? != 0)
    }

    fn read_u8(&mut self) -> Option<u8> {
        let value = *self.read_buffer.get(self.cursor)?;
        self.cursor = self.cursor.checked_add(1)?;
        if self.cursor > self.declared {
            return None;
        }
        Some(value)
    }

    fn read_u16(&mut self) -> Option<u16> {
        let bytes = self
            .read_buffer
            .get(self.cursor..self.cursor.checked_add(2)?)?;
        self.cursor = self.cursor.checked_add(2)?;
        if self.cursor > self.declared {
            return None;
        }
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Option<u32> {
        let value = read_le_u32(self.read_buffer, self.cursor)?;
        self.cursor = self.cursor.checked_add(4)?;
        if self.cursor > self.declared {
            return None;
        }
        Some(value)
    }

    fn skip_bytes(&mut self, count: usize) -> Option<()> {
        self.cursor = self.cursor.checked_add(count)?;
        if self.cursor > self.declared {
            return None;
        }
        Some(())
    }

    fn read_string(&mut self) -> Option<()> {
        let len = usize::try_from(self.read_u32()?).ok()?;
        if len > MAX_STRING_BYTES {
            return None;
        }
        self.skip_bytes(len)
    }

    fn read_server_locstring(&mut self) -> Option<()> {
        // `WriteCExoLocStringServer` is bit-fronted: the fragment stream first
        // selects a TLK/string-ref shape or an inline `CExoString`. PlayerList
        // uses the same decompile-backed reader shape.
        if self.read_bool()? {
            let _language_selector = self.read_bits(1)?;
            let _string_ref = self.read_u32()?;
        } else {
            self.read_string()?;
        }
        Some(())
    }

    fn consumed_fragment_bits(&self) -> usize {
        self.fragment_cursor * 8 + usize::from(self.fragment_bit)
    }
}

fn meaningful_fragment_bits(fragment_bytes: &[u8]) -> Option<usize> {
    if fragment_bytes.is_empty() {
        return None;
    }
    let final_fragment_bits = (u32::from((fragment_bytes[0] & 0x80) != 0) << 2)
        | (u32::from((fragment_bytes[0] & 0x40) != 0) << 1)
        | u32::from((fragment_bytes[0] & 0x20) != 0);
    let meaningful_bits = if final_fragment_bits == 0 {
        fragment_bytes.len().checked_mul(8)?
    } else {
        fragment_bytes
            .len()
            .checked_sub(1)?
            .checked_mul(8)?
            .checked_add(usize::try_from(final_fragment_bits).ok()?)?
    };
    if meaningful_bits < 3 || meaningful_bits > fragment_bytes.len().checked_mul(8)? {
        return None;
    }
    Some(meaningful_bits)
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn starcore_response_fixture_matches_decompile_cursor_shape() {
        let fixture =
            include_bytes!("../../fixtures/play_module_character_list/starcore_response_jaxxs.bin");
        let summary = claim_payload_if_verified(fixture).expect("fixture should be claimed");

        assert_eq!(summary.kind, PlayModuleCharacterListKind::Response);
        assert_eq!(summary.packet_name, "PlayModuleCharacterList_Response");
        assert_eq!(summary.declared, 71);
        assert_eq!(summary.fragment_bytes, 1);
        assert_eq!(summary.object_id, Some(0x7FFF_FFF9));
        assert_eq!(summary.success, Some(true));
        assert_eq!(summary.class_count, Some(3));
    }

    #[test]
    fn start_and_stop_are_exact_empty_high_level_packets() {
        assert_eq!(
            claim_payload_if_verified(&[b'P', PLAY_MODULE_CHARACTER_LIST_MAJOR, START_MINOR])
                .map(|summary| summary.kind),
            Some(PlayModuleCharacterListKind::Start)
        );
        assert_eq!(
            claim_payload_if_verified(&[b'P', PLAY_MODULE_CHARACTER_LIST_MAJOR, STOP_MINOR])
                .map(|summary| summary.kind),
            Some(PlayModuleCharacterListKind::Stop)
        );
        assert!(
            claim_payload_if_verified(&[b'P', PLAY_MODULE_CHARACTER_LIST_MAJOR, START_MINOR, 0x00])
                .is_none()
        );
    }
}
