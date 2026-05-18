//! Shared runtime resource-profile discovery for semantic translators.
//!
//! Resource-table helpers such as `baseitems.2da` and `genericdoors.2da` must
//! follow the same module resource context that the legacy server declares.
//! In live sessions that comes from `Module_Info`; in offline fixtures and
//! harness smoke tests it can come from the NWSync build env file.  Keep this
//! lookup in one small module so packet-family translators do not grow their
//! own working-directory assumptions.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const DEFAULT_ASSET_ENV_FILE: &str = "hg-bridge-nwsync.env";

/// Returns the configured asset profile name, if one has been proven by the
/// environment or by the bridge's generated asset env file.
pub(crate) fn configured_asset_profile_name() -> Option<String> {
    for key in ["NWN_BRIDGE_ASSET_PROFILE", "HG_BRIDGE_ASSET_PROFILE"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }

    read_asset_env_file_value("HG_BRIDGE_ASSET_PROFILE")
}

fn read_asset_env_file_value(key: &str) -> Option<String> {
    asset_env_file_candidates()
        .into_iter()
        .find_map(|path| read_env_file_value(&path, key))
}

fn asset_env_file_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    for key in [
        "NWN_BRIDGE_ASSET_ENV",
        "NWN_BRIDGE_NWSYNC_ENV",
        "HG_BRIDGE_NWSYNC_ENV",
    ] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                candidates.push(PathBuf::from(value));
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(DEFAULT_ASSET_ENV_FILE));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join(DEFAULT_ASSET_ENV_FILE));
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest_dir.join(DEFAULT_ASSET_ENV_FILE));
    if let Some(parent) = manifest_dir.parent() {
        candidates.push(parent.join(DEFAULT_ASSET_ENV_FILE));
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|path| path.is_file())
        .filter(|path| seen.insert(path_key(path)))
        .collect()
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

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}
