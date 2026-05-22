//! Safe projectile server payload claims.
//!
//! Decompile evidence:
//! - EE's packet-name table maps `0x22/0x01` to `SafeProjectile_Spawn`
//!   (`nwn ee decompile.txt:1100948`).
//! - EE sender `CNWSMessage::SendServerToPlayerSafeProjectile`
//!   (`nwn ee decompile.txt:1856588`) creates a `0x29` CNW write message,
//!   writes source/target OBJECTIDServer values, two Vector triples, a DWORD
//!   projectile id, a BYTE projectile type, and then type-specific fields
//!   before sending family `0x22`, minor `0x01`.
//! - The local XP2 capture is the type-6 branch: one additional DWORD after
//!   the projectile type, followed by a single CNW fragment byte.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const SAFE_PROJECTILE_MAJOR: u8 = 0x22;
const SPAWN_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const TYPE6_PROJECTILE_TYPE: u8 = 6;
const TYPE6_READ_BYTES: usize = (2 * 4) + (6 * 4) + 4 + 1 + 4;
const TYPE6_DECLARED_BYTES: usize = READ_START + TYPE6_READ_BYTES;
const TYPE6_PAYLOAD_BYTES: usize = TYPE6_DECLARED_BYTES + SINGLE_FRAGMENT_BYTE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafeProjectileClaimSummary {
    pub minor: u8,
    pub packet_name: &'static str,
    pub declared: usize,
    pub read_bytes: usize,
    pub projectile_type: u8,
    pub fragment_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SafeProjectileSpawnType6 {
    source_object_id: u32,
    target_object_id: u32,
    start_bits: [u32; 3],
    end_bits: [u32; 3],
    projectile_id: u32,
    extra_arg: u32,
    fragment_tail: u8,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<SafeProjectileClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != SAFE_PROJECTILE_MAJOR || high.minor != SPAWN_MINOR {
        return None;
    }

    let message = parse_spawn_type6_payload(payload)?;
    let rewritten = message.to_ee_payload();
    (rewritten == payload).then(|| message.summary())
}

fn parse_spawn_type6_payload(payload: &[u8]) -> Option<SafeProjectileSpawnType6> {
    if payload.len() != TYPE6_PAYLOAD_BYTES {
        return None;
    }
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared != TYPE6_DECLARED_BYTES {
        return None;
    }
    let fragment_tail = exact_single_empty_fragment_tail(payload, declared)?;

    let cursor = READ_START;
    let projectile_type = *payload.get(cursor + 36)?;
    if projectile_type != TYPE6_PROJECTILE_TYPE {
        return None;
    }

    Some(SafeProjectileSpawnType6 {
        source_object_id: read_le_u32(payload, cursor)?,
        target_object_id: read_le_u32(payload, cursor + 4)?,
        start_bits: [
            read_le_u32(payload, cursor + 8)?,
            read_le_u32(payload, cursor + 12)?,
            read_le_u32(payload, cursor + 16)?,
        ],
        end_bits: [
            read_le_u32(payload, cursor + 20)?,
            read_le_u32(payload, cursor + 24)?,
            read_le_u32(payload, cursor + 28)?,
        ],
        projectile_id: read_le_u32(payload, cursor + 32)?,
        extra_arg: read_le_u32(payload, cursor + 37)?,
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

impl SafeProjectileSpawnType6 {
    fn summary(self) -> SafeProjectileClaimSummary {
        SafeProjectileClaimSummary {
            minor: SPAWN_MINOR,
            packet_name: "SafeProjectile_Spawn",
            declared: TYPE6_DECLARED_BYTES,
            read_bytes: TYPE6_READ_BYTES,
            projectile_type: TYPE6_PROJECTILE_TYPE,
            fragment_bytes: SINGLE_FRAGMENT_BYTE,
        }
    }

    fn to_ee_payload(self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(TYPE6_PAYLOAD_BYTES);
        payload.extend_from_slice(&[b'P', SAFE_PROJECTILE_MAJOR, SPAWN_MINOR]);
        payload.extend_from_slice(&(TYPE6_DECLARED_BYTES as u32).to_le_bytes());
        payload.extend_from_slice(&self.source_object_id.to_le_bytes());
        payload.extend_from_slice(&self.target_object_id.to_le_bytes());
        for bits in self.start_bits {
            payload.extend_from_slice(&bits.to_le_bytes());
        }
        for bits in self.end_bits {
            payload.extend_from_slice(&bits.to_le_bytes());
        }
        payload.extend_from_slice(&self.projectile_id.to_le_bytes());
        payload.push(TYPE6_PROJECTILE_TYPE);
        payload.extend_from_slice(&self.extra_arg.to_le_bytes());
        payload.push(self.fragment_tail);
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL_XP2_TYPE6_PROJECTILE: [u8; TYPE6_PAYLOAD_BYTES] = [
        0x50, 0x22, 0x01, 0x30, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x80, 0x34, 0x12, 0x00, 0x80,
        0xD8, 0x49, 0x6F, 0x41, 0x46, 0x1E, 0xC0, 0x41, 0x00, 0x00, 0x00, 0x00, 0xD8, 0x49, 0x6F,
        0x41, 0x46, 0x1E, 0xC0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x39,
        0x00, 0x00, 0x00, 0x61,
    ];

    #[test]
    fn type6_spawn_fixture_matches_decompile_cursor_shape() {
        let summary = claim_payload_if_verified(&LOCAL_XP2_TYPE6_PROJECTILE)
            .expect("type-6 SafeProjectile_Spawn should claim");

        assert_eq!(summary.minor, SPAWN_MINOR);
        assert_eq!(summary.packet_name, "SafeProjectile_Spawn");
        assert_eq!(summary.declared, TYPE6_DECLARED_BYTES);
        assert_eq!(summary.read_bytes, TYPE6_READ_BYTES);
        assert_eq!(summary.projectile_type, TYPE6_PROJECTILE_TYPE);
        assert_eq!(summary.fragment_bytes, SINGLE_FRAGMENT_BYTE);
    }

    #[test]
    fn type6_spawn_rejects_stale_declared_boundary() {
        let mut payload = LOCAL_XP2_TYPE6_PROJECTILE;
        payload[HIGH_LEVEL_HEADER_BYTES] = 0x2F;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn type6_spawn_rejects_wrong_fragment_final_bits() {
        let mut payload = LOCAL_XP2_TYPE6_PROJECTILE;
        payload[TYPE6_DECLARED_BYTES] = 0x41;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn type6_spawn_rejects_unowned_projectile_type_shape() {
        let mut payload = LOCAL_XP2_TYPE6_PROJECTILE;
        payload[READ_START + 36] = 7;

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn local_xp2_type6_projectile_fixture_is_exact_noop_claim() {
        let payload = include_bytes!(
            "../../fixtures/safe_projectile/local_xp2_safe_projectile_spawn_type6_20260522.bin"
        );

        assert!(claim_payload_if_verified(payload).is_some());
    }
}
