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
//!   NWSync advertisement data, then the module custom-TLK string, a secondary
//!   module-resource string, a HAK count byte, and fixed 16-byte HAK resrefs.
//!   The first string is copied to the client module at `+0x28` and later used
//!   by `CTlkTable::OpenFileAlternate`; it must be the server-declared
//!   `Mod_CustomTlk` value, not an empty placeholder.
//! - The legacy Diamond `Module_Info` HAK block is not part of EE's
//!   `LoadModule` stream, so `translate::module` removes it. This packet is the
//!   structured EE replacement for the same resource information.
//! - Do not synthesize a server-specific HAK order here. Diamond's client reads
//!   the module declaration byte count, then that many fixed 16-byte HAK
//!   resrefs in order. The proxy records that exact server-provided order from
//!   `Module_Info` and re-emits it in EE's `LoadModuleResources` shape.

const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const SERVER_STATUS_MAJOR: u8 = 0x01;
const MODULE_RUNNING_MINOR: u8 = 0x03;
const RESREF_BYTES: usize = 16;
const MAX_SERVER_STATUS_STRING: usize = 4096;
const MAX_MODULE_RESOURCES_PAYLOAD: usize = 4096;
const MAX_OBSERVED_HAK_COUNT: usize = 255;

use crate::nwsync::Advertisement;

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ModuleResourcesRewriteSummary {
    pub old_declared: u32,
    pub new_declared: u32,
    pub old_payload_length: usize,
    pub new_payload_length: usize,
    pub status_module_name: String,
    pub profile_name: String,
    pub resource_source: &'static str,
    pub custom_tlk: Option<String>,
    pub hak_count: usize,
    pub nwsync_advertised: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleResourceRuntime {
    profile_name: String,
    nwsync_advertisement: Option<Advertisement>,
    observed_declaration: Arc<Mutex<Option<ObservedModuleResourceDeclaration>>>,
}

#[derive(Debug, Clone)]
struct ObservedModuleResourceDeclaration {
    hak_order_top_first: Vec<String>,
    custom_tlk: Option<String>,
}

impl ModuleResourceRuntime {
    pub fn new(profile_name: String, nwsync_advertisement: Option<Advertisement>) -> Self {
        Self {
            profile_name,
            nwsync_advertisement,
            observed_declaration: Arc::new(Mutex::new(None)),
        }
    }

    /// Build a fresh runtime for one client session.
    ///
    /// The HAK/TLK declaration is observed from that session's Diamond
    /// `Module_Info` packet.  Cloned translator templates keep the configured
    /// NWSync advertisement, but must not share an observed module declaration
    /// across different connections.
    pub fn for_new_session(&self) -> Self {
        Self {
            profile_name: self.profile_name.clone(),
            nwsync_advertisement: self.nwsync_advertisement.clone(),
            observed_declaration: Arc::new(Mutex::new(None)),
        }
    }

    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    pub fn nwsync_advertisement(&self) -> Option<&Advertisement> {
        self.nwsync_advertisement.as_ref()
    }

    pub fn observe_legacy_module_info_resources(
        &self,
        hak_order_top_first: &[String],
        custom_tlk: Option<&str>,
    ) -> bool {
        if hak_order_top_first.len() > MAX_OBSERVED_HAK_COUNT
            || !hak_order_top_first
                .iter()
                .all(|hak| fixed_resref_value_is_valid(hak))
            || custom_tlk
                .is_some_and(|tlk| !tlk.trim().is_empty() && !fixed_resref_value_is_valid(tlk))
        {
            return false;
        }

        // Share the decompile-confirmed, server-provided HAK stack with
        // resource-table helpers such as `baseitems.2da` lookup. Those helpers
        // must follow the same observed Diamond/EE module order instead of an
        // HG fallback profile whenever live Module_Info has already proved the
        // active stack for this session.
        crate::translate::baseitems::observe_hak_order_top_first(hak_order_top_first);

        let Ok(mut observed) = self.observed_declaration.lock() else {
            return false;
        };
        *observed = Some(ObservedModuleResourceDeclaration {
            hak_order_top_first: hak_order_top_first.to_vec(),
            custom_tlk: custom_tlk.map(str::to_owned),
        });
        true
    }

    fn observed_module_resource_declaration(&self) -> Option<ObservedModuleResourceDeclaration> {
        self.observed_declaration.lock().ok()?.clone()
    }
}

impl Default for ModuleResourceRuntime {
    fn default() -> Self {
        Self {
            profile_name: "higher-ground".to_string(),
            nwsync_advertisement: None,
            observed_declaration: Arc::new(Mutex::new(None)),
        }
    }
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

    fn write_nwsync_advertisement(&mut self, advertisement: &Advertisement) -> Option<()> {
        self.write_string(advertisement.root_hash())?;
        self.write_byte(1);
        self.write_string(advertisement.url())?;
        // EE's `CNWCModule::LoadModuleResources` first calls
        // `CExoResMan::AddManifest(root)` and then iterates the explicit
        // manifest-advert vector, calling `AddManifest(extra)` for each entry
        // whose language should load. This is not the same semantic role as
        // BNXR's pre-module client-download advert list: every extra here must
        // already be a manifest the EE resource manager can mount as a key
        // table, and a required missing extra is the decompile-confirmed
        // `0x10BBE` failure path.
        let module_manifests = advertisement
            .module_manifests()
            .iter()
            .filter(|manifest| manifest.hash != advertisement.root_hash())
            .collect::<Vec<_>>();
        self.write_byte(u8::try_from(module_manifests.len()).ok()?);
        for manifest in module_manifests {
            self.write_string(&manifest.hash)?;
            self.write_byte(manifest.flags);
            self.write_byte(manifest.language);
        }
        Some(())
    }

    fn finish(mut self) -> Option<(Vec<u8>, Vec<u8>, u32)> {
        let declared = u32::try_from(
            self.read_buffer
                .len()
                .checked_add(HIGH_LEVEL_HEADER_BYTES)?,
        )
        .ok()?;
        self.read_buffer[..CNW_LENGTH_BYTES].copy_from_slice(&declared.to_le_bytes());
        let fragments = pack_cnw_msb_bits(self.fragment_bits)?;
        Some((self.read_buffer, fragments, declared))
    }
}

pub fn rewrite_server_status_module_resources_payload(
    payload: &mut Vec<u8>,
    runtime: &ModuleResourceRuntime,
) -> Option<ModuleResourcesRewriteSummary> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES
        || !is_high_level_envelope(payload[0])
        || payload[1] != SERVER_STATUS_MAJOR
        || payload[2] != MODULE_RUNNING_MINOR
    {
        return None;
    }

    let observed = runtime.observed_module_resource_declaration()?;
    rewrite_payload_for_observed_declaration(payload, runtime, observed)
}

fn rewrite_payload_for_observed_declaration(
    payload: &mut Vec<u8>,
    runtime: &ModuleResourceRuntime,
    observed: ObservedModuleResourceDeclaration,
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

    if let Some(advertisement) = runtime.nwsync_advertisement() {
        writer.write_bit(true);
        writer.write_nwsync_advertisement(advertisement)?;
    } else {
        writer.write_bit(false);
    }

    writer.write_string(observed.custom_tlk.as_deref().unwrap_or(""))?;
    // EE's writer emits a second resource string after the custom TLK and the
    // client passes it through `sub_1406ACA00`. Diamond's 1.69 Module_Info does
    // not carry an equivalent field in the decompile-backed resource block we
    // consume here, so keep it empty rather than inventing server-specific data.
    writer.write_string("")?;
    writer.write_byte(u8::try_from(observed.hak_order_top_first.len()).ok()?);
    for hak in &observed.hak_order_top_first {
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
        profile_name: runtime.profile_name().to_string(),
        resource_source: "observed-diamond-module-info",
        custom_tlk: observed.custom_tlk,
        hak_count: observed.hak_order_top_first.len(),
        nwsync_advertised: runtime.nwsync_advertisement().is_some(),
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
    Some(
        String::from_utf8_lossy(&read_buffer[CNW_LENGTH_BYTES + 4..CNW_LENGTH_BYTES + 4 + length])
            .to_string(),
    )
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

fn fixed_resref_value_is_valid(value: &str) -> bool {
    let value = value.trim().strip_suffix(".tlk").unwrap_or(value.trim());
    !value.is_empty() && value.len() <= RESREF_BYTES && value.bytes().all(is_resref_char)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_legacy_short_module_running_status_into_ee_resource_block() {
        // Quarantined HG capture shape:
        // P 01 03 + CNW declared length 0x0B, empty leading status string,
        // and one legacy fragment byte. EE keeps the same high-level family
        // but expects `CNWCModule::LoadModuleResources` data after the leading
        // status string, so this must be claimed by the module-resource
        // translator rather than allowed as an unowned raw status packet.
        let mut payload = vec![
            b'P', 0x01, 0x03, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x79,
        ];
        let runtime = ModuleResourceRuntime::default();
        assert!(runtime.observe_legacy_module_info_resources(
            &[
                "cep2_custom".to_string(),
                "cep2_top_v23".to_string(),
            ],
            Some("cep23_v1"),
        ));

        let summary = rewrite_server_status_module_resources_payload(&mut payload, &runtime)
            .expect("legacy module-running status should be rewritten");

        assert_eq!(summary.old_declared, 0x0B);
        assert_eq!(summary.status_module_name, "");
        assert_eq!(summary.profile_name, "higher-ground");
        assert_eq!(summary.resource_source, "observed-diamond-module-info");
        assert_eq!(summary.hak_count, 2);
        assert_eq!(summary.new_payload_length, payload.len());
        assert_eq!(&payload[..3], &[b'P', 0x01, 0x03]);
        assert_eq!(read_u32_le(&payload, 3), Some(summary.new_declared));
        assert!(summary.new_declared > summary.old_declared);
    }
}
