//! `BNDM` EE direct-disconnect translation.
//!
//! EE `CNetLayerInternal::SendBNDMMessage` emits exactly four bytes:
//! `BNDM`. The legacy/Diamond disconnect datagram is `BNDS` followed by the
//! UDP port captured from the session's `BNCS`. Keeping this as a focused BN
//! module prevents EE direct-control cleanup from becoming a permissive
//! top-level passthrough rule.

use super::SessionState;

pub(super) enum BndmTranslation {
    LegacyDisconnectRetireSession(Vec<u8>),
    NwsyncHandoffConsumedRetireSession,
}

pub(super) fn translate_client_bndm(
    bytes: &[u8],
    state: &mut SessionState,
) -> anyhow::Result<BndmTranslation> {
    if bytes != b"BNDM" {
        anyhow::bail!("BNDM disconnect has invalid length/shape: {}", bytes.len());
    }

    // EE can send this exact four-byte BNDM during the native NWSync handoff
    // before the legacy BNCS/BNVR verifier path exists, or after a BNVR accept
    // but before reliable gameplay begins. Diamond has no equivalent handoff:
    // EE intentionally tears down its current net-layer session while the
    // native downloader runs, then the user/UI must connect again after the
    // content is local.
    //
    // Once any reliable `M` gameplay frame has been observed, BNDM is no
    // longer proof of the pre-game downloader handoff. EE's decompile names it
    // as a direct disconnect control, so post-gameplay BNDM must translate to
    // Diamond's BNDS shape and stay visible in the logs instead of being hidden
    // behind the NWSync-hand-off escape hatch.
    if state.should_consume_nwsync_handoff_bndm() {
        state.remember_nwsync_handoff_bndm_consumed();
        tracing::info!(
            old_len = bytes.len(),
            "client BNDM consumed as EE NWSync handoff control; proxy session will be retired"
        );
        return Ok(BndmTranslation::NwsyncHandoffConsumedRetireSession);
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

    Ok(BndmTranslation::LegacyDisconnectRetireSession(rewritten))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumes_pre_bncs_bndm_after_nwsync_advert() {
        let mut state = SessionState::default();
        state.remember_nwsync_advertised_to_client();

        let translated = translate_client_bndm(b"BNDM", &mut state).expect("pre-BNCS BNDM");
        assert!(matches!(
            translated,
            BndmTranslation::NwsyncHandoffConsumedRetireSession
        ));
    }

    #[test]
    fn consumes_first_bndm_after_nwsync_advert_and_bnvr_accept() {
        let mut state = SessionState::default();
        state.remember_bncs_udp_port(5121);
        state.remember_nwsync_advertised_to_client();
        state.remember_bnvr_result(true);

        let first = translate_client_bndm(b"BNDM", &mut state).expect("first BNDM");
        assert!(matches!(
            first,
            BndmTranslation::NwsyncHandoffConsumedRetireSession
        ));

        let second = translate_client_bndm(b"BNDM", &mut state).expect("second BNDM");
        assert!(matches!(
            second,
            BndmTranslation::LegacyDisconnectRetireSession(_)
        ));
    }

    #[test]
    fn post_gameplay_bndm_is_real_disconnect_not_nwsync_handoff() {
        let mut state = SessionState::default();
        state.remember_bncs_udp_port(5121);
        state.remember_nwsync_advertised_to_client();
        state.remember_bnvr_result(true);
        state.remember_reliable_gameplay_seen();

        let translated = translate_client_bndm(b"BNDM", &mut state).expect("post-gameplay BNDM");
        assert!(matches!(
            translated,
            BndmTranslation::LegacyDisconnectRetireSession(_)
        ));
    }

    #[test]
    fn rewrites_normal_bndm_to_legacy_bnds() {
        let mut state = SessionState::default();
        state.remember_bncs_udp_port(0x1673);

        let translated = translate_client_bndm(b"BNDM", &mut state).expect("normal BNDM");
        let BndmTranslation::LegacyDisconnectRetireSession(packet) = translated else {
            panic!("normal BNDM should not be consumed");
        };
        assert_eq!(packet, b"BNDS\x73\x16");
    }
}
