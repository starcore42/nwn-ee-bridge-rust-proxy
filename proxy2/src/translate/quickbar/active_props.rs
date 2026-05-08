// Active item-property parser for quickbar item buttons.

fn parse_legacy_quickbar_active_item_properties(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<QuickbarActiveItemProperties> {
    let before = reader.clone();
    if let Some(properties) =
        parse_legacy_quickbar_active_item_properties_standard(reader, base_item_id)
    {
        return Some(properties);
    }
    *reader = before;
    parse_legacy_quickbar_active_item_properties_bare_inline_fallback(reader, base_item_id)
}

fn parse_legacy_quickbar_active_item_properties_standard(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<QuickbarActiveItemProperties> {
    let mut properties = QuickbarActiveItemProperties::default();
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        properties.has_armor_word = true;
        properties.armor_word = reader.read_word()?;
    }

    properties.name_is_locstring = reader.read_bit()?;
    if properties.name_is_locstring {
        properties.locstring_name = reader.read_loc_string()?;
    } else {
        properties.string_name = reader.read_string()?;
    }

    parse_legacy_quickbar_active_item_properties_tail(reader, properties)
}

fn parse_legacy_quickbar_active_item_properties_bare_inline_fallback(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<QuickbarActiveItemProperties> {
    let mut probe = reader.clone();
    let mut prefix = QuickbarActiveItemProperties::default();
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        prefix.has_armor_word = true;
        prefix.armor_word = probe.read_word()?;
    }

    let name_length_offset = probe.cursor;
    if probe.read_buffer.len().saturating_sub(name_length_offset) < CNW_LENGTH_BYTES + 1 + 9
        || read_u32_le(probe.read_buffer, name_length_offset)? != 0
    {
        return None;
    }

    let text_start = name_length_offset.checked_add(CNW_LENGTH_BYTES)?;
    let text_limit = probe
        .read_buffer
        .len()
        .min(text_start.checked_add(MAX_QUICKBAR_BARE_ACTIVE_ITEM_NAME_BYTES)?);
    let mut printable_end = text_start;
    while printable_end < text_limit
        && is_legacy_bare_active_item_name_byte(probe.read_buffer[printable_end])
    {
        printable_end += 1;
    }
    if printable_end == text_start {
        return None;
    }

    for candidate_text_end in (text_start + 1..=printable_end).rev() {
        let mut trial = probe.clone();
        let mut candidate = prefix.clone();
        let _legacy_name_bit = trial.read_bit()?;
        trial.cursor = candidate_text_end;
        candidate.name_is_locstring = false;
        candidate.string_name = probe
            .read_buffer
            .get(text_start..candidate_text_end)?
            .to_vec();
        if let Some(parsed) =
            parse_legacy_quickbar_active_item_properties_tail(&mut trial, candidate)
        {
            // HG's empty-inline fallback is a verified legacy encoding variant:
            // the source carries a zero length DWORD followed by printable name
            // bytes. EE expects a direct CExoString length, so the writer below
            // synthesizes the missing length and preserves the text.
            *reader = trial;
            return Some(parsed);
        }
    }

    None
}

fn parse_legacy_quickbar_active_item_properties_tail(
    reader: &mut QuickbarPacketReader<'_>,
    mut properties: QuickbarActiveItemProperties,
) -> Option<QuickbarActiveItemProperties> {
    properties.post_name_bool1 = reader.read_bit()?;
    properties.cost = reader.read_dword()?;
    properties.stack_or_charges = reader.read_dword()?;
    properties.post_name_bool2 = reader.read_bit()?;
    properties.post_name_bool3 = reader.read_bit()?;
    properties.post_name_bool4 = reader.read_bit()?;
    let property_count = reader.read_byte()?;
    if property_count > MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES {
        return None;
    }
    properties.properties.reserve(usize::from(property_count));
    for _ in 0..property_count {
        properties.properties.push(QuickbarActivePropertyEntry {
            property: reader.read_word()?,
            subtype: reader.read_word()?,
            cost_table_value: reader.read_word()?,
            param: reader.read_byte()?,
        });
    }

    properties.state_mask = reader.read_byte()?;
    properties.value_mask = reader.read_byte()?;
    for bit in 0..8 {
        if (properties.value_mask & (1u8 << bit)) != 0 {
            properties.value_mask_bytes.push(reader.read_byte()?);
        }
    }
    Some(properties)
}

fn skip_legacy_quickbar_active_item_properties(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<()> {
    let before = reader.clone();
    if skip_legacy_quickbar_active_item_properties_standard(reader, base_item_id).is_some() {
        return Some(());
    }
    *reader = before;
    skip_legacy_quickbar_active_item_properties_bare_inline_fallback(reader, base_item_id)
}

fn skip_legacy_quickbar_active_item_properties_standard(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<()> {
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        let _armor_word = reader.read_word()?;
    }

    if reader.read_bit()? {
        reader.skip_loc_string()?;
    } else {
        reader.skip_string()?;
    }

    skip_legacy_quickbar_active_item_properties_tail(reader)
}

fn skip_legacy_quickbar_active_item_properties_bare_inline_fallback(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
) -> Option<()> {
    let mut probe = reader.clone();
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        let _armor_word = probe.read_word()?;
    }

    let name_length_offset = probe.cursor;
    if probe.read_buffer.len().saturating_sub(name_length_offset) < CNW_LENGTH_BYTES + 1 + 9
        || read_u32_le(probe.read_buffer, name_length_offset)? != 0
    {
        return None;
    }

    let text_start = name_length_offset.checked_add(CNW_LENGTH_BYTES)?;
    let text_limit =
        probe
            .read_buffer
            .len()
            .min(text_start.checked_add(MAX_QUICKBAR_BARE_ACTIVE_ITEM_NAME_BYTES)?);
    let mut printable_end = text_start;
    while printable_end < text_limit
        && is_legacy_bare_active_item_name_byte(probe.read_buffer[printable_end])
    {
        printable_end += 1;
    }
    if printable_end == text_start {
        return None;
    }

    for candidate_text_end in (text_start + 1..=printable_end).rev() {
        let mut trial = probe.clone();
        let _legacy_name_bit = trial.read_bit()?;
        trial.cursor = candidate_text_end;
        if skip_legacy_quickbar_active_item_properties_tail(&mut trial).is_some() {
            *reader = trial;
            return Some(());
        }
    }

    None
}

fn skip_legacy_quickbar_active_item_properties_tail(
    reader: &mut QuickbarPacketReader<'_>,
) -> Option<()> {
    let _post_name_bool1 = reader.read_bit()?;
    let _cost = reader.read_dword()?;
    let _stack_or_charges = reader.read_dword()?;
    let _post_name_bool2 = reader.read_bit()?;
    let _post_name_bool3 = reader.read_bit()?;
    let _post_name_bool4 = reader.read_bit()?;
    let property_count = reader.read_byte()?;
    if property_count > MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES {
        return None;
    }
    for _ in 0..property_count {
        let _property = reader.read_word()?;
        let _subtype = reader.read_word()?;
        let _cost_table_value = reader.read_word()?;
        let _param = reader.read_byte()?;
    }

    let _state_mask = reader.read_byte()?;
    let value_mask = reader.read_byte()?;
    for bit in 0..8 {
        if (value_mask & (1u8 << bit)) != 0 {
            let _value = reader.read_byte()?;
        }
    }
    Some(())
}

fn is_legacy_bare_active_item_name_byte(ch: u8) -> bool {
    (0x20..=0x7E).contains(&ch)
}

