//! Camera server payload claims.
//!
//! EE's packet-name table maps family `0x10` to the camera messages. The
//! inspected EE server writers for `Camera_ChangeLocation` and
//! `Camera_SetMode` both create a CNW write message, write only fixed
//! read-buffer scalar fields, and send high-level family `0x10` with minors
//! `0x01` and `0x02`. Local Diamond XP2 traffic emits the same declared read
//! windows and the same empty CNW fragment terminator, so these two currently
//! observed camera messages are exact no-op claims after model round-trip
//! validation.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const CAMERA_MAJOR: u8 = 0x10;
const CHANGE_LOCATION_MINOR: u8 = 0x01;
const SET_MODE_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const EMPTY_CNW_FRAGMENT_TERMINATOR: u8 = 0x60;
const CHANGE_LOCATION_MASK_BITS: u8 = 0x0F;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub read_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CameraMessage {
    ChangeLocation(CameraChangeLocation),
    SetMode { mode: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CameraChangeLocation {
    mask: u8,
    x_bits: Option<u32>,
    y_bits: Option<u32>,
    z_bits: Option<u32>,
    instant: Option<i32>,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<CameraClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != CAMERA_MAJOR {
        return None;
    }

    let (message, declared) = parse_camera_message(payload, high.minor)?;
    let rewritten = message.to_ee_payload();
    if rewritten != payload {
        return None;
    }

    Some(CameraClaimSummary {
        minor: high.minor,
        declared,
        read_bytes: declared - READ_START,
    })
}

fn parse_camera_message(payload: &[u8], minor: u8) -> Option<(CameraMessage, usize)> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START
        || declared >= payload.len()
        || payload.len() != declared + 1
        || payload[declared] != EMPTY_CNW_FRAGMENT_TERMINATOR
    {
        return None;
    }

    let message = match minor {
        CHANGE_LOCATION_MINOR => parse_change_location(payload, declared)?,
        SET_MODE_MINOR => parse_set_mode(payload, declared)?,
        _ => return None,
    };

    Some((message, declared))
}

fn parse_change_location(payload: &[u8], declared: usize) -> Option<CameraMessage> {
    let mut cursor = READ_START;
    let mask = *payload.get(cursor)?;
    cursor += 1;
    if (mask & !CHANGE_LOCATION_MASK_BITS) != 0 {
        return None;
    }

    let x_bits = read_optional_u32(payload, declared, &mut cursor, mask, 0x01)?;
    let y_bits = read_optional_u32(payload, declared, &mut cursor, mask, 0x02)?;
    let z_bits = read_optional_u32(payload, declared, &mut cursor, mask, 0x04)?;
    let instant = read_optional_u32(payload, declared, &mut cursor, mask, 0x08)?
        .map(|bits| i32::from_le_bytes(bits.to_le_bytes()));
    if cursor != declared {
        return None;
    }

    Some(CameraMessage::ChangeLocation(CameraChangeLocation {
        mask,
        x_bits,
        y_bits,
        z_bits,
        instant,
    }))
}

fn parse_set_mode(payload: &[u8], declared: usize) -> Option<CameraMessage> {
    if declared != READ_START + 1 {
        return None;
    }
    Some(CameraMessage::SetMode {
        mode: *payload.get(READ_START)?,
    })
}

fn read_optional_u32(
    payload: &[u8],
    declared: usize,
    cursor: &mut usize,
    mask: u8,
    bit: u8,
) -> Option<Option<u32>> {
    if (mask & bit) == 0 {
        return Some(None);
    }
    let end = cursor.checked_add(4)?;
    if end > declared {
        return None;
    }
    let value = read_le_u32(payload, *cursor)?;
    *cursor = end;
    Some(Some(value))
}

impl CameraMessage {
    fn to_ee_payload(&self) -> Vec<u8> {
        let (minor, read_window) = match self {
            Self::ChangeLocation(change) => (CHANGE_LOCATION_MINOR, change.read_window()),
            Self::SetMode { mode } => (SET_MODE_MINOR, vec![*mode]),
        };

        let declared = READ_START + read_window.len();
        let mut payload = Vec::with_capacity(declared + 1);
        payload.extend_from_slice(&[b'P', CAMERA_MAJOR, minor]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&read_window);
        payload.push(EMPTY_CNW_FRAGMENT_TERMINATOR);
        payload
    }
}

impl CameraChangeLocation {
    fn read_window(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 4 * 4);
        out.push(self.mask);
        push_optional_u32(&mut out, self.x_bits);
        push_optional_u32(&mut out, self.y_bits);
        push_optional_u32(&mut out, self.z_bits);
        push_optional_i32(&mut out, self.instant);
        out
    }
}

fn push_optional_u32(out: &mut Vec<u8>, value: Option<u32>) {
    if let Some(value) = value {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_optional_i32(out: &mut Vec<u8>, value: Option<i32>) {
    if let Some(value) = value {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_local_xp2_camera_change_location_shape() {
        let payload = [
            0x50, 0x10, 0x01, 0x18, 0x00, 0x00, 0x00, 0x0F, 0xDF, 0x66, 0xF3, 0x3F, 0x00, 0x00,
            0x20, 0x41, 0x00, 0x00, 0x48, 0x42, 0x00, 0x00, 0x00, 0x00, 0x60,
        ];

        let claim = claim_payload_if_verified(&payload).expect("camera change should claim");

        assert_eq!(claim.minor, CHANGE_LOCATION_MINOR);
        assert_eq!(claim.declared, 0x18);
        assert_eq!(claim.read_bytes, 17);
    }

    #[test]
    fn claims_local_xp2_camera_set_mode_shape() {
        let payload = [0x50, 0x10, 0x02, 0x08, 0x00, 0x00, 0x00, 0x01, 0x60];

        let claim = claim_payload_if_verified(&payload).expect("camera mode should claim");

        assert_eq!(claim.minor, SET_MODE_MINOR);
        assert_eq!(claim.declared, 0x08);
        assert_eq!(claim.read_bytes, 1);
    }

    #[test]
    fn rejects_change_location_with_stale_declared_length() {
        let payload = [
            0x50, 0x10, 0x01, 0x17, 0x00, 0x00, 0x00, 0x0F, 0xDF, 0x66, 0xF3, 0x3F, 0x00, 0x00,
            0x20, 0x41, 0x00, 0x00, 0x48, 0x42, 0x00, 0x00, 0x00, 0x00, 0x60,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_set_mode_without_empty_fragment_terminator() {
        let payload = [0x50, 0x10, 0x02, 0x08, 0x00, 0x00, 0x00, 0x01];

        assert!(claim_payload_if_verified(&payload).is_none());
    }
}
