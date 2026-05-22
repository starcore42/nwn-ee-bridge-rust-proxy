//! Raw/zlib deflate helpers for reliable `M` gameplay payloads.
//!
//! This module deliberately knows nothing about packet families, sequence
//! shifts, or semantic rewrites. It only handles the compression shapes that
//! Diamond and EE use around already-reassembled gameplay bytes.

use std::io::Write;

use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};

use crate::packet::m::MAX_REASONABLE_GAMEPLAY_PAYLOAD;

pub(super) fn inflate_with_server_stream(
    compressed: &[u8],
    inflated_length: usize,
    zlib_header: bool,
    server_stream: &mut Option<Decompress>,
) -> anyhow::Result<Option<Vec<u8>>> {
    if inflated_length > MAX_REASONABLE_GAMEPLAY_PAYLOAD {
        return Ok(None);
    }

    if server_stream.is_none() {
        *server_stream = Some(Decompress::new(zlib_header));
    }
    let Some(decompressor) = server_stream.as_mut() else {
        return Ok(None);
    };

    let before_in = decompressor.total_in();
    let before_out = decompressor.total_out();
    let mut output = vec![0; inflated_length];
    let status = match decompressor.decompress(compressed, &mut output, FlushDecompress::Sync) {
        Ok(status) => status,
        Err(_) => return Ok(None),
    };
    let consumed = (decompressor.total_in() - before_in) as usize;
    let produced = (decompressor.total_out() - before_out) as usize;

    if produced == inflated_length
        && consumed == compressed.len()
        && matches!(status, Status::Ok | Status::StreamEnd | Status::BufError)
    {
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
