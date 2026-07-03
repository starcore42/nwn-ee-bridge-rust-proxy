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
    pub verified_family: VerifiedFamily,
    pub reason: &'static str,
}

pub fn claim_consumed_payload_if_verified(payload: &[u8]) -> Option<ClientHighConsumedSummary> {
    let high = HighLevel::parse(payload)?;

    if client_translator_may_claim_parsed_high_level("ClientGuiEvent", high)
        && let Some(summary) = client_gui_event::claim_payload_if_verified(payload)
    {
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
            verified_family: VerifiedFamily::ClientGuiEvent,
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

    if client_translator_may_claim_parsed_high_level("ClientServerStatus", high)
        && let Some(summary) = client_server_status::claim_payload_if_verified(payload)
    {
        return Some(ClientHighClaimSummary {
            family_name: "ClientServerStatus",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientServerStatus,
        });
    }
    if client_translator_may_claim_parsed_high_level("ClientCharList", high)
        && let Some(summary) = client_char_list::claim_payload_if_verified(payload)
    {
        return Some(ClientHighClaimSummary {
            family_name: "ClientCharList",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientCharList,
        });
    }
    if client_translator_may_claim_parsed_high_level("PlayModuleCharacterList", high)
        && let Some(summary) = play_module_character_list::claim_client_payload_if_verified(payload)
    {
        return Some(ClientHighClaimSummary {
            family_name: "PlayModuleCharacterList",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientPlayModuleCharacterList,
        });
    }
    if client_translator_may_claim_parsed_high_level("ClientLogin", high)
        && let Some(summary) = client_login::claim_payload_if_verified(payload)
    {
        return Some(ClientHighClaimSummary {
            family_name: "ClientLogin",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientLogin,
        });
    }
    if client_translator_may_claim_parsed_high_level("ClientModule", high)
        && let Some(summary) = client_module::claim_payload_if_verified(payload)
    {
        return Some(ClientHighClaimSummary {
            family_name: "ClientModule",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientModule,
        });
    }
    if client_translator_may_claim_parsed_high_level("ClientGuiInventory", high)
        && let Some(summary) = client_gui_inventory::claim_or_rewrite_payload_if_verified(payload)
    {
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
    if client_translator_may_claim_parsed_high_level("ClientCharacterSheet", high)
        && let Some(summary) = client_character_sheet::claim_payload_if_verified(payload)
    {
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
    if client_translator_may_claim_parsed_high_level("ClientInput", high)
        && let Some(summary) =
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
    if client_translator_may_claim_parsed_high_level("ClientDialog", high)
        && let Some(summary) = dialog::claim_client_payload_if_verified(payload)
    {
        tracing::info!(
            packet_name = high.name(),
            kind = ?summary.kind,
            declared = summary.declared,
            fragment_bytes = summary.fragment_bytes,
            "client Dialog payload validated for Diamond/1.69"
        );
        return Some(ClientHighClaimSummary {
            family_name: "ClientDialog",
            packet_name: high.name(),
            verified_family: VerifiedFamily::ClientDialog,
        });
    }
    if client_translator_may_claim_parsed_high_level("ClientJournal", high)
        && let Some(summary) = journal::claim_client_payload_if_verified(payload)
    {
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
            verified_family: VerifiedFamily::ClientJournal,
        });
    }
    if client_translator_may_claim_parsed_high_level("ClientQuickbar", high)
        && let Some(summary) = client_quickbar::claim_payload_if_verified(payload)
    {
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
    if client_translator_may_claim_parsed_high_level("ClientArea", high)
        && let Some(summary) = client_area::claim_payload_if_verified(payload)
    {
        return Some(ClientHighClaimSummary {
            family_name: "ClientArea",
            packet_name: summary.packet_name,
            verified_family: VerifiedFamily::ClientArea,
        });
    }
    if client_translator_may_claim_parsed_high_level("ClientParty", high)
        && let Some(_summary) = party::claim_client_payload_if_verified(payload)
    {
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

fn client_translator_may_claim_parsed_high_level(family_name: &str, high: HighLevel) -> bool {
    match family_name {
        "ClientServerStatus" => high.major == 0x01 && high.minor == 0x00,
        "ClientLogin" => high.major == 0x02 && matches!(high.minor, 0x0D | 0x11),
        "ClientModule" => high.major == 0x03 && high.minor == 0x02,
        "ClientArea" => high.major == 0x04 && high.minor == 0x03,
        "ClientInput" => {
            high.major == 0x06
                && matches!(
                    high.minor,
                    0x01 | 0x02
                        | 0x03
                        | 0x05
                        | 0x06
                        | 0x07
                        | 0x09
                        | 0x0A
                        | 0x0B
                        | 0x0C
                        | 0x0D
                        | 0x0E
                        | 0x10
                        | 0x11
                )
        }
        "ClientGuiInventory" => high.major == 0x0D && matches!(high.minor, 0x01 | 0x02),
        "ClientParty" => high.major == 0x0E && high.minor == 0x02,
        "ClientCharList" => high.major == 0x11 && matches!(high.minor, 0x01 | 0x03),
        "ClientDialog" => high.major == 0x14 && high.minor == 0x03,
        "ClientCharacterSheet" => high.major == 0x15 && high.minor == 0x01,
        "ClientJournal" => high.major == 0x1C && matches!(high.minor, 0x0A | 0x0B),
        "ClientQuickbar" => high.major == 0x1E && high.minor == 0x02,
        "PlayModuleCharacterList" => high.major == 0x31 && matches!(high.minor, 0x01 | 0x02),
        "ClientGuiEvent" => high.major == 0x35 && high.minor == 0x01,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn high(major: u8, minor: u8) -> HighLevel {
        HighLevel {
            envelope: 0x70,
            major,
            minor,
        }
    }

    #[test]
    fn client_translator_filter_accepts_owned_high_level_families() {
        for (family_name, high) in [
            ("ClientServerStatus", high(0x01, 0x00)),
            ("ClientLogin", high(0x02, 0x0D)),
            ("ClientLogin", high(0x02, 0x11)),
            ("ClientModule", high(0x03, 0x02)),
            ("ClientArea", high(0x04, 0x03)),
            ("ClientInput", high(0x06, 0x01)),
            ("ClientInput", high(0x06, 0x11)),
            ("ClientGuiInventory", high(0x0D, 0x02)),
            ("ClientParty", high(0x0E, 0x02)),
            ("ClientCharList", high(0x11, 0x03)),
            ("ClientDialog", high(0x14, 0x03)),
            ("ClientCharacterSheet", high(0x15, 0x01)),
            ("ClientJournal", high(0x1C, 0x0A)),
            ("ClientQuickbar", high(0x1E, 0x02)),
            ("PlayModuleCharacterList", high(0x31, 0x01)),
            ("ClientGuiEvent", high(0x35, 0x01)),
        ] {
            assert!(
                client_translator_may_claim_parsed_high_level(family_name, high),
                "{family_name} should probe owned client high-level {:02X}/{:02X}",
                high.major,
                high.minor
            );
        }
    }

    #[test]
    fn client_translator_filter_rejects_sibling_high_level_families() {
        for (family_name, high) in [
            ("ClientServerStatus", high(0x01, 0x03)),
            ("ClientLogin", high(0x02, 0x10)),
            ("ClientModule", high(0x03, 0x01)),
            ("ClientArea", high(0x04, 0x01)),
            ("ClientInput", high(0x06, 0x04)),
            ("ClientGuiInventory", high(0x0D, 0x03)),
            ("ClientParty", high(0x0E, 0x0E)),
            ("ClientCharList", high(0x11, 0x04)),
            ("ClientDialog", high(0x14, 0x01)),
            ("ClientCharacterSheet", high(0x15, 0x02)),
            ("ClientJournal", high(0x1C, 0x09)),
            ("ClientQuickbar", high(0x1E, 0x01)),
            ("PlayModuleCharacterList", high(0x31, 0x03)),
            ("ClientGuiEvent", high(0x35, 0x02)),
        ] {
            assert!(
                !client_translator_may_claim_parsed_high_level(family_name, high),
                "{family_name} must not probe unsupported client high-level {:02X}/{:02X}",
                high.major,
                high.minor
            );
        }
    }

    #[test]
    fn client_translator_filter_rejects_cross_family_major() {
        assert!(!client_translator_may_claim_parsed_high_level(
            "ClientInput",
            high(0x04, 0x03)
        ));
        assert!(!client_translator_may_claim_parsed_high_level(
            "ClientArea",
            high(0x06, 0x01)
        ));
    }

    #[test]
    fn client_dialog_reply_emits_client_dialog_family() {
        const READ_START: usize = 3 + 4;
        const DECLARED: usize = READ_START + 4 + 4 + 1 + 4;

        let mut payload = vec![0x70, 0x14, 0x03];
        payload.extend_from_slice(&(DECLARED as u32).to_le_bytes());
        payload.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.push(0x60);

        let mut state = SemanticSessionState::default();
        let claim = claim_or_rewrite_payload_if_verified(&mut payload, &mut state)
            .expect("client Dialog_Reply should be claimed");

        assert_eq!(claim.family_name, "ClientDialog");
        assert_eq!(claim.packet_name, "Dialog_Reply");
        assert_eq!(claim.verified_family, VerifiedFamily::ClientDialog);
    }

    #[test]
    fn client_play_module_character_list_emits_client_family_only_for_controls() {
        let mut state = SemanticSessionState::default();
        for minor in [0x01, 0x02] {
            let mut payload = vec![0x70, 0x31, minor];
            let claim = claim_or_rewrite_payload_if_verified(&mut payload, &mut state)
                .expect("client PlayModuleCharacterList control should be claimed");

            assert_eq!(claim.family_name, "PlayModuleCharacterList");
            assert_eq!(
                claim.verified_family,
                VerifiedFamily::ClientPlayModuleCharacterList
            );
        }

        let mut response = vec![
            0x70, 0x31, 0x03, 0x0B, 0x00, 0x00, 0x00, 0xF9, 0xFF, 0xFF, 0x7F, 0x80,
        ];
        assert!(
            claim_or_rewrite_payload_if_verified(&mut response, &mut state).is_none(),
            "PlayModuleCharacterList_Response is server-originated and must not claim as client"
        );
    }
}
