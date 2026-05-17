//! Decompile-backed `Login_GetWaypoint` / `Login_WaypointResponse` bridge.
//!
//! Diamond and EE both define server `Login_GetWaypoint` as login family
//! `0x02/0x0C` with no payload body. The client response is
//! `Login_WaypointResponse` (`0x02/0x0D`) and writes exactly one
//! `CExoString` bounded to `0x20` bytes.
//!
//! The harnessed EE client has been observed to stall after character select
//! when it receives the legacy prompt but does not emit the corresponding
//! response promptly. Diamond clients answer with an empty string when no
//! local waypoint tag is available, so this helper queues that exact empty
//! response upstream. This module owns only that login handshake side effect;
//! the central M-frame layer still only routes transport events.

use crate::{
    packet::m::{HighLevel, LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
    translate::client_login,
};

use super::{
    sequence::{SequenceShift, shift_sequence_for_peer, trim_sequence_shifts},
    state::SessionState,
    synthetic_area,
};

const LOGIN_MAJOR: u8 = 0x02;
const LOGIN_GET_WAYPOINT_MINOR: u8 = 0x0C;
const LOGIN_WAYPOINT_RESPONSE_MINOR: u8 = 0x0D;
const HIGH_LEVEL_ENVELOPE: u8 = 0x70;
const CNW_DECLARED_EMPTY_CEXOSTRING_END: u32 = 11;
const EMPTY_CEXOSTRING_LEN: u32 = 0;
const FINAL_FRAGMENT_BYTE: u8 = 0x60;

pub(super) fn maybe_queue_empty_waypoint_response(
    state: &mut SessionState,
    bytes: &[u8],
    view: &MFrameView,
) -> anyhow::Result<()> {
    let payload_end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET.saturating_add(view.payload_length);
    let Some(payload) = bytes.get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..payload_end) else {
        return Ok(());
    };
    if !is_login_get_waypoint_payload(payload) {
        return Ok(());
    }

    maybe_queue_empty_waypoint_response_payload(state, payload, view.sequence, view.ack_sequence)
}

pub(super) fn maybe_queue_empty_waypoint_response_payload(
    state: &mut SessionState,
    payload: &[u8],
    server_sequence: u16,
    server_ack_sequence: u16,
) -> anyhow::Result<()> {
    if !is_login_get_waypoint_payload(payload) {
        return Ok(());
    }

    if state
        .login_waypoint
        .last_server_get_waypoint_sequence
        .is_some_and(|sequence| sequence == server_sequence)
    {
        tracing::debug!(
            sequence = server_sequence,
            "duplicate Login_GetWaypoint server sequence already has a queued synthetic response"
        );
        return Ok(());
    }

    let Some(latest_client_sequence) = state.sequence.latest_client_sequence_from_client else {
        tracing::warn!(
            server_sequence,
            server_ack_sequence,
            "cannot queue Login_WaypointResponse because no client sequence has been observed"
        );
        return Ok(());
    };

    let original_sequence = latest_client_sequence.wrapping_add(1);
    let shifted_sequence =
        shift_sequence_for_peer(&state.sequence.client_sequence_shifts, original_sequence);
    let payload = build_empty_waypoint_response_payload();
    debug_assert!(client_login::waypoint_response_payload_shape_valid(
        &payload
    ));

    let packet =
        synthetic_area::build_synthetic_gameplay_frame(shifted_sequence, server_sequence, &payload)?;

    state.sequence.pending_client_to_server_packets.push(packet);
    state.sequence.client_sequence_shifts.push(SequenceShift {
        base: original_sequence,
        delta: 1,
    });
    trim_sequence_shifts(&mut state.sequence.client_sequence_shifts);
    state.login_waypoint.last_server_get_waypoint_sequence = Some(server_sequence);
    state.login_waypoint.synthetic_empty_response_count = state
        .login_waypoint
        .synthetic_empty_response_count
        .saturating_add(1);

    tracing::info!(
        original_sequence,
        shifted_sequence,
        ack_sequence = server_sequence,
        server_ack_sequence,
        count = state.login_waypoint.synthetic_empty_response_count,
        "queued synthetic empty Login_WaypointResponse for legacy Login_GetWaypoint"
    );

    Ok(())
}

fn is_login_get_waypoint_payload(payload: &[u8]) -> bool {
    let Some(high) = HighLevel::parse(payload) else {
        return false;
    };
    high.major == LOGIN_MAJOR && high.minor == LOGIN_GET_WAYPOINT_MINOR && payload.len() == 3
}

fn build_empty_waypoint_response_payload() -> [u8; 12] {
    let mut payload = [0u8; 12];
    payload[0] = HIGH_LEVEL_ENVELOPE;
    payload[1] = LOGIN_MAJOR;
    payload[2] = LOGIN_WAYPOINT_RESPONSE_MINOR;
    payload[3..7].copy_from_slice(&CNW_DECLARED_EMPTY_CEXOSTRING_END.to_le_bytes());
    payload[7..11].copy_from_slice(&EMPTY_CEXOSTRING_LEN.to_le_bytes());
    payload[11] = FINAL_FRAGMENT_BYTE;
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_waypoint_response_payload_matches_client_login_validator() {
        let payload = build_empty_waypoint_response_payload();

        assert!(client_login::waypoint_response_payload_shape_valid(
            &payload
        ));
        assert_eq!(
            payload,
            [0x70, 0x02, 0x0D, 0x0B, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0x60]
        );
    }

    #[test]
    fn queues_one_synthetic_response_and_shifts_client_sequence() {
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(74);
        let get_waypoint =
            synthetic_area::build_synthetic_gameplay_frame(21, 74, &[0x70, 0x02, 0x0C]).unwrap();
        let view = MFrameView::parse(&get_waypoint).unwrap();

        maybe_queue_empty_waypoint_response(&mut state, &get_waypoint, &view).unwrap();

        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        let pending = state.sequence.pending_client_to_server_packets.remove(0);
        let pending_view = MFrameView::parse(&pending).unwrap();
        assert_eq!(pending_view.sequence, 75);
        assert_eq!(pending_view.ack_sequence, 21);
        assert!(client_login::waypoint_response_payload_shape_valid(
            &pending[LEGACY_GAMEPLAY_PAYLOAD_OFFSET
                ..LEGACY_GAMEPLAY_PAYLOAD_OFFSET + pending_view.payload_length]
        ));
        assert_eq!(state.sequence.latest_client_sequence_from_client, Some(74));
        assert_eq!(state.sequence.client_sequence_shifts.len(), 1);
        assert_eq!(
            state.login_waypoint.last_server_get_waypoint_sequence,
            Some(21)
        );
    }

    #[test]
    fn duplicate_server_sequence_is_not_queued_twice() {
        let mut state = SessionState::default();
        state.sequence.latest_client_sequence_from_client = Some(74);
        let get_waypoint =
            synthetic_area::build_synthetic_gameplay_frame(21, 74, &[0x70, 0x02, 0x0C]).unwrap();
        let view = MFrameView::parse(&get_waypoint).unwrap();

        maybe_queue_empty_waypoint_response(&mut state, &get_waypoint, &view).unwrap();
        maybe_queue_empty_waypoint_response(&mut state, &get_waypoint, &view).unwrap();

        assert_eq!(state.sequence.pending_client_to_server_packets.len(), 1);
        assert_eq!(state.login_waypoint.synthetic_empty_response_count, 1);
    }
}
