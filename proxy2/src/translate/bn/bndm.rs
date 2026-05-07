//! `BNDM` EE direct-disconnect translation.
//!
//! EE `CNetLayerInternal::SendBNDMMessage` emits exactly four bytes:
//! `BNDM`. The legacy/Diamond disconnect datagram is `BNDS` followed by the
//! UDP port captured from the session's `BNCS`. Keeping this as a focused BN
//! module prevents EE direct-control cleanup from becoming a permissive
//! top-level passthrough rule.

use super::SessionState;

pub(super) fn rewrite_client_to_legacy_bnds(
    bytes: &[u8],
    state: &SessionState,
) -> anyhow::Result<Vec<u8>> {
    if bytes != b"BNDM" {
        anyhow::bail!("BNDM disconnect has invalid length/shape: {}", bytes.len());
    }
    let udp_port = state
        .latest_bncs_udp_port()
        .ok_or_else(|| anyhow::anyhow!("BNDM disconnect before BNCS UDP port was captured"))?;

    let mut rewritten = Vec::with_capacity(6);
    rewritten.extend_from_slice(b"BNDS");
    rewritten.extend_from_slice(&udp_port.to_le_bytes());

    tracing::info!(
        udp_port,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        "client BNDM disconnect translated to legacy BNDS"
    );

    Ok(rewritten)
}
