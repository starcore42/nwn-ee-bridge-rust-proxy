//! CNWMessage transport-shape normalization for known legacy packets.
//!
//! EE and Diamond both route high-level gameplay packets through
//! `CNWMessage::SetReadMessage` after stripping the three-byte `P major minor`
//! header. The first DWORD in that read window is interpreted as the write
//! message size and also determines where the fragment bits live. EE adds a
//! guard that rejects impossible fragment offsets; Diamond tolerated some
//! legacy server packets whose first four post-header bytes are actually the
//! leading fragment bytes.
//!
//! This module performs only that transport repair:
//!
//! `P major minor [four leading fragment bytes] [read bytes...]`
//!
//! becomes:
//!
//! `P major minor [u32 declared] [read bytes...] [four fragment bytes]`
//!
//! Message-body semantics remain in focused modules such as `player_list` and
//! `client_side_message`. Do not add whole high-level families here: even a
//! byte-identical packet needs a semantic owner that documents the decompile
//! evidence for that packet type.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
const LEGACY_READ_BYTES_OFFSET: usize =
    HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES;

#[derive(Debug, Clone)]
pub struct PrefixedFragmentsNormalizeSummary {
    pub major: u8,
    pub minor: u8,
    pub old_wire_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES],
    pub read_bytes_offset: usize,
    pub read_bytes_length: usize,
}

pub fn normalize_prefixed_fragments_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    normalize_prefixed_fragments_payload_for(payload, is_known_prefixed_fragment_family)
}

pub fn normalize_prefixed_fragments_payload_for(
    payload: &mut Vec<u8>,
    is_known_family: impl Fn(HighLevel) -> bool,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    if payload.len() < LEGACY_READ_BYTES_OFFSET + 1 {
        return None;
    }

    let high = HighLevel::parse(payload)?;
    if !is_known_family(high) {
        return None;
    }

    let old_wire_declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if cnw_fragment_offset_valid(payload, old_wire_declared) {
        return None;
    }
    if old_wire_declared == 0 {
        return None;
    }

    let read_bytes_length = payload.len() - LEGACY_READ_BYTES_OFFSET;
    let new_declared =
        (CNW_LENGTH_BYTES + read_bytes_length + HIGH_LEVEL_HEADER_BYTES) as u32;
    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] = payload
        [HIGH_LEVEL_HEADER_BYTES..LEGACY_READ_BYTES_OFFSET]
        .try_into()
        .ok()?;

    let mut rewritten = Vec::with_capacity(payload.len() + CNW_LENGTH_BYTES);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&new_declared.to_le_bytes());
    rewritten.extend_from_slice(&payload[LEGACY_READ_BYTES_OFFSET..]);
    rewritten.extend_from_slice(&prefixed_fragment_bytes);

    let old_payload_length = payload.len();
    let new_payload_length = rewritten.len();
    *payload = rewritten;

    Some(PrefixedFragmentsNormalizeSummary {
        major: high.major,
        minor: high.minor,
        old_wire_declared,
        new_declared,
        old_payload_length,
        new_payload_length,
        prefixed_fragment_bytes,
        read_bytes_offset: LEGACY_READ_BYTES_OFFSET,
        read_bytes_length,
    })
}

fn is_known_prefixed_fragment_family(high: HighLevel) -> bool {
    matches!(
        (high.major, high.minor),
        // Observed from the 1.69 server during module entry. Player-list
        // semantic augmentation, if needed, remains in `player_list`.
        (0x0A, 0x01)
    )
}

fn cnw_fragment_offset_valid(payload: &[u8], declared: u32) -> bool {
    let read_message_len = payload.len().saturating_sub(HIGH_LEVEL_HEADER_BYTES);
    if declared < HIGH_LEVEL_HEADER_BYTES as u32 || read_message_len == 0 {
        return false;
    }
    (declared as usize - HIGH_LEVEL_HEADER_BYTES) < read_message_len
}
