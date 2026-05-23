//! Client-originated high-level gameplay semantic claims.
//!
//! The bridge rule is deliberately symmetric: a client-to-server high-level
//! opcode being known is not permission to pass it through. This router only
//! delegates to focused family modules; each family module documents why the
//! verified EE client shape is also valid for the Diamond/1.69 server, or
//! performs a dialect rewrite before claiming the packet.

use crate::{
    packet::{hex_prefix, m::HighLevel},
    translate::{
        VerifiedFamily, client_area, client_char_list, client_character_sheet, client_gui_event,
        client_gui_inventory, client_input, client_login, client_module, client_quickbar,
        client_server_status, dialog, journal, party, play_module_character_list,
        semantic::SemanticSessionState,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct ClientHighClaimSummary {
    pub family_name: &'static str,
    pub packet_name: &'static str,
    pub verified_family: VerifiedFamily,
}

#[derive(Debug, Clone, Copy)]
pub struct ClientHighConsumedSummary {
    pub family_name: &'static str,
    pub packet_name: &'static str,
    pub reason: &'static str,
}

pub fn claim_consumed_payload_if_verified(payload: &[u8]) -> Option<ClientHighConsumedSummary> {
    if let Some(summary) = client_gui_event::claim_payload_if_verified(payload) {
        tracing::info!(
            packet_name = summary.packet_name,
            event_a = summary.event_a,
            event_b = summary.event_b,
            object_id = %format_args!("0x{:08X}", summary.object_id),
            declared_bytes = summary.declared_bytes,
            trailing_fragment_bytes = summary.trailing_fragment_bytes,
            has_vector = summary.vector.is_some(),
            legacy_action = ?summary.legacy_action,
            "client GuiEvent payload claimed as EE-only with no proven Diamond handler"
        );
        return Some(ClientHighConsumedSummary {
            family_name: "ClientGuiEvent",
            packet_name: summary.packet_name,
            reason: "EE-only GuiEvent_Notify has no proven Diamond/1.69 handler",
        });
    }

    None
}

pub fn claim_or_rewrite_payload_if_verified(
    payload: &mut Vec<u8>,
    state: &mut SemanticSessionState,
) -> Option<ClientHighClaimSummary> {
    let high = HighLevel::parse(payload)?;

    if let Some(summary) = client_server_status::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientServerStatus",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientServerStatus,
        });
    }
    if let Some(summary) = client_char_list::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientCharList",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientCharList,
        });
    }
    if let Some(summary) = play_module_character_list::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "PlayModuleCharacterList",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::PlayModuleCharacterList,
        });
    }
    if let Some(summary) = client_login::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientLogin",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientLogin,
        });
    }
    if let Some(summary) = client_module::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientModule",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientModule,
        });
    }
    if let Some(summary) = client_gui_inventory::claim_or_rewrite_payload_if_verified(payload) {
        tracing::info!(
            packet_name = summary.packet_name,
            kind = ?summary.kind,
            object_id = ?summary.object_id,
            panel = ?summary.panel,
            player_inventory_gui = ?summary.player_inventory_gui,
            rewritten_self_object_id = summary.rewritten_self_object_id,
            "client GuiInventory payload validated for Diamond/1.69"
        );
        return Some(ClientHighClaimSummary {
            family_name: "ClientGuiInventory",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientGuiInventory,
        });
    }
    if let Some(summary) = client_character_sheet::claim_payload_if_verified(payload) {
        tracing::info!(
            packet_name = summary.packet_name,
            status = summary.status,
            object_id = %format_args!("0x{:08X}", summary.object_id),
            declared = summary.declared,
            fragment_bytes = summary.fragment_bytes,
            "client GuiCharacterSheet payload validated for Diamond/1.69"
        );
        return Some(ClientHighClaimSummary {
            family_name: "ClientCharacterSheet",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientCharacterSheet,
        });
    }
    if let Some(summary) =
        client_input::claim_or_rewrite_payload_if_verified_with_state(payload, state)
    {
        tracing::info!(
            packet_name = summary.packet_name,
            object_id = %format_args!("0x{:08X}", summary.primary_object_id),
            declared = summary.declared,
            fragment_bytes = summary.fragment_bytes,
            rewritten_self_object_id = summary.rewritten_self_object_id,
            rewritten_transition_door_close = summary.rewritten_transition_door_close,
            "client Input payload validated for Diamond/1.69"
        );
        return Some(ClientHighClaimSummary {
            family_name: "ClientInput",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientInput,
        });
    }
    if let Some(summary) = dialog::claim_client_payload_if_verified(payload) {
        tracing::info!(
            packet_name = high.name(),
            kind = ?summary.kind,
            declared = summary.declared,
            fragment_bytes = summary.fragment_bytes,
            "client Dialog payload validated for Diamond/1.69"
        );
        return Some(ClientHighClaimSummary {
            family_name: "Dialog",
            packet_name: high.name(),
            verified_family: VerifiedFamily::Dialog,
        });
    }
    if let Some(summary) = journal::claim_client_payload_if_verified(payload) {
        tracing::info!(
            packet_name = high.name(),
            minor = summary.minor,
            declared = summary.declared,
            fragment_bytes = summary.fragment_bytes,
            "client Journal payload validated for Diamond/1.69"
        );
        return Some(ClientHighClaimSummary {
            family_name: "ClientJournal",
            packet_name: high.name(),
            verified_family: VerifiedFamily::Journal,
        });
    }
    if let Some(summary) = client_quickbar::claim_payload_if_verified(payload) {
        tracing::info!(
            packet_name = summary.packet_name,
            slot = summary.slot,
            button_type = summary.button_type,
            body_kind = ?summary.body_kind,
            "client GuiQuickbar_SetButton payload validated as Diamond/1.69 receiver-compatible"
        );
        return Some(ClientHighClaimSummary {
            family_name: "ClientQuickbar",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientQuickbar,
        });
    }
    if let Some(summary) = client_area::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientArea",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientArea,
        });
    }
    if let Some(_summary) = party::claim_payload_if_verified(payload) {
        return Some(ClientHighClaimSummary {
            family_name: "ClientParty",
            packet_name: high.name(),
            verified_family: VerifiedFamily::ClientParty,
        });
    }

    tracing::warn!(
        major = high.major,
        minor = high.minor,
        name = high.name(),
        payload_len = payload.len(),
        prefix = %hex_prefix(payload, 64),
        "client high-level payload has no semantic owner"
    );
    None
}
