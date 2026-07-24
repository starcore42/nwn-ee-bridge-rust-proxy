//! EE-derived downstream ACK ownership for server rewrites that expand one
//! reliable source into multiple EE-facing reliable frames.
//!
//! A generic sequence insertion records how later server sequences move, but
//! it does not prove that the first output of a `1 -> N` rewrite represents the
//! complete source message. Keep that ownership explicit when mapping an ACK
//! actually observed from EE: while it is inside the rebuilt completion range
//! but before its terminal frame, the legacy-facing cumulative ACK remains at
//! `source - 1`.
//! Proxy-owned reliable insertions may occupy sequence numbers between the
//! source's first and terminal outputs; they advance transport but do not make
//! the source message complete.
//!
//! Diamond `sub_5F36E0`/`sub_5F3940` and EE
//! `CNetLayerWindow::FrameSend`/`FrameReceive` store and retire reliable data
//! cumulatively by the 16-bit sequence field. This module changes only the ACK
//! sequence selected at that transport boundary. It does not inspect or alter
//! CNW payload fields, bit order, BOOL order, cursor alignment, padding, or
//! nested object/string boundaries.

use super::{
    sequence::{
        OrderedServerSequenceEpochs, SequenceEpochKey, SequenceShift, sequence_at_or_after,
        unshift_ack_for_origin,
    },
    server_replay::ServerReliableSlotKey,
};

const MAX_SERVER_OUTPUT_ACK_SPANS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ServerOutputAckDestinationSpan {
    /// The semantic producer has identified the final raw destination
    /// sequences, but the complete output batch has not yet entered the
    /// destination send window that assigns exact generations.
    Pending { first: u16, last: u16 },
    /// Exact destination coordinates assigned by the staged EE send window.
    Exact {
        first: SequenceEpochKey,
        last: SequenceEpochKey,
    },
}

impl ServerOutputAckDestinationSpan {
    pub(super) fn sequences(self) -> (u16, u16) {
        match self {
            Self::Pending { first, last } => (first, last),
            Self::Exact { first, last } => (first.sequence, last.sequence),
        }
    }

    pub(super) fn exact(self) -> Option<(SequenceEpochKey, SequenceEpochKey)> {
        match self {
            Self::Pending { .. } => None,
            Self::Exact { first, last } => Some((first, last)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ServerOutputAckSpan {
    pub(super) source: ServerReliableSlotKey,
    /// First and terminal EE-facing destinations produced from `source`.
    /// Intermediate sequence numbers may belong to proxy-owned insertions.
    pub(super) destination: ServerOutputAckDestinationSpan,
}

pub(super) fn register_server_output_ack_span(
    spans: &mut Vec<ServerOutputAckSpan>,
    source: ServerReliableSlotKey,
    destination_first: u16,
    destination_last: u16,
) -> anyhow::Result<bool> {
    let extra_output_distance = destination_last.wrapping_sub(destination_first);
    if extra_output_distance == 0 || extra_output_distance >= 0x8000 {
        anyhow::bail!(
            "server output ACK span must contain at least two forward reliable sequences"
        );
    }

    if let Some(existing) = spans.iter().find(|span| span.source == source) {
        if existing.destination.sequences() == (destination_first, destination_last) {
            return Ok(false);
        }
        anyhow::bail!("server reliable source already owns a different downstream ACK span");
    }
    if spans.iter().any(|span| {
        let (existing_first, existing_last) = span.destination.sequences();
        forward_closed_ranges_intersect(
            existing_first,
            existing_last,
            destination_first,
            destination_last,
        )
    }) {
        anyhow::bail!("downstream ACK span overlaps an active server output owner");
    }
    if spans.len() >= MAX_SERVER_OUTPUT_ACK_SPANS {
        anyhow::bail!(
            "server output ACK span window exceeded {} active entries",
            MAX_SERVER_OUTPUT_ACK_SPANS
        );
    }

    spans.push(ServerOutputAckSpan {
        source,
        destination: ServerOutputAckDestinationSpan::Pending {
            first: destination_first,
            last: destination_last,
        },
    });
    tracing::info!(
        source_sequence = source.sequence,
        source_origin_generation = source.origin_generation,
        destination_first,
        destination_last,
        active_spans = spans.len(),
        "registered downstream ACK completion span for expanded server rewrite"
    );
    Ok(true)
}

/// Bind one producer-discovered completion range to the exact destination
/// generations assigned by the staged EE send window.
///
/// The raw sequence pair remains part of the producer identity. Exact binding
/// must therefore preserve both endpoints and the same forward distance,
/// including a `0xFFFF -> 0x0000` wrap. Rebinding the same coordinates is
/// idempotent; a different generation assignment is a transport conflict.
pub(super) fn bind_server_output_ack_span_destination(
    span: &mut ServerOutputAckSpan,
    destination_first: SequenceEpochKey,
    destination_last: SequenceEpochKey,
) -> anyhow::Result<bool> {
    let (expected_first, expected_last) = span.destination.sequences();
    if destination_first.sequence != expected_first || destination_last.sequence != expected_last {
        anyhow::bail!("exact server output ACK span changed its raw destination endpoints");
    }
    let distance = expected_last.wrapping_sub(expected_first);
    if distance == 0 || distance >= 0x8000 {
        anyhow::bail!("exact server output ACK span has an invalid forward width");
    }
    if destination_first.checked_advance(u64::from(distance))? != destination_last {
        anyhow::bail!("exact server output ACK span generation disagrees with its forward width");
    }

    let exact = ServerOutputAckDestinationSpan::Exact {
        first: destination_first,
        last: destination_last,
    };
    match span.destination {
        ServerOutputAckDestinationSpan::Pending { .. } => {
            span.destination = exact;
            Ok(true)
        }
        existing if existing == exact => Ok(false),
        ServerOutputAckDestinationSpan::Exact { .. } => {
            anyhow::bail!("server output ACK span was rebound to a different destination epoch")
        }
    }
}

/// Map an observed EE cumulative ACK into the Diamond server's source sequence
/// domain.
///
/// Generic sequence unshifting remains authoritative before all owned
/// expansions. Inside `[destination_first, destination_last)`, the source is
/// incomplete and must remain unacknowledged. At or after a terminal output,
/// map only through the latest completed active owner. That conservative cap
/// is deliberate: an accepted upstream ACK retires the span, after which the
/// next EE-derived cumulative ACK can advance through later source frames.
/// This makes the completion rule independent of trimmed or reordered generic
/// shift history and cannot over-retire a source.
pub(super) fn map_client_ack_for_server(
    shifts: &[SequenceShift],
    spans: &[ServerOutputAckSpan],
    destination_ack: u16,
) -> u16 {
    for span in spans {
        let (destination_first, destination_last) = span.destination.sequences();
        if sequence_in_forward_half_open(destination_ack, destination_first, destination_last) {
            return span.source.sequence.wrapping_sub(1);
        }
    }

    if let Some(latest_completed) = spans
        .iter()
        .filter(|span| {
            let (_, destination_last) = span.destination.sequences();
            sequence_at_or_after(destination_ack, destination_last)
        })
        .min_by_key(|span| {
            let (_, destination_last) = span.destination.sequences();
            destination_ack.wrapping_sub(destination_last)
        })
    {
        return latest_completed.source.sequence;
    }

    unshift_ack_for_origin(shifts, destination_ack)
}

/// Map an exact EE cumulative ACK into the exact Diamond source epoch.
///
/// Expanded sources add a completion boundary on top of ordinary insertion
/// mapping: any ACK before the terminal rebuilt output stays at the source
/// predecessor, while the terminal output completes exactly that source.
/// Every active span must already have been bound by destination send-window
/// staging; a pending bare sequence fails closed instead of borrowing a
/// generation.
pub(super) fn map_exact_client_ack_for_server(
    epochs: &OrderedServerSequenceEpochs,
    spans: &[ServerOutputAckSpan],
    destination_ack: SequenceEpochKey,
) -> anyhow::Result<SequenceEpochKey> {
    let mut latest_completed = None::<(SequenceEpochKey, SequenceEpochKey)>;
    for span in spans {
        let (destination_first, destination_last) = span.destination.exact().ok_or_else(|| {
            anyhow::anyhow!("server output ACK span has no exact destination generation")
        })?;
        let source_generation = i64::try_from(span.source.origin_generation)
            .map_err(|_| anyhow::anyhow!("server output ACK source generation overflow"))?;
        let source = SequenceEpochKey::new(span.source.sequence, source_generation);
        if destination_ack >= destination_first && destination_ack < destination_last {
            return source.checked_retreat(1);
        }
        if destination_last <= destination_ack
            && latest_completed
                .as_ref()
                .is_none_or(|(latest_last, _)| destination_last > *latest_last)
        {
            latest_completed = Some((destination_last, source));
        }
    }

    if let Some((_, source)) = latest_completed {
        return Ok(source);
    }
    epochs.map_destination_ack(destination_ack)
}

pub(super) fn retire_server_output_ack_spans(
    spans: &mut Vec<ServerOutputAckSpan>,
    retired_sources: &[ServerReliableSlotKey],
) -> usize {
    if retired_sources.is_empty() {
        return 0;
    }
    let before = spans.len();
    spans.retain(|span| !retired_sources.contains(&span.source));
    before.saturating_sub(spans.len())
}

fn sequence_in_forward_half_open(sequence: u16, first: u16, end: u16) -> bool {
    let width = end.wrapping_sub(first);
    width != 0 && width < 0x8000 && sequence.wrapping_sub(first) < width
}

fn sequence_in_forward_closed(sequence: u16, first: u16, last: u16) -> bool {
    let width = last.wrapping_sub(first);
    width < 0x8000 && sequence.wrapping_sub(first) <= width
}

fn forward_closed_ranges_intersect(first_a: u16, last_a: u16, first_b: u16, last_b: u16) -> bool {
    sequence_in_forward_closed(first_a, first_b, last_b)
        || sequence_in_forward_closed(last_a, first_b, last_b)
        || sequence_in_forward_closed(first_b, first_a, last_a)
        || sequence_in_forward_closed(last_b, first_a, last_a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sequence: u16, origin_generation: u64) -> ServerReliableSlotKey {
        ServerReliableSlotKey {
            sequence,
            origin_generation,
        }
    }

    fn pending_span(
        source: ServerReliableSlotKey,
        destination_first: u16,
        destination_last: u16,
    ) -> ServerOutputAckSpan {
        ServerOutputAckSpan {
            source,
            destination: ServerOutputAckDestinationSpan::Pending {
                first: destination_first,
                last: destination_last,
            },
        }
    }

    fn epoch(sequence: u16, generation: i64) -> SequenceEpochKey {
        SequenceEpochKey::new(sequence, generation)
    }

    #[test]
    fn partial_expanded_ack_stays_before_source_until_terminal_output() {
        let shifts = [SequenceShift { base: 62, delta: 1 }];
        let spans = [pending_span(key(61, 4), 61, 62)];

        assert_eq!(map_client_ack_for_server(&shifts, &spans, 60), 60);
        assert_eq!(map_client_ack_for_server(&shifts, &spans, 61), 60);
        assert_eq!(map_client_ack_for_server(&shifts, &spans, 62), 61);
        assert_eq!(
            map_client_ack_for_server(&shifts, &spans, 63),
            61,
            "an active owner conservatively caps a later cumulative ACK"
        );
        assert_eq!(map_client_ack_for_server(&shifts, &[], 63), 62);
    }

    #[test]
    fn wrapped_expanded_ack_waits_for_sequence_zero() {
        let shifts = [SequenceShift { base: 0, delta: 1 }];
        let spans = [pending_span(key(u16::MAX, 9), u16::MAX, 0)];

        assert_eq!(
            map_client_ack_for_server(&shifts, &spans, u16::MAX),
            u16::MAX - 1
        );
        assert_eq!(map_client_ack_for_server(&shifts, &spans, 0), u16::MAX);
    }

    #[test]
    fn proxy_owned_sequences_between_outputs_do_not_complete_the_source() {
        let spans = [pending_span(key(24, 0), 25, 29)];

        for partial in 25..29 {
            assert_eq!(map_client_ack_for_server(&[], &spans, partial), 23);
        }
        assert_eq!(map_client_ack_for_server(&[], &spans, 29), 24);
    }

    #[test]
    fn exact_source_generation_owns_registration_and_retirement() {
        let mut spans = Vec::new();
        let first = key(61, 4);
        let next_generation = key(61, 5);
        assert!(register_server_output_ack_span(&mut spans, first, 61, 62).expect("first span"));
        assert!(!register_server_output_ack_span(&mut spans, first, 61, 62).expect("exact replay"));
        assert!(
            register_server_output_ack_span(&mut spans, next_generation, 63, 64)
                .expect("wrapped source generation")
        );

        assert_eq!(retire_server_output_ack_spans(&mut spans, &[first]), 1);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].source, next_generation);
    }

    #[test]
    fn active_span_mapping_does_not_depend_on_trimmed_shift_prefix() {
        let mut shifts = (1..=16)
            .map(|base| SequenceShift { base, delta: 1 })
            .collect::<Vec<_>>();
        shifts.push(SequenceShift { base: 17, delta: 1 });
        super::super::sequence::trim_sequence_shifts(&mut shifts);
        assert_eq!(unshift_ack_for_origin(&shifts, 33), 17);
        let spans = [pending_span(key(16, 0), 32, 33)];

        assert_eq!(map_client_ack_for_server(&shifts, &spans, 32), 15);
        assert_eq!(map_client_ack_for_server(&shifts, &spans, 33), 16);
    }

    #[test]
    fn registration_rejects_overlaps_but_accepts_adjacent_and_wrapped_ranges() {
        let mut spans = Vec::new();
        assert!(
            register_server_output_ack_span(&mut spans, key(10, 0), 20, 21)
                .expect("register first range")
        );
        assert!(
            register_server_output_ack_span(&mut spans, key(12, 0), 22, 23)
                .expect("adjacent range")
        );
        assert!(
            register_server_output_ack_span(&mut spans, key(14, 0), u16::MAX, 0)
                .expect("wrapped range")
        );
        assert!(
            register_server_output_ack_span(&mut spans, key(16, 0), 1, 2)
                .expect("wrapped-adjacent range")
        );

        assert!(
            register_server_output_ack_span(&mut spans, key(11, 1), 21, 22).is_err(),
            "shared endpoints overlap two active owners"
        );
        assert!(
            register_server_output_ack_span(&mut spans, key(15, 1), u16::MAX - 1, 1).is_err(),
            "a wrapped range cannot partially contain an active wrapped owner"
        );
    }

    #[test]
    fn exact_expanded_ack_mapping_preserves_both_destination_and_source_wraps() {
        let mut spans = Vec::new();
        register_server_output_ack_span(&mut spans, key(0, 9), u16::MAX, 0)
            .expect("register wrapped expanded output");
        assert!(
            bind_server_output_ack_span_destination(
                &mut spans[0],
                epoch(u16::MAX, 3),
                epoch(0, 4),
            )
            .expect("bind exact wrapped destination")
        );
        assert!(
            !bind_server_output_ack_span_destination(
                &mut spans[0],
                epoch(u16::MAX, 3),
                epoch(0, 4),
            )
            .expect("exact binding replay")
        );

        let epochs = OrderedServerSequenceEpochs::identity();
        assert_eq!(
            map_exact_client_ack_for_server(&epochs, &spans, epoch(u16::MAX, 3))
                .expect("partial exact ACK"),
            epoch(u16::MAX, 8),
            "the first rebuilt output must stay before wrapped source zero"
        );
        assert_eq!(
            map_exact_client_ack_for_server(&epochs, &spans, epoch(0, 4))
                .expect("terminal exact ACK"),
            epoch(0, 9)
        );
        assert_eq!(
            map_exact_client_ack_for_server(&epochs, &spans, epoch(1, 4)).expect("later exact ACK"),
            epoch(0, 9),
            "an active completed owner conservatively caps later ACK progress"
        );
    }

    #[test]
    fn exact_span_binding_rejects_a_borrowed_destination_generation() {
        let mut spans = Vec::new();
        register_server_output_ack_span(&mut spans, key(u16::MAX, 2), u16::MAX, 0)
            .expect("register wrapped output");

        assert!(
            bind_server_output_ack_span_destination(
                &mut spans[0],
                epoch(u16::MAX, 7),
                epoch(0, 7),
            )
            .is_err(),
            "a wrapped raw endpoint must advance the exact generation"
        );
        assert!(matches!(
            spans[0].destination,
            ServerOutputAckDestinationSpan::Pending { .. }
        ));
    }
}
