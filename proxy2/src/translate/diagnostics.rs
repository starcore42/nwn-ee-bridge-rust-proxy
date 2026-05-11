//! Diagnostic dump-path helpers for strict packet development.
//!
//! Packet-family work depends on preserving the exact bytes that strict mode
//! refused to emit. The harness sets `NWN_BRIDGE_QUARANTINE_DIR` for those
//! captures; older local debugging used `HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR`.
//! Keep both names supported, but prefer the quarantine directory so harness
//! runs and offline fixture promotion look in one predictable place.

use std::{env, path::PathBuf};

const QUARANTINE_DIR_ENV: &str = "NWN_BRIDGE_QUARANTINE_DIR";
const LEGACY_DUMP_DIR_ENV: &str = "HGBRIDGE_PROXY2_DUMP_MODULE_INFO_DIR";

pub(crate) fn diagnostic_dump_dir() -> Option<PathBuf> {
    env_value(QUARANTINE_DIR_ENV)
        .or_else(|| env_value(LEGACY_DUMP_DIR_ENV))
        .map(PathBuf::from)
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
