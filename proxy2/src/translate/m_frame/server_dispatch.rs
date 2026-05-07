//! Direct server-to-client high-level `M` dispatch.
//!
//! This module owns direct-frame routing only: extract the reliable gameplay
//! payload, delegate semantic translation to focused siblings, then repair the
//! M-frame length/CRC. Deflated-window and coalesced-window routing stays in
//! the parent M-frame transport layer.

use crate::{
    packet::m::{HighLevel, MFrameView},
    translate::{
        area, cnw_message, custom_token, live_object, module, module_resources, player_list,
        quickbar,
    },
};

use super::{live_update, parse_window};

#[derive(Debug, Clone, Copy)]
pub(super) enum SemanticScope {
    DeflatedReassembly,
    CoalescedSpan,
}

#[derive(Debug, Default)]
pub(super) struct InflatedPayloadRewrite {
    family_names: Vec<&'static str>,
    pub(super) area_rewrite: Option<area::AreaRewriteSummary>,
    pub(super) module_info_candidate_offset: Option<usize>,
}

impl InflatedPayloadRewrite {
    pub(super) fn note_rewrite(&mut self, family_name: &'static str) {
        if !self.family_names.contains(&family_name) {
            self.family_names.push(family_name);
        }
    }

    pub(super) fn any_rewrite(&self) -> bool {
        !self.family_names.is_empty()
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

pub(super) fn rewrite_inflated_payload_for_ee(
    payload: &mut Vec<u8>,
    latest_area_placeables: Option<&area::AreaPlaceableContext>,
    scope: SemanticScope,
) -> InflatedPayloadRewrite {
    let mut rewrite = InflatedPayloadRewrite::default();

    if custom_token::rewrite_payload_if_possible(payload).is_some() {
        rewrite.note_rewrite("SetCustomToken");
    }
    if quickbar::normalize_quickbar_payload_if_needed(payload).is_some() {
        rewrite.note_rewrite("GuiQuickbarPrefixedFragments");
    }
    if quickbar::rewrite_simple_quickbar_payload_if_possible(payload).is_some() {
        rewrite.note_rewrite("GuiQuickbar");
    }
    if cnw_message::normalize_prefixed_fragments_payload_if_needed(payload).is_some() {
        rewrite.note_rewrite("CNWMessagePrefixedFragments");
    }
    if player_list::rewrite_player_list_payload_if_possible(payload).is_some() {
        rewrite.note_rewrite("PlayerList");
    }
    if live_object::normalize_prefixed_fragments_payload_if_needed(payload).is_some() {
        rewrite.note_rewrite("GameObjUpdate_LiveObjectPrefixedFragments");
    }
    if live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        payload,
        latest_area_placeables,
    )
    .is_some()
    {
        rewrite.note_rewrite("GameObjUpdate_LiveObjectAddRecords");
    }
    if live_update::rewrite_payload_if_needed(payload).is_some() {
        rewrite.note_rewrite("GameObjUpdate_LiveObjectUpdateRecords");
    }

    if matches!(scope, SemanticScope::DeflatedReassembly) {
        if let Some(summary) = area::rewrite_area_client_area_payload(payload) {
            rewrite.note_rewrite("Area_ClientArea");
            rewrite.area_rewrite = Some(summary);
        }
        if module::rewrite_module_info_payload(payload).is_some() {
            rewrite.note_rewrite("Module_Info");
        }
        if !rewrite.any_rewrite() {
            rewrite.module_info_candidate_offset = module::first_module_info_candidate_offset(payload);
        }
    }

    rewrite
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
    if custom_token::rewrite_payload_if_possible(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if player_list::rewrite_player_list_payload_if_possible(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if quickbar::normalize_quickbar_payload_if_needed(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if quickbar::rewrite_simple_quickbar_payload_if_possible(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    if cnw_message::normalize_prefixed_fragments_payload_if_needed(&mut probe).is_some() {
        return true;
    }
    let mut probe = inflated.to_vec();
    live_object::normalize_prefixed_fragments_payload_if_needed(&mut probe).is_some()
}

pub(super) fn rewrite_direct_frame_if_needed(
    bytes: &[u8],
    view: &MFrameView,
) -> anyhow::Result<Option<Vec<u8>>> {
    if let Some(rewritten) = rewrite_server_status_module_resources_frame_if_needed(bytes, view)? {
        return Ok(Some(rewritten));
    }
    live_update::rewrite_direct_frame_if_needed(bytes, view)
}

fn rewrite_server_status_module_resources_frame_if_needed(
    bytes: &[u8],
    view: &MFrameView,
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
    let Some(summary) =
        module_resources::rewrite_server_status_module_resources_payload(&mut rewritten_payload)
    else {
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
