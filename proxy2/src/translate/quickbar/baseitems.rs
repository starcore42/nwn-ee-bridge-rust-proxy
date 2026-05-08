use super::*;

// baseitems.2da support used by the quickbar item-object parser.
//
// The parser needs the model-type column to know the exact legacy appearance
// width. Direct 2DA files are preferred. If HG assets are still packaged inside
// HAKs and no extracted baseitems.2da is visible, fall back to a conservative
// built-in table for common base item ids so spells/general slots still translate
// while opaque item bodies are blanked instead of leaked.

pub(super) fn quickbar_base_item_model_types() -> Option<&'static [i8]> {
    QUICKBAR_BASE_ITEM_MODEL_TYPES
        .get_or_init(load_quickbar_base_item_model_types)
        .as_deref()
}

fn load_quickbar_base_item_model_types() -> Option<Vec<i8>> {
    if let Some(path) = find_direct_baseitems_2da() {
        match fs::read_to_string(&path) {
            Ok(text) => {
                if let Some(parsed) = parse_baseitems_model_types_2da(&text) {
                    tracing::info!(path = %path.display(), rows = parsed.len(), "loaded baseitems.2da model types for quickbar translation");
                    return Some(parsed);
                }
                tracing::warn!(path = %path.display(), "baseitems.2da found but model-type column could not be parsed");
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "failed to read baseitems.2da for quickbar translation");
            }
        }
    }

    tracing::warn!(
        "baseitems.2da not found as an extracted file; using conservative built-in quickbar model-type fallback"
    );
    Some(fallback_baseitems_model_types())
}

fn find_direct_baseitems_2da() -> Option<PathBuf> {
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

    let candidates = [
        PathBuf::from(BASEITEMS_2DA_NAME),
        PathBuf::from(HG_REQUIRED_FILES_DIR).join(BASEITEMS_2DA_NAME),
        PathBuf::from("assets").join(BASEITEMS_2DA_NAME),
        PathBuf::from("fixtures").join(BASEITEMS_2DA_NAME),
        PathBuf::from("NWN Diamond")
            .join("1.72 builder resources")
            .join("1.72 full 2dasource")
            .join(BASEITEMS_2DA_NAME),
    ];
    candidates.into_iter().find(|path| path.is_file())
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
