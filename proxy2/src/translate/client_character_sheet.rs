//! Client-originated `GuiCharacterSheet_Status` (`0x15/0x01`) semantic claim.
//!
//! Decompile evidence:
//! - EE client writer `sub_1407BDDD0` (`nwn ee decompile.txt:2938760`)
//!   creates a five-byte CNW write message, writes `WriteCHAR(8)` followed by
//!   a raw 32-bit object id (`sub_1409737F0`), then sends family `0x15`,
//!   minor `0x01`.
//! - EE server reader `CNWSMessage::HandlePlayerToServerCharacterSheetMessage`
//!   (`nwn ee decompile.txt:1639034`) reads the same `CHAR` and
//!   `ReadOBJECTIDServer` before updating the displayed character sheet.
//! - Local Diamond harness evidence produced the same declared five-byte read
//!   window plus one byte-aligned CNW fragment tail for both open (`0x00`) and
//!   close (`0xFF`) status values.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const CHARACTER_SHEET_MAJOR: u8 = 0x15;
const STATUS_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const STATUS_READ_BYTES: usize = 1;
const OBJECT_ID_BYTES: usize = 4;
const STATUS_DECLARED_BYTES: usize = READ_START + STATUS_READ_BYTES + OBJECT_ID_BYTES;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct ClientCharacterSheetClaimSummary {
    pub packet_name: &'static str,
    pub status: u8,
    pub object_id: u32,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientCharacterSheetClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != CHARACTER_SHEET_MAJOR || high.minor != STATUS_MINOR {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != STATUS_DECLARED_BYTES {
        return None;
    }
    let _fragment_tail = exact_single_empty_fragment_tail(payload, declared)?;
    let status = *payload.get(READ_START)?;
    let object_id = read_le_u32(payload, READ_START + STATUS_READ_BYTES)?;

    Some(ClientCharacterSheetClaimSummary {
        packet_name: "GuiCharacterSheet_Status",
        status,
        object_id,
        declared,
        fragment_bytes: SINGLE_FRAGMENT_BYTE,
    })
}

fn exact_single_empty_fragment_tail(payload: &[u8], declared: usize) -> Option<u8> {
    if payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)? {
        return None;
    }
    let tail = *payload.get(declared)?;
    let final_bits = usize::from((tail & 0xE0) >> 5);
    (final_bits == CNW_FRAGMENT_HEADER_BITS).then_some(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_local_character_sheet_open_status_shape() {
        let payload = [
            0x70, 0x15, 0x01, 0x0C, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0xFF, 0xFF, 0x7C,
        ];

        let claim = claim_payload_if_verified(&payload)
            .expect("observed character-sheet open status should claim");

        assert_eq!(claim.packet_name, "GuiCharacterSheet_Status");
        assert_eq!(claim.status, 0x00);
        assert_eq!(claim.object_id, 0xFFFF_FFFE);
        assert_eq!(claim.declared, STATUS_DECLARED_BYTES);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn claims_local_character_sheet_close_status_shape() {
        let payload = [
            0x70, 0x15, 0x01, 0x0C, 0x00, 0x00, 0x00, 0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0x6C,
        ];

        let claim = claim_payload_if_verified(&payload)
            .expect("observed character-sheet close status should claim");

        assert_eq!(claim.status, 0xFF);
        assert_eq!(claim.object_id, 0xFFFF_FFFE);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_wrong_declared_or_fragment_cursor() {
        let wrong_declared = [
            0x70, 0x15, 0x01, 0x0B, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0xFF, 0xFF, 0x7C,
        ];
        assert!(claim_payload_if_verified(&wrong_declared).is_none());

        let wrong_fragment = [
            0x70, 0x15, 0x01, 0x0C, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0xFF, 0xFF, 0x80,
        ];
        assert!(claim_payload_if_verified(&wrong_fragment).is_none());
    }
}
