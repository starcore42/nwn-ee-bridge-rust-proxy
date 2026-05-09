use std::sync::OnceLock;

pub(super) const HIGH_LEVEL_HEADER_BYTES: usize = 3;
pub(super) const CNW_LENGTH_BYTES: usize = 4;
pub(super) const LEGACY_PREFIXED_FRAGMENT_BYTES: usize = 4;
pub(super) const QUICKBAR_MAJOR: u8 = 0x1E;
pub(super) const SET_ALL_BUTTONS_MINOR: u8 = 0x01;
pub(super) const LEGACY_QUICKBAR_BUTTON_COUNT: usize = 36;
pub(super) const LEGACY_QUICKBAR_READ_CURSOR_START: usize = 0;
pub(super) const C_RESREF_TEXT_BYTES: usize = 16;
pub(super) const MAX_REASONABLE_QUICKBAR_STRING_BYTES: usize = 4096;
pub(super) const MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES: usize = 32 * 1024;
pub(super) const MAX_REASONABLE_QUICKBAR_ITEM_PROPERTIES: u8 = 128;
pub(super) const MAX_QUICKBAR_BARE_ACTIVE_ITEM_NAME_BYTES: usize = 128;
pub(super) const MAX_QUICKBAR_FOUR_PREFIX_FRAGMENT_TAIL_BYTES: usize = 512;
pub(super) const QUICKBAR_BAD_SCORE: i32 = -1_000_000;
pub(super) const QUICKBAR_UNKNOWN_SCORE: i32 = i32::MIN;
pub(super) const EE_SERVER_OBJECT_ID_MARKER_BIT: u32 = 0x8000_0000;
pub(super) const NWN_OBJECT_INVALID: u32 = 0x7F00_0000;
pub(super) const EE_QUICKBAR_ANIMATION_ICON_COUNT: u32 = 23;
pub(super) const NWN_BASE_ITEM_ARMOR: u32 = 0x10;
pub(super) const EE_QUICKBAR_ARMOR_LAYERED_COLOR_BYTES: usize = 19 * 6;
pub(super) const EE_QUICKBAR_LEGACY_VISUAL_TRANSFORM_IDENTITY_BYTES: [u8; 40] = [
    0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F,
];
pub(super) const BASEITEMS_2DA_NAME: &str = "baseitems.2da";
pub(super) const HG_REQUIRED_FILES_DIR: &str = "HG REQUIRED FILES";

pub(super) static QUICKBAR_BASE_ITEM_MODEL_TYPES: OnceLock<Option<Vec<i8>>> = OnceLock::new();
