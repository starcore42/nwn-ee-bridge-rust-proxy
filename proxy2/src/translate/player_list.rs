//! `PlayerList_All/Add` semantic translation.
//!
//! This module keeps the PlayerList rule small and explicit: after the legacy
//! packet has been normalized to an EE CNW envelope, insert EE's empty platform
//! identity field (`BYTE 0`, empty `CExoString`) immediately after each
//! `has_creature` BOOL when the legacy entry does not already contain it.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const PLAYER_LIST_MAJOR: u8 = 0x0A;
const PLAYER_LIST_ALL_MINOR: u8 = 0x01;
const PLAYER_LIST_ADD_MINOR: u8 = 0x02;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_CURSOR_START: usize = 4;
const CRESREF_TEXT_BYTES: usize = 16;
const MAX_REASONABLE_PAYLOAD: usize = 256 * 1024;
const EE_EMPTY_IDENTITY: [u8; 5] = [0, 0, 0, 0, 0];

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
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub normalized_prefixed_short_declared: bool,
    pub normalized_short_declared: bool,
}

#[derive(Debug, Clone, Copy)]
struct Layout {
    declared: u32,
    read_size: usize,
    fragment_size: usize,
    normalized_prefixed_short_declared: bool,
    normalized_short_declared: bool,
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

pub fn rewrite_player_list_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<PlayerListRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || payload[0] != HIGH_LEVEL_ENVELOPE
        || payload[1] != PLAYER_LIST_MAJOR
        || (payload[2] != PLAYER_LIST_ALL_MINOR && payload[2] != PLAYER_LIST_ADD_MINOR)
        || payload.len() > MAX_REASONABLE_PAYLOAD
    {
        return None;
    }

    let old_payload_length = payload.len();
    let minor = payload[2];
    let layout = normalize_player_list_layout(payload)?;
    let cnw = &payload[HIGH_LEVEL_HEADER_BYTES..];
    let fragments = &cnw[layout.read_size..layout.read_size + layout.fragment_size];
    let mut reader = Reader {
        read_buffer: cnw,
        read_size: layout.read_size,
        fragments,
        cursor: READ_CURSOR_START,
        fragment_cursor: 0,
        fragment_bit: 0,
        final_fragment_bits: 0,
    };

    let final_fragment_bits = reader.read_bits(3)? as u8;
    let _player_list_flag = reader.read_bool()?;
    reader.final_fragment_bits = final_fragment_bits;

    let entry_count = if minor == PLAYER_LIST_ALL_MINOR {
        reader.read_u8()?
    } else {
        1
    };
    if entry_count == 0 {
        return None;
    }

    let mut insert_offsets = Vec::new();
    for _ in 0..entry_count {
        let _player_id = reader.read_u32()?;
        let _player_object = reader.read_u32()?;
        let _dm = reader.read_bool()?;
        reader.read_string(256)?;
        let has_creature = reader.read_bool()?;

        let identity_offset = reader.cursor;
        if looks_like_ee_identity(&reader) {
            skip_ee_identity(&mut reader)?;
        } else {
            insert_offsets.push(identity_offset);
        }

        if has_creature {
            let _creature_object = reader.read_u32()?;
            reader.read_locstring()?;
            reader.read_locstring()?;
            let portrait_id = reader.read_u16()?;
            if portrait_id >= 0xFFFE {
                reader.read_resref16()?;
            }
        }
    }

    let consumed_fragment_bits = reader.fragment_cursor * 8 + usize::from(reader.fragment_bit);
    let consumed_fragment_bytes = reader.fragment_cursor + usize::from(reader.fragment_bit != 0);
    if consumed_fragment_bits < 3 || consumed_fragment_bytes > layout.fragment_size {
        return None;
    }

    let original_fragments = payload[HIGH_LEVEL_HEADER_BYTES + layout.read_size
        ..HIGH_LEVEL_HEADER_BYTES + layout.read_size + layout.fragment_size]
        .to_vec();
    let mut fragment_bits = decode_cnw_fragment_bits(&original_fragments)?;
    if consumed_fragment_bits > fragment_bits.len() {
        return None;
    }

    let fragments_rewritten = consumed_fragment_bytes != layout.fragment_size
        || reader.final_fragment_bits != (consumed_fragment_bits % 8) as u8;
    let rewritten_fragments = if fragments_rewritten {
        fragment_bits.truncate(consumed_fragment_bits);
        refresh_cnw_fragment_final_bit_header(&mut fragment_bits);
        let mut packed = pack_cnw_msb_bits(&fragment_bits);
        if packed.is_empty() {
            packed.push(0);
        }
        packed
    } else {
        original_fragments
    };

    if insert_offsets.is_empty()
        && !fragments_rewritten
        && layout.declared >= (layout.read_size + HIGH_LEVEL_HEADER_BYTES) as u32
    {
        return None;
    }

    let total_inserted = insert_offsets.len() * EE_EMPTY_IDENTITY.len();
    if payload.len() > MAX_REASONABLE_PAYLOAD.saturating_sub(total_inserted) {
        return None;
    }

    for offset in insert_offsets.iter().rev().copied() {
        if offset > layout.read_size {
            return None;
        }
        payload.splice(
            HIGH_LEVEL_HEADER_BYTES + offset..HIGH_LEVEL_HEADER_BYTES + offset,
            EE_EMPTY_IDENTITY,
        );
    }

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
        entries: entry_count,
        insertions: insert_offsets.len() as u32,
        bytes_inserted: total_inserted as u32,
        old_fragment_bytes: layout.fragment_size as u32,
        new_fragment_bytes: rewritten_fragments.len() as u32,
        consumed_fragment_bits: consumed_fragment_bits as u32,
        fragments_rewritten,
        old_payload_length,
        new_payload_length: payload.len(),
        normalized_prefixed_short_declared: layout.normalized_prefixed_short_declared,
        normalized_short_declared: layout.normalized_short_declared,
    })
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
    None
}

fn probe_current_layout(
    payload: &[u8],
    normalized_prefixed_short_declared: bool,
    normalized_short_declared: bool,
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
    if !probe_player_list_layout(cnw, minor, read_size, fragment_size, &mut probe) {
        return None;
    }
    Some(Layout {
        declared,
        read_size,
        fragment_size,
        normalized_prefixed_short_declared,
        normalized_short_declared,
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

fn probe_player_list_layout(
    cnw: &[u8],
    minor: u8,
    read_size: usize,
    fragment_size: usize,
    result: &mut Probe,
) -> bool {
    if read_size < READ_CURSOR_START
        || read_size > cnw.len()
        || fragment_size == 0
        || read_size + fragment_size != cnw.len()
    {
        return false;
    }

    let mut reader = Reader {
        read_buffer: cnw,
        read_size,
        fragments: &cnw[read_size..],
        cursor: READ_CURSOR_START,
        fragment_cursor: 0,
        fragment_bit: 0,
        final_fragment_bits: 0,
    };
    let Some(final_fragment_bits) = reader.read_bits(3).map(|value| value as u8) else {
        return false;
    };
    let Some(_) = reader.read_bool() else {
        return false;
    };
    reader.final_fragment_bits = final_fragment_bits;

    let entry_count = if minor == PLAYER_LIST_ALL_MINOR {
        let Some(count) = reader.read_u8() else {
            return false;
        };
        count
    } else {
        1
    };
    if entry_count == 0 {
        return false;
    }

    for _ in 0..entry_count {
        if reader.read_u32().is_none()
            || reader.read_u32().is_none()
            || reader.read_bool().is_none()
            || reader.read_string(256).is_none()
        {
            return false;
        }
        let Some(has_creature) = reader.read_bool() else {
            return false;
        };
        if looks_like_ee_identity(&reader) && skip_ee_identity(&mut reader).is_none() {
            return false;
        }
        if has_creature {
            if reader.read_u32().is_none()
                || reader.read_locstring().is_none()
                || reader.read_locstring().is_none()
            {
                return false;
            }
            let Some(portrait_id) = reader.read_u16() else {
                return false;
            };
            if portrait_id >= 0xFFFE && reader.read_resref16().is_none() {
                return false;
            }
        }
    }

    let consumed_fragment_bits = reader.fragment_cursor * 8 + usize::from(reader.fragment_bit);
    let consumed_fragment_bytes = reader.fragment_cursor + usize::from(reader.fragment_bit != 0);
    if reader.cursor != read_size
        || consumed_fragment_bytes != fragment_size
        || consumed_fragment_bits < 3
        || reader.final_fragment_bits != (consumed_fragment_bits % 8) as u8
    {
        return false;
    }

    result.entry_count = entry_count;
    true
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

    fn read_locstring(&mut self) -> Option<()> {
        let custom_tlk = self.read_bool()?;
        if custom_tlk {
            self.read_bits(1)?;
            self.read_u32()?;
        } else {
            self.read_string(4096)?;
        }
        Some(())
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
        && identity_length <= 256
        && identity_length as usize <= reader.read_size - reader.cursor - 5
}

fn skip_ee_identity(reader: &mut Reader<'_>) -> Option<()> {
    reader.read_u8()?;
    reader.read_string(256)
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
    if bits.len() < 3 {
        bits.resize(3, 0);
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
