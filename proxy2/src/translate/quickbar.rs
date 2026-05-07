//! Quickbar packet transforms.
//!
//! The EE decompile identifies high-level `0x1E01` as
//! `GuiQuickbar_SetAllButtons` and `0x1E02` as `GuiQuickbar_SetButton`.
//! `CNWSMessage::SendServerToPlayerGuiQuickbar_SetButton` builds a
//! `CNWMessage`, obtains its write buffer with `GetWriteMessage`, then sends
//! family `0x1E` with minor `1` for the full bar or `2` for one button.
//!
//! Some 1.69 server captures carry the first four CNW fragment bytes directly
//! after `P 1E minor` instead of at EE's declared fragment offset. Given that
//! verified legacy quickbar transport shape, emit the EE CNW shape and leave
//! the button semantics untouched.
//!
//! The semantic quickbar rewrite below is deliberately narrow. The EE decompile
//! for `CNWSMessage::SendServerToPlayerGuiQuickbar_SetButton` confirms spell
//! buttons are emitted as:
//!
//! - type byte `2`
//! - spell class byte
//! - spell id DWORD
//! - metamagic byte
//! - domain byte
//!
//! Item buttons are much harder because EE's writer expands item appearance and
//! active-property records. Until the Rust bridge owns that full parser, strict
//! translation blanks item and unknown slots instead of forwarding bytes whose
//! EE layout has not been proven.

use crate::{
    crc::read_le_u32,
    packet::m::{HighLevel, MAX_REASONABLE_GAMEPLAY_PAYLOAD},
};

use super::cnw_message::{self, PrefixedFragmentsNormalizeSummary};
use std::{fs, sync::OnceLock};

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const QUICKBAR_MAJOR: u8 = 0x1E;
const SET_ALL_BUTTONS_MINOR: u8 = 0x01;
const LEGACY_QUICKBAR_BUTTON_COUNT: usize = 36;
const LEGACY_QUICKBAR_READ_CURSOR_START: usize = 4;
const C_RESREF_TEXT_BYTES: usize = 16;
const MAX_REASONABLE_QUICKBAR_STRING_BYTES: usize = 4096;
const MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES: usize = 32 * 1024;
const MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES: u8 = 128;

static QUICKBAR_BASE_ITEM_MODEL_TYPES: OnceLock<Option<Vec<i8>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct QuickbarRewriteSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub read_size: usize,
    pub fragment_size: usize,
    pub final_cursor: usize,
    pub trailing_read_bytes: usize,
    pub spells_preserved: u32,
    pub general_buttons_preserved: u32,
    pub item_buttons_blanked: u32,
    pub unsupported_buttons_blanked: u32,
    pub direct_opcode_stream: bool,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
}

#[derive(Debug, Clone)]
struct QuickbarParse {
    envelope: u8,
    declared: u32,
    read_size: usize,
    fragment_size: usize,
    final_cursor: usize,
    buttons: Vec<QuickbarButton>,
    direct_opcode_stream: bool,
}

#[derive(Debug, Clone)]
struct QuickbarButton {
    kind: QuickbarButtonKind,
}

#[derive(Debug, Clone)]
enum QuickbarButtonKind {
    Spell {
        spell_class: u8,
        spell_id: u32,
        metamagic: u8,
        domain: u8,
    },
    General {
        bytes: Vec<u8>,
    },
    ItemCandidate,
    Unsupported,
}

pub fn normalize_quickbar_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    cnw_message::normalize_prefixed_fragments_payload_for(payload, is_quickbar_family)
}

fn is_quickbar_family(high: HighLevel) -> bool {
    matches!(
        (high.major, high.minor),
        (0x1E, 0x01) | (0x1E, 0x02)
    )
}

pub fn full_set_all_buttons_target_length(payload: &[u8]) -> Option<usize> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    let target = declared.checked_add(1)?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || target <= payload.len()
        || target > MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES
        || target > MAX_REASONABLE_GAMEPLAY_PAYLOAD
    {
        return None;
    }
    Some(target)
}

pub fn rewrite_simple_quickbar_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<QuickbarRewriteSummary> {
    let parsed = parse_cnw_quickbar_payload(payload)
        .or_else(|| parse_direct_opcode_quickbar_stream(payload))?;
    let rewritten = build_ee_quickbar_payload(&parsed)?;
    if rewritten == *payload {
        return None;
    }

    let summary = QuickbarRewriteSummary {
        old_declared: parsed.declared,
        new_declared: read_le_u32(&rewritten, HIGH_LEVEL_HEADER_BYTES)?,
        read_size: parsed.read_size,
        fragment_size: parsed.fragment_size,
        final_cursor: parsed.final_cursor,
        trailing_read_bytes: parsed.read_size.saturating_sub(parsed.final_cursor),
        spells_preserved: parsed
            .buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
            .count() as u32,
        general_buttons_preserved: parsed
            .buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::General { .. }))
            .count() as u32,
        item_buttons_blanked: parsed
            .buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::ItemCandidate))
            .count() as u32,
        unsupported_buttons_blanked: parsed
            .buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Unsupported))
            .count() as u32,
        direct_opcode_stream: parsed.direct_opcode_stream,
        old_payload_length: payload.len(),
        new_payload_length: rewritten.len(),
    };
    if rewrite_summary_needs_more_quickbar_bytes(&summary) {
        // Do not turn a partial or false-positive quickbar-shaped stream into
        // an EE-valid empty bar. The harness has shown that live-object/zlib
        // continuations can contain `P 1E 01` followed by text bytes such as
        // "Mine"; until the full decompile-backed shape is available, strict
        // mode must leave those un-emitted rather than manufacturing a
        // misleading blank quickbar.
        return None;
    }
    *payload = rewritten;
    Some(summary)
}

pub fn normalize_and_rewrite_quickbar_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<(Option<PrefixedFragmentsNormalizeSummary>, QuickbarRewriteSummary)> {
    let normalize = normalize_quickbar_payload_if_needed(payload);
    let rewrite = rewrite_simple_quickbar_payload_if_possible(payload)?;
    Some((normalize, rewrite))
}

pub fn rewrite_summary_needs_more_quickbar_bytes(summary: &QuickbarRewriteSummary) -> bool {
    summary.spells_preserved == 0
        && summary.unsupported_buttons_blanked != 0
        && summary.trailing_read_bytes > 128
        && summary.old_payload_length > summary.new_payload_length.saturating_mul(4)
}

fn parse_cnw_quickbar_payload(payload: &[u8]) -> Option<QuickbarParse> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return None;
    }

    let declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let read_size = usize::try_from(declared.checked_sub(3)?).ok()?;
    let payload_size = payload.len().checked_sub(HIGH_LEVEL_HEADER_BYTES)?;
    if read_size < LEGACY_QUICKBAR_READ_CURSOR_START || read_size > payload_size {
        return None;
    }

    let read_offset = HIGH_LEVEL_HEADER_BYTES;
    let read_end = read_offset.checked_add(read_size)?;
    let read_buffer = payload.get(read_offset..read_end)?;
    let fragments = payload.get(read_end..)?;
    let (buttons, final_cursor) =
        parse_quickbar_read_buffer_with_fragments(
            read_buffer,
            fragments,
            LEGACY_QUICKBAR_READ_CURSOR_START,
        )
        .or_else(|| parse_quickbar_read_buffer(read_buffer, LEGACY_QUICKBAR_READ_CURSOR_START))?;

    Some(QuickbarParse {
        envelope: payload[0],
        declared,
        read_size,
        fragment_size: payload_size - read_size,
        final_cursor,
        buttons,
        direct_opcode_stream: false,
    })
}

fn parse_direct_opcode_quickbar_stream(payload: &[u8]) -> Option<QuickbarParse> {
    if payload.len() <= HIGH_LEVEL_HEADER_BYTES {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return None;
    }

    let read_buffer = &payload[HIGH_LEVEL_HEADER_BYTES..];
    let (buttons, final_cursor) = parse_quickbar_read_buffer(read_buffer, 0)?;
    let has_non_empty_content = buttons.iter().any(|button| {
        !matches!(
            button.kind,
            QuickbarButtonKind::General { ref bytes } if bytes.len() == 1 && bytes[0] == 0
        )
    });
    if !has_non_empty_content {
        return None;
    }

    Some(QuickbarParse {
        envelope: payload[0],
        declared: u32::try_from(read_buffer.len().checked_add(3)?).ok()?,
        read_size: read_buffer.len(),
        fragment_size: 0,
        final_cursor,
        buttons,
        direct_opcode_stream: true,
    })
}

fn parse_quickbar_read_buffer(
    read_buffer: &[u8],
    mut cursor: usize,
) -> Option<(Vec<QuickbarButton>, usize)> {
    let mut buttons = Vec::with_capacity(LEGACY_QUICKBAR_BUTTON_COUNT);
    for slot in 0..LEGACY_QUICKBAR_BUTTON_COUNT {
        if cursor >= read_buffer.len() {
            buttons.extend((slot..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| QuickbarButton {
                kind: QuickbarButtonKind::Unsupported,
            }));
            break;
        }

        let ty = read_buffer[cursor];
        if ty == 1 {
            buttons.push(QuickbarButton {
                kind: QuickbarButtonKind::ItemCandidate,
            });
            cursor = choose_legacy_quickbar_item_end(read_buffer, slot, cursor)
                .filter(|next_cursor| *next_cursor > cursor)
                .unwrap_or_else(|| cursor.saturating_add(1));
            continue;
        }

        let parsed = parse_legacy_quickbar_non_item(read_buffer, cursor).or_else(|| {
            let resync_cursor = find_legacy_quickbar_resync(read_buffer, slot, cursor)?;
            parse_legacy_quickbar_non_item(read_buffer, resync_cursor)
        });
        let (button, next_cursor) = parsed.unwrap_or((
            QuickbarButton {
                kind: QuickbarButtonKind::Unsupported,
            },
            cursor.saturating_add(1),
        ));
        if next_cursor <= cursor || next_cursor > read_buffer.len() {
            return None;
        }
        buttons.push(button);
        cursor = next_cursor;
    }

    if buttons.len() != LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }
    Some((buttons, cursor.min(read_buffer.len())))
}

fn parse_quickbar_read_buffer_with_fragments(
    read_buffer: &[u8],
    fragments: &[u8],
    cursor: usize,
) -> Option<(Vec<QuickbarButton>, usize)> {
    if fragments.is_empty() {
        return None;
    }
    let model_types = quickbar_base_item_model_types()?;
    let mut reader = QuickbarPacketReader {
        read_buffer,
        fragments,
        cursor,
        fragment_cursor: 0,
        fragment_bit: 0,
        final_fragment_bits: 0,
    };
    reader.final_fragment_bits = reader.read_bits(3)? as u8;

    let mut buttons = Vec::with_capacity(LEGACY_QUICKBAR_BUTTON_COUNT);
    for slot in 0..LEGACY_QUICKBAR_BUTTON_COUNT {
        if reader.cursor >= read_buffer.len() {
            buttons.extend((slot..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| QuickbarButton {
                kind: QuickbarButtonKind::Unsupported,
            }));
            break;
        }

        let ty = reader.read_byte()?;
        if ty == 1 {
            skip_legacy_quickbar_item_payload(&mut reader, model_types)?;
            buttons.push(QuickbarButton {
                kind: QuickbarButtonKind::ItemCandidate,
            });
            continue;
        }

        let kind = parse_legacy_quickbar_non_item_from_reader(&mut reader, ty)?;
        buttons.push(QuickbarButton { kind });
    }

    if buttons.len() != LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }
    Some((buttons, reader.cursor.min(read_buffer.len())))
}

#[derive(Clone)]
struct QuickbarPacketReader<'a> {
    read_buffer: &'a [u8],
    fragments: &'a [u8],
    cursor: usize,
    fragment_cursor: usize,
    fragment_bit: u8,
    final_fragment_bits: u8,
}

impl<'a> QuickbarPacketReader<'a> {
    fn read_bit(&mut self) -> Option<bool> {
        if self.fragment_cursor >= self.fragments.len() || self.fragment_bit >= 8 {
            return None;
        }
        let bit = (self.fragments[self.fragment_cursor] >> (7 - self.fragment_bit)) & 1;
        self.fragment_bit += 1;
        if self.fragment_bit >= 8 {
            self.fragment_bit = 0;
            self.fragment_cursor += 1;
        }
        Some(bit != 0)
    }

    fn read_bits(&mut self, bit_count: u8) -> Option<u32> {
        if bit_count > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..bit_count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Some(value)
    }

    fn read_byte(&mut self) -> Option<u8> {
        let value = *self.read_buffer.get(self.cursor)?;
        self.cursor += 1;
        Some(value)
    }

    fn read_word(&mut self) -> Option<u16> {
        let end = self.cursor.checked_add(2)?;
        let chunk = self.read_buffer.get(self.cursor..end)?;
        self.cursor = end;
        Some(u16::from_le_bytes(chunk.try_into().ok()?))
    }

    fn read_dword(&mut self) -> Option<u32> {
        let end = self.cursor.checked_add(CNW_LENGTH_BYTES)?;
        let value = read_u32_le(self.read_buffer, self.cursor)?;
        self.cursor = end;
        Some(value)
    }

    fn skip_bytes(&mut self, count: usize) -> Option<()> {
        let end = self.cursor.checked_add(count)?;
        self.read_buffer.get(self.cursor..end)?;
        self.cursor = end;
        Some(())
    }

    fn skip_string(&mut self) -> Option<()> {
        let length = usize::try_from(self.read_dword()?).ok()?;
        if length > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
            return None;
        }
        self.skip_bytes(length)
    }

    fn skip_loc_string(&mut self) -> Option<()> {
        let custom_tlk = self.read_bit()?;
        if custom_tlk {
            let _language_selector = self.read_bit()?;
            let _string_ref = self.read_dword()?;
            Some(())
        } else {
            self.skip_string()
        }
    }
}

fn parse_legacy_quickbar_non_item_from_reader(
    reader: &mut QuickbarPacketReader<'_>,
    ty: u8,
) -> Option<QuickbarButtonKind> {
    if !is_legacy_quickbar_plausible_type(ty) || ty == 1 {
        return None;
    }

    let start = reader.cursor.checked_sub(1)?;
    if legacy_quickbar_type_has_no_payload(ty) {
        return Some(QuickbarButtonKind::General { bytes: vec![ty] });
    }

    if ty == 2 {
        let spell_class = reader.read_byte()?;
        let spell_id = reader.read_dword()?;
        let metamagic = reader.read_byte()?;
        let domain = reader.read_byte()?;
        if spell_id > 10_000 {
            return Some(QuickbarButtonKind::Unsupported);
        }
        return Some(QuickbarButtonKind::Spell {
            spell_class,
            spell_id,
            metamagic,
            domain,
        });
    }

    if legacy_quickbar_type_has_int_payload(ty) {
        reader.skip_bytes(CNW_LENGTH_BYTES)?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if ty == 44 {
        reader.skip_bytes(CNW_LENGTH_BYTES + 1)?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if (11..=17).contains(&ty) {
        reader.skip_bytes(C_RESREF_TEXT_BYTES)?;
        reader.skip_string()?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if ty == 18 {
        reader.skip_string()?;
        reader.skip_string()?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if ty == 29 || ty == 30 {
        reader.skip_bytes(C_RESREF_TEXT_BYTES)?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    None
}

fn skip_legacy_quickbar_item_payload(
    reader: &mut QuickbarPacketReader<'_>,
    model_types: &[i8],
) -> Option<()> {
    let before = reader.clone();
    if skip_legacy_quickbar_item_object(reader, true, model_types).is_some()
        && skip_legacy_quickbar_item_object(reader, false, model_types).is_some()
    {
        return Some(());
    }
    *reader = before;
    None
}

fn skip_legacy_quickbar_item_object(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<()> {
    let present = reader.read_bit()?;
    if !present {
        return Some(());
    }

    let _object_id = reader.read_dword()?;
    if include_int_param {
        let _int_param = reader.read_dword()?;
    }
    let base_item_id = reader.read_dword()?;
    let model_type = *model_types.get(usize::try_from(base_item_id).ok()?)?;
    let appearance_size = legacy_item_appearance_read_size(model_type)?;
    reader.skip_bytes(appearance_size.checked_sub(CNW_LENGTH_BYTES)?)?;
    skip_legacy_quickbar_active_item_properties(reader, base_item_id)
}

fn skip_legacy_quickbar_active_item_properties(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<()> {
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        let _armor_word = reader.read_word()?;
    }

    if reader.read_bit()? {
        reader.skip_loc_string()?;
    } else {
        reader.skip_string()?;
    }

    let _post_name_bool1 = reader.read_bit()?;
    let _cost = reader.read_dword()?;
    let _stack_or_charges = reader.read_dword()?;
    let _post_name_bool2 = reader.read_bit()?;
    let _post_name_bool3 = reader.read_bit()?;
    let _post_name_bool4 = reader.read_bit()?;
    let property_count = reader.read_byte()?;
    if property_count > MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES {
        return None;
    }
    for _ in 0..property_count {
        let _property = reader.read_word()?;
        let _subtype = reader.read_word()?;
        let _cost_table_value = reader.read_word()?;
        let _param = reader.read_byte()?;
    }

    let _state_mask = reader.read_byte()?;
    let value_mask = reader.read_byte()?;
    for bit in 0..8 {
        if (value_mask & (1u8 << bit)) != 0 {
            let _value = reader.read_byte()?;
        }
    }
    Some(())
}

fn parse_legacy_quickbar_non_item(
    read_buffer: &[u8],
    cursor: usize,
) -> Option<(QuickbarButton, usize)> {
    let ty = *read_buffer.get(cursor)?;
    if !is_legacy_quickbar_plausible_type(ty) || ty == 1 {
        return None;
    }

    if legacy_quickbar_type_has_no_payload(ty) {
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General {
                    bytes: vec![ty],
                },
            },
            cursor + 1,
        ));
    }

    let payload_cursor = cursor.checked_add(1)?;
    if ty == 2 {
        let spell_class = *read_buffer.get(payload_cursor)?;
        let spell_id = read_u32_le(read_buffer, payload_cursor + 1)?;
        let metamagic = *read_buffer.get(payload_cursor + 5)?;
        let domain = *read_buffer.get(payload_cursor + 6)?;
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::Spell {
                    spell_class,
                    spell_id,
                    metamagic,
                    domain,
                },
            },
            payload_cursor + 7,
        ));
    }

    if legacy_quickbar_type_has_int_payload(ty) {
        let next_cursor = payload_cursor.checked_add(4)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if ty == 44 {
        let next_cursor = payload_cursor.checked_add(5)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if (11..=17).contains(&ty) {
        let after_resref = payload_cursor.checked_add(C_RESREF_TEXT_BYTES)?;
        let next_cursor = advance_legacy_quickbar_string(read_buffer, after_resref)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if ty == 18 {
        let after_first = advance_legacy_quickbar_string(read_buffer, payload_cursor)?;
        let next_cursor = advance_legacy_quickbar_string(read_buffer, after_first)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if ty == 29 || ty == 30 {
        let next_cursor = payload_cursor.checked_add(C_RESREF_TEXT_BYTES)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    None
}

fn build_ee_quickbar_payload(parsed: &QuickbarParse) -> Option<Vec<u8>> {
    let mut read_buffer = vec![0, 0, 0, 0];
    for button in &parsed.buttons {
        match button.kind {
            QuickbarButtonKind::Spell {
                spell_class,
                spell_id,
                metamagic,
                domain,
            } => {
                read_buffer.push(2);
                read_buffer.push(spell_class);
                read_buffer.extend_from_slice(&spell_id.to_le_bytes());
                read_buffer.push(metamagic);
                read_buffer.push(domain);
            }
            QuickbarButtonKind::General { ref bytes } => read_buffer.extend_from_slice(bytes),
            QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported => {
                read_buffer.push(0);
            }
        }
    }

    let declared = u32::try_from(read_buffer.len().checked_add(3)?).ok()?;
    write_u32_le(&mut read_buffer, 0, declared)?;
    let fragments = quickbar_fragment_header_only();

    let new_len = HIGH_LEVEL_HEADER_BYTES
        .checked_add(read_buffer.len())?
        .checked_add(fragments.len())?;
    if new_len > MAX_REASONABLE_GAMEPLAY_PAYLOAD {
        return None;
    }

    let mut payload = Vec::with_capacity(new_len);
    payload.push(parsed.envelope);
    payload.push(QUICKBAR_MAJOR);
    payload.push(SET_ALL_BUTTONS_MINOR);
    payload.extend_from_slice(&read_buffer);
    payload.extend_from_slice(&fragments);
    Some(payload)
}

fn quickbar_fragment_header_only() -> [u8; 1] {
    // Three MSB-first fragment bits are reserved for "final fragment bits".
    // With no additional bit fields, the consumed bit count is 3, so the
    // header value is binary 011 packed into the high bits of the first byte.
    [0x60]
}

fn choose_legacy_quickbar_item_end(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
) -> Option<usize> {
    let remaining_slots_after_this = LEGACY_QUICKBAR_BUTTON_COUNT.checked_sub(slot + 1)?;
    let item_payload_start = cursor.checked_add(1)?;
    let min_candidate = read_buffer.len().min(item_payload_start.checked_add(8)?);
    let max_candidate = read_buffer.len().min(item_payload_start.checked_add(512)?);
    let mut best_score = i32::MIN;
    let mut best_candidate = None;

    for candidate in min_candidate..=max_candidate {
        if candidate.checked_add(remaining_slots_after_this)? > read_buffer.len() {
            break;
        }
        if remaining_slots_after_this > 0
            && (candidate >= read_buffer.len()
                || !is_legacy_quickbar_plausible_type(read_buffer[candidate]))
        {
            continue;
        }

        let mut score = if remaining_slots_after_this == 0 {
            100
        } else {
            score_legacy_quickbar_candidate_window(
                read_buffer,
                candidate,
                remaining_slots_after_this,
            )?
        };
        if remaining_slots_after_this > 0 {
            let next_type = read_buffer[candidate];
            if next_type == 2 {
                score += 35;
            } else if next_type == 0 {
                score += 2;
            } else if legacy_quickbar_type_has_no_payload(next_type) {
                score += 8;
            }
        }
        let skipped = candidate.saturating_sub(item_payload_start);
        score -= skipped.checked_div(32).unwrap_or(0).min(80) as i32;
        if score > best_score {
            best_score = score;
            best_candidate = Some(candidate);
        }
    }

    best_candidate
}

fn score_legacy_quickbar_candidate_window(
    read_buffer: &[u8],
    mut cursor: usize,
    remaining_slots: usize,
) -> Option<i32> {
    if cursor >= read_buffer.len() || remaining_slots == 0 {
        return None;
    }

    let mut score = 0;
    let slots_to_probe = remaining_slots.min(8);
    for probe in 0..slots_to_probe {
        if cursor >= read_buffer.len() {
            return if probe == 0 { None } else { Some(score - 20) };
        }

        let ty = read_buffer[cursor];
        if !is_legacy_quickbar_plausible_type(ty) {
            return None;
        }
        if ty == 1 || ty == 9 {
            return Some(score + 12);
        }

        let (button, next_cursor) = parse_legacy_quickbar_non_item(read_buffer, cursor)?;
        if next_cursor <= cursor {
            return None;
        }
        match button.kind {
            QuickbarButtonKind::Spell { .. } => score += 80,
            QuickbarButtonKind::General { ref bytes } if bytes.len() == 1 && bytes[0] == 0 => {
                score += 8
            }
            QuickbarButtonKind::General { .. } => score += 4,
            QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported => return None,
        }
        cursor = next_cursor;
    }

    Some(score)
}

fn find_legacy_quickbar_resync(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
) -> Option<usize> {
    let remaining_slots = LEGACY_QUICKBAR_BUTTON_COUNT.checked_sub(slot)?;
    let max_candidate = read_buffer.len().min(cursor.checked_add(2048)?);
    let mut best_score = i32::MIN;
    let mut best_candidate = None;

    for candidate in cursor.saturating_add(1)..max_candidate {
        let ty = read_buffer[candidate];
        if !is_legacy_quickbar_plausible_type(ty) || ty == 1 {
            continue;
        }

        let mut score =
            score_legacy_quickbar_candidate_window(read_buffer, candidate, remaining_slots)?;
        if ty == 2 {
            score += 120;
        } else if ty == 0 {
            score += 4;
        } else if legacy_quickbar_type_has_no_payload(ty) {
            score += 12;
        }
        let skipped = candidate.saturating_sub(cursor);
        score -= skipped.checked_div(64).unwrap_or(0).min(40) as i32;

        if score > best_score {
            best_score = score;
            best_candidate = Some(candidate);
        }
    }

    if best_score >= 80 {
        best_candidate
    } else {
        None
    }
}

fn advance_legacy_quickbar_string(read_buffer: &[u8], cursor: usize) -> Option<usize> {
    let length = usize::try_from(read_u32_le(read_buffer, cursor)?).ok()?;
    if length > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
        return None;
    }
    cursor.checked_add(CNW_LENGTH_BYTES)?.checked_add(length)
}

fn is_legacy_quickbar_plausible_type(ty: u8) -> bool {
    ty <= 48
}

fn legacy_quickbar_type_has_no_payload(ty: u8) -> bool {
    matches!(
        ty,
        0 | 5 | 6 | 7 | 9 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 35 | 36 | 38 | 40 | 41
    )
}

fn legacy_quickbar_type_has_int_payload(ty: u8) -> bool {
    matches!(
        ty,
        3 | 4 | 8 | 10 | 27 | 28 | 31 | 32 | 33 | 34 | 37 | 42 | 43 | 45 | 46 | 47 | 48
    )
}

fn quickbar_base_item_model_types() -> Option<&'static [i8]> {
    QUICKBAR_BASE_ITEM_MODEL_TYPES
        .get_or_init(load_quickbar_base_item_model_types)
        .as_deref()
}

fn load_quickbar_base_item_model_types() -> Option<Vec<i8>> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("HG_BRIDGE_BASEITEMS_PATH") {
        if !path.trim().is_empty() {
            candidates.push(path);
        }
    }
    candidates.extend([
        "NWN Diamond\\1.72 builder resources\\auto2damerger\\merged\\baseitems.2da".to_string(),
        "NWN Diamond\\1.72 builder resources\\1.72 full 2dasource\\baseitems.2da".to_string(),
        "NWN Diamond\\1.72 builder resources\\1.72-hak 2das\\baseitems.2da".to_string(),
        "NWN Diamond\\1.72 builder resources\\1.72-only 2das\\baseitems.2da".to_string(),
    ]);

    for path in candidates {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(model_types) = parse_base_item_model_types_2da(&contents) {
            return Some(model_types);
        }
    }

    None
}

fn parse_base_item_model_types_2da(contents: &str) -> Option<Vec<i8>> {
    let mut model_types = Vec::new();
    let mut model_type_column = None;
    let mut have_header = false;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with('#')
            || line.eq_ignore_ascii_case("2DA V2.0")
        {
            continue;
        }

        let tokens = line.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }

        if !have_header {
            model_type_column = tokens.iter().position(|token| {
                matches!(
                    token.to_ascii_lowercase().as_str(),
                    "modeltype" | "appearancetype" | "appearance_type"
                )
            });
            have_header = true;
            model_type_column?;
            continue;
        }

        let row = tokens.first()?.parse::<usize>().ok()?;
        if row > 65_535 {
            continue;
        }
        let value_index = model_type_column?.checked_add(1)?;
        let Some(raw_model_type) = tokens.get(value_index) else {
            continue;
        };
        let Ok(model_type) = raw_model_type.parse::<i8>() else {
            continue;
        };
        if !(0..=3).contains(&model_type) {
            continue;
        }
        if model_types.len() <= row {
            model_types.resize(row + 1, -1);
        }
        model_types[row] = model_type;
    }

    if model_types.iter().any(|model_type| *model_type >= 0) {
        Some(model_types)
    } else {
        None
    }
}

fn legacy_item_appearance_read_size(model_type: i8) -> Option<usize> {
    match model_type {
        0 => Some(CNW_LENGTH_BYTES + 1),
        1 => Some(CNW_LENGTH_BYTES + 1 + 6),
        2 => Some(CNW_LENGTH_BYTES + 3 + 1),
        3 => Some(CNW_LENGTH_BYTES + 19 + 6),
        _ => None,
    }
}

fn legacy_quickbar_base_item_requires_active_property_word(base_item_id: u32) -> bool {
    base_item_id == 0x10
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset.checked_add(CNW_LENGTH_BYTES)?)?;
    Some(u32::from_le_bytes(chunk.try_into().ok()?))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let target = bytes.get_mut(offset..offset.checked_add(CNW_LENGTH_BYTES)?)?;
    target.copy_from_slice(&value.to_le_bytes());
    Some(())
}
