//! Direct server-to-client high-level `M` dispatch.
//!
//! This module owns direct-frame routing only: extract the reliable gameplay
//! payload, delegate semantic translation to focused siblings, then repair the
//! M-frame length/CRC. Deflated-window and coalesced-window routing stays in
//! the parent M-frame transport layer.

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{HighLevel, MFrameView},
    translate::{
        VerifiedFamily, VerifiedPacket, VerifiedProof, ambient, area, area_change_day_night,
        area_visual_effect, camera, char_list, chat, client_side_message, cnw_message,
        custom_token, cutscene, dialog, game_obj_update, gameplay_stream, gui_timing_event,
        inventory, journal, live_object, loadbar, login, module, module_resources, module_time,
        party, play_module_character_list, player_list, quickbar, safe_projectile, semantic, sound,
    },
};

use super::{deferred_module_resources, live_update, parse_window};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy)]
pub(super) enum SemanticScope {
    DeflatedReassembly,
    CoalescedSpan,
}

#[derive(Debug, Default)]
pub(super) struct InflatedPayloadRewrite {
    family_names: Vec<&'static str>,
    families: Vec<VerifiedFamily>,
    unit_families: Vec<VerifiedFamily>,
    pub(super) area_rewrite: Option<area::AreaRewriteSummary>,
    pub(super) module_info_candidate_offset: Option<usize>,
    pub(super) quarantine_reason: Option<&'static str>,
}

impl InflatedPayloadRewrite {
    pub(super) fn note_rewrite(&mut self, family_name: &'static str, family: VerifiedFamily) {
        self.quarantine_reason = None;
        if !self.family_names.contains(&family_name) {
            self.family_names.push(family_name);
        }
        if !self.families.contains(&family) {
            self.families.push(family);
        }
        if self.unit_families.last().copied() != Some(family) {
            self.unit_families.push(family);
        }
    }

    pub(super) fn any_rewrite(&self) -> bool {
        !self.family_names.is_empty()
    }

    pub(super) fn should_quarantine(&self) -> bool {
        self.quarantine_reason.is_some()
    }

    pub(super) fn verified_family(&self) -> VerifiedFamily {
        self.families
            .as_slice()
            .first()
            .copied()
            .filter(|_| self.families.len() == 1)
            .unwrap_or(VerifiedFamily::SemanticDeflated)
    }

    pub(super) fn verified_proof(&self) -> VerifiedProof {
        VerifiedProof::from_unit_families(self.unit_families.clone())
    }
}

pub(super) struct DeflatedSemanticLogContext {
    pub(super) frames: usize,
    pub(super) first_sequence: u16,
    pub(super) packetized_sequence: u16,
    pub(super) old_inflated_length: usize,
    pub(super) rewritten_inflated_length: usize,
    pub(super) compressed_length: usize,
    pub(super) used_server_stream: bool,
    pub(super) proxy_owned_stream: bool,
}

pub(super) fn wrap_legacy_live_object_continuation_if_needed(payload: &mut Vec<u8>) -> bool {
    HighLevel::parse(payload).is_none()
        && live_object::wrap_legacy_live_object_continuation_payload_if_plausible(payload).is_some()
}

#[derive(Debug, Default)]
struct ServerTranslatorClaim {
    area_rewrite: Option<area::AreaRewriteSummary>,
}

#[derive(Debug)]
enum ServerTranslatorOutcome {
    None,
    TransportOnly,
    Claim(ServerTranslatorClaim),
}

type ServerTranslatorFn = fn(
    &mut Vec<u8>,
    Option<&area::AreaPlaceableContext>,
    SemanticScope,
    Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome;

#[derive(Debug, Clone, Copy)]
struct ServerToClientTranslator {
    family_name: &'static str,
    verified_family: Option<VerifiedFamily>,
    translate: ServerTranslatorFn,
}

const SERVER_TO_CLIENT_TRANSLATORS: &[ServerToClientTranslator] = &[
    ServerToClientTranslator {
        family_name: "SetCustomToken",
        verified_family: Some(VerifiedFamily::SetCustomToken),
        translate: translate_custom_token,
    },
    ServerToClientTranslator {
        family_name: "Login",
        verified_family: Some(VerifiedFamily::Login),
        translate: translate_login,
    },
    ServerToClientTranslator {
        family_name: "Module_Time",
        verified_family: Some(VerifiedFamily::ModuleTime),
        translate: translate_module_time,
    },
    ServerToClientTranslator {
        family_name: "Module_EndGame",
        verified_family: Some(VerifiedFamily::ModuleEndGame),
        translate: translate_module_end_game,
    },
    ServerToClientTranslator {
        family_name: "Camera",
        verified_family: Some(VerifiedFamily::Camera),
        translate: translate_camera,
    },
    ServerToClientTranslator {
        family_name: "Cutscene",
        verified_family: Some(VerifiedFamily::Cutscene),
        translate: translate_cutscene,
    },
    ServerToClientTranslator {
        family_name: "ServerStatus_ModuleResources",
        verified_family: Some(VerifiedFamily::ServerStatusModuleResources),
        translate: translate_server_status_module_resources,
    },
    ServerToClientTranslator {
        family_name: "LoadBar",
        verified_family: Some(VerifiedFamily::LoadBar),
        translate: translate_loadbar,
    },
    ServerToClientTranslator {
        family_name: "GuiTimingEvent_Info",
        verified_family: Some(VerifiedFamily::GuiTimingEvent),
        translate: translate_gui_timing_event,
    },
    ServerToClientTranslator {
        family_name: "ClientSideMessage",
        verified_family: Some(VerifiedFamily::ClientSideMessage),
        translate: translate_client_side_message,
    },
    ServerToClientTranslator {
        family_name: "Journal",
        verified_family: Some(VerifiedFamily::Journal),
        translate: translate_journal,
    },
    ServerToClientTranslator {
        family_name: "Chat",
        verified_family: Some(VerifiedFamily::Chat),
        translate: translate_chat,
    },
    ServerToClientTranslator {
        family_name: "Sound",
        verified_family: Some(VerifiedFamily::Sound),
        translate: translate_sound,
    },
    ServerToClientTranslator {
        family_name: "Ambient",
        verified_family: Some(VerifiedFamily::Ambient),
        translate: translate_ambient,
    },
    ServerToClientTranslator {
        family_name: "Dialog",
        verified_family: Some(VerifiedFamily::Dialog),
        translate: translate_dialog,
    },
    ServerToClientTranslator {
        family_name: "Inventory",
        verified_family: Some(VerifiedFamily::Inventory),
        translate: translate_inventory,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_ObjControl",
        verified_family: Some(VerifiedFamily::GameObjUpdateObjectControl),
        translate: translate_game_obj_update_obj_control,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_VisEffect",
        verified_family: Some(VerifiedFamily::GameObjUpdateVisEffect),
        translate: translate_game_obj_update_vis_effect,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_DestroyItem",
        verified_family: Some(VerifiedFamily::GameObjUpdateDestroyItem),
        translate: translate_game_obj_update_destroy_item,
    },
    ServerToClientTranslator {
        family_name: "Area_VisualEffect",
        verified_family: Some(VerifiedFamily::AreaVisualEffect),
        translate: translate_area_visual_effect,
    },
    ServerToClientTranslator {
        family_name: "Area_ChangeDayNight",
        verified_family: Some(VerifiedFamily::AreaChangeDayNight),
        translate: translate_area_change_day_night,
    },
    ServerToClientTranslator {
        family_name: "SafeProjectile",
        verified_family: Some(VerifiedFamily::SafeProjectile),
        translate: translate_safe_projectile,
    },
    ServerToClientTranslator {
        family_name: "Party",
        verified_family: Some(VerifiedFamily::Party),
        translate: translate_party,
    },
    ServerToClientTranslator {
        family_name: "PlayModuleCharacterList",
        verified_family: Some(VerifiedFamily::PlayModuleCharacterList),
        translate: translate_play_module_character_list,
    },
    ServerToClientTranslator {
        family_name: "GuiQuickbar",
        verified_family: Some(VerifiedFamily::GuiQuickbar),
        translate: translate_quickbar,
    },
    ServerToClientTranslator {
        family_name: "CNWPrefixedFragmentsTransportOnly",
        verified_family: None,
        translate: normalize_cnw_transport_only,
    },
    ServerToClientTranslator {
        family_name: "CharList",
        verified_family: Some(VerifiedFamily::CharList),
        translate: translate_char_list,
    },
    ServerToClientTranslator {
        family_name: "PlayerList",
        verified_family: Some(VerifiedFamily::PlayerList),
        translate: translate_player_list,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_LiveObjectPrefixedFragments",
        verified_family: Some(VerifiedFamily::GameObjUpdateLiveObject),
        translate: translate_live_object_prefixed_fragments,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_LiveObjectExactRecords",
        verified_family: Some(VerifiedFamily::GameObjUpdateLiveObject),
        translate: translate_live_object_exact_records,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_LiveObjectAddRecords",
        verified_family: Some(VerifiedFamily::GameObjUpdateLiveObject),
        translate: translate_live_object_add_records,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_LiveObjectUpdateRecords",
        verified_family: Some(VerifiedFamily::GameObjUpdateLiveObject),
        translate: translate_live_object_update_records,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_LiveObjectClaimedRecords",
        verified_family: Some(VerifiedFamily::GameObjUpdateLiveObject),
        translate: translate_live_object_claimed_records,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate_LiveObjectDeclaredLengthRepair",
        verified_family: Some(VerifiedFamily::GameObjUpdateLiveObject),
        translate: translate_live_object_declared_length_repair,
    },
    ServerToClientTranslator {
        family_name: "Area_ClientArea",
        verified_family: Some(VerifiedFamily::AreaClientArea),
        translate: translate_area_client_area,
    },
    ServerToClientTranslator {
        family_name: "Module_Info",
        verified_family: Some(VerifiedFamily::ModuleInfo),
        translate: translate_module_info,
    },
];

pub(super) fn rewrite_inflated_payload_for_ee(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
    module_resource_runtime: Option<&module_resources::ModuleResourceRuntime>,
    object_registry: Option<&semantic::ObjectRegistry>,
    preclaimed_family: Option<(&'static str, VerifiedFamily)>,
) -> InflatedPayloadRewrite {
    let split_units = {
        let split = gameplay_stream::split_inflated_gameplay(payload);
        if !split.complete {
            tracing::debug!(
                units = split.units.len(),
                payload_length = payload.len(),
                "inflated gameplay stream classified as incomplete/non-header continuation"
            );
        }
        if split.units.len() > 1 {
            Some(
                split
                    .units
                    .iter()
                    .map(|unit| match unit {
                        gameplay_stream::GameplayUnit::HighLevel(message) => {
                            OwnedGameplayUnit::HighLevel(message.payload.to_vec())
                        }
                        gameplay_stream::GameplayUnit::Continuation(bytes) => {
                            OwnedGameplayUnit::Continuation(bytes.to_vec())
                        }
                        gameplay_stream::GameplayUnit::PendingFragment(bytes) => {
                            OwnedGameplayUnit::PendingFragment(bytes.to_vec())
                        }
                        gameplay_stream::GameplayUnit::Unknown(bytes) => {
                            OwnedGameplayUnit::Unknown(bytes.to_vec())
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        }
    };

    if let Some(units) = split_units {
        return rewrite_split_inflated_payload_for_ee(
            payload,
            units,
            latest_area_placeables,
            scope,
            module_resource_runtime,
            object_registry,
        );
    }

    rewrite_single_inflated_payload_for_ee(
        payload,
        latest_area_placeables,
        scope,
        module_resource_runtime,
        object_registry,
        preclaimed_family,
    )
}

#[derive(Debug)]
enum OwnedGameplayUnit {
    HighLevel(Vec<u8>),
    Continuation(Vec<u8>),
    PendingFragment(Vec<u8>),
    Unknown(Vec<u8>),
}

fn rewrite_split_inflated_payload_for_ee(
    payload: &mut Vec<u8>,
    units: Vec<OwnedGameplayUnit>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
    module_resource_runtime: Option<&module_resources::ModuleResourceRuntime>,
    object_registry: Option<&semantic::ObjectRegistry>,
) -> InflatedPayloadRewrite {
    let mut rewrite = InflatedPayloadRewrite::default();
    let mut translated_units = Vec::with_capacity(units.len());

    for unit in units {
        match unit {
            OwnedGameplayUnit::HighLevel(mut unit_payload) => {
                let unit_rewrite = rewrite_single_inflated_payload_for_ee(
                    &mut unit_payload,
                    latest_area_placeables,
                    scope,
                    module_resource_runtime,
                    object_registry,
                    None,
                );
                if unit_rewrite.should_quarantine() || !unit_rewrite.any_rewrite() {
                    rewrite.quarantine_reason = unit_rewrite
                        .quarantine_reason
                        .or(Some("split-unit-unclaimed-high-level"));
                    return rewrite;
                }
                let unit_family = unit_rewrite.verified_family();
                let unit_families = unit_rewrite.unit_families.clone();
                for family_name in unit_rewrite.family_names {
                    if !rewrite.family_names.contains(&family_name) {
                        rewrite.family_names.push(family_name);
                    }
                }
                for family in unit_rewrite.families {
                    if !rewrite.families.contains(&family) {
                        rewrite.families.push(family);
                    }
                }
                rewrite.unit_families.extend(unit_families);
                if unit_rewrite.area_rewrite.is_some() {
                    rewrite.area_rewrite = unit_rewrite.area_rewrite;
                }
                if unit_rewrite.module_info_candidate_offset.is_some() {
                    rewrite.module_info_candidate_offset =
                        unit_rewrite.module_info_candidate_offset;
                }
                translated_units.push(gameplay_stream::TranslatedGameplayUnit::Owned {
                    family: unit_family,
                    bytes: unit_payload,
                });
            }
            OwnedGameplayUnit::Continuation(bytes) => {
                tracing::warn!(
                    len = bytes.len(),
                    "split inflated gameplay stream contains continuation bytes without owner"
                );
                rewrite.quarantine_reason = Some("split-continuation-without-owner");
                return rewrite;
            }
            OwnedGameplayUnit::PendingFragment(bytes) => {
                tracing::warn!(
                    len = bytes.len(),
                    "split inflated gameplay stream contains pending fragment bytes"
                );
                rewrite.quarantine_reason = Some("split-pending-fragment");
                return rewrite;
            }
            OwnedGameplayUnit::Unknown(bytes) => {
                tracing::warn!(
                    len = bytes.len(),
                    "split inflated gameplay stream contains unknown bytes"
                );
                rewrite.quarantine_reason = Some("split-unknown-unit");
                return rewrite;
            }
        }
    }

    if let Some(joined) = gameplay_stream::rejoin_translated_units(&translated_units) {
        *payload = joined;
    } else {
        rewrite.quarantine_reason = Some("split-unit-rejoin-failed");
    }
    rewrite
}

fn rewrite_single_inflated_payload_for_ee(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
    module_resource_runtime: Option<&module_resources::ModuleResourceRuntime>,
    object_registry: Option<&semantic::ObjectRegistry>,
    preclaimed_family: Option<(&'static str, VerifiedFamily)>,
) -> InflatedPayloadRewrite {
    let mut rewrite = InflatedPayloadRewrite::default();

    if let Some((family_name, family)) = preclaimed_family {
        rewrite.note_rewrite(family_name, family);
    }

    if is_live_object_high_level_payload(payload) {
        return rewrite_live_object_high_level_payload_for_ee(
            payload,
            latest_area_placeables,
            scope,
            module_resource_runtime,
            object_registry,
            rewrite,
        );
    }

    dump_live_object_probe_if_enabled(payload, "server-dispatch-original-probe");

    for translator in SERVER_TO_CLIENT_TRANSLATORS {
        let outcome = if translator.family_name == "GuiQuickbar" {
            translate_quickbar_with_registry(payload, object_registry)
        } else {
            (translator.translate)(
                payload,
                latest_area_placeables,
                scope,
                module_resource_runtime,
            )
        };
        match outcome {
            ServerTranslatorOutcome::None => {}
            ServerTranslatorOutcome::TransportOnly => {
                // Transport-only normalizers may repair a CNW envelope so a
                // later semantic translator can see it, but they never count as
                // ownership. This preserves the strict no-raw-passthrough rule.
            }
            ServerTranslatorOutcome::Claim(claim) => {
                let Some(family) = translator.verified_family else {
                    rewrite.quarantine_reason = Some("claimed-semantic-missing-verified-family");
                    break;
                };
                if !finalize_server_translator_claim(
                    payload,
                    &mut rewrite,
                    translator.family_name,
                    family,
                    claim,
                    object_registry,
                ) {
                    break;
                }
                // A semantic claim is exclusive ownership of this high-level
                // payload. Continue past transport-only repair so normalized
                // CNW envelopes can reach their semantic owner, but stop after
                // the first real family claim. Otherwise later generic probes
                // can mutate or diagnose an already-owned packet, which is how
                // valid ClientSideMessage feedback was being reported as a
                // CNW-fragment transport failure.
                break;
            }
        }
    }

    if matches!(scope, SemanticScope::DeflatedReassembly) && !rewrite.any_rewrite() {
        rewrite.module_info_candidate_offset = module::first_module_info_candidate_offset(payload);
    }

    if !rewrite.any_rewrite() {
        mark_untranslated_semantic_quarantine(payload, scope, &mut rewrite);
    }

    rewrite
}

fn is_live_object_high_level_payload(payload: &[u8]) -> bool {
    matches!(
        (
            payload.get(0).copied(),
            payload.get(1).copied(),
            payload.get(2).copied()
        ),
        (Some(b'P'), Some(0x05), Some(0x01))
    )
}

fn rewrite_live_object_high_level_payload_for_ee(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
    module_resource_runtime: Option<&module_resources::ModuleResourceRuntime>,
    object_registry: Option<&semantic::ObjectRegistry>,
    mut rewrite: InflatedPayloadRewrite,
) -> InflatedPayloadRewrite {
    dump_live_object_probe_if_enabled(payload, "server-dispatch-live-object-family");

    // Live-object is intentionally a family-level strict decision point.  The
    // EE client reaches this reader through `P/05/01`, then consumes a declared
    // read window plus compact CNW fragment bits.  Once that family is known,
    // trying unrelated translators cannot make the packet safer; it only
    // repeats expensive boundary probes and risks turning "unsupported record"
    // into a CPU/log storm.  Each candidate below is still a focused semantic
    // translator, and a miss quarantines the exact payload for decompile work.
    let mut attempts = [
        "GameObjUpdate_LiveObjectPrefixedFragments",
        "GameObjUpdate_LiveObjectExactRecords",
        "GameObjUpdate_LiveObjectCombinedRecords",
        "GameObjUpdate_LiveObjectDeclaredLengthRepair",
    ]
    .into_iter();

    while let Some(family_name) = attempts.next() {
        let outcome = match family_name {
            "GameObjUpdate_LiveObjectPrefixedFragments" => {
                translate_live_object_prefixed_fragments(
                    payload,
                    latest_area_placeables,
                    scope,
                    module_resource_runtime,
                )
            }
            "GameObjUpdate_LiveObjectExactRecords" => translate_live_object_exact_records(
                payload,
                latest_area_placeables,
                scope,
                module_resource_runtime,
            ),
            "GameObjUpdate_LiveObjectCombinedRecords" => translate_live_object_records_if_verified(
                payload,
                latest_area_placeables,
                "live-object-combined-records",
            ),
            "GameObjUpdate_LiveObjectDeclaredLengthRepair" => {
                translate_live_object_declared_length_repair(
                    payload,
                    latest_area_placeables,
                    scope,
                    module_resource_runtime,
                )
            }
            _ => ServerTranslatorOutcome::None,
        };
        match outcome {
            ServerTranslatorOutcome::None | ServerTranslatorOutcome::TransportOnly => {}
            ServerTranslatorOutcome::Claim(claim) => {
                if !finalize_server_translator_claim(
                    payload,
                    &mut rewrite,
                    family_name,
                    VerifiedFamily::GameObjUpdateLiveObject,
                    claim,
                    object_registry,
                ) {
                    return rewrite;
                }
                return rewrite;
            }
        }
    }

    rewrite.quarantine_reason = Some("live-object-unclaimed-strict-family");
    let dump_path =
        dump_unrewritten_semantic_payload(payload, "live-object-unclaimed-strict-family");
    tracing::warn!(
        payload_length = payload.len(),
        dump_path = dump_path.as_deref().unwrap_or(""),
        "server live-object payload quarantined: no focused live-object translator produced an exact EE reader shape"
    );
    rewrite
}

fn finalize_server_translator_claim(
    payload: &mut Vec<u8>,
    rewrite: &mut InflatedPayloadRewrite,
    family_name: &'static str,
    family: VerifiedFamily,
    claim: ServerTranslatorClaim,
    object_registry: Option<&semantic::ObjectRegistry>,
) -> bool {
    if family == VerifiedFamily::GameObjUpdateLiveObject {
        if let Some(registry) = object_registry {
            if let Some(summary) =
                live_update::canonicalize_player_session_creature_ids_payload_for_ee(
                    payload,
                    |compact_id| registry.session_creature_id_for_compact(compact_id),
                )
            {
                tracing::info!(
                    family = family_name,
                    compact_add_ids_observed = summary.compact_add_ids_observed,
                    add_ids_rewritten = summary.add_ids_rewritten,
                    reference_ids_rewritten = summary.reference_ids_rewritten,
                    "server live-object payload canonicalized PlayerList-proven session creature ids for EE"
                );
            }
        }
    }

    if family == VerifiedFamily::GameObjUpdateLiveObject
        && live_update::claim_payload_if_verified_with_lifecycle(
            payload,
            |object_type, object_id| {
                object_registry
                    .map(|registry| {
                        registry.has_active_live_object_for_record(object_type, object_id)
                    })
                    .unwrap_or(false)
            },
        )
        .is_none()
    {
        if let Some(summary) = live_update::remove_unmaterialized_update_records_payload_if_possible(
            payload,
            |object_type, object_id| {
                object_registry
                    .map(|registry| {
                        registry.has_active_live_object_for_record(object_type, object_id)
                    })
                    .unwrap_or(false)
            },
        ) {
            tracing::info!(
                family = family_name,
                old_declared = summary.old_declared,
                new_declared = summary.new_declared,
                removed_update_records = summary.removed_update_records,
                diamond_missing_object_update_records =
                    summary.diamond_missing_object_update_records,
                diamond_missing_object_appearance_records =
                    summary.diamond_missing_object_appearance_records,
                ee_sentinel_inventory_owner_records = summary.ee_sentinel_inventory_owner_records,
                removed_bytes = summary.removed_bytes,
                removed_fragment_bits = summary.removed_fragment_bits,
                "server live-object payload removed Diamond no-op missing-object updates after exact lifecycle proof"
            );
        }
    }

    if family == VerifiedFamily::GameObjUpdateLiveObject
        && live_update::claim_payload_if_verified_with_lifecycle(
            payload,
            |object_type, object_id| {
                object_registry
                    .map(|registry| {
                        registry.has_active_live_object_for_record(object_type, object_id)
                    })
                    .unwrap_or(false)
            },
        )
        .is_none()
    {
        rewrite.quarantine_reason = Some("live-object-lifecycle-unverified");
        let dump_path =
            dump_unrewritten_semantic_payload(payload, "live-object-lifecycle-unverified");
        tracing::warn!(
            family = family_name,
            payload_length = payload.len(),
            dump_path = dump_path.as_deref().unwrap_or(""),
            "server live-object payload quarantined: exact record shape passed but EE lifecycle proof failed"
        );
        return false;
    }

    rewrite.note_rewrite(family_name, family);
    if let Some(area_rewrite) = claim.area_rewrite {
        rewrite.area_rewrite = Some(area_rewrite);
    }
    true
}

fn claimed() -> ServerTranslatorOutcome {
    ServerTranslatorOutcome::Claim(ServerTranslatorClaim::default())
}

fn translate_custom_token(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if custom_token::claim_or_rewrite_payload_if_verified(payload).is_some() {
        claimed()
    } else if crate::strict::module_info_shape_valid(payload) {
        tracing::info!(
            "server Module_Info already matches the EE reader shape; semantic no-op claim retained behind exact strict validation"
        );
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_login(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if login::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_module_time(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if module_time::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_module_end_game(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if module::claim_module_end_game_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_camera(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if camera::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_cutscene(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if cutscene::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_server_status_module_resources(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    module_resource_runtime: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    let Some(module_resource_runtime) = module_resource_runtime else {
        return ServerTranslatorOutcome::None;
    };
    if module_resources::rewrite_server_status_module_resources_payload(
        payload,
        module_resource_runtime,
    )
    .is_some()
    {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_loadbar(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if loadbar::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_gui_timing_event(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if gui_timing_event::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_client_side_message(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if client_side_message::claim_or_rewrite_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_journal(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if journal::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_chat(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if chat::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_sound(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if sound::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_ambient(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if ambient::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_dialog(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if dialog::claim_server_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_inventory(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if inventory::claim_or_rewrite_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_game_obj_update_obj_control(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if game_obj_update::claim_obj_control_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_game_obj_update_vis_effect(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if game_obj_update::claim_vis_effect_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_game_obj_update_destroy_item(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if game_obj_update::claim_destroy_item_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_area_visual_effect(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if area_visual_effect::claim_or_rewrite_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_area_change_day_night(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if area_change_day_night::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_safe_projectile(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if safe_projectile::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_party(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if party::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_play_module_character_list(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if play_module_character_list::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_quickbar(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if quickbar::normalize_and_rewrite_quickbar_payload_if_possible(payload).is_some()
        || quickbar::rewrite_simple_quickbar_payload_if_possible(payload).is_some()
    {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_quickbar_with_registry(
    payload: &mut Vec<u8>,
    object_registry: Option<&semantic::ObjectRegistry>,
) -> ServerTranslatorOutcome {
    let Some(registry) = object_registry else {
        return translate_quickbar(payload, None, SemanticScope::DeflatedReassembly, None);
    };
    let item_object_is_known = |object_id| registry.has_active_object_id(object_id);
    let materialization = quickbar::QuickbarMaterializationContext::new(&item_object_is_known);
    if quickbar::normalize_and_rewrite_quickbar_payload_with_context_if_possible(
        payload,
        Some(&materialization),
    )
    .is_some()
        || quickbar::rewrite_simple_quickbar_payload_with_context_if_possible(
            payload,
            Some(&materialization),
        )
        .is_some()
    {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn normalize_cnw_transport_only(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if cnw_message::normalize_prefixed_fragments_payload_if_needed(payload).is_some() {
        ServerTranslatorOutcome::TransportOnly
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_char_list(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if char_list::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_player_list(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if player_list::rewrite_player_list_payload_if_possible(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_live_object_prefixed_fragments(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if !is_live_object_high_level_payload(payload) {
        return ServerTranslatorOutcome::None;
    }

    let mut candidate = payload.clone();
    let Some(summary) = live_object::normalize_prefixed_fragments_payload_if_needed(&mut candidate)
    else {
        return ServerTranslatorOutcome::None;
    };

    let _ = live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut candidate,
        latest_area_placeables,
    );
    let _ = live_update::rewrite_payload_if_needed(&mut candidate);
    let _ = live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
        &mut candidate,
        latest_area_placeables,
    );
    let _ = live_update::rewrite_payload_if_needed(&mut candidate);
    // Some local Diamond streams alternate legacy `A/9` add and `U/9` update
    // records. A focused update rewrite can shrink/drop a legacy name tail and
    // expose the next add record at its real boundary; that add rewrite can then
    // expose the paired update. Keep this as one more bounded typed add/update
    // pair instead of falling back to raw live-object forwarding.
    let _ = live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
        &mut candidate,
        latest_area_placeables,
    );
    let _ = live_update::rewrite_payload_if_needed(&mut candidate);
    // Update translation can shrink conservative legacy `U` record windows and
    // expose following `A` door/placeable records that were intentionally not
    // split while the update shape was still unproven. Run the same exact
    // add-record translator once more after update finalization; this is not a
    // passthrough fallback, it is the focused add-family owner claiming records
    // that become visible only after the update-family owner has emitted EE.
    let _ = live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
        &mut candidate,
        latest_area_placeables,
    );
    let _ = live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut candidate);
    // Add-name bit repair can expose following records at their final EE bit
    // cursor. Re-run the exact add/update translators after that repair so
    // ownership remains typed instead of treating the adjusted payload as a
    // raw/passive frame.
    let _ = live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
        &mut candidate,
        latest_area_placeables,
    );
    let _ = live_update::rewrite_payload_if_needed(&mut candidate);
    if live_update::claim_payload_if_verified(&candidate).is_some() {
        dump_accepted_live_object_payload_if_enabled(
            &candidate,
            "accepted-live-object-prefixed-fragments",
        );
        *payload = candidate;
        claimed()
    } else {
        tracing::debug!(
            old_payload_length = summary.old_payload_length,
            new_payload_length = summary.new_payload_length,
            old_wire_declared = summary.old_wire_declared,
            new_declared = summary.new_declared,
            live_bytes_offset = summary.live_bytes_offset,
            live_bytes_length = summary.live_bytes_length,
            dropped_leadin_bytes = summary.dropped_leadin_bytes,
            salvaged_partial_leadin = summary.salvaged_partial_leadin,
            "live-object prefixed-fragment candidate did not claim: exact record-boundary validator rejected this intermediate rewrite"
        );
        ServerTranslatorOutcome::None
    }
}

fn translate_live_object_declared_length_repair(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if !is_live_object_high_level_payload(payload) {
        return ServerTranslatorOutcome::None;
    }

    // Decompile-backed transport repair, not a passthrough fallback:
    // EE reaches live-object handling through `P 05 01`, then calls
    // `CNWMessage::SetReadMessage` with the declared byte window and reads the
    // compact BOOL fragment stream from the remaining tail. Some HG/1.69 bursts
    // carry a stale packetized declared value that lands inside an otherwise
    // legal live-object `A/U/W/...` read stream. The candidate search only
    // proposes possible read-window/tail splits; this translator claims a packet
    // only after the focused live-object semantic rewriters and exact validator
    // accept the fully repaired payload.
    if live_object::declared_length_window_transport_plausible(payload) {
        return ServerTranslatorOutcome::None;
    }

    let mut ambiguous_tail_candidates_skipped = 0u32;
    let mut first_ambiguous_tail_repair: Option<
        live_object::LiveObjectDeclaredLengthRepairCandidate,
    > = None;
    let mut last_ambiguous_tail_repair: Option<
        live_object::LiveObjectDeclaredLengthRepairCandidate,
    > = None;
    let mut fragment_capacity_candidates_skipped = 0u32;
    let mut first_fragment_capacity_repair: Option<
        live_object::LiveObjectDeclaredLengthRepairCandidate,
    > = None;
    let mut last_fragment_capacity_repair: Option<
        live_object::LiveObjectDeclaredLengthRepairCandidate,
    > = None;

    if let Some(repair) =
        live_object::declared_length_repair_creature_appearance_update_read_split_candidate(payload)
    {
        if let Some((candidate, claim, changed_by_semantic_rewrite)) =
            build_declared_length_repaired_live_object_candidate(
                payload,
                &repair,
                latest_area_placeables,
            )
        {
            if claim.add_records > 0
                && claim.creature_appearance_records > 0
                && claim.creature_update_records > 0
            {
                tracing::info!(
                    old_declared = repair.old_declared,
                    repaired_declared = repair.new_declared,
                    old_payload_length = repair.old_payload_length,
                    read_bytes = repair.read_bytes_length,
                    fragment_bytes = repair.fragment_bytes_length,
                    add_records = claim.add_records,
                    creature_appearance_records = claim.creature_appearance_records,
                    creature_update_records = claim.creature_update_records,
                    changed_by_semantic_rewrite,
                    exact_claim = true,
                    "server live-object declared length repaired by exact creature appearance/update proof despite ambiguous fragment-tail bytes"
                );
                *payload = candidate;
                return claimed();
            }
        }
    }

    for repair in live_object::declared_length_repair_candidates(payload) {
        if repair.old_declared == repair.new_declared {
            continue;
        }
        let source_fragment_capacity_plausible =
            live_object::declared_length_repair_fragment_capacity_plausible(payload, &repair);
        let ambiguous_live_tail =
            live_object::declared_length_repair_tail_contains_live_object_read_boundary(
                payload, &repair,
            );
        if ambiguous_live_tail {
            ambiguous_tail_candidates_skipped = ambiguous_tail_candidates_skipped.saturating_add(1);
            if first_ambiguous_tail_repair.is_none() {
                first_ambiguous_tail_repair = Some(repair.clone());
            }
            last_ambiguous_tail_repair = Some(repair);
            continue;
        }
        if !source_fragment_capacity_plausible && latest_area_placeables.is_none() {
            fragment_capacity_candidates_skipped =
                fragment_capacity_candidates_skipped.saturating_add(1);
            if first_fragment_capacity_repair.is_none() {
                first_fragment_capacity_repair = Some(repair.clone());
            }
            last_fragment_capacity_repair = Some(repair);
            continue;
        }

        if let Some((candidate, _claim, changed_by_semantic_rewrite)) =
            build_declared_length_repaired_live_object_candidate(
                payload,
                &repair,
                latest_area_placeables,
            )
        {
            tracing::info!(
                old_declared = repair.old_declared,
                repaired_declared = repair.new_declared,
                old_payload_length = repair.old_payload_length,
                read_bytes = repair.read_bytes_length,
                fragment_bytes = repair.fragment_bytes_length,
                source_fragment_capacity_preflight = source_fragment_capacity_plausible,
                changed_by_semantic_rewrite,
                exact_claim = true,
                "server live-object declared length repaired by exact semantic proof in dispatch"
            );
            *payload = candidate;
            return claimed();
        }
        if !source_fragment_capacity_plausible {
            // The source-side capacity walk is a useful fast rejection for false
            // splits. Area-backed placeable add rewrites can legitimately change
            // fragment ownership before exact EE validation, so those candidates
            // are allowed one bounded semantic proof attempt before being counted
            // as skipped.
            fragment_capacity_candidates_skipped =
                fragment_capacity_candidates_skipped.saturating_add(1);
            if first_fragment_capacity_repair.is_none() {
                first_fragment_capacity_repair = Some(repair.clone());
            }
            last_fragment_capacity_repair = Some(repair);
        }
    }

    if ambiguous_tail_candidates_skipped > 0 {
        let first = first_ambiguous_tail_repair.as_ref();
        let last = last_ambiguous_tail_repair.as_ref();
        tracing::debug!(
            candidates_skipped = ambiguous_tail_candidates_skipped,
            old_declared = first.map(|repair| repair.old_declared).unwrap_or_default(),
            first_repaired_declared = first.map(|repair| repair.new_declared).unwrap_or_default(),
            first_read_bytes = first
                .map(|repair| repair.read_bytes_length)
                .unwrap_or_default(),
            first_fragment_bytes = first
                .map(|repair| repair.fragment_bytes_length)
                .unwrap_or_default(),
            last_repaired_declared = last.map(|repair| repair.new_declared).unwrap_or_default(),
            last_read_bytes = last
                .map(|repair| repair.read_bytes_length)
                .unwrap_or_default(),
            last_fragment_bytes = last
                .map(|repair| repair.fragment_bytes_length)
                .unwrap_or_default(),
            "live-object declared-length repair skipped ambiguous splits whose fragment tails still contain plausible live-object read boundaries"
        );
    }

    if fragment_capacity_candidates_skipped > 0 {
        let first = first_fragment_capacity_repair.as_ref();
        let last = last_fragment_capacity_repair.as_ref();
        tracing::debug!(
            candidates_skipped = fragment_capacity_candidates_skipped,
            old_declared = first.map(|repair| repair.old_declared).unwrap_or_default(),
            first_repaired_declared = first.map(|repair| repair.new_declared).unwrap_or_default(),
            first_read_bytes = first
                .map(|repair| repair.read_bytes_length)
                .unwrap_or_default(),
            first_fragment_bytes = first
                .map(|repair| repair.fragment_bytes_length)
                .unwrap_or_default(),
            last_repaired_declared = last.map(|repair| repair.new_declared).unwrap_or_default(),
            last_read_bytes = last
                .map(|repair| repair.read_bytes_length)
                .unwrap_or_default(),
            last_fragment_bytes = last
                .map(|repair| repair.fragment_bytes_length)
                .unwrap_or_default(),
            "live-object declared-length repair skipped splits whose fragment tails cannot supply the typed legacy read prefix"
        );
    }

    ServerTranslatorOutcome::None
}

fn build_declared_length_repaired_live_object_candidate(
    payload: &[u8],
    repair: &live_object::LiveObjectDeclaredLengthRepairCandidate,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
) -> Option<(Vec<u8>, live_update::ClaimSummary, bool)> {
    let mut candidate = payload.to_vec();
    let declared_slot = candidate.get_mut(3..7)?;
    declared_slot.copy_from_slice(&repair.new_declared.to_le_bytes());

    let changed_by_semantic_rewrite = live_update::rewrite_payload_to_exact_ee_if_possible(
        &mut candidate,
        latest_area_placeables,
    )
    .is_some();
    let claim = live_update::claim_payload_if_verified(&candidate)?;
    Some((candidate, claim, changed_by_semantic_rewrite))
}

fn translate_live_object_add_records(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    translate_live_object_records_if_verified(
        payload,
        latest_area_placeables,
        "live-object-add-records",
    )
}

fn translate_live_object_exact_records(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if !is_live_object_high_level_payload(payload) {
        return ServerTranslatorOutcome::None;
    }

    let mut candidate = payload.clone();
    if let Some(summary) =
        live_update::canonicalize_compact_external_object_ids_payload_for_ee(&mut candidate)
    {
        if live_update::claim_payload_if_verified(&candidate).is_some() {
            tracing::info!(
                compact_add_ids_observed = summary.compact_add_ids_observed,
                add_ids_rewritten = summary.add_ids_rewritten,
                reference_ids_rewritten = summary.reference_ids_rewritten,
                "server live-object exact payload canonicalized Diamond compact external object ids for EE"
            );
            dump_accepted_live_object_payload_if_enabled(
                &candidate,
                "accepted-live-object-exact-records-canonicalized-object-ids",
            );
            *payload = candidate;
            return claimed();
        }
    }
    if live_update::claim_payload_if_verified(payload).is_some() {
        dump_accepted_live_object_payload_if_enabled(payload, "accepted-live-object-exact-records");
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_live_object_records_if_verified(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    source: &'static str,
) -> ServerTranslatorOutcome {
    if !is_live_object_high_level_payload(payload) {
        return ServerTranslatorOutcome::None;
    }

    dump_live_object_probe_if_enabled(payload, source);

    let mut candidate = payload.clone();
    if let Some(summary) =
        live_update::rewrite_payload_to_exact_ee_if_possible(&mut candidate, latest_area_placeables)
    {
        let external_object_id_summary =
            live_update::canonicalize_compact_external_object_ids_payload_for_ee(&mut candidate);
        if live_update::claim_payload_if_verified(&candidate).is_some() {
            tracing::info!(
                source,
                update_passes_changed = summary.update_passes_changed,
                add_passes_changed = summary.add_passes_changed,
                add_name_bit_passes_changed = summary.add_name_bit_passes_changed,
                "server live-object payload reached exact EE shape through bounded typed orchestrator"
            );
            if let Some(summary) = external_object_id_summary {
                tracing::info!(
                    source,
                    compact_add_ids_observed = summary.compact_add_ids_observed,
                    add_ids_rewritten = summary.add_ids_rewritten,
                    reference_ids_rewritten = summary.reference_ids_rewritten,
                    "server live-object payload canonicalized Diamond compact external object ids for EE"
                );
            }
            dump_accepted_live_object_payload_if_enabled(&candidate, source);
            *payload = candidate;
            return claimed();
        }
    }

    let mut candidate = payload.clone();
    let add_before_update_summary =
        live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut candidate,
            latest_area_placeables,
        );
    let update_pre_summary = live_update::rewrite_payload_if_needed(&mut candidate);
    let add_summary =
        live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
            &mut candidate,
            latest_area_placeables,
        );
    let update_post_summary = live_update::rewrite_payload_if_needed(&mut candidate);
    let add_name_bit_summary =
        live_update::rewrite_add_name_fragment_bits_payload_if_possible(&mut candidate);
    let add_after_add_name_summary =
        live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
            &mut candidate,
            latest_area_placeables,
        );
    let update_after_add_name_summary = live_update::rewrite_payload_if_needed(&mut candidate);
    let add_after_update_summary =
        live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
            &mut candidate,
            latest_area_placeables,
        );
    let update_after_final_add_summary = live_update::rewrite_payload_if_needed(&mut candidate);
    let add_after_final_update_summary =
        live_object::rewrite_creature_add_visual_transform_maps_after_update_if_possible(
            &mut candidate,
            latest_area_placeables,
        );
    let update_after_second_final_add_summary =
        live_update::rewrite_payload_if_needed(&mut candidate);
    let external_object_id_summary =
        live_update::canonicalize_compact_external_object_ids_payload_for_ee(&mut candidate);

    if add_summary.is_none()
        && add_before_update_summary.is_none()
        && update_pre_summary.is_none()
        && update_post_summary.is_none()
        && add_name_bit_summary.is_none()
        && add_after_add_name_summary.is_none()
        && update_after_add_name_summary.is_none()
        && add_after_update_summary.is_none()
        && update_after_final_add_summary.is_none()
        && add_after_final_update_summary.is_none()
        && update_after_second_final_add_summary.is_none()
        && external_object_id_summary.is_none()
    {
        crate::translate::live_object_update::dump_live_object_fixture_candidate(
            &candidate, source,
        );
        return ServerTranslatorOutcome::None;
    }

    if live_update::claim_payload_if_verified(&candidate).is_some() {
        if let Some(summary) = external_object_id_summary {
            tracing::info!(
                source,
                compact_add_ids_observed = summary.compact_add_ids_observed,
                add_ids_rewritten = summary.add_ids_rewritten,
                reference_ids_rewritten = summary.reference_ids_rewritten,
                "server live-object payload canonicalized Diamond compact external object ids for EE"
            );
        }
        dump_accepted_live_object_payload_if_enabled(&candidate, source);
        *payload = candidate;
        claimed()
    } else {
        crate::translate::live_object_update::dump_live_object_fixture_candidate(
            &candidate,
            "live-object-semantic-candidate-rejected-exact-validator",
        );
        let (add_records_examined, maps_inserted, add_bytes_inserted, add_bytes_removed) = [
            add_before_update_summary.as_ref(),
            add_summary.as_ref(),
            add_after_add_name_summary.as_ref(),
            add_after_update_summary.as_ref(),
            add_after_final_update_summary.as_ref(),
        ]
        .into_iter()
        .flatten()
        .fold((0u32, 0u32, 0u32, 0u32), |acc, summary| {
            (
                acc.0.saturating_add(summary.records_examined),
                acc.1.saturating_add(summary.maps_inserted),
                acc.2.saturating_add(summary.bytes_inserted),
                acc.3.saturating_add(summary.bytes_removed),
            )
        });
        let (
            update_records_examined,
            update_records_rewritten,
            update_bytes_inserted,
            update_bytes_removed,
        ) = [
            update_pre_summary.as_ref(),
            update_post_summary.as_ref(),
            update_after_add_name_summary.as_ref(),
            update_after_final_add_summary.as_ref(),
            update_after_second_final_add_summary.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|summary| {
            (
                summary.update_records_examined,
                summary.update_records_rewritten,
                summary.bytes_inserted,
                summary.bytes_removed,
            )
        })
        .fold((0u32, 0u32, 0u32, 0u32), |acc, summary| {
            (
                acc.0.saturating_add(summary.0),
                acc.1.saturating_add(summary.1),
                acc.2.saturating_add(summary.2),
                acc.3.saturating_add(summary.3),
            )
        });
        tracing::debug!(
            source,
            add_changed = add_before_update_summary.is_some()
                || add_summary.is_some()
                || add_after_add_name_summary.is_some()
                || add_after_update_summary.is_some()
                || add_after_final_update_summary.is_some()
                || add_name_bit_summary.is_some(),
            update_changed = update_pre_summary.is_some()
                || update_post_summary.is_some()
                || update_after_add_name_summary.is_some()
                || update_after_final_add_summary.is_some()
                || update_after_second_final_add_summary.is_some(),
            add_records_examined,
            maps_inserted,
            add_bytes_inserted,
            add_bytes_removed,
            update_records_examined,
            update_records_rewritten,
            update_bytes_inserted,
            update_bytes_removed,
            "live-object semantic candidate did not claim: exact record-boundary validator rejected this intermediate rewrite"
        );
        ServerTranslatorOutcome::None
    }
}

fn dump_accepted_live_object_payload_if_enabled(payload: &[u8], source: &str) {
    if std::env::var_os("HGBRIDGE_PROXY2_DUMP_ACCEPTED_LIVE_OBJECT").is_none() {
        return;
    }
    crate::translate::live_object_update::dump_live_object_fixture_candidate(payload, source);
}

fn dump_live_object_probe_if_enabled(payload: &[u8], source: &str) {
    if std::env::var_os("HGBRIDGE_PROXY2_DUMP_LIVE_OBJECT_PROBES").is_none() {
        return;
    }
    if payload.len() > 2048
        || payload.get(0).copied() != Some(b'P')
        || payload.get(1).copied() != Some(0x05)
        || payload.get(2).copied() != Some(0x01)
    {
        return;
    }
    crate::translate::live_object_update::dump_live_object_fixture_candidate(
        payload,
        &format!("{source}-original-probe"),
    );
}

fn translate_live_object_update_records(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    translate_live_object_records_if_verified(
        payload,
        latest_area_placeables,
        "live-object-update-records",
    )
}

fn translate_live_object_claimed_records(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    _: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    if !is_live_object_high_level_payload(payload) {
        return ServerTranslatorOutcome::None;
    }

    if crate::translate::live_object_update::payload_contains_door_or_placeable_add_update_record(
        payload,
    ) {
        crate::translate::live_object_update::dump_live_object_fixture_candidate(
            payload,
            "live-object-claimed-records-rejected-door-placeable-requires-translator",
        );
        return ServerTranslatorOutcome::None;
    }

    if live_update::claim_payload_if_verified(payload).is_some() {
        crate::translate::live_object_update::dump_live_object_fixture_candidate(
            payload,
            "live-object-claimed-records-noop-semantic-claim",
        );
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_area_client_area(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
    module_resource_runtime: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    // `Area_ClientArea` is a semantic CNW payload; the reliable-window
    // transport may carry it either as the whole deflated reassembly or as a
    // deflated primary record inside a coalesced M datagram. The EE/Diamond
    // sender/reader shape is the same after the M layer inflates it, so the
    // area translator owns both scopes instead of letting coalescing decide
    // packet validity.
    let _ = scope;
    let observed_module_context =
        module_resource_runtime.and_then(|runtime| runtime.observed_module_context());
    tracing::debug!(
        has_observed_module_context = observed_module_context.is_some(),
        "server Area_ClientArea translator checking runtime Module_Info context"
    );
    match area::rewrite_area_client_area_payload_with_module_context(
        payload,
        observed_module_context.as_ref(),
    ) {
        Some(summary) => ServerTranslatorOutcome::Claim(ServerTranslatorClaim {
            area_rewrite: Some(summary),
        }),
        None => ServerTranslatorOutcome::None,
    }
}

fn translate_module_info(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
    module_resource_runtime: Option<&module_resources::ModuleResourceRuntime>,
) -> ServerTranslatorOutcome {
    let mut candidate = payload.clone();
    if let Some(summary) = module::rewrite_module_info_payload(&mut candidate) {
        if let Some(runtime) = module_resource_runtime {
            if !runtime.observe_legacy_module_info_resources(
                &summary.hak_order_top_first,
                summary.custom_tlk.as_deref(),
            ) {
                tracing::warn!(
                    hak_count = summary.hak_count,
                    hak_order_top_first = ?summary.hak_order_top_first,
                    custom_tlk = summary.custom_tlk.as_deref().unwrap_or(""),
                    "server Module_Info resource declaration was not accepted by runtime"
                );
                return ServerTranslatorOutcome::None;
            }
            if let Some(context) = summary.observed_context.clone() {
                if !runtime.observe_module_context(context) {
                    tracing::warn!(
                        "server Module_Info observed module context was not accepted by runtime"
                    );
                    return ServerTranslatorOutcome::None;
                }
                tracing::debug!("server Module_Info observed module context recorded in runtime");
            }
        }
        *payload = candidate;
        tracing::info!(
            hak_count = summary.hak_count,
            hak_order_top_first = ?summary.hak_order_top_first,
            custom_tlk = summary.custom_tlk.as_deref().unwrap_or(""),
            custom_tlk_converted_to_resref = summary.custom_tlk_converted_to_resref,
            module_resref = summary.module_resref.as_deref().unwrap_or(""),
            "server Module_Info legacy resource declaration recorded for EE module resources"
        );
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}
fn mark_untranslated_semantic_quarantine(
    payload: &[u8],
    scope: SemanticScope,
    rewrite: &mut InflatedPayloadRewrite,
) {
    if rewrite.quarantine_reason.is_some() {
        return;
    }
    let Some(high) = HighLevel::parse(payload) else {
        return;
    };
    let reason = untranslated_semantic_quarantine_reason(high);

    rewrite.quarantine_reason = Some(reason);
    let dump_path = dump_unrewritten_semantic_payload(payload, reason);
    let prefix = hex_prefix(payload, 128);
    tracing::warn!(
        scope = ?scope,
        reason,
        family = high.name(),
        major = high.major,
        minor = high.minor,
        payload_length = payload.len(),
        dump_path = dump_path.as_deref().unwrap_or(""),
        prefix = %prefix,
        "server high-level payload quarantined: semantic translator did not claim required family"
    );
}

fn untranslated_semantic_quarantine_reason(high: HighLevel) -> &'static str {
    // Strict bridge discipline: `HighLevel::is_known()` is only a classifier,
    // never an allow decision. A server-to-client gameplay payload may be
    // emitted only after a focused semantic translator has claimed it. Packets
    // whose opcode we have never seen are quarantined by the same rule instead
    // of getting a hidden "unknown passthrough" path.
    if high.is_known() {
        "unclaimed-known-high-level"
    } else {
        "unclaimed-unknown-high-level"
    }
}

fn dump_unrewritten_semantic_payload(payload: &[u8], reason: &str) -> Option<String> {
    let mut path = crate::translate::diagnostics::diagnostic_dump_dir()?;
    fs::create_dir_all(&path).ok()?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    path.push(format!("{reason}-{nanos}.bin"));
    fs::write(&path, payload).ok()?;
    Some(path.to_string_lossy().into_owned())
}

fn hex_prefix(bytes: &[u8], limit: usize) -> String {
    let mut out = String::new();
    for (index, byte) in bytes.iter().take(limit).enumerate() {
        if index != 0 {
            out.push(' ');
        }
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

pub(super) fn log_deflated_semantic_rewrite(
    rewrite: &InflatedPayloadRewrite,
    context: DeflatedSemanticLogContext,
) {
    if rewrite.any_rewrite() {
        tracing::info!(
            frames = context.frames,
            first_sequence = context.first_sequence,
            packetized_sequence = context.packetized_sequence,
            inflated = context.old_inflated_length,
            rewritten_inflated = context.rewritten_inflated_length,
            compressed = context.compressed_length,
            families = ?rewrite.family_names,
            used_server_stream = context.used_server_stream,
            proxy_owned_stream = context.proxy_owned_stream,
            "server deflated M semantic payload rewritten for EE"
        );
    } else {
        tracing::info!(
            frames = context.frames,
            first_sequence = context.first_sequence,
            packetized_sequence = context.packetized_sequence,
            inflated = context.old_inflated_length,
            compressed = context.compressed_length,
            proxy_owned_stream = context.proxy_owned_stream,
            "server deflated M stream converted to EE one-shot zlib"
        );
    }
}

pub(super) fn inflated_cnw_fragment_offset_valid_or_normalizable(inflated: &[u8]) -> bool {
    if super::inflated_cnw_fragment_offset_valid(inflated) {
        return true;
    }
    let mut probe = inflated.to_vec();
    if client_side_message::claim_or_rewrite_payload_if_verified(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if custom_token::rewrite_payload_if_possible(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if player_list::rewrite_player_list_payload_if_possible(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if quickbar::normalize_and_rewrite_quickbar_payload_if_possible(&mut probe).is_some()
        || quickbar::rewrite_simple_quickbar_payload_if_possible(&mut probe).is_some()
    {
        return true;
    }
    let mut probe = inflated.to_vec();
    live_object::normalize_prefixed_fragments_payload_if_needed(&mut probe).is_some()
}

pub(super) fn rewrite_direct_frame_if_needed(
    bytes: &[u8],
    view: &MFrameView,
    module_resource_runtime: &module_resources::ModuleResourceRuntime,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    object_registry: Option<&semantic::ObjectRegistry>,
) -> anyhow::Result<Option<VerifiedPacket>> {
    if let Some(rewritten) = rewrite_server_status_module_resources_frame_if_needed(
        bytes,
        view,
        module_resource_runtime,
    )? {
        return Ok(Some(VerifiedPacket {
            proof: VerifiedProof::family(VerifiedFamily::ServerStatusModuleResources),
            packet: rewritten,
        }));
    }
    if let Some(consumed) = consume_deferred_server_status_module_running_frame_if_needed(
        bytes,
        view,
        module_resource_runtime,
    )? {
        return Ok(Some(VerifiedPacket {
            proof: VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
            packet: consumed,
        }));
    }
    // Keep direct `GameObjUpdate_LiveObject` frames on the same strict path as
    // deflated/coalesced gameplay payloads. A legacy live-object frame can mix
    // compact `A` add records with translated `U` update records; update-only
    // repair is not semantic ownership of the whole packet. The registry below
    // must prove add-record visual transforms, update masks, fragment bits, and
    // the final exact validator before this M frame is emitted.
    if let Some(high) = view.high {
        if let Some(verified) = rewrite_direct_semantic_frame_if_claimed(
            bytes,
            view,
            high,
            module_resource_runtime,
            latest_area_placeables,
            object_registry,
        )? {
            return Ok(Some(verified));
        }
        let reason = untranslated_semantic_quarantine_reason(high);
        return consume_untranslated_direct_frame(bytes, view, high, reason).map(|packet| {
            Some(VerifiedPacket {
                proof: VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
                packet,
            })
        });
    }
    Ok(None)
}

fn rewrite_direct_semantic_frame_if_claimed(
    bytes: &[u8],
    view: &MFrameView,
    high: HighLevel,
    module_resource_runtime: &module_resources::ModuleResourceRuntime,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    object_registry: Option<&semantic::ObjectRegistry>,
) -> anyhow::Result<Option<VerifiedPacket>> {
    let Some(payload) = parse_window::primary_payload(bytes, view) else {
        return Ok(None);
    };
    let mut rewritten_payload = payload.to_vec();
    let semantic_rewrite_summary = rewrite_inflated_payload_for_ee(
        &mut rewritten_payload,
        latest_area_placeables,
        SemanticScope::CoalescedSpan,
        Some(module_resource_runtime),
        object_registry,
        None,
    );
    if semantic_rewrite_summary.should_quarantine() || !semantic_rewrite_summary.any_rewrite() {
        return Ok(None);
    }

    let rewritten = parse_window::replace_primary_payload_and_repair(
        bytes,
        view,
        &rewritten_payload,
        "direct semantic high-level payload",
    )?;
    let verified_family = semantic_rewrite_summary.verified_family();
    tracing::info!(
        family = high.name(),
        verified_family = verified_family.as_str(),
        major = high.major,
        minor = high.minor,
        sequence = view.sequence,
        old_payload_length = payload.len(),
        new_payload_length = rewritten_payload.len(),
        "server direct M high-level payload semantically claimed for EE"
    );
    Ok(Some(VerifiedPacket {
        proof: semantic_rewrite_summary.verified_proof(),
        packet: rewritten,
    }))
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod live_object_dispatch_tests {
    use super::*;

    #[test]
    fn declared_length_repair_claims_stale_live_object_fixture() {
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/docks_placeable_boards_stale_declared.bin"
        )
        .to_vec();
        let area_context = area::AreaPlaceableContext {
            area_resref: "zdl_docks".to_string(),
            static_rows: vec![area::AreaPlaceableContextRow {
                object_id: 0x8000_35C8,
                appearance: 0x0E60,
                x: 89.0,
                y: 9.0,
                z: 0.8,
                dir_x: 0.0,
                dir_y: -1.0,
                dir_z: 0.0,
                has_direction: true,
            }],
            light_rows: Vec::new(),
        };

        let outcome = translate_live_object_declared_length_repair(
            &mut payload,
            Some(&area_context),
            SemanticScope::CoalescedSpan,
            None,
        );

        assert!(matches!(outcome, ServerTranslatorOutcome::Claim(_)));
        let claim = live_update::claim_payload_if_verified(&payload)
            .expect("declared-length repaired payload should be exact live-object shape");
        let emitted_declared = u32::from_le_bytes(payload[3..7].try_into().unwrap()) as usize;
        assert_eq!(claim.declared, emitted_declared);
        assert!(claim.add_records > 0);
        assert!(claim.update_records > 0);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn declared_length_repair_claims_cepv22_full_stream_without_stranding_live_tail() {
        for (fixture_name, original) in [
            (
                "local_cepv22_builder_seq13_declared95_stream_20260520",
                include_bytes!(
                "../../../fixtures/live_object/local_cepv22_builder_seq13_declared95_stream_20260520.bin"
            )
                .as_slice(),
            ),
            (
                "local_cepv22_seq14_creature_burst_declared32_20260520",
                include_bytes!(
                "../../../fixtures/live_object/local_cepv22_seq14_creature_burst_declared32_20260520.bin"
            )
                .as_slice(),
            ),
            (
                "local_cepv22_builder_seq15_declared112_stream_20260520",
                include_bytes!(
                "../../../fixtures/live_object/local_cepv22_builder_seq15_declared112_stream_20260520.bin"
            )
                .as_slice(),
            ),
        ] {
            // Local CEP v2.2 builder captures from 2026-05-20. The declared
            // window is stale, and later `P/U` live-object records are still
            // present in the suffix. Declared-length repair must claim only the
            // full typed stream, never a short rewritten prefix that strands
            // those records as CNW fragment storage.
            let original = original.to_vec();
            let mut payload = original.clone();
            let old_declared = u32::from_le_bytes(original[3..7].try_into().unwrap()) as usize;
            assert!(
                old_declared < original.len(),
                "fixture should exercise a stale declared live-object window"
            );
            let candidates = live_object::declared_length_repair_candidates(&original);
            assert!(
                candidates.iter().any(|candidate| {
                    !live_object::declared_length_repair_tail_contains_live_object_read_boundary(
                        &original, candidate,
                    )
                }),
                "candidate search should include at least one split that does not strand later live-object records"
            );

            let outcome = translate_live_object_declared_length_repair(
                &mut payload,
                None,
                SemanticScope::CoalescedSpan,
                None,
            );

            assert!(
                matches!(outcome, ServerTranslatorOutcome::Claim(_)),
                "{fixture_name} should claim"
            );
            assert_ne!(payload, original);
            let claim = live_update::claim_payload_if_verified(&payload)
                .expect("CEP v2.2 full stream should rewrite to exact EE live-object shape");
            assert!(claim.add_records >= 1);
            assert!(claim.creature_appearance_records >= 1);
            assert!(claim.creature_update_records >= 1);
            assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
            assert!(claim.declared > old_declared);

            if fixture_name == "local_cepv22_builder_seq13_declared95_stream_20260520" {
                assert!(
                    live_update::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
                        .is_none(),
                    "seq13 should contain an exact Diamond missing-object no-op before lifecycle cleanup"
                );
                let cleanup = live_update::remove_unmaterialized_update_records_payload_if_possible(
                    &mut payload,
                    |_, _| false,
                )
                .expect("seq13 missing-object P/U pair should be removable after exact proof");
                assert_eq!(cleanup.removed_update_records, 2);
                assert_eq!(cleanup.diamond_missing_object_appearance_records, 1);
                assert_eq!(cleanup.diamond_missing_object_update_records, 1);
                live_update::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
                    .expect("seq13 should be exact and lifecycle-safe after paired cleanup");
            }
        }
    }

    #[test]
    fn declared_length_repair_claims_xp2_4408_inventory_stream() {
        // Local XP2 Chapter 1 harness capture from 2026-05-22.  The packet has
        // a stale declared window in a compact `U/5 0x4408` current-player
        // inventory burst; the repair must prove the full read-window/tail
        // split instead of accepting a short prefix that strands later `U/5`
        // records as fragment storage.
        let original = include_bytes!(
            "../../../fixtures/live_object/local_xp2_seq27_4408_live_object_20260522_unclaimed.bin"
        )
        .to_vec();
        let mut payload = original.clone();

        let outcome = translate_live_object_declared_length_repair(
            &mut payload,
            None,
            SemanticScope::CoalescedSpan,
            None,
        );

        assert!(matches!(outcome, ServerTranslatorOutcome::Claim(_)));
        assert_ne!(payload, original);
        let claim = live_update::claim_payload_if_verified(&payload)
            .expect("XP2 0x4408 inventory burst should rewrite to exact EE live-object shape");
        assert!(claim.creature_update_records >= 1);
        assert!(claim.inventory_records >= 1);
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[test]
    fn live_object_translators_ignore_non_live_high_level_payloads() {
        for original in [
            vec![b'P', 0x03, 0x01, 0x00, 0x00, 0x00, 0x00],
            vec![b'P', 0x04, 0x01, 0x00, 0x00, 0x00, 0x00],
        ] {
            let mut payload = original.clone();
            assert!(matches!(
                translate_live_object_prefixed_fragments(
                    &mut payload,
                    None,
                    SemanticScope::CoalescedSpan,
                    None
                ),
                ServerTranslatorOutcome::None
            ));
            assert_eq!(payload, original);

            assert!(matches!(
                translate_live_object_exact_records(
                    &mut payload,
                    None,
                    SemanticScope::CoalescedSpan,
                    None
                ),
                ServerTranslatorOutcome::None
            ));
            assert_eq!(payload, original);

            assert!(matches!(
                translate_live_object_add_records(
                    &mut payload,
                    None,
                    SemanticScope::CoalescedSpan,
                    None
                ),
                ServerTranslatorOutcome::None
            ));
            assert_eq!(payload, original);

            assert!(matches!(
                translate_live_object_update_records(
                    &mut payload,
                    None,
                    SemanticScope::CoalescedSpan,
                    None
                ),
                ServerTranslatorOutcome::None
            ));
            assert_eq!(payload, original);

            assert!(matches!(
                translate_live_object_claimed_records(
                    &mut payload,
                    None,
                    SemanticScope::CoalescedSpan,
                    None
                ),
                ServerTranslatorOutcome::None
            ));
            assert_eq!(payload, original);

            assert!(matches!(
                translate_live_object_declared_length_repair(
                    &mut payload,
                    None,
                    SemanticScope::CoalescedSpan,
                    None
                ),
                ServerTranslatorOutcome::None
            ));
            assert_eq!(payload, original);
        }
    }
}

fn consume_untranslated_direct_frame(
    bytes: &[u8],
    view: &MFrameView,
    high: HighLevel,
    reason: &'static str,
) -> anyhow::Result<Vec<u8>> {
    let rewritten = build_consumed_empty_direct_frame(bytes, view)?;

    tracing::warn!(
        reason,
        family = high.name(),
        major = high.major,
        minor = high.minor,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        flags = view.flags,
        packetized_sequence = view.packetized_sequence,
        "server direct M frame quarantined: semantic translator did not claim required family"
    );

    Ok(rewritten)
}

fn consume_deferred_server_status_module_running_frame_if_needed(
    bytes: &[u8],
    view: &MFrameView,
    module_resource_runtime: &module_resources::ModuleResourceRuntime,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(high) = view.high else {
        return Ok(None);
    };
    if high.major != 0x01 || high.minor != 0x03 || view.payload_length == 0 {
        return Ok(None);
    }

    let Some(payload) = parse_window::primary_payload(bytes, view) else {
        return Ok(None);
    };

    // If runtime already has Module_Info's HAK/TLK declaration, the immediate
    // ServerStatus_ModuleResources translator above should own and emit this
    // packet. This path is only for the startup gap where the focused deferred
    // module has captured the legacy short status shape and will emit a verified
    // EE resource packet after Module_Info arrives.
    let mut immediate_probe = payload.to_vec();
    if module_resources::rewrite_server_status_module_resources_payload(
        &mut immediate_probe,
        module_resource_runtime,
    )
    .is_some()
    {
        return Ok(None);
    }

    let Some(shape) = deferred_module_resources::LegacyStatusShape::parse(payload) else {
        return Ok(None);
    };

    let rewritten = build_consumed_empty_direct_frame(bytes, view)?;
    tracing::info!(
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        declared = shape.declared,
        status_string_len = shape.status_string_len,
        fragment_tail_len = shape.fragment_tail_len,
        "server ServerStatus_ModuleRunning consumed as verified deferred module-resource status"
    );
    Ok(Some(rewritten))
}

fn build_consumed_empty_direct_frame(bytes: &[u8], view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume extended-length direct M frame yet");
    }

    let mut rewritten = bytes.to_vec();
    rewritten.truncate(crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if rewritten.len() > 7 {
        // This is a semantic consumption shell, not a packetized payload. Keep
        // the sequence/ack bytes intact, but clear stream/packetized/deflate
        // delivery bits before setting the payload length to zero.
        rewritten[7] &= !0x07;
    }
    write_be_u16(&mut rewritten, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear consumed direct M payload length"))?;
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair consumed direct M CRC"))?;
    Ok(rewritten)
}

fn rewrite_server_status_module_resources_frame_if_needed(
    bytes: &[u8],
    view: &MFrameView,
    module_resource_runtime: &module_resources::ModuleResourceRuntime,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(high) = view.high else {
        return Ok(None);
    };
    if high.major != 0x01 || high.minor != 0x03 || view.payload_length == 0 {
        return Ok(None);
    }

    let Some(payload) = parse_window::primary_payload(bytes, view) else {
        return Ok(None);
    };
    let mut rewritten_payload = payload.to_vec();
    let Some(summary) = module_resources::rewrite_server_status_module_resources_payload(
        &mut rewritten_payload,
        module_resource_runtime,
    ) else {
        return Ok(None);
    };
    let rewritten = parse_window::replace_primary_payload_and_repair(
        bytes,
        view,
        &rewritten_payload,
        "ServerStatus_ModuleRunning module resources",
    )?;

    tracing::info!(
        old_declared = summary.old_declared,
        new_declared = summary.new_declared,
        old_payload_length = summary.old_payload_length,
        new_payload_length = summary.new_payload_length,
        status_module_name = %summary.status_module_name,
        profile_name = summary.profile_name,
        resource_source = summary.resource_source,
        custom_tlk = summary.custom_tlk.as_deref().unwrap_or(""),
        hak_count = summary.hak_count,
        nwsync_advertised = summary.nwsync_advertised,
        "server ServerStatus_ModuleRunning module resources rewritten for EE"
    );
    Ok(Some(rewritten))
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    fn dispatch_live_object_fixture(payload: &mut Vec<u8>) -> InflatedPayloadRewrite {
        rewrite_single_inflated_payload_for_ee(
            payload,
            None,
            SemanticScope::DeflatedReassembly,
            None,
            None,
            None,
        )
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_cepv23_declared_zero_module_info() {
        let mut payload = include_bytes!(
            "../../../fixtures/module_info/local_cepv23_declared_zero_hak_module_info_20260520.bin"
        )
        .to_vec();
        let runtime = module_resources::ModuleResourceRuntime::default();

        let rewrite = rewrite_single_inflated_payload_for_ee(
            &mut payload,
            None,
            SemanticScope::CoalescedSpan,
            Some(&runtime),
            None,
            None,
        );

        assert!(
            !rewrite.should_quarantine(),
            "dispatcher must not quarantine exact CEPv23 compact Module_Info"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(rewrite.verified_family(), VerifiedFamily::ModuleInfo);
        assert!(crate::strict::module_info_shape_valid(&payload));
    }

    #[test]
    fn dispatcher_claims_local_kingmaker_module_end_game_without_raw_passthrough() {
        let mut payload = include_bytes!(
            "../../../fixtures/module_info/local_kingmaker_module_end_game_premiumdemo_20260523.bin"
        )
        .to_vec();

        let rewrite = rewrite_single_inflated_payload_for_ee(
            &mut payload,
            None,
            SemanticScope::CoalescedSpan,
            None,
            None,
            None,
        );

        assert!(
            !rewrite.should_quarantine(),
            "dispatcher must not quarantine exact Module_EndGame"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(rewrite.verified_family(), VerifiedFamily::ModuleEndGame);
        assert!(module::claim_module_end_game_payload_if_verified(&payload).is_some());
    }

    #[test]
    fn dispatcher_claims_hg_seq41_captain_mixed_live_object_without_raw_passthrough() {
        // HG driver-only mixed creature stream: inventory/update/add/appearance/
        // `U/5 0x3967` are only safe once the live-object family owns the whole
        // byte cursor and fragment cursor. This pins the server-dispatch
        // registry path so a broad exact/raw live-object classifier cannot
        // bypass the typed add/update translators.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_seq41_creature_captain_mixed_add_update.bin"
        )
        .to_vec();

        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn dispatcher_claims_town_greeter_trader_mixed_live_object_without_raw_passthrough() {
        // HG Docks NPC burst with adjacent inventory/GUI records and creature
        // `P/5` appearances. The dispatcher must reach the same exact semantic
        // ownership as the live-object module-level fixture instead of treating
        // the deflated payload as an opaque zlib/raw blob.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/starcore_npc_town_greeter_trader_stream_claimed_but_ee_rejects.bin"
        )
        .to_vec();

        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn dispatcher_claims_local_diamond_auto_inventory_gui_rows() {
        // Local Diamond auto-open-inventory stream from 2026-05-19: declared
        // `G I A` / `G R A` item-create rows are owned by the focused GUI
        // item-create translator. The dispatcher may emit it only after the
        // typed rewrite and exact EE live-object validator agree.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_diamond_seq20_auto_inventory_gia_gra_20260519.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "dispatcher local auto-inventory GUI claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned local auto-inventory GUI payload must be exact EE live-object shape");
        assert!(claim.live_gui_item_create_records >= 5);
    }

    #[test]
    fn dispatcher_claims_hg_seq36_declared_repair_without_retry_storm() {
        // Live HG Starcore5 Docks seq36 carries a stale CNW declared value
        // inside a legal live-object burst. The dispatcher may repair the
        // transport split only after the typed live-object translator and exact
        // EE validator own the resulting shape; this regression keeps that path
        // bounded so the M-frame layer does not spend seconds retrying raw
        // candidate splits under the reliable-window gate.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_starc5_docks_seq36_town_greeter_northern_trader_slow_20260518.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(6),
            "dispatcher live-object seq36 declared repair must stay bounded, elapsed={elapsed:?}"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn dispatcher_claims_local_dark_ranger_current_player_declared_repair() {
        // Local Dark Ranger seq13 carries a stale declared read window over a
        // current-player `A/5`, `P/5`, `U/5` trio. Its real CNW fragment tail
        // contains byte patterns that resemble live-object opcodes, so the
        // dispatcher must not accept a short prefix, but it may accept the
        // exact same-object creature appearance/update split after the focused
        // typed translators and final EE validator prove it.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_dark_ranger_seq13_current_player_liveobject_20260521.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher Dark Ranger declared repair must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned Dark Ranger payload must be exact EE live-object shape");
        assert!(claim.add_records >= 1);
        assert!(claim.creature_appearance_records >= 1);
        assert!(claim.creature_update_records >= 1);
    }

    #[test]
    fn dispatcher_claims_local_dark_ranger_seq15_4408_inventory_gui_stream() {
        // Local Dark Ranger seq15 from 2026-05-23: full declared `P/05/01`
        // payload with compact `U/5 0x4408`, current-player inventory/GUI
        // rows, and an innkeeper full `P/5` appearance followed immediately by
        // `U/5 0x3967`. The dispatcher must keep this in the typed live-object
        // path until the final EE validator owns both records.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_dark_ranger_seq15_u5_4408_inventory_gui_20260523_unclaimed.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher Dark Ranger seq15 claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect(
                "dispatcher-owned Dark Ranger seq15 payload must be exact EE live-object shape",
            );
        assert!(claim.records_examined >= 1);
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_dark_ranger_seq18_auto_inventory_gui_stream() {
        // Local Dark Ranger seq18 from 2026-05-24 after auto-opening inventory:
        // the server emitted a compact GIA/GRA live-object payload whose final
        // EE bytes were captured by the accepted-live-object diagnostic.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_dark_ranger_seq18_auto_inventory_gui_20260524_legacy.bin"
        )
        .to_vec();
        let expected_ee = include_bytes!(
            "../../../fixtures/live_object/local_dark_ranger_seq18_auto_inventory_gui_20260524_ee.bin"
        )
        .as_slice();

        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "raw Dark Ranger seq18 stream documents the pre-rewrite Diamond shape"
        );

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher Dark Ranger seq18 claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert_eq!(
            payload.as_slice(),
            expected_ee,
            "dispatcher rewrite should match the harness-dumped EE byte shape"
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned Dark Ranger seq18 payload must exact-claim");
        assert!(
            claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
            "GUI live-object rows should remain owned after dispatcher rewrite"
        );
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_cepv23_skies_auto_inventory_gui_stream() {
        // Local CEP v2.3 skies seq17 from 2026-05-24 after auto-opening
        // inventory. Dispatcher ownership must stay on the bounded typed
        // live-object path and match the accepted-live-object EE dump exactly.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_cepv23_skies_seq17_auto_inventory_gui_20260524_legacy.bin"
        )
        .to_vec();
        let expected_ee = include_bytes!(
            "../../../fixtures/live_object/local_cepv23_skies_seq17_auto_inventory_gui_20260524_ee.bin"
        )
        .as_slice();

        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "raw CEP v2.3 skies stream documents the pre-rewrite Diamond shape"
        );

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher CEP v2.3 skies seq17 claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert_eq!(
            payload.as_slice(),
            expected_ee,
            "dispatcher rewrite should match the harness-dumped EE byte shape"
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned CEP v2.3 skies payload must exact-claim");
        assert!(
            claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
            "GUI live-object rows should remain owned after dispatcher rewrite"
        );
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_hg_live_seq42_auto_inventory_gui_stream() {
        // Live HG seq42 from 2026-05-24 after auto-opening inventory in the
        // Docks. This large two-frame burst must stay on the typed live-object
        // dispatcher path and match the accepted-live-object EE dump exactly.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_live_seq42_auto_inventory_gui_20260524_legacy.bin"
        )
        .to_vec();
        let expected_ee = include_bytes!(
            "../../../fixtures/live_object/hg_live_seq42_auto_inventory_gui_20260524_ee.bin"
        )
        .as_slice();

        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "raw live HG seq42 stream documents the pre-rewrite Diamond shape"
        );

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher live HG seq42 claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert_eq!(
            payload.as_slice(),
            expected_ee,
            "dispatcher rewrite should match the live HG EE byte shape"
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned live HG seq42 payload must exact-claim");
        assert!(
            claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
            "GUI live-object rows should remain owned after dispatcher rewrite"
        );
        assert!(
            claim.records_examined > 1,
            "live HG seq42 should remain a combined live-object burst"
        );
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_hg_live_docks_091731_live_object_pairs() {
        // Live HG Docks run from 2026-05-24 produced multiple accepted
        // live-object diagnostics while exercising area entry, object probes,
        // and auto-inventory. Dispatcher ownership must stay on the bounded
        // live-object path and match the dumped EE bytes exactly.
        // Seq34 is pinned in live_object_update/tests.rs, but its dispatcher
        // finalization is session-registry dependent in the live harness.
        for (name, legacy, expected_ee, legacy_already_exact) in [
            (
                "seq28",
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq28_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq28_liveobject_20260524_ee.bin"
                )
                .as_slice(),
                false,
            ),
            (
                "seq29",
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq29_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq29_liveobject_20260524_ee.bin"
                )
                .as_slice(),
                false,
            ),
            (
                "seq35",
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq35_exact_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq35_exact_liveobject_20260524_ee.bin"
                )
                .as_slice(),
                true,
            ),
            (
                "seq42_auto_inventory_gui",
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq42_auto_inventory_gui_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_docks_091731_seq42_auto_inventory_gui_20260524_ee.bin"
                )
                .as_slice(),
                false,
            ),
        ] {
            let mut payload = legacy.to_vec();
            let raw_exact =
                crate::translate::live_object_update::claim_payload_if_verified(&payload)
                    .is_some();
            assert_eq!(
                raw_exact, legacy_already_exact,
                "{name} raw fixture exactness should match the accepted-live-object evidence"
            );

            let started = std::time::Instant::now();
            let rewrite = dispatch_live_object_fixture(&mut payload);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "dispatcher live HG {name} claim must stay bounded"
            );
            assert!(
                !rewrite.should_quarantine(),
                "dispatcher must not quarantine accepted live HG {name}"
            );
            if !legacy_already_exact {
                assert!(rewrite.any_rewrite(), "{name} should be rewritten");
            }
            assert_eq!(
                rewrite.verified_family(),
                VerifiedFamily::GameObjUpdateLiveObject
            );
            assert_eq!(
                payload.as_slice(),
                expected_ee,
                "{name} dispatcher rewrite should match the live HG EE byte shape"
            );
            let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
                .expect("dispatcher-owned live HG payload must exact-claim");
            if name == "seq42_auto_inventory_gui" {
                assert!(
                    claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                    "{name} should retain GUI live-object row ownership"
                );
                assert!(
                    claim.records_examined > 1,
                    "{name} should remain a combined live-object burst"
                );
            } else {
                assert!(claim.records_examined >= 1);
            }
            assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
        }
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_contest_champions_area_entry_liveobject() {
        // Local Contest Of Champions 0492 seq11 from 2026-05-24 at area entry.
        // Dispatcher ownership must stay on the bounded typed live-object path
        // and match the accepted-live-object EE dump exactly.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_contest_champions_seq11_area_entry_liveobject_20260524_legacy.bin"
        )
        .to_vec();
        let expected_ee = include_bytes!(
            "../../../fixtures/live_object/local_contest_champions_seq11_area_entry_liveobject_20260524_ee.bin"
        )
        .as_slice();

        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "raw Contest Of Champions seq11 stream documents the pre-rewrite Diamond shape"
        );

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "dispatcher Contest Of Champions seq11 claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert_eq!(
            payload.as_slice(),
            expected_ee,
            "dispatcher rewrite should match the harness-dumped EE byte shape"
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned Contest Of Champions seq11 payload must exact-claim");
        assert!(
            claim.records_examined >= 1,
            "dispatcher should leave area-entry live-object records exactly typed"
        );
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_winds_eremor_live_object_pairs() {
        // Local The Winds of Eremor run from 2026-05-24 produced new
        // placeable-heavy streams plus an auto-inventory GUI stream. The
        // dispatcher must keep them on the bounded live-object path until the
        // final exact EE payload matches the harness-dumped bytes.
        for (name, legacy, expected_ee) in [
            (
                "initial_placeables",
                include_bytes!(
                    "../../../fixtures/live_object/local_winds_eremor_seq_initial_placeables_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_winds_eremor_seq_initial_placeables_20260524_ee.bin"
                )
                .as_slice(),
            ),
            (
                "placeable_burst",
                include_bytes!(
                    "../../../fixtures/live_object/local_winds_eremor_seq_placeable_burst_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_winds_eremor_seq_placeable_burst_20260524_ee.bin"
                )
                .as_slice(),
            ),
            (
                "auto_inventory_gui",
                include_bytes!(
                    "../../../fixtures/live_object/local_winds_eremor_seq_auto_inventory_gui_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_winds_eremor_seq_auto_inventory_gui_20260524_ee.bin"
                )
                .as_slice(),
            ),
        ] {
            let mut payload = legacy.to_vec();

            assert!(
                crate::translate::live_object_update::claim_payload_if_verified(&payload)
                    .is_none(),
                "{name} raw Winds of Eremor stream should document the pre-rewrite Diamond shape"
            );

            let started = std::time::Instant::now();
            let rewrite = dispatch_live_object_fixture(&mut payload);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "dispatcher Winds of Eremor {name} claim must stay bounded"
            );
            assert!(rewrite.any_rewrite(), "{name} should be rewritten");
            assert_eq!(
                rewrite.verified_family(),
                VerifiedFamily::GameObjUpdateLiveObject
            );
            assert_eq!(
                payload.as_slice(),
                expected_ee,
                "{name} dispatcher rewrite should match the harness-dumped EE byte shape"
            );
            let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
                .expect("dispatcher-owned Winds of Eremor payload must exact-claim");
            assert!(claim.records_examined >= 1);
            assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
        }
    }

    #[test]
    fn dispatcher_claims_local_to_heir_kraegen_thoraulik_live_object_stream() {
        // Local To Heir creature auto-use/dialog harness capture from
        // 2026-05-24. This payload appeared as a deflated server live-object
        // window after the area-load gate opened; keep it pinned in the
        // dispatcher path so it cannot regress into a silent stream stall.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_to_heir_seq19_kraegen_thoraulik_liveobject_20260524_unclaimed.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher To Heir live-object claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned To Heir live-object payload must exact-claim");
        assert!(claim.add_records >= 1);
        assert!(claim.creature_appearance_records >= 1);
        assert!(claim.creature_update_records >= 1);
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_cepv23_starter_lance_lute_patron_live_object() {
        // Local CEP v2.3 starter seq17 from 2026-05-23: NPC/placeable add
        // records for Lance, Lute, and Tavern Patron arrive as a declared
        // P/05/01 stream whose compact Diamond add rows must be rewritten into
        // exact EE live-object records before strict dispatch accepts them.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_cepv23_starter_seq17_lance_lute_patron_liveobject_20260523_unclaimed.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher CEP v2.3 starter live-object claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned CEP v2.3 starter payload must be exact EE live-object shape");
        assert!(claim.add_records >= 1);
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[test]
    fn dispatcher_claims_local_chapter2_area_entry_coalesced_live_object() {
        // Local Diamond Chapter2 after the `a08_barracks` area load: the
        // coalesced live-object stream carries placeable/object updates and
        // must be owned by the focused live-object translators before strict
        // EE validation accepts it.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_chapter2_seq20_coalesced_liveobject_20260523_unclaimed.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(6),
            "dispatcher Chapter2 coalesced live-object claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned Chapter2 coalesced live-object payload must exact-claim");
        assert!(claim.add_records >= 1);
        assert!(claim.creature_appearance_records >= 1);
    }

    #[test]
    fn dispatcher_claims_local_chapter2e_area_entry_live_object() {
        // Local Diamond Chapter2E area-entry harness run from 2026-05-24. This
        // pins the dispatcher path against the same legacy->EE fixture pair
        // captured by the accepted-live-object diagnostic, keeping ownership in
        // the typed live-object translators instead of any raw fallback.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_chapter2e_seq16_liveobject_20260524_legacy.bin"
        )
        .to_vec();
        let expected_ee = include_bytes!(
            "../../../fixtures/live_object/local_chapter2e_seq16_liveobject_20260524_ee.bin"
        )
        .as_slice();

        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "raw Chapter2E area-entry stream documents the pre-rewrite Diamond shape"
        );

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher Chapter2E area-entry live-object claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert_eq!(
            payload.as_slice(),
            expected_ee,
            "dispatcher rewrite should match the harness-dumped EE byte shape"
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned Chapter2E live-object payload must exact-claim");
        assert!(claim.records_examined >= 1);
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_xp1_chapter1_live_object_pairs() {
        // Local XP1-Chapter 1 harness run from 2026-05-24. Area entry produced
        // several compact live-object updates, then auto-inventory produced a
        // compact GIA/GRA GUI stream. The dispatcher must keep each payload on
        // the bounded live-object rewrite path until the exact EE bytes match
        // the accepted-live-object diagnostics.
        for (name, legacy, expected_ee, expect_gui) in [
            (
                "seq13",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq13_area_entry_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq13_area_entry_liveobject_20260524_ee.bin"
                )
                .as_slice(),
                false,
            ),
            (
                "seq14",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq14_area_entry_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq14_area_entry_liveobject_20260524_ee.bin"
                )
                .as_slice(),
                false,
            ),
            (
                "seq15",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq15_area_entry_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq15_area_entry_liveobject_20260524_ee.bin"
                )
                .as_slice(),
                false,
            ),
            (
                "seq16",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq16_area_entry_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq16_area_entry_liveobject_20260524_ee.bin"
                )
                .as_slice(),
                false,
            ),
            (
                "seq26_auto_inventory_gui",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq26_auto_inventory_gui_20260524_legacy.bin"
                )
                .as_slice(),
                include_bytes!(
                    "../../../fixtures/live_object/local_xp1_chapter1_seq26_auto_inventory_gui_20260524_ee.bin"
                )
                .as_slice(),
                true,
            ),
        ] {
            let mut payload = legacy.to_vec();

            assert!(
                crate::translate::live_object_update::claim_payload_if_verified(&payload)
                    .is_none(),
                "{name} raw XP1-Chapter 1 stream should document the pre-rewrite Diamond shape"
            );

            let started = std::time::Instant::now();
            let rewrite = dispatch_live_object_fixture(&mut payload);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "dispatcher XP1-Chapter 1 {name} claim must stay bounded"
            );
            assert!(rewrite.any_rewrite(), "{name} should be rewritten");
            assert_eq!(
                rewrite.verified_family(),
                VerifiedFamily::GameObjUpdateLiveObject
            );
            assert_eq!(
                payload.as_slice(),
                expected_ee,
                "{name} dispatcher rewrite should match the harness-dumped EE byte shape"
            );
            let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
                .expect("dispatcher-owned XP1-Chapter 1 payload must exact-claim");
            assert!(
                claim.records_examined >= 1,
                "{name} should retain at least one typed live-object record"
            );
            if expect_gui {
                assert!(
                    claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                    "{name} should retain GUI live-object row ownership"
                );
            }
            assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
        }
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_xp1_chapter2_4408_inventory_creature_stream() {
        // Local XP1-Chapter 2 seq16 accepted-live-object dump from 2026-05-24.
        // The first `U/5 0x4408` record has two counted visual-effect rows
        // before current-player inventory/read-buffer state and Merom Rescher
        // add/update records. Dispatcher ownership must stay in the bounded
        // live-object rewrite path through exact EE validation.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_xp1_chapter2_seq16_4408_inventory_creature_20260524_legacy.bin"
        )
        .to_vec();

        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "raw XP1-Chapter 2 stream should document the pre-rewrite Diamond shape"
        );

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher XP1-Chapter 2 0x4408 stream claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned XP1-Chapter 2 payload must exact-claim");
        assert!(claim.records_examined >= 1);
        assert!(claim.creature_update_records >= 1);
        assert!(claim.add_records >= 1);
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[cfg(hgbridge_private_fixtures)]
    #[test]
    fn dispatcher_claims_local_xp2_chapter1_live_object_pairs() {
        // Local XP2_Chapter1 `xp2_intro` area-entry run from 2026-05-24.
        // These deflated live-object windows were accepted only after the
        // dispatcher routed them through the focused typed live-object rewrites.
        for (name, legacy) in [
            (
                "seq11",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq11_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq12",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq12_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq13",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq13_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq14",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq14_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq15",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq15_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq16",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq16_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq17",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq17_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq18",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq18_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq19",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq19_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq20",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq20_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq21",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq21_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq22",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq22_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
            (
                "seq23",
                include_bytes!(
                    "../../../fixtures/live_object/local_xp2_chapter1_seq23_liveobject_20260524_legacy.bin"
                )
                .as_slice(),
            ),
        ] {
            let mut payload = legacy.to_vec();

            assert!(
                crate::translate::live_object_update::claim_payload_if_verified(&payload)
                    .is_none(),
                "{name} raw XP2_Chapter1 stream should document the pre-rewrite Diamond shape"
            );

            let started = std::time::Instant::now();
            let rewrite = dispatch_live_object_fixture(&mut payload);
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "dispatcher XP2_Chapter1 {name} claim must stay bounded"
            );
            assert!(rewrite.any_rewrite(), "{name} should be rewritten");
            assert_eq!(
                rewrite.verified_family(),
                VerifiedFamily::GameObjUpdateLiveObject
            );
            let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
                .expect("dispatcher-owned XP2_Chapter1 payload must exact-claim");
            assert!(
                crate::translate::live_object_update::claim_payload_if_verified_with_lifecycle(
                    &payload,
                    |_, _| false
                )
                .is_some(),
                "{name} dispatcher output should be lifecycle-safe after bounded cleanup"
            );
            assert!(
                claim.records_examined >= 1,
                "{name} should retain at least one typed live-object record"
            );
            assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
        }
    }

    #[test]
    fn dispatcher_claims_local_chapter3_auto_inventory_gui_live_object() {
        // Local Chapter3 `m3q1a10` after auto-opening inventory on 2026-05-23:
        // the stream starts with live GUI item-create rows followed by current
        // player update records. Keep this on the typed live-object
        // path until the final EE validator owns the rewritten GUI body.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/local_chapter3_seq26_auto_inventory_gui_20260523_unclaimed.bin"
        )
        .to_vec();

        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_none(),
            "raw Chapter3 auto-inventory stream documents the pre-rewrite Diamond shape"
        );

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "dispatcher Chapter3 auto-inventory GUI claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned Chapter3 GUI/inventory payload must exact-claim");
        assert!(claim.live_gui_item_create_records >= 1);
        assert!(claim.records_examined >= claim.live_gui_item_create_records);
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }

    #[test]
    fn dispatcher_claims_current_hg_town_npc_ids_without_retry_storm() {
        for (name, fixture) in current_hg_town_npc_fixtures() {
            let mut payload = fixture.to_vec();

            let started = std::time::Instant::now();
            let rewrite = dispatch_live_object_fixture(&mut payload);
            let elapsed = started.elapsed();
            assert!(
                elapsed < std::time::Duration::from_secs(6),
                "{name} dispatcher live-object current HG town ids must stay bounded, elapsed={elapsed:?}"
            );
            assert!(rewrite.any_rewrite(), "{name} should be rewritten");
            assert_eq!(
                rewrite.verified_family(),
                VerifiedFamily::GameObjUpdateLiveObject,
                "{name} should verify as live-object"
            );
            assert!(
                crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some(),
                "{name} should exact-claim after rewrite"
            );
        }
    }

    #[test]
    fn dispatcher_claims_current_hg_town_npc_ids_with_session_registry() {
        for (name, fixture) in current_hg_town_npc_fixtures() {
            let mut payload = fixture.to_vec();
            let state = semantic::SemanticSessionState::default();

            let started = std::time::Instant::now();
            let rewrite = rewrite_single_inflated_payload_for_ee(
                &mut payload,
                None,
                SemanticScope::DeflatedReassembly,
                None,
                Some(&state.objects),
                None,
            );
            let elapsed = started.elapsed();
            assert!(
                elapsed < std::time::Duration::from_secs(6),
                "{name} dispatcher live-object current HG town ids with registry must stay bounded, elapsed={elapsed:?}"
            );
            assert!(
                !rewrite.should_quarantine(),
                "{name} registry finalization should not quarantine in-payload add/P/U lifecycle"
            );
            assert!(rewrite.any_rewrite(), "{name} should be rewritten");
            assert_eq!(
                rewrite.verified_family(),
                VerifiedFamily::GameObjUpdateLiveObject,
                "{name} should verify as live-object"
            );
        }
    }

    fn current_hg_town_npc_fixtures() -> [(&'static str, &'static [u8]); 3] {
        [
            (
                "2026-05-19-live-hg-seq38-town-greeter-northern-trader",
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_seq38_town_greeter_northern_trader_20260519.bin"
                ),
            ),
            (
                "2026-05-19-live-hg-seq40-town-greeter-northern-trader",
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_seq40_town_greeter_northern_trader_20260519.bin"
                ),
            ),
            (
                "2026-05-19-live-hg-seq39-town-greeter-northern-trader",
                include_bytes!(
                    "../../../fixtures/live_object/hg_live_seq39_town_greeter_northern_trader_20260519.bin"
                ),
            ),
        ]
    }

    #[test]
    fn dispatcher_claims_hg_seq37_declared_repair_without_retry_storm() {
        // Live HG Starcore5 Docks seq37 proved the same stale-declared repair
        // pressure with a longer creature update burst. Keep the dispatcher
        // accountable for routing this to the semantic live-object family
        // quickly, with no fallback passthrough and no broad zlib/raw claim.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_starc5_docks_seq37_creature_update_slow_20260518.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(6),
            "dispatcher live-object seq37 declared repair must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn dispatcher_claims_hg_seq38_creature_update_through_bounded_orchestrator() {
        // HG Docks `Otis` burst reproduced the live runtime stall at
        // first_sequence=38: exact-claim probes rejected several intermediate
        // boundaries while the bounded typed live-object orchestrator could
        // already prove the Diamond shape and emit exact EE records. Keep this
        // pinned at the dispatcher layer so the deflated path cannot regress
        // into a retry/log storm before the verified translator owns it.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_starc5_docks_seq38_creature_update_unacked_20260518.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(6),
            "dispatcher live-object seq38 claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        assert!(
            crate::translate::live_object_update::claim_payload_if_verified(&payload).is_some()
        );
    }

    #[test]
    fn dispatcher_claims_hg_seq40_otis_elrawiel_mixed_live_object_through_bounded_orchestrator() {
        // HG Docks mixed `Otis`/`Elrawiel` stream: the payload combines fixed
        // `A/5` add records with following `P/5` creature appearance/name
        // records. Decompile-backed ownership lives in the focused live-object
        // passes; the dispatcher must only route and accept the family after
        // those typed passes produce an exact EE reader shape.
        let mut payload = include_bytes!(
            "../../../fixtures/live_object/hg_seq40_creature_otis_mixed_add_update.bin"
        )
        .to_vec();

        let started = std::time::Instant::now();
        let rewrite = dispatch_live_object_fixture(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(6),
            "dispatcher live-object seq40 claim must stay bounded"
        );
        assert!(rewrite.any_rewrite());
        assert_eq!(
            rewrite.verified_family(),
            VerifiedFamily::GameObjUpdateLiveObject
        );
        let claim = crate::translate::live_object_update::claim_payload_if_verified(&payload)
            .expect("dispatcher-owned HG seq40 payload must be exact EE live-object shape");
        assert!(claim.add_records > 0);
        assert!(claim.creature_appearance_records > 0);
        assert!(claim.creature_update_records > 0);
    }
}
