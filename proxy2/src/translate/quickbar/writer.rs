// EE quickbar writer. This module emits a fresh verified EE-side packet from
// the typed legacy parse; unsupported source bytes are never copied raw.

#[derive(Debug, Clone)]
struct QuickbarPacketWriter {
    read_buffer: Vec<u8>,
    fragment_bits: Vec<bool>,
}

impl QuickbarPacketWriter {
    fn new() -> Self {
        Self {
            read_buffer: vec![0, 0, 0, 0],
            fragment_bits: vec![false, false, false],
        }
    }

    fn write_byte(&mut self, value: u8) {
        self.read_buffer.push(value);
    }

    fn write_word(&mut self, value: u16) {
        self.read_buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn write_dword(&mut self, value: u32) {
        self.read_buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.read_buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn write_string(&mut self, value: &[u8]) -> Option<()> {
        let len = u32::try_from(value.len()).ok()?;
        self.write_dword(len);
        self.read_buffer.extend_from_slice(value);
        Some(())
    }

    fn write_bit(&mut self, value: bool) {
        self.fragment_bits.push(value);
    }

    fn fragment_bytes(mut self) -> Vec<u8> {
        let final_bits = u8::try_from(self.fragment_bits.len() % 8).unwrap_or(0);
        for bit in 0..3 {
            self.fragment_bits[bit] = (final_bits & (1u8 << bit)) != 0;
        }
        let mut bytes = Vec::with_capacity((self.fragment_bits.len() + 7) / 8);
        for chunk in self.fragment_bits.chunks(8) {
            let mut byte = 0u8;
            for (bit, value) in chunk.iter().enumerate() {
                if *value {
                    byte |= 1u8 << bit;
                }
            }
            bytes.push(byte);
        }
        bytes
    }
}

fn build_ee_quickbar_payload(parsed: &QuickbarParse) -> Option<Vec<u8>> {
    let mut writer = QuickbarPacketWriter::new();
    for button in &parsed.buttons {
        match &button.kind {
            QuickbarButtonKind::Item {
                primary: _,
                secondary: _,
                recovered_type_tag: _,
            } => {
                // The legacy source item boundary is consumed by the parser, but
                // the EE item-bearing receiver shape still needs exact per-variant
                // validation before we can safely emit it. Emit a known-valid
                // blank slot instead of forwarding reconstructed item bytes.
                writer.write_byte(0);
            }
            QuickbarButtonKind::Spell {
                spell_class,
                spell_id,
                metamagic,
                domain,
            } => {
                writer.write_byte(2);
                writer.write_byte(*spell_class);
                writer.write_dword(*spell_id);
                writer.write_byte(*metamagic);
                writer.write_byte(*domain);
            }
            QuickbarButtonKind::General { bytes: _ } => {
                // General quickbar buttons are structurally diverse. The sender
                // decompile gives us their byte widths, but until each general
                // family has an EE receiver-backed validator, strict translation
                // preserves only spells and emits blank slots for these records.
                writer.write_byte(0);
            }
            QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported => {
                writer.write_byte(0);
            }
        }
    }

    let fragments = writer.clone().fragment_bytes();
    let declared = u32::try_from(HIGH_LEVEL_HEADER_BYTES.checked_add(writer.read_buffer.len())?)
        .ok()?;
    let mut payload = Vec::with_capacity(
        HIGH_LEVEL_HEADER_BYTES
            .checked_add(CNW_LENGTH_BYTES)?
            .checked_add(writer.read_buffer.len())?
            .checked_add(fragments.len())?,
    );
    payload.push(parsed.envelope);
    payload.push(QUICKBAR_MAJOR);
    payload.push(SET_ALL_BUTTONS_MINOR);
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&writer.read_buffer);
    payload.extend_from_slice(&fragments);
    Some(payload)
}

/// Build a valid EE/Diamond-compatible `GuiQuickbar_SetAllButtons` payload with all 36 slots blank.
///
/// Decompile evidence:
/// - `CNWSMessage::SendServerToPlayerGuiQuickbar_SetButton` uses the all-buttons path when the
///   single-slot flag is false.
/// - That path emits exactly 36 button records and does not emit a slot index byte.
/// - Button type 0 is the blank/general-empty shape and has no trailing payload.
///
/// We use this only as a reliable-window placeholder while buffering a legacy quickbar stream that
/// spans multiple deflated M windows. It is intentionally not a raw passthrough: the emitted packet is
/// constructed from a known semantic shape and then validated as `GuiQuickbar`.
pub fn build_blank_set_all_buttons_payload(envelope: u8) -> Option<Vec<u8>> {
    let buttons = (0..LEGACY_QUICKBAR_BUTTON_COUNT)
        .map(|_| QuickbarButton {
            kind: QuickbarButtonKind::General { bytes: vec![0] },
        })
        .collect();

    let parsed = QuickbarParse {
        envelope,
        declared: 0,
        read_size: 0,
        fragment_size: 0,
        final_cursor: 0,
        buttons,
        direct_opcode_stream: false,
    };

    build_ee_quickbar_payload(&parsed)
}

fn write_quickbar_item_object(
    writer: &mut QuickbarPacketWriter,
    item: &QuickbarItemObject,
    include_int_param: bool,
) -> Option<()> {
    writer.write_bit(item.present);
    if !item.present {
        return Some(());
    }
    writer.write_dword(ee_server_object_id_wire_value(item.object_id));
    if include_int_param {
        writer.write_i32(item.int_param);
    }
    write_quickbar_item_appearance(writer, item)?;
    if let Some(active_props) = item.active_props.as_ref() {
        write_quickbar_active_item_properties(writer, active_props, item.base_item)
    } else {
        write_empty_quickbar_active_item_properties(writer, item.base_item)
    }
}

fn write_quickbar_item_appearance(
    writer: &mut QuickbarPacketWriter,
    item: &QuickbarItemObject,
) -> Option<()> {
    if item.appearance_bytes.len() < CNW_LENGTH_BYTES {
        return None;
    }
    if read_u32_le(&item.appearance_bytes, 0)? != item.base_item {
        return None;
    }
    let expected_legacy = legacy_item_appearance_read_size(item.appearance_type)?;
    if item.appearance_bytes.len() != expected_legacy {
        return None;
    }
    writer.read_buffer.extend_from_slice(&item.appearance_bytes);
    if item.base_item == NWN_BASE_ITEM_ARMOR
        && item.appearance_bytes.len() == EE_QUICKBAR_ARMOR_EXACT_COPY_BYTES
    {
        append_ee_extended_armor_table(writer);
    }
    append_ee_legacy_visual_transform_identity(writer);
    Some(())
}

fn append_ee_extended_armor_table(writer: &mut QuickbarPacketWriter) {
    for _ in 0..EE_QUICKBAR_ARMOR_EXTRA_TABLE_ZERO_DWORDS {
        writer.write_dword(0);
    }
}

fn append_ee_legacy_visual_transform_identity(writer: &mut QuickbarPacketWriter) {
    for value in EE_LEGACY_VISUAL_TRANSFORM_IDENTITY_DWORDS {
        writer.write_dword(value);
    }
}

fn write_quickbar_active_item_properties(
    writer: &mut QuickbarPacketWriter,
    active_props: &QuickbarActiveItemProperties,
    base_item: u32,
) -> Option<()> {
    if legacy_quickbar_base_item_requires_active_property_word(base_item) {
        writer.write_word(active_props.armor_word);
    }
    writer.write_bit(active_props.name_is_locstring);
    if active_props.name_is_locstring {
        write_quickbar_loc_string(writer, &active_props.locstring_name)?;
    } else {
        writer.write_string(&active_props.string_name)?;
    }
    writer.write_bit(active_props.post_name_bool1);
    writer.write_dword(active_props.cost);
    writer.write_dword(active_props.stack_or_charges);
    writer.write_bit(active_props.post_name_bool2);
    writer.write_bit(active_props.post_name_bool3);
    writer.write_bit(active_props.post_name_bool4);
    let property_count = u8::try_from(active_props.properties.len()).ok()?;
    if property_count > MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES {
        return None;
    }
    writer.write_byte(property_count);
    for property in &active_props.properties {
        writer.write_word(property.property);
        writer.write_word(property.subtype);
        writer.write_word(property.cost_table_value);
        writer.write_byte(property.param);
    }
    writer.write_byte(active_props.state_mask);
    writer.write_byte(active_props.value_mask);
    write_quickbar_active_value_mask_bytes(writer, active_props);
    Some(())
}

fn write_empty_quickbar_active_item_properties(
    writer: &mut QuickbarPacketWriter,
    base_item: u32,
) -> Option<()> {
    if legacy_quickbar_base_item_requires_active_property_word(base_item) {
        writer.write_word(0);
    }
    writer.write_bit(false);
    writer.write_string(&[])?;
    writer.write_bit(false);
    writer.write_dword(0);
    writer.write_dword(0);
    writer.write_bit(false);
    writer.write_bit(false);
    writer.write_bit(false);
    writer.write_byte(0);
    writer.write_byte(0);
    writer.write_byte(0);
    Some(())
}

fn write_quickbar_loc_string(
    writer: &mut QuickbarPacketWriter,
    loc: &QuickbarLocStringField,
) -> Option<()> {
    writer.write_bit(loc.custom_tlk);
    if loc.custom_tlk {
        writer.write_bit(loc.language_selector);
        writer.write_dword(loc.string_ref);
    } else {
        writer.write_string(&loc.text)?;
    }
    Some(())
}

fn write_quickbar_active_value_mask_bytes(
    writer: &mut QuickbarPacketWriter,
    active_props: &QuickbarActiveItemProperties,
) {
    let mut values = active_props.value_mask_bytes.iter().copied();
    for bit in 0..8 {
        if (active_props.value_mask & (1u8 << bit)) != 0 {
            writer.write_byte(values.next().unwrap_or(0));
        }
    }
}

fn ee_server_object_id_wire_value(object_id: u32) -> u32 {
    if object_id == NWN_OBJECT_INVALID || (object_id & EE_SERVER_OBJECT_ID_MARKER_BIT) != 0 {
        object_id
    } else {
        object_id | EE_SERVER_OBJECT_ID_MARKER_BIT
    }
}
