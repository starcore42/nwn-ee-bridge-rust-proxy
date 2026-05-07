//! Inventory-family live-object update policy.
//!
//! Inventory and GUI item-create submessages can own fragment BOOLs. Until the
//! exact family parsers are implemented, the live-object updater treats them as
//! tail owners so shared fragment bits are preserved rather than trimmed.

pub(super) fn owns_fragment_tail(opcode: u8) -> bool {
    matches!(opcode, b'I' | b'G')
}
