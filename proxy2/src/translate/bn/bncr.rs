//! `BNCR` verifier-challenge observation.
//!
//! Question answered here:
//! "Given a verified legacy server `BNCR`, what session state do later BN
//! translators need?"

use super::{SessionState, wire::read_counted_bytes};

pub(super) fn observe(bytes: &[u8], state: &mut SessionState) -> anyhow::Result<()> {
    if bytes.len() < 7 {
        return Ok(());
    }

    let status = bytes[6];
    if status != b'V' && status != b'P' {
        state.clear_bncr_challenge();
        return Ok(());
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

    state.remember_bncr_challenge(status, cd_key_challenge);

    tracing::info!(
        status = %char::from(status),
        cd_challenge_len = cd_key_challenge.len(),
        mst_challenge_len = mst_challenge.len(),
        trailing = bytes.len().saturating_sub(cursor),
        "server BNCR challenge captured"
    );

    Ok(())
}
