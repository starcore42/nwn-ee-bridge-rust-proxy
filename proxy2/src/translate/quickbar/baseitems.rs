use super::*;
use std::{
    collections::HashSet,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

// baseitems.2da support used by the quickbar item-object parser.
//
// The parser needs the model-type column to know the exact legacy appearance
// width. The game resolves this from the active module resource stack: an
// explicit loose override may win, then module/profile HAKs, then stock 1.69
// resources. That order matters for HG/CEP custom base-item ids such as 374.
// If no resource-backed table is available, fall back to a conservative built-in
// table for common base item ids so spells/general slots still translate while
// opaque item bodies are blanked instead of leaked.

const BASEITEMS_RESTYPE_2DA: u16 = 2017;
const MAX_ERF_KEY_COUNT: u32 = 250_000;
const MAX_BASEITEMS_2DA_BYTES: u32 = 8 * 1024 * 1024;

pub(super) fn quickbar_base_item_model_types() -> Option<&'static [i8]> {
    QUICKBAR_BASE_ITEM_MODEL_TYPES
        .get_or_init(load_quickbar_base_item_model_types)
        .as_deref()
}

fn load_quickbar_base_item_model_types() -> Option<Vec<i8>> {
    if let Some(path) = explicit_baseitems_2da_path() {
        if let Some(parsed) = load_direct_baseitems_2da(&path) {
            return Some(parsed);
        }
    }

    if let Some((path, parsed)) = load_baseitems_model_types_from_haks() {
        tracing::info!(
            path = %path.display(),
            rows = parsed.len(),
            "loaded HAK baseitems.2da model types for quickbar translation"
        );
        return Some(parsed);
    }

    for path in direct_baseitems_2da_candidates() {
        if let Some(parsed) = load_direct_baseitems_2da(&path) {
            return Some(parsed);
        }
    }

    tracing::warn!(
        "baseitems.2da not found as an extracted file; using conservative built-in quickbar model-type fallback"
    );
    Some(fallback_baseitems_model_types())
}

fn explicit_baseitems_2da_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NWN_BRIDGE_BASEITEMS_2DA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "NWN_BRIDGE_BASEITEMS_2DA was set but does not point to a readable file"
        );
    }
    None
}

fn direct_baseitems_2da_candidates() -> Vec<PathBuf> {
    [
        PathBuf::from(BASEITEMS_2DA_NAME),
        PathBuf::from(HG_REQUIRED_FILES_DIR).join(BASEITEMS_2DA_NAME),
        PathBuf::from("assets").join(BASEITEMS_2DA_NAME),
        PathBuf::from("fixtures").join(BASEITEMS_2DA_NAME),
        PathBuf::from(r"C:\nwnbridge")
            .join("assets")
            .join("staged")
            .join("higher-ground")
            .join(BASEITEMS_2DA_NAME),
        PathBuf::from("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(BASEITEMS_2DA_NAME),
    ]
    .into_iter()
    .filter(|path| path.is_file())
    .collect()
}

fn load_direct_baseitems_2da(path: &Path) -> Option<Vec<i8>> {
    match fs::read_to_string(path) {
        Ok(text) => {
            if let Some(parsed) = parse_baseitems_model_types_2da(&text) {
                tracing::info!(
                    path = %path.display(),
                    rows = parsed.len(),
                    "loaded direct baseitems.2da model types for quickbar translation"
                );
                return Some(parsed);
            }
            tracing::warn!(
                path = %path.display(),
                "baseitems.2da found but model-type column could not be parsed"
            );
        }
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to read baseitems.2da for quickbar translation"
            );
        }
    }
    None
}

fn load_baseitems_model_types_from_haks() -> Option<(PathBuf, Vec<i8>)> {
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
            if let Some(parsed) = read_baseitems_model_types_from_hak(&path) {
                return Some((path, parsed));
            }
        }
    }

    for dir in hak_dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut haks = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("hak"))
            })
            .collect::<Vec<_>>();
        haks.sort_by(|a, b| {
            a.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .cmp(
                    b.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                )
        });
        for path in haks {
            if !tried.insert(path_key(&path)) {
                continue;
            }
            if let Some(parsed) = read_baseitems_model_types_from_hak(&path) {
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

    let profile_name = configured_asset_profile_name().unwrap_or_else(|| "generic-169".to_owned());
    crate::translate::profiles::module_resources_profile(&profile_name)
        .hak_order_top_first
        .iter()
        .map(|hak| (*hak).to_owned())
        .collect()
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

fn read_baseitems_model_types_from_hak(path: &Path) -> Option<Vec<i8>> {
    let bytes = read_erf_resource(path, "baseitems", BASEITEMS_RESTYPE_2DA)?;
    let text = String::from_utf8_lossy(&bytes);
    parse_baseitems_model_types_2da(&text)
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
                    "HAK contains duplicate resource entries; refusing ambiguous quickbar 2DA source"
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
    if resource_size > MAX_BASEITEMS_2DA_BYTES {
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

fn parse_baseitems_model_types_2da(text: &str) -> Option<Vec<i8>> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"));
    let _version = lines.next()?;
    let header = lines.next()?;
    let columns: Vec<&str> = header.split_whitespace().collect();
    let model_type_column = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("ModelType"))?;
    let mut model_types = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() <= model_type_column + 1 {
            continue;
        }
        let row = fields[0].parse::<usize>().ok()?;
        if model_types.len() <= row {
            model_types.resize(row + 1, 0);
        }
        let value = fields[model_type_column + 1].parse::<i8>().unwrap_or(0);
        model_types[row] = value;
    }
    if model_types.is_empty() {
        None
    } else {
        Some(model_types)
    }
}

fn fallback_baseitems_model_types() -> Vec<i8> {
    let mut model_types = vec![0i8; 512];
    // The armor row uses the extended armor appearance layout in both Diamond
    // and EE. This row is critical because armor quickbar entries otherwise
    // look like malformed item bodies.
    if let Some(slot) = model_types.get_mut(usize::try_from(NWN_BASE_ITEM_ARMOR).unwrap_or(16)) {
        *slot = 3;
    }
    model_types
}
