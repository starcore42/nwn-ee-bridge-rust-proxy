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
    sequence::{SequenceShift, sequence_at_or_after, unshift_ack_for_origin},
    server_replay::ServerReliableSlotKey,
};

const MAX_SERVER_OUTPUT_ACK_SPANS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ServerOutputAckSpan {
    pub(super) source: ServerReliableSlotKey,
    /// First EE-facing reliable sequence produced from `source`.
    pub(super) destination_first: u16,
    /// Last EE-facing reliable sequence required to complete `source`.
    ///
    /// Intermediate sequence numbers may belong to proxy-owned insertions.
    pub(super) destination_last: u16,
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
        if existing.destination_first == destination_first
            && existing.destination_last == destination_last
        {
            return Ok(false);
        }
        anyhow::bail!("server reliable source already owns a different downstream ACK span");
    }
    if spans.iter().any(|span| {
        forward_closed_ranges_intersect(
            span.destination_first,
            span.destination_last,
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
        destination_first,
        destination_last,
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
        if sequence_in_forward_half_open(
            destination_ack,
            span.destination_first,
            span.destination_last,
        ) {
            return span.source.sequence.wrapping_sub(1);
        }
    }

    if let Some(latest_completed) = spans
        .iter()
        .filter(|span| sequence_at_or_after(destination_ack, span.destination_last))
        .min_by_key(|span| destination_ack.wrapping_sub(span.destination_last))
    {
        return latest_completed.source.sequence;
    }

    unshift_ack_for_origin(shifts, destination_ack)
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

    #[test]
    fn partial_expanded_ack_stays_before_source_until_terminal_output() {
        let shifts = [SequenceShift { base: 62, delta: 1 }];
        let spans = [ServerOutputAckSpan {
            source: key(61, 4),
            destination_first: 61,
            destination_last: 62,
        }];

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
        let spans = [ServerOutputAckSpan {
            source: key(u16::MAX, 9),
            destination_first: u16::MAX,
            destination_last: 0,
        }];

        assert_eq!(
            map_client_ack_for_server(&shifts, &spans, u16::MAX),
            u16::MAX - 1
        );
        assert_eq!(map_client_ack_for_server(&shifts, &spans, 0), u16::MAX);
    }

    #[test]
    fn proxy_owned_sequences_between_outputs_do_not_complete_the_source() {
        let spans = [ServerOutputAckSpan {
            source: key(24, 0),
            destination_first: 25,
            destination_last: 29,
        }];

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
        let spans = [ServerOutputAckSpan {
            source: key(16, 0),
            destination_first: 32,
            destination_last: 33,
        }];

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
}
