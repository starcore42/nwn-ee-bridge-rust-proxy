//! Active `genericdoors.2da` lookup for door-add compatibility checks.
//!
//! EE and Diamond door add readers both use the same decompiled branch:
//!
//! - if the first door DWORD is non-zero, read `DoorTypes.2da` column `Model`;
//! - otherwise read `GenericDoors.2da` column `ModelName` from the second DWORD.
//!
//! The bridge must not invent a replacement model when the active resource
//! stack has no row/model for that second value. This module only answers the
//! resource-table question from the observed module HAK order; live-object
//! translation decides what to do with that proof.

use std::{
    collections::HashSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

const GENERICDOORS_2DA_NAME: &str = "genericdoors.2da";
const GENERICDOORS_RESREF: &str = "genericdoors";
const RESTYPE_2DA: u16 = 2017;
const MAX_ERF_KEY_COUNT: u32 = 250_000;
const MAX_GENERICDOORS_2DA_BYTES: u32 = 8 * 1024 * 1024;

static OBSERVED_HAK_ORDER_TOP_FIRST: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static GENERIC_DOOR_TABLE_CACHE: OnceLock<RwLock<GenericDoorTableCache>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenericDoorModelStatus {
    KnownModel,
    MissingOrEmpty,
    TableUnavailable,
}

#[derive(Debug, Default)]
struct GenericDoorTableCache {
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
    if hak_order_top_first.is_empty() {
        return;
    }

    let observed = OBSERVED_HAK_ORDER_TOP_FIRST.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(mut observed) = observed.write() {
        *observed = hak_order_top_first.to_vec();
    }

    if let Some(cache) = GENERIC_DOOR_TABLE_CACHE.get() {
        if let Ok(mut cache) = cache.write() {
            *cache = GenericDoorTableCache::default();
        }
    }
}

pub(crate) fn generic_door_model_status(row: u32) -> GenericDoorModelStatus {
    let Some(row) = usize::try_from(row).ok() else {
        return GenericDoorModelStatus::MissingOrEmpty;
    };
    let cache = GENERIC_DOOR_TABLE_CACHE.get_or_init(|| RwLock::new(GenericDoorTableCache::default()));

    if let Ok(cache_read) = cache.read() {
        if cache_read.loaded {
            return status_from_table(cache_read.table.as_deref(), row);
        }
    }

    let loaded_table = load_genericdoors_model_names();
    if let Ok(mut cache_write) = cache.write() {
        cache_write.loaded = true;
        cache_write.table = loaded_table;
        return status_from_table(cache_write.table.as_deref(), row);
    }

    GenericDoorModelStatus::TableUnavailable
}

fn status_from_table(table: Option<&[Option<String>]>, row: usize) -> GenericDoorModelStatus {
    let Some(table) = table else {
        return GenericDoorModelStatus::TableUnavailable;
    };
    match table.get(row).and_then(Option::as_deref) {
        Some(model) if !model.trim().is_empty() => GenericDoorModelStatus::KnownModel,
        _ => GenericDoorModelStatus::MissingOrEmpty,
    }
}

fn load_genericdoors_model_names() -> Option<Vec<Option<String>>> {
    if let Some(path) = explicit_genericdoors_2da_path() {
        if let Some(parsed) = load_direct_genericdoors_2da(&path) {
            return Some(parsed);
        }
    }

    if let Some((path, parsed)) = load_genericdoors_from_haks() {
        tracing::info!(
            path = %path.display(),
            rows = parsed.len(),
            "loaded active genericdoors.2da model names for door add validation"
        );
        return Some(parsed);
    }

    tracing::warn!(
        "active genericdoors.2da not found in observed module HAK stack; unsupported generic door rows will not be suppressed without resource proof"
    );
    None
}

fn explicit_genericdoors_2da_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NWN_BRIDGE_GENERICDOORS_2DA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "NWN_BRIDGE_GENERICDOORS_2DA was set but does not point to a readable file"
        );
    }
    None
}

fn load_direct_genericdoors_2da(path: &Path) -> Option<Vec<Option<String>>> {
    match fs::read_to_string(path) {
        Ok(text) => parse_genericdoors_model_names_2da(&text),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to read genericdoors.2da for door add validation"
            );
            None
        }
    }
}

fn load_genericdoors_from_haks() -> Option<(PathBuf, Vec<Option<String>>)> {
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
            if let Some(parsed) = read_genericdoors_model_names_from_hak(&path) {
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

    let profile_name = configured_asset_profile_name().unwrap_or_else(|| "generic-169".to_owned());
    crate::translate::profiles::module_resources_profile(&profile_name)
        .hak_order_top_first
        .iter()
        .map(|hak| (*hak).to_owned())
        .collect()
}

fn observed_hak_order_top_first() -> Option<Vec<String>> {
    let observed = OBSERVED_HAK_ORDER_TOP_FIRST.get()?;
    let observed = observed.read().ok()?;
    if observed.is_empty() {
        None
    } else {
        Some(observed.clone())
    }
}

fn configured_asset_profile_name() -> Option<String> {
    for key in ["NWN_BRIDGE_ASSET_PROFILE", "HG_BRIDGE_ASSET_PROFILE"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    read_env_file_value(Path::new("hg-bridge-nwsync.env"), "HG_BRIDGE_ASSET_PROFILE")
}

fn read_env_file_value(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let (lhs, rhs) = line.split_once('=')?;
        if lhs.trim() == key {
            let value = rhs.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            }
        } else {
            None
        }
    })
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

fn read_genericdoors_model_names_from_hak(path: &Path) -> Option<Vec<Option<String>>> {
    let bytes = read_erf_resource(path, GENERICDOORS_RESREF, RESTYPE_2DA)?;
    let text = String::from_utf8_lossy(&bytes);
    parse_genericdoors_model_names_2da(&text)
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
                    "HAK contains duplicate resource entries; refusing ambiguous genericdoors.2da source"
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
    if resource_size > MAX_GENERICDOORS_2DA_BYTES {
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

fn parse_genericdoors_model_names_2da(text: &str) -> Option<Vec<Option<String>>> {
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
    fn genericdoors_parser_distinguishes_missing_model_rows() {
        let table = parse_genericdoors_model_names_2da(
            "2DA V2.0\nLabel StrRef ModelName BlockSight\n0 **** **** **** 0\n12 tndoor 111922 TN_GDOOR_02 1\n",
        )
        .expect("genericdoors table should parse");

        assert_eq!(status_from_table(Some(&table), 12), GenericDoorModelStatus::KnownModel);
        assert_eq!(
            status_from_table(Some(&table), 0),
            GenericDoorModelStatus::MissingOrEmpty
        );
        assert_eq!(
            status_from_table(Some(&table), 5349),
            GenericDoorModelStatus::MissingOrEmpty
        );
    }
}
