//! Diagnostic-only whole-packet audit for the terminal compact-tail9 EE stage.
//!
//! The normal translator already has a typed byte/bit plan for the terminal
//! door/placeable row.  That row-local proof is weaker than an exact P/05/01
//! claim: a fragment-only continuation can remain after the row reader stops.
//! This module materializes the staged packet on the heap and runs the same
//! exact validator used at the wire boundary.  Its result is evidence only and
//! cannot authorize a claim, rewrite, cursor advance, or fragment trim.

use super::record::TerminalDoorPlaceableTail9EeStage;
use super::{
    LiveObjectPayloadClaimReject, LiveObjectPayloadClaimRejectStage, LiveObjectUpdateRewriteFailure,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TerminalEeWholePacketAudit {
    pub(super) candidate_payload_length: usize,
    pub(super) candidate_read_buffer_end: usize,
    pub(super) candidate_fragment_bit_end: usize,
    pub(super) typed_row_read_buffer_cursor: usize,
    pub(super) typed_row_read_buffer_end: usize,
    pub(super) typed_row_fragment_cursor: usize,
    pub(super) exact_payload_validator_accepted: bool,
    pub(super) reject: Option<LiveObjectPayloadClaimReject>,
}

impl TerminalEeWholePacketAudit {
    pub(super) fn status(self) -> &'static str {
        if self.exact_payload_validator_accepted {
            "exact-payload-accepted"
        } else {
            "exact-payload-rejected"
        }
    }

    pub(super) fn reject_stage(self) -> Option<LiveObjectPayloadClaimRejectStage> {
        self.reject.map(|reject| reject.stage)
    }

    pub(super) fn authorizes_claim(self) -> bool {
        false
    }

    pub(super) fn authorizes_rewrite(self) -> bool {
        false
    }

    pub(super) fn authorizes_cursor_advance(self) -> bool {
        false
    }

    pub(super) fn authorizes_fragment_trim(self) -> bool {
        false
    }
}

pub(super) fn audit_staged_terminal_ee_candidate(
    stage: &TerminalDoorPlaceableTail9EeStage,
) -> Option<TerminalEeWholePacketAudit> {
    // The stage is created only after the typed EE row reader consumes the
    // complete emitted read buffer. Keep those terminal cursor invariants
    // explicit here so a future caller cannot turn an arbitrary partial stage
    // into apparently authoritative whole-packet evidence.
    if stage.typed_row_read_buffer_cursor != stage.typed_row_read_buffer_end
        || stage.typed_row_read_buffer_end != stage.live_bytes.len()
        || stage.typed_row_fragment_cursor > stage.candidate_fragment_bit_end
        || stage.candidate_fragment_bit_end != stage.fragment_bits.len()
    {
        return None;
    }

    let candidate = super::live_object_payload_from_parts(&stage.live_bytes, &stage.fragment_bits)?;
    let validation = super::claim_payload_if_verified_with_reject(&candidate);
    let (exact_payload_validator_accepted, reject) = match validation {
        Ok(_) => (true, None),
        Err(reject) => (false, Some(reject)),
    };

    Some(TerminalEeWholePacketAudit {
        candidate_payload_length: candidate.len(),
        candidate_read_buffer_end: stage.live_bytes.len(),
        candidate_fragment_bit_end: stage.fragment_bits.len(),
        typed_row_read_buffer_cursor: stage.typed_row_read_buffer_cursor,
        typed_row_read_buffer_end: stage.typed_row_read_buffer_end,
        typed_row_fragment_cursor: stage.typed_row_fragment_cursor,
        exact_payload_validator_accepted,
        reject,
    })
}

/// Rerun the transactional update translator solely to recover the heap-backed
/// terminal stage at the exact failure boundary. Binding the rerun back to the
/// supplied compact failure prevents diagnostics for a later orchestration
/// candidate from being mislabeled as evidence for a different terminal
/// failure contract. The artifact separately records its paired payload digest.
pub(super) fn audit_terminal_ee_writer_candidate(
    payload: &[u8],
    expected_failure: LiveObjectUpdateRewriteFailure,
) -> Option<TerminalEeWholePacketAudit> {
    let expected_requirement = expected_failure
        .terminal_door_placeable_tail9_residual?
        .writer_handoff_requirement()?;
    let mut replay = payload.to_vec();
    let mut replay_failure = None;
    let mut audit = None;
    let summary = super::rewrite_update_records_payload_with_area_context_inner(
        &mut replay,
        None,
        &mut replay_failure,
        Some(&mut audit),
    );
    if summary.is_some() {
        return None;
    }
    let replay_failure = replay_failure?;
    let replay_requirement = replay_failure
        .terminal_door_placeable_tail9_residual?
        .writer_handoff_requirement()?;
    if replay_failure.kind != expected_failure.kind
        || replay_failure.offset != expected_failure.offset
        || replay_failure.record_end != expected_failure.record_end
        || replay_failure.bit_cursor != expected_failure.bit_cursor
        || replay_requirement != expected_requirement
    {
        return None;
    }

    let audit = audit?;
    if audit.candidate_read_buffer_end != expected_requirement.emitted_read_buffer_end
        || audit.typed_row_read_buffer_cursor != expected_requirement.emitted_read_buffer_cursor
        || audit.typed_row_read_buffer_end != expected_requirement.emitted_read_buffer_end
        || audit.typed_row_fragment_cursor != expected_requirement.emitted_fragment_bit_start
        || audit.candidate_fragment_bit_end != expected_requirement.emitted_fragment_bit_end
    {
        return None;
    }
    Some(audit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::live_object_update::{
        CNW_FRAGMENT_HEADER_BITS, LEGACY_UPDATE_STATE_MASK,
    };

    fn exact_state_only_placeable_stage() -> TerminalDoorPlaceableTail9EeStage {
        let mut live_bytes = vec![b'U', 0x09];
        live_bytes.extend_from_slice(&0x8000_1003u32.to_le_bytes());
        live_bytes.extend_from_slice(&LEGACY_UPDATE_STATE_MASK.to_le_bytes());
        let mut fragment_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        // EE placeable state is exactly five BOOLs in decompile reader order.
        fragment_bits.extend([false, false, true, false, false]);
        TerminalDoorPlaceableTail9EeStage {
            typed_row_read_buffer_cursor: live_bytes.len(),
            typed_row_read_buffer_end: live_bytes.len(),
            typed_row_fragment_cursor: fragment_bits.len(),
            candidate_fragment_bit_end: fragment_bits.len(),
            live_bytes,
            fragment_bits,
        }
    }

    #[test]
    fn exact_whole_packet_control_is_observed_but_never_authorized() {
        let audit = audit_staged_terminal_ee_candidate(&exact_state_only_placeable_stage())
            .expect("bounded exact EE stage");
        assert!(audit.exact_payload_validator_accepted);
        assert_eq!(audit.status(), "exact-payload-accepted");
        assert_eq!(audit.reject, None);
        assert!(!audit.authorizes_claim());
        assert!(!audit.authorizes_rewrite());
        assert!(!audit.authorizes_cursor_advance());
        assert!(!audit.authorizes_fragment_trim());
    }

    #[test]
    fn whole_packet_validator_rejects_fragment_after_exact_typed_row() {
        let mut stage = exact_state_only_placeable_stage();
        let typed_row_fragment_cursor = stage.typed_row_fragment_cursor;
        stage.fragment_bits.push(false);
        stage.candidate_fragment_bit_end = stage.fragment_bits.len();

        let audit = audit_staged_terminal_ee_candidate(&stage).expect("bounded residual stage");
        assert!(!audit.exact_payload_validator_accepted);
        assert_eq!(audit.status(), "exact-payload-rejected");
        assert_eq!(
            audit.reject_stage(),
            Some(LiveObjectPayloadClaimRejectStage::FragmentCursor)
        );
        assert_eq!(
            audit.reject.and_then(|reject| reject.offset),
            Some(stage.live_bytes.len())
        );
        assert_eq!(
            audit.reject.and_then(|reject| reject.bit_cursor),
            Some(typed_row_fragment_cursor)
        );
        assert!(!audit.authorizes_claim());
        assert!(!audit.authorizes_rewrite());
        assert!(!audit.authorizes_cursor_advance());
        assert!(!audit.authorizes_fragment_trim());
    }

    #[test]
    fn malformed_stage_cursor_contract_fails_closed() {
        let mut stage = exact_state_only_placeable_stage();
        stage.candidate_fragment_bit_end = stage.fragment_bits.len().saturating_add(1);
        assert!(audit_staged_terminal_ee_candidate(&stage).is_none());

        let mut stage = exact_state_only_placeable_stage();
        stage.typed_row_read_buffer_cursor = stage.typed_row_read_buffer_end.saturating_sub(1);
        assert!(audit_staged_terminal_ee_candidate(&stage).is_none());
    }
}
