//! BIC GFF section canonicalization for `CharList_UpdateCharResponse`.
//!
//! This is deliberately scoped to the character-list BIC payload. The EE
//! server decompile sends the BIC as raw `WriteVOIDPtr` bytes after a DWORD
//! size, while legacy captures can carry a valid but sparse 1.69 GFF section
//! layout. Canonicalizing the embedded GFF gives the EE client the same
//! semantic BIC tree in a compact layout without inventing packet bytes outside
//! the decompile-confirmed `0x11/0x04` envelope.

pub(super) const GFF_HEADER_BYTES: usize = 56;

const GFF_STRUCT_BYTES: usize = 12;
const GFF_FIELD_BYTES: usize = 12;
const GFF_LABEL_BYTES: usize = 16;
const MAX_REASONABLE_REASSEMBLED_GAMEPLAY_PAYLOAD: usize = 1024 * 1024;
const MAX_REASONABLE_GFF_STRUCT_FIELDS: u32 = 0x3E80;
const MAX_REASONABLE_GFF_CLONE_RECORDS: usize = 0x3E800;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct GffLayout {
    pub struct_offset: u32,
    pub struct_count: u32,
    pub field_offset: u32,
    pub field_count: u32,
    pub label_offset: u32,
    pub label_count: u32,
    pub field_data_offset: u32,
    pub field_data_count: u32,
    pub field_indices_offset: u32,
    pub field_indices_count: u32,
    pub list_indices_offset: u32,
    pub list_indices_count: u32,
}

#[derive(Debug, Clone)]
pub(super) struct GffCanonicalizeSummary {
    pub bytes: Option<Vec<u8>>,
    pub old_layout: GffLayout,
    pub new_layout: GffLayout,
    pub repaired_legacy_section_offsets: bool,
    pub clamped_struct_field_ranges: u32,
    pub normalized_locstring_fields: u32,
    pub normalized_variable_fields: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct GffStructRecord {
    type_id: u32,
    data_or_offset: u32,
    field_count: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct GffFieldRecord {
    type_id: u32,
    label_index: u32,
    value: u32,
}

#[derive(Debug, Default)]
struct CloneStats {
    clamped_struct_field_ranges: u32,
    normalized_locstring_fields: u32,
    normalized_variable_fields: u32,
}

pub(super) fn canonicalize_bic_gff(gff: &[u8]) -> Result<GffCanonicalizeSummary, String> {
    let (old_layout, repaired_legacy_section_offsets) = read_gff_layout(gff)?;
    let _section_only = build_canonical_layout(old_layout, gff.len())?;
    if old_layout.struct_count == 0 {
        return Err("GFF has no root structure".into());
    }

    let label_bytes = checked_section_bytes(old_layout.label_count, GFF_LABEL_BYTES)?;
    let old_field_data = if old_layout.field_data_count == 0 {
        Vec::new()
    } else {
        let start = usize::try_from(old_layout.field_data_offset)
            .map_err(|_| "field-data offset overflow".to_string())?;
        let len = usize::try_from(old_layout.field_data_count)
            .map_err(|_| "field-data length overflow".to_string())?;
        if !section_fits(gff.len(), old_layout.field_data_offset, len) {
            return Err("old GFF field-data section out of bounds".into());
        }
        gff[start..start + len].to_vec()
    };

    let mut new_structs = Vec::new();
    let mut new_fields = Vec::new();
    let mut new_field_data = old_field_data.clone();
    let mut new_field_indices = Vec::new();
    let mut new_list_indices = Vec::new();
    let mut active_old_structs = vec![
        0_u8;
        usize::try_from(old_layout.struct_count)
            .map_err(|_| "struct count overflow".to_string())?
    ];
    let mut stats = CloneStats::default();
    let root_new_struct = clone_gff_struct_tree(
        gff,
        old_layout,
        0,
        0,
        &mut active_old_structs,
        &mut new_structs,
        &mut new_fields,
        &old_field_data,
        &mut new_field_data,
        &mut new_field_indices,
        &mut new_list_indices,
        &mut stats,
    )?;
    if root_new_struct != 0 {
        return Err(format!(
            "compacted GFF root was emitted at {root_new_struct}"
        ));
    }

    let new_layout = build_rewritten_layout(
        new_structs.len(),
        new_fields.len(),
        old_layout.label_count,
        new_field_data.len(),
        new_field_indices.len(),
        new_list_indices.len(),
    )?;
    let new_size = rewritten_layout_size(new_layout)?;
    let mut canonical = Vec::with_capacity(new_size);
    canonical.extend_from_slice(
        gff.get(..GFF_HEADER_BYTES)
            .ok_or_else(|| "GFF header unexpectedly missing".to_string())?,
    );
    write_layout(&mut canonical, new_layout)?;

    for record in &new_structs {
        append_u32(&mut canonical, record.type_id);
        append_u32(&mut canonical, record.data_or_offset);
        append_u32(&mut canonical, record.field_count);
    }
    for record in &new_fields {
        append_u32(&mut canonical, record.type_id);
        append_u32(&mut canonical, record.label_index);
        append_u32(&mut canonical, record.value);
    }

    append_section(gff, old_layout.label_offset, label_bytes, &mut canonical)?;
    canonical.extend_from_slice(&new_field_data);
    canonical.extend_from_slice(&new_field_indices);
    canonical.extend_from_slice(&new_list_indices);

    if canonical.len() != new_size {
        return Err(format!(
            "canonical size mismatch expected={new_size} actual={}",
            canonical.len()
        ));
    }

    let already_canonical = new_layout.struct_count == old_layout.struct_count
        && new_layout.field_count == old_layout.field_count
        && new_layout.field_indices_count == old_layout.field_indices_count
        && new_layout.list_indices_count == old_layout.list_indices_count
        && is_canonical_layout(old_layout, gff.len())
        && canonical.len() == gff.len()
        && canonical == gff;

    Ok(GffCanonicalizeSummary {
        bytes: (!already_canonical).then_some(canonical),
        old_layout,
        new_layout,
        repaired_legacy_section_offsets,
        clamped_struct_field_ranges: stats.clamped_struct_field_ranges,
        normalized_locstring_fields: stats.normalized_locstring_fields,
        normalized_variable_fields: stats.normalized_variable_fields,
    })
}

fn read_gff_layout(gff: &[u8]) -> Result<(GffLayout, bool), String> {
    if gff.len() < GFF_HEADER_BYTES {
        return Err(format!(
            "size {} < GFF header {GFF_HEADER_BYTES}",
            gff.len()
        ));
    }
    if gff.get(0..4) != Some(b"BIC ") {
        return Err("file type is not BIC".into());
    }
    if gff.get(4..8) != Some(b"V3.2") {
        return Err("unsupported GFF version".into());
    }

    let raw = GffLayout {
        struct_offset: read_u32(gff, 8)?,
        struct_count: read_u32(gff, 12)?,
        field_offset: read_u32(gff, 16)?,
        field_count: read_u32(gff, 20)?,
        label_offset: read_u32(gff, 24)?,
        label_count: read_u32(gff, 28)?,
        field_data_offset: read_u32(gff, 32)?,
        field_data_count: read_u32(gff, 36)?,
        field_indices_offset: read_u32(gff, 40)?,
        field_indices_count: read_u32(gff, 44)?,
        list_indices_offset: read_u32(gff, 48)?,
        list_indices_count: read_u32(gff, 52)?,
    };

    repair_legacy_contiguous_section_offsets(raw, gff.len())
}

fn repair_legacy_contiguous_section_offsets(
    raw: GffLayout,
    gff_size: usize,
) -> Result<(GffLayout, bool), String> {
    let struct_bytes = checked_section_bytes(raw.struct_count, GFF_STRUCT_BYTES)?;
    let field_bytes = checked_section_bytes(raw.field_count, GFF_FIELD_BYTES)?;
    let label_bytes = checked_section_bytes(raw.label_count, GFF_LABEL_BYTES)?;

    let inferred_field_offset = usize::try_from(raw.struct_offset)
        .map_err(|_| "struct offset overflow".to_string())?
        .checked_add(struct_bytes)
        .ok_or_else(|| "legacy inferred field offset overflow".to_string())?;
    let inferred_label_offset = inferred_field_offset
        .checked_add(field_bytes)
        .ok_or_else(|| "legacy inferred label offset overflow".to_string())?;
    let inferred_field_data_offset = inferred_label_offset
        .checked_add(label_bytes)
        .ok_or_else(|| "legacy inferred field-data offset overflow".to_string())?;

    let inferred_field_offset_u32 =
        checked_u32(inferred_field_offset, "legacy inferred field offset")?;
    let inferred_label_offset_u32 =
        checked_u32(inferred_label_offset, "legacy inferred label offset")?;
    let inferred_field_data_offset_u32 = checked_u32(
        inferred_field_data_offset,
        "legacy inferred field-data offset",
    )?;

    // The EE server/client GFF reader treats these header entries as absolute
    // byte offsets. Some legacy BICs observed inside 1.69
    // CharList_UpdateCharResponse packets keep the actual struct, field, and
    // label sections in the standard contiguous order, and the later
    // field-data offset proves the exact boundary, but the field/label offset
    // header entries themselves are not the absolute values the EE-side model
    // expects. This repair is intentionally narrow: only the two table offsets
    // are corrected, only when the inferred standard layout lands exactly on
    // the declared field-data section, and only when every inferred section
    // fits inside the BIC byte count.
    let can_repair = raw.field_data_offset == inferred_field_data_offset_u32
        && section_fits(gff_size, raw.struct_offset, struct_bytes)
        && section_fits(gff_size, inferred_field_offset_u32, field_bytes)
        && section_fits(gff_size, inferred_label_offset_u32, label_bytes)
        && section_fits(
            gff_size,
            raw.field_data_offset,
            usize::try_from(raw.field_data_count)
                .map_err(|_| "field-data count overflow".to_string())?,
        )
        && section_fits(
            gff_size,
            raw.field_indices_offset,
            usize::try_from(raw.field_indices_count)
                .map_err(|_| "field-index count overflow".to_string())?,
        )
        && section_fits(
            gff_size,
            raw.list_indices_offset,
            usize::try_from(raw.list_indices_count)
                .map_err(|_| "list-index count overflow".to_string())?,
        )
        && (raw.field_offset != inferred_field_offset_u32
            || raw.label_offset != inferred_label_offset_u32);

    if !can_repair {
        return Ok((raw, false));
    }

    let mut repaired = raw;
    repaired.field_offset = inferred_field_offset_u32;
    repaired.label_offset = inferred_label_offset_u32;
    Ok((repaired, true))
}

fn build_canonical_layout(layout: GffLayout, gff_size: usize) -> Result<GffLayout, String> {
    let struct_bytes = checked_section_bytes(layout.struct_count, GFF_STRUCT_BYTES)?;
    let field_bytes = checked_section_bytes(layout.field_count, GFF_FIELD_BYTES)?;
    let label_bytes = checked_section_bytes(layout.label_count, GFF_LABEL_BYTES)?;
    let field_data_bytes = usize::try_from(layout.field_data_count)
        .map_err(|_| "field-data count overflow".to_string())?;
    let field_indices_bytes = usize::try_from(layout.field_indices_count)
        .map_err(|_| "field-index count overflow".to_string())?;
    let list_indices_bytes = usize::try_from(layout.list_indices_count)
        .map_err(|_| "list-index count overflow".to_string())?;

    if !section_fits(gff_size, layout.struct_offset, struct_bytes)
        || !section_fits(gff_size, layout.field_offset, field_bytes)
        || !section_fits(gff_size, layout.label_offset, label_bytes)
        || !section_fits(gff_size, layout.field_data_offset, field_data_bytes)
        || !section_fits(gff_size, layout.field_indices_offset, field_indices_bytes)
        || !section_fits(gff_size, layout.list_indices_offset, list_indices_bytes)
    {
        return Err("one or more GFF sections are out of bounds".into());
    }

    build_rewritten_layout(
        usize::try_from(layout.struct_count).map_err(|_| "struct count overflow".to_string())?,
        usize::try_from(layout.field_count).map_err(|_| "field count overflow".to_string())?,
        layout.label_count,
        field_data_bytes,
        field_indices_bytes,
        list_indices_bytes,
    )
}

fn build_rewritten_layout(
    struct_count: usize,
    field_count: usize,
    label_count: u32,
    field_data_count: usize,
    field_indices_count: usize,
    list_indices_count: usize,
) -> Result<GffLayout, String> {
    let label_bytes = checked_section_bytes(label_count, GFF_LABEL_BYTES)?;
    let mut cursor = GFF_HEADER_BYTES;
    let struct_offset = checked_u32(cursor, "struct offset")?;
    cursor = cursor
        .checked_add(
            struct_count
                .checked_mul(GFF_STRUCT_BYTES)
                .ok_or("struct bytes overflow")?,
        )
        .ok_or("struct cursor overflow")?;
    let field_offset = checked_u32(cursor, "field offset")?;
    cursor = cursor
        .checked_add(
            field_count
                .checked_mul(GFF_FIELD_BYTES)
                .ok_or("field bytes overflow")?,
        )
        .ok_or("field cursor overflow")?;
    let label_offset = checked_u32(cursor, "label offset")?;
    cursor = cursor
        .checked_add(label_bytes)
        .ok_or("label cursor overflow")?;
    let field_data_offset = checked_u32(cursor, "field-data offset")?;
    cursor = cursor
        .checked_add(field_data_count)
        .ok_or("field-data cursor overflow")?;
    let field_indices_offset = checked_u32(cursor, "field-index offset")?;
    cursor = cursor
        .checked_add(field_indices_count)
        .ok_or("field-index cursor overflow")?;
    let list_indices_offset = checked_u32(cursor, "list-index offset")?;
    cursor = cursor
        .checked_add(list_indices_count)
        .ok_or("list-index cursor overflow")?;
    if cursor > MAX_REASONABLE_REASSEMBLED_GAMEPLAY_PAYLOAD {
        return Err(format!("canonical GFF size too large: {cursor}"));
    }

    Ok(GffLayout {
        struct_offset,
        struct_count: checked_u32(struct_count, "struct count")?,
        field_offset,
        field_count: checked_u32(field_count, "field count")?,
        label_offset,
        label_count,
        field_data_offset,
        field_data_count: checked_u32(field_data_count, "field-data count")?,
        field_indices_offset,
        field_indices_count: checked_u32(field_indices_count, "field-index count")?,
        list_indices_offset,
        list_indices_count: checked_u32(list_indices_count, "list-index count")?,
    })
}

fn clone_gff_struct_tree(
    gff: &[u8],
    old_layout: GffLayout,
    old_struct_index: u32,
    depth: u32,
    active_old_structs: &mut [u8],
    new_structs: &mut Vec<GffStructRecord>,
    new_fields: &mut Vec<GffFieldRecord>,
    old_field_data: &[u8],
    new_field_data: &mut Vec<u8>,
    new_field_indices: &mut Vec<u8>,
    new_list_indices: &mut Vec<u8>,
    stats: &mut CloneStats,
) -> Result<u32, String> {
    if old_struct_index >= old_layout.struct_count {
        return Err(format!(
            "clone struct index {old_struct_index} >= count {}",
            old_layout.struct_count
        ));
    }
    if depth > old_layout.struct_count.saturating_add(16) {
        return Err(format!(
            "GFF struct clone depth exceeded at old struct {old_struct_index}"
        ));
    }
    let active_index = usize::try_from(old_struct_index)
        .map_err(|_| "active struct index overflow".to_string())?;
    if active_old_structs
        .get(active_index)
        .copied()
        .ok_or_else(|| "active struct index out of bounds".to_string())?
        != 0
    {
        return Err(format!(
            "GFF struct cycle detected at old struct {old_struct_index}"
        ));
    }
    if new_structs.len() >= MAX_REASONABLE_GFF_CLONE_RECORDS
        || new_fields.len() >= MAX_REASONABLE_GFF_CLONE_RECORDS
    {
        return Err(format!(
            "GFF clone grew too large structs={} fields={}",
            new_structs.len(),
            new_fields.len()
        ));
    }

    let old_struct = read_gff_struct_record(gff, old_layout, old_struct_index)?;
    let current_new_struct_index = checked_u32(new_structs.len(), "new struct index")?;
    new_structs.push(GffStructRecord::default());
    active_old_structs[active_index] = 1;

    let (old_field_ids, clamped) = read_gff_struct_field_ids(gff, old_layout, old_struct_index)?;
    if clamped {
        stats.clamped_struct_field_ranges = stats.clamped_struct_field_ranges.saturating_add(1);
    }

    let mut new_struct = old_struct;
    let mut new_field_ids = Vec::with_capacity(old_field_ids.len());
    for old_field_id in old_field_ids {
        let mut new_field = read_gff_field_record(gff, old_layout, old_field_id)?;
        if new_field.type_id > 15 {
            active_old_structs[active_index] = 0;
            return Err(format!(
                "field {old_field_id} has invalid GFF type {}",
                new_field.type_id
            ));
        }
        if new_field.label_index >= old_layout.label_count {
            active_old_structs[active_index] = 0;
            return Err(format!(
                "field {old_field_id} label {} >= label count {}",
                new_field.label_index, old_layout.label_count
            ));
        }

        match new_field.type_id {
            14 => {
                let child_new_struct = clone_gff_struct_tree(
                    gff,
                    old_layout,
                    new_field.value,
                    depth + 1,
                    active_old_structs,
                    new_structs,
                    new_fields,
                    old_field_data,
                    new_field_data,
                    new_field_indices,
                    new_list_indices,
                    stats,
                )?;
                new_field.value = child_new_struct;
            }
            12 => {
                let (new_offset, normalized) = normalize_gff_locstring_field_data(
                    old_field_data,
                    new_field.value,
                    new_field_data,
                )?;
                if normalized {
                    stats.normalized_locstring_fields =
                        stats.normalized_locstring_fields.saturating_add(1);
                }
                new_field.value = new_offset;
            }
            6 | 7 | 9 | 10 | 11 | 13 => {
                let (new_offset, normalized) = normalize_gff_variable_field_data(
                    new_field.type_id,
                    old_field_data,
                    new_field.value,
                    new_field_data,
                )?;
                if normalized {
                    stats.normalized_variable_fields =
                        stats.normalized_variable_fields.saturating_add(1);
                }
                new_field.value = new_offset;
            }
            15 => {
                let old_list_structs = read_gff_list_struct_ids(gff, old_layout, new_field.value)?;
                let new_list_offset = checked_u32(new_list_indices.len(), "new list offset")?;
                append_u32(
                    new_list_indices,
                    checked_u32(old_list_structs.len(), "list count")?,
                );
                for old_list_struct in old_list_structs {
                    let child_new_struct = clone_gff_struct_tree(
                        gff,
                        old_layout,
                        old_list_struct,
                        depth + 1,
                        active_old_structs,
                        new_structs,
                        new_fields,
                        old_field_data,
                        new_field_data,
                        new_field_indices,
                        new_list_indices,
                        stats,
                    )?;
                    append_u32(new_list_indices, child_new_struct);
                }
                new_field.value = new_list_offset;
            }
            _ => {}
        }

        let new_field_id = checked_u32(new_fields.len(), "new field id")?;
        new_fields.push(new_field);
        new_field_ids.push(new_field_id);
    }

    if new_field_ids.is_empty() {
        new_struct.data_or_offset = 0;
        new_struct.field_count = 0;
    } else if new_field_ids.len() == 1 {
        new_struct.data_or_offset = new_field_ids[0];
        new_struct.field_count = 1;
    } else {
        new_struct.data_or_offset = checked_u32(new_field_indices.len(), "field-index offset")?;
        new_struct.field_count = checked_u32(new_field_ids.len(), "struct field count")?;
        for new_field_id in new_field_ids {
            append_u32(new_field_indices, new_field_id);
        }
    }

    let target = usize::try_from(current_new_struct_index)
        .map_err(|_| "new struct index usize overflow".to_string())?;
    new_structs[target] = new_struct;
    active_old_structs[active_index] = 0;
    Ok(current_new_struct_index)
}

fn read_gff_struct_record(
    gff: &[u8],
    layout: GffLayout,
    index: u32,
) -> Result<GffStructRecord, String> {
    if index >= layout.struct_count {
        return Err(format!(
            "struct index {index} >= count {}",
            layout.struct_count
        ));
    }
    let offset = record_offset(layout.struct_offset, index, GFF_STRUCT_BYTES)?;
    let end = offset
        .checked_add(GFF_STRUCT_BYTES)
        .ok_or("struct record offset overflow")?;
    let record = gff
        .get(offset..end)
        .ok_or_else(|| format!("struct index {index} out of bounds offset={offset}"))?;
    Ok(GffStructRecord {
        type_id: read_u32(record, 0)?,
        data_or_offset: read_u32(record, 4)?,
        field_count: read_u32(record, 8)?,
    })
}

fn read_gff_field_record(
    gff: &[u8],
    layout: GffLayout,
    index: u32,
) -> Result<GffFieldRecord, String> {
    if index >= layout.field_count {
        return Err(format!(
            "field index {index} >= count {}",
            layout.field_count
        ));
    }
    let offset = record_offset(layout.field_offset, index, GFF_FIELD_BYTES)?;
    let end = offset
        .checked_add(GFF_FIELD_BYTES)
        .ok_or("field record offset overflow")?;
    let record = gff
        .get(offset..end)
        .ok_or_else(|| format!("field index {index} out of bounds offset={offset}"))?;
    Ok(GffFieldRecord {
        type_id: read_u32(record, 0)?,
        label_index: read_u32(record, 4)?,
        value: read_u32(record, 8)?,
    })
}

fn read_gff_struct_field_ids(
    gff: &[u8],
    layout: GffLayout,
    struct_index: u32,
) -> Result<(Vec<u32>, bool), String> {
    let record = read_gff_struct_record(gff, layout, struct_index)?;
    if record.field_count > MAX_REASONABLE_GFF_STRUCT_FIELDS {
        return Err(format!(
            "struct {struct_index} has unreasonable field count {}",
            record.field_count
        ));
    }
    if record.field_count == 0 {
        return Ok((Vec::new(), false));
    }
    if record.field_count == 1 {
        if record.data_or_offset >= layout.field_count {
            return Err(format!(
                "struct {struct_index} single field id {} >= field count {}",
                record.data_or_offset, layout.field_count
            ));
        }
        return Ok((vec![record.data_or_offset], false));
    }

    if record.data_or_offset > layout.field_indices_count {
        return Err(format!(
            "struct {struct_index} field index offset out of bounds offset={} count={} table={}",
            record.data_or_offset, record.field_count, layout.field_indices_count
        ));
    }
    let available_count = (layout.field_indices_count - record.data_or_offset) / 4;
    let effective_count = record.field_count.min(available_count);
    let clamped = effective_count != record.field_count;
    if effective_count == 0 {
        return Ok((Vec::new(), clamped));
    }

    let indices_bytes = checked_section_bytes(effective_count, 4)?;
    if indices_bytes
        > usize::try_from(layout.field_indices_count - record.data_or_offset)
            .map_err(|_| "field-index remaining overflow".to_string())?
    {
        return Err(format!(
            "struct {struct_index} field index range out of bounds offset={} count={} table={}",
            record.data_or_offset, effective_count, layout.field_indices_count
        ));
    }
    let table_offset = layout
        .field_indices_offset
        .checked_add(record.data_or_offset)
        .ok_or_else(|| "field-index table offset overflow".to_string())?;
    if !section_fits(gff.len(), table_offset, indices_bytes) {
        return Err(format!(
            "struct {struct_index} field index range out of bounds offset={} count={} table={}",
            record.data_or_offset, effective_count, layout.field_indices_count
        ));
    }

    let start =
        usize::try_from(table_offset).map_err(|_| "field-index offset overflow".to_string())?;
    let mut field_ids = Vec::with_capacity(
        usize::try_from(effective_count)
            .map_err(|_| "effective field count overflow".to_string())?,
    );
    for index in 0..effective_count {
        let field_id = read_u32(
            gff,
            start
                .checked_add(
                    usize::try_from(index)
                        .map_err(|_| "field id index overflow".to_string())?
                        .checked_mul(4)
                        .ok_or("field id byte offset overflow")?,
                )
                .ok_or("field id offset overflow")?,
        )?;
        if field_id >= layout.field_count {
            return Err(format!(
                "struct {struct_index} field-index[{index}]={field_id} >= field count {}",
                layout.field_count
            ));
        }
        field_ids.push(field_id);
    }
    Ok((field_ids, clamped))
}

fn read_gff_list_struct_ids(
    gff: &[u8],
    layout: GffLayout,
    list_offset: u32,
) -> Result<Vec<u32>, String> {
    if list_offset > layout.list_indices_count || layout.list_indices_count - list_offset < 4 {
        return Err(format!(
            "list offset {list_offset} out of bounds table={}",
            layout.list_indices_count
        ));
    }
    let table_offset = layout
        .list_indices_offset
        .checked_add(list_offset)
        .ok_or_else(|| "list table offset overflow".to_string())?;
    if !section_fits(gff.len(), table_offset, 4) {
        return Err(format!(
            "list offset {list_offset} out of bounds table={}",
            layout.list_indices_count
        ));
    }
    let start =
        usize::try_from(table_offset).map_err(|_| "list table offset overflow".to_string())?;
    let count = read_u32(gff, start)?;
    let ids_bytes = checked_section_bytes(count, 4)?;
    if ids_bytes
        > usize::try_from(layout.list_indices_count - list_offset - 4)
            .map_err(|_| "list table remaining overflow".to_string())?
        || !section_fits(
            gff.len(),
            table_offset
                .checked_add(4)
                .ok_or_else(|| "list id table offset overflow".to_string())?,
            ids_bytes,
        )
    {
        return Err(format!(
            "list offset {list_offset} count {count} exceeds table={}",
            layout.list_indices_count
        ));
    }

    let mut struct_ids =
        Vec::with_capacity(usize::try_from(count).map_err(|_| "list count overflow".to_string())?);
    for index in 0..count {
        let struct_id = read_u32(
            gff,
            start
                .checked_add(4)
                .and_then(|offset| offset.checked_add(usize::try_from(index).ok()?.checked_mul(4)?))
                .ok_or("list struct id offset overflow")?,
        )?;
        if struct_id >= layout.struct_count {
            return Err(format!(
                "list offset {list_offset} struct[{index}]={struct_id} >= count {}",
                layout.struct_count
            ));
        }
        struct_ids.push(struct_id);
    }
    Ok(struct_ids)
}

fn normalize_gff_locstring_field_data(
    field_data: &[u8],
    old_offset: u32,
    rewritten_field_data: &mut Vec<u8>,
) -> Result<(u32, bool), String> {
    let old = usize::try_from(old_offset).map_err(|_| "locstring offset overflow".to_string())?;
    if old > field_data.len() || field_data.len().saturating_sub(old) < 4 {
        return Err(format!(
            "locstring offset {old_offset} out of field-data bounds {}",
            field_data.len()
        ));
    }

    let declared_size = read_u32(field_data, old)?;
    let available_after_size = field_data.len() - old - 4;
    let mut string_ref = 0xFFFF_FFFF_u32;
    let mut parsed_count = 0_u32;
    let mut cursor = old + 4;
    let mut structurally_valid = true;

    if available_after_size < 8 || declared_size < 8 {
        structurally_valid = false;
    } else {
        string_ref = read_u32(field_data, cursor)?;
        cursor += 4;
        let declared_count = read_u32(field_data, cursor)?;
        cursor += 4;
        let mut remaining = usize::try_from(declared_size)
            .map_err(|_| "locstring declared size overflow".to_string())?
            - 8;
        if usize::try_from(declared_size)
            .map_err(|_| "locstring declared size overflow".to_string())?
            > available_after_size
        {
            structurally_valid = false;
            remaining = available_after_size.saturating_sub(8);
        }
        if declared_count >= 0xFF {
            structurally_valid = false;
        }

        let parse_limit = declared_count.min(0xFE);
        for _ in 0..parse_limit {
            if remaining < 8 || field_data.len().saturating_sub(cursor) < 8 {
                structurally_valid = false;
                break;
            }
            let text_length = usize::try_from(read_u32(field_data, cursor + 4)?)
                .map_err(|_| "locstring text length overflow".to_string())?;
            if text_length > remaining - 8
                || field_data.len().saturating_sub(cursor + 8) < text_length
            {
                structurally_valid = false;
                break;
            }
            cursor += 8 + text_length;
            remaining -= 8 + text_length;
            parsed_count = parsed_count.saturating_add(1);
        }
        if parsed_count != declared_count || remaining != 0 {
            structurally_valid = false;
        }
    }

    if structurally_valid {
        return Ok((old_offset, false));
    }

    let appended_offset = checked_u32(rewritten_field_data.len(), "locstring rewrite offset")?;
    let entries_start = old + 12;
    let parsed_entry_bytes = cursor.saturating_sub(entries_start);
    let normalized_size = 8_usize
        .checked_add(parsed_entry_bytes)
        .ok_or("locstring normalized size overflow")?;
    append_u32(
        rewritten_field_data,
        checked_u32(normalized_size, "locstring normalized size")?,
    );
    append_u32(rewritten_field_data, string_ref);
    append_u32(rewritten_field_data, parsed_count);
    if parsed_entry_bytes > 0 {
        let end = entries_start
            .checked_add(parsed_entry_bytes)
            .ok_or("locstring parsed entry range overflow")?;
        let entries = field_data
            .get(entries_start..end)
            .ok_or_else(|| "locstring parsed entry range out of bounds".to_string())?;
        rewritten_field_data.extend_from_slice(entries);
    }
    Ok((appended_offset, true))
}

fn normalize_gff_variable_field_data(
    field_type: u32,
    old_field_data: &[u8],
    old_offset: u32,
    new_field_data: &mut Vec<u8>,
) -> Result<(u32, bool), String> {
    let old =
        usize::try_from(old_offset).map_err(|_| "variable field offset overflow".to_string())?;
    let valid = match field_type {
        6 | 7 | 9 => old <= old_field_data.len() && old_field_data.len().saturating_sub(old) >= 8,
        10 | 13 => {
            if old > old_field_data.len() || old_field_data.len().saturating_sub(old) < 4 {
                false
            } else {
                let length = usize::try_from(read_u32(old_field_data, old)?)
                    .map_err(|_| "variable field length overflow".to_string())?;
                length <= old_field_data.len().saturating_sub(old + 4)
            }
        }
        11 => {
            if old >= old_field_data.len() {
                false
            } else {
                let length = usize::from(old_field_data[old]);
                length <= 16 && length <= old_field_data.len().saturating_sub(old + 1)
            }
        }
        _ => return Ok((old_offset, false)),
    };

    if valid {
        return Ok((old_offset, false));
    }

    let new_offset = checked_u32(new_field_data.len(), "variable field rewrite offset")?;
    match field_type {
        6 | 7 | 9 => new_field_data.extend_from_slice(&[0; 8]),
        10 | 13 => append_u32(new_field_data, 0),
        11 => new_field_data.push(0),
        _ => {}
    }
    Ok((new_offset, true))
}

fn write_layout(bytes: &mut [u8], layout: GffLayout) -> Result<(), String> {
    write_u32(bytes, 8, layout.struct_offset)?;
    write_u32(bytes, 12, layout.struct_count)?;
    write_u32(bytes, 16, layout.field_offset)?;
    write_u32(bytes, 20, layout.field_count)?;
    write_u32(bytes, 24, layout.label_offset)?;
    write_u32(bytes, 28, layout.label_count)?;
    write_u32(bytes, 32, layout.field_data_offset)?;
    write_u32(bytes, 36, layout.field_data_count)?;
    write_u32(bytes, 40, layout.field_indices_offset)?;
    write_u32(bytes, 44, layout.field_indices_count)?;
    write_u32(bytes, 48, layout.list_indices_offset)?;
    write_u32(bytes, 52, layout.list_indices_count)?;
    Ok(())
}

fn is_canonical_layout(layout: GffLayout, gff_size: usize) -> bool {
    build_canonical_layout(layout, gff_size)
        .map(|canonical| {
            rewritten_layout_size(canonical).ok() == Some(gff_size)
                && layout.struct_offset == canonical.struct_offset
                && layout.field_offset == canonical.field_offset
                && layout.label_offset == canonical.label_offset
                && layout.field_data_offset == canonical.field_data_offset
                && layout.field_indices_offset == canonical.field_indices_offset
                && layout.list_indices_offset == canonical.list_indices_offset
        })
        .unwrap_or(false)
}

fn rewritten_layout_size(layout: GffLayout) -> Result<usize, String> {
    let list_offset = usize::try_from(layout.list_indices_offset)
        .map_err(|_| "list offset overflow".to_string())?;
    let list_count = usize::try_from(layout.list_indices_count)
        .map_err(|_| "list count overflow".to_string())?;
    list_offset
        .checked_add(list_count)
        .ok_or_else(|| "layout size overflow".to_string())
}

fn append_section(
    gff: &[u8],
    old_offset: u32,
    length: usize,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    if length == 0 {
        return Ok(());
    }
    if !section_fits(gff.len(), old_offset, length) {
        return Err("GFF section out of bounds".into());
    }
    let start = usize::try_from(old_offset).map_err(|_| "section offset overflow".to_string())?;
    output.extend_from_slice(&gff[start..start + length]);
    Ok(())
}

fn section_fits(total_size: usize, offset: u32, length: usize) -> bool {
    let Ok(start) = usize::try_from(offset) else {
        return false;
    };
    start <= total_size && length <= total_size - start
}

fn checked_section_bytes(count: u32, stride: usize) -> Result<usize, String> {
    usize::try_from(count)
        .map_err(|_| "section count overflow".to_string())?
        .checked_mul(stride)
        .ok_or_else(|| "section byte count overflow".to_string())
}

fn checked_u32(value: usize, name: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{name} exceeds u32"))
}

fn record_offset(base: u32, index: u32, stride: usize) -> Result<usize, String> {
    let base = usize::try_from(base).map_err(|_| "record base offset overflow".to_string())?;
    let index = usize::try_from(index).map_err(|_| "record index overflow".to_string())?;
    base.checked_add(index.checked_mul(stride).ok_or("record stride overflow")?)
        .ok_or_else(|| "record offset overflow".to_string())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset.checked_add(4).ok_or("u32 offset overflow")?;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| format!("u32 read out of bounds offset={offset}"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> Result<(), String> {
    let end = offset.checked_add(4).ok_or("u32 write offset overflow")?;
    let target = bytes
        .get_mut(offset..end)
        .ok_or_else(|| format!("u32 write out of bounds offset={offset}"))?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn append_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_legacy_contiguous_bic_field_and_label_offsets() {
        let mut gff = Vec::new();
        gff.extend_from_slice(b"BIC ");
        gff.extend_from_slice(b"V3.2");
        append_u32(&mut gff, 56); // struct offset
        append_u32(&mut gff, 1); // struct count
        append_u32(&mut gff, 0); // legacy-bad field offset; should be 68
        append_u32(&mut gff, 1); // field count
        append_u32(&mut gff, 40); // legacy-bad label offset; should be 80
        append_u32(&mut gff, 1); // label count
        append_u32(&mut gff, 96); // proves contiguous struct/field/label boundary
        append_u32(&mut gff, 0); // field-data count
        append_u32(&mut gff, 96); // field-indices offset
        append_u32(&mut gff, 0); // field-indices count
        append_u32(&mut gff, 96); // list-indices offset
        append_u32(&mut gff, 0); // list-indices count

        append_u32(&mut gff, 0xFFFF_FFFF); // root struct type
        append_u32(&mut gff, 0); // one direct field id
        append_u32(&mut gff, 1); // field count

        append_u32(&mut gff, 0); // BYTE field type
        append_u32(&mut gff, 0); // label index
        append_u32(&mut gff, 7); // scalar value

        let mut label = [0_u8; 16];
        label[..4].copy_from_slice(b"Test");
        gff.extend_from_slice(&label);
        assert_eq!(gff.len(), 96);

        let summary =
            canonicalize_bic_gff(&gff).expect("legacy contiguous BIC should canonicalize");
        assert!(summary.repaired_legacy_section_offsets);
        let canonical = summary
            .bytes
            .expect("header offset repair must produce replacement bytes");

        assert_eq!(read_u32(&canonical, 16).unwrap(), 68);
        assert_eq!(read_u32(&canonical, 24).unwrap(), 80);
        assert_eq!(read_u32(&canonical, 32).unwrap(), 96);
    }
}
