//! Journal packet semantic claims.
//!
//! The strict bridge does not treat a known opcode as safe by itself. Even
//! packet families that are byte-identical between Diamond and EE need a
//! focused translator module to claim the exact shape. The decompile reference
//! names high-level major `0x1C` as Journal; both EE and Diamond route minor
//! `0x0C` through the journal-updated reader. The HG captures seen during
//! login are already EE-compatible CNW `CExoString` payloads, so this module's
//! translation is identity after exact cursor validation:
//!
//! ```text
//! P 1C 0C <declared:u32> <title_len:u32> <title bytes> <fragment tail>
//! ```
//!
//! No broader journal minors are claimed here yet. They should each get their
//! own exact reader before being allowed in strict player mode.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const JOURNAL_MAJOR: u8 = 0x1C;
const JOURNAL_UPDATED_MINOR: u8 = 0x0C;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const MAX_JOURNAL_TITLE_BYTES: usize = 512;
const MAX_JOURNAL_FRAGMENT_BYTES: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct JournalClaimSummary {
    pub minor: u8,
    pub declared: usize,
    pub title_len: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<JournalClaimSummary> {
    let high = HighLevel::parse(payload)?;
    match (high.major, high.minor) {
        (JOURNAL_MAJOR, JOURNAL_UPDATED_MINOR) => claim_journal_updated(payload, high.minor),
        _ => None,
    }
}

fn claim_journal_updated(payload: &[u8], minor: u8) -> Option<JournalClaimSummary> {
    if payload.len() < READ_START + CNW_LENGTH_BYTES {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START + CNW_LENGTH_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_JOURNAL_FRAGMENT_BYTES
    {
        return None;
    }

    let title_len = usize::try_from(read_le_u32(payload, READ_START)?).ok()?;
    if title_len > MAX_JOURNAL_TITLE_BYTES {
        return None;
    }
    let title_start = READ_START.checked_add(CNW_LENGTH_BYTES)?;
    let title_end = title_start.checked_add(title_len)?;
    if title_end != declared {
        return None;
    }

    Some(JournalClaimSummary {
        minor,
        declared,
        title_len,
        fragment_bytes: payload.len() - declared,
    })
}
