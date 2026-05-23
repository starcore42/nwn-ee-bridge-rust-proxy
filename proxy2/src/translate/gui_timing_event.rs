//! Server-originated `GuiTimingEvent_Info` (`0x30/0x01`) semantic claim.
//!
//! Decompile evidence:
//! - EE server writer `CNWSMessage::SendServerToPlayerGuiTimingEvent`
//!   (`nwn ee decompile.txt:1848729`) creates a CNW message, writes one
//!   `BOOL`, and only when that BOOL is true writes a 32-bit timer id plus an
//!   eight-bit timing-event type before sending family `0x30`, minor `0x01`.
//! - EE client handler `sub_14079ECD0` dispatches minor `0x01` to
//!   `sub_14079EE50` (`nwn ee decompile.txt:2895232`), whose reader consumes
//!   `ReadBOOL` and conditionally `ReadDWORD(32)` plus `ReadBYTE(8, 1)`.
//! - Local Diamond harness evidence hit both decompile branches: false
//!   status with only the fragment BOOL, and true status with the five-byte
//!   DWORD/BYTE read window plus the same single-BOOL fragment cursor.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const GUI_TIMING_MAJOR: u8 = 0x30;
const INFO_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const TIMER_ID_BYTES: usize = 4;
const TIMER_TYPE_BYTES: usize = 1;
const FALSE_DECLARED_BYTES: usize = READ_START;
const TRUE_DECLARED_BYTES: usize = READ_START + TIMER_ID_BYTES + TIMER_TYPE_BYTES;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const FRAGMENT_CURSOR_MASK: u8 = 0xE0;
const SINGLE_BOOL_FINAL_CURSOR: u8 = 0x80;
const SINGLE_BOOL_DATA_BIT: u8 = 0x10;

#[derive(Debug, Clone, Copy)]
pub struct GuiTimingEventClaimSummary {
    pub active: bool,
    pub timer_id: Option<u32>,
    pub timer_type: Option<u8>,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<GuiTimingEventClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != GUI_TIMING_MAJOR || high.minor != INFO_MINOR {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)? {
        return None;
    }
    let active = decode_single_bool_fragment(*payload.get(declared)?)?;
    match (active, declared) {
        (false, FALSE_DECLARED_BYTES) => Some(GuiTimingEventClaimSummary {
            active,
            timer_id: None,
            timer_type: None,
            declared,
            fragment_bytes: SINGLE_FRAGMENT_BYTE,
        }),
        (true, TRUE_DECLARED_BYTES) => Some(GuiTimingEventClaimSummary {
            active,
            timer_id: Some(read_le_u32(payload, READ_START)?),
            timer_type: payload.get(READ_START + TIMER_ID_BYTES).copied(),
            declared,
            fragment_bytes: SINGLE_FRAGMENT_BYTE,
        }),
        _ => None,
    }
}

fn decode_single_bool_fragment(byte: u8) -> Option<bool> {
    if byte & FRAGMENT_CURSOR_MASK != SINGLE_BOOL_FINAL_CURSOR {
        return None;
    }
    Some(byte & SINGLE_BOOL_DATA_BIT != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_local_gui_timing_event_start_shape() {
        let payload = [
            0x50, 0x30, 0x01, 0x0C, 0x00, 0x00, 0x00, 0x5C, 0x44, 0x00, 0x00, 0x06, 0x9B,
        ];

        let claim = claim_payload_if_verified(&payload)
            .expect("observed GUI timing start event should claim");

        assert!(claim.active);
        assert_eq!(claim.timer_id, Some(0x0000_445C));
        assert_eq!(claim.timer_type, Some(0x06));
        assert_eq!(claim.declared, TRUE_DECLARED_BYTES);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn claims_local_gui_timing_event_stop_shape() {
        let payload = [0x50, 0x30, 0x01, 0x07, 0x00, 0x00, 0x00, 0x80];

        let claim = claim_payload_if_verified(&payload)
            .expect("observed GUI timing stop event should claim");

        assert!(!claim.active);
        assert_eq!(claim.timer_id, None);
        assert_eq!(claim.timer_type, None);
        assert_eq!(claim.declared, FALSE_DECLARED_BYTES);
    }

    #[test]
    fn rejects_mismatched_bool_branch_and_declared_length() {
        let false_with_true_length = [
            0x50, 0x30, 0x01, 0x0C, 0x00, 0x00, 0x00, 0x5C, 0x44, 0x00, 0x00, 0x06, 0x80,
        ];
        assert!(claim_payload_if_verified(&false_with_true_length).is_none());

        let true_with_false_length = [0x50, 0x30, 0x01, 0x07, 0x00, 0x00, 0x00, 0x90];
        assert!(claim_payload_if_verified(&true_with_false_length).is_none());
    }
}
