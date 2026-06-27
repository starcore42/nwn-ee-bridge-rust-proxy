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
        ContinuationOwner, VerifiedFamily, VerifiedProof, ambient, area, area_change_day_night,
        area_visual_effect, camera, char_list, chat, client_area, client_char_list,
        client_character_sheet, client_gui_event, client_gui_inventory, client_input, client_login,
        client_module, client_quickbar, client_server_admin, client_server_status,
        client_side_message, custom_token, cutscene, dialog, game_obj_update, gameplay_stream,
        gui_timing_event, inventory, journal, live_object_update, loadbar, login, module,
        module_resources, module_time, party, play_module_character_list, player_list, quickbar,
        safe_projectile, server_status, sound,
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

            if !verified_family_direction_valid(direction, family) {
                return StrictDecision::quarantine(
                    "M/verified-direction",
                    family.as_str(),
                    "verified-family-wrong-direction",
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
                if server_zlib_stream_empty_progress_valid(direction, family) {
                    return StrictDecision::allow(
                        "M/verified-empty",
                        family.as_str(),
                        "verified-server-zlib-empty-progress-frame",
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

            if family == VerifiedFamily::ClientServerAdmin {
                if !matches!(direction, Direction::ClientToServer) {
                    return StrictDecision::quarantine(
                        "M/verified-client-admin",
                        family.as_str(),
                        "client-server-admin-wrong-direction",
                    );
                }
                if view.trailing_payload_length != 0 {
                    return StrictDecision::quarantine(
                        "M/verified-client-admin",
                        family.as_str(),
                        "client-server-admin-trailing-spans-unsupported",
                    );
                }
                if view.high.is_some() || view.payload_length == 0 {
                    return StrictDecision::quarantine(
                        "M/verified-client-admin",
                        family.as_str(),
                        "client-server-admin-invalid-frame-kind",
                    );
                }
                let payload_start = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
                let payload_end = payload_start + view.payload_length;
                let Some(payload) = frame.bytes.get(payload_start..payload_end) else {
                    return StrictDecision::quarantine(
                        "M/verified-client-admin",
                        family.as_str(),
                        "client-server-admin-payload-overflow",
                    );
                };
                if client_server_admin::exact_payload_valid(payload) {
                    return StrictDecision::allow(
                        "M/verified-client-admin",
                        family.as_str(),
                        "verified-client-server-admin-exact-shape",
                    );
                }
                return StrictDecision::quarantine(
                    "M/verified-client-admin",
                    family.as_str(),
                    "client-server-admin-invalid-shape",
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

fn server_zlib_stream_empty_progress_valid(direction: Direction, family: VerifiedFamily) -> bool {
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
        VerifiedFamily::ServerZlibZeroFillWindow {
            inflated_length,
            compressed_length,
            ..
        } => inflated_length >= 8 && compressed_length != 0 && compressed_length <= 16,
        _ => false,
    }
}

fn verified_family_direction_valid(direction: Direction, family: VerifiedFamily) -> bool {
    if server_verified_family(family) {
        return matches!(
            direction,
            Direction::ServerToClient | Direction::ServerToClientSynthetic
        );
    }
    if client_verified_family(family) {
        return matches!(direction, Direction::ClientToServer);
    }
    true
}

fn server_verified_family(family: VerifiedFamily) -> bool {
    matches!(
        family,
        VerifiedFamily::Ambient
            | VerifiedFamily::AreaClientArea
            | VerifiedFamily::AreaChangeDayNight
            | VerifiedFamily::AreaVisualEffect
            | VerifiedFamily::CharList
            | VerifiedFamily::Chat
            | VerifiedFamily::Camera
            | VerifiedFamily::Cutscene
            | VerifiedFamily::ClientSideMessage
            | VerifiedFamily::Dialog
            | VerifiedFamily::GameObjUpdateObjectControl
            | VerifiedFamily::GameObjUpdateVisEffect
            | VerifiedFamily::GameObjUpdateDestroyItem
            | VerifiedFamily::GameObjUpdateLiveObject
            | VerifiedFamily::GuiTimingEvent
            | VerifiedFamily::GuiQuickbar
            | VerifiedFamily::GuiQuickbarPlaceholder
            | VerifiedFamily::Inventory
            | VerifiedFamily::Journal
            | VerifiedFamily::LoadBar
            | VerifiedFamily::Login
            | VerifiedFamily::ModuleEndGame
            | VerifiedFamily::ModuleInfo
            | VerifiedFamily::ModuleTime
            | VerifiedFamily::Party
            | VerifiedFamily::PlayModuleCharacterList
            | VerifiedFamily::PlayerList
            | VerifiedFamily::SetCustomToken
            | VerifiedFamily::ServerStatusStatus
            | VerifiedFamily::ServerStatusModuleResources
            | VerifiedFamily::SafeProjectile
            | VerifiedFamily::Sound
    )
}

fn client_verified_family(family: VerifiedFamily) -> bool {
    matches!(
        family,
        VerifiedFamily::ClientArea
            | VerifiedFamily::ClientCharList
            | VerifiedFamily::ClientCharacterSheet
            | VerifiedFamily::ClientDialog
            | VerifiedFamily::ClientGuiEvent
            | VerifiedFamily::ClientGuiInventory
            | VerifiedFamily::ClientInput
            | VerifiedFamily::ClientJournal
            | VerifiedFamily::ClientLogin
            | VerifiedFamily::ClientModule
            | VerifiedFamily::ClientParty
            | VerifiedFamily::ClientPlayModuleCharacterList
            | VerifiedFamily::ClientQuickbar
            | VerifiedFamily::ClientServerStatus
    )
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
                if verified_gameplay_stream_payload_valid(direction, families, &inflated) {
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
                if verified_gameplay_stream_payload_valid(direction, families, payload) {
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
        VerifiedProof::Family(
            family @ (VerifiedFamily::ServerZlibStreamContinuation { .. }
            | VerifiedFamily::ServerZlibZeroFillWindow { .. }),
        ) => payload.is_empty() && server_zlib_stream_empty_progress_valid(direction, *family),
        VerifiedProof::Family(family) => {
            verified_family_direction_valid(direction, *family)
                && coalesced_family_payload_valid(*family, payload, deflated)
        }
        VerifiedProof::GameplayStream(families) => {
            coalesced_gameplay_stream_payload_valid(direction, families, payload, deflated)
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
    direction: Direction,
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
        let valid = verified_gameplay_stream_payload_valid(direction, families, &inflated);
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
        .map(|_| verified_gameplay_stream_payload_valid(direction, families, payload))
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
fn verified_gameplay_stream_payload_valid(
    direction: Direction,
    families: &[VerifiedFamily],
    payload: &[u8],
) -> bool {
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
                verified_family_direction_valid(direction, *family)
                    && verified_family_inflated_payload_valid(*family, message.payload)
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
            if verified_gameplay_stream_payload_valid(direction, families, &inflated) {
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
    if family == VerifiedFamily::ClientServerAdmin {
        return client_server_admin::claim_payload_if_verified(payload).is_some();
    }

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
    if let VerifiedFamily::ServerZlibZeroFillWindow {
        first_sequence,
        inflated_length,
        compressed_length,
    } = family
    {
        tracing::warn!(
            first_sequence,
            inflated_length,
            compressed_length,
            continuation_len = payload.len(),
            "strict rejected standalone zlib zero-fill proof without empty reliable progress shell"
        );
        return false;
    }

    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };

    match family {
        VerifiedFamily::Ambient => {
            high.major == 0x28 && ambient::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::AreaClientArea => {
            high.major == 0x04 && high.minor == 0x01 && area_client_area_shape_valid(payload)
        }
        VerifiedFamily::AreaChangeDayNight => {
            high.major == 0x04
                && high.minor == 0x06
                && area_change_day_night::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::AreaVisualEffect => {
            high.major == 0x04
                && high.minor == 0x02
                && area_visual_effect::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::CharList => high.major == 0x11 && char_list_shape_valid(payload),
        VerifiedFamily::Chat => high.major == 0x09 && chat_shape_valid(payload, high),
        VerifiedFamily::Camera => {
            high.major == 0x10 && camera::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::Cutscene => {
            high.major == 0x33 && cutscene::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientArea => {
            high.major == 0x04
                && high.minor == 0x03
                && client_area::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientCharList => client_char_list_shape_valid(payload),
        VerifiedFamily::ClientCharacterSheet => {
            high.major == 0x15
                && high.minor == 0x01
                && client_character_sheet::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientDialog => high.major == 0x14 && client_dialog_shape_valid(payload),
        VerifiedFamily::ClientGuiEvent => {
            high.major == 0x35
                && high.minor == 0x01
                && client_gui_event::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientGuiInventory => {
            high.major == 0x0D && client_gui_inventory::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientInput => {
            high.major == 0x06 && client_input::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientJournal => high.major == 0x1C && client_journal_shape_valid(payload),
        VerifiedFamily::ClientLogin => {
            high.major == 0x02 && client_login::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientModule => {
            high.major == 0x03
                && high.minor == 0x02
                && client_module::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientParty => {
            high.major == 0x0E && high.minor == 0x02 && client_party_shape_valid(payload)
        }
        VerifiedFamily::ClientPlayModuleCharacterList => {
            high.major == 0x31
                && matches!(high.minor, 0x01 | 0x02)
                && play_module_character_list::claim_client_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientQuickbar => {
            high.major == 0x1E
                && high.minor == 0x02
                && client_quickbar::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientServerAdmin => false,
        VerifiedFamily::ClientServerStatus => {
            high.major == 0x01
                && high.minor == 0x00
                && client_server_status::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ClientSideMessage => {
            high.major == 0x12 && high.minor == 0x0B && client_side_message_shape_valid(payload)
        }
        VerifiedFamily::Dialog => high.major == 0x14 && server_dialog_shape_valid(payload),
        VerifiedFamily::GameObjUpdateObjectControl => {
            high.major == 0x05 && game_obj_update_shape_valid(payload, 0x02)
        }
        VerifiedFamily::GameObjUpdateVisEffect => {
            high.major == 0x05 && game_obj_update_shape_valid(payload, 0x03)
        }
        VerifiedFamily::GameObjUpdateDestroyItem => {
            high.major == 0x05 && game_obj_update_shape_valid(payload, 0x07)
        }
        VerifiedFamily::GuiTimingEvent => {
            high.major == 0x30
                && high.minor == 0x01
                && gui_timing_event::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::GameObjUpdateLiveObject => {
            high.major == 0x05 && high.minor == 0x01 && live_object_shape_valid(payload)
        }
        VerifiedFamily::GuiQuickbar => {
            high.major == 0x1E && high.minor == 0x01 && quickbar_shape_valid(payload)
        }
        VerifiedFamily::GuiQuickbarPlaceholder => quickbar_placeholder_shape_valid(payload),
        VerifiedFamily::Inventory => inventory::claim_payload_if_verified(payload).is_some(),
        VerifiedFamily::Journal => high.major == 0x1C && server_journal_shape_valid(payload),
        VerifiedFamily::LoadBar => {
            high.major == 0x2C
                && (0x01..=0x03).contains(&high.minor)
                && loadbar_shape_valid(payload)
        }
        VerifiedFamily::Login => {
            high.major == 0x02 && login::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ModuleEndGame => {
            high.major == 0x03
                && high.minor == 0x0E
                && module::claim_module_end_game_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ModuleInfo => {
            high.major == 0x03 && high.minor == 0x01 && module_info_shape_valid(payload)
        }
        VerifiedFamily::ModuleTime => {
            high.major == 0x03 && high.minor == 0x03 && module_time_shape_valid(payload)
        }
        VerifiedFamily::Party => high.major == 0x0E && server_party_shape_valid(payload),
        VerifiedFamily::PlayModuleCharacterList => {
            high.major == 0x31
                && high.minor == 0x03
                && play_module_character_list::claim_server_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::PlayerList => {
            high.major == 0x0A
                && matches!(high.minor, 0x01 | 0x02 | 0x03)
                && player_list_shape_valid(payload)
        }
        VerifiedFamily::SetCustomToken => {
            high.major == 0x32 && custom_token::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ServerStatusStatus => {
            high.major == 0x01
                && high.minor == 0x01
                && server_status::claim_status_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::ServerStatusModuleResources => {
            high.major == 0x01
                && high.minor == 0x03
                && module_resources::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::SafeProjectile => {
            high.major == 0x22
                && high.minor == 0x01
                && safe_projectile::claim_payload_if_verified(payload).is_some()
        }
        VerifiedFamily::Sound => {
            high.major == 0x17 && high.minor == 0x03 && sound_shape_valid(payload)
        }
        VerifiedFamily::CoalescedWindow
        | VerifiedFamily::ConsumedEmptyMFrame
        | VerifiedFamily::SemanticDeflated
        | VerifiedFamily::ServerZlibStreamContinuation { .. }
        | VerifiedFamily::ServerZlibZeroFillWindow { .. } => false,
    }
}

fn verified_family_allows_deflated_continuation(family: VerifiedFamily) -> bool {
    matches!(
        family,
        VerifiedFamily::AreaClientArea
            | VerifiedFamily::Ambient
            | VerifiedFamily::AreaChangeDayNight
            | VerifiedFamily::AreaVisualEffect
            | VerifiedFamily::Camera
            | VerifiedFamily::CharList
            | VerifiedFamily::Chat
            | VerifiedFamily::Cutscene
            | VerifiedFamily::ClientSideMessage
            | VerifiedFamily::Dialog
            | VerifiedFamily::GameObjUpdateObjectControl
            | VerifiedFamily::GameObjUpdateVisEffect
            | VerifiedFamily::GameObjUpdateDestroyItem
            | VerifiedFamily::GameObjUpdateLiveObject
            | VerifiedFamily::GuiTimingEvent
            | VerifiedFamily::GuiQuickbar
            | VerifiedFamily::GuiQuickbarPlaceholder
            | VerifiedFamily::Inventory
            | VerifiedFamily::Journal
            | VerifiedFamily::LoadBar
            | VerifiedFamily::Login
            | VerifiedFamily::ModuleEndGame
            | VerifiedFamily::ModuleInfo
            | VerifiedFamily::ModuleTime
            | VerifiedFamily::Party
            | VerifiedFamily::PlayModuleCharacterList
            | VerifiedFamily::PlayerList
            | VerifiedFamily::SetCustomToken
            | VerifiedFamily::SafeProjectile
            | VerifiedFamily::Sound
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
    /// A known or unknown opcode with no family validator. These packets must
    /// be quarantined until the decompiles/captures justify a translator.
    Missing,
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
        HighPayloadValidation::Missing => {
            tracing::warn!(
                major = high.major,
                minor = high.minor,
                name = high.name(),
                strict_profile = profile.as_str(),
                "strict M high-level validator missing for known opcode"
            );
            false
        }
    }
}

fn high_payload_validation(payload: &[u8], high: HighLevel) -> HighPayloadValidation {
    match (high.major, high.minor) {
        (0x01, 0x00) => HighPayloadValidation::Exact(
            client_server_status::claim_payload_if_verified(payload).is_some(),
        ),
        (0x01, 0x01) => HighPayloadValidation::Exact(
            server_status::claim_status_payload_if_verified(payload).is_some(),
        ),
        (0x01, 0x03) => HighPayloadValidation::Exact(
            module_resources::claim_payload_if_verified(payload).is_some(),
        ),
        (0x02, 0x05 | 0x0A | 0x0C | 0x10 | 0x12) => {
            HighPayloadValidation::Exact(login::claim_payload_if_verified(payload).is_some())
        }
        (0x02, 0x0D | 0x11) => {
            HighPayloadValidation::Exact(client_login::claim_payload_if_verified(payload).is_some())
        }
        (0x03, 0x01) => HighPayloadValidation::Exact(module_info_shape_valid(payload)),
        (0x03, 0x02) => HighPayloadValidation::Exact(
            client_module::claim_payload_if_verified(payload).is_some(),
        ),
        (0x03, 0x03) => HighPayloadValidation::Exact(module_time_shape_valid(payload)),
        (0x03, 0x0E) => HighPayloadValidation::Exact(module::module_end_game_shape_valid(payload)),
        (0x04, 0x01) => HighPayloadValidation::Exact(area_client_area_shape_valid(payload)),
        (0x04, 0x02) => HighPayloadValidation::Exact(
            area_visual_effect::claim_payload_if_verified(payload).is_some(),
        ),
        (0x04, 0x03) => {
            HighPayloadValidation::Exact(client_area::claim_payload_if_verified(payload).is_some())
        }
        (0x04, 0x06) => HighPayloadValidation::Exact(
            area_change_day_night::claim_payload_if_verified(payload).is_some(),
        ),
        (0x05, 0x01) => HighPayloadValidation::Exact(live_object_shape_valid(payload)),
        (0x05, 0x02 | 0x03 | 0x07) => {
            HighPayloadValidation::Exact(game_obj_update_shape_valid(payload, high.minor))
        }
        (
            0x06,
            0x01 | 0x02 | 0x03 | 0x05 | 0x06 | 0x07 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x0E
            | 0x10 | 0x11,
        ) => {
            HighPayloadValidation::Exact(client_input::claim_payload_if_verified(payload).is_some())
        }
        (0x09, 0x04 | 0x05 | 0x07 | 0x08 | 0x09 | 0x0A | 0x0B | 0x0C) => {
            HighPayloadValidation::Exact(chat_shape_valid(payload, high))
        }
        (0x0A, 0x01 | 0x02 | 0x03) => {
            HighPayloadValidation::Exact(player_list_shape_valid(payload))
        }
        (0x0D, 0x01 | 0x02) => HighPayloadValidation::Exact(
            client_gui_inventory::claim_payload_if_verified(payload).is_some(),
        ),
        (0x0C, 0x01 | 0x02) => {
            HighPayloadValidation::Exact(inventory::claim_payload_if_verified(payload).is_some())
        }
        (0x0E, 0x01..=0x0E) => HighPayloadValidation::Exact(party_shape_valid(payload)),
        (0x10, 0x01 | 0x02 | 0x03 | 0x04 | 0x05) => {
            HighPayloadValidation::Exact(camera::claim_payload_if_verified(payload).is_some())
        }
        (0x11, 0x02 | 0x04) => HighPayloadValidation::Exact(char_list_shape_valid(payload)),
        (0x11, 0x01 | 0x03) => HighPayloadValidation::Exact(client_char_list_shape_valid(payload)),
        (0x12, 0x0B) => HighPayloadValidation::Exact(client_side_message_shape_valid(payload)),
        (0x14, 0x01 | 0x02 | 0x03 | 0x04 | 0x05) => {
            HighPayloadValidation::Exact(dialog::claim_payload_if_verified(payload).is_some())
        }
        (0x17, 0x03) => HighPayloadValidation::Exact(sound_shape_valid(payload)),
        (0x1C, 0x01..=0x05 | 0x07 | 0x08 | 0x0A | 0x0B | 0x0C) => {
            HighPayloadValidation::Exact(journal_shape_valid(payload))
        }
        (0x1E, 0x01) => HighPayloadValidation::Exact(quickbar_shape_valid(payload)),
        (0x1E, 0x02) => HighPayloadValidation::Exact(
            client_quickbar::claim_payload_if_verified(payload).is_some(),
        ),
        (0x22, 0x01) => HighPayloadValidation::Exact(
            safe_projectile::claim_payload_if_verified(payload).is_some(),
        ),
        (0x28, 0x01..=0x08) => {
            HighPayloadValidation::Exact(ambient::claim_payload_if_verified(payload).is_some())
        }
        (0x31, 0x01 | 0x02 | 0x03) => HighPayloadValidation::Exact(
            play_module_character_list::claim_payload_if_verified(payload).is_some(),
        ),
        (0x32, 0x01 | 0x02) => {
            HighPayloadValidation::Exact(custom_token::claim_payload_if_verified(payload).is_some())
        }
        (0x33, 0x01 | 0x03 | 0x04 | 0x05 | 0x06 | 0x07) => {
            HighPayloadValidation::Exact(cutscene::claim_payload_if_verified(payload).is_some())
        }
        (0x2C, 0x01..=0x03) => HighPayloadValidation::Exact(loadbar_shape_valid(payload)),
        _ => HighPayloadValidation::Missing,
    }
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

fn module_time_shape_valid(payload: &[u8]) -> bool {
    // Strict validation must share the focused Module_Time owner. The packet
    // has a mask-driven read-buffer body and consumes no CNW BOOLs, so local
    // copies of the cursor walk can drift on zero masks or fragment-tail bits.
    module_time::claim_payload_if_verified(payload).is_some()
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
        0x04 | 0x05 | 0x07 | 0x08 | 0x09 | 0x0A | 0x0B | 0x0C => {
            chat::claim_payload_if_verified(payload).is_some()
        }
        _ => false,
    }
}

pub(crate) fn module_info_shape_valid(payload: &[u8]) -> bool {
    // Strict validation must share the focused Module_Info owner. The packet
    // carries the locstring branch selector plus EE tail bits in the CNW
    // fragment cursor, so a second local parser can drift on tail ownership.
    module::claim_module_info_payload_if_verified(payload).is_some()
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

fn game_obj_update_shape_valid(payload: &[u8], expected_minor: u8) -> bool {
    game_obj_update::claim_payload_if_verified(payload)
        .map(|summary| summary.minor == expected_minor)
        .unwrap_or(false)
}

fn player_list_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // EE `SendServerToPlayerPlayerList_Add/All` writes a platform identity
    // byte plus `CExoString` immediately after each `has_creature` bit, and
    // the EE client handler reads that field before optional creature details.
    // Strict validation therefore delegates to the focused PlayerList claim
    // instead of accepting any generic `P 0A xx` CNW wrapper or maintaining a
    // second object-id/fragment cursor parser here.
    player_list::claim_payload_if_verified(payload).is_some()
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

fn client_char_list_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed client CharList validation lives in the focused
    // translator. Reuse it here so strict mode cannot keep an older, looser
    // view of the optional `GetWriteMessage` empty cursor byte.
    client_char_list::claim_payload_if_verified(payload).is_some()
}

fn journal_shape_valid(payload: &[u8]) -> bool {
    server_journal_shape_valid(payload) || client_journal_shape_valid(payload)
}

fn server_journal_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed no-op translator proof:
    // EE's packet-name table maps 0x1C/0x0C to `Journal_Updated`, and the
    // exported sender takes a `CExoLocString`. HG's observed login updates use
    // the same compact CNW read-window form already documented in
    // `translate::journal`, so strict delegates exact cursor validation to
    // that semantic owner instead of allowing the opcode generically.
    journal::claim_payload_if_verified(payload).is_some()
}

fn client_journal_shape_valid(payload: &[u8]) -> bool {
    // Client quest-screen open/closed rows are exact no-body high-level
    // packets. Keep them separate from server Journal proofs so a
    // server-dispatch claim cannot also validate client-originated traffic.
    journal::claim_client_payload_if_verified(payload).is_some()
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
    // Strict validation must share the typed LoadBar owner. `LoadBar_End`
    // owns four result bits in the CNW fragment stream; accepting a generic
    // declared window would let shifted fragment tails pass as UI progress.
    loadbar::claim_payload_if_verified(payload).is_some()
}

fn sound_shape_valid(payload: &[u8]) -> bool {
    sound::claim_payload_if_verified(payload).is_some()
}

fn client_side_message_shape_valid(payload: &[u8]) -> bool {
    // Decompile-backed shape:
    // `CNWSCreature::SendFeedbackMessage` stores the feedback id in
    // `CNWCCMessageData` slot 9, then calls
    // `CNWSMessage::SendServerToPlayerCCMessage(..., 0x0B, ...)`.
    // The CC-message case 11 creates a bounded 0x80-byte write message,
    // always writes that slot-9 value as a 16-bit WORD first, and then writes
    // a small set of optional fields selected by the feedback id. For feedback
    // id `0xCC`, it calls `WriteCExoString(..., 0x20)`, whose decompile writes
    // a direct DWORD length followed by the text bytes. The strict gate
    // therefore validates through the focused semantic owner instead of
    // carrying a second local copy of the feedback cursor rules in strict mode.
    client_side_message::claim_payload_if_verified(payload).is_some()
}

fn server_dialog_shape_valid(payload: &[u8]) -> bool {
    // `VerifiedFamily::Dialog` is emitted by the server dispatch table, so its
    // proof must not also admit client `Dialog_Reply` traffic. Directionless
    // known-opcode validation still uses the broader dialog owner below.
    dialog::claim_server_payload_if_verified(payload).is_some()
}

fn client_dialog_shape_valid(payload: &[u8]) -> bool {
    // Client high-level routing emits this proof for `Dialog_Reply`. Keep it
    // separate from the server Dialog proof so translated client replies do
    // not depend on the directionless known-opcode fallback.
    dialog::claim_client_payload_if_verified(payload).is_some()
}

fn party_shape_valid(payload: &[u8]) -> bool {
    // Reuse the focused party owner instead of accepting a generic CNW wrapper.
    // `P/0E/01 Party_List` has a decompile-backed member count followed by one
    // OBJECTIDServer per row; strict mode must not admit a wrapper whose count
    // and body length disagree.
    party::claim_payload_if_verified(payload).is_some()
}

fn server_party_shape_valid(payload: &[u8]) -> bool {
    // `VerifiedFamily::Party` is emitted by server dispatch. Keep it scoped to
    // server-owned party rows so it cannot prove the client no-body get-list
    // signal just because directionless known-opcode validation can parse it.
    party::claim_server_payload_if_verified(payload).is_some()
}

fn client_party_shape_valid(payload: &[u8]) -> bool {
    // Client high-level routing emits this proof only for `Party_GetList`.
    party::claim_client_payload_if_verified(payload).is_some()
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
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bncr) => {
            validate_bncr(packet)
        }
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bnvr) => {
            validate_bnvr(packet)
        }
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bnds) => {
            StrictDecision::quarantine(
                "BN",
                packet.tag.name(),
                "legacy-server-BNDS-has-no-EE-client-translator",
            )
        }
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bndr) => {
            validate_bndr(packet)
        }
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bnxr) => {
            validate_bnxr(packet)
        }
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bndp) => {
            validate_bndp(packet)
        }
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bner) => {
            validate_bner(packet)
        }
        (Direction::ServerToClient | Direction::ServerToClientSynthetic, BnTag::Bnlr) => {
            require_len(
                packet,
                11,
                "known-ee-server-latency-response",
                "decompile HandleBNLRMessage",
            )
        }
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
    // Diamond `BNVR` reject is exactly six bytes (`BNVR`, `R`, reason). EE's
    // `HandleBNVRMessage` accepts the legacy nine-byte accept, but only the
    // 21-byte accept carries the server major/minor/revision fields that feed
    // `CNetLayer::ServerSatisfiesBuild`. The EE-facing proxy therefore emits
    // an exact extended accept carrying the proxy-owned server dialect build
    // after the BNVR semantic translator has claimed it.
    let bytes = packet.bytes;
    if bytes.len() < 6 {
        return StrictDecision::quarantine("BN", packet.tag.name(), "BNVR-too-short");
    }
    match bytes[4] {
        b'R' if bytes.len() == 6 => {
            StrictDecision::allow("BN", packet.tag.name(), "known-legacy-BNVR-reject")
        }
        b'A' if bytes.len() == 21 => {
            StrictDecision::allow("BN", packet.tag.name(), "known-ee-BNVR-accept-with-build")
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
    fn strict_bare_client_signals_use_focused_owners() {
        let cases = [
            (VerifiedFamily::ClientServerStatus, [0x50, 0x01, 0x00]),
            (VerifiedFamily::ClientModule, [0x50, 0x03, 0x02]),
            (VerifiedFamily::ClientArea, [0x50, 0x04, 0x03]),
        ];

        for (family, exact) in cases {
            assert!(bare_client_signal_claimed(family, &exact));
            assert!(verified_family_inflated_payload_valid(family, &exact));
            assert!(exact_high_payload_shape_valid(&exact));

            let mut body_bearing = exact.to_vec();
            body_bearing.push(0x60);

            assert!(!bare_client_signal_claimed(family, &body_bearing));
            assert!(
                !verified_family_inflated_payload_valid(family, &body_bearing),
                "{family:?} must not accept generic non-empty client signal bodies"
            );
            assert!(
                !exact_high_payload_shape_valid(&body_bearing),
                "known-high validation must share {family:?}'s focused bare-signal owner"
            );
        }

        fn bare_client_signal_claimed(family: VerifiedFamily, payload: &[u8]) -> bool {
            match family {
                VerifiedFamily::ClientServerStatus => {
                    client_server_status::claim_payload_if_verified(payload).is_some()
                }
                VerifiedFamily::ClientModule => {
                    client_module::claim_payload_if_verified(payload).is_some()
                }
                VerifiedFamily::ClientArea => {
                    client_area::claim_payload_if_verified(payload).is_some()
                }
                _ => false,
            }
        }
    }

    #[test]
    fn strict_client_login_uses_focused_owner() {
        let server_subdirectory =
            build_client_login_server_subdirectory_character(b"febrieltestxilo");
        assert!(
            client_login::claim_payload_if_verified(&server_subdirectory).is_some(),
            "focused ClientLogin owner accepts the exact server-subdirectory character request"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientLogin,
            &server_subdirectory,
        ));
        assert!(exact_high_payload_shape_valid(&server_subdirectory));

        let waypoint = build_client_login_waypoint_response(b"");
        assert!(
            client_login::claim_payload_if_verified(&waypoint).is_some(),
            "focused ClientLogin owner accepts the empty waypoint response branch"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientLogin,
            &waypoint,
        ));
        assert!(exact_high_payload_shape_valid(&waypoint));

        let mut trailing = server_subdirectory;
        trailing.push(0);
        assert!(
            client_login::claim_payload_if_verified(&trailing).is_none(),
            "ClientLogin owns no bytes after the final empty fragment cursor"
        );
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientLogin,
            &trailing,
        ));
        assert!(
            !exact_high_payload_shape_valid(&trailing),
            "known-high validation must share the focused ClientLogin owner"
        );
    }

    #[test]
    fn strict_login_verified_proofs_are_server_owned() {
        let login_confirm = [0x50, 0x02, 0x05];
        assert!(login::claim_payload_if_verified(&login_confirm).is_some());
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Login,
            &login_confirm,
        ));
        assert!(
            exact_high_payload_shape_valid(&login_confirm),
            "directionless known-opcode validation still recognizes exact Login payloads"
        );
        assert!(verified_gameplay_stream_payload_valid(
            Direction::ServerToClient,
            &[VerifiedFamily::Login],
            &login_confirm,
        ));
        assert!(!verified_gameplay_stream_payload_valid(
            Direction::ClientToServer,
            &[VerifiedFamily::Login],
            &login_confirm,
        ));

        let frame = build_client_raw_m_frame(0x004E, 0x000B, &login_confirm, &[]);
        for direction in [
            Direction::ServerToClient,
            Direction::ServerToClientSynthetic,
        ] {
            let decision = decide_verified_translated(direction, VerifiedFamily::Login, &frame);
            assert!(
                decision.allowed(),
                "{direction:?} should allow Login: {decision:?}"
            );
        }

        let client_decision =
            decide_verified_translated(Direction::ClientToServer, VerifiedFamily::Login, &frame);
        assert_eq!(client_decision.verdict, Verdict::Quarantine);
        assert_eq!(client_decision.family, "M/verified-direction");
        assert_eq!(client_decision.reason, "verified-family-wrong-direction");

        let coalesced_client_decision = decide_verified_coalesced_window_translated(
            Direction::ClientToServer,
            &[VerifiedProof::family(VerifiedFamily::Login)],
            &frame,
        );
        assert_eq!(coalesced_client_decision.verdict, Verdict::Quarantine);
        assert_eq!(
            coalesced_client_decision.reason,
            "coalesced-record-proof-invalid"
        );
    }

    #[test]
    fn strict_client_side_message_uses_focused_feedback_owner() {
        let text = b"abcdefghijklmnop";
        let declared = 3 + 4 + 2 + 4 + text.len();
        let mut exact_feedback = vec![0x50, 0x12, 0x0B];
        exact_feedback.extend_from_slice(&(declared as u32).to_le_bytes());
        exact_feedback.extend_from_slice(&0x00CCu16.to_le_bytes());
        exact_feedback.extend_from_slice(&(text.len() as u32).to_le_bytes());
        exact_feedback.extend_from_slice(text);
        exact_feedback.push(0x60);

        assert!(client_side_message::claim_payload_if_verified(&exact_feedback).is_some());
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientSideMessage,
            &exact_feedback,
        ));
        assert!(exact_high_payload_shape_valid(&exact_feedback));

        let mut legacy_feedback = vec![0x50, 0x12, 0x0B];
        legacy_feedback.extend_from_slice(&0x94u32.to_le_bytes());
        legacy_feedback.extend_from_slice(&0x00CCu16.to_le_bytes());
        legacy_feedback.extend_from_slice(&(text.len() as u32).to_le_bytes());
        legacy_feedback.extend_from_slice(text);
        legacy_feedback.push(0x60);

        assert!(client_side_message::claim_payload_if_verified(&legacy_feedback).is_none());
        assert!(
            !verified_family_inflated_payload_valid(
                VerifiedFamily::ClientSideMessage,
                &legacy_feedback,
            ),
            "strict validation must not accept a legacy feedback preamble before the semantic owner rewrites it"
        );
        assert!(
            !exact_high_payload_shape_valid(&legacy_feedback),
            "known-high validation must share the focused ClientSideMessage owner"
        );

        let mut rewritten_feedback = legacy_feedback;
        assert!(
            client_side_message::claim_or_rewrite_payload_if_verified(&mut rewritten_feedback)
                .is_some()
        );
        assert!(client_side_message::claim_payload_if_verified(&rewritten_feedback).is_some());
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientSideMessage,
            &rewritten_feedback,
        ));
    }

    #[test]
    fn strict_client_server_admin_uses_focused_raw_owner() {
        let exact = b"sModule.Run";
        assert!(client_server_admin::claim_payload_if_verified(exact).is_some());
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientServerAdmin,
            exact,
        ));

        let mut malformed = exact.to_vec();
        malformed.push(0);
        assert!(client_server_admin::claim_payload_if_verified(&malformed).is_none());
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientServerAdmin,
            &malformed,
        ));

        let frame = build_client_raw_m_frame(0x004B, 0x0009, exact, &[]);
        let decision = decide_verified_translated(
            Direction::ClientToServer,
            VerifiedFamily::ClientServerAdmin,
            &frame,
        );
        assert_eq!(decision.verdict, Verdict::Allow);
        assert_eq!(decision.family, "M/verified-client-admin");
        assert_eq!(decision.reason, "verified-client-server-admin-exact-shape");

        let wrong_direction = decide_verified_translated(
            Direction::ServerToClient,
            VerifiedFamily::ClientServerAdmin,
            &frame,
        );
        assert_eq!(wrong_direction.verdict, Verdict::Quarantine);
        assert_eq!(
            wrong_direction.reason,
            "client-server-admin-wrong-direction"
        );

        let trailing = build_client_raw_m_frame(0x004C, 0x000A, exact, &[0]);
        let trailing_decision = decide_verified_translated(
            Direction::ClientToServer,
            VerifiedFamily::ClientServerAdmin,
            &trailing,
        );
        assert_eq!(trailing_decision.verdict, Verdict::Quarantine);
        assert_eq!(
            trailing_decision.reason,
            "client-server-admin-trailing-spans-unsupported"
        );

        let malformed_frame = build_client_raw_m_frame(0x004D, 0x000B, &malformed, &[]);
        let malformed_decision = decide_verified_translated(
            Direction::ClientToServer,
            VerifiedFamily::ClientServerAdmin,
            &malformed_frame,
        );
        assert_eq!(malformed_decision.verdict, Verdict::Quarantine);
        assert_eq!(
            malformed_decision.reason,
            "client-server-admin-invalid-shape"
        );
    }

    #[test]
    fn server_zlib_continuation_rejects_client_originated_families() {
        let continuation = build_m_opaque_continuation_frame(&[0x99, 0x88, 0x77]);

        for family in [
            VerifiedFamily::ClientCharacterSheet,
            VerifiedFamily::ClientGuiEvent,
        ] {
            let decision =
                decide_verified_translated(Direction::ServerToClient, family, &continuation);
            assert_eq!(
                decision.verdict,
                Verdict::Quarantine,
                "{family:?} must not own a server zlib continuation: {decision:?}"
            );
            assert_eq!(decision.family, "M/verified-direction");
            assert_eq!(decision.reason, "verified-family-wrong-direction");
        }

        let feedback = decide_verified_translated(
            Direction::ServerToClient,
            VerifiedFamily::ClientSideMessage,
            &continuation,
        );
        assert_eq!(feedback.verdict, Verdict::Allow);
        assert_eq!(feedback.family, "M/verified-deflated-continuation");
        assert_eq!(
            feedback.reason,
            "verified-family-deflated-continuation-frame"
        );
    }

    #[test]
    fn strict_replay_exercised_server_proofs_are_server_owned() {
        let custom_token = build_custom_token_set(0x1234, b"server-only");
        assert_server_owned_verified_family(
            VerifiedFamily::SetCustomToken,
            &custom_token,
            "SetCustomToken",
            true,
        );

        let loadbar_start = loadbar::start_payload(2);
        assert_server_owned_verified_family(
            VerifiedFamily::LoadBar,
            &loadbar_start,
            "LoadBar",
            true,
        );

        let status = server_status::status_payload();
        assert_server_owned_verified_family(
            VerifiedFamily::ServerStatusStatus,
            &status,
            "ServerStatus_Status",
            true,
        );
    }

    fn assert_server_owned_verified_family(
        family: VerifiedFamily,
        payload: &[u8],
        expected_name: &'static str,
        check_gameplay_stream: bool,
    ) {
        assert!(
            verified_family_inflated_payload_valid(family, payload),
            "{family:?} test payload should be owned by its focused parser"
        );
        assert!(
            exact_high_payload_shape_valid(payload),
            "directionless known-opcode validation still recognizes exact {family:?} payloads"
        );
        if check_gameplay_stream {
            assert!(
                verified_gameplay_stream_payload_valid(
                    Direction::ServerToClient,
                    &[family],
                    payload
                ),
                "server gameplay stream proof should accept {family:?}"
            );
            assert!(
                !verified_gameplay_stream_payload_valid(
                    Direction::ClientToServer,
                    &[family],
                    payload
                ),
                "client gameplay stream proof must not accept server-owned {family:?}"
            );
        }

        let frame = build_client_raw_m_frame(0x004E, 0x000B, payload, &[]);
        for direction in [
            Direction::ServerToClient,
            Direction::ServerToClientSynthetic,
        ] {
            let decision = decide_verified_translated(direction, family, &frame);
            assert!(
                decision.allowed(),
                "{direction:?} should allow {family:?}: {decision:?}"
            );
            assert_eq!(decision.name, expected_name);
        }

        let client_decision = decide_verified_translated(Direction::ClientToServer, family, &frame);
        assert_eq!(client_decision.verdict, Verdict::Quarantine);
        assert_eq!(client_decision.family, "M/verified-direction");
        assert_eq!(client_decision.reason, "verified-family-wrong-direction");

        let coalesced_client_decision = decide_verified_coalesced_window_translated(
            Direction::ClientToServer,
            &[VerifiedProof::family(family)],
            &frame,
        );
        assert_eq!(coalesced_client_decision.verdict, Verdict::Quarantine);
        assert_eq!(
            coalesced_client_decision.reason,
            "coalesced-record-proof-invalid"
        );
    }

    #[test]
    fn strict_chat_verified_proofs_are_server_owned() {
        let text = b"Bridge ready";
        let declared = 3 + 4 + 4 + text.len();
        let mut chat_server_tell = vec![0x50, 0x09, 0x05];
        chat_server_tell.extend_from_slice(&(declared as u32).to_le_bytes());
        chat_server_tell.extend_from_slice(&(text.len() as u32).to_le_bytes());
        chat_server_tell.extend_from_slice(text);
        chat_server_tell.push(0x60);

        assert!(chat::claim_payload_if_verified(&chat_server_tell).is_some());
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Chat,
            &chat_server_tell,
        ));
        assert!(verified_gameplay_stream_payload_valid(
            Direction::ServerToClient,
            &[VerifiedFamily::Chat],
            &chat_server_tell,
        ));
        assert!(!verified_gameplay_stream_payload_valid(
            Direction::ClientToServer,
            &[VerifiedFamily::Chat],
            &chat_server_tell,
        ));

        let frame = build_client_raw_m_frame(0x004E, 0x000B, &chat_server_tell, &[]);
        for direction in [
            Direction::ServerToClient,
            Direction::ServerToClientSynthetic,
        ] {
            let decision = decide_verified_translated(direction, VerifiedFamily::Chat, &frame);
            assert!(
                decision.allowed(),
                "{direction:?} should allow Chat: {decision:?}"
            );
        }

        let client_decision =
            decide_verified_translated(Direction::ClientToServer, VerifiedFamily::Chat, &frame);
        assert_eq!(client_decision.verdict, Verdict::Quarantine);
        assert_eq!(client_decision.family, "M/verified-direction");
        assert_eq!(client_decision.reason, "verified-family-wrong-direction");

        let coalesced_client_decision = decide_verified_coalesced_window_translated(
            Direction::ClientToServer,
            &[VerifiedProof::family(VerifiedFamily::Chat)],
            &frame,
        );
        assert_eq!(coalesced_client_decision.verdict, Verdict::Quarantine);
        assert_eq!(
            coalesced_client_decision.reason,
            "coalesced-record-proof-invalid"
        );
    }

    #[test]
    fn coalesced_chat_strref_and_ai_sound_records_revalidate_exact_proofs() {
        let chat_talk_ref = [
            0x50, 0x09, 0x08, 0x0F, 0x00, 0x00, 0x00, 0x31, 0x12, 0x00, 0x80, 0xEC, 0x47, 0x01,
            0x00, 0x62,
        ];
        let chat_ai_action_play_sound = [
            0x50, 0x09, 0x07, 0x1D, 0x00, 0x00, 0x00, 0x31, 0x12, 0x00, 0x80, 0x0E, 0x00, 0x00,
            0x00, b'v', b's', b'_', b'n', b'x', b'2', b'm', b'a', b't', b'r', b'f', b'_', b'5',
            b'0', 0x62,
        ];

        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Chat,
            &chat_talk_ref,
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Chat,
            &chat_ai_action_play_sound,
        ));

        let mut frame = vec![
            b'M', 0x00, 0x00, 0x00, 0x3C, 0x00, 0x4D, 0x0A, 0x00, 0x01, 0x00, 0x10,
        ];
        frame.extend_from_slice(&chat_talk_ref);
        frame.extend_from_slice(&[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x01, 0x00, 0x1E,
        ]);
        frame.extend_from_slice(&chat_ai_action_play_sound);
        assert!(encode_legacy_m_crc(&mut frame));

        let decision = decide_verified_coalesced_window_translated(
            Direction::ServerToClient,
            &[
                VerifiedProof::family(VerifiedFamily::Chat),
                VerifiedProof::family(VerifiedFamily::Chat),
            ],
            &frame,
        );

        assert!(decision.allowed(), "{decision:?}");
    }

    #[test]
    fn strict_dialog_splits_server_and_client_verified_owners() {
        let server_close = [0x50, 0x14, 0x05];
        assert!(dialog::claim_server_payload_if_verified(&server_close).is_some());
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Dialog,
            &server_close,
        ));
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientDialog,
            &server_close,
        ));
        assert!(exact_high_payload_shape_valid(&server_close));

        let client_reply = build_client_dialog_reply_payload();
        assert!(dialog::claim_client_payload_if_verified(&client_reply).is_some());
        assert!(dialog::claim_server_payload_if_verified(&client_reply).is_none());
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::Dialog, &client_reply),
            "server Dialog proofs must not also admit client Dialog_Reply payloads"
        );
        assert!(
            verified_family_inflated_payload_valid(VerifiedFamily::ClientDialog, &client_reply),
            "client Dialog_Reply payloads must validate through the client Dialog proof"
        );
        assert!(
            exact_high_payload_shape_valid(&client_reply),
            "directionless known-opcode validation may still recognize exact client dialog payloads"
        );

        let mut trailing_server_close = server_close.to_vec();
        trailing_server_close.push(0);
        assert!(dialog::claim_server_payload_if_verified(&trailing_server_close).is_none());
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::Dialog,
            &trailing_server_close,
        ));
        assert!(!exact_high_payload_shape_valid(&trailing_server_close));

        let mut trailing_client_reply = client_reply;
        trailing_client_reply.push(0);
        assert!(dialog::claim_client_payload_if_verified(&trailing_client_reply).is_none());
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientDialog,
            &trailing_client_reply,
        ));
    }

    #[test]
    fn verified_cutscene_accepts_only_claimed_shapes() {
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Cutscene,
            &[0x50, 0x33, 0x06],
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Cutscene,
            &[
                0x50, 0x33, 0x04, 0x0B, 0x00, 0x00, 0x00, 0x0A, 0xD7, 0x23, 0x3C, 0x78,
            ],
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Cutscene,
            &[0x50, 0x33, 0x01, 0x07, 0x00, 0x00, 0x00, 0xB0],
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Cutscene,
            &[0x50, 0x33, 0x07, 0x07, 0x00, 0x00, 0x00, 0x98],
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Cutscene,
            &[0x50, 0x33, 0x05],
        ));
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::Cutscene,
            &[0x50, 0x33, 0x02],
        ));
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::Cutscene,
            &[
                0x50, 0x33, 0x04, 0x0A, 0x00, 0x00, 0x00, 0x0A, 0xD7, 0x23, 0x3C, 0x60
            ],
        ));
    }

    #[test]
    fn strict_game_obj_update_uses_shared_family_owner_for_sibling_minors() {
        let obj_control = [
            0x50, 0x05, 0x02, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0xFF,
            0xFF, 0x73,
        ];
        let vis_effect = [
            0x50, 0x05, 0x03, 0x19, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x80, 0x25, 0x00, 0xD8,
            0x49, 0x6F, 0x41, 0x46, 0x1E, 0xC0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x61,
        ];
        let destroy_item = [
            0x50, 0x05, 0x07, 0x0B, 0x00, 0x00, 0x00, 0x67, 0x2C, 0x00, 0x80, 0x7C,
        ];
        let cases: [(VerifiedFamily, &[u8], u8); 3] = [
            (
                VerifiedFamily::GameObjUpdateObjectControl,
                obj_control.as_slice(),
                0x02,
            ),
            (
                VerifiedFamily::GameObjUpdateVisEffect,
                vis_effect.as_slice(),
                0x03,
            ),
            (
                VerifiedFamily::GameObjUpdateDestroyItem,
                destroy_item.as_slice(),
                0x07,
            ),
        ];

        for (family, payload, expected_minor) in cases {
            let summary = game_obj_update::claim_payload_if_verified(payload)
                .expect("shared GameObjUpdate owner should claim exact sibling payload");
            assert_eq!(summary.minor, expected_minor);
            assert!(verified_family_inflated_payload_valid(family, payload));
            assert!(exact_high_payload_shape_valid(payload));

            let mut trailing = payload.to_vec();
            trailing.push(0);
            assert!(game_obj_update::claim_payload_if_verified(&trailing).is_none());
            assert!(
                !verified_family_inflated_payload_valid(family, &trailing),
                "verified {family:?} must reject bytes outside the focused owner"
            );
            assert!(
                !exact_high_payload_shape_valid(&trailing),
                "known-high validation must share the focused GameObjUpdate owner"
            );
        }

        assert!(
            !verified_family_inflated_payload_valid(
                VerifiedFamily::GameObjUpdateObjectControl,
                &vis_effect
            ),
            "family-specific strict proof must still check the shared owner's minor"
        );
        assert!(
            !verified_family_inflated_payload_valid(
                VerifiedFamily::GameObjUpdateVisEffect,
                &destroy_item
            ),
            "family-specific strict proof must not accept a sibling GameObjUpdate minor"
        );
    }

    #[test]
    fn strict_player_list_uses_focused_owner() {
        let exact_delete = build_player_list_delete_payload(0x8000_0001, 0x80);
        let claim = player_list::claim_payload_if_verified(&exact_delete)
            .expect("focused PlayerList owner should claim exact delete payload");

        assert!(claim.object_ids.is_empty());
        assert!(player_list_shape_valid(&exact_delete));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::PlayerList,
            &exact_delete
        ));
        assert!(exact_high_payload_shape_valid(&exact_delete));

        let shifted_fragment_cursor = build_player_list_delete_payload(0x8000_0001, 0xA0);
        assert!(
            player_list::claim_payload_if_verified(&shifted_fragment_cursor).is_none(),
            "PlayerList_Delete owns exactly the fragment header plus module-PVP BOOL"
        );
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::PlayerList,
            &shifted_fragment_cursor
        ));
        assert!(!exact_high_payload_shape_valid(&shifted_fragment_cursor));

        let mut trailing = exact_delete;
        trailing.push(0);
        assert!(player_list::claim_payload_if_verified(&trailing).is_none());
        assert!(
            !exact_high_payload_shape_valid(&trailing),
            "known-high validation must reject unowned PlayerList tail slack"
        );
    }

    #[test]
    fn verified_projectile_and_vis_effect_accept_only_claimed_shapes() {
        let vis_effect = [
            0x50, 0x05, 0x03, 0x19, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x80, 0x25, 0x00, 0xD8,
            0x49, 0x6F, 0x41, 0x46, 0x1E, 0xC0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x61,
        ];
        let projectile = [
            0x50, 0x22, 0x01, 0x30, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x80, 0x34, 0x12, 0x00,
            0x80, 0xD8, 0x49, 0x6F, 0x41, 0x46, 0x1E, 0xC0, 0x41, 0x00, 0x00, 0x00, 0x00, 0xD8,
            0x49, 0x6F, 0x41, 0x46, 0x1E, 0xC0, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x06, 0x39, 0x00, 0x00, 0x00, 0x61,
        ];

        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::GameObjUpdateVisEffect,
            &vis_effect,
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::SafeProjectile,
            &projectile,
        ));

        let mut stale_vis_effect = vis_effect;
        stale_vis_effect[3] = 0x18;
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::GameObjUpdateVisEffect,
            &stale_vis_effect,
        ));

        let mut stale_projectile = projectile;
        stale_projectile[3] = 0x2F;
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::SafeProjectile,
            &stale_projectile,
        ));
    }

    #[test]
    fn verified_client_input_accepts_exact_parser_owned_minors() {
        let payloads = vec![
            ("attack", build_client_input_object_only(0x02)),
            ("targeted use-feat", build_client_input_use_feat_target()),
            ("location use-feat", build_client_input_use_feat_location()),
            ("use-skill", build_client_input_use_skill()),
            ("toggle-mode", build_client_input_toggle_mode(None)),
            (
                "counterspell toggle-mode",
                build_client_input_toggle_mode(Some(0x8000_34D2)),
            ),
            ("unlock-object", build_client_input_object_only(0x0C)),
            ("rest", build_client_input_high_level_only(0x0D)),
            ("lock-object", build_client_input_object_only(0x0E)),
            ("memorize-spell", build_client_input_memorize_spell()),
            ("unmemorize-spell", build_client_input_unmemorize_spell()),
        ];

        for (name, payload) in payloads {
            assert!(
                verified_family_inflated_payload_valid(VerifiedFamily::ClientInput, &payload),
                "{name} should be accepted through exact client-input parser ownership"
            );
        }
    }

    #[test]
    fn verified_client_input_rejects_unclaimed_minors() {
        for minor in [0x04, 0x08, 0x0F, 0x12] {
            let payload = build_client_input_empty(minor);
            assert!(
                !verified_family_inflated_payload_valid(VerifiedFamily::ClientInput, &payload),
                "client input minor {minor:#04x} should not be admitted without an exact parser claim"
            );
        }
    }

    #[test]
    fn strict_journal_splits_server_and_client_verified_owners() {
        let server_delete_world = vec![
            0x70, 0x1C, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x60,
        ];
        assert!(journal::claim_payload_if_verified(&server_delete_world).is_some());
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Journal,
            &server_delete_world
        ));
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientJournal,
            &server_delete_world
        ));

        for minor in [0x0A, 0x0B] {
            let payload = vec![0x70, 0x1C, minor];
            assert!(
                journal::claim_client_payload_if_verified(&payload).is_some(),
                "focused client Journal owner should claim quest-screen minor {minor:#04x}"
            );
            assert!(
                verified_family_inflated_payload_valid(VerifiedFamily::ClientJournal, &payload),
                "client Journal quest-screen minor {minor:#04x} should be accepted through its exact parser ownership"
            );
            assert!(
                !verified_family_inflated_payload_valid(VerifiedFamily::Journal, &payload),
                "server Journal proofs must not also admit client quest-screen minor {minor:#04x}"
            );

            let mut trailing = payload;
            trailing.push(0);
            assert!(
                !verified_family_inflated_payload_valid(VerifiedFamily::ClientJournal, &trailing),
                "journal quest-screen minor {minor:#04x} should reject trailing bytes"
            );
        }
    }

    #[test]
    fn strict_known_high_journal_uses_focused_client_owner() {
        for minor in [0x0A, 0x0B] {
            let payload = vec![0x70, 0x1C, minor];
            assert!(
                journal::claim_client_payload_if_verified(&payload).is_some(),
                "focused client Journal owner should claim quest-screen minor {minor:#04x}"
            );
            assert!(
                exact_high_payload_shape_valid(&payload),
                "known-high validation must share the focused client Journal owner"
            );

            let mut trailing = payload;
            trailing.push(0);
            assert!(
                !exact_high_payload_shape_valid(&trailing),
                "known-high validation must reject trailing client Journal bytes"
            );
        }
    }

    #[test]
    fn strict_module_time_uses_focused_fragment_owner() {
        let zero_mask = build_module_time(0, &[], &[0x60]);
        assert!(
            module_time::claim_payload_if_verified(&zero_mask).is_some(),
            "focused Module_Time owner accepts the exact zero-mask cursor"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ModuleTime,
            &zero_mask
        ));
        assert!(exact_high_payload_shape_valid(&zero_mask));

        let data_bits = build_module_time(0x02, &[0x12], &[0x80]);
        assert!(
            module_time::claim_payload_if_verified(&data_bits).is_none(),
            "Module_Time owns no fragment BOOLs"
        );
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ModuleTime,
            &data_bits
        ));
        assert!(
            !exact_high_payload_shape_valid(&data_bits),
            "known-opcode strict validation must share the focused Module_Time owner"
        );
    }

    #[test]
    fn strict_module_info_uses_focused_owner_for_fragment_tail_bits() {
        let full_byte_tail = build_module_info_with_fragment_tail(&[0x00]);
        assert!(
            module::claim_module_info_payload_if_verified(&full_byte_tail).is_some(),
            "focused Module_Info owner should accept a full-byte final cursor"
        );
        assert!(
            module_info_shape_valid(&full_byte_tail),
            "existing exact EE Module_Info tests use a full-byte final cursor"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ModuleInfo,
            &full_byte_tail
        ));
        assert!(exact_high_payload_shape_valid(&full_byte_tail));

        let compact_tail = build_module_info_with_fragment_tail(&[0xC0]);
        assert!(
            module::claim_module_info_payload_if_verified(&compact_tail).is_some(),
            "focused Module_Info owner should accept the compact six-bit final cursor"
        );
        assert!(
            module_info_shape_valid(&compact_tail),
            "compact Module_Info rewrites can preserve the six-bit final cursor"
        );

        let short_tail = build_module_info_with_fragment_tail(&[0xA0]);
        assert!(
            module::claim_module_info_payload_if_verified(&short_tail).is_none(),
            "focused Module_Info owner must count the locstring selector plus two EE tail bits"
        );
        assert!(
            !module_info_shape_valid(&short_tail),
            "Module_Info owns the locstring selector plus two EE tail bits"
        );
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ModuleInfo,
            &short_tail
        ));
        assert!(!exact_high_payload_shape_valid(&short_tail));

        let missing_tail = build_module_info_with_fragment_tail(&[]);
        assert!(module::claim_module_info_payload_if_verified(&missing_tail).is_none());
        assert!(!module_info_shape_valid(&missing_tail));
        assert!(!exact_high_payload_shape_valid(&missing_tail));

        let tail_slack = build_module_info_with_fragment_tail(&[0xC0, 0x00]);
        assert!(
            module::claim_module_info_payload_if_verified(&tail_slack).is_none(),
            "focused Module_Info owner has no multi-byte fragment-tail proof"
        );
        assert!(
            !module_info_shape_valid(&tail_slack),
            "Module_Info has no multi-byte fragment-tail owner"
        );
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ModuleInfo,
            &tail_slack
        ));
        assert!(!exact_high_payload_shape_valid(&tail_slack));
    }

    #[test]
    fn strict_server_status_status_uses_focused_owner() {
        let exact = server_status::status_payload();
        assert!(
            server_status::claim_status_payload_if_verified(&exact).is_some(),
            "focused ServerStatus_Status owner claims only the EE mode/status envelope"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ServerStatusStatus,
            &exact
        ));
        assert!(exact_high_payload_shape_valid(&exact));

        let wrong_empty_status = [b'P', 0x01, 0x00];
        assert!(
            server_status::claim_status_payload_if_verified(&wrong_empty_status).is_none(),
            "client ServerStatus_0 is not the server mode/status transition"
        );
        assert!(
            !verified_family_inflated_payload_valid(
                VerifiedFamily::ServerStatusStatus,
                &wrong_empty_status
            ),
            "verified ServerStatus_Status proof must not accept any three-byte P/01 sibling"
        );

        let mut tail_slack = exact.to_vec();
        tail_slack.push(0);
        assert!(server_status::claim_status_payload_if_verified(&tail_slack).is_none());
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ServerStatusStatus,
            &tail_slack
        ));
        assert!(
            !exact_high_payload_shape_valid(&tail_slack),
            "known-opcode strict validation must reject unowned ServerStatus_Status tail slack"
        );
    }

    #[test]
    fn strict_server_status_module_resources_uses_focused_owner() {
        let runtime = module_resources::ModuleResourceRuntime::default();
        assert!(runtime.observe_legacy_module_info_resources(
            &["cep2_custom".to_string(), "cep2_top_v23".to_string()],
            Some("cep23_v1"),
        ));
        let (payload, _) = module_resources::build_server_status_module_resources_payload(
            &runtime,
            "Path of Ascension",
        )
        .expect("module-resource payload");

        assert!(
            module_resources::claim_payload_if_verified(&payload).is_some(),
            "focused ServerStatus_ModuleResources owner claims exact EE payloads"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ServerStatusModuleResources,
            &payload
        ));
        assert!(exact_high_payload_shape_valid(&payload));

        let mut tail_slack = payload.clone();
        tail_slack.push(0);
        assert!(
            module_resources::claim_payload_if_verified(&tail_slack).is_none(),
            "focused owner rejects unowned fragment tail slack"
        );
        assert!(
            !verified_family_inflated_payload_valid(
                VerifiedFamily::ServerStatusModuleResources,
                &tail_slack,
            ),
            "verified-family proof must share the focused module-resource owner"
        );
        assert!(
            !exact_high_payload_shape_valid(&tail_slack),
            "known-opcode strict validation must share the focused module-resource owner"
        );

        let mut shifted_fragment = payload;
        *shifted_fragment.last_mut().expect("fragment byte") = 0x60;
        assert!(
            module_resources::claim_payload_if_verified(&shifted_fragment).is_none(),
            "focused owner rejects shifted fragment headers"
        );
        assert!(!exact_high_payload_shape_valid(&shifted_fragment));
    }

    #[test]
    fn strict_loadbar_uses_focused_fragment_owner() {
        let start = loadbar::start_payload(2);
        let end = loadbar::end_success_payload(2);

        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::LoadBar,
            &start
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::LoadBar,
            &end
        ));
        assert!(exact_high_payload_shape_valid(&start));
        assert!(exact_high_payload_shape_valid(&end));

        let mut shifted_end = end;
        *shifted_end.last_mut().expect("fragment tail") = 0x60;
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::LoadBar, &shifted_end),
            "LoadBar_End must prove the decompiled four result bits"
        );
        assert!(
            !exact_high_payload_shape_valid(&shifted_end),
            "known-opcode strict validation must share the focused LoadBar owner"
        );

        let mut tail_slack = start;
        tail_slack.push(0);
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::LoadBar, &tail_slack),
            "LoadBar has no multi-byte fragment-tail owner"
        );
        assert!(
            !exact_high_payload_shape_valid(&tail_slack),
            "known-opcode strict validation must reject unowned LoadBar tail slack"
        );
    }

    #[test]
    fn strict_loadbar_splits_in_gameplay_stream() {
        let mut stream = loadbar::start_payload(2);
        stream.extend_from_slice(&server_status::status_payload());

        assert!(
            verified_gameplay_stream_payload_valid(
                Direction::ServerToClient,
                &[VerifiedFamily::LoadBar, VerifiedFamily::ServerStatusStatus,],
                &stream,
            ),
            "LoadBar owns its compact fragment tail before the following status signal"
        );
        assert!(
            !verified_gameplay_stream_payload_valid(
                Direction::ClientToServer,
                &[VerifiedFamily::LoadBar, VerifiedFamily::ServerStatusStatus,],
                &stream,
            ),
            "LoadBar/ServerStatus gameplay streams are server-owned"
        );

        let mut shifted = loadbar::end_success_payload(2);
        *shifted.last_mut().expect("fragment tail") = 0x60;
        shifted.extend_from_slice(&server_status::status_payload());
        assert!(
            !verified_gameplay_stream_payload_valid(
                Direction::ServerToClient,
                &[VerifiedFamily::LoadBar, VerifiedFamily::ServerStatusStatus,],
                &shifted,
            ),
            "a shifted LoadBar_End tail must not be split before the status signal"
        );
    }

    #[test]
    fn strict_journal_splits_in_gameplay_stream() {
        let mut stream = build_journal_delete_world(7);
        stream.extend_from_slice(&server_status::status_payload());

        assert!(
            journal::claim_payload_if_verified(&build_journal_delete_world(7)).is_some(),
            "focused Journal owner accepts the exact DeleteWorld row"
        );
        assert!(
            verified_gameplay_stream_payload_valid(
                Direction::ServerToClient,
                &[VerifiedFamily::Journal, VerifiedFamily::ServerStatusStatus,],
                &stream,
            ),
            "Journal owns its exact fragment cursor before the following status signal"
        );
        assert!(
            !verified_gameplay_stream_payload_valid(
                Direction::ClientToServer,
                &[VerifiedFamily::Journal, VerifiedFamily::ServerStatusStatus,],
                &stream,
            ),
            "Journal/ServerStatus gameplay streams are server-owned"
        );

        let mut shifted = build_journal_delete_world(7);
        *shifted.last_mut().expect("fragment tail") = 0x80;
        shifted.extend_from_slice(&server_status::status_payload());
        assert!(
            !verified_gameplay_stream_payload_valid(
                Direction::ServerToClient,
                &[VerifiedFamily::Journal, VerifiedFamily::ServerStatusStatus,],
                &shifted,
            ),
            "a shifted Journal fragment tail must not be split before the status signal"
        );
    }

    #[test]
    fn strict_sound_object_stop_uses_empty_fragment_owner() {
        let exact = [
            0x50, 0x17, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x47, 0x02, 0x00, 0x80, 0x76,
        ];
        assert!(
            sound::claim_payload_if_verified(&exact).is_some(),
            "focused Sound owner accepts the observed one-byte empty cursor"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Sound,
            &exact
        ));
        assert!(exact_high_payload_shape_valid(&exact));

        let shifted = [
            0x50, 0x17, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x47, 0x02, 0x00, 0x80, 0x80,
        ];
        assert!(
            sound::claim_payload_if_verified(&shifted).is_none(),
            "Sound_Object_Stop owns no fragment BOOLs"
        );
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::Sound, &shifted),
            "verified Sound proof must reject shifted fragment data bits"
        );
        assert!(
            !exact_high_payload_shape_valid(&shifted),
            "known-opcode strict validation must share the focused Sound owner"
        );

        let tail_slack = [
            0x50, 0x17, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x47, 0x02, 0x00, 0x80, 0x60, 0x00,
        ];
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::Sound, &tail_slack),
            "Sound_Object_Stop has no multi-byte fragment-tail owner"
        );
        assert!(
            !exact_high_payload_shape_valid(&tail_slack),
            "known-opcode strict validation must reject Sound tail slack"
        );
    }

    #[test]
    fn strict_custom_token_uses_focused_owner() {
        let exact_set = build_custom_token_set(0x1234, b"hello");
        assert!(
            custom_token::claim_payload_if_verified(&exact_set).is_some(),
            "focused SetCustomToken owner accepts exact CNW token payloads"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::SetCustomToken,
            &exact_set
        ));
        assert!(exact_high_payload_shape_valid(&exact_set));

        let exact_list =
            build_custom_token_list_with_count(2, &[(0x1234, &b"a"[..]), (0x5678, &b"bc"[..])]);
        assert!(
            custom_token::claim_payload_if_verified(&exact_list).is_some(),
            "focused SetCustomTokenList owner accepts counted token rows"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::SetCustomToken,
            &exact_list
        ));
        assert!(exact_high_payload_shape_valid(&exact_list));

        let malformed = build_custom_token_list_with_count(2, &[(0x1234, &b"a"[..])]);
        assert!(
            custom_token::claim_payload_if_verified(&malformed).is_none(),
            "malformed custom token lists are owned only by the rewrite path"
        );
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::SetCustomToken, &malformed),
            "verified SetCustomToken proof must not bypass the focused owner"
        );
        assert!(
            !exact_high_payload_shape_valid(&malformed),
            "known-opcode strict validation must share the focused custom-token owner"
        );
    }

    #[test]
    fn strict_party_list_counts_declared_member_rows() {
        let empty_party = build_party_list(&[]);
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::Party,
            &empty_party
        ));
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::ClientParty, &empty_party),
            "client Party proof must not validate server Party_List rows"
        );
        assert!(exact_high_payload_shape_valid(&empty_party));

        let missing_member = build_party_list_with_count(2, &[]);
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::Party, &missing_member),
            "Party_List count must match the decompiled OBJECTID row count"
        );
        assert!(
            !exact_high_payload_shape_valid(&missing_member),
            "known-opcode strict validation must share the focused Party owner"
        );
    }

    #[test]
    fn strict_party_rejects_unmodeled_control_wrappers() {
        let get_list = vec![0x70, 0x0E, 0x02];
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientParty,
            &get_list
        ));
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::Party, &get_list),
            "server Party proof must not validate client Party_GetList requests"
        );
        assert!(exact_high_payload_shape_valid(&get_list));

        let get_list_cnw = build_party_control_wrapper(0x02);
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::ClientParty, &get_list_cnw),
            "Party_GetList is only owned as the exact no-body client request"
        );
        assert!(
            !exact_high_payload_shape_valid(&get_list_cnw),
            "known-opcode strict validation must reject the same unowned GetList wrapper"
        );

        let transfer_control = build_party_control_wrapper(0x0E);
        assert!(
            !verified_family_inflated_payload_valid(VerifiedFamily::Party, &transfer_control),
            "unmodeled party control minors must not validate as generic CNW wrappers"
        );
        assert!(
            !exact_high_payload_shape_valid(&transfer_control),
            "known-opcode strict validation must not allow unmodeled Party control wrappers"
        );
    }

    #[test]
    fn strict_client_party_get_list_splits_in_gameplay_stream() {
        let payload = [0x70, 0x0E, 0x02, 0x70, 0x04, 0x03];

        assert!(
            verified_gameplay_stream_payload_valid(
                Direction::ClientToServer,
                &[VerifiedFamily::ClientParty, VerifiedFamily::ClientArea],
                &payload,
            ),
            "ClientParty and ClientArea no-body signals should split as two exact stream units"
        );
        assert!(
            !verified_gameplay_stream_payload_valid(
                Direction::ServerToClient,
                &[VerifiedFamily::ClientParty, VerifiedFamily::ClientArea],
                &payload,
            ),
            "client-owned stream units must still reject the server direction"
        );
    }

    #[test]
    fn strict_play_module_character_list_splits_client_controls_and_server_response() {
        for minor in [0x01, 0x02] {
            let payload = [0x50, 0x31, minor];
            assert!(
                play_module_character_list::claim_client_payload_if_verified(&payload).is_some(),
                "focused client PlayModuleCharacterList owner should claim minor {minor:#04x}"
            );
            assert!(
                verified_family_inflated_payload_valid(
                    VerifiedFamily::ClientPlayModuleCharacterList,
                    &payload,
                ),
                "client verified-family proof must share the focused PlayModuleCharacterList owner"
            );
            assert!(
                !verified_family_inflated_payload_valid(
                    VerifiedFamily::PlayModuleCharacterList,
                    &payload,
                ),
                "server verified-family proof must not claim client PlayModuleCharacterList controls"
            );
            assert!(
                exact_high_payload_shape_valid(&payload),
                "known-opcode strict validation remains directionless for exact shapes"
            );

            let mut trailing = payload.to_vec();
            trailing.push(0);
            assert!(
                play_module_character_list::claim_client_payload_if_verified(&trailing).is_none(),
                "focused client owner must reject trailing bytes for minor {minor:#04x}"
            );
            assert!(
                !exact_high_payload_shape_valid(&trailing),
                "known-opcode strict validation must reject the same trailing bytes"
            );
        }

        let response = [
            0x50, 0x31, 0x03, 0x0B, 0x00, 0x00, 0x00, 0xF9, 0xFF, 0xFF, 0x7F, 0x80,
        ];
        assert!(
            play_module_character_list::claim_server_payload_if_verified(&response).is_some(),
            "focused server owner should claim response payload"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::PlayModuleCharacterList,
            &response,
        ));
        assert!(
            !verified_family_inflated_payload_valid(
                VerifiedFamily::ClientPlayModuleCharacterList,
                &response,
            ),
            "client proof must not claim PlayModuleCharacterList responses"
        );
        assert!(exact_high_payload_shape_valid(&response));
    }

    #[test]
    fn strict_client_char_list_uses_focused_fragment_tail_owner() {
        let request = vec![0x70, 0x11, 0x01];
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientCharList,
            &request
        ));
        assert!(exact_high_payload_shape_valid(&request));

        for tail in [Vec::new(), vec![0x60]] {
            let payload = build_client_char_list_request_update(&tail);
            assert!(
                verified_family_inflated_payload_valid(VerifiedFamily::ClientCharList, &payload),
                "focused ClientCharList owner accepts tail {tail:02X?}"
            );
            assert!(
                exact_high_payload_shape_valid(&payload),
                "known-opcode strict validation must share the focused owner for tail {tail:02X?}"
            );
        }

        for tail in [vec![0x80], vec![0x60, 0x00]] {
            let payload = build_client_char_list_request_update(&tail);
            assert!(
                !verified_family_inflated_payload_valid(VerifiedFamily::ClientCharList, &payload),
                "0x11/0x03 has no decompiled BOOL reader for tail {tail:02X?}"
            );
            assert!(
                !exact_high_payload_shape_valid(&payload),
                "known-opcode strict validation must reject the same unowned tail {tail:02X?}"
            );
        }
    }

    #[test]
    fn strict_client_quickbar_uses_focused_set_button_owner() {
        let payload = build_client_quickbar_set_button(5, 43, &[0x52, 0x01, 0xF0, 0x03]);
        assert!(
            client_quickbar::claim_payload_if_verified(&payload).is_some(),
            "focused ClientQuickbar owner should claim an exact int-param SetButton"
        );
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientQuickbar,
            &payload
        ));
        assert!(
            exact_high_payload_shape_valid(&payload),
            "known-opcode validation must share the focused ClientQuickbar owner"
        );

        let mut trailing = payload.clone();
        trailing.push(0);
        assert!(
            client_quickbar::claim_payload_if_verified(&trailing).is_none(),
            "focused ClientQuickbar owner must reject bytes after the empty fragment cursor"
        );
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientQuickbar,
            &trailing
        ));
        assert!(
            !exact_high_payload_shape_valid(&trailing),
            "known-opcode validation must reject the same trailing bytes"
        );

        let mut shifted_fragment = payload;
        *shifted_fragment
            .last_mut()
            .expect("test payload should carry one fragment cursor byte") = 0x80;
        assert!(
            client_quickbar::claim_payload_if_verified(&shifted_fragment).is_none(),
            "SetButton owns only an empty final fragment cursor"
        );
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientQuickbar,
            &shifted_fragment
        ));
        assert!(
            !exact_high_payload_shape_valid(&shifted_fragment),
            "known-opcode validation must reject the same shifted cursor"
        );
    }

    #[test]
    fn verified_client_character_sheet_accepts_exact_status_shape() {
        let payload = [
            0x70, 0x15, 0x01, 0x0C, 0x00, 0x00, 0x00, 0x00, 0xFE, 0xFF, 0xFF, 0xFF, 0x7C,
        ];
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::ClientCharacterSheet,
            &payload,
        ));

        let mut trailing = payload.to_vec();
        trailing.push(0);
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::ClientCharacterSheet,
            &trailing,
        ));
    }

    #[test]
    fn verified_gui_timing_event_accepts_exact_bool_branches() {
        let start = [
            0x50, 0x30, 0x01, 0x0C, 0x00, 0x00, 0x00, 0x5C, 0x44, 0x00, 0x00, 0x06, 0x9B,
        ];
        let stop = [0x50, 0x30, 0x01, 0x07, 0x00, 0x00, 0x00, 0x80];

        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::GuiTimingEvent,
            &start,
        ));
        assert!(verified_family_inflated_payload_valid(
            VerifiedFamily::GuiTimingEvent,
            &stop,
        ));

        let mut stale_start = start;
        stale_start[3] = 0x0B;
        assert!(!verified_family_inflated_payload_valid(
            VerifiedFamily::GuiTimingEvent,
            &stale_start,
        ));
    }

    fn build_client_quickbar_set_button(slot: u8, button_type: u8, body: &[u8]) -> Vec<u8> {
        const HIGH_LEVEL_HEADER_BYTES: usize = 3;
        const CNW_LENGTH_BYTES: usize = 4;
        const SLOT_AND_TYPE_BYTES: usize = 2;
        const EMPTY_CNW_FRAGMENT_CURSOR: u8 = 0x60;

        let declared =
            HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + SLOT_AND_TYPE_BYTES + body.len();
        let mut payload = Vec::with_capacity(declared + 1);
        payload.extend_from_slice(&[0x70, 0x1E, 0x02]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.push(slot);
        payload.push(button_type);
        payload.extend_from_slice(body);
        payload.push(EMPTY_CNW_FRAGMENT_CURSOR);
        payload
    }

    fn build_client_input_payload(minor: u8, body: &[u8]) -> Vec<u8> {
        const CLIENT_INPUT_HEADER_BYTES: usize = 7;

        let declared = CLIENT_INPUT_HEADER_BYTES + body.len();
        let mut payload = Vec::with_capacity(declared + 1);
        payload.extend_from_slice(&[0x70, 0x06, minor]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(body);
        payload.push(0x60);
        payload
    }

    fn build_client_dialog_reply_payload() -> Vec<u8> {
        const HIGH_LEVEL_HEADER_BYTES: usize = 3;
        const CNW_LENGTH_BYTES: usize = 4;
        const OBJECT_ID_BYTES: usize = 4;
        const DWORD_BYTES: usize = 4;
        const BYTE_BYTES: usize = 1;
        const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
        const DECLARED: usize =
            READ_START + OBJECT_ID_BYTES + DWORD_BYTES + BYTE_BYTES + DWORD_BYTES;

        let mut payload = vec![0x70, 0x14, 0x03];
        payload.extend_from_slice(&(DECLARED as u32).to_le_bytes());
        payload.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.push(0x60);
        payload
    }

    fn build_module_time(mask: u8, body: &[u8], tail: &[u8]) -> Vec<u8> {
        let declared = 3 + 4 + 1 + body.len();
        let mut payload = vec![b'P', 0x03, 0x03];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.push(mask);
        payload.extend_from_slice(body);
        payload.extend_from_slice(tail);
        payload
    }

    fn build_module_info_with_fragment_tail(fragment_tail: &[u8]) -> Vec<u8> {
        let mut payload = vec![b'P', 0x03, 0x01, 0, 0, 0, 0];
        write_test_string(&mut payload, "Path of Ascension CEP Legends");
        write_test_string(&mut payload, "Path of Ascension CEP Legends");
        payload.push(0x02);
        write_test_resref16(&mut payload, "poa_mod");
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&0x8000_0001u32.to_le_bytes());
        write_test_string(&mut payload, "Armor Shop");
        payload.push(0);
        let declared = u32::try_from(payload.len()).expect("test Module_Info length fits u32");
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(fragment_tail);
        payload
    }

    fn write_test_string(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn write_test_resref16(out: &mut Vec<u8>, value: &str) {
        assert!(value.len() <= 16);
        let mut bytes = [0u8; 16];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        out.extend_from_slice(&bytes);
    }

    fn build_client_login_server_subdirectory_character(resref: &[u8]) -> Vec<u8> {
        const RESREF_BYTES: usize = 16;

        assert!(resref.len() <= RESREF_BYTES);
        let declared = 3 + 4 + RESREF_BYTES;
        let mut payload = vec![0x70, 0x02, 0x11];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        let mut fixed_resref = [0u8; RESREF_BYTES];
        fixed_resref[..resref.len()].copy_from_slice(resref);
        payload.extend_from_slice(&fixed_resref);
        payload.push(0x60);
        payload
    }

    fn build_client_login_waypoint_response(tag: &[u8]) -> Vec<u8> {
        let declared = 3 + 4 + 4 + tag.len();
        let mut payload = vec![0x70, 0x02, 0x0D];
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&(tag.len() as u32).to_le_bytes());
        payload.extend_from_slice(tag);
        payload.push(0x60);
        payload
    }

    fn build_client_input_object_only(minor: u8) -> Vec<u8> {
        build_client_input_payload(minor, &0x8000_34D1u32.to_le_bytes())
    }

    fn build_client_input_empty(minor: u8) -> Vec<u8> {
        build_client_input_payload(minor, &[])
    }

    fn build_client_input_high_level_only(minor: u8) -> Vec<u8> {
        vec![0x70, 0x06, minor]
    }

    fn build_client_input_use_feat_target() -> Vec<u8> {
        let mut body = Vec::with_capacity(8);
        body.extend_from_slice(&0x0123u16.to_le_bytes());
        body.extend_from_slice(&0x0045u16.to_le_bytes());
        body.extend_from_slice(&0x8000_34D1u32.to_le_bytes());
        build_client_input_payload(0x06, &body)
    }

    fn build_client_input_use_feat_location() -> Vec<u8> {
        let mut body = Vec::with_capacity(20);
        body.extend_from_slice(&0x0123u16.to_le_bytes());
        body.extend_from_slice(&0x0045u16.to_le_bytes());
        body.extend_from_slice(&0x7F00_0000u32.to_le_bytes());
        body.extend_from_slice(&11.0f32.to_le_bytes());
        body.extend_from_slice(&12.0f32.to_le_bytes());
        body.extend_from_slice(&0.5f32.to_le_bytes());
        build_client_input_payload(0x06, &body)
    }

    fn build_client_input_use_skill() -> Vec<u8> {
        let mut body = Vec::with_capacity(18);
        body.push(3);
        body.push(0);
        body.extend_from_slice(&0x8000_34D1u32.to_le_bytes());
        body.extend_from_slice(&1.0f32.to_le_bytes());
        body.extend_from_slice(&2.0f32.to_le_bytes());
        body.extend_from_slice(&0.0f32.to_le_bytes());
        build_client_input_payload(0x07, &body)
    }

    fn build_client_input_toggle_mode(counterspell_target: Option<u32>) -> Vec<u8> {
        let mut body = Vec::with_capacity(1 + counterspell_target.map_or(0, |_| 4));
        body.push(if counterspell_target.is_some() { 5 } else { 1 });
        if let Some(target) = counterspell_target {
            body.extend_from_slice(&target.to_le_bytes());
        }
        build_client_input_payload(0x0A, &body)
    }

    fn build_client_input_memorize_spell() -> Vec<u8> {
        let mut body = Vec::with_capacity(8);
        body.push(0);
        body.extend_from_slice(&0x0000_1234u32.to_le_bytes());
        body.push(2);
        body.push(1);
        body.push(0);
        build_client_input_payload(0x10, &body)
    }

    fn build_client_input_unmemorize_spell() -> Vec<u8> {
        build_client_input_payload(0x11, &[0, 2, 1])
    }

    fn build_client_char_list_request_update(fragment_tail: &[u8]) -> Vec<u8> {
        const DECLARED_BYTES: usize = 3 + 4 + 1 + 16;

        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x70, 0x11, 0x03]);
        payload.extend_from_slice(&(DECLARED_BYTES as u32).to_le_bytes());
        payload.push(0x01);
        payload.extend_from_slice(b"starcore-druid60");
        payload.extend_from_slice(fragment_tail);
        payload
    }

    fn build_player_list_delete_payload(player_id: u32, fragment_tail: u8) -> Vec<u8> {
        const DECLARED_BYTES: usize = 3 + 4 + 4;

        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x50, 0x0A, 0x03]);
        payload.extend_from_slice(&(DECLARED_BYTES as u32).to_le_bytes());
        payload.extend_from_slice(&player_id.to_le_bytes());
        payload.push(fragment_tail);
        payload
    }

    fn build_party_list(member_ids: &[u32]) -> Vec<u8> {
        build_party_list_with_count(member_ids.len() as u32, member_ids)
    }

    fn build_party_list_with_count(count: u32, member_ids: &[u32]) -> Vec<u8> {
        const PARTY_LIST_READ_START: usize = 3 + 4;
        const PARTY_LIST_COUNT_BYTES: usize = 4;
        const EMPTY_FRAGMENT_BYTE: u8 = 0x60;

        let declared = PARTY_LIST_READ_START + PARTY_LIST_COUNT_BYTES + member_ids.len() * 4;
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x70, 0x0E, 0x01]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&count.to_le_bytes());
        for id in member_ids {
            payload.extend_from_slice(&id.to_le_bytes());
        }
        payload.push(EMPTY_FRAGMENT_BYTE);
        payload
    }

    fn build_party_control_wrapper(minor: u8) -> Vec<u8> {
        const PARTY_READ_START: usize = 3 + 4;
        const EMPTY_FRAGMENT_BYTE: u8 = 0x60;

        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x70, 0x0E, minor]);
        payload.extend_from_slice(&(PARTY_READ_START as u32).to_le_bytes());
        payload.push(EMPTY_FRAGMENT_BYTE);
        payload
    }

    fn build_journal_delete_world(entry_id: u32) -> Vec<u8> {
        let declared = 3 + 4 + 4;
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x50, 0x1C, 0x03]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&entry_id.to_le_bytes());
        payload.push(0x60);
        payload
    }

    fn build_custom_token_set(token_id: u32, value: &[u8]) -> Vec<u8> {
        let declared = 3 + 4 + 4 + 4 + value.len();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x50, 0x32, 0x01]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&token_id.to_le_bytes());
        payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
        payload.extend_from_slice(value);
        payload.push(0x60);
        payload
    }

    fn build_custom_token_list_with_count(count: u32, entries: &[(u32, &[u8])]) -> Vec<u8> {
        let body_len = 4 + entries
            .iter()
            .map(|(_, value)| 4 + 4 + value.len())
            .sum::<usize>();
        let declared = 3 + 4 + body_len;
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x50, 0x32, 0x02]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&count.to_le_bytes());
        for (token_id, value) in entries {
            payload.extend_from_slice(&token_id.to_le_bytes());
            payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
            payload.extend_from_slice(value);
        }
        payload.push(0x60);
        payload
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

        let decision = decide(Direction::ServerToClient, &packet, StrictProfile::Player);
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

    fn build_m_opaque_continuation_frame(payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![b'M', 0, 0, 0, 0x3C, 0, 0x4D, 0x08, 0, 0, 0, 0];
        assert!(write_be_u16(&mut packet, 10, payload.len() as u16));
        packet.extend_from_slice(payload);
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }

    fn build_client_raw_m_frame(
        sequence: u16,
        ack_sequence: u16,
        payload: &[u8],
        trailing: &[u8],
    ) -> Vec<u8> {
        let mut packet = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, sequence));
        assert!(write_be_u16(&mut packet, 5, ack_sequence));
        packet[7] = 0x0A;
        assert!(write_be_u16(&mut packet, 8, 1));
        assert!(write_be_u16(&mut packet, 10, payload.len() as u16));
        packet.extend_from_slice(payload);
        packet.extend_from_slice(trailing);
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
    // strings. EE's server-side `HandleBNXIMessage` accepts the shorter
    // six-byte request envelope too, and live Diamond/HG source traffic emits
    // that minimal `BNXI + UDP port` form. The minimal form is a verified
    // legacy discovery probe; only the longer EE form proves a client build.
    let bytes = packet.bytes;
    if bytes.len() == 6 {
        return StrictDecision::allow("BN", packet.tag.name(), "known-legacy-bnxi-probe");
    }
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

#[cfg(test)]
mod bn_synthetic_direction_tests {
    use super::*;

    #[test]
    fn synthetic_server_bner_uses_server_response_validator() {
        let mut packet = Vec::from(&b"BNER"[..]);
        packet.push(0x55);
        packet.extend_from_slice(&5133u16.to_le_bytes());
        packet.push(0);
        packet.push(5);
        packet.extend_from_slice(b"HG213");

        let decision = decide(
            Direction::ServerToClientSynthetic,
            &packet,
            StrictProfile::Player,
        );

        assert_eq!(decision.verdict, Verdict::Allow);
        assert_eq!(decision.reason, "known-ee-server-enumerate-response");
    }

    #[test]
    fn client_bnxi_accepts_legacy_probe_and_full_ee_request() {
        let legacy = decide(
            Direction::ClientToServer,
            b"BNXI\x00\x14",
            StrictProfile::Player,
        );
        assert_eq!(legacy.verdict, Verdict::Allow);
        assert_eq!(legacy.reason, "known-legacy-bnxi-probe");

        let full = [
            b'B', b'N', b'X', b'I', 0x69, 0xC9, 0, 0, 0, 0, 0, 2, 4, b'8', b'1', b'9', b'3', 2,
            b'3', b'7', 2, b'1', b'7', 8, b'2', b'6', b'c', b'6', b'e', b'5', b'7', b'3',
        ];
        let full = decide(Direction::ClientToServer, &full, StrictProfile::Player);
        assert_eq!(full.verdict, Verdict::Allow);
        assert_eq!(full.reason, "known-ee-extended-info-request");
    }

    #[test]
    fn synthetic_server_bnxr_uses_server_response_validator() {
        let module = b"Path of Ascension CEP Legends";
        let mut packet = Vec::from(&b"BNXR"[..]);
        packet.extend_from_slice(&5133u16.to_le_bytes());
        packet.extend_from_slice(&[
            0xFD, 0x00, 0x01, 0x28, 0x00, 0x10, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x03,
        ]);
        packet.push(module.len() as u8);
        packet.extend_from_slice(module);

        let decision = decide(
            Direction::ServerToClientSynthetic,
            &packet,
            StrictProfile::Player,
        );

        assert_eq!(decision.verdict, Verdict::Allow);
        assert_eq!(decision.reason, "known-bnxr-extended-server-control");
    }
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
