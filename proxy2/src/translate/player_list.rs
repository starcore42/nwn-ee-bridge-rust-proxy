//! `PlayerList_All/Add/Delete` semantic translation.
//!
//! This module keeps the PlayerList rule small and explicit: after the legacy
//! packet has been normalized to an EE CNW envelope, insert EE's empty platform
//! identity field (`BYTE 0`, empty `CExoString`) when the legacy entry does
//! not already contain it.
//!
//! Decompile notes:
//! - Diamond's `sub_453BD0` PlayerList reader uses `sub_4FBB40` only as a
//!   read-overflow guard; the first real packet field is the `sub_4FB4C0`
//!   `ReadBOOL` at `00453CCE`. For `All`, case 0 clears the local list, reads
//!   an 8-bit count at `00453D66`, and falls into the shared per-entry body.
//! - EE's `CNWSMessage::SendServerToPlayerPlayerList_All` writes the same
//!   leading BOOL before the 8-bit count, and EE's
//!   `HandleServerToPlayerPlayerList` reads it before the per-entry count/body.
//! - EE additionally reads `ReadBYTE(8,0)` + `ReadCExoString(32)` immediately
//!   after each `has_creature` BOOL for platform identity. Diamond has no
//!   equivalent field in this packet family.
//! - `PlayerList_Delete` is intentionally claimed here even though the bytes
//!   are identical between Diamond and EE. EE
//!   `SendServerToPlayerPlayerList_Delete` writes the common leading BOOL and a
//!   single `WriteDWORD(32)` player id before sending major `0x0A`, minor
//!   `0x03`; Diamond client `sub_453BD0` case 2 reads the same `DWORD(32)`
//!   after the common BOOL. There is no per-entry body and no EE platform
//!   identity field to insert.
//! - Diamond server `0x44A4C0` emits the creature body as object id, two
//!   `0x508D00` CExoLocStringServer writes, portrait WORD, and optional
//!   CResRef. HG captures show the self entry in `PlayerList_All` may carry a
//!   legacy inline second-name locstring with its four-byte zero slot trailing
//!   the string. We normalize that narrow shape only when the complete
//!   PlayerList body then validates exactly.
//! - HG captures also show a coalesced Diamond `PlayerList_All` span whose CNW
//!   declared length is stale/short (`0x3d`) even though the span carries one
//!   complete legacy read body followed by the exact CNW fragment tail. We only
//!   repair that declaration after an exact legacy PlayerList parse proves a
//!   unique read-body/tail split; following live-object continuation bytes are
//!   therefore rejected instead of being folded into PlayerList.
//! - EE PlayerList object-id fields are read with `ReadOBJECTIDServer`, whose
//!   decompile consumes a raw DWORD and clears only bit 31. Real Diamond/HG
//!   session-local player ids such as `0xffff_fffe`/`0xffff_ff8e` therefore
//!   remain meaningful as stripped ids `0x7fff_fffe`/`0x7fff_ff8e`. Only the
//!   literal invalid OBJECTID wire value (`0x7f000000`) is invalid; high
//!   session ids must not be collapsed to invalid or later live-object
//!   creature add/update lifecycle ids stop matching.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const PLAYER_LIST_MAJOR: u8 = 0x0A;
const PLAYER_LIST_ALL_MINOR: u8 = 0x01;
const PLAYER_LIST_ADD_MINOR: u8 = 0x02;
const PLAYER_LIST_DELETE_MINOR: u8 = 0x03;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_CURSOR_START: usize = 4;
const CRESREF_TEXT_BYTES: usize = 16;
const MAX_REASONABLE_PAYLOAD: usize = 256 * 1024;
const EE_EMPTY_IDENTITY: [u8; 5] = [0, 0, 0, 0, 0];
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const NWN_OBJECT_INVALID: u32 = 0x7F00_0000;
const EE_SERVER_OBJECT_ID_MARKER_BIT: u32 = 0x8000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerListObjectIds {
    pub player_object_id: u32,
    pub creature_object_id: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct PlayerListRewriteSummary {
    pub minor: u8,
    pub old_declared: u32,
    pub new_declared: u32,
    pub entries: u8,
    pub insertions: u32,
    pub bytes_inserted: u32,
    pub old_fragment_bytes: u32,
    pub new_fragment_bytes: u32,
    pub consumed_fragment_bits: u32,
    pub fragments_rewritten: bool,
    pub locstring_length_repairs: u32,
    pub object_id_repairs: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub normalized_prefixed_short_declared: bool,
    pub normalized_short_declared: bool,
    pub normalized_exact_tail_short_declared: bool,
}

#[derive(Debug, Clone, Copy)]
struct Layout {
    declared: u32,
    read_size: usize,
    fragment_size: usize,
    normalized_prefixed_short_declared: bool,
    normalized_short_declared: bool,
    normalized_exact_tail_short_declared: bool,
}

#[derive(Debug, Clone)]
struct Reader<'a> {
    read_buffer: &'a [u8],
    read_size: usize,
    fragments: &'a [u8],
    cursor: usize,
    fragment_cursor: usize,
    fragment_bit: u8,
    final_fragment_bits: u8,
}

#[derive(Debug, Clone, Default)]
struct Probe {
    entry_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlatformIdentityShape {
    /// Rewrite mode for legacy/Diamond streams: skip an EE platform identity
    /// field when the decompile-backed cursor proves one is already present,
    /// otherwise insert EE's empty identity before the optional creature body.
    InsertMissing,
    EeRequired,
}

#[derive(Debug, Clone)]
struct ParsedPlayerList {
    entry_count: u8,
    insert_offsets: Vec<usize>,
    locstring_repairs: Vec<LegacyLocStringLengthRepair>,
    object_id_repairs: Vec<(usize, u32)>,
    object_ids: Vec<PlayerListObjectIds>,
    consumed_fragment_bits: usize,
    consumed_fragment_bytes: usize,
    final_fragment_bits: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyLocStringLengthRepair {
    /// Diamond/HG PlayerList captures can encode the second display-name
    /// locstring as raw printable bytes followed by a four-byte zero slot where
    /// EE's `sub_1409735F0` expects the missing CExoString length. Rewrite by
    /// inserting that length before the raw bytes and dropping the zero slot.
    InsertLengthAndDropZeroSlot { raw_start: usize, raw_end: usize },

    /// The Diamond reader (`sub_453BD0`) consumes two locstring helpers and then
    /// joins them with a space when the second is non-empty. Some HG entries put
    /// a raw first-name/title prefix in front of an otherwise valid second
    /// length-prefixed string while the first helper's length is zero. Keep the
    /// byte order and repair the first helper's zero length to consume the raw
    /// prefix, leaving the following CExoString as the second helper.
    MoveRawPrefixIntoPreviousEmpty {
        previous_length_offset: usize,
        raw_start: usize,
        raw_end: usize,
    },

    /// Some second-name locstrings omit only the four-byte length prefix and are
    /// followed immediately by the decompile-confirmed portrait WORD/CResRef
    /// fields. Rewrite by inserting the missing CExoString length before the
    /// raw bytes; the following portrait boundary remains in place.
    InsertMissingLengthBeforePortrait { raw_start: usize, raw_end: usize },

    /// A closely related HG PlayerList shape keeps the CExoString length slot
    /// but leaves it as zero, then places the raw second-name bytes before the
    /// portrait WORD/CResRef boundary. Rewrite the existing zero length to the
    /// raw byte count.
    ExtendCurrentEmptyBeforePortrait {
        length_offset: usize,
        raw_start: usize,
        raw_end: usize,
    },
}

impl LegacyLocStringLengthRepair {
    fn net_read_size_delta(self) -> usize {
        match self {
            Self::InsertLengthAndDropZeroSlot { .. }
            | Self::MoveRawPrefixIntoPreviousEmpty { .. }
            | Self::ExtendCurrentEmptyBeforePortrait { .. } => 0,
            Self::InsertMissingLengthBeforePortrait { .. } => CNW_LENGTH_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LocStringRead {
    empty_inline_length_offset: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerListReadMutation {
    InsertIdentity { offset: usize },
    InsertLengthPrefix { offset: usize, length: u32 },
    RemoveZeroLengthSlot { offset: usize },
    WriteLength { offset: usize, length: u32 },
    WriteObjectId { offset: usize, value: u32 },
}

impl PlayerListReadMutation {
    fn offset(self) -> usize {
        match self {
            Self::InsertIdentity { offset }
            | Self::InsertLengthPrefix { offset, .. }
            | Self::RemoveZeroLengthSlot { offset }
            | Self::WriteLength { offset, .. }
            | Self::WriteObjectId { offset, .. } => offset,
        }
    }

    fn sort_rank(self) -> u8 {
        match self {
            Self::RemoveZeroLengthSlot { .. } => 0,
            Self::WriteLength { .. } | Self::WriteObjectId { .. } => 1,
            Self::InsertLengthPrefix { .. } => 2,
            Self::InsertIdentity { .. } => 3,
        }
    }
}

pub fn rewrite_player_list_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<PlayerListRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != PLAYER_LIST_MAJOR
        || !matches!(
            payload[2],
            PLAYER_LIST_ALL_MINOR | PLAYER_LIST_ADD_MINOR | PLAYER_LIST_DELETE_MINOR
        )
        || payload.len() > MAX_REASONABLE_PAYLOAD
    {
        return None;
    }

    let old_payload_length = payload.len();
    let minor = payload[2];
    let layout = normalize_player_list_layout(payload)?;
    let cnw = &payload[HIGH_LEVEL_HEADER_BYTES..];
    let fragments = &cnw[layout.read_size..layout.read_size + layout.fragment_size];
    let parsed = parse_player_list_cnw(
        cnw,
        minor,
        layout.read_size,
        fragments,
        PlatformIdentityShape::InsertMissing,
    )
    .or_else(|| {
        parse_player_list_cnw(
            cnw,
            minor,
            layout.read_size,
            fragments,
            PlatformIdentityShape::EeRequired,
        )
    })?;

    let original_fragments = payload[HIGH_LEVEL_HEADER_BYTES + layout.read_size
        ..HIGH_LEVEL_HEADER_BYTES + layout.read_size + layout.fragment_size]
        .to_vec();
    let mut fragment_bits = decode_cnw_fragment_bits(&original_fragments)?;
    if parsed.consumed_fragment_bits > fragment_bits.len() {
        return None;
    }

    let fragments_rewritten = parsed.consumed_fragment_bytes != layout.fragment_size
        || parsed.final_fragment_bits != (parsed.consumed_fragment_bits % 8) as u8;
    let rewritten_fragments = if fragments_rewritten {
        fragment_bits.truncate(parsed.consumed_fragment_bits);
        refresh_cnw_fragment_final_bit_header(&mut fragment_bits);
        let mut packed = pack_cnw_msb_bits(&fragment_bits);
        if packed.is_empty() {
            packed.push(0);
        }
        packed
    } else {
        original_fragments
    };

    let identity_bytes_inserted = parsed.insert_offsets.len() * EE_EMPTY_IDENTITY.len();
    let locstring_bytes_inserted: usize = parsed
        .locstring_repairs
        .iter()
        .copied()
        .map(LegacyLocStringLengthRepair::net_read_size_delta)
        .sum();
    let total_inserted = identity_bytes_inserted.checked_add(locstring_bytes_inserted)?;
    if payload.len() > MAX_REASONABLE_PAYLOAD.saturating_sub(total_inserted) {
        return None;
    }

    let mut mutations = Vec::new();
    for repair in parsed.locstring_repairs.iter().copied() {
        enqueue_legacy_locstring_length_repair(repair, &mut mutations)?;
    }
    for (offset, value) in parsed.object_id_repairs.iter().copied() {
        mutations.push(PlayerListReadMutation::WriteObjectId { offset, value });
    }
    for offset in parsed.insert_offsets.iter().copied() {
        if offset > layout.read_size {
            return None;
        }
        mutations.push(PlayerListReadMutation::InsertIdentity { offset });
    }
    apply_player_list_read_mutations(payload, mutations)?;

    let normalized_declared_base = (layout.read_size + HIGH_LEVEL_HEADER_BYTES) as u32;
    let new_declared = normalized_declared_base.checked_add(total_inserted as u32)?;
    write_u32_le(payload, HIGH_LEVEL_HEADER_BYTES, new_declared)?;

    if fragments_rewritten {
        let new_fragment_offset = HIGH_LEVEL_HEADER_BYTES + layout.read_size + total_inserted;
        if new_fragment_offset > payload.len() {
            return None;
        }
        payload.truncate(new_fragment_offset);
        payload.extend_from_slice(&rewritten_fragments);
    }

    Some(PlayerListRewriteSummary {
        minor,
        old_declared: layout.declared,
        new_declared,
        entries: parsed.entry_count,
        insertions: parsed.insert_offsets.len() as u32,
        bytes_inserted: total_inserted as u32,
        old_fragment_bytes: layout.fragment_size as u32,
        new_fragment_bytes: rewritten_fragments.len() as u32,
        consumed_fragment_bits: parsed.consumed_fragment_bits as u32,
        fragments_rewritten,
        locstring_length_repairs: parsed.locstring_repairs.len() as u32,
        object_id_repairs: parsed.object_id_repairs.len() as u32,
        old_payload_length,
        new_payload_length: payload.len(),
        normalized_prefixed_short_declared: layout.normalized_prefixed_short_declared,
        normalized_short_declared: layout.normalized_short_declared,
        normalized_exact_tail_short_declared: layout.normalized_exact_tail_short_declared,
    })
}

pub fn ee_player_list_payload_shape_valid(payload: &[u8]) -> bool {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != PLAYER_LIST_MAJOR
        || !matches!(
            payload[2],
            PLAYER_LIST_ALL_MINOR | PLAYER_LIST_ADD_MINOR | PLAYER_LIST_DELETE_MINOR
        )
        || payload.len() > MAX_REASONABLE_PAYLOAD
    {
        return false;
    }

    let Some(layout) = probe_current_layout_for(
        payload,
        false,
        false,
        false,
        &[PlatformIdentityShape::EeRequired],
    ) else {
        return false;
    };

    let cnw = &payload[HIGH_LEVEL_HEADER_BYTES..];
    let fragments = &cnw[layout.read_size..layout.read_size + layout.fragment_size];
    parse_player_list_cnw(
        cnw,
        payload[2],
        layout.read_size,
        fragments,
        PlatformIdentityShape::EeRequired,
    )
    .is_some()
}

pub fn object_ids_from_verified_payload(payload: &[u8]) -> Option<Vec<PlayerListObjectIds>> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != PLAYER_LIST_MAJOR
        || !matches!(
            payload[2],
            PLAYER_LIST_ALL_MINOR | PLAYER_LIST_ADD_MINOR | PLAYER_LIST_DELETE_MINOR
        )
        || payload.len() > MAX_REASONABLE_PAYLOAD
    {
        return None;
    }

    let layout = probe_current_layout_for(
        payload,
        false,
        false,
        false,
        &[PlatformIdentityShape::EeRequired],
    )?;
    let cnw = &payload[HIGH_LEVEL_HEADER_BYTES..];
    let fragments = &cnw[layout.read_size..layout.read_size + layout.fragment_size];
    let parsed = parse_player_list_cnw(
        cnw,
        payload[2],
        layout.read_size,
        fragments,
        PlatformIdentityShape::EeRequired,
    )?;
    Some(parsed.object_ids)
}

fn normalize_player_list_layout(payload: &mut Vec<u8>) -> Option<Layout> {
    if let Some(layout) = probe_current_layout(payload, false, false) {
        return Some(layout);
    }
    if normalize_short_declared(payload, false) {
        return probe_current_layout(payload, false, true);
    }
    if normalize_prefixed_short_declared(payload) {
        return probe_current_layout(payload, true, false);
    }
    if normalize_exact_tail_short_declared(payload) {
        return probe_current_layout_for(
            payload,
            false,
            false,
            true,
            &[
                PlatformIdentityShape::InsertMissing,
                PlatformIdentityShape::EeRequired,
            ],
        );
    }
    None
}

fn probe_current_layout(
    payload: &[u8],
    normalized_prefixed_short_declared: bool,
    normalized_short_declared: bool,
) -> Option<Layout> {
    probe_current_layout_for(
        payload,
        normalized_prefixed_short_declared,
        normalized_short_declared,
        false,
        &[
            PlatformIdentityShape::InsertMissing,
            PlatformIdentityShape::EeRequired,
        ],
    )
}

fn probe_current_layout_for(
    payload: &[u8],
    normalized_prefixed_short_declared: bool,
    normalized_short_declared: bool,
    normalized_exact_tail_short_declared: bool,
    identity_shapes: &[PlatformIdentityShape],
) -> Option<Layout> {
    let payload_size = payload.len().checked_sub(HIGH_LEVEL_HEADER_BYTES)?;
    let declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    if declared < (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u32 {
        return None;
    }
    let read_size = declared as usize - HIGH_LEVEL_HEADER_BYTES;
    if read_size < READ_CURSOR_START || read_size > payload_size {
        return None;
    }
    let fragment_size = payload_size - read_size;
    if fragment_size == 0 {
        return None;
    }
    let minor = payload[2];
    let cnw = &payload[HIGH_LEVEL_HEADER_BYTES..];
    let mut probe = Probe::default();
    if !probe_player_list_layout(
        cnw,
        minor,
        read_size,
        fragment_size,
        identity_shapes,
        &mut probe,
    ) {
        return None;
    }
    Some(Layout {
        declared,
        read_size,
        fragment_size,
        normalized_prefixed_short_declared,
        normalized_short_declared,
        normalized_exact_tail_short_declared,
    })
}

fn normalize_short_declared(payload: &mut Vec<u8>, prefixed: bool) -> bool {
    let legacy_declared_offset = if prefixed {
        HIGH_LEVEL_HEADER_BYTES + 2
    } else {
        HIGH_LEVEL_HEADER_BYTES
    };
    let Some(legacy_declared) = read_u16_le(payload, legacy_declared_offset) else {
        return false;
    };
    if legacy_declared < (HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES) as u16 {
        return false;
    }

    let legacy_read_size = legacy_declared as usize - HIGH_LEVEL_HEADER_BYTES;
    if legacy_read_size < 2 + READ_CURSOR_START {
        return false;
    }
    let data_start = legacy_declared_offset + 2;
    let data_len = legacy_read_size - 2;
    let tail_start = data_start + data_len;
    if tail_start > payload.len() {
        return false;
    }
    let mut fragment_bytes = Vec::new();
    if prefixed {
        fragment_bytes.extend_from_slice(&payload[HIGH_LEVEL_HEADER_BYTES..legacy_declared_offset]);
    }
    fragment_bytes.extend_from_slice(&payload[tail_start..]);
    if fragment_bytes.is_empty() || fragment_bytes.len() > 128 {
        return false;
    }

    let normalized_read_size = legacy_read_size + 2;
    let normalized_declared = (normalized_read_size + HIGH_LEVEL_HEADER_BYTES) as u32;
    let mut candidate = Vec::with_capacity(payload.len() + 2);
    candidate.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    candidate.extend_from_slice(&normalized_declared.to_le_bytes());
    candidate.extend_from_slice(&payload[data_start..tail_start]);
    candidate.extend_from_slice(&fragment_bytes);

    let minor = payload[2];
    let cnw = &candidate[HIGH_LEVEL_HEADER_BYTES..];
    let mut probe = Probe::default();
    if !probe_player_list_layout(
        cnw,
        minor,
        normalized_read_size,
        fragment_bytes.len(),
        &[
            PlatformIdentityShape::InsertMissing,
            PlatformIdentityShape::EeRequired,
        ],
        &mut probe,
    ) {
        return false;
    }

    *payload = candidate;
    true
}

fn normalize_prefixed_short_declared(payload: &mut Vec<u8>) -> bool {
    normalize_short_declared(payload, true)
}

fn normalize_exact_tail_short_declared(payload: &mut Vec<u8>) -> bool {
    let Some(payload_size) = payload.len().checked_sub(HIGH_LEVEL_HEADER_BYTES) else {
        return false;
    };
    let Some(old_declared) = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES) else {
        return false;
    };
    let full_declared = match u32::try_from(payload_size + HIGH_LEVEL_HEADER_BYTES) {
        Ok(value) => value,
        Err(_) => return false,
    };
    if old_declared >= full_declared || payload_size <= READ_CURSOR_START {
        return false;
    }

    let minor = payload[2];
    let max_fragment_size = 128.min(payload_size.saturating_sub(READ_CURSOR_START));
    let mut accepted: Option<Vec<u8>> = None;

    for fragment_size in 1..=max_fragment_size {
        let Some(read_size) = payload_size.checked_sub(fragment_size) else {
            continue;
        };
        if read_size < READ_CURSOR_START {
            continue;
        }
        let normalized_declared = match u32::try_from(read_size + HIGH_LEVEL_HEADER_BYTES) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let mut candidate = payload.clone();
        if write_u32_le(&mut candidate, HIGH_LEVEL_HEADER_BYTES, normalized_declared).is_none() {
            continue;
        }
        let cnw = &candidate[HIGH_LEVEL_HEADER_BYTES..];
        let mut probe = Probe::default();
        if !probe_player_list_layout(
            cnw,
            minor,
            read_size,
            fragment_size,
            &[
                PlatformIdentityShape::InsertMissing,
                PlatformIdentityShape::EeRequired,
            ],
            &mut probe,
        ) {
            continue;
        }
        if accepted.is_some() {
            return false;
        }
        accepted = Some(candidate);
    }

    if let Some(candidate) = accepted {
        *payload = candidate;
        return true;
    }
    false
}

fn probe_player_list_layout(
    cnw: &[u8],
    minor: u8,
    read_size: usize,
    fragment_size: usize,
    identity_shapes: &[PlatformIdentityShape],
    result: &mut Probe,
) -> bool {
    if read_size < READ_CURSOR_START
        || read_size > cnw.len()
        || fragment_size == 0
        || read_size + fragment_size != cnw.len()
    {
        return false;
    }

    for shape in identity_shapes {
        let Some(parsed) = parse_player_list_cnw(cnw, minor, read_size, &cnw[read_size..], *shape)
        else {
            continue;
        };
        if parsed.consumed_fragment_bytes == fragment_size
            && parsed.consumed_fragment_bits >= 3
            && parsed.final_fragment_bits == (parsed.consumed_fragment_bits % 8) as u8
        {
            result.entry_count = parsed.entry_count;
            return true;
        }
    }

    false
}

fn parse_player_list_cnw(
    cnw: &[u8],
    minor: u8,
    read_size: usize,
    fragments: &[u8],
    identity_shape: PlatformIdentityShape,
) -> Option<ParsedPlayerList> {
    let mut reader = Reader {
        read_buffer: cnw,
        read_size,
        fragments,
        cursor: READ_CURSOR_START,
        fragment_cursor: 0,
        fragment_bit: 0,
        final_fragment_bits: 0,
    };
    let final_fragment_bits = reader.read_bits(CNW_FRAGMENT_HEADER_BITS as u8)? as u8;
    reader.final_fragment_bits = final_fragment_bits;
    let _module_pvp_flag = reader.read_bool()?;

    if minor == PLAYER_LIST_DELETE_MINOR {
        let _deleted_player_id = reader.read_u32()?;
        let consumed_fragment_bits = reader.fragment_cursor * 8 + usize::from(reader.fragment_bit);
        let consumed_fragment_bytes =
            reader.fragment_cursor + usize::from(reader.fragment_bit != 0);
        if reader.cursor != read_size
            || consumed_fragment_bits < 3
            || consumed_fragment_bytes > fragments.len()
        {
            return None;
        }
        return Some(ParsedPlayerList {
            entry_count: 1,
            insert_offsets: Vec::new(),
            locstring_repairs: Vec::new(),
            object_id_repairs: Vec::new(),
            object_ids: Vec::new(),
            consumed_fragment_bits,
            consumed_fragment_bytes,
            final_fragment_bits,
        });
    }

    let entry_count = match minor {
        PLAYER_LIST_ALL_MINOR => reader.read_u8()?,
        PLAYER_LIST_ADD_MINOR => 1,
        _ => return None,
    };
    if entry_count == 0 {
        return None;
    }

    let mut insert_offsets = Vec::new();
    let mut locstring_repairs = Vec::new();
    let mut object_id_repairs = Vec::new();
    let mut object_ids = Vec::new();
    for _ in 0..entry_count {
        let _player_id = reader.read_u32()?;
        let player_object_offset = reader.cursor;
        let player_object = reader.read_u32()?;
        record_player_list_object_id_rewrite(
            player_object,
            player_object_offset,
            identity_shape,
            &mut object_id_repairs,
        )?;
        let _dm = reader.read_bool()?;
        reader.read_string(256)?;
        let has_creature = reader.read_bool()?;

        match identity_shape {
            PlatformIdentityShape::InsertMissing => {
                if looks_like_ee_identity(&reader) {
                    skip_ee_identity(&mut reader)?;
                } else {
                    insert_offsets.push(reader.cursor);
                }
            }
            PlatformIdentityShape::EeRequired => {
                if !looks_like_ee_identity(&reader) {
                    return None;
                }
                skip_ee_identity(&mut reader)?;
            }
        }

        let mut creature_object_id = None;
        if has_creature {
            let creature_object_offset = reader.cursor;
            let creature_object = reader.read_u32()?;
            creature_object_id = Some(creature_object);
            record_player_list_object_id_rewrite(
                creature_object,
                creature_object_offset,
                identity_shape,
                &mut object_id_repairs,
            )?;
            let first_locstring = reader.read_locstring(false, None, &mut locstring_repairs)?;
            reader.read_locstring(
                minor == PLAYER_LIST_ALL_MINOR
                    && identity_shape == PlatformIdentityShape::InsertMissing,
                first_locstring.empty_inline_length_offset,
                &mut locstring_repairs,
            )?;
            let portrait_id = reader.read_u16()?;
            if portrait_id >= 0xFFFE {
                reader.read_resref16()?;
            }
        }
        object_ids.push(PlayerListObjectIds {
            player_object_id: player_object,
            creature_object_id,
        });
    }

    let consumed_fragment_bits = reader.fragment_cursor * 8 + usize::from(reader.fragment_bit);
    let consumed_fragment_bytes = reader.fragment_cursor + usize::from(reader.fragment_bit != 0);
    if reader.cursor != read_size
        || consumed_fragment_bits < 3
        || consumed_fragment_bytes > fragments.len()
    {
        return None;
    }

    Some(ParsedPlayerList {
        entry_count,
        insert_offsets,
        locstring_repairs,
        object_id_repairs,
        object_ids,
        consumed_fragment_bits,
        consumed_fragment_bytes,
        final_fragment_bits,
    })
}

impl<'a> Reader<'a> {
    fn read_bit(&mut self) -> Option<u32> {
        if self.fragment_cursor >= self.fragments.len() || self.fragment_bit >= 8 {
            return None;
        }
        let bit = (self.fragments[self.fragment_cursor] >> (7 - self.fragment_bit)) & 1;
        self.fragment_bit += 1;
        if self.fragment_bit >= 8 {
            self.fragment_bit = 0;
            self.fragment_cursor += 1;
        }
        Some(u32::from(bit))
    }

    fn read_bits(&mut self, bit_count: u8) -> Option<u32> {
        if bit_count > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..bit_count {
            value = (value << 1) | self.read_bit()?;
        }
        Some(value)
    }

    fn read_bool(&mut self) -> Option<bool> {
        Some(self.read_bit()? != 0)
    }

    fn read_u8(&mut self) -> Option<u8> {
        let value = *self.read_buffer.get(self.cursor)?;
        self.cursor += 1;
        Some(value)
    }

    fn read_u16(&mut self) -> Option<u16> {
        let value = read_u16_le(self.read_buffer, self.cursor)?;
        self.cursor += 2;
        Some(value)
    }

    fn read_u32(&mut self) -> Option<u32> {
        let value = read_u32_le(self.read_buffer, self.cursor)?;
        self.cursor += 4;
        Some(value)
    }

    fn read_string(&mut self, max_length: u32) -> Option<()> {
        let length = self.read_u32()? as usize;
        if length > max_length as usize || length > self.read_size.checked_sub(self.cursor)? {
            return None;
        }
        self.cursor += length;
        Some(())
    }

    fn read_locstring(
        &mut self,
        allow_legacy_suffix_length_repair: bool,
        previous_empty_length_offset: Option<usize>,
        locstring_repairs: &mut Vec<LegacyLocStringLengthRepair>,
    ) -> Option<LocStringRead> {
        let custom_tlk = self.read_bool()?;
        if custom_tlk {
            self.read_bits(1)?;
            self.read_u32()?;
            return Some(LocStringRead::default());
        } else {
            let length_offset = self.cursor;
            let length = self.read_u32()? as usize;
            if length <= 4096 && length <= self.read_size.checked_sub(self.cursor)? {
                self.cursor += length;
                if length == 0 && allow_legacy_suffix_length_repair {
                    if let Some(repair) = self.read_legacy_current_empty_before_portrait_string(
                        length_offset,
                        self.cursor,
                        4096,
                    ) {
                        locstring_repairs.push(repair);
                        return Some(LocStringRead::default());
                    }
                }
                return Some(LocStringRead {
                    empty_inline_length_offset: (length == 0).then_some(length_offset),
                });
            }
            self.cursor = length_offset;
            if !allow_legacy_suffix_length_repair {
                return None;
            }
            let repair = self.read_legacy_locstring_length_repair(
                length_offset,
                4096,
                previous_empty_length_offset,
            )?;
            locstring_repairs.push(repair);
        }
        Some(LocStringRead::default())
    }

    fn read_legacy_locstring_length_repair(
        &mut self,
        raw_start: usize,
        max_length: usize,
        previous_empty_length_offset: Option<usize>,
    ) -> Option<LegacyLocStringLengthRepair> {
        if let Some(repair) = self.read_legacy_zero_slot_string(raw_start, max_length) {
            return Some(repair);
        }
        if let Some(previous_length_offset) = previous_empty_length_offset {
            if let Some(repair) = self.read_legacy_split_second_name_string(
                previous_length_offset,
                raw_start,
                max_length,
            ) {
                return Some(repair);
            }
        }
        self.read_legacy_missing_length_before_portrait_string(raw_start, max_length)
    }

    fn read_legacy_zero_slot_string(
        &mut self,
        raw_start: usize,
        max_length: usize,
    ) -> Option<LegacyLocStringLengthRepair> {
        if raw_start != self.cursor || raw_start >= self.read_size {
            return None;
        }
        let remaining = self.read_size.checked_sub(raw_start)?;
        let max_candidate = max_length.min(remaining.checked_sub(4)?);
        for raw_len in 1..=max_candidate {
            let raw_end = raw_start.checked_add(raw_len)?;
            let suffix_end = raw_end.checked_add(4)?;
            if suffix_end > self.read_size {
                break;
            }
            let raw = self.read_buffer.get(raw_start..raw_end)?;
            if !raw
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
            {
                continue;
            }
            if self.read_buffer.get(raw_end..suffix_end)? != [0, 0, 0, 0] {
                continue;
            }
            self.cursor = suffix_end;
            return Some(LegacyLocStringLengthRepair::InsertLengthAndDropZeroSlot {
                raw_start,
                raw_end,
            });
        }
        None
    }

    fn read_legacy_split_second_name_string(
        &mut self,
        previous_length_offset: usize,
        raw_start: usize,
        max_length: usize,
    ) -> Option<LegacyLocStringLengthRepair> {
        if raw_start != self.cursor
            || raw_start >= self.read_size
            || previous_length_offset >= raw_start
            || read_u32_le(self.read_buffer, previous_length_offset)? != 0
        {
            return None;
        }
        let remaining = self.read_size.checked_sub(raw_start)?;
        let max_candidate = max_length.min(remaining.checked_sub(CNW_LENGTH_BYTES)?);
        for raw_len in 1..=max_candidate {
            let raw_end = raw_start.checked_add(raw_len)?;
            let suffix_length_offset = raw_end;
            let suffix_start = suffix_length_offset.checked_add(CNW_LENGTH_BYTES)?;
            if suffix_start > self.read_size {
                break;
            }
            let raw = self.read_buffer.get(raw_start..raw_end)?;
            if !raw.iter().all(is_player_list_inline_text_byte) {
                continue;
            }
            let suffix_len = read_u32_le(self.read_buffer, suffix_length_offset)? as usize;
            if suffix_len == 0 || suffix_len > max_length {
                continue;
            }
            let suffix_end = suffix_start.checked_add(suffix_len)?;
            if suffix_end > self.read_size {
                continue;
            }
            let suffix = self.read_buffer.get(suffix_start..suffix_end)?;
            if !suffix.iter().all(is_player_list_inline_text_byte) {
                continue;
            }
            self.cursor = suffix_end;
            return Some(
                LegacyLocStringLengthRepair::MoveRawPrefixIntoPreviousEmpty {
                    previous_length_offset,
                    raw_start,
                    raw_end,
                },
            );
        }
        None
    }

    fn read_legacy_missing_length_before_portrait_string(
        &mut self,
        raw_start: usize,
        max_length: usize,
    ) -> Option<LegacyLocStringLengthRepair> {
        if raw_start != self.cursor || raw_start >= self.read_size {
            return None;
        }
        let remaining = self.read_size.checked_sub(raw_start)?;
        let max_candidate = max_length.min(remaining.checked_sub(2 + CRESREF_TEXT_BYTES)?);
        for raw_len in 1..=max_candidate {
            let raw_end = raw_start.checked_add(raw_len)?;
            let raw = self.read_buffer.get(raw_start..raw_end)?;
            if !raw.iter().all(is_player_list_inline_text_byte) {
                continue;
            }
            let portrait_id = read_u16_le(self.read_buffer, raw_end)?;
            if portrait_id < 0xFFFE {
                continue;
            }
            let resref_start = raw_end.checked_add(2)?;
            let resref_end = resref_start.checked_add(CRESREF_TEXT_BYTES)?;
            if resref_end > self.read_size {
                continue;
            }
            let resref = self.read_buffer.get(resref_start..resref_end)?;
            if !looks_like_player_list_resref16(resref) {
                continue;
            }
            self.cursor = raw_end;
            return Some(
                LegacyLocStringLengthRepair::InsertMissingLengthBeforePortrait {
                    raw_start,
                    raw_end,
                },
            );
        }
        None
    }

    fn read_legacy_current_empty_before_portrait_string(
        &mut self,
        length_offset: usize,
        raw_start: usize,
        max_length: usize,
    ) -> Option<LegacyLocStringLengthRepair> {
        if raw_start != self.cursor
            || raw_start >= self.read_size
            || read_u32_le(self.read_buffer, length_offset)? != 0
        {
            return None;
        }
        let remaining = self.read_size.checked_sub(raw_start)?;
        let max_candidate = max_length.min(remaining.checked_sub(2 + CRESREF_TEXT_BYTES)?);
        for raw_len in 1..=max_candidate {
            let raw_end = raw_start.checked_add(raw_len)?;
            let raw = self.read_buffer.get(raw_start..raw_end)?;
            if !raw.iter().all(is_player_list_inline_text_byte) {
                continue;
            }
            let portrait_id = read_u16_le(self.read_buffer, raw_end)?;
            if portrait_id < 0xFFFE {
                continue;
            }
            let resref_start = raw_end.checked_add(2)?;
            let resref_end = resref_start.checked_add(CRESREF_TEXT_BYTES)?;
            if resref_end > self.read_size {
                continue;
            }
            let resref = self.read_buffer.get(resref_start..resref_end)?;
            if !looks_like_player_list_resref16(resref) {
                continue;
            }
            self.cursor = raw_end;
            return Some(
                LegacyLocStringLengthRepair::ExtendCurrentEmptyBeforePortrait {
                    length_offset,
                    raw_start,
                    raw_end,
                },
            );
        }
        None
    }

    fn read_resref16(&mut self) -> Option<()> {
        if CRESREF_TEXT_BYTES > self.read_size.checked_sub(self.cursor)? {
            return None;
        }
        self.cursor += CRESREF_TEXT_BYTES;
        Some(())
    }
}

fn looks_like_ee_identity(reader: &Reader<'_>) -> bool {
    if reader.cursor > reader.read_size || reader.read_size - reader.cursor < 5 {
        return false;
    }
    let identity_type = reader.read_buffer[reader.cursor];
    let Some(identity_length) = read_u32_le(reader.read_buffer, reader.cursor + 1) else {
        return false;
    };
    identity_type <= 4
        && (identity_length != 0 || identity_type == 0)
        && identity_length <= 256
        && identity_length as usize <= reader.read_size - reader.cursor - 5
}

fn skip_ee_identity(reader: &mut Reader<'_>) -> Option<()> {
    reader.read_u8()?;
    reader.read_string(256)
}

fn record_player_list_object_id_rewrite(
    object_id: u32,
    offset: usize,
    identity_shape: PlatformIdentityShape,
    object_id_repairs: &mut Vec<(usize, u32)>,
) -> Option<()> {
    let ee_wire = ee_player_list_object_id_wire_value(object_id);
    match identity_shape {
        PlatformIdentityShape::InsertMissing => {
            if ee_wire != object_id {
                object_id_repairs.push((offset, ee_wire));
            }
            Some(())
        }
        PlatformIdentityShape::EeRequired => (ee_wire == object_id).then_some(()),
    }
}

fn ee_player_list_object_id_wire_value(object_id: u32) -> u32 {
    if object_id == NWN_OBJECT_INVALID || (object_id & EE_SERVER_OBJECT_ID_MARKER_BIT) != 0 {
        object_id
    } else {
        object_id | EE_SERVER_OBJECT_ID_MARKER_BIT
    }
}

fn decode_cnw_fragment_bits(fragment_bytes: &[u8]) -> Option<Vec<u8>> {
    if fragment_bytes.is_empty() {
        return Some(vec![0, 0, 0]);
    }
    let mut bits = Vec::with_capacity(fragment_bytes.len() * 8);
    for byte in fragment_bytes {
        for bit in 0..8 {
            bits.push((byte >> (7 - bit)) & 1);
        }
    }
    if bits.len() < 3 {
        return None;
    }
    let final_fragment_bits = (u32::from((fragment_bytes[0] & 0x80) != 0) << 2)
        | (u32::from((fragment_bytes[0] & 0x40) != 0) << 1)
        | u32::from((fragment_bytes[0] & 0x20) != 0);
    let meaningful_bits = if final_fragment_bits == 0 {
        fragment_bytes.len() * 8
    } else {
        (fragment_bytes.len() - 1) * 8 + final_fragment_bits as usize
    };
    if meaningful_bits < 3 || meaningful_bits > bits.len() {
        return None;
    }
    bits.truncate(meaningful_bits);
    Some(bits)
}

fn refresh_cnw_fragment_final_bit_header(bits: &mut Vec<u8>) {
    if bits.len() < CNW_FRAGMENT_HEADER_BITS {
        bits.resize(CNW_FRAGMENT_HEADER_BITS, 0);
    }
    let final_fragment_bits = (bits.len() % 8) as u8;
    bits[0] = u8::from((final_fragment_bits & 0x04) != 0);
    bits[1] = u8::from((final_fragment_bits & 0x02) != 0);
    bits[2] = u8::from((final_fragment_bits & 0x01) != 0);
}

fn pack_cnw_msb_bits(bits: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; (bits.len() + 7) / 8];
    for (index, bit) in bits.iter().enumerate() {
        if *bit != 0 {
            bytes[index / 8] |= 1 << (7 - (index % 8));
        }
    }
    bytes
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    bytes
        .get_mut(offset..offset + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn is_player_list_inline_text_byte(byte: &u8) -> bool {
    byte.is_ascii_graphic() || *byte == b' '
}

fn looks_like_player_list_resref16(bytes: &[u8]) -> bool {
    if bytes.len() != CRESREF_TEXT_BYTES {
        return false;
    }
    let mut seen_nul = false;
    for byte in bytes {
        if *byte == 0 {
            seen_nul = true;
            continue;
        }
        if seen_nul {
            return false;
        }
        if !(byte.is_ascii_alphanumeric() || *byte == b'_') {
            return false;
        }
    }
    true
}

fn enqueue_legacy_locstring_length_repair(
    repair: LegacyLocStringLengthRepair,
    mutations: &mut Vec<PlayerListReadMutation>,
) -> Option<()> {
    match repair {
        LegacyLocStringLengthRepair::InsertLengthAndDropZeroSlot { raw_start, raw_end } => {
            let raw_len = u32::try_from(raw_end.checked_sub(raw_start)?).ok()?;
            mutations.push(PlayerListReadMutation::RemoveZeroLengthSlot { offset: raw_end });
            mutations.push(PlayerListReadMutation::InsertLengthPrefix {
                offset: raw_start,
                length: raw_len,
            });
        }
        LegacyLocStringLengthRepair::MoveRawPrefixIntoPreviousEmpty {
            previous_length_offset,
            raw_start,
            raw_end,
        } => {
            let raw_len = u32::try_from(raw_end.checked_sub(raw_start)?).ok()?;
            mutations.push(PlayerListReadMutation::WriteLength {
                offset: previous_length_offset,
                length: raw_len,
            });
        }
        LegacyLocStringLengthRepair::InsertMissingLengthBeforePortrait { raw_start, raw_end } => {
            let raw_len = u32::try_from(raw_end.checked_sub(raw_start)?).ok()?;
            mutations.push(PlayerListReadMutation::InsertLengthPrefix {
                offset: raw_start,
                length: raw_len,
            });
        }
        LegacyLocStringLengthRepair::ExtendCurrentEmptyBeforePortrait {
            length_offset,
            raw_start,
            raw_end,
        } => {
            let raw_len = u32::try_from(raw_end.checked_sub(raw_start)?).ok()?;
            mutations.push(PlayerListReadMutation::WriteLength {
                offset: length_offset,
                length: raw_len,
            });
        }
    }
    Some(())
}

fn apply_player_list_read_mutations(
    payload: &mut Vec<u8>,
    mut mutations: Vec<PlayerListReadMutation>,
) -> Option<()> {
    mutations.sort_by(|left, right| {
        right
            .offset()
            .cmp(&left.offset())
            .then_with(|| left.sort_rank().cmp(&right.sort_rank()))
    });
    for mutation in mutations {
        match mutation {
            PlayerListReadMutation::InsertIdentity { offset } => {
                let insert_at = HIGH_LEVEL_HEADER_BYTES.checked_add(offset)?;
                if insert_at > payload.len() {
                    return None;
                }
                payload.splice(insert_at..insert_at, EE_EMPTY_IDENTITY);
            }
            PlayerListReadMutation::InsertLengthPrefix { offset, length } => {
                let insert_at = HIGH_LEVEL_HEADER_BYTES.checked_add(offset)?;
                if insert_at > payload.len() {
                    return None;
                }
                payload.splice(insert_at..insert_at, length.to_le_bytes());
            }
            PlayerListReadMutation::RemoveZeroLengthSlot { offset } => {
                let remove_start = HIGH_LEVEL_HEADER_BYTES.checked_add(offset)?;
                let remove_end = remove_start.checked_add(CNW_LENGTH_BYTES)?;
                if payload.get(remove_start..remove_end)? != [0, 0, 0, 0] {
                    return None;
                }
                payload.drain(remove_start..remove_end);
            }
            PlayerListReadMutation::WriteLength { offset, length } => {
                let write_at = HIGH_LEVEL_HEADER_BYTES.checked_add(offset)?;
                write_u32_le(payload, write_at, length)?;
            }
            PlayerListReadMutation::WriteObjectId { offset, value } => {
                let write_at = HIGH_LEVEL_HEADER_BYTES.checked_add(offset)?;
                write_u32_le(payload, write_at, value)?;
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_all_inserts_platform_identity_for_each_entry() {
        let mut payload = build_legacy_player_list_all_fixture();

        let summary = rewrite_player_list_payload_if_possible(&mut payload)
            .expect("legacy PlayerList_All should be claimed and rewritten");

        assert_eq!(summary.minor, PLAYER_LIST_ALL_MINOR);
        assert_eq!(summary.entries, 2);
        assert_eq!(summary.insertions, 2);
        assert_eq!(summary.bytes_inserted, 10);
        assert!(ee_player_list_payload_shape_valid(&payload));
    }

    #[test]
    fn legacy_all_preserves_high_session_object_ids() {
        const DIAMOND_SESSION_OBJECT_ID: u32 = 0xFFFF_FFFE;
        let mut payload = build_legacy_player_list_all_fixture_with_object_ids(
            DIAMOND_SESSION_OBJECT_ID,
            DIAMOND_SESSION_OBJECT_ID,
            DIAMOND_SESSION_OBJECT_ID,
            DIAMOND_SESSION_OBJECT_ID,
        );

        let summary = rewrite_player_list_payload_if_possible(&mut payload)
            .expect("legacy PlayerList_All high session ids should normalize to EE");

        assert_eq!(summary.minor, PLAYER_LIST_ALL_MINOR);
        assert_eq!(summary.entries, 2);
        assert_eq!(summary.object_id_repairs, 0);
        assert!(
            payload
                .windows(4)
                .any(|window| window == DIAMOND_SESSION_OBJECT_ID.to_le_bytes())
        );
        assert_eq!(
            payload
                .windows(4)
                .filter(|window| *window == NWN_OBJECT_INVALID.to_le_bytes())
                .count(),
            0
        );
        assert!(ee_player_list_payload_shape_valid(&payload));
    }

    #[test]
    fn legacy_all_rewrites_compact_object_ids_to_ee_external_wire() {
        let mut payload = build_legacy_player_list_all_fixture_with_object_ids(1, 2, 3, 4);

        let summary = rewrite_player_list_payload_if_possible(&mut payload)
            .expect("legacy PlayerList_All compact object ids should normalize to EE");

        assert_eq!(summary.minor, PLAYER_LIST_ALL_MINOR);
        assert_eq!(summary.entries, 2);
        assert_eq!(summary.object_id_repairs, 4);
        for object_id in [0x8000_0001u32, 0x8000_0002, 0x8000_0003, 0x8000_0004] {
            assert!(
                payload
                    .windows(4)
                    .any(|window| window == object_id.to_le_bytes()),
                "rewritten PlayerList payload should contain {object_id:#010x}"
            );
        }
        assert!(ee_player_list_payload_shape_valid(&payload));
    }

    #[test]
    fn hg_legacy_all_repairs_self_second_name_length_slot() {
        let mut payload = include_bytes!(
            "../../fixtures/player_list/hg_player_list_all_suffix_length_legacy.bin"
        )
        .to_vec();

        let summary = rewrite_player_list_payload_if_possible(&mut payload)
            .expect("HG PlayerList_All suffix-length locstring should be normalized");

        assert_eq!(summary.minor, PLAYER_LIST_ALL_MINOR);
        assert_eq!(summary.entries, 6);
        assert_eq!(summary.insertions, 6);
        assert_eq!(summary.locstring_length_repairs, 1);
        assert!(ee_player_list_payload_shape_valid(&payload));
    }

    #[test]
    fn hg_streamed_all_claims_after_transport_continuations_are_reassembled() {
        let mut payload = include_bytes!(
            "../../fixtures/player_list/hg_player_list_all_short_declared_exact_tail_legacy.bin"
        )
        .to_vec();

        let summary = rewrite_player_list_payload_if_possible(&mut payload)
            .expect("stream-reassembled HG PlayerList_All should normalize to EE");

        assert_eq!(summary.minor, PLAYER_LIST_ALL_MINOR);
        assert_eq!(summary.entries, 5);
        assert_eq!(summary.insertions, u32::from(summary.entries));
        assert!(summary.normalized_exact_tail_short_declared);
        assert!(ee_player_list_payload_shape_valid(&payload));
    }

    #[test]
    fn hg_coalesced_all_title_locstring_variants_rewrite_to_exact_ee_shape() {
        let mut payload = include_bytes!(
            "../../fixtures/player_list/hg_player_list_all_title_locstring_variants_seq34.bin"
        )
        .to_vec();

        let summary = rewrite_player_list_payload_if_possible(&mut payload)
            .expect("HG coalesced PlayerList_All title locstring variants should normalize");

        assert_eq!(summary.minor, PLAYER_LIST_ALL_MINOR);
        assert_eq!(summary.entries, 11);
        assert_eq!(summary.insertions, u32::from(summary.entries));
        assert_eq!(summary.locstring_length_repairs, 3);
        assert!(ee_player_list_payload_shape_valid(&payload));
    }

    #[test]
    fn legacy_delete_is_claimed_as_exact_identity_shape() {
        let mut payload =
            include_bytes!("../../fixtures/player_list/hg_player_list_delete_legacy.bin").to_vec();
        let original = payload.clone();

        let summary = rewrite_player_list_payload_if_possible(&mut payload)
            .expect("legacy PlayerList_Delete should be claimed as an exact no-op rewrite");

        assert_eq!(summary.minor, PLAYER_LIST_DELETE_MINOR);
        assert_eq!(summary.old_declared, 11);
        assert_eq!(summary.new_declared, 11);
        assert_eq!(summary.entries, 1);
        assert_eq!(summary.insertions, 0);
        assert_eq!(summary.bytes_inserted, 0);
        assert!(!summary.fragments_rewritten);
        assert_eq!(payload, original);
        assert!(ee_player_list_payload_shape_valid(&payload));
    }

    fn build_legacy_player_list_all_fixture() -> Vec<u8> {
        build_legacy_player_list_all_fixture_with_object_ids(
            0xFFFF_FFAD,
            0x0000_0001,
            0xFFFF_FFAD,
            0x0000_0002,
        )
    }

    fn build_legacy_player_list_all_fixture_with_object_ids(
        alpha_player_object_id: u32,
        alpha_creature_object_id: u32,
        beta_player_object_id: u32,
        beta_creature_object_id: u32,
    ) -> Vec<u8> {
        let mut read = Vec::new();
        read.extend_from_slice(&[0, 0, 0, 0]);
        read.push(2);
        append_legacy_entry(
            &mut read,
            1,
            alpha_player_object_id,
            alpha_creature_object_id,
            "Alpha",
        );
        append_legacy_entry(
            &mut read,
            2,
            beta_player_object_id,
            beta_creature_object_id,
            "Beta",
        );

        let declared = u32::try_from(read.len() + HIGH_LEVEL_HEADER_BYTES)
            .expect("fixture declared length should fit");
        read[0..4].copy_from_slice(&declared.to_le_bytes());

        let mut payload = Vec::new();
        payload.extend_from_slice(&[
            HIGH_LEVEL_ENVELOPE,
            PLAYER_LIST_MAJOR,
            PLAYER_LIST_ALL_MINOR,
        ]);
        payload.extend_from_slice(&read);
        payload.extend_from_slice(&[0x94, 0x40]);
        payload
    }

    fn append_legacy_entry(
        read: &mut Vec<u8>,
        player_id: u32,
        player_object_id: u32,
        creature_object_id: u32,
        name: &str,
    ) {
        read.extend_from_slice(&player_id.to_le_bytes());
        read.extend_from_slice(&player_object_id.to_le_bytes());
        append_string(read, name);
        read.extend_from_slice(&creature_object_id.to_le_bytes());
        append_string(read, "");
        append_string(read, "");
        read.extend_from_slice(&1u16.to_le_bytes());
    }

    fn append_string(read: &mut Vec<u8>, value: &str) {
        let bytes = value.as_bytes();
        read.extend_from_slice(
            &u32::try_from(bytes.len())
                .expect("fixture string length should fit")
                .to_le_bytes(),
        );
        read.extend_from_slice(bytes);
    }
}
