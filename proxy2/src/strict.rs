//! Strict post-translation packet validation.
//!
//! A packet is allowed only after it has been structurally classified and its
//! direction-specific shape is understood. This module is deliberately
//! conservative: when a new packet appears, we quarantine it, inspect the
//! decompiles, add the translator/validator, and only then allow it.

use crate::{
    config::StrictProfile,
    crc::read_le_u32,
    packet::{
        Direction, Packet,
        bn::{BnPacket, BnTag, parse_bndr_extended_server_info},
        hex_prefix,
        m::{HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, parse_packetized_spans},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Quarantine,
}

#[derive(Debug, Clone)]
pub struct StrictDecision {
    pub verdict: Verdict,
    pub family: &'static str,
    pub name: &'static str,
    pub reason: &'static str,
}

impl StrictDecision {
    pub fn allow(family: &'static str, name: &'static str, reason: &'static str) -> Self {
        Self {
            verdict: Verdict::Allow,
            family,
            name,
            reason,
        }
    }

    pub fn quarantine(family: &'static str, name: &'static str, reason: &'static str) -> Self {
        Self {
            verdict: Verdict::Quarantine,
            family,
            name,
            reason,
        }
    }

    pub fn allowed(&self) -> bool {
        self.verdict == Verdict::Allow
    }
}

pub fn decide(direction: Direction, bytes: &[u8], profile: StrictProfile) -> StrictDecision {
    match Packet::classify(bytes) {
        Packet::Bn(packet) => decide_bn(direction, &packet),
        Packet::M(frame) => {
            let Some(view) = &frame.parsed else {
                return StrictDecision::quarantine("M", "invalid M frame", "parse-failed");
            };
            if !view.crc_valid {
                return StrictDecision::quarantine("M", "invalid M frame", "crc-mismatch");
            }
            if view.declared_payload_length != 0
                && view.declared_payload_length > view.available_payload_length
            {
                return StrictDecision::quarantine(
                    "M",
                    "invalid M frame",
                    "declared-payload-overflow",
                );
            }
            let has_trailing = view.trailing_payload_length != 0;
            if has_trailing {
                let trailing_offset = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + view.payload_length;
                if let Err(decision) =
                    validate_packetized_trailing(bytes, trailing_offset, profile)
                {
                    return decision;
                }
            }
            if matches!(direction, Direction::ServerToClient)
                && view.payload_length != 0
                && view.declared_payload_length == 0
                && view.packetized_sequence == 0
                && (view.flags & 0x08) != 0
            {
                // Decompile/C++ parity for the reliable-window compressor:
                // the first compressed frame carries the deflate envelope; the
                // following high-priority frames are opaque compressed bytes
                // with a zero packetized length field. They are not high-level
                // CNW messages until the EE window layer reassembles/inflates
                // them, even if the compressed byte stream happens to begin
                // with 0x70 / `p`.
                return StrictDecision::allow(
                    "M/window",
                    "deflated reliable-window continuation",
                    "known-deflated-window-continuation",
                );
            }
            if let Some(high) = view.high {
                if high.is_known() {
                    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
                    let payload_end = payload_start + view.payload_length;
                    let Some(payload) = frame.bytes.get(payload_start..payload_end) else {
                        return StrictDecision::quarantine(
                            "M/high",
                            high.name(),
                            "known-high-level-payload-overflow",
                        );
                    };
                    if !known_high_payload_shape_valid(payload, profile) {
                        return StrictDecision::quarantine(
                            "M/high",
                            high.name(),
                            "known-high-level-invalid-shape",
                        );
                    }
                    return StrictDecision::allow(
                        "M/high",
                        high.name(),
                        if has_trailing {
                            "known-high-level-payload-with-window-spans"
                        } else {
                            "known-high-level-payload"
                        },
                    );
                }
                return StrictDecision::quarantine(
                    "M/high",
                    high.name(),
                    "unknown-high-level-payload",
                );
            }
            if let Some(deflated) = &view.deflated {
                if deflated.plausible {
                    return StrictDecision::allow(
                        "M/deflated",
                        "validated deflated envelope",
                        if has_trailing {
                            "known-deflated-envelope-with-window-spans"
                        } else {
                            "known-deflated-envelope"
                        },
                    );
                }
                return StrictDecision::quarantine(
                    "M/deflated",
                    "invalid deflated envelope",
                    "invalid-deflated-envelope",
                );
            }
            if view.payload_length == 0 {
                return StrictDecision::allow(
                    "M/control",
                    "empty ack/control",
                    if has_trailing {
                        "empty-M-frame-with-window-spans"
                    } else {
                        "empty-M-frame"
                    },
                );
            }
            if view.packetized_sequence != 0 {
                return StrictDecision::allow(
                    "M/fragment",
                    "packetized continuation",
                    if has_trailing {
                        "known-fragment-continuation-with-window-spans"
                    } else {
                        "known-fragment-continuation"
                    },
                );
            }
            StrictDecision::quarantine("M", "unknown M payload", "unclassified-M-payload")
        }
        Packet::UnknownTopLevel(_) => {
            StrictDecision::quarantine("top-level", "unknown top-level packet", "unknown-top-level")
        }
    }
}

pub fn decide_verified_translated(direction: Direction, bytes: &[u8]) -> StrictDecision {
    match Packet::classify(bytes) {
        Packet::M(frame) => {
            let Some(view) = &frame.parsed else {
                return StrictDecision::quarantine("M", "invalid translated M frame", "parse-failed");
            };
            if !view.crc_valid {
                return StrictDecision::quarantine(
                    "M",
                    "invalid translated M frame",
                    "crc-mismatch",
                );
            }
            if view.declared_payload_length != 0
                && view.declared_payload_length > view.available_payload_length
            {
                return StrictDecision::quarantine(
                    "M",
                    "invalid translated M frame",
                    "declared-payload-overflow",
                );
            }
            StrictDecision::allow(
                "M/translated-deflated",
                "verified translated deflated frame",
                match direction {
                    Direction::ServerToClient => "semantic-module-info-rewrite",
                    Direction::ServerToClientSynthetic => "synthetic-semantic-module-info-rewrite",
                    Direction::ClientToServer => "unexpected-client-verified-translation",
                },
            )
        }
        Packet::Bn(_) => StrictDecision::quarantine(
            "BN",
            "invalid verified translation",
            "verified-translation-not-M",
        ),
        Packet::UnknownTopLevel(_) => StrictDecision::quarantine(
            "top-level",
            "invalid verified translation",
            "unknown-top-level",
        ),
    }
}

fn validate_packetized_trailing(
    bytes: &[u8],
    offset: usize,
    profile: StrictProfile,
) -> Result<(), StrictDecision> {
    let Some(spans) = parse_packetized_spans(bytes, offset) else {
        return Err(StrictDecision::quarantine(
            "M/window",
            "invalid packetized span",
            "packetized-span-parse-failed",
        ));
    };
    if spans.is_empty() {
        return Err(StrictDecision::quarantine(
            "M/window",
            "invalid packetized span",
            "packetized-span-empty-trailing",
        ));
    }

    for span in spans {
        if let Some(high) = span.high {
            if high.is_known() {
                let payload_start = span.offset + LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
                let payload_end = payload_start + span.payload_length;
                let Some(payload) = bytes.get(payload_start..payload_end) else {
                    return Err(StrictDecision::quarantine(
                        "M/high",
                        high.name(),
                        "known-window-high-level-payload-overflow",
                    ));
                };
                if !known_high_payload_shape_valid(payload, profile) {
                    return Err(StrictDecision::quarantine(
                        "M/high",
                        high.name(),
                        "known-window-high-level-invalid-shape",
                    ));
                }
                continue;
            }
            return Err(StrictDecision::quarantine(
                "M/high",
                high.name(),
                "unknown-window-high-level-payload",
            ));
        }
        if let Some(deflated) = &span.deflated {
            if deflated.plausible {
                continue;
            }
            return Err(StrictDecision::quarantine(
                "M/deflated",
                "invalid window deflated envelope",
                "invalid-window-deflated-envelope",
            ));
        }
        if span.payload_length == 0 {
            continue;
        }
        if span.packetized_sequence != 0 {
            continue;
        }
        return Err(StrictDecision::quarantine(
            "M/window",
            "unknown packetized span payload",
            "unclassified-window-span-payload",
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum HighPayloadValidation {
    /// A validator that owns the packet family and consumes the declared shape
    /// narrowly enough for strict player-mode delivery.
    Exact(bool),
    /// A named family validator that still admits a broad CNW wrapper shape.
    /// This is intentionally alpha-only so "known opcode" cannot silently mean
    /// "safe forever."
    Shallow { valid: bool, critical: bool },
    /// A known or unknown opcode with no family validator. These packets must
    /// be quarantined until the decompiles/captures justify a translator.
    Missing,
}

impl HighPayloadValidation {
    fn shallow_noncritical(valid: bool) -> Self {
        Self::Shallow {
            valid,
            critical: false,
        }
    }
}

fn known_high_payload_shape_valid(payload: &[u8], profile: StrictProfile) -> bool {
    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    match high_payload_validation(payload, high) {
        HighPayloadValidation::Exact(valid) => valid,
        HighPayloadValidation::Shallow { valid, critical } => {
            if valid {
                if profile.allows_shallow_high_level_validator(critical) {
                    tracing::warn!(
                        major = high.major,
                        minor = high.minor,
                        name = high.name(),
                        strict_profile = profile.as_str(),
                        critical,
                        "strict M high-level validator is shallow and profile-gated"
                    );
                } else {
                    tracing::warn!(
                        major = high.major,
                        minor = high.minor,
                        name = high.name(),
                        strict_profile = profile.as_str(),
                        critical,
                        "strict M high-level shallow validator rejected by profile"
                    );
                }
            }
            valid && profile.allows_shallow_high_level_validator(critical)
        }
        HighPayloadValidation::Missing => {
            tracing::warn!(
                major = high.major,
                minor = high.minor,
                name = high.name(),
                "strict M high-level validator missing for known opcode"
            );
            false
        }
    }
}

fn high_payload_validation(payload: &[u8], high: HighLevel) -> HighPayloadValidation {
    match (high.major, high.minor) {
        (0x01, 0x00) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x01, 0x03) => HighPayloadValidation::Exact(server_status_module_running_shape_valid(payload)),
        (0x02, 0x05 | 0x0C) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x02, 0x11) => {
            HighPayloadValidation::Exact(client_login_server_subdirectory_shape_valid(payload))
        }
        (0x03, 0x01) => HighPayloadValidation::Exact(module_info_shape_valid(payload)),
        (0x03, 0x02) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x04, 0x01) => HighPayloadValidation::Exact(area_client_area_shape_valid(payload)),
        (0x04, 0x03) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x05, 0x01) => HighPayloadValidation::Exact(live_object_shape_valid(payload)),
        (0x05, 0x02) => HighPayloadValidation::Exact(game_obj_update_obj_control_shape_valid(payload)),
        (0x0A, 0x01 | 0x02) => HighPayloadValidation::Exact(player_list_shape_valid(payload)),
        (0x0E, 0x02) => HighPayloadValidation::Exact(party_get_list_payload_shape_valid(payload)),
        (0x0E, 0x01 | 0x03..=0x0E) => {
            HighPayloadValidation::Exact(party_cnw_wrapped_payload_shape_valid(payload))
        }
        (0x11, 0x01) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x11, 0x03) => HighPayloadValidation::Exact(char_list_request_update_char_shape_valid(payload)),
        (0x12, 0x0B) => HighPayloadValidation::Exact(client_side_feedback_shape_valid(payload)),
        (0x1E, 0x01 | 0x02) => HighPayloadValidation::Exact(quickbar_shape_valid(payload)),
        (0x31, 0x01 | 0x02) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x32, 0x01) => HighPayloadValidation::Exact(set_custom_token_shape_valid(payload)),
        (0x32, 0x02) => HighPayloadValidation::Exact(set_custom_token_list_shape_valid(payload)),
        (0x02, 0x0A | 0x10 | 0x12)
        | (0x03, 0x03 | 0x0E)
        | (0x09, 0x01..=0x05)
        | (0x0C, 0x01)
        | (0x1C, 0x0C) => {
            HighPayloadValidation::shallow_noncritical(bare_or_cnw_wrapped_payload_shape_valid(payload))
        }
        (0x2C, 0x01..=0x03) => HighPayloadValidation::Exact(loadbar_shape_valid(payload)),
        _ => HighPayloadValidation::Missing,
    }
}

fn empty_high_level_shape_valid(payload: &[u8]) -> bool {
    payload.len() == 3
}

fn client_login_server_subdirectory_shape_valid(payload: &[u8]) -> bool {
    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < 7 || declared > payload.len() || payload.len().saturating_sub(declared) > 8 {
        return false;
    }
    let identifier = &payload[7..declared];
    !identifier.is_empty()
        && identifier.len() <= 256
        && identifier
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'.'))
}

fn cnw_wrapped_payload_shape_valid(
    payload: &[u8],
    min_declared: usize,
    max_fragment_bytes: usize,
) -> bool {
    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    declared >= min_declared
        && declared <= payload.len()
        && payload.len().saturating_sub(declared) <= max_fragment_bytes
}

fn bare_or_cnw_wrapped_payload_shape_valid(payload: &[u8]) -> bool {
    payload.len() == 3 || cnw_wrapped_payload_shape_valid(payload, 3 + 4, 64)
}

fn server_status_module_running_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // EE `CNWCMessage::HandleServerToPlayerServerStatus` reads a leading
    // status `CExoString` for high-level 0x01/0x03, then
    // `CNWCModule::LoadModuleResources` consumes one fragment BOOL for the
    // optional NWSync advertisement. When that BOOL is true, EE reads:
    // root hash string, a single repository-count byte, repository URL string,
    // manifest count byte, manifest records, module resource name/description,
    // then a byte HAK count followed by fixed 16-byte HAK resrefs.
    server_status_module_resources_shape_valid(payload)
}

fn module_info_shape_valid(payload: &[u8]) -> bool {
    cnw_wrapped_payload_shape_valid(payload, 3 + 4, 64)
}

fn area_client_area_shape_valid(payload: &[u8]) -> bool {
    const MIN_AREA_CLIENT_AREA_DECLARED: usize = 3 + 4 + 4 + 4 * 4 + 4 + 16;
    cnw_wrapped_payload_shape_valid(payload, MIN_AREA_CLIENT_AREA_DECLARED, 64)
}

fn live_object_shape_valid(payload: &[u8]) -> bool {
    cnw_wrapped_payload_shape_valid(payload, 3 + 4, 4096)
}

fn player_list_shape_valid(payload: &[u8]) -> bool {
    cnw_wrapped_payload_shape_valid(payload, 3 + 4, 64)
}

fn quickbar_shape_valid(payload: &[u8]) -> bool {
    cnw_wrapped_payload_shape_valid(payload, 3 + 4, 64)
}

fn loadbar_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // EE `CNWSMessage::SendServerToPlayerLoadBar_StartStallEvent` and
    // `_EndStallEvent` write one DWORD stall-event id, while
    // `_UpdateStallEvent` writes that id plus one DWORD progress value.
    // Diamond exposes the same LoadBar family and observed 1.69/HG payloads
    // use the same declared CNW read-window shape. Validate that read cursor
    // exactly; the trailing CNW fragment bytes are bounded separately.
    const READ_START: usize = 3 + 4;
    const ONE_DWORD_DECLARED: usize = READ_START + 4;
    const TWO_DWORD_DECLARED: usize = READ_START + 8;
    const MAX_FRAGMENT_BYTES: usize = 8;

    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != 0x2C {
        return false;
    }
    let expected_declared = match high.minor {
        0x01 | 0x03 => ONE_DWORD_DECLARED,
        0x02 => TWO_DWORD_DECLARED,
        _ => return false,
    };
    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };

    declared == expected_declared
        && declared <= payload.len()
        && payload.len().saturating_sub(declared) <= MAX_FRAGMENT_BYTES
}

fn char_list_request_update_char_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // `CNWSMessage::HandlePlayerToServerCharListMessage` dispatches minor 3
    // by reading one byte (`ReadBYTE(8, 1)`) followed by one fixed 16-byte
    // `CResRef`, then checking `MessageReadUnderflow`. The CNW read window is
    // therefore exactly high-level tag + declared length + byte + CResRef.
    //
    // The observed EE driver-only client packet carries one legacy packetized
    // fragment byte after that declared window, so strict mode accepts the
    // exact declared shape with at most that single trailing byte.
    const DECLARED_BYTES: usize = 3 + 4 + 1 + 16;
    const MAX_OBSERVED_FRAGMENT_BYTES: usize = 1;

    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };

    declared == DECLARED_BYTES
        && payload.len() >= declared
        && payload.len().saturating_sub(declared) <= MAX_OBSERVED_FRAGMENT_BYTES
}

fn client_side_feedback_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // `CNWSCreature::SendFeedbackMessage` stores the feedback id in
    // `CNWCCMessageData` slot 9, then calls
    // `CNWSMessage::SendServerToPlayerCCMessage(..., 0x0B, ...)`.
    // The CC-message case 11 creates a bounded 0x80-byte write message,
    // always writes that slot-9 value as a 16-bit WORD first, and then writes
    // a small set of optional fields selected by the feedback id. For feedback
    // id `0xCC`, it calls `WriteCExoString(..., 0x20)`, whose decompile writes
    // a direct DWORD length followed by the text bytes. The strict gate
    // therefore validates the family/minor-specific CNW cursor instead of
    // allowing every known client-side-message opcode.
    const MIN_DECLARED_BYTES: usize = 3 + 4 + 2;
    const MAX_OBSERVED_FRAGMENT_BYTES: usize = 64;
    const MAX_FEEDBACK_TEXT_BYTES: usize = 4096;
    const MAX_FIXED_ARGUMENT_BYTES: usize = 64;

    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < MIN_DECLARED_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_OBSERVED_FRAGMENT_BYTES
    {
        return false;
    }

    let tail_start = MIN_DECLARED_BYTES;
    let tail_len = declared - tail_start;
    if tail_len == 0 {
        return true;
    }

    if let Some(string_len) = read_le_u32(payload, tail_start).and_then(|value| usize::try_from(value).ok()) {
        if string_len <= MAX_FEEDBACK_TEXT_BYTES
            && tail_start
                .checked_add(4)
                .and_then(|text_start| text_start.checked_add(string_len))
                == Some(declared)
        {
            return true;
        }
    }

    tail_len <= MAX_FIXED_ARGUMENT_BYTES && tail_len % 4 == 0
}

fn set_custom_token_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // `CNWSMessage::SendServerToPlayerSetCustomToken` sizes the write message
    // as string_length + 8, writes a 32-bit token id, then writes a
    // `CExoString` with a 32-bit length prefix. The declared CNW window must
    // exactly consume those fields; the one extra byte observed in legacy M
    // packetization is accepted only as a trailing fragment byte.
    custom_token_payload_shape_valid(payload, 0x01)
}

fn set_custom_token_list_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // `CNWSMessage::SendServerToPlayerSetCustomTokenList` writes a 32-bit
    // token count, then `(DWORD token id, CExoString value)` for each entry.
    // A zero-count list is therefore exactly `P 32 02`, declared 11, count 0,
    // plus the observed legacy packetized fragment terminator byte.
    custom_token_payload_shape_valid(payload, 0x02)
}

fn custom_token_payload_shape_valid(payload: &[u8], expected_minor: u8) -> bool {
    const READ_START: usize = 3 + 4;
    const MAX_REASONABLE_CUSTOM_TOKEN_BYTES: usize = 4096;
    const MAX_REASONABLE_CUSTOM_TOKEN_COUNT: usize = 4096;
    const MAX_OBSERVED_FRAGMENT_BYTES: usize = 1;

    if HighLevel::parse(payload)
        .map(|high| high.major != 0x32 || high.minor != expected_minor)
        .unwrap_or(true)
    {
        return false;
    }

    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < READ_START
        || declared >= payload.len()
        || payload.len().saturating_sub(declared) > MAX_OBSERVED_FRAGMENT_BYTES
    {
        return false;
    }

    let mut cursor = READ_START;
    match expected_minor {
        0x01 => {
            cursor = match cursor.checked_add(4) {
                Some(cursor) if cursor <= declared => cursor,
                _ => return false,
            };
            cursor = match custom_token_c_exo_string_end(
                payload,
                cursor,
                declared,
                MAX_REASONABLE_CUSTOM_TOKEN_BYTES,
            ) {
                Some(cursor) => cursor,
                None => return false,
            };
        }
        0x02 => {
            let Some(count) =
                read_le_u32(payload, cursor).and_then(|value| usize::try_from(value).ok())
            else {
                return false;
            };
            if count > MAX_REASONABLE_CUSTOM_TOKEN_COUNT {
                return false;
            }
            cursor = match cursor.checked_add(4) {
                Some(cursor) => cursor,
                None => return false,
            };
            for _ in 0..count {
                cursor = match cursor.checked_add(4) {
                    Some(cursor) if cursor <= declared => cursor,
                    _ => return false,
                };
                cursor = match custom_token_c_exo_string_end(
                    payload,
                    cursor,
                    declared,
                    MAX_REASONABLE_CUSTOM_TOKEN_BYTES,
                ) {
                    Some(cursor) => cursor,
                    None => return false,
                };
            }
        }
        _ => return false,
    }

    cursor == declared
}

fn custom_token_c_exo_string_end(
    payload: &[u8],
    cursor: usize,
    declared: usize,
    max_string_bytes: usize,
) -> Option<usize> {
    let length = read_le_u32(payload, cursor).and_then(|value| usize::try_from(value).ok())?;
    if length > max_string_bytes {
        return None;
    }
    cursor
        .checked_add(4)?
        .checked_add(length)
        .filter(|end| *end <= declared)
}

fn leading_cnw_string_consumes_inside_declared(payload: &[u8]) -> bool {
    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < 3 + 4 || declared > payload.len() {
        return false;
    }
    let read_start = 3;
    let string_len_offset = read_start + 4;
    let Some(length) =
        read_le_u32(payload, string_len_offset).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    string_len_offset
        .checked_add(4)
        .and_then(|start| start.checked_add(length))
        .map(|end| end <= declared)
        .unwrap_or(false)
}

fn server_status_module_resources_shape_valid(payload: &[u8]) -> bool {
    const READ_START: usize = 3 + 4;
    const MAX_SERVER_STATUS_STRING: usize = 4096;
    const MAX_NWSYNC_STRING: usize = 255;
    const MAX_MODULE_RESOURCE_STRING: usize = 4096;
    const MAX_HAK_COUNT: usize = 255;
    const RESREF_BYTES: usize = 16;
    const MAX_FRAGMENT_BYTES: usize = 8;

    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != 0x01 || high.minor != 0x03 {
        return false;
    }

    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < READ_START
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
    {
        return false;
    }

    let mut cursor = READ_START;
    cursor = match cnw_string_end(payload, cursor, declared, MAX_SERVER_STATUS_STRING) {
        Some(cursor) => cursor,
        None => return false,
    };

    let Some(nwsync_advertised) = cnw_fragment_bool(payload, declared, 0) else {
        return false;
    };
    if nwsync_advertised {
        cursor = match cnw_string_end(payload, cursor, declared, MAX_NWSYNC_STRING) {
            Some(cursor) => cursor,
            None => return false,
        };
        if payload.get(cursor).copied() != Some(1) {
            return false;
        }
        cursor += 1;
        cursor = match cnw_string_end(payload, cursor, declared, MAX_NWSYNC_STRING) {
            Some(cursor) => cursor,
            None => return false,
        };
        let Some(manifest_count) = payload.get(cursor).copied().map(usize::from) else {
            return false;
        };
        cursor += 1;
        for _ in 0..manifest_count {
            cursor = match cnw_string_end(payload, cursor, declared, MAX_NWSYNC_STRING) {
                Some(cursor) => cursor,
                None => return false,
            };
            cursor = match cursor.checked_add(2) {
                Some(cursor) if cursor <= declared => cursor,
                _ => return false,
            };
        }
    }

    cursor = match cnw_string_end(payload, cursor, declared, MAX_MODULE_RESOURCE_STRING) {
        Some(cursor) => cursor,
        None => return false,
    };
    cursor = match cnw_string_end(payload, cursor, declared, MAX_MODULE_RESOURCE_STRING) {
        Some(cursor) => cursor,
        None => return false,
    };
    let Some(hak_count) = payload.get(cursor).copied().map(usize::from) else {
        return false;
    };
    if hak_count > MAX_HAK_COUNT {
        return false;
    }
    cursor += 1;
    cursor
        .checked_add(hak_count.saturating_mul(RESREF_BYTES))
        .map(|end| end == declared)
        .unwrap_or(false)
}

fn cnw_string_end(
    payload: &[u8],
    cursor: usize,
    declared: usize,
    max_string_bytes: usize,
) -> Option<usize> {
    let length = read_le_u32(payload, cursor).and_then(|value| usize::try_from(value).ok())?;
    if length > max_string_bytes {
        return None;
    }
    cursor
        .checked_add(4)?
        .checked_add(length)
        .filter(|end| *end <= declared)
}

fn cnw_fragment_bool(payload: &[u8], declared: usize, semantic_bit_index: usize) -> Option<bool> {
    let fragment = payload.get(declared..)?;
    let bit_index = 3usize.checked_add(semantic_bit_index)?;
    let byte = *fragment.get(bit_index / 8)?;
    Some((byte & (0x80 >> (bit_index % 8))) != 0)
}

fn game_obj_update_obj_control_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // - SendServerToPlayerGameObjUpdate_ObjControl creates an 8-byte
    //   CNWMessage write buffer.
    // - The read window therefore contains the 4-byte declared length plus
    //   DWORD player id plus WriteOBJECTIDServer object id.
    // - Observed/Diamond-compatible CNW wrapping is `declared = 15`, which
    //   places one fragment byte after the 12-byte read buffer.
    const OBJ_CONTROL_DECLARED: u32 = 15;
    const OBJ_CONTROL_PAYLOAD_BYTES: usize = 16;

    payload.len() == OBJ_CONTROL_PAYLOAD_BYTES
        && HighLevel::parse(payload)
            .map(|high| high.major == 0x05 && high.minor == 0x02)
            .unwrap_or(false)
        && read_le_u32(payload, 3) == Some(OBJ_CONTROL_DECLARED)
}

fn party_cnw_wrapped_payload_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed classification:
    // EE's packet table names 0x0E01..0x0E0E as the Party family, and the
    // exported CNWSMessage Party_List / TransferObjectControl senders use the
    // normal CNWMessage write-buffer path. The exact read payload is variable
    // for member lists, so strict mode validates the CNW wrapper boundary
    // here and leaves semantic field parsing to the translate/party module.
    const MIN_CNW_DECLARED_BYTES: usize = 3 + 4;
    const MAX_REASONABLE_PARTY_FRAGMENT_BYTES: usize = 32;

    cnw_wrapped_payload_shape_valid(
        payload,
        MIN_CNW_DECLARED_BYTES,
        MAX_REASONABLE_PARTY_FRAGMENT_BYTES,
    )
}

fn party_get_list_payload_shape_valid(payload: &[u8]) -> bool {
    // EE sends the Party_GetList request as a bare high-level tag with no
    // CNWMessage read payload; server-to-client party list/control responses
    // use the CNW-wrapped sender path and are validated by the generic party
    // wrapper check.
    payload.len() == 3 || party_cnw_wrapped_payload_shape_valid(payload)
}

fn decide_bn(direction: Direction, packet: &BnPacket<'_>) -> StrictDecision {
    match (direction, packet.tag) {
        (Direction::ClientToServer, BnTag::Bncs) => validate_bncs(packet),
        (Direction::ClientToServer, BnTag::Bnvs) => validate_bnvs(packet),
        (Direction::ClientToServer, BnTag::Bndm) => validate_bndm(packet),
        (Direction::ClientToServer, BnTag::Bnds) => validate_client_bnds(packet),
        (Direction::ClientToServer, BnTag::Bnes) => require_len(
            packet,
            7,
            "known-ee-client-enumerate",
            "decompile SendBNESDirectMessageToAddress",
        ),
        (Direction::ClientToServer, BnTag::Bnlm) => require_len(
            packet,
            11,
            "known-ee-client-latency-request",
            "decompile SendBNLMMessage",
        ),
        (Direction::ClientToServer, BnTag::Bnxi) => validate_bnxi(packet),
        (Direction::ServerToClient, BnTag::Bncr) => validate_bncr(packet),
        (Direction::ServerToClient, BnTag::Bnvr) => validate_bnvr(packet),
        (Direction::ServerToClient, BnTag::Bnds) => StrictDecision::quarantine(
            "BN",
            packet.tag.name(),
            "legacy-server-BNDS-has-no-EE-client-translator",
        ),
        (Direction::ServerToClient, BnTag::Bndr) => validate_bndr(packet),
        (Direction::ServerToClient, BnTag::Bnxr) => validate_bnxr(packet),
        (Direction::ServerToClient, BnTag::Bndp) => validate_bndp(packet),
        (Direction::ServerToClient, BnTag::Bner) => validate_bner(packet),
        (Direction::ServerToClient, BnTag::Bnlr) => require_len(
            packet,
            11,
            "known-ee-server-latency-response",
            "decompile HandleBNLRMessage",
        ),
        (_, BnTag::Bnk0 | BnTag::Bnk1 | BnTag::Bnk2 | BnTag::Bnk3 | BnTag::Bnk4) => {
            StrictDecision::quarantine(
                "BN/EE-crypto",
                packet.tag.name(),
                "crypto-handshake-not-implemented-in-proxy2",
            )
        }
        (_, BnTag::EeDirectCollision) => StrictDecision::quarantine(
            "BN/EE-direct",
            packet.tag.name(),
            "ee-direct-control-collision",
        ),
        (_, BnTag::Unknown) => {
            StrictDecision::quarantine("BN", packet.tag.name(), "unknown-bn-control")
        }
        _ => StrictDecision::quarantine("BN", packet.tag.name(), "known-tag-wrong-direction"),
    }
}

fn require_len(
    packet: &BnPacket<'_>,
    expected: usize,
    allow_reason: &'static str,
    source: &'static str,
) -> StrictDecision {
    if packet.bytes.len() == expected {
        StrictDecision::allow("BN", packet.tag.name(), allow_reason)
    } else {
        tracing::warn!(
            tag = packet.tag.name(),
            len = packet.bytes.len(),
            expected,
            source,
            "strict BN length validation failed"
        );
        StrictDecision::quarantine("BN", packet.tag.name(), "known-bn-invalid-length")
    }
}

fn validate_bndm(packet: &BnPacket<'_>) -> StrictDecision {
    require_len(
        packet,
        4,
        "known-ee-client-direct-disconnect",
        "decompile SendBNDMMessage",
    )
}

fn validate_client_bnds(packet: &BnPacket<'_>) -> StrictDecision {
    require_len(
        packet,
        6,
        "known-legacy-client-disconnect",
        "Diamond-compatible BNDS client UDP-port disconnect",
    )
}

fn validate_bndr(packet: &BnPacket<'_>) -> StrictDecision {
    // Decompile-backed shape:
    // EE `CNetLayerInternal::HandleBNDRMessage` reads three DWORD-length
    // `CExoString` fields starting at offset 6, followed by a final WORD. This
    // exact parser means server-info text can be delivered to EE only after the
    // full declared cursor is consumed without overflow or trailing bytes.
    if parse_bndr_extended_server_info(packet.bytes).is_some() {
        StrictDecision::allow(
            "BN",
            packet.tag.name(),
            "known-ee-bndr-extended-server-info",
        )
    } else {
        StrictDecision::quarantine("BN", packet.tag.name(), "BNDR-invalid-extended-info-shape")
    }
}

fn validate_bncs(packet: &BnPacket<'_>) -> StrictDecision {
    // Diamond `sub_5F6630` emits exactly two counted strings after the fixed
    // 18-byte header: player name and public CD key. If anything remains after
    // those two segments, the EE tail was not translated and must not pass.
    let bytes = packet.bytes;
    if bytes.len() < 20 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNCS-too-short");
    }
    let mut cursor = 18;
    for _ in 0..2 {
        if cursor >= bytes.len() {
            return StrictDecision::quarantine("BN", packet.tag.name(), "BNCS-segment-overflow");
        }
        let len = bytes[cursor] as usize;
        cursor += 1;
        if cursor + len > bytes.len() {
            return StrictDecision::quarantine("BN", packet.tag.name(), "BNCS-segment-overflow");
        }
        cursor += len;
    }
    if cursor != bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNCS-untranslated-ee-tail");
    }
    StrictDecision::allow("BN", packet.tag.name(), "known-diamond-client-control")
}

fn validate_bnvs(packet: &BnPacket<'_>) -> StrictDecision {
    // Diamond `sub_5F8460` reads status, verifier count, verifier counted
    // strings, one mandatory response string, then an optional password
    // response when status is `P`. HG account login expects the three-key
    // Diamond verifier list rather than EE's one-key response.
    let bytes = packet.bytes;
    if bytes.len() < 6 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNVS-too-short");
    }
    let status = bytes[4];
    let count = bytes[5] as usize;
    if status != b'V' && status != b'P' {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNVS-invalid-status");
    }
    if count < 3 {
        return StrictDecision::quarantine(
            "BN",
            packet.tag.name(),
            "BNVS-verifier-count-too-small",
        );
    }

    let mut cursor = 6;
    for _ in 0..count {
        if !consume_counted(bytes, &mut cursor) {
            return StrictDecision::quarantine("BN", packet.tag.name(), "BNVS-verifier-overflow");
        }
    }
    if !consume_counted(bytes, &mut cursor) {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNVS-response-overflow");
    }
    if status == b'P' && !consume_counted(bytes, &mut cursor) {
        return StrictDecision::quarantine(
            "BN",
            packet.tag.name(),
            "BNVS-password-response-overflow",
        );
    }
    if cursor != bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNVS-trailing-bytes");
    }
    StrictDecision::allow("BN", packet.tag.name(), "known-diamond-verifier-control")
}

fn validate_bner(packet: &BnPacket<'_>) -> StrictDecision {
    // EE `HandleBNERMessage` requires at least 9 bytes, reads section at offset
    // 7, reads a one-byte session-name length at offset 8, and rejects section
    // values >= 6 or names that run beyond the datagram.
    let bytes = packet.bytes;
    if bytes.len() < 9 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNER-too-short");
    }
    let section = bytes[7];
    let name_len = bytes[8] as usize;
    if section >= 6 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNER-invalid-section");
    }
    if 9 + name_len != bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNER-name-overflow");
    }
    StrictDecision::allow(
        "BN",
        packet.tag.name(),
        "known-ee-server-enumerate-response",
    )
}

fn validate_bncr(packet: &BnPacket<'_>) -> StrictDecision {
    // Decompile-backed shape:
    // EE `HandleBNCRMessage` requires status at offset 6 and accepts reject
    // (`R`) plus one detail byte or challenge statuses (`P`/`V`) followed by
    // exact counted challenge strings. Diamond's writer produces the same
    // offset-6 layout after its two-byte port field.
    let bytes = packet.bytes;
    if bytes.len() < 8 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNCR-too-short");
    }
    match bytes[6] {
        b'R' => {
            if bytes.len() == 8 {
                StrictDecision::allow("BN", packet.tag.name(), "known-legacy-BNCR-reject")
            } else {
                StrictDecision::quarantine("BN", packet.tag.name(), "BNCR-reject-trailing-bytes")
            }
        }
        b'P' | b'V' => {
            let mut cursor = 7;
            if bytes[6] == b'P' && !consume_counted(bytes, &mut cursor) {
                return StrictDecision::quarantine(
                    "BN",
                    packet.tag.name(),
                    "BNCR-password-challenge-overflow",
                );
            }
            if !consume_counted(bytes, &mut cursor) || !consume_counted(bytes, &mut cursor) {
                return StrictDecision::quarantine(
                    "BN",
                    packet.tag.name(),
                    "BNCR-verifier-challenge-overflow",
                );
            }
            if cursor != bytes.len() {
                return StrictDecision::quarantine(
                    "BN",
                    packet.tag.name(),
                    "BNCR-trailing-bytes",
                );
            }
            StrictDecision::allow("BN", packet.tag.name(), "known-legacy-BNCR-challenge")
        }
        _ => StrictDecision::quarantine("BN", packet.tag.name(), "BNCR-invalid-status"),
    }
}

fn validate_bnvr(packet: &BnPacket<'_>) -> StrictDecision {
    // Decompile-backed shape:
    // Diamond `BNVR` reject is exactly six bytes (`BNVR`, `R`, reason) and
    // accept is exactly nine bytes (`BNVR`, `A`, u32le window value). EE's
    // `HandleBNVRMessage` accepts these legacy forms, so strict mode requires
    // one of those exact cursor-consumed packets.
    let bytes = packet.bytes;
    if bytes.len() < 6 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNVR-too-short");
    }
    match bytes[4] {
        b'R' if bytes.len() == 6 => {
            StrictDecision::allow("BN", packet.tag.name(), "known-legacy-BNVR-reject")
        }
        b'A' if bytes.len() == 9 => {
            StrictDecision::allow("BN", packet.tag.name(), "known-legacy-BNVR-accept")
        }
        b'R' => StrictDecision::quarantine("BN", packet.tag.name(), "BNVR-reject-invalid-length"),
        b'A' => StrictDecision::quarantine("BN", packet.tag.name(), "BNVR-accept-invalid-length"),
        _ => StrictDecision::quarantine("BN", packet.tag.name(), "BNVR-invalid-status"),
    }
}

fn validate_bndp(packet: &BnPacket<'_>) -> StrictDecision {
    // EE `CNetLayerInternal::HandleBNDPMessage` accepts the 8-byte no-string
    // disconnect form (`BNDP` + u32 reason) and optionally reads a u16 string
    // length plus a sub-0x400 byte reason string. We require exact cursor
    // consumption so a malformed or overlong legacy disconnect cannot slide
    // through as arbitrary EE direct-control data.
    let bytes = packet.bytes;
    if bytes.len() == 8 {
        return StrictDecision::allow(
            "BN",
            packet.tag.name(),
            "known-ee-disconnect-with-reason-code",
        );
    }
    if bytes.len() < 10 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNDP-string-header-overflow");
    }
    let reason_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    if reason_len >= 0x400 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNDP-string-too-long");
    }
    if 10 + reason_len != bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNDP-string-overflow");
    }
    StrictDecision::allow(
        "BN",
        packet.tag.name(),
        "known-ee-disconnect-with-reason-string",
    )
}

fn consume_counted(bytes: &[u8], cursor: &mut usize) -> bool {
    if *cursor >= bytes.len() {
        return false;
    }
    let len = bytes[*cursor] as usize;
    *cursor += 1;
    if *cursor + len > bytes.len() {
        return false;
    }
    *cursor += len;
    true
}

fn validate_bnxr(packet: &BnPacket<'_>) -> StrictDecision {
    // Decompile-backed shape:
    // EE `HandleBNXRMessage` parses the extended BNXR form with an 0xFD marker,
    // a counted module name at offset 19/20, then probes one byte after the
    // module name as an optional extended-section tag. Tag 0x02 is the NWSync
    // advertisement section used by `NWSync::Advertisement::ReadFromNetwork`.
    const EXTENDED_MARKER_OFFSET: usize = 6;
    const LENGTH_HINT_OFFSET: usize = 18;
    const MODULE_NAME_LENGTH_OFFSET: usize = 19;
    const MODULE_NAME_OFFSET: usize = 20;
    const EXTENDED_MARKER: u8 = 0xFD;
    const NWSYNC_SECTION_TAG: u8 = 0x02;

    let bytes = packet.bytes;
    if bytes.len() < MODULE_NAME_OFFSET || bytes.get(EXTENDED_MARKER_OFFSET) != Some(&EXTENDED_MARKER) {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-invalid-extended-header");
    }
    let length_hint_end = MODULE_NAME_OFFSET + bytes[LENGTH_HINT_OFFSET] as usize;
    if length_hint_end > bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-length-hint-overflow");
    }
    let module_end = MODULE_NAME_OFFSET + bytes[MODULE_NAME_LENGTH_OFFSET] as usize;
    if module_end > bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-module-name-overflow");
    }
    if module_end == bytes.len() || bytes[module_end] != NWSYNC_SECTION_TAG {
        return StrictDecision::allow("BN", packet.tag.name(), "known-bnxr-extended-server-control");
    }

    let mut cursor = module_end + 1;
    let Some(enabled) = bytes.get(cursor).copied() else {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-nwsync-truncated");
    };
    cursor += 1;
    if enabled == 0 {
        return StrictDecision::allow("BN", packet.tag.name(), "known-bnxr-nwsync-disabled");
    }
    if !consume_counted(bytes, &mut cursor) || !consume_counted(bytes, &mut cursor) {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-nwsync-string-overflow");
    }
    let Some(manifest_count) = bytes.get(cursor).copied().map(usize::from) else {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-nwsync-manifest-count-overflow");
    };
    cursor += 1;
    for _ in 0..manifest_count {
        cursor = match cursor.checked_add(2) {
            Some(cursor) if cursor <= bytes.len() => cursor,
            _ => return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-nwsync-manifest-overflow"),
        };
        if !consume_counted(bytes, &mut cursor) {
            return StrictDecision::quarantine("BN", packet.tag.name(), "BNXR-nwsync-manifest-hash-overflow");
        }
    }
    StrictDecision::allow("BN", packet.tag.name(), "known-bnxr-nwsync-advertisement")
}

fn validate_bnxi(packet: &BnPacket<'_>) -> StrictDecision {
    // EE `RequestExtendedServerInfo` serializes:
    // BNXI, UDP port, three counted strings, four build header bytes where the
    // fourth byte is the build-number length, then three more counted build
    // strings. This exact cursor walk catches both overflow and trailing data.
    let bytes = packet.bytes;
    if bytes.len() < 16 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXI-too-short");
    }
    let mut cursor = 6;
    for _ in 0..3 {
        if cursor >= bytes.len() {
            return StrictDecision::quarantine("BN", packet.tag.name(), "BNXI-string-overflow");
        }
        let len = bytes[cursor] as usize;
        cursor += 1;
        if cursor + len > bytes.len() {
            return StrictDecision::quarantine("BN", packet.tag.name(), "BNXI-string-overflow");
        }
        cursor += len;
    }
    if cursor + 4 > bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXI-build-header-overflow");
    }
    let build_number_len = bytes[cursor + 3] as usize;
    cursor += 4;
    if cursor + build_number_len > bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXI-build-string-overflow");
    }
    cursor += build_number_len;
    for _ in 0..3 {
        if cursor >= bytes.len() {
            return StrictDecision::quarantine(
                "BN",
                packet.tag.name(),
                "BNXI-build-string-overflow",
            );
        }
        let len = bytes[cursor] as usize;
        cursor += 1;
        if cursor + len > bytes.len() {
            return StrictDecision::quarantine(
                "BN",
                packet.tag.name(),
                "BNXI-build-string-overflow",
            );
        }
        cursor += len;
    }
    if cursor != bytes.len() {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNXI-trailing-bytes");
    }
    StrictDecision::allow("BN", packet.tag.name(), "known-ee-extended-info-request")
}

pub fn log_decision(direction: Direction, bytes: &[u8], decision: &StrictDecision, strict: bool) {
    let action = if !strict || decision.allowed() {
        "allow"
    } else {
        "quarantine"
    };
    let m_diagnostic = if action == "quarantine" {
        m_window_diagnostic(bytes).unwrap_or_default()
    } else {
        String::new()
    };
    tracing::info!(
        direction = direction.as_str(),
        action,
        family = decision.family,
        name = decision.name,
        reason = decision.reason,
        len = bytes.len(),
        prefix = %hex_prefix(bytes, 96),
        m = %m_diagnostic,
        "strict translation decision"
    );
}

fn m_window_diagnostic(bytes: &[u8]) -> Option<String> {
    let Packet::M(frame) = Packet::classify(bytes) else {
        return None;
    };
    let view = frame.parsed?;
    let primary = view
        .high
        .map(|high| format!("{:02X}/{:02X} {}", high.major, high.minor, high.name()))
        .unwrap_or_else(|| "-".to_string());
    let mut parts = vec![format!(
        "seq={} ack={} flags=0x{:02X} pktseq={} decl={} payload={} avail={} trail={} primary={}",
        view.sequence,
        view.ack_sequence,
        view.flags,
        view.packetized_sequence,
        view.declared_payload_length,
        view.payload_length,
        view.available_payload_length,
        view.trailing_payload_length,
        primary,
    )];

    if view.trailing_payload_length != 0 {
        let trailing_offset = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + view.payload_length;
        match parse_packetized_spans(bytes, trailing_offset) {
            Some(spans) => {
                for span in spans {
                    let high = span
                        .high
                        .map(|high| {
                            format!("{:02X}/{:02X} {}", high.major, high.minor, high.name())
                        })
                        .unwrap_or_else(|| "-".to_string());
                    let deflated = span
                        .deflated
                        .as_ref()
                        .map(|deflated| {
                            format!(
                                "deflated(inflated={} plausible={})",
                                deflated.inflated_length, deflated.plausible
                            )
                        })
                        .unwrap_or_else(|| "-".to_string());
                    parts.push(format!(
                        "span@{} flags=0x{:02X} pktseq={} decl={} payload={} record={} high={} {}",
                        span.offset,
                        span.flags,
                        span.packetized_sequence,
                        span.declared_payload_length,
                        span.payload_length,
                        span.record_length,
                        high,
                        deflated,
                    ));
                }
            }
            None => parts.push(format!("span-parse-failed@{}", trailing_offset)),
        }
    }

    Some(parts.join(" | "))
}
