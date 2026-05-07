//! EE `ServerStatus_ModuleRunning` module-resource rewrite.
//!
//! This module answers one narrow question:
//! "Given a verified legacy/EE server-status module-running packet, what exact
//! EE module-resource payload should be emitted so the client mounts the HG
//! HAK list after the legacy Module_Info hak block has been stripped?"
//!
//! Decompile and mature-proxy anchors:
//! - EE `CNWCMessage::HandleServerToPlayerServerStatus` reads a leading status
//!   `CExoString` for high-level `0x01/0x03`, then calls
//!   `CNWCModule::LoadModuleResources`.
//! - `CNWCModule::LoadModuleResources` consumes a fragment BOOL for optional
//!   NWSync advertisement data, then module resource name/description strings,
//!   a HAK count byte, and fixed 16-byte HAK resrefs.
//! - The legacy Diamond `Module_Info` HAK block is not part of EE's
//!   `LoadModule` stream, so `translate::module` removes it. This packet is the
//!   structured EE replacement for the same resource information.

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const SERVER_STATUS_MAJOR: u8 = 0x01;
const MODULE_RUNNING_MINOR: u8 = 0x03;
const RESREF_BYTES: usize = 16;
const MAX_SERVER_STATUS_STRING: usize = 4096;
const MAX_MODULE_RESOURCES_PAYLOAD: usize = 4096;

use super::profiles::{self, ModuleResourceProfile};

#[derive(Debug, Clone)]
pub struct ModuleResourcesRewriteSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub status_module_name: String,
    pub profile_name: &'static str,
    pub hak_count: usize,
    pub nwsync_advertised: bool,
}

struct ModuleResourceWriter {
    read_buffer: Vec<u8>,
    fragment_bits: Vec<bool>,
}

impl ModuleResourceWriter {
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

    fn write_string(&mut self, value: &str) -> Option<()> {
        let length = u32::try_from(value.len()).ok()?;
        self.read_buffer.extend_from_slice(&length.to_le_bytes());
        self.read_buffer.extend_from_slice(value.as_bytes());
        Some(())
    }

    fn write_fixed_resref16(&mut self, value: &str) -> Option<()> {
        if value.is_empty() || value.len() > RESREF_BYTES || !value.bytes().all(is_resref_char) {
            return None;
        }
        let mut bytes = [0u8; RESREF_BYTES];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        self.read_buffer.extend_from_slice(&bytes);
        Some(())
    }

    fn finish(mut self) -> Option<(Vec<u8>, Vec<u8>, u32)> {
        let declared = u32::try_from(self.read_buffer.len().checked_add(HIGH_LEVEL_HEADER_BYTES)?).ok()?;
        self.read_buffer[..CNW_LENGTH_BYTES].copy_from_slice(&declared.to_le_bytes());
        let fragments = pack_cnw_msb_bits(self.fragment_bits)?;
        Some((self.read_buffer, fragments, declared))
    }
}

pub fn rewrite_server_status_module_resources_payload(
    payload: &mut Vec<u8>,
) -> Option<ModuleResourcesRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || !is_high_level_envelope(payload[0])
        || payload[1] != SERVER_STATUS_MAJOR
        || payload[2] != MODULE_RUNNING_MINOR
    {
        return None;
    }

    let profile = profiles::module_resources_profile("higher-ground");
    rewrite_payload_for_profile(payload, profile)
}

fn rewrite_payload_for_profile(
    payload: &mut Vec<u8>,
    profile: ModuleResourceProfile,
) -> Option<ModuleResourcesRewriteSummary> {
    let old_declared = read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES)?;
    let declared = usize::try_from(old_declared).ok()?;
    if declared < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES || declared > payload.len() {
        return None;
    }

    let read_size = declared - HIGH_LEVEL_HEADER_BYTES;
    let read_buffer = payload.get(HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + read_size)?;
    let status_module_name = read_leading_string_from_read_buffer(read_buffer).unwrap_or_default();

    let mut writer = ModuleResourceWriter::new();
    writer.write_string(&status_module_name)?;

    // The local EE user folder already has the HAKs installed by the harness.
    // Advertise no NWSync repository here and provide the explicit HAK list.
    writer.write_bit(profile.advertise_nwsync);

    writer.write_string("")?;
    writer.write_string("")?;
    writer.write_byte(u8::try_from(profile.hak_order_top_first.len()).ok()?);
    for hak in profile.hak_order_top_first {
        writer.write_fixed_resref16(hak)?;
    }

    let (read_buffer, fragments, new_declared) = writer.finish()?;
    let new_len = HIGH_LEVEL_HEADER_BYTES
        .checked_add(read_buffer.len())?
        .checked_add(fragments.len())?;
    if new_len > MAX_MODULE_RESOURCES_PAYLOAD {
        return None;
    }

    let mut rewritten = Vec::with_capacity(new_len);
    rewritten.extend_from_slice(&payload[..HIGH_LEVEL_HEADER_BYTES]);
    rewritten.extend_from_slice(&read_buffer);
    rewritten.extend_from_slice(&fragments);

    let summary = ModuleResourcesRewriteSummary {
        old_declared,
        new_declared,
        old_payload_length: payload.len(),
        new_payload_length: rewritten.len(),
        status_module_name,
        profile_name: profile.name,
        hak_count: profile.hak_order_top_first.len(),
        nwsync_advertised: profile.advertise_nwsync,
    };
    *payload = rewritten;
    Some(summary)
}

fn read_leading_string_from_read_buffer(read_buffer: &[u8]) -> Option<String> {
    if read_buffer.len() < CNW_LENGTH_BYTES {
        return None;
    }
    let length = usize::try_from(read_u32_le(read_buffer, CNW_LENGTH_BYTES)?).ok()?;
    if length > MAX_SERVER_STATUS_STRING || CNW_LENGTH_BYTES + 4 + length > read_buffer.len() {
        return None;
    }
    Some(String::from_utf8_lossy(&read_buffer[CNW_LENGTH_BYTES + 4..CNW_LENGTH_BYTES + 4 + length]).to_string())
}

fn pack_cnw_msb_bits(mut bits: Vec<bool>) -> Option<Vec<u8>> {
    if bits.len() < 3 {
        return None;
    }
    let final_fragment_bits = bits.len() % 8;
    bits[0] = (final_fragment_bits & 0x04) != 0;
    bits[1] = (final_fragment_bits & 0x02) != 0;
    bits[2] = (final_fragment_bits & 0x01) != 0;

    let mut packed = vec![0u8; bits.len().div_ceil(8)];
    for (bit_index, bit) in bits.into_iter().enumerate() {
        if bit {
            packed[bit_index / 8] |= 0x80 >> (bit_index % 8);
        }
    }
    Some(packed)
}

fn is_high_level_envelope(byte: u8) -> bool {
    byte == b'P' || byte == 0x70
}

fn is_resref_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
