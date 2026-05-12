//! Reliable-window sequence/ack arithmetic.
//!
//! NWN's `M` reliable window uses wrapping 16-bit sequence numbers. The bridge
//! sometimes inserts synthetic packets, so peer-facing sequence numbers and
//! origin-facing ACKs must be shifted without confusing retransmit windows.
//! Keep this pure and packet-format-free: callers own packet mutation, this
//! module only answers sequence-ordering and delta questions.

#[derive(Debug, Clone)]
pub(super) struct SequenceShift {
    pub(super) base: u16,
    pub(super) delta: u16,
}

pub(super) fn sequence_at_or_after(sequence: u16, base: u16) -> bool {
    sequence.wrapping_sub(base) < 0x8000
}

pub(super) fn record_forward_progress(latest: &mut Option<u16>, observed: u16) {
    let should_update = latest
        .map(|current| sequence_at_or_after(observed, current))
        .unwrap_or(true);
    if should_update {
        *latest = Some(observed);
    }
}

fn sequence_before(sequence: u16, base: u16) -> bool {
    sequence != base && base.wrapping_sub(sequence) < 0x8000
}

fn sequence_in_forward_closed_range(sequence: u16, first: u16, last: u16) -> bool {
    sequence_at_or_after(sequence, first) && sequence_at_or_after(last, sequence)
}

pub(super) fn shift_sequence_for_peer(shifts: &[SequenceShift], original_sequence: u16) -> u16 {
    let mut delta = 0u16;
    for shift in shifts {
        if shift.delta != 0 && sequence_at_or_after(original_sequence, shift.base) {
            delta = delta.wrapping_add(shift.delta);
        }
    }
    original_sequence.wrapping_add(delta)
}

pub(super) fn unshift_ack_for_origin(shifts: &[SequenceShift], shifted_ack_sequence: u16) -> u16 {
    let mut cumulative_delta = 0u16;
    for shift in shifts {
        if shift.delta == 0 {
            continue;
        }

        let synthetic_first = shift.base.wrapping_add(cumulative_delta);
        if sequence_before(shifted_ack_sequence, synthetic_first) {
            return shifted_ack_sequence.wrapping_sub(cumulative_delta);
        }

        let synthetic_last = synthetic_first.wrapping_add(shift.delta).wrapping_sub(1);
        if sequence_in_forward_closed_range(shifted_ack_sequence, synthetic_first, synthetic_last) {
            return shift.base.wrapping_sub(1);
        }

        cumulative_delta = cumulative_delta.wrapping_add(shift.delta);
    }
    shifted_ack_sequence.wrapping_sub(cumulative_delta)
}

pub(super) fn trim_sequence_shifts(shifts: &mut Vec<SequenceShift>) {
    const MAX_SEQUENCE_SHIFTS: usize = 16;
    if shifts.len() > MAX_SEQUENCE_SHIFTS {
        let overflow = shifts.len() - MAX_SEQUENCE_SHIFTS;
        shifts.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_forward_progress_ignores_retransmitted_older_sequence() {
        let mut latest = Some(74);

        record_forward_progress(&mut latest, 73);

        assert_eq!(latest, Some(74));
    }

    #[test]
    fn record_forward_progress_accepts_equal_forward_and_wrapped_sequences() {
        let mut latest = Some(74);
        record_forward_progress(&mut latest, 74);
        assert_eq!(latest, Some(74));

        let mut wrapped = Some(u16::MAX);
        record_forward_progress(&mut wrapped, 1);
        assert_eq!(wrapped, Some(1));
    }
}
