use super::*;

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
            // Diamond and EE both enter the SetAllButtons slot loop immediately after the CNW
            // declared-length field. The four bytes before this slice are that CNW length, not a
            // quickbar-owned prefix, so the emitted read buffer starts with slot 0's type byte.
            read_buffer: Vec::new(),
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
        self.fragment_bits[0] = (final_bits & 0x04) != 0;
        self.fragment_bits[1] = (final_bits & 0x02) != 0;
        self.fragment_bits[2] = (final_bits & 0x01) != 0;
        let mut bytes = Vec::with_capacity((self.fragment_bits.len() + 7) / 8);
        for chunk in self.fragment_bits.chunks(8) {
            let mut byte = 0u8;
            for (bit, value) in chunk.iter().enumerate() {
                if *value {
                    byte |= 0x80 >> bit;
                }
            }
            bytes.push(byte);
        }
        bytes
    }
}

pub(super) fn build_ee_quickbar_payload(parsed: &QuickbarParse) -> Option<Vec<u8>> {
    let mut writer = QuickbarPacketWriter::new();
    for button in &parsed.buttons {
        match &button.kind {
            QuickbarButtonKind::Item {
                primary,
                secondary,
                recovered_type_tag,
            } => {
                if !quickbar_item_button_has_verified_ee_materialization(
                    primary,
                    secondary,
                    *recovered_type_tag,
                ) {
                    // EE `CNWCMessage::HandleServerToPlayerGuiQuickbar_SetButton`
                    // does more than consume bytes for type-1 buttons: after
                    // `sub_14079FAC0` reads the item-object body, the receiver
                    // resolves or creates a client item object and aborts the
                    // all-buttons application if that object materialization
                    // fails. Diamond's server writer similarly emits the item
                    // branch only after resolving a real `CNWSItem`.
                    //
                    // The parser below owns the legacy item bytes well enough
                    // to prove the 36-slot quickbar boundary, but the bridge
                    // does not yet have an exact decompile-backed proof that
                    // every captured item appearance/active-property subobject
                    // can be materialized by EE. A type-0 blank is therefore
                    // the strict translation for item slots until that focused
                    // item-materialization translator exists. This preserves
                    // later spell/general slots instead of letting one unproven
                    // item poison the entire SetAllButtons packet.
                    writer.write_byte(0);
                    continue;
                }
                // Type 1 is the decompile-owned item button branch for both
                // Diamond and EE SetAllButtons. The source bytes were already
                // parsed into bounded item-object models, so emission is not a
                // raw copy: write the EE receiver shape from typed fields and
                // insert the EE-only appearance/property additions in the
                // focused helpers below.
                writer.write_byte(1);
                write_quickbar_item_object(&mut writer, primary, true)?;
                write_quickbar_item_object(&mut writer, secondary, false)?;
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
            QuickbarButtonKind::General { bytes } => {
                if quickbar_general_bytes_are_verified_ee_identical(bytes) {
                    // These general slot records are byte-identical between
                    // Diamond's `sub_469FD0` receiver and EE's
                    // `SendServerToPlayerGuiQuickbar_SetButton` writer. The
                    // model is still typed and bounded by the reader; this is
                    // not an unknown raw passthrough.
                    writer.read_buffer.extend_from_slice(bytes);
                } else {
                    writer.write_byte(0);
                }
            }
            QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported => {
                writer.write_byte(0);
            }
        }
    }

    let fragments = writer.clone().fragment_bytes();
    let declared =
        u32::try_from(HIGH_LEVEL_HEADER_BYTES.checked_add(writer.read_buffer.len())?).ok()?;
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

pub(super) fn quickbar_item_button_has_verified_ee_materialization(
    primary: &QuickbarItemObject,
    secondary: &QuickbarItemObject,
    recovered_type_tag: bool,
) -> bool {
    // EE's server writer only emits a type-1 quickbar item branch after it has
    // resolved a real CNWSItem, then writes a BOOL-gated primary item object and
    // a BOOL-gated secondary item object. The client receiver consumes those
    // same BOOL-gated objects and sets the quickbar slot from the object id(s).
    //
    // The EE client receiver does not stop at byte consumption. In
    // `sub_14079DB00`, the type-1 branch reads the primary/secondary item
    // object bodies, resolves or creates client item objects, registers them in
    // the external object array, and only then calls the quickbar item setter.
    // Any failure jumps to the function-wide abort path before later spell slots
    // are applied.
    //
    // A bounded legacy item parse is therefore necessary but not sufficient. The
    // proxy must also prove that the EE-facing object materialization will
    // succeed. That proof belongs in the state-aware object registry layer:
    //   legacy item object -> verified EE client item object -> quickbar slot
    //
    // Until that focused translator exists, item slots are deliberately emitted
    // as type-0 blanks. This is a strict semantic downgrade, not a passthrough:
    // it preserves the decompile-owned SetAllButtons boundary and lets verified
    // spell/general slots apply instead of letting one unproven item abort the
    // whole quickbar update.
    let _ = (primary, secondary, recovered_type_tag);
    false
}

fn quickbar_item_object_has_verified_ee_materialization(item: &QuickbarItemObject) -> bool {
    if !item.present {
        return true;
    }
    if item.object_id == NWN_OBJECT_INVALID {
        return false;
    }
    if item.active_props.is_none() {
        return false;
    }
    let Some(expected_legacy) = legacy_item_appearance_read_size(item.appearance_type) else {
        return false;
    };
    item.appearance_bytes.len() == expected_legacy
        && read_u32_le(&item.appearance_bytes, 0) == Some(item.base_item)
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
    write_ee_quickbar_item_appearance_from_legacy(writer, item)?;
    append_empty_ee_visual_transform_map(writer);
    Some(())
}

fn write_ee_quickbar_item_appearance_from_legacy(
    writer: &mut QuickbarPacketWriter,
    item: &QuickbarItemObject,
) -> Option<()> {
    // EE `sub_14079FAC0` first reads the base item DWORD, then checks the
    // EE protocol feature gate `(0x2001, 0x23)`. In the EE-facing bridge
    // session this reader takes the EE-era branch: model-part fields that
    // Diamond's `sub_4514C0` reads as BYTEs are read as WORDs by EE. Emit the
    // widened shape from the typed legacy bytes instead of copying the legacy
    // byte body, otherwise the following visual-transform map is parsed from
    // the wrong offset and the client aborts the whole SetAllButtons update.
    writer.write_dword(item.base_item);
    let appearance = item.appearance_bytes.as_slice();
    match item.appearance_type {
        0 => {
            writer.write_word(u16::from(*appearance.get(CNW_LENGTH_BYTES)?));
        }
        1 => {
            let body_start = CNW_LENGTH_BYTES;
            let colors_start = body_start.checked_add(1)?;
            let colors_end = colors_start.checked_add(6)?;
            writer.write_word(u16::from(*appearance.get(body_start)?));
            writer
                .read_buffer
                .extend_from_slice(appearance.get(colors_start..colors_end)?);
        }
        2 => {
            let part_start = CNW_LENGTH_BYTES;
            for offset in 0..3 {
                writer.write_word(u16::from(*appearance.get(part_start.checked_add(offset)?)?));
            }
            writer.write_byte(*appearance.get(part_start.checked_add(3)?)?);
        }
        3 => {
            let parts_start = CNW_LENGTH_BYTES;
            let colors_start = parts_start.checked_add(19)?;
            let colors_end = colors_start.checked_add(6)?;
            for byte in appearance.get(parts_start..colors_start)? {
                writer.write_word(u16::from(*byte));
            }
            writer
                .read_buffer
                .extend_from_slice(appearance.get(colors_start..colors_end)?);
            append_ee_armor_layered_color_table(writer);
        }
        _ => return None,
    }
    Some(())
}

fn append_ee_armor_layered_color_table(writer: &mut QuickbarPacketWriter) {
    // EE's model-type-3 branch reads an additional 19x6 BYTE layered-color
    // table after the legacy armor colors. Diamond has no corresponding
    // quickbar field, so zeroes are the neutral, decompile-owned expansion.
    writer
        .read_buffer
        .extend(std::iter::repeat(0).take(EE_QUICKBAR_ARMOR_LAYERED_COLOR_BYTES));
}

fn append_empty_ee_visual_transform_map(writer: &mut QuickbarPacketWriter) {
    // `sub_14079FAC0` always calls `sub_140973160` after item appearance. In
    // the EE feature-0x23 branch, that helper reads two INT32-prefixed
    // transform maps. Diamond has no quickbar-side transform map, so the exact
    // neutral expansion is two zero counts.
    writer.write_i32(0);
    writer.write_i32(0);
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
    // EE `sub_14076BD30` reads one additional post-DWORD BOOL that Diamond
    // `sub_451020` does not write. The live-object equipment translator calls
    // this the EE-only active-property / CanUseItem bit and inserts it as false;
    // quickbar item buttons use the same item-property reader, so emit the same
    // decompile-backed neutral value here instead of shifting the property count
    // into a BOOL slot.
    writer.write_bit(false);
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
        writer.write_byte(loc.language_id);
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
