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
        VerifiedFamily, VerifiedPacket, area, char_list, chat, client_side_message, cnw_message,
        custom_token, game_obj_update, gameplay_stream, inventory, journal, live_object, loadbar,
        login, module, module_resources, module_time, party, player_list, quickbar,
    },
};

use super::{live_update, parse_window};
use std::{
    fs,
    path::PathBuf,
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

type ServerTranslatorFn =
    fn(&mut Vec<u8>, Option<&area::AreaPlaceableContext>, SemanticScope) -> ServerTranslatorOutcome;

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
        family_name: "LoadBar",
        verified_family: Some(VerifiedFamily::LoadBar),
        translate: translate_loadbar,
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
        family_name: "Inventory",
        verified_family: Some(VerifiedFamily::Inventory),
        translate: translate_inventory,
    },
    ServerToClientTranslator {
        family_name: "GameObjUpdate",
        verified_family: Some(VerifiedFamily::GameObjUpdateObjectControl),
        translate: translate_game_obj_update,
    },
    ServerToClientTranslator {
        family_name: "Party",
        verified_family: Some(VerifiedFamily::Party),
        translate: translate_party,
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
    preclaimed_family: Option<(&'static str, VerifiedFamily)>,
) -> InflatedPayloadRewrite {
    let mut rewrite = InflatedPayloadRewrite::default();

    let split = gameplay_stream::split_inflated_gameplay(payload);
    if !split.complete {
        tracing::debug!(
            units = split.units.len(),
            payload_length = payload.len(),
            "inflated gameplay stream classified as incomplete/non-header continuation"
        );
    }

    if let Some((family_name, family)) = preclaimed_family {
        rewrite.note_rewrite(family_name, family);
    }

    for translator in SERVER_TO_CLIENT_TRANSLATORS {
        match (translator.translate)(payload, latest_area_placeables, scope) {
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
                rewrite.note_rewrite(translator.family_name, family);
                if let Some(area_rewrite) = claim.area_rewrite {
                    rewrite.area_rewrite = Some(area_rewrite);
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

fn claimed() -> ServerTranslatorOutcome {
    ServerTranslatorOutcome::Claim(ServerTranslatorClaim::default())
}

fn translate_custom_token(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if custom_token::claim_or_rewrite_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_login(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
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
) -> ServerTranslatorOutcome {
    if module_time::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_loadbar(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if loadbar::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_client_side_message(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
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
) -> ServerTranslatorOutcome {
    if chat::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_inventory(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if inventory::claim_or_rewrite_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_game_obj_update(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if game_obj_update::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_party(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if party::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_quickbar(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if quickbar::normalize_and_rewrite_quickbar_payload_if_possible(payload).is_some()
        || quickbar::rewrite_simple_quickbar_payload_if_possible(payload).is_some()
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
) -> ServerTranslatorOutcome {
    if player_list::rewrite_player_list_payload_if_possible(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_live_object_prefixed_fragments(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if live_object::normalize_prefixed_fragments_payload_if_needed(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_live_object_add_records(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        payload,
        latest_area_placeables,
    )
    .is_some()
    {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_live_object_update_records(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if live_update::rewrite_payload_if_needed(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_live_object_claimed_records(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    _: SemanticScope,
) -> ServerTranslatorOutcome {
    if live_update::claim_payload_if_verified(payload).is_some() {
        claimed()
    } else {
        ServerTranslatorOutcome::None
    }
}

fn translate_area_client_area(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
) -> ServerTranslatorOutcome {
    // `Area_ClientArea` is a semantic CNW payload; the reliable-window
    // transport may carry it either as the whole deflated reassembly or as a
    // deflated primary record inside a coalesced M datagram. The EE/Diamond
    // sender/reader shape is the same after the M layer inflates it, so the
    // area translator owns both scopes instead of letting coalescing decide
    // packet validity.
    let _ = scope;
    match area::rewrite_area_client_area_payload(payload) {
        Some(summary) => ServerTranslatorOutcome::Claim(ServerTranslatorClaim {
            area_rewrite: Some(summary),
        }),
        None => ServerTranslatorOutcome::None,
    }
}

fn translate_module_info(
    payload: &mut Vec<u8>,
    _: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
) -> ServerTranslatorOutcome {
    if !matches!(scope, SemanticScope::DeflatedReassembly) {
        return ServerTranslatorOutcome::None;
    }
    if module::rewrite_module_info_payload(payload).is_some() {
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
    let dir = std::env::var("HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR").ok()?;
    if dir.trim().is_empty() {
        return None;
    }
    let mut path = PathBuf::from(dir);
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
) -> anyhow::Result<Option<VerifiedPacket>> {
    if let Some(rewritten) = rewrite_server_status_module_resources_frame_if_needed(
        bytes,
        view,
        module_resource_runtime,
    )? {
        return Ok(Some(VerifiedPacket {
            family: VerifiedFamily::ServerStatusModuleResources,
            packet: rewritten,
        }));
    }
    if let Some(rewritten) = live_update::rewrite_direct_frame_if_needed(bytes, view)? {
        return Ok(Some(VerifiedPacket {
            family: VerifiedFamily::GameObjUpdateLiveObject,
            packet: rewritten,
        }));
    }
    if let Some(high) = view.high {
        if let Some(verified) = rewrite_direct_semantic_frame_if_claimed(bytes, view, high)? {
            return Ok(Some(verified));
        }
        let reason = untranslated_semantic_quarantine_reason(high);
        return consume_untranslated_direct_frame(bytes, view, high, reason).map(|packet| {
            Some(VerifiedPacket {
                family: VerifiedFamily::ConsumedEmptyMFrame,
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
) -> anyhow::Result<Option<VerifiedPacket>> {
    let Some(payload) = parse_window::primary_payload(bytes, view) else {
        return Ok(None);
    };
    let mut rewritten_payload = payload.to_vec();
    let semantic_rewrite_summary = rewrite_inflated_payload_for_ee(
        &mut rewritten_payload,
        None,
        SemanticScope::CoalescedSpan,
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
        family: verified_family,
        packet: rewritten,
    }))
}

fn consume_untranslated_direct_frame(
    bytes: &[u8],
    view: &MFrameView,
    high: HighLevel,
    reason: &'static str,
) -> anyhow::Result<Vec<u8>> {
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume untranslated extended-length direct M frame yet");
    }

    let mut rewritten = bytes.to_vec();
    rewritten.truncate(crate::packet::m::LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if rewritten.len() > 7 {
        // This is a semantic quarantine shell, not a packetized payload. Keep
        // the sequence/ack bytes intact, but clear stream/packetized/deflate
        // delivery bits before setting the payload length to zero.
        rewritten[7] &= !0x07;
    }
    write_be_u16(&mut rewritten, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear untranslated direct M payload length"))?;
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair untranslated direct M CRC"))?;

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
        hak_count = summary.hak_count,
        nwsync_advertised = summary.nwsync_advertised,
        "server ServerStatus_ModuleRunning module resources rewritten for EE"
    );
    Ok(Some(rewritten))
}
