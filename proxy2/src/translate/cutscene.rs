//! Cutscene server payload claims.
//!
//! EE's packet-name table maps family `0x33` to cutscene messages. The EE
//! `Status` writer creates an empty byte window and writes two fragment BOOLs,
//! the `Fade*` writers create a four-byte CNW write message and write exactly
//! one float, `HideGui` writes one fragment BOOL, and `StopFade`/`BlackScreen`
//! send a null payload pointer with zero payload length. Diamond's adjacent
//! send wrappers use the same major/minor and payload-pointer/length contract,
//! so the observed local XP2 cutscene messages are exact no-op claims after
//! model round-trip validation.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const CUTSCENE_MAJOR: u8 = 0x33;
const STATUS_MINOR: u8 = 0x01;
const FADE_TO_BLACK_MINOR: u8 = 0x03;
const FADE_FROM_BLACK_MINOR: u8 = 0x04;
const STOP_FADE_MINOR: u8 = 0x05;
const BLACK_SCREEN_MINOR: u8 = 0x06;
const HIDE_GUI_MINOR: u8 = 0x07;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const FLOAT_BYTES: usize = 4;
const FADE_DECLARED: usize = READ_START + FLOAT_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutsceneClaimSummary {
    pub minor: u8,
    pub packet_name: &'static str,
    pub declared: Option<usize>,
    pub read_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CutsceneMessage {
    Status {
        mode_enabled: bool,
        secondary: bool,
        fragment_tail: u8,
    },
    FadeToBlack {
        duration_bits: u32,
        fragment_tail: u8,
    },
    FadeFromBlack {
        duration_bits: u32,
        fragment_tail: u8,
    },
    StopFade,
    BlackScreen,
    HideGui {
        hidden: bool,
        fragment_tail: u8,
    },
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<CutsceneClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != CUTSCENE_MAJOR {
        return None;
    }

    let message = parse_cutscene_message(payload, high.minor)?;
    let rewritten = message.to_ee_payload();
    if rewritten != payload {
        return None;
    }

    Some(CutsceneClaimSummary {
        minor: high.minor,
        packet_name: message.packet_name(),
        declared: message.declared(),
        read_bytes: message.read_bytes(),
    })
}

fn parse_cutscene_message(payload: &[u8], minor: u8) -> Option<CutsceneMessage> {
    match minor {
        STATUS_MINOR => parse_status_payload(payload),
        FADE_TO_BLACK_MINOR => parse_fade_payload(payload, minor),
        FADE_FROM_BLACK_MINOR => parse_fade_payload(payload, minor),
        STOP_FADE_MINOR => parse_empty_payload(payload, minor),
        BLACK_SCREEN_MINOR => parse_empty_payload(payload, minor),
        HIDE_GUI_MINOR => parse_hide_gui_payload(payload),
        _ => None,
    }
}

fn parse_status_payload(payload: &[u8]) -> Option<CutsceneMessage> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != READ_START {
        return None;
    }
    let fragment_tail = exact_single_fragment_tail(payload, declared, 2)?;
    Some(CutsceneMessage::Status {
        mode_enabled: read_fragment_bool(fragment_tail, 0)?,
        secondary: read_fragment_bool(fragment_tail, 1)?,
        fragment_tail,
    })
}

fn parse_fade_payload(payload: &[u8], minor: u8) -> Option<CutsceneMessage> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != FADE_DECLARED {
        return None;
    }
    let fragment_tail = exact_single_fragment_tail(payload, declared, 0)?;

    let duration_bits = read_le_u32(payload, READ_START)?;
    match minor {
        FADE_TO_BLACK_MINOR => Some(CutsceneMessage::FadeToBlack {
            duration_bits,
            fragment_tail,
        }),
        FADE_FROM_BLACK_MINOR => Some(CutsceneMessage::FadeFromBlack {
            duration_bits,
            fragment_tail,
        }),
        _ => None,
    }
}

fn parse_empty_payload(payload: &[u8], minor: u8) -> Option<CutsceneMessage> {
    if payload != [b'P', CUTSCENE_MAJOR, minor] {
        return None;
    }
    match minor {
        STOP_FADE_MINOR => Some(CutsceneMessage::StopFade),
        BLACK_SCREEN_MINOR => Some(CutsceneMessage::BlackScreen),
        _ => None,
    }
}

fn parse_hide_gui_payload(payload: &[u8]) -> Option<CutsceneMessage> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != READ_START {
        return None;
    }
    let fragment_tail = exact_single_fragment_tail(payload, declared, 1)?;
    Some(CutsceneMessage::HideGui {
        hidden: read_fragment_bool(fragment_tail, 0)?,
        fragment_tail,
    })
}

fn exact_single_fragment_tail(payload: &[u8], declared: usize, data_bits: usize) -> Option<u8> {
    if payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)? {
        return None;
    }
    let tail = *payload.get(declared)?;
    // CNWMessage::GetWriteMessage masks only the high final-bit-count header
    // into the last fragment byte (`and 1Fh`, then ORs the count), so lower
    // padding bits are preserved and must round-trip exactly.
    let final_bits = usize::from((tail & 0xE0) >> 5);
    (final_bits == CNW_FRAGMENT_HEADER_BITS.checked_add(data_bits)?).then_some(tail)
}

fn read_fragment_bool(fragment_tail: u8, data_bit_index: usize) -> Option<bool> {
    let bit_index = CNW_FRAGMENT_HEADER_BITS.checked_add(data_bit_index)?;
    let shift = 7usize.checked_sub(bit_index)?;
    Some(((fragment_tail >> shift) & 1) != 0)
}

impl CutsceneMessage {
    fn packet_name(self) -> &'static str {
        match self {
            Self::Status { .. } => "Cutscene_Status",
            Self::FadeToBlack { .. } => "Cutscene_FadeToBlack",
            Self::FadeFromBlack { .. } => "Cutscene_FadeFromBlack",
            Self::StopFade => "Cutscene_StopFade",
            Self::BlackScreen => "Cutscene_BlackScreen",
            Self::HideGui { .. } => "Cutscene_HideGui",
        }
    }

    fn declared(self) -> Option<usize> {
        match self {
            Self::Status { .. } | Self::HideGui { .. } => Some(READ_START),
            Self::FadeToBlack { .. } | Self::FadeFromBlack { .. } => Some(FADE_DECLARED),
            Self::StopFade | Self::BlackScreen => None,
        }
    }

    fn read_bytes(self) -> usize {
        match self {
            Self::Status { .. } | Self::HideGui { .. } => 0,
            Self::FadeToBlack { .. } | Self::FadeFromBlack { .. } => FLOAT_BYTES,
            Self::StopFade | Self::BlackScreen => 0,
        }
    }

    fn to_ee_payload(self) -> Vec<u8> {
        match self {
            Self::Status {
                mode_enabled,
                secondary,
                fragment_tail,
            } => {
                debug_assert_eq!(read_fragment_bool(fragment_tail, 0), Some(mode_enabled));
                debug_assert_eq!(read_fragment_bool(fragment_tail, 1), Some(secondary));
                fragment_payload(STATUS_MINOR, READ_START, &[], fragment_tail)
            }
            Self::FadeToBlack {
                duration_bits,
                fragment_tail,
            } => fade_payload(FADE_TO_BLACK_MINOR, duration_bits, fragment_tail),
            Self::FadeFromBlack {
                duration_bits,
                fragment_tail,
            } => fade_payload(FADE_FROM_BLACK_MINOR, duration_bits, fragment_tail),
            Self::StopFade => vec![b'P', CUTSCENE_MAJOR, STOP_FADE_MINOR],
            Self::BlackScreen => vec![b'P', CUTSCENE_MAJOR, BLACK_SCREEN_MINOR],
            Self::HideGui {
                hidden,
                fragment_tail,
            } => {
                debug_assert_eq!(read_fragment_bool(fragment_tail, 0), Some(hidden));
                fragment_payload(HIDE_GUI_MINOR, READ_START, &[], fragment_tail)
            }
        }
    }
}

fn fade_payload(minor: u8, duration_bits: u32, fragment_tail: u8) -> Vec<u8> {
    fragment_payload(
        minor,
        FADE_DECLARED,
        &duration_bits.to_le_bytes(),
        fragment_tail,
    )
}

fn fragment_payload(minor: u8, declared: usize, read_window: &[u8], fragment_tail: u8) -> Vec<u8> {
    let mut payload = Vec::with_capacity(FADE_DECLARED + 1);
    payload.extend_from_slice(&[b'P', CUTSCENE_MAJOR, minor]);
    payload.extend_from_slice(&(declared as u32).to_le_bytes());
    payload.extend_from_slice(read_window);
    payload.push(fragment_tail);
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_decompile_backed_cutscene_fade_to_black_shape() {
        let payload = [
            0x50, 0x33, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x0A, 0xD7, 0x23, 0x3C, 0x78,
        ];

        let claim = claim_payload_if_verified(&payload).expect("fade to black should claim");

        assert_eq!(claim.minor, FADE_TO_BLACK_MINOR);
        assert_eq!(claim.packet_name, "Cutscene_FadeToBlack");
        assert_eq!(claim.declared, Some(FADE_DECLARED));
        assert_eq!(claim.read_bytes, FLOAT_BYTES);
    }

    #[test]
    fn claims_local_xp2_cutscene_fade_from_black_shape() {
        let payload = [
            0x50, 0x33, 0x04, 0x0B, 0x00, 0x00, 0x00, 0x0A, 0xD7, 0x23, 0x3C, 0x78,
        ];

        let claim = claim_payload_if_verified(&payload).expect("fade from black should claim");

        assert_eq!(claim.minor, FADE_FROM_BLACK_MINOR);
        assert_eq!(claim.packet_name, "Cutscene_FadeFromBlack");
        assert_eq!(claim.declared, Some(FADE_DECLARED));
        assert_eq!(claim.read_bytes, FLOAT_BYTES);
    }

    #[test]
    fn claims_decompile_backed_cutscene_stop_fade_shape() {
        let payload = [0x50, 0x33, 0x05];

        let claim = claim_payload_if_verified(&payload).expect("stop fade should claim");

        assert_eq!(claim.minor, STOP_FADE_MINOR);
        assert_eq!(claim.packet_name, "Cutscene_StopFade");
        assert_eq!(claim.declared, None);
        assert_eq!(claim.read_bytes, 0);
    }

    #[test]
    fn claims_local_xp2_cutscene_black_screen_shape() {
        let payload = [0x50, 0x33, 0x06];

        let claim = claim_payload_if_verified(&payload).expect("black screen should claim");

        assert_eq!(claim.minor, BLACK_SCREEN_MINOR);
        assert_eq!(claim.packet_name, "Cutscene_BlackScreen");
        assert_eq!(claim.declared, None);
        assert_eq!(claim.read_bytes, 0);
    }

    #[test]
    fn claims_local_xp2_cutscene_status_shape() {
        let payload = [0x50, 0x33, 0x01, 0x07, 0x00, 0x00, 0x00, 0xB0];

        let claim = claim_payload_if_verified(&payload).expect("status should claim");

        assert_eq!(claim.minor, STATUS_MINOR);
        assert_eq!(claim.packet_name, "Cutscene_Status");
        assert_eq!(claim.declared, Some(READ_START));
        assert_eq!(claim.read_bytes, 0);
    }

    #[test]
    fn claims_decompile_backed_cutscene_hide_gui_shape() {
        let payload = [0x50, 0x33, 0x07, 0x07, 0x00, 0x00, 0x00, 0x98];

        let claim = claim_payload_if_verified(&payload).expect("hide gui should claim");

        assert_eq!(claim.minor, HIDE_GUI_MINOR);
        assert_eq!(claim.packet_name, "Cutscene_HideGui");
        assert_eq!(claim.declared, Some(READ_START));
        assert_eq!(claim.read_bytes, 0);
    }

    #[test]
    fn rejects_cutscene_fade_with_stale_declared_length() {
        let payload = [
            0x50, 0x33, 0x04, 0x0A, 0x00, 0x00, 0x00, 0x0A, 0xD7, 0x23, 0x3C, 0x60,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_cutscene_fade_without_empty_fragment_terminator() {
        let payload = [
            0x50, 0x33, 0x04, 0x0B, 0x00, 0x00, 0x00, 0x0A, 0xD7, 0x23, 0x3C,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_cutscene_status_with_wrong_fragment_final_bits() {
        let payload = [0x50, 0x33, 0x01, 0x07, 0x00, 0x00, 0x00, 0x98];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_cutscene_black_screen_with_body() {
        let payload = [0x50, 0x33, 0x06, 0x00];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_unverified_cutscene_minor() {
        let payload = [0x50, 0x33, 0x02];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_client_marker_even_with_matching_major_minor() {
        let payload = [0x70, 0x33, 0x06];

        assert!(claim_payload_if_verified(&payload).is_none());
    }
}
