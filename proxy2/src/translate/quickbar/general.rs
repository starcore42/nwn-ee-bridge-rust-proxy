use super::*;

// Decompile-backed general quickbar slot ownership.
//
// The quickbar writer used to blank every non-item/non-spell record. That was
// strict, but too lossy: EE's `CNWSMessage::SendServerToPlayerGuiQuickbar_SetButton`
// and Diamond's `sub_469FD0` use the same wire shape for many general button
// records. This module is the narrow allow-list for those byte-identical
// shapes. Divergent or unowned records are still consumed and emitted as blank
// slots by the writer instead of being forwarded raw.

pub(super) fn quickbar_general_bytes_are_verified_ee_identical(bytes: &[u8]) -> bool {
    let Some(&ty) = bytes.first() else {
        return false;
    };

    match ty {
        ty if verified_no_payload_general_type(ty) => bytes.len() == 1,
        ty if legacy_quickbar_type_has_int_payload(ty) => {
            bytes.len() == 1 + CNW_LENGTH_BYTES
                && read_u32_le(bytes, 1)
                    .is_some_and(|value| legacy_quickbar_int_payload_is_valid_for_ee(ty, value))
        }
        44 => bytes.len() == 1 + CNW_LENGTH_BYTES + 1,
        11..=17 => c_resref_string_general_len(bytes).is_some_and(|len| len == bytes.len()),
        18 => two_string_general_len(bytes).is_some_and(|len| len == bytes.len()),
        29 | 30 => bytes.len() == 1 + C_RESREF_TEXT_BYTES,
        _ => false,
    }
}

pub(super) fn validate_ee_quickbar_general_button(
    reader: &mut QuickbarPacketReader<'_>,
    ty: u8,
) -> bool {
    match ty {
        ty if verified_no_payload_general_type(ty) => true,
        ty if legacy_quickbar_type_has_int_payload(ty) => reader
            .read_dword()
            .is_some_and(|value| legacy_quickbar_int_payload_is_valid_for_ee(ty, value)),
        44 => reader.read_dword().is_some() && reader.read_byte().is_some(),
        11..=17 => reader.skip_bytes(C_RESREF_TEXT_BYTES).is_some() && reader.skip_string().is_some(),
        18 => reader.skip_string().is_some() && reader.skip_string().is_some(),
        29 | 30 => reader.skip_bytes(C_RESREF_TEXT_BYTES).is_some(),
        _ => false,
    }
}

fn verified_no_payload_general_type(ty: u8) -> bool {
    matches!(
        ty,
        // EE sender default/no-extra-read cases match Diamond's receiver
        // default/no-extra-read cases for these slot types. Legacy type 9 is
        // intentionally excluded: Diamond treats it as a one-byte general
        // record, but EE's sender has a type-9 item-bearing branch.
        0 | 5 | 6 | 7 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 35 | 36 | 38 | 40 | 41
    )
}

fn c_resref_string_general_len(bytes: &[u8]) -> Option<usize> {
    let string_len_offset = 1usize.checked_add(C_RESREF_TEXT_BYTES)?;
    let string_len = usize::try_from(read_u32_le(bytes, string_len_offset)?).ok()?;
    if string_len > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
        return None;
    }
    string_len_offset
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(string_len)
}

fn two_string_general_len(bytes: &[u8]) -> Option<usize> {
    let first_len = usize::try_from(read_u32_le(bytes, 1)?).ok()?;
    if first_len > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
        return None;
    }
    let second_len_offset = 1usize
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(first_len)?;
    let second_len = usize::try_from(read_u32_le(bytes, second_len_offset)?).ok()?;
    if second_len > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
        return None;
    }
    second_len_offset
        .checked_add(CNW_LENGTH_BYTES)?
        .checked_add(second_len)
}
