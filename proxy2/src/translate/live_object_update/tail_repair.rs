//! Fragment-tail repair after typed creature appearance rewrites.
//!
//! This module owns one narrow transport consequence of the decompile-backed
//! `P/5` appearance translation: EE reads one or more fragment BOOLs that the
//! Diamond writer did not emit for visible-equipment active-property tails.
//! In short-declared live-object stream windows, the following submessage bits
//! can still belong to the original legacy tail.  Rather than let the `P/5`
//! translator or the M-frame layer guess, this helper keeps an original-tail
//! proof and only accepts a splice when the next focused semantic validator
//! consumes the following record exactly.

use super::creature;

const CREATURE_P_TAIL_REPAIR_CANDIDATE_WINDOW_BITS: usize = 16;

#[derive(Debug, Clone)]
pub(super) struct PendingCreatureAppearanceTailRepair {
    original_bits: Vec<bool>,
    original_tail_start: usize,
    rewritten_tail_start: usize,
    inserted_bits: usize,
    appearance_offset: usize,
    object_id: u32,
}

impl PendingCreatureAppearanceTailRepair {
    pub(super) fn new(
        original_bits: Vec<bool>,
        original_tail_start: usize,
        rewritten_tail_start: usize,
        inserted_bits: usize,
        appearance_offset: usize,
        object_id: u32,
    ) -> Option<Self> {
        if original_tail_start > original_bits.len() || rewritten_tail_start < original_tail_start {
            return None;
        }
        Some(Self {
            original_bits,
            original_tail_start,
            rewritten_tail_start,
            inserted_bits,
            appearance_offset,
            object_id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CreatureTailRepairResult {
    pub bit_cursor: usize,
    pub old_bits_len: usize,
    pub new_bits_len: usize,
}

pub(super) fn try_repair_for_creature_update(
    pending: &PendingCreatureAppearanceTailRepair,
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureTailRepairResult> {
    if pending.rewritten_tail_start > fragment_bits.len() || bit_cursor > fragment_bits.len() {
        return None;
    }
    if live_object_record_object_id(live_bytes, offset, record_end) != Some(pending.object_id) {
        trace_tail_repair_reject(pending, offset, record_end, bit_cursor);
        return None;
    }

    for candidate_tail_start in candidate_tail_starts(pending) {
        let mut candidate_bits = Vec::with_capacity(
            pending.rewritten_tail_start.saturating_add(
                pending
                    .original_bits
                    .len()
                    .saturating_sub(candidate_tail_start),
            ),
        );
        candidate_bits.extend_from_slice(&fragment_bits[..pending.rewritten_tail_start]);
        candidate_bits.extend_from_slice(&pending.original_bits[candidate_tail_start..]);

        let mut trial_cursor = bit_cursor;
        if creature::advance_verified_noop_creature_update_record(
            live_bytes,
            offset,
            record_end,
            &candidate_bits,
            &mut trial_cursor,
        ) {
            let old_bits_len = fragment_bits.len();
            let new_bits_len = candidate_bits.len();
            *fragment_bits = candidate_bits;
            trace_tail_repair_accept(
                pending,
                offset,
                record_end,
                candidate_tail_start,
                bit_cursor,
                trial_cursor,
                old_bits_len,
                new_bits_len,
            );
            return Some(CreatureTailRepairResult {
                bit_cursor: trial_cursor,
                old_bits_len,
                new_bits_len,
            });
        }
    }

    trace_tail_repair_reject(pending, offset, record_end, bit_cursor);
    None
}

fn candidate_tail_starts(pending: &PendingCreatureAppearanceTailRepair) -> Vec<usize> {
    let mut starts = Vec::new();
    push_unique(&mut starts, pending.original_tail_start);

    if pending.original_tail_start >= pending.inserted_bits {
        push_unique(
            &mut starts,
            pending.original_tail_start - pending.inserted_bits,
        );
    }
    push_unique(
        &mut starts,
        pending
            .original_tail_start
            .saturating_add(pending.inserted_bits)
            .min(pending.original_bits.len()),
    );

    for delta in 1..=CREATURE_P_TAIL_REPAIR_CANDIDATE_WINDOW_BITS {
        if pending.original_tail_start >= delta {
            push_unique(&mut starts, pending.original_tail_start - delta);
        }
        if delta
            <= pending
                .original_bits
                .len()
                .saturating_sub(pending.original_tail_start)
        {
            push_unique(&mut starts, pending.original_tail_start + delta);
        }
    }

    starts.retain(|start| *start <= pending.original_bits.len());
    starts
}

fn push_unique(values: &mut Vec<usize>, value: usize) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn live_object_record_object_id(
    live_bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> Option<u32> {
    if offset + 6 > record_end || record_end > live_bytes.len() {
        return None;
    }
    Some(u32::from_le_bytes(
        live_bytes.get(offset + 2..offset + 6)?.try_into().ok()?,
    ))
}

fn trace_tail_repair_accept(
    pending: &PendingCreatureAppearanceTailRepair,
    offset: usize,
    record_end: usize,
    candidate_tail_start: usize,
    bit_cursor: usize,
    repaired_cursor: usize,
    old_bits_len: usize,
    new_bits_len: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object creature-P tail repaired: p_offset={} object_id=0x{:08X} record_offset={offset} record_end={record_end} original_tail={} accepted_tail={} rewritten_tail={} inserted_bits={} bit_cursor={bit_cursor}->{repaired_cursor} bits_len={old_bits_len}->{new_bits_len}",
        pending.appearance_offset,
        pending.object_id,
        pending.original_tail_start,
        candidate_tail_start,
        pending.rewritten_tail_start,
        pending.inserted_bits,
    );
}

fn trace_tail_repair_reject(
    pending: &PendingCreatureAppearanceTailRepair,
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    if std::env::var_os("HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM").is_none() {
        return;
    }
    eprintln!(
        "live-object creature-P tail repair rejected: p_offset={} object_id=0x{:08X} record_offset={offset} record_end={record_end} original_tail={} rewritten_tail={} inserted_bits={} bit_cursor={bit_cursor}",
        pending.appearance_offset,
        pending.object_id,
        pending.original_tail_start,
        pending.rewritten_tail_start,
        pending.inserted_bits,
    );
}
