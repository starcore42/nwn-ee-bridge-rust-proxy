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
    translate::{
        ContinuationOwner, VerifiedFamily, VerifiedProof, area, char_list, chat,
        client_gui_inventory, client_input, client_login,
        client_quickbar, gameplay_stream, journal, live_object_update, play_module_character_list,
        player_list, quickbar,
    },
};
use flate2::read::ZlibDecoder;
use std::{
    fs,
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
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
                if let Err(decision) = validate_packetized_trailing(bytes, trailing_offset, profile)
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
            if let Some(deflated) = &view.deflated {
                // The M transport flag is the authoritative owner of this
                // payload shape. A little-endian inflated length can begin
                // with byte 0x50 (`P`) and the next two bytes can look like a
                // high-level family/minor pair, as happened with a rewritten
                // live-object window whose length was 0x00000350. The
                // decompiled window path enters the compressed/extended branch
                // from the frame flags before CNW high-level dispatch, so
                // strict validation must do the same and never let an
                // accidental `P xx yy` length prefix win over a plausible
                // deflated envelope.
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

pub fn decide_verified_translated(
    direction: Direction,
    family: VerifiedFamily,
    bytes: &[u8],
) -> StrictDecision {
    match Packet::classify(bytes) {
        Packet::M(frame) => {
            let Some(view) = &frame.parsed else {
                return StrictDecision::quarantine("M", family.as_str(), "parse-failed");
            };
            if !view.crc_valid {
                return StrictDecision::quarantine("M", family.as_str(), "crc-mismatch");
            }
            if view.declared_payload_length != 0
                && view.declared_payload_length > view.available_payload_length
            {
                return StrictDecision::quarantine(
                    "M",
                    family.as_str(),
                    "declared-payload-overflow",
                );
            }

            if view.payload_length == 0 && view.trailing_payload_length == 0 {
                if family == VerifiedFamily::ConsumedEmptyMFrame {
                    return StrictDecision::allow(
                        "M/verified-empty",
                        family.as_str(),
                        "verified-consumed-empty-frame",
                    );
                }
                if server_zlib_stream_continuation_empty_progress_valid(direction, family) {
                    return StrictDecision::allow(
                        "M/verified-empty",
                        family.as_str(),
                        "verified-server-zlib-continuation-empty-progress-frame",
                    );
                }
                return StrictDecision::quarantine(
                    "M/verified-empty",
                    family.as_str(),
                    "empty-frame-family-mismatch",
                );
            }

            if family == VerifiedFamily::ConsumedEmptyMFrame {
                return StrictDecision::quarantine(
                    "M/verified-empty",
                    family.as_str(),
                    "consumed-frame-not-empty",
                );
            }

            if verified_family_allows_deflated_continuation(family)
                && matches!(
                    direction,
                    Direction::ServerToClient | Direction::ServerToClientSynthetic
                )
                && view.payload_length != 0
                && view.payload_length == view.available_payload_length
                && view.trailing_payload_length == 0
                && view.packetized_sequence == 0
                && (view.flags & 0x08) != 0
                && (view.flags & 0x04) == 0
            {
                return StrictDecision::allow(
                    "M/verified-deflated-continuation",
                    family.as_str(),
                    "verified-family-deflated-continuation-frame",
                );
            }

            if family == VerifiedFamily::CoalescedWindow {
                if coalesced_window_shape_valid(frame.bytes, view) {
                    return StrictDecision::allow(
                        "M/verified-coalesced",
                        family.as_str(),
                        "verified-family-exact-coalesced-window-shape",
                    );
                }
                return StrictDecision::quarantine(
                    "M/verified-coalesced",
                    family.as_str(),
                    "verified-family-coalesced-window-invalid-shape",
                );
            }

            if let Some(deflated) = &view.deflated {
                if !deflated.plausible {
                    return StrictDecision::quarantine(
                        "M/verified-deflated",
                        family.as_str(),
                        "verified-family-invalid-deflated-envelope",
                    );
                }
                if family == VerifiedFamily::SemanticDeflated {
                    return StrictDecision::quarantine(
                        "M/verified-deflated",
                        family.as_str(),
                        "verified-deflated-missing-semantic-family",
                    );
                }
                let Some(inflated) = inflate_verified_deflated_payload(
                    frame.bytes,
                    view.payload_length,
                    deflated.inflated_length,
                ) else {
                    return StrictDecision::quarantine(
                        "M/verified-deflated",
                        family.as_str(),
                        "verified-family-deflated-inflate-failed",
                    );
                };
                if verified_family_inflated_payload_valid(family, &inflated) {
                    return StrictDecision::allow(
                        "M/verified-deflated-exact",
                        family.as_str(),
                        match direction {
                            Direction::ServerToClient => {
                                "verified-family-deflated-exact-high-level-shape"
                            }
                            Direction::ServerToClientSynthetic => {
                                "verified-family-synthetic-deflated-exact-high-level-shape"
                            }
                            Direction::ClientToServer => {
                                "unexpected-client-verified-deflated-exact-high-level-shape"
                            }
                        },
                    );
                }
                return StrictDecision::quarantine(
                    "M/verified-deflated",
                    family.as_str(),
                    "verified-family-deflated-high-level-invalid-shape",
                );
            }

            if let Some(high) = view.high {
                let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
                let payload_end = payload_start + view.payload_length;
                let Some(payload) = frame.bytes.get(payload_start..payload_end) else {
                    return StrictDecision::quarantine(
                        "M/verified-high",
                        family.as_str(),
                        "verified-high-payload-overflow",
                    );
                };
                if verified_family_inflated_payload_valid(family, payload) {
                    return StrictDecision::allow(
                        "M/verified-high",
                        family.as_str(),
                        "verified-family-exact-high-level-shape",
                    );
                }
                return StrictDecision::quarantine(
                    "M/verified-high",
                    high.name(),
                    "verified-family-high-level-invalid-shape",
                );
            }

            StrictDecision::quarantine(
                "M/verified",
                family.as_str(),
                "verified-family-unclassified-M-payload",
            )
        }
        Packet::Bn(_) => {
            StrictDecision::quarantine("BN", family.as_str(), "verified-translation-not-M")
        }
        Packet::UnknownTopLevel(_) => {
            StrictDecision::quarantine("top-level", family.as_str(), "unknown-top-level")
        }
    }
}

fn server_zlib_stream_continuation_empty_progress_valid(
    direction: Direction,
    family: VerifiedFamily,
) -> bool {
    if !matches!(
        direction,
        Direction::ServerToClient | Direction::ServerToClientSynthetic
    ) {
        return false;
    }

    match family {
        VerifiedFamily::ServerZlibStreamContinuation {
            owner,
            stream_epoch,
            ..
        } => owner != ContinuationOwner::UnknownProxyOwned && stream_epoch != 0,
        _ => false,
    }
}

pub fn decide_verified_proof_translated(
    direction: Direction,
    proof: &VerifiedProof,
    bytes: &[u8],
) -> StrictDecision {
    match proof {
        VerifiedProof::Family(family) => decide_verified_translated(direction, *family, bytes),
        VerifiedProof::GameplayStream(families) => {
            decide_verified_gameplay_stream_translated(direction, families, bytes)
        }
        VerifiedProof::CoalescedWindow(records) => {
            decide_verified_coalesced_window_translated(direction, records, bytes)
        }
    }
}

fn decide_verified_gameplay_stream_translated(
    direction: Direction,
    families: &[VerifiedFamily],
    bytes: &[u8],
) -> StrictDecision {
    if families.is_empty() {
        return StrictDecision::quarantine(
            "M/verified-gameplay-stream",
            "GameplayStream",
            "empty-gameplay-proof",
        );
    }

    match Packet::classify(bytes) {
        Packet::M(frame) => {
            let Some(view) = &frame.parsed else {
                return StrictDecision::quarantine(
                    "M/verified-gameplay-stream",
                    "GameplayStream",
                    "parse-failed",
                );
            };
            if !view.crc_valid {
                return StrictDecision::quarantine(
                    "M/verified-gameplay-stream",
                    "GameplayStream",
                    "crc-mismatch",
                );
            }
            if view.declared_payload_length != 0
                && view.declared_payload_length > view.available_payload_length
            {
                return StrictDecision::quarantine(
                    "M/verified-gameplay-stream",
                    "GameplayStream",
                    "declared-payload-overflow",
                );
            }

            if view.payload_length == 0 || view.trailing_payload_length != 0 {
                return StrictDecision::quarantine(
                    "M/verified-gameplay-stream",
                    "GameplayStream",
                    "invalid-gameplay-stream-window",
                );
            }

            if let Some(deflated) = &view.deflated {
                if !deflated.plausible {
                    return StrictDecision::quarantine(
                        "M/verified-gameplay-stream",
                        "GameplayStream",
                        "invalid-deflated-envelope",
                    );
                }
                let Some(inflated) = inflate_verified_deflated_payload(
                    frame.bytes,
                    view.payload_length,
                    deflated.inflated_length,
                ) else {
                    return StrictDecision::quarantine(
                        "M/verified-gameplay-stream",
                        "GameplayStream",
                        "deflated-inflate-failed",
                    );
                };
                if verified_gameplay_stream_payload_valid(families, &inflated) {
                    return StrictDecision::allow(
                        "M/verified-gameplay-stream-deflated",
                        "GameplayStream",
                        match direction {
                            Direction::ServerToClient => {
                                "verified-unit-proof-deflated-high-level-shapes"
                            }
                            Direction::ServerToClientSynthetic => {
                                "verified-unit-proof-synthetic-deflated-high-level-shapes"
                            }
                            Direction::ClientToServer => {
                                "unexpected-client-verified-unit-proof-deflated-high-level-shapes"
                            }
                        },
                    );
                }
                return StrictDecision::quarantine(
                    "M/verified-gameplay-stream",
                    "GameplayStream",
                    "deflated-unit-proof-high-level-invalid-shape",
                );
            }

            if view.high.is_some() {
                let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
                let payload_end = payload_start + view.payload_length;
                let Some(payload) = frame.bytes.get(payload_start..payload_end) else {
                    return StrictDecision::quarantine(
                        "M/verified-gameplay-stream",
                        "GameplayStream",
                        "high-payload-overflow",
                    );
                };
                if verified_gameplay_stream_payload_valid(families, payload) {
                    return StrictDecision::allow(
                        "M/verified-gameplay-stream",
                        "GameplayStream",
                        "verified-unit-proof-high-level-shapes",
                    );
                }
                return StrictDecision::quarantine(
                    "M/verified-gameplay-stream",
                    "GameplayStream",
                    "unit-proof-high-level-invalid-shape",
                );
            }

            StrictDecision::quarantine(
                "M/verified-gameplay-stream",
                "GameplayStream",
                "unclassified-M-payload",
            )
        }
        Packet::Bn(_) => {
            StrictDecision::quarantine("BN", "GameplayStream", "verified-proof-translation-not-M")
        }
        Packet::UnknownTopLevel(_) => {
            StrictDecision::quarantine("top-level", "GameplayStream", "unknown-top-level")
        }
    }
}

fn decide_verified_coalesced_window_translated(
    direction: Direction,
    record_proofs: &[VerifiedProof],
    bytes: &[u8],
) -> StrictDecision {
    if record_proofs.is_empty() {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "empty-coalesced-proof",
        );
    }

    let Packet::M(frame) = Packet::classify(bytes) else {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "verified-proof-translation-not-M",
        );
    };
    let Some(view) = &frame.parsed else {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "parse-failed",
        );
    };
    if !view.crc_valid {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "crc-mismatch",
        );
    }
    // `MFrameView` treats declared length 0 as a whole-datagram sentinel for
    // broad frame classification. EE's decompiled
    // `CNetLayerWindow::UnpacketizeFullMessages`, however, walks coalesced
    // reliable-window records as `12 byte record header + declared length`.
    // In that exact typed coalesced context, a zero declared primary record is
    // a valid empty progress shell and the next record begins immediately at
    // offset 12. Only allow that narrower interpretation when the translator
    // supplied per-record proofs for additional coalesced records.
    let primary_payload_length = if view.declared_payload_length == 0 && record_proofs.len() > 1 {
        0
    } else if view.declared_payload_length != view.payload_length
        || view.payload_length > view.available_payload_length
    {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "coalesced-primary-declared-mismatch",
        );
    } else {
        view.payload_length
    };

    if primary_payload_length > view.available_payload_length {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "coalesced-primary-declared-mismatch",
        );
    }

    let primary_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let primary_end = primary_start + primary_payload_length;
    let Some(primary_payload) = frame.bytes.get(primary_start..primary_end) else {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "coalesced-primary-payload-overflow",
        );
    };

    let primary_deflated = if primary_payload_length == view.payload_length {
        view.deflated
            .as_ref()
            .map(|deflated| (deflated.plausible, deflated.inflated_length))
    } else {
        None
    };
    if !coalesced_record_proof_valid(
        direction,
        &record_proofs[0],
        primary_payload,
        primary_deflated,
    ) {
        tracing::warn!(
            record_index = 0usize,
            proof = record_proofs[0].as_str(),
            payload_len = primary_payload.len(),
            prefix = %hex_prefix(primary_payload, 64),
            "coalesced typed proof rejected primary payload"
        );
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            record_proofs[0].as_str(),
            "coalesced-record-proof-invalid",
        );
    }

    let spans = if primary_end == frame.bytes.len() {
        Vec::new()
    } else {
        let Some(spans) = parse_packetized_spans(frame.bytes, primary_end) else {
            return StrictDecision::quarantine(
                "M/verified-coalesced",
                "CoalescedWindow",
                "coalesced-span-parse-failed",
            );
        };
        spans
    };

    if 1 + spans.len() != record_proofs.len() {
        return StrictDecision::quarantine(
            "M/verified-coalesced",
            "CoalescedWindow",
            "coalesced-proof-record-count-mismatch",
        );
    }

    for (record_index, (span, proof)) in spans.iter().zip(record_proofs.iter().skip(1)).enumerate()
    {
        if span.declared_payload_length != span.payload_length {
            return StrictDecision::quarantine(
                "M/verified-coalesced",
                proof.as_str(),
                "coalesced-span-declared-mismatch",
            );
        }
        let payload_offset = span.offset + LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
        let payload_end = payload_offset + span.payload_length;
        let Some(payload) = frame.bytes.get(payload_offset..payload_end) else {
            return StrictDecision::quarantine(
                "M/verified-coalesced",
                proof.as_str(),
                "coalesced-span-payload-overflow",
            );
        };
        let span_deflated = span
            .deflated
            .as_ref()
            .map(|deflated| (deflated.plausible, deflated.inflated_length));
        if !coalesced_record_proof_valid(direction, proof, payload, span_deflated) {
            tracing::warn!(
                record_index = record_index + 1,
                proof = proof.as_str(),
                payload_len = payload.len(),
                declared_payload_length = span.declared_payload_length,
                span_offset = span.offset,
                prefix = %hex_prefix(payload, 64),
                "coalesced typed proof rejected span payload"
            );
            return StrictDecision::quarantine(
                "M/verified-coalesced",
                proof.as_str(),
                "coalesced-record-proof-invalid",
            );
        }
    }

    StrictDecision::allow(
        "M/verified-coalesced",
        "CoalescedWindow",
        "verified-family-exact-coalesced-window-proofs",
    )
}

fn coalesced_record_proof_valid(
    direction: Direction,
    proof: &VerifiedProof,
    payload: &[u8],
    deflated: Option<(bool, usize)>,
) -> bool {
    match proof {
        VerifiedProof::Family(VerifiedFamily::ConsumedEmptyMFrame) => payload.is_empty(),
        VerifiedProof::Family(family @ VerifiedFamily::ServerZlibStreamContinuation { .. }) => {
            payload.is_empty()
                && server_zlib_stream_continuation_empty_progress_valid(direction, *family)
        }
        VerifiedProof::Family(family) => coalesced_family_payload_valid(*family, payload, deflated),
        VerifiedProof::GameplayStream(families) => {
            coalesced_gameplay_stream_payload_valid(families, payload, deflated)
        }
        VerifiedProof::CoalescedWindow(_) => false,
    }
}

fn coalesced_family_payload_valid(
    family: VerifiedFamily,
    payload: &[u8],
    deflated: Option<(bool, usize)>,
) -> bool {
    if payload.is_empty() {
        return family == VerifiedFamily::ConsumedEmptyMFrame;
    }

    if let Some((plausible, inflated_length)) = deflated {
        if !plausible {
            return false;
        }
        let Some(inflated) =
            inflate_verified_deflated_combined_payload(payload, Some(inflated_length))
        else {
            dump_strict_coalesced_inflated_reject(
                family,
                payload,
                None,
                "coalesced-deflated-inflate-failed",
            );
            return false;
        };
        let valid = verified_family_inflated_payload_valid(family, &inflated);
        if !valid {
            dump_strict_coalesced_inflated_reject(
                family,
                payload,
                Some(&inflated),
                "coalesced-deflated-family-invalid",
            );
        }
        return valid;
    }

    HighLevel::parse(payload)
        .map(|_| verified_family_inflated_payload_valid(family, payload))
        .unwrap_or(false)
}

fn coalesced_gameplay_stream_payload_valid(
    families: &[VerifiedFamily],
    payload: &[u8],
    deflated: Option<(bool, usize)>,
) -> bool {
    if payload.is_empty() {
        return false;
    }

    if let Some((plausible, inflated_length)) = deflated {
        if !plausible {
            return false;
        }
        let Some(inflated) =
            inflate_verified_deflated_combined_payload(payload, Some(inflated_length))
        else {
            dump_strict_gameplay_stream_reject(
                families,
                payload,
                None,
                "coalesced-gameplay-stream-inflate-failed",
            );
            return false;
        };
        let valid = verified_gameplay_stream_payload_valid(families, &inflated);
        if !valid {
            dump_strict_gameplay_stream_reject(
                families,
                payload,
                Some(&inflated),
                "coalesced-gameplay-stream-family-invalid",
            );
        }
        return valid;
    }

    HighLevel::parse(payload)
        .map(|_| verified_gameplay_stream_payload_valid(families, payload))
        .unwrap_or(false)
}

fn dump_strict_coalesced_inflated_reject(
    family: VerifiedFamily,
    deflated_payload: &[u8],
    inflated: Option<&[u8]>,
    reason: &str,
) {
    let name = family.as_str();
    dump_strict_payload_reject(name, deflated_payload, inflated, reason);
}

fn dump_strict_gameplay_stream_reject(
    families: &[VerifiedFamily],
    deflated_payload: &[u8],
    inflated: Option<&[u8]>,
    reason: &str,
) {
    let mut name = String::from("GameplayStream");
    for family in families.iter().take(4) {
        name.push('-');
        name.push_str(family.as_str());
    }
    dump_strict_payload_reject(&name, deflated_payload, inflated, reason);
}

fn dump_strict_payload_reject(
    name: &str,
    deflated_payload: &[u8],
    inflated: Option<&[u8]>,
    reason: &str,
) {
    let Some(mut path) = crate::translate::diagnostics::diagnostic_dump_dir() else {
        return;
    };
    if fs::create_dir_all(&path).is_err() {
        return;
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let safe_name = name.replace(['<', '>', '/', '\\', ':', '*', '?', '"', '|'], "_");
    path.push(format!("strict-{reason}-{safe_name}-{millis}.bin"));

    let bytes = inflated.unwrap_or(deflated_payload);
    if fs::write(&path, bytes).is_ok() {
        tracing::warn!(
            path = %path.display(),
            reason,
            family = safe_name,
            deflated_len = deflated_payload.len(),
            inflated_len = inflated.map(|bytes| bytes.len()).unwrap_or(0),
            inflated = inflated.is_some(),
            prefix = %hex_prefix(bytes, 64),
            "strict dumped rejected coalesced semantic payload for fixture analysis"
        );
    }
}
fn verified_gameplay_stream_payload_valid(families: &[VerifiedFamily], payload: &[u8]) -> bool {
    let split = gameplay_stream::split_inflated_gameplay(payload);
    if !split.complete || split.units.len() != families.len() {
        return false;
    }

    split
        .units
        .iter()
        .zip(families)
        .all(|(unit, family)| match unit {
            gameplay_stream::GameplayUnit::HighLevel(message) => {
                verified_family_inflated_payload_valid(*family, message.payload)
            }
            gameplay_stream::GameplayUnit::Continuation(_)
            | gameplay_stream::GameplayUnit::PendingFragment(_)
            | gameplay_stream::GameplayUnit::Unknown(_) => false,
        })
}

pub fn decide_verified_translated_batch(
    direction: Direction,
    family: VerifiedFamily,
    packets: &[Vec<u8>],
) -> Option<StrictDecision> {
    if packets.len() <= 1
        || !matches!(
            direction,
            Direction::ServerToClient | Direction::ServerToClientSynthetic
        )
    {
        return None;
    }

    let Packet::M(first_frame) = Packet::classify(packets.first()?.as_slice()) else {
        return None;
    };
    let first_view = match &first_frame.parsed {
        Some(view) => view,
        None => {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-first-frame-parse-failed",
            ));
        }
    };
    let expected_frames = usize::from(first_view.packetized_sequence);
    if expected_frames <= 1 {
        return None;
    }
    if expected_frames != packets.len() {
        return None;
    }
    if family == VerifiedFamily::SemanticDeflated {
        return Some(StrictDecision::quarantine(
            "M/verified-deflated-batch",
            family.as_str(),
            "verified-deflated-batch-missing-semantic-family",
        ));
    }
    if !verified_family_allows_deflated_continuation(family) {
        return None;
    }

    let mut combined = Vec::new();
    for (index, packet) in packets.iter().enumerate() {
        let Packet::M(frame) = Packet::classify(packet.as_slice()) else {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-member-not-M",
            ));
        };
        let Some(view) = &frame.parsed else {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-member-parse-failed",
            ));
        };
        if !view.crc_valid {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-member-crc-mismatch",
            ));
        }
        if view.declared_payload_length != 0
            && view.declared_payload_length > view.available_payload_length
        {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-member-declared-payload-overflow",
            ));
        }
        if view.trailing_payload_length != 0 {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-member-invalid-payload-window",
            ));
        }
        if index == 0 && usize::from(view.packetized_sequence) != expected_frames {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-first-frame-count-mismatch",
            ));
        }
        if view.payload_length == 0 {
            if index == 0
                || view.available_payload_length != 0
                || view.declared_payload_length != 0
                || view.packetized_sequence != 0
                || view.frame_type != 0
                || (view.flags & !0x08) != 0
            {
                return Some(StrictDecision::quarantine(
                    "M/verified-deflated-batch",
                    family.as_str(),
                    "batch-member-invalid-empty-shell",
                ));
            }
            continue;
        }
        let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
        let payload_end = payload_start + view.payload_length;
        let Some(payload) = frame.bytes.get(payload_start..payload_end) else {
            return Some(StrictDecision::quarantine(
                "M/verified-deflated-batch",
                family.as_str(),
                "batch-member-payload-overflow",
            ));
        };
        combined.extend_from_slice(payload);
    }

    let Some(inflated) = inflate_verified_deflated_combined_payload(&combined, None) else {
        return Some(StrictDecision::quarantine(
            "M/verified-deflated-batch",
            family.as_str(),
            "batch-deflated-inflate-failed",
        ));
    };
    if verified_family_inflated_payload_valid(family, &inflated) {
        return Some(StrictDecision::allow(
            "M/verified-deflated-batch-exact",
            family.as_str(),
            "verified-family-deflated-batch-exact-high-level-shape",
        ));
    }

    Some(StrictDecision::quarantine(
        "M/verified-deflated-batch",
        family.as_str(),
        "batch-deflated-high-level-invalid-shape",
    ))
}

pub fn decide_verified_proof_translated_batch(
    direction: Direction,
    proof: &VerifiedProof,
    packets: &[Vec<u8>],
) -> Option<StrictDecision> {
    match proof {
        VerifiedProof::Family(family) => {
            decide_verified_translated_batch(direction, *family, packets)
        }
        VerifiedProof::CoalescedWindow(_) => None,
        VerifiedProof::GameplayStream(families) => {
            if packets.len() <= 1
                || !matches!(
                    direction,
                    Direction::ServerToClient | Direction::ServerToClientSynthetic
                )
            {
                return None;
            }

            let Packet::M(first_frame) = Packet::classify(packets.first()?.as_slice()) else {
                return None;
            };
            let first_view = match &first_frame.parsed {
                Some(view) => view,
                None => {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-first-frame-parse-failed",
                    ));
                }
            };
            let expected_frames = usize::from(first_view.packetized_sequence);
            if expected_frames <= 1 {
                return None;
            }
            if expected_frames != packets.len() {
                return None;
            }

            let mut combined = Vec::new();
            for (index, packet) in packets.iter().enumerate() {
                let Packet::M(frame) = Packet::classify(packet.as_slice()) else {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-member-not-M",
                    ));
                };
                let Some(view) = &frame.parsed else {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-member-parse-failed",
                    ));
                };
                if !view.crc_valid {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-member-crc-mismatch",
                    ));
                }
                if view.declared_payload_length != 0
                    && view.declared_payload_length > view.available_payload_length
                {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-member-declared-payload-overflow",
                    ));
                }
                if view.trailing_payload_length != 0 {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-member-invalid-payload-window",
                    ));
                }
                if index == 0 && usize::from(view.packetized_sequence) != expected_frames {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-first-frame-count-mismatch",
                    ));
                }
                if view.payload_length == 0 {
                    if index == 0
                        || view.available_payload_length != 0
                        || view.declared_payload_length != 0
                        || view.packetized_sequence != 0
                        || view.frame_type != 0
                        || (view.flags & !0x08) != 0
                    {
                        return Some(StrictDecision::quarantine(
                            "M/verified-gameplay-stream-batch",
                            "GameplayStream",
                            "batch-member-invalid-empty-shell",
                        ));
                    }
                    continue;
                }
                let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
                let payload_end = payload_start + view.payload_length;
                let Some(payload) = frame.bytes.get(payload_start..payload_end) else {
                    return Some(StrictDecision::quarantine(
                        "M/verified-gameplay-stream-batch",
                        "GameplayStream",
                        "batch-member-payload-overflow",
                    ));
                };
                combined.extend_from_slice(payload);
            }

            let Some(inflated) = inflate_verified_deflated_combined_payload(&combined, None) else {
                return Some(StrictDecision::quarantine(
                    "M/verified-gameplay-stream-batch",
                    "GameplayStream",
                    "batch-deflated-inflate-failed",
                ));
            };
            if verified_gameplay_stream_payload_valid(families, &inflated) {
                return Some(StrictDecision::allow(
                    "M/verified-gameplay-stream-batch",
                    "GameplayStream",
                    "verified-unit-proof-deflated-batch-high-level-shapes",
                ));
            }

            Some(StrictDecision::quarantine(
                "M/verified-gameplay-stream-batch",
                "GameplayStream",
                "batch-deflated-unit-proof-high-level-invalid-shape",
            ))
        }
    }
}

fn inflate_verified_deflated_payload(
    bytes: &[u8],
    payload_length: usize,
    expected_inflated_length: usize,
) -> Option<Vec<u8>> {
    let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
    let payload_end = payload_start.checked_add(payload_length)?;
    let payload = bytes.get(payload_start..payload_end)?;
    inflate_verified_deflated_combined_payload(payload, Some(expected_inflated_length))
}

fn inflate_verified_deflated_combined_payload(
    payload: &[u8],
    expected_inflated_length: Option<usize>,
) -> Option<Vec<u8>> {
    let declared_inflated = usize::try_from(read_le_u32(payload, 0)?).ok()?;
    if expected_inflated_length.is_some_and(|expected| declared_inflated != expected) {
        return None;
    }

    let mut decoder = ZlibDecoder::new(payload.get(4..)?);
    let mut inflated = Vec::new();
    decoder.read_to_end(&mut inflated).ok()?;
    if inflated.len() != declared_inflated {
        return None;
    }
    Some(inflated)
}

fn verified_family_inflated_payload_valid(family: VerifiedFamily, payload: &[u8]) -> bool {
    if let VerifiedFamily::ServerZlibStreamContinuation {
        owner,
        stream_epoch,
        first_sequence,
    } = family
    {
        // Remembered zlib-stream ownership is diagnostic context, not an allow
        // proof. No-header bytes are not valid EE-facing gameplay by
        // themselves; a future family-specific stream translator must rebuild
        // a complete typed payload and emit that exact semantic family instead.
        tracing::warn!(
            owner = owner.as_str(),
            stream_epoch,
            first_sequence,
            continuation_len = payload.len(),
            "strict rejected standalone zlib-stream continuation proof without semantic payload"
        );
        return false;
    }

    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };

    match family {
        VerifiedFamily::AreaClientArea => {
            high.major == 0x04 && high.minor == 0x01 && area_client_area_shape_valid(payload)
        }
        VerifiedFamily::CharList => high.major == 0x11 && char_list_shape_valid(payload),
        VerifiedFamily::Chat => high.major == 0x09 && chat_shape_valid(payload, high),
        VerifiedFamily::ClientArea => {
            high.major == 0x04 && high.minor == 0x03 && empty_high_level_shape_valid(payload)
        }
        VerifiedFamily::ClientCharList => {
            high.major == 0x11
                && match high.minor {
                    0x01 => empty_high_level_shape_valid(payload),
                    0x03 => char_list_request_update_char_shape_valid(payload),
                    _ => false,
                }
        }
        VerifiedFamily::ClientGuiInventory => {
            high.major == 0x0D && client_gui_inventory::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientInput => {
            high.major == 0x06
                && matches!(high.minor, 0x01 | 0x03 | 0x05 | 0x09 | 0x0B)
                && client_input::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientLogin => {
            high.major == 0x02
                && match high.minor {
                    0x0D => client_login_waypoint_response_shape_valid(payload),
                    0x11 => client_login_server_subdirectory_shape_valid(payload),
                    _ => false,
                }
        }
        VerifiedFamily::ClientModule => {
            high.major == 0x03 && high.minor == 0x02 && empty_high_level_shape_valid(payload)
        }
        VerifiedFamily::ClientParty => {
            high.major == 0x0E && high.minor == 0x02 && party_get_list_payload_shape_valid(payload)
        }
        VerifiedFamily::ClientQuickbar => {
            high.major == 0x1E
                && high.minor == 0x02
                && client_quickbar::set_button_payload_shape_valid(payload)
        }
        VerifiedFamily::ClientServerStatus => {
            high.major == 0x01 && high.minor == 0x00 && empty_high_level_shape_valid(payload)
        }
        VerifiedFamily::ClientSideMessage => {
            high.major == 0x12 && high.minor == 0x0B && client_side_feedback_shape_valid(payload)
        }
        VerifiedFamily::GameObjUpdateObjectControl => {
            high.major == 0x05
                && high.minor == 0x02
                && game_obj_update_obj_control_shape_valid(payload)
        }
        VerifiedFamily::GameObjUpdateLiveObject => {
            high.major == 0x05 && high.minor == 0x01 && live_object_shape_valid(payload)
        }
        VerifiedFamily::GuiQuickbar => {
            high.major == 0x1E && high.minor == 0x01 && quickbar_shape_valid(payload)
        }
        VerifiedFamily::GuiQuickbarPlaceholder => quickbar_placeholder_shape_valid(payload),
        VerifiedFamily::Inventory => {
            high.major == 0x0C
                && matches!(high.minor, 0x01 | 0x02 | 0x03)
                && bare_or_cnw_wrapped_payload_shape_valid(payload)
        }
        VerifiedFamily::Journal => {
            high.major == 0x1C && high.minor == 0x0C && journal_shape_valid(payload)
        }
        VerifiedFamily::LoadBar => {
            high.major == 0x2C
                && (0x01..=0x03).contains(&high.minor)
                && loadbar_shape_valid(payload)
        }
        VerifiedFamily::Login => {
            high.major == 0x02
                && matches!(high.minor, 0x05 | 0x0C)
                && empty_high_level_shape_valid(payload)
        }
        VerifiedFamily::ModuleInfo => {
            high.major == 0x03 && high.minor == 0x01 && module_info_shape_valid(payload)
        }
        VerifiedFamily::ModuleTime => {
            high.major == 0x03 && high.minor == 0x03 && module_time_shape_valid(payload)
        }
        VerifiedFamily::Party => {
            high.major == 0x0E
                && match high.minor {
                    0x02 => party_get_list_payload_shape_valid(payload),
                    0x01 | 0x03..=0x0E => party_cnw_wrapped_payload_shape_valid(payload),
                    _ => false,
                }
        }
        VerifiedFamily::PlayModuleCharacterList => {
            high.major == 0x31
                && play_module_character_list::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::PlayerList => {
            high.major == 0x0A
                && matches!(high.minor, 0x01 | 0x02 | 0x03)
                && player_list_shape_valid(payload)
        }
        VerifiedFamily::SetCustomToken => {
            high.major == 0x32
                && match high.minor {
                    0x01 => set_custom_token_shape_valid(payload),
                    0x02 => set_custom_token_list_shape_valid(payload),
                    _ => false,
                }
        }
        VerifiedFamily::ServerStatusModuleResources => {
            high.major == 0x01
                && high.minor == 0x03
                && server_status_module_resources_shape_valid(payload)
        }
        VerifiedFamily::CoalescedWindow
        | VerifiedFamily::ConsumedEmptyMFrame
        | VerifiedFamily::SemanticDeflated
        | VerifiedFamily::ServerZlibStreamContinuation { .. } => false,
    }
}

fn verified_family_allows_deflated_continuation(family: VerifiedFamily) -> bool {
    matches!(
        family,
        VerifiedFamily::AreaClientArea
            | VerifiedFamily::CharList
            | VerifiedFamily::Chat
            | VerifiedFamily::ClientSideMessage
            | VerifiedFamily::GameObjUpdateObjectControl
            | VerifiedFamily::GameObjUpdateLiveObject
            | VerifiedFamily::GuiQuickbar
            | VerifiedFamily::GuiQuickbarPlaceholder
            | VerifiedFamily::Inventory
            | VerifiedFamily::Journal
            | VerifiedFamily::LoadBar
            | VerifiedFamily::Login
            | VerifiedFamily::ModuleInfo
            | VerifiedFamily::ModuleTime
            | VerifiedFamily::Party
            | VerifiedFamily::PlayModuleCharacterList
            | VerifiedFamily::PlayerList
            | VerifiedFamily::SetCustomToken
            | VerifiedFamily::ServerStatusModuleResources
    )
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

fn exact_high_payload_shape_valid(payload: &[u8]) -> bool {
    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    matches!(
        high_payload_validation(payload, high),
        HighPayloadValidation::Exact(true)
    )
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
        (0x01, 0x03) => {
            HighPayloadValidation::Exact(server_status_module_running_shape_valid(payload))
        }
        (0x02, 0x05 | 0x0C) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x02, 0x0D) => {
            HighPayloadValidation::Exact(client_login_waypoint_response_shape_valid(payload))
        }
        (0x02, 0x11) => {
            HighPayloadValidation::Exact(client_login_server_subdirectory_shape_valid(payload))
        }
        (0x03, 0x01) => HighPayloadValidation::Exact(module_info_shape_valid(payload)),
        (0x03, 0x02) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x03, 0x03) => HighPayloadValidation::Exact(module_time_shape_valid(payload)),
        (0x04, 0x01) => HighPayloadValidation::Exact(area_client_area_shape_valid(payload)),
        (0x04, 0x03) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x05, 0x01) => HighPayloadValidation::Exact(live_object_shape_valid(payload)),
        (0x05, 0x02) => {
            HighPayloadValidation::Exact(game_obj_update_obj_control_shape_valid(payload))
        }
        (0x06, 0x01 | 0x03 | 0x0B) => {
            HighPayloadValidation::Exact(client_input::claim_payload_if_verified(payload).is_some())
        }
        (0x09, 0x04 | 0x05 | 0x0B | 0x0C) => {
            HighPayloadValidation::Exact(chat_shape_valid(payload, high))
        }
        (0x0A, 0x01 | 0x02 | 0x03) => {
            HighPayloadValidation::Exact(player_list_shape_valid(payload))
        },
        (0x0D, 0x01 | 0x02) => {
            HighPayloadValidation::Exact(client_gui_inventory::claim_payload_if_verified(payload).is_some())
        }
        (0x0E, 0x02) => HighPayloadValidation::Exact(party_get_list_payload_shape_valid(payload)),
        (0x0E, 0x01 | 0x03..=0x0E) => {
            HighPayloadValidation::Exact(party_cnw_wrapped_payload_shape_valid(payload))
        }
        (0x11, 0x01) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x11, 0x02 | 0x04) => HighPayloadValidation::Exact(char_list_shape_valid(payload)),
        (0x11, 0x03) => {
            HighPayloadValidation::Exact(char_list_request_update_char_shape_valid(payload))
        }
        (0x12, 0x0B) => HighPayloadValidation::Exact(client_side_feedback_shape_valid(payload)),
        (0x1C, 0x0C) => HighPayloadValidation::Exact(journal_shape_valid(payload)),
        (0x1E, 0x01) => HighPayloadValidation::Exact(quickbar_shape_valid(payload)),
        (0x1E, 0x02) => {
            HighPayloadValidation::Exact(client_quickbar::set_button_payload_shape_valid(payload))
        }
        (0x31, 0x01 | 0x02) => HighPayloadValidation::Exact(empty_high_level_shape_valid(payload)),
        (0x31, 0x03) => HighPayloadValidation::Exact(
            play_module_character_list::claim_payload_if_verified(payload).is_some(),
        ),
        (0x32, 0x01) => HighPayloadValidation::Exact(set_custom_token_shape_valid(payload)),
        (0x32, 0x02) => HighPayloadValidation::Exact(set_custom_token_list_shape_valid(payload)),
        (0x02, 0x0A | 0x10 | 0x12) | (0x03, 0x0E) | (0x0C, 0x01) => {
            HighPayloadValidation::shallow_noncritical(bare_or_cnw_wrapped_payload_shape_valid(
                payload,
            ))
        }
        (0x2C, 0x01..=0x03) => HighPayloadValidation::Exact(loadbar_shape_valid(payload)),
        _ => HighPayloadValidation::Missing,
    }
}

fn empty_high_level_shape_valid(payload: &[u8]) -> bool {
    payload.len() == 3
}

fn client_login_server_subdirectory_shape_valid(payload: &[u8]) -> bool {
    client_login::server_subdirectory_character_shape_valid(payload)
}

fn client_login_waypoint_response_shape_valid(payload: &[u8]) -> bool {
    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    let Some(tag_len) = read_le_u32(payload, 7).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    let expected_declared = 11usize.saturating_add(tag_len);
    declared == expected_declared
        && tag_len <= 0x20
        && declared < payload.len()
        && payload.len() == declared + 1
        && payload[declared] == 0x60
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

fn coalesced_window_shape_valid(bytes: &[u8], view: &crate::packet::m::MFrameView) -> bool {
    if view.trailing_payload_length == 0 {
        return false;
    }
    if view.declared_payload_length != 0
        && view.declared_payload_length > view.available_payload_length
    {
        return false;
    }

    let primary_end = match LEGACY_GAMEPLAY_PAYLOAD_OFFSET.checked_add(view.payload_length) {
        Some(end) if end <= bytes.len() => end,
        _ => return false,
    };
    let Some(primary_payload) = bytes.get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..primary_end) else {
        return false;
    };
    if !coalesced_payload_shape_valid(primary_payload, view.deflated.as_ref()) {
        return false;
    }

    let Some(spans) = parse_packetized_spans(bytes, primary_end) else {
        return false;
    };
    if spans.is_empty() {
        return false;
    }

    spans.into_iter().all(|span| {
        if span.declared_payload_length != span.payload_length {
            return false;
        }
        let payload_offset = span.offset + LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
        let payload_end = payload_offset + span.payload_length;
        let Some(payload) = bytes.get(payload_offset..payload_end) else {
            return false;
        };
        coalesced_payload_shape_valid(payload, span.deflated.as_ref())
    })
}

fn coalesced_payload_shape_valid(
    payload: &[u8],
    deflated: Option<&crate::packet::m::DeflatedEnvelope>,
) -> bool {
    if payload.is_empty() {
        return true;
    }
    if HighLevel::parse(payload).is_some() {
        return exact_high_payload_shape_valid(payload);
    }
    let Some(deflated) = deflated else {
        return false;
    };
    if !deflated.plausible {
        return false;
    }
    let Some(inflated) =
        inflate_verified_deflated_combined_payload(payload, Some(deflated.inflated_length))
    else {
        return false;
    };
    exact_high_payload_shape_valid(&inflated)
}

fn bare_or_cnw_wrapped_payload_shape_valid(payload: &[u8]) -> bool {
    payload.len() == 3 || cnw_wrapped_payload_shape_valid(payload, 3 + 4, 64)
}

fn module_time_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // EE `CNWSMessage::SendServerToPlayerModuleUpdate_Time` emits server
    // message 03/03 by creating a CNW read message, writing one update mask
    // byte, then conditionally writing byte/dword fields for mask bits 0x01,
    // 0x02, 0x04, 0x08, and 0x10. The 1.69 capture is byte-identical for
    // the simple time-of-day case (`mask=0x02`, one value byte), so this is a
    // verified no-op translator path rather than a permissive passthrough.
    const HEADER_AND_DECLARED_LEN: usize = 7;
    const MASK_OFFSET: usize = HEADER_AND_DECLARED_LEN;
    const MAX_OBSERVED_TRAILING_BYTES: usize = 1;

    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared <= MASK_OFFSET
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_OBSERVED_TRAILING_BYTES
    {
        return false;
    }

    let mask = payload[MASK_OFFSET];
    if mask == 0 || (mask & !0x1F) != 0 {
        return false;
    }

    let mut cursor = MASK_OFFSET + 1;
    if (mask & 0x01) != 0 {
        if cursor >= declared {
            return false;
        }
        let time_state = payload[cursor];
        cursor += 1;
        if matches!(time_state, 3 | 4) {
            cursor = match cursor.checked_add(4) {
                Some(next) if next <= declared => next,
                _ => return false,
            };
        }
    }
    for bit in [0x02, 0x04, 0x08] {
        if (mask & bit) != 0 {
            cursor = match cursor.checked_add(1) {
                Some(next) if next <= declared => next,
                _ => return false,
            };
        }
    }
    if (mask & 0x10) != 0 {
        cursor = match cursor.checked_add(4) {
            Some(next) if next <= declared => next,
            _ => return false,
        };
    }

    cursor == declared
}

fn chat_shape_valid(payload: &[u8], high: HighLevel) -> bool {
    // Decompile-backed shape:
    // EE `CNWSMessage::SendServerToPlayerChat_ServerTell` calls
    // `CreateWriteMessage(message_len + 4, ..., 1)`, writes exactly one
    // `CExoString` via `WriteCExoString(..., 0x20)`, and sends high-level
    // family 0x09/minor 0x05. Observed HG/1.69 packets carry the same declared
    // read-window with only the normal CNW fragment tail after the text.
    //
    // EE `CNWSMessage::SendServerToPlayerChat_Tell` writes object id,
    // `CExoString`, three FLOATs, then one BOOL fragment selecting the speaker
    // name branch before sending family 0x09/minor 0x04.
    //
    // EE `CNWSMessage::SendServerToPlayerChatMultiLangMessage` writes the
    // token-talk payload as two OBJECTIDs, a localized/string body, fixed
    // CResRef, BOOL, and final OBJECTID before sending family 0x09/minor 0x0B
    // or 0x0C. Strict delegates those subcases to the focused chat translator
    // so the same decompile-backed cursor proof gates direct and verified
    // frames.
    match high.minor {
        0x04 => chat_tell_shape_valid(payload),
        0x05 => chat_server_tell_shape_valid(payload),
        0x0B | 0x0C => chat::claim_payload_if_verified(payload).is_some(),
        _ => false,
    }
}

fn chat_server_tell_shape_valid(payload: &[u8]) -> bool {
    const READ_START: usize = 3 + 4;
    const STRING_LENGTH_BYTES: usize = 4;
    const MAX_CHAT_TEXT_BYTES: usize = 8192;
    const MAX_OBSERVED_FRAGMENT_BYTES: usize = 16;

    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < READ_START + STRING_LENGTH_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_OBSERVED_FRAGMENT_BYTES
    {
        return false;
    }

    let Some(text_len) =
        read_le_u32(payload, READ_START).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if text_len > MAX_CHAT_TEXT_BYTES {
        return false;
    }

    READ_START
        .checked_add(STRING_LENGTH_BYTES)
        .and_then(|text_start| text_start.checked_add(text_len))
        == Some(declared)
}

fn chat_tell_shape_valid(payload: &[u8]) -> bool {
    const READ_START: usize = 3 + 4;
    const OBJECT_ID_BYTES: usize = 4;
    const STRING_LENGTH_BYTES: usize = 4;
    const FLOAT_BYTES: usize = 4;
    const POSITION_FLOATS: usize = 3;
    const MAX_CHAT_TEXT_BYTES: usize = 8192;
    const MAX_CHAT_SPEAKER_BYTES: usize = 512;
    const REQUIRED_BOOL_FRAGMENT_BYTES: usize = 1;

    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared < READ_START + OBJECT_ID_BYTES + STRING_LENGTH_BYTES
        || declared > payload.len()
        || payload.len().saturating_sub(declared) != REQUIRED_BOOL_FRAGMENT_BYTES
        || !cnw_fragment_tail_can_hold_one_bool(&payload[declared..])
    {
        return false;
    }

    let mut cursor = READ_START;
    let Some(object_id) = read_le_u32(payload, cursor) else {
        return false;
    };
    if object_id == 0 || object_id == u32::MAX {
        return false;
    }
    cursor += OBJECT_ID_BYTES;

    let Some((message_end, _)) =
        read_bounded_cexo_string_end(payload, cursor, declared, MAX_CHAT_TEXT_BYTES)
    else {
        return false;
    };
    cursor = message_end;

    for _ in 0..POSITION_FLOATS {
        let Some(value) = read_le_f32(payload, cursor) else {
            return false;
        };
        if !value.is_finite() || value.abs() > 1_000_000.0 {
            return false;
        }
        cursor += FLOAT_BYTES;
    }

    if let Some((speaker_end, _)) =
        read_bounded_cexo_string_end(payload, cursor, declared, MAX_CHAT_SPEAKER_BYTES)
    {
        speaker_end == declared
            || read_bounded_cexo_string_end(payload, speaker_end, declared, MAX_CHAT_SPEAKER_BYTES)
                .is_some_and(|(second_end, _)| second_end == declared)
    } else {
        false
    }
}

fn read_bounded_cexo_string_end(
    payload: &[u8],
    offset: usize,
    declared: usize,
    max_bytes: usize,
) -> Option<(usize, usize)> {
    let length = usize::try_from(read_le_u32(payload, offset)?).ok()?;
    if length > max_bytes {
        return None;
    }
    let end = offset.checked_add(4)?.checked_add(length)?;
    if end > declared {
        return None;
    }
    Some((end, length))
}

fn read_le_f32(payload: &[u8], offset: usize) -> Option<f32> {
    let bytes = payload.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn cnw_fragment_tail_can_hold_one_bool(fragment: &[u8]) -> bool {
    cnw_fragment_tail_can_hold_bools(fragment, 1)
}

fn cnw_fragment_tail_can_hold_bools(fragment: &[u8], semantic_bool_count: usize) -> bool {
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
    valid_bits >= CNW_FRAGMENT_HEADER_BITS.saturating_add(semantic_bool_count)
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
    const READ_START: usize = 3 + 4;
    const MAX_MODULE_INFO_STRING: usize = 4096;
    const MAX_AREA_NAME_STRING: usize = 512;
    const MAX_AREA_COUNT: u32 = 4096;
    const RESREF_BYTES: usize = 16;
    const MAX_FRAGMENT_BYTES: usize = 64;

    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != 0x03 || high.minor != 0x01 {
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
    cursor = match cnw_string_end(payload, cursor, declared, MAX_MODULE_INFO_STRING) {
        Some(cursor) => cursor,
        None => return false,
    };
    // EE's writer uses `WriteCExoLocStringServer(..., 0)` here, and the client
    // reader consumes the common raw-string branch through the fragment-bit
    // discriminator. There is no inline marker byte in the read buffer for this
    // observed Module_Info shape, so cursor validation treats it as the bounded
    // CExoString payload the decompiled writer places in the read window.
    cursor = match cnw_string_end(payload, cursor, declared, MAX_MODULE_INFO_STRING) {
        Some(cursor) => cursor,
        None => return false,
    };

    if cursor >= declared {
        return false;
    }
    cursor += 1;

    let Some(module_resref) = payload.get(cursor..cursor + RESREF_BYTES) else {
        return false;
    };
    if !fixed_resref16_shape_valid(module_resref, true) {
        return false;
    }
    cursor += RESREF_BYTES;

    let Some(area_count) = read_le_u32(payload, cursor) else {
        return false;
    };
    if area_count > MAX_AREA_COUNT {
        return false;
    }
    cursor += 4;

    for _ in 0..area_count {
        let Some(area_id) = read_le_u32(payload, cursor) else {
            return false;
        };
        if (area_id & 0x8000_0000) == 0 || area_id == 0xffff_ffff {
            return false;
        }
        cursor += 4;
        cursor = match cnw_string_end(payload, cursor, declared, MAX_AREA_NAME_STRING) {
            Some(cursor) => cursor,
            None => return false,
        };
    }

    let Some(official_campaign) = payload.get(cursor).copied() else {
        return false;
    };
    if official_campaign > 1 {
        return false;
    }
    cursor += 1;

    cursor == declared && cnw_fragment_tail_can_hold_bools(&payload[declared..], 2)
}

fn fixed_resref16_shape_valid(bytes: &[u8], allow_empty: bool) -> bool {
    if bytes.len() != 16 {
        return false;
    }

    let mut cursor = 0;
    while cursor < bytes.len() && bytes[cursor] != 0 {
        if !matches!(bytes[cursor], b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' | b'_' | b'-') {
            return false;
        }
        cursor += 1;
    }

    allow_empty || cursor != 0
}

fn area_client_area_shape_valid(payload: &[u8]) -> bool {
    // `Area_ClientArea` is not safe to validate as a generic CNW wrapper. EE's
    // `CNWCArea::LoadArea` consumes a fixed, decompile-backed static header
    // before the tile stream; if the proxy mis-identifies width/height/tileset
    // offsets, the client can cleanly disconnect without a message overflow.
    // Delegate exact cursor proof to the semantic area translator.
    area::ee_area_client_area_payload_shape_valid(payload)
}

fn live_object_shape_valid(payload: &[u8]) -> bool {
    // A P05 CNW wrapper is not a semantic proof. HG captures can split a
    // GameObjUpdate_LiveObject stream mid record; accepting only the declared
    // length wrapper lets later no-header zlib bytes leak through as if the
    // original live-object packet was fully understood. The exact validator
    // walks decompile-backed live-object record boundaries and requires full
    // cursor consumption before this family can be considered translated.
    live_object_update::claim_payload_if_verified(payload).is_some()
}

fn player_list_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // EE `SendServerToPlayerPlayerList_Add/All` writes a platform identity
    // byte plus `CExoString` immediately after each `has_creature` bit, and
    // the EE client handler reads that field before optional creature details.
    // Strict validation therefore delegates to the focused PlayerList owner
    // instead of accepting any generic `P 0A xx` CNW wrapper.
    player_list::ee_player_list_payload_shape_valid(payload)
}

fn char_list_shape_valid(payload: &[u8]) -> bool {
    // Reuse the focused semantic owner for the exact CharList packet proof.
    // The module's parser is decompile-backed for `CharList_ListResponse`
    // (`0x11/0x02`) and `CharList_UpdateCharResponse` (`0x11/0x04`), including
    // exact cursor consumption and the EE-safe BIC canonicalization path. Strict
    // validation clones here so verification cannot mutate the packet being
    // checked.
    let mut candidate = payload.to_vec();
    char_list::claim_payload_if_verified(&mut candidate).is_some()
}

fn journal_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed no-op translator proof:
    // EE's packet-name table maps 0x1C/0x0C to `Journal_Updated`, and the
    // exported sender takes a `CExoLocString`. HG's observed login updates use
    // the same compact CNW read-window form already documented in
    // `translate::journal`, so strict delegates exact cursor validation to
    // that semantic owner instead of allowing the opcode generically.
    journal::claim_payload_if_verified(payload).is_some()
}

fn quickbar_shape_valid(payload: &[u8]) -> bool {
    quickbar::ee_set_all_buttons_payload_shape_valid(payload)
}

fn quickbar_placeholder_shape_valid(payload: &[u8]) -> bool {
    // Placeholder quickbars are a transport workaround, not a real semantic
    // quickbar update. They may only validate as the exact all-blank
    // SetAllButtons shape emitted by `m_frame::quickbar_stream` while buffering
    // a fragmented legacy quickbar stream.
    const PLACEHOLDER_READ_BYTES: usize = 36;
    const PLACEHOLDER_DECLARED: usize = 3 + PLACEHOLDER_READ_BYTES;
    const PLACEHOLDER_FRAGMENT_BYTE: u8 = 0x60;

    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != 0x1E || high.minor != 0x01 {
        return false;
    }
    let Some(declared) = read_le_u32(payload, 3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    if declared != PLACEHOLDER_DECLARED || payload.len() != 3 + 4 + PLACEHOLDER_READ_BYTES + 1 {
        return false;
    }
    let read_start = 3 + 4;
    let read_end = read_start + PLACEHOLDER_READ_BYTES;
    payload
        .get(read_start..read_end)
        .map(|read| read.iter().all(|byte| *byte == 0))
        .unwrap_or(false)
        && payload.get(read_end).copied() == Some(PLACEHOLDER_FRAGMENT_BYTE)
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

    if let Some(string_len) =
        read_le_u32(payload, tail_start).and_then(|value| usize::try_from(value).ok())
    {
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
                return StrictDecision::quarantine("BN", packet.tag.name(), "BNCR-trailing-bytes");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::{encode_legacy_m_crc, write_be_u16};
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn coalesced_login_feedback_records_have_exact_semantic_shapes() {
        let feedback_with_two_dwords = [
            0x50, 0x12, 0x0B, 0x11, 0x00, 0x00, 0x00, 0x47, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x60,
        ];
        let feedback_id_only = [0x50, 0x12, 0x0B, 0x09, 0x00, 0x00, 0x00, 0xBE, 0x00, 0x60];
        let login_confirm = [0x50, 0x02, 0x05];

        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientSideMessage,
            &feedback_with_two_dwords,
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientSideMessage,
            &feedback_id_only,
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Login,
            &login_confirm,
        ));
    }

    #[test]
    fn strict_prefers_deflated_envelope_over_accidental_p_major_minor_length() {
        // Regression for rewritten live-object zlib windows whose EE-facing
        // inflated length was 0x00000350. The first four payload bytes are the
        // little-endian length (`50 03 00 00`), which also looks like an
        // unknown high-level `P 03/00` header if strict checks HighLevel before
        // the M-frame deflate flag. The decompiled reliable-window branch is
        // flag-driven, so this must remain a deflated envelope.
        let packet = build_m_deflated_packet_with_inflated_len(0x350);

        let decision = decide(
            Direction::ServerToClient,
            &packet,
            StrictProfile::Player,
        );
        assert_eq!(decision.verdict, Verdict::Allow);
        assert_eq!(decision.family, "M/deflated");

        let verified = decide_verified_translated(
            Direction::ServerToClient,
            VerifiedFamily::SemanticDeflated,
            &packet,
        );
        assert_eq!(verified.verdict, Verdict::Quarantine);
        assert_eq!(verified.family, "M/verified-deflated");
        assert_eq!(verified.reason, "verified-deflated-missing-semantic-family");
    }

    fn build_m_deflated_packet_with_inflated_len(inflated_len: usize) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        let inflated = vec![0u8; inflated_len];
        encoder.write_all(&inflated).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut payload = Vec::with_capacity(4 + compressed.len());
        payload.extend_from_slice(&(inflated_len as u32).to_le_bytes());
        payload.extend_from_slice(&compressed);

        let mut packet = vec![b'M', 0, 0, 0, 1, 0, 0, 0x0E, 0, 1, 0, 0];
        packet.extend_from_slice(&payload);
        write_be_u16(&mut packet, 10, payload.len() as u16);
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }
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
    if bytes.len() < MODULE_NAME_OFFSET
        || bytes.get(EXTENDED_MARKER_OFFSET) != Some(&EXTENDED_MARKER)
    {
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
        return StrictDecision::allow(
            "BN",
            packet.tag.name(),
            "known-bnxr-extended-server-control",
        );
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
        return StrictDecision::quarantine(
            "BN",
            packet.tag.name(),
            "BNXR-nwsync-manifest-count-overflow",
        );
    };
    cursor += 1;
    for _ in 0..manifest_count {
        cursor = match cursor.checked_add(2) {
            Some(cursor) if cursor <= bytes.len() => cursor,
            _ => {
                return StrictDecision::quarantine(
                    "BN",
                    packet.tag.name(),
                    "BNXR-nwsync-manifest-overflow",
                );
            }
        };
        if !consume_counted(bytes, &mut cursor) {
            return StrictDecision::quarantine(
                "BN",
                packet.tag.name(),
                "BNXR-nwsync-manifest-hash-overflow",
            );
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
        .deflated
        .as_ref()
        .filter(|deflated| deflated.plausible)
        .map(|deflated| {
            format!(
                "deflated(inflated={} compressed={})",
                deflated.inflated_length, deflated.compressed_length
            )
        })
        .or_else(|| {
            view.high
                .map(|high| format!("{:02X}/{:02X} {}", high.major, high.minor, high.name()))
        })
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
