//! `BNES` server-enumerate request no-op claim.
//!
//! The packet-alignment reference records the Diamond dispatcher route for
//! `BNES` on the server-mode side and the EE sender
//! `SendBNESDirectMessageToAddress`. The verified direct-control shape is a
//! fixed seven-byte datagram, and there is no dialect field to rewrite for the
//! 1.69 server. This is still an explicit translator claim, not generic BN
//! pass-through.

pub(super) struct ProxyEnumerateResponse<'a> {
    pub server_port: u16,
    pub section: u8,
    pub session_name: &'a str,
}

pub(super) fn claim_client_to_legacy_if_verified(bytes: &[u8]) -> Option<()> {
    (bytes.get(..4)? == b"BNES" && bytes.len() == 7).then_some(())
}

pub(super) fn build_proxy_owned_bner_response(
    response: ProxyEnumerateResponse<'_>,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        response.section < 6,
        "BNER section must satisfy EE HandleBNERMessage section < 6"
    );
    anyhow::ensure!(
        response.session_name.len() <= u8::MAX as usize,
        "BNER session name exceeds one-byte length"
    );

    let mut bytes = Vec::with_capacity(9 + response.session_name.len());
    bytes.extend_from_slice(b"BNER");
    // Observed Diamond/HG BNER replies carry an unknown non-reader byte followed
    // by the little-endian server UDP port before EE's section/name cursor. EE
    // `HandleBNERMessage` validates from byte 7 onward, but preserving the
    // observed prefix keeps the proxy-owned discovery row close to real server
    // traffic while still deriving the port from configuration.
    bytes.push(0x55);
    bytes.extend_from_slice(&response.server_port.to_le_bytes());
    bytes.push(response.section);
    bytes.push(response.session_name.len() as u8);
    bytes.extend_from_slice(response.session_name.as_bytes());
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_exact_proxy_owned_bner_shape() {
        let packet = build_proxy_owned_bner_response(ProxyEnumerateResponse {
            server_port: 5133,
            section: 0,
            session_name: "Higher Ground (Party 2-3)",
        })
        .expect("valid BNER response");

        assert_eq!(&packet[..9], b"BNER\x55\x0D\x14\x00\x19");
        assert_eq!(&packet[9..], b"Higher Ground (Party 2-3)");
        assert_eq!(
            claim_client_to_legacy_if_verified(b"BNES\x01\x14\x00"),
            Some(())
        );
    }
}
