//! `BNVR` verifier-result translation.
//!
//! Diamond `BNVR` writer/parser and EE `HandleBNVRMessage` agree on the
//! reject form (`BNVR`, `R`, reason). Accept is more subtle: EE accepts the
//! legacy nine-byte form, but the decompiled EE accept handler only fills the
//! server build fields when the packet is at least 21 bytes:
//!
//! ```text
//! offset  5: DWORD sliding-window value
//! offset  9: DWORD server major build
//! offset 13: DWORD server minor build
//! offset 17: DWORD server revision
//! ```
//!
//! Modern gameplay readers call `CNetLayer::ServerSatisfiesBuild` to decide
//! which packet dialect the server is writing. The bridge must therefore
//! advertise the EE-facing server dialect it actually emits, not the raw EE
//! client build. At the moment that dialect intentionally satisfies the
//! decompiled build-35 visual-transform/static-area gates, but not the later
//! build-36.3 `CNWCArea::LoadArea` grass-info gate. Advertising the client build
//! made the client read legacy tile bytes as grass rows, causing cursor
//! overflow before the tile loop.

use super::bnxi::ClientBuild;

const EE_FACING_SERVER_DIALECT_BUILD_MAJOR: u32 = 0x2001;
const EE_FACING_SERVER_DIALECT_BUILD_MINOR: u32 = 0x23;
const EE_FACING_SERVER_DIALECT_BUILD_REVISION: u32 = 0;

pub(super) fn rewrite_server_to_ee_if_verified(
    bytes: &[u8],
    client_build: Option<ClientBuild>,
) -> anyhow::Result<Option<Vec<u8>>> {
    if bytes.get(..4) != Some(b"BNVR") {
        return Ok(None);
    }

    let Some(status) = bytes.get(4).copied() else {
        return Ok(None);
    };

    match status {
        b'R' if bytes.len() == 6 => Ok(Some(bytes.to_vec())),
        b'A' if bytes.len() == 9 || bytes.len() == 21 => {
            require_client_can_speak_emulated_server_dialect(client_build)?;

            let mut rewritten = Vec::with_capacity(21);
            rewritten.extend_from_slice(&bytes[..9]);
            rewritten.extend_from_slice(&EE_FACING_SERVER_DIALECT_BUILD_MAJOR.to_le_bytes());
            rewritten.extend_from_slice(&EE_FACING_SERVER_DIALECT_BUILD_MINOR.to_le_bytes());
            rewritten.extend_from_slice(&EE_FACING_SERVER_DIALECT_BUILD_REVISION.to_le_bytes());
            tracing::info!(
                old_len = bytes.len(),
                new_len = rewritten.len(),
                client_build_major = client_build.map(|build| build.major),
                client_build_minor = client_build.map(|build| build.minor),
                client_build_revision = client_build.map(|build| build.revision),
                ee_facing_server_build_major = EE_FACING_SERVER_DIALECT_BUILD_MAJOR,
                ee_facing_server_build_minor = EE_FACING_SERVER_DIALECT_BUILD_MINOR,
                ee_facing_server_build_revision = EE_FACING_SERVER_DIALECT_BUILD_REVISION,
                "server BNVR accept rewritten to proxy-owned EE server dialect build"
            );
            Ok(Some(rewritten))
        }
        _ => Ok(None),
    }
}

fn require_client_can_speak_emulated_server_dialect(
    client_build: Option<ClientBuild>,
) -> anyhow::Result<()> {
    let Some(build) = client_build else {
        anyhow::bail!("BNVR accept cannot be translated before a verified BNXI client build");
    };
    if !build.satisfies(
        EE_FACING_SERVER_DIALECT_BUILD_MAJOR,
        EE_FACING_SERVER_DIALECT_BUILD_MINOR,
        EE_FACING_SERVER_DIALECT_BUILD_REVISION,
    ) {
        anyhow::bail!(
            "BNVR accept client build {}.{}.{} does not satisfy proxy EE-facing server dialect {}.{}.{}",
            build.major,
            build.minor,
            build.revision,
            EE_FACING_SERVER_DIALECT_BUILD_MAJOR,
            EE_FACING_SERVER_DIALECT_BUILD_MINOR,
            EE_FACING_SERVER_DIALECT_BUILD_REVISION
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extends_legacy_accept_with_proxy_owned_ee_server_dialect_build() {
        let legacy = [
            b'B', b'N', b'V', b'R', b'A', 0xB0, 0xF5, 0x63, 0x53,
        ];
        let rewritten = rewrite_server_to_ee_if_verified(
            &legacy,
            Some(ClientBuild {
                major: 8193,
                minor: 37,
                revision: 17,
            }),
        )
        .expect("rewrite should not fail")
        .expect("legacy accept should be claimed");

        assert_eq!(rewritten.len(), 21);
        assert_eq!(&rewritten[..9], &legacy);
        assert_eq!(u32::from_le_bytes(rewritten[9..13].try_into().unwrap()), 8193);
        assert_eq!(u32::from_le_bytes(rewritten[13..17].try_into().unwrap()), 35);
        assert_eq!(u32::from_le_bytes(rewritten[17..21].try_into().unwrap()), 0);
    }

    #[test]
    fn normalizes_extended_accept_to_proxy_owned_ee_server_dialect_build() {
        let mut extended = [
            b'B', b'N', b'V', b'R', b'A', 0xB0, 0xF5, 0x63, 0x53, 0x01, 0x20, 0x00, 0x00,
            0x25, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00,
        ];
        let rewritten = rewrite_server_to_ee_if_verified(
            &extended,
            Some(ClientBuild {
                major: 8193,
                minor: 37,
                revision: 17,
            }),
        )
        .expect("rewrite should not fail")
        .expect("extended accept should be claimed");

        extended[13] = 0x23;
        extended[17] = 0x00;
        assert_eq!(rewritten, extended);
    }

    #[test]
    fn rejects_client_too_old_for_proxy_owned_ee_server_dialect_build() {
        let legacy = [
            b'B', b'N', b'V', b'R', b'A', 0xB0, 0xF5, 0x63, 0x53,
        ];
        let err = rewrite_server_to_ee_if_verified(
            &legacy,
            Some(ClientBuild {
                major: 8193,
                minor: 34,
                revision: 0,
            }),
        )
        .expect_err("old client cannot accept the bridge's EE-facing dialect");
        assert!(err.to_string().contains("does not satisfy proxy EE-facing server dialect"));
    }
}
