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
//!
//! The item-object reader is still decompile-shaped: the quickbar writer calls
//! the item appearance writer, whose byte count is selected from the active
//! `baseitems.2da` `ModelType` row. Higher Ground/CEP extend that table beyond
//! Diamond's stock rows, so the Rust bridge must resolve `baseitems.2da` from
//! the staged HAK/resource profile before judging an item update as unknown.

use crate::{
    crc::read_le_u32,
    packet::m::{HighLevel, MAX_REASONABLE_GAMEPLAY_PAYLOAD},
};

use super::cnw_message::PrefixedFragmentsNormalizeSummary;
use std::{
    fs,
    path::PathBuf,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
const QUICKBAR_MAJOR: u8 = 0x1E;
const SET_ALL_BUTTONS_MINOR: u8 = 0x01;
const LEGACY_QUICKBAR_BUTTON_COUNT: usize = 36;
const LEGACY_QUICKBAR_READ_CURSOR_START: usize = 4;
const C_RESREF_TEXT_BYTES: usize = 16;
const MAX_REASONABLE_QUICKBAR_STRING_BYTES: usize = 4096;
const MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES: usize = 32 * 1024;
const MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES: u8 = 128;
const MAX_QUICKBAR_ITEM_PRESENCE_RESYNC_BITS: u8 = 5;
const MAX_QUICKBAR_BARE_ACTIVE_ITEM_NAME_BYTES: usize = 128;
const MAX_QUICKBAR_FOUR_PREFIX_FRAGMENT_TAIL_BYTES: usize = 512;
const ERF_KEY_ENTRY_BYTES: usize = 24;
const ERF_RESOURCE_ENTRY_BYTES: usize = 8;
const ERF_HEADER_MIN_BYTES: usize = 160;
const ERF_ENTRY_COUNT_OFFSET: usize = 16;
const ERF_KEY_LIST_OFFSET_OFFSET: usize = 24;
const ERF_RESOURCE_LIST_OFFSET_OFFSET: usize = 28;
const ERF_KEY_RESREF_BYTES: usize = 16;
const ERF_KEY_RESOURCE_ID_OFFSET: usize = 16;
const ERF_KEY_RESOURCE_TYPE_OFFSET: usize = 20;
const ERF_RESOURCE_2DA_TYPE: u16 = 2017;
const MAX_REASONABLE_ERF_KEY_COUNT: usize = 100_000;
const QUICKBAR_BAD_SCORE: i32 = -1_000_000_000;
const QUICKBAR_UNKNOWN_SCORE: i32 = i32::MIN;
const EE_QUICKBAR_ANIMATION_ICON_COUNT: u32 = 23;
const EE_LIVE_ARMOR_PART_COUNT: usize = 19;
const EE_LIVE_ARMOR_LAYER_COUNT: usize = 6;
const EE_LIVE_EXTENDED_ARMOR_TABLE_BYTES: usize =
    EE_LIVE_ARMOR_PART_COUNT * EE_LIVE_ARMOR_LAYER_COUNT;
const EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_DWORDS: [u32; 10] = [
    0x3F80_0000,
    0x3F80_0000,
    0x3F80_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x0000_0000,
    0x3F80_0000,
];

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
    pub item_buttons_translated: u32,
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
    Item {
        primary: QuickbarItemObject,
        secondary: QuickbarItemObject,
        recovered_type_tag: bool,
    },
    ItemCandidate,
    Unsupported,
}

#[derive(Debug, Clone, Default)]
struct QuickbarLocStringField {
    custom_tlk: bool,
    language_selector: bool,
    string_ref: u32,
    text: Vec<u8>,
}

#[derive(Debug, Clone)]
struct QuickbarActivePropertyEntry {
    property: u16,
    subtype: u16,
    cost_table_value: u16,
    param: u8,
}

#[derive(Debug, Clone, Default)]
struct QuickbarActiveItemProperties {
    has_armor_word: bool,
    armor_word: u16,
    name_is_locstring: bool,
    locstring_name: QuickbarLocStringField,
    string_name: Vec<u8>,
    post_name_bool1: bool,
    cost: u32,
    stack_or_charges: u32,
    post_name_bool2: bool,
    post_name_bool3: bool,
    post_name_bool4: bool,
    properties: Vec<QuickbarActivePropertyEntry>,
    state_mask: u8,
    value_mask: u8,
    value_mask_bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct QuickbarItemObject {
    present: bool,
    object_id: u32,
    int_param: i32,
    base_item: u32,
    appearance_type: i8,
    active_props: Option<QuickbarActiveItemProperties>,
    appearance_bytes: Vec<u8>,
}

impl Default for QuickbarItemObject {
    fn default() -> Self {
        Self {
            present: false,
            object_id: 0x7F00_0000,
            int_param: -1,
            base_item: u32::MAX,
            appearance_type: -1,
            active_props: None,
            appearance_bytes: Vec::new(),
        }
    }
}

pub fn normalize_quickbar_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    normalize_quickbar_scanned_tail_payload_if_needed(payload)
        .or_else(|| normalize_quickbar_four_prefixed_fragments_payload_if_needed(payload))
        .or_else(|| normalize_quickbar_prefixed_short_declared_payload_if_needed(payload))
}

fn is_quickbar_family(high: HighLevel) -> bool {
    matches!(
        (high.major, high.minor),
        (0x1E, 0x01) | (0x1E, 0x02)
    )
}

fn normalize_quickbar_prefixed_short_declared_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    // HG's Diamond quickbar capture is not the generic four-leading-fragment
    // CNW shape. The observed wire form is:
    //
    // `P 1E 01 [two leading fragment bytes] [u16 legacy declared] [read body] [fragment tail]`
    //
    // EE's decompiled `GuiQuickbar_SetAllButtons` reader consumes a normal
    // CNW read buffer: a 32-bit declared length at CNW offset 0, the read body
    // starting at offset 4, and all fragment bits after the declared read
    // window. Therefore the verified translation is:
    //
    // `P 1E 01 [u32 normalized declared] [read body] [two leading fragment bytes] [fragment tail]`
    //
    // Do not send this through the generic four-byte prefixed-fragment repair:
    // that misclassifies the u16 length bytes as fragment bytes and produces a
    // syntactically valid but semantically empty quickbar.
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + 2 + 2 + 1 {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if !is_quickbar_family(high) {
        return None;
    }

    let old_wire_declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let legacy_declared =
        usize::from(read_u16_le(payload, HIGH_LEVEL_HEADER_BYTES.checked_add(2)?)?);
    if legacy_declared < HIGH_LEVEL_HEADER_BYTES + 2 {
        return None;
    }
    let legacy_read_size = legacy_declared.checked_sub(HIGH_LEVEL_HEADER_BYTES)?;
    if legacy_read_size < 2 + LEGACY_QUICKBAR_READ_CURSOR_START {
        return None;
    }

    let legacy_read_start = HIGH_LEVEL_HEADER_BYTES.checked_add(2)?;
    let legacy_read_end = legacy_read_start.checked_add(legacy_read_size)?;
    if legacy_read_end > payload.len() {
        return None;
    }

    let normalized_read_size = legacy_read_size.checked_add(2)?;
    let normalized_declared = u32::try_from(normalized_read_size.checked_add(3)?).ok()?;
    let prefix_fragment_bytes = [
        *payload.get(HIGH_LEVEL_HEADER_BYTES)?,
        *payload.get(HIGH_LEVEL_HEADER_BYTES + 1)?,
        0,
        0,
    ];

    let mut rewritten = Vec::with_capacity(payload.len().checked_add(2)?);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&normalized_declared.to_le_bytes());
    rewritten.extend_from_slice(payload.get(legacy_read_start + 2..legacy_read_end)?);
    rewritten.extend_from_slice(&prefix_fragment_bytes[..2]);
    rewritten.extend_from_slice(payload.get(legacy_read_end..)?);

    let old_payload_length = payload.len();
    let new_payload_length = rewritten.len();
    *payload = rewritten;

    Some(PrefixedFragmentsNormalizeSummary {
        major: high.major,
        minor: high.minor,
        old_wire_declared,
        new_declared: normalized_declared,
        old_payload_length,
        new_payload_length,
        prefixed_fragment_bytes: prefix_fragment_bytes,
        read_bytes_offset: legacy_read_start,
        read_bytes_length: normalized_read_size,
    })
}

fn normalize_quickbar_scanned_tail_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    // HG/Diamond quickbar captures can start with a stale/transport DWORD after
    // `P 1E 01`, followed by the real 36-slot read buffer and the actual CNW
    // fragment tail. The EE and Diamond decompiles agree that
    // `GuiQuickbar_SetAllButtons` reads exactly 36 slot records and that the
    // all-buttons path does not carry a slot index byte. Use that semantic
    // boundary as the authority here: the legacy DWORD is accepted only as a
    // transport marker, never as the EE CNW declared length.
    //
    // Observed HG shape:
    //
    // `P 1E 01 [legacy transport/stale length DWORD] [36-slot read body] [fragment tail]`
    //
    // Verified EE emission:
    //
    // `P 1E 01 [u32 normalized declared] [36-slot read body] [fragment tail]`
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES + LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return None;
    }

    let old_wire_declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if old_wire_declared >= (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u32
        && (old_wire_declared as usize) <= payload.len()
    {
        return None;
    }

    let body_and_tail = payload.get(HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..)?;
    let Some(candidate) =
        choose_quickbar_split(body_and_tail, &[], QuickbarSplitPolicy::ExactSemantic)
    else {
        tracing::warn!(
            old_wire_declared,
            old_payload_length = payload.len(),
            "server GuiQuickbar_SetAllButtons scanned-tail transport rejected: no exact semantic quickbar split"
        );
        return None;
    };
    if !candidate.preserves_meaningful_button_payload() {
        tracing::warn!(
            old_wire_declared,
            read_body_len = candidate.read_body_len,
            fragment_tail_len = candidate.fragment_tail_len,
            translated_item_slots = candidate.translated_item_slots,
            spell_slots = candidate.spell_slots,
            general_slots = candidate.general_slots,
            item_candidate_slots = candidate.item_candidate_slots,
            unsupported_slots = candidate.unsupported_slots,
            "server GuiQuickbar_SetAllButtons scanned-tail transport rejected: no decompile-owned item or spell payload survived"
        );
        return None;
    }
    let normalized_read_size = candidate.read_body_len.checked_add(CNW_LENGTH_BYTES)?;
    let normalized_declared = u32::try_from(normalized_read_size.checked_add(3)?).ok()?;

    let mut rewritten = Vec::with_capacity(payload.len());
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&normalized_declared.to_le_bytes());
    rewritten.extend_from_slice(body_and_tail.get(..candidate.read_body_len)?);
    rewritten.extend_from_slice(body_and_tail.get(candidate.read_body_len..)?);

    let old_payload_length = payload.len();
    let new_payload_length = rewritten.len();
    *payload = rewritten;

    tracing::info!(
        old_wire_declared,
        new_declared = normalized_declared,
        old_payload_length,
        new_payload_length,
        read_body_len = candidate.read_body_len,
        fragment_tail_len = candidate.fragment_tail_len,
        translated_item_slots = candidate.translated_item_slots,
        spell_slots = candidate.spell_slots,
        general_slots = candidate.general_slots,
        "server GuiQuickbar_SetAllButtons scanned-tail transport normalized"
    );

    Some(PrefixedFragmentsNormalizeSummary {
        major: high.major,
        minor: high.minor,
        old_wire_declared,
        new_declared: normalized_declared,
        old_payload_length,
        new_payload_length,
        prefixed_fragment_bytes: old_wire_declared.to_le_bytes(),
        read_bytes_offset: HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES,
        read_bytes_length: normalized_read_size,
    })
}

fn normalize_quickbar_four_prefixed_fragments_payload_if_needed(
    payload: &mut Vec<u8>,
) -> Option<PrefixedFragmentsNormalizeSummary> {
    // Alternate HG/Diamond quickbar transport shape:
    //
    // `P 1E 01 [four leading CNW fragment bytes] [read body] [fragment tail]`
    //
    // This is the shape documented in the packet notes for the working
    // v0.5.334/v0.5.350 bridge path. The previous Rust-only two-prefix/u16
    // repair misread bytes `58 04` from the leading fragment block as a legacy
    // declared length, cutting the 36-slot read buffer short and hiding real
    // slot bytes in the fragment tail. Do not guess a split from length alone:
    // choose the split only when the decompile-backed 36-slot quickbar parser
    // can consume the read buffer exactly with the moved fragment bytes.
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES + 1 {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if !is_quickbar_family(high) {
        return None;
    }

    let old_wire_declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if old_wire_declared >= (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u32
        && (old_wire_declared as usize) <= payload.len()
    {
        return None;
    }

    let prefixed_fragment_bytes: [u8; LEGACY_PREFIXED_FRAGMENT_BYTES] = payload
        [HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES]
        .try_into()
        .ok()?;
    let body_and_tail =
        payload.get(HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES..)?;
    let Some(candidate) = choose_quickbar_split(
        body_and_tail,
        &prefixed_fragment_bytes,
        QuickbarSplitPolicy::ExactSemantic,
    ) else {
        tracing::warn!(
            old_wire_declared,
            old_payload_length = payload.len(),
            prefixed_fragment_bytes = ?prefixed_fragment_bytes,
            "server GuiQuickbar_SetAllButtons four-prefixed transport rejected: no exact semantic quickbar split"
        );
        return None;
    };
    let normalized_read_size = candidate.read_body_len.checked_add(CNW_LENGTH_BYTES)?;
    let normalized_declared = u32::try_from(normalized_read_size.checked_add(3)?).ok()?;

    let mut rewritten = Vec::with_capacity(payload.len().checked_add(CNW_LENGTH_BYTES)?);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&normalized_declared.to_le_bytes());
    rewritten.extend_from_slice(body_and_tail.get(..candidate.read_body_len)?);
    rewritten.extend_from_slice(&prefixed_fragment_bytes);
    rewritten.extend_from_slice(body_and_tail.get(candidate.read_body_len..)?);

    let old_payload_length = payload.len();
    let new_payload_length = rewritten.len();
    *payload = rewritten;

    tracing::info!(
        old_wire_declared,
        new_declared = normalized_declared,
        old_payload_length,
        new_payload_length,
        read_body_len = candidate.read_body_len,
        fragment_tail_len = candidate.fragment_tail_len,
        translated_item_slots = candidate.translated_item_slots,
        spell_slots = candidate.spell_slots,
        general_slots = candidate.general_slots,
        "server GuiQuickbar_SetAllButtons four-prefixed fragment transport normalized"
    );

    Some(PrefixedFragmentsNormalizeSummary {
        major: high.major,
        minor: high.minor,
        old_wire_declared,
        new_declared: normalized_declared,
        old_payload_length,
        new_payload_length,
        prefixed_fragment_bytes,
        read_bytes_offset: HIGH_LEVEL_HEADER_BYTES + LEGACY_PREFIXED_FRAGMENT_BYTES,
        read_bytes_length: normalized_read_size,
    })
}

#[derive(Debug, Clone, Copy)]
struct QuickbarTransportSplit {
    read_body_len: usize,
    fragment_tail_len: usize,
    translated_item_slots: u32,
    spell_slots: u32,
    general_slots: u32,
    item_candidate_slots: u32,
    unsupported_slots: u32,
    trailing_read_bytes: usize,
}

impl QuickbarTransportSplit {
    fn preserves_meaningful_button_payload(&self) -> bool {
        self.translated_item_slots != 0 || self.spell_slots != 0
    }
}

#[derive(Debug, Clone, Copy)]
enum QuickbarSplitPolicy {
    /// Transport-only repair can be considered when the 36-slot reader consumes
    /// a plausible legacy stream. This is deliberately not used for HG's
    /// malformed quickbar transport paths, because a syntactically consumable
    /// split can still erase all button semantics.
    Structural,
    /// Decompile-backed quickbar transport repair for server-to-client
    /// `GuiQuickbar_SetAllButtons`: EE/Diamond both read exactly 36 slot type
    /// bytes and the item/spell payloads that follow them. A split is allowed
    /// only when the semantic translator can preserve real item or spell slots,
    /// consumes the read buffer exactly, and does not rely on candidate/unknown
    /// blanking to make the parse fit.
    ExactSemantic,
}

fn choose_quickbar_split(
    body_and_tail: &[u8],
    prefixed_fragment_bytes: &[u8],
    policy: QuickbarSplitPolicy,
) -> Option<QuickbarTransportSplit> {
    if body_and_tail.len() < LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }

    let max_tail = body_and_tail
        .len()
        .saturating_sub(LEGACY_QUICKBAR_BUTTON_COUNT)
        .min(MAX_QUICKBAR_FOUR_PREFIX_FRAGMENT_TAIL_BYTES);
    let mut best: Option<QuickbarTransportSplit> = None;
    let mut best_score = i32::MIN;

    for fragment_tail_len in 0..=max_tail {
        let read_body_len = body_and_tail.len().checked_sub(fragment_tail_len)?;
        let mut read_buffer = Vec::with_capacity(read_body_len.checked_add(CNW_LENGTH_BYTES)?);
        read_buffer.extend_from_slice(&[0, 0, 0, 0]);
        read_buffer.extend_from_slice(body_and_tail.get(..read_body_len)?);

        let mut fragments =
            Vec::with_capacity(prefixed_fragment_bytes.len().checked_add(fragment_tail_len)?);
        fragments.extend_from_slice(prefixed_fragment_bytes);
        fragments.extend_from_slice(body_and_tail.get(read_body_len..)?);

        let Some((buttons, final_cursor)) = parse_quickbar_read_buffer_with_fragments(
            &read_buffer,
            &fragments,
            LEGACY_QUICKBAR_READ_CURSOR_START,
        ) else {
            continue;
        };
        let translated_item_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Item { .. }))
            .count() as u32;
        let spell_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
            .count() as u32;
        let general_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::General { .. }))
            .count() as u32;
        let item_candidate_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::ItemCandidate))
            .count() as u32;
        let unsupported_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Unsupported))
            .count() as u32;
        let blanked_or_unsupported = buttons
            .iter()
            .filter(|button| {
                matches!(
                    button.kind,
                    QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported
                )
            })
            .count() as i32;
        let trailing_read_bytes = read_buffer.len().saturating_sub(final_cursor);
        match policy {
            QuickbarSplitPolicy::ExactSemantic => {
                if translated_item_slots == 0 && spell_slots == 0 {
                    continue;
                }
                if item_candidate_slots != 0 || unsupported_slots != 0 {
                    continue;
                }
                if trailing_read_bytes != 0 {
                    continue;
                }
            }
            QuickbarSplitPolicy::Structural => {
                if final_cursor != read_buffer.len()
                    && translated_item_slots == 0
                    && spell_slots == 0
                {
                    continue;
                }
            }
        }

        if final_cursor > read_buffer.len() {
            continue;
        }

        let score = translated_item_slots as i32 * 200
            + spell_slots as i32 * 120
            + general_slots as i32 * 8
            - blanked_or_unsupported * 100
            - trailing_read_bytes.min(512) as i32
            - fragment_tail_len.min(128) as i32;
        if score > best_score {
            best_score = score;
            best = Some(QuickbarTransportSplit {
                read_body_len,
                fragment_tail_len,
                translated_item_slots,
                spell_slots,
                general_slots,
                item_candidate_slots,
                unsupported_slots,
                trailing_read_bytes,
            });
        }
    }

    best
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
    let parsed = if quickbar_has_plausible_cnw_declared(payload) {
        match parse_cnw_quickbar_payload(payload) {
            Some(parsed) => parsed,
            None => {
                trace_quickbar_rewrite_skip(payload, "cnw-parse-failed", None);
                return None;
            }
        }
    } else {
        match parse_cnw_quickbar_payload(payload)
            .or_else(|| parse_direct_opcode_quickbar_stream(payload))
        {
            Some(parsed) => parsed,
            None => {
                trace_quickbar_rewrite_skip(payload, "parse-failed", None);
                return None;
            }
        }
    };
    let Some(rewritten) = build_ee_quickbar_payload(&parsed) else {
        trace_quickbar_rewrite_skip(payload, "build-failed", None);
        return None;
    };
    if rewritten == *payload {
        trace_quickbar_rewrite_skip(payload, "already-ee-equivalent", None);
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
        item_buttons_translated: parsed
            .buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Item { .. }))
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
        trace_quickbar_rewrite_skip(payload, "partial-rewrite-safety", Some(&summary));
        return None;
    }
    tracing::info!(
        old_declared = summary.old_declared,
        new_declared = summary.new_declared,
        old_payload_length = summary.old_payload_length,
        new_payload_length = summary.new_payload_length,
        read_size = summary.read_size,
        fragment_size = summary.fragment_size,
        final_cursor = summary.final_cursor,
        trailing_read_bytes = summary.trailing_read_bytes,
        spells_preserved = summary.spells_preserved,
        general_buttons_preserved = summary.general_buttons_preserved,
        item_buttons_translated = summary.item_buttons_translated,
        item_buttons_blanked = summary.item_buttons_blanked,
        unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
        direct_opcode_stream = summary.direct_opcode_stream,
        "server GuiQuickbar_SetAllButtons semantically rewritten for EE"
    );
    *payload = rewritten;
    Some(summary)
}

fn quickbar_has_plausible_cnw_declared(payload: &[u8]) -> bool {
    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return false;
    }
    let Some(declared) = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES) else {
        return false;
    };
    let Some(read_size) = declared.checked_sub(3).and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    let payload_size = payload.len().saturating_sub(HIGH_LEVEL_HEADER_BYTES);
    read_size >= LEGACY_QUICKBAR_READ_CURSOR_START && read_size <= payload_size
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
        || summary.item_buttons_translated == 0
            && summary.item_buttons_blanked > 8
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
    let (buttons, final_cursor) = if fragments.is_empty() {
        parse_quickbar_read_buffer(read_buffer, LEGACY_QUICKBAR_READ_CURSOR_START)?
    } else {
        parse_quickbar_read_buffer_with_fragments(
            read_buffer,
            fragments,
            LEGACY_QUICKBAR_READ_CURSOR_START,
        )?
    };

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

fn trace_quickbar_rewrite_skip(
    payload: &[u8],
    reason: &'static str,
    summary: Option<&QuickbarRewriteSummary>,
) {
    let Some(high) = HighLevel::parse(payload) else {
        return;
    };
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return;
    }

    let declared = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES).unwrap_or(0);
    let read_size = declared.saturating_sub(3);
    let payload_size = payload.len().saturating_sub(HIGH_LEVEL_HEADER_BYTES);
    let fragment_size = usize::try_from(read_size)
        .ok()
        .and_then(|read_size| payload_size.checked_sub(read_size))
        .unwrap_or(0);
    let prefix = hex_prefix(payload, 96);
    let dump_path = dump_quickbar_payload(payload, reason);

    if let Some(summary) = summary {
        tracing::warn!(
            reason,
            old_declared = summary.old_declared,
            old_payload_length = summary.old_payload_length,
            new_declared = summary.new_declared,
            new_payload_length = summary.new_payload_length,
            read_size = summary.read_size,
            fragment_size = summary.fragment_size,
            final_cursor = summary.final_cursor,
            trailing_read_bytes = summary.trailing_read_bytes,
            spells_preserved = summary.spells_preserved,
            general_buttons_preserved = summary.general_buttons_preserved,
            item_buttons_translated = summary.item_buttons_translated,
            item_buttons_blanked = summary.item_buttons_blanked,
            unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
            direct_opcode_stream = summary.direct_opcode_stream,
            dump_path = dump_path.as_deref().unwrap_or(""),
            prefix = %prefix,
            "server GuiQuickbar_SetAllButtons rewrite skipped"
        );
    } else {
        tracing::warn!(
            reason,
            declared,
            payload_length = payload.len(),
            read_size,
            fragment_size,
            dump_path = dump_path.as_deref().unwrap_or(""),
            prefix = %prefix,
            "server GuiQuickbar_SetAllButtons rewrite skipped"
        );
    }
}

fn dump_quickbar_payload(payload: &[u8], reason: &str) -> Option<String> {
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
    path.push(format!("quickbar-unrewritten-{reason}-{nanos}.bin"));
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
    let memo_width = read_buffer.len().checked_add(1)?;
    let mut memo = vec![
        QUICKBAR_UNKNOWN_SCORE;
        (LEGACY_QUICKBAR_BUTTON_COUNT + 1).checked_mul(memo_width)?
    ];
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
            cursor = choose_legacy_quickbar_item_end(read_buffer, slot, cursor, &mut memo)
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
    let memo_width = read_buffer.len().checked_add(1)?;
    let mut memo = vec![
        QUICKBAR_UNKNOWN_SCORE;
        (LEGACY_QUICKBAR_BUTTON_COUNT + 1).checked_mul(memo_width)?
    ];
    let mut opaque_item_slots_blanked = false;
    for slot in 0..LEGACY_QUICKBAR_BUTTON_COUNT {
        if reader.cursor >= read_buffer.len() {
            buttons.extend((slot..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| QuickbarButton {
                kind: QuickbarButtonKind::Unsupported,
            }));
            opaque_item_slots_blanked = true;
            break;
        }

        let before_button = reader.clone();
        let button_start = reader.cursor;
        let ty = reader.read_byte()?;
        if ty == 0 && looks_like_quickbar_item_object_body_at(&reader, true, model_types) {
            // HG captures can lose the type-1 tag for the first item slot while
            // leaving the following item-object body byte-aligned. The old C++
            // proxy only recovered this as a semantic identity after the full
            // item object, active-property tail, and 36-slot parse validated.
            let mut trial = reader.clone();
            if let Some((primary, secondary)) =
                parse_legacy_quickbar_item_payload(&mut trial, model_types)
            {
                if primary.present && primary.active_props.is_some() {
                    tracing::info!(
                        slot,
                        offset = reader.cursor.saturating_sub(1),
                        object_id = format_args!("0x{:08X}", primary.object_id),
                        base_item = primary.base_item,
                        "server GuiQuickbar_SetAllButtons recovered missing item type tag"
                    );
                    reader = trial;
                    buttons.push(QuickbarButton {
                        kind: QuickbarButtonKind::Item {
                            primary,
                            secondary,
                            recovered_type_tag: true,
                        },
                    });
                    continue;
                }
            }
        }
        if ty == 1 {
            if let Some((primary, secondary)) =
                parse_legacy_quickbar_item_payload(&mut reader, model_types)
            {
                buttons.push(QuickbarButton {
                    kind: QuickbarButtonKind::Item {
                        primary,
                        secondary,
                        recovered_type_tag: false,
                    },
                });
            } else {
                // Decompile-backed quickbar discipline:
                // `P 1E 01` contains exactly 36 button records. If a type-1
                // item object cannot be translated because its legacy
                // item/active-property body is not yet owned by the Rust
                // parser, do not forward those bytes and do not blank the whole
                // bar. Use the bounded legacy item-end scorer from the mature
                // bridge to find the next plausible button boundary, blank this
                // item, and continue preserving later spell/general buttons.
                // The shared fragment tail may contain only the skipped item
                // BOOLs, so final fragment exhaustion is required only when no
                // opaque item slot was blanked.
                let next_cursor =
                    choose_legacy_quickbar_item_end(read_buffer, slot, button_start, &mut memo)
                        .filter(|next_cursor| *next_cursor > button_start);
                let Some(next_cursor) = next_cursor else {
                    if quickbar_can_blank_remaining_after_source_parse_failure(&buttons, slot) {
                        buttons.push(QuickbarButton {
                            kind: QuickbarButtonKind::ItemCandidate,
                        });
                        buttons.extend((slot + 1..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| {
                            QuickbarButton {
                                kind: QuickbarButtonKind::Unsupported,
                            }
                        }));
                        opaque_item_slots_blanked = true;
                        break;
                    }
                    return None;
                };
                reader = before_button;
                reader.cursor = next_cursor;
                opaque_item_slots_blanked = true;
                buttons.push(QuickbarButton {
                    kind: QuickbarButtonKind::ItemCandidate,
                });
            }
            continue;
        }

        let kind = if let Some(kind) = parse_legacy_quickbar_non_item_from_reader(&mut reader, ty) {
            kind
        } else {
            let Some(resync_cursor) = find_legacy_quickbar_resync(read_buffer, slot, button_start)
            else {
                if quickbar_can_blank_remaining_after_source_parse_failure(&buttons, slot) {
                    reader = before_button;
                    buttons.push(QuickbarButton {
                        kind: QuickbarButtonKind::Unsupported,
                    });
                    buttons.extend((slot + 1..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| {
                        QuickbarButton {
                            kind: QuickbarButtonKind::Unsupported,
                        }
                    }));
                    opaque_item_slots_blanked = true;
                    break;
                }
                return None;
            };
            reader = before_button;
            reader.cursor = resync_cursor;
            let resynced_type = reader.read_byte()?;
            parse_legacy_quickbar_non_item_from_reader(&mut reader, resynced_type)?
        };
        buttons.push(QuickbarButton { kind });
    }

    if buttons.len() != LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }
    if !opaque_item_slots_blanked && reader.cursor != read_buffer.len() {
        return None;
    }
    let consumed_fragment_bits = reader
        .fragment_cursor
        .checked_mul(8)?
        .checked_add(usize::from(reader.fragment_bit))?;
    let consumed_fragment_bytes = reader.fragment_cursor + usize::from(reader.fragment_bit != 0);
    if !opaque_item_slots_blanked
        && (consumed_fragment_bytes != fragments.len()
            || reader.final_fragment_bits != u8::try_from(consumed_fragment_bits % 8).ok()?)
    {
        return None;
    }
    Some((buttons, reader.cursor.min(read_buffer.len())))
}

fn quickbar_can_blank_remaining_after_source_parse_failure(
    buttons: &[QuickbarButton],
    slot: usize,
) -> bool {
    // EE/Diamond both define `GuiQuickbar_SetAllButtons` as exactly 36 slot
    // records. The C++ bridge's decompile-backed path used this as a semantic
    // boundary: once at least one earlier slot has been parsed or the failure
    // occurs after slot zero, unowned later source bytes may be consumed and
    // emitted as empty EE slots, but they must never be forwarded raw.
    slot > 0
        || buttons.iter().any(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::Spell { .. }
                    | QuickbarButtonKind::General { .. }
                    | QuickbarButtonKind::Item { .. }
                    | QuickbarButtonKind::ItemCandidate
            )
        })
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

    fn read_i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.read_dword()?.to_le_bytes()))
    }

    fn read_bytes(&mut self, count: usize) -> Option<Vec<u8>> {
        let end = self.cursor.checked_add(count)?;
        let bytes = self.read_buffer.get(self.cursor..end)?.to_vec();
        self.cursor = end;
        Some(bytes)
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

    fn read_string(&mut self) -> Option<Vec<u8>> {
        let length = usize::try_from(self.read_dword()?).ok()?;
        if length > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
            return None;
        }
        self.read_bytes(length)
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

    fn read_loc_string(&mut self) -> Option<QuickbarLocStringField> {
        let custom_tlk = self.read_bit()?;
        if custom_tlk {
            Some(QuickbarLocStringField {
                custom_tlk,
                language_selector: self.read_bit()?,
                string_ref: self.read_dword()?,
                text: Vec::new(),
            })
        } else {
            Some(QuickbarLocStringField {
                custom_tlk,
                language_selector: false,
                string_ref: u32::MAX,
                text: self.read_string()?,
            })
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
        let value = reader.read_dword()?;
        if !legacy_quickbar_int_payload_is_valid_for_ee(ty, value) {
            return Some(QuickbarButtonKind::Unsupported);
        }
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

fn parse_legacy_quickbar_item_payload(
    reader: &mut QuickbarPacketReader<'_>,
    model_types: &[i8],
) -> Option<(QuickbarItemObject, QuickbarItemObject)> {
    let before = reader.clone();
    let primary = match parse_legacy_quickbar_item_object(reader, true, model_types) {
        Some(item) => item,
        None => {
            *reader = before;
            return None;
        }
    };
    let secondary = match parse_legacy_quickbar_item_object(reader, false, model_types) {
        Some(item) => item,
        None => {
            *reader = before;
            return None;
        }
    };
    Some((primary, secondary))
}

fn parse_legacy_quickbar_item_object(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<QuickbarItemObject> {
    let before_present = reader.clone();
    let present = reader.read_bit()?;
    if !present {
        if looks_like_quickbar_item_object_body_at(reader, include_int_param, model_types) {
            // EE and Diamond both read primary/secondary item-object presence
            // BOOLs before any item object body. HG captures can have the CNW
            // fragment cursor shifted by a few already-consumed source bits;
            // only resync when the byte-side object body and active-property
            // tail validate completely.
            for skipped_bits in 1..=MAX_QUICKBAR_ITEM_PRESENCE_RESYNC_BITS {
                let mut trial = before_present.clone();
                let mut candidate_present = false;
                let mut bits_ok = true;
                for _ in 0..=skipped_bits {
                    match trial.read_bit() {
                        Some(bit) => candidate_present = bit,
                        None => {
                            bits_ok = false;
                            break;
                        }
                    }
                }
                if !bits_ok || !candidate_present {
                    continue;
                }
                if let Some(mut item) =
                    parse_legacy_quickbar_item_object_body(&mut trial, include_int_param, model_types)
                {
                    item.present = true;
                    tracing::info!(
                        skipped_bits,
                        cursor = before_present.cursor,
                        fragment_cursor = before_present.fragment_cursor,
                        fragment_bit = before_present.fragment_bit,
                        object_id = %format_args!("0x{:08X}", item.object_id),
                        base_item = item.base_item,
                        "server GuiQuickbar_SetAllButtons item presence bit resynced"
                    );
                    *reader = trial;
                    return Some(item);
                }
            }
            return None;
        }
        return Some(QuickbarItemObject::default());
    }

    let mut item = parse_legacy_quickbar_item_object_body(reader, include_int_param, model_types)?;
    item.present = true;
    Some(item)
}

fn parse_legacy_quickbar_item_object_body(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<QuickbarItemObject> {
    let object_id = reader.read_dword()?;
    let int_param = if include_int_param {
        reader.read_i32()?
    } else {
        -1
    };
    let (base_item, appearance_type, appearance_bytes) =
        parse_legacy_quickbar_item_appearance(reader, model_types)?;
    let active_props = parse_legacy_quickbar_active_item_properties(reader, base_item)?;

    Some(QuickbarItemObject {
        present: true,
        object_id,
        int_param,
        base_item,
        appearance_type,
        active_props: Some(active_props),
        appearance_bytes,
    })
}

fn parse_legacy_quickbar_item_appearance(
    reader: &mut QuickbarPacketReader<'_>,
    model_types: &[i8],
) -> Option<(u32, i8, Vec<u8>)> {
    let start = reader.cursor;
    let base_item_id = reader.read_dword()?;
    let model_type = *model_types.get(usize::try_from(base_item_id).ok()?)?;
    let appearance_size = legacy_item_appearance_read_size(model_type)?;
    let end = start.checked_add(appearance_size)?;
    let appearance_bytes = reader.read_buffer.get(start..end)?.to_vec();
    reader.cursor = end;
    Some((base_item_id, model_type, appearance_bytes))
}

fn parse_legacy_quickbar_active_item_properties(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<QuickbarActiveItemProperties> {
    let before = reader.clone();
    if let Some(properties) =
        parse_legacy_quickbar_active_item_properties_standard(reader, base_item_id)
    {
        return Some(properties);
    }
    *reader = before;
    parse_legacy_quickbar_active_item_properties_bare_inline_fallback(reader, base_item_id)
}

fn parse_legacy_quickbar_active_item_properties_standard(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<QuickbarActiveItemProperties> {
    let mut properties = QuickbarActiveItemProperties::default();
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        properties.has_armor_word = true;
        properties.armor_word = reader.read_word()?;
    }

    properties.name_is_locstring = reader.read_bit()?;
    if properties.name_is_locstring {
        properties.locstring_name = reader.read_loc_string()?;
    } else {
        properties.string_name = reader.read_string()?;
    }

    parse_legacy_quickbar_active_item_properties_tail(reader, properties)
}

fn parse_legacy_quickbar_active_item_properties_bare_inline_fallback(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<QuickbarActiveItemProperties> {
    let mut probe = reader.clone();
    let mut prefix = QuickbarActiveItemProperties::default();
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        prefix.has_armor_word = true;
        prefix.armor_word = probe.read_word()?;
    }

    let name_length_offset = probe.cursor;
    if probe.read_buffer.len().saturating_sub(name_length_offset) < CNW_LENGTH_BYTES + 1 + 9
        || read_u32_le(probe.read_buffer, name_length_offset)? != 0
    {
        return None;
    }

    let text_start = name_length_offset.checked_add(CNW_LENGTH_BYTES)?;
    let text_limit = probe
        .read_buffer
        .len()
        .min(text_start.checked_add(MAX_QUICKBAR_BARE_ACTIVE_ITEM_NAME_BYTES)?);
    let mut printable_end = text_start;
    while printable_end < text_limit
        && is_legacy_bare_active_item_name_byte(probe.read_buffer[printable_end])
    {
        printable_end += 1;
    }
    if printable_end == text_start {
        return None;
    }

    for candidate_text_end in (text_start + 1..=printable_end).rev() {
        let mut trial = probe.clone();
        let mut candidate = prefix.clone();
        let _legacy_name_bit = trial.read_bit()?;
        trial.cursor = candidate_text_end;
        candidate.name_is_locstring = false;
        candidate.string_name = probe
            .read_buffer
            .get(text_start..candidate_text_end)?
            .to_vec();
        if let Some(parsed) =
            parse_legacy_quickbar_active_item_properties_tail(&mut trial, candidate)
        {
            // HG's empty-inline fallback is a verified legacy encoding variant:
            // the source carries a zero length DWORD followed by printable name
            // bytes. EE expects a direct CExoString length, so the writer below
            // synthesizes the missing length and preserves the text.
            *reader = trial;
            return Some(parsed);
        }
    }

    None
}

fn parse_legacy_quickbar_active_item_properties_tail(
    reader: &mut QuickbarPacketReader<'_>,
    mut properties: QuickbarActiveItemProperties,
) -> Option<QuickbarActiveItemProperties> {
    properties.post_name_bool1 = reader.read_bit()?;
    properties.cost = reader.read_dword()?;
    properties.stack_or_charges = reader.read_dword()?;
    properties.post_name_bool2 = reader.read_bit()?;
    properties.post_name_bool3 = reader.read_bit()?;
    properties.post_name_bool4 = reader.read_bit()?;
    let property_count = reader.read_byte()?;
    if property_count > MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES {
        return None;
    }
    properties.properties.reserve(usize::from(property_count));
    for _ in 0..property_count {
        properties.properties.push(QuickbarActivePropertyEntry {
            property: reader.read_word()?,
            subtype: reader.read_word()?,
            cost_table_value: reader.read_word()?,
            param: reader.read_byte()?,
        });
    }

    properties.state_mask = reader.read_byte()?;
    properties.value_mask = reader.read_byte()?;
    for bit in 0..8 {
        if (properties.value_mask & (1u8 << bit)) != 0 {
            properties.value_mask_bytes.push(reader.read_byte()?);
        }
    }
    Some(properties)
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
    let before_present = reader.clone();
    let present = reader.read_bit()?;
    if !present {
        if looks_like_quickbar_item_object_body_at(reader, include_int_param, model_types) {
            for skipped_bits in 1..=MAX_QUICKBAR_ITEM_PRESENCE_RESYNC_BITS {
                let mut trial = before_present.clone();
                let mut candidate_present = false;
                let mut bits_ok = true;
                for _ in 0..=skipped_bits {
                    match trial.read_bit() {
                        Some(bit) => candidate_present = bit,
                        None => {
                            bits_ok = false;
                            break;
                        }
                    }
                }
                if !bits_ok || !candidate_present {
                    continue;
                }
                if skip_legacy_quickbar_item_object_body(
                    &mut trial,
                    include_int_param,
                    model_types,
                )
                .is_some()
                {
                    *reader = trial;
                    return Some(());
                }
            }
            return None;
        }
        return Some(());
    }

    skip_legacy_quickbar_item_object_body(reader, include_int_param, model_types)
}

fn skip_legacy_quickbar_item_object_body(
    reader: &mut QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> Option<()> {
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

fn looks_like_quickbar_item_object_body_at(
    reader: &QuickbarPacketReader<'_>,
    include_int_param: bool,
    model_types: &[i8],
) -> bool {
    let minimum = CNW_LENGTH_BYTES
        + if include_int_param {
            CNW_LENGTH_BYTES
        } else {
            0
        }
        + CNW_LENGTH_BYTES;
    if reader.cursor > reader.read_buffer.len()
        || reader.read_buffer.len().saturating_sub(reader.cursor) < minimum
    {
        return false;
    }

    let mut cursor = reader.cursor;
    let Some(object_id) = read_u32_le(reader.read_buffer, cursor) else {
        return false;
    };
    if (object_id & 0x8000_0000) == 0 && object_id != 0xFFFF_FFFF {
        return false;
    }
    cursor += CNW_LENGTH_BYTES;
    if include_int_param {
        cursor += CNW_LENGTH_BYTES;
    }

    let Some(base_item_id) = read_u32_le(reader.read_buffer, cursor) else {
        return false;
    };
    let Some(model_type) = usize::try_from(base_item_id)
        .ok()
        .and_then(|index| model_types.get(index))
        .copied()
    else {
        return false;
    };
    let Some(legacy_size) = legacy_item_appearance_read_size(model_type) else {
        return false;
    };
    reader.read_buffer.len().saturating_sub(cursor) >= legacy_size
}

fn skip_legacy_quickbar_active_item_properties(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<()> {
    let before = reader.clone();
    if skip_legacy_quickbar_active_item_properties_standard(reader, base_item_id).is_some() {
        return Some(());
    }
    *reader = before;
    skip_legacy_quickbar_active_item_properties_bare_inline_fallback(reader, base_item_id)
}

fn skip_legacy_quickbar_active_item_properties_standard(
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

    skip_legacy_quickbar_active_item_properties_tail(reader)
}

fn skip_legacy_quickbar_active_item_properties_bare_inline_fallback(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<()> {
    let mut probe = reader.clone();
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        let _armor_word = probe.read_word()?;
    }

    let name_length_offset = probe.cursor;
    if probe.read_buffer.len().saturating_sub(name_length_offset) < CNW_LENGTH_BYTES + 1 + 9
        || read_u32_le(probe.read_buffer, name_length_offset)? != 0
    {
        return None;
    }

    let text_start = name_length_offset.checked_add(CNW_LENGTH_BYTES)?;
    let text_limit =
        probe
            .read_buffer
            .len()
            .min(text_start.checked_add(MAX_QUICKBAR_BARE_ACTIVE_ITEM_NAME_BYTES)?);
    let mut printable_end = text_start;
    while printable_end < text_limit
        && is_legacy_bare_active_item_name_byte(probe.read_buffer[printable_end])
    {
        printable_end += 1;
    }
    if printable_end == text_start {
        return None;
    }

    for candidate_text_end in (text_start + 1..=printable_end).rev() {
        let mut trial = probe.clone();
        let _legacy_name_bit = trial.read_bit()?;
        trial.cursor = candidate_text_end;
        if skip_legacy_quickbar_active_item_properties_tail(&mut trial).is_some() {
            *reader = trial;
            return Some(());
        }
    }

    None
}

fn skip_legacy_quickbar_active_item_properties_tail(
    reader: &mut QuickbarPacketReader<'_>,
) -> Option<()> {
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

fn is_legacy_bare_active_item_name_byte(ch: u8) -> bool {
    (0x20..=0x7E).contains(&ch)
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
        if spell_id > 10_000 {
            return Some((
                QuickbarButton {
                    kind: QuickbarButtonKind::Unsupported,
                },
                payload_cursor + 7,
            ));
        }
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
        let value = read_u32_le(read_buffer, payload_cursor)?;
        if !legacy_quickbar_int_payload_is_valid_for_ee(ty, value) {
            return Some((
                QuickbarButton {
                    kind: QuickbarButtonKind::Unsupported,
                },
                next_cursor,
            ));
        }
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

#[derive(Debug, Clone)]
struct QuickbarPacketWriter {
    read_buffer: Vec<u8>,
    fragment_bits: Vec<bool>,
}

impl QuickbarPacketWriter {
    fn new() -> Self {
        Self {
            read_buffer: vec![0, 0, 0, 0],
            fragment_bits: vec![false, false, false],
        }
    }

    fn write_bit(&mut self, value: bool) {
        self.fragment_bits.push(value);
    }

    fn write_byte(&mut self, value: u8) {
        self.read_buffer.push(value);
    }

    fn write_word(&mut self, value: u16) {
        self.read_buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn write_dword(&mut self, value: u32) {
        self.read_buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.read_buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn write_string(&mut self, value: &[u8]) -> Option<()> {
        let length = u32::try_from(value.len()).ok()?;
        self.write_dword(length);
        self.read_buffer.extend_from_slice(value);
        Some(())
    }

    fn build_fragment_bytes(mut self) -> Vec<u8> {
        let final_fragment_bits = u8::try_from(self.fragment_bits.len() % 8).unwrap_or(0);
        self.fragment_bits[0] = (final_fragment_bits & 0x04) != 0;
        self.fragment_bits[1] = (final_fragment_bits & 0x02) != 0;
        self.fragment_bits[2] = (final_fragment_bits & 0x01) != 0;

        let mut fragments = Vec::with_capacity((self.fragment_bits.len() + 7) / 8);
        for chunk in self.fragment_bits.chunks(8) {
            let mut byte = 0u8;
            for (index, bit) in chunk.iter().enumerate() {
                if *bit {
                    byte |= 1u8 << (7 - index);
                }
            }
            fragments.push(byte);
        }
        if fragments.is_empty() {
            fragments.push(0);
        }
        fragments
    }
}

fn build_ee_quickbar_payload(parsed: &QuickbarParse) -> Option<Vec<u8>> {
    let mut writer = QuickbarPacketWriter::new();
    for button in &parsed.buttons {
        match button.kind {
            QuickbarButtonKind::Spell {
                spell_class,
                spell_id,
                metamagic,
                domain,
            } => {
                writer.write_byte(2);
                writer.write_byte(spell_class);
                writer.write_dword(spell_id);
                writer.write_byte(metamagic);
                writer.write_byte(domain);
            }
            QuickbarButtonKind::General { ref bytes } => {
                writer.read_buffer.extend_from_slice(bytes)
            }
            QuickbarButtonKind::Item {
                ref primary,
                ref secondary,
                ..
            } => {
                writer.write_byte(1);
                write_quickbar_item_object(&mut writer, primary, true)?;
                write_quickbar_item_object(&mut writer, secondary, false)?;
            }
            QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported => {
                writer.write_byte(0);
            }
        }
    }

    let declared = u32::try_from(writer.read_buffer.len().checked_add(3)?).ok()?;
    write_u32_le(&mut writer.read_buffer, 0, declared)?;
    let read_buffer = writer.read_buffer.clone();
    let fragments = writer.build_fragment_bytes();

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

fn write_quickbar_item_object(
    writer: &mut QuickbarPacketWriter,
    item: &QuickbarItemObject,
    include_int_param: bool,
) -> Option<()> {
    writer.write_bit(item.present);
    if !item.present {
        return Some(());
    }

    writer.write_dword(item.object_id);
    if include_int_param {
        writer.write_i32(item.int_param);
    }
    write_quickbar_item_appearance(writer, item)?;
    if let Some(ref active_props) = item.active_props {
        write_quickbar_active_item_properties(writer, active_props, item.base_item)
    } else {
        write_empty_quickbar_active_item_properties(writer, item.base_item)
    }
}

fn write_quickbar_item_appearance(
    writer: &mut QuickbarPacketWriter,
    item: &QuickbarItemObject,
) -> Option<()> {
    if item.appearance_bytes.len() < CNW_LENGTH_BYTES {
        return None;
    }
    writer
        .read_buffer
        .extend_from_slice(item.appearance_bytes.as_slice());

    if item.appearance_type == 3 {
        append_ee_extended_armor_table(&mut writer.read_buffer, &item.appearance_bytes);
    }

    // EE unconditionally enters the visual-transform reader after item
    // appearance. On HG's legacy negotiated build path the decompile resolves
    // that map to ten 32-bit legacy LerpFloat values, not a counted EE feature
    // 0x23 map. Use the identity values verified in the live-object path.
    for value in EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_DWORDS {
        writer.write_dword(value);
    }
    Some(())
}

fn append_ee_extended_armor_table(read_buffer: &mut Vec<u8>, appearance_bytes: &[u8]) {
    let color_offset = CNW_LENGTH_BYTES + EE_LIVE_ARMOR_PART_COUNT;
    if let Some(legacy_colors) =
        appearance_bytes.get(color_offset..color_offset + EE_LIVE_ARMOR_LAYER_COUNT)
    {
        for _ in 0..EE_LIVE_ARMOR_PART_COUNT {
            read_buffer.extend_from_slice(legacy_colors);
        }
    } else {
        read_buffer.extend(std::iter::repeat(0).take(EE_LIVE_EXTENDED_ARMOR_TABLE_BYTES));
    }
}

fn write_quickbar_active_item_properties(
    writer: &mut QuickbarPacketWriter,
    properties: &QuickbarActiveItemProperties,
    base_item_id: u32,
) -> Option<()> {
    if properties.properties.len() > usize::from(u8::MAX) {
        return None;
    }
    let expanded_value_bytes = expand_quickbar_active_value_mask_bytes(properties)?;

    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        writer.write_word(if properties.has_armor_word {
            properties.armor_word
        } else {
            0
        });
    }

    let flatten_inline_locstring_name =
        properties.name_is_locstring && !properties.locstring_name.custom_tlk;
    let write_name_as_locstring = properties.name_is_locstring && !flatten_inline_locstring_name;
    writer.write_bit(write_name_as_locstring);
    if write_name_as_locstring {
        write_quickbar_loc_string(writer, &properties.locstring_name)?;
    } else if flatten_inline_locstring_name {
        writer.write_string(&properties.locstring_name.text)?;
    } else {
        writer.write_string(&properties.string_name)?;
    }

    writer.write_bit(properties.post_name_bool1);
    writer.write_dword(properties.cost);
    writer.write_dword(properties.stack_or_charges);
    writer.write_bit(properties.post_name_bool2);
    writer.write_bit(properties.post_name_bool3);
    writer.write_bit(properties.post_name_bool4);
    // EE's active-property reader checks one extra feature-gated BOOL before
    // the property table on this build path. Diamond/HG does not send it, so
    // the bridge supplies a verified false bit instead of shifting the source
    // property-count byte.
    writer.write_bit(false);

    writer.write_byte(u8::try_from(properties.properties.len()).ok()?);
    for entry in &properties.properties {
        writer.write_word(entry.property);
        writer.write_word(entry.subtype);
        writer.write_word(entry.cost_table_value);
        writer.write_byte(entry.param);
    }

    writer.write_byte(properties.state_mask);
    writer.write_byte(0xFF);
    for value in expanded_value_bytes {
        writer.write_byte(value);
    }
    Some(())
}

fn write_empty_quickbar_active_item_properties(
    writer: &mut QuickbarPacketWriter,
    base_item_id: u32,
) -> Option<()> {
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        writer.write_word(0);
    }
    writer.write_bit(false);
    writer.write_string(&[])?;
    writer.write_bit(false);
    writer.write_dword(0);
    writer.write_dword(0);
    writer.write_bit(false);
    writer.write_bit(false);
    writer.write_bit(false);
    writer.write_bit(false);
    writer.write_byte(0);
    writer.write_byte(0);
    writer.write_byte(0);
    Some(())
}

fn write_quickbar_loc_string(
    writer: &mut QuickbarPacketWriter,
    field: &QuickbarLocStringField,
) -> Option<()> {
    writer.write_bit(field.custom_tlk);
    if field.custom_tlk {
        writer.write_bit(field.language_selector);
        writer.write_dword(field.string_ref);
        Some(())
    } else {
        writer.write_string(&field.text)
    }
}

fn expand_quickbar_active_value_mask_bytes(
    properties: &QuickbarActiveItemProperties,
) -> Option<[u8; 8]> {
    let mut expanded = [0u8; 8];
    let mut value_index = 0usize;
    for bit in 0..8 {
        if (properties.value_mask & (1u8 << bit)) == 0 {
            continue;
        }
        *expanded.get_mut(bit)? = *properties.value_mask_bytes.get(value_index)?;
        value_index += 1;
    }
    Some(expanded)
}

fn choose_legacy_quickbar_item_end(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
    memo: &mut [i32],
) -> Option<usize> {
    let remaining_slots_after_this = LEGACY_QUICKBAR_BUTTON_COUNT.checked_sub(slot + 1)?;
    let item_payload_start = cursor.checked_add(1)?;
    let min_candidate = read_buffer.len().min(item_payload_start.checked_add(8)?);
    let max_candidate = read_buffer.len().min(item_payload_start.checked_add(512)?);
    let mut best_score = QUICKBAR_BAD_SCORE;
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

        let mut score =
            score_legacy_quickbar_parse_from(read_buffer, slot + 1, candidate, memo);
        if score <= QUICKBAR_BAD_SCORE / 2 {
            continue;
        }
        let skipped = candidate.saturating_sub(item_payload_start);
        score += 12 - skipped.checked_div(16).unwrap_or(0).min(120) as i32;
        if score > best_score {
            best_score = score;
            best_candidate = Some(candidate);
        }
    }

    if best_score < 0 {
        None
    } else {
        best_candidate
    }
}

fn score_legacy_quickbar_parse_from(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
    memo: &mut [i32],
) -> i32 {
    if cursor > read_buffer.len() || slot > LEGACY_QUICKBAR_BUTTON_COUNT {
        return QUICKBAR_BAD_SCORE;
    }
    if slot == LEGACY_QUICKBAR_BUTTON_COUNT {
        let unread = cursor.abs_diff(read_buffer.len());
        return 100_000 - unread.min(10_000) as i32 * 25;
    }
    if cursor >= read_buffer.len() {
        return QUICKBAR_BAD_SCORE;
    }

    let memo_width = read_buffer.len() + 1;
    let memo_index = slot
        .checked_mul(memo_width)
        .and_then(|base| base.checked_add(cursor));
    if let Some(index) = memo_index {
        if let Some(score) = memo.get(index).copied() {
            if score != QUICKBAR_UNKNOWN_SCORE {
                return score;
            }
        }
    }

    let ty = read_buffer[cursor];
    let remaining_slots_after_this = LEGACY_QUICKBAR_BUTTON_COUNT - slot - 1;
    let mut best_score = QUICKBAR_BAD_SCORE;

    if ty == 1 || ty == 9 {
        let item_payload_start = cursor + 1;
        let min_candidate = read_buffer.len().min(item_payload_start.saturating_add(8));
        let max_candidate = read_buffer.len().min(item_payload_start.saturating_add(420));
        for candidate in min_candidate..=max_candidate {
            if candidate.saturating_add(remaining_slots_after_this) > read_buffer.len() {
                break;
            }
            if remaining_slots_after_this > 0
                && (candidate >= read_buffer.len()
                    || !is_legacy_quickbar_plausible_type(read_buffer[candidate]))
            {
                continue;
            }

            let mut score =
                score_legacy_quickbar_parse_from(read_buffer, slot + 1, candidate, memo);
            if score <= QUICKBAR_BAD_SCORE / 2 {
                continue;
            }
            let skipped = candidate.saturating_sub(item_payload_start);
            score += 12 - skipped.checked_div(16).unwrap_or(0).min(120) as i32;
            best_score = best_score.max(score);
        }
    } else if let Some((button, next_cursor)) = parse_legacy_quickbar_non_item(read_buffer, cursor) {
        let mut score =
            score_legacy_quickbar_parse_from(read_buffer, slot + 1, next_cursor, memo);
        if score > QUICKBAR_BAD_SCORE / 2 {
            match button.kind {
                QuickbarButtonKind::Spell { .. } => score += 60,
                QuickbarButtonKind::General { ref bytes } if bytes.len() == 1 && bytes[0] == 0 => {
                    score += 8;
                }
                QuickbarButtonKind::General { ref bytes }
                    if bytes.len() == 1
                        && legacy_quickbar_type_has_no_payload(bytes[0]) =>
                {
                    score += 20;
                }
                QuickbarButtonKind::General { .. } | QuickbarButtonKind::Unsupported => {
                    score += 4;
                }
                QuickbarButtonKind::Item { .. } => {
                    score += 100;
                }
                QuickbarButtonKind::ItemCandidate => {}
            }
            best_score = score;
        }
    }

    if let Some(index) = memo_index {
        if let Some(slot) = memo.get_mut(index) {
            *slot = best_score;
        }
    }
    best_score
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
            QuickbarButtonKind::Item { .. } => score += 100,
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

fn legacy_quickbar_int_payload_is_valid_for_ee(ty: u8, value: u32) -> bool {
    match ty {
        // EE's quickbar case 8 reads `ReadINT(32)`, then calls `sub_14086B160`.
        // That path reaches `sub_140866C90`, which stores the value and indexes
        // `off_141297500[value]` plus `dword_140E46CF0[value]` directly. The
        // 8193.37.17 decompile shows 23 animation/icon entries (indices 0..22).
        // Therefore this is still a strict semantic translation: in-range
        // values are byte-identical and preserved, while out-of-range values
        // are consumed and emitted as an empty slot instead of raw passthrough.
        8 => value < EE_QUICKBAR_ANIMATION_ICON_COUNT,
        _ => true,
    }
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
    if let Ok(asset_bundle) = std::env::var("HG_BRIDGE_ASSET_BUNDLE") {
        push_asset_bundle_baseitems_candidates(&mut candidates, &asset_bundle);
    }
    push_asset_bundle_baseitems_candidates(
        &mut candidates,
        r"C:\Program Files (x86)\Steam\steamapps\common\Neverwinter Nights\hg-bridge-assets",
    );
    candidates.extend([
        r"C:\Program Files (x86)\Steam\steamapps\common\Neverwinter Nights\hg-bridge-assets\ee-fixes\2da\baseitems.2da".to_string(),
        "NWN Diamond\\1.72 builder resources\\auto2damerger\\merged\\baseitems.2da".to_string(),
        "NWN Diamond\\1.72 builder resources\\1.72 full 2dasource\\baseitems.2da".to_string(),
        "NWN Diamond\\1.72 builder resources\\1.72-hak 2das\\baseitems.2da".to_string(),
        "NWN Diamond\\1.72 builder resources\\1.72-only 2das\\baseitems.2da".to_string(),
    ]);

    for path in candidates {
        if let Some(model_types) = load_base_item_model_types_from_path(&path) {
            tracing::info!(
                source = %path,
                rows = model_types.len(),
                "quickbar baseitems ModelType table loaded"
            );
            return Some(model_types);
        }
    }

    None
}

fn push_asset_bundle_baseitems_candidates(candidates: &mut Vec<String>, asset_bundle: &str) {
    if asset_bundle.trim().is_empty() {
        return;
    }
    let base = PathBuf::from(asset_bundle);
    for relative in [
        "hg-std\\hak\\cep2_custom.hak",
        "hg-gui\\hak\\cep2_custom.hak",
        "diamond\\hak\\cep2_custom.hak",
        "cep23\\hak\\cep2_custom.hak",
        "ee-fixes\\2da\\baseitems.2da",
    ] {
        candidates.push(base.join(relative).to_string_lossy().into_owned());
    }
}

fn load_base_item_model_types_from_path(path: &str) -> Option<Vec<i8>> {
    let bytes = fs::read(path).ok()?;
    let contents = if path.to_ascii_lowercase().ends_with(".hak")
        || path.to_ascii_lowercase().ends_with(".erf")
        || path.to_ascii_lowercase().ends_with(".mod")
    {
        let resource = extract_baseitems_2da_from_erf_container(&bytes)?;
        String::from_utf8_lossy(resource).into_owned()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    parse_base_item_model_types_2da(&contents)
}

fn extract_baseitems_2da_from_erf_container(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < ERF_HEADER_MIN_BYTES {
        return None;
    }
    if !matches!(bytes.get(0..4)?, b"HAK " | b"ERF " | b"MOD " | b"NWM ")
        || bytes.get(4..8)? != b"V1.0"
    {
        return None;
    }

    let entry_count = usize::try_from(read_u32_le(bytes, ERF_ENTRY_COUNT_OFFSET)?).ok()?;
    if entry_count > MAX_REASONABLE_ERF_KEY_COUNT {
        return None;
    }
    let key_list_offset = usize::try_from(read_u32_le(bytes, ERF_KEY_LIST_OFFSET_OFFSET)?).ok()?;
    let resource_list_offset =
        usize::try_from(read_u32_le(bytes, ERF_RESOURCE_LIST_OFFSET_OFFSET)?).ok()?;

    for index in 0..entry_count {
        let key_offset = key_list_offset.checked_add(index.checked_mul(ERF_KEY_ENTRY_BYTES)?)?;
        let key = bytes.get(key_offset..key_offset.checked_add(ERF_KEY_ENTRY_BYTES)?)?;
        let resref = key.get(..ERF_KEY_RESREF_BYTES)?;
        let resource_type = read_u16_le(key, ERF_KEY_RESOURCE_TYPE_OFFSET)?;
        if resource_type != ERF_RESOURCE_2DA_TYPE
            || !erf_resref_eq_ignore_ascii_case(resref, b"baseitems")
        {
            continue;
        }

        let resource_id = usize::try_from(read_u32_le(key, ERF_KEY_RESOURCE_ID_OFFSET)?).ok()?;
        let resource_entry_offset =
            resource_list_offset.checked_add(resource_id.checked_mul(ERF_RESOURCE_ENTRY_BYTES)?)?;
        let resource_offset = usize::try_from(read_u32_le(bytes, resource_entry_offset)?).ok()?;
        let resource_size =
            usize::try_from(read_u32_le(bytes, resource_entry_offset + CNW_LENGTH_BYTES)?).ok()?;
        return bytes.get(resource_offset..resource_offset.checked_add(resource_size)?);
    }

    None
}

fn erf_resref_eq_ignore_ascii_case(raw_resref: &[u8], expected: &[u8]) -> bool {
    let end = raw_resref
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(raw_resref.len());
    raw_resref[..end].eq_ignore_ascii_case(expected)
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

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let chunk = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes(chunk.try_into().ok()?))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let target = bytes.get_mut(offset..offset.checked_add(CNW_LENGTH_BYTES)?)?;
    target.copy_from_slice(&value.to_le_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn debug_quickbar_transport_split_from_short_normalized_dump() {
        let path = std::env::var("HGBRIDGE_QUICKBAR_DUMP")
            .expect("set HGBRIDGE_QUICKBAR_DUMP to a quarantined GuiQuickbar_SetAllButtons dump");
        let normalized = fs::read(&path).expect("read quickbar dump");
        assert!(normalized.len() > HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES);
        assert_eq!(normalized[0], b'P');
        assert_eq!(normalized[1], QUICKBAR_MAJOR);
        assert_eq!(normalized[2], SET_ALL_BUTTONS_MINOR);

        let raw = reconstruct_short_declared_quickbar_dump(&normalized)
            .expect("reconstruct legacy/stale quickbar transport");
        let body_and_tail = &raw[HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES..];
        let max_tail = body_and_tail
            .len()
            .saturating_sub(LEGACY_QUICKBAR_BUTTON_COUNT)
            .min(128);

        eprintln!("dump={path}");
        eprintln!(
            "normalized_len={} raw_len={} body_and_tail={} max_tail={}",
            normalized.len(),
            raw.len(),
            body_and_tail.len(),
            max_tail
        );

        for fragment_tail_len in 0..=max_tail {
            let Some(read_body_len) = body_and_tail.len().checked_sub(fragment_tail_len) else {
                continue;
            };
            let mut read_buffer = Vec::with_capacity(read_body_len + CNW_LENGTH_BYTES);
            read_buffer.extend_from_slice(&[0, 0, 0, 0]);
            read_buffer.extend_from_slice(&body_and_tail[..read_body_len]);
            let fragments = &body_and_tail[read_body_len..];
            let Some((buttons, final_cursor)) = parse_quickbar_read_buffer_with_fragments(
                &read_buffer,
                fragments,
                LEGACY_QUICKBAR_READ_CURSOR_START,
            ) else {
                continue;
            };

            let translated_item_slots = buttons
                .iter()
                .filter(|button| matches!(button.kind, QuickbarButtonKind::Item { .. }))
                .count();
            let spell_slots = buttons
                .iter()
                .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
                .count();
            let general_slots = buttons
                .iter()
                .filter(|button| matches!(button.kind, QuickbarButtonKind::General { .. }))
                .count();
            let item_candidate_slots = buttons
                .iter()
                .filter(|button| matches!(button.kind, QuickbarButtonKind::ItemCandidate))
                .count();
            let unsupported_slots = buttons
                .iter()
                .filter(|button| matches!(button.kind, QuickbarButtonKind::Unsupported))
                .count();
            let trailing_read_bytes = read_buffer.len().saturating_sub(final_cursor);
            if translated_item_slots != 0
                || spell_slots != 0
                || item_candidate_slots != 0
                || unsupported_slots < LEGACY_QUICKBAR_BUTTON_COUNT
                || fragment_tail_len == 19
            {
                eprintln!(
                    "tail={fragment_tail_len} read_body={read_body_len} final={final_cursor}/{} trailing={trailing_read_bytes} items={translated_item_slots} spells={spell_slots} general={general_slots} candidates={item_candidate_slots} unsupported={unsupported_slots}",
                    read_buffer.len()
                );
            }
        }
    }

    fn reconstruct_short_declared_quickbar_dump(normalized: &[u8]) -> Option<Vec<u8>> {
        let declared = usize::try_from(read_le_u32(normalized, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
        let normalized_read_size = declared.checked_sub(HIGH_LEVEL_HEADER_BYTES)?;
        let legacy_read_size = normalized_read_size.checked_sub(2)?;
        let normalized_read_start = HIGH_LEVEL_HEADER_BYTES.checked_add(CNW_LENGTH_BYTES)?;
        let normalized_read_end = normalized_read_start.checked_add(legacy_read_size)?;
        let prefix_start = normalized_read_end;
        let prefix_end = prefix_start.checked_add(2)?;
        let tail = normalized.get(prefix_end..)?;
        let prefix = normalized.get(prefix_start..prefix_end)?;
        let mut raw = Vec::with_capacity(normalized.len().checked_sub(2)?);
        raw.extend_from_slice(normalized.get(..HIGH_LEVEL_HEADER_BYTES)?);
        raw.extend_from_slice(prefix);
        raw.extend_from_slice(&[0x58, 0x04]);
        raw.extend_from_slice(normalized.get(normalized_read_start..normalized_read_end)?);
        raw.extend_from_slice(tail);
        Some(raw)
    }
}
