//! Raw/zlib deflate helpers for reliable `M` gameplay payloads.
//!
//! This module deliberately knows nothing about packet families, sequence
//! shifts, or semantic rewrites. It only handles the compression shapes that
//! Diamond and EE use around already-reassembled gameplay bytes.

use std::{fmt, io::Write};

use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};
use miniz_oxide::{
    DataFormat, MZError, MZFlush, MZStatus,
    inflate::stream::{InflateState, inflate},
};

use crate::packet::m::MAX_REASONABLE_GAMEPLAY_PAYLOAD;

/// Cloneable persistent raw/zlib inflater used by Diamond stream-bit records.
///
/// `flate2::Decompress` is not cloneable even though its default Rust backend
/// is `miniz_oxide`. Keep the same backend and `Sync` call contract directly
/// so an ordered reliable successor can speculate against an exact deep copy
/// of the 32 KiB dictionary, bit/Huffman cursor, pending output, and wrapper
/// state. The original copy remains available until outer strict validation
/// commits or rejects the reconstructed packet.
#[derive(Clone)]
pub(super) struct PersistentServerInflater {
    inner: Box<InflateState>,
    total_in: u64,
    total_out: u64,
}

impl PersistentServerInflater {
    fn new(zlib_header: bool) -> Self {
        Self {
            inner: InflateState::new_boxed(if zlib_header {
                DataFormat::Zlib
            } else {
                DataFormat::Raw
            }),
            total_in: 0,
            total_out: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn total_in(&self) -> u64 {
        self.total_in
    }

    #[cfg(test)]
    pub(super) fn total_out(&self) -> u64 {
        self.total_out
    }
}

impl fmt::Debug for PersistentServerInflater {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PersistentServerInflater")
            .field("total_in", &self.total_in)
            .field("total_out", &self.total_out)
            .finish_non_exhaustive()
    }
}

pub(super) fn inflate_with_server_stream(
    compressed: &[u8],
    inflated_length: usize,
    zlib_header: bool,
    server_stream: &mut Option<PersistentServerInflater>,
) -> anyhow::Result<Option<Vec<u8>>> {
    if inflated_length > MAX_REASONABLE_GAMEPLAY_PAYLOAD {
        return Ok(None);
    }

    if server_stream.is_none() {
        *server_stream = Some(PersistentServerInflater::new(zlib_header));
    }
    let Some(decompressor) = server_stream.as_mut() else {
        return Ok(None);
    };

    let mut output = vec![0; inflated_length];
    // This is the same `miniz_oxide::inflate::stream::inflate` call used by
    // flate2's default Rust `Decompress` backend. Preserve flate2's mapping of
    // `MZError::Buf` to `Status::BufError`: an exact-length Sync call may report
    // Buf when the output slice is precisely full and is still a valid record.
    let result = inflate(
        &mut decompressor.inner,
        compressed,
        &mut output,
        MZFlush::Sync,
    );
    decompressor.total_in = decompressor
        .total_in
        .saturating_add(result.bytes_consumed as u64);
    decompressor.total_out = decompressor
        .total_out
        .saturating_add(result.bytes_written as u64);
    let accepted_status = matches!(
        result.status,
        Ok(MZStatus::Ok | MZStatus::StreamEnd) | Err(MZError::Buf)
    );
    let consumed = result.bytes_consumed;
    let produced = result.bytes_written;

    if produced == inflated_length && consumed == compressed.len() && accepted_status {
        output.truncate(produced);
        return Ok(Some(output));
    }

    Ok(None)
}

pub(super) fn inflate_with_window(
    compressed: &[u8],
    inflated_length: usize,
    zlib_header: bool,
    flush: FlushDecompress,
) -> anyhow::Result<Option<Vec<u8>>> {
    let mut decompressor = Decompress::new(zlib_header);
    let mut output = vec![0; inflated_length];
    let status = match decompressor.decompress(compressed, &mut output, flush) {
        Ok(status) => status,
        Err(_) => return Ok(None),
    };
    let total_in = decompressor.total_in() as usize;
    let total_out = decompressor.total_out() as usize;
    let finished = status == Status::StreamEnd
        || (flush != FlushDecompress::Finish
            && status == Status::Ok
            && total_out == inflated_length);

    if finished && total_in == compressed.len() && total_out == inflated_length {
        output.truncate(total_out);
        Ok(Some(output))
    } else {
        Ok(None)
    }
}

pub(super) fn deflate_zlib(inflated: &[u8]) -> anyhow::Result<Vec<u8>> {
    // Use the same one-shot zlib wrapper the mature bridge emits after it has
    // rewritten an EE-facing deflated stream. Clearing the stream bit on the
    // first M frame tells the client this is now a self-contained zlib payload.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(inflated)?;
    Ok(encoder.finish()?)
}

pub(super) fn looks_like_zlib_wrapped_deflate(compressed: &[u8]) -> bool {
    if compressed.len() < 2 {
        return false;
    }
    let cmf = u16::from(compressed[0]);
    let flg = u16::from(compressed[1]);
    if (cmf & 0x0f) != 8 {
        return false;
    }
    let compression_info = (cmf >> 4) & 0x0f;
    compression_info <= 7 && (((cmf << 8) | flg) % 31) == 0
}
