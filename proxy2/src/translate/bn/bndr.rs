//! BNDR extended server-info observation.
//!
//! BNDR does not need a dialect rewrite when the EE client is the receiver:
//! the EE decompile has an explicit `HandleBNDRMessage` parser for this shape.
//! We still parse and log it here before strict validation so this remains a
//! known, understood packet rather than an accidental direct-control pass.

use crate::packet::bn::parse_bndr_extended_server_info;

pub(crate) fn claim_server_to_ee_if_verified(bytes: &[u8]) -> Option<()> {
    let Some(info) = parse_bndr_extended_server_info(bytes) else {
        tracing::warn!(
            len = bytes.len(),
            "server BNDR extended-info response did not match decompile-backed shape"
        );
        return None;
    };

    tracing::info!(
        header_word = info.header_word,
        details_len = info.details.len(),
        module_description_len = info.module_description.len(),
        build_len = info.build.len(),
        trailing_word = info.trailing_word,
        "server BNDR extended-info response parsed for EE client"
    );
    Some(())
}
