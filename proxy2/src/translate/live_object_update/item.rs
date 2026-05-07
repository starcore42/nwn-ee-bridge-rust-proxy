//! Item-family live-object boundary helpers.
//!
//! These routines classify legacy item sentinels only; they do not rewrite item
//! records. That keeps item parsing policy out of the generic boundary walker.

pub(super) fn is_known_legacy_item_marker(marker: u8) -> bool {
    matches!(marker, 0x05 | 0xC5)
}

pub(super) fn is_legacy_item_sentinel(bytes: &[u8], offset: usize) -> bool {
    bytes.get(offset + 1) == Some(&0xFD)
        && bytes.get(offset + 2) == Some(&0xFF)
        && bytes.get(offset + 3) == Some(&0xFF)
        && bytes.get(offset + 4) == Some(&0xFF)
}
