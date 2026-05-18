//! Active `placeables.2da` lookup for placeable-add compatibility checks.
//!
//! Diamond `CNWSMessage::AddPlaceableAppearanceToMessage` and EE
//! `CNWCMessage::HandleServerToPlayerPlaceableUpdate_Add` both resolve the
//! add-record appearance WORD through `placeables.2da` and load the row's
//! `ModelName` value. The bridge must not invent a model when the active
//! module resource stack has no row/model for that appearance. This module only
//! answers that resource-table question from the observed `Module_Info` HAK
//! order; live-object translation decides what to do with the proof.

use std::{
    collections::HashSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

const PLACEABLES_2DA_NAME: &str = "placeables.2da";
const PLACEABLES_RESREF: &str = "placeables";
const RESTYPE_2DA: u16 = 2017;
const MAX_ERF_KEY_COUNT: u32 = 250_000;
const MAX_PLACEABLES_2DA_BYTES: u32 = 8 * 1024 * 1024;

static OBSERVED_HAK_ORDER_TOP_FIRST: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static PLACEABLE_TABLE_CACHE: OnceLock<RwLock<PlaceableTableCache>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceableModelStatus {
    KnownModel,
    MissingOrEmpty,
    TableUnavailable,
}

#[derive(Debug, Default)]
struct PlaceableTableCache {
    loaded: bool,
    table: Option<Vec<Option<String>>>,
}

/// Records the server-provided module HAK stack.
///
/// This mirrors `baseitems.2da` lookup: the table must follow the exact
/// `Module_Info` declaration proven by the legacy server, not a baked-in HG
/// list. Observing a new stack invalidates the cached table so a future session
/// cannot accidentally reuse the previous module's resource context.
pub(crate) fn observe_hak_order_top_first(hak_order_top_first: &[String]) {
    let observed = OBSERVED_HAK_ORDER_TOP_FIRST.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(mut observed) = observed.write() {
        // `hak_count=0` from Module_Info is a real, server-authored resource
        // state. Treating it as "not observed yet" lets an ambient HG profile
        // from `hg-bridge-nwsync.env` leak into generic Diamond sessions and
        // falsely validate placeable rows the server never mounted.
        *observed = hak_order_top_first.to_vec();
    }

    if let Some(cache) = PLACEABLE_TABLE_CACHE.get() {
        if let Ok(mut cache) = cache.write() {
            *cache = PlaceableTableCache::default();
        }
    }
}

pub(crate) fn placeable_model_status(row: u32) -> PlaceableModelStatus {
    let Some(row) = usize::try_from(row).ok() else {
        return PlaceableModelStatus::MissingOrEmpty;
    };
    let cache = PLACEABLE_TABLE_CACHE.get_or_init(|| RwLock::new(PlaceableTableCache::default()));

    if let Ok(cache_read) = cache.read() {
        if cache_read.loaded {
            return status_from_table(cache_read.table.as_deref(), row);
        }
    }

    let loaded_table = load_placeables_model_names();
    if let Ok(mut cache_write) = cache.write() {
        cache_write.loaded = true;
        cache_write.table = loaded_table;
        return status_from_table(cache_write.table.as_deref(), row);
    }

    PlaceableModelStatus::TableUnavailable
}

fn status_from_table(table: Option<&[Option<String>]>, row: usize) -> PlaceableModelStatus {
    let Some(table) = table else {
        return PlaceableModelStatus::TableUnavailable;
    };
    match table.get(row).and_then(Option::as_deref) {
        Some(model) if !model.trim().is_empty() => PlaceableModelStatus::KnownModel,
        _ => PlaceableModelStatus::MissingOrEmpty,
    }
}

fn load_placeables_model_names() -> Option<Vec<Option<String>>> {
    if let Some(path) = explicit_placeables_2da_path() {
        if let Some(parsed) = load_direct_placeables_2da(&path) {
            return Some(parsed);
        }
    }

    if let Some((path, parsed)) = load_placeables_from_haks() {
        tracing::info!(
            path = %path.display(),
            rows = parsed.len(),
            "loaded active placeables.2da model names for placeable add validation"
        );
        return Some(parsed);
    }

    if direct_base_placeables_2da_fallback_is_authoritative() {
        for path in direct_placeables_2da_candidates() {
            if let Some(parsed) = load_direct_placeables_2da(&path) {
                tracing::info!(
                    path = %path.display(),
                    rows = parsed.len(),
                    "loaded direct placeables.2da model names for placeable add validation"
                );
                return Some(parsed);
            }
        }
    } else {
        tracing::info!(
            "placeables.2da direct base-game fallback withheld until Module_Info proves an empty HAK stack"
        );
    }

    tracing::warn!(
        "placeables.2da not found in observed module HAK stack or direct base-game candidates; placeable rows without model proof cannot be validated"
    );
    None
}

fn direct_base_placeables_2da_fallback_is_authoritative() -> bool {
    // Base-game 2DA files are authoritative only after the server-authored
    // Module_Info resource block has been observed and proved that no HAKs are
    // mounted. Before that point a capture/fixture may be from an HG-style
    // module whose active HAK stack overrides `placeables.2da`; treating a base
    // empty row as proof would wrongly suppress live placeable/sign add records.
    observed_hak_order_top_first().is_some_and(|order| order.is_empty())
}

fn explicit_placeables_2da_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NWN_BRIDGE_PLACEABLES_2DA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "NWN_BRIDGE_PLACEABLES_2DA was set but does not point to a readable file"
        );
    }
    None
}

fn direct_placeables_2da_candidates() -> Vec<PathBuf> {
    [
        PathBuf::from(PLACEABLES_2DA_NAME),
        PathBuf::from("assets").join(PLACEABLES_2DA_NAME),
        PathBuf::from("fixtures").join(PLACEABLES_2DA_NAME),
        PathBuf::from("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(PLACEABLES_2DA_NAME),
        PathBuf::from(r"C:\NWN")
            .join("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(PLACEABLES_2DA_NAME),
    ]
    .into_iter()
    .filter(|path| path.is_file())
    .collect()
}

fn load_direct_placeables_2da(path: &Path) -> Option<Vec<Option<String>>> {
    match fs::read_to_string(path) {
        Ok(text) => parse_placeables_model_names_2da(&text),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to read placeables.2da for placeable add validation"
            );
            None
        }
    }
}

fn load_placeables_from_haks() -> Option<(PathBuf, Vec<Option<String>>)> {
    let hak_dirs = hak_search_dirs();
    if hak_dirs.is_empty() {
        return None;
    }

    let mut tried = HashSet::new();
    for resref in configured_hak_order_top_first() {
        for dir in &hak_dirs {
            let path = dir.join(format!("{resref}.hak"));
            if !path.is_file() || !tried.insert(path_key(&path)) {
                continue;
            }
            if let Some(parsed) = read_placeables_model_names_from_hak(&path) {
                return Some((path, parsed));
            }
        }
    }

    None
}

fn hak_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(value) = std::env::var("NWN_BRIDGE_HAK_DIRS") {
        dirs.extend(split_env_list(&value).map(PathBuf::from));
    }
    for root_var in ["NWN_BRIDGE_ASSET_ROOT", "HG_BRIDGE_HG_ASSET_ROOT"] {
        if let Ok(root) = std::env::var(root_var) {
            let root = PathBuf::from(root);
            dirs.push(root.join("staged").join("higher-ground").join("hak"));
            dirs.push(root.join("hg-std").join("hak"));
            dirs.push(root.join("hg-gui").join("hak"));
            dirs.push(root.join("cep23").join("hak"));
            dirs.push(root.join("hak"));
        }
    }
    dirs.extend([
        PathBuf::from(r"C:\nwnbridge")
            .join("assets")
            .join("staged")
            .join("higher-ground")
            .join("hak"),
        PathBuf::from("assets")
            .join("staged")
            .join("higher-ground")
            .join("hak"),
        PathBuf::from("hg-bridge-assets")
            .join("staged")
            .join("higher-ground")
            .join("hak"),
    ]);

    let mut seen = HashSet::new();
    dirs.into_iter()
        .filter(|path| path.is_dir())
        .filter(|path| seen.insert(path_key(path)))
        .collect()
}

fn configured_hak_order_top_first() -> Vec<String> {
    if let Ok(value) = std::env::var("NWN_BRIDGE_HAK_ORDER_TOP_FIRST") {
        let order = split_env_list(&value)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !order.is_empty() {
            return order;
        }
    }

    if let Some(order) = observed_hak_order_top_first() {
        return order;
    }

    let profile_name = crate::translate::resource_config::configured_asset_profile_name()
        .unwrap_or_else(|| "generic-169".to_owned());
    crate::translate::profiles::module_resources_profile(&profile_name)
        .hak_order_top_first
        .iter()
        .map(|hak| (*hak).to_owned())
        .collect()
}

fn observed_hak_order_top_first() -> Option<Vec<String>> {
    let observed = OBSERVED_HAK_ORDER_TOP_FIRST.get()?;
    let observed = observed.read().ok()?;
    Some(observed.clone())
}

fn split_env_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([';', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

fn read_placeables_model_names_from_hak(path: &Path) -> Option<Vec<Option<String>>> {
    let bytes = read_erf_resource(path, PLACEABLES_RESREF, RESTYPE_2DA)?;
    let text = String::from_utf8_lossy(&bytes);
    parse_placeables_model_names_2da(&text)
}

fn read_erf_resource(path: &Path, wanted_resref: &str, wanted_type: u16) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    if !matches!(&magic, b"HAK " | b"ERF " | b"MOD " | b"NWM ") {
        return None;
    }
    file.read_exact(&mut magic).ok()?;
    if &magic != b"V1.0" {
        return None;
    }

    let _language_count = read_file_u32(&mut file)?;
    let _localized_string_size = read_file_u32(&mut file)?;
    let entry_count = read_file_u32(&mut file)?;
    let _localized_string_offset = read_file_u32(&mut file)?;
    let key_list_offset = u64::from(read_file_u32(&mut file)?);
    let resource_list_offset = u64::from(read_file_u32(&mut file)?);
    if entry_count > MAX_ERF_KEY_COUNT
        || key_list_offset >= file_len
        || resource_list_offset >= file_len
    {
        return None;
    }

    file.seek(SeekFrom::Start(key_list_offset)).ok()?;
    let mut match_resource_id = None;
    for _ in 0..entry_count {
        let mut resref_bytes = [0u8; 16];
        file.read_exact(&mut resref_bytes).ok()?;
        let resource_id = read_file_u32(&mut file)?;
        let resource_type = read_file_u16(&mut file)?;
        let _unused = read_file_u16(&mut file)?;
        let end = resref_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(resref_bytes.len());
        let resref = std::str::from_utf8(&resref_bytes[..end]).ok()?;
        if resref.eq_ignore_ascii_case(wanted_resref) && resource_type == wanted_type {
            if match_resource_id.is_some() {
                tracing::warn!(
                    path = %path.display(),
                    resref = wanted_resref,
                    "HAK contains duplicate resource entries; refusing ambiguous placeables.2da source"
                );
                return None;
            }
            match_resource_id = Some(resource_id);
        }
    }

    let resource_id = u64::from(match_resource_id?);
    let resource_entry_offset = resource_list_offset.checked_add(resource_id.checked_mul(8)?)?;
    if resource_entry_offset.checked_add(8)? > file_len {
        return None;
    }
    file.seek(SeekFrom::Start(resource_entry_offset)).ok()?;
    let resource_offset = u64::from(read_file_u32(&mut file)?);
    let resource_size = read_file_u32(&mut file)?;
    if resource_size > MAX_PLACEABLES_2DA_BYTES {
        return None;
    }
    let resource_size_u64 = u64::from(resource_size);
    if resource_offset.checked_add(resource_size_u64)? > file_len {
        return None;
    }
    file.seek(SeekFrom::Start(resource_offset)).ok()?;
    let mut bytes = vec![0u8; usize::try_from(resource_size).ok()?];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

fn read_file_u16(file: &mut fs::File) -> Option<u16> {
    let mut bytes = [0u8; 2];
    file.read_exact(&mut bytes).ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_file_u32(file: &mut fs::File) -> Option<u32> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

pub(crate) fn parse_placeables_model_names_2da(text: &str) -> Option<Vec<Option<String>>> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"));
    let _version = lines.next()?;
    let header = lines.next()?;
    let columns: Vec<&str> = header.split_whitespace().collect();
    let model_name_column = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("ModelName"))?;
    let mut model_names = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() <= model_name_column + 1 {
            continue;
        }
        let row = fields[0].parse::<usize>().ok()?;
        if model_names.len() <= row {
            model_names.resize(row + 1, None);
        }
        let value = fields[model_name_column + 1].trim_matches('"');
        if value != "****" && !value.is_empty() {
            model_names[row] = Some(value.to_owned());
        }
    }
    if model_names.is_empty() {
        None
    } else {
        Some(model_names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeables_parser_distinguishes_missing_model_rows() {
        let table = parse_placeables_model_names_2da(
            "2DA V2.0\nLabel StrRef ModelName BlockSight\n0 **** **** **** 0\n12 tnplaceable 111922 TN_Gplaceable_02 1\n",
        )
        .expect("placeables table should parse");

        assert_eq!(
            status_from_table(Some(&table), 12),
            PlaceableModelStatus::KnownModel
        );
        assert_eq!(
            status_from_table(Some(&table), 0),
            PlaceableModelStatus::MissingOrEmpty
        );
        assert_eq!(
            status_from_table(Some(&table), 5349),
            PlaceableModelStatus::MissingOrEmpty
        );
    }
}
