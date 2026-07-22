//! Exact ACK-only carriers for source frames whose payload cannot be emitted.
//!
//! Reliable `M` frames carry two independent facts: type-0 payload admission
//! and the cumulative ACK for the opposite reliable lane.  A payload may be
//! consumed, held, or rejected without invalidating an ACK that already passed
//! the source CRC and writer-shape boundary.  Preserve that ACK with the exact
//! type-1 control shape when the translated batch does not already carry that
//! exact mapped progress.

use crate::{
    crc::{encode_legacy_m_crc, write_be_u16},
    packet::m::{LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameType, MFrameView},
    translate::{Emit, VerifiedFamily, VerifiedProof},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::translate) struct PreparedSourceAckCarrier {
    pub(super) source_ack_sequence: u16,
    pub(super) mapped_ack_sequence: u16,
    packet: Vec<u8>,
}

pub(super) fn prepare(
    source_ack_sequence: u16,
    mapped_ack_sequence: u16,
) -> anyhow::Result<PreparedSourceAckCarrier> {
    Ok(PreparedSourceAckCarrier {
        source_ack_sequence,
        mapped_ack_sequence,
        packet: build_exact_ack_control_frame(mapped_ack_sequence)?,
    })
}

/// Build the fixed 12-byte ACK control emitted by the original window writer.
///
/// Diamond `sub_5F36E0` lines 751193-751321 and EE
/// `CNetLayerWindow::FrameSend` lines 879829-879929 allocate a zeroed 12-byte
/// frame, write the supplied transport sequence/ACK fields, select the frame
/// kind in byte 7, and finish with the ordinary `M` CRC. Stock type-1 callers
/// pass sequence zero (Diamond 752003-752009 / 753771-753780; EE
/// 880555-880563 / 903893-903900). Controls do not occupy the type-0 receive
/// window, and this family has no CNW payload or bit cursor.
pub(super) fn build_exact_ack_control_frame(ack_sequence: u16) -> anyhow::Result<Vec<u8>> {
    let mut packet = vec![0; LEGACY_GAMEPLAY_PAYLOAD_OFFSET];
    packet[0] = b'M';
    write_be_u16(&mut packet, 3, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write ACK-control sequence"))?;
    write_be_u16(&mut packet, 5, ack_sequence)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to write ACK-control cumulative ACK"))?;
    packet[7] = 0x10;
    write_be_u16(&mut packet, 8, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear ACK-control packetized sequence"))?;
    write_be_u16(&mut packet, 10, 0)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to clear ACK-control payload length"))?;
    encode_legacy_m_crc(&mut packet)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("failed to encode ACK-control CRC"))?;

    let view = MFrameView::parse(&packet)
        .ok_or_else(|| anyhow::anyhow!("built ACK-control frame failed to parse"))?;
    if !view.crc_valid
        || view.frame_kind() != Some(MFrameType::AckControl)
        || !view.is_exact_control_frame()
        || view.sequence != 0
        || view.ack_sequence != ack_sequence
    {
        anyhow::bail!("built ACK-control frame failed exact writer self-validation");
    }

    Ok(packet)
}

pub(super) fn ensure_carried(
    emit: Emit,
    prepared: PreparedSourceAckCarrier,
    source_lane: &'static str,
) -> Emit {
    if emit_carries_exact_ack(&emit, prepared.mapped_ack_sequence) {
        return emit;
    }

    tracing::info!(
        source_lane,
        source_ack_sequence = prepared.source_ack_sequence,
        mapped_ack_sequence = prepared.mapped_ack_sequence,
        "inserted exact type-1 carrier for independently valid source ACK"
    );
    append_carrier(emit, prepared.packet)
}

fn emit_carries_exact_ack(emit: &Emit, expected_ack_sequence: u16) -> bool {
    let mut covered = false;
    visit_emit_packets(emit, &mut |packet| {
        let Some(view) = MFrameView::parse(packet) else {
            return;
        };
        if !view.crc_valid
            || view.frame_kind().is_none()
            || (view.frame_kind() != Some(MFrameType::ReliableData)
                && !view.is_exact_control_frame())
        {
            return;
        }
        // Require the exact mapped ACK. A newer ACK can still be sparse inside
        // the 16-slot allocation; retirement correctly rejects that gap, so it
        // must not suppress the exact contiguous-prefix carrier.
        covered |= view.ack_sequence == expected_ack_sequence;
    });
    covered
}

fn append_carrier(emit: Emit, carrier: Vec<u8>) -> Emit {
    match emit {
        Emit::Packet(packet) => Emit::Packets(vec![packet, carrier]),
        Emit::Packets(mut packets) => {
            packets.push(carrier);
            Emit::Packets(packets)
        }
        Emit::PacketsPreShifted(mut packets) => {
            packets.push(carrier);
            Emit::PacketsPreShifted(packets)
        }
        Emit::VerifiedPackets { family, packets } => {
            let mut mixed = packets
                .into_iter()
                .map(|packet| (family, packet))
                .collect::<Vec<_>>();
            mixed.push((VerifiedFamily::ConsumedEmptyMFrame, carrier));
            Emit::MixedVerifiedPackets(mixed)
        }
        Emit::VerifiedPacketsPreShifted { family, packets } => {
            let mut mixed = packets
                .into_iter()
                .map(|packet| (VerifiedProof::family(family), packet))
                .collect::<Vec<_>>();
            mixed.push((
                VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
                carrier,
            ));
            Emit::MixedVerifiedProofPacketsPreShifted(mixed)
        }
        Emit::MixedVerifiedPackets(mut packets) => {
            packets.push((VerifiedFamily::ConsumedEmptyMFrame, carrier));
            Emit::MixedVerifiedPackets(packets)
        }
        Emit::VerifiedProofPackets { proof, packets } => {
            let mut mixed = packets
                .into_iter()
                .map(|packet| (proof.clone(), packet))
                .collect::<Vec<_>>();
            mixed.push((
                VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
                carrier,
            ));
            Emit::MixedVerifiedProofPackets(mixed)
        }
        Emit::VerifiedProofPacketsPreShifted { proof, packets } => {
            let mut mixed = packets
                .into_iter()
                .map(|packet| (proof.clone(), packet))
                .collect::<Vec<_>>();
            mixed.push((
                VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
                carrier,
            ));
            Emit::MixedVerifiedProofPacketsPreShifted(mixed)
        }
        Emit::MixedVerifiedProofPackets(mut packets) => {
            packets.push((
                VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
                carrier,
            ));
            Emit::MixedVerifiedProofPackets(packets)
        }
        Emit::MixedVerifiedProofPacketsPreShifted(mut packets) => {
            packets.push((
                VerifiedProof::family(VerifiedFamily::ConsumedEmptyMFrame),
                carrier,
            ));
            Emit::MixedVerifiedProofPacketsPreShifted(packets)
        }
        Emit::Consumed | Emit::Drop => Emit::VerifiedPackets {
            family: VerifiedFamily::ConsumedEmptyMFrame,
            packets: vec![carrier],
        },
        emit @ (Emit::PacketRetireSession { .. } | Emit::ConsumedRetireSession { .. }) => {
            tracing::debug!(
                "session-retiring M disposition intentionally suppresses a separate ACK carrier"
            );
            emit
        }
    }
}

fn visit_emit_packets(emit: &Emit, visitor: &mut impl FnMut(&[u8])) {
    match emit {
        Emit::Packet(packet) | Emit::PacketRetireSession { packet, .. } => visitor(packet),
        Emit::Packets(packets)
        | Emit::PacketsPreShifted(packets)
        | Emit::VerifiedPackets { packets, .. }
        | Emit::VerifiedPacketsPreShifted { packets, .. }
        | Emit::VerifiedProofPackets { packets, .. }
        | Emit::VerifiedProofPacketsPreShifted { packets, .. } => {
            for packet in packets {
                visitor(packet);
            }
        }
        Emit::MixedVerifiedPackets(packets) => {
            for (_, packet) in packets {
                visitor(packet);
            }
        }
        Emit::MixedVerifiedProofPackets(packets)
        | Emit::MixedVerifiedProofPacketsPreShifted(packets) => {
            for (_, packet) in packets {
                visitor(packet);
            }
        }
        Emit::Consumed | Emit::ConsumedRetireSession { .. } | Emit::Drop => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_ack_control_writer_owns_only_the_fixed_transport_fields() {
        for ack_sequence in [0, 1, 0xffff] {
            let packet = build_exact_ack_control_frame(ack_sequence).expect("exact ACK control");
            let view = MFrameView::parse(&packet).expect("parse exact ACK control");
            assert_eq!(packet.len(), LEGACY_GAMEPLAY_PAYLOAD_OFFSET);
            assert_eq!(view.sequence, 0);
            assert_eq!(view.ack_sequence, ack_sequence);
            assert_eq!(view.flags, 0x10);
            assert_eq!(view.packetized_sequence, 0);
            assert_eq!(view.declared_payload_length, 0);
            assert_eq!(view.available_payload_length, 0);
            assert!(view.crc_valid);
            assert!(view.is_exact_control_frame());
        }
    }

    #[test]
    fn exact_output_ack_suppresses_redundant_carrier() {
        let existing = build_exact_ack_control_frame(10).expect("existing ACK");
        let emit = Emit::Packet(existing);
        let prepared = prepare(10, 10).expect("prepared ACK carrier");
        let Emit::Packet(packet) = ensure_carried(emit, prepared, "test") else {
            panic!("the exact cumulative ACK should preserve the original single packet");
        };
        assert_eq!(MFrameView::parse(&packet).unwrap().ack_sequence, 10);
    }
}
