//! Bounded ingestion of terminal server-writer trace evidence.
//!
//! A writer trace is diagnostic proof, not a live rewrite input. Exact proof
//! is constructed only here from one ordered owner/list/finalizer trace and
//! one complete finalized `P/05/01` payload. The factory validates the CNW
//! envelope, derives record identity and terminal MSB-first bits from those
//! finalized bytes, and then compares the entire payload with quarantine.

use super::{
    LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT, LiveObjectUpdateRewriteBitSliceEvidence,
    LiveObjectUpdateTerminalWriterHandoffRequirement, LiveObjectUpdateTerminalWriterHandoffVerdict,
    MAX_REASONABLE_LIVE_PAYLOAD_BYTES,
};

const LIVE_OBJECT_CNW_WRITER_HEADER_BYTES: usize = 7;
const LIVE_OBJECT_UPDATE_HEADER_BYTES: usize = 10;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalWriterPacketCorrelation {
    Unproven,
    FingerprintOnly,
    DifferentPayloadBytes,
    ExactPayloadBytes,
}

/// Strength of the packet evidence attached to the finalizer event.
///
/// A matching suffix or digest can locate a likely trace, but only a complete
/// finalized payload can establish exact packet identity.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum TerminalWriterTracePacketEvidence<'a> {
    Unproven,
    FingerprintOnly { matches: bool },
    FullFinalizedPayload(&'a [u8]),
}

/// Ordered events emitted by one trace instance. The exact four-event pattern
/// is intentional: mere presence of a cursor snapshot does not prove that the
/// update-list handoff or message finalizer ran after the candidate owner.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
enum TerminalWriterTraceEvent<'a> {
    OwnerBegin {
        absolute_record_offset: usize,
        absolute_read_buffer_cursor: usize,
        fragment_bit_cursor: usize,
    },
    OwnerEnd {
        absolute_read_buffer_cursor: usize,
        fragment_bit_cursor: usize,
    },
    ListHandoff,
    Finalize {
        absolute_read_buffer_end: usize,
        fragment_bit_cursor: usize,
        packet_evidence: TerminalWriterTracePacketEvidence<'a>,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
struct TerminalWriterTrace<'a> {
    events: [TerminalWriterTraceEvent<'a>; 4],
}

#[derive(Debug, Clone, Copy)]
struct TerminalWriterObservation {
    object_type: u8,
    object_id: u32,
    raw_mask: u32,
    replayed_read_buffer_cursor: usize,
    replayed_read_buffer_end: usize,
    fragment_bits_written: LiveObjectUpdateRewriteBitSliceEvidence,
    finalized_fragment_bit_cursor: usize,
    packet_correlation: TerminalWriterPacketCorrelation,
}

#[derive(Debug, Clone, Copy)]
struct ValidatedPacketLayout<'a> {
    payload: &'a [u8],
    declared: usize,
    fragment: &'a [u8],
    fragment_valid_bits: usize,
}

/// Production currently has no full HG writer trace. Keep the only sibling
/// entry point incapable of supplying a fabricated observation; future trace
/// ingestion must be implemented inside this sealed module.
pub(super) fn correlate_terminal_writer_trace(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    quarantined_payload: &[u8],
) -> LiveObjectUpdateTerminalWriterHandoffVerdict {
    correlate_bounded_terminal_writer_trace(requirement, quarantined_payload, None)
}

fn correlate_bounded_terminal_writer_trace(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    quarantined_payload: &[u8],
    trace: Option<TerminalWriterTrace<'_>>,
) -> LiveObjectUpdateTerminalWriterHandoffVerdict {
    let Some(trace) = trace else {
        return LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace;
    };
    let Some(observation) = build_terminal_writer_observation(quarantined_payload, trace) else {
        return LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace;
    };
    correlate_observation(requirement, observation)
}

fn build_terminal_writer_observation(
    quarantined_payload: &[u8],
    trace: TerminalWriterTrace<'_>,
) -> Option<TerminalWriterObservation> {
    let [
        TerminalWriterTraceEvent::OwnerBegin {
            absolute_record_offset,
            absolute_read_buffer_cursor: owner_begin_read_cursor,
            fragment_bit_cursor: owner_begin_fragment_cursor,
        },
        TerminalWriterTraceEvent::OwnerEnd {
            absolute_read_buffer_cursor: owner_end_read_cursor,
            fragment_bit_cursor: owner_end_fragment_cursor,
        },
        TerminalWriterTraceEvent::ListHandoff,
        TerminalWriterTraceEvent::Finalize {
            absolute_read_buffer_end: final_read_end,
            fragment_bit_cursor: final_fragment_cursor,
            packet_evidence,
        },
    ] = trace.events
    else {
        return None;
    };

    let packet_correlation = packet_correlation(quarantined_payload, packet_evidence);
    let structural_payload = match packet_evidence {
        TerminalWriterTracePacketEvidence::FullFinalizedPayload(payload) => payload,
        TerminalWriterTracePacketEvidence::Unproven
        | TerminalWriterTracePacketEvidence::FingerprintOnly { .. } => quarantined_payload,
    };
    let layout = validated_packet_layout(structural_payload)?;

    if absolute_record_offset != owner_begin_read_cursor
        || absolute_record_offset < LIVE_OBJECT_CNW_WRITER_HEADER_BYTES
        || absolute_record_offset.checked_add(LIVE_OBJECT_UPDATE_HEADER_BYTES)? > layout.declared
        || owner_end_read_cursor != layout.declared
        || final_read_end != layout.declared
        || owner_begin_fragment_cursor > owner_end_fragment_cursor
        || owner_end_fragment_cursor != final_fragment_cursor
        || final_fragment_cursor != layout.fragment_valid_bits
    {
        return None;
    }

    let record = layout.payload.get(
        absolute_record_offset
            ..absolute_record_offset.checked_add(LIVE_OBJECT_UPDATE_HEADER_BYTES)?,
    )?;
    if record.first().copied() != Some(b'U') {
        return None;
    }
    let object_type = record[1];
    let object_id = u32::from_le_bytes(record[2..6].try_into().ok()?);
    let raw_mask = u32::from_le_bytes(record[6..10].try_into().ok()?);
    let fragment_bits_written = rewrite_bit_slice_from_payload(
        layout.fragment,
        owner_begin_fragment_cursor,
        owner_end_fragment_cursor,
    )?;

    Some(TerminalWriterObservation {
        object_type,
        object_id,
        raw_mask,
        replayed_read_buffer_cursor: owner_end_read_cursor
            .checked_sub(LIVE_OBJECT_CNW_WRITER_HEADER_BYTES)?,
        replayed_read_buffer_end: final_read_end
            .checked_sub(LIVE_OBJECT_CNW_WRITER_HEADER_BYTES)?,
        fragment_bits_written,
        finalized_fragment_bit_cursor: final_fragment_cursor,
        packet_correlation,
    })
}

fn validated_packet_layout(payload: &[u8]) -> Option<ValidatedPacketLayout<'_>> {
    if payload.len() > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
        || payload.len() <= LIVE_OBJECT_CNW_WRITER_HEADER_BYTES
        || payload.get(0..3) != Some([b'P', 0x05, 0x01].as_slice())
    {
        return None;
    }
    let declared = usize::try_from(u32::from_le_bytes(payload.get(3..7)?.try_into().ok()?)).ok()?;
    if declared < LIVE_OBJECT_CNW_WRITER_HEADER_BYTES || declared >= payload.len() {
        return None;
    }
    let fragment = payload.get(declared..)?;
    let fragment_valid_bits = cnw_fragment_valid_bits(fragment)?;
    Some(ValidatedPacketLayout {
        payload,
        declared,
        fragment,
        fragment_valid_bits,
    })
}

fn cnw_fragment_valid_bits(fragment: &[u8]) -> Option<usize> {
    let first = *fragment.first()?;
    let final_byte_bits = usize::from((first & 0xE0) >> 5);
    let valid_bits = if final_byte_bits == 0 {
        fragment.len().checked_mul(8)?
    } else {
        fragment
            .len()
            .checked_sub(1)?
            .checked_mul(8)?
            .checked_add(final_byte_bits)?
    };
    if valid_bits < CNW_FRAGMENT_HEADER_BITS || valid_bits > fragment.len().checked_mul(8)? {
        return None;
    }
    Some(valid_bits)
}

fn rewrite_bit_slice_from_payload(
    fragment: &[u8],
    bit_start: usize,
    bit_end: usize,
) -> Option<LiveObjectUpdateRewriteBitSliceEvidence> {
    let bit_count = bit_end.checked_sub(bit_start)?;
    if bit_count > LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT {
        return None;
    }
    let mut bits = [None; LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT];
    for (slot, bit_cursor) in bits.iter_mut().zip(bit_start..bit_end) {
        *slot = Some(fragment_bit(fragment, bit_cursor)?);
    }
    Some(LiveObjectUpdateRewriteBitSliceEvidence {
        bit_start,
        bit_end,
        bit_count,
        bits_retained: bit_count,
        bits,
    })
}

fn fragment_bit(fragment: &[u8], bit_cursor: usize) -> Option<bool> {
    let byte = *fragment.get(bit_cursor / 8)?;
    Some((byte & (0x80 >> (bit_cursor % 8))) != 0)
}

fn packet_correlation(
    quarantined_payload: &[u8],
    evidence: TerminalWriterTracePacketEvidence<'_>,
) -> TerminalWriterPacketCorrelation {
    match evidence {
        TerminalWriterTracePacketEvidence::Unproven => TerminalWriterPacketCorrelation::Unproven,
        TerminalWriterTracePacketEvidence::FingerprintOnly { matches: true } => {
            TerminalWriterPacketCorrelation::FingerprintOnly
        }
        TerminalWriterTracePacketEvidence::FingerprintOnly { matches: false } => {
            TerminalWriterPacketCorrelation::Unproven
        }
        TerminalWriterTracePacketEvidence::FullFinalizedPayload(finalized)
            if finalized.len() <= MAX_REASONABLE_LIVE_PAYLOAD_BYTES
                && quarantined_payload.len() <= MAX_REASONABLE_LIVE_PAYLOAD_BYTES
                && finalized == quarantined_payload =>
        {
            TerminalWriterPacketCorrelation::ExactPayloadBytes
        }
        TerminalWriterTracePacketEvidence::FullFinalizedPayload(finalized)
            if finalized.len() > MAX_REASONABLE_LIVE_PAYLOAD_BYTES
                || quarantined_payload.len() > MAX_REASONABLE_LIVE_PAYLOAD_BYTES =>
        {
            TerminalWriterPacketCorrelation::Unproven
        }
        TerminalWriterTracePacketEvidence::FullFinalizedPayload(_) => {
            TerminalWriterPacketCorrelation::DifferentPayloadBytes
        }
    }
}

fn correlate_observation(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    observation: TerminalWriterObservation,
) -> LiveObjectUpdateTerminalWriterHandoffVerdict {
    use LiveObjectUpdateTerminalWriterHandoffVerdict as Verdict;

    if observation.packet_correlation == TerminalWriterPacketCorrelation::DifferentPayloadBytes {
        return Verdict::PacketMismatch;
    }
    if observation.object_type != requirement.object_type
        || observation.object_id != requirement.object_id
        || observation.raw_mask != requirement.raw_mask
    {
        return Verdict::IdentityMismatch;
    }
    if observation.replayed_read_buffer_cursor != requirement.source_read_buffer_cursor
        || observation.replayed_read_buffer_end != requirement.source_read_buffer_end
    {
        return Verdict::ReadBufferMismatch;
    }

    let observed_bits = observation.fragment_bits_written;
    if observed_bits.bit_count == 0 && observed_bits.bit_start == observed_bits.bit_end {
        if observed_bits.bit_start != requirement.source_fragment_bits.bit_start
            || observation.finalized_fragment_bit_cursor != observed_bits.bit_end
        {
            return Verdict::CursorGapOrOverlap;
        }
        return match observation.packet_correlation {
            TerminalWriterPacketCorrelation::ExactPayloadBytes => Verdict::NoTerminalFragmentWrites,
            TerminalWriterPacketCorrelation::Unproven
            | TerminalWriterPacketCorrelation::FingerprintOnly => {
                Verdict::MatchingWriterTracePacketUncorrelated
            }
            TerminalWriterPacketCorrelation::DifferentPayloadBytes => Verdict::PacketMismatch,
        };
    }
    if observed_bits.bit_start != requirement.source_fragment_bits.bit_start
        || observed_bits.bit_end != requirement.source_fragment_bits.bit_end
        || observed_bits.bit_count != requirement.source_fragment_bits.bit_count
    {
        return Verdict::CursorGapOrOverlap;
    }
    if observation.finalized_fragment_bit_cursor != requirement.source_fragment_bits.bit_end {
        return Verdict::CursorGapOrOverlap;
    }
    if !bit_slice_is_fully_retained(observed_bits)
        || !bit_slice_is_fully_retained(requirement.source_fragment_bits)
    {
        return Verdict::IncompleteTrace;
    }
    if observed_bits
        .bits
        .iter()
        .take(observed_bits.bit_count)
        .ne(requirement
            .source_fragment_bits
            .bits
            .iter()
            .take(requirement.source_fragment_bits.bit_count))
    {
        return Verdict::BitMismatch;
    }

    match observation.packet_correlation {
        TerminalWriterPacketCorrelation::DifferentPayloadBytes => Verdict::PacketMismatch,
        TerminalWriterPacketCorrelation::ExactPayloadBytes => Verdict::ExactObservedHandoff,
        TerminalWriterPacketCorrelation::Unproven
        | TerminalWriterPacketCorrelation::FingerprintOnly => {
            Verdict::MatchingWriterTracePacketUncorrelated
        }
    }
}

fn bit_slice_is_fully_retained(evidence: LiveObjectUpdateRewriteBitSliceEvidence) -> bool {
    evidence.bit_count == evidence.bit_end.saturating_sub(evidence.bit_start)
        && evidence.bit_count <= evidence.bits.len()
        && evidence.bits_retained == evidence.bit_count
        && evidence
            .bits
            .iter()
            .take(evidence.bits_retained)
            .all(Option::is_some)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE_VALUES: [bool; 13] = [
        false, false, false, false, false, false, true, false, false, false, true, true, false,
    ];

    fn bit_slice(bit_start: usize, values: &[bool]) -> LiveObjectUpdateRewriteBitSliceEvidence {
        let mut bits = [None; LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT];
        for (slot, value) in bits.iter_mut().zip(values.iter().copied()) {
            *slot = Some(value);
        }
        LiveObjectUpdateRewriteBitSliceEvidence {
            bit_start,
            bit_end: bit_start + values.len(),
            bit_count: values.len(),
            bits_retained: values.len(),
            bits,
        }
    }

    fn requirement() -> LiveObjectUpdateTerminalWriterHandoffRequirement {
        LiveObjectUpdateTerminalWriterHandoffRequirement {
            object_type: 0x09,
            object_id: 0x8000_1003,
            raw_mask: 0xFFFF_FFF7,
            source_read_buffer_cursor: 229,
            source_read_buffer_end: 229,
            source_fragment_bits: bit_slice(63, &SOURCE_VALUES),
            source_next_opcode_read_overflows: true,
            emitted_read_buffer_cursor: 243,
            emitted_read_buffer_end: 243,
            emitted_fragment_bit_start: 71,
            emitted_fragment_bit_end: 88,
            emitted_fragment_bit_count: 17,
            emitted_fragment_bits_retained: 17,
            emitted_fragment_bits: super::super::LiveObjectUpdatePackedFragmentBitSpanEvidence {
                bit_start: 71,
                bit_end: 88,
                packed_msb: 0x4046,
            },
            emitted_next_opcode_read_overflows: true,
        }
    }

    fn coherent_payload() -> (Vec<u8>, usize) {
        let door_id = 0x8000_1001u32;
        let first_placeable_id = 0x8000_1002u32;
        let second_placeable_id = 0x8000_1003u32;
        let first_name = b"Storage Drum";
        let second_name = b"Generic Placeable Interaction Gate";
        let mut live = Vec::new();

        live.extend_from_slice(&[b'A', 10]);
        live.extend_from_slice(&door_id.to_le_bytes());
        live.extend_from_slice(&0u32.to_le_bytes());
        live.extend_from_slice(&1u32.to_le_bytes());
        live.extend_from_slice(&0x0000_14E5u32.to_le_bytes());
        live.extend_from_slice(&0x0033u16.to_le_bytes());
        live.extend_from_slice(&[b'U', 10]);
        live.extend_from_slice(&door_id.to_le_bytes());
        live.extend_from_slice(&0xFFFF_FFF7u32.to_le_bytes());
        live.extend_from_slice(&[
            0x0C, 0x17, 0x66, 0x1C, 0x0F, 0x0F, 0x00, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x33,
            0x00, 0xE5, 0x14, 0x00, 0x00,
        ]);

        let mut terminal_record_offset = None;
        for (object_id, name, appearance, update_body) in [
            (
                first_placeable_id,
                first_name.as_slice(),
                0x01CFu16,
                [
                    0x43, 0x19, 0x1A, 0x1D, 0x11, 0x0F, 0x00, 0xE7, 0x03, 0x00, 0x00, 0x80, 0x3F,
                    0x00, 0x00,
                ],
            ),
            (
                second_placeable_id,
                second_name.as_slice(),
                0x0090u16,
                [
                    0x5A, 0x14, 0xFC, 0x1B, 0x0F, 0x0F, 0x00, 0xF6, 0x01, 0x00, 0x00, 0x80, 0x3F,
                    0x00, 0x00,
                ],
            ),
        ] {
            live.extend_from_slice(&[b'A', 9]);
            live.extend_from_slice(&object_id.to_le_bytes());
            live.extend_from_slice(&(name.len() as u32).to_le_bytes());
            live.extend_from_slice(name);
            live.push(0x05);
            live.extend_from_slice(&appearance.to_le_bytes());
            live.extend_from_slice(&0u16.to_le_bytes());

            if object_id == second_placeable_id {
                terminal_record_offset = Some(LIVE_OBJECT_CNW_WRITER_HEADER_BYTES + live.len());
            }
            live.extend_from_slice(&[b'U', 9]);
            live.extend_from_slice(&object_id.to_le_bytes());
            live.extend_from_slice(&0xFFFF_FFF7u32.to_le_bytes());
            live.extend_from_slice(&update_body);
            live.extend_from_slice(&(name.len() as u32).to_le_bytes());
            live.extend_from_slice(name);
        }

        let mut payload = vec![b'P', 0x05, 0x01];
        let declared = LIVE_OBJECT_CNW_WRITER_HEADER_BYTES + live.len();
        payload.extend_from_slice(&(declared as u32).to_le_bytes());
        payload.extend_from_slice(&live);
        payload.extend_from_slice(&[0x9A, 0x60, 0x23, 0xAB, 0x88, 0x08, 0xD5, 0xC4, 0x04, 0x62]);
        assert_eq!(declared, 236);
        assert_eq!(cnw_fragment_valid_bits(&payload[declared..]), Some(76));
        (
            payload,
            terminal_record_offset.expect("terminal update offset"),
        )
    }

    fn exact_trace<'a>(payload: &'a [u8], record_offset: usize) -> TerminalWriterTrace<'a> {
        let declared = usize::try_from(u32::from_le_bytes(
            payload[3..7].try_into().expect("declared bytes"),
        ))
        .expect("declared length");
        let fragment_bit_cursor =
            cnw_fragment_valid_bits(&payload[declared..]).expect("coherent fragment bit count");
        TerminalWriterTrace {
            events: [
                TerminalWriterTraceEvent::OwnerBegin {
                    absolute_record_offset: record_offset,
                    absolute_read_buffer_cursor: record_offset,
                    fragment_bit_cursor: 63,
                },
                TerminalWriterTraceEvent::OwnerEnd {
                    absolute_read_buffer_cursor: declared,
                    fragment_bit_cursor,
                },
                TerminalWriterTraceEvent::ListHandoff,
                TerminalWriterTraceEvent::Finalize {
                    absolute_read_buffer_end: declared,
                    fragment_bit_cursor,
                    packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(
                        payload,
                    ),
                },
            ],
        }
    }

    #[test]
    fn exact_trace_is_derived_from_one_coherent_finalized_packet() {
        let (payload, record_offset) = coherent_payload();
        let trace = exact_trace(&payload, record_offset);
        let observation = build_terminal_writer_observation(&payload, trace)
            .expect("coherent bounded trace observation");
        assert_eq!(observation.object_type, 0x09);
        assert_eq!(observation.object_id, 0x8000_1003);
        assert_eq!(observation.raw_mask, 0xFFFF_FFF7);
        assert_eq!(observation.replayed_read_buffer_cursor, 229);
        assert_eq!(observation.replayed_read_buffer_end, 229);
        assert_eq!(observation.fragment_bits_written.bit_start, 63);
        assert_eq!(observation.fragment_bits_written.bit_end, 76);
        assert_eq!(
            correlate_bounded_terminal_writer_trace(requirement(), &payload, Some(trace)),
            LiveObjectUpdateTerminalWriterHandoffVerdict::ExactObservedHandoff
        );
    }

    #[test]
    fn stock_zero_delta_requires_a_coherent_63_bit_finalized_packet() {
        let (mut payload, record_offset) = coherent_payload();
        let declared = 236;
        payload.truncate(declared + 8);
        payload[declared] = (payload[declared] & 0x1F) | 0xE0;
        payload[declared + 7] &= 0xFE;
        assert_eq!(cnw_fragment_valid_bits(&payload[declared..]), Some(63));
        let mut trace = exact_trace(&payload, record_offset);
        trace.events[1] = TerminalWriterTraceEvent::OwnerEnd {
            absolute_read_buffer_cursor: declared,
            fragment_bit_cursor: 63,
        };
        trace.events[3] = TerminalWriterTraceEvent::Finalize {
            absolute_read_buffer_end: declared,
            fragment_bit_cursor: 63,
            packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(&payload),
        };
        assert_eq!(
            correlate_bounded_terminal_writer_trace(requirement(), &payload, Some(trace)),
            LiveObjectUpdateTerminalWriterHandoffVerdict::NoTerminalFragmentWrites
        );
    }

    #[test]
    fn malformed_envelopes_declared_windows_fragments_and_event_order_reject() {
        let (payload, record_offset) = coherent_payload();

        let mut bad_header = payload.clone();
        bad_header[2] = 0x02;
        let mut bad_header_trace = exact_trace(&payload, record_offset);
        bad_header_trace.events[3] = TerminalWriterTraceEvent::Finalize {
            absolute_read_buffer_end: 236,
            fragment_bit_cursor: 76,
            packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(&bad_header),
        };
        assert!(build_terminal_writer_observation(&bad_header, bad_header_trace).is_none());

        let mut bad_declared = payload.clone();
        let bad_declared_len = bad_declared.len() as u32;
        bad_declared[3..7].copy_from_slice(&bad_declared_len.to_le_bytes());
        let mut bad_declared_trace = exact_trace(&payload, record_offset);
        bad_declared_trace.events[3] = TerminalWriterTraceEvent::Finalize {
            absolute_read_buffer_end: 236,
            fragment_bit_cursor: 76,
            packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(&bad_declared),
        };
        assert!(build_terminal_writer_observation(&bad_declared, bad_declared_trace).is_none());

        let mut bad_fragment = payload.clone();
        bad_fragment.truncate(237);
        bad_fragment[236] = (bad_fragment[236] & 0x1F) | 0x20;
        let mut bad_fragment_trace = exact_trace(&payload, record_offset);
        bad_fragment_trace.events[3] = TerminalWriterTraceEvent::Finalize {
            absolute_read_buffer_end: 236,
            fragment_bit_cursor: 1,
            packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(&bad_fragment),
        };
        assert!(build_terminal_writer_observation(&bad_fragment, bad_fragment_trace).is_none());

        let mut bad_order = exact_trace(&payload, record_offset);
        bad_order.events.swap(1, 2);
        assert!(build_terminal_writer_observation(&payload, bad_order).is_none());
    }

    #[test]
    fn cursor_fragment_and_record_token_mismatches_reject_before_correlation() {
        let (payload, record_offset) = coherent_payload();

        let mut read_cursor = exact_trace(&payload, record_offset);
        read_cursor.events[1] = TerminalWriterTraceEvent::OwnerEnd {
            absolute_read_buffer_cursor: 235,
            fragment_bit_cursor: 76,
        };
        assert!(build_terminal_writer_observation(&payload, read_cursor).is_none());

        let mut fragment_cursor = exact_trace(&payload, record_offset);
        fragment_cursor.events[3] = TerminalWriterTraceEvent::Finalize {
            absolute_read_buffer_end: 236,
            fragment_bit_cursor: 75,
            packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(&payload),
        };
        assert!(build_terminal_writer_observation(&payload, fragment_cursor).is_none());

        let mut record_token = exact_trace(&payload, record_offset);
        record_token.events[0] = TerminalWriterTraceEvent::OwnerBegin {
            absolute_record_offset: record_offset + 1,
            absolute_read_buffer_cursor: record_offset + 1,
            fragment_bit_cursor: 63,
        };
        assert!(build_terminal_writer_observation(&payload, record_token).is_none());
    }

    #[test]
    fn packet_bound_identity_bits_and_cross_packet_evidence_cannot_false_match() {
        let (payload, record_offset) = coherent_payload();

        let mut changed_identity = payload.clone();
        changed_identity[record_offset + 2] ^= 1;
        let identity_trace = exact_trace(&changed_identity, record_offset);
        assert_eq!(
            correlate_bounded_terminal_writer_trace(
                requirement(),
                &changed_identity,
                Some(identity_trace),
            ),
            LiveObjectUpdateTerminalWriterHandoffVerdict::IdentityMismatch
        );

        let mut changed_bits = payload.clone();
        let declared = 236;
        changed_bits[declared + 8] ^= 0x02;
        let changed_bits_trace = exact_trace(&changed_bits, record_offset);
        assert_eq!(
            correlate_bounded_terminal_writer_trace(
                requirement(),
                &changed_bits,
                Some(changed_bits_trace),
            ),
            LiveObjectUpdateTerminalWriterHandoffVerdict::BitMismatch
        );

        let mut other_packet = payload.clone();
        other_packet[declared + 8] ^= 0x02;
        let cross_packet_trace = exact_trace(&other_packet, record_offset);
        assert_eq!(
            correlate_bounded_terminal_writer_trace(
                requirement(),
                &payload,
                Some(cross_packet_trace),
            ),
            LiveObjectUpdateTerminalWriterHandoffVerdict::PacketMismatch
        );
    }

    #[test]
    fn fingerprints_and_oversized_payloads_never_become_exact() {
        let (payload, record_offset) = coherent_payload();
        for packet_evidence in [
            TerminalWriterTracePacketEvidence::FingerprintOnly { matches: true },
            TerminalWriterTracePacketEvidence::FingerprintOnly { matches: false },
            TerminalWriterTracePacketEvidence::Unproven,
        ] {
            let mut trace = exact_trace(&payload, record_offset);
            trace.events[3] = TerminalWriterTraceEvent::Finalize {
                absolute_read_buffer_end: 236,
                fragment_bit_cursor: 76,
                packet_evidence,
            };
            assert_eq!(
                correlate_bounded_terminal_writer_trace(requirement(), &payload, Some(trace)),
                LiveObjectUpdateTerminalWriterHandoffVerdict::MatchingWriterTracePacketUncorrelated
            );
        }

        let oversized = vec![0u8; MAX_REASONABLE_LIVE_PAYLOAD_BYTES + 1];
        assert_eq!(
            packet_correlation(
                &oversized,
                TerminalWriterTracePacketEvidence::FullFinalizedPayload(&oversized),
            ),
            TerminalWriterPacketCorrelation::Unproven
        );
    }

    #[test]
    fn production_facade_cannot_accept_a_sibling_constructed_observation() {
        let (payload, _) = coherent_payload();
        assert_eq!(
            correlate_terminal_writer_trace(requirement(), &payload),
            LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace
        );
    }
}
