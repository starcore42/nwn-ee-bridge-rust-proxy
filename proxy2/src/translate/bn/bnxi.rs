//! `BNXI` extended-info request no-op claim.
//!
//! EE `RequestExtendedServerInfo` serializes `BNXI`, a UDP port, three counted
//! strings, a four-byte build header whose fourth byte is the build-number
//! length, and three more counted build strings. The Diamond dispatcher routes
//! `BNXI` on the server-mode side, so this exact cursor walk is a verified
//! identity translation for the legacy server.

pub(super) fn claim_client_to_legacy_if_verified(bytes: &[u8]) -> Option<()> {
    if bytes.get(..4)? != b"BNXI" || bytes.len() < 16 {
        return None;
    }

    let mut cursor = 6usize;
    for _ in 0..3 {
        consume_counted(bytes, &mut cursor)?;
    }

    if cursor.checked_add(4)? > bytes.len() {
        return None;
    }
    let build_number_len = bytes[cursor + 3] as usize;
    cursor += 4;
    cursor = cursor.checked_add(build_number_len)?;
    if cursor > bytes.len() {
        return None;
    }

    for _ in 0..3 {
        consume_counted(bytes, &mut cursor)?;
    }

    (cursor == bytes.len()).then_some(())
}

fn consume_counted(bytes: &[u8], cursor: &mut usize) -> Option<()> {
    let len = *bytes.get(*cursor)? as usize;
    *cursor = (*cursor).checked_add(1)?;
    *cursor = (*cursor).checked_add(len)?;
    (*cursor <= bytes.len()).then_some(())
}
