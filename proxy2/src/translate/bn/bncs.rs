//! `BNCS` connect-start translation.
//!
//! Question answered here:
//! "Given a stock EE `BNCS`, what exact Diamond/1.69 `BNCS` do we emit?"

use crate::{
    crc::read_le_u32,
    identity::{DiamondIdentity, looks_like_public_cdkey},
};

use super::wire::{append_counted_segment, legacy_segment, read_counted_segment};

pub(super) fn rewrite_client_to_diamond(
    bytes: &[u8],
    identity: &DiamondIdentity,
    bncs_private_build: u32,
    bncs_build_field: u16,
) -> anyhow::Result<Vec<u8>> {
    if bytes.len() < 18 {
        anyhow::bail!("BNCS too short: {}", bytes.len());
    }

    let udp_port = u16::from_le_bytes([bytes[4], bytes[5]]);
    let connection_type = bytes[6];
    let auth_mode = bytes[13];
    let input_build =
        read_le_u32(bytes, 7).ok_or_else(|| anyhow::anyhow!("BNCS missing build range"))?;
    let input_field = u16::from_le_bytes([bytes[11], bytes[12]]);
    let challenge =
        read_le_u32(bytes, 14).ok_or_else(|| anyhow::anyhow!("BNCS missing challenge range"))?;

    let mut cursor = 18;
    let player = read_counted_segment(bytes, &mut cursor)
        .ok_or_else(|| anyhow::anyhow!("BNCS missing player segment"))?;
    let public_segment_offset = cursor;
    let original_public =
        read_counted_segment(bytes, &mut cursor).filter(|value| looks_like_public_cdkey(value));
    let public_key = identity
        .primary_public_key()
        .or(original_public)
        .ok_or_else(|| anyhow::anyhow!("BNCS missing usable public CD key"))?;

    let player = legacy_segment(player);
    let public_key = legacy_segment(public_key);
    if player.is_empty() || public_key.is_empty() {
        anyhow::bail!("BNCS empty Diamond player/public segment");
    }

    let mut rewritten = Vec::with_capacity(20 + player.len() + public_key.len());
    rewritten.extend_from_slice(b"BNCS");
    rewritten.extend_from_slice(&udp_port.to_le_bytes());
    rewritten.push(connection_type);
    rewritten.extend_from_slice(&bncs_private_build.to_le_bytes());
    rewritten.extend_from_slice(&bncs_build_field.to_le_bytes());
    rewritten.push(auth_mode);
    rewritten.extend_from_slice(&challenge.to_le_bytes());
    append_counted_segment(&mut rewritten, player);
    append_counted_segment(&mut rewritten, public_key);

    tracing::info!(
        old_len = bytes.len(),
        new_len = rewritten.len(),
        udp_port,
        connection_type,
        auth_mode,
        input_build,
        input_field,
        private_build = bncs_private_build,
        build_field = bncs_build_field,
        challenge,
        player_len = player.len(),
        public_key,
        forced_public_key = identity.primary_public_key().is_some(),
        public_segment_offset,
        raw_cdkey_count = identity.cd_keys.len(),
        cdkey_source = identity
            .source
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        "client BNCS rewritten to Diamond layout"
    );

    Ok(rewritten)
}
