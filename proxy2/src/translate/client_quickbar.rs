//! Client-originated `GuiQuickbar_SetButton` semantic claims.
//!
//! This is intentionally an identity translator, not a raw passthrough. The
//! packet is emitted to the Diamond/1.69 server only after this module consumes
//! the exact slot/type shape shared by the EE and Diamond receivers.
//!
//! Decompile anchors:
//!
//! - EE `CNWSMessage::HandlePlayerToServerGuiQuickbar`
//!   (`nwn ee decompile.txt:0x140452103`) dispatches minor `0x02` from a CNW
//!   read message, then reads two `ReadBYTE(8, 1)` fields: slot and button
//!   type.
//! - EE `CNWSMessage::HandlePlayerToServerGuiQuickbar_SetButton`
//!   (`nwn ee decompile.txt:0x1404521D0`) switches on the type byte and reads
//!   the bounded type-specific body below.
//! - Diamond 1.69's stripped receiver at
//!   `nwn diamond decompile.txt:0x006D3F80` takes the same slot/type arguments
//!   and its jump table reads the same type-family bodies: item, spell,
//!   integer-param, CResRef/CExoString, command strings, and no-param buttons.
//!
//! The available Diamond names are stripped, so the strict proof here is the
//! receiver bytecode shape: each accepted type family advances the cursor by
//! exactly the fields read in both decompiles. Unknown/default types are not
//! claimed.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const QUICKBAR_MAJOR: u8 = 0x1E;
const SET_BUTTON_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const SLOT_BYTE_OFFSET: usize = READ_START;
const TYPE_BYTE_OFFSET: usize = READ_START + 1;
const SET_BUTTON_BODY_OFFSET: usize = READ_START + 2;
const QUICKBAR_SLOT_COUNT: u8 = 36;
const OBJECT_ID_BYTES: usize = 4;
const DWORD_BYTES: usize = 4;
const INT_BYTES: usize = 4;
const WORD_BYTES: usize = 2;
const BYTE_BYTES: usize = 1;
const BOOL_WIRE_BYTES: usize = 1;
const C_RESREF_BYTES: usize = 16;
const SINGLE_FRAGMENT_BYTE: usize = 1;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const MAX_REASONABLE_QUICKBAR_STRING_BYTES: usize = 4096;
const MAX_REASONABLE_SPELL_ID: u32 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientQuickbarClaimSummary {
    pub packet_name: &'static str,
    pub slot: u8,
    pub button_type: u8,
    pub body_kind: ClientQuickbarSetButtonKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientQuickbarSetButtonKind {
    NoParam,
    Item,
    Spell,
    SpellWithDomain,
    IntParam,
    ResRefString,
    CommandLine,
    ResRef,
    IntWordObject,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientQuickbarClaimSummary> {
    let parsed = parse_set_button_payload(payload)?;
    Some(ClientQuickbarClaimSummary {
        packet_name: "GuiQuickbar_SetButton",
        slot: parsed.slot,
        button_type: parsed.button_type,
        body_kind: parsed.body_kind,
    })
}

pub fn set_button_payload_shape_valid(payload: &[u8]) -> bool {
    parse_set_button_payload(payload).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedClientQuickbarSetButton {
    slot: u8,
    button_type: u8,
    body_kind: ClientQuickbarSetButtonKind,
}

fn parse_set_button_payload(payload: &[u8]) -> Option<ParsedClientQuickbarSetButton> {
    let high = HighLevel::parse(payload)?;
    if high.major != QUICKBAR_MAJOR || high.minor != SET_BUTTON_MINOR {
        return None;
    }
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < SET_BUTTON_BODY_OFFSET
        || payload.len() != declared.checked_add(SINGLE_FRAGMENT_BYTE)?
        || !empty_fragment_tail_valid(*payload.get(declared)?)
    {
        return None;
    }
    let slot = *payload.get(SLOT_BYTE_OFFSET)?;
    let button_type = *payload.get(TYPE_BYTE_OFFSET)?;
    if slot >= QUICKBAR_SLOT_COUNT {
        return None;
    }

    let (body_kind, cursor) =
        parse_set_button_body(payload, SET_BUTTON_BODY_OFFSET, declared, button_type)?;
    if cursor != declared {
        return None;
    }

    Some(ParsedClientQuickbarSetButton {
        slot,
        button_type,
        body_kind,
    })
}

fn parse_set_button_body(
    payload: &[u8],
    cursor: usize,
    declared: usize,
    button_type: u8,
) -> Option<(ClientQuickbarSetButtonKind, usize)> {
    if client_quickbar_type_has_no_payload(button_type) {
        return Some((ClientQuickbarSetButtonKind::NoParam, cursor));
    }

    if button_type == 1 {
        return parse_item_button_body(payload, cursor, declared)
            .map(|cursor| (ClientQuickbarSetButtonKind::Item, cursor));
    }

    if button_type == 2 {
        return parse_spell_button_body(payload, cursor, declared)
            .map(|cursor| (ClientQuickbarSetButtonKind::Spell, cursor));
    }

    if button_type == 44 {
        return parse_spell_with_domain_body(payload, cursor, declared)
            .map(|cursor| (ClientQuickbarSetButtonKind::SpellWithDomain, cursor));
    }

    if client_quickbar_type_has_int_payload(button_type) {
        return skip_bytes(payload, cursor, declared, INT_BYTES)
            .map(|cursor| (ClientQuickbarSetButtonKind::IntParam, cursor));
    }

    if button_type == 39 {
        return parse_int_word_object_body(payload, cursor, declared)
            .map(|cursor| (ClientQuickbarSetButtonKind::IntWordObject, cursor));
    }

    if (11..=17).contains(&button_type) {
        return parse_resref_string_body(payload, cursor, declared)
            .map(|cursor| (ClientQuickbarSetButtonKind::ResRefString, cursor));
    }

    if button_type == 18 {
        return parse_command_line_body(payload, cursor, declared)
            .map(|cursor| (ClientQuickbarSetButtonKind::CommandLine, cursor));
    }

    if button_type == 29 || button_type == 30 {
        return skip_bytes(payload, cursor, declared, C_RESREF_BYTES)
            .map(|cursor| (ClientQuickbarSetButtonKind::ResRef, cursor));
    }

    None
}

fn parse_item_button_body(payload: &[u8], mut cursor: usize, declared: usize) -> Option<usize> {
    cursor = skip_bytes(payload, cursor, declared, OBJECT_ID_BYTES)?;
    cursor = skip_bytes(payload, cursor, declared, INT_BYTES)?;
    let has_target_object = *payload.get(cursor)? != 0;
    cursor = skip_bytes(payload, cursor, declared, BOOL_WIRE_BYTES)?;
    if has_target_object {
        cursor = skip_bytes(payload, cursor, declared, OBJECT_ID_BYTES)?;
    }
    Some(cursor)
}

fn parse_spell_button_body(payload: &[u8], mut cursor: usize, declared: usize) -> Option<usize> {
    cursor = skip_bytes(payload, cursor, declared, BYTE_BYTES)?;
    let spell_id = read_le_u32(payload, cursor)?;
    if spell_id > MAX_REASONABLE_SPELL_ID {
        return None;
    }
    cursor = skip_bytes(payload, cursor, declared, DWORD_BYTES)?;
    cursor = skip_bytes(payload, cursor, declared, BYTE_BYTES)?;
    skip_bytes(payload, cursor, declared, BYTE_BYTES)
}

fn parse_spell_with_domain_body(
    payload: &[u8],
    mut cursor: usize,
    declared: usize,
) -> Option<usize> {
    let spell_id = read_le_u32(payload, cursor)?;
    if spell_id > MAX_REASONABLE_SPELL_ID {
        return None;
    }
    cursor = skip_bytes(payload, cursor, declared, DWORD_BYTES)?;
    skip_bytes(payload, cursor, declared, BYTE_BYTES)
}

fn parse_int_word_object_body(payload: &[u8], mut cursor: usize, declared: usize) -> Option<usize> {
    cursor = skip_bytes(payload, cursor, declared, INT_BYTES)?;
    cursor = skip_bytes(payload, cursor, declared, WORD_BYTES)?;
    skip_bytes(payload, cursor, declared, OBJECT_ID_BYTES)
}

fn parse_resref_string_body(payload: &[u8], mut cursor: usize, declared: usize) -> Option<usize> {
    cursor = skip_bytes(payload, cursor, declared, C_RESREF_BYTES)?;
    advance_c_exo_string(payload, cursor, declared)
}

fn parse_command_line_body(payload: &[u8], cursor: usize, declared: usize) -> Option<usize> {
    let cursor = advance_c_exo_string(payload, cursor, declared)?;
    advance_c_exo_string(payload, cursor, declared)
}

fn advance_c_exo_string(payload: &[u8], cursor: usize, declared: usize) -> Option<usize> {
    let length = usize::try_from(read_le_u32(payload, cursor)?).ok()?;
    if length > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
        return None;
    }
    let end = cursor.checked_add(DWORD_BYTES)?.checked_add(length)?;
    (end <= declared && payload.get(cursor..end).is_some()).then_some(end)
}

fn skip_bytes(payload: &[u8], cursor: usize, declared: usize, len: usize) -> Option<usize> {
    let end = cursor.checked_add(len)?;
    if end > declared {
        return None;
    }
    payload.get(cursor..end)?;
    Some(end)
}

fn empty_fragment_tail_valid(byte: u8) -> bool {
    usize::from((byte & 0xE0) >> 5) == CNW_FRAGMENT_HEADER_BITS
}

fn client_quickbar_type_has_no_payload(button_type: u8) -> bool {
    matches!(
        button_type,
        // Diamond 1.69 `0x006D3F80` and EE `0x1404521D0` both route these
        // type bytes directly to `SetQuickbarButton_GeneralNoParam`-style
        // handlers without additional CNW reads.
        0 | 6 | 7 | 19..=25 | 35 | 36 | 38 | 40 | 41
    )
}

fn client_quickbar_type_has_int_payload(button_type: u8) -> bool {
    matches!(
        button_type,
        // Both receiver jump tables read one 32-bit integer for these button
        // families, then check overflow before setting the quickbar slot.
        3 | 4 | 8 | 10 | 27 | 28 | 31 | 32 | 33 | 34 | 37 | 42 | 43 | 45 | 46 | 47 | 48
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_button_payload(slot: u8, button_type: u8, body: &[u8]) -> Vec<u8> {
        let declared = SET_BUTTON_BODY_OFFSET + body.len();
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0x70, 0x1E, 0x02]);
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.push(slot);
        payload.push(button_type);
        payload.extend_from_slice(body);
        payload.push(0x60);
        payload
    }

    #[test]
    fn claims_observed_no_param_cnw_wrapped_shape() {
        let payload = [0x70, 0x1E, 0x02, 0x09, 0x00, 0x00, 0x00, 0x05, 0x00, 0x70];

        let summary = claim_payload_if_verified(&payload)
            .expect("fresh XP2 no-param SetButton should be claimed");

        assert_eq!(summary.packet_name, "GuiQuickbar_SetButton");
        assert_eq!(summary.slot, 5);
        assert_eq!(summary.button_type, 0);
        assert_eq!(summary.body_kind, ClientQuickbarSetButtonKind::NoParam);
    }

    #[test]
    fn claims_observed_int_param_cnw_wrapped_shape() {
        let payload = [
            0x70, 0x1E, 0x02, 0x0D, 0x00, 0x00, 0x00, 0x05, 0x2B, 0x52, 0x01, 0xF0, 0x03, 0x70,
        ];

        let summary = claim_payload_if_verified(&payload)
            .expect("fresh XP2 int-param SetButton should be claimed");

        assert_eq!(summary.slot, 5);
        assert_eq!(summary.button_type, 43);
        assert_eq!(summary.body_kind, ClientQuickbarSetButtonKind::IntParam);
    }

    #[test]
    fn claims_observed_spell_with_domain_width() {
        let payload = set_button_payload(0, 44, &[0xE2, 0x04, 0x00, 0x00, 0x01]);

        let summary = claim_payload_if_verified(&payload).expect("type 44 should be claimed");

        assert_eq!(summary.packet_name, "GuiQuickbar_SetButton");
        assert_eq!(summary.slot, 0);
        assert_eq!(summary.button_type, 44);
        assert_eq!(
            summary.body_kind,
            ClientQuickbarSetButtonKind::SpellWithDomain
        );
    }

    #[test]
    fn claims_observed_item_without_target_width() {
        let payload = set_button_payload(
            1,
            1,
            &[0x45, 0x23, 0x01, 0x80, 0xFF, 0xFF, 0xFF, 0xFF, 0x00],
        );

        let summary = claim_payload_if_verified(&payload).expect("item false-target should claim");

        assert_eq!(summary.slot, 1);
        assert_eq!(summary.button_type, 1);
        assert_eq!(summary.body_kind, ClientQuickbarSetButtonKind::Item);
    }

    #[test]
    fn claims_spell_shape() {
        let payload = set_button_payload(2, 2, &[0x00, 0xE2, 0x04, 0x00, 0x00, 0x00, 0x00]);

        let summary = claim_payload_if_verified(&payload).expect("spell should claim");

        assert_eq!(summary.slot, 2);
        assert_eq!(summary.body_kind, ClientQuickbarSetButtonKind::Spell);
    }

    #[test]
    fn rejects_default_receiver_types() {
        let payload = set_button_payload(0, 5, &[]);

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_trailing_unconsumed_bytes() {
        let payload = set_button_payload(0, 44, &[1, 0, 0, 0, 0, 0]);

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn rejects_wrong_declared_or_fragment_cursor() {
        let mut wrong_declared = set_button_payload(0, 0, &[]);
        wrong_declared[3..7].copy_from_slice(&8u32.to_le_bytes());
        assert!(claim_payload_if_verified(&wrong_declared).is_none());

        let mut wrong_fragment = set_button_payload(0, 0, &[]);
        *wrong_fragment.last_mut().unwrap() = 0x80;
        assert!(claim_payload_if_verified(&wrong_fragment).is_none());
    }
}
