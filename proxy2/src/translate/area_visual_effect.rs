//! `Area_VisualEffect` semantic rewrite and exact claim.
//!
//! Decompile evidence:
//! - EE's packet-name table maps `0x04/0x02` to `Area_VisualEffect`
//!   (`nwn ee decompile.txt:1099644`).
//! - EE `CNWSMessage::SendServerToPlayerArea_VisualEffect`
//!   (`nwn ee decompile.txt:1832708..1832833`) writes a WORD visual-effect id
//!   and three FLOAT vector components, then for EE-build clients satisfying
//!   build `2001/0x0E` writes `ObjectVisualTransformData`.
//! - For an identity transform, this bridge uses the same current-build object
//!   visual-transform identity map used by live-object traffic: two zero DWORD
//!   counts. Legacy Diamond captures omit that EE-only map, so the translator
//!   inserts it before the fragment tail and then validates the exact EE shape.

use crate::{
    crc::read_le_u32, packet::m::HighLevel,
    translate::live_object_update::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES,
};

const AREA_MAJOR: u8 = 0x04;
const AREA_VISUAL_EFFECT_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const EFFECT_ID_BYTES: usize = 2;
const VECTOR_FLOATS: usize = 3;
const FLOAT_BYTES: usize = 4;
const LEGACY_READ_BYTES: usize = EFFECT_ID_BYTES + (VECTOR_FLOATS * FLOAT_BYTES);
const EE_READ_BYTES: usize = LEGACY_READ_BYTES + EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len();
const LEGACY_DECLARED_BYTES: usize = READ_START + LEGACY_READ_BYTES;
const EE_DECLARED_BYTES: usize = READ_START + EE_READ_BYTES;

#[derive(Debug, Clone, Copy)]
pub struct AreaVisualEffectClaimSummary {
    pub declared: usize,
    pub read_bytes: usize,
    pub rewritten: bool,
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut Vec<u8>,
) -> Option<AreaVisualEffectClaimSummary> {
    let message = parse_area_visual_effect_message(payload)?;
    let rewritten = message.to_ee_payload();
    let changed = rewritten != *payload;
    if changed {
        *payload = rewritten;
    }
    parse_area_visual_effect_message(payload)?
        .is_exact_ee()
        .then_some(AreaVisualEffectClaimSummary {
            declared: EE_DECLARED_BYTES,
            read_bytes: EE_READ_BYTES,
            rewritten: changed,
        })
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<AreaVisualEffectClaimSummary> {
    parse_area_visual_effect_message(payload)?
        .is_exact_ee()
        .then_some(AreaVisualEffectClaimSummary {
            declared: EE_DECLARED_BYTES,
            read_bytes: EE_READ_BYTES,
            rewritten: false,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AreaVisualEffectMessage {
    effect_id: u16,
    position_bits: [u32; VECTOR_FLOATS],
    has_ee_identity_transform: bool,
    fragment_tail: u8,
}

fn parse_area_visual_effect_message(payload: &[u8]) -> Option<AreaVisualEffectMessage> {
    let high = HighLevel::parse(payload)?;
    if high.major != AREA_MAJOR || high.minor != AREA_VISUAL_EFFECT_MINOR {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let has_ee_identity_transform = match declared {
        LEGACY_DECLARED_BYTES => false,
        EE_DECLARED_BYTES => true,
        _ => return None,
    };
    let fragment_tail = exact_single_empty_fragment_tail(payload, declared)?;

    let cursor = READ_START;
    let effect_id = read_le_u16(payload, cursor)?;
    let position_bits = [
        read_finite_f32_bits(payload, cursor + EFFECT_ID_BYTES)?,
        read_finite_f32_bits(payload, cursor + EFFECT_ID_BYTES + FLOAT_BYTES)?,
        read_finite_f32_bits(payload, cursor + EFFECT_ID_BYTES + (2 * FLOAT_BYTES))?,
    ];

    if has_ee_identity_transform {
        let identity_start = READ_START + LEGACY_READ_BYTES;
        let identity_end = identity_start + EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.len();
        if payload.get(identity_start..identity_end)
            != Some(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES.as_slice())
        {
            return None;
        }
    }

    Some(AreaVisualEffectMessage {
        effect_id,
        position_bits,
        has_ee_identity_transform,
        fragment_tail,
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

fn read_le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_finite_f32_bits(bytes: &[u8], offset: usize) -> Option<u32> {
    let bits = read_le_u32(bytes, offset)?;
    let value = f32::from_bits(bits);
    value.is_finite().then_some(bits)
}

impl AreaVisualEffectMessage {
    fn is_exact_ee(self) -> bool {
        self.has_ee_identity_transform
    }

    fn to_ee_payload(self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(EE_DECLARED_BYTES + SINGLE_FRAGMENT_BYTE);
        payload.extend_from_slice(&[b'P', AREA_MAJOR, AREA_VISUAL_EFFECT_MINOR]);
        payload.extend_from_slice(&(EE_DECLARED_BYTES as u32).to_le_bytes());
        payload.extend_from_slice(&self.effect_id.to_le_bytes());
        for bits in self.position_bits {
            payload.extend_from_slice(&bits.to_le_bytes());
        }
        payload.extend_from_slice(&EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
        payload.push(self.fragment_tail);
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp2_area_visual_effect_rewrites_to_ee_identity_transform_shape() {
        let mut payload =
            include_bytes!("../../fixtures/area/local_xp2_area_visual_effect_20260522.bin")
                .to_vec();

        assert!(
            claim_payload_if_verified(&payload).is_none(),
            "raw legacy Area_VisualEffect should document the missing EE transform map"
        );

        let summary = claim_or_rewrite_payload_if_verified(&mut payload)
            .expect("legacy Area_VisualEffect should rewrite through the bounded adapter");
        assert!(summary.rewritten);
        assert_eq!(summary.declared, EE_DECLARED_BYTES);
        assert_eq!(summary.read_bytes, EE_READ_BYTES);
        assert!(claim_payload_if_verified(&payload).is_some());
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp2_area_visual_effect_rejects_bad_fragment_tail_after_rewrite() {
        let mut payload =
            include_bytes!("../../fixtures/area/local_xp2_area_visual_effect_20260522.bin")
                .to_vec();
        claim_or_rewrite_payload_if_verified(&mut payload).expect("fixture should rewrite");
        let declared = usize::try_from(read_le_u32(&payload, HIGH_LEVEL_HEADER_BYTES).unwrap())
            .expect("declared should fit");
        payload[declared] = 0x9C;

        assert!(claim_payload_if_verified(&payload).is_none());
    }
}
