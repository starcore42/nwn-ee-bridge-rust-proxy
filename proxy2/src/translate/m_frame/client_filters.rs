//! Client-to-server `M` cleanup filters.
//!
//! These are transport-level compatibility filters for EE client packets that
//! the legacy 1.69 server must not see. Keep packet semantics out of here:
//! this module may consume or reshape a whole reliable frame, but focused
//! semantic ownership remains delegated to `translate::client_high`.

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
    translate::{client_high, semantic::SemanticSessionState, VerifiedFamily},
};

use super::{parse_window, transport_identity};

const DEVICE_ADVERTISE_PROPERTY_MAJOR: u8 = 0x36;
const DEVICE_ADVERTISE_PROPERTY_MINOR: u8 = 0x01;

#[derive(Debug, Clone)]
pub(super) struct ClientFrameTranslation {
    pub family: VerifiedFamily,
    pub packet: Vec<u8>,
}

pub(super) fn translate_client_frame(
    bytes: Vec<u8>,
    view: &MFrameView,
    state: &mut SemanticSessionState,
) -> anyhow::Result<ClientFrameTranslation> {
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
            return Ok(ClientFrameTranslation {
                family: VerifiedFamily::ConsumedEmptyMFrame,
                packet: bytes,
            });
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
        client_high::claim_or_rewrite_payload_if_verified(&mut translated_payload, state)
    {
        let payload_rewritten = translated_payload.as_slice() != payload;
        let logged_high = HighLevel::parse(&translated_payload).unwrap_or(high);
        let out = if payload_rewritten {
            replace_client_payload_and_repair_crc(&bytes, view, &translated_payload)?
        } else {
            bytes
        };
        tracing::info!(
            family = summary.family_name,
            packet = summary.packet_name,
            major = logged_high.major,
            minor = logged_high.minor,
            original_major = high.major,
            original_minor = high.minor,
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            payload_len = translated_payload.len(),
            original_payload_len = view.payload_length,
            trailing_payload_len = view.trailing_payload_length,
            payload_rewritten,
            "client high-level payload semantically claimed for Diamond/1.69"
        );
        return Ok(ClientFrameTranslation {
            family: summary.verified_family,
            packet: out,
        });
    }

    return consume_unclaimed_client_high_level(&bytes, view);
}

pub(super) fn consume_claimed_high_level_as_empty(
    bytes: Vec<u8>,
    view: &MFrameView,
    packet_name: &'static str,
    reason: &'static str,
) -> anyhow::Result<ClientFrameTranslation> {
    let rewritten = build_consumed_empty_client_frame(&bytes, view)?;

    tracing::info!(
        packet = packet_name,
        reason,
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        payload_len = view.payload_length,
        trailing_payload_len = view.trailing_payload_length,
        old_len = bytes.len(),
        new_len = rewritten.len(),
        "client high-level M frame consumed as verified proxy-owned empty progress carrier"
    );

    Ok(ClientFrameTranslation {
        family: VerifiedFamily::ConsumedEmptyMFrame,
        packet: rewritten,
    })
}

fn replace_client_payload_and_repair_crc(
    bytes: &[u8],
    view: &MFrameView,
    payload: &[u8],
) -> anyhow::Result<Vec<u8>> {
    parse_window::replace_primary_payload_and_repair(
        bytes,
        view,
        payload,
        "client high-level payload",
    )
}

fn consume_unclaimed_client_high_level(
    bytes: &[u8],
    view: &MFrameView,
) -> anyhow::Result<ClientFrameTranslation> {
    let high_name = view
        .high
        .map(|high| high.name())
        .unwrap_or("unknown client high-level");
    let rewritten = build_consumed_empty_client_frame(bytes, view)?;

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

    Ok(ClientFrameTranslation {
        family: VerifiedFamily::ConsumedEmptyMFrame,
        packet: rewritten,
    })
}

fn consume_device_advertise_property(
    bytes: &[u8],
    view: &MFrameView,
) -> anyhow::Result<ClientFrameTranslation> {
    let rewritten = build_consumed_empty_client_frame(bytes, view)?;

    tracing::info!(
        old_len = bytes.len(),
        new_len = rewritten.len(),
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        flags = view.flags,
        packetized_sequence = view.packetized_sequence,
        "client Device_AdvertiseProperty consumed as empty reliable M payload"
    );

    Ok(ClientFrameTranslation {
        family: VerifiedFamily::ConsumedEmptyMFrame,
        packet: rewritten,
    })
}

fn build_consumed_empty_client_frame(bytes: &[u8], view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume extended-length client M frame yet");
    }

    let mut rewritten = bytes.to_vec();
    rewritten.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if rewritten.len() > 7 {
        rewritten[7] &= !0x07;
    }
    write_be_u16(&mut rewritten, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear consumed client M packetized length"))?;
    encode_legacy_m_crc(&mut rewritten)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to repair consumed client M CRC"))?;
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_client_m_frame(sequence: u16, ack_sequence: u16, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
        packet[0] = b'M';
        assert!(write_be_u16(&mut packet, 3, sequence));
        assert!(write_be_u16(&mut packet, 5, ack_sequence));
        packet[7] = 0x0A;
        assert!(write_be_u16(&mut packet, 8, 1));
        assert!(write_be_u16(&mut packet, 10, payload.len() as u16));
        packet.extend_from_slice(payload);
        assert!(encode_legacy_m_crc(&mut packet));
        packet
    }

    #[test]
    fn claimed_area_loaded_duplicate_rewrites_to_empty_progress_frame() {
        let packet = build_client_m_frame(0x1234, 0x0056, &[0x70, 0x04, 0x03]);
        let view = MFrameView::parse(&packet).expect("fixture should parse as M frame");

        let translated = consume_claimed_high_level_as_empty(
            packet,
            &view,
            "Area_AreaLoaded",
            "duplicate synthetic fallback test",
        )
        .expect("claimed duplicate should consume to empty progress frame");

        assert_eq!(translated.family, VerifiedFamily::ConsumedEmptyMFrame);
        let consumed = MFrameView::parse(&translated.packet).expect("consumed frame should parse");
        assert_eq!(consumed.sequence, 0x1234);
        assert_eq!(consumed.ack_sequence, 0x0056);
        assert_eq!(consumed.payload_length, 0);
        assert_eq!(translated.packet.len(), LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    }
}
