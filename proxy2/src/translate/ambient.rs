//! `Ambient_*` high-level packet claims.
//!
//! Decompile evidence:
//! - EE's packet-name table maps `0x28/0x01..0x08` to the ambient music,
//!   battle-music, ambient-sound play/change/volume family
//!   (`nwn ee decompile.txt:1101070..1101092`).
//! - EE `CNWSMessage::SendServerToPlayerAmbientMusicPlay`
//!   (`nwn ee decompile.txt:1830750..1830810`) writes one `BOOL` then sends
//!   family `0x28`, minor `0x01`.
//! - EE ambient set/change senders at `0x1404CD290..0x1404CD759` use only three
//!   CNW shapes: one `BOOL`, one 32-bit `INT`, or `BOOL + INT`. The EE client
//!   ambient dispatcher (`sub_14076C9D0`) routes those minors to matching
//!   readers, including minor `0x01`'s `ReadBOOL` path (`sub_14076D0B0`).
//! - Local Diamond XP2 Chapter 3 emitted the same `0x28/0x01` one-BOOL shape
//!   while opening inventory. This module claims only the exact declared CNW
//!   cursor shapes and performs no rewrite.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const AMBIENT_MAJOR: u8 = 0x28;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const INT_BYTES: usize = 4;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const FRAGMENT_FINAL_CURSOR_MASK: u8 = 0xE0;
const CNW_FRAGMENT_HEADER_FINAL_CURSOR: u8 = 0x60;
const SINGLE_BOOL_FINAL_CURSOR: u8 = 0x80;
const SINGLE_BOOL_DATA_BIT: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientMessageKind {
    BoolOnly,
    IntOnly,
    BoolAndInt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbientClaimSummary {
    pub packet_name: &'static str,
    pub minor: u8,
    pub kind: AmbientMessageKind,
    pub selector: Option<bool>,
    pub value: Option<i32>,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<AmbientClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.envelope != HIGH_LEVEL_ENVELOPE || high.major != AMBIENT_MAJOR {
        return None;
    }

    match high.minor {
        0x01 => claim_bool_only(payload, high.minor, "Ambient_AmbientMusicPlay"),
        0x02 => claim_int_only(payload, high.minor, "Ambient_AmbientMusicSetDelay"),
        0x03 => claim_bool_and_int(payload, high.minor, "Ambient_AmbientMusicChange"),
        0x04 => claim_bool_only(payload, high.minor, "Ambient_AmbientBattleMusicPlay"),
        0x05 => claim_int_only(payload, high.minor, "Ambient_AmbientBattleMusicChange"),
        0x06 => claim_bool_only(payload, high.minor, "Ambient_AmbientSoundPlay"),
        0x07 => claim_bool_and_int(payload, high.minor, "Ambient_AmbientSoundChange"),
        0x08 => claim_bool_and_int(payload, high.minor, "Ambient_AmbientSoundVolume"),
        _ => None,
    }
}

fn claim_bool_only(
    payload: &[u8],
    minor: u8,
    packet_name: &'static str,
) -> Option<AmbientClaimSummary> {
    let declared = declared_len(payload)?;
    if declared != READ_START || payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)? {
        return None;
    }
    let selector = decode_single_bool_fragment(*payload.get(declared)?)?;
    Some(AmbientClaimSummary {
        packet_name,
        minor,
        kind: AmbientMessageKind::BoolOnly,
        selector: Some(selector),
        value: None,
        declared,
        fragment_bytes: SINGLE_FRAGMENT_BYTE,
    })
}

fn claim_int_only(
    payload: &[u8],
    minor: u8,
    packet_name: &'static str,
) -> Option<AmbientClaimSummary> {
    let declared = declared_len(payload)?;
    if declared != READ_START + INT_BYTES
        || payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)?
        || payload.get(declared)? & FRAGMENT_FINAL_CURSOR_MASK != CNW_FRAGMENT_HEADER_FINAL_CURSOR
    {
        return None;
    }
    Some(AmbientClaimSummary {
        packet_name,
        minor,
        kind: AmbientMessageKind::IntOnly,
        selector: None,
        value: Some(read_i32_le(payload, READ_START)?),
        declared,
        fragment_bytes: SINGLE_FRAGMENT_BYTE,
    })
}

fn claim_bool_and_int(
    payload: &[u8],
    minor: u8,
    packet_name: &'static str,
) -> Option<AmbientClaimSummary> {
    let declared = declared_len(payload)?;
    if declared != READ_START + INT_BYTES
        || payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)?
    {
        return None;
    }
    let selector = decode_single_bool_fragment(*payload.get(declared)?)?;
    Some(AmbientClaimSummary {
        packet_name,
        minor,
        kind: AmbientMessageKind::BoolAndInt,
        selector: Some(selector),
        value: Some(read_i32_le(payload, READ_START)?),
        declared,
        fragment_bytes: SINGLE_FRAGMENT_BYTE,
    })
}

fn declared_len(payload: &[u8]) -> Option<usize> {
    usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()
}

fn read_i32_le(payload: &[u8], offset: usize) -> Option<i32> {
    let bytes: [u8; 4] = payload
        .get(offset..offset.checked_add(4)?)?
        .try_into()
        .ok()?;
    Some(i32::from_le_bytes(bytes))
}

fn decode_single_bool_fragment(byte: u8) -> Option<bool> {
    if byte & FRAGMENT_FINAL_CURSOR_MASK != SINGLE_BOOL_FINAL_CURSOR {
        return None;
    }
    Some(byte & SINGLE_BOOL_DATA_BIT != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_exact_ambient_music_play_bool_shape() {
        let payload = [b'P', AMBIENT_MAJOR, 0x01, 0x07, 0x00, 0x00, 0x00, 0x9E];

        let claim = claim_payload_if_verified(&payload)
            .expect("Ambient_AmbientMusicPlay one-BOOL shape should claim");
        assert_eq!(claim.packet_name, "Ambient_AmbientMusicPlay");
        assert_eq!(claim.kind, AmbientMessageKind::BoolOnly);
        assert_eq!(claim.selector, Some(true));
        assert_eq!(claim.declared, READ_START);
    }

    #[test]
    fn claims_exact_ambient_int_shape() {
        let payload = [
            b'P',
            AMBIENT_MAJOR,
            0x02,
            0x0B,
            0x00,
            0x00,
            0x00,
            0x78,
            0x56,
            0x34,
            0x12,
            0x60,
        ];

        let claim = claim_payload_if_verified(&payload)
            .expect("Ambient_AmbientMusicSetDelay one-INT shape should claim");
        assert_eq!(claim.packet_name, "Ambient_AmbientMusicSetDelay");
        assert_eq!(claim.kind, AmbientMessageKind::IntOnly);
        assert_eq!(claim.value, Some(0x1234_5678));
    }

    #[test]
    fn claims_exact_ambient_bool_and_int_shape() {
        let payload = [
            b'P',
            AMBIENT_MAJOR,
            0x03,
            0x0B,
            0x00,
            0x00,
            0x00,
            0xFF,
            0xFF,
            0xFF,
            0xFF,
            0x8E,
        ];

        let claim = claim_payload_if_verified(&payload)
            .expect("Ambient_AmbientMusicChange BOOL+INT shape should claim");
        assert_eq!(claim.packet_name, "Ambient_AmbientMusicChange");
        assert_eq!(claim.kind, AmbientMessageKind::BoolAndInt);
        assert_eq!(claim.selector, Some(false));
        assert_eq!(claim.value, Some(-1));
    }

    #[test]
    fn rejects_ambient_music_play_without_bool_fragment_cursor() {
        let payload = [b'P', AMBIENT_MAJOR, 0x01, 0x07, 0x00, 0x00, 0x00, 0x7E];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp2_chapter3_ambient_music_play_fixture_claims() {
        let payload = include_bytes!(
            "../../fixtures/ambient/local_xp2_chapter3_ambient_music_play_20260523.bin"
        );

        let claim = claim_payload_if_verified(payload)
            .expect("local XP2 Chapter 3 ambient music-play fixture should claim exactly");
        assert_eq!(claim.packet_name, "Ambient_AmbientMusicPlay");
        assert_eq!(claim.selector, Some(true));
    }
}
