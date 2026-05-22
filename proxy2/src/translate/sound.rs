//! Sound-object server payload claims.
//!
//! EE's packet-name table maps family `0x17` minor `0x03` to
//! `Sound_Object_Stop`. `CNWSMessage::SendServerToPlayerSoundObject_Stop`
//! creates a 4-byte CNW write message, writes one object id through
//! `WriteOBJECTIDServer`, and sends high-level family `0x17`, minor `0x03`.
//! The EE client sound dispatcher routes minor `3` to the matching stop reader,
//! which reads exactly that object id and then checks read overflow/underflow.
//! Diamond emits the same compact object-id read window, so this is an exact
//! no-op claim after bounded cursor and fragment-tail validation.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const SOUND_MAJOR: u8 = 0x17;
const SOUND_OBJECT_STOP_MINOR: u8 = 0x03;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const OBJECT_ID_BYTES: usize = 4;
const SOUND_OBJECT_ID_DECLARED: usize = READ_START + OBJECT_ID_BYTES;
const MAX_FRAGMENT_BYTES: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct SoundClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub object_id: u32,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<SoundClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != SOUND_MAJOR || high.minor != SOUND_OBJECT_STOP_MINOR {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != SOUND_OBJECT_ID_DECLARED
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
        || !cnw_fragment_tail_has_header(&payload[declared..])
    {
        return None;
    }

    let object_id = read_le_u32(payload, READ_START)?;
    if object_id == 0 || object_id == u32::MAX {
        return None;
    }

    Some(SoundClaimSummary {
        minor: high.minor,
        declared,
        object_id,
        fragment_bytes: payload.len() - declared,
    })
}

fn cnw_fragment_tail_has_header(fragment: &[u8]) -> bool {
    const CNW_FRAGMENT_HEADER_BITS: usize = 3;

    let Some(first) = fragment.first().copied() else {
        return false;
    };
    let final_bits = usize::from((first & 0xE0) >> 5);
    let valid_bits = if final_bits == 0 {
        fragment.len().saturating_mul(8)
    } else {
        fragment
            .len()
            .saturating_sub(1)
            .saturating_mul(8)
            .saturating_add(final_bits)
    };
    valid_bits >= CNW_FRAGMENT_HEADER_BITS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_local_xp1_sound_object_stop_shape() {
        let payload = [
            0x50, 0x17, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x47, 0x02, 0x00, 0x80, 0x76,
        ];

        let claim = claim_payload_if_verified(&payload).expect("sound stop should claim");

        assert_eq!(claim.minor, SOUND_OBJECT_STOP_MINOR);
        assert_eq!(claim.declared, SOUND_OBJECT_ID_DECLARED);
        assert_eq!(claim.object_id, 0x8000_0247);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_sound_object_stop_without_fragment_tail() {
        let payload = [
            0x50, 0x17, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x47, 0x02, 0x00, 0x80,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }
}
