//! Local reliable-window ACKs for consumed server frames.
//!
//! Strict deflated reassembly intentionally withholds legacy server payloads
//! from EE until the inflated gameplay bytes are classified and rewritten. The
//! 1.69 server, however, still runs a normal reliable send window: if the proxy
//! consumes several packetized frames and waits for a later frame, the server
//! may stop sending because no peer has ACKed the frames already buffered.
//!
//! This module makes the proxy act like a narrow reliable-window endpoint for
//! frames it has already consumed into a strict reassembly buffer. It sends only
//! empty `M` ACK/control frames upstream. No gameplay payload is forwarded or
//! invented here; semantic delivery still happens through the rebuilt
//! translator-owned server-to-client frames.

use crate::translate::VerifiedFamily;

use super::{SessionState, ack_carrier::build_exact_ack_control_frame, state::PendingClientPacket};

pub(super) fn queue_consumed_server_frame_ack(
    state: &mut SessionState,
    ack_sequence: u16,
    reason: &'static str,
) -> anyhow::Result<()> {
    let packet = build_exact_ack_control_frame(ack_sequence)?;
    state
        .sequence
        .pending_client_to_server_packets
        .push(PendingClientPacket {
            family: VerifiedFamily::ConsumedEmptyMFrame,
            packet,
            reason,
        });
    tracing::info!(
        ack_sequence,
        pending_local_acks = state.sequence.pending_client_to_server_packets.len(),
        reason,
        "queued local client->server ACK for consumed server M frame"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::packet::m::MFrameView;

    use super::*;

    #[test]
    fn empty_ack_control_frame_is_parseable_and_payload_free() {
        let packet = build_exact_ack_control_frame(40).expect("ack frame");
        let view = MFrameView::parse(&packet).expect("parse M frame");
        assert_eq!(view.sequence, 0);
        assert_eq!(view.ack_sequence, 40);
        assert_eq!(view.flags, 0x10);
        assert_eq!(view.packetized_sequence, 0);
        assert_eq!(view.payload_length, 0);
    }
}
