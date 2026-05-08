//! Inflated gameplay stream splitting.
//!
//! The reliable `M` layer inflates zlib envelopes, but the resulting bytes are
//! not always "one packet equals one semantic message". A window may contain a
//! complete `P major minor` high-level message, a continuation of a previously
//! classified zlib stream, or bytes that are still waiting for a later fragment.
//!
//! This module is deliberately pure: it classifies inflated byte ranges and can
//! rejoin translated units, but it does not mutate packets or decide semantic
//! ownership. Semantic translators still live in focused packet-family modules.

use crate::packet::m::HighLevel;

use super::VerifiedFamily;

#[derive(Debug, Clone, Copy)]
pub enum GameplayUnit<'a> {
    HighLevel(HighLevelMessage<'a>),
    Continuation(&'a [u8]),
    PendingFragment(&'a [u8]),
    Unknown(&'a [u8]),
}

#[derive(Debug, Clone, Copy)]
pub struct HighLevelMessage<'a> {
    pub offset: usize,
    pub envelope: u8,
    pub major: u8,
    pub minor: u8,
    pub payload: &'a [u8],
    pub declared: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum TranslatedGameplayUnit {
    Owned {
        family: VerifiedFamily,
        bytes: Vec<u8>,
    },
    TransportOnly(Vec<u8>),
    Quarantined {
        reason: &'static str,
    },
}

#[derive(Debug, Clone)]
pub struct SplitResult<T> {
    pub units: T,
    pub complete: bool,
}

pub fn split_inflated_gameplay(bytes: &[u8]) -> SplitResult<Vec<GameplayUnit<'_>>> {
    if bytes.is_empty() {
        return SplitResult {
            units: Vec::new(),
            complete: true,
        };
    }

    let Some(high) = HighLevel::parse(bytes) else {
        return SplitResult {
            units: vec![GameplayUnit::Continuation(bytes)],
            complete: false,
        };
    };

    let declared = bytes
        .get(3..7)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .and_then(|value| usize::try_from(value).ok());

    SplitResult {
        units: vec![GameplayUnit::HighLevel(HighLevelMessage {
            offset: 0,
            envelope: bytes[0],
            major: high.major,
            minor: high.minor,
            payload: bytes,
            declared,
        })],
        complete: true,
    }
}

pub fn rejoin_translated_units(units: &[TranslatedGameplayUnit]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for unit in units {
        match unit {
            TranslatedGameplayUnit::Owned { bytes, .. }
            | TranslatedGameplayUnit::TransportOnly(bytes) => out.extend_from_slice(bytes),
            TranslatedGameplayUnit::Quarantined { .. } => return None,
        }
    }
    Some(out)
}

pub fn translate_units<'a, F>(
    units: Vec<GameplayUnit<'a>>,
    mut translate_high_level: F,
) -> Vec<TranslatedGameplayUnit>
where
    F: FnMut(HighLevelMessage<'a>) -> TranslatedGameplayUnit,
{
    units
        .into_iter()
        .map(|unit| match unit {
            GameplayUnit::HighLevel(message) => translate_high_level(message),
            GameplayUnit::Continuation(bytes) | GameplayUnit::PendingFragment(bytes) => {
                TranslatedGameplayUnit::TransportOnly(bytes.to_vec())
            }
            GameplayUnit::Unknown(bytes) => TranslatedGameplayUnit::Quarantined {
                reason: if bytes.is_empty() {
                    "empty-unknown-gameplay-unit"
                } else {
                    "unknown-gameplay-unit"
                },
            },
        })
        .collect()
}
