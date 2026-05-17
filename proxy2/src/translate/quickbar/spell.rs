//! Spell-slot policy for `GuiQuickbar_SetAllButtons`.
//!
//! EE and Diamond read type-2 quickbar spell slots with the same cursor shape:
//! slot type, class byte, spell-id DWORD, byte, byte. The setters are not byte
//! sinks, so the bridge still treats the two trailing bytes as typed fields:
//! `metamagic, level`.
//!
//! Decompile evidence:
//! - Diamond `sub_469FD0` reads BYTE/DWORD/BYTE/BYTE, then `sub_520FB0` packs
//!   arg_10 as metamagic bits and arg_C as the level nibble.
//! - EE `sub_14079DB00` reads the same BYTE/DWORD/BYTE/BYTE. Its caller stores
//!   the first post-DWORD byte in the stack slot consumed by `sub_14086B680` as
//!   `arg_28`; `sub_140868E70` later extracts metamagic from `action >> 31`.
//!   The second post-DWORD byte is consumed as `arg_20`, whose low nibble is
//!   extracted as the spell level.
//!
//! The EE/Diamond wire order is therefore identical for this subobject. The
//! translator still owns the semantic fields and bounds them before emission
//! so a future layout change cannot silently become raw passthrough.
//!
//! The normal translator therefore preserves spell slots only through this
//! focused policy seam and rewrites the typed legacy tuple into EE semantic
//! order. The diagnostic switch below is intentionally explicit and disabled
//! by default; it lets harness runs distinguish "spell/resource semantic issue"
//! from "M-frame/zlib/window issue" without creating a hidden fallback.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EeSpellButtonWire {
    pub(super) class_byte: u8,
    pub(super) spell_id: u32,
    pub(super) ee_metamagic: u8,
    pub(super) ee_level: u8,
}

pub(super) fn quickbar_spell_slot_has_verified_ee_materialization(
    class_byte: u8,
    spell_id: u32,
    legacy_metamagic: u8,
    legacy_level: u8,
) -> bool {
    legacy_spell_tuple_to_ee_wire(class_byte, spell_id, legacy_metamagic, legacy_level).is_some()
        && !diagnostic_blank_server_quickbar_spells()
}

pub(super) fn legacy_spell_tuple_to_ee_wire(
    class_byte: u8,
    spell_id: u32,
    legacy_metamagic: u8,
    legacy_level: u8,
) -> Option<EeSpellButtonWire> {
    if class_byte > 3 || spell_id > 0xFFFF || legacy_metamagic > 0x3F || legacy_level > 0x0F {
        return None;
    }
    Some(EeSpellButtonWire {
        class_byte,
        spell_id,
        ee_metamagic: legacy_metamagic,
        ee_level: legacy_level,
    })
}

fn diagnostic_blank_server_quickbar_spells() -> bool {
    std::env::var("NWN_BRIDGE_DIAGNOSTIC_BLANK_SERVER_QUICKBAR_SPELLS")
        .map(|value| {
            let value = value.trim();
            value == "1"
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::legacy_spell_tuple_to_ee_wire;

    #[test]
    fn preserves_decompile_confirmed_spell_wire_order() {
        let ee = legacy_spell_tuple_to_ee_wire(0, 2282, 2, 0).unwrap();
        assert_eq!(ee.class_byte, 0);
        assert_eq!(ee.spell_id, 2282);
        assert_eq!(ee.ee_metamagic, 2);
        assert_eq!(ee.ee_level, 0);
    }

    #[test]
    fn rejects_spell_values_outside_decompiled_bit_fields() {
        assert!(legacy_spell_tuple_to_ee_wire(4, 2282, 0, 0).is_none());
        assert!(legacy_spell_tuple_to_ee_wire(0, 0x1_0000, 0, 0).is_none());
        assert!(legacy_spell_tuple_to_ee_wire(0, 2282, 0x40, 0).is_none());
        assert!(legacy_spell_tuple_to_ee_wire(0, 2282, 0, 0x10).is_none());
    }
}
