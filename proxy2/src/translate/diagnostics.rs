//! Diagnostic dump-path helpers for strict packet development.
//!
//! Packet-family work depends on preserving the exact bytes that strict mode
//! refused to emit. The harness sets `NWN_BRIDGE_QUARANTINE_DIR` for those
//! captures; older local debugging used `HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR`.
//! Keep both names supported, but prefer the quarantine directory so harness
//! runs and offline fixture promotion look in one predictable place.

use std::{env, path::PathBuf, sync::OnceLock};

const QUARANTINE_DIR_ENV: &str = "NWN_BRIDGE_QUARANTINE_DIR";
const LEGACY_DUMP_DIR_ENV: &str = "HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR";

static DEFAULT_DUMP_DIR: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_default_diagnostic_dump_dir(path: PathBuf) {
    let _ = DEFAULT_DUMP_DIR.set(path);
}

pub(crate) fn diagnostic_dump_dir() -> Option<PathBuf> {
    env_value(QUARANTINE_DIR_ENV)
        .or_else(|| env_value(LEGACY_DUMP_DIR_ENV))
        .map(PathBuf::from)
        .or_else(|| DEFAULT_DUMP_DIR.get().cloned())
}

fn env_value(name: &str) -> Option<String> {
    let value = env::var(name).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
