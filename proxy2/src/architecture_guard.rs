//! Project-level guardrails for protocol translator shape ownership.
//!
//! Fixture and harness names are useful in tests because they identify the
//! capture that proved a reader shape. Production translator code should not
//! branch on those names. If a real packet requires new handling, model the
//! decompiled reader order, active session state, or resource-table data instead.

use std::{
    fs,
    path::{Path, PathBuf},
};

const FORBIDDEN_PRODUCTION_EXAMPLE_TERMS: &[&str] = &[
    "local_winds",
    "winds_of_eremor",
    "eremor",
    "starcore",
    "sooty",
    "docks",
    "prelude",
    "town_watch",
    "path of ascension",
    "fashion_accessory",
    "cep_hg",
];

#[test]
fn production_translators_do_not_name_fixture_examples() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();

    for path in rust_files_under(&root) {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "tests.rs" || name == "architecture_guard.rs")
        {
            continue;
        }
        if !path_is_translator_or_strict(&path) {
            continue;
        }
        if path_is_resource_profile(&path) {
            continue;
        }

        let text = fs::read_to_string(&path).expect("source file should be readable");
        let production = strip_comments(&strip_test_modules(&text));
        let lowered = production.to_ascii_lowercase();
        for term in FORBIDDEN_PRODUCTION_EXAMPLE_TERMS {
            if lowered.contains(term) {
                violations.push(format!(
                    "{} contains capture/example term `{term}` in production code",
                    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
                        .unwrap_or(&path)
                        .display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "production protocol translators must be keyed by decompiled reader shape, session state, or resource tables, not by named examples:\n{}",
        violations.join("\n")
    );
}

fn path_is_translator_or_strict(path: &Path) -> bool {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Ok(relative) = path.strip_prefix(manifest) else {
        return false;
    };
    relative == Path::new("src").join("strict.rs")
        || relative
            .components()
            .map(|component| component.as_os_str())
            .take(2)
            .eq([
                Path::new("src").as_os_str(),
                Path::new("translate").as_os_str(),
            ])
}

fn path_is_resource_profile(path: &Path) -> bool {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Ok(relative) = path.strip_prefix(manifest) else {
        return false;
    };
    relative.starts_with(Path::new("src").join("translate").join("profiles"))
}

fn rust_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files);
    files
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            collect_rust_files(&entry.path(), files);
        }
        return;
    }
    if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
        files.push(path.to_owned());
    }
}

fn strip_test_modules(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;

    while let Some(relative_cfg_start) = text[cursor..].find("#[cfg") {
        let cfg_start = cursor + relative_cfg_start;
        output.push_str(&text[cursor..cfg_start]);

        let Some(cfg_end) = find_attr_end(text, cfg_start) else {
            cursor = cfg_start;
            break;
        };
        let attr = &text[cfg_start..cfg_end];
        if !attr.contains("test") {
            output.push_str(attr);
            cursor = cfg_end;
            continue;
        }

        let after_attr = text[cfg_end..].trim_start();
        let skipped_ws = text[cfg_end..].len() - after_attr.len();
        if !after_attr.starts_with("mod ") {
            output.push_str(attr);
            cursor = cfg_end;
            continue;
        }

        let module_start = cfg_end + skipped_ws;
        let Some(open_brace_relative) = text[module_start..].find('{') else {
            cursor = module_start;
            break;
        };
        let open_brace = module_start + open_brace_relative;
        let Some(module_end) = matching_brace_end(text, open_brace) else {
            cursor = open_brace;
            break;
        };
        cursor = module_end;
    }

    output.push_str(&text[cursor..]);
    output
}

fn find_attr_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut cursor = start;
    while cursor < bytes.len() {
        if bytes[cursor] == b']' {
            return cursor.checked_add(1);
        }
        cursor = cursor.checked_add(1)?;
    }
    None
}

fn matching_brace_end(text: &str, open_brace: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open_brace;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'{' => depth = depth.checked_add(1)?,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return cursor.checked_add(1);
                }
            }
            _ => {}
        }
        cursor = cursor.checked_add(1)?;
    }
    None
}

fn strip_comments(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut block_depth = 0usize;

    for line in text.lines() {
        let mut cursor = 0usize;
        while cursor < line.len() {
            let rest = &line[cursor..];
            if block_depth > 0 {
                if let Some(end) = rest.find("*/") {
                    block_depth -= 1;
                    cursor += end + 2;
                } else {
                    cursor = line.len();
                }
                continue;
            }

            let line_comment = rest.find("//");
            let block_comment = rest.find("/*");
            match (line_comment, block_comment) {
                (Some(line_at), Some(block_at)) if line_at < block_at => {
                    output.push_str(&rest[..line_at]);
                    cursor = line.len();
                }
                (Some(line_at), None) => {
                    output.push_str(&rest[..line_at]);
                    cursor = line.len();
                }
                (_, Some(block_at)) => {
                    output.push_str(&rest[..block_at]);
                    block_depth += 1;
                    cursor += block_at + 2;
                }
                (None, None) => {
                    output.push_str(rest);
                    cursor = line.len();
                }
            }
        }
        output.push('\n');
    }

    output
}
