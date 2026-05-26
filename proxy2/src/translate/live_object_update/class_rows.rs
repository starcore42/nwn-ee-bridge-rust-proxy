//! Class-row metadata used by creature `U/5` identity rows.
//!
//! EE `CNWSMessage::HandleGameObjUpdate_UpdateObject` (`sub_140781E80`,
//! identity/classification branch at `loc_140785330`) reads each row as:
//!
//! - one fixed BYTE class id, then `CNWCCreatureStats::SetClass`
//! - one fixed BYTE class level, then `SetClassLevel`
//! - one optional BYTE when `g_pRules->class[class_id] + 0x4F8` is non-zero
//! - two optional BYTEs when `g_pRules->class[class_id] + 0x4F4` is non-zero,
//!   then `SetDomain1` and `SetDomain2`
//!
//! This module owns that rules-table policy. The creature update parser only
//! asks "how many optional bytes does this class row consume?" and remains a
//! bounded cursor simulator rather than a split-scoring heuristic.

use std::{fs, path::PathBuf, sync::OnceLock};

const CLASSES_2DA_NAME: &str = "classes.2da";
const HG_REQUIRED_FILES_DIR: &str = "HG REQUIRED FILES";

static CLASS_ROW_OPTIONAL_COUNTS: OnceLock<Option<Vec<Option<u8>>>> = OnceLock::new();

pub(super) fn creature_identity_row_optional_extra_byte_counts(
    class_id: u8,
) -> Option<&'static [u8]> {
    let loaded_rows = CLASS_ROW_OPTIONAL_COUNTS
        .get_or_init(load_class_row_optional_counts)
        .as_ref()
        .map(Vec::as_slice);

    class_row_optional_count(loaded_rows, class_id).and_then(optional_count_slice)
}

fn class_row_optional_count(loaded_rows: Option<&[Option<u8>]>, class_id: u8) -> Option<u8> {
    if let Some(rows) = loaded_rows {
        return rows.get(usize::from(class_id)).copied().flatten();
    }

    stock_legacy_class_row_optional_count(class_id)
}

fn stock_legacy_class_row_optional_count(class_id: u8) -> Option<u8> {
    match class_id {
        // Public stock 1.72 class rows that consume only the fixed class
        // id/level bytes. Server-specific merged tables can still be supplied
        // with `NWN_BRIDGE_CLASSES_2DA`.
        0 | 1 | 3..=9 | 11..=41 => Some(0),
        // Stock Cleric consumes the two domain bytes from the `+0x4F4` flag.
        2 => Some(2),
        // Stock Wizard consumes the one spell-option byte from `+0x4F8`.
        10 => Some(1),
        _ => None,
    }
}

fn optional_count_slice(count: u8) -> Option<&'static [u8]> {
    match count {
        0 => Some(&[0]),
        1 => Some(&[1]),
        2 => Some(&[2]),
        3 => Some(&[3]),
        _ => None,
    }
}

fn load_class_row_optional_counts() -> Option<Vec<Option<u8>>> {
    let path = find_direct_classes_2da()?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "failed to read classes.2da for creature identity row validation"
            );
            return None;
        }
    };
    let parsed = parse_class_row_optional_counts_2da(&text);
    if parsed.is_none() {
        tracing::warn!(
            path = %path.display(),
            "classes.2da found but class optional columns could not be parsed"
        );
    }
    parsed
}

fn find_direct_classes_2da() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NWN_BRIDGE_CLASSES_2DA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "NWN_BRIDGE_CLASSES_2DA was set but does not point to a readable file"
        );
    }

    let candidates = [
        PathBuf::from(CLASSES_2DA_NAME),
        PathBuf::from("..").join(CLASSES_2DA_NAME),
        PathBuf::from(HG_REQUIRED_FILES_DIR).join(CLASSES_2DA_NAME),
        PathBuf::from("..")
            .join(HG_REQUIRED_FILES_DIR)
            .join(CLASSES_2DA_NAME),
        PathBuf::from("assets").join(CLASSES_2DA_NAME),
        PathBuf::from("..").join("assets").join(CLASSES_2DA_NAME),
        PathBuf::from("fixtures").join(CLASSES_2DA_NAME),
        PathBuf::from("..").join("fixtures").join(CLASSES_2DA_NAME),
        PathBuf::from("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(CLASSES_2DA_NAME),
        PathBuf::from("..")
            .join("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(CLASSES_2DA_NAME),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

fn parse_class_row_optional_counts_2da(text: &str) -> Option<Vec<Option<u8>>> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"));
    let _version = lines.next()?;
    let header = lines.next()?;
    let columns: Vec<&str> = header.split_whitespace().collect();
    let label_column = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("Label"))?;
    let spell_opt_column = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("SpellOptTable"))?;

    let mut rows = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() <= label_column + 1 || fields.len() <= spell_opt_column + 1 {
            continue;
        }
        let row = fields[0].parse::<usize>().ok()?;
        if rows.len() <= row {
            rows.resize(row + 1, None);
        }

        let label = fields[label_column + 1];
        let has_spell_options = !is_2da_null(fields[spell_opt_column + 1]);

        // The public 1.72 `classes.2da` has no explicit "has domains" column,
        // but the decompile shows the `+0x4F4` bytes are exactly domain1 and
        // domain2. In the stock table this applies to Cleric. Server-specific
        // merged tables can be supplied via `NWN_BRIDGE_CLASSES_2DA`; otherwise
        // unknown/non-stock domain layouts are expected to quarantine.
        let has_domains = label.eq_ignore_ascii_case("Cleric");

        let count = u8::from(has_spell_options) + if has_domains { 2 } else { 0 };
        rows[row] = Some(count);
    }

    if rows.is_empty() { None } else { Some(rows) }
}

fn is_2da_null(value: &str) -> bool {
    value == "****" || value.eq_ignore_ascii_case("null")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI_CLASSES_2DA: &str = r#"
2DA V2.0

      Label      SpellOptTable
0     Fighter    ****
2     Cleric     ****
10    Wizard     cls_spopt_wiz
42    Custom     custom_spopt
"#;

    #[test]
    fn parses_stock_optional_class_row_byte_counts() {
        let rows = parse_class_row_optional_counts_2da(MINI_CLASSES_2DA)
            .expect("mini classes.2da should parse");

        assert_eq!(rows.get(0).copied().flatten(), Some(0));
        assert_eq!(
            rows.get(2).copied().flatten(),
            Some(2),
            "Cleric owns two domain bytes from the decompiled +0x4F4 rule"
        );
        assert_eq!(
            rows.get(10).copied().flatten(),
            Some(1),
            "Wizard owns one SpellOptTable byte from the decompiled +0x4F8 rule"
        );
        assert_eq!(
            rows.get(42).copied().flatten(),
            Some(1),
            "non-stock classes with a SpellOptTable own the same one-byte option field"
        );
    }

    #[test]
    fn loaded_classes_table_overrides_stock_fallback() {
        let mut rows = Vec::new();
        rows.resize(11, None);
        rows[10] = Some(0);

        assert_eq!(
            class_row_optional_count(None, 10),
            Some(1),
            "without a loaded table the stock Wizard row owns its spell-option byte"
        );
        assert_eq!(
            class_row_optional_count(Some(&rows), 10),
            Some(0),
            "a loaded server classes.2da is the reader state and must own the cursor width"
        );
    }

    #[test]
    fn loaded_classes_table_rejects_unknown_rows() {
        let rows = vec![Some(0)];

        assert_eq!(
            class_row_optional_count(Some(&rows), 41),
            None,
            "once a table is loaded, absent rows are unproven instead of falling back to stock"
        );
    }
}
