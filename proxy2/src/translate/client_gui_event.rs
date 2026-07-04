//! Client-originated `GuiEvent_Notify` (`0x35/0x01`) semantic ownership.
//!
//! Evidence:
//! - EE names `0x35/0x01` as `GuiEvent_Notify` in the high-level message
//!   table.
//! - EE `CNWSMessage::HandlePlayerToServerGuiEvent` reads minor `1` as:
//!   `WORD event_a`, `WORD event_b`, `OBJECTID object`, and, when the server
//!   satisfies build `8193.35`, three 32-bit floats.
//! - EE client writer `sub_1407BDF30` writes the same fields and conditionally
//!   appends the vector under `ServerSatisfiesBuild(8193, 35, 0)`.
//! - EE `CNWMessage::GetWriteMessage` stores the fragment cursor in the high
//!   three bits of the final fragment byte (`cursor << 5`) and preserves the
//!   low five residual fragment bits. Captures therefore legitimately differ
//!   between `0x60` and `0x70` while still proving the same cursor position.
//! - The Diamond server decompile does not expose an equivalent
//!   `HandlePlayerToServerGuiEvent`/`GuiEvent` dispatch family. Until a legacy
//!   handler is proven, this module claims the EE event and the M layer
//!   consumes it instead of leaking an EE-only high-level packet to 1.69.

use crate::packet::m::HighLevel;

const GUI_EVENT_MAJOR: u8 = 0x35;
const GUI_EVENT_NOTIFY_MINOR: u8 = 0x01;
const DECLARED_OFFSET: usize = 3;
const BODY_OFFSET: usize = 7;
const LEGACY_NOTIFY_DECLARED_BYTES: usize = BODY_OFFSET + 2 + 2 + 4;
pub(crate) const EE_8193_35_NOTIFY_DECLARED_BYTES: usize = LEGACY_NOTIFY_DECLARED_BYTES + 12;
const FRAGMENT_CURSOR_MASK: u8 = 0xE0;
const EXPECTED_FINAL_FRAGMENT_CURSOR: u8 = 0x60;
pub(crate) const RADIAL_NOTIFY_PROBE_TRAILING_FRAGMENT_BYTES: usize = 1;
pub(crate) const RADIAL_NOTIFY_PROBE_EVENT_A: u16 = 0x0011;
pub(crate) const RADIAL_NOTIFY_PROBE_EVENT_B: u16 = 0x0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientGuiEventLegacyAction {
    /// No Diamond/1.69 server-side equivalent has been found in the decompile.
    /// The gateway must consume the packet rather than forwarding it raw.
    ConsumeNoLegacyEquivalent,
}

#[derive(Debug, Clone, Copy)]
pub struct ClientGuiEventClaimSummary {
    pub packet_name: &'static str,
    pub event_a: u16,
    pub event_b: u16,
    pub object_id: u32,
    pub vector: Option<[f32; 3]>,
    pub declared_bytes: usize,
    pub trailing_fragment_bytes: usize,
    pub legacy_action: ClientGuiEventLegacyAction,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientGuiEventClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != GUI_EVENT_MAJOR || high.minor != GUI_EVENT_NOTIFY_MINOR {
        return None;
    }

    let declared_bytes = read_le_u32(payload, DECLARED_OFFSET)? as usize;
    if declared_bytes != LEGACY_NOTIFY_DECLARED_BYTES
        && declared_bytes != EE_8193_35_NOTIFY_DECLARED_BYTES
    {
        return None;
    }
    if declared_bytes > payload.len() {
        return None;
    }

    let trailing_fragment = payload.get(declared_bytes..)?;
    if !trailing_fragment_shape_valid(trailing_fragment) {
        return None;
    }

    let event_a = read_le_u16(payload, BODY_OFFSET)?;
    let event_b = read_le_u16(payload, BODY_OFFSET + 2)?;
    let object_id = read_le_u32(payload, BODY_OFFSET + 4)?;
    let vector = if declared_bytes == EE_8193_35_NOTIFY_DECLARED_BYTES {
        Some([
            read_le_f32(payload, LEGACY_NOTIFY_DECLARED_BYTES)?,
            read_le_f32(payload, LEGACY_NOTIFY_DECLARED_BYTES + 4)?,
            read_le_f32(payload, LEGACY_NOTIFY_DECLARED_BYTES + 8)?,
        ])
    } else {
        None
    };

    Some(ClientGuiEventClaimSummary {
        packet_name: "GuiEvent_Notify",
        event_a,
        event_b,
        object_id,
        vector,
        declared_bytes,
        trailing_fragment_bytes: trailing_fragment.len(),
        legacy_action: ClientGuiEventLegacyAction::ConsumeNoLegacyEquivalent,
    })
}

pub fn build_notify_payload(
    event_a: u16,
    event_b: u16,
    object_id: u32,
    vector: Option<[f32; 3]>,
) -> Option<Vec<u8>> {
    let declared_bytes = if vector.is_some() {
        EE_8193_35_NOTIFY_DECLARED_BYTES
    } else {
        LEGACY_NOTIFY_DECLARED_BYTES
    };
    let mut payload = Vec::with_capacity(declared_bytes.checked_add(1)?);
    payload.extend_from_slice(&[0x70, GUI_EVENT_MAJOR, GUI_EVENT_NOTIFY_MINOR]);
    payload.extend_from_slice(&(u32::try_from(declared_bytes).ok()?).to_le_bytes());
    payload.extend_from_slice(&event_a.to_le_bytes());
    payload.extend_from_slice(&event_b.to_le_bytes());
    payload.extend_from_slice(&object_id.to_le_bytes());
    if let Some(vector) = vector {
        for component in vector {
            payload.extend_from_slice(&component.to_bits().to_le_bytes());
        }
    }
    payload.push(EXPECTED_FINAL_FRAGMENT_CURSOR);
    claim_payload_if_verified(&payload)?;
    Some(payload)
}

pub fn build_radial_notify_probe_payload(object_id: u32) -> Option<Vec<u8>> {
    build_notify_payload(
        RADIAL_NOTIFY_PROBE_EVENT_A,
        RADIAL_NOTIFY_PROBE_EVENT_B,
        object_id,
        Some([0.0, 0.0, 0.0]),
    )
}

fn read_le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes(slice.try_into().ok()?))
}

fn trailing_fragment_shape_valid(bytes: &[u8]) -> bool {
    match bytes {
        [] => true,
        [byte] => (byte & FRAGMENT_CURSOR_MASK) == EXPECTED_FINAL_FRAGMENT_CURSOR,
        _ => false,
    }
}

fn read_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn read_le_f32(bytes: &[u8], offset: usize) -> Option<f32> {
    Some(f32::from_bits(read_le_u32(bytes, offset)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_starc5_ee_gui_event_notify_vector_shape_is_claimed() {
        let payload = [
            0x70, 0x35, 0x01, 0x1B, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x70,
        ];

        let summary = claim_payload_if_verified(&payload)
            .expect("observed EE GuiEvent_Notify should be claimed");

        assert_eq!(summary.packet_name, "GuiEvent_Notify");
        assert_eq!(summary.event_a, 0x0011);
        assert_eq!(summary.event_b, 0);
        assert_eq!(summary.object_id, 0x7F000000);
        assert_eq!(summary.vector, Some([0.0, 0.0, 0.0]));
        assert_eq!(summary.declared_bytes, EE_8193_35_NOTIFY_DECLARED_BYTES);
        assert_eq!(summary.trailing_fragment_bytes, 1);
        assert_eq!(
            summary.legacy_action,
            ClientGuiEventLegacyAction::ConsumeNoLegacyEquivalent
        );
    }

    #[test]
    fn builds_exact_gui_event_notify_radial_probe_shape() {
        let payload = build_radial_notify_probe_payload(0x8000_0100)
            .expect("radial notify probe should build");

        assert_eq!(
            payload,
            [
                0x70, 0x35, 0x01, 0x1B, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
                0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x60,
            ]
        );

        let summary = claim_payload_if_verified(&payload)
            .expect("built radial notify probe should satisfy the focused verifier");
        assert_eq!(summary.packet_name, "GuiEvent_Notify");
        assert_eq!(summary.event_a, RADIAL_NOTIFY_PROBE_EVENT_A);
        assert_eq!(summary.event_b, RADIAL_NOTIFY_PROBE_EVENT_B);
        assert_eq!(summary.object_id, 0x8000_0100);
        assert_eq!(summary.vector, Some([0.0, 0.0, 0.0]));
        assert_eq!(summary.declared_bytes, EE_8193_35_NOTIFY_DECLARED_BYTES);
        assert_eq!(summary.trailing_fragment_bytes, 1);
    }

    #[test]
    fn builds_exact_gui_event_notify_without_vector_shape() {
        let payload = build_notify_payload(2, 3, 0xDEAD_BEEF, None)
            .expect("legacy-sized notify should build");

        let summary = claim_payload_if_verified(&payload)
            .expect("built legacy-sized notify should satisfy the focused verifier");
        assert_eq!(summary.event_a, 2);
        assert_eq!(summary.event_b, 3);
        assert_eq!(summary.object_id, 0xDEAD_BEEF);
        assert_eq!(summary.vector, None);
        assert_eq!(summary.declared_bytes, LEGACY_NOTIFY_DECLARED_BYTES);
        assert_eq!(summary.trailing_fragment_bytes, 1);
    }

    #[test]
    fn legacy_gui_event_notify_without_vector_shape_is_claimed() {
        let payload = [
            0x70, 0x35, 0x01, 0x0F, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03, 0x00, 0xEF, 0xBE, 0xAD,
            0xDE,
        ];

        let summary = claim_payload_if_verified(&payload)
            .expect("legacy-sized GuiEvent_Notify should be claimed");

        assert_eq!(summary.event_a, 2);
        assert_eq!(summary.event_b, 3);
        assert_eq!(summary.object_id, 0xDEADBEEF);
        assert_eq!(summary.vector, None);
        assert_eq!(summary.declared_bytes, LEGACY_NOTIFY_DECLARED_BYTES);
        assert_eq!(summary.trailing_fragment_bytes, 0);
    }

    #[test]
    fn gui_event_notify_rejects_unproven_declared_lengths() {
        let payload = [
            0x70, 0x35, 0x01, 0x13, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03, 0x00, 0xEF, 0xBE, 0xAD,
            0xDE, 0x00, 0x00, 0x00, 0x00,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn gui_event_notify_rejects_unknown_fragment_cursor() {
        let payload = [
            0x70, 0x35, 0x01, 0x0F, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03, 0x00, 0xEF, 0xBE, 0xAD,
            0xDE, 0x80,
        ];

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn gui_event_notify_accepts_decompile_cursor_with_residual_low_bits() {
        let payload = [
            0x70, 0x35, 0x01, 0x0F, 0x00, 0x00, 0x00, 0x02, 0x00, 0x03, 0x00, 0xEF, 0xBE, 0xAD,
            0xDE, 0x7F,
        ];

        let summary = claim_payload_if_verified(&payload)
            .expect("low residual fragment bits should not invalidate cursor proof");

        assert_eq!(summary.trailing_fragment_bytes, 1);
    }
}
