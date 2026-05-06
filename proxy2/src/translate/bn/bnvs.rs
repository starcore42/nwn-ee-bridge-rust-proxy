//! `BNVS` verifier-response translation.
//!
//! Question answered here:
//! "Given EE's one-key verifier response and the captured `BNCR` challenge,
//! what Diamond/HG three-key verifier list do we emit?"

use crate::identity::DiamondIdentity;

use super::{
    SessionState,
    wire::{append_counted_segment, read_counted_bytes},
};

pub(super) fn rewrite_client_to_diamond(
    bytes: &[u8],
    identity: &DiamondIdentity,
    state: &SessionState,
) -> anyhow::Result<Vec<u8>> {
    if bytes.len() < 6 {
        anyhow::bail!("BNVS too short: {}", bytes.len());
    }
    if state.latest_cd_key_challenge().is_empty() {
        anyhow::bail!("BNVS cannot be rewritten before BNCR challenge is captured");
    }

    let client_status = bytes[4];
    let verifier_count = bytes[5] as usize;
    let mut cursor = 6;
    for _ in 0..verifier_count {
        read_counted_bytes(bytes, &mut cursor)
            .ok_or_else(|| anyhow::anyhow!("BNVS verifier segment overflow"))?;
    }

    let response_tail_start = cursor;
    read_counted_bytes(bytes, &mut cursor)
        .ok_or_else(|| anyhow::anyhow!("BNVS missing mandatory response segment"))?;
    if client_status == b'P' {
        read_counted_bytes(bytes, &mut cursor)
            .ok_or_else(|| anyhow::anyhow!("BNVS missing password response segment"))?;
    }
    if cursor != bytes.len() {
        anyhow::bail!("BNVS trailing bytes after response segments");
    }

    let verifiers = identity.legacy_cdkey_verifiers(state.latest_cd_key_challenge())?;
    let rewritten_count = verifiers.len();
    if rewritten_count > u8::MAX as usize {
        anyhow::bail!("too many Diamond CD key verifiers loaded");
    }

    let status = state.latest_bncr_status().unwrap_or(client_status);
    let mut rewritten = Vec::with_capacity(bytes.len() + rewritten_count * 41);
    rewritten.extend_from_slice(b"BNVS");
    rewritten.push(status);
    rewritten.push(rewritten_count as u8);
    for verifier in &verifiers {
        append_counted_segment(&mut rewritten, verifier);
    }
    rewritten.extend_from_slice(&bytes[response_tail_start..]);

    tracing::info!(
        old_len = bytes.len(),
        new_len = rewritten.len(),
        old_count = verifier_count,
        new_count = rewritten_count,
        status = %char::from(status),
        challenge_len = state.latest_cd_key_challenge().len(),
        response_tail_len = bytes.len() - response_tail_start,
        "client BNVS rewritten to Diamond verifier layout"
    );

    Ok(rewritten)
}
