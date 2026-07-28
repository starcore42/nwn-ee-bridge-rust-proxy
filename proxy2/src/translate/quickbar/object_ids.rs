use super::{EE_SERVER_OBJECT_ID_MARKER_BIT, NWN_OBJECT_INVALID};

/// Return the OBJECTID value that the EE quickbar receiver sees on the wire.
///
/// Diamond quickbar bodies may carry a compact server object id. The existing
/// EE writer contract marks that id as external before `sub_14079DB00` hands
/// the item to `CGameObjectArray::AddExternalObject`. Keep the same value in
/// committed slot signatures and semantic registry lookups so later `G Q`
/// rows address the object that EE actually registered.
///
/// Exact wire proof: Diamond server writer `0x508CB0` compares the stock
/// invalid sentinel, ORs the external marker at `0x508CD5`, stores one
/// little-endian DWORD at `0x508CE1`, then advances both byte cursors by four
/// through `0x508CF3`. EE `sub_14079DB00` reads the primary OBJECTID at
/// `0x14079DD34` and the secondary OBJECTID at `0x14079DEA5`, after their
/// respective BOOL guards. OBJECTID itself owns no fragment bit and performs
/// no fragment-cursor reset or alignment.
///
/// `0x7F00_0000` is the stock NWN invalid-object sentinel and must not be
/// converted into an apparently external id. Values that already carry the EE
/// marker (including the all-ones invalid value) are already in wire form.
pub(crate) fn ee_quickbar_object_id_wire_value(object_id: u32) -> u32 {
    if object_id == NWN_OBJECT_INVALID || (object_id & EE_SERVER_OBJECT_ID_MARKER_BIT) != 0 {
        object_id
    } else {
        object_id | EE_SERVER_OBJECT_ID_MARKER_BIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ee_quickbar_object_ids_mark_compact_values_and_preserve_sentinels() {
        assert_eq!(ee_quickbar_object_id_wire_value(0x0000_0042), 0x8000_0042);
        assert_eq!(ee_quickbar_object_id_wire_value(0x8000_0042), 0x8000_0042);
        assert_eq!(
            ee_quickbar_object_id_wire_value(NWN_OBJECT_INVALID),
            NWN_OBJECT_INVALID
        );
        assert_eq!(ee_quickbar_object_id_wire_value(u32::MAX), u32::MAX);
    }
}
