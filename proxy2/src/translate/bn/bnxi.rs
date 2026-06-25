//! `BNXI` extended-info request claim.
//!
//! EE `RequestExtendedServerInfo` serializes `BNXI`, a UDP port, three counted
//! strings, a four-byte build header whose fourth byte is the build-number
//! length, and three more counted build strings. Diamond can also send a
//! six-byte `BNXI` probe containing only the tag and UDP port; live HG traffic
//! captured on 2026-06-25 showed `42 4E 58 49 00 14`. The Diamond dispatcher
//! routes `BNXI` on the server-mode side, so both forms are explicit verified
//! request parsers rather than generic BN pass-through.
//!
//! The proxy answers the full EE form with a proxy-owned `BNXR` discovery
//! response from module/profile state instead of relying on the legacy server
//! to answer an EE pre-connect discovery request. The legacy six-byte form is
//! forwarded to the legacy server and deliberately does not prove an EE client
//! build.
//!
//! The same packet is also our earliest decompile-backed proof of the EE
//! client's own protocol build. Later, `BNVR` advertises the proxy-owned
//! EE-facing server dialect build, and this captured client build proves that
//! the connected client can safely consume that dialect.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClientBuild {
    pub major: u32,
    pub minor: u32,
    pub revision: u32,
}

impl ClientBuild {
    pub fn satisfies(self, major: u32, minor: u32, revision: u32) -> bool {
        (self.major, self.minor, self.revision) >= (major, minor, revision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClientRequest {
    LegacyProbe { udp_port: u16 },
    EeExtended { udp_port: u16, build: ClientBuild },
}

pub(super) fn claim_client_to_legacy_if_verified(bytes: &[u8]) -> Option<ClientRequest> {
    if bytes.get(..4)? != b"BNXI" {
        return None;
    }

    let udp_port = u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]);
    if bytes.len() == 6 {
        return Some(ClientRequest::LegacyProbe { udp_port });
    }

    if bytes.len() < 16 {
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
    let build_number = bytes.get(cursor..cursor.checked_add(build_number_len)?)?;
    cursor = cursor.checked_add(build_number_len)?;
    if cursor > bytes.len() {
        return None;
    }

    let minor = consume_counted(bytes, &mut cursor)?;
    let revision = consume_counted(bytes, &mut cursor)?;
    let _build_hash = consume_counted(bytes, &mut cursor)?;

    let build = ClientBuild {
        major: parse_ascii_u32(build_number)?,
        minor: parse_ascii_u32(minor)?,
        revision: parse_ascii_u32(revision)?,
    };

    (cursor == bytes.len()).then_some(ClientRequest::EeExtended { udp_port, build })
}

fn consume_counted<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let len = *bytes.get(*cursor)? as usize;
    *cursor = (*cursor).checked_add(1)?;
    let start = *cursor;
    *cursor = (*cursor).checked_add(len)?;
    bytes.get(start..*cursor)
}

fn parse_ascii_u32(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ee_build_from_observed_bnxi_shape() {
        let packet = [
            b'B', b'N', b'X', b'I', 0x69, 0xC9, 0, 0, 0, 0, 0, 2, 4, b'8', b'1', b'9', b'3', 2,
            b'3', b'7', 2, b'1', b'7', 8, b'2', b'6', b'c', b'6', b'e', b'5', b'7', b'3',
        ];

        let request = claim_client_to_legacy_if_verified(&packet).expect("BNXI request");
        let ClientRequest::EeExtended { udp_port, build } = request else {
            panic!("full BNXI must parse as EE extended request");
        };

        assert_eq!(udp_port, 0xC969);
        assert_eq!(build.major, 8193);
        assert_eq!(build.minor, 37);
        assert_eq!(build.revision, 17);
        assert!(build.satisfies(8193, 35, 0));
    }

    #[test]
    fn parses_legacy_six_byte_probe_without_build_proof() {
        let request =
            claim_client_to_legacy_if_verified(b"BNXI\x00\x14").expect("legacy BNXI probe");

        assert_eq!(request, ClientRequest::LegacyProbe { udp_port: 0x1400 });
    }
}
