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
    LEGACY_UPDATE_HEADER_BYTES, LiveObjectPayloadClaimReject, LiveObjectPayloadClaimRejectStage,
    LiveObjectUpdatePackedFragmentBitSpanEvidence, LiveObjectUpdateRewriteFailure,
    LiveObjectUpdateTerminalWriterHandoffRequirement,
};

/// Opaque proof that the typed EE stage consumed the complete candidate and
/// that the existing whole-payload validator accepted those exact bytes and
/// MSB-first fragment values. Only this module can construct the token.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ExactTerminalEeFinalClaim {
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    source_payload: Option<Box<[u8]>>,
    candidate_payload: Box<[u8]>,
    read_buffer_cursor: usize,
    read_buffer_end: usize,
    fragment_bits_written: LiveObjectUpdatePackedFragmentBitSpanEvidence,
    final_fragment_bit_cursor: usize,
    final_fragment_bit_end: usize,
}

impl ExactTerminalEeFinalClaim {
    pub(super) fn matches(
        &self,
        requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
        source_payload: &[u8],
    ) -> bool {
        self.requirement == requirement
            && self.source_payload.as_deref() == Some(source_payload)
            && self.candidate_payload.starts_with(&[b'P', 0x05, 0x01])
    }

    pub(super) fn read_buffer_cursor(&self) -> usize {
        self.read_buffer_cursor
    }

    pub(super) fn read_buffer_end(&self) -> usize {
        self.read_buffer_end
    }

    pub(super) fn fragment_bits_written(&self) -> LiveObjectUpdatePackedFragmentBitSpanEvidence {
        self.fragment_bits_written
    }

    pub(super) fn final_fragment_bit_cursor(&self) -> usize {
        self.final_fragment_bit_cursor
    }

    pub(super) fn final_fragment_bit_end(&self) -> usize {
        self.final_fragment_bit_end
    }
}

#[cfg(test)]
pub(super) fn exact_terminal_ee_final_claim_for_test(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    source_payload: &[u8],
) -> ExactTerminalEeFinalClaim {
    ExactTerminalEeFinalClaim {
        requirement,
        source_payload: Some(source_payload.into()),
        candidate_payload: [b'P', 0x05, 0x01].into(),
        read_buffer_cursor: requirement.emitted_read_buffer_cursor,
        read_buffer_end: requirement.emitted_read_buffer_end,
        fragment_bits_written: requirement.emitted_fragment_bits,
        final_fragment_bit_cursor: requirement.emitted_fragment_bit_end,
        final_fragment_bit_end: requirement.emitted_fragment_bit_end,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TerminalEeWholePacketAudit {
    pub(super) candidate_payload_length: usize,
    pub(super) candidate_read_buffer_end: usize,
    pub(super) candidate_fragment_bit_end: usize,
    pub(super) typed_row_read_buffer_cursor: usize,
    pub(super) typed_row_read_buffer_end: usize,
    pub(super) typed_row_fragment_cursor: usize,
    pub(super) exact_payload_validator_accepted: bool,
    pub(super) reject: Option<LiveObjectPayloadClaimReject>,
    exact_final_claim: Option<ExactTerminalEeFinalClaim>,
}

impl TerminalEeWholePacketAudit {
    pub(super) fn status(&self) -> &'static str {
        if self.exact_payload_validator_accepted {
            "exact-payload-accepted"
        } else {
            "exact-payload-rejected"
        }
    }

    pub(super) fn reject_stage(&self) -> Option<LiveObjectPayloadClaimRejectStage> {
        self.reject.map(|reject| reject.stage)
    }

    pub(super) fn authorizes_claim(&self) -> bool {
        false
    }

    pub(super) fn authorizes_rewrite(&self) -> bool {
        false
    }

    pub(super) fn authorizes_cursor_advance(&self) -> bool {
        false
    }

    pub(super) fn authorizes_fragment_trim(&self) -> bool {
        false
    }

    pub(super) fn exact_final_claim(&self) -> Option<&ExactTerminalEeFinalClaim> {
        self.exact_final_claim.as_ref()
    }

    fn bind_source_payload(&mut self, source_payload: &[u8]) {
        if let Some(claim) = self.exact_final_claim.as_mut() {
            claim.source_payload = Some(source_payload.into());
        }
    }
}

pub(super) fn audit_staged_terminal_ee_candidate(
    stage: &TerminalDoorPlaceableTail9EeStage,
) -> Option<TerminalEeWholePacketAudit> {
    audit_staged_terminal_ee_candidate_for_requirement(stage, None)
}

pub(super) fn audit_staged_terminal_ee_candidate_for_requirement(
    stage: &TerminalDoorPlaceableTail9EeStage,
    requirement: Option<LiveObjectUpdateTerminalWriterHandoffRequirement>,
) -> Option<TerminalEeWholePacketAudit> {
    // The stage is created only after the typed EE row reader consumes the
    // complete emitted read buffer. Keep those terminal cursor invariants
    // explicit here so a future caller cannot turn an arbitrary partial stage
    // into apparently authoritative whole-packet evidence.
    if stage.typed_row_read_buffer_cursor() != stage.typed_row_read_buffer_end()
        || stage.typed_row_read_buffer_end() != stage.live_bytes().len()
        || stage.typed_row_fragment_cursor() > stage.candidate_fragment_bit_end()
        || stage.candidate_fragment_bit_end() != stage.fragment_bits().len()
    {
        return None;
    }

    let candidate =
        super::live_object_payload_from_parts(stage.live_bytes(), stage.fragment_bits())?;
    let validation = super::claim_payload_if_verified_with_reject(&candidate);
    let (exact_payload_validator_accepted, reject) = match validation {
        Ok(_) => (true, None),
        Err(reject) => (false, Some(reject)),
    };

    let exact_final_claim = if exact_payload_validator_accepted {
        requirement
            .and_then(|requirement| exact_final_claim_from_stage(stage, requirement, &candidate))
    } else {
        None
    };

    Some(TerminalEeWholePacketAudit {
        candidate_payload_length: candidate.len(),
        candidate_read_buffer_end: stage.live_bytes().len(),
        candidate_fragment_bit_end: stage.fragment_bits().len(),
        typed_row_read_buffer_cursor: stage.typed_row_read_buffer_cursor(),
        typed_row_read_buffer_end: stage.typed_row_read_buffer_end(),
        typed_row_fragment_cursor: stage.typed_row_fragment_cursor(),
        exact_payload_validator_accepted,
        reject,
        exact_final_claim,
    })
}

fn exact_final_claim_from_stage(
    stage: &TerminalDoorPlaceableTail9EeStage,
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    candidate_payload: &[u8],
) -> Option<ExactTerminalEeFinalClaim> {
    if !requirement.emitted_contract_is_valid()
        || !staged_record_identity_matches_requirement(stage, requirement)
        || stage.typed_row_read_buffer_cursor() != requirement.emitted_read_buffer_cursor
        || stage.typed_row_read_buffer_end() != requirement.emitted_read_buffer_end
        || stage.typed_row_fragment_cursor() != stage.candidate_fragment_bit_end()
        || stage.candidate_fragment_bit_end() != requirement.emitted_fragment_bit_end
        || requirement.emitted_fragment_bit_start > requirement.emitted_fragment_bit_end
        || requirement.emitted_fragment_bit_end > stage.fragment_bits().len()
    {
        return None;
    }
    let fragment_bits_written = packed_fragment_span(
        stage.fragment_bits(),
        requirement.emitted_fragment_bit_start,
        requirement.emitted_fragment_bit_end,
    )?;
    if fragment_bits_written != requirement.emitted_fragment_bits {
        return None;
    }
    Some(ExactTerminalEeFinalClaim {
        requirement,
        source_payload: None,
        candidate_payload: candidate_payload.into(),
        read_buffer_cursor: stage.typed_row_read_buffer_cursor(),
        read_buffer_end: stage.typed_row_read_buffer_end(),
        fragment_bits_written,
        final_fragment_bit_cursor: stage.typed_row_fragment_cursor(),
        final_fragment_bit_end: stage.candidate_fragment_bit_end(),
    })
}

fn staged_record_identity_matches_requirement(
    stage: &TerminalDoorPlaceableTail9EeStage,
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
) -> bool {
    let Some(record_end) = requirement
        .emitted_record_offset
        .checked_add(LEGACY_UPDATE_HEADER_BYTES)
    else {
        return false;
    };
    let Some(record) = stage
        .live_bytes()
        .get(requirement.emitted_record_offset..record_end)
    else {
        return false;
    };
    record.first().copied() == Some(b'U')
        && record.get(1).copied() == Some(requirement.object_type)
        && record
            .get(2..6)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            == Some(requirement.object_id)
        && record
            .get(6..10)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            == Some(requirement.emitted_mask)
}

fn packed_fragment_span(
    bits: &[bool],
    bit_start: usize,
    bit_end: usize,
) -> Option<LiveObjectUpdatePackedFragmentBitSpanEvidence> {
    let bit_count = bit_end.checked_sub(bit_start)?;
    if bit_count > u32::BITS as usize || bit_end > bits.len() {
        return None;
    }
    let mut packed_msb = 0u32;
    for value in bits.get(bit_start..bit_end)? {
        packed_msb = (packed_msb << 1) | u32::from(*value);
    }
    Some(LiveObjectUpdatePackedFragmentBitSpanEvidence {
        bit_start,
        bit_end,
        packed_msb,
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

    let mut audit = audit?;
    if !audit_matches_requirement(&audit, expected_requirement) {
        return None;
    }
    audit.bind_source_payload(payload);
    Some(audit)
}

fn audit_matches_requirement(
    audit: &TerminalEeWholePacketAudit,
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
) -> bool {
    let expected_typed_fragment_cursor = if audit.exact_final_claim().is_some() {
        requirement.emitted_fragment_bit_end
    } else {
        requirement.emitted_fragment_bit_start
    };
    audit.candidate_read_buffer_end == requirement.emitted_read_buffer_end
        && audit.typed_row_read_buffer_cursor == requirement.emitted_read_buffer_cursor
        && audit.typed_row_read_buffer_end == requirement.emitted_read_buffer_end
        && audit.typed_row_fragment_cursor == expected_typed_fragment_cursor
        && audit.candidate_fragment_bit_end == requirement.emitted_fragment_bit_end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::live_object_update::{
        CNW_FRAGMENT_HEADER_BITS, LEGACY_UPDATE_STATE_MASK, PLACEABLE_OBJECT_TYPE,
    };

    fn exact_state_only_placeable_stage() -> TerminalDoorPlaceableTail9EeStage {
        let mut live_bytes = vec![b'U', 0x09];
        live_bytes.extend_from_slice(&0x8000_1003u32.to_le_bytes());
        live_bytes.extend_from_slice(&LEGACY_UPDATE_STATE_MASK.to_le_bytes());
        let mut fragment_bits = vec![false; CNW_FRAGMENT_HEADER_BITS];
        // EE placeable state is exactly five BOOLs in decompile reader order.
        fragment_bits.extend([false, false, true, false, false]);
        let live_len = live_bytes.len();
        let fragment_len = fragment_bits.len();
        TerminalDoorPlaceableTail9EeStage::for_terminal_ee_writer_test(
            live_bytes,
            fragment_bits,
            live_len,
            live_len,
            fragment_len,
            fragment_len,
        )
    }

    fn exact_state_only_requirement(
        stage: &TerminalDoorPlaceableTail9EeStage,
    ) -> LiveObjectUpdateTerminalWriterHandoffRequirement {
        let source_bits = super::super::live_object_rewrite_bit_slice_evidence(3, 3, &[]);
        let emitted_fragment_bits = packed_fragment_span(
            stage.fragment_bits(),
            CNW_FRAGMENT_HEADER_BITS,
            stage.typed_row_fragment_cursor(),
        )
        .expect("bounded exact emitted span");
        LiveObjectUpdateTerminalWriterHandoffRequirement {
            source_record_offset: 0,
            object_type: PLACEABLE_OBJECT_TYPE,
            object_id: 0x8000_1003,
            raw_mask: LEGACY_UPDATE_STATE_MASK,
            source_read_buffer_cursor: stage.live_bytes().len(),
            source_read_buffer_end: stage.live_bytes().len(),
            source_fragment_bits: source_bits,
            source_next_opcode_read_overflows: true,
            emitted_record_offset: 0,
            emitted_mask: LEGACY_UPDATE_STATE_MASK,
            emitted_read_buffer_cursor: stage.typed_row_read_buffer_cursor(),
            emitted_read_buffer_end: stage.typed_row_read_buffer_end(),
            emitted_fragment_bit_start: CNW_FRAGMENT_HEADER_BITS,
            emitted_fragment_bit_end: stage.typed_row_fragment_cursor(),
            emitted_fragment_bit_count: emitted_fragment_bits.bit_count(),
            emitted_fragment_bits_retained: emitted_fragment_bits.bit_count(),
            emitted_fragment_bits,
            emitted_next_opcode_read_overflows: true,
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
    fn exact_whole_packet_and_requirement_mint_one_opaque_ee_claim() {
        let stage = exact_state_only_placeable_stage();
        let requirement = exact_state_only_requirement(&stage);
        let source_payload = b"exact source packet";
        let mut audit =
            audit_staged_terminal_ee_candidate_for_requirement(&stage, Some(requirement))
                .expect("bounded exact EE stage");
        audit.bind_source_payload(source_payload);
        let claim = audit
            .exact_final_claim()
            .expect("exact typed reader and whole-payload validator should mint a sealed claim");
        assert!(claim.matches(requirement, source_payload));
        assert_eq!(claim.read_buffer_cursor(), stage.live_bytes().len());
        assert_eq!(
            claim.final_fragment_bit_cursor(),
            stage.fragment_bits().len()
        );
        assert!(audit_matches_requirement(&audit, requirement));

        let mut mismatched_requirement = requirement;
        mismatched_requirement.emitted_fragment_bits.packed_msb ^= 1;
        let mismatched = audit_staged_terminal_ee_candidate_for_requirement(
            &stage,
            Some(mismatched_requirement),
        )
        .expect("the candidate remains structurally auditable");
        assert!(mismatched.exact_payload_validator_accepted);
        assert!(mismatched.exact_final_claim().is_none());
        assert!(!audit_matches_requirement(
            &mismatched,
            mismatched_requirement
        ));

        let mismatched_identity = LiveObjectUpdateTerminalWriterHandoffRequirement {
            object_id: requirement.object_id ^ 1,
            ..requirement
        };
        let mismatched =
            audit_staged_terminal_ee_candidate_for_requirement(&stage, Some(mismatched_identity))
                .expect("the candidate remains structurally auditable");
        assert!(mismatched.exact_payload_validator_accepted);
        assert!(mismatched.exact_final_claim().is_none());
    }

    #[test]
    fn whole_packet_validator_rejects_fragment_after_exact_typed_row() {
        let exact = exact_state_only_placeable_stage();
        let typed_row_fragment_cursor = exact.typed_row_fragment_cursor();
        let live_bytes = exact.live_bytes().to_vec();
        let mut fragment_bits = exact.fragment_bits().to_vec();
        fragment_bits.push(false);
        let stage = TerminalDoorPlaceableTail9EeStage::for_terminal_ee_writer_test(
            live_bytes,
            fragment_bits.clone(),
            exact.typed_row_read_buffer_cursor(),
            exact.typed_row_read_buffer_end(),
            typed_row_fragment_cursor,
            fragment_bits.len(),
        );

        let audit = audit_staged_terminal_ee_candidate(&stage).expect("bounded residual stage");
        assert!(!audit.exact_payload_validator_accepted);
        assert_eq!(audit.status(), "exact-payload-rejected");
        assert_eq!(
            audit.reject_stage(),
            Some(LiveObjectPayloadClaimRejectStage::FragmentCursor)
        );
        assert_eq!(
            audit.reject.and_then(|reject| reject.offset),
            Some(stage.live_bytes().len())
        );
        assert_eq!(
            audit.reject.and_then(|reject| reject.bit_cursor),
            Some(typed_row_fragment_cursor)
        );
        assert!(!audit.authorizes_claim());
        assert!(!audit.authorizes_rewrite());
        assert!(!audit.authorizes_cursor_advance());
        assert!(!audit.authorizes_fragment_trim());
        assert!(audit.exact_final_claim().is_none());
    }

    #[test]
    fn malformed_stage_cursor_contract_fails_closed() {
        let exact = exact_state_only_placeable_stage();
        let stage = TerminalDoorPlaceableTail9EeStage::for_terminal_ee_writer_test(
            exact.live_bytes().to_vec(),
            exact.fragment_bits().to_vec(),
            exact.typed_row_read_buffer_cursor(),
            exact.typed_row_read_buffer_end(),
            exact.typed_row_fragment_cursor(),
            exact.fragment_bits().len().saturating_add(1),
        );
        assert!(audit_staged_terminal_ee_candidate(&stage).is_none());

        let exact = exact_state_only_placeable_stage();
        let stage = TerminalDoorPlaceableTail9EeStage::for_terminal_ee_writer_test(
            exact.live_bytes().to_vec(),
            exact.fragment_bits().to_vec(),
            exact.typed_row_read_buffer_end().saturating_sub(1),
            exact.typed_row_read_buffer_end(),
            exact.typed_row_fragment_cursor(),
            exact.candidate_fragment_bit_end(),
        );
        assert!(audit_staged_terminal_ee_candidate(&stage).is_none());
    }
}
