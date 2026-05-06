//! Small BN counted-string helpers.
//!
//! These helpers describe the legacy BN wire convention only. They deliberately
//! do not know which packet is being rewritten; tag-specific modules decide how
//! each counted field is interpreted.

pub(super) fn read_counted_bytes<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    if *cursor >= bytes.len() {
        return None;
    }
    let len = bytes[*cursor] as usize;
    *cursor += 1;
    if *cursor + len > bytes.len() {
        return None;
    }
    let segment = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Some(segment)
}

pub(super) fn read_counted_segment<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a str> {
    let segment = read_counted_bytes(bytes, cursor)?;
    std::str::from_utf8(segment).ok()
}

pub(super) fn append_counted_segment(out: &mut Vec<u8>, value: &str) {
    out.push(value.len() as u8);
    out.extend_from_slice(value.as_bytes());
}

pub(super) fn legacy_segment(value: &str) -> &str {
    if value.len() <= 255 {
        return value;
    }

    let mut end = 255;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
