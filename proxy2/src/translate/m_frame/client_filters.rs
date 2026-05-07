//! Client-to-server `M` cleanup filters.
//!
//! These are transport-level compatibility filters for EE client packets that
//! the legacy 1.69 server must not see. Keep packet semantics out of here:
//! this module may consume or reshape a whole reliable frame, but it should not
//! parse game-object payloads.

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{MFrameView, LEGACY_GAMEPLAY_PAYLOAD_OFFSET},
};

const DEVICE_ADVERTISE_PROPERTY_MAJOR: u8 = 0x36;
const DEVICE_ADVERTISE_PROPERTY_MINOR: u8 = 0x01;

pub(super) fn translate_client_frame(bytes: Vec<u8>, view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    let Some(high) = view.high else {
        return Ok(bytes);
    };

    if high.major == DEVICE_ADVERTISE_PROPERTY_MAJOR
        && high.minor == DEVICE_ADVERTISE_PROPERTY_MINOR
    {
        return consume_device_advertise_property(&bytes, view);
    }

    Ok(bytes)
}

fn consume_device_advertise_property(bytes: &[u8], view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume extended-length Device_AdvertiseProperty frame yet");
    }

    let mut rewritten = bytes.to_vec();
    rewritten.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    write_be_u16(&mut rewritten, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to rewrite M packetized length"))?;
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair M CRC"))?;

    tracing::info!(
        old_len = bytes.len(),
        new_len = rewritten.len(),
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        flags = view.flags,
        packetized_sequence = view.packetized_sequence,
        "client Device_AdvertiseProperty consumed as empty reliable M payload"
    );

    Ok(rewritten)
}
