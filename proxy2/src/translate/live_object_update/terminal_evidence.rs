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
        read_buffer_cursor: usize,
        read_buffer_end: usize,
        source_fragment_bit_cursor: usize,
        source_fragment_bit_end: usize,
        emitted_fragment_bit_cursor: usize,
        emitted_fragment_bit_end: usize,
    ) -> Self {
        let source_more_data_source = LiveObjectUpdateReaderContinuationSource::from_cursors(
            read_buffer_cursor,
            read_buffer_end,
            source_fragment_bit_cursor,
            source_fragment_bit_end,
        );
        let emitted_more_data_source = LiveObjectUpdateReaderContinuationSource::from_cursors(
            read_buffer_cursor,
            read_buffer_end,
            emitted_fragment_bit_cursor,
            emitted_fragment_bit_end,
        );
        Self {
            source_read_buffer_cursor: read_buffer_cursor,
            source_read_buffer_end: read_buffer_end,
            source_fragment_bit_cursor,
            source_fragment_bit_end,
            source_more_data_source,
            source_next_opcode_read_overflows: source_more_data_source.has_more_data()
                && read_buffer_cursor >= read_buffer_end,
            emitted_read_buffer_cursor: read_buffer_cursor,
            emitted_read_buffer_end: read_buffer_end,
            emitted_fragment_bit_cursor,
            emitted_fragment_bit_end,
            emitted_more_data_source,
            emitted_next_opcode_read_overflows: emitted_more_data_source.has_more_data()
                && read_buffer_cursor >= read_buffer_end,
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

/// A bounded source-reader span packed in reader order. Terminal failure
/// evidence is copied through retry paths, so retaining one 16-bit walk avoids
/// embedding a large bit-slice array for every individual field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveObjectUpdatePackedFragmentBitSpanEvidence {
    pub bit_start: usize,
    pub bit_end: usize,
    pub packed_msb: u16,
}

impl LiveObjectUpdatePackedFragmentBitSpanEvidence {
    pub fn bit(self, bit_cursor: usize) -> Option<bool> {
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
    use super::LiveObjectUpdateTerminalFragmentOwnershipVerdict as Verdict;

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
}
