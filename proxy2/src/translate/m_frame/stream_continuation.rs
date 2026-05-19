//! Proxy-owned server zlib-stream continuation handling.
//!
//! A deflated M window that inflates to bytes without a `P major minor`
//! high-level header is not a gameplay packet by itself. In Diamond/HG captures
//! these windows can still be valid as continuation bytes from the server's
//! persistent zlib stream after the proxy has already replaced an earlier stream
//! window with an EE-safe recompressed payload.
//!
//! Strict bridge rule:
//!
//! - consume the Diamond zlib stream state in the proxy,
//! - do not re-emit no-header bytes to EE as a standalone payload,
//! - dump the bytes for fixture/decompile work,
//! - emit an empty reliable progress shell so the receive window does not stall,
//! - attach a typed continuation proof only when the current stream has a
//!   remembered, non-unknown semantic owner.
//!
//! This module never passes the continuation bytes through. The proof is about
//! consuming a proxy-owned transport tail, not about validating standalone
//! gameplay semantics. Unknown owners stay noisy so they can be researched from
//! captures and decompiles before being promoted to a family-specific claim.

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::translate::{ContinuationOwner, Emit, VerifiedFamily};

use super::{
    SessionState, hex_prefix,
    reassembly::{
        CompletedDeflatedReplay, ServerDeflatedReassembly, build_consumed_server_deflated_frames,
        emit_family_packets_with_interleaved, remember_completed_server_stream_window,
    },
};

pub(super) fn emit_verified_server_stream_continuation(
    state: &mut SessionState,
    reassembly: &ServerDeflatedReassembly,
    source_compressed_length: usize,
    inflated: &[u8],
) -> anyhow::Result<Emit> {
    let owner = state
        .deflate
        .server_zlib_stream_owner
        .unwrap_or(ContinuationOwner::UnknownProxyOwned);
    let stream_epoch = state.deflate.server_zlib_stream_epoch;
    let claimed_family =
        proxy_owned_continuation_family(owner, stream_epoch, reassembly.first_sequence, inflated);

    dump_server_stream_continuation(
        inflated,
        reassembly.first_sequence,
        owner,
        stream_epoch,
        if claimed_family.is_some() {
            "claimed-server-zlib-stream-continuation"
        } else {
            "unclaimed-server-zlib-stream-continuation"
        },
    );

    let outputs = build_consumed_server_deflated_frames(reassembly)?;
    let replay_family = claimed_family.unwrap_or(VerifiedFamily::ConsumedEmptyMFrame);
    remember_completed_server_stream_window(
        state,
        reassembly,
        source_compressed_length,
        CompletedDeflatedReplay::VerifiedPackets {
            family: replay_family,
            packets: outputs.clone(),
        },
    );
    let interleaved_packets = reassembly.interleaved_packets.clone();

    if claimed_family.is_some() {
        tracing::info!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            stream_epoch,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = inflated.len(),
            owner = owner.as_str(),
            prefix = %hex_prefix(inflated, 32),
            "server deflated zlib-stream continuation claimed as proxy-owned semantic stream tail"
        );
    } else {
        tracing::warn!(
            frames = reassembly.frames.len(),
            first_sequence = reassembly.first_sequence,
            stream_epoch,
            packetized_sequence = reassembly.packetized_sequence,
            inflated = inflated.len(),
            owner = owner.as_str(),
            prefix = %hex_prefix(inflated, 32),
            "server deflated zlib-stream continuation consumed: no remembered semantic stream owner claimed it"
        );
    }

    Ok(emit_family_packets_with_interleaved(
        replay_family,
        outputs,
        interleaved_packets,
    ))
}

fn proxy_owned_continuation_family(
    owner: ContinuationOwner,
    stream_epoch: u64,
    first_sequence: u16,
    inflated: &[u8],
) -> Option<VerifiedFamily> {
    if inflated.is_empty() || stream_epoch == 0 || owner == ContinuationOwner::UnknownProxyOwned {
        return None;
    }

    // Decompile-backed transport distinction: EE's reliable-window receive path
    // advances on data-frame sequence progress, but no-header zlib stream tails
    // are not valid high-level gameplay packets. Once a previous zlib-stream
    // window has been semantically rewritten and marked proxy-owned, later
    // no-header tails for the same remembered owner are consumed and represented
    // only as empty reliable progress shells.
    Some(VerifiedFamily::ServerZlibStreamContinuation {
        owner,
        stream_epoch,
        first_sequence,
    })
}

fn dump_server_stream_continuation(
    inflated: &[u8],
    first_sequence: u16,
    owner: ContinuationOwner,
    stream_epoch: u64,
    reason: &str,
) {
    let Some(dir) = crate::translate::diagnostics::diagnostic_dump_dir() else {
        return;
    };

    let mut path = dir;
    if fs::create_dir_all(&path).is_err() {
        return;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.push(format!(
        "{reason}-{}-epoch{stream_epoch}-seq{first_sequence}-{nanos}.bin",
        owner
            .as_str()
            .replace(['<', '>', '/', '\\', ':', '*', '?', '"', '|'], "_")
    ));

    if fs::write(&path, inflated).is_ok() {
        tracing::info!(
            path = %path.display(),
            first_sequence,
            owner = owner.as_str(),
            stream_epoch,
            inflated_length = inflated.len(),
            reason,
            "dumped server zlib-stream continuation for fixture/decompile work"
        );
    }
}

#[cfg(all(test, hgbridge_private_fixtures))]
mod tests {
    use super::*;

    #[test]
    fn captured_live_object_zlib_stream_tail_requires_known_owner() {
        let inflated = include_bytes!(
            "../../../fixtures/m_frame/hg_starc5_epoch9_seq30_live_object_zlib_continuation_20260513.bin"
        );

        assert_eq!(
            proxy_owned_continuation_family(ContinuationOwner::UnknownProxyOwned, 9, 30, inflated),
            None,
            "captured no-header zlib tail must not be claimed without a remembered semantic owner"
        );
        assert_eq!(
            proxy_owned_continuation_family(
                ContinuationOwner::GameObjUpdateLiveObject,
                9,
                30,
                inflated
            ),
            Some(VerifiedFamily::ServerZlibStreamContinuation {
                owner: ContinuationOwner::GameObjUpdateLiveObject,
                stream_epoch: 9,
                first_sequence: 30,
            }),
            "captured live-object tail is a consume-only proof, not a raw gameplay emit"
        );
    }

    #[test]
    fn zlib_stream_tail_rejects_zero_epoch_even_with_owner() {
        let inflated = b"tail bytes without high-level header";

        assert_eq!(
            proxy_owned_continuation_family(
                ContinuationOwner::GameObjUpdateLiveObject,
                0,
                30,
                inflated
            ),
            None
        );
    }
}
