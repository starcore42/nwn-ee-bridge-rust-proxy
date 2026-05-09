use super::*;

// Active item-property parser for quickbar item buttons.

pub(super) fn parse_legacy_quickbar_active_item_properties(
    reader: &mut QuickbarPacketReader<'_>,
    base_item_id: u32,
    allow_model_type2_zero_overlap: bool,
) -> Option<QuickbarActiveItemProperties> {
    let before = reader.clone();
    if let Some(properties) =
        parse_legacy_quickbar_active_item_properties_standard(reader, base_item_id)
    {
        return Some(properties);
    }
    *reader = before.clone();
    if let Some(properties) =
        parse_legacy_quickbar_active_item_properties_bare_inline_fallback(reader, base_item_id)
    {
        return Some(properties);
    }

    *reader = before.clone();
    if allow_model_type2_zero_overlap
        && reader.cursor > 0
        && reader.read_buffer.get(reader.cursor - 1).copied() == Some(0)
        && read_u32_le(reader.read_buffer, reader.cursor - 1) == Some(0)
    {
        // Diamond `sub_4514C0` / EE `sub_14079FAC0` consume four model-type-2
        // appearance bytes after the base item. HG/CEP captures also contain a
        // small set of rod quickbar entries whose active-property name is the
        // legacy bare-inline variant: a zero CExoString length immediately
        // followed by printable bytes. For those records the fourth appearance
        // byte is zero and is also the first byte of that zero length. Treat this
        // as a named compatibility encoding only when the complete bare-inline
        // active-property tail validates from `cursor - 1`; no bytes from an
        // unvalidated item body are forwarded raw.
        let mut trial = before;
        trial.cursor -= 1;
        if let Some(properties) = parse_legacy_quickbar_active_item_properties_bare_inline_fallback(
            &mut trial,
            base_item_id,
        ) {
            tracing::info!(
                cursor = reader.cursor,
                base_item = base_item_id,
                "server GuiQuickbar_SetAllButtons accepted model-type-2 zero-overlap active-property tail"
            );
            *reader = trial;
            return Some(properties);
        }
    }

    None
}

pub(super) fn parse_legacy_quickbar_active_item_properties_compact_byte_tail(
    read_buffer: &[u8],
    cursor: usize,
    record_end: usize,
    base_item_id: u32,
    allow_model_type2_zero_overlap: bool,
) -> Option<(QuickbarActiveItemProperties, usize)> {
    let mut cursor = cursor;
    let mut properties = QuickbarActiveItemProperties::default();
    if legacy_quickbar_base_item_requires_active_property_word(base_item_id) {
        properties.has_armor_word = true;
        properties.armor_word = read_u16_le(read_buffer, cursor)?;
        cursor = cursor.checked_add(2)?;
    }

    let (name_start, length_offset) = if allow_model_type2_zero_overlap
        && cursor > 0
        && read_buffer.get(cursor - 1).copied() == Some(0)
        && read_u32_le(read_buffer, cursor - 1) == Some(0)
    {
        (cursor, cursor - 1)
    } else {
        (cursor.checked_add(CNW_LENGTH_BYTES)?, cursor)
    };
    let name_len = usize::try_from(read_u32_le(read_buffer, length_offset)?).ok()?;
    if name_len > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
        return None;
    }
    let name_end = name_start.checked_add(name_len)?;
    if name_end > record_end {
        return None;
    }
    properties.name_is_locstring = false;
    properties.string_name = read_buffer.get(name_start..name_end)?.to_vec();
    cursor = name_end;

    // HG's late `SetAllButtons` item records sometimes continue with a compact
    // byte-owned active-property tail after the shared fragment stream has only
    // enough bits left to reach its declared final-bit modulus. This is not raw
    // passthrough: the byte layout below is still the Diamond/EE
    // `sub_451020` active-property body with the BOOL fields supplied by a
    // narrow compatibility policy instead of source fragment bits. The source
    // record must end exactly at the proven next quickbar slot boundary.
    properties.post_name_bool1 = true;
    properties.cost = read_u32_le(read_buffer, cursor)?;
    cursor = cursor.checked_add(CNW_LENGTH_BYTES)?;
    properties.stack_or_charges = read_u32_le(read_buffer, cursor)?;
    cursor = cursor.checked_add(CNW_LENGTH_BYTES)?;
    properties.post_name_bool2 = true;
    properties.post_name_bool3 = false;
    properties.post_name_bool4 = true;

    let property_count = *read_buffer.get(cursor)?;
    cursor = cursor.checked_add(1)?;
    if property_count > MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES {
        return None;
    }
    properties.properties.reserve(usize::from(property_count));
    for _ in 0..property_count {
        properties.properties.push(QuickbarActivePropertyEntry {
            property: read_u16_le(read_buffer, cursor)?,
            subtype: read_u16_le(read_buffer, cursor.checked_add(2)?)?,
            cost_table_value: read_u16_le(read_buffer, cursor.checked_add(4)?)?,
            param: *read_buffer.get(cursor.checked_add(6)?)?,
        });
        cursor = cursor.checked_add(7)?;
    }

    properties.state_mask = *read_buffer.get(cursor)?;
    cursor = cursor.checked_add(1)?;
    properties.value_mask = *read_buffer.get(cursor)?;
    cursor = cursor.checked_add(1)?;
    for bit in 0..8 {
        if (properties.value_mask & (1u8 << bit)) != 0 {
            properties.value_mask_bytes.push(*read_buffer.get(cursor)?);
            cursor = cursor.checked_add(1)?;
        }
    }

    if cursor != record_end {
        return None;
    }
    Some((properties, cursor))
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

    let encoded_text_start = name_length_offset.checked_add(CNW_LENGTH_BYTES)?;
    for text_start in legacy_bare_inline_text_starts(probe.read_buffer, encoded_text_start) {
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
            continue;
        }

        for candidate_text_end in (text_start + 1..=printable_end).rev() {
            let mut trial = probe.clone();
            let mut candidate = prefix.clone();
            consume_legacy_bare_inline_name_bits(&mut trial)?;
            trial.cursor = candidate_text_end;
            candidate.name_is_locstring = false;
            candidate.string_name = probe
                .read_buffer
                .get(text_start..candidate_text_end)?
                .to_vec();
            if let Some(parsed) =
                parse_legacy_quickbar_active_item_properties_tail(&mut trial, candidate)
            {
                // HG's empty-inline fallback is a verified legacy encoding
                // variant: the source carries a zero length DWORD, sometimes a
                // single NUL pad byte, then printable name bytes. EE expects a
                // direct CExoString length, so the writer below synthesizes the
                // missing length and preserves the text.
                *reader = trial;
                return Some(parsed);
            }
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

pub(super) fn skip_legacy_quickbar_active_item_properties(
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

    let encoded_text_start = name_length_offset.checked_add(CNW_LENGTH_BYTES)?;
    for text_start in legacy_bare_inline_text_starts(probe.read_buffer, encoded_text_start) {
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
            continue;
        }

        for candidate_text_end in (text_start + 1..=printable_end).rev() {
            let mut trial = probe.clone();
            consume_legacy_bare_inline_name_bits(&mut trial)?;
            trial.cursor = candidate_text_end;
            if skip_legacy_quickbar_active_item_properties_tail(&mut trial).is_some() {
                *reader = trial;
                return Some(());
            }
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

fn consume_legacy_bare_inline_name_bits(reader: &mut QuickbarPacketReader<'_>) -> Option<()> {
    let name_is_locstring = reader.read_bit()?;
    if name_is_locstring {
        // Diamond `sub_451020` calls `sub_53E700` for locstring-shaped active
        // item names. `sub_53E700` then reads another BOOL: true means
        // BYTE-language + DWORD-strref, false means a string body. The HG
        // bare-inline compatibility shape is the latter, so consume and require
        // that second selector before scanning the printable source text.
        if reader.read_bit()? {
            return None;
        }
    }
    Some(())
}

fn legacy_bare_inline_text_starts(read_buffer: &[u8], encoded_text_start: usize) -> Vec<usize> {
    let mut starts = vec![encoded_text_start];
    if read_buffer.get(encoded_text_start).copied() == Some(0)
        && read_buffer
            .get(encoded_text_start.saturating_add(1))
            .copied()
            .is_some_and(is_legacy_bare_active_item_name_byte)
    {
        starts.push(encoded_text_start + 1);
    }
    starts
}
