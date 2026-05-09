//! Client-to-server `M` cleanup filters.
//!
//! These are transport-level compatibility filters for EE client packets that
//! the legacy 1.69 server must not see. Keep packet semantics out of here:
//! this module may consume or reshape a whole reliable frame, but focused
//! semantic ownership remains delegated to `translate::client_high`.

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
    translate::client_high,
};

use super::transport_identity;

const DEVICE_ADVERTISE_PROPERTY_MAJOR: u8 = 0x36;
const DEVICE_ADVERTISE_PROPERTY_MINOR: u8 = 0x01;

pub(super) fn translate_client_frame(bytes: Vec<u8>, view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    let Some(high) = view.high else {
        if let Some(summary) = transport_identity::claim_client_frame_if_verified(view) {
            tracing::info!(
                packet = summary.packet_name,
                reason = summary.reason,
                sequence = view.sequence,
                ack_sequence = view.ack_sequence,
                flags = view.flags,
                packetized_sequence = view.packetized_sequence,
                payload_len = view.payload_length,
                "client M transport-only frame semantically claimed as verified no-op"
            );
            return Ok(bytes);
        }
        anyhow::bail!("client M frame has no high-level translator or transport identity owner");
    };

    if high.major == DEVICE_ADVERTISE_PROPERTY_MAJOR
        && high.minor == DEVICE_ADVERTISE_PROPERTY_MINOR
    {
        return consume_device_advertise_property(&bytes, view);
    }

    let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET
        .checked_add(view.payload_length)
        .ok_or_else(|| anyhow::anyhow!("client M payload length overflow"))?;
    let payload = bytes
        .get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end)
        .ok_or_else(|| anyhow::anyhow!("client M high-level payload outside frame"))?;
    let mut translated_payload = payload.to_vec();

    if let Some(summary) =
        client_high::claim_or_rewrite_payload_if_verified(&mut translated_payload)
    {
        let payload_rewritten = translated_payload.as_slice() != payload;
        let out = if payload_rewritten {
            replace_client_payload_and_repair_crc(&bytes, view, &translated_payload)?
        } else {
            bytes
        };
        tracing::info!(
            family = summary.family_name,
            packet = summary.packet_name,
            major = high.major,
            minor = high.minor,
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            payload_len = view.payload_length,
            trailing_payload_len = view.trailing_payload_length,
            payload_rewritten,
            "client high-level payload semantically claimed for Diamond/1.69"
        );
        return Ok(out);
    }

    return consume_unclaimed_client_high_level(&bytes, view);
}

fn replace_client_payload_and_repair_crc(
    bytes: &[u8],
    view: &MFrameView,
    payload: &[u8],
) -> anyhow::Result<Vec<u8>> {
    if payload.len() != view.payload_length {
        anyhow::bail!(
            "client high-level translator changed payload length without M-frame repacketizer"
        );
    }

    let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET
        .checked_add(view.payload_length)
        .ok_or_else(|| anyhow::anyhow!("client M payload length overflow"))?;
    let mut rewritten = bytes.to_vec();
    let target = rewritten
        .get_mut(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end)
        .ok_or_else(|| anyhow::anyhow!("client M replacement payload outside frame"))?;
    target.copy_from_slice(payload);
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair rewritten client M CRC"))?;
    Ok(rewritten)
}

fn consume_unclaimed_client_high_level(bytes: &[u8], view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume unclaimed extended-length client M frame yet");
    }

    let high_name = view
        .high
        .map(|high| high.name())
        .unwrap_or("unknown client high-level");
    let mut rewritten = bytes.to_vec();
    rewritten.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if rewritten.len() > 7 {
        rewritten[7] &= !0x07;
    }
    write_be_u16(&mut rewritten, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear unclaimed client M packetized length"))?;
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair unclaimed client M CRC"))?;

    tracing::warn!(
        name = high_name,
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        payload_len = view.payload_length,
        trailing_payload_len = view.trailing_payload_length,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        "client high-level M frame quarantined: semantic translator did not claim payload"
    );

    Ok(rewritten)
}

fn consume_device_advertise_property(bytes: &[u8], view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume extended-length Device_AdvertiseProperty frame yet");
    }

    let mut rewritten = bytes.to_vec();
    rewritten.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if rewritten.len() > 7 {
        rewritten[7] &= !0x07;
    }
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
