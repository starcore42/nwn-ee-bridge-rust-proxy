//! Exact ownership for server zlib-stream zero-fill windows.
//!
//! This module exists to keep an observed transport artifact out of semantic
//! dispatch. HG/1.69 can emit a persistent-zlib `M` window whose inflated bytes
//! are exactly `P` followed by zeroes. `HighLevel::parse` would otherwise treat
//! the leading byte as an unknown `P 00 00` gameplay packet and quarantine it as
//! an unclaimed semantic family.
//!
//! Decompile-backed boundary:
//!
//! - EE `CNetLayerWindow::FrameReceive` advances the reliable window from the
//!   `M` data-frame sequence branch before gameplay dispatch sees a payload.
//! - EE `CNetLayerInternal::UncompressMessage` owns zlib inflation separately
//!   from `CNWMessage` high-level readers.
//! - No Diamond/EE high-level reader owns `P 00 00`; when this exact all-zero
//!   zlib-stream artifact appears, the proxy should preserve reliable progress
//!   with an empty `M` shell rather than forwarding fake gameplay bytes.

use crate::translate::{Emit, VerifiedFamily};

use super::{
    SessionState,
    reassembly::{self, CompletedDeflatedReplay, ServerDeflatedReassembly},
};

const MAX_ZERO_FILL_COMPRESSED_BYTES: usize = 16;

pub(super) fn maybe_claim_server_zlib_zero_fill_window(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    used_server_stream: bool,
    inflated: &[u8],
) -> anyhow::Result<Option<Emit>> {
    if !is_exact_server_zlib_zero_fill_window(
        reassembly,
        source_compressed_length,
        used_server_stream,
        inflated,
    ) {
        return Ok(None);
    }

    let family = VerifiedFamily::ServerZlibZeroFillWindow {
        first_sequence: reassembly.first_sequence,
        inflated_length: inflated.len(),
        compressed_length: source_compressed_length,
    };
    let outputs = reassembly::build_consumed_server_deflated_frames(reassembly)?;
    reassembly::remember_completed_server_stream_window(
        state,
        reassembly,
        source_compressed_length,
        CompletedDeflatedReplay::VerifiedPackets {
            family,
            packets: outputs.clone(),
        },
    );

    tracing::info!(
        frames = reassembly.frames.len(),
        first_sequence = reassembly.first_sequence,
        packetized_sequence = reassembly.packetized_sequence,
        inflated_length = inflated.len(),
        compressed_length = source_compressed_length,
        "server zlib-stream zero-fill window claimed as verified empty reliable progress"
    );

    Ok(Some(reassembly::emit_family_packets_with_interleaved(
        family,
        outputs,
        reassembly.interleaved_packets.clone(),
    )))
}

fn is_exact_server_zlib_zero_fill_window(
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    used_server_stream: bool,
    inflated: &[u8],
) -> bool {
    used_server_stream
        && reassembly.zlib_stream
        && reassembly.expected_frames == 1
        && reassembly.packetized_sequence == 1
        && source_compressed_length != 0
        && source_compressed_length <= MAX_ZERO_FILL_COMPRESSED_BYTES
        && inflated.len() == reassembly.inflated_length
        && is_p_prefixed_zero_fill_payload(inflated)
}

fn is_p_prefixed_zero_fill_payload(inflated: &[u8]) -> bool {
    inflated.len() >= 8 && inflated[0] == b'P' && inflated[1..].iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_matches_exact_p_prefixed_zero_fill_shape() {
        let bytes = include_bytes!("../../../fixtures/m_frame/server_zlib_zero_fill_charlist_tail.bin");
        assert_eq!(bytes.len(), 517);
        assert!(is_p_prefixed_zero_fill_payload(bytes));
    }

    #[test]
    fn zero_fill_shape_rejects_non_zero_payload_bytes() {
        let mut bytes = vec![0; 517];
        bytes[0] = b'P';
        bytes[42] = 1;
        assert!(!is_p_prefixed_zero_fill_payload(&bytes));
    }

    #[test]
    fn zero_fill_shape_rejects_missing_high_level_sentinel() {
        let bytes = vec![0; 517];
        assert!(!is_p_prefixed_zero_fill_payload(&bytes));
    }
}
