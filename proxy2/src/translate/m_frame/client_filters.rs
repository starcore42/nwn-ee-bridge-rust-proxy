//! Client-to-server `M` cleanup filters.
//!
//! These are transport-level compatibility filters for EE client packets that
//! the legacy 1.69 server must not see. Keep packet semantics out of here:
//! this module may consume or reshape a whole reliable frame, but focused
//! semantic ownership remains delegated to `translate::client_high`.

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameType, MFrameView},
    translate::{
        VerifiedFamily, client_device, client_high, client_server_admin, gameplay_stream,
        semantic::SemanticSessionState,
    },
};

use super::{parse_window, transport_identity};

const DEVICE_ADVERTISE_PROPERTY_MAJOR: u8 = 0x36;
const DEVICE_ADVERTISE_PROPERTY_MINOR: u8 = 0x01;

#[derive(Debug, Clone)]
pub(super) struct ClientFrameTranslation {
    pub family: VerifiedFamily,
    pub packet: Option<Vec<u8>>,
    pub semantic_observations: Vec<ClientSemanticObservation>,
    /// When the proxy owns an EE-only reliable client frame, it may need to ACK
    /// that frame toward EE immediately instead of waiting for the 1.69 server
    /// to ACK anything. This remains a fallback for frames that cannot be
    /// represented as a server-paced empty data carrier.
    pub proxy_ack_client_sequence: Option<u16>,
    pub elide_client_sequence: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ClientSemanticObservation {
    pub family: VerifiedFamily,
    pub payload: Vec<u8>,
}

pub(super) fn translate_client_frame(
    bytes: Vec<u8>,
    view: &MFrameView,
    state: &mut SemanticSessionState,
) -> anyhow::Result<ClientFrameTranslation> {
    let Some(frame_kind) = view.frame_kind() else {
        anyhow::bail!(
            "client M frame has unsupported frame type {}",
            view.frame_type
        );
    };
    if frame_kind != MFrameType::ReliableData {
        let Some(summary) = transport_identity::claim_client_frame_if_verified(view) else {
            anyhow::bail!("client M control frame has no exact transport identity owner");
        };
        tracing::info!(
            packet = summary.packet_name,
            reason = summary.reason,
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            flags = view.flags,
            frame_type = view.frame_type,
            "client M control frame semantically claimed as verified no-op"
        );
        return Ok(ClientFrameTranslation {
            family: VerifiedFamily::ConsumedEmptyMFrame,
            packet: Some(bytes),
            semantic_observations: Vec::new(),
            proxy_ack_client_sequence: None,
            elide_client_sequence: false,
        });
    }

    let Some(high) = view.high else {
        let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET
            .checked_add(view.payload_length)
            .ok_or_else(|| anyhow::anyhow!("client raw M payload length overflow"))?;
        let payload = bytes
            .get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end)
            .ok_or_else(|| anyhow::anyhow!("client raw M payload outside frame"))?;

        if let Some(summary) = client_server_admin::claim_payload_if_verified(payload) {
            tracing::info!(
                packet = summary.packet_name,
                command = ?summary.command,
                sequence = view.sequence,
                ack_sequence = view.ack_sequence,
                flags = view.flags,
                packetized_sequence = view.packetized_sequence,
                payload_len = view.payload_length,
                trailing_payload_len = view.trailing_payload_length,
                "client server-admin M payload semantically claimed for Diamond/1.69"
            );
            return Ok(ClientFrameTranslation {
                family: VerifiedFamily::ClientServerAdmin,
                packet: Some(bytes),
                semantic_observations: Vec::new(),
                proxy_ack_client_sequence: None,
                elide_client_sequence: false,
            });
        }

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
                packet: Some(bytes),
                semantic_observations: Vec::new(),
                proxy_ack_client_sequence: None,
                elide_client_sequence: false,
            });
        }
        anyhow::bail!("client M frame has no high-level translator or transport identity owner");
    };

    let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET
        .checked_add(view.payload_length)
        .ok_or_else(|| anyhow::anyhow!("client M payload length overflow"))?;
    let payload = bytes
        .get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end)
        .ok_or_else(|| anyhow::anyhow!("client M high-level payload outside frame"))?;

    if let Some(outcome) = translate_mixed_client_primary_payload_if_needed(payload, state) {
        return finalize_mixed_client_primary_payload(bytes, view, outcome);
    }

    if high.major == DEVICE_ADVERTISE_PROPERTY_MAJOR
        && high.minor == DEVICE_ADVERTISE_PROPERTY_MINOR
    {
        if client_device::claim_payload_if_verified(payload).is_some() {
            return consume_device_advertise_property(&bytes, view, payload);
        }
        return consume_unclaimed_client_high_level(&bytes, view);
    }

    let mut translated_payload = payload.to_vec();

    if let Some(summary) = client_high::claim_consumed_payload_if_verified(payload) {
        let consumed_payload = payload.to_vec();
        tracing::info!(
            family = summary.family_name,
            packet = summary.packet_name,
            reason = summary.reason,
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            payload_len = view.payload_length,
            trailing_payload_len = view.trailing_payload_length,
            "client high-level payload semantically claimed and consumed for Diamond/1.69"
        );
        let mut translated =
            consume_claimed_high_level_as_empty(bytes, view, summary.packet_name, summary.reason)?;
        translated
            .semantic_observations
            .push(ClientSemanticObservation {
                family: summary.verified_family,
                payload: consumed_payload,
            });
        return Ok(translated);
    }

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
            packet: Some(out),
            semantic_observations: Vec::new(),
            proxy_ack_client_sequence: None,
            elide_client_sequence: false,
        });
    }

    return consume_unclaimed_client_high_level(&bytes, view);
}

struct MixedClientPrimaryPayload {
    payload: Vec<u8>,
    family: Option<VerifiedFamily>,
    semantic_observations: Vec<ClientSemanticObservation>,
    consumed_units: usize,
    forwarded_units: usize,
    total_units: usize,
    quarantine_reason: Option<&'static str>,
}

fn translate_mixed_client_primary_payload_if_needed(
    payload: &[u8],
    state: &mut SemanticSessionState,
) -> Option<MixedClientPrimaryPayload> {
    if let Some(device_end) = client_device::unit_end_if_verified(payload) {
        if device_end < payload.len() {
            let tail = &payload[device_end..];
            let split = gameplay_stream::split_inflated_gameplay(tail);
            let tail_units = split.units.len();
            if !split.complete || tail_units == 0 {
                return Some(MixedClientPrimaryPayload {
                    payload: Vec::new(),
                    family: None,
                    semantic_observations: Vec::new(),
                    consumed_units: 1,
                    forwarded_units: 0,
                    total_units: 1 + tail_units,
                    quarantine_reason: Some("mixed-client-leading-device-unbounded-tail"),
                });
            }
            return Some(translate_mixed_client_high_level_units(
                split.units,
                state,
                1,
                1 + tail_units,
                device_end,
            ));
        }
    }

    let split = gameplay_stream::split_inflated_gameplay(payload);
    if !split.complete || split.units.len() <= 1 {
        return None;
    }

    let total_units = split.units.len();
    Some(translate_mixed_client_high_level_units(
        split.units,
        state,
        0,
        total_units,
        0,
    ))
}

fn translate_mixed_client_high_level_units(
    units: Vec<gameplay_stream::GameplayUnit<'_>>,
    state: &mut SemanticSessionState,
    mut consumed_units: usize,
    total_units: usize,
    unit_offset_base: usize,
) -> MixedClientPrimaryPayload {
    let mut rewritten = Vec::new();
    let mut family: Option<VerifiedFamily> = None;
    let mut semantic_observations = Vec::new();
    let mut forwarded_units = 0usize;

    for unit in units {
        let gameplay_stream::GameplayUnit::HighLevel(message) = unit else {
            return MixedClientPrimaryPayload {
                payload: Vec::new(),
                family: None,
                semantic_observations,
                consumed_units,
                forwarded_units,
                total_units,
                quarantine_reason: Some("mixed-client-primary-non-high-level-unit"),
            };
        };

        if client_device::claim_payload_if_verified(message.payload).is_some() {
            consumed_units = consumed_units.saturating_add(1);
            continue;
        }

        if let Some(summary) = client_high::claim_consumed_payload_if_verified(message.payload) {
            let unit_offset = unit_offset_base + message.offset;
            tracing::info!(
                family = summary.family_name,
                packet = summary.packet_name,
                reason = summary.reason,
                unit_offset,
                unit_len = message.payload.len(),
                "mixed client primary payload unit semantically claimed and consumed"
            );
            semantic_observations.push(ClientSemanticObservation {
                family: summary.verified_family,
                payload: message.payload.to_vec(),
            });
            consumed_units = consumed_units.saturating_add(1);
            continue;
        }

        let mut translated_unit = message.payload.to_vec();
        let Some(summary) =
            client_high::claim_or_rewrite_payload_if_verified(&mut translated_unit, state)
        else {
            return MixedClientPrimaryPayload {
                payload: Vec::new(),
                family: None,
                semantic_observations,
                consumed_units,
                forwarded_units,
                total_units,
                quarantine_reason: Some("mixed-client-primary-unclaimed-high-level-unit"),
            };
        };

        if let Some(existing) = family {
            if existing != summary.verified_family {
                let unit_offset = unit_offset_base + message.offset;
                tracing::warn!(
                    existing_family = existing.as_str(),
                    new_family = summary.verified_family.as_str(),
                    unit_offset,
                    unit_len = message.payload.len(),
                    "mixed client primary payload quarantined: multiple forwarded semantic families in one primary payload"
                );
                return MixedClientPrimaryPayload {
                    payload: Vec::new(),
                    family: None,
                    semantic_observations,
                    consumed_units,
                    forwarded_units,
                    total_units,
                    quarantine_reason: Some("mixed-client-primary-multiple-forwarded-families"),
                };
            }
        } else {
            family = Some(summary.verified_family);
        }

        let unit_offset = unit_offset_base + message.offset;
        tracing::info!(
            family = summary.family_name,
            packet = summary.packet_name,
            unit_offset,
            old_unit_len = message.payload.len(),
            new_unit_len = translated_unit.len(),
            "mixed client primary payload unit semantically claimed for Diamond/1.69"
        );
        rewritten.extend_from_slice(&translated_unit);
        forwarded_units = forwarded_units.saturating_add(1);
    }

    MixedClientPrimaryPayload {
        payload: rewritten,
        family,
        semantic_observations,
        consumed_units,
        forwarded_units,
        total_units,
        quarantine_reason: None,
    }
}

fn finalize_mixed_client_primary_payload(
    bytes: Vec<u8>,
    view: &MFrameView,
    outcome: MixedClientPrimaryPayload,
) -> anyhow::Result<ClientFrameTranslation> {
    if let Some(reason) = outcome.quarantine_reason {
        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            payload_len = view.payload_length,
            trailing_payload_len = view.trailing_payload_length,
            total_units = outcome.total_units,
            consumed_units = outcome.consumed_units,
            forwarded_units = outcome.forwarded_units,
            reason,
            "mixed client primary payload quarantined before any Diamond/1.69 send"
        );
        return consume_unclaimed_client_high_level(&bytes, view);
    }

    if outcome.forwarded_units == 0 {
        tracing::info!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            payload_len = view.payload_length,
            trailing_payload_len = view.trailing_payload_length,
            total_units = outcome.total_units,
            consumed_units = outcome.consumed_units,
            "mixed client primary payload fully consumed as proxy-owned EE-only units"
        );
        let mut translated = consume_claimed_high_level_as_empty(
            bytes,
            view,
            "ClientMixedPrimaryPayload",
            "all mixed primary units are EE-only/proxy-owned",
        )?;
        translated.semantic_observations = outcome.semantic_observations;
        return Ok(translated);
    }

    if view.trailing_payload_length != 0 {
        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            payload_len = view.payload_length,
            trailing_payload_len = view.trailing_payload_length,
            total_units = outcome.total_units,
            consumed_units = outcome.consumed_units,
            forwarded_units = outcome.forwarded_units,
            "mixed client primary payload quarantined: packetized trailing spans need typed per-span client proof before forwarding"
        );
        return consume_unclaimed_client_high_level(&bytes, view);
    }

    let Some(family) = outcome.family else {
        tracing::warn!(
            sequence = view.sequence,
            ack_sequence = view.ack_sequence,
            payload_len = view.payload_length,
            total_units = outcome.total_units,
            consumed_units = outcome.consumed_units,
            forwarded_units = outcome.forwarded_units,
            "mixed client primary payload quarantined: forwarded units had no verified family"
        );
        return consume_unclaimed_client_high_level(&bytes, view);
    };

    let rewritten = replace_client_payload_and_repair_crc(&bytes, view, &outcome.payload)?;
    tracing::info!(
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        family = family.as_str(),
        old_payload_len = view.payload_length,
        new_payload_len = outcome.payload.len(),
        total_units = outcome.total_units,
        consumed_units = outcome.consumed_units,
        forwarded_units = outcome.forwarded_units,
        "mixed client primary payload rewritten with proxy-owned units removed"
    );

    Ok(ClientFrameTranslation {
        family,
        packet: Some(rewritten),
        semantic_observations: outcome.semantic_observations,
        proxy_ack_client_sequence: None,
        elide_client_sequence: false,
    })
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
        packet: Some(rewritten),
        semantic_observations: Vec::new(),
        proxy_ack_client_sequence: None,
        elide_client_sequence: false,
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
        packet: Some(rewritten),
        semantic_observations: Vec::new(),
        proxy_ack_client_sequence: None,
        elide_client_sequence: false,
    })
}

fn consume_device_advertise_property(
    bytes: &[u8],
    view: &MFrameView,
    payload: &[u8],
) -> anyhow::Result<ClientFrameTranslation> {
    let Some(summary) = client_device::claim_payload_if_verified(payload) else {
        anyhow::bail!("client Device_AdvertiseProperty payload did not match verified EE shape");
    };

    tracing::info!(
        old_len = bytes.len(),
        sequence = view.sequence,
        ack_sequence = view.ack_sequence,
        flags = view.flags,
        packetized_sequence = view.packetized_sequence,
        payload_len = view.payload_length,
        trailing_payload_len = view.trailing_payload_length,
        declared = summary.declared,
        property_name_len = summary.property_name_len,
        flag = summary.flag,
        has_value = summary.has_value,
        fragment_bytes = summary.fragment_bytes,
        "client Device_AdvertiseProperty consumed as proxy-owned EE-only reliable M payload"
    );

    // EE advertises local device/UI properties as reliable gameplay messages
    // during pregame. Diamond has no corresponding semantic reader, so the
    // Device payload must never be forwarded. The reliable sequence itself,
    // however, is real transport state. Emit a decompile-owned empty M data
    // carrier with the original sequence so the 1.69 server ACKs the exact EE
    // sequence naturally. This avoids sequence elision and keeps the EE
    // outgoing window draining without letting an unclaimed gameplay payload
    // leak through the bridge.
    let rewritten = build_consumed_empty_client_frame(bytes, view)?;
    Ok(ClientFrameTranslation {
        family: VerifiedFamily::ConsumedEmptyMFrame,
        packet: Some(rewritten),
        semantic_observations: Vec::new(),
        proxy_ack_client_sequence: None,
        elide_client_sequence: false,
    })
}

fn build_consumed_empty_client_frame(bytes: &[u8], view: &MFrameView) -> anyhow::Result<Vec<u8>> {
    if view.frame_kind() != Some(MFrameType::ReliableData) {
        anyhow::bail!("cannot turn a client M control frame into a reliable-data carrier");
    }
    if view.uses_extended_packet_length {
        anyhow::bail!("cannot consume extended-length client M frame yet");
    }

    let mut rewritten = bytes.to_vec();
    rewritten.truncate(LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
    if rewritten.len() > 7 {
        // EE and Diamond both route kind-0 M frames through the reliable data
        // window. Kind-1 (0x10) is ACK-only and higher high-nibble kinds are not
        // a safe carrier for a consumed gameplay payload. Preserve only the
        // high-priority queue bit. Keep the packetized sequence field intact:
        // server->client Diamond captures include accepted empty progress
        // shells shaped as flags=0x08, packetized_sequence=1,
        // packetized_length=0, so clearing packetized_sequence would create a
        // different transport shape.
        rewritten[7] &= 0x08;
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

    fn valid_device_advertise_property_payload() -> Vec<u8> {
        let mut payload = vec![0x70, 0x36, 0x01];
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.push(b'x');
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        let declared = payload.len() as u32;
        payload[3..7].copy_from_slice(&declared.to_le_bytes());
        payload
    }

    fn client_gui_event_notify_payload(object_id: u32) -> Vec<u8> {
        const DECLARED: usize = 15;
        let mut payload = Vec::with_capacity(DECLARED);
        payload.extend_from_slice(&[0x70, 0x35, 0x01]);
        payload.extend_from_slice(&(DECLARED as u32).to_le_bytes());
        payload.extend_from_slice(&2u16.to_le_bytes());
        payload.extend_from_slice(&3u16.to_le_bytes());
        payload.extend_from_slice(&object_id.to_le_bytes());
        payload
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
        let packet = translated
            .packet
            .as_ref()
            .expect("duplicate area-loaded keeps a legacy progress carrier");
        let consumed = MFrameView::parse(packet).expect("consumed frame should parse");
        assert_eq!(consumed.sequence, 0x1234);
        assert_eq!(consumed.ack_sequence, 0x0056);
        assert_eq!(consumed.payload_length, 0);
        assert_eq!(packet.len(), LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
        assert!(!translated.elide_client_sequence);
    }

    #[test]
    fn mixed_primary_payload_consumes_device_and_forwards_claimed_unit() {
        let mut payload = valid_device_advertise_property_payload();
        payload.extend_from_slice(&[0x70, 0x11, 0x01]);

        let packet = build_client_m_frame(0x0034, 0x0007, &payload);
        let view = MFrameView::parse(&packet).expect("fixture should parse as M frame");
        let mut state = SemanticSessionState::default();

        let translated = translate_client_frame(packet, &view, &mut state)
            .expect("mixed payload should translate");

        assert_eq!(translated.family, VerifiedFamily::ClientCharList);
        assert_eq!(translated.proxy_ack_client_sequence, None);
        assert!(!translated.elide_client_sequence);
        let out = translated.packet.expect("forwarded unit should remain");
        let out_view = MFrameView::parse(&out).expect("rewritten frame should parse");
        assert!(out_view.crc_valid);
        assert_eq!(out_view.payload_length, 3);
        assert_eq!(out_view.trailing_payload_length, 0);
        assert_eq!(
            out_view.high.map(|high| (high.major, high.minor)),
            Some((0x11, 0x01))
        );
    }

    #[test]
    fn device_advertise_property_becomes_empty_server_paced_carrier() {
        let payload = valid_device_advertise_property_payload();
        let packet = build_client_m_frame(0x002A, 0x0003, &payload);
        let view = MFrameView::parse(&packet).expect("fixture should parse as M frame");

        let translated = consume_device_advertise_property(&packet, &view, &payload)
            .expect("device should consume");

        assert_eq!(translated.family, VerifiedFamily::ConsumedEmptyMFrame);
        assert_eq!(translated.proxy_ack_client_sequence, None);
        assert!(!translated.elide_client_sequence);
        let out = translated
            .packet
            .expect("empty carrier should be forwarded");
        let out_view = MFrameView::parse(&out).expect("empty carrier should parse");
        assert!(out_view.crc_valid);
        assert_eq!(out_view.sequence, 0x002A);
        assert_eq!(out_view.ack_sequence, 0x0003);
        assert_eq!(out_view.flags, 0x08);
        assert_eq!(out_view.packetized_sequence, 1);
        assert_eq!(out_view.payload_length, 0);
        assert_eq!(out_view.trailing_payload_length, 0);
    }

    #[test]
    fn consumed_client_gui_event_keeps_original_semantic_observation() {
        let payload = client_gui_event_notify_payload(0x8001_53FD);
        let packet = build_client_m_frame(0x003A, 0x0005, &payload);
        let view = MFrameView::parse(&packet).expect("fixture should parse as M frame");
        let mut state = SemanticSessionState::default();

        let translated = translate_client_frame(packet, &view, &mut state)
            .expect("verified ClientGuiEvent should be consumed as proxy-owned");

        assert_eq!(translated.family, VerifiedFamily::ConsumedEmptyMFrame);
        assert_eq!(translated.semantic_observations.len(), 1);
        assert_eq!(
            translated.semantic_observations[0].family,
            VerifiedFamily::ClientGuiEvent
        );
        assert_eq!(translated.semantic_observations[0].payload, payload);
        let out = translated
            .packet
            .expect("GUI event should still produce a server-paced empty carrier");
        let out_view = MFrameView::parse(&out).expect("empty carrier should parse");
        assert!(out_view.crc_valid);
        assert_eq!(out_view.sequence, 0x003A);
        assert_eq!(out_view.ack_sequence, 0x0005);
        assert_eq!(out_view.payload_length, 0);
    }

    #[test]
    fn malformed_device_advertise_property_is_not_semantically_claimed() {
        let payload = [0x70, 0x36, 0x01, 0x04, 0x00, 0x00, 0x00];
        let packet = build_client_m_frame(0x002B, 0x0003, &payload);
        let view = MFrameView::parse(&packet).expect("fixture should parse as M frame");

        assert!(client_device::claim_payload_if_verified(&payload).is_none());
        assert!(
            consume_device_advertise_property(&packet, &view, &payload).is_err(),
            "major/minor alone must not prove Device_AdvertiseProperty ownership"
        );

        let mut state = SemanticSessionState::default();
        let translated = translate_client_frame(packet, &view, &mut state)
            .expect("malformed client high-level is isolated through the quarantine path");
        assert_eq!(translated.family, VerifiedFamily::ConsumedEmptyMFrame);
        let out = translated
            .packet
            .expect("unclaimed client high-level should still be isolated from Diamond");
        let out_view = MFrameView::parse(&out).expect("empty quarantine carrier should parse");
        assert!(out_view.crc_valid);
        assert_eq!(out_view.payload_length, 0);
    }

    #[test]
    fn server_admin_module_run_claims_exact_frame() {
        let packet = build_client_m_frame(0x004B, 0x0009, b"sModule.Run");
        let view = MFrameView::parse(&packet).expect("fixture should parse as admin M frame");
        let mut state = SemanticSessionState::default();

        let translated =
            translate_client_frame(packet.clone(), &view, &mut state).expect("admin should claim");

        assert_eq!(translated.family, VerifiedFamily::ClientServerAdmin);
        assert_eq!(translated.proxy_ack_client_sequence, None);
        assert!(!translated.elide_client_sequence);
        assert_eq!(translated.packet.as_deref(), Some(packet.as_slice()));
    }
}
