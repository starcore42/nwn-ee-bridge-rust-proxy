//! Diagnostic-only whole-packet audit and output plan for the terminal
//! compact-tail9 EE stage.
//!
//! The normal translator already has a typed byte/bit plan for the terminal
//! door/placeable row.  That row-local proof is weaker than an exact P/05/01
//! claim: a fragment-only continuation can remain after the row reader stops.
//! This module materializes both the unmodified staged packet and a sealed
//! candidate ending at the exact typed EE cursor on the heap, then runs the
//! same exact validator used at the wire boundary. The latter candidate proves
//! only the EE output shape: the removed residual remains unowned until the HG
//! writer/list trace independently proves its source contract. Neither result
//! can authorize a claim, rewrite, cursor advance, or fragment trim.

use super::record::TerminalDoorPlaceableTail9EeStage;
use super::{
    LEGACY_UPDATE_HEADER_BYTES, LiveObjectPayloadClaimReject, LiveObjectPayloadClaimRejectStage,
    LiveObjectUpdatePackedFragmentBitSpanEvidence, LiveObjectUpdateRewriteFailure,
    LiveObjectUpdateTerminalWriterHandoffRequirement,
};

/// Opaque proof that the sealed EE output plan removed exactly the retained
/// unconsumed MSB-first residual, ended at the typed reader cursor, and passed
/// the existing whole-payload validator. Only this module can construct the
/// token.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ExactTerminalEeFinalClaim {
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    source_payload: Option<Box<[u8]>>,
    candidate_payload: Box<[u8]>,
    read_buffer_cursor: usize,
    read_buffer_end: usize,
    residual_fragment_bits_removed: LiveObjectUpdatePackedFragmentBitSpanEvidence,
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
            && requirement.emitted_contract_is_valid()
            && self.source_payload.as_deref() == Some(source_payload)
            && self.candidate_payload.starts_with(&[b'P', 0x05, 0x01])
            && self.read_buffer_cursor == requirement.emitted_read_buffer_cursor
            && self.read_buffer_end == requirement.emitted_read_buffer_end
            && self.residual_fragment_bits_removed == requirement.emitted_fragment_bits
            && self.final_fragment_bit_cursor == requirement.emitted_fragment_bit_start
            && self.final_fragment_bit_end == requirement.emitted_fragment_bit_start
    }

    pub(super) fn read_buffer_cursor(&self) -> usize {
        self.read_buffer_cursor
    }

    pub(super) fn read_buffer_end(&self) -> usize {
        self.read_buffer_end
    }

    pub(super) fn residual_fragment_bits_removed(
        &self,
    ) -> LiveObjectUpdatePackedFragmentBitSpanEvidence {
        self.residual_fragment_bits_removed
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
        residual_fragment_bits_removed: requirement.emitted_fragment_bits,
        final_fragment_bit_cursor: requirement.emitted_fragment_bit_start,
        final_fragment_bit_end: requirement.emitted_fragment_bit_start,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TerminalEeWholePacketAudit {
    pub(super) candidate_payload_length: usize,
    pub(super) candidate_read_buffer_end: usize,
    pub(super) candidate_fragment_bit_end: usize,
    pub(super) candidate_fragment_final_bits: u8,
    pub(super) typed_row_read_buffer_cursor: usize,
    pub(super) typed_row_read_buffer_end: usize,
    pub(super) typed_row_fragment_cursor: usize,
    pub(super) exact_payload_validator_accepted: bool,
    pub(super) reject: Option<LiveObjectPayloadClaimReject>,
    pub(super) residual_removal_candidate_payload_length: Option<usize>,
    pub(super) residual_removal_candidate_fragment_bit_end: Option<usize>,
    pub(super) residual_removal_candidate_fragment_final_bits: Option<u8>,
    pub(super) residual_removal_exact_payload_validator_accepted: bool,
    pub(super) residual_removal_reject: Option<LiveObjectPayloadClaimReject>,
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

    pub(super) fn residual_removal_status(&self) -> &'static str {
        if self.residual_removal_exact_payload_validator_accepted {
            "exact-residual-removal-accepted"
        } else if self.residual_removal_candidate_payload_length.is_some() {
            "exact-residual-removal-rejected"
        } else {
            "residual-removal-plan-unavailable"
        }
    }

    pub(super) fn residual_removal_reject_stage(
        &self,
    ) -> Option<LiveObjectPayloadClaimRejectStage> {
        self.residual_removal_reject.map(|reject| reject.stage)
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

    let residual_removal =
        requirement.and_then(|requirement| audit_residual_removal_candidate(stage, requirement));
    let residual_removal_candidate_payload_length = residual_removal
        .as_ref()
        .map(|plan| plan.candidate_payload_length);
    let residual_removal_candidate_fragment_bit_end = residual_removal
        .as_ref()
        .map(|plan| plan.candidate_fragment_bit_end);
    let residual_removal_candidate_fragment_final_bits = residual_removal
        .as_ref()
        .map(|plan| plan.candidate_fragment_final_bits);
    let residual_removal_exact_payload_validator_accepted = residual_removal
        .as_ref()
        .is_some_and(|plan| plan.exact_payload_validator_accepted);
    let residual_removal_reject = residual_removal.as_ref().and_then(|plan| plan.reject);
    let exact_final_claim = residual_removal.and_then(|plan| plan.exact_final_claim);

    Some(TerminalEeWholePacketAudit {
        candidate_payload_length: candidate.len(),
        candidate_read_buffer_end: stage.live_bytes().len(),
        candidate_fragment_bit_end: stage.fragment_bits().len(),
        candidate_fragment_final_bits: (stage.fragment_bits().len() % 8) as u8,
        typed_row_read_buffer_cursor: stage.typed_row_read_buffer_cursor(),
        typed_row_read_buffer_end: stage.typed_row_read_buffer_end(),
        typed_row_fragment_cursor: stage.typed_row_fragment_cursor(),
        exact_payload_validator_accepted,
        reject,
        residual_removal_candidate_payload_length,
        residual_removal_candidate_fragment_bit_end,
        residual_removal_candidate_fragment_final_bits,
        residual_removal_exact_payload_validator_accepted,
        residual_removal_reject,
        exact_final_claim,
    })
}

struct TerminalEeResidualRemovalAudit {
    candidate_payload_length: usize,
    candidate_fragment_bit_end: usize,
    candidate_fragment_final_bits: u8,
    exact_payload_validator_accepted: bool,
    reject: Option<LiveObjectPayloadClaimReject>,
    exact_final_claim: Option<ExactTerminalEeFinalClaim>,
}

fn audit_residual_removal_candidate(
    stage: &TerminalDoorPlaceableTail9EeStage,
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
) -> Option<TerminalEeResidualRemovalAudit> {
    if !requirement.emitted_contract_is_valid()
        || !staged_record_identity_matches_requirement(stage, requirement)
        || stage.typed_row_read_buffer_cursor() != requirement.emitted_read_buffer_cursor
        || stage.typed_row_read_buffer_end() != requirement.emitted_read_buffer_end
        // EE sub_1405C3D40 consumes the translated door/placeable row only
        // through this cursor. Bits after it are the exact unconsumed residual,
        // not output written by that reader/writer contract.
        || stage.typed_row_fragment_cursor() != requirement.emitted_fragment_bit_start
        || stage.typed_row_fragment_cursor() < super::CNW_FRAGMENT_HEADER_BITS
        || stage.candidate_fragment_bit_end() != requirement.emitted_fragment_bit_end
        || requirement.emitted_fragment_bit_start > requirement.emitted_fragment_bit_end
        || requirement.emitted_fragment_bit_end > stage.fragment_bits().len()
    {
        return None;
    }
    let residual_fragment_bits_removed = packed_fragment_span(
        stage.fragment_bits(),
        requirement.emitted_fragment_bit_start,
        requirement.emitted_fragment_bit_end,
    )?;
    if residual_fragment_bits_removed != requirement.emitted_fragment_bits {
        return None;
    }
    let final_fragment_bits = stage
        .fragment_bits()
        .get(..stage.typed_row_fragment_cursor())?;
    let candidate = super::live_object_payload_from_parts(stage.live_bytes(), final_fragment_bits)?;
    let validation = super::claim_payload_if_verified_with_reject(&candidate);
    let (exact_payload_validator_accepted, reject) = match validation {
        Ok(_) => (true, None),
        Err(reject) => (false, Some(reject)),
    };
    let candidate_payload_length = candidate.len();
    let exact_final_claim = exact_payload_validator_accepted.then(|| ExactTerminalEeFinalClaim {
        requirement,
        source_payload: None,
        candidate_payload: candidate.into(),
        read_buffer_cursor: stage.typed_row_read_buffer_cursor(),
        read_buffer_end: stage.typed_row_read_buffer_end(),
        residual_fragment_bits_removed,
        final_fragment_bit_cursor: stage.typed_row_fragment_cursor(),
        final_fragment_bit_end: stage.typed_row_fragment_cursor(),
    });
    Some(TerminalEeResidualRemovalAudit {
        candidate_payload_length,
        candidate_fragment_bit_end: stage.typed_row_fragment_cursor(),
        candidate_fragment_final_bits: (stage.typed_row_fragment_cursor() % 8) as u8,
        exact_payload_validator_accepted,
        reject,
        exact_final_claim,
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
    audit.candidate_read_buffer_end == requirement.emitted_read_buffer_end
        && audit.typed_row_read_buffer_cursor == requirement.emitted_read_buffer_cursor
        && audit.typed_row_read_buffer_end == requirement.emitted_read_buffer_end
        && audit.typed_row_fragment_cursor == requirement.emitted_fragment_bit_start
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
            stage.typed_row_fragment_cursor(),
            stage.typed_row_fragment_cursor(),
        )
        .expect("bounded empty residual span");
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
            emitted_fragment_bit_start: stage.typed_row_fragment_cursor(),
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
    fn empty_residual_requirement_cannot_mint_an_ee_claim() {
        let stage = exact_state_only_placeable_stage();
        let requirement = exact_state_only_requirement(&stage);
        assert!(!requirement.emitted_contract_is_valid());

        let audit = audit_staged_terminal_ee_candidate_for_requirement(&stage, Some(requirement))
            .expect("bounded exact EE stage");
        assert!(audit.exact_payload_validator_accepted);
        assert!(!audit.residual_removal_exact_payload_validator_accepted);
        assert!(audit.exact_final_claim().is_none());
        assert_eq!(
            audit.residual_removal_status(),
            "residual-removal-plan-unavailable"
        );
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
        assert_eq!(
            audit.residual_removal_status(),
            "residual-removal-plan-unavailable"
        );
    }

    #[test]
    fn exact_residual_removal_plan_mints_non_authorizing_ee_claim() {
        let exact = exact_state_only_placeable_stage();
        let typed_row_fragment_cursor = exact.typed_row_fragment_cursor();
        let live_bytes = exact.live_bytes().to_vec();
        let mut fragment_bits = exact.fragment_bits().to_vec();
        // Sequence 95 leaves these exact 17 MSB-first bits after the typed EE
        // placeable reader. The plan retains their value as removal evidence
        // while the final packet ends at the typed cursor.
        let residual_bits = (0..17)
            .map(|bit| ((0x4046u32 >> (16 - bit)) & 1) != 0)
            .collect::<Vec<_>>();
        fragment_bits.extend_from_slice(&residual_bits);
        let stage = TerminalDoorPlaceableTail9EeStage::for_terminal_ee_writer_test(
            live_bytes,
            fragment_bits.clone(),
            exact.typed_row_read_buffer_cursor(),
            exact.typed_row_read_buffer_end(),
            typed_row_fragment_cursor,
            fragment_bits.len(),
        );
        let mut requirement = exact_state_only_requirement(&exact);
        requirement.emitted_fragment_bit_start = typed_row_fragment_cursor;
        requirement.emitted_fragment_bit_end = fragment_bits.len();
        requirement.emitted_fragment_bit_count = residual_bits.len();
        requirement.emitted_fragment_bits_retained = residual_bits.len();
        requirement.emitted_fragment_bits = packed_fragment_span(
            &fragment_bits,
            typed_row_fragment_cursor,
            fragment_bits.len(),
        )
        .expect("17 exact residual bits");

        let source_payload = b"immutable source packet";
        let mut audit =
            audit_staged_terminal_ee_candidate_for_requirement(&stage, Some(requirement))
                .expect("bounded residual stage");
        audit.bind_source_payload(source_payload);

        assert!(!audit.exact_payload_validator_accepted);
        assert_eq!(
            audit.reject_stage(),
            Some(LiveObjectPayloadClaimRejectStage::FragmentCursor)
        );
        assert!(audit.residual_removal_exact_payload_validator_accepted);
        assert_eq!(
            audit.residual_removal_candidate_fragment_bit_end,
            Some(typed_row_fragment_cursor)
        );
        assert_eq!(audit.residual_removal_reject, None);
        let claim = audit
            .exact_final_claim()
            .expect("whole-payload validation should seal the EE output plan");
        assert!(claim.matches(requirement, source_payload));
        assert_eq!(
            claim.residual_fragment_bits_removed(),
            requirement.emitted_fragment_bits
        );
        assert_eq!(claim.final_fragment_bit_cursor(), typed_row_fragment_cursor);
        assert_eq!(claim.final_fragment_bit_end(), typed_row_fragment_cursor);
        let fragment_offset = 7 + stage.live_bytes().len();
        let original =
            super::super::live_object_payload_from_parts(stage.live_bytes(), stage.fragment_bits())
                .expect("bounded staged packet");
        let original_fragment = &original[fragment_offset..];
        let final_fragment = &claim.candidate_payload[fragment_offset..];
        assert_eq!(
            (original_fragment[0] & 0xE0) >> 5,
            audit.candidate_fragment_final_bits
        );
        assert_eq!(
            (final_fragment[0] & 0xE0) >> 5,
            audit
                .residual_removal_candidate_fragment_final_bits
                .expect("sealed candidate valid-bit header")
        );
        let decoded_final =
            super::super::bits::decode_msb_valid_bits(final_fragment, CNW_FRAGMENT_HEADER_BITS)
                .expect("71-bit repacked final fragment");
        assert_eq!(decoded_final.len(), typed_row_fragment_cursor);
        assert_eq!(
            &decoded_final[CNW_FRAGMENT_HEADER_BITS..],
            &stage.fragment_bits()[CNW_FRAGMENT_HEADER_BITS..typed_row_fragment_cursor]
        );
        assert!(!audit.authorizes_claim());
        assert!(!audit.authorizes_rewrite());
        assert!(!audit.authorizes_cursor_advance());
        assert!(!audit.authorizes_fragment_trim());

        let mut changed_bits = requirement;
        changed_bits.emitted_fragment_bits.packed_msb ^= 1;
        let mismatched =
            audit_staged_terminal_ee_candidate_for_requirement(&stage, Some(changed_bits))
                .expect("the original candidate remains auditable");
        assert!(mismatched.exact_final_claim().is_none());
        assert_eq!(
            mismatched.residual_removal_status(),
            "residual-removal-plan-unavailable"
        );

        let mismatched_identity = LiveObjectUpdateTerminalWriterHandoffRequirement {
            object_id: requirement.object_id ^ 1,
            ..requirement
        };
        let mismatched =
            audit_staged_terminal_ee_candidate_for_requirement(&stage, Some(mismatched_identity))
                .expect("the original candidate remains auditable");
        assert!(mismatched.exact_final_claim().is_none());
    }

    #[test]
    fn malformed_residual_removal_candidate_is_reported_and_never_sealed() {
        let exact = exact_state_only_placeable_stage();
        let final_cursor = exact.typed_row_fragment_cursor() - 1;
        let stage = TerminalDoorPlaceableTail9EeStage::for_terminal_ee_writer_test(
            exact.live_bytes().to_vec(),
            exact.fragment_bits().to_vec(),
            exact.typed_row_read_buffer_cursor(),
            exact.typed_row_read_buffer_end(),
            final_cursor,
            exact.fragment_bits().len(),
        );
        let mut requirement = exact_state_only_requirement(&exact);
        requirement.emitted_fragment_bit_start = final_cursor;
        requirement.emitted_fragment_bit_end = exact.fragment_bits().len();
        requirement.emitted_fragment_bit_count = 1;
        requirement.emitted_fragment_bits_retained = 1;
        requirement.emitted_fragment_bits = packed_fragment_span(
            exact.fragment_bits(),
            final_cursor,
            exact.fragment_bits().len(),
        )
        .expect("one bounded residual bit");

        let audit = audit_staged_terminal_ee_candidate_for_requirement(&stage, Some(requirement))
            .expect("bounded malformed final candidate");
        assert_eq!(
            audit.residual_removal_status(),
            "exact-residual-removal-rejected"
        );
        assert!(!audit.residual_removal_exact_payload_validator_accepted);
        assert_eq!(
            audit.residual_removal_reject_stage(),
            Some(LiveObjectPayloadClaimRejectStage::RecordValidator)
        );
        assert!(audit.residual_removal_reject.is_some());
        assert!(audit.exact_final_claim().is_none());
        assert!(!audit.authorizes_claim());
        assert!(!audit.authorizes_rewrite());
        assert!(!audit.authorizes_cursor_advance());
        assert!(!audit.authorizes_fragment_trim());
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
