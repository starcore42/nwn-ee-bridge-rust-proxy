use super::{
    LEGACY_UPDATE_HEADER_BYTES, LiveObjectUpdateRewriteBitSliceEvidence,
    LiveObjectUpdateRewriteTailEvidence,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateDoorPlaceableTail9ResidualEvidence {
    /// Typed identity captured from the exact terminal record at failure time.
    /// Keep this with the evidence: later orchestration passes may shift the
    /// staged payload, so the formatter must not reapply `failure.offset`.
    pub object_type: u8,
    pub object_id: u32,
    pub raw_mask: u32,
    pub translated_mask: u32,
    pub source_fragment_bit_count: usize,
    pub source_bit_cursor: usize,
    pub source_reader_bit_cursor: usize,
    pub source_reader_bits_consumed: usize,
    pub source_name_selector_bit_cursor: Option<usize>,
    pub source_name_selector: Option<bool>,
    pub source_name_locstring_selector_bit_cursor: Option<usize>,
    pub source_name_locstring_selector: Option<bool>,
    pub source_name_kind: Option<&'static str>,
    pub source_reader_residual: LiveObjectUpdateRewriteBitSliceEvidence,
    pub emitted_bit_cursor: usize,
    pub emitted_fragment_bit_count: usize,
    pub rewritten_bit_cursor: usize,
    pub rewritten_fragment_bit_count: usize,
    pub residual_fragment_bits: usize,
    pub rewritten_residual: LiveObjectUpdateRewriteBitSliceEvidence,
    pub rewritten_residual_exact: Option<LiveObjectUpdatePackedFragmentBitSpanEvidence>,
    pub proven_terminal_packed_name_bits: usize,
    pub precursor_tail: Option<LiveObjectUpdateRewriteTailEvidence>,
    pub stock_diamond_source: Option<LiveObjectUpdateDoorPlaceableStockSourceEvidence>,
    pub terminal_reader_continuation: LiveObjectUpdateTerminalReaderContinuationEvidence,
    pub terminal_fragment_handoff_correlation:
        Option<LiveObjectUpdateTerminalFragmentHandoffCorrelationEvidence>,
    pub end_aligned_diamond_reader_candidate_count: usize,
    pub end_aligned_diamond_reader_candidates:
        [Option<LiveObjectUpdateDoorPlaceableEndAlignedDiamondReaderCandidateEvidence>;
            LIVE_OBJECT_UPDATE_END_ALIGNED_DIAMOND_READER_CANDIDATE_LIMIT],
    pub source_suffix_candidate_count: usize,
    pub source_suffix_candidates: [Option<
        LiveObjectUpdateDoorPlaceableTail9SourceCandidateEvidence,
    >; LIVE_OBJECT_UPDATE_TAIL9_SOURCE_CANDIDATE_LIMIT],
}

/// The exact join contract that a server-side writer/list trace must satisfy
/// before the terminal fragment can be attributed to a writer.
///
/// This remains diagnostic evidence. Even an exact writer observation does
/// not authorize a cursor advance, rewrite, or trim: proxy2 still needs a
/// typed EE writer and a final exact EE payload claim before wire behavior can
/// change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateTerminalWriterHandoffRequirement {
    pub object_type: u8,
    pub object_id: u32,
    pub raw_mask: u32,
    pub source_read_buffer_cursor: usize,
    pub source_read_buffer_end: usize,
    pub source_fragment_bits: LiveObjectUpdateRewriteBitSliceEvidence,
    pub source_next_opcode_read_overflows: bool,
    pub emitted_read_buffer_cursor: usize,
    pub emitted_read_buffer_end: usize,
    pub emitted_fragment_bit_start: usize,
    pub emitted_fragment_bit_end: usize,
    pub emitted_fragment_bit_count: usize,
    pub emitted_fragment_bits_retained: usize,
    /// Exact staged EE-side fragment values in full MSB-first reader order.
    /// This is an obligation for a future typed EE writer/final validator, not
    /// permission to consume or trim the still-unowned source fragment.
    pub emitted_fragment_bits: LiveObjectUpdatePackedFragmentBitSpanEvidence,
    pub emitted_next_opcode_read_overflows: bool,
}

/// Why a writer/list observation does or does not satisfy the exact source
/// handoff contract. No verdict here is itself an EE rewrite authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveObjectUpdateTerminalWriterHandoffVerdict {
    IncompleteTrace,
    IdentityMismatch,
    ReadBufferMismatch,
    NoTerminalFragmentWrites,
    CursorGapOrOverlap,
    BitMismatch,
    PacketMismatch,
    MatchingWriterTracePacketUncorrelated,
    ExactObservedHandoff,
}

impl LiveObjectUpdateTerminalWriterHandoffVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IncompleteTrace => "incomplete-trace",
            Self::IdentityMismatch => "identity-mismatch",
            Self::ReadBufferMismatch => "read-buffer-mismatch",
            Self::NoTerminalFragmentWrites => "no-terminal-fragment-writes",
            Self::CursorGapOrOverlap => "cursor-gap-or-overlap",
            Self::BitMismatch => "bit-mismatch",
            Self::PacketMismatch => "packet-mismatch",
            Self::MatchingWriterTracePacketUncorrelated => {
                "matching-writer-trace-packet-uncorrelated"
            }
            Self::ExactObservedHandoff => "exact-observed-handoff",
        }
    }

    pub fn writer_handoff_observed(self) -> bool {
        self == Self::ExactObservedHandoff
    }

    /// Writer ownership alone is not an exact EE packet claim.
    pub fn allows_exact_claim(self) -> bool {
        false
    }
}

/// One future typed EE-writer/final-validator observation for the emitted half
/// of the terminal handoff contract.
///
/// This is deliberately separate from source-writer ownership. Matching these
/// cursors and values only proves that the prospective EE output is exact; it
/// cannot authorize a claim until the HG source owner is independently proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LiveObjectUpdateTerminalEeFinalClaimObservation {
    read_buffer_cursor: usize,
    read_buffer_end: usize,
    fragment_bits_written: LiveObjectUpdatePackedFragmentBitSpanEvidence,
    final_fragment_bit_cursor: usize,
    final_fragment_bit_end: usize,
    exact_payload_validator_accepted: bool,
}

/// Whether the emitted half of a terminal writer handoff is ready to be joined
/// to a separately proven HG source owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveObjectUpdateTerminalEeFinalClaimReadinessVerdict {
    InvalidEmittedRequirement,
    IncompleteTypedEeWriter,
    ReadBufferMismatch,
    CursorGapOrOverlap,
    BitMismatch,
    TypedCursorOverrun,
    ExactTypedEeFinalClaimReady,
}

impl LiveObjectUpdateTerminalEeFinalClaimReadinessVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidEmittedRequirement => "invalid-emitted-requirement",
            Self::IncompleteTypedEeWriter => "incomplete-typed-ee-writer",
            Self::ReadBufferMismatch => "read-buffer-mismatch",
            Self::CursorGapOrOverlap => "cursor-gap-or-overlap",
            Self::BitMismatch => "bit-mismatch",
            Self::TypedCursorOverrun => "typed-cursor-overrun",
            Self::ExactTypedEeFinalClaimReady => "exact-typed-ee-final-claim-ready",
        }
    }

    pub fn final_claim_ready(self) -> bool {
        self == Self::ExactTypedEeFinalClaimReady
    }

    /// Emitted-side readiness alone never owns the source fragment.
    pub fn allows_exact_claim(self) -> bool {
        false
    }

    pub fn authorizes_rewrite(self) -> bool {
        false
    }

    pub fn authorizes_cursor_advance(self) -> bool {
        false
    }

    pub fn authorizes_fragment_trim(self) -> bool {
        false
    }
}

impl LiveObjectUpdateTerminalWriterHandoffRequirement {
    fn correlate_ee_final_claim(
        self,
        observation: Option<LiveObjectUpdateTerminalEeFinalClaimObservation>,
    ) -> LiveObjectUpdateTerminalEeFinalClaimReadinessVerdict {
        use LiveObjectUpdateTerminalEeFinalClaimReadinessVerdict as Verdict;

        let emitted_bits = self.emitted_fragment_bits;
        if !emitted_bits.is_valid()
            || self.emitted_read_buffer_cursor != self.emitted_read_buffer_end
            || self.emitted_fragment_bit_start != emitted_bits.bit_start
            || self.emitted_fragment_bit_end != emitted_bits.bit_end
            || self.emitted_fragment_bit_count != emitted_bits.bit_count()
            || self.emitted_fragment_bits_retained != emitted_bits.bit_count()
            || self.emitted_fragment_bit_count
                != self
                    .emitted_fragment_bit_end
                    .saturating_sub(self.emitted_fragment_bit_start)
            || !self.emitted_next_opcode_read_overflows
        {
            return Verdict::InvalidEmittedRequirement;
        }
        let Some(observation) = observation else {
            return Verdict::IncompleteTypedEeWriter;
        };
        if !observation.fragment_bits_written.is_valid() {
            return Verdict::IncompleteTypedEeWriter;
        }
        if !observation.exact_payload_validator_accepted {
            return Verdict::IncompleteTypedEeWriter;
        }
        if observation.read_buffer_cursor > observation.read_buffer_end
            || observation.final_fragment_bit_cursor > observation.final_fragment_bit_end
        {
            return Verdict::TypedCursorOverrun;
        }
        if observation.read_buffer_cursor != self.emitted_read_buffer_cursor
            || observation.read_buffer_end != self.emitted_read_buffer_end
        {
            return Verdict::ReadBufferMismatch;
        }
        if observation.fragment_bits_written.bit_start != self.emitted_fragment_bits.bit_start
            || observation.fragment_bits_written.bit_end != self.emitted_fragment_bits.bit_end
            || observation.final_fragment_bit_cursor != self.emitted_fragment_bit_end
            || observation.final_fragment_bit_end != self.emitted_fragment_bit_end
        {
            return Verdict::CursorGapOrOverlap;
        }
        if observation.fragment_bits_written.packed_msb != self.emitted_fragment_bits.packed_msb {
            return Verdict::BitMismatch;
        }
        Verdict::ExactTypedEeFinalClaimReady
    }

    pub(super) fn ee_final_claim_readiness_without_observation(
        self,
    ) -> LiveObjectUpdateTerminalEeFinalClaimReadinessVerdict {
        self.correlate_ee_final_claim(None)
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

/// An end-aligned exact Diamond reader interpretation that would require
/// reusing the already-consumed update record bytes to consume only the
/// terminal fragment residue.
///
/// Diamond `sub_44EF00` and EE `sub_14079BCE0` both test
/// `MessageMoreDataToRead`, then read a fresh 8-bit row opcode before entering
/// another typed row reader. Consequently, a fragment-only interpretation
/// with no remaining read-buffer bytes cannot be a legal second stock row: it
/// has no `U/type/object-id/mask` header and the opcode read overflows. This is
/// still only failure evidence. It does not prove that a custom writer replayed
/// the fields or authorize a cursor advance, rewrite, or fragment trim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateTerminalReusedRecordReaderInterpretationEvidence {
    pub candidate_index: usize,
    pub record_end: usize,
    pub read_buffer_cursor: usize,
    pub read_buffer_end: usize,
    pub required_second_row_header_bytes: usize,
    pub available_second_row_header_bytes: usize,
    pub stock_fragment_bit_start: usize,
    pub stock_fragment_bit_end: usize,
    pub candidate_fragment_bit_start: usize,
    pub candidate_fragment_bit_end: usize,
    pub fragment_gap_bits: usize,
    pub reader_shape_bits: usize,
    pub second_stock_row_dispatch_possible: bool,
}

impl LiveObjectUpdateDoorPlaceableTail9ResidualEvidence {
    /// Build the exact source writer span plus the emitted final-claim
    /// obligation for a future server-side writer or list-handoff trace.
    /// Return no contract when the bounded evidence did not retain every
    /// source MSB-first residual bit.
    pub fn writer_handoff_requirement(
        &self,
    ) -> Option<LiveObjectUpdateTerminalWriterHandoffRequirement> {
        let continuation = self.terminal_reader_continuation;
        let emitted_fragment_bits = self.rewritten_residual_exact?;
        let source_fragment_bits = match self.stock_diamond_source {
            Some(source)
                if source.read_end == continuation.source_read_buffer_cursor
                    && source.source_reader_bit_cursor
                        == continuation.source_fragment_bit_cursor =>
            {
                source.source_reader_residual
            }
            Some(_) => return None,
            None => self.source_reader_residual,
        };
        if continuation.source_read_buffer_cursor != continuation.source_read_buffer_end
            || continuation.emitted_read_buffer_cursor != continuation.emitted_read_buffer_end
            || continuation.source_more_data_source
                != LiveObjectUpdateReaderContinuationSource::FragmentOnly
            || continuation.emitted_more_data_source
                != LiveObjectUpdateReaderContinuationSource::FragmentOnly
            || !continuation.source_next_opcode_read_overflows
            || !continuation.emitted_next_opcode_read_overflows
            || source_fragment_bits.bit_start != continuation.source_fragment_bit_cursor
            || source_fragment_bits.bit_end != continuation.source_fragment_bit_end
            || self.rewritten_residual.bit_start != continuation.emitted_fragment_bit_cursor
            || self.rewritten_residual.bit_end != continuation.emitted_fragment_bit_end
            || emitted_fragment_bits.bit_start != continuation.emitted_fragment_bit_cursor
            || emitted_fragment_bits.bit_end != continuation.emitted_fragment_bit_end
            || !bit_slice_is_fully_retained(source_fragment_bits)
        {
            return None;
        }

        Some(LiveObjectUpdateTerminalWriterHandoffRequirement {
            object_type: self.object_type,
            object_id: self.object_id,
            raw_mask: self.raw_mask,
            source_read_buffer_cursor: continuation.source_read_buffer_cursor,
            source_read_buffer_end: continuation.source_read_buffer_end,
            source_fragment_bits,
            source_next_opcode_read_overflows: continuation.source_next_opcode_read_overflows,
            emitted_read_buffer_cursor: continuation.emitted_read_buffer_cursor,
            emitted_read_buffer_end: continuation.emitted_read_buffer_end,
            emitted_fragment_bit_start: self.rewritten_residual.bit_start,
            emitted_fragment_bit_end: self.rewritten_residual.bit_end,
            emitted_fragment_bit_count: self.rewritten_residual.bit_count,
            emitted_fragment_bits_retained: emitted_fragment_bits.bit_count(),
            emitted_fragment_bits,
            emitted_next_opcode_read_overflows: continuation.emitted_next_opcode_read_overflows,
        })
    }

    pub fn source_fragment_ownership_verdict(
        &self,
    ) -> LiveObjectUpdateTerminalFragmentOwnershipVerdict {
        self.terminal_reader_continuation
            .source_fragment_ownership_verdict()
    }

    pub fn emitted_fragment_ownership_verdict(
        &self,
    ) -> LiveObjectUpdateTerminalFragmentOwnershipVerdict {
        self.terminal_reader_continuation
            .emitted_fragment_ownership_verdict()
    }

    /// Classify exact end-aligned walks that repeat the anchored stock reader's
    /// ordered field topology and would require reusing the same exhausted byte
    /// record.
    /// Bit values deliberately are not compared: a shape match narrows the
    /// owner trace even when the terminal values differ from the first walk.
    pub fn reused_record_reader_interpretation(
        &self,
    ) -> Option<LiveObjectUpdateTerminalReusedRecordReaderInterpretationEvidence> {
        let Some(stock) = self.stock_diamond_source else {
            return None;
        };
        let continuation = self.terminal_reader_continuation;
        if continuation.source_more_data_source
            != LiveObjectUpdateReaderContinuationSource::FragmentOnly
            || !continuation.source_next_opcode_read_overflows
            || continuation.source_read_buffer_cursor != continuation.source_read_buffer_end
            || stock.read_end != continuation.source_read_buffer_end
            || continuation.source_fragment_bit_cursor != stock.source_reader_bit_cursor
            || continuation.source_fragment_bit_end != self.source_fragment_bit_count
        {
            return None;
        }

        let stock_relative_state =
            relative_fragment_cursor(stock.source_state_bit_cursor, stock.source_bit_cursor);
        let stock_relative_name = relative_fragment_cursor(
            stock.source_name_selector_bit_cursor,
            stock.source_bit_cursor,
        );
        let stock_relative_locstring = relative_fragment_cursor(
            stock.source_name_locstring_selector_bit_cursor,
            stock.source_bit_cursor,
        );
        for (candidate_index, candidate) in self
            .end_aligned_diamond_reader_candidates
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| candidate.map(|candidate| (index, candidate)))
        {
            let same_field_topology = candidate.raw_mask == stock.raw_mask
                && candidate.effective_mask == stock.effective_mask
                && candidate.ignored_mask == stock.ignored_mask
                && candidate.source_reader_bits_consumed == stock.source_reader_bits_consumed
                && candidate.source_orientation_vector == stock.source_orientation_vector
                && relative_fragment_cursor(
                    candidate.source_state_bit_cursor,
                    candidate.source_bit_cursor,
                ) == stock_relative_state
                && relative_fragment_cursor(
                    candidate.source_name_selector_bit_cursor,
                    candidate.source_bit_cursor,
                ) == stock_relative_name
                && relative_fragment_cursor(
                    candidate.source_name_locstring_selector_bit_cursor,
                    candidate.source_bit_cursor,
                ) == stock_relative_locstring
                && candidate.source_name_kind == stock.source_name_kind;
            let contiguous_reused_record = candidate.read_end == stock.read_end
                && candidate.read_end == continuation.source_read_buffer_end
                && candidate.source_bit_cursor == stock.source_reader_bit_cursor
                && candidate.source_reader_bit_cursor == self.source_fragment_bit_count
                && candidate
                    .source_gap_from_anchored_reader
                    .is_some_and(|gap| {
                        gap.bit_start == stock.source_reader_bit_cursor
                            && gap.bit_end == candidate.source_bit_cursor
                            && gap.bit_count == 0
                    });
            if !same_field_topology || !contiguous_reused_record {
                continue;
            }

            return Some(
                LiveObjectUpdateTerminalReusedRecordReaderInterpretationEvidence {
                    candidate_index,
                    record_end: candidate.read_end,
                    read_buffer_cursor: continuation.source_read_buffer_cursor,
                    read_buffer_end: continuation.source_read_buffer_end,
                    required_second_row_header_bytes: LEGACY_UPDATE_HEADER_BYTES,
                    available_second_row_header_bytes: continuation
                        .source_read_buffer_end
                        .saturating_sub(continuation.source_read_buffer_cursor),
                    stock_fragment_bit_start: stock.source_bit_cursor,
                    stock_fragment_bit_end: stock.source_reader_bit_cursor,
                    candidate_fragment_bit_start: candidate.source_bit_cursor,
                    candidate_fragment_bit_end: candidate.source_reader_bit_cursor,
                    fragment_gap_bits: candidate
                        .source_bit_cursor
                        .saturating_sub(stock.source_reader_bit_cursor),
                    reader_shape_bits: candidate.source_reader_bits_consumed,
                    second_stock_row_dispatch_possible: false,
                },
            );
        }
        None
    }
}

fn relative_fragment_cursor(cursor: Option<usize>, bit_start: usize) -> Option<usize> {
    cursor.and_then(|cursor| cursor.checked_sub(bit_start))
}

/// Which CNW input store makes `MessageMoreDataToRead` continue dispatch.
///
/// Diamond `sub_4FBBA0` and EE `CNWMessage::MessageMoreDataToRead`
/// (`0x1402D9430`) both check the read-buffer cursor first and then the
/// fragment-byte/bit cursor. The live-object dispatchers consequently attempt
/// another 8-bit row-opcode read even when only fragment bits remain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveObjectUpdateReaderContinuationSource {
    None,
    ReadBufferOnly,
    FragmentOnly,
    ReadBufferAndFragment,
}

/// Whether the terminal CNW read-buffer and MSB-first fragment cursor are
/// fully owned by typed readers.
///
/// A fragment-only continuation is not padding: Diamond `sub_44EF00` and EE
/// `sub_14079BCE0` both attempt another row-opcode read and overflow once the
/// byte cursor is exhausted. Consequently only `FullyConsumedByTypedReaders`
/// may pass the exact claim gate. A future HG-specific owner must consume its
/// proven field span through a typed reader/writer before this verdict can
/// become exact; the verdict itself never authorizes trimming or synthesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveObjectUpdateTerminalFragmentOwnershipVerdict {
    FullyConsumedByTypedReaders,
    ReadBufferUnconsumed,
    FragmentWriterOwnerUnproven,
    ReadBufferUnconsumedAndFragmentWriterOwnerUnproven,
    TypedCursorOverrun,
}

impl LiveObjectUpdateTerminalFragmentOwnershipVerdict {
    pub(super) fn from_cursors(
        read_buffer_cursor: usize,
        read_buffer_end: usize,
        fragment_bit_cursor: usize,
        fragment_bit_end: usize,
    ) -> Self {
        if read_buffer_cursor > read_buffer_end || fragment_bit_cursor > fragment_bit_end {
            return Self::TypedCursorOverrun;
        }
        Self::from_reader_continuation(LiveObjectUpdateReaderContinuationSource::from_cursors(
            read_buffer_cursor,
            read_buffer_end,
            fragment_bit_cursor,
            fragment_bit_end,
        ))
    }

    fn from_reader_continuation(source: LiveObjectUpdateReaderContinuationSource) -> Self {
        match source {
            LiveObjectUpdateReaderContinuationSource::None => Self::FullyConsumedByTypedReaders,
            LiveObjectUpdateReaderContinuationSource::ReadBufferOnly => Self::ReadBufferUnconsumed,
            LiveObjectUpdateReaderContinuationSource::FragmentOnly => {
                Self::FragmentWriterOwnerUnproven
            }
            LiveObjectUpdateReaderContinuationSource::ReadBufferAndFragment => {
                Self::ReadBufferUnconsumedAndFragmentWriterOwnerUnproven
            }
        }
    }

    pub fn allows_exact_claim(self) -> bool {
        self == Self::FullyConsumedByTypedReaders
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FullyConsumedByTypedReaders => "fully-consumed-by-typed-readers",
            Self::ReadBufferUnconsumed => "read-buffer-unconsumed",
            Self::FragmentWriterOwnerUnproven => "fragment-writer-owner-unproven",
            Self::ReadBufferUnconsumedAndFragmentWriterOwnerUnproven => {
                "read-buffer-unconsumed-and-fragment-writer-owner-unproven"
            }
            Self::TypedCursorOverrun => "typed-cursor-overrun",
        }
    }
}

impl LiveObjectUpdateReaderContinuationSource {
    fn from_cursors(
        read_buffer_cursor: usize,
        read_buffer_end: usize,
        fragment_bit_cursor: usize,
        fragment_bit_end: usize,
    ) -> Self {
        match (
            read_buffer_cursor < read_buffer_end,
            fragment_bit_cursor < fragment_bit_end,
        ) {
            (false, false) => Self::None,
            (true, false) => Self::ReadBufferOnly,
            (false, true) => Self::FragmentOnly,
            (true, true) => Self::ReadBufferAndFragment,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ReadBufferOnly => "read-buffer-only",
            Self::FragmentOnly => "fragment-only",
            Self::ReadBufferAndFragment => "read-buffer-and-fragment",
        }
    }

    fn has_more_data(self) -> bool {
        self != Self::None
    }
}

/// Exact terminal reader state for the immutable Diamond source and the
/// staged EE output candidate.
///
/// This is failure evidence only. In particular, `FragmentOnly` means the
/// original client reader will attempt another row opcode and overflow the
/// exhausted read buffer; it is not permission to ignore or erase those bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateTerminalReaderContinuationEvidence {
    pub source_read_buffer_cursor: usize,
    pub source_read_buffer_end: usize,
    pub source_fragment_bit_cursor: usize,
    pub source_fragment_bit_end: usize,
    pub source_more_data_source: LiveObjectUpdateReaderContinuationSource,
    pub source_next_opcode_read_overflows: bool,
    pub emitted_read_buffer_cursor: usize,
    pub emitted_read_buffer_end: usize,
    pub emitted_fragment_bit_cursor: usize,
    pub emitted_fragment_bit_end: usize,
    pub emitted_more_data_source: LiveObjectUpdateReaderContinuationSource,
    pub emitted_next_opcode_read_overflows: bool,
}

impl LiveObjectUpdateTerminalReaderContinuationEvidence {
    pub(super) fn from_terminal_cursors(
        source_read_buffer_cursor: usize,
        source_read_buffer_end: usize,
        source_fragment_bit_cursor: usize,
        source_fragment_bit_end: usize,
        emitted_read_buffer_cursor: usize,
        emitted_read_buffer_end: usize,
        emitted_fragment_bit_cursor: usize,
        emitted_fragment_bit_end: usize,
    ) -> Self {
        let source_more_data_source = LiveObjectUpdateReaderContinuationSource::from_cursors(
            source_read_buffer_cursor,
            source_read_buffer_end,
            source_fragment_bit_cursor,
            source_fragment_bit_end,
        );
        let emitted_more_data_source = LiveObjectUpdateReaderContinuationSource::from_cursors(
            emitted_read_buffer_cursor,
            emitted_read_buffer_end,
            emitted_fragment_bit_cursor,
            emitted_fragment_bit_end,
        );
        Self {
            source_read_buffer_cursor,
            source_read_buffer_end,
            source_fragment_bit_cursor,
            source_fragment_bit_end,
            source_more_data_source,
            source_next_opcode_read_overflows: source_more_data_source.has_more_data()
                && source_read_buffer_cursor >= source_read_buffer_end,
            emitted_read_buffer_cursor,
            emitted_read_buffer_end,
            emitted_fragment_bit_cursor,
            emitted_fragment_bit_end,
            emitted_more_data_source,
            emitted_next_opcode_read_overflows: emitted_more_data_source.has_more_data()
                && emitted_read_buffer_cursor >= emitted_read_buffer_end,
        }
    }

    pub fn source_fragment_ownership_verdict(
        self,
    ) -> LiveObjectUpdateTerminalFragmentOwnershipVerdict {
        LiveObjectUpdateTerminalFragmentOwnershipVerdict::from_cursors(
            self.source_read_buffer_cursor,
            self.source_read_buffer_end,
            self.source_fragment_bit_cursor,
            self.source_fragment_bit_end,
        )
    }

    pub fn emitted_fragment_ownership_verdict(
        self,
    ) -> LiveObjectUpdateTerminalFragmentOwnershipVerdict {
        LiveObjectUpdateTerminalFragmentOwnershipVerdict::from_cursors(
            self.emitted_read_buffer_cursor,
            self.emitted_read_buffer_end,
            self.emitted_fragment_bit_cursor,
            self.emitted_fragment_bit_end,
        )
    }
}

pub const LIVE_OBJECT_UPDATE_TERMINAL_FRAGMENT_REPLAY_CANDIDATE_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateTerminalFragmentHandoffCorrelationEvidence {
    pub anchored_source_bit_cursor: usize,
    pub source_fragment_bit_count: usize,
    pub unresolved_source_bits: LiveObjectUpdateRewriteBitSliceEvidence,
    pub candidate_count: usize,
    pub candidates_retained: usize,
    pub ambiguity_count: usize,
    pub candidates: [Option<LiveObjectUpdateTerminalFragmentReplayCandidateEvidence>;
        LIVE_OBJECT_UPDATE_TERMINAL_FRAGMENT_REPLAY_CANDIDATE_LIMIT],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateTerminalFragmentReplayCandidateEvidence {
    pub prior_offset: usize,
    pub prior_record_end: usize,
    pub prior_opcode: u8,
    pub prior_marker: u8,
    pub prior_object_id: Option<u32>,
    pub focus_offset: usize,
    pub focus_object_id: Option<u32>,
    pub same_object: bool,
    pub immediately_precedes_focus: bool,
    pub prior_source_bit_start: usize,
    pub prior_source_bit_end: usize,
    pub prior_source_bit_count: usize,
    pub unresolved_prefix: LiveObjectUpdateRewriteBitSliceEvidence,
    pub replayed_source_bits: LiveObjectUpdateRewriteBitSliceEvidence,
    pub unresolved_suffix: LiveObjectUpdateRewriteBitSliceEvidence,
    pub direct_name_placeable_add_replay:
        Option<LiveObjectUpdateTerminalDirectNamePlaceableAddReplayEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateTerminalDirectNamePlaceableAddReplayEvidence {
    pub prior_emitted_bit_start: usize,
    pub prior_emitted_bit_end: usize,
    pub prior_emitted_bit_count: usize,
    pub prior_bits_inserted: usize,
    pub prior_bits_removed: usize,
    pub source_name_selector_bit_cursor: usize,
    pub emitted_name_selector_bit_cursor: usize,
    pub emitted_post_name_bit_cursor: usize,
    pub emitted_next_bit_cursor: usize,
    pub emitted_bits: LiveObjectUpdateRewriteBitSliceEvidence,
}

pub const LIVE_OBJECT_UPDATE_DOOR_PLACEABLE_FRAGMENT_FIELD_LIMIT: usize = 10;

/// A decompile-owned fragment field from one exact Diamond door/placeable
/// reader walk.  These spans describe reader semantics only; they do not prove
/// which server writer emitted a terminal duplicate-looking span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveObjectUpdateDoorPlaceableFragmentFieldKind {
    PositionZLow,
    OrientationSelector,
    ScalarOrientationLow,
    StateVisualSelector,
    StateVisualActive,
    StateLocked,
    StateLockable,
    StateVisualPayload,
    NameSelector,
    NameLocStringSelector,
}

impl LiveObjectUpdateDoorPlaceableFragmentFieldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PositionZLow => "position-z-low",
            Self::OrientationSelector => "orientation-selector",
            Self::ScalarOrientationLow => "scalar-orientation-low",
            Self::StateVisualSelector => "state-visual-selector",
            Self::StateVisualActive => "state-visual-active",
            Self::StateLocked => "state-locked",
            Self::StateLockable => "state-lockable",
            Self::StateVisualPayload => "state-visual-payload",
            Self::NameSelector => "name-selector",
            Self::NameLocStringSelector => "name-locstring-selector",
        }
    }
}

/// A bounded fragment span packed in MSB-first reader order. Terminal failure
/// evidence is copied through retry paths, so retaining at most 32 exact bits
/// here avoids embedding a large bit-slice array for every individual field.
/// Stock Diamond reader admission remains separately bounded to 16 bits; the
/// wider capacity exists for the exact 17-bit staged EE final-claim obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdatePackedFragmentBitSpanEvidence {
    pub bit_start: usize,
    pub bit_end: usize,
    pub packed_msb: u32,
}

impl LiveObjectUpdatePackedFragmentBitSpanEvidence {
    pub fn bit_count(self) -> usize {
        self.bit_end.saturating_sub(self.bit_start)
    }

    pub fn is_valid(self) -> bool {
        let Some(bit_count) = self.bit_end.checked_sub(self.bit_start) else {
            return false;
        };
        if bit_count > u32::BITS as usize {
            return false;
        }
        bit_count == u32::BITS as usize || self.packed_msb < (1u32 << bit_count)
    }

    pub fn bit(self, bit_cursor: usize) -> Option<bool> {
        if !self.is_valid() {
            return None;
        }
        if bit_cursor < self.bit_start || bit_cursor >= self.bit_end {
            return None;
        }
        let bit_count = self.bit_end.checked_sub(self.bit_start)?;
        let bit_offset = bit_cursor.checked_sub(self.bit_start)?;
        let shift = bit_count.checked_sub(bit_offset.saturating_add(1))?;
        Some(((self.packed_msb >> shift) & 1) != 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateDoorPlaceableStockSourceEvidence {
    pub raw_mask: u32,
    pub effective_mask: u32,
    pub ignored_mask: u32,
    pub read_end: usize,
    pub source_bit_cursor: usize,
    pub source_reader_bit_cursor: usize,
    pub source_reader_bits_consumed: usize,
    pub source_reader_bits: LiveObjectUpdatePackedFragmentBitSpanEvidence,
    pub source_orientation_vector: Option<bool>,
    pub source_state_bit_cursor: Option<usize>,
    pub source_name_selector_bit_cursor: Option<usize>,
    pub source_name_selector: Option<bool>,
    pub source_name_locstring_selector_bit_cursor: Option<usize>,
    pub source_name_locstring_selector: Option<bool>,
    pub source_name_kind: Option<&'static str>,
    pub source_reader_residual: LiveObjectUpdateRewriteBitSliceEvidence,
}

pub const LIVE_OBJECT_UPDATE_END_ALIGNED_DIAMOND_READER_CANDIDATE_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateDoorPlaceableEndAlignedDiamondReaderCandidateEvidence {
    pub raw_mask: u32,
    pub effective_mask: u32,
    pub ignored_mask: u32,
    pub read_end: usize,
    pub source_bit_cursor: usize,
    pub source_reader_bit_cursor: usize,
    pub source_reader_bits_consumed: usize,
    pub source_orientation_vector: Option<bool>,
    pub source_state_bit_cursor: Option<usize>,
    pub source_name_selector_bit_cursor: Option<usize>,
    pub source_name_selector: Option<bool>,
    pub source_name_locstring_selector_bit_cursor: Option<usize>,
    pub source_name_locstring_selector: Option<bool>,
    pub source_name_kind: Option<&'static str>,
    pub source_gap_from_ledger_cursor: LiveObjectUpdateRewriteBitSliceEvidence,
    pub source_gap_from_anchored_reader: Option<LiveObjectUpdateRewriteBitSliceEvidence>,
    pub source_bits: LiveObjectUpdateRewriteBitSliceEvidence,
}

pub const LIVE_OBJECT_UPDATE_TAIL9_SOURCE_CANDIDATE_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdateDoorPlaceableTail9SourceCandidateEvidence {
    pub source_bit_cursor: usize,
    pub source_reader_bit_cursor: usize,
    pub source_reader_bits_consumed: usize,
    pub source_name_selector_bit_cursor: Option<usize>,
    pub source_name_selector: Option<bool>,
    pub source_name_locstring_selector_bit_cursor: Option<usize>,
    pub source_name_locstring_selector: Option<bool>,
    pub source_name_kind: Option<&'static str>,
    pub source_gap_from_selected_reader: LiveObjectUpdateRewriteBitSliceEvidence,
    pub source_bits: LiveObjectUpdateRewriteBitSliceEvidence,
}

#[cfg(test)]
mod tests {
    use super::{
        LiveObjectUpdatePackedFragmentBitSpanEvidence as PackedSpan,
        LiveObjectUpdateTerminalEeFinalClaimObservation as EeFinalClaimObservation,
        LiveObjectUpdateTerminalEeFinalClaimReadinessVerdict as EeFinalClaimVerdict,
        LiveObjectUpdateTerminalFragmentOwnershipVerdict as Verdict,
        LiveObjectUpdateTerminalWriterHandoffRequirement as WriterRequirement,
    };
    use crate::translate::live_object_update::{
        LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT, LiveObjectUpdateRewriteBitSliceEvidence,
    };

    fn bit_slice(bit_start: usize, values: &[bool]) -> LiveObjectUpdateRewriteBitSliceEvidence {
        let mut bits = [None; LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT];
        for (slot, value) in bits.iter_mut().zip(values.iter().copied()) {
            *slot = Some(value);
        }
        LiveObjectUpdateRewriteBitSliceEvidence {
            bit_start,
            bit_end: bit_start.saturating_add(values.len()),
            bit_count: values.len(),
            bits_retained: values
                .len()
                .min(LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT),
            bits,
        }
    }

    fn packed_span(bit_start: usize, values: &[bool]) -> PackedSpan {
        assert!(values.len() <= u32::BITS as usize);
        let mut packed_msb = 0u32;
        for value in values {
            packed_msb = (packed_msb << 1) | u32::from(*value);
        }
        PackedSpan {
            bit_start,
            bit_end: bit_start + values.len(),
            packed_msb,
        }
    }

    fn writer_requirement() -> WriterRequirement {
        WriterRequirement {
            object_type: 9,
            object_id: 0x8000_1003,
            raw_mask: 0xFFFF_FFF7,
            source_read_buffer_cursor: 245,
            source_read_buffer_end: 245,
            source_fragment_bits: bit_slice(
                63,
                &[
                    false, false, false, false, false, false, true, false, false, false, true,
                    true, false,
                ],
            ),
            source_next_opcode_read_overflows: true,
            emitted_read_buffer_cursor: 243,
            emitted_read_buffer_end: 243,
            emitted_fragment_bit_start: 71,
            emitted_fragment_bit_end: 88,
            emitted_fragment_bit_count: 17,
            emitted_fragment_bits_retained: 17,
            emitted_fragment_bits: packed_span(
                71,
                &[
                    false, false, true, false, false, false, false, false, false, false, true,
                    false, false, false, true, true, false,
                ],
            ),
            emitted_next_opcode_read_overflows: true,
        }
    }

    fn ee_final_claim_observation(requirement: WriterRequirement) -> EeFinalClaimObservation {
        EeFinalClaimObservation {
            read_buffer_cursor: requirement.emitted_read_buffer_cursor,
            read_buffer_end: requirement.emitted_read_buffer_end,
            fragment_bits_written: requirement.emitted_fragment_bits,
            final_fragment_bit_cursor: requirement.emitted_fragment_bit_end,
            final_fragment_bit_end: requirement.emitted_fragment_bit_end,
            exact_payload_validator_accepted: true,
        }
    }

    #[test]
    fn terminal_fragment_ownership_requires_all_typed_cursors_to_finish() {
        let cases = [
            ((12, 12, 27, 27), Verdict::FullyConsumedByTypedReaders),
            ((11, 12, 27, 27), Verdict::ReadBufferUnconsumed),
            ((12, 12, 26, 27), Verdict::FragmentWriterOwnerUnproven),
            (
                (11, 12, 26, 27),
                Verdict::ReadBufferUnconsumedAndFragmentWriterOwnerUnproven,
            ),
            ((13, 12, 27, 27), Verdict::TypedCursorOverrun),
            ((12, 12, 28, 27), Verdict::TypedCursorOverrun),
            ((13, 12, 28, 27), Verdict::TypedCursorOverrun),
        ];

        for ((read_cursor, read_end, fragment_cursor, fragment_end), expected) in cases {
            let verdict =
                Verdict::from_cursors(read_cursor, read_end, fragment_cursor, fragment_end);
            assert_eq!(verdict, expected);
            assert_eq!(
                verdict.allows_exact_claim(),
                expected == Verdict::FullyConsumedByTypedReaders
            );
        }
    }

    #[test]
    fn terminal_ee_final_claim_readiness_requires_exact_bits_and_cursors() {
        let requirement = writer_requirement();
        let exact = ee_final_claim_observation(requirement);
        let verdict = requirement.correlate_ee_final_claim(Some(exact));
        assert_eq!(verdict, EeFinalClaimVerdict::ExactTypedEeFinalClaimReady);
        assert!(verdict.final_claim_ready());
        assert!(!verdict.allows_exact_claim());
        assert!(!verdict.authorizes_rewrite());
        assert!(!verdict.authorizes_cursor_advance());
        assert!(!verdict.authorizes_fragment_trim());

        assert_eq!(
            requirement.correlate_ee_final_claim(None),
            EeFinalClaimVerdict::IncompleteTypedEeWriter
        );
        assert_eq!(
            requirement.correlate_ee_final_claim(Some(EeFinalClaimObservation {
                exact_payload_validator_accepted: false,
                ..exact
            })),
            EeFinalClaimVerdict::IncompleteTypedEeWriter
        );
        assert_eq!(
            requirement.correlate_ee_final_claim(Some(EeFinalClaimObservation {
                read_buffer_cursor: 245,
                read_buffer_end: 245,
                ..exact
            })),
            EeFinalClaimVerdict::ReadBufferMismatch
        );

        let mut changed_bits = exact.fragment_bits_written;
        changed_bits.packed_msb ^= 1 << 7;
        assert_eq!(
            requirement.correlate_ee_final_claim(Some(EeFinalClaimObservation {
                fragment_bits_written: changed_bits,
                ..exact
            })),
            EeFinalClaimVerdict::BitMismatch
        );

        assert_eq!(
            requirement.correlate_ee_final_claim(Some(EeFinalClaimObservation {
                fragment_bits_written: PackedSpan {
                    bit_end: exact.fragment_bits_written.bit_end - 1,
                    ..exact.fragment_bits_written
                },
                final_fragment_bit_cursor: exact.final_fragment_bit_cursor - 1,
                ..exact
            })),
            EeFinalClaimVerdict::CursorGapOrOverlap
        );

        assert_eq!(
            requirement.correlate_ee_final_claim(Some(EeFinalClaimObservation {
                final_fragment_bit_cursor: exact.final_fragment_bit_end + 1,
                ..exact
            })),
            EeFinalClaimVerdict::TypedCursorOverrun
        );

        let incomplete = requirement.correlate_ee_final_claim(Some(EeFinalClaimObservation {
            fragment_bits_written: PackedSpan {
                bit_start: 71,
                bit_end: 104,
                packed_msb: 0,
            },
            ..exact
        }));
        assert_eq!(incomplete, EeFinalClaimVerdict::IncompleteTypedEeWriter);
        assert!(!incomplete.final_claim_ready());

        assert_eq!(
            WriterRequirement {
                emitted_fragment_bits_retained: 16,
                ..requirement
            }
            .correlate_ee_final_claim(None),
            EeFinalClaimVerdict::InvalidEmittedRequirement
        );
        assert_eq!(
            WriterRequirement {
                emitted_fragment_bits: PackedSpan {
                    bit_start: 88,
                    bit_end: 71,
                    packed_msb: 0,
                },
                ..requirement
            }
            .correlate_ee_final_claim(None),
            EeFinalClaimVerdict::InvalidEmittedRequirement
        );
    }

    #[test]
    fn packed_fragment_span_rejects_more_than_u32_bits() {
        let malformed = PackedSpan {
            bit_start: 4,
            bit_end: 37,
            packed_msb: 0,
        };
        assert_eq!(malformed.bit(4), None);
        assert_eq!(malformed.bit(36), None);

        let reversed = PackedSpan {
            bit_start: 37,
            bit_end: 4,
            packed_msb: 0,
        };
        assert!(!reversed.is_valid());
        assert_eq!(reversed.bit(4), None);

        let noncanonical = PackedSpan {
            bit_start: 4,
            bit_end: 8,
            packed_msb: 0x10,
        };
        assert!(!noncanonical.is_valid());
        assert_eq!(noncanonical.bit(4), None);
    }
}
