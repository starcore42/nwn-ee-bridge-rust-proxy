//! Bounded ingestion of terminal server-writer trace evidence.
//!
//! A writer trace is diagnostic proof, not a live rewrite input. Exact proof
//! is constructed only here from one ordered owner/list/finalizer trace and
//! one complete finalized `P/05/01` payload. A configured journal may contain
//! multiple independently sealed traces. Correlation first requires one unique
//! full-payload selection, then applies the existing exact-requirement proof.
//! The factory validates the CNW envelope, derives record identity and terminal
//! MSB-first bits from those finalized bytes, and compares the entire payload
//! with quarantine.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::{
    LIVE_OBJECT_UPDATE_REWRITE_BIT_PREVIEW_LIMIT, LiveObjectUpdateRewriteBitSliceEvidence,
    LiveObjectUpdateTerminalWriterHandoffRequirement, LiveObjectUpdateTerminalWriterHandoffVerdict,
    MAX_REASONABLE_LIVE_PAYLOAD_BYTES, bits::msb_valid_bit_count,
};

const LIVE_OBJECT_CNW_WRITER_HEADER_BYTES: usize = 7;
const LIVE_OBJECT_UPDATE_HEADER_BYTES: usize = 10;
const CNW_FRAGMENT_HEADER_BITS: usize = 3;
const TERMINAL_WRITER_TRACE_ARTIFACT_VERSION: &str = "2";
const TERMINAL_WRITER_TRACE_ARTIFACT_OVERHEAD_BYTES: usize = 4 * 1024;
const MAX_TERMINAL_WRITER_TRACE_ARTIFACT_BYTES: usize =
    MAX_REASONABLE_LIVE_PAYLOAD_BYTES * 2 + TERMINAL_WRITER_TRACE_ARTIFACT_OVERHEAD_BYTES;
const MAX_TERMINAL_WRITER_TRACE_JOURNAL_ARTIFACTS: usize = 64;
const MAX_TERMINAL_WRITER_TRACE_JOURNAL_BYTES: usize = MAX_TERMINAL_WRITER_TRACE_ARTIFACT_BYTES * 8;

static TERMINAL_WRITER_TRACE_JOURNAL: OnceLock<OwnedTerminalWriterTraceJournal> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalWriterTraceArtifactStatus {
    NotConfigured,
    ReadFailed,
    TooLarge,
    InvalidUtf8,
    InvalidFormat,
    Loaded,
}

impl TerminalWriterTraceArtifactStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not-configured",
            Self::ReadFailed => "read-failed",
            Self::TooLarge => "too-large",
            Self::InvalidUtf8 => "invalid-utf8",
            Self::InvalidFormat => "invalid-format",
            Self::Loaded => "loaded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalWriterTraceSelectionStatus {
    NotConfigured,
    NoPayloadMatch,
    UniquePayloadMatch,
    AmbiguousPayloadMatch,
}

impl TerminalWriterTraceSelectionStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not-configured",
            Self::NoPayloadMatch => "no-payload-match",
            Self::UniquePayloadMatch => "unique-payload-match",
            Self::AmbiguousPayloadMatch => "ambiguous-payload-match",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TerminalWriterTraceCorrelation {
    pub(super) artifact_status: TerminalWriterTraceArtifactStatus,
    pub(super) selection_status: TerminalWriterTraceSelectionStatus,
    pub(super) artifact_count: usize,
    pub(super) payload_match_count: usize,
    pub(super) verdict: LiveObjectUpdateTerminalWriterHandoffVerdict,
    pub(super) trace_id: Option<u64>,
    pub(super) message_id: Option<u64>,
    pub(super) component_sha256: Option<[u8; 32]>,
    pub(super) cursor_evidence: Option<TerminalWriterTraceCursorEvidence>,
    pub(super) exact_handoff: Option<ExactTerminalWriterHandoff>,
}

/// Absolute writer cursors retained for diagnostics after a unique artifact
/// selection. The deltas distinguish work performed by the typed owner from
/// the post-owner list/finalizer handoff without granting either phase wire
/// authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TerminalWriterTraceCursorEvidence {
    pub(super) owner_begin_read_cursor: usize,
    pub(super) owner_end_read_cursor: usize,
    pub(super) list_handoff_read_cursor: usize,
    pub(super) final_read_end: usize,
    pub(super) owner_begin_fragment_cursor: usize,
    pub(super) owner_end_fragment_cursor: usize,
    pub(super) list_handoff_fragment_cursor: usize,
    pub(super) final_fragment_cursor: usize,
}

impl TerminalWriterTraceCursorEvidence {
    pub(super) fn post_owner_read_bytes(self) -> Option<usize> {
        self.list_handoff_read_cursor
            .checked_sub(self.owner_end_read_cursor)
    }

    pub(super) fn post_owner_fragment_bits(self) -> Option<usize> {
        self.list_handoff_fragment_cursor
            .checked_sub(self.owner_end_fragment_cursor)
    }
}

/// Opaque proof that one ordered operator trace matched the complete source
/// packet and the exact terminal writer-handoff requirement. Sibling modules
/// may carry or compare this token, but only this module can construct one.
/// It remains source-side evidence and cannot authorize an EE rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExactTerminalWriterHandoff {
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    identity: TerminalWriterTraceIdentity,
    source_payload: Box<[u8]>,
}

impl ExactTerminalWriterHandoff {
    pub(super) fn matches(
        &self,
        requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
        source_payload: &[u8],
    ) -> bool {
        requirement.source_contract_is_valid()
            && self.requirement == requirement
            && self.identity.trace_id != 0
            && self.source_payload.as_ref() == source_payload
    }
}

#[cfg(test)]
pub(super) fn exact_terminal_writer_handoff_for_test(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    source_payload: &[u8],
) -> ExactTerminalWriterHandoff {
    ExactTerminalWriterHandoff {
        requirement,
        identity: TerminalWriterTraceIdentity {
            trace_id: 1,
            message_id: 1,
            component_sha256: [0xA5; 32],
        },
        source_payload: source_payload.into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalWriterTraceIdentity {
    trace_id: u64,
    message_id: u64,
    component_sha256: [u8; 32],
}

#[derive(Debug)]
struct OwnedTerminalWriterTraceJournal {
    artifacts: Vec<OwnedTerminalWriterTraceArtifact>,
}

#[derive(Debug)]
struct OwnedTerminalWriterTraceArtifact {
    identity: TerminalWriterTraceIdentity,
    absolute_record_offset: usize,
    owner_begin_read_cursor: usize,
    owner_begin_fragment_cursor: usize,
    owner_end_read_cursor: usize,
    owner_end_fragment_cursor: usize,
    list_handoff_read_cursor: usize,
    list_handoff_fragment_cursor: usize,
    final_read_end: usize,
    final_fragment_cursor: usize,
    finalized_payload: Vec<u8>,
}

impl OwnedTerminalWriterTraceArtifact {
    fn as_trace(&self) -> TerminalWriterTrace<'_> {
        TerminalWriterTrace {
            events: [
                TerminalWriterTraceEvent::OwnerBegin {
                    identity: self.identity,
                    absolute_record_offset: self.absolute_record_offset,
                    absolute_read_buffer_cursor: self.owner_begin_read_cursor,
                    fragment_bit_cursor: self.owner_begin_fragment_cursor,
                },
                TerminalWriterTraceEvent::OwnerEnd {
                    identity: self.identity,
                    absolute_read_buffer_cursor: self.owner_end_read_cursor,
                    fragment_bit_cursor: self.owner_end_fragment_cursor,
                },
                TerminalWriterTraceEvent::ListHandoff {
                    identity: self.identity,
                    absolute_read_buffer_cursor: self.list_handoff_read_cursor,
                    fragment_bit_cursor: self.list_handoff_fragment_cursor,
                },
                TerminalWriterTraceEvent::Finalize {
                    identity: self.identity,
                    absolute_read_buffer_end: self.final_read_end,
                    fragment_bit_cursor: self.final_fragment_cursor,
                    packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(
                        &self.finalized_payload,
                    ),
                },
            ],
        }
    }
}

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
        identity: TerminalWriterTraceIdentity,
        absolute_record_offset: usize,
        absolute_read_buffer_cursor: usize,
        fragment_bit_cursor: usize,
    },
    OwnerEnd {
        identity: TerminalWriterTraceIdentity,
        absolute_read_buffer_cursor: usize,
        fragment_bit_cursor: usize,
    },
    ListHandoff {
        identity: TerminalWriterTraceIdentity,
        absolute_read_buffer_cursor: usize,
        fragment_bit_cursor: usize,
    },
    Finalize {
        identity: TerminalWriterTraceIdentity,
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
    absolute_record_offset: usize,
    object_type: u8,
    object_id: u32,
    raw_mask: u32,
    handoff_read_buffer_cursor: usize,
    final_read_buffer_end: usize,
    post_owner_fragment_bits: LiveObjectUpdateRewriteBitSliceEvidence,
    finalized_fragment_bit_cursor: usize,
    packet_correlation: TerminalWriterPacketCorrelation,
    cursor_evidence: TerminalWriterTraceCursorEvidence,
}

#[derive(Debug, Clone, Copy)]
struct ValidatedPacketLayout<'a> {
    payload: &'a [u8],
    declared: usize,
    fragment: &'a [u8],
    fragment_valid_bits: usize,
}

/// Configure and eagerly validate the private operator journal that terminal
/// diagnostics may ingest. Eager loading prevents a partial or transient
/// first diagnostic read from becoming the process-wide result.
pub(crate) fn configure_terminal_writer_trace_path(path: PathBuf) -> Result<(), &'static str> {
    if TERMINAL_WRITER_TRACE_JOURNAL.get().is_some() {
        return Err("already-configured");
    }
    let journal = load_terminal_writer_trace_journal(&path).map_err(|status| status.as_str())?;
    TERMINAL_WRITER_TRACE_JOURNAL
        .set(journal)
        .map_err(|_| "already-configured")
}

pub(crate) fn terminal_writer_trace_configured() -> bool {
    TERMINAL_WRITER_TRACE_JOURNAL.get().is_some()
}

/// Correlate the configured operator journal inside this sealed module.
/// Loading is bounded and happens at most once. A source token is minted only
/// after a unique complete-payload selection also reaches exact requirement
/// correlation. Even that result remains diagnostic:
/// `ExactObservedHandoff::allows_exact_claim()` is false.
pub(super) fn correlate_terminal_writer_trace(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    quarantined_payload: &[u8],
) -> TerminalWriterTraceCorrelation {
    let Some(journal) = TERMINAL_WRITER_TRACE_JOURNAL.get() else {
        return TerminalWriterTraceCorrelation {
            artifact_status: TerminalWriterTraceArtifactStatus::NotConfigured,
            selection_status: TerminalWriterTraceSelectionStatus::NotConfigured,
            artifact_count: 0,
            payload_match_count: 0,
            verdict: LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace,
            trace_id: None,
            message_id: None,
            component_sha256: None,
            cursor_evidence: None,
            exact_handoff: None,
        };
    };
    correlate_loaded_terminal_writer_trace_journal(requirement, quarantined_payload, journal)
}

fn correlate_loaded_terminal_writer_trace_journal(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    quarantined_payload: &[u8],
    journal: &OwnedTerminalWriterTraceJournal,
) -> TerminalWriterTraceCorrelation {
    let artifact_count = journal.artifacts.len();
    let payload_matches: Vec<_> = journal
        .artifacts
        .iter()
        .filter(|artifact| artifact.finalized_payload == quarantined_payload)
        .collect();
    let payload_match_count = payload_matches.len();
    if payload_matches.is_empty() {
        return TerminalWriterTraceCorrelation {
            artifact_status: TerminalWriterTraceArtifactStatus::Loaded,
            selection_status: TerminalWriterTraceSelectionStatus::NoPayloadMatch,
            artifact_count,
            payload_match_count,
            verdict: if requirement.source_contract_is_valid() {
                LiveObjectUpdateTerminalWriterHandoffVerdict::PacketMismatch
            } else {
                LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace
            },
            trace_id: None,
            message_id: None,
            component_sha256: None,
            cursor_evidence: None,
            exact_handoff: None,
        };
    }
    if payload_match_count > 1 {
        return TerminalWriterTraceCorrelation {
            artifact_status: TerminalWriterTraceArtifactStatus::Loaded,
            selection_status: TerminalWriterTraceSelectionStatus::AmbiguousPayloadMatch,
            artifact_count,
            payload_match_count,
            verdict: LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace,
            trace_id: None,
            message_id: None,
            component_sha256: None,
            cursor_evidence: None,
            exact_handoff: None,
        };
    }

    let mut correlation = correlate_loaded_terminal_writer_trace(
        requirement,
        quarantined_payload,
        payload_matches[0],
    );
    correlation.artifact_count = artifact_count;
    correlation.payload_match_count = payload_match_count;
    correlation
}

fn correlate_loaded_terminal_writer_trace(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    quarantined_payload: &[u8],
    artifact: &OwnedTerminalWriterTraceArtifact,
) -> TerminalWriterTraceCorrelation {
    let observation = build_terminal_writer_observation(quarantined_payload, artifact.as_trace());
    let verdict = correlate_optional_observation(requirement, observation);
    let exact_handoff = (verdict
        == LiveObjectUpdateTerminalWriterHandoffVerdict::ExactObservedHandoff)
        .then(|| ExactTerminalWriterHandoff {
            requirement,
            identity: artifact.identity,
            source_payload: quarantined_payload.into(),
        });
    TerminalWriterTraceCorrelation {
        artifact_status: TerminalWriterTraceArtifactStatus::Loaded,
        selection_status: TerminalWriterTraceSelectionStatus::UniquePayloadMatch,
        artifact_count: 1,
        payload_match_count: 1,
        verdict,
        trace_id: Some(artifact.identity.trace_id),
        message_id: Some(artifact.identity.message_id),
        component_sha256: Some(artifact.identity.component_sha256),
        cursor_evidence: observation.map(|observation| observation.cursor_evidence),
        exact_handoff,
    }
}

fn load_terminal_writer_trace_journal(
    path: &Path,
) -> Result<OwnedTerminalWriterTraceJournal, TerminalWriterTraceArtifactStatus> {
    let file = File::open(path).map_err(|_| TerminalWriterTraceArtifactStatus::ReadFailed)?;
    // Inspect and read the same open handle. `take(MAX + 1)` bounds allocation
    // even if a producer grows or replaces the path between metadata and read.
    let metadata = file
        .metadata()
        .map_err(|_| TerminalWriterTraceArtifactStatus::ReadFailed)?;
    let journal_len =
        usize::try_from(metadata.len()).map_err(|_| TerminalWriterTraceArtifactStatus::TooLarge)?;
    if journal_len > MAX_TERMINAL_WRITER_TRACE_JOURNAL_BYTES {
        return Err(TerminalWriterTraceArtifactStatus::TooLarge);
    }
    let read_limit = u64::try_from(MAX_TERMINAL_WRITER_TRACE_JOURNAL_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(journal_len.min(MAX_TERMINAL_WRITER_TRACE_JOURNAL_BYTES));
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|_| TerminalWriterTraceArtifactStatus::ReadFailed)?;
    if bytes.len() > MAX_TERMINAL_WRITER_TRACE_JOURNAL_BYTES {
        return Err(TerminalWriterTraceArtifactStatus::TooLarge);
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| TerminalWriterTraceArtifactStatus::InvalidUtf8)?;
    parse_terminal_writer_trace_journal(text)
        .ok_or(TerminalWriterTraceArtifactStatus::InvalidFormat)
}

/// Parse one or more consecutive private v2 trace blocks. Every six-line block
/// remains independently sealed: each ordered event repeats the trace id,
/// message id, and component fingerprint so separately captured rows cannot be
/// spliced into one observation. Cursor coordinates are absolute writer byte
/// offsets and full MSB-first fragment offsets. Partial blocks, separators,
/// duplicate identities, and journals beyond the entry cap fail closed.
fn parse_terminal_writer_trace_journal(text: &str) -> Option<OwnedTerminalWriterTraceJournal> {
    let mut lines = text.lines();
    let mut artifacts = Vec::new();
    while let Some(header_line) = lines.next() {
        if artifacts.len() >= MAX_TERMINAL_WRITER_TRACE_JOURNAL_ARTIFACTS {
            return None;
        }
        let block = [
            header_line,
            lines.next()?,
            lines.next()?,
            lines.next()?,
            lines.next()?,
            lines.next()?,
        ];
        let artifact = parse_terminal_writer_trace_artifact_lines(block)?;
        if !terminal_writer_trace_artifact_is_structurally_valid(&artifact) {
            return None;
        }
        if artifacts
            .iter()
            .any(|existing: &OwnedTerminalWriterTraceArtifact| {
                existing.identity == artifact.identity
            })
        {
            return None;
        }
        artifacts.push(artifact);
    }
    (!artifacts.is_empty()).then_some(OwnedTerminalWriterTraceJournal { artifacts })
}

fn terminal_writer_trace_artifact_is_structurally_valid(
    artifact: &OwnedTerminalWriterTraceArtifact,
) -> bool {
    let Some(layout) = validated_packet_layout(&artifact.finalized_payload) else {
        return false;
    };
    artifact.absolute_record_offset == artifact.owner_begin_read_cursor
        && artifact.absolute_record_offset >= LIVE_OBJECT_CNW_WRITER_HEADER_BYTES
        && artifact
            .absolute_record_offset
            .checked_add(LIVE_OBJECT_UPDATE_HEADER_BYTES)
            .is_some_and(|end| end <= artifact.owner_end_read_cursor)
        && artifact.owner_end_read_cursor <= artifact.list_handoff_read_cursor
        && artifact.list_handoff_read_cursor == artifact.final_read_end
        && artifact.final_read_end == layout.declared
        && artifact.owner_begin_fragment_cursor <= artifact.owner_end_fragment_cursor
        && artifact.owner_end_fragment_cursor <= artifact.list_handoff_fragment_cursor
        && artifact.list_handoff_fragment_cursor == artifact.final_fragment_cursor
        && artifact.final_fragment_cursor == layout.fragment_valid_bits
        && layout.payload.get(artifact.absolute_record_offset).copied() == Some(b'U')
}

#[cfg(test)]
fn parse_terminal_writer_trace_artifact(text: &str) -> Option<OwnedTerminalWriterTraceArtifact> {
    let mut journal = parse_terminal_writer_trace_journal(text)?;
    (journal.artifacts.len() == 1)
        .then(|| journal.artifacts.pop())
        .flatten()
}

fn parse_terminal_writer_trace_artifact_lines(
    lines: [&str; 6],
) -> Option<OwnedTerminalWriterTraceArtifact> {
    let [
        header_line,
        owner_begin_line,
        owner_end_line,
        list_handoff_line,
        finalize_line,
        payload_line,
    ] = lines;

    let header = split_tsv_exact::<9>(header_line)?;
    if header[0] != "terminal-writer-trace"
        || header[1] != "version"
        || header[2] != TERMINAL_WRITER_TRACE_ARTIFACT_VERSION
    {
        return None;
    }
    let identity = parse_trace_identity(&header, 3)?;

    let owner_begin = split_tsv_exact::<13>(owner_begin_line)?;
    if owner_begin[0] != "owner-begin" || parse_trace_identity(&owner_begin, 1)? != identity {
        return None;
    }
    expect_keys(
        &owner_begin,
        &[
            (7, "absolute_record_offset"),
            (9, "absolute_read_buffer_cursor"),
            (11, "fragment_bit_cursor"),
        ],
    )?;
    let absolute_record_offset = parse_decimal_usize(owner_begin[8])?;
    let owner_begin_read_cursor = parse_decimal_usize(owner_begin[10])?;
    let owner_begin_fragment_cursor = parse_decimal_usize(owner_begin[12])?;

    let owner_end = split_tsv_exact::<11>(owner_end_line)?;
    if owner_end[0] != "owner-end" || parse_trace_identity(&owner_end, 1)? != identity {
        return None;
    }
    expect_keys(
        &owner_end,
        &[
            (7, "absolute_read_buffer_cursor"),
            (9, "fragment_bit_cursor"),
        ],
    )?;
    let owner_end_read_cursor = parse_decimal_usize(owner_end[8])?;
    let owner_end_fragment_cursor = parse_decimal_usize(owner_end[10])?;

    let list_handoff = split_tsv_exact::<11>(list_handoff_line)?;
    if list_handoff[0] != "list-handoff" || parse_trace_identity(&list_handoff, 1)? != identity {
        return None;
    }
    expect_keys(
        &list_handoff,
        &[
            (7, "absolute_read_buffer_cursor"),
            (9, "fragment_bit_cursor"),
        ],
    )?;
    let list_handoff_read_cursor = parse_decimal_usize(list_handoff[8])?;
    let list_handoff_fragment_cursor = parse_decimal_usize(list_handoff[10])?;

    let finalize = split_tsv_exact::<11>(finalize_line)?;
    if finalize[0] != "finalize" || parse_trace_identity(&finalize, 1)? != identity {
        return None;
    }
    expect_keys(
        &finalize,
        &[(7, "absolute_read_buffer_end"), (9, "fragment_bit_cursor")],
    )?;
    let final_read_end = parse_decimal_usize(finalize[8])?;
    let final_fragment_cursor = parse_decimal_usize(finalize[10])?;

    let payload = split_tsv_exact::<9>(payload_line)?;
    if payload[0] != "finalized-payload"
        || parse_trace_identity(&payload, 1)? != identity
        || payload[7] != "hex"
    {
        return None;
    }
    let finalized_payload = decode_bounded_hex_payload(payload[8])?;

    Some(OwnedTerminalWriterTraceArtifact {
        identity,
        absolute_record_offset,
        owner_begin_read_cursor,
        owner_begin_fragment_cursor,
        owner_end_read_cursor,
        owner_end_fragment_cursor,
        list_handoff_read_cursor,
        list_handoff_fragment_cursor,
        final_read_end,
        final_fragment_cursor,
        finalized_payload,
    })
}

fn split_tsv_exact<const N: usize>(line: &str) -> Option<[&str; N]> {
    line.split('\t').collect::<Vec<_>>().try_into().ok()
}

fn expect_keys(tokens: &[&str], keys: &[(usize, &str)]) -> Option<()> {
    keys.iter()
        .all(|(index, expected)| tokens.get(*index).copied() == Some(*expected))
        .then_some(())
}

fn parse_trace_identity(tokens: &[&str], key_start: usize) -> Option<TerminalWriterTraceIdentity> {
    expect_keys(
        tokens,
        &[
            (key_start, "trace_id"),
            (key_start + 2, "message_id"),
            (key_start + 4, "component_sha256"),
        ],
    )?;
    let trace_id = parse_decimal_u64(*tokens.get(key_start + 1)?)?;
    if trace_id == 0 {
        return None;
    }
    let message_id = parse_fixed_hex_u64(*tokens.get(key_start + 3)?)?;
    let component_sha256 = parse_sha256(*tokens.get(key_start + 5)?)?;
    Some(TerminalWriterTraceIdentity {
        trace_id,
        message_id,
        component_sha256,
    })
}

fn parse_decimal_usize(value: &str) -> Option<usize> {
    parse_decimal_u64(value).and_then(|value| usize::try_from(value).ok())
}

fn parse_decimal_u64(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_fixed_hex_u64(value: &str) -> Option<u64> {
    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(value, 16).ok()
}

fn parse_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut digest = [0u8; 32];
    for (slot, pair) in digest.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *slot = decode_hex_byte(pair[0], pair[1])?;
    }
    Some(digest)
}

fn decode_bounded_hex_payload(value: &str) -> Option<Vec<u8>> {
    if value.is_empty()
        || value.len() % 2 != 0
        || value.len() > MAX_REASONABLE_LIVE_PAYLOAD_BYTES.checked_mul(2)?
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let mut payload = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        payload.push(decode_hex_byte(pair[0], pair[1])?);
    }
    Some(payload)
}

fn decode_hex_byte(high: u8, low: u8) -> Option<u8> {
    Some(
        hex_nibble(high)?
            .checked_mul(16)?
            .checked_add(hex_nibble(low)?)?,
    )
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn correlate_bounded_terminal_writer_trace(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    quarantined_payload: &[u8],
    trace: Option<TerminalWriterTrace<'_>>,
) -> LiveObjectUpdateTerminalWriterHandoffVerdict {
    let Some(trace) = trace else {
        return LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace;
    };
    correlate_optional_observation(
        requirement,
        build_terminal_writer_observation(quarantined_payload, trace),
    )
}

fn correlate_optional_observation(
    requirement: LiveObjectUpdateTerminalWriterHandoffRequirement,
    observation: Option<TerminalWriterObservation>,
) -> LiveObjectUpdateTerminalWriterHandoffVerdict {
    if !requirement.source_contract_is_valid() {
        return LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace;
    }
    let Some(observation) = observation else {
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
            identity: owner_begin_identity,
            absolute_record_offset,
            absolute_read_buffer_cursor: owner_begin_read_cursor,
            fragment_bit_cursor: owner_begin_fragment_cursor,
        },
        TerminalWriterTraceEvent::OwnerEnd {
            identity: owner_end_identity,
            absolute_read_buffer_cursor: owner_end_read_cursor,
            fragment_bit_cursor: owner_end_fragment_cursor,
        },
        TerminalWriterTraceEvent::ListHandoff {
            identity: list_handoff_identity,
            absolute_read_buffer_cursor: list_handoff_read_cursor,
            fragment_bit_cursor: list_handoff_fragment_cursor,
        },
        TerminalWriterTraceEvent::Finalize {
            identity: finalizer_identity,
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

    if owner_begin_identity != owner_end_identity
        || owner_begin_identity != list_handoff_identity
        || owner_begin_identity != finalizer_identity
        || absolute_record_offset != owner_begin_read_cursor
        || absolute_record_offset < LIVE_OBJECT_CNW_WRITER_HEADER_BYTES
        || absolute_record_offset.checked_add(LIVE_OBJECT_UPDATE_HEADER_BYTES)?
            > owner_end_read_cursor
        || owner_end_read_cursor > list_handoff_read_cursor
        || list_handoff_read_cursor != final_read_end
        || final_read_end != layout.declared
        || owner_begin_fragment_cursor > owner_end_fragment_cursor
        || owner_end_fragment_cursor > list_handoff_fragment_cursor
        || list_handoff_fragment_cursor != final_fragment_cursor
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
    // The typed owner may itself consume fragment bits. Only the interval
    // written after that owner returns and before the list hands the message to
    // the finalizer can satisfy the terminal handoff requirement.
    let post_owner_fragment_bits = rewrite_bit_slice_from_payload(
        layout.fragment,
        owner_end_fragment_cursor,
        list_handoff_fragment_cursor,
    )?;
    let cursor_evidence = TerminalWriterTraceCursorEvidence {
        owner_begin_read_cursor,
        owner_end_read_cursor,
        list_handoff_read_cursor,
        final_read_end,
        owner_begin_fragment_cursor,
        owner_end_fragment_cursor,
        list_handoff_fragment_cursor,
        final_fragment_cursor,
    };

    Some(TerminalWriterObservation {
        absolute_record_offset,
        object_type,
        object_id,
        raw_mask,
        // Source reader coordinates exclude the fixed CNW writer envelope and
        // describe the list handoff/finalized read window, not the earlier
        // typed-owner return point.
        handoff_read_buffer_cursor: list_handoff_read_cursor
            .checked_sub(LIVE_OBJECT_CNW_WRITER_HEADER_BYTES)?,
        final_read_buffer_end: final_read_end.checked_sub(LIVE_OBJECT_CNW_WRITER_HEADER_BYTES)?,
        post_owner_fragment_bits,
        finalized_fragment_bit_cursor: final_fragment_cursor,
        packet_correlation,
        cursor_evidence,
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
    msb_valid_bit_count(fragment, CNW_FRAGMENT_HEADER_BITS)
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
    if requirement
        .source_record_offset
        .checked_add(LIVE_OBJECT_CNW_WRITER_HEADER_BYTES)
        != Some(observation.absolute_record_offset)
    {
        return Verdict::RecordOffsetMismatch;
    }
    if observation.object_type != requirement.object_type
        || observation.object_id != requirement.object_id
        || observation.raw_mask != requirement.raw_mask
    {
        return Verdict::IdentityMismatch;
    }
    if observation.handoff_read_buffer_cursor != requirement.source_read_buffer_cursor
        || observation.final_read_buffer_end != requirement.source_read_buffer_end
    {
        return Verdict::ReadBufferMismatch;
    }

    let observed_bits = observation.post_owner_fragment_bits;
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(0);

    const SOURCE_VALUES: [bool; 13] = [
        false, false, false, false, false, false, true, false, false, false, true, true, false,
    ];

    fn temp_artifact_path(label: &str) -> PathBuf {
        let sequence = TEMP_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "hgbridge-terminal-writer-trace-{label}-{}-{sequence}.tsv",
            std::process::id()
        ))
    }

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
            source_record_offset: 166,
            object_type: 0x09,
            object_id: 0x8000_1003,
            raw_mask: 0xFFFF_FFF7,
            source_read_buffer_cursor: 229,
            source_read_buffer_end: 229,
            source_fragment_bits: bit_slice(63, &SOURCE_VALUES),
            source_next_opcode_read_overflows: true,
            emitted_record_offset: 164,
            emitted_mask: 0xFFFF_FFF7,
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

    fn trace_identity() -> TerminalWriterTraceIdentity {
        TerminalWriterTraceIdentity {
            trace_id: 17,
            message_id: 0x0000_0000_1234_ABCD,
            component_sha256: [0xA5; 32],
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
        assert_eq!(terminal_record_offset, Some(173));
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
        let identity = trace_identity();
        TerminalWriterTrace {
            events: [
                TerminalWriterTraceEvent::OwnerBegin {
                    identity,
                    absolute_record_offset: record_offset,
                    absolute_read_buffer_cursor: record_offset,
                    fragment_bit_cursor: 50,
                },
                TerminalWriterTraceEvent::OwnerEnd {
                    identity,
                    absolute_read_buffer_cursor: declared,
                    fragment_bit_cursor: 63,
                },
                TerminalWriterTraceEvent::ListHandoff {
                    identity,
                    absolute_read_buffer_cursor: declared,
                    fragment_bit_cursor,
                },
                TerminalWriterTraceEvent::Finalize {
                    identity,
                    absolute_read_buffer_end: declared,
                    fragment_bit_cursor,
                    packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(
                        payload,
                    ),
                },
            ],
        }
    }

    fn artifact_text(payload: &[u8], record_offset: usize) -> String {
        artifact_text_for_identity(payload, record_offset, trace_identity())
    }

    fn artifact_text_for_identity(
        payload: &[u8],
        record_offset: usize,
        identity: TerminalWriterTraceIdentity,
    ) -> String {
        use std::fmt::Write as _;

        let mut payload_hex = String::with_capacity(payload.len() * 2);
        for byte in payload {
            write!(&mut payload_hex, "{byte:02X}").expect("write payload hex");
        }
        let mut component_sha256 = String::with_capacity(64);
        for byte in identity.component_sha256 {
            write!(&mut component_sha256, "{byte:02X}").expect("write component digest hex");
        }
        let identity = format!(
            "trace_id\t{}\tmessage_id\t{:016X}\tcomponent_sha256\t{}",
            identity.trace_id, identity.message_id, component_sha256
        );
        let declared = usize::try_from(u32::from_le_bytes(
            payload[3..7].try_into().expect("declared bytes"),
        ))
        .expect("declared length");
        let fragment_bit_cursor =
            cnw_fragment_valid_bits(&payload[declared..]).expect("coherent fragment bit count");
        format!(
            "terminal-writer-trace\tversion\t2\t{identity}\n\
             owner-begin\t{identity}\tabsolute_record_offset\t{record_offset}\tabsolute_read_buffer_cursor\t{record_offset}\tfragment_bit_cursor\t50\n\
             owner-end\t{identity}\tabsolute_read_buffer_cursor\t{declared}\tfragment_bit_cursor\t63\n\
             list-handoff\t{identity}\tabsolute_read_buffer_cursor\t{declared}\tfragment_bit_cursor\t{fragment_bit_cursor}\n\
             finalize\t{identity}\tabsolute_read_buffer_end\t{declared}\tfragment_bit_cursor\t{fragment_bit_cursor}\n\
             finalized-payload\t{identity}\thex\t{payload_hex}\n"
        )
    }

    #[test]
    fn strict_v2_artifact_parses_and_reaches_exact_diagnostic_correlation() {
        let (payload, record_offset) = coherent_payload();
        let artifact =
            parse_terminal_writer_trace_artifact(&artifact_text(&payload, record_offset))
                .expect("strict operator artifact");
        assert_eq!(artifact.identity, trace_identity());
        assert_eq!(artifact.absolute_record_offset, record_offset);
        assert_eq!(artifact.owner_begin_fragment_cursor, 50);
        assert_eq!(artifact.owner_end_fragment_cursor, 63);
        assert_eq!(artifact.list_handoff_fragment_cursor, 76);
        assert_eq!(artifact.final_fragment_cursor, 76);
        assert_eq!(artifact.finalized_payload, payload);
        assert_eq!(
            correlate_bounded_terminal_writer_trace(
                requirement(),
                &artifact.finalized_payload,
                Some(artifact.as_trace()),
            ),
            LiveObjectUpdateTerminalWriterHandoffVerdict::ExactObservedHandoff
        );
        assert!(
            !LiveObjectUpdateTerminalWriterHandoffVerdict::ExactObservedHandoff
                .allows_exact_claim(),
            "operator proof alone must remain non-claiming"
        );
        let correlation = correlate_loaded_terminal_writer_trace(
            requirement(),
            &artifact.finalized_payload,
            &artifact,
        );
        let exact_handoff = correlation
            .exact_handoff
            .expect("byte-for-byte exact trace should mint one opaque source token");
        assert!(exact_handoff.matches(requirement(), &artifact.finalized_payload));
        assert_eq!(
            correlation.cursor_evidence,
            Some(TerminalWriterTraceCursorEvidence {
                owner_begin_read_cursor: 173,
                owner_end_read_cursor: 236,
                list_handoff_read_cursor: 236,
                final_read_end: 236,
                owner_begin_fragment_cursor: 50,
                owner_end_fragment_cursor: 63,
                list_handoff_fragment_cursor: 76,
                final_fragment_cursor: 76,
            })
        );
        let mut different_payload = artifact.finalized_payload.clone();
        different_payload[7] ^= 1;
        assert!(!exact_handoff.matches(requirement(), &different_payload));

        let invalid_requirement = LiveObjectUpdateTerminalWriterHandoffRequirement {
            source_next_opcode_read_overflows: false,
            ..requirement()
        };
        let invalid = correlate_loaded_terminal_writer_trace(
            invalid_requirement,
            &artifact.finalized_payload,
            &artifact,
        );
        assert_eq!(
            invalid.verdict,
            LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace
        );
        assert!(invalid.exact_handoff.is_none());
    }

    #[test]
    fn bounded_journal_selects_one_exact_payload_and_reports_provenance() {
        let (payload, record_offset) = coherent_payload();
        let mut unrelated_payload = payload.clone();
        unrelated_payload[7] ^= 0x01;
        let unrelated_identity = TerminalWriterTraceIdentity {
            trace_id: 18,
            message_id: 0x0000_0000_1234_ABCE,
            component_sha256: [0xB6; 32],
        };
        let journal_text = format!(
            "{}{}",
            artifact_text_for_identity(&unrelated_payload, record_offset, unrelated_identity),
            artifact_text(&payload, record_offset)
        );
        let journal = parse_terminal_writer_trace_journal(&journal_text)
            .expect("two independently sealed trace blocks");
        assert_eq!(journal.artifacts.len(), 2);

        let correlation =
            correlate_loaded_terminal_writer_trace_journal(requirement(), &payload, &journal);
        assert_eq!(
            correlation.artifact_status,
            TerminalWriterTraceArtifactStatus::Loaded
        );
        assert_eq!(
            correlation.selection_status,
            TerminalWriterTraceSelectionStatus::UniquePayloadMatch
        );
        assert_eq!(correlation.artifact_count, 2);
        assert_eq!(correlation.payload_match_count, 1);
        assert_eq!(
            correlation.verdict,
            LiveObjectUpdateTerminalWriterHandoffVerdict::ExactObservedHandoff
        );
        assert_eq!(correlation.trace_id, Some(trace_identity().trace_id));
        assert_eq!(correlation.message_id, Some(trace_identity().message_id));
        assert_eq!(
            correlation.component_sha256,
            Some(trace_identity().component_sha256)
        );
        let exact_handoff = correlation
            .exact_handoff
            .expect("the unique exact block should mint one opaque source token");
        assert!(exact_handoff.matches(requirement(), &payload));
        assert!(
            !LiveObjectUpdateTerminalWriterHandoffVerdict::ExactObservedHandoff
                .allows_exact_claim(),
            "journal selection must not turn source proof into wire authority"
        );

        let mut absent_payload = payload.clone();
        absent_payload[8] ^= 0x02;
        let absent = correlate_loaded_terminal_writer_trace_journal(
            requirement(),
            &absent_payload,
            &journal,
        );
        assert_eq!(
            absent.artifact_status,
            TerminalWriterTraceArtifactStatus::Loaded
        );
        assert_eq!(
            absent.selection_status,
            TerminalWriterTraceSelectionStatus::NoPayloadMatch
        );
        assert_eq!(absent.artifact_count, 2);
        assert_eq!(absent.payload_match_count, 0);
        assert_eq!(
            absent.verdict,
            LiveObjectUpdateTerminalWriterHandoffVerdict::PacketMismatch
        );
        assert_eq!(absent.trace_id, None);
        assert_eq!(absent.message_id, None);
        assert_eq!(absent.component_sha256, None);
        assert!(absent.exact_handoff.is_none());
    }

    #[test]
    fn journal_rejects_ambiguous_payload_before_requirement_correlation() {
        let (payload, record_offset) = coherent_payload();
        let other_identity = TerminalWriterTraceIdentity {
            trace_id: 19,
            message_id: 0x0000_0000_1234_ABCF,
            component_sha256: [0xC7; 32],
        };
        let journal_text = format!(
            "{}{}",
            artifact_text(&payload, record_offset),
            artifact_text_for_identity(&payload, 83, other_identity)
        );
        let journal = parse_terminal_writer_trace_journal(&journal_text)
            .expect("same payload with distinct identities stays parseable");
        let correlation =
            correlate_loaded_terminal_writer_trace_journal(requirement(), &payload, &journal);

        assert_eq!(
            correlation.artifact_status,
            TerminalWriterTraceArtifactStatus::Loaded
        );
        assert_eq!(
            correlation.selection_status,
            TerminalWriterTraceSelectionStatus::AmbiguousPayloadMatch
        );
        assert_eq!(correlation.artifact_count, 2);
        assert_eq!(correlation.payload_match_count, 2);
        assert_eq!(
            correlation.verdict,
            LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace
        );
        assert_eq!(correlation.trace_id, None);
        assert_eq!(correlation.message_id, None);
        assert_eq!(correlation.component_sha256, None);
        assert!(
            correlation.exact_handoff.is_none(),
            "even one requirement-exact block cannot win an ambiguous payload selection"
        );
    }

    #[test]
    fn journal_parser_rejects_duplicate_identities_partial_blocks_separators_and_entry_overflow() {
        let (payload, record_offset) = coherent_payload();
        let exact = artifact_text(&payload, record_offset);
        assert!(parse_terminal_writer_trace_journal(&format!("{exact}{exact}")).is_none());

        let partial = exact.lines().take(5).collect::<Vec<_>>().join("\n");
        assert!(parse_terminal_writer_trace_journal(&partial).is_none());

        let poisoned =
            format!("{exact}terminal-writer-trace\tincomplete\ttrace_id\t65\tstatus\t8\n");
        assert!(
            parse_terminal_writer_trace_journal(&poisoned).is_none(),
            "an omitted producer trace must invalidate every earlier unique-looking block"
        );

        let second_identity = TerminalWriterTraceIdentity {
            trace_id: 18,
            message_id: 0x0000_0000_1234_ABCE,
            component_sha256: [0xB6; 32],
        };
        let separated = format!(
            "{exact}\n{}",
            artifact_text_for_identity(&payload, record_offset, second_identity)
        );
        assert!(
            parse_terminal_writer_trace_journal(&separated).is_none(),
            "the append-only grammar admits consecutive sealed blocks only"
        );

        let capped = (1..=MAX_TERMINAL_WRITER_TRACE_JOURNAL_ARTIFACTS)
            .map(|trace_id| {
                artifact_text_for_identity(
                    &payload,
                    record_offset,
                    TerminalWriterTraceIdentity {
                        trace_id: trace_id as u64,
                        message_id: 0x1000 + trace_id as u64,
                        component_sha256: [trace_id as u8; 32],
                    },
                )
            })
            .collect::<String>();
        assert_eq!(
            parse_terminal_writer_trace_journal(&capped)
                .expect("journal at entry cap")
                .artifacts
                .len(),
            MAX_TERMINAL_WRITER_TRACE_JOURNAL_ARTIFACTS
        );
        let overflow = format!(
            "{capped}{}",
            artifact_text_for_identity(
                &payload,
                record_offset,
                TerminalWriterTraceIdentity {
                    trace_id: (MAX_TERMINAL_WRITER_TRACE_JOURNAL_ARTIFACTS + 1) as u64,
                    message_id: 0x2000,
                    component_sha256: [0xEE; 32],
                }
            )
        );
        assert!(parse_terminal_writer_trace_journal(&overflow).is_none());
    }

    #[test]
    fn journal_parser_rejects_malformed_packet_layout_and_writer_cursors() {
        let (payload, record_offset) = coherent_payload();

        let mut malformed_envelope = payload.clone();
        malformed_envelope[2] = 0x02;
        assert!(
            parse_terminal_writer_trace_journal(&artifact_text(&malformed_envelope, record_offset))
                .is_none()
        );

        let malformed_cursor = artifact_text(&payload, record_offset).replacen(
            "\tabsolute_read_buffer_cursor\t236\tfragment_bit_cursor\t63\n",
            "\tabsolute_read_buffer_cursor\t237\tfragment_bit_cursor\t63\n",
            1,
        );
        assert!(parse_terminal_writer_trace_journal(&malformed_cursor).is_none());

        assert!(
            parse_terminal_writer_trace_journal(&artifact_text(&payload, record_offset + 1))
                .is_none(),
            "the bracket must begin at an actual U/type/id/mask record"
        );
    }

    #[test]
    fn artifact_rejects_missing_reordered_cross_identity_and_unknown_events() {
        let (payload, record_offset) = coherent_payload();
        let exact = artifact_text(&payload, record_offset);
        let lines: Vec<_> = exact.lines().collect();

        let missing_handoff = lines
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 3)
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_terminal_writer_trace_artifact(&missing_handoff).is_none());

        let mut reordered = lines.clone();
        reordered.swap(2, 3);
        assert!(parse_terminal_writer_trace_artifact(&reordered.join("\n")).is_none());

        let cross_identity = exact.replacen(
            "list-handoff\ttrace_id\t17",
            "list-handoff\ttrace_id\t18",
            1,
        );
        assert!(parse_terminal_writer_trace_artifact(&cross_identity).is_none());

        let unknown_event = format!("{exact}unknown-event\n");
        assert!(parse_terminal_writer_trace_artifact(&unknown_event).is_none());

        let legacy_v1 = exact.replacen(
            "terminal-writer-trace\tversion\t2",
            "terminal-writer-trace\tversion\t1",
            1,
        );
        assert!(
            parse_terminal_writer_trace_artifact(&legacy_v1).is_none(),
            "v1 conflated owner return and list handoff, so it must fail closed"
        );
    }

    #[test]
    fn artifact_payload_and_handoff_cursors_remain_exact_packet_bound_evidence() {
        let (payload, record_offset) = coherent_payload();

        let mut wrong_handoff =
            parse_terminal_writer_trace_artifact(&artifact_text(&payload, record_offset))
                .expect("cursor mismatch stays parseable evidence");
        wrong_handoff.list_handoff_read_cursor = 235;
        assert!(
            build_terminal_writer_observation(&payload, wrong_handoff.as_trace()).is_none(),
            "a handoff label without the exact owner/finalizer cursor must reject"
        );

        let mut other_payload = payload.clone();
        other_payload[236 + 8] ^= 0x02;
        let other_artifact =
            parse_terminal_writer_trace_artifact(&artifact_text(&other_payload, record_offset))
                .expect("different complete packet stays structurally parseable");
        assert_eq!(
            correlate_bounded_terminal_writer_trace(
                requirement(),
                &payload,
                Some(other_artifact.as_trace()),
            ),
            LiveObjectUpdateTerminalWriterHandoffVerdict::PacketMismatch
        );
    }

    #[test]
    fn exact_trace_is_derived_from_one_coherent_finalized_packet() {
        let (payload, record_offset) = coherent_payload();
        let trace = exact_trace(&payload, record_offset);
        let observation = build_terminal_writer_observation(&payload, trace)
            .expect("coherent bounded trace observation");
        assert_eq!(observation.object_type, 0x09);
        assert_eq!(observation.absolute_record_offset, 173);
        assert_eq!(observation.object_id, 0x8000_1003);
        assert_eq!(observation.raw_mask, 0xFFFF_FFF7);
        assert_eq!(observation.handoff_read_buffer_cursor, 229);
        assert_eq!(observation.final_read_buffer_end, 229);
        assert_eq!(observation.post_owner_fragment_bits.bit_start, 63);
        assert_eq!(observation.post_owner_fragment_bits.bit_end, 76);
        assert_eq!(observation.cursor_evidence.post_owner_read_bytes(), Some(0));
        assert_eq!(
            observation.cursor_evidence.post_owner_fragment_bits(),
            Some(13)
        );
        assert_eq!(
            correlate_bounded_terminal_writer_trace(requirement(), &payload, Some(trace)),
            LiveObjectUpdateTerminalWriterHandoffVerdict::ExactObservedHandoff
        );
    }

    #[test]
    fn stock_three_byte_zero_bit_postlude_parses_but_cannot_mint_a_token() {
        let (mut payload, record_offset) = coherent_payload();
        let declared = 236;
        payload.truncate(declared + 8);
        payload[declared] = (payload[declared] & 0x1F) | 0xE0;
        payload[declared + 7] &= 0xFE;
        assert_eq!(cnw_fragment_valid_bits(&payload[declared..]), Some(63));
        let stock_text = artifact_text(&payload, record_offset).replacen(
            "absolute_read_buffer_cursor\t236\tfragment_bit_cursor\t63\n",
            "absolute_read_buffer_cursor\t233\tfragment_bit_cursor\t63\n",
            1,
        );
        let artifact = parse_terminal_writer_trace_artifact(&stock_text)
            .expect("v2 permits a byte-only post-owner list postlude");
        assert_eq!(artifact.owner_end_read_cursor, 233);
        assert_eq!(artifact.list_handoff_read_cursor, 236);
        assert_eq!(artifact.owner_end_fragment_cursor, 63);
        assert_eq!(artifact.list_handoff_fragment_cursor, 63);
        let correlation =
            correlate_loaded_terminal_writer_trace(requirement(), &payload, &artifact);
        assert_eq!(
            correlation.verdict,
            LiveObjectUpdateTerminalWriterHandoffVerdict::NoTerminalFragmentWrites
        );
        assert!(correlation.exact_handoff.is_none());
        let cursor_evidence = correlation
            .cursor_evidence
            .expect("unique structurally valid stock trace retains diagnostics");
        assert_eq!(cursor_evidence.post_owner_read_bytes(), Some(3));
        assert_eq!(cursor_evidence.post_owner_fragment_bits(), Some(0));
    }

    #[test]
    fn malformed_envelopes_declared_windows_fragments_and_event_order_reject() {
        let (payload, record_offset) = coherent_payload();

        let mut bad_header = payload.clone();
        bad_header[2] = 0x02;
        let mut bad_header_trace = exact_trace(&payload, record_offset);
        bad_header_trace.events[3] = TerminalWriterTraceEvent::Finalize {
            identity: trace_identity(),
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
            identity: trace_identity(),
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
            identity: trace_identity(),
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
            identity: trace_identity(),
            absolute_read_buffer_cursor: 237,
            fragment_bit_cursor: 63,
        };
        assert!(build_terminal_writer_observation(&payload, read_cursor).is_none());

        let mut owner_ends_before_header = exact_trace(&payload, record_offset);
        owner_ends_before_header.events[1] = TerminalWriterTraceEvent::OwnerEnd {
            identity: trace_identity(),
            absolute_read_buffer_cursor: record_offset + LIVE_OBJECT_UPDATE_HEADER_BYTES - 1,
            fragment_bit_cursor: 63,
        };
        assert!(build_terminal_writer_observation(&payload, owner_ends_before_header).is_none());

        let mut fragment_order = exact_trace(&payload, record_offset);
        fragment_order.events[1] = TerminalWriterTraceEvent::OwnerEnd {
            identity: trace_identity(),
            absolute_read_buffer_cursor: 236,
            fragment_bit_cursor: 77,
        };
        assert!(build_terminal_writer_observation(&payload, fragment_order).is_none());

        let mut fragment_cursor = exact_trace(&payload, record_offset);
        fragment_cursor.events[3] = TerminalWriterTraceEvent::Finalize {
            identity: trace_identity(),
            absolute_read_buffer_end: 236,
            fragment_bit_cursor: 75,
            packet_evidence: TerminalWriterTracePacketEvidence::FullFinalizedPayload(&payload),
        };
        assert!(build_terminal_writer_observation(&payload, fragment_cursor).is_none());

        let mut record_token = exact_trace(&payload, record_offset);
        record_token.events[0] = TerminalWriterTraceEvent::OwnerBegin {
            identity: trace_identity(),
            absolute_record_offset: record_offset + 1,
            absolute_read_buffer_cursor: record_offset + 1,
            fragment_bit_cursor: 50,
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
    fn exact_packet_with_duplicate_identity_cannot_bind_the_wrong_record_offset() {
        let (mut payload, _) = coherent_payload();
        let earlier_placeable_update_absolute_offset = 83;
        assert_eq!(
            payload.get(
                earlier_placeable_update_absolute_offset
                    ..earlier_placeable_update_absolute_offset + 2
            ),
            Some([b'U', 9].as_slice())
        );
        payload[earlier_placeable_update_absolute_offset + 2
            ..earlier_placeable_update_absolute_offset + 6]
            .copy_from_slice(&0x8000_1003u32.to_le_bytes());
        let artifact = parse_terminal_writer_trace_artifact(&artifact_text(
            &payload,
            earlier_placeable_update_absolute_offset,
        ))
        .expect("same-object earlier update remains a structurally valid artifact");
        assert_eq!(
            correlate_bounded_terminal_writer_trace(
                requirement(),
                &payload,
                Some(artifact.as_trace()),
            ),
            LiveObjectUpdateTerminalWriterHandoffVerdict::RecordOffsetMismatch
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
                identity: trace_identity(),
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
            TerminalWriterTraceCorrelation {
                artifact_status: TerminalWriterTraceArtifactStatus::NotConfigured,
                selection_status: TerminalWriterTraceSelectionStatus::NotConfigured,
                artifact_count: 0,
                payload_match_count: 0,
                verdict: LiveObjectUpdateTerminalWriterHandoffVerdict::IncompleteTrace,
                trace_id: None,
                message_id: None,
                component_sha256: None,
                cursor_evidence: None,
                exact_handoff: None,
            }
        );
    }

    #[test]
    fn artifact_loader_maps_filesystem_boundary_statuses_and_accepts_valid_v2() {
        let (payload, record_offset) = coherent_payload();

        let valid_path = temp_artifact_path("valid");
        std::fs::write(&valid_path, artifact_text(&payload, record_offset))
            .expect("write valid terminal writer artifact");
        let loaded = load_terminal_writer_trace_journal(&valid_path)
            .expect("valid single-block v2 terminal writer journal should load");
        assert_eq!(loaded.artifacts.len(), 1);
        assert_eq!(loaded.artifacts[0].identity, trace_identity());
        assert_eq!(loaded.artifacts[0].finalized_payload, payload);
        std::fs::remove_file(&valid_path).expect("remove valid terminal writer artifact");

        let journal_path = temp_artifact_path("valid-journal");
        let second_identity = TerminalWriterTraceIdentity {
            trace_id: 18,
            message_id: 0x0000_0000_1234_ABCE,
            component_sha256: [0xB6; 32],
        };
        std::fs::write(
            &journal_path,
            format!(
                "{}{}",
                artifact_text(&payload, record_offset),
                artifact_text_for_identity(&payload, record_offset, second_identity)
            ),
        )
        .expect("write valid terminal writer journal");
        let loaded = load_terminal_writer_trace_journal(&journal_path)
            .expect("valid consecutive v2 trace blocks should load");
        assert_eq!(loaded.artifacts.len(), 2);
        assert_eq!(loaded.artifacts[0].identity, trace_identity());
        assert_eq!(loaded.artifacts[1].identity, second_identity);
        std::fs::remove_file(&journal_path).expect("remove valid terminal writer journal");

        let invalid_utf8_path = temp_artifact_path("invalid-utf8");
        std::fs::write(&invalid_utf8_path, [0xFF, 0xFE]).expect("write invalid UTF-8 artifact");
        assert!(matches!(
            load_terminal_writer_trace_journal(&invalid_utf8_path),
            Err(TerminalWriterTraceArtifactStatus::InvalidUtf8)
        ));
        std::fs::remove_file(&invalid_utf8_path).expect("remove invalid UTF-8 artifact");

        let invalid_format_path = temp_artifact_path("invalid-format");
        std::fs::write(&invalid_format_path, b"terminal-writer-trace\tversion\t1\n")
            .expect("write invalid format artifact");
        assert!(matches!(
            load_terminal_writer_trace_journal(&invalid_format_path),
            Err(TerminalWriterTraceArtifactStatus::InvalidFormat)
        ));
        std::fs::remove_file(&invalid_format_path).expect("remove invalid format artifact");

        let malformed_packet_path = temp_artifact_path("malformed-packet");
        let mut malformed_packet = payload.clone();
        malformed_packet[2] = 0x02;
        std::fs::write(
            &malformed_packet_path,
            artifact_text(&malformed_packet, record_offset),
        )
        .expect("write malformed packet artifact");
        assert!(matches!(
            load_terminal_writer_trace_journal(&malformed_packet_path),
            Err(TerminalWriterTraceArtifactStatus::InvalidFormat)
        ));
        std::fs::remove_file(&malformed_packet_path).expect("remove malformed packet artifact");

        let oversized_path = temp_artifact_path("oversized");
        let oversized = File::create(&oversized_path).expect("create sparse oversized artifact");
        oversized
            .set_len((MAX_TERMINAL_WRITER_TRACE_JOURNAL_BYTES as u64).saturating_add(1))
            .expect("size sparse oversized artifact");
        drop(oversized);
        assert!(matches!(
            load_terminal_writer_trace_journal(&oversized_path),
            Err(TerminalWriterTraceArtifactStatus::TooLarge)
        ));
        std::fs::remove_file(&oversized_path).expect("remove oversized artifact");
    }
}
