//! `Area_ChangeDayNight` exact semantic claim.
//!
//! Decompile evidence:
//! - EE's packet-name table maps `0x04/0x06` to `Area_ChangeDayNight`
//!   (`nwn ee decompile.txt:1099660`).
//! - EE `CNWSMessage::SendServerToPlayerArea_ChangeDayNight`
//!   (`nwn ee decompile.txt:1831211..1831274`) creates an 8-byte CNW write
//!   message, writes one `BOOL` day/night selector, then writes one 32-bit
//!   `FLOAT` transition amount before sending family `0x04`, minor `0x06`.
//! - The local Diamond XP2 Chapter 3 capture uses the same receiver shape:
//!   declared offset `0x0B`, a four-byte finite float read window, and one
//!   fragment byte with `3 + 1` meaningful bits. This module claims only that
//!   exact shared shape; it does not rewrite or pass through unknown area
//!   control layouts.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const AREA_MAJOR: u8 = 0x04;
const CHANGE_DAY_NIGHT_MINOR: u8 = 0x06;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const FLOAT_BYTES: usize = 4;
const DECLARED_BYTES: usize = READ_START + FLOAT_BYTES;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const BOOL_BITS: usize = 1;
const SINGLE_BOOL_FINAL_BITS: usize = CNW_FRAGMENT_HEADER_BITS + BOOL_BITS;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaChangeDayNightClaimSummary {
    pub day_night: bool,
    pub transition: f32,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<AreaChangeDayNightClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != AREA_MAJOR || high.minor != CHANGE_DAY_NIGHT_MINOR {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != DECLARED_BYTES || payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)? {
        return None;
    }

    let transition_bits = read_le_u32(payload, READ_START)?;
    let transition = f32::from_bits(transition_bits);
    if !transition.is_finite() {
        return None;
    }

    let fragment = *payload.get(declared)?;
    if usize::from((fragment & 0xE0) >> 5) != SINGLE_BOOL_FINAL_BITS {
        return None;
    }

    Some(AreaChangeDayNightClaimSummary {
        day_night: (fragment & 0x10) != 0,
        transition,
        declared,
        fragment_bytes: SINGLE_FRAGMENT_BYTE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_exact_ee_area_change_day_night_shape() {
        let payload = [
            b'P',
            AREA_MAJOR,
            CHANGE_DAY_NIGHT_MINOR,
            0x0B,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x9E,
        ];

        let claim = claim_payload_if_verified(&payload)
            .expect("Area_ChangeDayNight exact CNW shape should claim");
        assert!(claim.day_night);
        assert_eq!(claim.transition, 0.0);
        assert_eq!(claim.declared, DECLARED_BYTES);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_area_change_day_night_without_bool_fragment_cursor() {
        let payload = [
            b'P',
            AREA_MAJOR,
            CHANGE_DAY_NIGHT_MINOR,
            0x0B,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x7E,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp2_chapter3_area_change_day_night_fixture_claims() {
        let payload =
            include_bytes!("../../fixtures/area/local_xp2_chapter3_p0406_unknown_20260523.bin");

        let claim = claim_payload_if_verified(payload)
            .expect("local XP2 Chapter 3 Area_ChangeDayNight fixture should claim exactly");
        assert!(claim.day_night);
        assert_eq!(claim.transition, 0.0);
    }
}
