//! Verified server zlib-stream continuation handling.
//!
//! A deflated M window that inflates to bytes without a `P major minor`
//! high-level header is not a gameplay packet by itself. In Diamond/HG captures
//! these windows can still be valid when they are part of the server's persistent
//! zlib byte stream: the high-level semantic packet began in an earlier window,
//! while this window carries continuation bytes from the same compressed stream.
//!
//! Once the proxy has rewritten any earlier server stream window, the EE client
//! no longer shares the original Diamond zlib inflater state. The strict owner
//! for these no-header chunks is therefore transport-level: consume the original
//! server stream bytes into the proxy inflater, then emit the exact inflated
//! continuation bytes in a fresh EE-facing zlib envelope. This is not raw
//! passthrough; the M-frame continuation translator claims only the stream shape
//! and rewrites the compression envelope while preserving the byte stream that
//! the already-classified packet family owns.

use crate::translate::{Emit, VerifiedFamily};

use super::{
    deflate::deflate_zlib,
    hex_prefix,
    reassembly::{
        build_server_deflated_output_frames, remember_completed_server_stream_window,
        CompletedDeflatedReplay, ServerDeflatedReassembly,
    },
    SessionState, CNW_LENGTH_BYTES,
};

pub(super) fn emit_verified_server_stream_continuation(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    inflated: &[u8],
) -> anyhow::Result<Emit> {
    let compressed = deflate_zlib(inflated)?;
    let mut combined = Vec::with_capacity(CNW_LENGTH_BYTES + compressed.len());
    combined.extend_from_slice(&(inflated.len() as u32).to_le_bytes());
    combined.extend_from_slice(&compressed);

    let mut outputs = build_server_deflated_output_frames(reassembly, &combined, 0x01, true)?;
    remember_completed_server_stream_window(
        state,
        reassembly,
        source_compressed_length,
        CompletedDeflatedReplay::VerifiedPackets {
            family: VerifiedFamily::ServerZlibStreamContinuation,
            packets: outputs.clone(),
        },
    );
    outputs.extend(reassembly.interleaved_packets.clone());

    tracing::info!(
        frames = reassembly.frames.len(),
        first_sequence = reassembly.first_sequence,
        packetized_sequence = reassembly.packetized_sequence,
        inflated = inflated.len(),
        compressed = compressed.len(),
        prefix = %hex_prefix(inflated, 32),
        "server deflated zlib-stream continuation converted to EE one-shot envelope"
    );

    Ok(Emit::VerifiedPackets {
        family: VerifiedFamily::ServerZlibStreamContinuation,
        packets: outputs,
    })
}
