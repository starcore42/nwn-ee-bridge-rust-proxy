//! `visualeffects.2da` row metadata for live-object visual-effect updates.
//!
//! Diamond `sub_44ED20` and EE `sub_1407B1F00` both resolve the effect row and
//! branch on `Type_FD`: rows with `P` or `B` own a DWORD object id plus one
//! BYTE before EE's object visual-transform map. The packet readers should use
//! that table state when it is available instead of guessing row boundaries
//! from byte shape.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

#[cfg(not(test))]
use std::{
    collections::HashSet,
    io::{Read, Seek, SeekFrom},
};

#[cfg(not(test))]
const VISUALEFFECTS_2DA_NAME: &str = "visualeffects.2da";
#[cfg(not(test))]
const VISUALEFFECTS_RESREF: &str = "visualeffects";
#[cfg(not(test))]
const RESTYPE_2DA: u16 = 2017;
#[cfg(not(test))]
const HG_REQUIRED_FILES_DIR: &str = "HG REQUIRED FILES";
#[cfg(not(test))]
const MAX_ERF_KEY_COUNT: u32 = 250_000;
#[cfg(not(test))]
const MAX_VISUALEFFECTS_2DA_BYTES: u32 = 8 * 1024 * 1024;
const TARGET_PAYLOAD_BYTES: usize = 5;

static OBSERVED_HAK_ORDER_TOP_FIRST: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static VISUAL_EFFECT_TARGET_PAYLOAD_BYTES: OnceLock<RwLock<VisualEffectTargetPayloadCache>> =
    OnceLock::new();

#[derive(Debug, Default)]
struct VisualEffectTargetPayloadCache {
    loaded: bool,
    table: Option<Vec<Option<usize>>>,
}

pub(crate) fn observe_hak_order_top_first(hak_order_top_first: &[String]) {
    let observed = OBSERVED_HAK_ORDER_TOP_FIRST.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(mut observed) = observed.write() {
        // A zero-HAK Module_Info declaration is real server resource state.
        // Keep it so direct base-game visualeffects.2da is only considered
        // authoritative after the server has proved there is no HAK override.
        *observed = hak_order_top_first.to_vec();
    }

    if let Some(cache) = VISUAL_EFFECT_TARGET_PAYLOAD_BYTES.get() {
        if let Ok(mut cache) = cache.write() {
            *cache = VisualEffectTargetPayloadCache::default();
        }
    }
}

pub(super) fn loaded_visual_effect_target_payload_bytes() -> Option<Vec<Option<usize>>> {
    let cache = VISUAL_EFFECT_TARGET_PAYLOAD_BYTES
        .get_or_init(|| RwLock::new(VisualEffectTargetPayloadCache::default()));

    if let Ok(cache_read) = cache.read() {
        if cache_read.loaded {
            return cache_read.table.clone();
        }
    }

    let loaded_table = load_visual_effect_target_payload_bytes();
    if let Ok(mut cache_write) = cache.write() {
        cache_write.loaded = true;
        cache_write.table = loaded_table;
        return cache_write.table.clone();
    }

    None
}

pub(super) fn target_payload_bytes_for_loaded_row(
    rows: &[Option<usize>],
    row: u16,
) -> Option<usize> {
    rows.get(usize::from(row)).copied().flatten()
}

fn load_visual_effect_target_payload_bytes() -> Option<Vec<Option<usize>>> {
    if let Some(path) = explicit_visualeffects_2da_path() {
        if let Some(parsed) = load_direct_visualeffects_2da(&path) {
            return Some(parsed);
        }
    }

    #[cfg(test)]
    {
        // Fixture-free packet tests use synthetic visual-effect rows to exercise
        // the conservative no-table fallback. Do not let a developer's local
        // Diamond/EE install silently change those test semantics.
        return None;
    }

    #[cfg(not(test))]
    {
        if let Some((path, parsed)) = load_visualeffects_from_haks() {
            tracing::info!(
                path = %path.display(),
                rows = parsed.len(),
                "loaded active visualeffects.2da row target policy for visual-effect packet validation"
            );
            return Some(parsed);
        }

        if direct_base_visualeffects_2da_fallback_is_authoritative() {
            for path in direct_visualeffects_2da_candidates() {
                if let Some(parsed) = load_direct_visualeffects_2da(&path) {
                    return Some(parsed);
                }
            }
        } else {
            tracing::info!(
                "visualeffects.2da direct base-game fallback withheld until Module_Info proves an empty HAK stack"
            );
        }

        tracing::warn!(
            "visualeffects.2da not found in observed module HAK stack or direct base-game candidates; visual-effect target payload boundaries remain conservative"
        );
        None
    }
}

fn explicit_visualeffects_2da_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NWN_BRIDGE_VISUALEFFECTS_2DA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "NWN_BRIDGE_VISUALEFFECTS_2DA was set but does not point to a readable file"
        );
    }
    None
}

fn load_direct_visualeffects_2da(path: &Path) -> Option<Vec<Option<usize>>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to read visualeffects.2da for visual-effect target payload validation"
            );
            return None;
        }
    };
    let parsed = parse_visual_effect_target_payload_bytes_2da(&text);
    if parsed.is_none() {
        tracing::warn!(
            path = %path.display(),
            "visualeffects.2da found but Type_FD column could not be parsed"
        );
    }
    parsed
}

#[cfg(not(test))]
fn direct_base_visualeffects_2da_fallback_is_authoritative() -> bool {
    // Base-game 2DA files are authoritative only after Module_Info has proved
    // that the server mounted no HAKs. Before that, a base Type_FD row may be
    // wrong for the active module and can shift every following target/map byte.
    observed_hak_order_top_first().is_some_and(|order| order.is_empty())
}

#[cfg(not(test))]
fn direct_visualeffects_2da_candidates() -> Vec<PathBuf> {
    [
        PathBuf::from(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("..").join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from(HG_REQUIRED_FILES_DIR).join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("..")
            .join(HG_REQUIRED_FILES_DIR)
            .join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("assets").join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("..")
            .join("assets")
            .join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("fixtures").join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("..")
            .join("fixtures")
            .join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(VISUALEFFECTS_2DA_NAME),
        PathBuf::from("..")
            .join("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(VISUALEFFECTS_2DA_NAME),
    ]
    .into_iter()
    .filter(|path| path.is_file())
    .collect()
}

#[cfg(not(test))]
fn load_visualeffects_from_haks() -> Option<(PathBuf, Vec<Option<usize>>)> {
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
            if let Some(parsed) = read_visualeffects_target_payload_bytes_from_hak(&path) {
                return Some((path, parsed));
            }
        }
    }

    None
}

#[cfg(not(test))]
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

#[cfg(not(test))]
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

#[cfg(not(test))]
fn observed_hak_order_top_first() -> Option<Vec<String>> {
    let observed = OBSERVED_HAK_ORDER_TOP_FIRST.get()?;
    let observed = observed.read().ok()?;
    Some(observed.clone())
}

#[cfg(not(test))]
fn split_env_list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([';', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
}

#[cfg(not(test))]
fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

#[cfg(not(test))]
fn read_visualeffects_target_payload_bytes_from_hak(path: &Path) -> Option<Vec<Option<usize>>> {
    let bytes = read_erf_resource(path, VISUALEFFECTS_RESREF, RESTYPE_2DA)?;
    let text = String::from_utf8_lossy(&bytes);
    parse_visual_effect_target_payload_bytes_2da(&text)
}

#[cfg(not(test))]
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
                    "HAK contains duplicate resource entries; refusing ambiguous visualeffects.2da source"
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
    if resource_size > MAX_VISUALEFFECTS_2DA_BYTES {
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

#[cfg(not(test))]
fn read_file_u16(file: &mut fs::File) -> Option<u16> {
    let mut bytes = [0u8; 2];
    file.read_exact(&mut bytes).ok()?;
    Some(u16::from_le_bytes(bytes))
}

#[cfg(not(test))]
fn read_file_u32(file: &mut fs::File) -> Option<u32> {
    let mut bytes = [0u8; 4];
    file.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn parse_visual_effect_target_payload_bytes_2da(text: &str) -> Option<Vec<Option<usize>>> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"));
    let _version = lines.next()?;
    let header = lines.next()?;
    let columns: Vec<&str> = header.split_whitespace().collect();
    let type_fd_column = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("Type_FD"))?;

    let mut rows = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() <= type_fd_column + 1 {
            continue;
        }
        let row = fields[0].parse::<usize>().ok()?;
        if rows.len() <= row {
            rows.resize(row + 1, None);
        }
        if rows[row].is_some() {
            // Row policy is used as packet-boundary proof. A duplicate numeric
            // row id means the active 2DA source is ambiguous, even if both
            // rows happen to spell the same Type_FD value.
            return None;
        }
        let type_fd = fields[type_fd_column + 1];
        let target_bytes = if type_fd.eq_ignore_ascii_case("P") || type_fd.eq_ignore_ascii_case("B")
        {
            TARGET_PAYLOAD_BYTES
        } else {
            0
        };
        rows[row] = Some(target_bytes);
    }

    if rows.is_empty() { None } else { Some(rows) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI_VISUALEFFECTS_2DA: &str = r#"
2DA V2.0

      Label          Type_FD
0     Ordinary       ****
18    BeamToObject   B
19    Projectile     P
20    CreatureOnly   C
"#;

    #[test]
    fn parses_target_payload_policy_from_type_fd() {
        let rows = parse_visual_effect_target_payload_bytes_2da(MINI_VISUALEFFECTS_2DA)
            .expect("mini visualeffects.2da should parse");

        assert_eq!(target_payload_bytes_for_loaded_row(&rows, 0), Some(0));
        assert_eq!(
            target_payload_bytes_for_loaded_row(&rows, 18),
            Some(TARGET_PAYLOAD_BYTES),
            "B rows own the object target payload"
        );
        assert_eq!(
            target_payload_bytes_for_loaded_row(&rows, 19),
            Some(TARGET_PAYLOAD_BYTES),
            "P rows own the object target payload"
        );
        assert_eq!(target_payload_bytes_for_loaded_row(&rows, 20), Some(0));
        assert_eq!(
            target_payload_bytes_for_loaded_row(&rows, 21),
            None,
            "loaded tables do not silently prove absent rows"
        );
    }

    #[test]
    fn duplicate_visualeffects_rows_are_not_boundary_proof() {
        let duplicate = r#"
2DA V2.0

      Label          Type_FD
18    BeamToObject   B
18    BeamNoTarget   ****
"#;

        assert!(
            parse_visual_effect_target_payload_bytes_2da(duplicate).is_none(),
            "duplicate visualeffects.2da row ids cannot prove target-payload bit boundaries"
        );
    }
}
