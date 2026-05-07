//! `BNCR` verifier-challenge no-op claim and observation.
//!
//! Question answered here:
//! "Given a verified legacy server `BNCR`, what session state do later BN
//! translators need, and is the same datagram valid for EE?"
//!
//! EE `HandleBNCRMessage` requires the status at offset `0x06` and accepts
//! `R`, `P`, or `V`. Diamond's writer includes the two-byte port field before
//! that status, then writes either a reject detail byte or counted challenge
//! strings. Those cursor rules are identical for the HG legacy verifier flow,
//! so this module claims the packet unchanged only after exact parsing.

use super::{SessionState, wire::read_counted_bytes};

pub(super) fn claim_server_to_ee_if_verified(
    bytes: &[u8],
    state: &mut SessionState,
) -> anyhow::Result<Option<()>> {
    if bytes.get(..4) != Some(b"BNCR") || bytes.len() < 8 {
        return Ok(None);
    }

    let status = bytes[6];
    if status != b'V' && status != b'P' {
        if status == b'R' && bytes.len() == 8 {
            state.clear_bncr_challenge();
            tracing::info!(
                detail = bytes[7],
                "server BNCR reject result parsed for EE client"
            );
            return Ok(Some(()));
        }
        state.clear_bncr_challenge();
        return Ok(None);
    }

    let mut cursor = 7;
    if status == b'P' {
        read_counted_bytes(bytes, &mut cursor)
            .ok_or_else(|| anyhow::anyhow!("BNCR game-password challenge overflow"))?;
    }
    let cd_key_challenge = read_counted_bytes(bytes, &mut cursor)
        .ok_or_else(|| anyhow::anyhow!("BNCR CD-key challenge overflow"))?;
    let mst_challenge = read_counted_bytes(bytes, &mut cursor)
        .ok_or_else(|| anyhow::anyhow!("BNCR master-server challenge overflow"))?;
    if cursor != bytes.len() {
        state.clear_bncr_challenge();
        return Ok(None);
    }

    state.remember_bncr_challenge(status, cd_key_challenge);

    tracing::info!(
        status = %char::from(status),
        cd_challenge_len = cd_key_challenge.len(),
        mst_challenge_len = mst_challenge.len(),
        "server BNCR challenge captured"
    );

    Ok(Some(()))
}
