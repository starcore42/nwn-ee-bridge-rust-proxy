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
    build_ee_quickbar_payload_with_context(parsed, None)
}

pub(super) fn build_ee_quickbar_payload_with_context(
    parsed: &QuickbarParse,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Option<Vec<u8>> {
    let mut writer = QuickbarPacketWriter::new();
    for button in &parsed.buttons {
        match &button.kind {
            QuickbarButtonKind::Item {
                primary,
                secondary,
                source,
                recovered_type_tag,
            } => {
                if !quickbar_item_button_has_verified_ee_materialization(
                    primary,
                    secondary,
                    *source,
                    *recovered_type_tag,
                    materialization,
                ) {
                    // The item branch is only emitted after the focused item
                    // materialization proof below accepts it. Ambiguous compact
                    // bodies, invalid object ids, missing active-property
                    // payloads, or appearance/baseitem mismatches become
                    // deliberate type-0 blanks so one unproven item cannot abort
                    // the whole SetAllButtons update.
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
                class_byte,
                spell_id,
                legacy_metamagic,
                legacy_level,
            } => {
                if !quickbar_spell_slot_has_verified_ee_materialization(
                    *class_byte,
                    *spell_id,
                    *legacy_metamagic,
                    *legacy_level,
                ) {
                    // Diagnostic-only strict downgrade. The spell tuple was
                    // parsed as the decompile-owned type-2 shape, but the
                    // harness can force blank spell slots to prove whether the
                    // client stall is caused by spell/resource materialization
                    // rather than by reliable-window/zlib framing. This path
                    // is never enabled by default and never forwards unknown
                    // bytes.
                    writer.write_byte(0);
                    continue;
                }
                let ee_spell = spell::legacy_spell_tuple_to_ee_wire(
                    *class_byte,
                    *spell_id,
                    *legacy_metamagic,
                    *legacy_level,
                )?;
                writer.write_byte(2);
                writer.write_byte(ee_spell.class_byte);
                writer.write_dword(ee_spell.spell_id);
                writer.write_byte(ee_spell.ee_metamagic);
                writer.write_byte(ee_spell.ee_level);
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
    let declared = u32::try_from(
        HIGH_LEVEL_HEADER_BYTES
            .checked_add(CNW_LENGTH_BYTES)?
            .checked_add(writer.read_buffer.len())?,
    )
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

pub(super) fn quickbar_item_button_has_verified_ee_materialization(
    primary: &QuickbarItemObject,
    secondary: &QuickbarItemObject,
    source: QuickbarItemSource,
    recovered_type_tag: bool,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> bool {
    quickbar_item_button_verified_materialization_proofs(
        primary,
        secondary,
        source,
        recovered_type_tag,
        materialization,
    )
    .is_some()
}

pub(super) fn quickbar_item_button_verified_materialization_proofs(
    primary: &QuickbarItemObject,
    secondary: &QuickbarItemObject,
    source: QuickbarItemSource,
    recovered_type_tag: bool,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Option<[Option<QuickbarItemMaterializationProof>; 2]> {
    quickbar_item_button_materialization_decision(
        primary,
        secondary,
        source,
        recovered_type_tag,
        materialization,
    )
    .ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuickbarItemMaterializationRejectReason {
    RecoveredTypeTag,
    MissingTypeSource,
    NoPresentItem,
    InvalidObjectId,
    MissingActiveProperties,
    UnsupportedAppearanceType,
    AppearanceShape,
    MissingStateProof,
}

pub(super) fn quickbar_item_button_materialization_decision(
    primary: &QuickbarItemObject,
    secondary: &QuickbarItemObject,
    source: QuickbarItemSource,
    recovered_type_tag: bool,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Result<[Option<QuickbarItemMaterializationProof>; 2], QuickbarItemMaterializationRejectReason>
{
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
    // A bounded legacy item parse is therefore necessary but not sufficient.
    // The accepted proof is intentionally narrow and decompile-backed:
    //   1. recovered compact slots are not promoted;
    //   2. each present item has a non-invalid id;
    //   3. the packet carries the active-property payload read by
    //      `sub_14076BD30`;
    //   4. the appearance body length matches the baseitems.2da appearance
    //      family and begins with the same base item DWORD.
    //
    // If the object registry already knows the id, that is a state proof. For
    // explicit source type-1 slots, EE `sub_14079DB00` also has a verified
    // self-materialization path: after `sub_14079FAC0` succeeds it calls
    // `sub_140769130`, assigns the quickbar item id, and registers the new
    // client item with `CGameObjectArray::AddExternalObject` before applying the
    // slot. Compact byte-owned HG slots do not prove the same source fragment
    // cursor, so they need registry proof from verified live-object, GUI
    // item-create, or inventory Feature-25 refs before being emitted. Missing
    // source type recovery remains boundary-only.
    if recovered_type_tag {
        return Err(QuickbarItemMaterializationRejectReason::RecoveredTypeTag);
    }
    if !primary.present && !secondary.present {
        return Err(QuickbarItemMaterializationRejectReason::NoPresentItem);
    }
    let requirement = match source {
        QuickbarItemSource::ExplicitTypeAndFragmentBits => {
            QuickbarItemMaterializationRequirement::AllowExplicitSelfMaterialization
        }
        QuickbarItemSource::CompactByteOwnedWithSourceType => {
            QuickbarItemMaterializationRequirement::RequireKnownState
        }
        QuickbarItemSource::RecoveredMissingType => {
            return Err(QuickbarItemMaterializationRejectReason::MissingTypeSource);
        }
    };
    let primary_proof =
        quickbar_item_object_verified_materialization_proof(primary, materialization, requirement)?;
    let secondary_proof = quickbar_item_object_verified_materialization_proof(
        secondary,
        materialization,
        requirement,
    )?;
    Ok([primary_proof, secondary_proof])
}

#[derive(Debug, Clone, Copy)]
enum QuickbarItemMaterializationRequirement {
    AllowExplicitSelfMaterialization,
    RequireKnownState,
}

fn quickbar_item_object_verified_materialization_proof(
    item: &QuickbarItemObject,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
    requirement: QuickbarItemMaterializationRequirement,
) -> Result<Option<QuickbarItemMaterializationProof>, QuickbarItemMaterializationRejectReason> {
    if !item.present {
        return Ok(None);
    }
    if item.object_id == NWN_OBJECT_INVALID {
        return Err(QuickbarItemMaterializationRejectReason::InvalidObjectId);
    }
    if item.active_props.is_none() {
        return Err(QuickbarItemMaterializationRejectReason::MissingActiveProperties);
    }
    let Some(expected_legacy) = legacy_item_appearance_read_size(item.appearance_type) else {
        return Err(QuickbarItemMaterializationRejectReason::UnsupportedAppearanceType);
    };
    if item.appearance_bytes.len() != expected_legacy
        || read_u32_le(&item.appearance_bytes, 0) != Some(item.base_item)
    {
        return Err(QuickbarItemMaterializationRejectReason::AppearanceShape);
    }
    match requirement {
        QuickbarItemMaterializationRequirement::AllowExplicitSelfMaterialization => Ok(Some(
            materialization
                .and_then(|materialization| {
                    materialization.item_object_materialization_proof(item.object_id)
                })
                .unwrap_or(QuickbarItemMaterializationProof::ExplicitSelfMaterialization),
        )),
        QuickbarItemMaterializationRequirement::RequireKnownState => materialization
            .and_then(|materialization| {
                materialization.item_object_materialization_proof(item.object_id)
            })
            .map(Some)
            .ok_or(QuickbarItemMaterializationRejectReason::MissingStateProof),
    }
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
            let legacy_palette = appearance.get(colors_start..colors_end)?;
            writer.read_buffer.extend_from_slice(legacy_palette);
            append_ee_armor_layered_color_table(writer, legacy_palette)?;
        }
        _ => return None,
    }
    Some(())
}

fn append_ee_armor_layered_color_table(
    writer: &mut QuickbarPacketWriter,
    legacy_palette: &[u8],
) -> Option<()> {
    // EE's model-type-3 branch reads an additional 19x6 BYTE layered-color
    // table after the legacy armor colors. Diamond supplies only the six global
    // palette bytes, so repeat that palette for each EE armor/accessory row.
    if legacy_palette.len() != 6 {
        return None;
    }
    for _ in 0..(EE_QUICKBAR_ARMOR_LAYERED_COLOR_BYTES / 6) {
        writer.read_buffer.extend_from_slice(legacy_palette);
    }
    Some(())
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

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_ARMOR_BASE_ITEM: u32 = 0x10;
    const LEGACY_WEAPON_BASE_ITEM: u32 = 0x01;
    const LEGACY_SHIELD_BASE_ITEM: u32 = 0x38;
    const LEGACY_CLOAK_BASE_ITEM: u32 = 0x50;

    fn model_type_3_armor_item_with_active_props(
        palette: [u8; 6],
        active_props: QuickbarActiveItemProperties,
    ) -> QuickbarItemObject {
        let mut appearance_bytes = Vec::new();
        appearance_bytes.extend_from_slice(&LEGACY_ARMOR_BASE_ITEM.to_le_bytes());
        for part in 0..19u8 {
            appearance_bytes.push(0x20 + part);
        }
        appearance_bytes.extend_from_slice(&palette);

        QuickbarItemObject {
            present: true,
            object_id: 0x8000_0042,
            int_param: -1,
            base_item: LEGACY_ARMOR_BASE_ITEM,
            appearance_type: 3,
            active_props: Some(active_props),
            appearance_bytes,
        }
    }

    fn quickbar_item_with_appearance(
        base_item: u32,
        appearance_type: i8,
        appearance_tail: &[u8],
    ) -> QuickbarItemObject {
        let mut appearance_bytes = Vec::new();
        appearance_bytes.extend_from_slice(&base_item.to_le_bytes());
        appearance_bytes.extend_from_slice(appearance_tail);

        QuickbarItemObject {
            present: true,
            object_id: 0x8000_0042,
            int_param: -1,
            base_item,
            appearance_type,
            active_props: Some(QuickbarActiveItemProperties::default()),
            appearance_bytes,
        }
    }

    fn model_type_3_armor_item_with_palette(palette: [u8; 6]) -> QuickbarItemObject {
        model_type_3_armor_item_with_active_props(palette, QuickbarActiveItemProperties::default())
    }

    fn quickbar_payload_with_primary_item(item: QuickbarItemObject) -> Vec<u8> {
        let mut buttons = vec![QuickbarButton {
            kind: QuickbarButtonKind::Item {
                primary: item,
                secondary: QuickbarItemObject::default(),
                source: QuickbarItemSource::ExplicitTypeAndFragmentBits,
                recovered_type_tag: false,
            },
        }];
        buttons.extend((1..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| QuickbarButton {
            kind: QuickbarButtonKind::General { bytes: vec![0] },
        }));
        let parsed = QuickbarParse {
            envelope: b'P',
            declared: 0,
            read_size: 0,
            fragment_size: 0,
            final_cursor: 0,
            buttons,
            direct_opcode_stream: false,
        };

        build_ee_quickbar_payload(&parsed).expect("quickbar payload should write")
    }

    #[test]
    fn explicit_item_materialization_reports_self_materialization_without_registry() {
        let item = quickbar_item_with_appearance(LEGACY_SHIELD_BASE_ITEM, 0, &[0x34]);

        let proofs = quickbar_item_button_verified_materialization_proofs(
            &item,
            &QuickbarItemObject::default(),
            QuickbarItemSource::ExplicitTypeAndFragmentBits,
            false,
            None,
        )
        .expect("explicit type-1 item body may self-materialize");

        assert_eq!(
            proofs,
            [
                Some(QuickbarItemMaterializationProof::ExplicitSelfMaterialization),
                None
            ]
        );
    }

    #[test]
    fn compact_item_materialization_reports_feature25_second_proof() {
        let item = quickbar_item_with_appearance(LEGACY_SHIELD_BASE_ITEM, 0, &[0x34]);
        let item_object_id = item.object_id;
        let item_object_proof = |object_id| {
            (object_id == item_object_id)
                .then_some(QuickbarItemMaterializationProof::InventoryFeature25SecondList)
        };
        let materialization = QuickbarMaterializationContext::new_with_proof(&item_object_proof);

        let proofs = quickbar_item_button_verified_materialization_proofs(
            &item,
            &QuickbarItemObject::default(),
            QuickbarItemSource::CompactByteOwnedWithSourceType,
            false,
            Some(&materialization),
        )
        .expect("state-proven compact item body may be emitted");

        assert_eq!(
            proofs,
            [
                Some(QuickbarItemMaterializationProof::InventoryFeature25SecondList),
                None
            ]
        );
    }

    #[test]
    fn compact_item_materialization_still_requires_registry_proof() {
        let item = quickbar_item_with_appearance(LEGACY_SHIELD_BASE_ITEM, 0, &[0x34]);

        assert!(
            quickbar_item_button_verified_materialization_proofs(
                &item,
                &QuickbarItemObject::default(),
                QuickbarItemSource::CompactByteOwnedWithSourceType,
                false,
                None,
            )
            .is_none(),
            "byte-owned compact item bodies need session-state proof"
        );
    }

    #[test]
    fn quickbar_item_materialization_reports_reject_reason_buckets() {
        let item = quickbar_item_with_appearance(LEGACY_SHIELD_BASE_ITEM, 0, &[0x34]);
        assert_eq!(
            quickbar_item_button_materialization_decision(
                &item,
                &QuickbarItemObject::default(),
                QuickbarItemSource::CompactByteOwnedWithSourceType,
                false,
                None,
            ),
            Err(QuickbarItemMaterializationRejectReason::MissingStateProof),
            "compact byte-owned item slots need registry or Feature-25 proof"
        );
        assert_eq!(
            quickbar_item_button_materialization_decision(
                &item,
                &QuickbarItemObject::default(),
                QuickbarItemSource::RecoveredMissingType,
                false,
                None,
            ),
            Err(QuickbarItemMaterializationRejectReason::MissingTypeSource),
            "missing source type recovery is boundary-only"
        );
        assert_eq!(
            quickbar_item_button_materialization_decision(
                &item,
                &QuickbarItemObject::default(),
                QuickbarItemSource::ExplicitTypeAndFragmentBits,
                true,
                None,
            ),
            Err(QuickbarItemMaterializationRejectReason::RecoveredTypeTag),
            "recovered compact slots remain blocked even with item-looking bytes"
        );

        let mut invalid_id = item.clone();
        invalid_id.object_id = NWN_OBJECT_INVALID;
        assert_eq!(
            quickbar_item_button_materialization_decision(
                &invalid_id,
                &QuickbarItemObject::default(),
                QuickbarItemSource::ExplicitTypeAndFragmentBits,
                false,
                None,
            ),
            Err(QuickbarItemMaterializationRejectReason::InvalidObjectId)
        );

        let mut missing_props = item.clone();
        missing_props.active_props = None;
        assert_eq!(
            quickbar_item_button_materialization_decision(
                &missing_props,
                &QuickbarItemObject::default(),
                QuickbarItemSource::ExplicitTypeAndFragmentBits,
                false,
                None,
            ),
            Err(QuickbarItemMaterializationRejectReason::MissingActiveProperties)
        );

        let mut unsupported_appearance = item.clone();
        unsupported_appearance.appearance_type = 9;
        assert_eq!(
            quickbar_item_button_materialization_decision(
                &unsupported_appearance,
                &QuickbarItemObject::default(),
                QuickbarItemSource::ExplicitTypeAndFragmentBits,
                false,
                None,
            ),
            Err(QuickbarItemMaterializationRejectReason::UnsupportedAppearanceType)
        );

        let mut shifted_appearance = item;
        shifted_appearance.appearance_bytes[0] ^= 1;
        assert_eq!(
            quickbar_item_button_materialization_decision(
                &shifted_appearance,
                &QuickbarItemObject::default(),
                QuickbarItemSource::ExplicitTypeAndFragmentBits,
                false,
                None,
            ),
            Err(QuickbarItemMaterializationRejectReason::AppearanceShape),
            "base-item/appearance cursor drift must stay distinguishable"
        );
    }

    fn ee_reader_for_payload(payload: &[u8]) -> QuickbarPacketReader<'_> {
        let declared = usize::try_from(
            read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES).expect("declared length"),
        )
        .expect("declared length should fit usize");
        let read_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
        let read_buffer = payload
            .get(read_start..declared)
            .expect("declared read buffer should be present");
        let fragments = payload
            .get(declared..)
            .expect("fragment bytes should be present");
        let mut reader = QuickbarPacketReader {
            read_buffer,
            fragments,
            cursor: 0,
            fragment_cursor: 0,
            fragment_bit: 0,
            final_fragment_bits: 0,
        };
        reader.final_fragment_bits = u8::try_from(reader.read_bits(3).expect("final bit header"))
            .expect("final bit header should fit byte");
        reader
    }

    fn skip_primary_armor_item_prefix(reader: &mut QuickbarPacketReader<'_>, armor_word: u16) {
        assert_eq!(reader.read_byte(), Some(1), "slot 0 is an item slot");
        assert_eq!(reader.read_bit(), Some(true), "primary item is present");
        assert_eq!(
            reader.read_dword(),
            Some(0x8000_0042),
            "quickbar item object id is EE-server marked"
        );
        assert_eq!(
            reader.read_i32(),
            Some(-1),
            "primary int param is preserved"
        );
        assert_eq!(reader.read_dword(), Some(LEGACY_ARMOR_BASE_ITEM));
        for part in 0..19u16 {
            assert_eq!(
                reader.read_word(),
                Some(0x20 + part),
                "EE reads model-type-3 armor parts as WORDs"
            );
        }
        reader
            .skip_bytes(6 + EE_QUICKBAR_ARMOR_LAYERED_COLOR_BYTES)
            .expect("legacy palette plus EE armor/accessory colors");
        assert_eq!(
            reader.read_i32(),
            Some(0),
            "first visual-transform map is empty"
        );
        assert_eq!(
            reader.read_i32(),
            Some(0),
            "second visual-transform map is empty"
        );
        assert_eq!(
            reader.read_word(),
            Some(armor_word),
            "armor active-property word precedes item-name bits"
        );
    }

    fn assert_common_active_property_tail(
        reader: &mut QuickbarPacketReader<'_>,
        active_props: &QuickbarActiveItemProperties,
    ) {
        assert_eq!(reader.read_bit(), Some(active_props.post_name_bool1));
        assert_eq!(reader.read_dword(), Some(active_props.cost));
        assert_eq!(reader.read_dword(), Some(active_props.stack_or_charges));
        assert_eq!(
            reader.read_bit(),
            Some(false),
            "EE-only CanUseItem bit is inserted after the shared pre-DWORD BOOL"
        );
        assert_eq!(reader.read_bit(), Some(active_props.post_name_bool2));
        assert_eq!(reader.read_bit(), Some(active_props.post_name_bool3));
        assert_eq!(reader.read_bit(), Some(active_props.post_name_bool4));
        assert_eq!(
            reader.read_byte(),
            Some(u8::try_from(active_props.properties.len()).unwrap())
        );
        for property in &active_props.properties {
            assert_eq!(reader.read_word(), Some(property.property));
            assert_eq!(reader.read_word(), Some(property.subtype));
            assert_eq!(reader.read_word(), Some(property.cost_table_value));
            assert_eq!(reader.read_byte(), Some(property.param));
        }
        assert_eq!(reader.read_byte(), Some(active_props.state_mask));
        assert_eq!(reader.read_byte(), Some(active_props.value_mask));
        for expected in &active_props.value_mask_bytes {
            assert_eq!(reader.read_byte(), Some(*expected));
        }
        assert_eq!(reader.read_bit(), Some(false), "secondary item is absent");
        for slot in 1..LEGACY_QUICKBAR_BUTTON_COUNT {
            assert_eq!(reader.read_byte(), Some(0), "slot {slot} stays blank");
        }
        assert_eq!(reader.cursor, reader.read_buffer.len());
    }

    fn flip_fragment_bit(payload: &mut [u8], bit_index: usize) {
        let declared = usize::try_from(
            read_u32_le(payload, HIGH_LEVEL_HEADER_BYTES).expect("declared length"),
        )
        .expect("declared length should fit usize");
        let byte_offset = declared + bit_index / 8;
        let mask = 0x80 >> (bit_index % 8);
        let byte = payload
            .get_mut(byte_offset)
            .expect("fragment bit should be present");
        *byte ^= mask;
    }

    fn first_model_part_high_byte_offset(payload: &[u8]) -> usize {
        let read_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
        assert_eq!(payload.get(read_start), Some(&1), "slot 0 is an item slot");
        let offset = read_start + 1 + CNW_LENGTH_BYTES + CNW_LENGTH_BYTES + CNW_LENGTH_BYTES + 1;
        assert!(
            offset < payload.len(),
            "payload should contain a widened model-part high byte"
        );
        offset
    }

    #[test]
    fn model_type_0_1_2_quickbar_appearances_zero_extend_legacy_bytes() {
        let cases = [
            (
                "model-type-0 shield",
                quickbar_item_with_appearance(LEGACY_SHIELD_BASE_ITEM, 0, &[0x34]),
                vec![0x34, 0x00],
            ),
            (
                "model-type-1 cloak",
                quickbar_item_with_appearance(LEGACY_CLOAK_BASE_ITEM, 1, &[0x45, 1, 2, 3, 4, 5, 6]),
                vec![0x45, 0x00, 1, 2, 3, 4, 5, 6],
            ),
            (
                "model-type-2 weapon",
                quickbar_item_with_appearance(
                    LEGACY_WEAPON_BASE_ITEM,
                    2,
                    &[0x07, 0x08, 0x09, 0x0A],
                ),
                vec![0x07, 0x00, 0x08, 0x00, 0x09, 0x00, 0x0A],
            ),
        ];

        for (label, item, expected_tail) in cases {
            let mut writer = QuickbarPacketWriter::new();
            write_ee_quickbar_item_appearance_from_legacy(&mut writer, &item)
                .unwrap_or_else(|| panic!("{label} should write"));
            assert_eq!(read_u32_le(&writer.read_buffer, 0), Some(item.base_item));
            assert_eq!(
                writer.read_buffer.get(CNW_LENGTH_BYTES..),
                Some(&expected_tail[..]),
                "{label} must zero-extend only Diamond's model BYTE fields"
            );

            let mut payload = quickbar_payload_with_primary_item(item);
            assert!(
                ee_set_all_buttons_payload_shape_valid(&payload),
                "{label} payload should satisfy the exact EE quickbar reader"
            );
            let high_byte = first_model_part_high_byte_offset(&payload);
            payload[high_byte] = 1;
            assert!(
                !ee_set_all_buttons_payload_shape_valid(&payload),
                "{label} validator must reject a nonzero high byte in a Diamond-widened model WORD"
            );
        }
    }

    #[test]
    fn model_type_3_quickbar_appearance_repeats_legacy_palette_per_ee_row() {
        let palette = [1, 2, 3, 4, 5, 6];
        let item = model_type_3_armor_item_with_palette(palette);
        let mut writer = QuickbarPacketWriter::new();

        write_ee_quickbar_item_appearance_from_legacy(&mut writer, &item)
            .expect("model-type-3 armor appearance should write");

        let bytes = writer.read_buffer;
        assert_eq!(read_u32_le(&bytes, 0), Some(LEGACY_ARMOR_BASE_ITEM));
        for part in 0..19usize {
            let offset = CNW_LENGTH_BYTES + part * 2;
            assert_eq!(
                bytes.get(offset..offset + 2),
                Some(&[0x20 + u8::try_from(part).unwrap(), 0][..]),
                "EE feature-0x23 branch reads model parts as zero-extended WORDs"
            );
        }

        let palette_start = CNW_LENGTH_BYTES + (19 * 2);
        let table_start = palette_start + palette.len();
        assert_eq!(
            bytes.get(palette_start..table_start),
            Some(&palette[..]),
            "the six Diamond armor palette bytes stay immediately before the EE table"
        );
        let table = bytes
            .get(table_start..table_start + EE_QUICKBAR_ARMOR_LAYERED_COLOR_BYTES)
            .expect("EE armor/accessory table should be present");
        assert_eq!(
            bytes.len(),
            table_start + EE_QUICKBAR_ARMOR_LAYERED_COLOR_BYTES
        );
        assert!(
            table.iter().any(|byte| *byte != 0),
            "the EE-only model-type-3 table must not be zero-filled when Diamond supplied palette bytes"
        );
        for chunk in table.chunks_exact(palette.len()) {
            assert_eq!(
                chunk, &palette,
                "each of the 19 EE armor/accessory rows inherits Diamond's six palette bytes"
            );
        }
    }

    #[test]
    fn model_type_3_quickbar_item_payload_still_validates_as_exact_ee_shape() {
        let item = model_type_3_armor_item_with_palette([6, 5, 4, 3, 2, 1]);
        let payload = quickbar_payload_with_primary_item(item);
        assert!(
            ee_set_all_buttons_payload_shape_valid(&payload),
            "palette-seeded model-type-3 item appearance must still satisfy the exact EE quickbar reader"
        );

        let mut shifted = payload;
        let high_byte = first_model_part_high_byte_offset(&shifted);
        shifted[high_byte] = 1;
        assert!(
            !ee_set_all_buttons_payload_shape_valid(&shifted),
            "validator must reject a nonzero high byte in a Diamond-widened model-type-3 WORD"
        );
    }

    #[test]
    fn active_property_direct_name_keeps_can_use_insert_after_pre_dword_bool() {
        let active_props = QuickbarActiveItemProperties {
            has_armor_word: true,
            armor_word: 0xBEEF,
            name_is_locstring: false,
            string_name: b"Moonblade".to_vec(),
            post_name_bool1: true,
            cost: 0x0102_0304,
            stack_or_charges: 0x0506_0708,
            post_name_bool2: false,
            post_name_bool3: true,
            post_name_bool4: false,
            properties: vec![QuickbarActivePropertyEntry {
                property: 0x1112,
                subtype: 0x2122,
                cost_table_value: 0x3132,
                param: 0x41,
            }],
            state_mask: 0xA5,
            value_mask: 0b1000_0001,
            value_mask_bytes: vec![0x51, 0x52],
            ..QuickbarActiveItemProperties::default()
        };
        let item =
            model_type_3_armor_item_with_active_props([9, 8, 7, 6, 5, 4], active_props.clone());
        let payload = quickbar_payload_with_primary_item(item);
        assert!(ee_set_all_buttons_payload_shape_valid(&payload));

        let mut reader = ee_reader_for_payload(&payload);
        skip_primary_armor_item_prefix(&mut reader, active_props.armor_word);
        assert_eq!(
            reader.read_bit(),
            Some(false),
            "direct CExoString item name consumes exactly one source-order BOOL"
        );
        assert_eq!(reader.read_string(), Some(active_props.string_name.clone()));
        assert_common_active_property_tail(&mut reader, &active_props);

        let mut shifted = payload.clone();
        flip_fragment_bit(&mut shifted, 6);
        assert!(
            !ee_set_all_buttons_payload_shape_valid(&shifted),
            "validator must reject a true EE-only CanUseItem bit instead of treating it as a shifted active-property field"
        );
    }

    #[test]
    fn active_property_locstring_token_name_keeps_inner_bits_before_active_state() {
        let active_props = QuickbarActiveItemProperties {
            has_armor_word: true,
            armor_word: 0xCAFE,
            name_is_locstring: true,
            locstring_name: QuickbarLocStringField {
                custom_tlk: true,
                language_id: 3,
                string_ref: 0x0100_75D6,
                text: Vec::new(),
            },
            post_name_bool1: false,
            cost: 0x1111_2222,
            stack_or_charges: 0x3333_4444,
            post_name_bool2: true,
            post_name_bool3: false,
            post_name_bool4: true,
            properties: Vec::new(),
            state_mask: 0x12,
            value_mask: 0b0000_0100,
            value_mask_bytes: vec![0x77],
            ..QuickbarActiveItemProperties::default()
        };
        let item =
            model_type_3_armor_item_with_active_props([1, 3, 5, 7, 9, 11], active_props.clone());
        let payload = quickbar_payload_with_primary_item(item);
        assert!(ee_set_all_buttons_payload_shape_valid(&payload));

        let mut reader = ee_reader_for_payload(&payload);
        skip_primary_armor_item_prefix(&mut reader, active_props.armor_word);
        assert_eq!(
            reader.read_bit(),
            Some(true),
            "locstring item name outer selector precedes active-property state"
        );
        assert_eq!(
            reader.read_bit(),
            Some(true),
            "custom-token locstring inner selector precedes language/string-ref bytes"
        );
        assert_eq!(
            reader.read_byte(),
            Some(active_props.locstring_name.language_id)
        );
        assert_eq!(
            reader.read_dword(),
            Some(active_props.locstring_name.string_ref)
        );
        assert_common_active_property_tail(&mut reader, &active_props);

        let mut shifted = payload.clone();
        flip_fragment_bit(&mut shifted, 7);
        assert!(
            !ee_set_all_buttons_payload_shape_valid(&shifted),
            "validator must reject a true EE-only CanUseItem bit after token-name bits"
        );
    }

    #[test]
    fn active_property_locstring_inline_name_keeps_inner_bit_before_active_state() {
        let active_props = QuickbarActiveItemProperties {
            has_armor_word: true,
            armor_word: 0x1234,
            name_is_locstring: true,
            locstring_name: QuickbarLocStringField {
                custom_tlk: false,
                language_id: 0,
                string_ref: 0,
                text: b"Etched Blade".to_vec(),
            },
            post_name_bool1: true,
            cost: 0x2122_2324,
            stack_or_charges: 0x3132_3334,
            post_name_bool2: false,
            post_name_bool3: false,
            post_name_bool4: true,
            properties: Vec::new(),
            state_mask: 0x44,
            value_mask: 0,
            value_mask_bytes: Vec::new(),
            ..QuickbarActiveItemProperties::default()
        };
        let item =
            model_type_3_armor_item_with_active_props([4, 5, 6, 7, 8, 9], active_props.clone());
        let payload = quickbar_payload_with_primary_item(item);
        assert!(ee_set_all_buttons_payload_shape_valid(&payload));

        let mut reader = ee_reader_for_payload(&payload);
        skip_primary_armor_item_prefix(&mut reader, active_props.armor_word);
        assert_eq!(
            reader.read_bit(),
            Some(true),
            "locstring item name outer selector precedes the inline-name branch"
        );
        assert_eq!(
            reader.read_bit(),
            Some(false),
            "inline locstring inner selector precedes its CExoString bytes"
        );
        assert_eq!(
            reader.read_string(),
            Some(active_props.locstring_name.text.clone())
        );
        assert_common_active_property_tail(&mut reader, &active_props);

        let mut shifted = payload.clone();
        flip_fragment_bit(&mut shifted, 7);
        assert!(
            !ee_set_all_buttons_payload_shape_valid(&shifted),
            "validator must reject a true EE-only CanUseItem bit after inline locstring-name bits"
        );
    }
}
